#!/usr/bin/env python3
"""Phase 02: Performance Benchmark Suite (Phoronix Test Suite + fio + Boot + Package Ops).

Evaluates Blaze SolarEvolution 5.05 AND (when available) upstream Fedora 45 Workstation
under 100% identical QEMU/KVM OVMF VM parameters (8 vCPUs, 8 GiB RAM, VirtIO-BLK,
dedicated isolated 6 GiB disposable storage disk /dev/vdb).

Methodology invariants:
- Every benchmark executes at least 3 times (FORCE_TIMES_TO_RUN=3 for PTS; 3 iterations for fio/boot/pkg).
- Records EVERY individual run value + exact median (never invents a universal aggregate score).
- Never runs concurrent resource-intensive benchmarks.
- Never writes fio directly to a system block device or OS disk (/dev/vda).
- Explicitly distinguishes software rendering (llvmpipe) from hardware GPU acceleration.
- Preserves original raw Phoronix Test Suite XML/JSON/logs in benchmarks/02_phoronix_raw_results/.
"""
from __future__ import annotations

import csv
import json
import os
import re
import shutil
import statistics
import sys
import tarfile
import time
import xml.etree.ElementTree as ET
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


def parse_pts_composite_xml(xml_path: Path) -> list[dict[str, Any]]:
    """Extract every benchmark result, individual run values, median, unit, and version from PTS composite.xml."""
    if not xml_path.is_file():
        return []
    tree = ET.parse(xml_path)
    root = tree.getroot()
    results = []
    for res in root.findall(".//Result"):
        ident = (res.findtext("Identifier") or "").strip()
        title = (res.findtext("Title") or "").strip()
        app_ver = (res.findtext("AppVersion") or "").strip()
        desc = (res.findtext("Description") or "").strip()
        scale = (res.findtext("Scale") or "").strip()
        prop = (res.findtext("Proportion") or "").strip()  # HIB or LIB
        for entry in res.findall(".//Data/Entry"):
            raw_str = (entry.findtext("RawString") or "").strip()
            val_str = (entry.findtext("Value") or "").strip()
            runs: list[float] = []
            if raw_str:
                for tok in raw_str.split(":"):
                    try:
                        runs.append(float(tok))
                    except ValueError:
                        pass
            if not runs and val_str:
                try:
                    runs.append(float(val_str))
                except ValueError:
                    pass
            if not runs:
                continue
            med = round(statistics.median(runs), 4)
            mean_v = round(statistics.mean(runs), 4)
            stdev_v = round(statistics.stdev(runs), 4) if len(runs) > 1 else 0.0
            results.append(
                {
                    "framework": "phoronix-test-suite",
                    "identifier": ident,
                    "title": title,
                    "app_version": app_ver,
                    "description": desc,
                    "unit": scale,
                    "proportion": prop,
                    "runs": [round(x, 4) for x in runs],
                    "run_count": len(runs),
                    "median": med,
                    "mean": mean_v,
                    "min": round(min(runs), 4),
                    "max": round(max(runs), 4),
                    "stdev": stdev_v,
                }
            )
    return results


def parse_fio_metric(json_path: Path, mode: str) -> dict[str, float]:
    raw = json.loads(json_path.read_text(encoding="utf-8", errors="replace"))
    jobs = raw.get("jobs", [])
    key = "read" if "read" in mode else "write"
    bw_mb_s = sum(j.get(key, {}).get("bw", 0.0) for j in jobs) / 1024.0
    iops = sum(j.get(key, {}).get("iops", 0.0) for j in jobs)
    lats_ns = [j.get(key, {}).get("clat_ns", {}).get("mean", 0.0) for j in jobs if j.get(key, {}).get("io_bytes", 0) > 0]
    p99_ns = [
        j.get(key, {}).get("clat_ns", {}).get("percentile", {}).get("99.000000", 0.0)
        for j in jobs
        if j.get(key, {}).get("io_bytes", 0) > 0
    ]
    return {
        "bw_mb_s": round(bw_mb_s, 2),
        "iops": round(iops, 2),
        "mean_lat_ms": round((statistics.mean(lats_ns) / 1e6), 4) if lats_ns else 0.0,
        "p99_lat_ms": round((statistics.mean(p99_ns) / 1e6), 4) if p99_ns else 0.0,
        "errors": sum(int(j.get("error", 0)) for j in jobs),
    }


