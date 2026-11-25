"""BERE / BERP Fault-Injection Resilience & Recovery Benchmark Suite (F1-F7).

Runs real QEMU/KVM OVMF virtual machines against overlay snapshots of an
installed Blaze SolarEvolution GPT system with Full Armored Vault (BLAZE_REC),
captures visual screenshots, measures concrete recovery timings, calculates
a /100 Resilience Score, and writes `docs/BERE-BENCHMARKS.md`.

Usage:
    python3 -m tests.e2e.test_bere_benchmark
"""
from __future__ import annotations

import argparse
from dataclasses import dataclass, asdict
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from typing import Any, Optional

from PIL import Image

from tests.e2e.config import (
    OVMF_CODE_PATH,
    OVMF_VARS_PATH,
    REPO_ROOT,
    get_artifacts_dir,
    get_target_iso_path,
)
from tests.e2e.harness.imaging import (
    convert_ppm_to_png,
    frame_diff_ratio,
    is_non_black_content,
)
from tests.e2e.harness.qga import QgaClient, QgaError
from tests.e2e.harness.qmp import QmpClient, QmpError


BENCH_USER = "blazeuser"
BENCH_USER_PW = "BlazeUser2026"
BENCH_ROOT_PW = "BlazeRoot2026"


@dataclass
class ScenarioResult:
    code: str
    title: str
    max_points: int
    points: int
    passed: bool
    elapsed_s: float
    details: str
    screenshot: str = ""


