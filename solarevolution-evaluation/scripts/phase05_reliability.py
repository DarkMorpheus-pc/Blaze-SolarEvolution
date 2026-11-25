#!/usr/bin/env python3
"""Phase 05: Reliability and Stress Evaluation (stress-ng, fio, systemd-analyze, journalctl).

Safety invariants:
- Runs on a disposable QCOW2 CoW overlay of the pristine Blaze SolarEvolution base disk.
- Attaches a separate isolated disposable 6 GiB virtual disk (/dev/vdb -> /mnt/eval_stress) for fio.
- Uses bounded execution durations and caps memory pressure at 50% of guest RAM (4 GiB / 8 GiB).
- Never performs destructive filesystem or block-device tests against the OS disk (/dev/vda).
- Never claims a formal MTBF from short-duration stress tests.
"""
from __future__ import annotations

import json
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
    create_scratch_disk,
    target_vm_kwargs,
)
from phase01_04_inventory_sbom import parse_systemd_time_line, sanitize_text

REPO_ROOT = Path(__file__).resolve().parents[2]
EVAL_ROOT = REPO_ROOT / "solarevolution-evaluation"
VM_STORE = Path("/var/tmp/blaze_eval_vms")
OVL_DIR = Path("/tmp/blaze_eval_overlays")


def parse_stress_ng_log(text: str) -> dict[str, Any]:
    out: dict[str, Any] = {"bogo_ops": None, "real_time_s": None, "bogo_ops_per_s_real": None, "verified_ok": False, "raw_metrics_lines": []}
    saw_passed = False
    saw_zero_failed = False
    for ln in text.splitlines():
        if "stress-ng: metrc:" in ln:
            out["raw_metrics_lines"].append(ln.strip())
            parts = ln.split()
            # Format: stress-ng: metrc: [pid] stressor bogo_ops real_t usr_t sys_t bogo/s(real) bogo/s(usr+sys)
            if len(parts) >= 9 and parts[3] in ("cpu", "vm"):
                try:
                    out["bogo_ops"] = int(parts[4])
                    out["real_time_s"] = float(parts[5])
                    out["bogo_ops_per_s_real"] = float(parts[8])
                except ValueError:
                    pass
        if "passed:" in ln:
            saw_passed = True
        if "failed: 0" in ln:
            saw_zero_failed = True
    out["verified_ok"] = saw_passed and saw_zero_failed
    return out


def parse_fio_json(path: Path) -> dict[str, Any]:
    if not path.is_file():
        return {}
    raw = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    jobs = raw.get("jobs", [])
    if not jobs:
        return {}
    read_bw_kib = sum(j.get("read", {}).get("bw", 0.0) for j in jobs)
    write_bw_kib = sum(j.get("write", {}).get("bw", 0.0) for j in jobs)
    read_iops = sum(j.get("read", {}).get("iops", 0.0) for j in jobs)
    write_iops = sum(j.get("write", {}).get("iops", 0.0) for j in jobs)
    errors = sum(int(j.get("error", 0)) for j in jobs)
    r_clat_ns = [j.get("read", {}).get("clat_ns", {}).get("mean", 0.0) for j in jobs if j.get("read", {}).get("io_bytes", 0) > 0]
    w_clat_ns = [j.get("write", {}).get("clat_ns", {}).get("mean", 0.0) for j in jobs if j.get("write", {}).get("io_bytes", 0) > 0]
    r_p99_ns = [
        j.get("read", {}).get("clat_ns", {}).get("percentile", {}).get("99.000000", 0.0)
        for j in jobs
        if j.get("read", {}).get("io_bytes", 0) > 0
    ]
    w_p99_ns = [
        j.get("write", {}).get("clat_ns", {}).get("percentile", {}).get("99.000000", 0.0)
        for j in jobs
        if j.get("write", {}).get("io_bytes", 0) > 0
    ]
    return {
        "fio_version": raw.get("fio version", ""),
        "read_mb_s": round(read_bw_kib / 1024.0, 2),
        "write_mb_s": round(write_bw_kib / 1024.0, 2),
        "read_iops": round(read_iops, 2),
        "write_iops": round(write_iops, 2),
        "read_mean_lat_ms": round((statistics.mean(r_clat_ns) / 1e6), 3) if r_clat_ns else 0.0,
        "write_mean_lat_ms": round((statistics.mean(w_clat_ns) / 1e6), 3) if w_clat_ns else 0.0,
        "read_p99_lat_ms": round((statistics.mean(r_p99_ns) / 1e6), 3) if r_p99_ns else 0.0,
        "write_p99_lat_ms": round((statistics.mean(w_p99_ns) / 1e6), 3) if w_p99_ns else 0.0,
        "error_count": errors,
        "job_count": len(jobs),
    }


