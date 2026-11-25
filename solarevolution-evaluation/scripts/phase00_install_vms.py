#!/usr/bin/env python3
"""Phase 00: Build clean, isolated UEFI OVMF installed VM base disks for evaluation.

Produces two pristine base images (never mutated during subsequent phases; all
evaluation workloads run on disposable copy-on-write QCOW2 overlays):
1. Blaze SolarEvolution 5.05.1 RC (from Blaze-SolarEvolution-5-x86_64.iso, Full BLAZE_REC vault)
2. Upstream Fedora 45 Workstation reference (from f45_base/Fedora-Workstation-Live-45-1.4.x86_64.iso)
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

from eval_vm import (
    DEFAULT_EVAL_CPUS,
    DEFAULT_EVAL_RAM_MB,
    EVAL_ROOT_PW,
    EVAL_USER,
    EVAL_USER_PW,
    OVMF_VARS_PATH,
    EvalQemuVM,
    find_free_tcp_port,
    target_vm_kwargs,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
EVAL_ROOT = REPO_ROOT / "solarevolution-evaluation"
VM_STORE = Path("/var/tmp/blaze_eval_vms")
EVAL_SSH_KEY = VM_STORE / "eval_id_ed25519"
EVAL_SSH_PUB = VM_STORE / "eval_id_ed25519.pub"
BLAZE_ISO = REPO_ROOT / "Blaze-SolarEvolution-5-x86_64.iso"
FEDORA_ISO = REPO_ROOT / "f45_base/Fedora-Workstation-Live-45-1.4.x86_64.iso"


def ensure_eval_ssh_key() -> tuple[Path, str]:
    """Ensure an isolated evaluation-only Ed25519 SSH keypair exists in VM_STORE."""
    VM_STORE.mkdir(parents=True, exist_ok=True)
    if not (EVAL_SSH_KEY.is_file() and EVAL_SSH_PUB.is_file()):
        EVAL_SSH_KEY.unlink(missing_ok=True)
        EVAL_SSH_PUB.unlink(missing_ok=True)
        subprocess.run(
            [
                "ssh-keygen",
                "-t",
                "ed25519",
                "-N",
                "",
                "-C",
                "blaze-eval@local",
                "-f",
                str(EVAL_SSH_KEY),
            ],
            check=True,
            capture_output=True,
            text=True,
        )
    os.chmod(EVAL_SSH_KEY, 0o600)
    pubkey = EVAL_SSH_PUB.read_text(encoding="utf-8").strip()
    return EVAL_SSH_KEY, pubkey


def create_fedora_oemdrv_image(img_path: Path, hostname: str, pubkey: str) -> None:
    """Create a raw ext4 OEMDRV disk containing kickstart + serial-logged runner for Fedora Live."""
    ks_cfg = f"""cmdline
lang en_US.UTF-8
keyboard us
timezone UTC --utc
network --hostname={hostname}
liveimg --url=file:///run/initramfs/live/LiveOS/squashfs.img
ignoredisk --only-use=vda
clearpart --all --initlabel --drives=vda
zerombr
bootloader --timeout=3
autopart --type=btrfs
services --enabled=sshd
rootpw --plaintext "{EVAL_ROOT_PW}"
user --name={EVAL_USER} --groups=wheel --password="{EVAL_USER_PW}" --plaintext
shutdown
%post --erroronfail
install -d -m 0700 /root/.ssh
cat > /root/.ssh/authorized_keys <<'EOFKEY'
{pubkey}
EOFKEY
chmod 0600 /root/.ssh/authorized_keys
install -d -m 0755 /etc/ssh/sshd_config.d
cat > /etc/ssh/sshd_config.d/99-eval.conf <<'EOFSSH'
PermitRootLogin prohibit-password
PubkeyAuthentication yes
PasswordAuthentication yes
EOFSSH
chmod 0644 /etc/ssh/sshd_config.d/99-eval.conf
systemctl enable sshd.service
systemctl enable qemu-guest-agent.service || true
restorecon -Rv /root/.ssh /etc/ssh/sshd_config.d || true
%end
"""
    run_sh = """#!/bin/bash
set -uxo pipefail
if [ -f /tmp/f45_eval_started ]; then
  exit 0
