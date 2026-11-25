#!/usr/bin/env python3
"""Phase 01 (Environment Inventory) & Phase 04 (Software Supply Chain SBOM & Grype Scan).

Runs on a pristine copy-on-write QCOW2 overlay of the installed Blaze SolarEvolution VM
WITHOUT installing any RPMs into the RPM database, ensuring:
1. Stock environment inventory (exact installed package count, services, kernel, graphics, boot).
2. Uncontaminated CycloneDX and SPDX SBOMs generated via official static Syft binary in /tmp.
3. Vulnerability scan via official static Grype binary in /tmp with DB metadata and timestamp.
"""
from __future__ import annotations

import json
import re
import shutil
import statistics
import sys
import tarfile
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from eval_vm import (
    DEFAULT_EVAL_CPUS,
    DEFAULT_EVAL_RAM_MB,
    EvalQemuVM,
    create_overlay_pair,
    target_vm_kwargs,
)

REPO_ROOT = Path(__file__).resolve().parents[2]
EVAL_ROOT = REPO_ROOT / "solarevolution-evaluation"
VM_STORE = Path("/var/tmp/blaze_eval_vms")
OVL_DIR = Path("/tmp/blaze_eval_overlays")

MAC_RE = re.compile(r"\b(?:[0-9a-fA-F]{2}:){5}[0-9a-fA-F]{2}\b")
PRIV_IPV4_RE = re.compile(
    r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}|172\.(?:1[6-9]|2\d|3[01])\.\d{1,3}\.\d{1,3}|192\.168\.\d{1,3}\.\d{1,3})(?:/\d{1,2})?\b"
)
IPV6_LL_RE = re.compile(r"\bfe80::[0-9a-fA-F:]+(?:/\d{1,3})?\b")


def sanitize_text(text: str) -> str:
    text = MAC_RE.sub("[REDACTED-MAC]", text)
    text = PRIV_IPV4_RE.sub("[REDACTED-PRIVATE-IP]", text)
    text = IPV6_LL_RE.sub("[REDACTED-LINK-LOCAL-IPV6]", text)
    text = text.replace("solarevolution-eval", "[REDACTED-HOSTNAME]")
    text = text.replace("fedora45-ref-eval", "[REDACTED-HOSTNAME]")
    text = text.replace("evaluser", "[REDACTED-USER]")
    return text


def parse_kv(text: str) -> dict[str, str]:
    out: dict[str, str] = {}
    for ln in text.splitlines():
        if "=" in ln and not ln.lstrip().startswith("#"):
            k, v = ln.split("=", 1)
            out[k.strip()] = v.strip().strip('"')
    return out


def parse_systemd_time_line(line: str) -> dict[str, float]:
    # Example: Startup finished in 2.154s (firmware) + 1.093s (loader) + 1.842s (kernel) + 2.311s (initrd) + 6.452s (userspace) = 13.852s
    def to_sec(val: str, unit: str) -> float:
        f = float(val)
        if unit == "ms":
            return round(f / 1000.0, 4)
        if unit == "min":
            return round(f * 60.0, 4)
        return round(f, 4)

    res: dict[str, float] = {}
    for m in re.finditer(r"(?:(\d+)min\s+)?(\d+(?:\.\d+)?)(ms|s)\s+\(([^)]+)\)", line):
        mins = float(m.group(1)) * 60.0 if m.group(1) else 0.0
        res[m.group(4).strip()] = round(mins + to_sec(m.group(2), m.group(3)), 4)
    tot = re.search(r"=\s*(?:(\d+)min\s+)?(\d+(?:\.\d+)?)(ms|s)", line)
    if tot:
        mins = float(tot.group(1)) * 60.0 if tot.group(1) else 0.0
        res["total"] = round(mins + to_sec(tot.group(2), tot.group(3)), 4)
    return res


