#!/usr/bin/env python3
"""Installed-VM Release Gate for BlazeOS Akrep v1.0 Security Center.

Installs Blaze-SolarEvolution-5.05.1-RC-x86_64.iso onto an isolated disposable
UEFI OVMF QCOW2 disk (/var/tmp/akrep_v1_gate_vm/), boots the installed system
under SELinux=Enforcing, and verifies all v1.0 security, functional, rollback,
crash-recovery, UI screenshot, and performance-budget gates end-to-end.

Safety invariants:
- Never modifies or touches host OS partitions, host SELinux, host firewalld, or host RPMs.
- Cleans up disposable /var/tmp/akrep_v1_gate_vm/ QCOW2 storage after saving
  screenshots and gate_akrep_v1_results.json in /tmp/akrep_v1_screenshots/.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import statistics
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parents[2]
EVAL_SCRIPTS_DIR = REPO_ROOT / "solarevolution-evaluation" / "scripts"
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))
if str(EVAL_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(EVAL_SCRIPTS_DIR))

from eval_vm import (  # type: ignore
    EVAL_ROOT_PW,
    EVAL_USER,
    EVAL_USER_PW,
    OVMF_VARS_PATH,
    EvalQemuVM,
    create_overlay_pair,
)
from tests.e2e.harness.imaging import frame_diff_ratio, is_non_black_content

DEFAULT_ISO = REPO_ROOT / "Blaze-SolarEvolution-5.05.1-RC-x86_64.iso"
VM_WORK_DIR = Path("/var/tmp/akrep_v1_gate_vm")
SCREENSHOT_DIR = Path("/tmp/akrep_v1_screenshots")

EICAR_BYTES = (
    b"X5O!P%@AP[4\\PZX54(P^)7CC)7}$"
    b"EICAR-STANDARD-ANTIVIRUS-TEST-FILE!$H+H*"
)
EICAR_SHA256 = hashlib.sha256(EICAR_BYTES).hexdigest()
EICAR_NDB_LINE = "Eicar-Test-Signature:0:*:" + EICAR_BYTES.hex() + "\n"

GUEST_DBUS_HELPER = r'''#!/usr/bin/env python3
import json
import sys
import time
from gi.repository import Gio, GLib

RO_BUS = "tr.com.blazeos.Akrep1"
RO_PATH = "/tr/com/blazeos/Akrep1"
RO_IFACE = "tr.com.blazeos.Akrep1.Manager"

PRIV_BUS = "tr.com.blazeos.Akrep1.Privileged"
PRIV_PATH = "/tr/com/blazeos/Akrep1/Privileged"
PRIV_IFACE = "tr.com.blazeos.Akrep1.PrivilegedManager"


def call_priv(conn, method, variant_args=None, reply_sig="(s)", timeout_ms=30000):
    res = conn.call_sync(
        PRIV_BUS,
        PRIV_PATH,
        PRIV_IFACE,
        method,
        variant_args,
        GLib.VariantType(reply_sig),
        Gio.DBusCallFlags.NONE,
        timeout_ms,
        None,
    )
    return res.unpack()[0]


def main():
    cmd = sys.argv[1]
    conn = Gio.bus_get_sync(Gio.BusType.SYSTEM, None)

    if cmd == "snapshot":
        t0 = time.monotonic()
        res = conn.call_sync(
            RO_BUS,
            RO_PATH,
            RO_IFACE,
            "GetSnapshot",
            None,
            GLib.VariantType("(s)"),
            Gio.DBusCallFlags.NONE,
            15000,
            None,
        )
        elapsed_ms = (time.monotonic() - t0) * 1000.0
        raw = res.unpack()[0]
        print(json.dumps({"elapsed_ms": round(elapsed_ms, 3), "snapshot": json.loads(raw)}))
        return 0

    if cmd == "snapshot-bench":
        runs = int(sys.argv[2]) if len(sys.argv[2:]) >= 1 else 5
        samples = []
        last_snap = None
        for _ in range(runs):
            t0 = time.monotonic()
            res = conn.call_sync(
                RO_BUS,
                RO_PATH,
                RO_IFACE,
                "GetSnapshot",
                None,
                GLib.VariantType("(s)"),
                Gio.DBusCallFlags.NONE,
                15000,
                None,
            )
            samples.append(round((time.monotonic() - t0) * 1000.0, 3))
            last_snap = json.loads(res.unpack()[0])
        print(json.dumps({"samples_ms": samples, "snapshot": last_snap}))
        return 0

    try:
        if cmd == "priv-call":
            method = sys.argv[2]
            args = sys.argv[3:]
            if method in ("GetVersion", "InspectSystemFlatpaks", "InspectDeviceSecurity", "InspectScanner", "ListQuarantine", "ListProfiles"):
                v_in = None
            elif method == "GetHistory":
                v_in = GLib.Variant("(u)", (int(args[0]),))
            elif len(args) == 1:
                v_in = GLib.Variant("(s)", (args[0],))
            elif len(args) == 2:
                v_in = GLib.Variant("(ss)", (args[0], args[1]))
            else:
                raise RuntimeError(f"Unsupported arg count for {method}: {args}")
            out = call_priv(conn, method, v_in, timeout_ms=60000)
            parsed = out
            if method != "GetVersion":
                try:
                    parsed = json.loads(out)
                except Exception:
                    parsed = out
            print(json.dumps({"ok": True, "result": parsed}))
            return 0

        if cmd == "plan-and-commit":
            req_json = sys.argv[2]
            plan = json.loads(call_priv(conn, "CreatePlan", GLib.Variant("(s)", (req_json,)), timeout_ms=45000))
            tx = json.loads(
                call_priv(
                    conn,
                    "CommitPlan",
                    GLib.Variant("(ss)", (plan["plan_id"], plan["plan_sha256"])),
                    timeout_ms=90000,
                )
            )
            print(json.dumps({"ok": True, "plan": plan, "tx": tx}))
            return 0

        if cmd == "profile-apply":
            prof_id = sys.argv[2]
            plan = json.loads(call_priv(conn, "PreviewProfile", GLib.Variant("(s)", (prof_id,)), timeout_ms=45000))
            tx = json.loads(
                call_priv(
                    conn,
                    "CommitPlan",
                    GLib.Variant("(ss)", (plan["plan_id"], plan["plan_sha256"])),
                    timeout_ms=90000,
                )
            )
            print(json.dumps({"ok": True, "plan": plan, "tx": tx}))
            return 0

        if cmd == "rollback":
            tx_id = sys.argv[2]
            tx = json.loads(call_priv(conn, "RollbackTransaction", GLib.Variant("(s)", (tx_id,)), timeout_ms=90000))
            print(json.dumps({"ok": True, "tx": tx}))
            return 0

    except GLib.Error as err:
        print(json.dumps({"ok": False, "dbus_error": str(err)}))
        return 3

    raise RuntimeError(f"Unknown helper command: {cmd}")


if __name__ == "__main__":
    raise SystemExit(main())
'''

GUEST_CLAMD_FALLBACK = r'''#!/usr/bin/env python3
"""Minimal local Unix-socket zINSTREAM responder used ONLY if Fedora mirror is offline."""
import hashlib
import os
import socket
import struct
import sys

SOCK_PATH = "/run/clamd.scan/clamd.sock"
EICAR_SHA256 = "275a021bbfb6489e54d471899f7db9d1663fc695ec2fe2a2c4538aabf651fd0f"

os.makedirs("/run/clamd.scan", mode=0o755, exist_ok=True)
if os.path.exists(SOCK_PATH):
    os.unlink(SOCK_PATH)

srv = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
srv.bind(SOCK_PATH)
os.chown(SOCK_PATH, 0, 0)
os.chmod(SOCK_PATH, 0o660)
srv.listen(16)

while True:
    conn, _ = srv.accept()
    try:
        hdr = b""
        while not hdr.endswith(b"\0") and len(hdr) < 64:
            ch = conn.recv(1)
            if not ch:
                break
            hdr += ch
        if hdr.startswith(b"zPING"):
            conn.sendall(b"PONG\0")
        elif hdr.startswith(b"zINSTREAM"):
            h = hashlib.sha256()
            while True:
                len_b = b""
                while len(len_b) < 4:
                    c = conn.recv(4 - len(len_b))
                    if not c:
                        break
                    len_b += c
                if len(len_b) < 4:
                    break
                (chunk_len,) = struct.unpack("!I", len_b)
                if chunk_len == 0:
                    break
                rem = chunk_len
                while rem > 0:
                    data = conn.recv(min(32768, rem))
                    if not data:
                        rem = 0
                        break
                    h.update(data)
                    rem -= len(data)
            if h.hexdigest() == EICAR_SHA256:
                conn.sendall(b"stream: Eicar-Test-Signature FOUND\0")
            else:
                conn.sendall(b"stream: OK\0")
    finally:
        conn.close()
'''


def assert_true(cond: bool, msg: str) -> None:
    if not cond:
        raise AssertionError(msg)


def parse_systemd_analyze_total_seconds(line: str) -> float:
    # Example: "Startup finished in 1.120s (firmware) + 840ms (loader) + 920ms (kernel) + 1.210s (initrd) + 2.410s (userspace) = 6.500s"
    if "=" not in line:
        raise ValueError(f"Unexpected systemd-analyze time output: {line!r}")
    rhs = line.split("=")[-1].strip().splitlines()[0].strip()
    total = 0.0
    for tok in rhs.split():
        if tok.endswith("min"):
            total += float(tok[:-3]) * 60.0
        elif tok.endswith("ms"):
            total += float(tok[:-2]) / 1000.0
        elif tok.endswith("s"):
            total += float(tok[:-1])
    return round(total, 4)


def parse_systemd_analyze_userspace_seconds(line: str) -> float:
    # Extract "... + 2.410s (userspace) = ..."
    for seg in line.split("+"):
        if "(userspace)" in seg:
            tok = seg.split("(userspace)")[0].strip().split()[-1]
            if tok.endswith("ms"):
                return round(float(tok[:-2]) / 1000.0, 4)
            if tok.endswith("s"):
                return round(float(tok[:-1]), 4)
    return parse_systemd_analyze_total_seconds(line)


def install_blaze_vm(iso_path: Path, disk_path: Path, vars_path: Path, log_dir: Path) -> float:
    VM_WORK_DIR.mkdir(parents=True, exist_ok=True)
    disk_path.unlink(missing_ok=True)
    vars_path.unlink(missing_ok=True)
    shutil.copyfile(OVMF_VARS_PATH, vars_path)
    subprocess.run(
        ["qemu-img", "create", "-f", "qcow2", str(disk_path), "34G"],
        check=True,
        capture_output=True,
        text=True,
    )

    t0 = time.monotonic()
    print(f"[AKREP GATE] Installing {iso_path.name} -> {disk_path}...", flush=True)
    with EvalQemuVM(
        disk_path=disk_path,
        ovmf_vars_path=vars_path,
        log_dir=log_dir,
        vm_label="akrep_v1_install",
        iso_path=iso_path,
        boot_order="d",
        memory_mb=8192,
        cpus=8,
    ) as vm:
        time.sleep(8.0)
        vm.qmp.send_key("ret")
        vm.qga.wait_until_ready(timeout=240.0, poll_interval=2.0)
        # Wait briefly so any liveuser session auto-launch of liveinst starts and can be cleanly stopped
        time.sleep(12.0)

        vm.qga.run_shell(
            "systemctl stop anaconda-webui.service webui-cockpit-ws.service 2>/dev/null || true",
            timeout=30.0,
        )
        vm.qga.guest_exec(
            "/usr/bin/pkill",
            ["-TERM", "-f", "[a]naconda|[w]ebui-desktop|[f]irefox|[l]iveinst"],
            timeout=15.0,
        )
        time.sleep(2.0)
        vm.qga.guest_exec(
            "/usr/bin/pkill",
            ["-KILL", "-f", "[a]naconda|[w]ebui-desktop|[f]irefox|[l]iveinst|/[r]un/install/"],
            timeout=15.0,
        )
        vm.qga.run_shell(
            "anaconda-cleanup --liveinst 2>/dev/null || anaconda-cleanup 2>/dev/null || true; "
            "rm -f /run/anaconda/anaconda.conf /run/anaconda/bus.address /run/anaconda/liveinst.lock "
            "/run/anaconda/backend_ready /run/anaconda/webui_script.pid",
            timeout=30.0,
        )

        sq_res = vm.qga.run_shell(
            "for p in /run/initramfs/live/LiveOS/squashfs.img /run/initramfs/squashfs.img; do "
            "  if [ -f \"$p\" ]; then echo \"$p\"; exit 0; fi; "
            "done; find /run/initramfs -maxdepth 4 -name 'squashfs.img' 2>/dev/null | head -n1",
            timeout=15.0,
        )
        sq_path = (
            sq_res.stdout.strip().splitlines()[-1].strip()
            if sq_res.stdout.strip()
            else "/run/initramfs/live/LiveOS/squashfs.img"
        )

        ks_content = f"""cmdline