fi
touch /tmp/f45_eval_started
exec > >(tee /tmp/f45_ks.log > /dev/ttyS0) 2>&1
echo "F45_EVAL_KS_STARTED"
systemctl stop anaconda-webui.service webui-cockpit-ws.service 2>/dev/null || true
pkill -f 'anaconda|webui-desktop|firefox|liveinst' 2>/dev/null || true
sleep 2
anaconda-cleanup 2>/dev/null || true
for i in raid0 raid1 raid5 raid6 dm-mod dm-zero dm-mirror dm-snapshot vfat btrfs ext4 xfs erofs loop squashfs overlay; do
  modprobe "$i" 2>/dev/null || true
done
setenforce 0 2>/dev/null || true
export ANACONDA_PRODUCTNAME="Fedora"
export ANACONDA_PRODUCTVERSION="45"
export ANACONDA_ISFINAL="true"
export PATH=/sbin:/usr/sbin:/bin:/usr/bin
cp /mnt/o/ks.cfg /tmp/f45_eval.ks
SQ=$(for p in /run/initramfs/live/LiveOS/squashfs.img /run/initramfs/squashfs.img; do [ -f "$p" ] && echo "$p" && break; done)
if [ -n "${SQ:-}" ]; then
  sed -i "s|^liveimg --url=.*|liveimg --url=file://${SQ}|" /tmp/f45_eval.ks
fi
echo "F45_EVAL_ANACONDA_LAUNCH SQ=${SQ:-missing}"
if command -v dbus-run-session >/dev/null 2>&1; then
  LC_ALL=C.UTF-8 dbus-run-session -- anaconda --cmdline --noninteractive --kickstart=/tmp/f45_eval.ks
else
  LC_ALL=C.UTF-8 anaconda --cmdline --noninteractive --kickstart=/tmp/f45_eval.ks
fi
rc=$?
echo "F45_EVAL_KS_DONE rc=$rc"
if [ "$rc" -ne 0 ]; then
  tail -n 60 /tmp/anaconda.log /tmp/storage.log /tmp/program.log 2>/dev/null || true
  exit "$rc"