def collect_target_inventory_and_sbom(target_name: str, base_disk: Path, base_vars: Path) -> None:
    if not base_disk.is_file() or not base_vars.is_file():
        raise FileNotFoundError(f"Missing pristine {target_name} base image: {base_disk}")

    suffix = "" if target_name == "blaze" else f"_{target_name}"
    raw_p1 = EVAL_ROOT / "raw-results" / ("phase01_inventory" if target_name == "blaze" else f"phase01_inventory_{target_name}")
    raw_p4 = EVAL_ROOT / "raw-results" / ("phase04_supply_chain" if target_name == "blaze" else f"phase04_supply_chain_{target_name}")
    raw_p1.mkdir(parents=True, exist_ok=True)
    raw_p4.mkdir(parents=True, exist_ok=True)
    (EVAL_ROOT / "metadata").mkdir(parents=True, exist_ok=True)
    (EVAL_ROOT / "sbom").mkdir(parents=True, exist_ok=True)

    grype_yaml_src = REPO_ROOT / ".grype.yaml"
    grype_yaml_txt = grype_yaml_src.read_text(encoding="utf-8") if grype_yaml_src.is_file() else 'distro: "fedora:45"\n'

    akrep_overhead: dict[str, Any] | None = None
    ovl_disk, ovl_vars = create_overlay_pair(base_disk, base_vars, OVL_DIR, f"p1_p4_{target_name}")
    try:
        with EvalQemuVM(
            disk_path=ovl_disk,
            ovmf_vars_path=ovl_vars,
            log_dir=raw_p1,
            vm_label=f"p1_p4_{target_name}",
            boot_order="c",
            memory_mb=DEFAULT_EVAL_RAM_MB,
            cpus=DEFAULT_EVAL_CPUS,
            **target_vm_kwargs(target_name),
        ) as vm:
            print(f"[PHASE 01:{target_name}] Booting pristine overlay...")
            vm.qga.wait_until_ready(timeout=210.0, poll_interval=2.0)
            vm.qga.run_shell("systemctl is-system-running --wait 2>/dev/null || sleep 8", timeout=60.0)
            time.sleep(3.0)

            collect_cmd = r"""
mkdir -p /tmp/p1_raw
cat /etc/os-release > /tmp/p1_raw/os-release.txt
cat /etc/fedora-release > /tmp/p1_raw/fedora-release.txt 2>/dev/null || true
cat /etc/blaze-release > /tmp/p1_raw/blaze-release.txt 2>/dev/null || true
cat /etc/blazeos-release > /tmp/p1_raw/blazeos-release.txt 2>/dev/null || true
rpm -q blaze-release fedora-release fedora-release-common > /tmp/p1_raw/release-rpms.txt 2>/dev/null || true
rpm -E '%fedora' > /tmp/p1_raw/fedora-base-ver.txt 2>/dev/null || true
BLAZE_RPMS="blaze-release blaze-sentinel blaze-tools blaze-recovery blaze-continuum blaze-house solarui solarui-config blaze-oobe blaze-firehub blaze-scap-compat blaze-akrep blaze-akrep-daemon blaze-akrep-ui"
rpm -q --qf '%{NAME}\t%{VERSION}-%{RELEASE}\t%{OPENPGP:pgpsig}\n' $BLAZE_RPMS > /tmp/p1_raw/blaze-first-party-rpms.txt 2>&1 || true
rpm -V $BLAZE_RPMS > /tmp/p1_raw/blaze-rpm-verify.txt 2>&1 || echo "RPM_V_RC=$?" >> /tmp/p1_raw/blaze-rpm-verify.txt
uname -a > /tmp/p1_raw/uname.txt
uname -r > /tmp/p1_raw/kernel-release.txt
uname -m > /tmp/p1_raw/arch.txt
hostnamectl > /tmp/p1_raw/hostnamectl.txt 2>&1 || true
lscpu -J > /tmp/p1_raw/lscpu.json 2>/dev/null || true
lscpu > /tmp/p1_raw/lscpu.txt 2>/dev/null || true
free -b > /tmp/p1_raw/free-bytes.txt
free -h > /tmp/p1_raw/free-human.txt
swapon --show --bytes > /tmp/p1_raw/swapon.txt 2>/dev/null || true
lsblk -J -o NAME,TYPE,SIZE,FSTYPE,LABEL,MOUNTPOINT,MODEL,RO > /tmp/p1_raw/lsblk.json
findmnt -J -l > /tmp/p1_raw/findmnt.json
if [ -d /sys/firmware/efi ]; then echo "UEFI" > /tmp/p1_raw/boot-mode.txt; else echo "BIOS" > /tmp/p1_raw/boot-mode.txt; fi
if command -v mokutil >/dev/null 2>&1; then mokutil --sb-state > /tmp/p1_raw/secure-boot.txt 2>&1 || true; else echo "mokutil_not_installed" > /tmp/p1_raw/secure-boot.txt; fi
if [ -r /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c ]; then
  od -An -t u1 /sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c > /tmp/p1_raw/secure-boot-efivar.txt 2>/dev/null || true
fi
rpm -qa --qf '%{NAME}\t%{VERSION}-%{RELEASE}\t%{ARCH}\n' | sort > /tmp/p1_raw/rpm-qa.txt
rpm -qa | wc -l > /tmp/p1_raw/rpm-count.txt
systemctl --version > /tmp/p1_raw/systemd-version.txt
systemctl list-units --type=service --state=running --no-pager --no-legend > /tmp/p1_raw/running-services.txt
systemctl --failed --no-pager --no-legend > /tmp/p1_raw/failed-units.txt
systemd-analyze time > /tmp/p1_raw/boot-time-1.txt 2>&1 || true
systemd-analyze critical-chain --no-pager > /tmp/p1_raw/critical-chain.txt 2>&1 || true
systemd-analyze blame --no-pager | head -n 40 > /tmp/p1_raw/blame-top40.txt 2>&1 || true
lspci -nnk > /tmp/p1_raw/lspci.txt 2>&1 || true
glxinfo -B > /tmp/p1_raw/glxinfo.txt 2>&1 || true
eglinfo -B > /tmp/p1_raw/eglinfo.txt 2>&1 || true
vulkaninfo --summary > /tmp/p1_raw/vulkaninfo.txt 2>&1 || true
ls -la /dev/dri > /tmp/p1_raw/dev-dri.txt 2>&1 || true
ip -j link show > /tmp/p1_raw/ip-link.json 2>/dev/null || true
nmcli -t -f DEVICE,TYPE,STATE,CONNECTION dev status > /tmp/p1_raw/nmcli.txt 2>/dev/null || true
firewall-cmd --state > /tmp/p1_raw/firewalld.txt 2>&1 || true
getenforce > /tmp/p1_raw/selinux.txt 2>&1 || true
if command -v inxi >/dev/null 2>&1; then inxi -Fxz --no-host > /tmp/p1_raw/inxi.txt 2>&1 || true; else echo "inxi_not_installed" > /tmp/p1_raw/inxi.txt; fi
tar -czf /tmp/p1_raw.tar.gz -C /tmp p1_raw
echo P1_DONE
"""
            res = vm.qga.run_shell(collect_cmd, timeout=90.0)
            assert res.exitcode == 0 and "P1_DONE" in res.stdout, f"P1 ({target_name}) collection failed: {res.stdout}\n{res.stderr}"
            vm.qga.pull_file("/tmp/p1_raw.tar.gz", raw_p1 / "p1_raw.tar.gz")

            # Also record 2 additional cold reboots for stock boot duration (3 runs total)
            boot_runs = []
            t1 = vm.qga.run_shell("systemd-analyze time", timeout=15.0).stdout.strip()
            boot_runs.append({"run": 1, "raw": t1, "parsed_seconds": parse_systemd_time_line(t1)})
            for run_idx in (2, 3):
                print(f"[PHASE 01:{target_name}] Performing clean cold reboot {run_idx}/3 for stock boot timing...")
                vm.reboot_and_wait(timeout=180.0)
                vm.qga.run_shell("systemctl is-system-running --wait 2>/dev/null || sleep 6", timeout=60.0)
                ti = vm.qga.run_shell("systemd-analyze time", timeout=15.0).stdout.strip()
                boot_runs.append({"run": run_idx, "raw": ti, "parsed_seconds": parse_systemd_time_line(ti)})

            # PHASE 04: Download static official Syft & Grype binaries into /tmp/eval_tools (ZERO RPMs installed!)
            print(f"[PHASE 04:{target_name}] Downloading official Syft & Grype static binaries into guest /tmp/eval_tools...")
            vm.qga.push_bytes(grype_yaml_txt.encode("utf-8"), "/tmp/eval_grype.yaml")
            p4_script = r"""
set -euo pipefail
mkdir -p /tmp/eval_tools /tmp/p4_out
mv /tmp/eval_grype.yaml /tmp/p4_out/.grype.yaml
curl -fsSL https://raw.githubusercontent.com/anchore/syft/main/install.sh | sh -s -- -b /tmp/eval_tools
curl -fsSL https://raw.githubusercontent.com/anchore/grype/main/install.sh | sh -s -- -b /tmp/eval_tools

/tmp/eval_tools/syft version -o json > /tmp/p4_out/syft_version.json 2>/dev/null || /tmp/eval_tools/syft version > /tmp/p4_out/syft_version.txt
/tmp/eval_tools/grype version -o json > /tmp/p4_out/grype_version.json 2>/dev/null || /tmp/eval_tools/grype version > /tmp/p4_out/grype_version.txt

# Full rootfs directory scan (indexes RPM DB, FireHub /opt binaries, Rust binaries, Python/system artifacts)
/tmp/eval_tools/syft scan dir:/ \
  --exclude './proc/**' \
  --exclude './sys/**' \
  --exclude './dev/**' \
  --exclude './run/**' \
  --exclude './tmp/**' \
  --exclude './var/tmp/**' \
  --exclude './var/cache/**' \
  --exclude './mnt/**' \
  --exclude './media/**' \
  --exclude './boot/efi/**' \
  -o cyclonedx-json=/tmp/p4_out/04_sbom_cyclonedx.json \
  -o spdx-json=/tmp/p4_out/04_sbom_spdx.json \
  > /tmp/p4_out/syft_scan.log 2>&1

# Update Grype vulnerability DB and record DB status & timestamp
/tmp/eval_tools/grype db update > /tmp/p4_out/grype_db_update.log 2>&1
/tmp/eval_tools/grype db status -o json > /tmp/p4_out/grype_db_status.json 2>/dev/null || /tmp/eval_tools/grype db status > /tmp/p4_out/grype_db_status.txt

# 1. Native Grype scan (autodetected distro from /etc/os-release)
/tmp/eval_tools/grype sbom:/tmp/p4_out/04_sbom_cyclonedx.json \
  -o json --file /tmp/p4_out/grype_native_autodetect.json \
  > /tmp/p4_out/grype_native_scan.log 2>&1 || true

# 2. Policy Grype scan using shipped .grype.yaml (distro: "fedora:45" for Blaze, native fedora:45 for Fedora)
/tmp/eval_tools/grype sbom:/tmp/p4_out/04_sbom_cyclonedx.json \
  -c /tmp/p4_out/.grype.yaml \
  -o json --file /tmp/p4_out/04_vulnerability_report.json \
  > /tmp/p4_out/grype_scan.log 2>&1 || true

test -s /tmp/p4_out/04_sbom_cyclonedx.json
test -s /tmp/p4_out/04_sbom_spdx.json
test -s /tmp/p4_out/04_vulnerability_report.json
tar -czf /tmp/p4_out.tar.gz -C /tmp p4_out
echo P4_DONE
"""
            rc, tail = vm.qga.run_long_shell(p4_script, f"phase04_sbom_grype_{target_name}", timeout=900.0, poll_interval=5.0)
            print(f"[PHASE 04:{target_name}] Finished rc={rc}\n{tail}")
            assert rc == 0, f"Phase 04 ({target_name}) failed (rc={rc}):\n{tail}"
            vm.qga.pull_file("/tmp/p4_out.tar.gz", raw_p4 / "p4_out.tar.gz")

            # Dedicated Akrep v0.1 live overhead benchmark on Blaze 5.05.1 RC (overlay only)
            if target_name == "blaze":
                print("[PHASE 01:blaze] Benchmarking Akrep v0.1 memory, idle CPU, D-Bus latency, and boot delta...")
                akrep_probe = vm.qga.run_shell(
                    r"""
python3 - <<'PY'
import json, subprocess, time, pathlib

def sh(cmd):
    return subprocess.run(cmd, shell=True, text=True, capture_output=True)

pid_s = sh("systemctl show -p MainPID --value akrepd.service").stdout.strip()
pid = int(pid_s) if pid_s.isdigit() else 0
status_txt = pathlib.Path(f"/proc/{pid}/status").read_text() if pid > 0 and pathlib.Path(f"/proc/{pid}/status").exists() else ""
vm_rss_kb = vm_hwm_kb = threads = None
for ln in status_txt.splitlines():
    if ln.startswith("VmRSS:"):
        vm_rss_kb = int(ln.split()[1])
    elif ln.startswith("VmHWM:"):
        vm_hwm_kb = int(ln.split()[1])
    elif ln.startswith("Threads:"):
        threads = int(ln.split()[1])

def cpu_ticks(p):
    if p <= 0 or not pathlib.Path(f"/proc/{p}/stat").exists():
        return None
    parts = pathlib.Path(f"/proc/{p}/stat").read_text().split()
    return int(parts[13]) + int(parts[14])

t_a = cpu_ticks(pid)
time.sleep(3.0)
t_b = cpu_ticks(pid)

bus_lat = {}
snap_count = None
for method in ("GetVersion", "GetCapabilities", "GetSnapshot"):
    t0 = time.perf_counter()
    r = sh(f"busctl --system call --json=short tr.com.blazeos.Akrep1 /tr/com/blazeos/Akrep1 tr.com.blazeos.Akrep1.Manager {method}")
    dt_ms = round((time.perf_counter() - t0) * 1000.0, 2)
    bus_lat[method] = {"rc": r.returncode, "latency_ms": dt_ms}
    if method == "GetSnapshot" and r.returncode == 0:
        try:
            payload = json.loads(r.stdout)["data"][0]
            snap = json.loads(payload)
            snap_count = len(snap.get("cards", []))
        except Exception:
            pass

sen = sh("blaze-sentinel --json")
sen_count = None
if sen.returncode == 0:
    try:
        sen_count = len(json.loads(sen.stdout).get("controls", []))
    except Exception:
        pass

avc = sh("ausearch -m avc -ts boot 2>&1 | grep -i akrep || true").stdout.strip()
print(json.dumps({
    "pid": pid,
    "active": sh("systemctl is-active akrepd.service").stdout.strip(),
    "enabled": sh("systemctl is-enabled akrepd.service").stdout.strip(),
    "user": sh("ps -o user= -p " + str(pid)).stdout.strip() if pid > 0 else "",
    "vm_rss_kb": vm_rss_kb,
    "vm_hwm_kb": vm_hwm_kb,
    "threads": threads,
    "idle_cpu_ticks_3s": (t_b - t_a) if (t_a is not None and t_b is not None) else None,
    "dbus_calls": bus_lat,
    "akrep_card_count": snap_count,
    "sentinel_control_count": sen_count,
    "selinux_avc_denials_akrep": 0 if not avc else len(avc.splitlines()),
}))
PY
""",
                    timeout=35.0,
                )
                probe_data = json.loads(akrep_probe.stdout.strip().splitlines()[-1]) if akrep_probe.exitcode == 0 and akrep_probe.stdout.strip() else {"error": akrep_probe.stderr}

                # Measure 2 cold reboots with akrepd masked to quantify startup cost accurately
                vm.qga.run_shell("systemctl mask akrepd.service", timeout=15.0)
                no_akrep_runs = []
                for idx in (1, 2):
                    vm.reboot_and_wait(timeout=180.0)
                    vm.qga.run_shell("systemctl is-system-running --wait 2>/dev/null || sleep 6", timeout=60.0)
                    ti = vm.qga.run_shell("systemd-analyze time", timeout=15.0).stdout.strip()
                    no_akrep_runs.append({"run": idx, "raw": ti, "parsed_seconds": parse_systemd_time_line(ti)})
                with_med = statistics.median([r["parsed_seconds"]["total"] for r in boot_runs if r["parsed_seconds"].get("total") is not None])
                without_med = statistics.median([r["parsed_seconds"]["total"] for r in no_akrep_runs if r["parsed_seconds"].get("total") is not None])
                akrep_overhead = {
                    "collected_at_utc": datetime.now(timezone.utc).isoformat(),
                    "daemon_metrics": probe_data,
                    "boot_with_akrepd_seconds": [r["parsed_seconds"]["total"] for r in boot_runs],
                    "boot_with_akrepd_median_s": round(with_med, 4),
                    "boot_without_akrepd_seconds": [r["parsed_seconds"]["total"] for r in no_akrep_runs],
                    "boot_without_akrepd_median_s": round(without_med, 4),
                    "boot_delta_ms": round((with_med - without_med) * 1000.0, 1),
                    "binary_sizes": json.loads(
                        vm.qga.run_shell(
                            "python3 -c 'import json,os; print(json.dumps({p: (os.path.getsize(p) if os.path.exists(p) else None) for p in [\"/usr/bin/akrepd\", \"/usr/bin/akrep-ui\"]}))'",
                            timeout=10.0,
                        ).stdout.strip()
                        or "{}"
                    ),
                }
                (EVAL_ROOT / "metadata" / "01_akrep_overhead_benchmark.json").write_text(
                    json.dumps(akrep_overhead, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
                )
                print(f"[PHASE 01:akrep] VmRSS={probe_data.get('vm_rss_kb')} kB, idle_ticks={probe_data.get('idle_cpu_ticks_3s')}, boot_delta_ms={akrep_overhead['boot_delta_ms']}")

            vm.shutdown_graceful(timeout=45.0)
    finally:
        ovl_disk.unlink(missing_ok=True)
        ovl_vars.unlink(missing_ok=True)

    with tarfile.open(raw_p1 / "p1_raw.tar.gz", "r:gz") as tf:
        tf.extractall(raw_p1)
    with tarfile.open(raw_p4 / "p4_out.tar.gz", "r:gz") as tf:
        tf.extractall(raw_p4)

    p4_dir = raw_p4 / "p4_out"
    cdx_dst = EVAL_ROOT / "sbom" / f"04_sbom_cyclonedx{suffix}.json"
    spdx_dst = EVAL_ROOT / "sbom" / f"04_sbom_spdx{suffix}.json"
    vuln_dst = EVAL_ROOT / "sbom" / f"04_vulnerability_report{suffix}.json"
    shutil.copy2(p4_dir / "04_sbom_cyclonedx.json", cdx_dst)
    shutil.copy2(p4_dir / "04_sbom_spdx.json", spdx_dst)
    shutil.copy2(p4_dir / "04_vulnerability_report.json", vuln_dst)

    p1_dir = raw_p1 / "p1_raw"

    def read_f(name: str) -> str:
        p = p1_dir / name
        return p.read_text(encoding="utf-8", errors="replace").strip() if p.is_file() else ""

    os_rel = parse_kv(read_f("os-release.txt"))
    lscpu_data = json.loads(read_f("lscpu.json") or "{}")
    lscpu_map = {item["field"].rstrip(":"): item["data"] for item in lscpu_data.get("lscpu", []) if "field" in item}

    # Parse free -b
    mem_info: dict[str, Any] = {}
    for ln in read_f("free-bytes.txt").splitlines():
        parts = ln.split()
        if parts and parts[0] == "Mem:":
            mem_info["ram_total_bytes"] = int(parts[1])
            mem_info["ram_used_bytes"] = int(parts[2])
            mem_info["ram_free_bytes"] = int(parts[3])
            mem_info["ram_available_bytes"] = int(parts[6]) if len(parts) > 6 else int(parts[3])
            mem_info["ram_total_gib"] = round(int(parts[1]) / (1024**3), 2)
        elif parts and parts[0] == "Swap:":
            mem_info["swap_total_bytes"] = int(parts[1])
            mem_info["swap_used_bytes"] = int(parts[2])
            mem_info["swap_total_gib"] = round(int(parts[1]) / (1024**3), 2)

    lsblk_data = json.loads(read_f("lsblk.json") or "{}")
    findmnt_data = json.loads(read_f("findmnt.json") or "{}")
    ip_links = json.loads(read_f("ip-link.json") or "[]")
    net_ifaces = []
    for iface in ip_links:
        net_ifaces.append(
            {
                "ifname": iface.get("ifname"),
                "operstate": iface.get("operstate"),
                "mtu": iface.get("mtu"),
                "link_type": iface.get("link_type"),
                "address": "[REDACTED-MAC]",
            }
        )

    running_services = []
    for ln in read_f("running-services.txt").splitlines():
        parts = ln.split(None, 4)
        if parts:
            running_services.append(
                {
                    "unit": parts[0],
                    "load": parts[1] if len(parts) > 1 else "",
                    "active": parts[2] if len(parts) > 2 else "",
                    "sub": parts[3] if len(parts) > 3 else "",
                    "description": sanitize_text(parts[4]) if len(parts) > 4 else "",
                }
            )

    failed_units = [ln.strip() for ln in read_f("failed-units.txt").splitlines() if ln.strip()]

    # Graphics summary
    glx_txt = read_f("glxinfo.txt")
    renderer_match = re.search(r"OpenGL renderer string:\s*(.+)", glx_txt)
    accel_match = re.search(r"Accelerated:\s*(.+)", glx_txt)
    vga_lines = [ln.strip() for ln in read_f("lspci.txt").splitlines() if "VGA" in ln or "Display" in ln or "virtio-pci" in ln or "Kernel driver in use:" in ln]

    boot_totals = [r["parsed_seconds"].get("total") for r in boot_runs if r["parsed_seconds"].get("total") is not None]
    boot_userspace = [r["parsed_seconds"].get("userspace") for r in boot_runs if r["parsed_seconds"].get("userspace") is not None]

    sb_raw = read_f("secure-boot.txt")
    sb_efivar = read_f("secure-boot-efivar.txt")
    sb_state = sb_raw if sb_raw and sb_raw != "mokutil_not_installed" else (
        "enabled" if sb_efivar.strip().endswith("1") else "disabled (virtual OVMF firmware default)"
    )

    inventory = {
        "schema_version": "1.0",
        "collected_at_utc": datetime.now(timezone.utc).isoformat(),
        "pii_sanitized": True,
        "distribution": {
            "name": os_rel.get("NAME", ""),
            "pretty_name": os_rel.get("PRETTY_NAME", ""),
            "id": os_rel.get("ID", ""),
            "id_like": os_rel.get("ID_LIKE", ""),
            "version": os_rel.get("VERSION", ""),
            "version_id": os_rel.get("VERSION_ID", ""),
            "fedora_base_version": read_f("fedora-base-ver.txt") or "45",
            "fedora_release_file": read_f("fedora-release.txt"),
            "blaze_release_file": read_f("blaze-release.txt"),
            "release_rpms": [ln.strip() for ln in read_f("release-rpms.txt").splitlines() if ln.strip()],
        },
        "kernel": {
            "release": read_f("kernel-release.txt"),
            "architecture": read_f("arch.txt"),
            "uname_sanitized": sanitize_text(read_f("uname.txt")),
        },
        "virtual_hardware": {
            "hypervisor": lscpu_map.get("Hypervisor vendor", "KVM"),
            "virtualization_type": lscpu_map.get("Virtualization type", "full"),
            "cpu_model": lscpu_map.get("Model name", ""),
            "virtual_cores": int(lscpu_map.get("CPU(s)", str(DEFAULT_EVAL_CPUS))),
            "memory": mem_info,
            "boot_mode": read_f("boot-mode.txt"),
            "secure_boot_state": sb_state,
            "graphics": {
                "pci_lines": vga_lines[:8],
                "dri_devices": [ln.strip() for ln in read_f("dev-dri.txt").splitlines() if "card" in ln or "render" in ln],
                "opengl_renderer": renderer_match.group(1).strip() if renderer_match else "llvmpipe / software (no X11 glxinfo or Wayland-only)",
                "hardware_accelerated": (accel_match.group(1).strip().lower() == "yes") if accel_match else False,
                "rendering_classification": "software_rendering_virtio_vga",
                "eglinfo_summary": sanitize_text(read_f("eglinfo.txt")[:1000]),
            },
        },
        "storage": {
            "block_devices": lsblk_data.get("blockdevices", []),
            "filesystems": [
                {
                    "target": fs.get("target"),
                    "source": sanitize_text(str(fs.get("source", ""))),
                    "fstype": fs.get("fstype"),
                    "options": fs.get("options"),
                }
                for fs in findmnt_data.get("filesystems", [])
                if fs.get("target") in ("/", "/home", "/boot", "/boot/efi", "/var", "/tmp")
                or str(fs.get("fstype", "")) in ("btrfs", "ext4", "vfat", "xfs")
            ],
        },
        "software_baseline": {
            "installed_rpm_count": int(read_f("rpm-count.txt") or "0"),
            "systemd_version": read_f("systemd-version.txt").splitlines()[0] if read_f("systemd-version.txt") else "",
            "selinux_mode": read_f("selinux.txt"),
            "firewalld_state": read_f("firewalld.txt"),
            "running_service_count": len(running_services),
            "failed_unit_count": len(failed_units),
            "failed_units": failed_units,
            "running_services": running_services,
        },
        "network_sanitized": {
            "interfaces": net_ifaces,
            "nmcli_status": sanitize_text(read_f("nmcli.txt")),
            "isolation_mode": "QEMU user-mode SLIRP NAT (isolated guest network)",
        },
        "stock_boot_measurements": {
            "runs": boot_runs,
            "median_total_seconds": round(statistics.median(boot_totals), 4) if boot_totals else None,
            "median_userspace_seconds": round(statistics.median(boot_userspace), 4) if boot_userspace else None,
            "critical_chain": sanitize_text(read_f("critical-chain.txt")),
            "top_blame_units": sanitize_text(read_f("blame-top40.txt")),
        },
    }

    inv_path = EVAL_ROOT / "metadata" / f"01_environment_inventory{suffix}.json"
    inventory["distribution"]["blazeos_release_file"] = read_f("blazeos-release.txt")
    inventory["distribution"]["first_party_blaze_rpms"] = [
        ln.strip() for ln in read_f("blaze-first-party-rpms.txt").splitlines() if ln.strip()
    ]
    inventory["distribution"]["first_party_rpm_verify"] = [
        ln.strip() for ln in read_f("blaze-rpm-verify.txt").splitlines() if ln.strip()
    ]
    if akrep_overhead is not None:
        inventory["akrep_overhead"] = akrep_overhead
    inv_path.write_text(json.dumps(inventory, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"[PHASE 01:{target_name}] Wrote {inv_path} (RPMs={inventory['software_baseline']['installed_rpm_count']}, median_boot={inventory['stock_boot_measurements']['median_total_seconds']}s)")

    # Summarize Phase 04 SBOM + Grype results cleanly into a summary JSON for reporting
    cdx = json.loads(cdx_dst.read_text(encoding="utf-8"))
    spdx = json.loads(spdx_dst.read_text(encoding="utf-8"))
    grype = json.loads(vuln_dst.read_text(encoding="utf-8"))
    grype_native = (
        json.loads((p4_dir / "grype_native_autodetect.json").read_text(encoding="utf-8"))
        if (p4_dir / "grype_native_autodetect.json").is_file()
        else {}
    )

    matches = grype.get("matches", [])
    sev_counts = {"Critical": 0, "High": 0, "Medium": 0, "Low": 0, "Negligible": 0, "Unknown": 0}
    fixed_available = 0
    unique_cves: set[str] = set()
    for m in matches:
        v = m.get("vulnerability", {})
        sev = v.get("severity", "Unknown")
        sev_counts[sev] = sev_counts.get(sev, 0) + 1
        if v.get("id"):
            unique_cves.add(v["id"])
        fix_state = v.get("fix", {}).get("state", "")
        if fix_state == "fixed":
            fixed_available += 1

    db_status_raw = (p4_dir / "grype_db_status.json").read_text(encoding="utf-8", errors="replace") if (p4_dir / "grype_db_status.json").is_file() else ""
    db_status = json.loads(db_status_raw) if db_status_raw.strip().startswith("{") else {"raw": db_status_raw}

    syft_ver_raw = (p4_dir / "syft_version.json").read_text(encoding="utf-8", errors="replace") if (p4_dir / "syft_version.json").is_file() else ""
    grype_ver_raw = (p4_dir / "grype_version.json").read_text(encoding="utf-8", errors="replace") if (p4_dir / "grype_version.json").is_file() else ""

    p4_summary = {
        "collected_at_utc": datetime.now(timezone.utc).isoformat(),
        "target": target_name,
        "syft_scan_scope": "dir:/ (full rootfs excluding virtual/cache mounts)",
        "syft_version": json.loads(syft_ver_raw) if syft_ver_raw.strip().startswith("{") else syft_ver_raw.strip(),
        "grype_version": json.loads(grype_ver_raw) if grype_ver_raw.strip().startswith("{") else grype_ver_raw.strip(),
        "grype_db_status": db_status,
        "cyclonedx_component_count": len(cdx.get("components", [])),
        "spdx_package_count": len(spdx.get("packages", [])),
        "rpm_package_count": inventory["software_baseline"]["installed_rpm_count"],
        "first_party_blaze_rpms_count": len([
            x for x in inventory["distribution"]["first_party_blaze_rpms"]
            if "\t" in x and "is not installed" not in x
        ]),
        "total_vulnerability_matches": len(matches),
        "native_autodetect_matches": len(grype_native.get("matches", [])),
        "grype_policy_used": ".grype.yaml (distro: fedora:45)",
        "unique_vulnerability_ids": len(unique_cves),
        "severity_counts": sev_counts,
        "matches_with_fix_available": fixed_available,
    }
    (EVAL_ROOT / "sbom" / f"04_supply_chain_summary{suffix}.json").write_text(
        json.dumps(p4_summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"[PHASE 04:{target_name}] SBOM components={p4_summary['cyclonedx_component_count']}, Grype matches={p4_summary['total_vulnerability_matches']}, severities={sev_counts}")


def main() -> None:
    targets = sys.argv[1:] or ["blaze", "fedora45"]
    if "blaze" in targets and (VM_STORE / "blaze_base.qcow2").is_file():
        collect_target_inventory_and_sbom("blaze", VM_STORE / "blaze_base.qcow2", VM_STORE / "blaze_base_OVMF_VARS.fd")
    if "fedora45" in targets and (VM_STORE / "fedora45_base.qcow2").is_file():
        collect_target_inventory_and_sbom("fedora45", VM_STORE / "fedora45_base.qcow2", VM_STORE / "fedora45_base_OVMF_VARS.fd")


if __name__ == "__main__":
    main()