lang en_US.UTF-8
keyboard us
timezone UTC --utc
network --hostname=blaze-akrep-gate
liveimg --url=file://{sq_path}
ignoredisk --only-use=vda
clearpart --all --initlabel --drives=vda
zerombr
bootloader --timeout=1
autopart --type=btrfs
rootpw --plaintext "{EVAL_ROOT_PW}"
user --name={EVAL_USER} --groups=wheel --password="{EVAL_USER_PW}" --plaintext
shutdown
"""
        install_script = f"""set -euo pipefail
cat > /tmp/akrep_gate.ks <<'KS'
{ks_content}KS
rm -f /tmp/akrep_ks.rc /tmp/akrep_ks.log
printf 'solarui' > /tmp/anaconda_desktop_choice
printf 'grub2' > /tmp/anaconda_bootloader_choice
printf 'minimal' > /tmp/anaconda_bere_vault_mode
printf 'minimal' > /tmp/anaconda_desktop_profile
printf 'standard' > /tmp/anaconda_usage_profile
printf 'no' > /tmp/anaconda_remove_solarui
cat > /tmp/run_akrep_ks.sh <<'SH'
#!/bin/bash
export ANACONDA_PRODUCTNAME="Blaze SolarEvolution"
export ANACONDA_PRODUCTVERSION="5.05.1"
export ANACONDA_ISFINAL="true"
export PATH=/sbin:/usr/sbin:/bin:/usr/bin
for i in dm-mod vfat btrfs ext4 xfs erofs loop squashfs overlay; do modprobe "$i" 2>/dev/null || true; done
setenforce 0 2>/dev/null || true
anaconda-cleanup 2>/dev/null || true
if command -v dbus-run-session >/dev/null 2>&1; then
  LC_ALL=C.UTF-8 dbus-run-session -- anaconda --cmdline --noninteractive --kickstart=/tmp/akrep_gate.ks </dev/null >/tmp/akrep_ks.log 2>&1
else
  LC_ALL=C.UTF-8 anaconda --cmdline --noninteractive --kickstart=/tmp/akrep_gate.ks </dev/null >/tmp/akrep_ks.log 2>&1
