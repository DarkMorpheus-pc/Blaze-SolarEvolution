#!/usr/bin/env python3
"""Phase 03: Security Assessment (OpenSCAP + SCAP Security Guide + Lynis + CIS Mapping).

Safety invariants:
- Runs on a disposable QCOW2 CoW overlay of the pristine Blaze SolarEvolution base disk.
- Installs ONLY openscap-scanner, scap-security-guide, and lynis (NO system upgrades).
- NEVER executes OpenSCAP, CIS, or Lynis remediation (--remediate is strictly forbidden).
- Calculates compliance strictly from applicable checks: Passed / (Passed + Failed) * 100.
"""
from __future__ import annotations

import json
import re
import shutil
import sys
import tarfile
import xml.etree.ElementTree as ET
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
from phase01_04_inventory_sbom import sanitize_text

REPO_ROOT = Path(__file__).resolve().parents[2]
EVAL_ROOT = REPO_ROOT / "solarevolution-evaluation"
VM_STORE = Path("/var/tmp/blaze_eval_vms")
OVL_DIR = Path("/tmp/blaze_eval_overlays")


def parse_xccdf_xml(xml_path: Path) -> dict[str, Any]:
    tree = ET.parse(xml_path)
    root = tree.getroot()

    # Handle arbitrary XCCDF namespace version (1.1 / 1.2)
    ns_uri = ""
    if root.tag.startswith("{"):
        ns_uri = root.tag.split("}")[0].lstrip("{")

    def tag_name(elem: ET.Element) -> str:
        return elem.tag.split("}")[-1] if "}" in elem.tag else elem.tag

    # Build rule title lookup from Benchmark definitions if present
    rule_titles: dict[str, str] = {}
    rule_severities: dict[str, str] = {}
    for el in root.iter():
        if tag_name(el) == "Rule" and el.attrib.get("id"):
            rid = el.attrib["id"]
            rule_severities[rid] = el.attrib.get("severity", "unknown")
            for child in el:
                if tag_name(child) == "title" and child.text:
                    rule_titles[rid] = " ".join(child.text.split())
                    break

    counts = {
        "pass": 0,
        "fail": 0,
        "notapplicable": 0,
        "notselected": 0,
        "notchecked": 0,
        "error": 0,
        "unknown": 0,
        "informational": 0,
        "fixed": 0,
    }
    failed_rules: list[dict[str, Any]] = []
    passed_rules: list[dict[str, Any]] = []
    error_rules: list[dict[str, Any]] = []

    profile_id = ""
    start_time = ""
    end_time = ""

    for tr in root.iter():
        if tag_name(tr) == "TestResult":
            start_time = tr.attrib.get("start-time", "")
            end_time = tr.attrib.get("end-time", "")
            for ch in tr:
                tn = tag_name(ch)
                if tn == "profile":
                    profile_id = ch.attrib.get("idref", "")
                elif tn == "rule-result":
                    rid = ch.attrib.get("idref", "")
                    sev = ch.attrib.get("severity") or rule_severities.get(rid, "unknown")
                    res_text = "unknown"
                    for rch in ch:
                        if tag_name(rch) == "result" and rch.text:
                            res_text = rch.text.strip().lower()
                            break
                    counts[res_text] = counts.get(res_text, 0) + 1
                    rec = {
                        "rule_id": rid,
                        "short_id": rid.replace("xccdf_org.ssgproject.content_rule_", ""),
                        "title": rule_titles.get(rid, rid.replace("xccdf_org.ssgproject.content_rule_", "")),
                        "severity": sev,
                        "result": res_text,
                    }
                    if res_text == "fail":
                        failed_rules.append(rec)
                    elif res_text == "pass":
                        passed_rules.append(rec)
                    elif res_text == "error":
                        error_rules.append(rec)

    applicable = counts["pass"] + counts["fail"]
    pass_pct = round((counts["pass"] / applicable) * 100.0, 2) if applicable > 0 else 0.0

    sev_failed: dict[str, int] = {}
    for r in failed_rules:
        s = r["severity"]
        sev_failed[s] = sev_failed.get(s, 0) + 1

    return {
        "namespace": ns_uri,
        "profile_id": profile_id,
        "start_time": start_time,
        "end_time": end_time,
        "total_applicable_checks": applicable,
        "passed_checks": counts["pass"],
        "failed_checks": counts["fail"],
        "not_applicable_checks": counts["notapplicable"],
        "not_selected_checks": counts["notselected"],
        "not_checked_checks": counts["notchecked"],
        "informational_checks": counts["informational"],
        "assessment_errors": counts["error"] + counts["unknown"],
        "pass_percentage": pass_pct,
        "scoring_formula": "Passed / (Passed + Failed) * 100",
        "failed_by_severity": sev_failed,
        "failed_rules": failed_rules,
        "error_rules": error_rules,
        "passed_rule_count_verified": len(passed_rules),
    }


