#!/usr/bin/env python3
"""Isolated QEMU/KVM OVMF VM controller for Blaze SolarEvolution Technical Evaluation.

Safety invariants:
- Operates exclusively on isolated QCOW2 virtual disks inside a dedicated temporary directory.
- Never mounts or writes to host partitions or physical block devices.
- Supports copy-on-write QCOW2 overlays so the pristine installed base disk is never mutated.
- Supports attaching an isolated disposable secondary virtual disk (/dev/vdb) for fio/stress I/O.
"""
from __future__ import annotations

import base64
import json
import os
import shutil
import socket
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Optional

from PIL import Image

OVMF_CODE_PATH = Path("/usr/share/edk2/x64/OVMF_CODE.4m.fd")
OVMF_VARS_PATH = Path("/usr/share/edk2/x64/OVMF_VARS.4m.fd")

DEFAULT_EVAL_RAM_MB = 8192
DEFAULT_EVAL_CPUS = 8
EVAL_USER = "evaluser"
EVAL_USER_PW = "EvalUser2026!"
EVAL_ROOT_PW = "EvalRoot2026!"


class QmpError(RuntimeError):
    pass


class QgaError(RuntimeError):
    pass


@dataclass
class GuestExecResult:
    exitcode: int
    stdout: str
    stderr: str
    signal: Optional[int] = None


class QmpClient:
    def __init__(self, sock_path: str, timeout: float = 15.0) -> None:
        self.sock_path = sock_path
        self.timeout = timeout
        self.sock: Optional[socket.socket] = None
        self._buf = b""
        self.events: list[dict[str, Any]] = []

    def connect(self, wait_timeout: float = 25.0, proc: Optional[subprocess.Popen] = None) -> dict[str, Any]:
        deadline = time.monotonic() + wait_timeout
        while time.monotonic() < deadline:
            if proc is not None and proc.poll() is not None:
                raise QmpError(f"QEMU exited early with rc={proc.returncode}")
            if os.path.exists(self.sock_path):
                try:
                    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    sock.settimeout(self.timeout)
                    sock.connect(self.sock_path)
                    self.sock = sock
                    self._buf = b""
                    break
                except OSError:
                    try:
                        sock.close()
                    except OSError:
                        pass
            time.sleep(0.2)
        if self.sock is None:
            raise QmpError(f"Timed out waiting for QMP socket: {self.sock_path}")
        greeting = self._read_json_line()
        self.execute("qmp_capabilities")
        return greeting

    def close(self) -> None:
        if self.sock is not None:
            try:
                self.sock.close()
            except OSError:
                pass
            self.sock = None
        self._buf = b""

    def _read_json_line(self) -> dict[str, Any]:
        assert self.sock is not None
        while b"\n" not in self._buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise QmpError("QMP socket closed unexpectedly")
            self._buf += chunk
        line, self._buf = self._buf.split(b"\n", 1)
        return json.loads(line.decode("utf-8", errors="replace"))

    def execute(self, command: str, arguments: Optional[dict[str, Any]] = None) -> Any:
        if self.sock is None:
            raise QmpError("QMP not connected")
        payload: dict[str, Any] = {"execute": command}
        if arguments is not None:
            payload["arguments"] = arguments
        self.sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))
        while True:
            msg = self._read_json_line()
            if "event" in msg:
                self.events.append(msg)
                continue
            if "error" in msg:
                raise QmpError(f"QMP {command} error: {msg['error']}")
            if "return" in msg:
                return msg["return"]

    def screendump(self, ppm_path: str) -> None:
        self.execute("screendump", {"filename": str(ppm_path)})

    def send_key(self, qcode: str, hold_secs: float = 0.04) -> None:
        if qcode == "return":
            qcode = "ret"
        self.execute(
            "input-send-event",
            {"events": [{"type": "key", "data": {"key": {"type": "qcode", "data": qcode}, "down": True}}]},
        )
        time.sleep(hold_secs)
        self.execute(
            "input-send-event",
            {"events": [{"type": "key", "data": {"key": {"type": "qcode", "data": qcode}, "down": False}}]},
        )

    def send_combo(self, qcodes: list[str], hold_secs: float = 0.05) -> None:
        mapped = ["ret" if q == "return" else q for q in qcodes]
        for q in mapped:
            self.execute(
                "input-send-event",
                {"events": [{"type": "key", "data": {"key": {"type": "qcode", "data": q}, "down": True}}]},
            )
            time.sleep(0.01)
        time.sleep(hold_secs)
        for q in reversed(mapped):
            self.execute(
                "input-send-event",
                {"events": [{"type": "key", "data": {"key": {"type": "qcode", "data": q}, "down": False}}]},
            )
            time.sleep(0.01)

    def type_ascii(self, text: str, delay_secs: float = 0.025) -> None:
        keymap: dict[str, str | list[str]] = {
            " ": "spc",
            "\n": "ret",
            "-": "minus",
            "_": ["shift", "minus"],
            "/": "slash",
            ".": "dot",
            ",": "comma",
            ":": ["shift", "semicolon"],
            ";": "semicolon",
            "=": "equal",
            "&": ["shift", "7"],
            "|": ["shift", "backslash"],
            ">": ["shift", "dot"],
            "<": ["shift", "comma"],
        }
        for ch in text:
            if "a" <= ch <= "z" or "0" <= ch <= "9":
                self.send_key(ch, hold_secs=0.02)
            elif "A" <= ch <= "Z":
                self.send_combo(["shift", ch.lower()], hold_secs=0.02)
            elif ch in keymap:
                val = keymap[ch]
                if isinstance(val, list):
                    self.send_combo(val, hold_secs=0.02)
                else:
                    self.send_key(val, hold_secs=0.02)
            else:
                raise ValueError(f"Unsupported ASCII character for QMP typing: {ch!r}")
            time.sleep(delay_secs)

    def quit(self) -> None:
        self.execute("quit")