class _BenchQemuVM:
    """Context-managed QEMU OVMF VM supporting installed disks, overlays, and optional ISO."""

    def __init__(
        self,
        *,
        disk_path: Path,
        ovmf_vars_path: Path,
        artifacts_dir: Path,
        vm_label: str,
        iso_path: Optional[Path] = None,
        boot_order: str = "c",
        memory_mb: int = 6144,
        cpus: int = 4,
    ) -> None:
        self.disk_path = Path(disk_path)
        self.ovmf_vars_path = Path(ovmf_vars_path)
        self.iso_path = Path(iso_path) if iso_path else None
        self.boot_order = boot_order
        self.memory_mb = memory_mb
        self.cpus = cpus
        self.vm_label = vm_label
        self.artifacts_dir = Path(artifacts_dir)
        self.artifacts_dir.mkdir(parents=True, exist_ok=True)

        self.run_dir = Path(tempfile.mkdtemp(prefix=f"blaze_bere_{vm_label}_{os.getpid()}_"))
        self.qmp_sock = self.run_dir / "qmp.sock"
        self.qga_sock = self.run_dir / "qga.sock"
        self.serial_log = self.artifacts_dir / f"{vm_label}_serial.log"
        self.stderr_log = self.artifacts_dir / f"{vm_label}_qemu_stderr.log"

        self.proc: Optional[subprocess.Popen] = None
        self._stderr_fh: Optional[Any] = None
        self.qmp = QmpClient(str(self.qmp_sock))
        self.qga = QgaClient(str(self.qga_sock))

    def build_command(self) -> list[str]:
        if not OVMF_CODE_PATH.is_file() or not self.ovmf_vars_path.is_file():
            raise FileNotFoundError(f"Missing OVMF firmware/vars: {OVMF_CODE_PATH}, {self.ovmf_vars_path}")
        cmd = [
            "qemu-system-x86_64",
            "-name",
            f"{self.vm_label}-{os.getpid()}",
            "-cpu",
            "host",
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
        if self.iso_path is not None:
            cmd.extend(["-cdrom", str(self.iso_path), "-boot", f"order={self.boot_order}"])
        else:
            cmd.extend(["-boot", f"order={self.boot_order}"])

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
                "user,id=net0",
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
        self.qmp.connect(wait_timeout=20.0, proc=self.proc)

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

    def __enter__(self) -> "_BenchQemuVM":
        self.start()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        self.cleanup()


def _create_overlay_pair(
    base_disk: Path, base_vars: Path, out_dir: Path, label: str
) -> tuple[Path, Path]:
    ovl = out_dir / f"{label}_disk.qcow2"
    vrs = out_dir / f"{label}_OVMF_VARS.fd"
    ovl.unlink(missing_ok=True)
    subprocess.run(
        ["qemu-img", "create", "-f", "qcow2", "-b", str(base_disk.resolve()), "-F", "qcow2", str(ovl)],
        check=True,
        capture_output=True,
        text=True,
    )
    shutil.copyfile(base_vars, vrs)
    return ovl, vrs


def _has_red_death_banner(png_path: Path) -> bool:
    if not png_path.is_file():
        return False
    with Image.open(png_path) as im:
        rgb = im.convert("RGB")
        w, h = rgb.size
        px = rgb.load()
        red_hits = 0
        samples = 0
        for y in range(min(180, h)):
            if y % 3 != 0:
                continue
            for x in range(0, w, 6):
                samples += 1
                r, g, b = px[x, y]
                if r >= 145 and g <= 70 and b <= 70:
                    red_hits += 1
        return samples > 0 and (red_hits / samples) >= 0.03


def _get_boot_id(vm: _BenchQemuVM) -> str:
    res = vm.qga.run_shell("cat /proc/sys/kernel/random/boot_id", timeout=10.0)
    return res.stdout.strip()


def _wait_qga_reconnect(
    vm: _BenchQemuVM, prev_boot_id: Optional[str] = None, timeout: float = 210.0
) -> float:
    t0 = time.monotonic()
    vm.qga.close()
    deadline = t0 + timeout
    while time.monotonic() < deadline:
        try:
            vm.qga.wait_until_ready(
                timeout=min(12.0, max(2.0, deadline - time.monotonic())), poll_interval=1.0
            )
            if prev_boot_id:
                new_id = _get_boot_id(vm)
                if not new_id or new_id == prev_boot_id:
                    vm.qga.close()
                    time.sleep(1.5)
                    continue
            return time.monotonic() - t0
        except Exception:
            vm.qga.close()
            time.sleep(1.5)
    raise TimeoutError(f"QGA did not reconnect within {timeout:.0f}s")


def _wait_bere_gui_ready(vm: _BenchQemuVM, timeout: float = 120.0) -> None:
    deadline = time.monotonic() + timeout
    last = ""
    while time.monotonic() < deadline:
        try:
            res = vm.qga.run_shell(
                "systemctl is-active blaze-recovery-gui.service || true; "
                "ss -ltn | grep -q ':49223' && echo PORT_49223_OK || true; "
                "pgrep -af '[b]erp-recovery-gui|[n]iri|[c]age|[w]eston' || true",
                timeout=12.0,
            )
            last = res.stdout
            if "PORT_49223_OK" in last and "berp-recovery-gui" in last:
                time.sleep(2.5)
                return
        except Exception as e:
            last = str(e)
        time.sleep(2.0)
    raise TimeoutError(f"BERE GUI did not become ready within {timeout:.0f}s: {last}")


def _bere_api_script(vm: _BenchQemuVM, py_body: str, timeout: float = 40.0) -> dict[str, Any]:
    cmd = f"python3 - <<'PY'\n{py_body}\nPY"
    res = vm.qga.run_shell(cmd, timeout=timeout)
    if res.exitcode != 0:
        raise RuntimeError(f"In-guest BERE API helper failed (rc={res.exitcode}): {res.stdout}\n{res.stderr}")
    lines = [ln.strip() for ln in res.stdout.splitlines() if ln.strip().startswith("{")]
    if not lines:
        raise RuntimeError(f"Expected JSON output from BERE API helper, got:\n{res.stdout}\n{res.stderr}")
    return json.loads(lines[-1])


def _prepare_base_installed_disk(
    iso_path: Path,
    work_dir: Path,
    artifacts_dir: Path,
    target_disk: Optional[Path] = None,
    target_vars: Optional[Path] = None,
) -> tuple[Path, Path]:
    """Install Blaze SolarEvolution onto a 30G UEFI GPT QCOW2 disk with Full BLAZE_REC Vault."""
    base_disk = target_disk if target_disk else (work_dir / "base_installed.qcow2")
    base_vars = target_vars if target_vars else (work_dir / "base_OVMF_VARS.fd")
    base_disk.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(OVMF_VARS_PATH, base_vars)
    subprocess.run(
        ["qemu-img", "create", "-f", "qcow2", str(base_disk), "30G"],
        check=True,
        capture_output=True,
        text=True,
    )

    print("[BENCH-PREP] Booting Live ISO in UEFI OVMF to build reference installed disk...")
    with _BenchQemuVM(
        disk_path=base_disk,
        ovmf_vars_path=base_vars,
        iso_path=iso_path,
        boot_order="d",
        memory_mb=6144,
        cpus=4,
        artifacts_dir=artifacts_dir,
        vm_label="prep_install",
    ) as vm:
        # Advance past GRUB
        for _ in range(25):
            time.sleep(1.0)
            png = vm.capture_png("00_prep_grub")
            if is_non_black_content(png, min_non_black_ratio=0.005, min_std_luma=1.0):
                break
        vm.qmp.send_key("ret")
        vm.qga.wait_until_ready(timeout=210.0, poll_interval=2.0)

        # Wait for SolarUI Live session and check Step 2 Anaconda WebUI visually first
        time.sleep(8.0)
        ui_res = vm.qga.run_shell(
            "set -e; "
            "uid=$(id -u liveuser); rt=\"/run/user/$uid\"; "
            "wdisp=$(ls \"$rt\"/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
            "nsock=$(ls \"$rt\"/niri*.sock 2>/dev/null | head -n1 || true); "
            "runuser -u liveuser -- bash -c "
            "\"export XDG_RUNTIME_DIR=$rt WAYLAND_DISPLAY=$wdisp DBUS_SESSION_BUS_ADDRESS=unix:path=$rt/bus NIRI_SOCKET=$nsock; "
            "niri msg action spawn -- /bin/sh -c 'exec /usr/bin/liveinst >/tmp/bench_liveinst.log 2>&1'\"; "
            "for _ in $(seq 1 30); do curl -sS -o /dev/null http://127.0.0.1:9090 && [ -e /run/anaconda/backend_ready ] && break; sleep 1.5; done; "
            "sleep 4",
            timeout=90.0,
        )
        if ui_res.exitcode != 0:
            raise RuntimeError(f"Failed starting liveinst for Step 2 check: {ui_res.stderr}")
        vm.capture_png("00_anaconda_step1_welcome")
        # Advance Step 1 (click 'Sonraki' button at 531, 698) -> Step 2 System Preferences
        vm.qmp.click(531, 698)
        time.sleep(3.5)
        vm.capture_png("00_anaconda_step2_top")
        # Focus inside the scrollable Step 2 div ('Kullanım profili' label at 415, 502) and Tab down to BLAZE_REC Vault cards
        vm.qmp.click(415, 502)
        time.sleep(0.3)
        for _ in range(8):
            vm.qmp.send_key("tab")
            time.sleep(0.15)
        vm.qmp.send_key("end")
        time.sleep(1.5)
        vm.capture_png("00_anaconda_step2_vault_ui")

        # Shut down GUI liveinst cleanly before running unattended kickstart install on /dev/vda
        vm.qga.guest_exec("/usr/bin/pkill", ["-f", "anaconda|webui-desktop|firefox|liveinst"], timeout=15.0)
        time.sleep(2.0)
        vm.qga.run_shell("anaconda-cleanup || true", timeout=30.0)

        ks_content = f"""lang tr_TR.UTF-8
keyboard trq
timezone Europe/Istanbul --utc
liveimg --url=file:///run/initramfs/live/LiveOS/squashfs.img
ignoredisk --only-use=vda
clearpart --all --initlabel --drives=vda
zerombr
bootloader --timeout=3
autopart --type=btrfs
rootpw --plaintext "{BENCH_ROOT_PW}"
user --name={BENCH_USER} --groups=wheel --password="{BENCH_USER_PW}" --plaintext
shutdown
"""
        start_install_script = f"""set -euo pipefail
cat > /tmp/bere_bench.ks <<'KS'
{ks_content}KS
rm -f /tmp/bench_ks.rc /tmp/bench_ks_install.log
printf 'solarui' > /tmp/anaconda_desktop_choice
printf 'grub2' > /tmp/anaconda_bootloader_choice
printf 'full' > /tmp/anaconda_bere_vault_mode
printf 'minimal' > /tmp/anaconda_desktop_profile
printf 'standard' > /tmp/anaconda_usage_profile
printf 'no' > /tmp/anaconda_remove_solarui

cat > /tmp/run_ks_bench.sh <<'SH'
#!/bin/bash
export ANACONDA_PRODUCTNAME="Blaze SolarEvolution"
export ANACONDA_PRODUCTVERSION="5.05.1"
export ANACONDA_ISFINAL="true"
export PATH=/sbin:/usr/sbin:/bin:/usr/bin
for i in dm-mod vfat btrfs ext4 xfs loop squashfs overlay; do modprobe "$i" 2>/dev/null || true; done
setenforce 0 || true
anaconda-cleanup || true
LC_ALL=C.UTF-8 dbus-run-session -- anaconda --cmdline --kickstart=/tmp/bere_bench.ks >/tmp/bench_ks_install.log 2>&1
rc=$?
echo "$rc" > /tmp/bench_ks.rc
sync
SH
chmod 0755 /tmp/run_ks_bench.sh
nohup /tmp/run_ks_bench.sh >/dev/null 2>&1 </dev/null &
echo KS_STARTED
"""
        print("[BENCH-PREP] Running Anaconda kickstart install (Full BLAZE_REC Vault + shutdown)...")
        start_res = vm.qga.run_shell(start_install_script, timeout=30.0)
        if start_res.exitcode != 0 or "KS_STARTED" not in start_res.stdout:
            raise RuntimeError(f"Failed to start kickstart installer: {start_res.stdout}\n{start_res.stderr}")

        ks_rc: Optional[str] = None
        powered_off = False
        ks_deadline = time.monotonic() + 900.0
        while time.monotonic() < ks_deadline:
            if vm.proc is not None and vm.proc.poll() is not None:
                powered_off = True
                break
            time.sleep(5.0)
            if vm.proc is not None and vm.proc.poll() is not None:
                powered_off = True
                break
            try:
                poll = vm.qga.run_shell(
                    "if [ -f /tmp/bench_ks.rc ]; then echo RC=$(cat /tmp/bench_ks.rc); lsblk -o NAME,LABEL,SIZE,FSTYPE,PARTFLAGS /dev/vda; else echo RUNNING; fi",
                    timeout=15.0,
                )
                if "RC=" in poll.stdout:
                    ks_rc = poll.stdout
                    break
            except Exception:
                vm.qga.close()
                if vm.proc is not None and vm.proc.poll() is not None:
                    powered_off = True
                    break
                try:
                    vm.qga.wait_until_ready(timeout=15.0, poll_interval=1.0)
                except Exception:
                    pass

        if not powered_off:
            if ks_rc is None:
                log_tail = vm.qga.run_shell("tail -n 60 /tmp/bench_ks_install.log /tmp/anaconda.log /tmp/storage.log || true", timeout=20.0)
                raise RuntimeError(f"Kickstart installation timed out after 900s:\n{log_tail.stdout}")
            if "RC=0" not in ks_rc:
                log_tail = vm.qga.run_shell("tail -n 80 /tmp/bench_ks_install.log /tmp/anaconda.log /tmp/storage.log || true", timeout=20.0)
                raise RuntimeError(f"Kickstart installation failed ({ks_rc}):\n{log_tail.stdout}")
            print("[BENCH-PREP] Partition layout after install:\n" + ks_rc)
            vm.qga.run_shell("(sleep 1; sync; poweroff) >/dev/null 2>&1 </dev/null &", timeout=15.0)
            time.sleep(8.0)
        else:
            print("[BENCH-PREP] Anaconda kickstart completed and powered off cleanly (rc=%s)." % vm.proc.returncode)

    # First boot verify of installed disk (captures installed OOBE/Desktop + updates NVRAM BootOrder)
    print("[BENCH-PREP] Verifying first cold boot of installed disk...")
    with _BenchQemuVM(
        disk_path=base_disk,
        ovmf_vars_path=base_vars,
        iso_path=None,
        boot_order="c",
        memory_mb=6144,
        cpus=4,
        artifacts_dir=artifacts_dir,
        vm_label="prep_firstboot",
    ) as vm:
        _wait_qga_reconnect(vm, timeout=210.0)
        for _ in range(15):
            time.sleep(2.5)
            cand = vm.capture_png("00b_installed_firstboot")
            if is_non_black_content(cand, min_non_black_ratio=0.05, min_std_luma=5.0):
                break
        chk = vm.qga.run_shell(
            "set -e; "
            "blkid -L BLAZE_REC; "
            "test -f /boot/efi/EFI/BlazeRecovery/grub.cfg; "
            "grep -q 'BLAZE_RECOVERY_VAULT.json' /boot/efi/EFI/fedora/grub.cfg; "
            "efibootmgr | grep -q 'BlazeOS Recovery (BERE Vault)'; "
            "echo FIRSTBOOT_VERIFIED",
            timeout=30.0,
        )
        if chk.exitcode != 0 or "FIRSTBOOT_VERIFIED" not in chk.stdout:
            raise RuntimeError(f"Installed base verification failed: {chk.stdout}\n{chk.stderr}")
        vm.qga.run_shell("(sleep 1; sync; poweroff) >/dev/null 2>&1 </dev/null &", timeout=15.0)
        if vm.proc is not None:
            vm.proc.wait(timeout=45.0)

    return base_disk, base_vars


def _run_f1_rootfs_wipe(base_disk: Path, base_vars: Path, ovl_dir: Path, artifacts_dir: Path) -> ScenarioResult:
    t0 = time.monotonic()
    ovl, vrs = _create_overlay_pair(base_disk, base_vars, ovl_dir, "f1")
    shot = "F1_bere_recovery.png"
    with _BenchQemuVM(
        disk_path=ovl, ovmf_vars_path=vrs, artifacts_dir=artifacts_dir, vm_label="f1"
    ) as vm:
        _wait_qga_reconnect(vm, timeout=180.0)
        old_boot_id = _get_boot_id(vm)
        # Wipe rootfs init & identity metadata cleanly then force immediate reboot
        vm.qga.run_shell(
            "python3 -c 'import os; "
            "[os.unlink(p) for p in (\"/etc/os-release\", \"/etc/fstab\", \"/usr/lib/os-release\", \"/sbin/init\", \"/usr/sbin/init\", \"/usr/lib/systemd/systemd\") if os.path.lexists(p)]; "
            "os.sync()'; "
            "(sleep 1; sync; reboot -f) >/dev/null 2>&1 </dev/null &",
            timeout=25.0,
        )
        time.sleep(2.0)
        vm.qga.close()
        death_png = None
        deadline = time.monotonic() + 90.0
        while time.monotonic() < deadline:
            time.sleep(2.5)
            cand = vm.capture_png("F1_death_screen")
            if _has_red_death_banner(cand):
                death_png = cand
                break
        if death_png is None:
            raise RuntimeError("F1 failed: Blaze Death Screen red banner never appeared after rootfs wipe")

        t_detect = time.monotonic() - t0
        # Trigger Enter -> kexec from BLAZE_REC into Graphical BERE GUI
        vm.qmp.send_key("ret")
        t_rec_start = time.monotonic()
        _wait_qga_reconnect(vm, prev_boot_id=old_boot_id, timeout=180.0)
        _wait_bere_gui_ready(vm, timeout=120.0)
        t_rec = time.monotonic() - t_rec_start
        vm.capture_png("F1_bere_recovery")
        ver = vm.qga.run_shell(
            "systemctl is-active blaze-recovery-gui.service && blkid -L BLAZE_REC && blockdev --getro $(blkid -L BLAZE_REC)",
            timeout=15.0,
        )
        elapsed = time.monotonic() - t0
        ok = ver.exitcode == 0 and "active" in ver.stdout
        return ScenarioResult(
            code="F1",
            title="Rootfs Silinme (rm -rf /*) -> Ölüm Ekranı -> BLAZE_REC BERE Kurtarma",
            max_points=15,
            points=15 if ok else 0,
            passed=ok,
            elapsed_s=round(elapsed, 2),
            details=f"Hata tespiti: {t_detect:.1f}s, BLAZE_REC BERE açılışı: {t_rec:.1f}s",
            screenshot=shot,
        )


def _run_f2_boot_wipe(base_disk: Path, base_vars: Path, ovl_dir: Path, artifacts_dir: Path) -> ScenarioResult:
    t0 = time.monotonic()
    ovl, vrs = _create_overlay_pair(base_disk, base_vars, ovl_dir, "f2")
    with _BenchQemuVM(
        disk_path=ovl, ovmf_vars_path=vrs, artifacts_dir=artifacts_dir, vm_label="f2"
    ) as vm:
        _wait_qga_reconnect(vm, timeout=180.0)
        old_boot_id = _get_boot_id(vm)
        wipe_res = vm.qga.run_shell(
            "set -e; "
            "BOOT_DEV=$(findmnt -M /boot -n -o SOURCE); "
            "test -b \"$BOOT_DEV\"; "
            "sync; "
            "umount /boot/efi || true; "
            "umount /boot || umount -l /boot; "
            "wipefs -a \"$BOOT_DEV\"; "
            "dd if=/dev/zero of=\"$BOOT_DEV\" bs=1M count=64 conv=fsync; "
            "sync; setsid sh -c 'sleep 3; sync; reboot -f' >/dev/null 2>&1 </dev/null &",
            timeout=45.0,
        )
        if wipe_res.exitcode != 0:
            raise RuntimeError(f"F2 wipe setup failed: {wipe_res.stdout}\n{wipe_res.stderr}")
        time.sleep(2.0)
        t_rec_start = time.monotonic()
        _wait_qga_reconnect(vm, prev_boot_id=old_boot_id, timeout=210.0)
        _wait_bere_gui_ready(vm, timeout=120.0)
        t_rec = time.monotonic() - t_rec_start
        vm.capture_png("F2_esp_boot_wipe_bere")
        chk = vm.qga.run_shell(
            "systemctl is-active blaze-recovery-gui.service && cat /proc/cmdline",
            timeout=15.0,
        )
        elapsed = time.monotonic() - t0
        ok = "blaze-recovery-gui.target" in chk.stdout and "active" in chk.stdout
        return ScenarioResult(
            code="F2",
            title="/boot (vda2) Tam Silinmesi -> ESP/BlazeRecovery Yedek Önyükleme ile BERE",
            max_points=15,
            points=15 if ok else 0,
            passed=ok,
            elapsed_s=round(elapsed, 2),
            details=f"/boot tamamen sıfırlandı; ESP yedek zinciri BERE GUI'yi {t_rec:.1f}s içinde açtı.",
            screenshot="F2_esp_boot_wipe_bere.png",
        )


def _run_f3_btrfs_rollback(base_disk: Path, base_vars: Path, ovl_dir: Path, artifacts_dir: Path) -> ScenarioResult:
    t0 = time.monotonic()
    ovl, vrs = _create_overlay_pair(base_disk, base_vars, ovl_dir, "f3")
    with _BenchQemuVM(
        disk_path=ovl, ovmf_vars_path=vrs, artifacts_dir=artifacts_dir, vm_label="f3"
    ) as vm:
        _wait_qga_reconnect(vm, timeout=180.0)
        old_boot_id = _get_boot_id(vm)
        snap_res = vm.qga.run_shell(
            "set -e; "
            "echo OK_BASELINE > /etc/blaze-bench-canary; "
            "SNAP_OUT=$(blaze-snapshot create --reason bench-ok); "
            "LATEST=$(awk '/^created /{print $2}' <<<\"$SNAP_OUT\"); "
            "[ -n \"$LATEST\" ] || LATEST=$(ls -1 /.blaze-snapshots | sort | tail -n1); "
            "test -n \"$LATEST\"; "
            "echo CORRUPTED_ROOTFS > /etc/blaze-bench-canary; "
            "blaze-snapshot rollback \"$LATEST\"; "
            "sync; setsid sh -c 'sleep 3; sync; reboot -f' >/dev/null 2>&1 </dev/null &",
            timeout=90.0,
        )
        if snap_res.exitcode != 0:
            raise RuntimeError(f"F3 snapshot rollback setup failed: {snap_res.stdout}\n{snap_res.stderr}")
        time.sleep(2.0)
        _wait_qga_reconnect(vm, prev_boot_id=old_boot_id, timeout=180.0)
        verify = vm.qga.run_shell("cat /etc/blaze-bench-canary || true", timeout=15.0)
        vm.capture_png("F3_snapshot_restored")
        elapsed = time.monotonic() - t0
        ok = verify.stdout.strip() == "OK_BASELINE"
        return ScenarioResult(
            code="F3",
            title="Btrfs Alt Birim Bozulması ve Anlık Görüntü Geri Yükleme",
            max_points=15,
            points=15 if ok else 0,
            passed=ok,
            elapsed_s=round(elapsed, 2),
            details=f"Canary doğrulaması geri yükleme sonrası: {verify.stdout.strip()!r}",
            screenshot="F3_snapshot_restored.png",
        )


def _run_f4_tamper_and_ro_lock(base_disk: Path, base_vars: Path, ovl_dir: Path, artifacts_dir: Path) -> ScenarioResult:
    t0 = time.monotonic()
    ovl, vrs = _create_overlay_pair(base_disk, base_vars, ovl_dir, "f4")
    with _BenchQemuVM(
        disk_path=ovl, ovmf_vars_path=vrs, artifacts_dir=artifacts_dir, vm_label="f4"
    ) as vm:
        _wait_qga_reconnect(vm, timeout=180.0)
        ro_res = vm.qga.run_shell(
            "set -e; "
            "VDEV=$(blkid -L BLAZE_REC); "
            "RO_FLAG=$(blockdev --getro \"$VDEV\"); "
            "echo RO_FLAG=$RO_FLAG; "
            "test \"$RO_FLAG\" = \"1\"; "
            "udevadm info -q property \"$VDEV\" | grep -q 'UDISKS_IGNORE=1' && echo UDISKS_HIDDEN=1; "
            "mkdir -p /tmp/test_vault_rw; "
            "if mount \"$VDEV\" /tmp/test_vault_rw 2>/tmp/rw_err.txt; then "
            "  if touch /tmp/test_vault_rw/bad 2>/tmp/rw_err.txt; then umount /tmp/test_vault_rw; exit 1; fi; "
            "  umount /tmp/test_vault_rw; "
            "fi; "
            "echo RW_REJECTED_OK; "
            "python3 - <<'PY'\n"
            "import tempfile, hashlib, json, importlib.util\n"
            "from importlib.machinery import SourceFileLoader\n"
            "from pathlib import Path\n"
            "loader = SourceFileLoader('g', '/usr/bin/berp-recovery-gui')\n"
            "spec = importlib.util.spec_from_loader('g', loader)\n"
            "mod = importlib.util.module_from_spec(spec)\n"
            "loader.exec_module(mod)\n"
            "with tempfile.TemporaryDirectory() as t:\n"
            "    p = Path(t); (p/'LiveOS').mkdir(); (p/'boot').mkdir()\n"
            "    (p/'LiveOS/squashfs.img').write_bytes(b'corrupted')\n"
            "    (p/'boot/vmlinuz-bere').write_bytes(b'vm')\n"
            "    (p/'boot/initrd-bere.img').write_bytes(b'in')\n"
            "    m = {'mode':'full','full_os_embedded':True,'sha256':{'squashfs':'0'*64,'vmlinuz':hashlib.sha256(b'vm').hexdigest(),'initrd':hashlib.sha256(b'in').hexdigest()}}\n"
            "    (p/'BLAZE_RECOVERY_VAULT.json').write_text(json.dumps(m))\n"
            "    mod.subprocess.check_output = lambda cmd, **k: ('/dev/fake' if cmd[:2]==['blkid','-L'] else ('u' if 'UUID' in cmd else t))\n"
            "    orig_exists = mod.os.path.exists\n"
            "    mod.os.path.exists = lambda x: True if x=='/dev/fake' else orig_exists(x)\n"
            "    res = mod.inspect_blaze_rec_vault(verify_hashes=True)\n"
            "    assert res.get('tamper_error') and 'SHA-256' in res['tamper_error'], res\n"
            "print('SHA256_TAMPER_BLOCKED_OK')\n"
            "PY",
            timeout=45.0,
        )
        vm.capture_png("F4_protection_verified")
        elapsed = time.monotonic() - t0
        ok = (
            ro_res.exitcode == 0
            and "RO_FLAG=1" in ro_res.stdout
            and "UDISKS_HIDDEN=1" in ro_res.stdout
            and "RW_REJECTED_OK" in ro_res.stdout
            and "SHA256_TAMPER_BLOCKED_OK" in ro_res.stdout
        )
        if not ok:
            raise RuntimeError(f"F4 protection check failed: {ro_res.stdout}\n{ro_res.stderr}")
        return ScenarioResult(
            code="F4",
            title="BLAZE_REC Yazma Kilidi (blockdev --setro / UDisks / GPT Bit-60) ve SHA-256 Sabotaj Reddi",
            max_points=15,
            points=15,
            passed=True,
            elapsed_s=round(elapsed, 2),
            details="blockdev RO=1, UDISKS_IGNORE=1, yazma engeli ve SHA-256 manipülasyon reddi doğrulandı.",
            screenshot="F4_protection_verified.png",
        )


def _run_f5_auth_lock(base_disk: Path, base_vars: Path, ovl_dir: Path, artifacts_dir: Path) -> ScenarioResult:
    t0 = time.monotonic()
    ovl, vrs = _create_overlay_pair(base_disk, base_vars, ovl_dir, "f5")
    with _BenchQemuVM(
        disk_path=ovl, ovmf_vars_path=vrs, artifacts_dir=artifacts_dir, vm_label="f5"
    ) as vm:
        _wait_qga_reconnect(vm, timeout=180.0)
        old_boot_id = _get_boot_id(vm)
        vm.qga.run_shell(
            'set -e; grub2-editenv /boot/grub2/grubenv set next_entry="BlazeOS Recovery Environment (BERE - Powered by BERP)" && '
            'grub2-editenv /boot/grub2/grubenv list | grep -q "BlazeOS Recovery Environment"',
            timeout=25.0,
        )
        vm.qga.run_shell("setsid sh -c 'sleep 3; sync; reboot -f' >/dev/null 2>&1 </dev/null &", timeout=15.0)
        time.sleep(3.5)
        _wait_qga_reconnect(vm, prev_boot_id=old_boot_id, timeout=210.0)
        _wait_bere_gui_ready(vm, timeout=120.0)
        vm.capture_png("F5_bere_auth_lock")

        auth_res = _bere_api_script(
            vm,
            f"""
import glob, json, urllib.request, urllib.error
tok = open(sorted(glob.glob('/tmp/berp-recovery-*/api.token'))[-1]).read().strip()
def call(ep, payload=None):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(f'http://127.0.0.1:49223/api/{{ep}}', data=data, headers={{'X-BERP-Token': tok, 'Content-Type': 'application/json'}}, method='POST' if data else 'GET')
    try:
        with urllib.request.urlopen(req, timeout=15) as r:
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read().decode())

s0, b0 = call('auth_status')
s_isos, b_isos = call('isos')
s_wipe, b_wipe = call('wipe', {{'targets': ['cache'], 'confirm_text': 'YES', 'password': ''}})
s_bad, b_bad = call('auth_unlock', {{'password': 'WrongPassword123'}})
s_ok, b_ok = call('auth_unlock', {{'password': {BENCH_USER_PW!r}}})
s1, b1 = call('auth_status')
print(json.dumps({{
    'auth_before': b0, 'isos_locked': s_isos, 'wipe_locked': s_wipe,
    'bad_unlock': [s_bad, b_bad.get('status')], 'good_unlock': [s_ok, b_ok.get('status')], 'auth_after': b1
}}))
""",
        )
        elapsed = time.monotonic() - t0
        ok = (
            auth_res["auth_before"].get("auth_required") is True
            and auth_res["auth_before"].get("unlocked") is False
            and auth_res["isos_locked"] == 401
            and auth_res["wipe_locked"] == 401
            and auth_res["bad_unlock"][1] != "ok"
            and auth_res["good_unlock"] == [200, "ok"]
            and auth_res["auth_after"].get("unlocked") is True
        )
        if not ok:
            raise RuntimeError(f"F5 auth gate failed: {auth_res}")
        return ScenarioResult(
            code="F5",
            title="BERE / BERP Zorunlu Yönetici Parola Kilidi (Yetkisiz API/Sıfırlama Reddi)",
            max_points=15,
            points=15,
            passed=True,
            elapsed_s=round(elapsed, 2),
            details="Kilitli oturumda 401 reddi, hatalı parola reddi ve yetkili parola kilidi açma doğrulandı.",
            screenshot="F5_bere_auth_lock.png",
        )


def _run_f6_zero_usb_reinstall(base_disk: Path, base_vars: Path, ovl_dir: Path, artifacts_dir: Path) -> ScenarioResult:
    t0 = time.monotonic()
    ovl, vrs = _create_overlay_pair(base_disk, base_vars, ovl_dir, "f6")
    # Zero ISO attached (`iso_path=None`)!
    with _BenchQemuVM(
        disk_path=ovl, ovmf_vars_path=vrs, iso_path=None, artifacts_dir=artifacts_dir, vm_label="f6"
    ) as vm:
        _wait_qga_reconnect(vm, timeout=180.0)
        old_boot_id = _get_boot_id(vm)
        vm.qga.run_shell(
            'set -e; grub2-editenv /boot/grub2/grubenv set next_entry="BlazeOS Recovery Environment (BERE - Powered by BERP)" && '
            'grub2-editenv /boot/grub2/grubenv list | grep -q "BlazeOS Recovery Environment"',
            timeout=25.0,
        )
        vm.qga.run_shell("setsid sh -c 'sleep 3; sync; reboot -f' >/dev/null 2>&1 </dev/null &", timeout=15.0)
        time.sleep(3.5)
        bere_boot_id = None
        _wait_qga_reconnect(vm, prev_boot_id=old_boot_id, timeout=210.0)
        _wait_bere_gui_ready(vm, timeout=120.0)
        bere_boot_id = _get_boot_id(vm)

        t_verify_start = time.monotonic()
        boot_res = _bere_api_script(
            vm,
            f"""
import glob, json, urllib.request
tok = open(sorted(glob.glob('/tmp/berp-recovery-*/api.token'))[-1]).read().strip()
def call(ep, payload=None):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(f'http://127.0.0.1:49223/api/{{ep}}', data=data, headers={{'X-BERP-Token': tok, 'Content-Type': 'application/json'}}, method='POST' if data else 'GET')
    with urllib.request.urlopen(req, timeout=120) as r:
        return json.loads(r.read().decode())

call('auth_unlock', {{'password': {BENCH_USER_PW!r}}})
isos_res = call('isos')
vault_item = next((i for i in isos_res.get('isos', []) if i.get('path') == 'vault://BLAZE_REC'), None)
assert vault_item is not None and isos_res.get('vault', {{}}).get('full_os_embedded'), isos_res
boot_ans = call('vault_verify_and_boot', {{'profile': 'install', 'copy_to_ram': False, 'password': {BENCH_USER_PW!r}}})
print(json.dumps({{'vault_item': vault_item, 'boot_ans': boot_ans}}))
""",
            timeout=150.0,
        )
        vm.capture_png("F6_bere_vault_ready")
        if boot_res["boot_ans"].get("status") != "ok" or not boot_res["boot_ans"].get("hash_verified"):
            raise RuntimeError(f"F6 vault_verify_and_boot failed: {boot_res}")

        time.sleep(2.0)
        t_kexec_boot = _wait_qga_reconnect(vm, prev_boot_id=bere_boot_id, timeout=240.0)
        t_sha_kexec = time.monotonic() - t_verify_start

        # Verify we booted via kexec from BLAZE_REC into Live SolarEvolution + Anaconda WITHOUT any ISO drive
        ana_chk = None
        deadline = time.monotonic() + 90.0
        while time.monotonic() < deadline:
            res = vm.qga.run_shell(
                "cat /proc/cmdline; echo '---'; "
                "lsblk -no NAME,TYPE /dev/sr0 2>/dev/null || echo NO_OPTICAL; "
                "ps -eo comm | grep -E 'anaconda|liveinst|niri' || true",
                timeout=15.0,
            )
            if "anaconda" in res.stdout or "liveinst" in res.stdout:
                ana_chk = res.stdout
                time.sleep(6.0)
                break
            time.sleep(2.5)
        vm.capture_png("F6_zero_usb_anaconda")
        elapsed = time.monotonic() - t0
        ok = (
            ana_chk is not None
            and "blaze.autoinstaller=1" in ana_chk
            and "rd.live.image" in ana_chk
            and ("anaconda" in ana_chk or "liveinst" in ana_chk)
        )
        if not ok:
            raise RuntimeError(f"F6 failed to reach Anaconda after kexec: {ana_chk}")
        return ScenarioResult(
            code="F6",
            title="USB Olmadan Sıfırdan Kurulum (BLAZE_REC SHA-256 + Doğrudan kexec -> Anaconda)",
            max_points=15,
            points=15,
            passed=True,
            elapsed_s=round(elapsed, 2),
            details=f"SHA-256 doğrulama + kexec Anaconda açılışı toplam {t_sha_kexec:.1f}s (QGA: {t_kexec_boot:.1f}s), Zero-ISO.",
            screenshot="F6_zero_usb_anaconda.png",
        )


def _run_f7_update_identity_guard(base_disk: Path, base_vars: Path, ovl_dir: Path, artifacts_dir: Path) -> ScenarioResult:
    t0 = time.monotonic()
    ovl, vrs = _create_overlay_pair(base_disk, base_vars, ovl_dir, "f7")
    with _BenchQemuVM(
        disk_path=ovl, ovmf_vars_path=vrs, artifacts_dir=artifacts_dir, vm_label="f7"
    ) as vm:
        _wait_qga_reconnect(vm, timeout=180.0)
        res = vm.qga.run_shell(
            "set -e; "
            "# Simulate upstream fedora-release / default grub stomp and run Blaze identity guard + grub2-mkconfig\n"
            "printf 'NAME=\"Fedora Linux\"\\nPRETTY_NAME=\"Fedora Linux 45 (Workstation Edition)\"\\n' > /etc/os-release; "
            "blazeos-sync-identity --force; "
            "grub2-mkconfig -o /boot/grub2/grub.cfg; "
            "grep -qx 'PRETTY_NAME=\"Blaze SolarEvolution 5.05.1\"' /etc/os-release; "
            "grep -qx '45' /etc/dnf/vars/releasever; "
            "grep -q 'DarkMorpheus-pc/Blaze-SolarEvolution' /etc/blazeos/update-channel.conf; "
            "grep -q 'vmlinuz-bere' /boot/grub2/grub.cfg; "
            "echo F7_GUARD_OK",
            timeout=90.0,
        )
        vm.capture_png("F7_identity_guard_verified")
        elapsed = time.monotonic() - t0
        ok = res.exitcode == 0 and "F7_GUARD_OK" in res.stdout
        if not ok:
            raise RuntimeError(f"F7 identity/BERE guard failed: {res.stdout}\n{res.stderr}")
        return ScenarioResult(
            code="F7",
            title="Güncelleme Sonrası Blaze SolarEvolution Kimlik ve 99_berp Koruması",
            max_points=10,
            points=10,
            passed=True,
            elapsed_s=round(elapsed, 2),
            details="Kimlik dosyaları, releasever=45, kanal yapılandırması ve GRUB BLAZE_REC girişi korundu.",
            screenshot="F7_identity_guard_verified.png",
        )


def _write_benchmark_markdown(results: list[ScenarioResult], out_doc: Path) -> None:
    total = sum(r.points for r in results)
    max_total = sum(r.max_points for r in results)
    lines = [
        "# BlazeOS BERE / BERP Dayanıklılık ve Kurtarma Benchmark Raporu",
        "",
        f"- **Tarih:** {time.strftime('%Y-%m-%d %H:%M:%S %Z')}",
        "- **Test Ortamı:** QEMU/KVM UEFI OVMF (6 GiB RAM, 4 vCPU, 30 GiB GPT VirtIO Disk, Tam `BLAZE_REC` Kasası)",
        f"- **Toplam Dayanıklılık Puanı:** **{total} / {max_total}**",
        "",
        "| Senaryo | Test Kapsamı | Durum | Puan | Süre (sn) | Gözlenen Ölçüm | Ekran Görüntüsü |",
        "|---|---|---|---:|---:|---|---|",
    ]
    for r in results:
        status = "BASARILI" if r.passed else "BASARISIZ"
        lines.append(
            f"| **{r.code}** | {r.title} | {status} | {r.points}/{r.max_points} | {r.elapsed_s:.2f} | {r.details} | `{r.screenshot}` |"
        )
    lines.extend(
        [
            "",
            "## Katmanlı Koruma Özeti",
            "",
            "1. **Disk & Bölüm İzolasyonu:** `BLAZE_REC` bağımsız ext4 GPT bölümüdür; Btrfs/LVM dışında tutulur, GPT `RequiredPartition,60` özniteliği ve udev `blockdev --setro $devnode` kuralı ile yazma/silme saldırılarına karşı çekirdek düzeyinde kilitlidir.",
            "2. **Önyükleme Zinciri Bağımsızlığı (`/boot` Silinme Koruması):** `/boot/efi/EFI/BlazeRecovery/` ve `EFI/fedora/grub.cfg` içinde yer alan `/BLAZE_RECOVERY_VAULT.json` arama yedeği sayesinde `/boot` (`vda2`) tamamen silinse bile BERE GUI doğrudan başlatılır.",
            "3. **USB Olmadan Sıfırdan Kurulum:** Tam Zırhlı Kasa (`4.3 GiB`) doğrulamalı `LiveOS/squashfs.img` imajını taşır; `kexec` öncesi tüm bileşenler (`squashfs`, `vmlinuz-bere`, `initrd-bere.img`) SHA-256 kontrolünden geçirilir.",
        ]
    )
    out_doc.parent.mkdir(parents=True, exist_ok=True)
    out_doc.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Run BlazeOS BERE fault-injection benchmarks (F1-F7).")
    parser.add_argument("--iso", type=Path, default=get_target_iso_path())
    parser.add_argument("--artifacts-dir", type=Path, default=get_artifacts_dir() / "bere_benchmark")
    parser.add_argument("--base-disk", type=Path, default=None, help="Reuse existing installed base qcow2.")
    args = parser.parse_args(argv)

    iso_path = Path(args.iso)
    if not iso_path.is_file():
        print(f"ERROR: Target ISO missing: {iso_path}", file=sys.stderr)
        return 1

    artifacts_dir = Path(args.artifacts_dir)
    artifacts_dir.mkdir(parents=True, exist_ok=True)
    work_dir = Path(tempfile.mkdtemp(prefix="blaze_bere_bench_work_"))
    results: list[ScenarioResult] = []
    try:
        if args.base_disk:
            req_disk = Path(args.base_disk)
            req_vars = req_disk.with_name("base_OVMF_VARS.fd")
            if req_disk.is_file() and req_vars.is_file():
                base_disk, base_vars = req_disk, req_vars
            else:
                base_disk, base_vars = _prepare_base_installed_disk(
                    iso_path, work_dir, artifacts_dir, target_disk=req_disk, target_vars=req_vars
                )
        else:
            base_disk, base_vars = _prepare_base_installed_disk(iso_path, work_dir, artifacts_dir)

        ovl_dir = work_dir / "overlays"
        ovl_dir.mkdir(parents=True, exist_ok=True)

        runners = [
            _run_f1_rootfs_wipe,
            _run_f2_boot_wipe,
            _run_f3_btrfs_rollback,
            _run_f4_tamper_and_ro_lock,
            _run_f5_auth_lock,
            _run_f6_zero_usb_reinstall,
            _run_f7_update_identity_guard,
        ]
        for idx, runner in enumerate(runners, start=1):
            print(f"[BENCH] Running {runner.__name__} ({idx}/{len(runners)})...")
            res = runner(base_disk, base_vars, ovl_dir, artifacts_dir)
            results.append(res)
            print(f"[BENCH] {res.code}: {'PASS' if res.passed else 'FAIL'} ({res.points}/{res.max_points}, {res.elapsed_s}s) — {res.details}")
            # Remove used overlay right away to save disk space
            for f in ovl_dir.glob(f"{res.code.lower()}_*"):
                f.unlink(missing_ok=True)

        report_json = artifacts_dir / "bere_benchmark_results.json"
        report_json.write_text(json.dumps([asdict(r) for r in results], indent=2, ensure_ascii=False), encoding="utf-8")
        doc_path = REPO_ROOT / "docs/BERE-BENCHMARKS.md"
        _write_benchmark_markdown(results, doc_path)
        total = sum(r.points for r in results)
        print(f"\n[BENCH] TOTAL SCORE: {total}/100 -> Report written to {doc_path}")
        return 0 if total == 100 else 1
    finally:
        shutil.rmtree(work_dir, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