def parse_lynis_report_dat(dat_text: str) -> dict[str, Any]:
    data: dict[str, Any] = {
        "hardening_index": None,
        "tests_performed": None,
        "warnings": [],
        "suggestions": [],
        "installed_security_controls": [],
        "raw_kv_highlights": {},
    }
    for ln in dat_text.splitlines():
        if not ln or ln.startswith("#") or "=" not in ln:
            continue
        k, v = ln.split("=", 1)
        k = k.strip()
        v = sanitize_text(v.strip())
        if k == "hardening_index":
            try:
                data["hardening_index"] = int(v)
            except ValueError:
                data["hardening_index"] = v
        elif k == "lynis_tests_done":
            try:
                data["tests_performed"] = int(v)
            except ValueError:
                data["tests_performed"] = v
        elif k == "warning[]":
            data["warnings"].append(v)
        elif k == "suggestion[]":
            data["suggestions"].append(v)
        elif k in (
            "lynis_version",
            "firewall_active",
            "firewall_software[]",
            "selinux_status",
            "selinux_mode",
            "audit_daemon_running",
            "apparmor_enabled",
            "malware_scanner_installed",
            "ids_ips_tooling[]",
            "file_integrity_tool[]",
            "compiler_installed",
            "usb_authorized_default",
        ):
            if k.endswith("[]"):
                data["raw_kv_highlights"].setdefault(k[:-2], []).append(v)
            else:
                data["raw_kv_highlights"][k] = v
    return data