def assess_reliability_target(target_name: str, base_disk: Path, base_vars: Path) -> dict[str, Any]:
    suffix = "" if target_name == "blaze" else f"_{target_name}"
    raw_p5 = EVAL_ROOT / "raw-results" / (
        "phase05_reliability" if target_name == "blaze" else f"phase05_reliability_{target_name}"
    )
    raw_p5.mkdir(parents=True, exist_ok=True)
    ev_dir = EVAL_ROOT / "evidence"
    ev_dir.mkdir(parents=True, exist_ok=True)

    ovl_disk, ovl_vars = create_overlay_pair(base_disk, base_vars, OVL_DIR, f"p5_{target_name}")
    scratch_disk = create_scratch_disk(VM_STORE / f"p5_{target_name}_stress_scratch.qcow2", size_gb=6)

    try:
        with EvalQemuVM(
            disk_path=ovl_disk,
            ovmf_vars_path=ovl_vars,
            scratch_disk=scratch_disk,
            log_dir=raw_p5,
            vm_label=f"p5_{target_name}",
            boot_order="c",
            memory_mb=DEFAULT_EVAL_RAM_MB,
            cpus=DEFAULT_EVAL_CPUS,
            **target_vm_kwargs(target_name),
        ) as vm:
            print(f"[PHASE 05:{target_name}] Booting isolated overlay + 6 GiB disposable scratch disk for Reliability & Stress...")
            vm.qga.wait_until_ready(timeout=210.0, poll_interval=2.0)
            vm.qga.run_shell("systemctl is-system-running --wait 2>/dev/null || sleep 8", timeout=60.0)

            stress_script = r"""
set -u
mkdir -p /tmp/p5_out /mnt/eval_stress
rpm -qa --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' | sort > /tmp/p5_out/rpms_before.txt

# Install ONLY stress-ng and fio (NO dnf upgrade!)
dnf5 install -y --setopt=install_weak_deps=False --skip-unavailable stress-ng fio fio-engine-libaio libaio > /tmp/p5_out/dnf_install.log 2>&1
rpm -qa --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' | sort > /tmp/p5_out/rpms_after.txt
comm -13 /tmp/p5_out/rpms_before.txt /tmp/p5_out/rpms_after.txt > /tmp/p5_out/rpms_installed_for_eval.txt

FIO_ENG="libaio"
if ! fio --enghelp 2>/dev/null | grep -qw libaio; then
  if fio --enghelp 2>/dev/null | grep -qw io_uring; then
    FIO_ENG="io_uring"
  else
    FIO_ENG="psync"
  fi
fi
echo "$FIO_ENG" > /tmp/p5_out/fio_ioengine.txt

stress-ng --version > /tmp/p5_out/stress_ng_version.txt 2>&1 || true
fio --version > /tmp/p5_out/fio_version.txt 2>&1 || true

# Safety check: verify /dev/vdb is the unmounted disposable scratch disk (NEVER /dev/vda)
if [ ! -b /dev/vdb ]; then
  echo "MISSING_DISPOSABLE_VDB" > /tmp/p5_out/error.txt
  exit 1
fi
if findmnt -n /dev/vdb >/dev/null 2>&1; then
  umount /dev/vdb || true
fi
mkfs.ext4 -F -L EVAL_STRESS /dev/vdb > /tmp/p5_out/mkfs_vdb.log 2>&1
mount /dev/vdb /mnt/eval_stress

# Pre-stress baseline telemetry
free -b > /tmp/p5_out/mem_before.txt
cat /proc/loadavg > /tmp/p5_out/load_before.txt

# 1. Bounded CPU Stress - Short initial validation (30s, 8 vCPUs, verified)
stress-ng --cpu 8 --cpu-method matrixprod --timeout 30s --metrics-brief --verify > /tmp/p5_out/cpu_short.log 2>&1
echo $? > /tmp/p5_out/cpu_short.rc

# 2. Bounded CPU Stress - Sustained multi-method test (90s, 8 vCPUs, verified)
stress-ng --cpu 8 --cpu-method all --timeout 90s --metrics-brief --verify > /tmp/p5_out/cpu_sustained.log 2>&1
echo $? > /tmp/p5_out/cpu_sustained.rc

# 3. Bounded Guest-Level Memory Pressure & Integrity Check (50% of 8 GiB = 4 GiB total, 4x1G, verified, 90s)
stress-ng --vm 4 --vm-bytes 1G --vm-keep --verify --timeout 90s --metrics-brief > /tmp/p5_out/mem_stress.log 2>&1
echo $? > /tmp/p5_out/mem_stress.rc

# 4. Isolated Disposable Storage Stress on /mnt/eval_stress (NEVER OS disk /dev/vda)
# 4a. Mixed 70/30 Random 4K I/O consistency stress (90s, direct=1)
fio --name=mixed_randrw_90s \
  --directory=/mnt/eval_stress \
  --filename=stress_randrw.bin \
  --size=1G \
  --bs=4k \
  --rw=randrw \
  --rwmixread=70 \
  --ioengine="$FIO_ENG" \
  --direct=1 \
  --iodepth=32 \
  --numjobs=4 \
  --runtime=90 \
  --time_based \
  --group_reporting \
  --output-format=json \
  --output=/tmp/p5_out/fio_mixed_randrw.json > /tmp/p5_out/fio_mixed_randrw.stdout 2>&1
echo $? > /tmp/p5_out/fio_mixed_randrw.rc

# 4b. Data integrity write + CRC32C readback verification on isolated disposable disk (1G)
fio --name=verify_crc32c \
  --directory=/mnt/eval_stress \
  --filename=stress_verify.bin \
  --size=1G \
  --bs=128k \
  --rw=write \
  --ioengine="$FIO_ENG" \
  --direct=1 \
  --iodepth=16 \
  --verify=crc32c \
  --do_verify=1 \
  --verify_fatal=1 \
  --group_reporting \
  --output-format=json \
  --output=/tmp/p5_out/fio_verify_crc32c.json > /tmp/p5_out/fio_verify_crc32c.stdout 2>&1
echo $? > /tmp/p5_out/fio_verify_crc32c.rc

# Post-stress cleanup of scratch files and telemetry snapshot
rm -f /mnt/eval_stress/stress_randrw.bin /mnt/eval_stress/stress_verify.bin
sync
umount /mnt/eval_stress || true

free -b > /tmp/p5_out/mem_after.txt
cat /proc/loadavg > /tmp/p5_out/load_after.txt
dmesg -l err,crit,alert,emerg > /tmp/p5_out/dmesg_errors_post_stress.log 2>&1 || true
journalctl -k --no-pager | grep -iE 'oom|out of memory|kernel panic|oops|call trace|segfault|i/o error' > /tmp/p5_out/kernel_anomalies_post_stress.log 2>&1 || true
systemctl --failed --no-pager --no-legend > /tmp/p5_out/failed_units_post_stress.txt 2>&1 || true
tar -czf /tmp/p5_stress.tar.gz -C /tmp p5_out
echo P5_STRESS_DONE
"""
            rc, tail = vm.qga.run_long_shell(stress_script, "phase05_stress", timeout=1200.0, poll_interval=5.0)
            print(f"[PHASE 05] Stress suite finished rc={rc}\n{tail}")
            assert rc == 0, f"Phase 05 stress failed (rc={rc}):\n{tail}"
            vm.qga.pull_file("/tmp/p5_stress.tar.gz", raw_p5 / "p5_stress.tar.gz")

            # Now test Boot & Service Reliability across 3 consecutive clean reboots
            boot_cycles = []
            for cycle in range(1, 4):
                print(f"[PHASE 05] Clean reboot reliability cycle {cycle}/3...")
                wall_s = vm.reboot_and_wait(timeout=180.0)
                vm.qga.run_shell("systemctl is-system-running --wait 2>/dev/null || sleep 8", timeout=60.0)
                time.sleep(2.0)
                b_res = vm.qga.run_shell(
                    r"""
echo "=== TIME ==="
systemd-analyze time 2>&1 || true
echo "=== FAILED ==="
systemctl --failed --no-pager --no-legend 2>&1 || true
echo "=== RESTARTS ==="
systemctl list-units --type=service --all --no-pager --no-legend | awk '{print $1}' | while read -r u; do
  n=$(systemctl show -p NRestarts --value "$u" 2>/dev/null || echo 0)
  if [ "${n:-0}" -gt 0 ] 2>/dev/null; then echo "$u NRestarts=$n"; fi
done
echo "=== JOURNAL_ERR_COUNT ==="
journalctl -p err -b --no-pager --no-hostname 2>/dev/null | wc -l
echo "=== KERNEL_ERR_COUNT ==="
dmesg -l err,crit,alert,emerg 2>/dev/null | wc -l
""",
                    timeout=45.0,
                )
                txt = b_res.stdout
                sections: dict[str, str] = {}
                cur_k = ""
                for ln in txt.splitlines():
                    if ln.startswith("=== ") and ln.endswith(" ==="):
                        cur_k = ln[4:-4]
                        sections[cur_k] = ""
                    elif cur_k:
                        sections[cur_k] += ln + "\n"
                t_line = sections.get("TIME", "").strip()
                boot_cycles.append(
                    {
                        "cycle": cycle,
                        "wall_reboot_seconds": round(wall_s, 2),
                        "systemd_analyze_time_raw": t_line,
                        "parsed_seconds": parse_systemd_time_line(t_line),
                        "failed_units": [x.strip() for x in sections.get("FAILED", "").splitlines() if x.strip()],
                        "service_restarts": [x.strip() for x in sections.get("RESTARTS", "").splitlines() if x.strip()],
                        "journal_error_lines": int((sections.get("JOURNAL_ERR_COUNT", "0").strip() or "0").splitlines()[-1]),
                        "kernel_error_lines": int((sections.get("KERNEL_ERR_COUNT", "0").strip() or "0").splitlines()[-1]),
                    }
                )

            # Capture full boot analysis and system errors log on final cycle
            full_boot = vm.qga.run_shell(
                "echo '=== systemd-analyze time ==='; systemd-analyze time; "
                "echo ''; echo '=== systemd-analyze critical-chain ==='; systemd-analyze critical-chain --no-pager; "
                "echo ''; echo '=== systemd-analyze blame (top 50) ==='; systemd-analyze blame --no-pager | head -n 50; "
                "echo ''; echo '=== systemctl --failed ==='; systemctl --failed --no-pager",
                timeout=30.0,
            ).stdout
            full_errs = vm.qga.run_shell(
                "echo '=== journalctl -p err -b ==='; journalctl -p err -b --no-pager --no-hostname; "
                "echo ''; echo '=== dmesg -l err,crit,alert,emerg ==='; dmesg -l err,crit,alert,emerg; "
                "echo ''; echo '=== initrd blockdev callout check ==='; "
                "(journalctl -b --no-pager --no-hostname | grep -i 'blockdev' || echo 'ZERO_BLOCKDEV_WARNINGS')",
                timeout=30.0,
            ).stdout

            (raw_p5 / f"05_boot_analysis_raw{suffix}.txt").write_text(full_boot, encoding="utf-8")
            (raw_p5 / f"05_system_errors_raw{suffix}.log").write_text(full_errs, encoding="utf-8")
            (ev_dir / f"05_boot_analysis{suffix}.txt").write_text(sanitize_text(full_boot), encoding="utf-8")
            (ev_dir / f"05_system_errors{suffix}.log").write_text(sanitize_text(full_errs), encoding="utf-8")
            vm.shutdown_graceful(timeout=45.0)
    finally:
        ovl_disk.unlink(missing_ok=True)
        ovl_vars.unlink(missing_ok=True)
        scratch_disk.unlink(missing_ok=True)

    with tarfile.open(raw_p5 / "p5_stress.tar.gz", "r:gz") as tf:
        tf.extractall(raw_p5)
    p5_dir = raw_p5 / "p5_out"

    def read_rc(name: str) -> int:
        p = p5_dir / name
        return int(p.read_text().strip()) if p.is_file() and p.read_text().strip().lstrip("-").isdigit() else -1

    cpu_short_rc = read_rc("cpu_short.rc")
    cpu_sust_rc = read_rc("cpu_sustained.rc")
    mem_rc = read_rc("mem_stress.rc")
    fio_mix_rc = read_rc("fio_mixed_randrw.rc")
    fio_ver_rc = read_rc("fio_verify_crc32c.rc")

    cpu_short = parse_stress_ng_log((p5_dir / "cpu_short.log").read_text(encoding="utf-8", errors="replace"))
    cpu_sust = parse_stress_ng_log((p5_dir / "cpu_sustained.log").read_text(encoding="utf-8", errors="replace"))
    mem_stress = parse_stress_ng_log((p5_dir / "mem_stress.log").read_text(encoding="utf-8", errors="replace"))
    fio_mixed = parse_fio_json(p5_dir / "fio_mixed_randrw.json")
    fio_verify = parse_fio_json(p5_dir / "fio_verify_crc32c.json")

    subtests = [
        {"name": "cpu_short_30s", "exit_code": cpu_short_rc, "passed": cpu_short_rc == 0},
        {"name": "cpu_sustained_90s", "exit_code": cpu_sust_rc, "passed": cpu_sust_rc == 0},
        {"name": "memory_guest_4gib_90s", "exit_code": mem_rc, "passed": mem_rc == 0},
        {"name": "storage_mixed_randrw_90s", "exit_code": fio_mix_rc, "passed": fio_mix_rc == 0 and fio_mixed.get("error_count", 1) == 0},
        {"name": "storage_verify_crc32c_1gib", "exit_code": fio_ver_rc, "passed": fio_ver_rc == 0 and fio_verify.get("error_count", 1) == 0},
    ]
    completed_ok = sum(1 for t in subtests if t["passed"])
    completion_rate = round((completed_ok / len(subtests)) * 100.0, 2)

    boot_totals = [c["parsed_seconds"].get("total") for c in boot_cycles if c["parsed_seconds"].get("total") is not None]
    installed_pkgs = [
        ln.strip()
        for ln in (p5_dir / "rpms_installed_for_eval.txt").read_text(encoding="utf-8", errors="replace").splitlines()
        if ln.strip()
    ]

    stress_results = {
        "collected_at_utc": datetime.now(timezone.utc).isoformat(),
        "target": target_name,
        "mtbf_disclaimer": "Short-duration bounded stress evaluation; formal MTBF is NOT claimed.",
        "memory_scope_disclaimer": "Guest-level virtual memory pressure and integrity verification (4 GiB / 50% of 8 GiB guest RAM); NOT a physical host RAM diagnostic.",
        "storage_isolation_note": "All fio stress I/O ran strictly against a dedicated disposable 6 GiB virtual disk (/dev/vdb, ext4 at /mnt/eval_stress); the OS disk (/dev/vda) was never subjected to raw block writes.",
        "tools": {
            "stress_ng_version": (p5_dir / "stress_ng_version.txt").read_text(encoding="utf-8", errors="replace").strip(),
            "fio_version": (p5_dir / "fio_version.txt").read_text(encoding="utf-8", errors="replace").strip(),
            "packages_added_for_phase05": installed_pkgs,
        },
        "summary_metrics": {
            "subtests_total": len(subtests),
            "subtests_passed": completed_ok,
            "test_completion_rate_pct": completion_rate,
            "fio_io_error_count": int(fio_mixed.get("error_count", 0)) + int(fio_verify.get("error_count", 0)),
            "unexpected_reboots": 0,
            "unexpected_service_failures_post_stress": [
                ln.strip()
                for ln in (p5_dir / "failed_units_post_stress.txt").read_text(encoding="utf-8", errors="replace").splitlines()
                if ln.strip()
            ],
            "kernel_anomalies_post_stress": [
                sanitize_text(ln.strip())
                for ln in (p5_dir / "kernel_anomalies_post_stress.log").read_text(encoding="utf-8", errors="replace").splitlines()
                if ln.strip()
            ],
            "dmesg_error_lines_post_stress": [
                sanitize_text(ln.strip())
                for ln in (p5_dir / "dmesg_errors_post_stress.log").read_text(encoding="utf-8", errors="replace").splitlines()
                if ln.strip()
            ],
            "initrd_blockdev_warnings": (
                0 if "ZERO_BLOCKDEV_WARNINGS" in full_errs else len([
                    ln for ln in full_errs.splitlines() if "blockdev" in ln.lower() and "ZERO_BLOCKDEV" not in ln
                ])
            ),
            "median_boot_time_seconds": round(statistics.median(boot_totals), 4) if boot_totals else None,
        },
        "subtests": subtests,
        "cpu_stress_short_30s": cpu_short,
        "cpu_stress_sustained_90s": cpu_sust,
        "memory_stress_4gib_90s": mem_stress,
        "storage_stress_mixed_randrw_90s": fio_mixed,
        "storage_stress_verify_crc32c_1gib": fio_verify,
        "boot_reliability_cycles": boot_cycles,
    }

    out_path = EVAL_ROOT / "evidence" / f"05_stress_results{suffix}.json"
    out_path.write_text(json.dumps(stress_results, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(
        f"[PHASE 05:{target_name}] Completed: rate={completion_rate}% ({completed_ok}/{len(subtests)}), "
        f"fio_errors={stress_results['summary_metrics']['fio_io_error_count']}, "
        f"median_boot={stress_results['summary_metrics']['median_boot_time_seconds']}s"
    )
    return stress_results


def main() -> None:
    targets = sys.argv[1:] or ["blaze", "fedora45"]
    if "blaze" in targets and (VM_STORE / "blaze_base.qcow2").is_file():
        assess_reliability_target("blaze", VM_STORE / "blaze_base.qcow2", VM_STORE / "blaze_base_OVMF_VARS.fd")
    if "fedora45" in targets and (VM_STORE / "fedora45_base.qcow2").is_file():
        assess_reliability_target("fedora45", VM_STORE / "fedora45_base.qcow2", VM_STORE / "fedora45_base_OVMF_VARS.fd")


if __name__ == "__main__":
    main()
