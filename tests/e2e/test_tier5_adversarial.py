"""Tier 5: White-box adversarial behavioral, harness, and contract tests (isolated /tmp sandboxes only)."""
import hashlib
import json
import os
import re
import shutil
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import unittest
from pathlib import Path
from unittest import mock

from PIL import Image, ImageDraw

from tests.e2e.config import OVERLAY_DIR, REPO_ROOT
from tests.e2e.harness.imaging import (
    DEFAULT_ANACONDA_UI_REGIONS,
    analyze_frame,
    convert_ppm_to_png,
    frame_diff_ratio,
    is_anaconda_webui_rendered,
    is_firehub_rendered,
)
from tests.e2e.harness.iso_inspect import checkisomd5_path, verify_iso_sha256_file
from tests.e2e.harness.qemu_vm import QemuVM
from tests.e2e.harness.qga import GuestExecResult, QgaClient, QgaError
from tests.e2e.harness.qmp import QmpClient, QmpError
from tests.e2e import test_harness_unit, test_tier4_qemu_e2e

RUN_E2E = os.environ.get("BLAZE_RUN_E2E") == "1"


def _make_stub(bindir: Path, name: str, body: str) -> Path:
    p = bindir / name
    p.write_text("#!/bin/sh\n" + body.lstrip("\n"), encoding="utf-8")
    p.chmod(0o755)
    return p