class QgaClient:
    def __init__(self, sock_path: str, timeout: float = 20.0) -> None:
        self.sock_path = sock_path
        self.timeout = timeout
        self.sock: Optional[socket.socket] = None
        self._buf = b""
        self._sync_counter = 0

    def connect(self, wait_timeout: float = 20.0) -> None:
        deadline = time.monotonic() + wait_timeout
        while time.monotonic() < deadline:
            if os.path.exists(self.sock_path):
                try:
                    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                    sock.settimeout(self.timeout)
                    sock.connect(self.sock_path)
                    self.sock = sock
                    self._buf = b""
                    return
                except OSError:
                    try:
                        sock.close()
                    except OSError:
                        pass
            time.sleep(0.25)
        raise QgaError(f"Timed out waiting for QGA socket: {self.sock_path}")

    def close(self) -> None:
        if self.sock is not None:
            try:
                self.sock.close()
            except OSError:
                pass
            self.sock = None
        self._buf = b""

    def _read_json_line(self) -> Any:
        assert self.sock is not None
        while b"\n" not in self._buf:
            chunk = self.sock.recv(8192)
            if not chunk:
                raise QgaError("QGA socket closed unexpectedly")
            self._buf += chunk
        line, self._buf = self._buf.split(b"\n", 1)
        return json.loads(line.decode("utf-8", errors="replace"))

    def sync(self, timeout: float = 10.0) -> int:
        assert self.sock is not None
        self._sync_counter += 1
        sync_id = ((int(time.monotonic_ns()) & 0x3FFFFFFF) ^ self._sync_counter) or 1
        self.sock.sendall((json.dumps({"execute": "guest-sync", "arguments": {"id": sync_id}}) + "\n").encode("utf-8"))
        prev = self.sock.gettimeout()
        deadline = time.monotonic() + timeout
        try:
            while True:
                rem = deadline - time.monotonic()
                if rem <= 0:
                    raise QgaError("QGA guest-sync timed out")
                self.sock.settimeout(rem)
                try:
                    msg = self._read_json_line()
                except (json.JSONDecodeError, UnicodeDecodeError):
                    continue
                if isinstance(msg, dict) and msg.get("return") == sync_id:
                    return sync_id
        finally:
            if self.sock is not None:
                try:
                    self.sock.settimeout(prev)
                except OSError:
                    pass

    def execute(self, command: str, arguments: Optional[dict[str, Any]] = None) -> Any:
        if self.sock is None:
            raise QgaError("QGA not connected")
        payload: dict[str, Any] = {"execute": command}
        if arguments is not None:
            payload["arguments"] = arguments
        self.sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))
        msg = self._read_json_line()
        if not isinstance(msg, dict):
            raise QgaError(f"QGA {command} returned non-dict: {msg!r}")
        if "error" in msg:
            raise QgaError(f"QGA {command} failed: {msg['error']}")
        return msg.get("return")

    def ping(self, timeout: float = 2.5) -> bool:
        if self.sock is None:
            return False
        prev = self.sock.gettimeout()
        try:
            self.sock.settimeout(timeout)
            self.execute("guest-ping")
            return True
        except Exception:
            return False
        finally:
            if self.sock is not None:
                try:
                    self.sock.settimeout(prev)
                except OSError:
                    pass

    def wait_until_ready(self, timeout: float = 240.0, poll_interval: float = 2.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.sock is None:
                try:
                    self.connect(wait_timeout=min(5.0, max(0.5, deadline - time.monotonic())))
                except QgaError:
                    time.sleep(poll_interval)
                    continue
            if self.ping(timeout=2.5):
                try:
                    self.sync(timeout=5.0)
                    return
                except Exception:
                    self.close()
            else:
                self.close()
            time.sleep(poll_interval)
        raise QgaError(f"QGA did not become ready within {timeout:.0f}s")

    def guest_exec(
        self,
        path: str,
        arg: Optional[list[str]] = None,
        timeout: float = 60.0,
        poll_interval: float = 0.4,
        capture_output: bool = True,
    ) -> GuestExecResult:
        args: dict[str, Any] = {"path": path, "capture-output": capture_output}
        if arg:
            args["arg"] = arg
        ret = self.execute("guest-exec", args)
        pid = ret["pid"]
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            status = self.execute("guest-exec-status", {"pid": pid})
            if status.get("exited"):
                out_b = base64.b64decode(status.get("out-data", "")) if status.get("out-data") else b""
                err_b = base64.b64decode(status.get("err-data", "")) if status.get("err-data") else b""
                return GuestExecResult(
                    exitcode=int(status.get("exitcode", -1)),
                    stdout=out_b.decode("utf-8", errors="replace"),
                    stderr=err_b.decode("utf-8", errors="replace"),
                    signal=status.get("signal"),
                )
            time.sleep(poll_interval)
        raise QgaError(f"guest-exec {path} timed out after {timeout:.0f}s")

    def run_shell(self, script: str, timeout: float = 120.0) -> GuestExecResult:
        return self.guest_exec("/bin/bash", ["-lc", script], timeout=timeout)

    def run_long_shell(
        self,
        script: str,
        job_name: str,
        timeout: float = 1800.0,
        poll_interval: float = 5.0,
    ) -> tuple[int, str]:
        """Run a long script detached via systemd-run/setsid inside guest, polling a marker file."""
        work = f"/tmp/eval_job_{job_name}"
        sh_path = f"{work}.sh"
        log_path = f"{work}.log"
        rc_path = f"{work}.rc"
        self.push_bytes(
            (
                "#!/bin/bash\nset -o pipefail\n"
                + script
                + f"\n__rc=$?\necho $__rc > {rc_path}\nsync\nexit $__rc\n"
            ).encode("utf-8"),
            sh_path,
        )
        self.guest_exec(
            "/bin/bash",
            [
                "-c",
                f"chmod +x {sh_path} && rm -f {rc_path} {log_path} && "
                f"systemd-run --no-block --unit=eval-{job_name}-{int(time.time())} "
                f"/bin/bash -c 'exec {sh_path} >{log_path} 2>&1 </dev/null' || "
                f"setsid -f /bin/bash -c 'exec {sh_path} >{log_path} 2>&1 </dev/null'",
            ],
            timeout=30.0,
            capture_output=False,
        )
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            chk = self.run_shell(f"if [ -f {rc_path} ]; then cat {rc_path}; else echo RUNNING; fi", timeout=20.0)
            val = chk.stdout.strip()
            if val and val != "RUNNING":
                rc = int(val.splitlines()[-1].strip())
                tail = self.run_shell(f"tail -n 60 {log_path} 2>/dev/null || true", timeout=20.0).stdout
                return rc, tail
            time.sleep(poll_interval)
        tail = self.run_shell(f"tail -n 60 {log_path} 2>/dev/null || true", timeout=20.0).stdout
        raise QgaError(f"Long job {job_name} timed out after {timeout:.0f}s. Tail:\n{tail}")

    def pull_file(self, guest_path: str, host_path: Path, chunk_size: int = 49152) -> int:
        host_path.parent.mkdir(parents=True, exist_ok=True)
        handle = self.execute("guest-file-open", {"path": guest_path, "mode": "r"})
        total = 0
        try:
            with open(host_path, "wb") as fh:
                while True:
                    res = self.execute("guest-file-read", {"handle": handle, "count": chunk_size})
                    b64 = res.get("buf-b64", "")
                    if b64:
                        raw = base64.b64decode(b64)
                        fh.write(raw)
                        total += len(raw)
                    if res.get("eof") or not b64:
                        break
        finally:
            try:
                self.execute("guest-file-close", {"handle": handle})
            except Exception:
                pass
        return total

    def push_bytes(self, data: bytes, guest_path: str, chunk_size: int = 32768) -> None:
        handle = self.execute("guest-file-open", {"path": guest_path, "mode": "w"})
        try:
            for offset in range(0, len(data), chunk_size):
                chunk = data[offset : offset + chunk_size]
                self.execute(
                    "guest-file-write",
                    {"handle": handle, "buf-b64": base64.b64encode(chunk).decode("ascii")},
                )
        finally:
            try:
                self.execute("guest-file-close", {"handle": handle})
            except Exception:
                pass


EVAL_SSH_KEY_PATH = Path("/var/tmp/blaze_eval_vms/eval_id_ed25519")


def find_free_tcp_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def target_vm_kwargs(target_name: str) -> dict[str, Any]:
    """Return transport kwargs (SSH for Fedora reference, QGA for BlazeOS) for EvalQemuVM."""
    if target_name == "fedora45":
        return {"ssh_port": find_free_tcp_port(), "ssh_key_path": EVAL_SSH_KEY_PATH}
    return {}


class SshClient:
    """QgaClient-compatible SSH/SCP transport for unmodified stock guests (SELinux Enforcing untouched)."""

    def __init__(self, port: int, key_path: Path, user: str = "root", timeout: float = 20.0) -> None:
        self.port = port
        self.key_path = Path(key_path)
        self.user = user
        self.timeout = timeout

    def _ssh_base(self, connect_timeout: int = 8) -> list[str]:
        return [
            "ssh",
            "-i",
            str(self.key_path),
            "-p",
            str(self.port),
            "-o",
            "StrictHostKeyChecking=no",
            "-o",
            "UserKnownHostsFile=/dev/null",
            "-o",
            "BatchMode=yes",
            "-o",
            "LogLevel=ERROR",
            "-o",
            f"ConnectTimeout={connect_timeout}",
            f"{self.user}@127.0.0.1",
        ]

    def close(self) -> None:
        pass

    def ping(self, timeout: float = 4.0) -> bool:
        try:
            p = subprocess.run(
                self._ssh_base(connect_timeout=max(2, int(timeout))) + ["true"],
                stdin=subprocess.DEVNULL,
                capture_output=True,
                text=True,
                timeout=max(4.0, timeout + 2.0),
            )
            return p.returncode == 0
        except Exception:
            return False

    def wait_until_ready(self, timeout: float = 240.0, poll_interval: float = 2.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.ping(timeout=4.0):
                return
            time.sleep(poll_interval)
        raise QgaError(f"SSH transport on 127.0.0.1:{self.port} did not become ready within {timeout:.0f}s")

    def guest_exec(
        self,
        path: str,
        arg: Optional[list[str]] = None,
        timeout: float = 60.0,
        poll_interval: float = 0.4,
        capture_output: bool = True,
    ) -> GuestExecResult:
        import shlex

        cmdline = " ".join([shlex.quote(path)] + [shlex.quote(a) for a in (arg or [])])
        p = subprocess.run(
            self._ssh_base() + [cmdline],
            stdin=subprocess.DEVNULL,
            capture_output=capture_output,
            text=True,
            timeout=timeout,
        )
        return GuestExecResult(
            exitcode=p.returncode,
            stdout=p.stdout if capture_output and p.stdout else "",
            stderr=p.stderr if capture_output and p.stderr else "",
        )

    def run_shell(self, script: str, timeout: float = 120.0) -> GuestExecResult:
        import shlex

        p = subprocess.run(
            self._ssh_base() + [f"/bin/bash -lc {shlex.quote(script)}"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        return GuestExecResult(exitcode=p.returncode, stdout=p.stdout, stderr=p.stderr)

    def push_bytes(self, data: bytes, guest_path: str, chunk_size: int = 32768) -> None:
        import shlex

        p = subprocess.run(
            self._ssh_base() + [f"cat > {shlex.quote(guest_path)}"],
            input=data,
            capture_output=True,
            timeout=120.0,
        )
        if p.returncode != 0:
            raise QgaError(f"SSH push_bytes failed: {p.stderr.decode('utf-8', errors='replace')}")

    def pull_file(self, guest_path: str, host_path: Path, chunk_size: int = 49152) -> int:
        import shlex

        host_path.parent.mkdir(parents=True, exist_ok=True)
        p = subprocess.run(
            self._ssh_base() + [f"cat {shlex.quote(guest_path)}"],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=180.0,
        )
        if p.returncode != 0:
            raise QgaError(f"SSH pull_file failed: {p.stderr.decode('utf-8', errors='replace')}")
        host_path.write_bytes(p.stdout)
        return len(p.stdout)

    def run_long_shell(
        self,
        script: str,
        job_name: str,
        timeout: float = 1800.0,
        poll_interval: float = 5.0,
    ) -> tuple[int, str]:
        work = f"/tmp/eval_job_{job_name}"
        sh_path = f"{work}.sh"
        log_path = f"{work}.log"
        rc_path = f"{work}.rc"
        self.push_bytes(
            (
                "#!/bin/bash\nset -o pipefail\n"
                + script
                + f"\n__rc=$?\necho $__rc > {rc_path}\nsync\nexit $__rc\n"
            ).encode("utf-8"),
            sh_path,
        )
        self.run_shell(
            f"chmod +x {sh_path} && rm -f {rc_path} {log_path} && "
            f"(systemd-run --no-block --unit=eval-{job_name}-{int(time.time())} "
            f"/bin/bash -c 'exec {sh_path} >{log_path} 2>&1 </dev/null' || "
            f"setsid -f /bin/bash -c 'exec {sh_path} >{log_path} 2>&1 </dev/null' </dev/null >/dev/null 2>&1)",
            timeout=30.0,
        )
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            chk = self.run_shell(f"if [ -f {rc_path} ]; then cat {rc_path}; else echo RUNNING; fi", timeout=20.0)
            val = chk.stdout.strip()
            if val and val != "RUNNING":
                rc = int(val.splitlines()[-1].strip())
                tail = self.run_shell(f"tail -n 60 {log_path} 2>/dev/null || true", timeout=20.0).stdout
                return rc, tail
            time.sleep(poll_interval)
        tail = self.run_shell(f"tail -n 60 {log_path} 2>/dev/null || true", timeout=20.0).stdout
        raise QgaError(f"Long job {job_name} timed out after {timeout:.0f}s. Tail:\n{tail}")


def convert_ppm_to_png(ppm_path: Path, png_path: Path) -> Path:
    png_path.parent.mkdir(parents=True, exist_ok=True)
    with Image.open(ppm_path) as im:
        im.save(png_path, format="PNG")
    ppm_path.unlink(missing_ok=True)
    return png_path


def create_overlay_pair(
    base_disk: Path,
    base_vars: Path,
    out_dir: Path,
    label: str,
) -> tuple[Path, Path]:
    out_dir.mkdir(parents=True, exist_ok=True)
    ovl = out_dir / f"{label}_disk.qcow2"
    vrs = out_dir / f"{label}_OVMF_VARS.fd"
    ovl.unlink(missing_ok=True)
    subprocess.run(
        [
            "qemu-img",
            "create",
            "-f",
            "qcow2",
            "-b",
            str(base_disk.resolve()),
            "-F",
            "qcow2",
            str(ovl),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    shutil.copyfile(base_vars, vrs)
    return ovl, vrs


def create_scratch_disk(path: Path, size_gb: int = 6) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.unlink(missing_ok=True)
    subprocess.run(
        ["qemu-img", "create", "-f", "qcow2", str(path), f"{size_gb}G"],
        check=True,
        capture_output=True,
        text=True,
    )
    return path


class EvalQemuVM:
    def __init__(
        self,
        disk_path: Path,
        ovmf_vars_path: Path,
        log_dir: Path,
        vm_label: str,
        iso_path: Optional[Path] = None,
        scratch_disk: Optional[Path] = None,
        extra_drives: Optional[list[str]] = None,
        ssh_port: Optional[int] = None,
        ssh_key_path: Optional[Path] = None,
        boot_order: str = "c",
        memory_mb: int = DEFAULT_EVAL_RAM_MB,
        cpus: int = DEFAULT_EVAL_CPUS,
        cpu_model: str = "host",
    ) -> None:
        self.disk_path = Path(disk_path)
        self.ovmf_vars_path = Path(ovmf_vars_path)
        self.iso_path = Path(iso_path) if iso_path else None
        self.scratch_disk = Path(scratch_disk) if scratch_disk else None
        self.extra_drives = list(extra_drives or [])
        self.ssh_port = ssh_port
        self.ssh_key_path = Path(ssh_key_path) if ssh_key_path else None
        self.boot_order = boot_order
        self.memory_mb = memory_mb
        self.cpus = cpus
        self.cpu_model = cpu_model
        self.vm_label = vm_label
        self.log_dir = Path(log_dir)
        self.log_dir.mkdir(parents=True, exist_ok=True)

        self.run_dir = Path(tempfile.mkdtemp(prefix=f"blaze_eval_{vm_label}_{os.getpid()}_"))
        self.qmp_sock = self.run_dir / "qmp.sock"
        self.qga_sock = self.run_dir / "qga.sock"
        self.serial_log = self.log_dir / f"{vm_label}_serial.log"
        self.stderr_log = self.log_dir / f"{vm_label}_qemu_stderr.log"

        self.proc: Optional[subprocess.Popen] = None
        self._stderr_fh: Optional[Any] = None
        self.qmp = QmpClient(str(self.qmp_sock))
        self.qga: Any = (
            SshClient(port=self.ssh_port, key_path=self.ssh_key_path)
            if (self.ssh_port is not None and self.ssh_key_path is not None)
            else QgaClient(str(self.qga_sock))
        )

    def build_command(self) -> list[str]:
        cmd = [
            "qemu-system-x86_64",
            "-name",
            f"eval-{self.vm_label}-{os.getpid()}",
            "-cpu",
            self.cpu_model,
            "-m",
            str(self.memory_mb),
            "-smp",
            str(self.cpus),
        ]
        if Path("/dev/kvm").exists() and os.access("/dev/kvm", os.R_OK | os.W_OK):
            cmd.append("-enable-kvm")

        cmd.extend(
            [
                "-drive",
                f"if=pflash,format=raw,readonly=on,file={OVMF_CODE_PATH}",
                "-drive",
                f"if=pflash,format=raw,file={self.ovmf_vars_path}",
                "-drive",
                f"file={self.disk_path},if=virtio,format=qcow2",
            ]
        )
        if self.scratch_disk is not None:
            cmd.extend(["-drive", f"file={self.scratch_disk},if=virtio,format=qcow2"])
        for drv in self.extra_drives:
            cmd.extend(["-drive", drv])
        if self.iso_path is not None:
            cmd.extend(["-cdrom", str(self.iso_path), "-boot", f"order={self.boot_order}"])
        else:
            cmd.extend(["-boot", f"order={self.boot_order}"])

        net_arg = (
            f"user,id=net0,hostfwd=tcp:127.0.0.1:{self.ssh_port}-:22"
            if self.ssh_port is not None
            else "user,id=net0"
        )
        cmd.extend(
            [
                "-vga",
                "virtio",
                "-device",
                "virtio-tablet-pci",
                "-display",
                "none",
                "-qmp",
                f"unix:{self.qmp_sock},server=on,wait=off",
                "-chardev",
                f"socket,path={self.qga_sock},server=on,wait=off,id=qga0",
                "-device",
                "virtio-serial-pci",
                "-device",
                "virtserialport,chardev=qga0,name=org.qemu.guest_agent.0",
                "-netdev",
                net_arg,
                "-device",
                "virtio-net-pci,netdev=net0",
                "-serial",
                f"file:{self.serial_log}",
            ]
        )
        return cmd

    def start(self) -> None:
        cmd = self.build_command()
        self._stderr_fh = open(self.stderr_log, "w", encoding="utf-8", errors="replace")
        self.proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=self._stderr_fh, text=True)
        self.qmp.connect(wait_timeout=25.0, proc=self.proc)

    def capture_png(self, out_png: Path) -> Path:
        ppm = self.run_dir / f"{out_png.stem}.ppm"
        ppm.unlink(missing_ok=True)
        self.qmp.screendump(str(ppm))
        deadline = time.monotonic() + 8.0
        while time.monotonic() < deadline and not ppm.is_file():
            time.sleep(0.1)
        return convert_ppm_to_png(ppm, out_png)

    def get_boot_id(self) -> str:
        return self.qga.run_shell("cat /proc/sys/kernel/random/boot_id", timeout=10.0).stdout.strip()

    def reboot_and_wait(self, timeout: float = 180.0) -> float:
        old_id = self.get_boot_id()
        t0 = time.monotonic()
        try:
            self.qga.guest_exec(
                "/bin/bash",
                ["-c", "nohup bash -c 'sleep 1; systemctl reboot' >/dev/null 2>&1 </dev/null &"],
                timeout=10.0,
                capture_output=False,
            )
        except Exception:
            if not isinstance(self.qga, SshClient):
                raise
        time.sleep(4.0)
        self.qga.close()
        deadline = t0 + timeout
        while time.monotonic() < deadline:
            try:
                self.qga.wait_until_ready(timeout=min(15.0, max(2.0, deadline - time.monotonic())), poll_interval=1.5)
                new_id = self.get_boot_id()
                if new_id and new_id != old_id:
                    return time.monotonic() - t0
                self.qga.close()
            except Exception:
                self.qga.close()
            time.sleep(1.5)
        raise TimeoutError(f"Guest did not reboot within {timeout:.0f}s")

    def shutdown_graceful(self, timeout: float = 60.0) -> None:
        try:
            self.qga.guest_exec("/bin/bash", ["-c", "nohup bash -c 'sleep 0.5; systemctl poweroff' >/dev/null 2>&1 </dev/null &"], timeout=10.0, capture_output=False)
        except Exception:
            pass
        if self.proc is not None:
            try:
                self.proc.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                pass

    def stop(self) -> None:
        try:
            self.qga.close()
        finally:
            try:
                if self.qmp.sock is not None:
                    try:
                        self.qmp.quit()
                    except Exception:
                        pass
                    self.qmp.close()
            finally:
                if self.proc is not None:
                    try:
                        self.proc.wait(timeout=10.0)
                    except subprocess.TimeoutExpired:
                        self.proc.terminate()
                        try:
                            self.proc.wait(timeout=5.0)
                        except subprocess.TimeoutExpired:
                            self.proc.kill()
                            self.proc.wait(timeout=3.0)
                    self.proc = None
                if self._stderr_fh is not None:
                    self._stderr_fh.close()
                    self._stderr_fh = None

    def cleanup(self) -> None:
        self.stop()
        shutil.rmtree(self.run_dir, ignore_errors=True)

    def __enter__(self) -> "EvalQemuVM":
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        self.cleanup()