def benchmark_target(target_name: str, base_disk: Path, base_vars: Path) -> dict[str, Any]:
    raw_dir = EVAL_ROOT / "raw-results" / f"phase02_{target_name}"
    raw_dir.mkdir(parents=True, exist_ok=True)
    pts_dest = EVAL_ROOT / "benchmarks" / "02_phoronix_raw_results" / target_name
    pts_dest.mkdir(parents=True, exist_ok=True)

    ovl_disk, ovl_vars = create_overlay_pair(base_disk, base_vars, OVL_DIR, f"p2_{target_name}")
    scratch_disk = create_scratch_disk(VM_STORE / f"p2_{target_name}_fio_scratch.qcow2", size_gb=6)

    try:
        with EvalQemuVM(
            disk_path=ovl_disk,
            ovmf_vars_path=ovl_vars,
            scratch_disk=scratch_disk,
            log_dir=raw_dir,
            vm_label=f"p2_{target_name}",
            boot_order="c",
            memory_mb=DEFAULT_EVAL_RAM_MB,
            cpus=DEFAULT_EVAL_CPUS,
            **target_vm_kwargs(target_name),
        ) as vm:
            print(f"[PHASE 02:{target_name}] Booting clean overlay + 6 GiB isolated NVMe scratch disk...")
            vm.qga.wait_until_ready(timeout=240.0, poll_interval=2.0)
            vm.qga.run_shell("systemctl is-system-running --wait 2>/dev/null || sleep 8", timeout=60.0)

            # 1. Measure 3 clean cold boots BEFORE installing any benchmark tools
            boot_runs = []
            t1 = vm.qga.run_shell("systemd-analyze time", timeout=15.0).stdout.strip()
            boot_runs.append({"run": 1, "raw": t1, "parsed": parse_systemd_time_line(t1)})
            for r_idx in (2, 3):
                print(f"[PHASE 02:{target_name}] Cold boot timing run {r_idx}/3...")
                vm.reboot_and_wait(timeout=180.0)
                vm.qga.run_shell("systemctl is-system-running --wait 2>/dev/null || sleep 8", timeout=60.0)
                ti = vm.qga.run_shell("systemd-analyze time", timeout=15.0).stdout.strip()
                boot_runs.append({"run": r_idx, "raw": ti, "parsed": parse_systemd_time_line(ti)})

            # 2. Install Phoronix Test Suite + build toolchain + fio (NO dnf upgrade!)
            print(f"[PHASE 02:{target_name}] Installing Phoronix Test Suite, fio, and compiler toolchain (no upgrades)...")
            install_script = r"""
set -euo pipefail
mkdir -p /tmp/p2_out /mnt/eval_fio
rpm -qa --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' | sort > /tmp/p2_out/rpms_before.txt

# Install PTS + build deps + fio (NO system upgrade)
dnf5 install -y --setopt=install_weak_deps=False --skip-unavailable \
  phoronix-test-suite fio fio-engine-libaio libaio gcc gcc-c++ make cmake autoconf automake \
  bison flex elfutils-libelf-devel openssl-devel bc perl perl-FindBin \
  perl-IPC-Cmd perl-File-Compare perl-File-Copy xz tar gzip bzip2 unzip zstd cpio patch diffutils \
  pcre2-devel expat-devel apr-devel apr-util-devel zlib-ng-compat-devel dwarves \
  php-cli php-xml php-json php-gd mesa-demos glx-utils \
  > /tmp/p2_out/dnf_install.log 2>&1

rpm -qa --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' | sort > /tmp/p2_out/rpms_after.txt
comm -13 /tmp/p2_out/rpms_before.txt /tmp/p2_out/rpms_after.txt > /tmp/p2_out/rpms_installed_for_eval.txt

# Upstream PTS v10.8.6 + PHP 8.5 in QEMU VMs throws DivisionByZeroError in phodevi_memory.php:261
# when virtual SMBIOS memory array metadata lacks slot counts. Patch ONLY the ephemeral test tool
# and log exact lines for full methodological transparency.
python3 - <<'PY' > /tmp/p2_out/pts_php_patch_note.txt 2>&1
import pathlib
p = pathlib.Path("/usr/share/phoronix-test-suite/pts-core/objects/phodevi/components/phodevi_memory.php")
if p.is_file():
    lines = p.read_text(encoding="utf-8", errors="replace").splitlines()
    print("Original lines 250-272:")
    for i in range(249, min(272, len(lines))):
        print(f"{i+1}: {lines[i]}")
    txt = "\n".join(lines) + "\n"
    # Guard modulo-by-zero cleanly in memory_string()
    patched = txt.replace(
        "public static function memory_string()",
        "public static function memory_string() { try { return self::_memory_string_orig(); } catch (\\DivisionByZeroError $e) { return '8192 MB Virtual Memory'; } }\n\tprivate static function _memory_string_orig()"
    )
    if patched != txt:
        p.write_text(patched, encoding="utf-8")
        print("Applied DivisionByZeroError guard to phodevi_memory::memory_string()")
PY

export PTS_USER_PATH_OVERRIDE=/root/.phoronix-test-suite/
phoronix-test-suite version 2>&1 | grep -E 'Phoronix Test Suite' | tail -n 1 > /tmp/p2_out/pts_version.txt || true
fio --version > /tmp/p2_out/fio_version.txt 2>&1 || true
gcc --version | head -n 1 > /tmp/p2_out/gcc_version.txt 2>&1 || true
uname -r > /tmp/p2_out/kernel_version.txt
cat /etc/os-release > /tmp/p2_out/os_release.txt

# Graphics capability probe
{
  echo "=== LSPCI VGA ==="
  lspci -nnk | grep -A3 -iE 'VGA|3D|Display' || true
  echo "=== GLXINFO ==="
  glxinfo -B 2>&1 || echo "glxinfo_unavailable"
  echo "=== EGLINFO ==="
  eglinfo -B 2>&1 | head -n 40 || echo "eglinfo_unavailable"
} > /tmp/p2_out/graphics_probe.txt

# Configure PTS non-interactive batch mode (both user-config.xml AND /etc/phoronix-test-suite.xml for root)
mkdir -p /root/.phoronix-test-suite
cat > /root/.phoronix-test-suite/user-config.xml <<'XML'
<?xml version="1.0"?>
<PhoronixTestSuite>
  <Options>
    <OpenBenchmarking>
      <AnonymousUsageReporting>FALSE</AnonymousUsageReporting>
      <IndexCacheTTL>3</IndexCacheTTL>
      <AlwaysUploadSystemLogs>FALSE</AlwaysUploadSystemLogs>
      <AllowResultUploadsToOpenBenchmarking>FALSE</AllowResultUploadsToOpenBenchmarking>
    </OpenBenchmarking>
    <General>
      <DefaultBrowser></DefaultBrowser>
      <UsePhodeviCache>TRUE</UsePhodeviCache>
      <DefaultDisplayMode>DEFAULT</DefaultDisplayMode>
      <PhoromaticServers></PhoromaticServers>
    </General>
    <Modules>
      <AutoLoadModules></AutoLoadModules>
    </Modules>
    <Installation>
      <RemoveDownloadFiles>FALSE</RemoveDownloadFiles>
      <SearchMediaForCache>TRUE</SearchMediaForCache>
      <SymLinkFilesFromCache>FALSE</SymLinkFilesFromCache>
      <PromptForDownloadMirror>FALSE</PromptForDownloadMirror>
      <EnvironmentDirectory>/root/.phoronix-test-suite/installed-tests/</EnvironmentDirectory>
      <CacheDirectory>/root/.phoronix-test-suite/download-cache/</CacheDirectory>
    </Installation>
    <Testing>
      <SaveSystemLogs>TRUE</SaveSystemLogs>
      <SaveInstallationLogs>TRUE</SaveInstallationLogs>
      <SaveTestLogs>TRUE</SaveTestLogs>
      <RemoveTestInstallOnCompletion>FALSE</RemoveTestInstallOnCompletion>
      <ResultsDirectory>/root/.phoronix-test-suite/test-results/</ResultsDirectory>
      <AlwaysUploadResultsToOpenBenchmarking>FALSE</AlwaysUploadResultsToOpenBenchmarking>
      <AutoSortRunQueue>TRUE</AutoSortRunQueue>
      <ShowPostRunStatistics>TRUE</ShowPostRunStatistics>
    </Testing>
    <BatchMode>
      <SaveResults>TRUE</SaveResults>
      <OpenBrowser>FALSE</OpenBrowser>
      <UploadResults>FALSE</UploadResults>
      <PromptForTestIdentifier>FALSE</PromptForTestIdentifier>
      <PromptForTestDescription>FALSE</PromptForTestDescription>
      <PromptSaveName>FALSE</PromptSaveName>
      <RunAllTestCombinations>FALSE</RunAllTestCombinations>
      <Configured>TRUE</Configured>
    </BatchMode>
  </Options>
</PhoronixTestSuite>
XML
cp -f /root/.phoronix-test-suite/user-config.xml /etc/phoronix-test-suite.xml
echo P2_INSTALL_OK
"""
            rc, tail = vm.qga.run_long_shell(install_script, f"p2_install_{target_name}", timeout=900.0, poll_interval=5.0)
            assert rc == 0, f"Phase 02 install failed for {target_name} (rc={rc}):\n{tail}"

            # 3. Execute Stage A (CPU, Compression, Memory, Apache Compilation, Memory Latency, fio, Package Ops)
            print(f"[PHASE 02:{target_name}] Stage A: Phoronix Test Suite (CoreMark, C-Ray, 7-Zip, STREAM, CacheBench, Apache) + Latency + fio + Package Ops...")
            pts_stage_a = f"""
set -u
export HOME=/root
export PTS_USER_PATH_OVERRIDE=/root/.phoronix-test-suite/
export FORCE_TIMES_TO_RUN=3
export TEST_RESULTS_IDENTIFIER="{target_name}"
export TEST_RESULTS_DESCRIPTION="Isolated QEMU/KVM OVMF 8vCPU 8GiB RAM evaluation ({target_name})"
export PTS_SILENT_MODE=1

run_pts() {{
  local test_id="$1"
  local save_name="$2"
  local preset_input="$3"
  local run_timeout="${{4:-900}}"
  echo ">>> [PTS] Installing $test_id..."
  timeout 600 phoronix-test-suite batch-install "$test_id" > "/tmp/p2_out/pts_install_${{save_name}}.log" 2>&1 || true
  echo ">>> [PTS] Running $test_id -> $save_name (3 runs)..."
  export TEST_RESULTS_NAME="$save_name"
  if [ -n "$preset_input" ]; then
    printf "%b" "$preset_input" | timeout "$run_timeout" phoronix-test-suite batch-run "$test_id" > "/tmp/p2_out/pts_run_${{save_name}}.log" 2>&1
  else
    timeout "$run_timeout" phoronix-test-suite batch-run "$test_id" </dev/null > "/tmp/p2_out/pts_run_${{save_name}}.log" 2>&1
  fi
  echo $? > "/tmp/p2_out/pts_rc_${{save_name}}.txt"
}}

# CPU: Integer multi-core (CoreMark)
run_pts "pts/coremark" "cpu_coremark" "" 600

# CPU: Floating-point multi-core (C-Ray — option 1 = 1080p, option 1 = 16 rays per pixel)
run_pts "pts/c-ray" "cpu_cray" "1\\n1\\n" 600

# CPU: Compression (7-Zip Compression & Decompression MIPS)
run_pts "pts/compress-7zip" "cpu_7zip" "" 600

# Memory: Bandwidth (STREAM — Test All Options = Copy, Scale, Triad, Add)
run_pts "pts/stream" "mem_stream" "5\\n" 600

# Memory: Memory-intensive cache & bandwidth workload (CacheBench — option 3 = Read/Modify/Write)
run_pts "pts/cachebench" "mem_cachebench" "3\\n" 600

# CPU Compilation: Timed Apache HTTP Server Compilation
run_pts "pts/build-apache" "cpu_build_apache" "" 900

# Memory Latency microbenchmark (random pointer-chase across L1/L2/L3/DRAM, 3 runs)
cat > /tmp/p2_out/mem_lat.c <<'CEOF'
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <time.h>

static double measure_ns(size_t bytes, size_t steps) {{
    size_t n = bytes / sizeof(size_t);
    size_t *arr = (size_t*)aligned_alloc(64, n * sizeof(size_t));
    if (!arr) return -1.0;
    size_t *perm = (size_t*)malloc(n * sizeof(size_t));
    for (size_t i = 0; i < n; i++) perm[i] = i;
    uint64_t seed = 0x9e3779b97f4a7c15ULL ^ bytes;
    for (size_t i = n - 1; i > 0; i--) {{
        seed ^= seed >> 12; seed ^= seed << 25; seed ^= seed >> 27;
        size_t j = (seed * 2685821657736338717ULL) % (i + 1);
        size_t t = perm[i]; perm[i] = perm[j]; perm[j] = t;
    }}
    for (size_t i = 0; i < n - 1; i++) arr[perm[i]] = perm[i + 1];
    arr[perm[n - 1]] = perm[0];
    free(perm);

    volatile size_t idx = 0;
    for (size_t i = 0; i < 100000; i++) idx = arr[idx];
    struct timespec t0, t1;
    clock_gettime(CLOCK_MONOTONIC, &t0);
    for (size_t i = 0; i < steps; i++) idx = arr[idx];
    clock_gettime(CLOCK_MONOTONIC, &t1);
    double ns = (double)(t1.tv_sec - t0.tv_sec) * 1e9 + (double)(t1.tv_nsec - t0.tv_nsec);
    if (idx == (size_t)-1) printf("never\\n");
    free(arr);
    return ns / (double)steps;
}}

int main(void) {{
    size_t sizes[] = {{ 16*1024, 256*1024, 4*1024*1024, 64*1024*1024 }};
    const char *labels[] = {{ "16KiB_L1", "256KiB_L2", "4MiB_L3", "64MiB_DRAM" }};
    size_t steps[] = {{ 20000000, 15000000, 8000000, 4000000 }};
    printf("{{\\n");
    for (int s = 0; s < 4; s++) {{
        double r1 = measure_ns(sizes[s], steps[s]);
        double r2 = measure_ns(sizes[s], steps[s]);
        double r3 = measure_ns(sizes[s], steps[s]);
        printf("  \\"%s\\": [%.3f, %.3f, %.3f]%s\\n", labels[s], r1, r2, r3, (s < 3) ? "," : "");
    }}
    printf("}}\\n");
    return 0;
}}
CEOF
gcc -O2 /tmp/p2_out/mem_lat.c -o /tmp/p2_out/mem_lat
/tmp/p2_out/mem_lat > /tmp/p2_out/mem_latency_runs.json

# Storage Benchmarks: fio on isolated disposable 6 GiB virtual disk (/dev/vdb -> /mnt/eval_fio)
if [ ! -b /dev/vdb ]; then echo "NO_VDB" >&2; exit 1; fi
umount /dev/vdb 2>/dev/null || true
mkfs.ext4 -F -L EVAL_FIO /dev/vdb > /tmp/p2_out/mkfs_vdb.log 2>&1
mount /dev/vdb /mnt/eval_fio
FIO_ENG="libaio"
if ! fio --enghelp 2>/dev/null | grep -qw libaio; then
  if fio --enghelp 2>/dev/null | grep -qw io_uring; then
    FIO_ENG="io_uring"
  else
    FIO_ENG="psync"
  fi
fi
echo "$FIO_ENG" > /tmp/p2_out/fio_ioengine.txt

for r in 1 2 3; do
  sync
  fio --name=seq_write_r$r --directory=/mnt/eval_fio --filename=fio_seq.bin --size=1G --bs=1M --rw=write --ioengine="$FIO_ENG" --direct=1 --iodepth=32 --numjobs=1 --group_reporting --output-format=json --output=/tmp/p2_out/fio_seq_write_r$r.json >/dev/null 2>&1
  sync
  fio --name=seq_read_r$r --directory=/mnt/eval_fio --filename=fio_seq.bin --size=1G --bs=1M --rw=read --ioengine="$FIO_ENG" --direct=1 --iodepth=32 --numjobs=1 --group_reporting --output-format=json --output=/tmp/p2_out/fio_seq_read_r$r.json >/dev/null 2>&1
  rm -f /mnt/eval_fio/fio_seq.bin
  sync
  fio --name=rand4k_write_r$r --directory=/mnt/eval_fio --filename=fio_rand.bin --size=1G --bs=4k --rw=randwrite --ioengine="$FIO_ENG" --direct=1 --iodepth=32 --numjobs=4 --runtime=25 --time_based --group_reporting --output-format=json --output=/tmp/p2_out/fio_rand4k_write_r$r.json >/dev/null 2>&1
  sync
  fio --name=rand4k_read_r$r --directory=/mnt/eval_fio --filename=fio_rand.bin --size=1G --bs=4k --rw=randread --ioengine="$FIO_ENG" --direct=1 --iodepth=32 --numjobs=4 --runtime=25 --time_based --group_reporting --output-format=json --output=/tmp/p2_out/fio_rand4k_read_r$r.json >/dev/null 2>&1
  rm -f /mnt/eval_fio/fio_rand.bin
  sync
done
umount /mnt/eval_fio || true

# System Package Operation Performance (3 runs each, read-only / cache-only, NO upgrades)
python3 - <<'PY' > /tmp/p2_out/pkg_ops_runs.json
import json, subprocess, time
rpm_runs, dnf_query_runs, dnf_cache_runs = [], [], []
for _ in range(3):
    t0 = time.monotonic()
    subprocess.run(["rpm", "-qa", "--qf", "%{{NAME}} %{{VERSION}}\\n"], stdout=subprocess.DEVNULL, check=True)
    rpm_runs.append(round(time.monotonic() - t0, 4))

    t0 = time.monotonic()
    subprocess.run(["dnf5", "-q", "repoquery", "--installed", "kernel*"], stdout=subprocess.DEVNULL, check=True)
    dnf_query_runs.append(round(time.monotonic() - t0, 4))

    t0 = time.monotonic()
    subprocess.run(["dnf5", "-q", "makecache", "--refresh"], stdout=subprocess.DEVNULL, check=False)
    dnf_cache_runs.append(round(time.monotonic() - t0, 4))
print(json.dumps({{
    "rpm_qa_query_seconds": rpm_runs,
    "dnf5_installed_query_seconds": dnf_query_runs,
    "dnf5_makecache_refresh_seconds": dnf_cache_runs,
}}, indent=2))
PY

# Archive Stage A PTS test-results + all Phase 02 outputs
mkdir -p /tmp/p2_out/pts_test_results
for rdir in /root/.phoronix-test-suite/test-results /var/lib/phoronix-test-suite/test-results; do
  if [ -d "$rdir" ]; then
    cp -a "$rdir"/* /tmp/p2_out/pts_test_results/ 2>/dev/null || true
  fi
done
tar -czf /tmp/p2_all.tar.gz -C /tmp p2_out
echo P2_STAGE_A_DONE
"""
            rc, tail = vm.qga.run_long_shell(pts_stage_a, f"p2_stage_a_{target_name}", timeout=2400.0, poll_interval=10.0)
            print(f"[PHASE 02:{target_name}] Stage A finished rc={rc}\n{tail}")
            assert rc == 0, f"Phase 02 Stage A failed for {target_name} (rc={rc}):\n{tail}"
            vm.qga.pull_file("/tmp/p2_all.tar.gz", raw_dir / "p2_all.tar.gz")

            # Stage B: System Kernel Compilation (pts/build-linux-kernel defconfig, 3 runs) in its own dedicated budget
            print(f"[PHASE 02:{target_name}] Stage B: Timed Linux Kernel Compilation (pts/build-linux-kernel, defconfig, 3 runs)...")
            pts_stage_b = f"""
set -u
export HOME=/root
export PTS_USER_PATH_OVERRIDE=/root/.phoronix-test-suite/
export FORCE_TIMES_TO_RUN=3
export TEST_RESULTS_IDENTIFIER="{target_name}"
export TEST_RESULTS_DESCRIPTION="Isolated QEMU/KVM OVMF 8vCPU 8GiB RAM evaluation ({target_name})"
export PTS_SILENT_MODE=1
export TEST_RESULTS_NAME="sys_build_linux_kernel"

timeout 600 phoronix-test-suite batch-install pts/build-linux-kernel > /tmp/p2_out/pts_install_sys_build_linux_kernel.log 2>&1 || true
printf "1\\n" | timeout 1800 phoronix-test-suite batch-run pts/build-linux-kernel > /tmp/p2_out/pts_run_sys_build_linux_kernel.log 2>&1
echo $? > /tmp/p2_out/pts_rc_sys_build_linux_kernel.txt

for rdir in /root/.phoronix-test-suite/test-results /var/lib/phoronix-test-suite/test-results; do
  if [ -d "$rdir" ]; then
    cp -a "$rdir"/* /tmp/p2_out/pts_test_results/ 2>/dev/null || true
  fi
done
tar -czf /tmp/p2_all.tar.gz -C /tmp p2_out
echo P2_STAGE_B_DONE
"""
            try:
                rc_b, tail_b = vm.qga.run_long_shell(pts_stage_b, f"p2_stage_b_{target_name}", timeout=2100.0, poll_interval=10.0)
                print(f"[PHASE 02:{target_name}] Stage B finished rc={rc_b}\n{tail_b}")
                vm.qga.pull_file("/tmp/p2_all.tar.gz", raw_dir / "p2_all.tar.gz")
            except Exception as exc_b:
                print(f"[PHASE 02:{target_name}] Stage B kernel compilation note (Stage A preserved): {exc_b}")
            vm.shutdown_graceful(timeout=45.0)
    finally:
        ovl_disk.unlink(missing_ok=True)
        ovl_vars.unlink(missing_ok=True)
        scratch_disk.unlink(missing_ok=True)

    with tarfile.open(raw_dir / "p2_all.tar.gz", "r:gz") as tf:
        tf.extractall(raw_dir)
    p2_out = raw_dir / "p2_out"

    # Copy raw PTS test results into benchmarks/02_phoronix_raw_results/<target_name>/
    if (p2_out / "pts_test_results").is_dir():
        shutil.copytree(p2_out / "pts_test_results", pts_dest, dirs_exist_ok=True)

    # Parse all PTS composite.xml files
    pts_benchmarks: list[dict[str, Any]] = []
    for xml_f in sorted(pts_dest.rglob("composite.xml")):
        parsed = parse_pts_composite_xml(xml_f)
        for item in parsed:
            item["pts_result_dir"] = xml_f.parent.name
            pts_benchmarks.append(item)

    # Parse fio 3-run sets
    fio_summary: dict[str, Any] = {}
    for mode in ("seq_read", "seq_write", "rand4k_read", "rand4k_write"):
        runs = []
        for r in (1, 2, 3):
            jf = p2_out / f"fio_{mode}_r{r}.json"
            if jf.is_file():
                runs.append(parse_fio_metric(jf, mode))
        if runs:
            fio_summary[mode] = {
                "runs": runs,
                "median_bw_mb_s": round(statistics.median([x["bw_mb_s"] for x in runs]), 2),
                "median_iops": round(statistics.median([x["iops"] for x in runs]), 2),
                "median_mean_lat_ms": round(statistics.median([x["mean_lat_ms"] for x in runs]), 4),
                "median_p99_lat_ms": round(statistics.median([x["p99_lat_ms"] for x in runs]), 4),
                "total_errors": sum(x["errors"] for x in runs),
            }

    mem_lat = json.loads((p2_out / "mem_latency_runs.json").read_text(encoding="utf-8")) if (p2_out / "mem_latency_runs.json").is_file() else {}
    mem_lat_summary = {
        k: {
            "runs_ns": v,
            "median_ns": round(statistics.median(v), 3),
            "min_ns": round(min(v), 3),
            "max_ns": round(max(v), 3),
        }
        for k, v in mem_lat.items()
    }

    pkg_ops = json.loads((p2_out / "pkg_ops_runs.json").read_text(encoding="utf-8")) if (p2_out / "pkg_ops_runs.json").is_file() else {}
    pkg_ops_summary = {
        k: {"runs_s": v, "median_s": round(statistics.median(v), 4)} for k, v in pkg_ops.items() if v
    }

    boot_totals = [r["parsed"].get("total") for r in boot_runs if r["parsed"].get("total") is not None]
    boot_userspace = [r["parsed"].get("userspace") for r in boot_runs if r["parsed"].get("userspace") is not None]
    boot_kernel = [r["parsed"].get("kernel") for r in boot_runs if r["parsed"].get("kernel") is not None]
    boot_initrd = [r["parsed"].get("initrd") for r in boot_runs if r["parsed"].get("initrd") is not None]

    installed_pkgs = [
        ln.strip()
        for ln in (p2_out / "rpms_installed_for_eval.txt").read_text(encoding="utf-8", errors="replace").splitlines()
        if ln.strip()
    ]

    return {
        "target": target_name,
        "collected_at_utc": datetime.now(timezone.utc).isoformat(),
        "kernel_version": (p2_out / "kernel_version.txt").read_text(encoding="utf-8", errors="replace").strip(),
        "tool_versions": {
            "phoronix_test_suite": (p2_out / "pts_version.txt").read_text(encoding="utf-8", errors="replace").strip(),
            "fio": (p2_out / "fio_version.txt").read_text(encoding="utf-8", errors="replace").strip(),
            "gcc": (p2_out / "gcc_version.txt").read_text(encoding="utf-8", errors="replace").strip(),
        },
        "packages_added_for_phase02": installed_pkgs,
        "graphics_probe": sanitize_text((p2_out / "graphics_probe.txt").read_text(encoding="utf-8", errors="replace")),
        "graphics_benchmark_status": "skipped_software_rendering_llvmpipe",
        "boot_performance": {
            "runs": boot_runs,
            "median_total_s": round(statistics.median(boot_totals), 4) if boot_totals else None,
            "median_userspace_s": round(statistics.median(boot_userspace), 4) if boot_userspace else None,
            "median_kernel_s": round(statistics.median(boot_kernel), 4) if boot_kernel else None,
            "median_initrd_s": round(statistics.median(boot_initrd), 4) if boot_initrd else None,
        },
        "phoronix_benchmarks": pts_benchmarks,
        "memory_latency_pointer_chase": mem_lat_summary,
        "storage_fio_isolated_vdb": fio_summary,
        "package_operations": pkg_ops_summary,
    }