@unittest.skipUnless(
    RUN_E2E,
    "Set BLAZE_RUN_E2E=1 or run `python3 -m tests.e2e.run_e2e --tier 5`",
)
class Tier5ShellAndSystemAdversarialTests(unittest.TestCase):
    """Behavioral, negative-path, locale, and packaging tests for BlazeOS shell/config layers."""

    def test_all_overlay_and_packaging_scripts_syntax_valid(self):
        checked = 0
        for base in (
            OVERLAY_DIR / "usr/bin",
            OVERLAY_DIR / "usr/libexec",
            OVERLAY_DIR / "usr/libexec/livesys/sessions.d",
            OVERLAY_DIR / "etc/grub.d",
            REPO_ROOT / "packaging",
        ):
            if not base.is_dir():
                continue
            for path in sorted(base.iterdir()):
                if not path.is_file() or path.is_symlink():
                    continue
                with open(path, "rb") as f:
                    first = f.readline(256)
                if not first.startswith(b"#!"):
                    continue
                shebang = first.decode("utf-8", errors="replace")
                if "bash" in shebang:
                    res = subprocess.run(["bash", "-n", str(path)], capture_output=True, text=True)
                    self.assertEqual(res.returncode, 0, f"bash -n failed for {path}: {res.stderr}")
                    checked += 1
                elif "/sh" in shebang:
                    res = subprocess.run(["sh", "-n", str(path)], capture_output=True, text=True)
                    self.assertEqual(res.returncode, 0, f"sh -n failed for {path}: {res.stderr}")
                    checked += 1
                elif "python" in shebang:
                    compile(path.read_text(encoding="utf-8"), str(path), "exec")
                    checked += 1
        self.assertGreaterEqual(checked, 30)

    def test_firehub_wrapper_branches_and_arg_forwarding(self):
        src = (OVERLAY_DIR / "usr/bin/firehub").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_fh_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            fake_bin = t / "firehub-bin"
            log = t / "args.json"
            fake_bin.write_text(
                f'#!/usr/bin/env python3\nimport json, os, sys\nopen({str(log)!r}, "w").write(json.dumps({{"argv": sys.argv[1:], "xdg": os.environ.get("XDG_SESSION_TYPE", "")}}))\n',
                encoding="utf-8",
            )
            fake_bin.chmod(0o755)
            _make_stub(bindir, "systemd-detect-virt", "exit 1\n")
            _make_stub(bindir, "id", "echo 1000\n")

            patched = src.replace('FIREHUB_BIN="/opt/FireHub/firehub"', f'FIREHUB_BIN="{fake_bin}"')
            patched = patched.replace("/dev/dri/renderD128", str(t / "renderD128"))
            (t / "renderD128").touch()

            r_missing = subprocess.run(
                ["bash", "-c", src.replace("/opt/FireHub/firehub", str(t / "missing_firehub"))],
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_missing.returncode, 1)
            self.assertIn("bulunamadı", r_missing.stderr)

            subprocess.run(
                ["bash", "-c", patched, "firehub", "--url", "https://example.com/a b"],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "WAYLAND_DISPLAY": "wayland-1"},
                check=True,
            )
            data = json.loads(log.read_text(encoding="utf-8"))
            self.assertEqual(data["xdg"], "wayland")
            self.assertIn("--ozone-platform=wayland", data["argv"])
            self.assertIn("--enable-features=WaylandWindowDecorations", data["argv"])
            self.assertEqual(data["argv"][-2:], ["--url", "https://example.com/a b"])
            self.assertNotIn("--no-sandbox", data["argv"])

            subprocess.run(
                ["bash", "-c", patched, "firehub"],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "XDG_SESSION_TYPE": "wayland"},
                check=True,
            )
            data = json.loads(log.read_text(encoding="utf-8"))
            self.assertIn("--ozone-platform-hint=auto", data["argv"])

            _make_stub(bindir, "id", "echo 0\n")
            (t / "renderD128").unlink()
            subprocess.run(
                ["bash", "-c", patched, "firehub"],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "XDG_SESSION_TYPE": "x11"},
                check=True,
            )
            data = json.loads(log.read_text(encoding="utf-8"))
            self.assertIn("--disable-gpu-sandbox", data["argv"])
            self.assertIn("--no-sandbox", data["argv"])
            self.assertNotIn("--ozone-platform=wayland", data["argv"])

    def test_quickshell_wrapper_exports_env_and_forwards_args(self):
        src = (OVERLAY_DIR / "usr/bin/quickshell").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_qs_") as tmp:
            t = Path(tmp)
            fake_qs = t / "quickshell.bin"
            out = t / "env.json"
            fake_qs.write_text(
                f'#!/usr/bin/env python3\nimport json, os, sys\nopen({str(out)!r}, "w").write(json.dumps({{"argv": sys.argv[1:], "env": dict(os.environ)}}))\n',
                encoding="utf-8",
            )
            fake_qs.chmod(0o755)
            patched = src.replace("/usr/bin/quickshell.bin", str(fake_qs))
            subprocess.run(
                ["sh", "-c", patched, "quickshell", "-p", "/tmp/a b/shell.qml"],
                env={"PATH": "/usr/bin:/bin"},
                check=True,
            )
            d = json.loads(out.read_text(encoding="utf-8"))
            self.assertEqual(d["argv"], ["-p", "/tmp/a b/shell.qml"])
            self.assertEqual(d["env"]["QT_QPA_PLATFORM"], "wayland;xcb")
            self.assertEqual(d["env"]["QSG_RENDER_LOOP"], "threaded")
            self.assertIn("/usr/lib64/quickshell", d["env"]["LD_LIBRARY_PATH"])
            self.assertIn("/usr/lib64/quickshell/plugins", d["env"]["QT_PLUGIN_PATH"])

    def test_solar_lock_live_suppression_and_fallbacks(self):
        src = (OVERLAY_DIR / "usr/bin/solar-lock").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_lock_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            calls = t / "calls.log"
            _make_stub(bindir, "notify-send", f'echo "notify $@" >> "{calls}"\n')
            _make_stub(bindir, "pgrep", "exit 1\n")
            _make_stub(bindir, "swaylock", f'echo "swaylock $@" >> "{calls}"\n')
            _make_stub(bindir, "loginctl", f'echo "loginctl $@" >> "{calls}"\n')

            live_cmd = t / "cmdline"
            live_cmd.write_text("quiet rd.live.image\n", encoding="utf-8")
            patched = (
                src.replace("/proc/cmdline", str(live_cmd))
                .replace("/run/initramfs/live", str(t / "no_live"))
                .replace("/run/live/medium", str(t / "no_med"))
            )
            r_live = subprocess.run(
                ["bash", "-c", patched],
                env={"PATH": f"{bindir}:/usr/bin:/bin"},
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_live.returncode, 0)
            self.assertIn("notify", calls.read_text(encoding="utf-8"))
            self.assertNotIn("swaylock", calls.read_text(encoding="utf-8"))

            calls.unlink()
            live_cmd.write_text("quiet root=UUID=abc\n", encoding="utf-8")
            r_sway = subprocess.run(
                ["bash", "-c", patched],
                env={"PATH": f"{bindir}:/usr/bin:/bin"},
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_sway.returncode, 0)
            self.assertIn("swaylock -f -c 1a1b26", calls.read_text(encoding="utf-8"))

    def test_solar_session_wrapper_fallback_on_failure(self):
        src = (OVERLAY_DIR / "usr/bin/solar-session-wrapper").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_ssw_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            (t / "home").mkdir()
            runs = t / "runs.log"
            _make_stub(bindir, "systemd-detect-virt", "exit 0\n")
            _make_stub(bindir, "loginctl", "exit 0\n")
            _make_stub(bindir, "systemctl", "exit 0\n")
            _make_stub(bindir, "dbus-update-activation-environment", "exit 0\n")
            _make_stub(bindir, "sleep", "exit 0\n")
            sess = t / "solar-session"
            sess.write_text(
                f'#!/bin/sh\necho "WLR=${{WLR_RENDERER:-}} SW=${{LIBGL_ALWAYS_SOFTWARE:-}} LOOP=${{QSG_RENDER_LOOP:-}}" >> "{runs}"\nif [ -z "${{WLR_RENDERER:-}}" ]; then exit 1; fi\nexit 0\n',
                encoding="utf-8",
            )
            sess.chmod(0o755)
            patched = (
                src.replace("/usr/bin/solar-session", str(sess))
                .replace("/proc/cmdline", str(t / "no_cmd"))
                .replace("/run/initramfs/live", str(t / "no_live"))
            )
            r = subprocess.run(
                ["bash", "-c", patched],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "HOME": str(t / "home"), "XDG_RUNTIME_DIR": str(t)},
                capture_output=True,
                text=True,
            )
            self.assertEqual(r.returncode, 0, r.stderr)
            lines = runs.read_text(encoding="utf-8").splitlines()
            self.assertEqual(lines, ["WLR= SW= LOOP=basic", "WLR=pixman SW=1 LOOP=basic"])

    def test_gen_grub_cfgstub_uuid_and_portable_fallback(self):
        src = (OVERLAY_DIR / "usr/bin/gen_grub_cfgstub").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_grub_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            grub_home = t / "boot/grub2"
            efi_home = t / "boot/efi/EFI/fedora"

            r_arg = subprocess.run([str(OVERLAY_DIR / "usr/bin/gen_grub_cfgstub")], capture_output=True, text=True)
            self.assertEqual(r_arg.returncode, 1)

            _make_stub(bindir, "grub2-probe", "echo dead-beef\n")
            _make_stub(bindir, "grub2-mkrelpath", "echo /grub2\n")
            subprocess.run(
                ["sh", "-c", src, "gen_grub_cfgstub", str(grub_home), str(efi_home)],
                env={"PATH": f"{bindir}:/usr/bin:/bin"},
                check=True,
            )
            cfg = (efi_home / "grub.cfg").read_text(encoding="utf-8")
            self.assertIn("--fs-uuid --set=dev dead-beef", cfg)
            self.assertEqual(stat.S_IMODE((efi_home / "grub.cfg").stat().st_mode), 0o600)

            _make_stub(bindir, "grub2-probe", "exit 1\n")
            patched = src.replace(
                "/usr/share/blazeos/grub-efi-stub.cfg",
                str(OVERLAY_DIR / "usr/share/blazeos/grub-efi-stub.cfg"),
            )
            subprocess.run(
                ["sh", "-c", patched, "gen_grub_cfgstub", str(grub_home), str(efi_home)],
                env={"PATH": f"{bindir}:/usr/bin:/bin"},
                check=True,
                capture_output=True,
            )
            cfg2 = (efi_home / "grub.cfg").read_text(encoding="utf-8")
            self.assertIn("search --no-floppy --file --set=dev /grub2/grub.cfg", cfg2)
            self.assertEqual(list(efi_home.glob("grub.cfg.stb.*")), [])

    def test_blazeos_oobe_hygiene_sysroot_behavior(self):
        script = str(OVERLAY_DIR / "usr/libexec/blazeos-oobe-hygiene")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_hyg_") as tmp:
            t = Path(tmp)
            for d in ("etc/sddm.conf.d", "usr/share/wayland-sessions", "var/lib/AccountsService/users", "etc/sudoers.d"):
                (t / d).mkdir(parents=True)
            oobe_conf = t / "etc/sddm.conf.d/00-blaze-oobe.conf"
            auto_conf = t / "etc/sddm.conf.d/autologin.conf"
            oobe_desk = t / "usr/share/wayland-sessions/blaze-oobe.desktop"
            acct = t / "var/lib/AccountsService/users/blaze-setup"
            sudoers = t / "etc/sudoers.d/99-blaze-setup"
            for f in (oobe_conf, oobe_desk, acct, sudoers):
                f.write_text("x\n", encoding="utf-8")
            auto_conf.write_text("[Autologin]\nUser=alice\n", encoding="utf-8")

            subprocess.run([script], env={**os.environ, "BLAZEOS_SYSROOT": str(t)}, check=True)
            self.assertTrue(oobe_conf.exists())
            self.assertTrue(auto_conf.exists())

            (t / "etc/blazeos-setup-completed").write_text("username=alice\n", encoding="utf-8")
            subprocess.run([script], env={**os.environ, "BLAZEOS_SYSROOT": str(t)}, check=True)
            for f in (oobe_conf, oobe_desk, acct, sudoers):
                self.assertFalse(f.exists())
            self.assertTrue(auto_conf.exists())

            auto_conf.write_text("[Autologin]\nUser=blaze-setup\n", encoding="utf-8")
            subprocess.run([script], env={**os.environ, "BLAZEOS_SYSROOT": str(t)}, check=True)
            self.assertFalse(auto_conf.exists())

    def test_blazeos_remove_solarui_guards_and_recovery_retention(self):
        src = (OVERLAY_DIR / "usr/bin/blazeos-remove-solarui").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_rms_") as tmp:
            t = Path(tmp)
            for d in (
                "etc", "usr/bin", "usr/share/wayland-sessions", "usr/share/xsessions",
                "usr/share/applications", "usr/lib/systemd/user", "usr/share/quickshell/end4-pC",
                "usr/share/caelestia", "etc/solarui", "etc/skel/.config/solarui",
                "etc/skel/.config/noctalia", "etc/skel/.config/caelestia", "etc/skel/.config/quickshell/caelestia",
            ):
                (t / d).mkdir(parents=True)
            (t / "usr/bin/blaze-recovery").write_text("rec", encoding="utf-8")
            (t / "usr/bin/solar-shell").write_text("sh", encoding="utf-8")
            (t / "usr/share/wayland-sessions/gnome.desktop").write_text("[Desktop Entry]\n", encoding="utf-8")

            patched = re.sub(r"(?<![A-Za-z0-9_./-])/(etc|usr)/", rf"{t}/\1/", src)

            self.assertEqual(subprocess.run(["bash", "-c", patched], capture_output=True, text=True).returncode, 1)

            (t / "etc/blazeos-selected-session").write_text("solarui.desktop\n", encoding="utf-8")
            self.assertEqual(subprocess.run(["bash", "-c", patched], capture_output=True, text=True).returncode, 1)

            (t / "etc/blazeos-selected-session").write_text("plasma.desktop\n", encoding="utf-8")
            self.assertEqual(subprocess.run(["bash", "-c", patched], capture_output=True, text=True).returncode, 1)

            (t / "etc/blazeos-selected-session").write_text("gnome.desktop\n", encoding="utf-8")
            r4 = subprocess.run(["bash", "-c", patched], capture_output=True, text=True)
            self.assertEqual(r4.returncode, 0, r4.stderr)
            self.assertFalse((t / "usr/bin/solar-shell").exists())
            self.assertTrue((t / "usr/bin/blaze-recovery").exists())

    def test_blazeos_apply_install_choices_bios_limine_fallback_and_queueing(self):
        src = (OVERLAY_DIR / "usr/bin/blazeos-apply-install-choices").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_aic_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            for d in ("var/lib/blazeos", "var/log", "etc", "boot/grub2", "usr/share/wayland-sessions", "usr/share/xsessions"):
                (t / d).mkdir(parents=True)
            (t / "usr/share/wayland-sessions/solarui.desktop").write_text("[Desktop Entry]\n", encoding="utf-8")
            (t / "etc/blazeos-desktop-choice").write_text("gnome\n", encoding="utf-8")
            (t / "etc/blazeos-bootloader-choice").write_text("limine\n", encoding="utf-8")
            (t / "etc/blazeos-desktop-profile").write_text("full\n", encoding="utf-8")
            (t / "etc/blazeos-usage-profile").write_text("gaming\n", encoding="utf-8")
            (t / "etc/blazeos-remove-solarui").write_text("no\n", encoding="utf-8")
            _make_stub(bindir, "grub2-mkconfig", "exit 0\n")

            patched = (
                src.replace("/var/log/blazeos-install-choices.log", str(t / "var/log/install.log"))
                .replace("/var/lib/blazeos", str(t / "var/lib/blazeos"))
                .replace("/proc/cmdline", str(t / "no_cmd"))
                .replace("/run/initramfs/live", str(t / "no_live"))
                .replace("/run/liveuser", str(t / "no_liveuser"))
                .replace("/etc/blazeos-", str(t / "etc/blazeos-"))
                .replace("/usr/share/wayland-sessions", str(t / "usr/share/wayland-sessions"))
                .replace("/usr/share/xsessions", str(t / "usr/share/xsessions"))
                .replace("/sys/firmware/efi", str(t / "no_efi"))
                .replace("/boot/grub2", str(t / "boot/grub2"))
            )
            r = subprocess.run(["bash", "-c", patched], env={"PATH": f"{bindir}:/usr/bin:/bin"}, capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertEqual((t / "etc/blazeos-selected-session").read_text().strip(), "solarui.desktop")
            self.assertEqual((t / "etc/blazeos-bootloader-choice").read_text().strip(), "grub2")
            pending = (t / "var/lib/blazeos/desktop-provision-pending").read_text()
            self.assertIn("desktop=gnome", pending)
            self.assertIn("usage_profile=gaming", pending)
            self.assertTrue((t / "var/lib/blazeos/install-choices-applied").is_file())

    def test_blazeos_postinstall_post_oobe_human_user_sync(self):
        src = (OVERLAY_DIR / "usr/bin/blazeos-postinstall").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_pi_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            for d in (
                "var/log", "var/lib/blaze-setup", "var/lib/AccountsService/users",
                "etc/sddm.conf.d", "etc/sudoers.d", "etc/systemd/system/graphical.target.wants",
                "etc/systemd/system/multi-user.target.wants", "etc/skel/Desktop",
                "etc/skel/.config/noctalia", "etc/solarui", "etc/gdm",
                "usr/share/applications", "usr/share/wayland-sessions", "usr/share/xsessions",
                "boot/grub2", "home/alice/.config",
            ):
                (t / d).mkdir(parents=True)
            (t / "etc/sddm.conf.d/autologin.conf").write_text("[Autologin]\nUser=liveuser\n", encoding="utf-8")
            (t / "usr/share/applications/liveinst.desktop").write_text("[Desktop Entry]\n", encoding="utf-8")
            (t / "etc/passwd").write_text("root:x:0:0:root:/root:/bin/bash\nalice:x:1000:1000::/home/alice:/bin/bash\n", encoding="utf-8")
            (t / "etc/hostname").write_text("localhost\n", encoding="utf-8")
            (t / "etc/hosts").write_text("127.0.0.1 localhost\n", encoding="utf-8")
            (t / "etc/blazeos-setup-completed").write_text("username=alice\n", encoding="utf-8")
            (t / "etc/blazeos-selected-session").write_text("gnome.desktop\n", encoding="utf-8")
            (t / "usr/share/wayland-sessions/gnome.desktop").write_text("[Desktop Entry]\n", encoding="utf-8")
            for c in ("systemctl", "id", "usermod", "groupmod", "userdel", "passwd", "hostnamectl", "gpasswd", "restorecon", "grub2-mkconfig", "getent", "chown"):
                _make_stub(bindir, c, "exit 0\n")

            patched = (
                src.replace("/proc/cmdline", str(t / "no_cmd"))
                .replace("/run/initramfs/live", str(t / "no_live"))
                .replace("/run/liveuser", str(t / "no_lu"))
                .replace("/var/log/blazeos-postinstall.log", str(t / "var/log/post.log"))
                .replace("/sys/firmware/efi", str(t / "no_efi"))
                .replace("chmod 4755 /usr/bin/newuidmap", "true")
            )
            patched = re.sub(r"(?<![A-Za-z0-9_./-])/(etc|var/lib|usr/share|boot/grub2)/", rf"{t}/\1/", patched)
            patched = patched.replace(" /home ", f" {t}/home ")

            r = subprocess.run(["bash", "-c", patched], env={"PATH": f"{bindir}:/usr/bin:/bin"}, capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertFalse((t / "usr/share/applications/liveinst.desktop").exists())
            self.assertFalse((t / "etc/sddm.conf.d/autologin.conf").exists())
            self.assertEqual((t / "etc/hostname").read_text().strip(), "blazeos")
            acct = (t / "var/lib/AccountsService/users/alice").read_text()
            self.assertIn("Session=gnome", acct)
            self.assertIn("XSession=gnome", acct)
            self.assertIn("SessionType=wayland", acct)

    def test_livesys_solarui_sandbox_outputs(self):
        src = (OVERLAY_DIR / "usr/libexec/livesys/sessions.d/livesys-solarui").read_text(encoding="utf-8")
        core = src.split("# Ensure networking is active", 1)[0]
        with tempfile.TemporaryDirectory(prefix="blaze_t5_lv_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            for d in (
                "etc/sddm.conf.d", "etc/gdm", "etc/sysconfig", "etc/solarui",
                "etc/skel/.config/noctalia", "etc/skel/Desktop", "etc/skel/.local",
                "var/lib/AccountsService/users", "usr/share/applications",
                "usr/share/polkit-1/rules.d", "usr/share/xdg-desktop-portal",
                "home/liveuser", "tmp",
            ):
                (t / d).mkdir(parents=True)
            (t / "etc/sddm.conf.d/00-blaze-oobe.conf").write_text("[Autologin]\nUser=blaze-setup\n", encoding="utf-8")
            (t / "usr/share/applications/liveinst.desktop").write_text("[Desktop Entry]\nNoDisplay=true\n", encoding="utf-8")
            for c in ("restorecon", "systemctl", "usermod", "chown"):
                _make_stub(bindir, c, "exit 0\n")

            patched = core.replace("~liveuser", str(t / "home/liveuser"))
            patched = re.sub(r"(?<![A-Za-z0-9_./-])/(etc|var/lib|usr/share|tmp)/", rf"{t}/\1/", patched)

            r = subprocess.run(["sh", "-c", patched], env={"PATH": f"{bindir}:/usr/bin:/bin"}, capture_output=True, text=True)
            self.assertEqual(r.returncode, 0, r.stderr)
            self.assertFalse((t / "etc/sddm.conf.d/00-blaze-oobe.conf").exists())
            auto = (t / "etc/sddm.conf.d/autologin.conf").read_text()
            self.assertIn("User=liveuser", auto)
            self.assertIn("Session=solarui.desktop", auto)
            rules = (t / "usr/share/polkit-1/rules.d/20-livesys-solarui.rules").read_text()
            self.assertIn("subject.local", rules)
            self.assertIn("'liveuser'", rules)
            self.assertIn("'blaze'", rules)
            self.assertIn("AnacondaInstaller", rules)
            self.assertEqual((t / "tmp/anaconda_desktop_choice").read_text().strip(), "solarui")
            self.assertEqual(stat.S_IMODE((t / "tmp/anaconda_desktop_choice").stat().st_mode), 0o666)
            self.assertIn("NoDisplay=false", (t / "usr/share/applications/liveinst.desktop").read_text())

    def test_blazeos_welcome_helper_validation_and_dock_exclusion(self):
        src = (OVERLAY_DIR / "usr/libexec/blazeos-welcome-helper").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_wh_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            _make_stub(bindir, "id", "echo 1000\n")
            _make_stub(bindir, "systemctl", "exit 0\n")
            _make_stub(bindir, "dnf", "exit 0\n")
            gsettings_log = t / "gsettings.log"
            _make_stub(
                bindir,
                "sudo",
                f'while [ $# -gt 0 ]; do case "$1" in -u) shift 2;; *=*) shift;; *) break;; esac; done\nif [ "$1" = gsettings ] && [ "$2" = get ]; then echo "[\'dash-to-panel@jderose9.github.com\', \'user-theme@example\']"; elif [ "$1" = gsettings ] && [ "$2" = set ]; then echo "$5" > "{gsettings_log}"; fi\n',
            )
            self.assertEqual(
                subprocess.run(["bash", "-c", src, "helper", "update"], env={"PATH": f"{bindir}:/usr/bin:/bin"}, capture_output=True).returncode,
                1,
            )
            _make_stub(bindir, "id", "echo 0\n")
            self.assertEqual(
                subprocess.run(["bash", "-c", src, "helper", "bogus"], env={"PATH": f"{bindir}:/usr/bin:/bin"}, capture_output=True).returncode,
                64,
            )
            for bad_pkg in ("-y", "../etc", "foo;rm", "pkg/bar"):
                self.assertEqual(
                    subprocess.run(
                        ["bash", "-c", src, "helper", "install-de", bad_pkg],
                        env={"PATH": f"{bindir}:/usr/bin:/bin", "LC_ALL": "C.UTF-8"},
                        capture_output=True,
                    ).returncode,
                    2,
                )
            r_dock = subprocess.run(
                ["bash", "-c", src, "helper", "set-dock", "dash-to-dock"],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "PKEXEC_UID": "1000"},
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_dock.returncode, 0, r_dock.stderr)
            res_ext = gsettings_log.read_text(encoding="utf-8").strip()
            self.assertIn("dash-to-dock@micxgx.gmail.com", res_ext)
            self.assertNotIn("dash-to-panel@jderose9.github.com", res_ext)
            self.assertIn("user-theme@example", res_ext)

    def test_blaze_optimize_pam_ordering_and_packaging_specs(self):
        self.assertEqual(subprocess.run([str(OVERLAY_DIR / "usr/bin/blaze-optimize"), "status"], capture_output=True).returncode, 0)
        self.assertEqual(subprocess.run([str(OVERLAY_DIR / "usr/bin/blaze-optimize"), "--bogus"], capture_output=True).returncode, 1)

        pam = (OVERLAY_DIR / "etc/pam.d/sddm-autologin").read_text(encoding="utf-8")
        session_lines = [
            " ".join(l.split())
            for l in pam.splitlines()
            if l.strip() and not l.strip().startswith("#") and l.strip().lstrip("-").startswith("session")
        ]
        idx_close = next(i for i, l in enumerate(session_lines) if "pam_selinux.so close" in l)
        idx_loginuid = next(i for i, l in enumerate(session_lines) if "pam_loginuid.so" in l)
        idx_open = next(i for i, l in enumerate(session_lines) if "pam_selinux.so open" in l)
        self.assertLess(idx_close, idx_loginuid)
        self.assertLess(idx_loginuid, idx_open)

        xul = json.loads((OVERLAY_DIR / "usr/share/anaconda/firefox-theme/live/xulstore.json").read_text(encoding="utf-8"))
        win = xul["chrome://browser/content/browser.xhtml"]["main-window"]
        self.assertEqual((win["width"], win["height"], win["sizemode"]), ("1050", "656", "normal"))

        pkgs = json.loads((REPO_ROOT / "packaging/packages.json").read_text(encoding="utf-8"))
        for pkg in pkgs:
            spec_txt = (REPO_ROOT / "packaging/specs" / f"{pkg['name']}.spec").read_text(encoding="utf-8")
            self.assertIn(f"Version:        {pkg['ver']}", spec_txt)
            for fpath in pkg["files"]:
                self.assertIn(fpath, spec_txt)

        verify_sh = (REPO_ROOT / "packaging/verify-repo.sh").read_text(encoding="utf-8")
        self.assertIn("fedora:45", verify_sh)
        for pkg_name in ("blaze-house", "solarui", "blaze-oobe"):
            self.assertIn(pkg_name, verify_sh)
        sentinel_spec = (REPO_ROOT / "packaging/specs/blaze-sentinel.spec").read_text(encoding="utf-8")
        self.assertIn("0.3.1-1", sentinel_spec)

    def test_bug_desktop_provision_solarui_pure_nonstandard_and_loginctl_uid_column(self):
        src = (OVERLAY_DIR / "usr/bin/blazeos-desktop-provision").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_bug_dp_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            for d in ("state", "ws", "xs"):
                (t / d).mkdir()
            (t / "ws/solarui.desktop").write_text("[Desktop Entry]\n", encoding="utf-8")
            (t / "state/desktop-provision-pending").write_text(
                "desktop=solarui_pure\nprofile=minimal\nusage_profile=gaming\nremove_solarui=no\n",
                encoding="utf-8",
            )
            (t / "setup_done").write_text("username=alice\n", encoding="utf-8")
            (t / "selected").write_text("solarui.desktop\n", encoding="utf-8")
            (t / "sddm_state").write_text("[Last]\nUser=alice\n", encoding="utf-8")
            for c, body in [
                ("dnf5", "exit 0\n"),
                ("nm-online", "exit 0\n"),
                ("ping", "exit 0\n"),
                ("getent", "exit 0\n"),
                ("logger", "exit 0\n"),
                ("loginctl", 'echo "2 1000 alice seat0 1972 user tty1 no -"\n'),
                ("systemctl", f'echo "systemctl $@" >> "{t}/sc.log"; exit 0\n'),
                ("su", f'echo "su $@" >> "{t}/su.log"; exit 0\n'),
                ("id", 'echo "alice"\n'),
            ]:
                _make_stub(bindir, c, body)

            patched = (
                src.replace("/var/log/blazeos-desktop-provision.log", str(t / "prov.log"))
                .replace("/var/lib/blazeos", str(t / "state"))
                .replace("/usr/share/wayland-sessions", str(t / "ws"))
                .replace("/usr/share/xsessions", str(t / "xs"))
                .replace("/etc/blazeos-selected-session", str(t / "selected"))
                .replace("/etc/blazeos-setup-completed", str(t / "setup_done"))
                .replace("/var/lib/sddm/state.conf", str(t / "sddm_state"))
                .replace("/var/lib/sddm", str(t / "sddm_dir"))
                .replace("/var/lib/AccountsService/users", str(t / "no_acct"))
                .replace("/etc/sddm.conf.d/autologin.conf", str(t / "no_auto"))
            )
            r = subprocess.run(
                ["bash", "-c", patched],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "BLAZEOS_FORCE_PROVISION": "1"},
                capture_output=True,
                text=True,
            )
            self.assertEqual(r.returncode, 0, (t / "prov.log").read_text(encoding="utf-8"))
            self.assertIn("unix:path=/run/user/1000/bus", (t / "su.log").read_text(encoding="utf-8"))

            (t / "state/desktop-provision-pending").write_text(
                "desktop=solarui_gnome\nprofile=minimal\nusage_profile=standard\nremove_solarui=no\n",
                encoding="utf-8",
            )
            (t / "sc.log").unlink(missing_ok=True)
            patched_no_live = patched.replace("[ -d /run/initramfs/live ]", "false").replace("[ -f /run/liveuser ]", "false")
            subprocess.run(["bash", "-c", patched_no_live], env={"PATH": f"{bindir}:/usr/bin:/bin"}, check=True)
            self.assertNotIn("restart sddm.service", (t / "sc.log").read_text(encoding="utf-8"))

    def test_bug_turkish_locale_regex_in_welcome_helper_and_snapshot(self):
        wh_src = (OVERLAY_DIR / "usr/libexec/blazeos-welcome-helper").read_text(encoding="utf-8")
        snap_src = (OVERLAY_DIR / "usr/bin/blaze-snapshot").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_bug_tr_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            _make_stub(bindir, "id", "echo 0\n")
            _make_stub(bindir, "systemctl", "exit 0\n")
            _make_stub(bindir, "dnf", "exit 0\n")
            _make_stub(bindir, "btrfs", "exit 0\n")
            _make_stub(bindir, "findmnt", "echo btrfs\n")
            r_wh = subprocess.run(
                ["bash", "-c", wh_src, "helper", "install-de", "@cinnamon-desktop-environment", "kitty", "niri"],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "LC_ALL": "tr_TR.UTF-8", "LANG": "tr_TR.UTF-8"},
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_wh.returncode, 0, f"tr_TR locale rejected valid DE packages: {r_wh.stderr}")

            patched_snap = snap_src.replace("/.blaze-snapshots", str(t / "snaps"))
            r_snap = subprocess.run(
                ["bash", "-c", patched_snap, "blaze-snapshot", "delete", "pre-install"],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "LC_ALL": "tr_TR.UTF-8", "LANG": "tr_TR.UTF-8"},
                capture_output=True,
                text=True,
            )
            self.assertNotIn("invalid snapshot name", r_snap.stderr)

    def test_bug_blaze_snapshot_unmount_on_die_and_prune_validation(self):
        snap_src = (OVERLAY_DIR / "usr/bin/blaze-snapshot").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_bug_snap_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            (t / "snapdir").mkdir()
            _make_stub(bindir, "btrfs", "exit 0\n")
            _make_stub(bindir, "findmnt", 'if [ "$2" = FSTYPE ]; then echo btrfs; elif [ "$2" = SOURCE ]; then echo /dev/sda1; elif [ "$2" = FSROOT ]; then echo /root; fi\n')
            _make_stub(bindir, "mount", f'touch "{t}/MOUNTED"\n')
            _make_stub(bindir, "umount", f'rm -f "{t}/MOUNTED"\n')
            _make_stub(bindir, "id", "echo 0\n")
            patched = snap_src.replace("/mnt/.blaze-btrfs-root", str(t / "top")).replace("/.blaze-snapshots", str(t / "snapdir"))

            r_rest = subprocess.run(
                ["bash", "-c", patched, "blaze-snapshot", "restore", "20260101T000000Z"],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "LC_ALL": "C.UTF-8"},
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(r_rest.returncode, 0)
            self.assertFalse((t / "MOUNTED").exists(), "blaze-snapshot leaked mount when die() exited")

            (t / "snapdir/20260101T000000Z").mkdir()
            r_prune = subprocess.run(
                ["bash", "-c", patched, "blaze-snapshot", "prune", "notanumber"],
                env={"PATH": f"{bindir}:/usr/bin:/bin", "LC_ALL": "C.UTF-8"},
                capture_output=True,
                text=True,
            )
            self.assertNotEqual(r_prune.returncode, 0, "blaze-snapshot prune accepted non-numeric keep")

    def test_bug_blaze_bootloader_arg_order_bootdir_and_bootorder_comma(self):
        bl_src = (OVERLAY_DIR / "usr/bin/blaze-bootloader").read_text(encoding="utf-8")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_bug_bl_") as tmp:
            t = Path(tmp)
            bindir = t / "bin"
            bindir.mkdir()
            (t / "sys/boot/efi/EFI/fedora").mkdir(parents=True)
            (t / "sys/boot/efi/EFI/BOOT").mkdir(parents=True)
            (t / "sys/boot/vmlinuz-6.18.0").write_bytes(b"KERN")
            (t / "sys/boot/initramfs-6.18.0.img").write_bytes(b"INIT")
            (t / "sys/usr/share/limine").mkdir(parents=True)
            (t / "sys/usr/share/limine/BOOTX64.EFI").write_bytes(b"EFI")
            (t / "sys/usr/share/limine/limine-bios.sys").write_bytes(b"BIOS")
            (t / "sys/etc").mkdir(parents=True)
            (t / "sys/etc/fstab").write_text("UUID=1234 / btrfs subvol=/root 0 0\n", encoding="utf-8")
            (t / "efi_sys").mkdir()
            _make_stub(bindir, "efibootmgr", f'if [ "$1" = -o ]; then echo "$2" > "{t}/order.txt"; else echo "BootOrder: 0001"; echo "Boot0001* Fedora"; fi\n')
            for c in ("findmnt", "lsblk", "limine"):
                _make_stub(bindir, c, "exit 0\n")

            r_order = subprocess.run(
                [str(OVERLAY_DIR / "usr/bin/blaze-bootloader"), "--sysroot", str(t / "sys"), "status"],
                env={"PATH": f"{bindir}:/usr/bin:/bin"},
                capture_output=True,
                text=True,
            )
            self.assertEqual(r_order.returncode, 0, r_order.stdout + r_order.stderr)

            patched = bl_src.replace("/sys/firmware/efi", str(t / "efi_sys"))
            subprocess.run(
                ["bash", "-c", patched, "blaze-bootloader", "--sysroot", str(t / "sys"), "switch-to-limine"],
                env={"PATH": f"{bindir}:/usr/bin:/bin"},
                check=True,
                capture_output=True,
            )
            self.assertTrue((t / "sys/boot/blazeos/vmlinuz").is_file())
            self.assertTrue((t / "sys/boot/blazeos/initramfs.img").is_file())

            subprocess.run(
                ["bash", "-c", patched, "blaze-bootloader", "switch-to-grub"],
                env={"PATH": f"{bindir}:/usr/bin:/bin"},
                check=True,
                capture_output=True,
            )
            self.assertEqual((t / "order.txt").read_text(encoding="utf-8").strip(), "0001")


