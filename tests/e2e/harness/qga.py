"""QEMU Guest Agent (QGA) client over AF_UNIX using Python stdlib only."""
import base64
import binascii
import json
import os
import socket
import time
from dataclasses import dataclass
from typing import Any, Dict, List, Optional


class QgaError(RuntimeError):
    """Raised when QGA communication or guest-exec fails."""


@dataclass
class GuestExecResult:
    exitcode: int
    stdout: str
    stderr: str
    signal: Optional[int] = None


class QgaClient:
    """Line-delimited JSON QEMU Guest Agent client."""

    def __init__(self, sock_path: str, timeout: float = 15.0) -> None:
        self.sock_path = sock_path
        self.timeout = timeout
        self.sock: Optional[socket.socket] = None
        self._buf = b""
        self._synced = False
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
                    self._synced = False
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
        self._synced = False

    def _read_raw_line(self) -> bytes:
        assert self.sock is not None
        while b"\n" not in self._buf:
            chunk = self.sock.recv(4096)
            if not chunk:
                raise QgaError("QGA socket closed unexpectedly")
            self._buf += chunk
        line, self._buf = self._buf.split(b"\n", 1)
        return line

    def _read_json_line(self) -> Any:
        line = self._read_raw_line()
        return json.loads(line.decode("utf-8"))

    def sync(self, timeout: Optional[float] = None) -> int:
        if self.sock is None:
            raise QgaError("QGA client is not connected")
        self._sync_counter += 1
        sync_id = ((int(time.monotonic_ns()) & 0x3FFFFFFF) ^ self._sync_counter) or 1
        payload = {"execute": "guest-sync", "arguments": {"id": sync_id}}
        try:
            self.sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))
        except (OSError, socket.timeout) as exc:
            raise QgaError(f"QGA command 'guest-sync' send failed: {exc}") from exc
        prev_timeout = self.sock.gettimeout()
        sync_timeout = (
            timeout
            if timeout is not None
            else (prev_timeout if prev_timeout is not None else self.timeout)
        )
        deadline = time.monotonic() + sync_timeout
        try:
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise QgaError(f"QGA command 'guest-sync' timed out after {sync_timeout}s")
                self.sock.settimeout(remaining)
                try:
                    msg = self._read_json_line()
                except (json.JSONDecodeError, UnicodeDecodeError):
                    continue
                except socket.timeout as exc:
                    raise QgaError(
                        f"QGA command 'guest-sync' timed out after {sync_timeout}s"
                    ) from exc
                except OSError as exc:
                    raise QgaError(f"QGA command 'guest-sync' I/O failed: {exc}") from exc
                if not isinstance(msg, dict):
                    continue
                if "error" in msg:
                    if msg.get("id") == sync_id:
                        raise QgaError(f"QGA command 'guest-sync' failed: {msg['error']}")
                    continue
                if msg.get("return") == sync_id:
                    self._synced = True
                    return sync_id
        finally:
            if self.sock is not None:
                try:
                    self.sock.settimeout(prev_timeout)
                except OSError:
                    pass

    def execute(self, command: str, arguments: Optional[Dict[str, Any]] = None) -> Any:
        if self.sock is None:
            raise QgaError("QGA client is not connected")
        payload: Dict[str, Any] = {"execute": command}
        if arguments is not None:
            payload["arguments"] = arguments
        try:
            self.sock.sendall((json.dumps(payload) + "\n").encode("utf-8"))
            msg = self._read_json_line()
        except (json.JSONDecodeError, UnicodeDecodeError, socket.timeout, OSError) as exc:
            raise QgaError(f"QGA command {command!r} protocol error: {exc}") from exc
        if not isinstance(msg, dict):
            raise QgaError(f"QGA command {command!r} returned non-object JSON: {msg!r}")
        if "error" in msg:
            raise QgaError(f"QGA command {command!r} failed: {msg['error']}")
        return msg.get("return")

    def ping(self, timeout: Optional[float] = None) -> bool:
        if self.sock is None:
            return False
        prev_timeout = self.sock.gettimeout()
        try:
            if timeout is not None:
                self.sock.settimeout(timeout)
            self.execute("guest-ping")
            return True
        except (QgaError, OSError, ValueError):
            return False
        finally:
            if self.sock is not None:
                try:
                    self.sock.settimeout(prev_timeout)
                except OSError:
                    pass

    def wait_until_ready(
        self,
        timeout: float = 150.0,
        poll_interval: float = 2.0,
        ping_timeout: float = 2.5,
    ) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.sock is None:
                try:
                    self.connect(wait_timeout=min(5.0, max(0.5, deadline - time.monotonic())))
                except QgaError:
                    time.sleep(poll_interval)
                    continue
            if self.ping(timeout=ping_timeout):
                try:
                    self.sync()
                    return
                except (QgaError, OSError, ValueError):
                    pass
            self.close()
            time.sleep(poll_interval)
        raise QgaError(f"QEMU Guest Agent did not respond within {timeout}s")

    def guest_exec(
        self,
        path: str,
        arg: Optional[List[str]] = None,
        env: Optional[List[str]] = None,
        capture_output: bool = True,
        timeout: float = 30.0,
    ) -> GuestExecResult:
        if not self._synced:
            self.sync()
        arguments: Dict[str, Any] = {
            "path": path,
            "capture-output": capture_output,
        }
        if arg:
            arguments["arg"] = arg
        if env:
            arguments["env"] = env

        ret = self.execute("guest-exec", arguments)
        if not isinstance(ret, dict) or "pid" not in ret:
            raise QgaError(f"QGA guest-exec returned invalid payload: {ret!r}")
        pid = ret["pid"]
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            status = self.execute("guest-exec-status", {"pid": pid})
            if not isinstance(status, dict):
                raise QgaError(f"QGA guest-exec-status returned invalid payload: {status!r}")
            if status.get("exited"):
                out_b64 = status.get("out-data", "")
                err_b64 = status.get("err-data", "")
                try:
                    stdout = (
                        base64.b64decode(out_b64, validate=True).decode("utf-8", errors="replace")
                        if out_b64
                        else ""
                    )
                    stderr = (
                        base64.b64decode(err_b64, validate=True).decode("utf-8", errors="replace")
                        if err_b64
                        else ""
                    )
                except (binascii.Error, ValueError) as exc:
                    raise QgaError(f"QGA guest-exec-status returned invalid base64 output: {exc}") from exc
                return GuestExecResult(
                    exitcode=int(status.get("exitcode", -1)),
                    stdout=stdout,
                    stderr=stderr,
                    signal=status.get("signal"),
                )
            time.sleep(0.25)
        raise QgaError(f"guest-exec pid={pid} ({path}) timed out after {timeout}s")

    def run_shell(self, script: str, timeout: float = 30.0) -> GuestExecResult:
        return self.guest_exec("/bin/bash", ["-lc", script], timeout=timeout)
