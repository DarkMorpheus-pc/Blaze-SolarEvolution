"""Clean QEMU/KVM VM lifecycle manager supporting Legacy BIOS and UEFI OVMF."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
from typing import Optional, TextIO

from ..config import OVMF_CODE_PATH, OVMF_VARS_PATH
from .imaging import convert_ppm_to_png
from .qga import QgaClient
from .qmp import QmpClient, QmpError


class QemuVM:
    """Context-managed QEMU VM using isolated temp sockets & OVMF vars."""

    def __init__(
        self,
        iso_path: Path,
        firmware: str = "bios",
        memory_mb: int = 4096,
        cpus: int = 4,
        artifacts_dir: Optional[Path] = None,
        vm_label: str = "blaze_e2e",
        disk_gb: int = 0,
    ) -> None:
        if firmware not in ("bios", "uefi"):
            raise ValueError(f"Unsupported firmware mode: {firmware!r}")
        self.iso_path = Path(iso_path)
        self.firmware = firmware
        self.memory_mb = memory_mb
        self.cpus = cpus
        self.vm_label = vm_label
        self.disk_gb = disk_gb

        self.run_dir = Path(tempfile.mkdtemp(prefix=f"blaze_e2e_{os.getpid()}_{firmware}_"))
        self.artifacts_dir = Path(artifacts_dir) if artifacts_dir else (self.run_dir / "artifacts")
        self.artifacts_dir.mkdir(parents=True, exist_ok=True)

        self.qmp_sock = self.run_dir / "qmp.sock"
        self.qga_sock = self.run_dir / "qga.sock"
        self.serial_log = self.artifacts_dir / f"serial_{firmware}.log"
        self.stderr_log = self.artifacts_dir / f"qemu_stderr_{firmware}.log"
        self.ovmf_vars_copy = self.run_dir / "OVMF_VARS.4m.fd"
        self.disk_path = self.run_dir / "disk.qcow2"

        self.proc: Optional[subprocess.Popen] = None
        self._stderr_fh: Optional[TextIO] = None
        self.qmp = QmpClient(str(self.qmp_sock))
        self.qga = QgaClient(str(self.qga_sock))

    def build_command(self) -> list[str]:
        if not self.iso_path.is_file():
            raise FileNotFoundError(f"ISO not found: {self.iso_path}")
        cmd = [
            "qemu-system-x86_64",
            "-name",
            f"{self.vm_label}-{self.firmware}-{os.getpid()}",
            "-cpu",
            "host",
            "-m",
            str(self.memory_mb),
            "-smp",
            str(self.cpus),
        ]
        if Path("/dev/kvm").exists() and os.access("/dev/kvm", os.R_OK | os.W_OK):
            cmd.append("-enable-kvm")

        if self.firmware == "uefi":
            if not OVMF_CODE_PATH.is_file() or not OVMF_VARS_PATH.is_file():
                raise FileNotFoundError(
                    f"OVMF firmware files not found: {OVMF_CODE_PATH}, {OVMF_VARS_PATH}"
                )
            shutil.copyfile(OVMF_VARS_PATH, self.ovmf_vars_copy)
            cmd.extend(
                [
                    "-drive",
                    f"if=pflash,format=raw,readonly=on,file={OVMF_CODE_PATH}",
                    "-drive",
                    f"if=pflash,format=raw,file={self.ovmf_vars_copy}",
                ]
            )

        if self.disk_gb > 0:
            subprocess.run(
                ["qemu-img", "create", "-f", "qcow2", str(self.disk_path), f"{self.disk_gb}G"],
                check=True,
                capture_output=True,
                text=True,
                timeout=30,
            )
            cmd.extend(
                [
                    "-drive",
                    f"file={self.disk_path},if=virtio,format=qcow2",
                ]
            )

        cmd.extend(
            [
                "-cdrom",
                str(self.iso_path),
                "-boot",
                "d",
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
                "user,id=net0",
                "-device",
                "virtio-net-pci,netdev=net0",
                "-serial",
                f"file:{self.serial_log}",
            ]
        )
        return cmd

    def start(self) -> None:
        try:
            cmd = self.build_command()
            self._stderr_fh = open(self.stderr_log, "w", encoding="utf-8", errors="replace")
            self.proc = subprocess.Popen(
                cmd,
                stdout=subprocess.DEVNULL,
                stderr=self._stderr_fh,
                text=True,
            )
            self.qmp.connect(wait_timeout=20.0, proc=self.proc)
        except Exception:
            self.cleanup()
            raise

    def capture_png(self, name: str) -> Path:
        ppm = self.run_dir / f"{name}.ppm"
        png = self.artifacts_dir / f"{name}.png"
        ppm.unlink(missing_ok=True)
        self.qmp.screendump(str(ppm))
        deadline = time.monotonic() + 5.0
        while time.monotonic() < deadline and not ppm.is_file():
            time.sleep(0.1)
        return convert_ppm_to_png(ppm, png)

    def stop(self) -> None:
        try:
            self.qga.close()
        finally:
            try:
                if self.qmp.sock is not None:
                    try:
                        self.qmp.quit()
                    except (QmpError, OSError, ValueError):
                        pass
                    self.qmp.close()
            finally:
                try:
                    if self.proc is not None:
                        try:
                            self.proc.wait(timeout=8.0)
                        except subprocess.TimeoutExpired:
                            self.proc.terminate()
                            try:
                                self.proc.wait(timeout=5.0)
                            except subprocess.TimeoutExpired:
                                self.proc.kill()
                                self.proc.wait(timeout=3.0)
                        self.proc = None
                finally:
                    if self._stderr_fh is not None:
                        try:
                            self._stderr_fh.close()
                        except OSError:
                            pass
                        self._stderr_fh = None

    def cleanup(self) -> None:
        self.stop()
        shutil.rmtree(self.run_dir, ignore_errors=True)

    def __enter__(self) -> "QemuVM":
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        self.cleanup()