fi
"""
    with tempfile.TemporaryDirectory(prefix="f45_oemdrv_") as td:
        stage = Path(td)
        (stage / "ks.cfg").write_text(ks_cfg, encoding="utf-8")
        run_path = stage / "run.sh"
        run_path.write_text(run_sh, encoding="utf-8")
        os.chmod(run_path, 0o755)
        img_path.unlink(missing_ok=True)
        subprocess.run(
            ["truncate", "-s", "16M", str(img_path)],
            check=True,
            capture_output=True,
            text=True,
        )
        subprocess.run(
            ["mkfs.ext4", "-F", "-L", "OEMDRV", "-d", str(stage), str(img_path)],
            check=True,
            capture_output=True,
            text=True,
        )


def _install_fedora_via_tty3(
    iso_path: Path,
    disk_path: Path,
    vars_path: Path,
    hostname: str,
    log_dir: Path,
) -> tuple[Path, str]:
    """Install upstream Fedora 45 Workstation Live onto disk_path via OEMDRV + tty3 console."""
    key_path, pubkey = ensure_eval_ssh_key()
    oemdrv_img = VM_STORE / f"{hostname}_oemdrv.img"
    create_fedora_oemdrv_image(oemdrv_img, hostname=hostname, pubkey=pubkey)

    try:
        with EvalQemuVM(
            disk_path=disk_path,
            ovmf_vars_path=vars_path,
            log_dir=log_dir,
            vm_label="install_fedora45",
            iso_path=iso_path,
            extra_drives=[f"file={oemdrv_img},if=virtio,format=raw,readonly=on"],
            boot_order="d",
            memory_mb=DEFAULT_EVAL_RAM_MB,
            cpus=DEFAULT_EVAL_CPUS,
        ) as vm:
            time.sleep(8.0)
            # Upstream Fedora Live GRUB defaults to "Test this media"; move up to Live boot
            vm.qmp.send_key("up")
            time.sleep(0.5)
            vm.qmp.send_key("ret")

            print("[PHASE 00] Waiting for Fedora 45 Live multi-user boot before tty3 trigger...", flush=True)
            time.sleep(28.0)

            started = False
            start_deadline = time.monotonic() + 180.0
            trigger_cmd = "sudo mkdir -p /mnt/o && sudo mount -r /dev/vdb /mnt/o && sudo bash /mnt/o/run.sh\n"
            attempt = 0
            while time.monotonic() < start_deadline:
                attempt += 1
                vm.qmp.send_combo(["ctrl", "alt", "f3"])
                time.sleep(2.0)
                vm.qmp.type_ascii("liveuser\n", delay_secs=0.03)
                time.sleep(2.5)
                vm.qmp.type_ascii("\n", delay_secs=0.03)
                time.sleep(1.5)
                vm.qmp.type_ascii(trigger_cmd, delay_secs=0.02)

                wait_until = time.monotonic() + 14.0
                while time.monotonic() < wait_until:
                    ser = vm.serial_log.read_text(encoding="utf-8", errors="replace") if vm.serial_log.is_file() else ""
                    if "F45_EVAL_KS_STARTED" in ser:
                        started = True
                        break
                    time.sleep(1.0)
                if started:
                    print(f"[PHASE 00] Fedora 45 unattended kickstart triggered on tty3 (attempt {attempt}).", flush=True)
                    break

            if not started:
                vm.capture_png(EVAL_ROOT / "evidence" / "00_fedora45_tty3_timeout.png")
                ser_tail = (
                    vm.serial_log.read_text(encoding="utf-8", errors="replace")[-2000:]
                    if vm.serial_log.is_file()
                    else ""
                )
                raise RuntimeError(f"Failed to start Fedora 45 kickstart from tty3:\n{ser_tail}")

            ks_deadline = time.monotonic() + 900.0
            seen_len = 0
            while time.monotonic() < ks_deadline:
                ser = vm.serial_log.read_text(encoding="utf-8", errors="replace") if vm.serial_log.is_file() else ""
                if len(ser) > seen_len:
                    chunk = ser[seen_len:].strip()
                    seen_len = len(ser)
                    if chunk:
                        tail_line = chunk.splitlines()[-1]
                        print(f"[PHASE 00:fedora45] {tail_line}", flush=True)
                if "F45_EVAL_KS_DONE rc=" in ser and "F45_EVAL_KS_DONE rc=0" not in ser:
                    raise RuntimeError(f"Fedora 45 kickstart failed:\n{ser[-3000:]}")
                if vm.proc is not None and vm.proc.poll() is not None:
                    print("[PHASE 00] fedora45 kickstart completed and powered off cleanly.")
                    return key_path, pubkey
                time.sleep(5.0)
            vm.capture_png(EVAL_ROOT / "evidence" / "00_fedora45_ks_timeout.png")
            raise TimeoutError("Fedora 45 kickstart did not complete within 900s")
    finally:
        oemdrv_img.unlink(missing_ok=True)
    return key_path, pubkey


def install_vm(
    label: str,
    iso_path: Path,
    disk_path: Path,
    vars_path: Path,
    hostname: str,
    is_blaze: bool,
    disk_size_gb: int = 40,
) -> None:
    if disk_path.exists() and vars_path.exists() and disk_path.stat().st_size > 2 * 1024**3:
        print(f"[PHASE 00] Reusing existing pristine base image: {disk_path}")
        return

    VM_STORE.mkdir(parents=True, exist_ok=True)
    disk_path.unlink(missing_ok=True)
    shutil.copyfile(OVMF_VARS_PATH, vars_path)
    subprocess.run(
        ["qemu-img", "create", "-f", "qcow2", str(disk_path), f"{disk_size_gb}G"],
        check=True,
        capture_output=True,
        text=True,
    )

    log_dir = EVAL_ROOT / "raw-results" / "phase00_install"
    print(f"[PHASE 00] Installing {label} from {iso_path.name} -> {disk_path} ({disk_size_gb}G)...")
    if not is_blaze:
        _install_fedora_via_tty3(
            iso_path=iso_path,
            disk_path=disk_path,
            vars_path=vars_path,
            hostname=hostname,
            log_dir=log_dir,
        )
    else:
        with EvalQemuVM(
            disk_path=disk_path,
            ovmf_vars_path=vars_path,
            log_dir=log_dir,
            vm_label=f"install_{label}",
            iso_path=iso_path,
            boot_order="d",
            memory_mb=DEFAULT_EVAL_RAM_MB,
            cpus=DEFAULT_EVAL_CPUS,
        ) as vm:
            # Advance past GRUB menu cleanly
            time.sleep(8.0)
            vm.qmp.send_key("ret")

            print(f"[PHASE 00] Waiting for QGA in {label} live session...")
            vm.qga.wait_until_ready(timeout=240.0, poll_interval=2.0)
            time.sleep(6.0)

            # Close any auto-launched liveinst GUI cleanly before unattended kickstart
            vm.qga.run_shell(
                "systemctl stop anaconda-webui.service webui-cockpit-ws.service 2>/dev/null || true; "
                "pkill -f 'anaconda|webui-desktop|firefox|liveinst' 2>/dev/null || true; sleep 2; anaconda-cleanup 2>/dev/null || true",
                timeout=45.0,
            )

            # Find exact squashfs path in Live environment
            sq_res = vm.qga.run_shell(
                "for p in /run/initramfs/live/LiveOS/squashfs.img /run/initramfs/squashfs.img; do "
                "  if [ -f \"$p\" ]; then echo \"$p\"; exit 0; fi; "
                "done; find /run/initramfs -maxdepth 4 -name 'squashfs.img' 2>/dev/null | head -n1",
                timeout=15.0,
            )
            sq_path = sq_res.stdout.strip().splitlines()[-1].strip() if sq_res.stdout.strip() else "/run/initramfs/live/LiveOS/squashfs.img"
            print(f"[PHASE 00] {label} squashfs path: {sq_path}")

            ks_extra = ""
            blaze_setup_lines = """
