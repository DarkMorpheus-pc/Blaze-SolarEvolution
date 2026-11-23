"""Unit tests for the E2E harness (mock QMP/QGA sockets, PIL imaging, QEMU cmdline, CLI)."""
import base64
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import unittest
from unittest import mock
from PIL import Image, ImageDraw

from tests.e2e.config import (
    OVMF_CODE_PATH,
    REPO_ROOT,
    get_artifacts_dir,
    get_target_iso_path,
    get_target_iso_sha256_path,
)
from tests.e2e.harness.imaging import (
    DEFAULT_FIREHUB_UI_REGIONS,
    analyze_frame,
    analyze_ui_regions,
    convert_ppm_to_png,
    frame_diff_ratio,
    is_anaconda_webui_rendered,
    is_firehub_rendered,
    is_non_black_content,
)
from tests.e2e.harness.iso_inspect import find_zstd_initrd_offset, sha256_file
from tests.e2e.harness.qemu_vm import QemuVM
from tests.e2e.harness.qga import QgaClient
from tests.e2e.harness.qmp import QmpClient, QmpError


class HarnessUnitTests(unittest.TestCase):
    def test_qmp_client_handles_async_events_and_commands(self):
        with tempfile.TemporaryDirectory(prefix="blaze_qmp_u_") as tmp:
            sock_path = Path(tmp) / "qmp.sock"
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(str(sock_path))
            srv.listen(1)
            received = []

            def server_loop():
                conn, _ = srv.accept()
                with conn:
                    conn.sendall(b'{"QMP": {"version": {"qemu": {"major": 11}}, "capabilities": []}}\n')
                    buf = b""
                    while True:
                        chunk = conn.recv(4096)
                        if not chunk:
                            break
                        buf += chunk
                        while b"\n" in buf:
                            line, buf = buf.split(b"\n", 1)
                            cmd = json.loads(line.decode("utf-8"))
                            received.append(cmd["execute"])
                            if cmd["execute"] == "query-status":
                                conn.sendall(b'{"event": "RESUME"}\n')
                                conn.sendall(b'{"return": {"running": true, "status": "running"}}\n')
                            elif cmd["execute"] == "bad-cmd":
                                conn.sendall(b'{"error": {"class": "CommandNotFound", "desc": "unknown"}}\n')
                            else:
                                conn.sendall(b'{"return": {}}\n')
                            if cmd["execute"] == "quit":
                                return

            t = threading.Thread(target=server_loop, daemon=True)
            t.start()
            client = QmpClient(str(sock_path), timeout=3.0)
            try:
                greeting = client.connect(wait_timeout=3.0)
                self.assertIn("QMP", greeting)
                status = client.query_status()
                self.assertTrue(status["running"])
                self.assertEqual(len(client.events), 1)
                self.assertEqual(client.events[0]["event"], "RESUME")
                client.click(640, 400)
                client.send_combo("ctrl", "l")
                with self.assertRaises(QmpError):
                    client.execute("bad-cmd")
                client.quit()
            finally:
                client.close()
                self.assertEqual(client._buf, b"")
                srv.close()
                t.join(timeout=2.0)

            self.assertIn("qmp_capabilities", received)
            self.assertIn("input-send-event", received)
            self.assertIn("quit", received)

    def test_qga_client_guest_exec_decodes_base64_and_polls(self):
        with tempfile.TemporaryDirectory(prefix="blaze_qga_u_") as tmp:
            sock_path = Path(tmp) / "qga.sock"
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(str(sock_path))
            srv.listen(1)

            def server_loop():
                conn, _ = srv.accept()
                with conn:
                    buf = b""
                    polls = 0
                    while True:
                        chunk = conn.recv(4096)
                        if not chunk:
                            break
                        buf += chunk
                        while b"\n" in buf:
                            line, buf = buf.split(b"\n", 1)
                            req = json.loads(line.decode("utf-8"))
                            op = req["execute"]
                            if op == "guest-sync":
                                conn.sendall(
                                    (json.dumps({"return": req["arguments"]["id"]}) + "\n").encode()
                                )
                            elif op == "guest-ping":
                                conn.sendall(b'{"return": {}}\n')
                            elif op == "guest-exec":
                                conn.sendall(b'{"return": {"pid": 4242}}\n')
                            elif op == "guest-exec-status":
                                polls += 1
                                if polls == 1:
                                    conn.sendall(b'{"return": {"exited": false}}\n')
                                else:
                                    out = base64.b64encode(b"solarui-active\n").decode()
                                    err = base64.b64encode(b"warn\n").decode()
                                    resp = {
                                        "return": {
                                            "exited": True,
                                            "exitcode": 0,
                                            "out-data": out,
                                            "err-data": err,
                                        }
                                    }
                                    conn.sendall((json.dumps(resp) + "\n").encode())
                                    return

            t = threading.Thread(target=server_loop, daemon=True)
            t.start()
            client = QgaClient(str(sock_path), timeout=3.0)
            try:
                client.connect(wait_timeout=3.0)
                self.assertTrue(client.ping())
                res = client.run_shell("echo solarui-active")
                self.assertEqual(res.exitcode, 0)
                self.assertEqual(res.stdout.strip(), "solarui-active")
                self.assertEqual(res.stderr.strip(), "warn")
            finally:
                client.close()
                srv.close()
                t.join(timeout=2.0)

    def test_qga_client_guest_sync_drains_stale_replies_and_close_clears_buffer(self):
        with tempfile.TemporaryDirectory(prefix="blaze_qga_sync_") as tmp:
            sock_path = Path(tmp) / "qga.sock"
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(str(sock_path))
            srv.listen(1)
            sync_ids = []

            def server_loop():
                for conn_idx in range(2):
                    conn, _ = srv.accept()
                    with conn:
                        buf = b""
                        while True:
                            chunk = conn.recv(4096)
                            if not chunk:
                                break
                            buf += chunk
                            while b"\n" in buf:
                                line, buf = buf.split(b"\n", 1)
                                req = json.loads(line.decode("utf-8"))
                                op = req["execute"]
                                if op == "guest-ping":
                                    if conn_idx == 0:
                                        # Leave trailing partial bytes before close
                                        conn.sendall(b'{"return": {}}\n{"partial":')
                                    else:
                                        # Simulate 2 stale buffered ping replies ahead of sync
                                        conn.sendall(b'{"return": {}}\n{"return": {}}\n')
                                        conn.sendall(b'{"return": {}}\n')
                                elif op == "guest-sync":
                                    sid = req["arguments"]["id"]
                                    sync_ids.append(sid)
                                    conn.sendall((json.dumps({"return": sid}) + "\n").encode())
                                elif op == "guest-exec":
                                    conn.sendall(b'{"return": {"pid": 77}}\n')
                                elif op == "guest-exec-status":
                                    out = base64.b64encode(b"synced-ok\n").decode()
                                    conn.sendall(
                                        (
                                            json.dumps(
                                                {
                                                    "return": {
                                                        "exited": True,
                                                        "exitcode": 0,
                                                        "out-data": out,
                                                    }
                                                }
                                            )
                                            + "\n"
                                        ).encode()
                                    )
                                    return

            t = threading.Thread(target=server_loop, daemon=True)
            t.start()
            client = QgaClient(str(sock_path), timeout=3.0)
            try:
                client.connect(wait_timeout=3.0)
                self.assertTrue(client.ping(timeout=1.5))
                client.close()
                self.assertEqual(client._buf, b"")
                self.assertFalse(client._synced)

                client.wait_until_ready(timeout=5.0, poll_interval=0.1, ping_timeout=1.5)
                self.assertTrue(client._synced)
                res = client.run_shell("echo synced-ok")
                self.assertEqual(res.exitcode, 0)
                self.assertEqual(res.stdout.strip(), "synced-ok")
                self.assertTrue(len(sync_ids) >= 1)
            finally:
                client.close()
                srv.close()
                t.join(timeout=2.0)

    def test_qga_client_guest_sync_ignores_partial_json_and_stale_errors(self):
        from tests.e2e.harness.qga import QgaError

        with tempfile.TemporaryDirectory(prefix="blaze_qga_stale_") as tmp:
            sock_path = Path(tmp) / "qga.sock"
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(str(sock_path))
            srv.listen(1)

            def server_loop():
                # Connection 1: stale partial JSON, bad UTF-8, stale error, non-dict JSON, wrong id, then valid sync reply
                conn, _ = srv.accept()
                with conn:
                    buf = b""
                    while True:
                        chunk = conn.recv(4096)
                        if not chunk:
                            break
                        buf += chunk
                        if b"\n" in buf:
                            line, buf = buf.split(b"\n", 1)
                            req = json.loads(line.decode("utf-8"))
                            sid = req["arguments"]["id"]
                            conn.sendall(b'{"partial":\n')
                            conn.sendall(b'\xff\xfe{"bad_utf8": 1}\n')
                            conn.sendall(b'{"error": {"class": "GenericError", "desc": "stale"}}\n')
                            conn.sendall(b'123\n')
                            conn.sendall(b'{"return": 999999}\n')
                            conn.sendall((json.dumps({"return": sid}) + "\n").encode("utf-8"))
                            break

                # Connection 2: only stale error/partial lines -> must fail within bounded sync timeout
                conn2, _ = srv.accept()
                with conn2:
                    conn2.recv(4096)
                    conn2.sendall(b'{"partial":\n')
                    conn2.sendall(b'{"error": {"class": "GenericError", "desc": "stale"}}\n')
                    threading.Event().wait(1.5)

            t = threading.Thread(target=server_loop, daemon=True)
            t.start()
            client = QgaClient(str(sock_path), timeout=2.0)
            try:
                client.connect(wait_timeout=3.0)
                sid = client.sync(timeout=2.0)
                self.assertGreater(sid, 0)
                self.assertTrue(client._synced)
                client.close()

                client.connect(wait_timeout=3.0)
                with self.assertRaises(QgaError):
                    client.sync(timeout=0.3)
            finally:
                client.close()
                srv.close()
                t.join(timeout=2.5)

    def test_imaging_conversion_non_black_and_frame_diff(self):
        with tempfile.TemporaryDirectory(prefix="blaze_img_u_") as tmp:
            p = Path(tmp)
            black_ppm = p / "black.ppm"
            grub_ppm = p / "grub.ppm"
            desktop_ppm = p / "desktop.ppm"

            Image.new("RGB", (320, 200), (0, 0, 0)).save(black_ppm, format="PPM")

            grub_im = Image.new("RGB", (320, 200), (10, 12, 18))
            draw = ImageDraw.Draw(grub_im)
            draw.rectangle((40, 40, 280, 160), fill=(30, 80, 180))
            grub_im.save(grub_ppm, format="PPM")

            desk_im = Image.new("RGB", (320, 200), (140, 60, 210))
            draw2 = ImageDraw.Draw(desk_im)
            draw2.ellipse((80, 30, 240, 170), fill=(240, 220, 90))
            desk_im.save(desktop_ppm, format="PPM")

            black_png = convert_ppm_to_png(black_ppm, p / "black.png")
            grub_png = convert_ppm_to_png(grub_ppm, p / "grub.png")
            desk_png = convert_ppm_to_png(desktop_ppm, p / "desktop.png")

            self.assertFalse(is_non_black_content(black_png))
            self.assertTrue(is_non_black_content(grub_png))
            self.assertTrue(is_non_black_content(desk_png))
            stats = analyze_frame(desk_png)
            self.assertEqual((stats.width, stats.height), (320, 200))
            self.assertGreater(frame_diff_ratio(grub_png, desk_png), 0.5)
            self.assertAlmostEqual(frame_diff_ratio(grub_png, grub_png), 0.0)

    def test_qemu_vm_command_builder_uses_isolated_sockets_for_bios_and_uefi(self):
        with tempfile.TemporaryDirectory(prefix="blaze_vm_u_") as tmp:
            fake_iso = Path(tmp) / "fake.iso"
            fake_iso.write_bytes(b"ISO")
            for fw in ("bios", "uefi"):
                vm = QemuVM(iso_path=fake_iso, firmware=fw, artifacts_dir=Path(tmp) / "art")
                try:
                    if fw == "uefi" and not OVMF_CODE_PATH.is_file():
                        continue
                    cmd = vm.build_command()
                    joined = " ".join(cmd)
                    self.assertIn("virtio-tablet-pci", cmd)
                    self.assertIn("org.qemu.guest_agent.0", joined)
                    self.assertNotIn("/tmp/blaze_test_qmp.sock", joined)
                    self.assertIn(str(os.getpid()), vm.run_dir.name)
                    if fw == "uefi":
                        self.assertIn(str(OVMF_CODE_PATH), joined)
                        self.assertTrue(vm.ovmf_vars_copy.is_file())
                finally:
                    vm.cleanup()

    def test_qemu_vm_cleans_up_on_failed_start(self):
        with tempfile.TemporaryDirectory(prefix="blaze_vm_fail_") as tmp:
            fake_iso = Path(tmp) / "fake.iso"
            fake_iso.write_bytes(b"ISO")
            vm = QemuVM(iso_path=fake_iso, firmware="bios", artifacts_dir=Path(tmp) / "art")
            run_dir = vm.run_dir
            self.assertTrue(run_dir.is_dir())

            with mock.patch.object(
                vm,
                "build_command",
                return_value=[sys.executable, "-c", "import time; time.sleep(30)"],
            ), mock.patch.object(vm.qmp, "connect", side_effect=QmpError("mock connect failure")):
                with self.assertRaises(QmpError):
                    with vm:
                        self.fail("Should not enter context when start() fails")

            self.assertIsNone(vm.proc)
            self.assertIsNone(vm._stderr_fh)
            self.assertFalse(run_dir.exists(), f"run_dir leaked after failed start: {run_dir}")
            self.assertTrue(vm.stderr_log.is_file())

    def test_iso_inspect_helpers_zstd_magic_and_sha256(self):
        payload = b"070701" + b"\x00" * 64 + b"\x28\xb5\x2f\xfd" + b"zstd-data"
        self.assertEqual(find_zstd_initrd_offset(payload), 70)
        with self.assertRaises(ValueError):
            find_zstd_initrd_offset(b"no-magic-here")
        with tempfile.TemporaryDirectory(prefix="blaze_hash_u_") as tmp:
            f = Path(tmp) / "sample.bin"
            f.write_bytes(b"blazeos")
            self.assertEqual(len(sha256_file(f)), 64)

    def test_run_e2e_cli_help_dry_run_and_flag_propagation(self):
        res_help = subprocess.run(
            [sys.executable, "-m", "tests.e2e.run_e2e", "--help"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(res_help.returncode, 0, res_help.stderr)
        self.assertIn("--tier", res_help.stdout)
        self.assertIn("--firmware", res_help.stdout)

        custom_iso = "/tmp/nonexistent_override_blaze.iso"
        custom_art = "/tmp/nonexistent_override_art"
        res_dry = subprocess.run(
            [
                sys.executable,
                "-m",
                "tests.e2e.run_e2e",
                "--all",
                "--iso",
                custom_iso,
                "--artifacts-dir",
                custom_art,
                "--dry-run",
            ],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(res_dry.returncode, 0, res_dry.stderr)
        self.assertIn("DRY RUN", res_dry.stdout)
        self.assertIn(custom_iso, res_dry.stdout)
        self.assertIn(custom_art, res_dry.stdout)

        res_missing = subprocess.run(
            [
                sys.executable,
                "-m",
                "tests.e2e.run_e2e",
                "--tier",
                "1",
                "--iso",
                custom_iso,
                "--artifacts-dir",
                custom_art,
            ],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertNotEqual(res_missing.returncode, 0)
        self.assertIn(custom_iso, res_missing.stderr + res_missing.stdout)

        with mock.patch.dict(
            os.environ,
            {"BLAZE_E2E_ISO": custom_iso, "BLAZE_E2E_ARTIFACTS": custom_art},
        ):
            self.assertEqual(get_target_iso_path(), Path(custom_iso))
            self.assertEqual(get_target_iso_sha256_path(), Path(f"{custom_iso}.sha256"))
            self.assertEqual(get_artifacts_dir(), Path(custom_art))

    def test_tier4_installer_proc_helpers_avoid_shell_self_match(self):
        from tests.e2e.harness.qga import GuestExecResult
        from tests.e2e.test_tier4_qemu_e2e import Tier4QemuE2ETests

        tc = Tier4QemuE2ETests("test_live_boot_legacy_bios")
        vm = mock.MagicMock()

        vm.qga.guest_exec.return_value = GuestExecResult(exitcode=1, stdout="", stderr="")
        self.assertEqual(tc._find_installer_procs(vm), [])
        vm.qga.guest_exec.assert_called_once_with(
            "/usr/bin/pgrep",
            ["-fa", "[a]naconda|[l]iveinst|[w]ebui-desktop"],
            timeout=15.0,
        )
        vm.qga.run_shell.assert_not_called()

        vm.qga.guest_exec.reset_mock()
        vm.qga.guest_exec.return_value = GuestExecResult(
            exitcode=0,
            stdout="1234 /usr/bin/python3 /usr/sbin/anaconda --liveinst --graphical\n",
            stderr="",
        )
        procs = tc._find_installer_procs(vm)
        self.assertEqual(procs, ["1234 /usr/bin/python3 /usr/sbin/anaconda --liveinst --graphical"])

        # Simulate auto-launch appearing, then terminating cleanly after pkill -TERM
        pgrep_seq = [
            GuestExecResult(exitcode=0, stdout="2001 /usr/bin/bash /usr/bin/liveinst\n", stderr=""),
            GuestExecResult(exitcode=1, stdout="", stderr=""),
            GuestExecResult(exitcode=1, stdout="", stderr=""),
        ]

        def guest_exec_side_effect(path, arg=None, **kwargs):
            if path == "/usr/bin/pgrep":
                return pgrep_seq.pop(0) if pgrep_seq else GuestExecResult(exitcode=1, stdout="", stderr="")
            return GuestExecResult(exitcode=0, stdout="", stderr="")

        vm2 = mock.MagicMock()
        vm2.qga.guest_exec.side_effect = guest_exec_side_effect
        vm2.qga.run_shell.return_value = GuestExecResult(exitcode=0, stdout="", stderr="")
        tc._stop_auto_launched_installer(vm2, "bios")

        exec_cmds = [(call.args[0], call.args[1]) for call in vm2.qga.guest_exec.call_args_list]
        systemctl_indices = [
            i
            for i, (cmd, args) in enumerate(exec_cmds)
            if cmd == "/usr/bin/systemctl" and args == ["stop", "webui-cockpit-ws.service"]
        ]
        pkill_indices = [i for i, (cmd, _) in enumerate(exec_cmds) if cmd == "/usr/bin/pkill"]
        rm_indices = [
            i
            for i, (cmd, args) in enumerate(exec_cmds)
            if cmd == "/usr/bin/rm"
            and args == ["-f", "/run/anaconda/anaconda.conf", "/run/anaconda/bus.address"]
        ]
        self.assertTrue(pkill_indices, "Expected direct guest_exec('/usr/bin/pkill', ...)")
        self.assertEqual(
            exec_cmds[pkill_indices[0]][1],
            ["-TERM", "-f", "[a]naconda|[l]iveinst|[w]ebui-desktop"],
        )
        self.assertGreaterEqual(
            len(systemctl_indices),
            2,
            f"Expected systemctl stop webui-cockpit-ws.service before and after pkill: {exec_cmds}",
        )
        self.assertLess(
            systemctl_indices[0],
            pkill_indices[0],
            f"webui-cockpit-ws.service must be stopped before pkill: {exec_cmds}",
        )
        self.assertGreater(
            systemctl_indices[-1],
            pkill_indices[0],
            f"webui-cockpit-ws.service should be stopped again after pkill: {exec_cmds}",
        )
        self.assertTrue(rm_indices, f"Expected stale /run/anaconda file cleanup: {exec_cmds}")
        self.assertGreater(rm_indices[-1], systemctl_indices[-1])

    def test_tier4_has_installer_ui_proc_ignores_webui_desktop(self):
        from tests.e2e.test_tier4_qemu_e2e import Tier4QemuE2ETests

        early_procs = [
            "1001 /usr/bin/bash /usr/bin/liveinst",
            "1002 /usr/bin/bash /usr/libexec/anaconda/webui-desktop -t live",
            "1003 /usr/bin/python3 /usr/sbin/anaconda --liveinst --graphical",
            "1004 /bin/sh /usr/libexec/anaconda/cockpit-coproc-wrapper.sh",
        ]
        self.assertFalse(Tier4QemuE2ETests._has_installer_ui_proc(early_procs))
        self.assertFalse(Tier4QemuE2ETests._has_installer_ui_proc([]))

        self.assertTrue(
            Tier4QemuE2ETests._has_installer_ui_proc(
                early_procs
                + [
                    "1005 /usr/bin/firefox --new-instance --profile /run/user/1000/anaconda/firefox-profile http://127.0.0.1/cockpit/@localhost/anaconda-webui/index.html"
                ]
            )
        )
        self.assertTrue(
            Tier4QemuE2ETests._has_installer_ui_proc(
                ["1006 /usr/libexec/anaconda/slitherer http://127.0.0.1/cockpit/@localhost/anaconda-webui/index.html"]
            )
        )
        self.assertTrue(
            Tier4QemuE2ETests._has_installer_ui_proc(
                ["1007 /usr/bin/epiphany http://127.0.0.1/cockpit/@localhost/anaconda-webui/index.html"]
            )
        )

    @staticmethod
    def _make_synthetic_blank_plus_welcome(path: Path) -> Path:
        im = Image.new("RGB", (1280, 800), (41, 41, 41))
        draw = ImageDraw.Draw(im)
        # Simulated 750x590 solar-welcome centered over blank #292929 Firefox window
        draw.rectangle((265, 105, 1015, 695), fill=(28, 30, 36), outline=(56, 200, 255), width=2)
        draw.rectangle((285, 120, 995, 165), fill=(40, 44, 52))
        draw.rectangle((310, 220, 960, 620), fill=(35, 38, 45))
        im.save(path, format="PNG")
        return path

    @staticmethod
    def _make_synthetic_rendered_anaconda(path: Path) -> Path:
        im = Image.new("RGB", (1280, 800), (18, 24, 34))
        draw = ImageDraw.Draw(im)
        # Centered 1050x656 floating liveinst window
        draw.rectangle((115, 89, 1165, 745), fill=(41, 41, 41))
        # Blue PatternFly top banner + white title text blocks
        draw.rectangle((115, 89, 1165, 149), fill=(79, 144, 201))
        draw.rectangle((165, 110, 460, 128), fill=(245, 248, 252))
        # Left sidebar steps (inside 140..330, 170..660)
        for y in (190, 250, 310, 370, 430):
            draw.ellipse((150, y, 168, y + 18), fill=(79, 144, 201))
            draw.rectangle((180, y + 3, 310, y + 15), fill=(215, 220, 225))
        # Center Welcome content widgets (inside 380..900, 220..620)
        draw.rectangle((420, 240, 820, 270), fill=(235, 238, 242))
        draw.rectangle((420, 310, 760, 355), fill=(62, 68, 76), outline=(140, 150, 160))
        draw.rectangle((420, 400, 760, 445), fill=(62, 68, 76), outline=(140, 150, 160))
        im.save(path, format="PNG")
        return path

    @staticmethod
    def _make_synthetic_textured_desktop_frame(path: Path) -> Path:
        im = Image.new("RGB", (1280, 800), (88, 106, 132))
        draw = ImageDraw.Draw(im)
        # Simulated bright snowy mountain wallpaper + top bar + textured solar-welcome
        draw.polygon([(0, 800), (420, 240), (860, 800)], fill=(155, 172, 194))
        draw.polygon([(400, 800), (920, 190), (1280, 800)], fill=(175, 190, 210))
        draw.rectangle((0, 0, 1280, 34), fill=(22, 26, 34))
        draw.rectangle((265, 122, 1015, 712), fill=(30, 34, 42), outline=(56, 200, 255), width=2)
        draw.rectangle((295, 150, 985, 205), fill=(52, 60, 74))
        draw.rectangle((315, 240, 965, 670), fill=(44, 50, 62))
        for y in range(260, 620, 48):
            draw.rectangle((335, y, 470, y + 26), fill=(68, 76, 92), outline=(140, 155, 175))
            draw.rectangle((495, y + 5, 860, y + 20), fill=(210, 216, 224))
        im.save(path, format="PNG")
        return path

    def test_analyze_ui_regions_rejects_blank_and_welcome_overlap_accepts_rendered(self):
        with tempfile.TemporaryDirectory(prefix="blaze_ui_reg_") as tmp:
            p = Path(tmp)
            pure_blank = p / "pure_blank.png"
            Image.new("RGB", (1280, 800), (41, 41, 41)).save(pure_blank, format="PNG")
            blank_welcome = self._make_synthetic_blank_plus_welcome(p / "blank_welcome.png")
            desktop_flat = self._make_synthetic_desktop_frame(p / "desktop_flat.png")
            desktop_textured = self._make_synthetic_textured_desktop_frame(p / "desktop_textured.png")
            rendered_ana = self._make_synthetic_rendered_anaconda(p / "rendered_ana.png")

            blank_stats = analyze_ui_regions(pure_blank)
            self.assertEqual(blank_stats["left_sidebar"].std_luma, 0.0)
            self.assertEqual(blank_stats["top_banner"].non_canvas_ratio, 0.0)
            self.assertFalse(is_anaconda_webui_rendered(pure_blank)[0])

            overlap_ok, overlap_stats = is_anaconda_webui_rendered(blank_welcome)
            self.assertFalse(
                overlap_ok,
                f"Blank #292929 + solar-welcome overlap must fail WebUI oracle: {overlap_stats}",
            )
            self.assertEqual(overlap_stats["outer_sidebar"].std_luma, 0.0)
            self.assertEqual(overlap_stats["outer_sidebar"].non_canvas_ratio, 0.0)

            for label, desk_path in (
                ("flat desktop", desktop_flat),
                ("textured desktop", desktop_textured),
            ):
                desk_ok, desk_stats = is_anaconda_webui_rendered(desk_path)
                self.assertFalse(
                    desk_ok,
                    f"SolarUI {label} frame must fail Anaconda WebUI oracle: {desk_stats}",
                )

            rendered_ok, rendered_stats = is_anaconda_webui_rendered(rendered_ana)
            self.assertTrue(
                rendered_ok,
                f"Rendered floating Anaconda WebUI frame must pass oracle: {rendered_stats}",
            )

    def test_ui_oracles_return_false_on_720x400_without_breaking_analyze_ui_regions_error(self):
        with tempfile.TemporaryDirectory(prefix="blaze_720x400_") as tmp:
            grub_720 = Path(tmp) / "grub_720x400.png"
            im = Image.new("RGB", (720, 400), (0, 0, 0))
            draw = ImageDraw.Draw(im)
            draw.rectangle((40, 60, 680, 320), outline=(170, 170, 170))
            draw.rectangle((60, 110, 420, 130), fill=(210, 210, 210))
            im.save(grub_720, format="PNG")

            self.assertEqual(is_anaconda_webui_rendered(grub_720), (False, {}))
            self.assertEqual(is_firehub_rendered(grub_720), (False, {}))
            with self.assertRaises(ValueError):
                analyze_ui_regions(grub_720, regions=DEFAULT_FIREHUB_UI_REGIONS)

    def test_stop_auto_launched_installer_enforces_post_rm_mount_cleanup(self):
        from tests.e2e.harness.qga import GuestExecResult
        from tests.e2e.test_tier4_qemu_e2e import Tier4QemuE2ETests

        tc = Tier4QemuE2ETests("test_live_boot_legacy_bios")
        vm = mock.MagicMock()
        pgrep_seq = [
            GuestExecResult(exitcode=0, stdout="2001 /usr/bin/bash /usr/bin/liveinst\n", stderr=""),
            GuestExecResult(exitcode=1, stdout="", stderr=""),
            GuestExecResult(exitcode=1, stdout="", stderr=""),
        ]

        def guest_exec_se(path, args=None, **kwargs):
            if path == "/usr/bin/pgrep":
                return pgrep_seq.pop(0) if pgrep_seq else GuestExecResult(exitcode=1, stdout="", stderr="")
            return GuestExecResult(exitcode=0, stdout="", stderr="")

        def run_shell_se(cmd, **kwargs):
            if "anaconda-cleanup" in cmd:
                return GuestExecResult(
                    exitcode=1,
                    stdout="",
                    stderr="STALE_INSTALL_MOUNTS:/run/install/sources/mount-0000-live_os_image",
                )
            return GuestExecResult(exitcode=0, stdout="", stderr="")

        vm.qga.guest_exec.side_effect = guest_exec_se
        vm.qga.run_shell.side_effect = run_shell_se
        with self.assertRaises(AssertionError) as ctx:
            tc._stop_auto_launched_installer(vm, "bios")
        self.assertIn("Post-stop installer mount/runtime cleanup failed", str(ctx.exception))
        last_shell_cmd = vm.qga.run_shell.call_args_list[-1].args[0]
        self.assertIn("/usr/bin/anaconda-cleanup --liveinst", last_shell_cmd)
        self.assertIn("/run/anaconda/backend_ready", last_shell_cmd)
        self.assertIn("/usr/sbin/setenforce Enforcing", last_shell_cmd)
        exec_cmds = [(call.args[0], call.args[1]) for call in vm.qga.guest_exec.call_args_list]
        self.assertIn(("/usr/bin/pkill", ["-KILL", "-f", "/[r]un/install/"]), exec_cmds)
        self.assertNotIn("pkill", last_shell_cmd)
        self.assertNotIn("pgrep", last_shell_cmd)

    def test_step7_4gate_oracle_checks_backend_floating_window_and_visual_settle(self):
        from tests.e2e.harness.qga import GuestExecResult
        from tests.e2e.test_tier4_qemu_e2e import Tier4QemuE2ETests

        tc = Tier4QemuE2ETests("test_live_boot_legacy_bios")
        vm = mock.MagicMock()

        # Gate 1: backend_ready + HTTP 200
        vm.qga.guest_exec.return_value = GuestExecResult(
            exitcode=0,
            stdout="1005 /usr/bin/firefox --new-instance\n",
            stderr="",
        )
        vm.qga.run_shell.return_value = GuestExecResult(exitcode=0, stdout="HTTP:200\n", stderr="")
        procs = tc._wait_for_anaconda_backend_ready(vm, "bios", timeout=2.0)
        self.assertEqual(len(procs), 1)

        # Gate 2: tiled (is_floating=False, e.g. 1280x766) must fail even with focus
        with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
            vm.qga.run_shell.return_value = GuestExecResult(
                exitcode=0,
                stdout=json.dumps([
                    {
                        "id": 3,
                        "app_id": "liveinst",
                        "title": "",
                        "is_focused": True,
                        "is_floating": False,
                        "layout": {"window_size": [1280, 766], "tile_size": [1280.0, 766.0]},
                    }
                ]),
                stderr="",
            )
            with self.assertRaises(AssertionError) as tiled_ctx:
                tc._verify_anaconda_niri_window(vm, "bios", "diag", timeout=0.05)
            self.assertIn("is_floating=True", str(tiled_ctx.exception))

            # Gate 2: browser error titles must fail even when floating, focused, and 82% sized
            for err_title in ("Problem loading page", "Unable to connect", "404 Not Found"):
                vm.qga.run_shell.return_value = GuestExecResult(
                    exitcode=0,
                    stdout=json.dumps([
                        {
                            "id": 5,
                            "app_id": "liveinst",
                            "title": err_title,
                            "is_focused": True,
                            "is_floating": True,
                            "layout": {"window_size": [1049, 628], "tile_size": [1049.0, 628.0]},
                        }
                    ]),
                    stderr="",
                )
                with self.assertRaises(AssertionError) as err_ctx:
                    tc._verify_anaconda_niri_window(vm, "bios", "diag", timeout=0.05)
                self.assertIn("browser error", str(err_ctx.exception))

            # Gate 2: tiny modal window (< 900x550) must fail geometry check
            vm.qga.run_shell.return_value = GuestExecResult(
                exitcode=0,
                stdout=json.dumps([
                    {
                        "id": 5,
                        "app_id": "liveinst",
                        "title": "",
                        "is_focused": True,
                        "is_floating": True,
                        "layout": {"window_size": [420, 260], "tile_size": [420.0, 260.0]},
                    }
                ]),
                stderr="",
            )
            with self.assertRaises(AssertionError) as tiny_ctx:
                tc._verify_anaconda_niri_window(vm, "bios", "diag", timeout=0.05)
            self.assertIn("[900, 550]", str(tiny_ctx.exception))

            # Gate 2: unfocused liveinst while solar-welcome holds focus must fail
            vm.qga.run_shell.return_value = GuestExecResult(
                exitcode=0,
                stdout=json.dumps([
                    {
                        "id": 2,
                        "app_id": "solar-welcome",
                        "title": "SolarUI Desktop — Karşılama ve Kısayollar",
                        "is_focused": True,
                        "is_floating": True,
                        "layout": {"window_size": [750, 590], "tile_size": [750.0, 590.0]},
                    },
                    {
                        "id": 5,
                        "app_id": "liveinst",
                        "title": "",
                        "is_focused": False,
                        "is_floating": True,
                        "layout": {"window_size": [1049, 628], "tile_size": [1049.0, 628.0]},
                    },
                ]),
                stderr="",
            )
            with self.assertRaises(AssertionError) as focus_ctx:
                tc._verify_anaconda_niri_window(vm, "bios", "diag", timeout=0.05)
            self.assertIn("not found", str(focus_ctx.exception))

            # Gate 2: solar-welcome focused simultaneously above liveinst must fail
            vm.qga.run_shell.return_value = GuestExecResult(
                exitcode=0,
                stdout=json.dumps([
                    {
                        "id": 2,
                        "app_id": "solar-welcome",
                        "title": "SolarUI Desktop — Karşılama ve Kısayollar",
                        "is_focused": True,
                        "is_floating": True,
                        "layout": {"window_size": [750, 590], "tile_size": [750.0, 590.0]},
                    },
                    {
                        "id": 5,
                        "app_id": "liveinst",
                        "title": "",
                        "is_focused": True,
                        "is_floating": True,
                        "layout": {"window_size": [1049, 628], "tile_size": [1049.0, 628.0]},
                    },
                ]),
                stderr="",
            )
            with self.assertRaises(AssertionError) as above_ctx:
                tc._verify_anaconda_niri_window(vm, "bios", "diag", timeout=0.05)
            self.assertIn("solar-welcome is focused above liveinst", str(above_ctx.exception))

        # Gate 2: real empty-title liveinst (MOZ_APP_TITLE="") + floating + focused + [1049, 628] over solar-welcome passes
        vm.qga.run_shell.return_value = GuestExecResult(
            exitcode=0,
            stdout=json.dumps([
                {
                    "id": 2,
                    "title": "SolarUI Desktop — Karşılama ve Kısayollar",
                    "app_id": "solar-welcome",
                    "is_focused": False,
                    "is_floating": True,
                    "layout": {"tile_size": [750.0, 590.0], "window_size": [750, 590]},
                },
                {
                    "id": 5,
                    "title": "",
                    "app_id": "liveinst",
                    "is_focused": True,
                    "is_floating": True,
                    "layout": {"tile_size": [1049.0, 628.0], "window_size": [1049, 628]},
                },
            ]),
            stderr="",
        )
        win = tc._verify_anaconda_niri_window(vm, "bios", "diag", timeout=0.5)
        self.assertTrue(win["is_floating"])
        self.assertEqual(win["layout"]["window_size"], [1049, 628])

        # Gate 2: fallback tile_size + non-empty valid title also passes
        vm.qga.run_shell.return_value = GuestExecResult(
            exitcode=0,
            stdout=json.dumps([
                {
                    "id": 6,
                    "title": "Anaconda Web UI",
                    "app_id": "firefox",
                    "is_focused": True,
                    "is_floating": True,
                    "layout": {"tile_size": [1049.0, 628.0]},
                }
            ]),
            stderr="",
        )
        win_fb = tc._verify_anaconda_niri_window(vm, "bios", "diag", timeout=0.5)
        self.assertEqual(win_fb["app_id"], "firefox")

        # Gates 3 & 4: reject blank+welcome, accept settled rendered frame
        with tempfile.TemporaryDirectory(prefix="blaze_step7_u_") as tmp:
            p = Path(tmp)
            desktop_png = self._make_synthetic_blank_plus_welcome(p / "desktop.png")
            # Re-color desktop background to distinguish from #292929
            desk_im = Image.open(desktop_png).convert("RGB")
            ImageDraw.Draw(desk_im).rectangle((0, 0, 1280, 90), fill=(90, 100, 115))
            desk_im.save(desktop_png, format="PNG")

            blank_png = self._make_synthetic_blank_plus_welcome(p / "blank.png")
            probe_a_png = self._make_synthetic_rendered_anaconda(p / "05_anaconda_bios_probe_a.png")
            rendered_png = self._make_synthetic_rendered_anaconda(p / "05_anaconda_bios.png")

            vm_blank = mock.MagicMock()
            vm_blank.capture_png.return_value = blank_png
            with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
                with self.assertRaises(AssertionError):
                    tc._capture_settled_anaconda_frame(vm_blank, "bios", desktop_png, "diag", timeout=0.05)

            vm_same = mock.MagicMock()
            vm_same.capture_png.side_effect = [rendered_png, rendered_png]
            with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
                with self.assertRaises(AssertionError):
                    tc._capture_settled_anaconda_frame(vm_same, "bios", desktop_png, "diag", timeout=1.0)

            vm_ok = mock.MagicMock()
            vm_ok.capture_png.side_effect = [probe_a_png, rendered_png]
            with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None), mock.patch(
                "tests.e2e.test_tier4_qemu_e2e.frame_diff_ratio",
                wraps=frame_diff_ratio,
            ) as diff_spy:
                settled = tc._capture_settled_anaconda_frame(vm_ok, "bios", desktop_png, "diag", timeout=1.0)
            self.assertEqual(settled, rendered_png)
            self.assertEqual(
                [call.args[0] for call in vm_ok.capture_png.call_args_list],
                ["05_anaconda_bios_probe_a", "05_anaconda_bios"],
            )
            diff_pairs = [tuple(call.args[:2]) for call in diff_spy.call_args_list]
            self.assertIn((probe_a_png, rendered_png), diff_pairs)
            self.assertNotEqual(probe_a_png, rendered_png)
            self.assertFalse(probe_a_png.exists(), "Temporary probe_a PNG should be removed after settle check")
            self.assertTrue(rendered_png.exists(), "Settled final PNG must remain in artifacts_dir")

    @staticmethod
    def _make_synthetic_unrendered_firehub(path: Path) -> Path:
        im = Image.new("RGB", (1280, 800), (45, 70, 105))
        draw = ImageDraw.Draw(im)
        # Noctalia top bar
        draw.rectangle((0, 0, 1280, 32), fill=(20, 24, 32))
        # Centered 75%x80% FireHub floating window with solid #0d0f12 canvas
        draw.rectangle((135, 54, 1144, 779), fill=(13, 15, 18), outline=(56, 200, 255), width=2)
        draw.rectangle((137, 56, 1142, 92), fill=(26, 29, 36))
        draw.rectangle((155, 68, 235, 82), fill=(220, 226, 234))
        # Center mouse cursor at (640, 400)
        draw.polygon([(640, 400), (640, 418), (652, 412)], fill=(250, 250, 250), outline=(10, 10, 10))
        im.save(path, format="PNG")
        return path

    @staticmethod
    def _make_synthetic_firehub_plus_welcome(path: Path) -> Path:
        im = Image.new("RGB", (1280, 800), (45, 70, 105))
        draw = ImageDraw.Draw(im)
        draw.rectangle((135, 54, 1144, 779), fill=(13, 15, 18), outline=(56, 200, 255), width=2)
        draw.rectangle((137, 56, 1142, 92), fill=(26, 29, 36))
        # Centered 750x590 solar-welcome sitting inside (265..1015, 122..712)
        draw.rectangle((265, 122, 1015, 712), fill=(32, 36, 44), outline=(56, 200, 255), width=2)
        draw.rectangle((290, 145, 990, 195), fill=(48, 56, 70))
        draw.rectangle((310, 230, 970, 660), fill=(42, 48, 58))
        im.save(path, format="PNG")
        return path

    @staticmethod
    def _make_synthetic_desktop_frame(path: Path) -> Path:
        im = Image.new("RGB", (1280, 800), (88, 106, 132))
        draw = ImageDraw.Draw(im)
        # Simulated bright snowy mountain wallpaper + top bar + solar-welcome
        draw.polygon([(0, 800), (420, 240), (860, 800)], fill=(155, 172, 194))
        draw.polygon([(400, 800), (920, 190), (1280, 800)], fill=(175, 190, 210))
        draw.rectangle((0, 0, 1280, 34), fill=(22, 26, 34))
        draw.rectangle((265, 122, 1015, 712), fill=(30, 34, 42), outline=(56, 200, 255), width=2)
        draw.rectangle((295, 150, 985, 205), fill=(52, 60, 74))
        draw.rectangle((315, 240, 965, 670), fill=(44, 50, 62))
        im.save(path, format="PNG")
        return path

    @staticmethod
    def _make_synthetic_rendered_firehub(path: Path, with_badge: bool = True) -> Path:
        im = Image.new("RGB", (1280, 800), (45, 70, 105))
        draw = ImageDraw.Draw(im)
        draw.rectangle((0, 0, 1280, 32), fill=(20, 24, 32))
        draw.rectangle((135, 54, 1144, 779), fill=(14, 16, 20), outline=(56, 200, 255), width=2)
        draw.rectangle((137, 56, 1142, 92), fill=(26, 29, 36))
        # Header logo, search bar, Sign in button, and category tabs (y=115..215)
        draw.rectangle((170, 126, 235, 148), fill=(235, 110, 45))
        draw.rectangle((360, 124, 760, 152), fill=(24, 28, 36), outline=(48, 54, 66))
        draw.rectangle((1035, 124, 1105, 152), fill=(52, 120, 225))
        for tx in (170, 290, 410, 530, 650, 1030):
            draw.rectangle((tx, 182, tx + 76, 198), fill=(24, 28, 36))
        # Subtle dark banner with left/right accents inside outer_left/outer_right
        draw.rectangle((170, 240, 1110, 335), fill=(20, 24, 32), outline=(36, 44, 58))
        draw.rectangle((185, 258, 235, 308), fill=(72, 140, 215))
        draw.rectangle((1045, 258, 1095, 308), fill=(72, 140, 215))
        draw.rectangle((260, 266, 520, 280), fill=(210, 218, 230))
        # 3 Editor's choice cards (y=396..480) leaving dark canvas margins inside 380..500
        card_specs = [
            (170, 468, (235, 120, 40)),
            (490, 788, (240, 145, 45)),
            (810, 1110, (55, 155, 240)),
        ]
        for x0, x1, accent in card_specs:
            draw.rectangle((x0, 396, x1, 480), fill=(24, 28, 36), outline=(46, 52, 64))
            draw.rectangle((x0 + 16, 414, x0 + 62, 460), fill=accent)
            draw.rectangle((x0 + 78, 418, x1 - 24, 430), fill=(215, 222, 232))
            draw.rectangle((x0 + 78, 442, x1 - 44, 452), fill=(120, 130, 146))
        if with_badge:
            draw.rectangle((826, 464, 846, 474), fill=(90, 200, 140))
        # 8 Categories dark-slate cards in 2 rows (y=575..735)
        for row_y in (575, 662):
            for col_x in (170, 410, 650, 890):
                draw.rectangle((col_x, row_y, col_x + 220, row_y + 72), fill=(23, 27, 35), outline=(38, 44, 54))
                draw.rectangle((col_x + 18, row_y + 30, col_x + 110, row_y + 38), fill=(150, 160, 175))
        im.save(path, format="PNG")
        return path

    def test_is_firehub_rendered_rejects_unrendered_and_desktop_accepts_storefront(self):
        with tempfile.TemporaryDirectory(prefix="blaze_fh_reg_") as tmp:
            p = Path(tmp)
            pure_blank = p / "pure_blank.png"
            Image.new("RGB", (1280, 800), (14, 16, 20)).save(pure_blank, format="PNG")
            unrendered = self._make_synthetic_unrendered_firehub(p / "unrendered_cursor.png")
            overlap = self._make_synthetic_firehub_plus_welcome(p / "blank_plus_welcome.png")
            desktop = self._make_synthetic_desktop_frame(p / "desktop.png")
            rendered = self._make_synthetic_rendered_firehub(p / "rendered_firehub.png")

            self.assertFalse(is_firehub_rendered(pure_blank)[0])
            unrendered_ok, unrendered_stats = is_firehub_rendered(unrendered)
            self.assertFalse(unrendered_ok, f"Blank #0d0f12 + cursor must fail: {unrendered_stats}")
            overlap_ok, overlap_stats = is_firehub_rendered(overlap)
            self.assertFalse(overlap_ok, f"Blank FireHub + solar-welcome overlap must fail: {overlap_stats}")
            desktop_ok, desktop_stats = is_firehub_rendered(desktop)
            self.assertFalse(desktop_ok, f"SolarUI desktop frame must fail: {desktop_stats}")
            rendered_ok, rendered_stats = is_firehub_rendered(rendered)
            self.assertTrue(rendered_ok, f"Rendered FireHub storefront must pass: {rendered_stats}")

    def test_step6_firehub_oracle_polls_and_settles_rendered_frame(self):
        from tests.e2e.test_tier4_qemu_e2e import Tier4QemuE2ETests

        tc = Tier4QemuE2ETests("test_live_boot_legacy_bios")
        with tempfile.TemporaryDirectory(prefix="blaze_step6_u_") as tmp:
            p = Path(tmp)
            desktop_png = self._make_synthetic_desktop_frame(p / "03_desktop_bios.png")
            unrendered_png = self._make_synthetic_unrendered_firehub(p / "unrendered_cursor.png")
            probe_a_png = self._make_synthetic_rendered_firehub(
                p / "04_firehub_bios_probe_a.png", with_badge=False
            )
            rendered_png = self._make_synthetic_rendered_firehub(
                p / "04_firehub_bios.png", with_badge=True
            )

            # Confirm naive diff check alone would have passed unrendered frame
            self.assertGreater(frame_diff_ratio(desktop_png, unrendered_png), 0.15)
            pre_settle_diff = frame_diff_ratio(probe_a_png, rendered_png)
            self.assertGreater(pre_settle_diff, 0.0)
            self.assertLess(pre_settle_diff, 0.02)

            vm_blank = mock.MagicMock()
            vm_blank.capture_png.return_value = unrendered_png
            with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
                with self.assertRaises(AssertionError) as blank_ctx:
                    tc._capture_settled_firehub_frame(vm_blank, "bios", desktop_png, "diag", timeout=0.05)
            self.assertIn("FireHub UI regions did not render", str(blank_ctx.exception))

            vm_desk = mock.MagicMock()
            vm_desk.capture_png.return_value = desktop_png
            with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
                with self.assertRaises(AssertionError) as desk_ctx:
                    tc._capture_settled_firehub_frame(vm_desk, "bios", desktop_png, "diag", timeout=0.05)
            self.assertIn("diff vs desktop", str(desk_ctx.exception))

            vm_same = mock.MagicMock()
            vm_same.capture_png.side_effect = [rendered_png, rendered_png]
            with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
                with self.assertRaises(AssertionError) as same_ctx:
                    tc._capture_settled_firehub_frame(vm_same, "bios", desktop_png, "diag", timeout=1.0)
            self.assertIn("must be distinct paths", str(same_ctx.exception))

            vm_ok = mock.MagicMock()
            vm_ok.capture_png.side_effect = [probe_a_png, rendered_png]
            with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None), mock.patch(
                "tests.e2e.test_tier4_qemu_e2e.frame_diff_ratio",
                wraps=frame_diff_ratio,
            ) as diff_spy:
                settled = tc._capture_settled_firehub_frame(vm_ok, "bios", desktop_png, "diag", timeout=1.0)
            self.assertEqual(settled, rendered_png)
            self.assertEqual(
                [call.args[0] for call in vm_ok.capture_png.call_args_list],
                ["04_firehub_bios_probe_a", "04_firehub_bios"],
            )
            diff_pairs = [tuple(call.args[:2]) for call in diff_spy.call_args_list]
            self.assertIn((probe_a_png, rendered_png), diff_pairs)
            self.assertGreater(frame_diff_ratio(desktop_png, rendered_png), 0.15)
            self.assertFalse(probe_a_png.exists(), "Temporary probe_a PNG should be removed after settle check")
            self.assertTrue(rendered_png.exists(), "Settled final PNG must remain in artifacts_dir")

    def test_probe_a_cleanup_on_exceptions_and_sleep_cap(self):
        import time
        from tests.e2e.test_tier4_qemu_e2e import Tier4QemuE2ETests

        tc = Tier4QemuE2ETests("test_live_boot_legacy_bios")
        with tempfile.TemporaryDirectory(prefix="blaze_probe_cleanup_") as tmp:
            p = Path(tmp)
            desktop_png = self._make_synthetic_desktop_frame(p / "03_desktop_bios.png")
            ana_blank = self._make_synthetic_blank_plus_welcome(p / "ana_blank.png")
            fh_blank = self._make_synthetic_unrendered_firehub(p / "fh_blank.png")

            targets = [
                (
                    "_capture_settled_anaconda_frame",
                    p / "05_anaconda_bios_probe_a.png",
                    p / "05_anaconda_bios.png",
                    self._make_synthetic_rendered_anaconda,
                    ana_blank,
                ),
                (
                    "_capture_settled_firehub_frame",
                    p / "04_firehub_bios_probe_a.png",
                    p / "04_firehub_bios.png",
                    self._make_synthetic_rendered_firehub,
                    fh_blank,
                ),
            ]

            for method_name, probe_a_png, final_png, make_rendered, blank_png in targets:
                capture_fn = getattr(tc, method_name)
                make_rendered(final_png)

                # 1. RuntimeError on cand_b capture must still unlink probe_a_png
                make_rendered(probe_a_png)
                vm_err = mock.MagicMock()
                vm_err.capture_png.side_effect = [probe_a_png, RuntimeError("qmp fail")]
                with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
                    with self.assertRaises(RuntimeError):
                        capture_fn(vm_err, "bios", desktop_png, "diag", timeout=1.0)
                self.assertFalse(probe_a_png.exists(), f"{method_name}: probe_a leaked on capture_png RuntimeError")

                # 2. AssertionError on identical cand_a/cand_b paths must still unlink probe_a_png
                make_rendered(probe_a_png)
                vm_same_probe = mock.MagicMock()
                vm_same_probe.capture_png.side_effect = [probe_a_png, probe_a_png]
                with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
                    with self.assertRaises(AssertionError):
                        capture_fn(vm_same_probe, "bios", desktop_png, "diag", timeout=1.0)
                self.assertFalse(probe_a_png.exists(), f"{method_name}: probe_a leaked on assertNotEqual")

                # 3. OSError inside frame_diff_ratio(cand_a, cand_b) must still unlink probe_a_png
                make_rendered(probe_a_png)
                vm_diff_err = mock.MagicMock()
                vm_diff_err.capture_png.side_effect = [probe_a_png, final_png]
                with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None), mock.patch(
                    "tests.e2e.test_tier4_qemu_e2e.frame_diff_ratio",
                    side_effect=[0.5, OSError("corrupt")],
                ):
                    with self.assertRaises(OSError):
                        capture_fn(vm_diff_err, "bios", desktop_png, "diag", timeout=1.0)
                self.assertFalse(probe_a_png.exists(), f"{method_name}: probe_a leaked on frame_diff_ratio OSError")

                # 4. Poll backoff sleep in else branch must be capped to remaining deadline (<= timeout)
                vm_blank = mock.MagicMock()
                vm_blank.capture_png.return_value = blank_png
                with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", wraps=time.sleep) as sleep_spy:
                    with self.assertRaises(AssertionError):
                        capture_fn(vm_blank, "bios", desktop_png, "diag", timeout=0.25)
                self.assertTrue(sleep_spy.called, f"{method_name}: expected else-branch backoff sleep")
                for call in sleep_spy.call_args_list:
                    self.assertGreater(call.args[0], 0.0)
                    self.assertLessEqual(call.args[0], 0.25)

    _SHELL_SEPARATOR_CHARS = frozenset(";&|()")
    _NESTED_SHELL_BINS = frozenset({
        "sh", "/bin/sh", "/usr/bin/sh",
        "bash", "/bin/bash", "/usr/bin/bash",
        "dash", "/bin/dash", "/usr/bin/dash",
    })
    _NESTED_SU_RUNUSER_BINS = frozenset({
        "su", "/bin/su", "/usr/bin/su",
        "runuser", "/sbin/runuser", "/usr/sbin/runuser",
    })
    _PKILL_PGREP_BINS = frozenset({"pkill", "pgrep", "/usr/bin/pkill", "/usr/bin/pgrep"})

    @classmethod
    def _is_shell_separator(cls, tok: str) -> bool:
        return bool(tok) and all(ch in cls._SHELL_SEPARATOR_CHARS for ch in tok)

    @classmethod
    def _extract_nested_c_script(cls, cmd: str, args: list[str]) -> str | None:
        if cmd in cls._NESTED_SHELL_BINS:
            has_c = False
            skip_next = False
            end_of_opts = False
            for arg in args:
                if skip_next:
                    skip_next = False
                    continue
                if not end_of_opts and arg == "--":
                    end_of_opts = True
                    continue
                if not end_of_opts and arg in ("-o", "+o", "--rcfile", "--init-file"):
                    skip_next = True
                    continue
                if not end_of_opts and arg.startswith("-") and len(arg) > 1:
                    if not arg.startswith("--") and "c" in arg[1:]:
                        has_c = True
                    continue
                if not end_of_opts and arg.startswith("+"):
                    continue
                if has_c:
                    return arg
                return None
            return None

        if cmd in cls._NESTED_SU_RUNUSER_BINS:
            skip_next = False
            expect_cmd = False
            after_double_dash = False
            after_dd_has_c = False
            for arg in args:
                if expect_cmd:
                    return arg
                if skip_next:
                    skip_next = False
                    continue
                if after_double_dash:
                    if after_dd_has_c:
                        return arg
                    if arg.startswith("-") and not arg.startswith("--") and "c" in arg[1:]:
                        after_dd_has_c = True
                        continue
                    return None
                if arg == "--":
                    after_double_dash = True
                    continue
                if arg in ("-c", "--command", "--session-command"):
                    expect_cmd = True
                    continue
                if arg.startswith("--command="):
                    return arg.split("=", 1)[1]
                if arg.startswith("--session-command="):
                    return arg.split("=", 1)[1]
                if arg.startswith("-") and not arg.startswith("--") and "c" in arg[1:]:
                    c_pos = arg.index("c", 1)
                    if c_pos < len(arg) - 1:
                        return arg[c_pos + 1 :]
                    expect_cmd = True
                    continue
                if arg in (
                    "-u", "--user", "-g", "--group", "-G", "--supp-group",
                    "-s", "--shell", "-w", "--whitelist-environment", "-t",
                ):
                    skip_next = True
                    continue
            return None

        return None

    @classmethod
    def _find_self_matching_pkill_or_pgrep(
        cls,
        local_script: str,
        enclosing_scripts: tuple[str, ...] = (),
    ) -> tuple[list[str], int]:
        import re
        import shlex

        violations: list[str] = []
        nested_count = 0
        lex = shlex.shlex(local_script, posix=True, punctuation_chars=";&|()")
        lex.whitespace_split = True
        tokens = list(lex)
        all_cmdlines = (local_script, *enclosing_scripts)

        for idx, tok in enumerate(tokens):
            args: list[str] = []
            j = idx + 1
            while j < len(tokens) and not cls._is_shell_separator(tokens[j]):
                args.append(tokens[j])
                j += 1

            if tok in cls._NESTED_SHELL_BINS or tok in cls._NESTED_SU_RUNUSER_BINS:
                nested_script = cls._extract_nested_c_script(tok, args)
                if nested_script is not None:
                    nested_count += 1
                    sub_violations, sub_nested = cls._find_self_matching_pkill_or_pgrep(
                        nested_script,
                        all_cmdlines,
                    )
                    violations.extend(sub_violations)
                    nested_count += sub_nested
                continue

            if tok not in cls._PKILL_PGREP_BINS:
                continue

            has_f = False
            pattern = None
            skip_next = False
            end_of_opts = False
            for arg in args:
                if skip_next:
                    skip_next = False
                    continue
                if not end_of_opts and arg == "--":
                    end_of_opts = True
                    continue
                if not end_of_opts and arg in (
                    "-u", "-U", "-G", "-g", "-P", "-s", "-t", "-F", "-d",
                    "--signal", "--uid", "--euid",
                ):
                    skip_next = True
                    continue
                if not end_of_opts and arg.startswith("-"):
                    if arg == "--full" or (not arg.startswith("--") and "f" in arg[1:]):
                        has_f = True
                    continue
                pattern = arg
                break
            if not has_f:
                continue
            if tok in ("pkill", "/usr/bin/pkill"):
                violations.append(
                    f"pkill -f must use guest_exec, not run_shell: {local_script!r}"
                )
                continue
            if pattern is None:
                violations.append("pgrep -f missing static pattern in run_shell")
                continue
            for cmdline in all_cmdlines:
                if re.search(pattern, cmdline) is not None:
                    violations.append(
                        f"pgrep -f pattern {pattern!r} self-matches cmdline {cmdline!r}"
                    )
                    break

        return violations, nested_count

    def test_e2e_run_shell_scripts_have_no_self_matching_pkill_or_pgrep(self):
        import ast

        def _extract_script(node: ast.AST) -> str | None:
            if isinstance(node, ast.Constant) and isinstance(node.value, str):
                return node.value
            if isinstance(node, ast.JoinedStr):
                parts = []
                for v in node.values:
                    if isinstance(v, ast.Constant) and isinstance(v.value, str):
                        parts.append(v.value)
                    else:
                        parts.append("ARG")
                return "".join(parts)
            if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Add):
                left = _extract_script(node.left)
                right = _extract_script(node.right)
                if left is not None and right is not None:
                    return left + right
            return None

        e2e_dir = Path(__file__).resolve().parent
        checked_calls = 0
        nested_calls = 0
        for pyfile in sorted(e2e_dir.rglob("*.py")):
            tree = ast.parse(pyfile.read_text(encoding="utf-8"), filename=str(pyfile))
            for node in ast.walk(tree):
                if not (
                    isinstance(node, ast.Call)
                    and isinstance(node.func, ast.Attribute)
                    and node.func.attr == "run_shell"
                ):
                    continue
                arg_node = node.args[0] if node.args else None
                if arg_node is None:
                    arg_node = next((kw.value for kw in node.keywords if kw.arg == "script"), None)
                if arg_node is None:
                    continue
                script = _extract_script(arg_node)
                if script is None:
                    continue
                checked_calls += 1
                violations, sub_nested = self._find_self_matching_pkill_or_pgrep(script)
                nested_calls += sub_nested
                self.assertEqual(
                    violations,
                    [],
                    f"{pyfile.name}:{node.lineno}: {violations}",
                )
        self.assertGreaterEqual(checked_calls, 20)
        self.assertGreaterEqual(nested_calls, 5)

    def test_nested_shell_pkill_pgrep_scanner_catches_nested_wrappers(self):
        bad_cases = [
            'runuser -u liveuser -- bash -c "pkill -f foo"',
            "niri msg action spawn -- /bin/sh -c 'pkill -f bar'",
            'su - liveuser -c "pkill --full baz"',
            'su -pc "pkill -f baz2"',
            'runuser -u liveuser -c "pkill -f qux"',
            'runuser -u liveuser -- -c "pkill -f quux"',
            'runuser -u liveuser -- bash -lc "sh -c \'pkill -f deep\'"',
            'out=$(bash -c "pkill -f sub")',
            'runuser -u liveuser -- bash -c "pgrep -f \'[r]unuser\'"',
            'runuser -u liveuser -- bash -c "pgrep -f my_proc"',
        ]
        for script in bad_cases:
            violations, nested_count = self._find_self_matching_pkill_or_pgrep(script)
            self.assertGreaterEqual(nested_count, 1, f"Expected nested script in {script!r}")
            self.assertTrue(violations, f"Expected violation in {script!r}")

        safe_cases = [
            'runuser -u liveuser -- bash -c "pgrep -u liveuser -f \'/[o]pt/FireHub/firehub\'"',
            'echo "bash -c \'pkill -f safe\'"',
            'bash /tmp/helper.sh -c "pkill -f noop"',
        ]
        for script in safe_cases:
            violations, _ = self._find_self_matching_pkill_or_pgrep(script)
            self.assertEqual(violations, [], f"Unexpected violation in {script!r}: {violations}")

    @staticmethod
    def _write_minimal_elf(path: Path, *, ei_class: int, e_machine: int) -> None:
        hdr = bytearray(64)
        hdr[0:4] = b"\x7fELF"
        hdr[4] = ei_class
        hdr[5] = 1  # ELFDATA2LSB
        hdr[18:20] = e_machine.to_bytes(2, "little")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(bytes(hdr))

    def test_tier1_collect_direct_so_names_confines_symlinks_and_checks_elf_arch(self):
        from tests.e2e.test_tier1_features import (
            ELFCLASS32,
            ELFCLASS64,
            EM_386,
            EM_X86_64,
            _collect_direct_so_names,
        )

        with tempfile.TemporaryDirectory(prefix="blaze_elf_u_") as tmp:
            tmp_path = Path(tmp)
            outside_elf = tmp_path / "host_only.so"
            self._write_minimal_elf(outside_elf, ei_class=ELFCLASS64, e_machine=EM_X86_64)

            ext_root = tmp_path / "ext_root"
            lib64 = ext_root / "usr/lib64"
            lib32 = ext_root / "usr/lib"
            lib64.mkdir(parents=True)
            lib32.mkdir(parents=True)

            # 1. Valid 64-bit ELF directly in usr/lib64
            self._write_minimal_elf(lib64 / "libdirect.so.1", ei_class=ELFCLASS64, e_machine=EM_X86_64)

            # 2. Chained absolute symlinks confined inside ext_root (mimics libnssckbi.so)
            self._write_minimal_elf(
                lib64 / "pkcs11/p11-kit-trust.so", ei_class=ELFCLASS64, e_machine=EM_X86_64
            )
            (ext_root / "etc/alternatives").mkdir(parents=True)
            (ext_root / "etc/alternatives/libnssckbi.so.x86_64").symlink_to(
                "/usr/lib64/pkcs11/p11-kit-trust.so"
            )
            (lib64 / "libnssckbi.so").symlink_to("/etc/alternatives/libnssckbi.so.x86_64")

            # 3. Absolute symlink to existing HOST file outside ext_root -> MUST NOT resolve
            (lib64 / "libhostabs.so").symlink_to(str(outside_elf))

            # 4. Relative symlink escaping ext_root -> MUST NOT resolve
            (lib64 / "libescape.so").symlink_to("../../../host_only.so")

            # 5. Symlink loop -> MUST terminate and NOT resolve
            (lib64 / "libloop_a.so").symlink_to("libloop_b.so")
            (lib64 / "libloop_b.so").symlink_to("libloop_a.so")

            # 6. Non-ELF .hmac file matching "*.so*" -> MUST be ignored
            (lib64 / ".libnettle.so.9.hmac").write_text("deadbeef\n", encoding="utf-8")

            # 7. Wrong-arch (32-bit) ELF in usr/lib64 + 32-bit ELF in usr/lib + misplaced 64-bit ELF in usr/lib
            self._write_minimal_elf(lib64 / "libwrong32.so", ei_class=ELFCLASS32, e_machine=EM_386)
            self._write_minimal_elf(lib32 / "libonly32.so", ei_class=ELFCLASS32, e_machine=EM_386)
            self._write_minimal_elf(lib32 / "libmisplaced64.so", ei_class=ELFCLASS64, e_machine=EM_X86_64)

            libs_64 = _collect_direct_so_names(
                [lib64], ext_root=ext_root, expected_arch=(ELFCLASS64, EM_X86_64)
            )
            self.assertEqual(libs_64, {"libdirect.so.1", "libnssckbi.so"})

            libs_32 = _collect_direct_so_names(
                [lib32], ext_root=ext_root, expected_arch=(ELFCLASS32, EM_386)
            )
            self.assertEqual(libs_32, {"libonly32.so"})


if __name__ == "__main__":
    unittest.main()