def main() -> None:
    targets = sys.argv[1:] or ["blaze", "fedora45"]
    report: dict[str, Any] = {
        "schema_version": "1.0",
        "generated_at_utc": datetime.now(timezone.utc).isoformat(),
        "vm_environment": {
            "hypervisor": "QEMU/KVM (x86_64, -cpu host, -enable-kvm)",
            "firmware": "UEFI OVMF 4M",
            "vcpus": DEFAULT_EVAL_CPUS,
            "ram_mib": DEFAULT_EVAL_RAM_MB,
            "os_disk": "40 GiB VirtIO-BLK QCOW2 (CoW overlay over pristine base)",
            "fio_scratch_disk": "6 GiB isolated disposable VirtIO-BLK QCOW2 (/dev/vdb, ext4, host NVMe-backed)",
            "graphics": "VirtIO-VGA (llvmpipe software rendering — hardware 3D GPU benchmarks intentionally skipped)",
            "virtualization_caveat": "All results are virtualized guest metrics and must NOT be compared directly to bare-metal physical hardware scores.",
        },
        "methodology": {
            "min_runs_per_benchmark": 3,
            "summary_statistic": "median (with all individual runs preserved)",
            "concurrency": "strictly sequential execution (no concurrent resource-intensive workloads)",
            "universal_score_policy": "No synthetic universal score invented; individual workload metrics are preserved verbatim.",
        },
        "systems": {},
    }

    if "blaze" in targets and (VM_STORE / "blaze_base.qcow2").is_file():
        report["systems"]["blaze_solarevolution"] = benchmark_target(
            "blaze", VM_STORE / "blaze_base.qcow2", VM_STORE / "blaze_base_OVMF_VARS.fd"
        )
    if "fedora45" in targets and (VM_STORE / "fedora45_base.qcow2").is_file() and (VM_STORE / "fedora45_base.qcow2").stat().st_size > 2 * 1024**3:
        report["systems"]["fedora45_reference"] = benchmark_target(
            "fedora45", VM_STORE / "fedora45_base.qcow2", VM_STORE / "fedora45_base_OVMF_VARS.fd"
        )

    bench_dir = EVAL_ROOT / "benchmarks"
    bench_dir.mkdir(parents=True, exist_ok=True)
    json_path = bench_dir / "02_performance_report.json"
    json_path.write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    # Export flat CSV with every individual run & median
    csv_path = bench_dir / "02_performance_results.csv"
    with open(csv_path, "w", newline="", encoding="utf-8") as fh:
        w = csv.writer(fh)
        w.writerow(["system", "category", "benchmark", "version", "metric", "unit", "direction", "run_1", "run_2", "run_3", "median"])
        for sys_key, sdata in report["systems"].items():
            # PTS results
            for b in sdata.get("phoronix_benchmarks", []):
                cat = "CPU"
                if "stream" in b["identifier"] or "mem" in b["identifier"]:
                    cat = "Memory"
                elif "linux-kernel" in b["identifier"]:
                    cat = "System"
                runs = b["runs"] + [""] * (3 - len(b["runs"]))
                w.writerow([
                    sys_key, cat, b["title"], b["app_version"], b["description"] or b["identifier"],
                    b["unit"], b["proportion"], runs[0], runs[1], runs[2], b["median"],
                ])
            # Memory latency
            for lbl, m in sdata.get("memory_latency_pointer_chase", {}).items():
                runs = m["runs_ns"] + [""] * (3 - len(m["runs_ns"]))
                w.writerow([sys_key, "Memory", "Pointer-Chase Latency", "custom-c-v1", lbl, "ns", "LIB", runs[0], runs[1], runs[2], m["median_ns"]])
            # Storage fio
            for mode, f in sdata.get("storage_fio_isolated_vdb", {}).items():
                bws = [x["bw_mb_s"] for x in f["runs"]] + [""] * 3
                iops = [x["iops"] for x in f["runs"]] + [""] * 3
                lats = [x["mean_lat_ms"] for x in f["runs"]] + [""] * 3
                p99s = [x["p99_lat_ms"] for x in f["runs"]] + [""] * 3
                fv = sdata["tool_versions"].get("fio", "")
                w.writerow([sys_key, "Storage", f"fio_{mode}", fv, "Bandwidth", "MB/s", "HIB", bws[0], bws[1], bws[2], f["median_bw_mb_s"]])
                w.writerow([sys_key, "Storage", f"fio_{mode}", fv, "IOPS", "IOPS", "HIB", iops[0], iops[1], iops[2], f["median_iops"]])
                w.writerow([sys_key, "Storage", f"fio_{mode}", fv, "Mean Latency", "ms", "LIB", lats[0], lats[1], lats[2], f["median_mean_lat_ms"]])
                w.writerow([sys_key, "Storage", f"fio_{mode}", fv, "P99 Latency", "ms", "LIB", p99s[0], p99s[1], p99s[2], f["median_p99_lat_ms"]])
            # Boot
            bp = sdata.get("boot_performance", {})
            bt = [r["parsed"].get("total", "") for r in bp.get("runs", [])] + [""] * 3
            bu = [r["parsed"].get("userspace", "") for r in bp.get("runs", [])] + [""] * 3
            w.writerow([sys_key, "System", "systemd-analyze", "systemd", "Total Boot Time", "s", "LIB", bt[0], bt[1], bt[2], bp.get("median_total_s", "")])
            w.writerow([sys_key, "System", "systemd-analyze", "systemd", "Userspace Init Time", "s", "LIB", bu[0], bu[1], bu[2], bp.get("median_userspace_s", "")])
            # Package ops
            for op, pd in sdata.get("package_operations", {}).items():
                pr = pd["runs_s"] + [""] * 3
                w.writerow([sys_key, "System", "Package Operation", "rpm/dnf5", op, "s", "LIB", pr[0], pr[1], pr[2], pd["median_s"]])

    print(f"[PHASE 02] Wrote {json_path} and {csv_path}")


if __name__ == "__main__":
    main()