printf 'solarui' > /tmp/anaconda_desktop_choice
printf 'grub2' > /tmp/anaconda_bootloader_choice
printf 'full' > /tmp/anaconda_bere_vault_mode
printf 'minimal' > /tmp/anaconda_desktop_profile
printf 'standard' > /tmp/anaconda_usage_profile
printf 'no' > /tmp/anaconda_remove_solarui
"""
            env_lines = """
export ANACONDA_PRODUCTNAME="Blaze SolarEvolution"
export ANACONDA_PRODUCTVERSION="5.05.1"
export ANACONDA_ISFINAL="true"
"""

            ks_content = f"""cmdline
lang en_US.UTF-8
keyboard us
timezone UTC --utc
network --hostname={hostname}
liveimg --url=file://{sq_path}
ignoredisk --only-use=vda
clearpart --all --initlabel --drives=vda
zerombr
bootloader --timeout=3
autopart --type=btrfs
rootpw --plaintext "{EVAL_ROOT_PW}"
user --name={EVAL_USER} --groups=wheel --password="{EVAL_USER_PW}" --plaintext
shutdown
{ks_extra}
"""
            install_script = f"""set -euo pipefail
cat > /tmp/eval_install.ks <<'KS'
{ks_content}KS
rm -f /tmp/eval_ks.rc /tmp/eval_ks.log
{blaze_setup_lines}
cat > /tmp/run_eval_ks.sh <<'SH'
#!/bin/bash
{env_lines}
export PATH=/sbin:/usr/sbin:/bin:/usr/bin
for i in dm-mod vfat btrfs ext4 xfs erofs loop squashfs overlay; do modprobe "$i" 2>/dev/null || true; done
setenforce 0 2>/dev/null || true
anaconda-cleanup 2>/dev/null || true
if command -v dbus-run-session >/dev/null 2>&1; then
  LC_ALL=C.UTF-8 dbus-run-session -- anaconda --cmdline --noninteractive --kickstart=/tmp/eval_install.ks </dev/null >/tmp/eval_ks.log 2>&1
else
  LC_ALL=C.UTF-8 anaconda --cmdline --noninteractive --kickstart=/tmp/eval_install.ks </dev/null >/tmp/eval_ks.log 2>&1
fi
rc=$?
echo "$rc" > /tmp/eval_ks.rc
sync
SH
chmod 0755 /tmp/run_eval_ks.sh
if command -v systemd-run >/dev/null 2>&1; then
  systemd-run --unit=eval-ks-install --no-block /tmp/run_eval_ks.sh
else
  setsid -f /tmp/run_eval_ks.sh </dev/null >/dev/null 2>&1