def assess_security_target(target_name: str, base_disk: Path, base_vars: Path) -> dict[str, Any]:
    raw_p3 = EVAL_ROOT / "raw-results" / (
        "phase03_security" if target_name == "blaze" else f"phase03_security_{target_name}"
    )
    raw_p3.mkdir(parents=True, exist_ok=True)
    sec_dir = EVAL_ROOT / "security"
    sec_dir.mkdir(parents=True, exist_ok=True)
    suffix = "" if target_name == "blaze" else f"_{target_name}"

    ovl_disk, ovl_vars = create_overlay_pair(base_disk, base_vars, OVL_DIR, f"p3_{target_name}")
    try:
        with EvalQemuVM(
            disk_path=ovl_disk,
            ovmf_vars_path=ovl_vars,
            log_dir=raw_p3,
            vm_label=f"p3_{target_name}",
            boot_order="c",
            memory_mb=DEFAULT_EVAL_RAM_MB,
            cpus=DEFAULT_EVAL_CPUS,
            **target_vm_kwargs(target_name),
        ) as vm:
            print(f"[PHASE 03:{target_name}] Booting isolated overlay for Security Assessment...")
            vm.qga.wait_until_ready(timeout=210.0, poll_interval=2.0)
            vm.qga.run_shell("systemctl is-system-running --wait 2>/dev/null || sleep 8", timeout=60.0)

            script = r"""
set -u
mkdir -p /tmp/p3_out
rpm -qa --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' | sort > /tmp/p3_out/rpms_before.txt

# Install ONLY evaluation security tools (NO dnf upgrade, NO remediation!)
dnf5 install -y --setopt=install_weak_deps=False openscap-scanner scap-security-guide lynis > /tmp/p3_out/dnf_install.log 2>&1
INSTALL_RC=$?
echo "$INSTALL_RC" > /tmp/p3_out/install_rc.txt

rpm -qa --qf '%{NAME}-%{VERSION}-%{RELEASE}.%{ARCH}\n' | sort > /tmp/p3_out/rpms_after.txt
comm -13 /tmp/p3_out/rpms_before.txt /tmp/p3_out/rpms_after.txt > /tmp/p3_out/rpms_installed_for_eval.txt

oscap --version > /tmp/p3_out/oscap_version.txt 2>&1 || true
lynis show version > /tmp/p3_out/lynis_version.txt 2>&1 || true
rpm -q openscap-scanner scap-security-guide lynis > /tmp/p3_out/tool_rpms.txt 2>&1 || true

# 1. Inspect available SCAP Security Guide content & profiles
ls -la /usr/share/xml/scap/ssg/content/ > /tmp/p3_out/ssg_files.txt 2>&1 || true
DS_FILE="/usr/share/xml/scap/ssg/content/ssg-fedora-ds.xml"
if [ ! -f "$DS_FILE" ]; then
  DS_FILE=$(ls /usr/share/xml/scap/ssg/content/ssg-fedora*-ds*.xml 2>/dev/null | head -n1 || true)
fi
echo "$DS_FILE" > /tmp/p3_out/selected_ds.txt

if [ -x /usr/bin/blazeos-scap-adapt ] && [ ! -s /usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml ]; then
  /usr/bin/blazeos-scap-adapt --quiet || true
fi

if [ -n "$DS_FILE" ] && [ -f "$DS_FILE" ]; then
  oscap info "$DS_FILE" > /tmp/p3_out/oscap_info.txt 2>&1 || true

  # Select primary profile: CIS Workstation L1, Standard, CUSP
  PROFILES=$(oscap info "$DS_FILE" 2>/dev/null | grep -oE 'xccdf_org\.ssgproject\.content_profile_[A-Za-z0-9_.-]+' | sort -u)
  echo "$PROFILES" > /tmp/p3_out/available_profiles.txt

  PRIMARY_PROFILE=""
  SECONDARY_PROFILE=""
  for cand in \
    xccdf_org.ssgproject.content_profile_cis_workstation_l1 \
    xccdf_org.ssgproject.content_profile_standard \
    xccdf_org.ssgproject.content_profile_ospp; do
    if echo "$PROFILES" | grep -qx "$cand"; then
      if [ -z "$PRIMARY_PROFILE" ]; then
        PRIMARY_PROFILE="$cand"
      elif [ -z "$SECONDARY_PROFILE" ]; then
        SECONDARY_PROFILE="$cand"
      fi
    fi
  done
  if [ -z "$PRIMARY_PROFILE" ]; then
    PRIMARY_PROFILE=$(echo "$PROFILES" | head -n1)
  fi
  echo "$PRIMARY_PROFILE" > /tmp/p3_out/primary_profile.txt
  echo "$SECONDARY_PROFILE" > /tmp/p3_out/secondary_profile.txt

  if [ -s /usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml ]; then
    # 1a. Unmodified upstream Fedora datastream (documents CPE behavior without adapter)
    oscap xccdf eval \
      --profile "$PRIMARY_PROFILE" \
      --results /tmp/p3_out/03_openscap_unmodified_cpe_results.xml \
      "$DS_FILE" > /tmp/p3_out/oscap_unmodified_stdout.log 2>&1 || true
    # 1b. Official shipped blaze-scap-compat datastream
    EVAL_DS="/usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml"
    echo "blaze-scap-compat" > /tmp/p3_out/scap_source.txt
  else
    # Reference Fedora 45 matches upstream ssg-fedora-ds.xml directly
    EVAL_DS="$DS_FILE"
    echo "upstream-ssg-fedora" > /tmp/p3_out/scap_source.txt
  fi
  echo "$EVAL_DS" > /tmp/p3_out/evaluated_ds.txt

  # Run primary profile (read-only audit, NO --remediate!)
  oscap xccdf eval \
    --profile "$PRIMARY_PROFILE" \
    --results /tmp/p3_out/03_openscap_results.xml \
    --report /tmp/p3_out/03_openscap_report.html \
    "$EVAL_DS" > /tmp/p3_out/oscap_primary_stdout.log 2>&1
  echo $? > /tmp/p3_out/oscap_primary_rc.txt

  # Run secondary profile if present
  if [ -n "$SECONDARY_PROFILE" ]; then
    oscap xccdf eval \
      --profile "$SECONDARY_PROFILE" \
      --results /tmp/p3_out/03_openscap_secondary_results.xml \
      --report /tmp/p3_out/03_openscap_secondary_report.html \
      "$EVAL_DS" > /tmp/p3_out/oscap_secondary_stdout.log 2>&1
    echo $? > /tmp/p3_out/oscap_secondary_rc.txt
  fi

  # Run Fedora Workstation CUSP profile if available
  if echo "$PROFILES" | grep -qx "xccdf_org.ssgproject.content_profile_cusp_fedora"; then
    oscap xccdf eval \
      --profile "xccdf_org.ssgproject.content_profile_cusp_fedora" \
      --results /tmp/p3_out/03_openscap_cusp_results.xml \
      --report /tmp/p3_out/03_openscap_cusp_report.html \
      "$EVAL_DS" > /tmp/p3_out/oscap_cusp_stdout.log 2>&1
    echo $? > /tmp/p3_out/oscap_cusp_rc.txt
  fi
fi

# 2. Run Lynis full system audit (read-only, quick mode, no colors)
lynis audit system --quick --no-colors --auditor "BlazeEval" \
  --logfile /tmp/p3_out/03_lynis_audit.log \
  --report-file /tmp/p3_out/lynis-report.dat \
  > /tmp/p3_out/lynis_stdout.log 2>&1 || true

# 3. Direct system security posture evidence
{
  echo "=== SELINUX ==="
  sestatus 2>&1 || getenforce 2>&1 || true
  echo "=== FIREWALLD ==="
  firewall-cmd --state 2>&1 || true
  firewall-cmd --list-all 2>&1 || true
  echo "=== LISTENING SOCKETS (ss -tulnp) ==="
  ss -tulnp 2>&1 || true
  echo "=== SSHD STATUS & CONFIG ==="
  systemctl is-enabled sshd 2>&1 || true
  systemctl is-active sshd 2>&1 || true
  sshd -T 2>/dev/null | grep -iE 'permitrootlogin|passwordauthentication|pubkeyauthentication|x11forwarding|maxauthtries|protocol|allowtcpforwarding' || echo "sshd_not_active_or_installed"
  echo "=== AUDITD ==="
  systemctl is-enabled auditd 2>&1 || true
  systemctl is-active auditd 2>&1 || true
  auditctl -s 2>&1 || true
  echo "=== PASSWORD & AUTH POLICIES ==="
  grep -vE '^\s*(#|$)' /etc/security/pwquality.conf 2>/dev/null || true
  grep -E '^(PASS_MAX_DAYS|PASS_MIN_DAYS|PASS_MIN_LEN|PASS_WARN_AGE|UMASK|ENCRYPT_METHOD|SHA_CRYPT)' /etc/login.defs 2>/dev/null || true
  echo "=== PAM SDDM-AUTOLOGIN & SYSTEM-AUTH ==="
  cat /etc/pam.d/sddm-autologin 2>/dev/null || true
  echo "=== CRITICAL FILE PERMISSIONS ==="
  stat -c '%a %U:%G %n' /etc/passwd /etc/shadow /etc/group /etc/gshadow /etc/sudoers /boot/grub2/grub.cfg 2>/dev/null || true
  echo "=== KERNEL SECURITY SYSCTLS ==="
  sysctl \
    kernel.kptr_restrict \
    kernel.dmesg_restrict \
    kernel.randomize_va_space \
    kernel.yama.ptrace_scope \
    kernel.unprivileged_bpf_disabled \
    kernel.sysrq \
    fs.suid_dumpable \
    fs.protected_hardlinks \
    fs.protected_symlinks \
    net.ipv4.ip_forward \
    net.ipv4.conf.all.accept_redirects \
    net.ipv4.conf.all.send_redirects \
    net.ipv4.conf.all.rp_filter \
    net.ipv4.tcp_syncookies \
    net.ipv6.conf.all.accept_redirects 2>&1 || true
  echo "=== GROUP & PASSWORDLESS ACCOUNT READ-ONLY CHECK ==="
  grpck -r 2>&1 || echo "GRPCK_EXIT=$?"
  awk -F: '($2 == "") {print "EMPTY_PASSWORD_ACCOUNT: " $1}' /etc/shadow 2>/dev/null || true
  echo "=== BLAZE-SETUP ACCOUNT STATUS ==="
  passwd -S blaze-setup 2>/dev/null || echo "blaze-setup_not_present"
  echo "=== LOGGING ==="
  systemctl is-active systemd-journald rsyslog 2>&1 || true
  journalctl --disk-usage 2>&1 || true
  echo "=== BLAZE SENTINEL ==="
  if command -v blaze-sentinel >/dev/null 2>&1; then blaze-sentinel --json 2>&1 || blaze-sentinel 2>&1 || true; fi
  echo "=== AKREP SECURITY CENTER ==="
  if systemctl list-unit-files akrepd.service >/dev/null 2>&1; then
    echo "akrepd_enabled=$(systemctl is-enabled akrepd.service 2>/dev/null || true)"
    echo "akrepd_active=$(systemctl is-active akrepd.service 2>/dev/null || true)"
    systemctl status akrepd.service --no-pager 2>&1 | head -n 20 || true
    busctl --system call tr.com.blazeos.Akrep1 /tr/com/blazeos/Akrep1 tr.com.blazeos.Akrep1.Manager GetVersion 2>&1 || true
    busctl --system call tr.com.blazeos.Akrep1 /tr/com/blazeos/Akrep1 tr.com.blazeos.Akrep1.Manager GetCapabilities 2>&1 || true
    busctl --system call tr.com.blazeos.Akrep1 /tr/com/blazeos/Akrep1 tr.com.blazeos.Akrep1.Manager GetSnapshot 2>&1 || true
    echo "=== AKREP AVC CHECK ==="
    ausearch -m avc -ts boot 2>&1 | grep -i akrep || echo "ZERO_AKREP_AVC"
  else
    echo "akrepd_not_installed"
  fi
} > /tmp/p3_out/direct_security_posture.txt

tar -czf /tmp/p3_out.tar.gz -C /tmp p3_out
echo P3_DONE
"""
            rc, tail = vm.qga.run_long_shell(script, f"phase03_security_{target_name}", timeout=900.0, poll_interval=5.0)
            print(f"[PHASE 03:{target_name}] Finished rc={rc}\n{tail}")
            assert rc == 0, f"Phase 03 ({target_name}) failed (rc={rc}):\n{tail}"
            vm.qga.pull_file("/tmp/p3_out.tar.gz", raw_p3 / "p3_out.tar.gz")
            vm.shutdown_graceful(timeout=45.0)
    finally:
        ovl_disk.unlink(missing_ok=True)
        ovl_vars.unlink(missing_ok=True)

    with tarfile.open(raw_p3 / "p3_out.tar.gz", "r:gz") as tf:
        tf.extractall(raw_p3)

    p3_dir = raw_p3 / "p3_out"
    for src_name, dst_base in (
        ("03_openscap_results.xml", f"03_openscap_results{suffix}.xml"),
        ("03_openscap_report.html", f"03_openscap_report{suffix}.html"),
        ("03_openscap_secondary_results.xml", f"03_openscap_secondary_results{suffix}.xml"),
        ("03_openscap_secondary_report.html", f"03_openscap_secondary_report{suffix}.html"),
        ("03_openscap_cusp_results.xml", f"03_openscap_cusp_results{suffix}.xml"),
        ("03_openscap_cusp_report.html", f"03_openscap_cusp_report{suffix}.html"),
    ):
        if (p3_dir / src_name).is_file():
            shutil.copy2(p3_dir / src_name, sec_dir / dst_base)

    lynis_raw = (p3_dir / "03_lynis_audit.log").read_text(encoding="utf-8", errors="replace") if (p3_dir / "03_lynis_audit.log").is_file() else ""
    (sec_dir / f"03_lynis_audit{suffix}.log").write_text(sanitize_text(lynis_raw), encoding="utf-8")

    unmodified_parsed = (
        parse_xccdf_xml(p3_dir / "03_openscap_unmodified_cpe_results.xml")
        if (p3_dir / "03_openscap_unmodified_cpe_results.xml").is_file()
        else None
    )
    primary_parsed = parse_xccdf_xml(sec_dir / f"03_openscap_results{suffix}.xml") if (sec_dir / f"03_openscap_results{suffix}.xml").is_file() else {}
    secondary_parsed = parse_xccdf_xml(sec_dir / f"03_openscap_secondary_results{suffix}.xml") if (sec_dir / f"03_openscap_secondary_results{suffix}.xml").is_file() else None
    cusp_parsed = parse_xccdf_xml(sec_dir / f"03_openscap_cusp_results{suffix}.xml") if (sec_dir / f"03_openscap_cusp_results{suffix}.xml").is_file() else None
    lynis_dat = (p3_dir / "lynis-report.dat").read_text(encoding="utf-8", errors="replace") if (p3_dir / "lynis-report.dat").is_file() else ""
    lynis_parsed = parse_lynis_report_dat(lynis_dat)

    installed_pkgs = [
        ln.strip()
        for ln in (p3_dir / "rpms_installed_for_eval.txt").read_text(encoding="utf-8", errors="replace").splitlines()
        if ln.strip()
    ]
    scap_source = (p3_dir / "scap_source.txt").read_text(encoding="utf-8", errors="replace").strip() if (p3_dir / "scap_source.txt").is_file() else ""
    cpe_note = (
        "Unmodified upstream ssg-fedora-ds.xml marks all rules 'notapplicable' on CPE 'cpe:/o:blazeos:solarevolution:5.05.1'. "
        "Blaze SolarEvolution 5.05.1 ships the signed 'blaze-scap-compat' RPM supplying '/usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml' "
        "via an RPM trigger when scap-security-guide is present, preserving upstream ssg-fedora-ds.xml untouched while enabling native XCCDF evaluation."
        if scap_source == "blaze-scap-compat"
        else "Upstream Fedora 45 matches native 'cpe:/o:fedoraproject:fedora:45' directly in ssg-fedora-ds.xml."
    )

    findings = {
        "collected_at_utc": datetime.now(timezone.utc).isoformat(),
        "target": target_name,
        "remediation_applied": False,
        "certification_disclaimer": "Automated configuration assessment only; NOT an official CIS, NIST, or third-party certification.",
        "cpe_interoperability_note": cpe_note,
        "tools": {
            "oscap_version": (p3_dir / "oscap_version.txt").read_text(encoding="utf-8", errors="replace").splitlines()[0] if (p3_dir / "oscap_version.txt").is_file() else "",
            "lynis_version": (p3_dir / "lynis_version.txt").read_text(encoding="utf-8", errors="replace").strip() if (p3_dir / "lynis_version.txt").is_file() else "",
            "installed_tool_rpms": [
                ln.strip() for ln in (p3_dir / "tool_rpms.txt").read_text(encoding="utf-8", errors="replace").splitlines() if ln.strip()
            ],
            "all_packages_added_for_phase03": installed_pkgs,
        },
        "scap_content": {
            "scap_source": scap_source,
            "selected_datastream": (p3_dir / "selected_ds.txt").read_text(encoding="utf-8", errors="replace").strip(),
            "evaluated_datastream": (p3_dir / "evaluated_ds.txt").read_text(encoding="utf-8", errors="replace").strip() if (p3_dir / "evaluated_ds.txt").is_file() else "",
            "available_profiles": [
                ln.strip() for ln in (p3_dir / "available_profiles.txt").read_text(encoding="utf-8", errors="replace").splitlines() if ln.strip()
            ],
            "primary_profile": (p3_dir / "primary_profile.txt").read_text(encoding="utf-8", errors="replace").strip(),
            "secondary_profile": (p3_dir / "secondary_profile.txt").read_text(encoding="utf-8", errors="replace").strip(),
        },
        "openscap_unmodified_cpe": unmodified_parsed,
        "openscap_primary": primary_parsed,
        "openscap_secondary": secondary_parsed,
        "openscap_cusp_fedora": cusp_parsed,
        "lynis": lynis_parsed,
        "direct_posture_sanitized": sanitize_text(
            (p3_dir / "direct_security_posture.txt").read_text(encoding="utf-8", errors="replace")
        ),
    }

    out_json = sec_dir / f"03_security_findings{suffix}.json"
    out_json.write_text(json.dumps(findings, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(
        f"[PHASE 03:{target_name}] Primary OpenSCAP ({findings['scap_content']['primary_profile']}): "
        f"pass={primary_parsed.get('passed_checks')}, fail={primary_parsed.get('failed_checks')}, "
        f"applicable={primary_parsed.get('total_applicable_checks')}, pct={primary_parsed.get('pass_percentage')}% | "
        f"Lynis Hardening Index={lynis_parsed.get('hardening_index')}"
    )
    return findings


def main() -> None:
    targets = sys.argv[1:] or ["blaze", "fedora45"]
    if "blaze" in targets and (VM_STORE / "blaze_base.qcow2").is_file():
        assess_security_target("blaze", VM_STORE / "blaze_base.qcow2", VM_STORE / "blaze_base_OVMF_VARS.fd")
    if "fedora45" in targets and (VM_STORE / "fedora45_base.qcow2").is_file():
        assess_security_target("fedora45", VM_STORE / "fedora45_base.qcow2", VM_STORE / "fedora45_base_OVMF_VARS.fd")


if __name__ == "__main__":
    main()