@unittest.skipUnless(
    RUN_E2E,
    "Set BLAZE_RUN_E2E=1 or run `python3 -m tests.e2e.run_e2e --tier 5`",
)
class Tier5AdversarialTests(unittest.TestCase):
    """White-box adversarial tests for niri.kdl, imaging, QGA/QMP, QemuVM, iso_inspect, CLI, and Rust binaries."""

    def test_niri_kdl_parity_validation_and_required_rules(self):
        repo_kdl = REPO_ROOT / "solarui/data/niri.kdl"
        overlay_kdl = OVERLAY_DIR / "etc/solarui/niri.kdl"
        self.assertEqual(
            repo_kdl.read_bytes(),
            overlay_kdl.read_bytes(),
            "solarui/data/niri.kdl and blazeos_custom_apps/etc/solarui/niri.kdl drifted",
        )
        txt = repo_kdl.read_text(encoding="utf-8")
        for required in (
            'workspace "background"',
            'match app-id="^liveinst$"',
            "open-maximized false",
            "open-fullscreen false",
            'ELECTRON_OZONE_PLATFORM_HINT "wayland"',
            'MOZ_ENABLE_WAYLAND "1"',
            'XDG_SESSION_TYPE "wayland"',
        ):
            self.assertIn(required, txt)
        niri_bin = None
        from tests.e2e.config import get_niri_validate_binary
        niri_bin = get_niri_validate_binary()
        if niri_bin:
            oobe_kdl = OVERLAY_DIR / "usr/share/blaze-setup/niri-setup.kdl"
            for kdl in (repo_kdl, overlay_kdl, oobe_kdl):
                res = subprocess.run(
                    [niri_bin, "validate", "-c", str(kdl)],
                    capture_output=True,
                    text=True,
                    timeout=10,
                )
                self.assertEqual(res.returncode, 0, f"niri validate failed for {kdl}: {res.stderr}")

    def test_imaging_non_rgb_modes_fallback_conversion_and_decoys(self):
        with tempfile.TemporaryDirectory(prefix="blaze_t5_img_") as tmp:
            p = Path(tmp)
            ppm = p / "sample.ppm"
            Image.new("RGB", (64, 48), (12, 34, 56)).save(ppm, format="PPM")
            with mock.patch("tests.e2e.harness.imaging.shutil.which", return_value=None):
                out_png = convert_ppm_to_png(ppm, p / "pil_fallback.png")
            self.assertTrue(out_png.is_file())
            self.assertGreater(out_png.stat().st_size, 0)

            pal_png = p / "pal.png"
            Image.new("RGB", (320, 200), (20, 40, 80)).convert("P").save(pal_png, format="PNG")
            stats_pal = analyze_frame(pal_png)
            self.assertEqual((stats_pal.width, stats_pal.height), (320, 200))

            banner_only = p / "ana_banner_only.png"
            im = Image.new("RGB", (1280, 800), (41, 41, 41))
            d = ImageDraw.Draw(im)
            d.rectangle((115, 89, 1165, 149), fill=(79, 144, 201))
            d.rectangle((165, 110, 460, 128), fill=(245, 248, 252))
            im.save(banner_only, format="PNG")
            self.assertFalse(is_anaconda_webui_rendered(banner_only)[0])

            fh_spinner = p / "fh_spinner.png"
            im_fh = Image.new("RGB", (1280, 800), (14, 16, 20))
            d_fh = ImageDraw.Draw(im_fh)
            d_fh.arc((610, 370, 670, 430), 0, 270, fill=(56, 200, 255), width=4)
            im_fh.save(fh_spinner, format="PNG")
            self.assertFalse(is_firehub_rendered(fh_spinner)[0])

            grub = p / "grub.png"
            ply = p / "ply.png"
            Image.new("RGB", (720, 400), (5, 5, 5)).save(grub, format="PNG")
            Image.new("RGB", (1280, 800), (40, 110, 200)).save(ply, format="PNG")
            self.assertGreater(frame_diff_ratio(grub, ply), 0.9)

            ana_ok = test_harness_unit.HarnessUnitTests._make_synthetic_rendered_anaconda(p / "ana_ok.png")
            no_outer = {k: v for k, v in DEFAULT_ANACONDA_UI_REGIONS.items() if k != "outer_sidebar"}
            ok_no_outer, _ = is_anaconda_webui_rendered(ana_ok, regions=no_outer)
            self.assertTrue(ok_no_outer)
            no_banner = {k: v for k, v in DEFAULT_ANACONDA_UI_REGIONS.items() if k != "top_banner"}
            with self.assertRaises(KeyError):
                is_anaconda_webui_rendered(ana_ok, regions=no_banner)

    def test_imaging_luma_and_ratio_boundary_thresholds(self):
        with tempfile.TemporaryDirectory(prefix="blaze_t5_bnd_") as tmp:
            p = Path(tmp)
            fh = test_harness_unit.HarnessUnitTests._make_synthetic_rendered_firehub(p / "fh_base.png")
            ok_base, stats_base = is_firehub_rendered(fh)
            self.assertTrue(ok_base)
            self.assertLessEqual(stats_base["interior"].mean_luma, 45.0)

            bright_im = Image.open(fh).convert("RGB")
            ImageDraw.Draw(bright_im).rectangle((260, 220, 1020, 370), fill=(210, 210, 210))
            bright_fh = p / "fh_bright.png"
            bright_im.save(bright_fh, format="PNG")
            ok_bright, stats_bright = is_firehub_rendered(bright_fh)
            self.assertFalse(ok_bright)
            self.assertGreater(stats_bright["interior"].mean_luma, 45.0)

    def test_qga_and_qmp_disconnect_timeout_and_clamping_edges(self):
        qga = QgaClient("/tmp/nonexistent_qga_t5.sock", timeout=0.5)
        with self.assertRaises(QgaError):
            qga.sync()
        with self.assertRaises(QgaError):
            qga.execute("guest-ping")
        self.assertFalse(qga.ping())

        qmp = QmpClient("/tmp/nonexistent_qmp_t5.sock", timeout=0.5)
        with self.assertRaises(QmpError):
            qmp.execute("query-status")

        with tempfile.TemporaryDirectory(prefix="blaze_t5_qmp_") as tmp:
            sock_path = Path(tmp) / "qmp.sock"
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(str(sock_path))
            srv.listen(1)
            captured_events = []

            def server():
                conn, _ = srv.accept()
                with conn:
                    conn.sendall(b'{"QMP": {"version": {}, "capabilities": []}}\n')
                    buf = b""
                    while True:
                        chunk = conn.recv(4096)
                        if not chunk:
                            break
                        buf += chunk
                        while b"\n" in buf:
                            line, buf = buf.split(b"\n", 1)
                            msg = json.loads(line.decode())
                            if msg["execute"] == "input-send-event":
                                captured_events.append(msg["arguments"]["events"])
                            if msg["execute"] == "quit":
                                conn.sendall(b'{"return": {}}\n')
                                return
                            conn.sendall(b'{"return": {}}\n')

            t = threading.Thread(target=server, daemon=True)
            t.start()
            cli = QmpClient(str(sock_path), timeout=2.0)
            try:
                cli.connect(wait_timeout=2.0)
                cli.move_abs(-100, 99999, width=1280, height=800)
                cli.send_combo("ctrl", "return")
                cli.quit()
            finally:
                cli.close()
                srv.close()
                t.join(timeout=2.0)

            self.assertEqual(len(captured_events), 5)
            self.assertEqual(captured_events[0][0]["data"]["value"], 0)
            self.assertEqual(captured_events[0][1]["data"]["value"], 32767)
            combo_keys = [ev[0]["data"]["key"]["data"] for ev in captured_events[1:]]
            self.assertEqual(combo_keys, ["ctrl", "ret", "ret", "ctrl"])

    def test_qemu_vm_unsupported_firmware_and_missing_iso(self):
        with self.assertRaises(ValueError):
            QemuVM(iso_path=Path("/tmp/x.iso"), firmware="arm64")
        with tempfile.TemporaryDirectory(prefix="blaze_t5_vm_") as tmp:
            vm = QemuVM(iso_path=Path(tmp) / "missing.iso", firmware="bios", artifacts_dir=Path(tmp) / "art")
            try:
                with self.assertRaises(FileNotFoundError):
                    vm.build_command()
            finally:
                vm.cleanup()

    def test_run_e2e_cli_unit_without_iso_and_invalid_args(self):
        r_unit = subprocess.run(
            [
                sys.executable,
                "-m",
                "tests.e2e.run_e2e",
                "--tier",
                "unit",
                "--iso",
                "/tmp/definitely_missing_t5.iso",
                "--dry-run",
            ],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(r_unit.returncode, 0, r_unit.stderr)
        r_t5_dry = subprocess.run(
            [sys.executable, "-m", "tests.e2e.run_e2e", "--tier", "5", "--tier", "5", "--dry-run"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(r_t5_dry.returncode, 0, r_t5_dry.stderr)
        self.assertEqual(r_t5_dry.stdout.count("tier 5:"), 1)

        r_bad = subprocess.run(
            [sys.executable, "-m", "tests.e2e.run_e2e", "--tier", "99"],
            cwd=str(REPO_ROOT),
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(r_bad.returncode, 2)

    def test_iso_inspect_verify_sha256_uppercase_and_missing_checkisomd5(self):
        with tempfile.TemporaryDirectory(prefix="blaze_t5_iso_") as tmp:
            p = Path(tmp)
            iso = p / "test.iso"
            iso.write_bytes(b"solar-evolution")
            digest_upper = hashlib.sha256(b"solar-evolution").hexdigest().upper()
            sha = p / "test.iso.sha256"
            sha.write_text(f"  {digest_upper}  test.iso\n", encoding="utf-8")
            ok, _ = verify_iso_sha256_file(iso, sha)
            self.assertTrue(ok)

        with mock.patch("tests.e2e.harness.iso_inspect.shutil.which", return_value=None), mock.patch(
            "tests.e2e.harness.iso_inspect.Path.home", return_value=Path("/nonexistent_t5_home")
        ):
            with self.assertRaises(FileNotFoundError):
                checkisomd5_path()

    def test_tier4_niri_window_size_edge_cases(self):
        self.assertIsNone(test_tier4_qemu_e2e.Tier4QemuE2ETests._extract_niri_window_size({}))
        self.assertIsNone(test_tier4_qemu_e2e.Tier4QemuE2ETests._extract_niri_window_size({"layout": None}))
        self.assertIsNone(test_tier4_qemu_e2e.Tier4QemuE2ETests._extract_niri_window_size({"layout": {"window_size": [100]}}))
        self.assertIsNone(
            test_tier4_qemu_e2e.Tier4QemuE2ETests._extract_niri_window_size({"layout": {"window_size": ["bad", 600]}})
        )
        self.assertEqual(
            test_tier4_qemu_e2e.Tier4QemuE2ETests._extract_niri_window_size({"layout": {"tile_size": [1049, 628]}}),
            (1049.0, 628.0),
        )

    def test_regression_niri_json_and_qga_malformed_inputs(self):
        tc = test_tier4_qemu_e2e.Tier4QemuE2ETests("test_live_boot_legacy_bios")
        vm = mock.MagicMock()
        vm.qga.run_shell.side_effect = [
            GuestExecResult(exitcode=0, stdout="{bad json", stderr=""),
            GuestExecResult(exitcode=0, stdout="null", stderr=""),
            GuestExecResult(
                exitcode=0,
                stdout=json.dumps([{
                    "id": 5, "app_id": "liveinst", "title": "",
                    "is_focused": True, "is_floating": True,
                    "layout": {"window_size": [1049, 628]},
                }]),
                stderr="",
            ),
        ]
        with mock.patch("tests.e2e.test_tier4_qemu_e2e.time.sleep", return_value=None):
            win = tc._verify_anaconda_niri_window(vm, "bios", "diag", timeout=2.0)
        self.assertEqual(win["app_id"], "liveinst")
        self.assertIsNone(tc._extract_niri_window_size({"layout": {"window_size": ["inf", 628]}}))
        self.assertIsNone(tc._extract_niri_window_size({"layout": {"window_size": ["nan", 628]}}))
        self.assertIsNone(tc._extract_niri_window_size({"layout": {"window_size": [True, False]}}))

    def test_regression_imaging_rgba_alpha_zero_and_grayscale_decoys(self):
        with tempfile.TemporaryDirectory(prefix="blaze_t5_b6_") as tmp:
            p = Path(tmp)
            im = Image.new("RGBA", (1280, 800), (18, 24, 34, 0))
            draw = ImageDraw.Draw(im)
            draw.rectangle((115, 89, 1165, 745), fill=(41, 41, 41, 0))
            draw.rectangle((115, 89, 1165, 149), fill=(79, 144, 201, 0))
            for y in (190, 250, 310, 370, 430):
                draw.ellipse((150, y, 168, y + 18), fill=(79, 144, 201, 0))
                draw.rectangle((180, y + 3, 310, y + 15), fill=(215, 220, 225, 0))
            draw.rectangle((420, 240, 820, 270), fill=(235, 238, 242, 0))
            rgba_blank = p / "rgba_alpha0.png"
            im.save(rgba_blank, format="PNG")
            self.assertFalse(is_anaconda_webui_rendered(rgba_blank)[0])

            fh_rgb = test_harness_unit.HarnessUnitTests._make_synthetic_rendered_firehub(p / "fh_rgb.png")
            fh_gray = p / "fh_gray.png"
            Image.open(fh_rgb).convert("L").save(fh_gray, format="PNG")
            self.assertFalse(is_firehub_rendered(fh_gray)[0])

    def test_regression_qga_qmp_malformed_json_base64_and_move_abs(self):
        with tempfile.TemporaryDirectory(prefix="blaze_t5_b5_") as tmp:
            qga_sock = Path(tmp) / "qga.sock"
            srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            srv.bind(str(qga_sock))
            srv.listen(1)

            def qga_server():
                conn, _ = srv.accept()
                with conn:
                    buf = b""
                    req_idx = 0
                    while True:
                        chunk = conn.recv(4096)
                        if not chunk:
                            break
                        buf += chunk
                        while b"\n" in buf:
                            line, buf = buf.split(b"\n", 1)
                            req = json.loads(line.decode())
                            op = req["execute"]
                            if op == "guest-sync":
                                conn.sendall((json.dumps({"return": req["arguments"]["id"]}) + "\n").encode())
                            elif op == "guest-ping":
                                req_idx += 1
                                conn.sendall(b"{bad\n" if req_idx == 1 else b"null\n")
                            elif op == "guest-exec":
                                conn.sendall(b'{"return": {"pid": 99}}\n')
                            elif op == "guest-exec-status":
                                conn.sendall(b'{"return": {"exited": true, "exitcode": 0, "out-data": "!!!not-base64!!!"}}\n')
                                return

            t = threading.Thread(target=qga_server, daemon=True)
            t.start()
            qga = QgaClient(str(qga_sock), timeout=1.0)
            try:
                qga.connect(wait_timeout=2.0)
                with self.assertRaises(QgaError):
                    qga.execute("guest-ping")
                with self.assertRaises(QgaError):
                    qga.execute("guest-ping")
                with self.assertRaises(QgaError):
                    qga.run_shell("echo hi", timeout=2.0)
            finally:
                qga.close()
                srv.close()
                t.join(timeout=2.0)

            qmp_sock = Path(tmp) / "qmp.sock"
            qsrv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            qsrv.bind(str(qmp_sock))
            qsrv.listen(1)

            def qmp_server():
                conn, _ = qsrv.accept()
                with conn:
                    conn.sendall(b'{"QMP": {"version": {}, "capabilities": []}}\n')
                    buf = b""
                    while True:
                        chunk = conn.recv(4096)
                        if not chunk:
                            break
                        buf += chunk
                        while b"\n" in buf:
                            line, buf = buf.split(b"\n", 1)
                            msg = json.loads(line.decode())
                            if msg["execute"] == "qmp_capabilities":
                                conn.sendall(b'{"return": {}}\n')
                            elif msg["execute"] == "query-status":
                                conn.sendall(b"null\n")
                                return

            qt = threading.Thread(target=qmp_server, daemon=True)
            qt.start()
            qmp = QmpClient(str(qmp_sock), timeout=1.0)
            try:
                qmp.connect(wait_timeout=2.0)
                with self.assertRaises(ValueError):
                    qmp.move_abs(10, 10, width=0, height=800)
                with self.assertRaises(QmpError):
                    qmp.query_status()
            finally:
                qmp.close()
                qsrv.close()
                qt.join(timeout=2.0)

    def test_regression_qemu_vm_capture_png_unlinks_stale_ppm(self):
        with tempfile.TemporaryDirectory(prefix="blaze_t5_ppm_") as tmp:
            p = Path(tmp)
            fake_iso = p / "fake.iso"
            fake_iso.write_bytes(b"ISO")
            vm = QemuVM(iso_path=fake_iso, firmware="bios", artifacts_dir=p / "art")
            try:
                stale_ppm = vm.run_dir / "probe.ppm"
                Image.new("RGB", (64, 48), (255, 0, 0)).save(stale_ppm, format="PPM")

                def write_fresh_ppm(ppm_path: str):
                    Image.new("RGB", (64, 48), (0, 255, 0)).save(ppm_path, format="PPM")

                with mock.patch.object(vm.qmp, "screendump", side_effect=write_fresh_ppm):
                    out_png = vm.capture_png("probe")
                with Image.open(out_png) as out_im:
                    px = out_im.convert("RGB").getpixel((10, 10))
                    self.assertGreater(px[1], 200)
                    self.assertLess(px[0], 50)
            finally:
                vm.cleanup()

    def test_regression_rust_binaries_cli_contracts(self):
        solar_shell = str(OVERLAY_DIR / "usr/bin/solar-shell")
        sentinel = str(OVERLAY_DIR / "usr/bin/blaze-sentinel")
        recovery = str(OVERLAY_DIR / "usr/bin/blaze-recovery")

        with tempfile.TemporaryDirectory(prefix="blaze_t5_rust_") as tmp:
            t = Path(tmp)
            env = {**os.environ, "HOME": str(t), "XDG_RUNTIME_DIR": str(t)}
            settings_path = t / ".local/state/noctalia/settings.toml"
            settings_path.parent.mkdir(parents=True)
            settings_path.write_text("scalar = 42\n", encoding="utf-8")

            # B2: reject empty key & non-table parent without modifying file
            r_empty = subprocess.run([solar_shell, "noctalia-config", "set", "", "badval"], env=env, capture_output=True, text=True, timeout=10)
            self.assertNotEqual(r_empty.returncode, 0)
            r_scalar = subprocess.run([solar_shell, "noctalia-config", "set", "scalar.nested", "dropped"], env=env, capture_output=True, text=True, timeout=10)
            self.assertNotEqual(r_scalar.returncode, 0)
            self.assertEqual(settings_path.read_text(encoding="utf-8"), "scalar = 42\n")

            # B7: unknown solar-shell subcommand & unknown blaze-recovery flag exit non-zero
            r_sh_bad = subprocess.run([solar_shell, "__unknown_subcmd__"], env=env, capture_output=True, text=True, timeout=10)
            self.assertNotEqual(r_sh_bad.returncode, 0)
            r_rec_bad = subprocess.run([recovery, "--__bogus_flag__"], env=env, capture_output=True, text=True, timeout=10)
            self.assertNotEqual(r_rec_bad.returncode, 0)

            # B3: --json before subcommand works, and inherited INVOCATION_ID does not hang CLI
            r_json = subprocess.run([sentinel, "--json", "status"], env=env, capture_output=True, text=True, timeout=10)
            self.assertEqual(r_json.returncode, 0, r_json.stderr)
            self.assertEqual(json.loads(r_json.stdout).get("total"), 7)

            r_inv = subprocess.run(
                [sentinel],
                env={**env, "INVOCATION_ID": "test-desktop-session"},
                capture_output=True,
                text=True,
                timeout=5,
            )
            self.assertEqual(r_inv.returncode, 0, r_inv.stderr)


if __name__ == "__main__":
    unittest.main()