fi
"""
            vm.qga.guest_exec("/bin/bash", ["-lc", install_script], timeout=30.0, capture_output=False)
            time.sleep(3.0)
            chk_start = vm.qga.run_shell("test -f /tmp/run_eval_ks.sh && echo KS_STARTED", timeout=15.0)
            if chk_start.exitcode != 0 or "KS_STARTED" not in chk_start.stdout:
                raise RuntimeError(f"{label} failed to launch kickstart: {chk_start.stdout}\n{chk_start.stderr}")

            deadline = time.monotonic() + 900.0
            powered_off = False
            ks_rc = None
            last_tail = ""
            while time.monotonic() < deadline:
                if vm.proc is not None and vm.proc.poll() is not None:
                    powered_off = True
                    break
                time.sleep(8.0)
                if vm.proc is not None and vm.proc.poll() is not None:
                    powered_off = True
                    break
                try:
                    poll = vm.qga.run_shell(
                        "if [ -f /tmp/eval_ks.rc ]; then echo RC=$(cat /tmp/eval_ks.rc); lsblk -o NAME,LABEL,SIZE,FSTYPE /dev/vda; else echo RUNNING; tail -n 6 /tmp/eval_ks.log 2>/dev/null || true; fi",
                        timeout=15.0,
                    )
                    if "RC=" in poll.stdout:
                        ks_rc = poll.stdout
                        break
                    cur_tail = poll.stdout.strip()
                    if cur_tail and cur_tail != last_tail:
                        print(f"[PHASE 00:{label}] {cur_tail}", flush=True)
                        last_tail = cur_tail
                except Exception:
                    vm.qga.close()
                    if vm.proc is not None and vm.proc.poll() is not None:
                        powered_off = True
                        break
                    try:
                        vm.qga.wait_until_ready(timeout=15.0, poll_interval=1.5)
                    except Exception:
                        pass

            if not powered_off:
                if ks_rc is None or "RC=0" not in ks_rc:
                    tail = vm.qga.run_shell("tail -n 80 /tmp/eval_ks.log /tmp/anaconda.log /tmp/storage.log 2>/dev/null || true", timeout=20.0)
                    raise RuntimeError(f"{label} kickstart install failed ({ks_rc}):\n{tail.stdout}")
                print(f"[PHASE 00] {label} install finished:\n{ks_rc}")
                vm.shutdown_graceful(timeout=45.0)
            else:
                print(f"[PHASE 00] {label} kickstart completed and powered off cleanly.")

    # Verify first cold boot so OOBE/firstboot initialization settles cleanly before snapshotting
    print(f"[PHASE 00] Verifying clean first cold boot of installed {label}...")
    with EvalQemuVM(
        disk_path=disk_path,
        ovmf_vars_path=vars_path,
        log_dir=log_dir,
        vm_label=f"firstboot_{label}",
        iso_path=None,
        boot_order="c",
        memory_mb=DEFAULT_EVAL_RAM_MB,
        cpus=DEFAULT_EVAL_CPUS,
        **target_vm_kwargs(label),
    ) as vm:
        vm.qga.wait_until_ready(timeout=240.0, poll_interval=2.0)
        time.sleep(10.0)
        chk = vm.qga.run_shell(
            "cat /etc/os-release; getenforce 2>/dev/null || true; uname -r; hostnamectl --static; lsblk -o NAME,LABEL,SIZE,FSTYPE,MOUNTPOINT /dev/vda; rpm -qa | wc -l",
            timeout=20.0,
        )
        print(f"[PHASE 00] {label} first-boot verify:\n{chk.stdout}")
        if not is_blaze:
            if "Enforcing" not in chk.stdout:
                raise RuntimeError(f"Expected SELinux Enforcing on Fedora 45 reference, got:\n{chk.stdout}")
        vm.capture_png(EVAL_ROOT / "evidence" / f"00_firstboot_{label}.png")
        vm.shutdown_graceful(timeout=60.0)
    print(f"[PHASE 00] Pristine base image ready: {disk_path} ({disk_path.stat().st_size / (1024**3):.2f} GiB)")


def main() -> None:
    targets = sys.argv[1:] or ["blaze", "fedora45"]
    if "blaze" in targets:
        install_vm(
            label="blaze",
            iso_path=BLAZE_ISO,
            disk_path=VM_STORE / "blaze_base.qcow2",
            vars_path=VM_STORE / "blaze_base_OVMF_VARS.fd",
            hostname="solarevolution-eval",
            is_blaze=True,
        )
    if "fedora45" in targets and FEDORA_ISO.is_file():
        install_vm(
            label="fedora45",
            iso_path=FEDORA_ISO,
            disk_path=VM_STORE / "fedora45_base.qcow2",
            vars_path=VM_STORE / "fedora45_base_OVMF_VARS.fd",
            hostname="fedora45-ref-eval",
            is_blaze=False,
        )


if __name__ == "__main__":
    main()
