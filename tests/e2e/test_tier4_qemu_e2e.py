"""Tier 4: Full QEMU E2E live boot verification in BOTH Legacy BIOS and UEFI OVMF."""
import json
import math
import os
import shlex
import time
import unittest

from tests.e2e.config import (
    CUSTOM_14_TOOLS,
    FORBIDDEN_LIVE_RPMS,
    IMMUTABLE_BASELINE_SHA256,
    REPO_ROOT,
    discover_overlay_usr_bin_tools,
    get_artifacts_dir,
    get_firmware_mode,
    get_target_iso_path,
)
from tests.e2e.harness.imaging import (
    frame_diff_ratio,
    is_anaconda_webui_rendered,
    is_firehub_rendered,
    is_non_black_content,
)
from tests.e2e.harness.iso_inspect import sha256_file
from tests.e2e.harness.qemu_vm import QemuVM

RUN_QEMU_E2E = os.environ.get("BLAZE_RUN_QEMU_E2E") == "1"


@unittest.skipUnless(
    RUN_QEMU_E2E,
    "Set BLAZE_RUN_QEMU_E2E=1 or run `python3 -m tests.e2e.run_e2e --tier 4`",
)
class Tier4QemuE2ETests(unittest.TestCase):
    """End-to-end QEMU live ISO boot and runtime verification (BIOS + UEFI)."""

    def _capture_grub_frame(self, vm: QemuVM, firmware: str):
        if firmware == "bios":
            time.sleep(2.5)
            grub_png = vm.capture_png(f"01_grub_{firmware}")
            for _ in range(12):
                if is_non_black_content(grub_png, min_non_black_ratio=0.005, min_std_luma=1.0):
                    return grub_png
                time.sleep(0.5)
                grub_png = vm.capture_png(f"01_grub_{firmware}")
            return grub_png

        # UEFI OVMF: capture early TianoCore splash and wait for transition into GRUB2 menu
        t0 = time.monotonic()
        time.sleep(1.2)
        early_png = vm.capture_png(f"00_firmware_{firmware}")
        early_visible = is_non_black_content(early_png, min_non_black_ratio=0.005, min_std_luma=1.0)
        deadline = t0 + 13.0
        grub_png = early_png
        while time.monotonic() < deadline:
            time.sleep(0.5)
            cand = vm.capture_png(f"01_grub_{firmware}")
            if not is_non_black_content(cand, min_non_black_ratio=0.005, min_std_luma=1.0):
                continue
            if early_visible and frame_diff_ratio(early_png, cand) > 0.05:
                time.sleep(0.4)
                return vm.capture_png(f"01_grub_{firmware}")
            if not early_visible and (time.monotonic() - t0) >= 4.5:
                return cand
            grub_png = cand
        return grub_png

    def _find_installer_procs(self, vm: QemuVM) -> list[str]:
        """Inspect live guest installer processes via direct guest_exec (no bash -lc self-match)."""
        res = vm.qga.guest_exec(
            "/usr/bin/pgrep",
            ["-fa", "[a]naconda|[l]iveinst|[w]ebui-desktop"],
            timeout=15.0,
        )
        if res.exitcode not in (0, 1):
            raise AssertionError(
                f"pgrep guest_exec failed (rc={res.exitcode}): {res.stderr}"
            )
        if res.exitcode == 1 or not res.stdout.strip():
            return []
        return [line.strip() for line in res.stdout.splitlines() if line.strip()]

    def _read_guest_file(self, vm: QemuVM, path: str) -> str:
        res = vm.qga.guest_exec("/usr/bin/cat", [path], timeout=15.0)
        return res.stdout if res.exitcode == 0 else ""

    def _collect_installer_logs(self, vm: QemuVM) -> str:
        session_log = self._read_guest_file(vm, "/home/liveuser/.local/share/solarui/session.log")
        liveinst_log = self._read_guest_file(vm, "/tmp/e2e_liveinst.log")
        return (
            "--- /home/liveuser/.local/share/solarui/session.log ---\n"
            f"{session_log.strip() or '<empty>'}\n"
            "--- /tmp/e2e_liveinst.log ---\n"
            f"{liveinst_log.strip() or '<empty>'}"
        )

    def _is_liveinst_lock_free(self, vm: QemuVM) -> bool:
        res = vm.qga.run_shell(
            "test ! -e /run/anaconda/liveinst.lock || flock -n /run/anaconda/liveinst.lock true",
            timeout=15.0,
        )
        return res.exitcode == 0

    @staticmethod
    def _has_installer_ui_proc(procs: list[str]) -> bool:
        """Return True only when the actual installer browser/UI process (not webui-desktop wrapper) is active."""
        return any(
            "firefox" in p or "slitherer" in p or "anaconda-webui/index.html" in p
            for p in procs
        )

    def _stop_auto_launched_installer(self, vm: QemuVM, firmware: str) -> None:
        """Wait for solar-session-wrapper's auto-launch window, then cleanly stop it before desktop capture."""
        trigger_deadline = time.monotonic() + 15.0
        auto_triggered = False
        procs: list[str] = []
        while time.monotonic() < trigger_deadline:
            procs = self._find_installer_procs(vm)
            if procs:
                auto_triggered = True
                break
            session_log = self._read_guest_file(vm, "/home/liveuser/.local/share/solarui/session.log")
            if "launching Anaconda installer" in session_log:
                auto_triggered = True
                break
            time.sleep(0.5)

        if auto_triggered and not procs:
            spawn_deadline = time.monotonic() + 4.0
            while time.monotonic() < spawn_deadline:
                procs = self._find_installer_procs(vm)
                if procs:
                    break
                time.sleep(0.5)

        if procs or not self._is_liveinst_lock_free(vm):
            # Stop webui-cockpit-ws.service before pkill to avoid Restart=on-failure race
            vm.qga.guest_exec(
                "/usr/bin/systemctl",
                ["stop", "webui-cockpit-ws.service"],
                timeout=15.0,
            )
            vm.qga.guest_exec(
                "/usr/bin/pkill",
                ["-TERM", "-f", "[a]naconda|[l]iveinst|[w]ebui-desktop"],
                timeout=15.0,
            )
            stop_deadline = time.monotonic() + 20.0
            killed = False
            while time.monotonic() < stop_deadline:
                if not self._find_installer_procs(vm) and self._is_liveinst_lock_free(vm):
                    break
                if not killed and (stop_deadline - time.monotonic()) < 5.0:
                    vm.qga.guest_exec(
                        "/usr/bin/pkill",
                        ["-KILL", "-f", "[a]naconda|[l]iveinst|[w]ebui-desktop"],
                        timeout=15.0,
                    )
                    killed = True
                time.sleep(0.5)
            # Stop once more in case webui-desktop re-triggered the unit before exiting
            vm.qga.guest_exec(
                "/usr/bin/systemctl",
                ["stop", "webui-cockpit-ws.service"],
                timeout=15.0,
            )

        remaining = self._find_installer_procs(vm)
        self.assertEqual(
            remaining,
            [],
            f"[{firmware}] Auto-launched installer still running before desktop capture: {remaining}\n"
            f"{self._collect_installer_logs(vm)}",
        )
        self.assertTrue(
            self._is_liveinst_lock_free(vm),
            f"[{firmware}] /run/anaconda/liveinst.lock still held before desktop capture\n"
            f"{self._collect_installer_logs(vm)}",
        )
        # Clean stale runtime files if pkill terminated anaconda before atexit cleanup
        vm.qga.guest_exec(
            "/usr/bin/rm",
            ["-f", "/run/anaconda/anaconda.conf", "/run/anaconda/bus.address"],
            timeout=15.0,
        )
        vm.qga.guest_exec(
            "/usr/bin/pkill",
            ["-KILL", "-f", "/[r]un/install/"],
            timeout=15.0,
        )
        cleanup_res = vm.qga.run_shell(
            "set -e; "
            "if findmnt -rn -o TARGET | grep -qE '^/run/install(/|$)'; then "
            "  /usr/bin/anaconda-cleanup --liveinst || true; "
            "fi; "
            "remaining_mounts=$(findmnt -rn -o TARGET | grep -E '^/run/install/sources(/|$)' || true); "
            "if [ -n \"$remaining_mounts\" ]; then "
            "  echo \"STALE_INSTALL_MOUNTS:$remaining_mounts\" >&2; "
            "  exit 1; "
            "fi; "
            "rm -f /run/anaconda/backend_ready /run/anaconda/webui_script.pid "
            "/tmp/anaconda-wldisplay /tmp/anaconda-atspibusaddr /tmp/webui-cockpit-ws.env; "
            "if [ \"$(/usr/sbin/getenforce 2>/dev/null || echo Enforcing)\" != \"Enforcing\" ]; then "
            "  /usr/sbin/setenforce Enforcing; "
            "fi",
            timeout=20.0,
        )
        self.assertEqual(
            cleanup_res.exitcode,
            0,
            f"[{firmware}] Post-stop installer mount/runtime cleanup failed:\n"
            f"{cleanup_res.stdout}\n{cleanup_res.stderr}\n{self._collect_installer_logs(vm)}",
        )

    def _wait_for_anaconda_backend_ready(
        self, vm: QemuVM, firmware: str, timeout: float = 50.0
    ) -> list[str]:
        """Gate 1: Wait for /run/anaconda/backend_ready, Cockpit WebUI HTTP 200, and UI proc."""
        deadline = time.monotonic() + timeout
        ana_procs: list[str] = []
        backend_ready = False
        last_diag = ""
        while time.monotonic() < deadline:
            ana_procs = self._find_installer_procs(vm)
            if self._has_installer_ui_proc(ana_procs):
                res = vm.qga.run_shell(
                    "test -f /run/anaconda/backend_ready && "
                    "code=$(curl -fsS -o /dev/null -w '%{http_code}' "
                    "http://127.0.0.1/cockpit/@localhost/anaconda-webui/index.html 2>/dev/null || echo 000) && "
                    "echo \"HTTP:$code\" && [ \"$code\" = \"200\" ]",
                    timeout=15.0,
                )
                last_diag = f"rc={res.exitcode} out={res.stdout!r} err={res.stderr!r}"
                if res.exitcode == 0:
                    backend_ready = True
                    break
            time.sleep(1.5)
        ana_procs = self._find_installer_procs(vm)
        logs_diag = self._collect_installer_logs(vm)
        self.assertTrue(
            bool(ana_procs) and self._has_installer_ui_proc(ana_procs),
            f"[{firmware}] Anaconda/liveinst UI process did not stay alive after spawn: {ana_procs}\n{logs_diag}",
        )
        self.assertTrue(
            backend_ready,
            f"[{firmware}] Step 7 Gate 1 failed (/run/anaconda/backend_ready + Cockpit HTTP 200): "
            f"{last_diag}\n{logs_diag}",
        )
        return ana_procs

    _ANACONDA_ERROR_TITLES = (
        "problem loading",
        "unable to connect",
        "404",
        "not found",
        "server not found",
    )

    @staticmethod
    def _extract_niri_window_size(win: dict) -> tuple[float, float] | None:
        layout = win.get("layout")
        if not isinstance(layout, dict):
            return None
        raw_size = layout.get("window_size")
        if raw_size is None:
            raw_size = layout.get("tile_size")
        if not isinstance(raw_size, (list, tuple)) or len(raw_size) < 2:
            return None
        w_raw, h_raw = raw_size[0], raw_size[1]
        if isinstance(w_raw, bool) or isinstance(h_raw, bool):
            return None
        try:
            w_val = float(w_raw)
            h_val = float(h_raw)
        except (TypeError, ValueError):
            return None
        if not (math.isfinite(w_val) and math.isfinite(h_val)):
            return None
        return (w_val, h_val)

    def _verify_anaconda_niri_window(
        self, vm: QemuVM, firmware: str, logs_diag: str, timeout: float = 25.0
    ) -> dict:
        """Gate 2: Verify liveinst window is focused, floating (>=900x550), error-free, and unobstructed.

        Fedora's /usr/libexec/anaconda/webui-desktop exports MOZ_APP_TITLE="" and
        userChrome.css collapses #navigator-toolbox, so Niri receives title="" even
        when WebUI renders cleanly; non-empty browser error titles are still rejected.
        """
        deadline = time.monotonic() + timeout
        matched_win = None
        last_candidate = None
        last_candidate_windows: list[dict] = []
        last_diag = ""
        while time.monotonic() < deadline:
            res = vm.qga.run_shell(
                "set -e; "
                "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
                "nsock=$(runuser -u liveuser -- bash -c "
                "\"export XDG_RUNTIME_DIR=$rt DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus; "
                "systemctl --user show-environment 2>/dev/null | sed -n 's/^NIRI_SOCKET=//p' | head -n1\"); "
                "if [ -z \"$nsock\" ] || [ ! -S \"$nsock\" ]; then "
                "  nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                "fi; "
                "test -n \"$nsock\" && test -S \"$nsock\"; "
                "runuser -u liveuser -- env XDG_RUNTIME_DIR=\"$rt\" NIRI_SOCKET=\"$nsock\" niri msg --json windows",
                timeout=15.0,
            )
            last_diag = f"rc={res.exitcode} out={res.stdout!r} err={res.stderr!r}"
            if res.exitcode == 0 and res.stdout.strip():
                try:
                    raw_windows = json.loads(res.stdout)
                except (json.JSONDecodeError, TypeError):
                    raw_windows = None
                if isinstance(raw_windows, list):
                    windows = [w for w in raw_windows if isinstance(w, dict)]
                    matches = [w for w in windows if w.get("app_id") in ("liveinst", "firefox")]
                    focused_matches = [w for w in matches if w.get("is_focused") is True]
                    if focused_matches:
                        cand = next(
                            (w for w in focused_matches if w.get("app_id") == "liveinst"),
                            focused_matches[0],
                        )
                        last_candidate = cand
                        last_candidate_windows = windows
                        title_raw = cand.get("title")
                        title_lower = title_raw.strip().lower() if isinstance(title_raw, str) else ""
                        has_error = bool(title_lower) and any(
                            err in title_lower for err in self._ANACONDA_ERROR_TITLES
                        )
                        size = self._extract_niri_window_size(cand)
                        size_ok = size is not None and size[0] >= 900.0 and size[1] >= 550.0
                        welcome_focused = any(
                            w.get("app_id") == "solar-welcome" and w.get("is_focused") is True
                            for w in windows
                        )
                        if (
                            cand.get("is_floating") is True
                            and not has_error
                            and size_ok
                            and not welcome_focused
                        ):
                            matched_win = cand
                            break
            remaining = deadline - time.monotonic()
            if remaining > 0:
                time.sleep(min(1.0, remaining))

        target_win = matched_win or last_candidate
        self.assertIsNotNone(
            target_win,
            f"[{firmware}] Step 7 Gate 2 failed: focused liveinst/firefox window not found: "
            f"{last_diag}\n{logs_diag}",
        )
        assert target_win is not None

        title = (target_win.get("title") or "").strip()
        if title:
            title_lower = title.lower()
            has_error = any(err in title_lower for err in self._ANACONDA_ERROR_TITLES)
            self.assertFalse(
                has_error,
                f"[{firmware}] Step 7 Gate 2 failed: liveinst/firefox window reported browser error title "
                f"{title!r}: {target_win}\n{logs_diag}",
            )

        self.assertTrue(
            target_win.get("is_floating") is True,
            f"[{firmware}] Step 7 Gate 2 failed: liveinst must be genuine floating foreground "
            f"(is_floating=True), got: {target_win}\n{logs_diag}",
        )

        welcome_above = [
            w for w in last_candidate_windows
            if w.get("app_id") == "solar-welcome" and w.get("is_focused") is True
        ]
        self.assertEqual(
            welcome_above,
            [],
            f"[{firmware}] Step 7 Gate 2 failed: solar-welcome is focused above liveinst: "
            f"{welcome_above}\n{logs_diag}",
        )

        size = self._extract_niri_window_size(target_win)
        self.assertTrue(
            size is not None and size[0] >= 900.0 and size[1] >= 550.0,
            f"[{firmware}] Step 7 Gate 2 failed: liveinst floating geometry must be >= [900, 550], "
            f"got size={size} window={target_win}\n{logs_diag}",
        )
        return target_win

    def _capture_settled_anaconda_frame(
        self, vm: QemuVM, firmware: str, desktop_png, logs_diag: str, timeout: float = 25.0
    ):
        """Gates 3 & 4: Poll QMP frames until Anaconda WebUI regions render and settle (<0.02 diff)."""
        deadline = time.monotonic() + timeout
        settled_png = None
        last_stats = None
        last_diff = 0.0
        last_settle_diff = None
        while time.monotonic() < deadline:
            cand_a = vm.capture_png(f"05_anaconda_{firmware}_probe_a")
            try:
                non_black = is_non_black_content(cand_a)
                diff_desktop = frame_diff_ratio(desktop_png, cand_a)
                rendered_a, stats_a = is_anaconda_webui_rendered(cand_a)
                last_stats = stats_a
                last_diff = diff_desktop
                if non_black and diff_desktop > 0.10 and rendered_a:
                    time.sleep(1.0)
                    cand_b = vm.capture_png(f"05_anaconda_{firmware}")
                    self.assertNotEqual(
                        cand_a,
                        cand_b,
                        f"[{firmware}] Gate 4 cand_a and cand_b must be distinct paths: {cand_a}",
                    )
                    settle_diff = frame_diff_ratio(cand_a, cand_b)
                    rendered_b, stats_b = is_anaconda_webui_rendered(cand_b)
                    last_stats = stats_b
                    last_diff = frame_diff_ratio(desktop_png, cand_b)
                    last_settle_diff = settle_diff
                    if (
                        is_non_black_content(cand_b)
                        and last_diff > 0.10
                        and rendered_b
                        and settle_diff < 0.02
                    ):
                        settled_png = cand_b
                        break
                else:
                    remaining = deadline - time.monotonic()
                    if remaining > 0:
                        time.sleep(min(1.5, remaining))
            finally:
                if cand_a.name.endswith("_probe_a.png"):
                    cand_a.unlink(missing_ok=True)

        if settled_png is None:
            ana_png = vm.capture_png(f"05_anaconda_{firmware}")
            rendered_final, stats_final = is_anaconda_webui_rendered(ana_png)
            diff_final = frame_diff_ratio(desktop_png, ana_png)
            self.assertTrue(
                is_non_black_content(ana_png),
                f"[{firmware}] Step 7 Gate 3 failed: Anaconda frame is black\n{logs_diag}",
            )
            self.assertGreater(
                diff_final,
                0.10,
                f"[{firmware}] Step 7 Gate 3 failed: diff vs desktop={diff_final:.4f} <= 0.10\n{logs_diag}",
            )
            self.assertTrue(
                rendered_final,
                f"[{firmware}] Step 7 Gate 3 failed: Anaconda WebUI UI regions did not render "
                f"(blank canvas / obstructed): stats={stats_final}\n{logs_diag}",
            )
            self.fail(
                f"[{firmware}] Step 7 Gate 4 failed: Anaconda frame did not settle within {timeout}s "
                f"(last_settle_diff={last_settle_diff}, last_diff={last_diff:.4f}, stats={last_stats})\n{logs_diag}"
            )
        return settled_png

    def _capture_settled_firehub_frame(
        self, vm: QemuVM, firmware: str, desktop_png, logs_diag: str, timeout: float = 25.0
    ):
        """Poll QMP frames until FireHub storefront regions render and settle (<0.02 diff)."""
        deadline = time.monotonic() + timeout
        settled_png = None
        last_stats = None
        last_diff = 0.0
        last_settle_diff = None
        while time.monotonic() < deadline:
            cand_a = vm.capture_png(f"04_firehub_{firmware}_probe_a")
            try:
                non_black = is_non_black_content(cand_a)
                diff_desktop = frame_diff_ratio(desktop_png, cand_a)
                rendered_a, stats_a = is_firehub_rendered(cand_a)
                last_stats = stats_a
                last_diff = diff_desktop
                if non_black and diff_desktop > 0.15 and rendered_a:
                    time.sleep(1.0)
                    cand_b = vm.capture_png(f"04_firehub_{firmware}")
                    self.assertNotEqual(
                        cand_a,
                        cand_b,
                        f"[{firmware}] Step 6 cand_a and cand_b must be distinct paths: {cand_a}",
                    )
                    settle_diff = frame_diff_ratio(cand_a, cand_b)
                    rendered_b, stats_b = is_firehub_rendered(cand_b)
                    last_stats = stats_b
                    last_diff = frame_diff_ratio(desktop_png, cand_b)
                    last_settle_diff = settle_diff
                    if (
                        is_non_black_content(cand_b)
                        and last_diff > 0.15
                        and rendered_b
                        and settle_diff < 0.02
                    ):
                        settled_png = cand_b
                        break
                else:
                    remaining = deadline - time.monotonic()
                    if remaining > 0:
                        time.sleep(min(1.5, remaining))
            finally:
                if cand_a.name.endswith("_probe_a.png"):
                    cand_a.unlink(missing_ok=True)

        if settled_png is None:
            fh_png = vm.capture_png(f"04_firehub_{firmware}")
            rendered_final, stats_final = is_firehub_rendered(fh_png)
            diff_final = frame_diff_ratio(desktop_png, fh_png)
            self.assertTrue(
                is_non_black_content(fh_png),
                f"[{firmware}] Step 6 failed: FireHub frame is black\n{logs_diag}",
            )
            self.assertGreater(
                diff_final,
                0.15,
                f"[{firmware}] Step 6 failed: FireHub diff vs desktop={diff_final:.4f} <= 0.15\n{logs_diag}",
            )
            self.assertTrue(
                rendered_final,
                f"[{firmware}] Step 6 failed: FireHub UI regions did not render "
                f"(blank #0d0f12 canvas / obstructed): stats={stats_final}\n{logs_diag}",
            )
            self.fail(
                f"[{firmware}] Step 6 failed: FireHub frame did not settle within {timeout}s "
                f"(last_settle_diff={last_settle_diff}, last_diff={last_diff:.4f}, stats={last_stats})\n{logs_diag}"
            )
        return settled_png

    def _run_full_boot_flow(self, firmware: str) -> None:
        iso_path = get_target_iso_path()
        if not iso_path.is_file():
            self.fail(f"Target ISO missing: {iso_path}")

        license_path = REPO_ROOT / "LICENSE"
        self.assertTrue(license_path.is_file(), f"Root LICENSE missing: {license_path}")
        license_text = license_path.read_text(encoding="utf-8", errors="replace")
        self.assertIn("GNU GENERAL PUBLIC LICENSE", license_text)
        self.assertIn("Version 3", license_text)

        artifacts = get_artifacts_dir() / firmware
        artifacts.mkdir(parents=True, exist_ok=True)

        with QemuVM(
            iso_path=iso_path,
            firmware=firmware,
            memory_mb=4096,
            cpus=4,
            artifacts_dir=artifacts,
            disk_gb=20,
        ) as vm:
            # 1. Capture GRUB menu screen (distinguishing UEFI OVMF POST splash from GRUB)
            grub_png = self._capture_grub_frame(vm, firmware)
            self.assertTrue(
                is_non_black_content(grub_png, min_non_black_ratio=0.005, min_std_luma=1.0),
                f"[{firmware}] GRUB screen remained black: {grub_png}",
            )

            # Confirm default GRUB entry ("Start Blaze SolarEvolution 5") immediately
            vm.qmp.send_key("ret")

            # 2. Capture Plymouth boot splash screen (must differ from GRUB)
            time.sleep(2.5)
            ply_png = None
            for _ in range(15):
                time.sleep(1.5)
                cand = vm.capture_png(f"02_plymouth_{firmware}")
                if (
                    is_non_black_content(cand, min_non_black_ratio=0.005, min_std_luma=1.0)
                    and frame_diff_ratio(grub_png, cand) > 0.03
                ):
                    ply_png = cand
                    break
            self.assertIsNotNone(ply_png, f"[{firmware}] Failed to observe non-black Plymouth frame")
            assert ply_png is not None

            # 3. Wait for QEMU Guest Agent inside booted Live OS
            vm.qga.wait_until_ready(timeout=180.0, poll_interval=2.0)

            # 4. Wait for SDDM direct autologin into liveuser SolarUI session (niri + solar-shell + noctalia)
            session_ready = False
            last_diag = ""
            deadline = time.monotonic() + 90.0
            while time.monotonic() < deadline:
                res = vm.qga.run_shell(
                    "loginctl list-sessions --no-legend; echo '---'; "
                    "pgrep -u liveuser -a 'niri|solar-shell|noctalia' || true"
                )
                last_diag = res.stdout
                if "liveuser" in res.stdout and "niri" in res.stdout and "solar-shell" in res.stdout:
                    session_ready = True
                    break
                time.sleep(2.5)
            self.assertTrue(
                session_ready,
                f"[{firmware}] Live SolarUI session did not become ready:\n{last_diag}",
            )

            # Reconcile solar-session-wrapper's auto-launched liveinst before capturing clean desktop
            self._stop_auto_launched_installer(vm, firmware)
            time.sleep(2.0)
            desktop_png = vm.capture_png(f"03_desktop_{firmware}")
            self.assertTrue(
                is_non_black_content(desktop_png),
                f"[{firmware}] SolarUI desktop frame is black: {desktop_png}",
            )
            self.assertGreater(
                frame_diff_ratio(ply_png, desktop_png),
                0.03,
                f"[{firmware}] Desktop frame did not visually transition from Plymouth",
            )

            # Verify SDDM autologin bypassed greeter & GDM is masked/inactive
            dm_check = vm.qga.run_shell(
                "set -e; "
                "systemctl is-active --quiet sddm.service; "
                "! systemctl is-active --quiet gdm.service; "
                "! pgrep -x sddm-greeter >/dev/null 2>&1; "
                "! pgrep -x sddm-greeter-qt6 >/dev/null 2>&1; "
                "test ! -e /etc/sddm.conf.d/00-blaze-oobe.conf; "
                "grep -q '^User=liveuser$' /etc/sddm.conf.d/autologin.conf; "
                "grep -q '^Session=solarui.desktop$' /etc/sddm.conf.d/autologin.conf; "
                "pgrep -u liveuser -x niri >/dev/null; "
                "pgrep -u liveuser -x solar-shell >/dev/null; "
                "pgrep -u liveuser -x noctalia >/dev/null"
            )
            self.assertEqual(
                dm_check.exitcode,
                0,
                f"[{firmware}] SDDM/SolarUI autologin verification failed:\n{dm_check.stdout}\n{dm_check.stderr}",
            )

            # Restore SELinux Enforcing if killed auto-launched liveinst left it Permissive
            enforce_res = vm.qga.run_shell(
                "set -e; "
                "if [ \"$(/usr/sbin/getenforce)\" != \"Enforcing\" ]; then "
                "  /usr/sbin/setenforce Enforcing; "
                "fi; "
                "test \"$(/usr/sbin/getenforce)\" = \"Enforcing\"",
                timeout=15.0,
            )
            self.assertEqual(
                enforce_res.exitcode,
                0,
                f"[{firmware}] Failed to ensure SELinux Enforcing after stopping auto-launched installer:\n"
                f"{enforce_res.stdout}\n{enforce_res.stderr}",
            )

            # Verify liveuser desktop processes transitioned out of xdm_t into unconfined_t
            ps_desktop = vm.qga.run_shell("ps -eo user=,label=,comm=", timeout=15.0)
            self.assertEqual(ps_desktop.exitcode, 0, ps_desktop.stderr)
            seen_desktop_comms: set[str] = set()
            for line in ps_desktop.stdout.splitlines():
                parts = line.split(None, 2)
                if len(parts) < 3:
                    continue
                proc_user, proc_label, proc_comm = parts
                if proc_user == "liveuser" and proc_comm in ("niri", "solar-shell", "noctalia"):
                    seen_desktop_comms.add(proc_comm)
                    self.assertIn(
                        ":unconfined_t:",
                        proc_label,
                        f"[{firmware}] liveuser process {proc_comm} not in unconfined_t: {proc_label}",
                    )
                    self.assertNotIn(
                        ":xdm_t:",
                        proc_label,
                        f"[{firmware}] liveuser process {proc_comm} trapped in xdm_t: {proc_label}",
                    )
            self.assertEqual(
                seen_desktop_comms,
                {"niri", "solar-shell", "noctalia"},
                f"[{firmware}] Missing expected liveuser desktop processes in ps output:\n{ps_desktop.stdout}",
            )

            # Verify SolarUI session startup and systemd --user had no SELinux Access denied errors
            session_log = self._read_guest_file(vm, "/home/liveuser/.local/share/solarui/session.log")
            self.assertNotIn(
                "Access denied",
                session_log,
                f"[{firmware}] session.log reported Access denied:\n{session_log}",
            )
            user_unit_res = vm.qga.run_shell(
                "set -e; "
                "uid=$(id -u liveuser); "
                "runuser -u liveuser -- env "
                "XDG_RUNTIME_DIR=\"/run/user/$uid\" "
                "DBUS_SESSION_BUS_ADDRESS=\"unix:path=/run/user/$uid/bus\" "
                "systemctl --user is-active solarui-session.service",
                timeout=15.0,
            )
            self.assertEqual(
                user_unit_res.exitcode,
                0,
                f"[{firmware}] solarui-session.service is not active for liveuser:\n"
                f"{user_unit_res.stdout}\n{user_unit_res.stderr}\n{session_log}",
            )

            # 5. In-guest parity: dynamic overlay tools, Plymouth logs, sentinel v0.3.1,
            #    blaze.repo + dnf5 repoinfo, 99-blazeos.conf, assets, FORBIDDEN_LIVE_RPMS
            overlay_tools = discover_overlay_usr_bin_tools()
            self.assertTrue(set(CUSTOM_14_TOOLS).issubset(set(overlay_tools)))
            tools_joined = " ".join(shlex.quote(t) for t in overlay_tools)
            parity = vm.qga.run_shell(
                f"set -e; "
                f"for t in {tools_joined}; do test -x /usr/bin/$t || {{ echo MISSING:$t; exit 1; }}; done; "
                "blaze-sentinel --version | grep -q '0.3.1'; "
                "blaze-recovery --version | grep -q '5.05'; "
                "test -f /usr/share/licenses/blazeos-control/PARDUS-GNOME-GREETER-LICENSE; "
                "test -f /usr/share/dnf5/repos.d/blaze.repo; "
                "test -f /etc/pki/rpm-gpg/RPM-GPG-KEY-blaze; "
                "rpm -q blaze-release >/dev/null; "
                "grep -qx 'PRETTY_NAME=\"Blaze SolarEvolution 5.05.1\"' /etc/os-release; "
                "grep -qx 'BLAZE_BASE_ID=fedora' /etc/os-release; "
                "grep -qx 'BLAZE_BASE_VERSION_ID=45' /etc/os-release; "
                "grep -qx 'Blaze SolarEvolution release 5.05.1 (SolarEvolution)' /etc/redhat-release; "
                "grep -qx 'Blaze SolarEvolution release 5.05.1 (SolarEvolution)' /etc/system-release; "
                "grep -qx 'GRUB_DISTRIBUTOR=\"Blaze SolarEvolution\"' /etc/default/grub; "
                "test -f /.buildstamp && grep -qx 'Product=Blaze SolarEvolution' /.buildstamp; "
                "LC_ALL=C dnf5 --dump-variables | grep -qx 'releasever = 45'; "
                "LC_ALL=C dnf5 --cacheonly repolist --enabled | grep -q '^blaze'; "
                "(LC_ALL=C dnf5 --cacheonly repoinfo blaze 2>/dev/null "
                "|| LC_ALL=C dnf5 --cacheonly repolist --enabled) | grep -qiE 'blaze|SolarEvolution'; "
                "grep -qE '^default_scheme\\s*=\\s*BTRFS' /etc/anaconda/conf.d/99-blazeos.conf; "
                "grep -q 'rhgb' /proc/cmdline; "
                "grep -q 'CDLABEL=Blaze-SE-5' /proc/cmdline; "
                "grep -qE '^Theme=blazeos' /etc/plymouth/plymouthd.conf; "
                "test -f /usr/lib64/plymouth/script.so; "
                "test -f /usr/share/plymouth/themes/blazeos/blazeos.plymouth; "
                "test -f /usr/share/plymouth/themes/blazeos/blazeos.script; "
                "test -f /usr/share/plymouth/themes/blazeos/logo.png; "
                "test -f /usr/share/plymouth/themes/blazeos/logo-glow.png; "
                "test -f /usr/share/solarui/noctalia-assets/noctalia.svg; "
                "test -f /usr/share/icons/hicolor/256x256/apps/solarui.png; "
                "test -d /usr/share/fonts/google-sans-flex; "
                "test -d /usr/share/fonts/TTF; "
                "test -d /usr/share/fonts/ttf-material-symbols-variable; "
                "test -d /usr/share/blazeos/wallpapers; "
                "! systemctl is-active --quiet audit-rules.service; "
                "! systemctl is-active --quiet virtqemud.service; "
                "echo PARITY_OK"
            )
            self.assertEqual(
                parity.exitcode,
                0,
                f"[{firmware}] In-guest parity failed:\n{parity.stdout}\n{parity.stderr}",
            )

            ply_log = vm.qga.run_shell("LC_ALL=C journalctl -b -u 'plymouth*' --no-pager || true")
            self.assertEqual(ply_log.exitcode, 0)
            ply_lower = ply_log.stdout.lower()
            for bad_tok in ("could not load", "failed to load", "script.so: cannot", "theme not found"):
                self.assertNotIn(
                    bad_tok,
                    ply_lower,
                    f"[{firmware}] Plymouth journal reported error {bad_tok!r}:\n{ply_log.stdout}",
                )

            rpm_res = vm.qga.run_shell("rpm -qa --qf '%{NAME}\\n'")
            self.assertEqual(rpm_res.exitcode, 0, rpm_res.stderr)
            guest_rpms = {line.strip() for line in rpm_res.stdout.splitlines() if line.strip()}
            self.assertGreater(len(guest_rpms), 1000)
            forbidden_present = sorted(set(FORBIDDEN_LIVE_RPMS) & guest_rpms)
            self.assertEqual(forbidden_present, [], f"[{firmware}] Forbidden RPMs present: {forbidden_present}")
            for pkg in guest_rpms:
                self.assertFalse(pkg.startswith("gnome-shell-extension-"), f"GNOME ext installed: {pkg}")
                self.assertFalse(pkg.startswith("libreoffice"), f"LibreOffice installed: {pkg}")
                self.assertFalse(pkg.startswith("libvirt"), f"Libvirt installed: {pkg}")

            sentinel_json = vm.qga.run_shell("blaze-sentinel status --json")
            self.assertEqual(sentinel_json.exitcode, 0, sentinel_json.stderr)
            sdata = json.loads(sentinel_json.stdout)
            self.assertEqual(sdata.get("total"), 7)

            # 6. Launch FireHub inside liveuser Wayland session & assert >8s survival + visual change
            firehub_res = vm.qga.run_shell(
                "set -e; "
                "uid=$(id -u liveuser); "
                "wdisp=$(ls /run/user/$uid/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
                "test -n \"$wdisp\"; "
                "! pgrep -u liveuser -f '/[o]pt/FireHub/firehub' >/dev/null 2>&1; "
                "rm -f /tmp/e2e_firehub.log; "
                "runuser -u liveuser -- bash -c "
                "\"export XDG_RUNTIME_DIR=/run/user/$uid WAYLAND_DISPLAY=$wdisp "
                "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus; "
                "nohup /usr/bin/firehub >/tmp/e2e_firehub.log 2>&1 </dev/null & echo \\$!\"; "
                "sleep 9; "
                "pgrep -u liveuser -f '/[o]pt/FireHub/firehub' >/dev/null && echo FIREHUB_ALIVE; "
                "cat /tmp/e2e_firehub.log || true",
                timeout=45.0,
            )
            self.assertEqual(
                firehub_res.exitcode,
                0,
                f"[{firmware}] FireHub failed to stay alive:\n{firehub_res.stdout}\n{firehub_res.stderr}",
            )
            self.assertIn("FIREHUB_ALIVE", firehub_res.stdout)
            for err_tok in ("error while loading shared libraries", "Segmentation fault", "Trace/breakpoint trap"):
                self.assertNotIn(err_tok, firehub_res.stdout, f"[{firmware}] FireHub log error: {firehub_res.stdout}")
            firehub_png = self._capture_settled_firehub_frame(
                vm, firmware, desktop_png, firehub_res.stdout, timeout=25.0
            )
            vm.qga.guest_exec(
                "/usr/bin/pkill",
                ["-u", "liveuser", "-f", "/[o]pt/FireHub/firehub"],
                timeout=15.0,
            )
            time.sleep(1.5)

            # 6b. Smoke-check blaze-house linking and launch inside liveuser Wayland session
            bh_res = vm.qga.run_shell(
                "set -e; "
                "! LC_ALL=C ldd /usr/bin/blaze-house | grep -q 'not found'; "
                "uid=$(id -u liveuser); "
                "wdisp=$(ls /run/user/$uid/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
                "test -n \"$wdisp\"; "
                "! pgrep -u liveuser -x blaze-house >/dev/null 2>&1; "
                "rm -f /tmp/e2e_blazehouse.log; "
                "runuser -u liveuser -- bash -c "
                "\"export XDG_RUNTIME_DIR=/run/user/$uid WAYLAND_DISPLAY=$wdisp "
                "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/$uid/bus "
                "XDG_SESSION_TYPE=wayland GDK_BACKEND=wayland,x11 "
                "WEBKIT_DISABLE_DMABUF_RENDERER=1; "
                "nohup /usr/bin/blaze-house >/tmp/e2e_blazehouse.log 2>&1 </dev/null & echo \\$!\"; "
                "sleep 4; "
                "pgrep -u liveuser -x blaze-house >/dev/null && echo BLAZEHOUSE_ALIVE; "
                "cat /tmp/e2e_blazehouse.log || true; "
                "pkill -u liveuser -x blaze-house || true",
                timeout=35.0,
            )
            self.assertEqual(
                bh_res.exitcode,
                0,
                f"[{firmware}] blaze-house smoke-check failed:\n{bh_res.stdout}\n{bh_res.stderr}",
            )
            self.assertIn("BLAZEHOUSE_ALIVE", bh_res.stdout)
            for err_tok in ("error while loading shared libraries", "Segmentation fault", "Trace/breakpoint trap"):
                self.assertNotIn(err_tok, bh_res.stdout, f"[{firmware}] blaze-house log error: {bh_res.stdout}")
            time.sleep(1.0)

            # 6c. Live We10X (Tanıdık Klasik Düzen) Shell Switch, Minimize/Restore, Surfaces & Rollback
            we10x_switch = vm.qga.run_shell(
                "set -e; "
                "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
                "wdisp=$(ls \"$rt\"/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
                "nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                "test -n \"$wdisp\" && test -S \"$nsock\"; "
                "runuser -u liveuser -- env HOME=/home/liveuser XDG_RUNTIME_DIR=\"$rt\" "
                "WAYLAND_DISPLAY=\"$wdisp\" DBUS_SESSION_BUS_ADDRESS=\"unix:path=$rt/bus\" "
                "NIRI_SOCKET=\"$nsock\" solar-shell switch we10x; "
                "sleep 2.0; "
                "pgrep -u liveuser -fa '[q]uickshell.*we10x' >/dev/null && echo WE10X_PROC_OK; "
                "grep -q 'SOLARUI-WE10X-PROFILE-ACTIVE' /home/liveuser/.config/solarui/niri.kdl && echo WE10X_KDL_OK; "
                "grep -q '#0078d7' /home/liveuser/.config/solarui/niri.kdl && echo WE10X_BORDER_OK; "
                "test -f /home/liveuser/.local/state/solarui/pre-we10x-snapshot.json && echo WE10X_SNAP_OK; "
                "st=$(runuser -u liveuser -- env HOME=/home/liveuser XDG_RUNTIME_DIR=\"$rt\" "
                "WAYLAND_DISPLAY=\"$wdisp\" NIRI_SOCKET=\"$nsock\" solar-shell status); "
                "echo \"$st\"; "
                "echo \"$st\" | grep -q 'Actual Shell Engine:  we10x' && echo WE10X_RECONCILED_OK",
                timeout=60.0,
            )
            self.assertEqual(
                we10x_switch.exitcode,
                0,
                f"[{firmware}] Step 6c switch to we10x failed:\n{we10x_switch.stdout}\n{we10x_switch.stderr}",
            )
            for marker in ("WE10X_PROC_OK", "WE10X_KDL_OK", "WE10X_BORDER_OK", "WE10X_SNAP_OK", "WE10X_RECONCILED_OK"):
                self.assertIn(marker, we10x_switch.stdout)

            time.sleep(1.5)
            we10x_desktop_png = vm.capture_png(f"04b_we10x_desktop_{firmware}")
            self.assertTrue(
                is_non_black_content(we10x_desktop_png),
                f"[{firmware}] We10X desktop screenshot is black",
            )
            self.assertGreater(
                frame_diff_ratio(desktop_png, we10x_desktop_png),
                0.05,
                f"[{firmware}] We10X desktop did not visually change from Noctalia desktop",
            )

            # Spawn floating Kitty (server-side 30px Win10 titlebar) & Nautilus (GTK4 CSD + We10X yellow folders),
            # verify #0078D7 border + 30px server titlebar, test Win+D show-desktop park/restore & single minimize.
            we10x_win_res = vm.qga.run_shell(
                "set -e; "
                "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
                "wdisp=$(ls \"$rt\"/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
                "nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                "runuser -u liveuser -- bash -c "
                "\"export HOME=/home/liveuser XDG_RUNTIME_DIR=$rt WAYLAND_DISPLAY=$wdisp "
                "DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus NIRI_SOCKET=$nsock; "
                "nohup kitty >/tmp/e2e_we10x_kitty.log 2>&1 </dev/null & "
                "nohup nautilus --new-window /home/liveuser >/tmp/e2e_we10x_nautilus.log 2>&1 </dev/null &\"; "
                "for _ in $(seq 1 20); do "
                "  cnt=$(runuser -u liveuser -- env XDG_RUNTIME_DIR=\"$rt\" NIRI_SOCKET=\"$nsock\" niri msg --json windows | "
                "    python3 -c 'import json,sys; ws=json.load(sys.stdin); print(sum(1 for w in ws if (w.get(\"app_id\") or \"\").lower() in (\"kitty\",\"org.gnome.nautilus\")))'); "
                "  [ \"$cnt\" -ge 2 ] && break; "
                "  sleep 0.5; "
                "done; "
                "sleep 1.0; "
                "runuser -u liveuser -- env XDG_RUNTIME_DIR=\"$rt\" NIRI_SOCKET=\"$nsock\" niri msg --json windows",
                timeout=45.0,
            )
            self.assertEqual(we10x_win_res.exitcode, 0, f"[{firmware}] We10X window spawn failed:\n{we10x_win_res.stderr}")
            we10x_windows = json.loads(we10x_win_res.stdout.strip().splitlines()[-1])
            kitty_win = next((w for w in we10x_windows if (w.get("app_id") or "").lower() == "kitty"), None)
            naut_win = next((w for w in we10x_windows if "nautilus" in (w.get("app_id") or "").lower()), None)
            self.assertIsNotNone(kitty_win, f"[{firmware}] Kitty window missing in We10X: {we10x_windows}")
            self.assertIsNotNone(naut_win, f"[{firmware}] Nautilus window missing in We10X: {we10x_windows}")
            # Kitty has server-titlebar true (30px) + 1px border*2 => tile_size[1] - window_size[1] >= 30
            kitty_tile_h = float(kitty_win["layout"]["tile_size"][1])
            kitty_win_h = float(kitty_win["layout"]["window_size"][1])
            self.assertGreaterEqual(
                kitty_tile_h - kitty_win_h,
                30.0,
                f"[{firmware}] Expected >=30px Windows 10 server titlebar on Kitty: {kitty_win}",
            )

            vm.qga.run_shell(
                "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
                "nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                f"runuser -u liveuser -- env XDG_RUNTIME_DIR=\"$rt\" NIRI_SOCKET=\"$nsock\" niri msg action move-floating-window --id {kitty_win['id']} -x 130 -y 36; "
                f"runuser -u liveuser -- env XDG_RUNTIME_DIR=\"$rt\" NIRI_SOCKET=\"$nsock\" niri msg action move-floating-window --id {naut_win['id']} -x 290 -y 110",
                timeout=15.0,
            )
            time.sleep(0.5)
            we10x_windows_png = vm.capture_png(f"04c_we10x_windows_{firmware}")
            self.assertTrue(is_non_black_content(we10x_windows_png))

            # Query active Wayland output logical dimensions so QMP tablet coordinates map 1:1
            out_dim_res = vm.qga.run_shell(
                "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
                "nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                "runuser -u liveuser -- env XDG_RUNTIME_DIR=\"$rt\" NIRI_SOCKET=\"$nsock\" niri msg --json focused-output",
                timeout=15.0,
            )
            out_w, out_h = 1280, 800
            if out_dim_res.exitcode == 0 and out_dim_res.stdout.strip():
                out_json = json.loads(out_dim_res.stdout.strip().splitlines()[-1])
                logical = (out_json or {}).get("logical") or {}
                out_w = int(logical.get("width") or 1280)
                out_h = int(logical.get("height") or 800)

            # Tap lone hardware Super key via QMP -> must trigger super-tap-spawn and open We10X Start Menu!
            vm.qmp.send_key("meta_l", hold_secs=0.06)
            time.sleep(1.3)
            we10x_start_png = vm.capture_png(f"04d_we10x_startmenu_{firmware}")
            self.assertGreater(
                frame_diff_ratio(we10x_windows_png, we10x_start_png),
                0.03,
                f"[{firmware}] We10X Start Menu did not visually appear after lone Super key tap",
            )

            # Click outside Start Menu onto upper-right area via QMP -> backdrop must dismiss Start Menu!
            vm.qmp.click(out_w - 60, max(80, out_h // 3), width=out_w, height=out_h)
            time.sleep(0.8)
            after_outside_click_png = vm.capture_png(f"04d2_we10x_startmenu_dismissed_{firmware}")
            self.assertGreater(
                frame_diff_ratio(we10x_start_png, after_outside_click_png),
                0.03,
                f"[{firmware}] Clicking outside Start Menu did not dismiss it",
            )

            # Toggle Action Center & capture screenshot, then dismiss via outside click
            vm.qga.run_shell(
                "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
                "wdisp=$(ls \"$rt\"/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
                "nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                "runuser -u liveuser -- env HOME=/home/liveuser XDG_RUNTIME_DIR=\"$rt\" "
                "WAYLAND_DISPLAY=\"$wdisp\" NIRI_SOCKET=\"$nsock\" solar-shell route control-center",
                timeout=15.0,
            )
            time.sleep(1.0)
            we10x_ac_png = vm.capture_png(f"04e_we10x_actioncenter_{firmware}")
            self.assertGreater(
                frame_diff_ratio(we10x_windows_png, we10x_ac_png),
                0.03,
                f"[{firmware}] We10X Action Center did not visually appear",
            )
            vm.qmp.click(120, max(80, out_h // 3), width=out_w, height=out_h)
            time.sleep(0.6)

            # Verify Win+Left & Win+Right Aero Snap percentage geometry on Kitty, then click Kitty's
            # 30px server-side `─` (Minimize) caption button via QMP pointer!
            kid = int(kitty_win["id"])
            snap_res = vm.qga.run_shell(
                "set -e; "
                "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
                "wdisp=$(ls \"$rt\"/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
                "nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                "run_as=\"runuser -u liveuser -- env HOME=/home/liveuser XDG_RUNTIME_DIR=$rt WAYLAND_DISPLAY=$wdisp DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus NIRI_SOCKET=$nsock\"; "
                f"$run_as niri msg action focus-window --id {kid}; "
                "$run_as solar-shell aero-snap left; sleep 0.6; "
                f"left_geom=$($run_as niri msg --json windows | python3 -c 'import json,sys; w=next(x for x in json.load(sys.stdin) if x[\"id\"]=={kid}); print(json.dumps({{\"pos\":w[\"layout\"][\"tile_pos_in_workspace_view\"],\"tile\":w[\"layout\"][\"tile_size\"]}}))'); "
                "echo \"SNAP_LEFT=$left_geom\"; "
                "$run_as solar-shell aero-snap right; sleep 0.6; "
                f"right_geom=$($run_as niri msg --json windows | python3 -c 'import json,sys; w=next(x for x in json.load(sys.stdin) if x[\"id\"]=={kid}); print(json.dumps({{\"pos\":w[\"layout\"][\"tile_pos_in_workspace_view\"],\"tile\":w[\"layout\"][\"tile_size\"]}}))'); "
                "echo \"SNAP_RIGHT=$right_geom\"",
                timeout=30.0,
            )
            self.assertEqual(snap_res.exitcode, 0, f"[{firmware}] Aero Snap failed:\n{snap_res.stdout}\n{snap_res.stderr}")
            snap_left_line = next(l.split("=", 1)[1] for l in snap_res.stdout.splitlines() if l.startswith("SNAP_LEFT="))
            snap_right_line = next(l.split("=", 1)[1] for l in snap_res.stdout.splitlines() if l.startswith("SNAP_RIGHT="))
            snap_left = json.loads(snap_left_line)
            snap_right = json.loads(snap_right_line)
            self.assertGreaterEqual(snap_left["tile"][0], out_w * 0.45, f"[{firmware}] Left Aero Snap shrunk window instead of 50%: {snap_left}")
            self.assertLessEqual(snap_left["pos"][0], 8.0, f"[{firmware}] Left Aero Snap x!=0: {snap_left}")
            self.assertGreaterEqual(snap_right["tile"][0], out_w * 0.45, f"[{firmware}] Right Aero Snap shrunk window instead of 50%: {snap_right}")
            self.assertGreaterEqual(snap_right["pos"][0], out_w * 0.45, f"[{firmware}] Right Aero Snap x<45%: {snap_right}")
            self.assertLess(snap_right["pos"][0], out_w * 0.60, f"[{firmware}] Right Aero Snap pushed window off-screen: {snap_right}")

            # Click Kitty's 30px server-side `─` (Minimize) button via QMP tablet click!
            # Rightmost 3 buttons on right-snapped Kitty: Close [w-46..w], Maximize [w-92..w-46], Minimize [w-138..w-92]
            kitty_right_edge = int(round(snap_right["pos"][0] + snap_right["tile"][0]))
            kitty_top_edge = int(round(snap_right["pos"][1]))
            min_btn_x = max(10, min(out_w - 10, kitty_right_edge - 115))
            min_btn_y = max(5, kitty_top_edge + 15)
            vm.qmp.click(min_btn_x, min_btn_y, width=out_w, height=out_h)
            time.sleep(0.8)

            # Verify QMP titlebar Minimize click parked Kitty onto "background", then test Win+D & Noctalia rollback + GTK hygiene!
            min_restore_res = vm.qga.run_shell(
                "set -e; "
                "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
                "wdisp=$(ls \"$rt\"/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
                "nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                "run_as=\"runuser -u liveuser -- env HOME=/home/liveuser XDG_RUNTIME_DIR=$rt WAYLAND_DISPLAY=$wdisp DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus NIRI_SOCKET=$nsock\"; "
                "bg_id=$($run_as niri msg --json workspaces | python3 -c 'import json,sys; print(next(w[\"id\"] for w in json.load(sys.stdin) if w.get(\"name\")==\"background\"))'); "
                f"kitty_ws=$($run_as niri msg --json windows | python3 -c 'import json,sys; print(next(w[\"workspace_id\"] for w in json.load(sys.stdin) if w[\"id\"]=={kid}))'); "
                "echo \"KITTY_WS_AFTER_TITLEBAR_MIN_CLICK=$kitty_ws BG=$bg_id\"; "
                "[ \"$kitty_ws\" -eq \"$bg_id\" ]; "
                "$run_as solar-shell show-desktop; "
                "sleep 0.8; "
                "parked=$($run_as niri msg --json windows | python3 -c \"import json,sys; print(sum(1 for w in json.load(sys.stdin) if w.get('workspace_id')==$bg_id))\"); "
                "echo \"PARKED_AFTER_SHOW_DESKTOP=$parked\"; "
                "[ \"$parked\" -ge 2 ]; "
                "$run_as solar-shell show-desktop; "
                "sleep 0.8; "
                "parked2=$($run_as niri msg --json windows | python3 -c \"import json,sys; print(sum(1 for w in json.load(sys.stdin) if w.get('workspace_id')==$bg_id))\"); "
                "echo \"PARKED_AFTER_RESTORE=$parked2\"; "
                "[ \"$parked2\" -eq 1 ]; "
                "$run_as solar-shell switch noctalia; "
                "sleep 2.0; "
                "parked_after_noctalia=$($run_as niri msg --json windows | python3 -c \"import json,sys; print(sum(1 for w in json.load(sys.stdin) if w.get('workspace_id')==$bg_id))\"); "
                "echo \"PARKED_AFTER_NOCTALIA=$parked_after_noctalia\"; "
                "[ \"$parked_after_noctalia\" -eq 0 ]; "
                "! pgrep -u liveuser -fa '[q]uickshell.*we10x' >/dev/null; "
                "! grep -q 'SOLARUI-WE10X-PROFILE-ACTIVE' /home/liveuser/.config/solarui/niri.kdl; "
                "test ! -e /home/liveuser/.local/state/solarui/pre-we10x-snapshot.json; "
                "! grep -qs 'gtk-icon-theme-name=We10X' /home/liveuser/.config/gtk-4.0/settings.ini; "
                "! grep -qs 'SOLARUI-WE10X-GTK-CSS' /home/liveuser/.config/gtk-4.0/gtk.css; "
                "kitty_post=$($run_as niri msg --json windows | python3 -c 'import json,sys; print(next(w[\"id\"] for w in json.load(sys.stdin) if (w.get(\"app_id\") or \"\").lower()==\"kitty\"))'); "
                "$run_as solar-shell minimize \"$kitty_post\"; "
                "sleep 0.6; "
                "kitty_ws_noct=$($run_as niri msg --json windows | python3 -c \"import json,sys; print(next(w['workspace_id'] for w in json.load(sys.stdin) if w['id']==$kitty_post))\"); "
                "[ \"$kitty_ws_noct\" != \"$bg_id\" ]; "
                "[ \"$(cat /home/liveuser/.config/solarui/niri.kdl | grep -c 'allow-minimize true' || true)\" -eq 0 ]; "
                "pkill -u liveuser -x kitty || true; "
                "pkill -u liveuser -x nautilus || true; "
                "echo WE10X_ROUNDTRIP_OK",
                timeout=60.0,
            )
            self.assertEqual(
                min_restore_res.exitcode,
                0,
                f"[{firmware}] We10X minimize/restore/Noctalia rollback check failed:\n"
                f"{min_restore_res.stdout}\n{min_restore_res.stderr}",
            )
            self.assertIn("WE10X_ROUNDTRIP_OK", min_restore_res.stdout)
            time.sleep(1.5)

            # 7. Launch Anaconda (liveinst) inside liveuser's local logind/Wayland session & verify GNOME + BTRFS
            pre_procs = self._find_installer_procs(vm)
            self.assertEqual(
                pre_procs,
                [],
                f"[{firmware}] Installer unexpectedly running before Step 7: {pre_procs}\n"
                f"{self._collect_installer_logs(vm)}",
            )
            self.assertTrue(
                self._is_liveinst_lock_free(vm),
                f"[{firmware}] /run/anaconda/liveinst.lock held before Step 7\n"
                f"{self._collect_installer_logs(vm)}",
            )

            launch_res = vm.qga.run_shell(
                "set -e; "
                "uid=$(id -u liveuser); "
                "rt=\"/run/user/$uid\"; "
                "wdisp=$(ls \"$rt\"/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
                "test -n \"$wdisp\"; "
                "nsock=$(runuser -u liveuser -- bash -c "
                "\"export XDG_RUNTIME_DIR=$rt DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus; "
                "systemctl --user show-environment 2>/dev/null | sed -n 's/^NIRI_SOCKET=//p' | head -n1\"); "
                "if [ -z \"$nsock\" ] || [ ! -S \"$nsock\" ]; then "
                "  nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
                "fi; "
                "test -n \"$nsock\" && test -S \"$nsock\"; "
                "rm -f /tmp/e2e_liveinst.log; "
                "runuser -u liveuser -- bash -c "
                "\"export XDG_RUNTIME_DIR=$rt WAYLAND_DISPLAY=$wdisp "
                "DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus NIRI_SOCKET=$nsock; "
                "niri msg action spawn -- /bin/sh -c 'exec /usr/bin/liveinst >/tmp/e2e_liveinst.log 2>&1'\"",
                timeout=30.0,
            )
            self.assertEqual(
                launch_res.exitcode,
                0,
                f"[{firmware}] Failed to spawn liveinst inside liveuser Niri session:\n"
                f"{launch_res.stdout}\n{launch_res.stderr}\n{self._collect_installer_logs(vm)}",
            )

            # Gate 1: /run/anaconda/backend_ready + Cockpit HTTP 200 + live UI process
            self._wait_for_anaconda_backend_ready(vm, firmware, timeout=50.0)
            logs_diag = self._collect_installer_logs(vm)

            ps_ana = vm.qga.run_shell("ps -eo pid=,label=,comm=", timeout=15.0)
            self.assertEqual(ps_ana.exitcode, 0, ps_ana.stderr)
            matched_installer_procs: list[tuple[str, str, str]] = []
            for line in ps_ana.stdout.splitlines():
                parts = line.split(None, 2)
                if len(parts) < 3:
                    continue
                pid, label, comm = parts
                comm_lower = comm.lower()
                if any(tok in comm_lower for tok in ("liveinst", "anaconda", "firefox")):
                    matched_installer_procs.append((pid, label, comm))
                    self.assertNotIn(
                        ":xdm_t:",
                        label,
                        f"[{firmware}] Installer process trapped in xdm_t: pid={pid} label={label} comm={comm}\n{logs_diag}",
                    )
            self.assertTrue(
                bool(matched_installer_procs),
                f"[{firmware}] Expected running liveinst/anaconda/firefox process in ps output:\n{ps_ana.stdout}\n{logs_diag}",
            )

            ana_cfg_res = vm.qga.run_shell(
                "set -e; "
                "grep -qE '^default_scheme\\s*=\\s*BTRFS' /etc/anaconda/conf.d/99-blazeos.conf; "
                "gzip -dc /usr/share/cockpit/anaconda-webui/index.js.gz | grep -Fq '\"solarui\",\"gnome\"' "
                "  && gzip -dc /usr/share/cockpit/anaconda-webui/index.js.gz | grep -Fq 'desktopNames' "
                "  && gzip -dc /usr/share/cockpit/anaconda-webui/index.js.gz | grep -Fq 'gnome:\"GNOME\"' "
                "  && echo GNOME_OPTION_OK",
                timeout=30.0,
            )
            self.assertEqual(
                ana_cfg_res.exitcode,
                0,
                f"[{firmware}] Anaconda config/WebUI check failed:\n"
                f"{ana_cfg_res.stdout}\n{ana_cfg_res.stderr}\n{logs_diag}",
            )
            self.assertIn("GNOME_OPTION_OK", ana_cfg_res.stdout)

            liveinst_log = self._read_guest_file(vm, "/tmp/e2e_liveinst.log")
            for bad_ana in (
                "Traceback (most recent call last):",
                "SourceSetupError",
                "error while loading shared libraries",
                "Authorization failed",
                "Permission denied",
                "Installer already running",
            ):
                self.assertNotIn(
                    bad_ana,
                    liveinst_log,
                    f"[{firmware}] liveinst error {bad_ana!r}:\n{logs_diag}",
                )

            # Gate 2: Niri window focus, genuine floating foreground (>=900x550), and non-error title
            self._verify_anaconda_niri_window(vm, firmware, logs_diag, timeout=25.0)

            # Gates 3 & 4: Visual UI region verification inside floating liveinst & 2-frame settle
            self._capture_settled_anaconda_frame(vm, firmware, desktop_png, logs_diag, timeout=25.0)

            # 8. Assert zero coredumps & zero failed systemd units
            cd_res = vm.qga.run_shell("LC_ALL=C coredumpctl list --no-pager --no-legend 2>&1 || true")
            cd_out = cd_res.stdout.strip()
            self.assertTrue(
                cd_out == "" or "No coredumps found" in cd_out,
                f"[{firmware}] Unexpected coredumps in live session:\n{cd_out}",
            )
            failed_res = vm.qga.run_shell("LC_ALL=C systemctl --failed --no-legend --no-pager")
            self.assertEqual(failed_res.exitcode, 0, failed_res.stderr)
            self.assertEqual(
                failed_res.stdout.strip(),
                "",
                f"[{firmware}] Failed systemd units in live session:\n{failed_res.stdout}",
            )

        # 9. Post-VM read-only reference immutability verification
        for ref_path, expected_sha in IMMUTABLE_BASELINE_SHA256.items():
            self.assertEqual(sha256_file(ref_path), expected_sha)

    def test_live_boot_legacy_bios(self):
        if get_firmware_mode() == "uefi":
            self.skipTest("Skipped when BLAZE_QEMU_FIRMWARE=uefi")
        self._run_full_boot_flow("bios")

    def test_live_boot_uefi_ovmf(self):
        if get_firmware_mode() == "bios":
            self.skipTest("Skipped when BLAZE_QEMU_FIRMWARE=bios")
        self._run_full_boot_flow("uefi")


if __name__ == "__main__":
    unittest.main()
