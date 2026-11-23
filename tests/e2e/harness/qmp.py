"""QEMU Machine Protocol (QMP) client over AF_UNIX using Python stdlib only."""
import json
import os
import socket
import time
from typing import Any, Dict, List, Optional


class QmpError(RuntimeError):
    """Raised when QMP communication or a QMP command fails."""


class QmpClient:
    """Line-buffered JSON QMP client."""

    def __init__(self, sock_path: str, timeout: float = 10.0) -> None:
        self.sock_path = sock_path
        self.timeout = timeout
        self.sock: Optional[socket.socket] = None
        self._buf = b""
        self.greeting: Dict[str, Any] = {}
        self.events: List[Dict[str, Any]] = []

    def connect(self, wait_timeout: float = 20.0, proc: Optional[Any] = None) -> Dict[str, Any]:
        deadline = time.monotonic() + wait_timeout
        while time.monotonic() < deadline:
            if proc is not None and proc.poll() is not None:
                raise QmpError(f"QEMU process exited early with code {proc.returncode}")
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

        try:
            self.greeting = self._read_json_line()
            if "QMP" not in self.greeting:
                raise QmpError(f"Unexpected QMP banner: {self.greeting}")
            self.execute("qmp_capabilities")
            return self.greeting
        except Exception:
            self.close()
            raise

    def close(self) -> None:
        if self.sock is not None:
            try:
                self.sock.close()
            except OSError:
                pass
            self.sock = None
        self._buf = b""

    def _read_json_line(self) -> Dict[str, Any]:
        assert self.sock is not None
        try:
            while b"\n" not in self._buf:
                chunk = self.sock.recv(4096)
                if not chunk:
                    raise QmpError("QMP socket closed unexpectedly")
                self._buf += chunk
            line, self._buf = self._buf.split(b"\n", 1)
            msg = json.loads(line.decode("utf-8", errors="replace"))
        except QmpError:
            raise
        except (json.JSONDecodeError, UnicodeDecodeError, socket.timeout, OSError) as exc:
            raise QmpError(f"QMP protocol error: {exc}") from exc
        if not isinstance(msg, dict):
            raise QmpError(f"QMP returned non-object JSON: {msg!r}")
        return msg

    def execute(self, command: str, arguments: Optional[Dict[str, Any]] = None) -> Any:
        if self.sock is None:
            raise QmpError("QMP client is not connected")
        payload: Dict[str, Any] = {"execute": command}
        if arguments is not None:
            payload["arguments"] = arguments
        raw = (json.dumps(payload) + "\n").encode("utf-8")
        try:
            self.sock.sendall(raw)
        except (OSError, socket.timeout) as exc:
            raise QmpError(f"QMP command {command!r} send failed: {exc}") from exc

        while True:
            msg = self._read_json_line()
            if "event" in msg:
                self.events.append(msg)
                continue
            if "error" in msg:
                raise QmpError(f"QMP command {command!r} failed: {msg['error']}")
            if "return" in msg:
                return msg["return"]

    def query_status(self) -> Dict[str, Any]:
        return self.execute("query-status")

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

    def send_combo(self, *qcodes: str, hold_secs: float = 0.05) -> None:
        if not qcodes:
            return
        normalized = ["ret" if k == "return" else k for k in qcodes]
        for k in normalized:
            self.execute(
                "input-send-event",
                {"events": [{"type": "key", "data": {"key": {"type": "qcode", "data": k}, "down": True}}]},
            )
            time.sleep(0.02)
        time.sleep(hold_secs)
        for k in reversed(normalized):
            self.execute(
                "input-send-event",
                {"events": [{"type": "key", "data": {"key": {"type": "qcode", "data": k}, "down": False}}]},
            )
            time.sleep(0.02)

    def move_abs(self, x: int, y: int, width: int = 1280, height: int = 800) -> None:
        if width <= 0 or height <= 0:
            raise ValueError(f"Screen dimensions must be positive, got width={width}, height={height}")
        nx = max(0, min(32767, int(x / width * 32767)))
        ny = max(0, min(32767, int(y / height * 32767)))
        self.execute(
            "input-send-event",
            {
                "events": [
                    {"type": "abs", "data": {"axis": "x", "value": nx}},
                    {"type": "abs", "data": {"axis": "y", "value": ny}},
                ]
            },
        )

    def click(self, x: int, y: int, width: int = 1280, height: int = 800) -> None:
        self.move_abs(x, y, width=width, height=height)
        time.sleep(0.05)
        self.execute(
            "input-send-event",
            {"events": [{"type": "btn", "data": {"button": "left", "down": True}}]},
        )
        time.sleep(0.05)
        self.execute(
            "input-send-event",
            {"events": [{"type": "btn", "data": {"button": "left", "down": False}}]},
        )

    def quit(self) -> None:
        self.execute("quit")