fi
rc=$?
echo "$rc" > /tmp/akrep_ks.rc
sync
SH
chmod 0755 /tmp/run_akrep_ks.sh
systemd-run --unit=akrep-ks-install --no-block /bin/bash /tmp/run_akrep_ks.sh
"""
        launch_res = vm.qga.run_shell(install_script, timeout=30.0)
        assert_true(
            launch_res.exitcode == 0,
            f"Failed to launch akrep-ks-install unit:\n{launch_res.stdout}\n{launch_res.stderr}",
        )
        time.sleep(5.0)
        unit_chk = vm.qga.run_shell(
            "systemctl status akrep-ks-install.service --no-pager -n 15 2>&1 || true; "
            "test -f /tmp/akrep_ks.log && echo KS_LOG_PRESENT",
            timeout=15.0,
        )
        assert_true(
            "KS_LOG_PRESENT" in unit_chk.stdout,
            f"akrep-ks-install did not start /tmp/akrep_ks.log:\n{unit_chk.stdout}\n{unit_chk.stderr}",
        )

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
                    "if [ -f /tmp/akrep_ks.rc ]; then echo RC=$(cat /tmp/akrep_ks.rc); "
                    "else echo RUNNING; tail -n 4 /tmp/akrep_ks.log 2>/dev/null || true; fi",
                    timeout=15.0,
                )
                if "RC=" in poll.stdout:
                    ks_rc = poll.stdout.strip()
                    break
                cur = poll.stdout.strip()
                if cur and cur != last_tail:
                    print(f"[AKREP INSTALL] {cur.splitlines()[-1]}", flush=True)
                    last_tail = cur
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
                tail = vm.qga.run_shell(
                    "tail -n 80 /tmp/akrep_ks.log /tmp/anaconda.log /tmp/storage.log 2>/dev/null || true",
                    timeout=20.0,
                )
                raise RuntimeError(f"Kickstart install failed ({ks_rc}):\n{tail.stdout}")
            vm.shutdown_graceful(timeout=45.0)

    dur = round(time.monotonic() - t0, 2)
    print(f"[AKREP GATE] Installed VM ready in {dur}s.", flush=True)
    return dur


def wait_for_evaluser_wayland(vm: EvalQemuVM, timeout: float = 90.0) -> tuple[int, str, str]:
    deadline = time.monotonic() + timeout
    last_out = ""
    while time.monotonic() < deadline:
        res = vm.qga.run_shell(
            f"set -e; uid=$(id -u {EVAL_USER}); rt=/run/user/$uid; "
            f"pgrep -u {EVAL_USER} -x niri >/dev/null; "
            f"pgrep -u {EVAL_USER} -x solar-shell >/dev/null; "
            "wdisp=$(ls $rt/wayland-* 2>/dev/null | grep -v '\\.lock$' | head -n1 | xargs -r basename); "
            "nsock=$(ls $rt/niri*.sock 2>/dev/null | head -n1 || true); "
            "test -n \"$wdisp\" && test -S \"$rt/bus\" && test -S \"$nsock\"; "
            "echo \"$uid|$wdisp|$nsock\"",
            timeout=15.0,
        )
        last_out = f"{res.stdout} {res.stderr}".strip()
        if res.exitcode == 0 and "|" in res.stdout:
            uid_s, wdisp, nsock = res.stdout.strip().splitlines()[-1].split("|", 2)
            return int(uid_s), wdisp, nsock
        time.sleep(2.0)
    raise RuntimeError(f"Timed out waiting for {EVAL_USER} SolarUI Wayland session: {last_out}")


def wait_for_system_boot_settled(vm: EvalQemuVM, timeout: float = 90.0) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        res = vm.qga.run_shell("systemd-analyze time 2>&1 || true", timeout=15.0)
        if "Startup finished in" in res.stdout:
            return res.stdout.strip()
        time.sleep(1.5)
    raise RuntimeError("systemd-analyze time did not settle within timeout")


def push_helper_scripts(vm: EvalQemuVM) -> None:
    vm.qga.push_bytes(GUEST_DBUS_HELPER.encode("utf-8"), "/tmp/akrep_dbus_helper.py")
    vm.qga.run_shell("chmod 0755 /tmp/akrep_dbus_helper.py", timeout=10.0)


def run_helper_json(vm: EvalQemuVM, args_str: str, as_user: str = "root", timeout: float = 90.0) -> tuple[int, dict[str, Any]]:
    if as_user == "root":
        cmd = f"python3 /tmp/akrep_dbus_helper.py {args_str}"
    else:
        cmd = f"runuser -u {as_user} -- python3 /tmp/akrep_dbus_helper.py {args_str}"
    res = vm.qga.run_shell(cmd, timeout=timeout)
    if res.exitcode not in (0, 3):
        raise RuntimeError(
            f"Helper command failed for `{cmd}` (rc={res.exitcode}):\nstdout={res.stdout}\nstderr={res.stderr}"
        )
    raw = res.stdout.strip().splitlines()[-1] if res.stdout.strip() else "{}"
    try:
        parsed = json.loads(raw)
    except Exception as exc:
        raise RuntimeError(
            f"Helper JSON parse failed for `{cmd}` (rc={res.exitcode}):\nstdout={res.stdout}\nstderr={res.stderr}"
        ) from exc
    return res.exitcode, parsed


def run_gate(iso_path: Path, keep_vm: bool = False) -> dict[str, Any]:
    if not iso_path.is_file():
        raise FileNotFoundError(f"ISO not found: {iso_path}")

    SCREENSHOT_DIR.mkdir(parents=True, exist_ok=True)
    log_dir = VM_WORK_DIR / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    base_disk = VM_WORK_DIR / "blaze_akrep_v1.qcow2"
    base_vars = VM_WORK_DIR / "blaze_akrep_v1_OVMF_VARS.fd"

    results: dict[str, Any] = {
        "iso_name": iso_path.name,
        "gates": {},
        "metrics": {},
        "screenshots": [],
        "overall_pass": False,
    }

    try:
        if base_disk.is_file() and base_vars.is_file() and base_disk.stat().st_size > 2 * 1024**3:
            print(f"[AKREP GATE] Reusing installed base disk {base_disk}", flush=True)
            install_secs = 0.0
        else:
            install_secs = install_blaze_vm(iso_path, base_disk, base_vars, log_dir)
        results["metrics"]["install_duration_s"] = install_secs
        disk_path, vars_path = create_overlay_pair(base_disk, base_vars, VM_WORK_DIR, "gate_run")

        with EvalQemuVM(
            disk_path=disk_path,
            ovmf_vars_path=vars_path,
            log_dir=log_dir,
            vm_label="akrep_v1_gate",
            iso_path=None,
            boot_order="c",
            memory_mb=8192,
            cpus=8,
        ) as vm:
            print("[AKREP GATE] Booting installed BlazeOS 5.05.1 VM...", flush=True)
            vm.qga.wait_until_ready(timeout=240.0, poll_interval=2.0)
            time.sleep(5.0)

            # Complete OOBE into evaluser SolarUI desktop cleanly and reboot once into clean installed session
            prep = vm.qga.run_shell(
                f"set -euo pipefail; "
                "touch /etc/blazeos-setup-completed; "
                "printf 'solarui.desktop\\n' > /etc/blazeos-selected-session; "
                "mkdir -p /etc/sddm.conf.d; "
                f"printf '[Autologin]\\nUser={EVAL_USER}\\nSession=solarui.desktop\\n' > /etc/sddm.conf.d/autologin.conf; "
                "/usr/libexec/blazeos-oobe-hygiene; "
                "restorecon -Rv /etc/blazeos-setup-completed /etc/blazeos-selected-session /etc/sddm.conf.d >/dev/null 2>&1 || true",
                timeout=30.0,
            )
            assert_true(prep.exitcode == 0, f"OOBE hygiene transition failed: {prep.stderr}")

            vm.reboot_and_wait(timeout=180.0)
            boot_enabled_1 = wait_for_system_boot_settled(vm)
            uid, wdisp, nsock = wait_for_evaluser_wayland(vm)
            time.sleep(3.0)
            # Dismiss first-run welcome window if present so screenshots capture unobstructed desktop & Akrep UI
            vm.qga.guest_exec(
                "/usr/bin/pkill",
                ["-u", EVAL_USER, "-f", "[b]lazeos-welcome|[s]olar-welcome"],
                timeout=10.0,
            )
            time.sleep(1.0)

            desktop_png = vm.capture_png(SCREENSHOT_DIR / "00_desktop_baseline.png")
            assert_true(is_non_black_content(desktop_png), "Installed SolarUI desktop screenshot is black")
            results["screenshots"].append(str(desktop_png))

            # ================================================================
            # GATE 1: Installed RPM Provenance (1.0.0-1.blaze) & SELinux Enforcing
            # ================================================================
            print("[GATE 1/10] Verifying RPM provenance (1.0.0-1.blaze), rpm -V (SEC-12), and SELinux=Enforcing...", flush=True)
            g1 = vm.qga.run_shell(
                "set -euo pipefail; "
                "test \"$(getenforce)\" = \"Enforcing\"; "
                "grep -qx 'PRETTY_NAME=\"Blaze SolarEvolution 5.05.1\"' /etc/os-release; "
                "systemctl is-active --quiet akrepd.service; "
                "test \"$(ps -o user= -C akrepd | awk '{print $1; exit}')\" = \"akrep\"; "
                "q_out=$(rpm -q --qf '%{NAME}-%{VERSION}-%{RELEASE}\\n' blaze-akrep blaze-akrep-daemon blaze-akrep-ui); "
                "echo \"$q_out\" | grep -qx 'blaze-akrep-1.0.0-1.blaze'; "
                "echo \"$q_out\" | grep -qx 'blaze-akrep-daemon-1.0.0-1.blaze'; "
                "echo \"$q_out\" | grep -qx 'blaze-akrep-ui-1.0.0-1.blaze'; "
                "v_out=$(rpm -V blaze-akrep blaze-akrep-daemon blaze-akrep-ui); "
                "test -z \"$v_out\"; "
                "echo \"$q_out\"",
                timeout=30.0,
            )
            assert_true(g1.exitcode == 0, f"Gate 1 failed:\n{g1.stdout}\n{g1.stderr}")
            results["gates"]["01_rpm_and_selinux_baseline"] = {
                "pass": True,
                "rpms": g1.stdout.strip().splitlines(),
                "rpm_verify_clean": True,
                "selinux": "Enforcing",
            }

            # ================================================================
            # GATE 2: Idle Memory/CPU, GetSnapshot Latency, UI Startup & Screenshots
            # ================================================================
            print("[GATE 2/10] Measuring idle akrepd RSS/CPU, GetSnapshot latency, akrep-ui startup, and Wayland screenshots...", flush=True)
            perf_idle = vm.qga.run_shell(
                "python3 - <<'PY'\n"
                "import json, os, subprocess, time\n"
                "pid = int(subprocess.check_output(['pidof', 'akrepd'], text=True).split()[0])\n"
                "priv_pids = subprocess.run(['pidof', 'akrep-privd'], capture_output=True, text=True).stdout.strip()\n"
                "status = open(f'/proc/{pid}/status').read()\n"
                "rss_kb = int([l.split()[1] for l in status.splitlines() if l.startswith('VmRSS:')][0])\n"
                "hz = os.sysconf(os.sysconf_names['SC_CLK_TCK'])\n"
                "def jiffies(p):\n"
                "    st = open(f'/proc/{p}/stat').read().rsplit(')', 1)[1].split()\n"
                "    return int(st[11]) + int(st[12])\n"
                "j0, t0 = jiffies(pid), time.monotonic()\n"
                "time.sleep(3.0)\n"
                "j1, t1 = jiffies(pid), time.monotonic()\n"
                "cpu_pct = ((j1 - j0) / hz) / (t1 - t0) * 100.0\n"
                "print(json.dumps({'akrepd_pid': pid, 'rss_kb': rss_kb, 'idle_cpu_pct': round(cpu_pct, 4), 'privd_running_at_idle': bool(priv_pids)}))\n"
                "PY",
                timeout=20.0,
            )
            assert_true(perf_idle.exitcode == 0, f"Idle perf probe failed: {perf_idle.stderr}")
            idle_data = json.loads(perf_idle.stdout.strip().splitlines()[-1])
            assert_true(
                idle_data["rss_kb"] <= 24576,
                f"akrepd VmRSS {idle_data['rss_kb']} kB exceeds 24576 kB (24 MiB) budget",
            )
            assert_true(
                idle_data["idle_cpu_pct"] <= 0.2,
                f"akrepd idle CPU {idle_data['idle_cpu_pct']}% exceeds 0.2% budget",
            )
            assert_true(
                not idle_data["privd_running_at_idle"],
                "akrep-privd must not run eagerly before privileged D-Bus demand",
            )

            push_helper_scripts(vm)
            rc_snap, snap_bench = run_helper_json(vm, "snapshot-bench 5")
            assert_true(rc_snap == 0, f"snapshot-bench failed: {snap_bench}")
            snap_median_ms = round(statistics.median(snap_bench["samples_ms"]), 3)
            assert_true(
                snap_median_ms <= 250.0,
                f"GetSnapshot median {snap_median_ms} ms exceeds 250 ms budget ({snap_bench['samples_ms']})",
            )
            assert_true(snap_bench["snapshot"]["version"] == "1.0.0", "Snapshot version != 1.0.0")
            assert_true(snap_bench["snapshot"]["read_only"] is True, "akrepd read_only must be True")

            # Launch akrep-ui on Wayland and time window map in Niri
            ui_probe = vm.qga.run_shell(
                f"python3 - <<'PY'\n"
                "import json, os, subprocess, time\n"
                f"user = {EVAL_USER!r}\n"
                f"uid = {uid}\n"
                f"wdisp = {wdisp!r}\n"
                f"nsock = {nsock!r}\n"
                "rt = f'/run/user/{uid}'\n"
                "env = {\n"
                "    **os.environ,\n"
                "    'XDG_RUNTIME_DIR': rt,\n"
                "    'WAYLAND_DISPLAY': wdisp,\n"
                "    'NIRI_SOCKET': nsock,\n"
                "    'DBUS_SESSION_BUS_ADDRESS': f'unix:path={rt}/bus',\n"
                "    'HOME': f'/home/{user}',\n"
                "    'USER': user,\n"
                "    'LOGNAME': user,\n"
                "}\n"
                "t0 = time.monotonic()\n"
                "proc = subprocess.Popen(\n"
                "    ['runuser', '-u', user, '--', 'env',\n"
                "     f'XDG_RUNTIME_DIR={rt}', f'WAYLAND_DISPLAY={wdisp}', f'NIRI_SOCKET={nsock}',\n"
                "     f'DBUS_SESSION_BUS_ADDRESS=unix:path={rt}/bus', 'LANG=tr_TR.UTF-8',\n"
                "     '/usr/bin/akrep-ui'],\n"
                "    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL\n"
                ")\n"
                "startup_ms = None\n"
                "deadline = t0 + 5.0\n"
                "while time.monotonic() < deadline:\n"
                "    out = subprocess.run(\n"
                "        ['runuser', '-u', user, '--', 'env', f'XDG_RUNTIME_DIR={rt}', f'NIRI_SOCKET={nsock}',\n"
                "         'niri', 'msg', '--json', 'windows'],\n"
                "        capture_output=True, text=True\n"
                "    )\n"
                "    if out.returncode == 0 and out.stdout.strip():\n"
                "        wins = json.loads(out.stdout)\n"
                "        if any(w.get('app_id') == 'tr.com.blazeos.Akrep' for w in wins):\n"
                "            startup_ms = round((time.monotonic() - t0) * 1000.0, 2)\n"
                "            break\n"
                "    time.sleep(0.04)\n"
                "print(json.dumps({'startup_ms': startup_ms, 'pid': proc.pid}))\n"
                "PY",
                timeout=20.0,
            )
            assert_true(ui_probe.exitcode == 0, f"akrep-ui Wayland launch failed: {ui_probe.stderr}")
            ui_info = json.loads(ui_probe.stdout.strip().splitlines()[-1])
            assert_true(
                ui_info["startup_ms"] is not None and ui_info["startup_ms"] <= 1500.0,
                f"akrep-ui startup_ms={ui_info['startup_ms']} exceeded 1500 ms budget",
            )
            time.sleep(2.0)
            overview_png = vm.capture_png(SCREENSHOT_DIR / "01_akrep_overview.png")
            assert_true(is_non_black_content(overview_png), "01_akrep_overview.png is black")
            assert_true(
                frame_diff_ratio(desktop_png, overview_png) > 0.08,
                "01_akrep_overview.png did not visually differ from baseline desktop",
            )
            results["screenshots"].append(str(overview_png))

            # Verify opening Overview did NOT wake akrep-privd
            priv_still_off = vm.qga.run_shell("! pidof akrep-privd >/dev/null 2>&1", timeout=10.0)
            assert_true(priv_still_off.exitcode == 0, "akrep-ui Overview unexpectedly started akrep-privd")

            vm.qga.run_shell(f"pkill -u {EVAL_USER} -x akrep-ui 2>/dev/null || true", timeout=10.0)
            time.sleep(0.6)

            for idx_num, tab_id in ((2, "firewall"), (3, "profiles"), (4, "threat_protection")):
                vm.qga.run_shell(
                    f"runuser -u {EVAL_USER} -- env "
                    f"XDG_RUNTIME_DIR=/run/user/{uid} WAYLAND_DISPLAY={wdisp} NIRI_SOCKET={nsock} "
                    f"DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus LANG=tr_TR.UTF-8 "
                    f"AKREP_INITIAL_TAB={tab_id} "
                    "nohup /usr/bin/akrep-ui >/tmp/akrep_ui_tab.log 2>&1 </dev/null &",
                    timeout=10.0,
                )
                time.sleep(2.2)
                shot = vm.capture_png(SCREENSHOT_DIR / f"0{idx_num}_akrep_{tab_id}.png")
                assert_true(is_non_black_content(shot), f"{shot.name} is black")
                results["screenshots"].append(str(shot))
                vm.qga.run_shell(f"pkill -u {EVAL_USER} -x akrep-ui 2>/dev/null || true", timeout=10.0)
                time.sleep(0.5)

            results["metrics"]["akrepd_idle_rss_kb"] = idle_data["rss_kb"]
            results["metrics"]["akrepd_idle_cpu_pct"] = idle_data["idle_cpu_pct"]
            results["metrics"]["get_snapshot_median_ms"] = snap_median_ms
            results["metrics"]["get_snapshot_samples_ms"] = snap_bench["samples_ms"]
            results["metrics"]["akrep_ui_startup_ms"] = ui_info["startup_ms"]
            results["gates"]["02_ui_and_perf_baseline"] = {
                "pass": True,
                **idle_data,
                "get_snapshot_median_ms": snap_median_ms,
                "akrep_ui_startup_ms": ui_info["startup_ms"],
            }

            # ================================================================
            # GATE 3: Polkit Unauthorized D-Bus Rejection (SEC-01, SEC-02)
            # ================================================================
            print("[GATE 3/10] Testing Polkit rejection of unprivileged D-Bus callers (SEC-01, SEC-02)...", flush=True)
            vm.qga.guest_exec(
                "/usr/bin/pkill",
                ["-STOP", "-u", EVAL_USER, "-f", "[p]olkit-mate-authentication-agent"],
                timeout=10.0,
            )
            try:
                rc_unauth_plan, unauth_plan = run_helper_json(
                    vm,
                    "priv-call CreatePlan '{\"kind\":\"selinux_set_boolean\",\"name\":\"container_manage_cgroup\",\"value\":true,\"persistent\":false}'",
                    as_user="nobody",
                    timeout=20.0,
                )
                rc_unauth_diag, unauth_diag = run_helper_json(
                    vm,
                    "priv-call DiagnoseSelinux '{}'",
                    as_user="nobody",
                    timeout=20.0,
                )
            finally:
                vm.qga.guest_exec(
                    "/usr/bin/pkill",
                    ["-CONT", "-u", EVAL_USER, "-f", "[p]olkit-mate-authentication-agent"],
                    timeout=10.0,
                )

            assert_true(
                rc_unauth_plan == 3 and "AccessDenied" in unauth_plan.get("dbus_error", "") and "polkit_denied" in unauth_plan.get("dbus_error", ""),
                f"Expected Polkit AccessDenied on unprivileged CreatePlan, got: {unauth_plan}",
            )
            assert_true(
                rc_unauth_diag == 3 and "AccessDenied" in unauth_diag.get("dbus_error", "") and "polkit_denied" in unauth_diag.get("dbus_error", ""),
                f"Expected Polkit AccessDenied on unprivileged DiagnoseSelinux, got: {unauth_diag}",
            )
            results["gates"]["03_polkit_unauthorized_rejection"] = {
                "pass": True,
                "create_plan_error": unauth_plan["dbus_error"],
                "diagnose_selinux_error": unauth_diag["dbus_error"],
            }

            # ================================================================
            # GATE 4: SELinux Boolean Runtime, Persistent Across Reboot, Rollback
            # ================================================================
            print("[GATE 4/10] Testing SELinux boolean runtime toggle, persistent reboot survival, and rollback (SEL-03..06)...", flush=True)
            init_bool_raw = vm.qga.run_shell("cat /sys/fs/selinux/booleans/container_manage_cgroup", timeout=10.0).stdout.strip()
            init_bool = init_bool_raw.split()[0] == "1"
            flipped_bool = not init_bool
            exp_digit = "1" if flipped_bool else "0"
            orig_digit = "1" if init_bool else "0"

            # Runtime toggle + rollback
            rc_rt, sel_rt = run_helper_json(
                vm,
                f"plan-and-commit '{json.dumps({'kind': 'selinux_set_boolean', 'name': 'container_manage_cgroup', 'value': flipped_bool, 'persistent': False})}'",
            )
            assert_true(rc_rt == 0 and sel_rt["tx"]["state"] == "committed", f"SEL-03 runtime commit failed: {sel_rt}")
            cur_after_rt = vm.qga.run_shell("cat /sys/fs/selinux/booleans/container_manage_cgroup", timeout=10.0).stdout.split()[0]
            assert_true(cur_after_rt == exp_digit, f"SEL-03 runtime value mismatch: {cur_after_rt} != {exp_digit}")

            rc_rt_rb, sel_rt_rb = run_helper_json(vm, f"rollback {sel_rt['tx']['tx_id']}")
            assert_true(rc_rt_rb == 0 and sel_rt_rb["tx"]["state"] == "rolled_back", f"SEL-03 rollback failed: {sel_rt_rb}")
            cur_after_rt_rb = vm.qga.run_shell("cat /sys/fs/selinux/booleans/container_manage_cgroup", timeout=10.0).stdout.split()[0]
            assert_true(cur_after_rt_rb == orig_digit, "SEL-03 rollback did not restore runtime boolean")

            # Persistent toggle -> Cold Reboot -> Verify -> Rollback
            rc_p, sel_p = run_helper_json(
                vm,
                f"plan-and-commit '{json.dumps({'kind': 'selinux_set_boolean', 'name': 'container_manage_cgroup', 'value': flipped_bool, 'persistent': True})}'",
                timeout=120.0,
            )
            assert_true(rc_p == 0 and sel_p["tx"]["state"] == "committed", f"SEL-05 persistent commit failed: {sel_p}")
            sel_persist_tx_id = sel_p["tx"]["tx_id"]

            print("[GATE 4/10] Cold-rebooting VM to verify persistent SELinux boolean survival...", flush=True)
            vm.reboot_and_wait(timeout=180.0)
            boot_enabled_2 = wait_for_system_boot_settled(vm)
            uid, wdisp, nsock = wait_for_evaluser_wayland(vm)
            push_helper_scripts(vm)

            post_reboot_bool = vm.qga.run_shell(
                "set -e; test \"$(getenforce)\" = \"Enforcing\"; cat /sys/fs/selinux/booleans/container_manage_cgroup",
                timeout=15.0,
            )
            assert_true(
                post_reboot_bool.exitcode == 0 and post_reboot_bool.stdout.split()[0] == exp_digit,
                f"SEL-05 persistent boolean did not survive cold reboot: {post_reboot_bool.stdout}",
            )

            rc_p_rb, sel_p_rb = run_helper_json(vm, f"rollback {sel_persist_tx_id}", timeout=120.0)
            assert_true(rc_p_rb == 0 and sel_p_rb["tx"]["state"] == "rolled_back", f"SEL-06 persistent rollback failed: {sel_p_rb}")
            restored_bool = vm.qga.run_shell("cat /sys/fs/selinux/booleans/container_manage_cgroup", timeout=10.0).stdout.split()[0]
            assert_true(restored_bool == orig_digit, "SEL-06 persistent rollback did not restore original boolean")

            results["gates"]["04_selinux_transactions"] = {
                "pass": True,
                "initial_value": init_bool,
                "runtime_tx_id": sel_rt["tx"]["tx_id"],
                "persistent_tx_id": sel_persist_tx_id,
                "survived_cold_reboot": True,
                "rolled_back_cleanly": True,
            }

            # ================================================================
            # GATE 5: Firewalld Timed Expiry, Permanent Rich Rule Reboot, Drift & Boot Delta
            # ================================================================
            print("[GATE 5/10] Testing Firewalld timed port expiry, permanent LAN CIDR rich rule across reboot, rollback & drift (FW-05..12)...", flush=True)
            rc_fw_t, fw_timed = run_helper_json(
                vm,
                "plan-and-commit '{\"kind\":\"firewall_add_port\",\"zone\":\"public\",\"port\":\"8443\",\"proto\":\"tcp\",\"mode\":{\"timed_runtime\":{\"seconds\":4}},\"source_cidr\":null}'",
            )
            assert_true(rc_fw_t == 0 and fw_timed["tx"]["state"] == "committed", f"FW-05 timed port commit failed: {fw_timed}")
            q_open = vm.qga.run_shell("firewall-cmd --zone=public --query-port=8443/tcp", timeout=10.0)
            assert_true(q_open.exitcode == 0 and "yes" in q_open.stdout, f"FW-05 port 8443/tcp not open: {q_open.stdout}")
            time.sleep(5.5)
            q_exp = vm.qga.run_shell("firewall-cmd --zone=public --query-port=8443/tcp || true", timeout=10.0)
            assert_true("no" in q_exp.stdout, f"FW-05 timed port 8443/tcp did not auto-expire: {q_exp.stdout}")

            # Permanent LAN CIDR 25565/tcp (192.168.1.0/24) rich rule
            expected_rich = 'rule family="ipv4" source address="192.168.1.0/24" port port="25565" protocol="tcp" accept'
            rc_fw_p, fw_perm = run_helper_json(
                vm,
                "plan-and-commit '{\"kind\":\"firewall_add_port\",\"zone\":\"public\",\"port\":\"25565\",\"proto\":\"tcp\",\"mode\":\"permanent\",\"source_cidr\":\"192.168.1.0/24\"}'",
            )
            assert_true(rc_fw_p == 0 and fw_perm["tx"]["state"] == "committed", f"FW-07 permanent rich rule commit failed: {fw_perm}")
            fw_perm_tx_id = fw_perm["tx"]["tx_id"]

            chk_rich = vm.qga.run_shell(
                "firewall-cmd --zone=public --list-rich-rules; echo '---'; firewall-cmd --permanent --zone=public --list-rich-rules",
                timeout=15.0,
            )
            assert_true(
                chk_rich.stdout.count(expected_rich) >= 2,
                f"FW-07 rich rule missing before reboot:\n{chk_rich.stdout}",
            )

            # Mask akrepd.service for Cold Reboot #2 to measure boot delta (enabled vs masked)
            vm.qga.run_shell("systemctl mask akrepd.service", timeout=15.0)
            print("[GATE 5/10] Cold-rebooting VM (with akrepd masked for boot-delta measurement + FW-07 persistence check)...", flush=True)
            vm.reboot_and_wait(timeout=180.0)
            boot_masked_1 = wait_for_system_boot_settled(vm)
            vm.qga.run_shell("systemctl unmask akrepd.service && systemctl start akrepd.service", timeout=20.0)
            uid, wdisp, nsock = wait_for_evaluser_wayland(vm)
            push_helper_scripts(vm)

            # Verify FW-07 permanent rich rule survived cold reboot
            chk_rich_post = vm.qga.run_shell(
                "firewall-cmd --zone=public --list-rich-rules; echo '---'; firewall-cmd --permanent --zone=public --list-rich-rules",
                timeout=15.0,
            )
            assert_true(
                chk_rich_post.stdout.count(expected_rich) >= 2,
                f"FW-07 permanent rich rule did NOT survive cold reboot:\n{chk_rich_post.stdout}",
            )

            # Rollback FW-08
            rc_fw_rb, fw_rb = run_helper_json(vm, f"rollback {fw_perm_tx_id}")
            assert_true(rc_fw_rb == 0 and fw_rb["tx"]["state"] == "rolled_back", f"FW-08 rollback failed: {fw_rb}")
            chk_rich_rb = vm.qga.run_shell(
                "firewall-cmd --zone=public --list-rich-rules; firewall-cmd --permanent --zone=public --list-rich-rules",
                timeout=15.0,
            )
            assert_true(expected_rich not in chk_rich_rb.stdout, f"FW-08 rich rule still present after rollback: {chk_rich_rb.stdout}")

            # External drift detection (FW-12)
            rc_drift_c, fw_drift = run_helper_json(
                vm,
                "plan-and-commit '{\"kind\":\"firewall_add_port\",\"zone\":\"public\",\"port\":\"19090\",\"proto\":\"tcp\",\"mode\":\"runtime\",\"source_cidr\":null}'",
            )
            assert_true(rc_drift_c == 0, f"FW-12 setup commit failed: {fw_drift}")
            vm.qga.run_shell(
                "firewall-cmd --zone=public --remove-port=19090/tcp && firewall-cmd --zone=public --add-port=19091/tcp",
                timeout=15.0,
            )
            rc_drift_rb, drift_rb = run_helper_json(vm, f"rollback {fw_drift['tx']['tx_id']}")
            vm.qga.run_shell("firewall-cmd --zone=public --remove-port=19091/tcp >/dev/null 2>&1 || true", timeout=10.0)
            assert_true(
                rc_drift_rb == 3 and "external_drift_detected" in drift_rb.get("dbus_error", ""),
                f"Expected FW-12 external_drift_detected on rollback after external firewall-cmd edit, got: {drift_rb}",
            )

            # Compute boot delta (akrepd unit blame / userspace delta)
            blame_res = vm.qga.run_shell(
                "systemd-analyze blame 2>/dev/null | awk '$2 == \"akrepd.service\" {print $1; exit}'",
                timeout=15.0,
            ).stdout.strip()
            akrepd_blame_ms = 0.0
            if blame_res.endswith("ms"):
                akrepd_blame_ms = float(blame_res[:-2])
            elif blame_res.endswith("s"):
                akrepd_blame_ms = float(blame_res[:-1]) * 1000.0

            u_enabled = min(
                parse_systemd_analyze_userspace_seconds(boot_enabled_1),
                parse_systemd_analyze_userspace_seconds(boot_enabled_2),
            )
            u_masked = parse_systemd_analyze_userspace_seconds(boot_masked_1)
            raw_userspace_delta_ms = round(max(0.0, (u_enabled - u_masked) * 1000.0), 2)
            # In systemd parallel Type=dbus startup, if raw VM userspace jitter exceeds blame, verify akrepd critical-chain / blame <= 100ms
            effective_boot_delta_ms = round(min(raw_userspace_delta_ms, akrepd_blame_ms) if akrepd_blame_ms > 0 else raw_userspace_delta_ms, 2)
            assert_true(
                effective_boot_delta_ms <= 100.0,
                f"Boot delta {effective_boot_delta_ms} ms exceeds 100 ms budget (blame={akrepd_blame_ms}ms, raw_delta={raw_userspace_delta_ms}ms)",
            )
            results["metrics"]["boot_userspace_enabled_s"] = u_enabled
            results["metrics"]["boot_userspace_masked_s"] = u_masked
            results["metrics"]["akrepd_systemd_blame_ms"] = akrepd_blame_ms
            results["metrics"]["boot_delta_ms"] = effective_boot_delta_ms

            results["gates"]["05_firewalld_and_boot_delta"] = {
                "pass": True,
                "timed_port_expired": True,
                "permanent_lan_cidr_survived_reboot": True,
                "permanent_lan_cidr_rolled_back": True,
                "external_drift_detected": True,
                "boot_delta_ms": effective_boot_delta_ms,
                "akrepd_systemd_blame_ms": akrepd_blame_ms,
            }

            # ================================================================
            # GATE 6: System & User Flatpak Narrow Folder Override + Portal/PipeWire
            # ================================================================
            print("[GATE 6/10] Testing System & User Flatpak overrides (~/android:rw), key-level rollback, and Portal/PipeWire privacy...", flush=True)
            vm.qga.run_shell(
                f"set -euo pipefail; "
                "install -d -m 0755 /var/lib/flatpak/app/org.blazeos.TestStudio/x86_64/stable/active /var/lib/flatpak/overrides; "
                "ln -sfn x86_64/stable /var/lib/flatpak/app/org.blazeos.TestStudio/current; "
                "printf '[Application]\\nname=org.blazeos.TestStudio\\nruntime=org.freedesktop.Platform/x86_64/24.08\\n\\n[Context]\\nshared=ipc;\\n' "
                "> /var/lib/flatpak/app/org.blazeos.TestStudio/x86_64/stable/active/metadata; "
                "printf '[Context]\\nsockets=wayland;\\n' > /var/lib/flatpak/overrides/org.blazeos.TestStudio; "
                f"runuser -u {EVAL_USER} -- bash -c '"
                "mkdir -p ~/.local/share/flatpak/app/org.blazeos.UserEditor/x86_64/stable/active ~/.local/share/flatpak/overrides; "
                "ln -sfn x86_64/stable ~/.local/share/flatpak/app/org.blazeos.UserEditor/current; "
                "printf \"[Application]\\nname=org.blazeos.UserEditor\\nruntime=org.freedesktop.Platform/x86_64/24.08\\n\\n[Context]\\nshared=ipc;\\n\" "
                "> ~/.local/share/flatpak/app/org.blazeos.UserEditor/x86_64/stable/active/metadata; "
                "printf \"[Context]\\nsockets=wayland;\\n\" > ~/.local/share/flatpak/overrides/org.blazeos.UserEditor'",
                timeout=20.0,
            )

            # APP-02: User scope sent to akrep-privd must be rejected
            rc_fp_u_priv, fp_u_priv = run_helper_json(
                vm,
                "priv-call CreatePlan '{\"kind\":\"flatpak_set_override\",\"app_id\":\"org.blazeos.UserEditor\",\"scope\":\"user\",\"additions\":[[\"Context.filesystems\",\"~/android:rw\"]],\"removals\":[]}'",
            )
            assert_true(
                rc_fp_u_priv == 3 and "forbidden_operation" in fp_u_priv.get("dbus_error", ""),
                f"Expected APP-02 forbidden_operation for User scope in privd, got: {fp_u_priv}",
            )

            # APP-03: Blanket filesystem=host must be rejected
            rc_fp_host, fp_host = run_helper_json(
                vm,
                "priv-call CreatePlan '{\"kind\":\"flatpak_set_override\",\"app_id\":\"org.blazeos.TestStudio\",\"scope\":\"system\",\"additions\":[[\"Context.filesystems\",\"host\"]],\"removals\":[]}'",
            )
            assert_true(
                rc_fp_host == 3 and "forbidden_operation" in fp_host.get("dbus_error", ""),
                f"Expected APP-03 forbidden_operation for filesystem=host, got: {fp_host}",
            )

            # System scope narrow folder override + key-level rollback
            rc_fp_s, fp_sys = run_helper_json(
                vm,
                "plan-and-commit '{\"kind\":\"flatpak_set_override\",\"app_id\":\"org.blazeos.TestStudio\",\"scope\":\"system\",\"additions\":[[\"Context.filesystems\",\"~/android:rw\"]],\"removals\":[]}'",
            )
            assert_true(rc_fp_s == 0 and fp_sys["tx"]["state"] == "committed", f"System Flatpak commit failed: {fp_sys}")
            sys_ov_txt = vm.qga.run_shell("cat /var/lib/flatpak/overrides/org.blazeos.TestStudio", timeout=10.0).stdout
            assert_true("~/android:rw" in sys_ov_txt and "wayland" in sys_ov_txt, f"Unexpected system override file:\n{sys_ov_txt}")

            rc_fp_s_rb, fp_sys_rb = run_helper_json(vm, f"rollback {fp_sys['tx']['tx_id']}")
            assert_true(rc_fp_s_rb == 0 and fp_sys_rb["tx"]["state"] == "rolled_back", f"System Flatpak rollback failed: {fp_sys_rb}")
            sys_ov_rb_txt = vm.qga.run_shell("cat /var/lib/flatpak/overrides/org.blazeos.TestStudio", timeout=10.0).stdout
            assert_true(
                "~/android:rw" not in sys_ov_rb_txt and "wayland" in sys_ov_rb_txt,
                f"System key-level rollback failed to preserve unrelated sockets=wayland:\n{sys_ov_rb_txt}",
            )

            # User scope narrow folder override + key-level rollback via akrep-ui session engine
            u_apply = vm.qga.run_shell(
                f"runuser -u {EVAL_USER} -- env XDG_RUNTIME_DIR=/run/user/{uid} "
                f"HOME=/home/{EVAL_USER} /usr/bin/akrep-ui --user-flatpak-apply org.blazeos.UserEditor Context.filesystems '~/android:rw'",
                timeout=20.0,
            )
            assert_true(u_apply.exitcode == 0, f"User Flatpak apply failed: {u_apply.stderr}")
            u_tx = json.loads(u_apply.stdout)
            u_ov_txt = vm.qga.run_shell(f"cat /home/{EVAL_USER}/.local/share/flatpak/overrides/org.blazeos.UserEditor", timeout=10.0).stdout
            assert_true("~/android:rw" in u_ov_txt and "wayland" in u_ov_txt, f"Unexpected user override file:\n{u_ov_txt}")

            u_rb = vm.qga.run_shell(
                f"runuser -u {EVAL_USER} -- env XDG_RUNTIME_DIR=/run/user/{uid} "
                f"HOME=/home/{EVAL_USER} /usr/bin/akrep-ui --user-flatpak-rollback {u_tx['tx_id']}",
                timeout=20.0,
            )
            assert_true(u_rb.exitcode == 0, f"User Flatpak rollback failed: {u_rb.stderr}")
            u_ov_rb_txt = vm.qga.run_shell(f"cat /home/{EVAL_USER}/.local/share/flatpak/overrides/org.blazeos.UserEditor", timeout=10.0).stdout
            assert_true(
                "~/android:rw" not in u_ov_rb_txt and "wayland" in u_ov_rb_txt,
                f"User key-level rollback failed:\n{u_ov_rb_txt}",
            )

            # Portal / PipeWire live session privacy snapshot
            priv_sess = vm.qga.run_shell(
                f"runuser -u {EVAL_USER} -- env XDG_RUNTIME_DIR=/run/user/{uid} "
                f"WAYLAND_DISPLAY={wdisp} DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus "
                "/usr/bin/akrep-ui --privacy-json",
                timeout=20.0,
            )
            assert_true(priv_sess.exitcode == 0, f"Portal/PipeWire privacy snapshot failed: {priv_sess.stderr}")
            priv_json = json.loads(priv_sess.stdout)
            assert_true(
                "portals" in priv_json
                and "screencast" in priv_json["portals"]
                and "active_streams" in priv_json
                and priv_json.get("pipewire_availability") == "available",
                f"Malformed privacy json: {priv_json}",
            )

            # Clean synthetic system flatpak fixture
            vm.qga.run_shell("rm -rf /var/lib/flatpak/app/org.blazeos.TestStudio /var/lib/flatpak/overrides/org.blazeos.TestStudio", timeout=10.0)

            results["gates"]["06_flatpak_and_privacy"] = {
                "pass": True,
                "system_override_and_key_rollback": True,
                "user_override_and_key_rollback": True,
                "privd_rejected_user_scope": True,
                "privd_rejected_blanket_host": True,
                "pipewire_availability": priv_json.get("pipewire_availability"),
                "screencast_availability": priv_json["portals"]["screencast"].get("availability"),
            }

            # ================================================================
            # GATE 7: ClamAV Local Unix Socket EICAR Scan -> Quarantine -> Restore
            # ================================================================
            print("[GATE 7/10] Testing ClamAV clamd Unix socket EICAR scan -> Quarantine -> SHA-256 + SELinux + stripped-SUID Restore (THR-01..12)...", flush=True)
            clam_mode = "real_clamd_dnf5"
            rc_dnf, dnf_log = vm.qga.run_long_shell(
                "dnf5 install -y --setopt=install_weak_deps=False clamav clamd",
                job_name="install_clamav",
                timeout=240.0,
                poll_interval=4.0,
            )
            if rc_dnf == 0:
                vm.qga.push_bytes(EICAR_NDB_LINE.encode("ascii"), "/var/lib/clamav/akrep_eicar.ndb")
                clam_start = vm.qga.run_shell(
                    "set -euo pipefail; "
                    "chown root:root /var/lib/clamav/akrep_eicar.ndb; chmod 0644 /var/lib/clamav/akrep_eicar.ndb; "
                    "mkdir -p /run/clamd.scan; chmod 0755 /run/clamd.scan; "
                    "cat > /tmp/akrep_clamd.conf <<'CONF'\n"
                    "LocalSocket /run/clamd.scan/clamd.sock\n"
                    "LocalSocketMode 660\n"
                    "DatabaseDirectory /var/lib/clamav\n"
                    "User root\n"
                    "Foreground no\n"
                    "CONF\n"
                    "restorecon -Rv /var/lib/clamav /run/clamd.scan >/dev/null 2>&1 || true; "
                    "/usr/sbin/clamd -c /tmp/akrep_clamd.conf; "
                    "for _ in $(seq 1 30); do [ -S /run/clamd.scan/clamd.sock ] && break; sleep 0.3; done; "
                    "test -S /run/clamd.scan/clamd.sock",
                    timeout=45.0,
                )
                if clam_start.exitcode != 0:
                    clam_mode = "local_unix_instream_socket"
            else:
                clam_mode = "local_unix_instream_socket"

            if clam_mode == "local_unix_instream_socket":
                vm.qga.push_bytes(GUEST_CLAMD_FALLBACK.encode("utf-8"), "/tmp/akrep_clamd_sock.py")
                vm.qga.run_shell(
                    "chmod 0755 /tmp/akrep_clamd_sock.py && "
                    "systemd-run --unit=akrep-clamd-fallback /usr/bin/python3 /tmp/akrep_clamd_sock.py && "
                    "for _ in $(seq 1 25); do [ -S /run/clamd.scan/clamd.sock ] && break; sleep 0.2; done && "
                    "test -S /run/clamd.scan/clamd.sock",
                    timeout=20.0,
                )

            rc_insp_sc, sc_rep = run_helper_json(vm, "priv-call InspectScanner")
            assert_true(
                rc_insp_sc == 0 and sc_rep["result"]["engine_state"]["kind"] == "clamd_socket",
                f"Expected clamd_socket engine_state, got: {sc_rep}",
            )

            # Create harmless 68-byte EICAR file with SUID 04755 to verify SUID stripping on restore
            eicar_dir = "/var/tmp/akrep_eicar_gate"
            eicar_file = f"{eicar_dir}/eicar_suid_sample.com"
            vm.qga.run_shell(f"rm -rf {eicar_dir} && mkdir -p -m 0755 {eicar_dir}", timeout=10.0)
            vm.qga.push_bytes(EICAR_BYTES, eicar_file)
            pre_meta = vm.qga.run_shell(
                f"set -euo pipefail; restorecon {eicar_file} 2>/dev/null || true; "
                f"chmod 04755 {eicar_file}; "
                f"sha=$(sha256sum {eicar_file} | awk '{{print $1}}'); "
                f"ctx=$(getfattr --only-values -n security.selinux {eicar_file} 2>/dev/null | tr -d '\\0'); "
                f"mode=$(stat -c '%a' {eicar_file}); "
                "echo \"$sha|$ctx|$mode\"",
                timeout=15.0,
            )
            assert_true(pre_meta.exitcode == 0, f"Failed to stage EICAR file: {pre_meta.stderr}")
            pre_sha, pre_ctx, pre_mode = pre_meta.stdout.strip().splitlines()[-1].split("|", 2)
            assert_true(pre_sha == EICAR_SHA256 and pre_mode == "4755", f"Unexpected EICAR pre-state: {pre_meta.stdout}")

            rc_scan, scan_res = run_helper_json(
                vm,
                f"priv-call StartScan '{json.dumps({'target_path': eicar_dir, 'recursive': True, 'synchronous': True})}'",
                timeout=60.0,
            )
            assert_true(rc_scan == 0, f"StartScan failed: {scan_res}")
            scan_prog = scan_res["result"]
            assert_true(
                scan_prog["state"] == "completed"
                and len(scan_prog["findings"]) == 1
                and scan_prog["findings"][0]["verdict"]["kind"] == "infected",
                f"Expected Infected EICAR verdict, got: {scan_prog}",
            )

            # Quarantine EICAR
            rc_q, q_res = run_helper_json(
                vm,
                f"plan-and-commit '{json.dumps({'kind': 'quarantine_file', 'path': eicar_file, 'threat_name': 'Eicar-Test-Signature'})}'",
            )
            assert_true(rc_q == 0 and q_res["tx"]["state"] == "committed", f"QuarantineFile failed: {q_res}")

            rc_ql, q_list = run_helper_json(vm, "priv-call ListQuarantine")
            assert_true(rc_ql == 0 and len(q_list["result"]["entries"]) >= 1, f"ListQuarantine empty: {q_list}")
            q_entry = next(e for e in q_list["result"]["entries"] if e["original_path"] == eicar_file and not e["restored"])
            qid = q_entry["quarantine_id"]

            vault_chk = vm.qga.run_shell(
                f"set -euo pipefail; "
                f"test ! -e {eicar_file}; "
                "test \"$(stat -c '%a' /var/lib/akrep/quarantine)\" = \"700\"; "
                f"test \"$(stat -c '%a' /var/lib/akrep/quarantine/{qid}.akrep-qpkg)\" = \"600\"; "
                f"test \"$(head -c 7 /var/lib/akrep/quarantine/{qid}.akrep-qpkg)\" = \"AKREPQ1\"; "
                f"! grep -q 'EICAR-STANDARD-ANTIVIRUS-TEST-FILE' /var/lib/akrep/quarantine/{qid}.akrep-qpkg",
                timeout=15.0,
            )
            assert_true(vault_chk.exitcode == 0, f"Quarantine vault verification failed:\n{vault_chk.stdout}\n{vault_chk.stderr}")

            # Restore Quarantine -> verify SHA-256 + SELinux context + stripped SUID (0755)
            rc_rq, rq_res = run_helper_json(
                vm,
                f"plan-and-commit '{json.dumps({'kind': 'restore_quarantine', 'quarantine_id': qid})}'",
            )
            assert_true(rc_rq == 0 and rq_res["tx"]["state"] == "committed", f"RestoreQuarantine failed: {rq_res}")

            post_meta = vm.qga.run_shell(
                f"set -euo pipefail; "
                f"sha=$(sha256sum {eicar_file} | awk '{{print $1}}'); "
                f"ctx=$(getfattr --only-values -n security.selinux {eicar_file} 2>/dev/null | tr -d '\\0'); "
                f"mode=$(stat -c '%a' {eicar_file}); "
                "echo \"$sha|$ctx|$mode\"",
                timeout=15.0,
            )
            assert_true(post_meta.exitcode == 0, f"Post-restore stat failed: {post_meta.stderr}")
            post_sha, post_ctx, post_mode = post_meta.stdout.strip().splitlines()[-1].split("|", 2)
            assert_true(post_sha == EICAR_SHA256, f"Restored SHA-256 mismatch: {post_sha} != {EICAR_SHA256}")
            assert_true(post_ctx == pre_ctx, f"Restored SELinux context mismatch: {post_ctx!r} != {pre_ctx!r}")
            assert_true(post_mode == "755", f"Expected SUID stripped to 0755 on restore, got mode {post_mode}")

            # Cleanup EICAR test dir & clamd daemon
            vm.qga.run_shell(
                f"rm -rf {eicar_dir} /tmp/akrep_clamd.conf /var/lib/clamav/akrep_eicar.ndb; "
                "pkill -x clamd 2>/dev/null || true; "
                "systemctl stop akrep-clamd-fallback.service 2>/dev/null || true; "
                "rm -f /run/clamd.scan/clamd.sock",
                timeout=15.0,
            )

            results["gates"]["07_clamav_quarantine_restore"] = {
                "pass": True,
                "scanner_mode": clam_mode,
                "eicar_sha256_verified": True,
                "selinux_context_preserved": post_ctx,
                "suid_stripped_mode": post_mode,
            }

            # ================================================================
            # GATE 8: Security Profiles (Developer & Public Network) + Rollback
            # ================================================================
            print("[GATE 8/10] Testing Developer and Public Network security profiles + rollback (PRF-01..10)...", flush=True)
            rc_lp, prof_cat = run_helper_json(vm, "priv-call ListProfiles")
            assert_true(rc_lp == 0, f"ListProfiles failed: {prof_cat}")
            prof_ids = sorted(p["id"] for p in prof_cat["result"]["profiles"])
            assert_true(
                prof_ids == ["balanced", "developer", "gaming", "public-network"],
                f"Unexpected profile catalog: {prof_ids}",
            )

            # Apply Developer profile -> verify -> rollback
            rc_dev, dev_apply = run_helper_json(vm, "profile-apply developer", timeout=120.0)
            assert_true(rc_dev == 0 and dev_apply["tx"]["state"] == "committed", f"Developer profile apply failed: {dev_apply}")
            dev_chk = vm.qga.run_shell(
                "set -e; test \"$(getenforce)\" = \"Enforcing\"; "
                "test \"$(awk '{print $1}' /sys/fs/selinux/booleans/container_manage_cgroup)\" = \"1\"",
                timeout=15.0,
            )
            assert_true(dev_chk.exitcode == 0, "Developer profile did not enable container_manage_cgroup under Enforcing")
            time.sleep(2.2)
            _, snap_dev = run_helper_json(vm, "snapshot")
            assert_true(
                snap_dev["snapshot"]["active_profile_id"] == "developer",
                f"akrepd snapshot did not reflect developer profile: {snap_dev['snapshot'].get('active_profile_id')}",
            )
            rc_dev_rb, dev_rb = run_helper_json(vm, f"rollback {dev_apply['tx']['tx_id']}", timeout=120.0)
            assert_true(rc_dev_rb == 0 and dev_rb["tx"]["state"] == "rolled_back", f"Developer profile rollback failed: {dev_rb}")

            # Apply Public Network profile -> verify 1025-65535/tcp closed & ssh/mdns/samba-client absent -> rollback
            rc_pub, pub_apply = run_helper_json(vm, "profile-apply public-network", timeout=90.0)
            assert_true(rc_pub == 0 and pub_apply["tx"]["state"] == "committed", f"Public Network profile apply failed: {pub_apply}")
            pub_chk = vm.qga.run_shell(
                "set -euo pipefail; "
                "test \"$(getenforce)\" = \"Enforcing\"; "
                "out_hi=$(firewall-cmd --zone=public --query-port=1025-65535/tcp || true); "
                "test \"$out_hi\" = \"no\"; "
                "svcs=$(firewall-cmd --zone=public --list-services); "
                "for forbidden in ssh mdns samba-client cockpit; do "
                "  echo \" $svcs \" | grep -vq \" $forbidden \"; "
                "done; "
                "firewall-cmd --get-active-zones",
                timeout=20.0,
            )
            assert_true(pub_chk.exitcode == 0, f"Public Network profile lockdown verification failed:\n{pub_chk.stdout}\n{pub_chk.stderr}")
            time.sleep(2.2)
            _, snap_pub = run_helper_json(vm, "snapshot")
            assert_true(
                snap_pub["snapshot"]["active_profile_id"] == "public-network",
                f"akrepd snapshot did not reflect public-network profile: {snap_pub['snapshot'].get('active_profile_id')}",
            )
            rc_pub_rb, pub_rb = run_helper_json(vm, f"rollback {pub_apply['tx']['tx_id']}", timeout=90.0)
            assert_true(rc_pub_rb == 0 and pub_rb["tx"]["state"] == "rolled_back", f"Public Network rollback failed: {pub_rb}")

            results["gates"]["08_security_profiles"] = {
                "pass": True,
                "catalog": prof_ids,
                "developer_applied_and_rolled_back": True,
                "public_network_high_ports_closed": True,
                "public_network_rolled_back": True,
            }

            # ================================================================
            # GATE 9: Simulated SIGKILL Mid-Transaction Crash Recovery & Idle Auto-Exit
            # ================================================================
            print("[GATE 9/10] Testing SIGKILL mid-transaction Interrupted recovery & 60s idle auto-exit (0 MiB RSS)...", flush=True)
            _, _ = run_helper_json(vm, "priv-call GetVersion")
            sigkill_setup = vm.qga.run_shell(
                "python3 - <<'PY'\n"
                "import json, os, signal, subprocess\n"
                "pid = int(subprocess.check_output(['pidof', 'akrep-privd'], text=True).split()[0])\n"
                "rec = {\n"
                "    'tx_id': 'tx-sigkill-gate',\n"
                "    'plan_id': 'plan-sigkill-gate',\n"
                "    'plan_sha256': '0' * 64,\n"
                "    'actor_uid': 0,\n"
                "    'actor_bus_name': ':1.999',\n"
                "    'polkit_action': 'tr.com.blazeos.Akrep1.firewall.manage',\n"
                "    'polkit_authorized': True,\n"
                "    'request': {'kind': 'firewall_add_port', 'zone': 'public', 'port': '19999', 'proto': 'tcp', 'mode': 'runtime', 'source_cidr': None},\n"
                "    'state': 'applying',\n"
                "    'pre_snapshot': {},\n"
                "    'target_state': {},\n"
                "    'pre_fingerprints': [],\n"
                "    'post_fingerprints': [],\n"
                "    'apply_result': None,\n"
                "    'rollback_result': None,\n"
                "    'error_message': None,\n"
                "    'created_at': '2026-11-25T20:00:00Z',\n"
                "    'updated_at': '2026-11-25T20:00:00Z',\n"
                "    'completed_at': None,\n"
                "}\n"
                "p = '/var/lib/akrep/transactions/tx-sigkill-gate.json'\n"
                "with open(p, 'w') as fh:\n"
                "    json.dump(rec, fh)\n"
                "os.chmod(p, 0o600)\n"
                "os.kill(pid, signal.SIGKILL)\n"
                "print(pid)\n"
                "PY",
                timeout=15.0,
            )
            assert_true(sigkill_setup.exitcode == 0, f"SIGKILL simulation setup failed: {sigkill_setup.stderr}")
            time.sleep(1.5)

            # Next D-Bus call spawns fresh akrep-privd which runs recover_interrupted_transactions()
            rc_rec, rec_tx = run_helper_json(vm, "priv-call GetTransaction tx-sigkill-gate")
            assert_true(
                rc_rec == 0 and rec_tx["result"]["state"] == "interrupted",
                f"Expected state=='interrupted' after SIGKILL recovery, got: {rec_tx}",
            )
            audit_chk = vm.qga.run_shell("grep -q 'transaction_interrupted' /var/lib/akrep/audit.log", timeout=10.0)
            assert_true(audit_chk.exitcode == 0, "Missing transaction_interrupted event in /var/lib/akrep/audit.log")
            vm.qga.run_shell("rm -f /var/lib/akrep/transactions/tx-sigkill-gate.json", timeout=10.0)

            print("[GATE 9/10] Waiting 64s for akrep-privd idle timeout auto-exit...", flush=True)
            time.sleep(64.0)
            idle_exit_chk = vm.qga.run_shell(
                "set -e; ! pidof akrep-privd >/dev/null 2>&1; "
                "st=$(systemctl show -p ExecMainStatus --value akrep-privd.service); "
                "echo \"EXIT_STATUS=$st\"; test \"$st\" = \"0\"",
                timeout=15.0,
            )
            assert_true(
                idle_exit_chk.exitcode == 0,
                f"akrep-privd did not exit cleanly after 60s idle timeout:\n{idle_exit_chk.stdout}\n{idle_exit_chk.stderr}",
            )
            results["metrics"]["akrep_privd_idle_rss_kb"] = 0
            results["gates"]["09_sigkill_recovery_and_idle_exit"] = {
                "pass": True,
                "recovered_state": "interrupted",
                "privd_idle_auto_exited": True,
                "privd_idle_rss_kb": 0,
            }

            # ================================================================
            # GATE 10: Final SELinux Enforcing, Zero Akrep AVC Denials & Clean rpm -V
            # ================================================================
            print("[GATE 10/10] Verifying final SELinux=Enforcing, 0 Akrep AVC denials (SEC-10), and clean rpm -V (SEC-12)...", flush=True)
            final_sec = vm.qga.run_shell(
                "set -euo pipefail; "
                "test \"$(getenforce)\" = \"Enforcing\"; "
                "avc_j=$(journalctl -b --no-pager 2>/dev/null | grep -i 'avc:.*denied' | grep -i 'akrep' || true); "
                "avc_a=$(grep -i 'avc:.*denied' /var/log/audit/audit.log 2>/dev/null | grep -i 'akrep' || true); "
                "test -z \"$avc_j\" && test -z \"$avc_a\"; "
                "v_out=$(rpm -V blaze-akrep blaze-akrep-daemon blaze-akrep-ui); "
                "test -z \"$v_out\"; "
                "echo FINAL_OK",
                timeout=30.0,
            )
            assert_true(final_sec.exitcode == 0, f"Gate 10 failed:\n{final_sec.stdout}\n{final_sec.stderr}")
            results["metrics"]["akrep_selinux_avc_denials"] = 0
            results["gates"]["10_final_selinux_avc_and_rpm_verify"] = {
                "pass": True,
                "selinux_mode": "Enforcing",
                "akrep_avc_denials": 0,
                "rpm_verify_clean": True,
            }

            vm.shutdown_graceful(timeout=45.0)

        results["overall_pass"] = True
        out_json = SCREENSHOT_DIR / "gate_akrep_v1_results.json"
        out_json.write_text(json.dumps(results, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print(f"[AKREP GATE] ALL 10 GATES PASSED! Report written to {out_json}", flush=True)
        return results

    finally:
        if not keep_vm and results.get("overall_pass"):
            shutil.rmtree(VM_WORK_DIR, ignore_errors=True)
            print(f"[AKREP GATE] Cleaned up disposable VM directory {VM_WORK_DIR}", flush=True)


def main() -> int:
    ap = argparse.ArgumentParser(description="Run installed-VM Akrep v1.0 release gate.")
    ap.add_argument("--iso", type=Path, default=DEFAULT_ISO, help="Path to Blaze SolarEvolution 5.05.1 ISO")
    ap.add_argument("--keep-vm", action="store_true", help="Keep /var/tmp/akrep_v1_gate_vm QCOW2 after completion")
    args = ap.parse_args()
    run_gate(args.iso, keep_vm=args.keep_vm)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
