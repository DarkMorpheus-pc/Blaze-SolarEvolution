use crate::adapters::overrides::{resolve_binary, run_command_with_timeout};
use crate::adapters::selinux::command_c_locale;
use blaze_akrep_common::{
    redact_secrets, AkrepError, DeviceSecurityStatus, KernelLockdownStatus, RpmIntegrityStatus,
    RuntimeIntegrityStatus, SecureBootState, SecureBootStatus, TpmStatus, UsbAuditReport,
    UsbDeviceEntry,
};
use blaze_sentinel_core::{
    evaluate_security_posture, is_canary_intact, AkrepSeverity, Availability, DriverMokSummary,
    SentinelEngine, ROOT_CANARY_PATH,
};
use chrono::Utc;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const EFI_SECURE_BOOT_VAR_REL: &str =
    "firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";

pub const CRITICAL_SECURITY_PACKAGES: &[&str] = &[
    "blaze-release",
    "blaze-akrep",
    "blaze-akrep-daemon",
    "blaze-akrep-ui",
    "blaze-recovery",
    "sudo",
    "polkit",
    "systemd-pam",
];

pub trait DeviceSecurityOps: Send + Sync {
    fn inspect(&self) -> Result<DeviceSecurityStatus, AkrepError>;
}

pub fn inspect_secure_boot_at(sys_root: &Path) -> SecureBootStatus {
    let efi_dir = sys_root.join("firmware/efi");
    if !efi_dir.exists() {
        return SecureBootStatus {
            uefi_present: false,
            secure_boot_enabled: None,
            state: SecureBootState::LegacyBios,
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
            detail:
                "Legacy BIOS boot detected (/sys/firmware/efi absent); UEFI Secure Boot unavailable"
                    .to_string(),
        };
    }

    let sb_var = sys_root.join(EFI_SECURE_BOOT_VAR_REL);
    let bios_vendor = fs::read_to_string(sys_root.join("class/dmi/id/bios_vendor"))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let product_name = fs::read_to_string(sys_root.join("class/dmi/id/product_name"))
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let is_virtual_ovmf = bios_vendor.to_ascii_lowercase().contains("edk ii")
        || bios_vendor.to_ascii_lowercase().contains("ovmf")
        || product_name.to_ascii_lowercase().contains("qemu")
        || product_name.to_ascii_lowercase().contains("standard pc");

    match fs::read(&sb_var) {
        Ok(bytes) if !bytes.is_empty() => match *bytes.last().unwrap_or(&0xff) {
            1 => SecureBootStatus {
                uefi_present: true,
                secure_boot_enabled: Some(true),
                state: SecureBootState::Enabled,
                severity: AkrepSeverity::Protected,
                availability: Availability::Available,
                detail: "UEFI Secure Boot enabled (efivarfs SecureBoot=1)".to_string(),
            },
            0 => SecureBootStatus {
                uefi_present: true,
                secure_boot_enabled: Some(false),
                state: SecureBootState::Disabled,
                severity: AkrepSeverity::Attention,
                availability: Availability::Available,
                detail: if is_virtual_ovmf {
                    "UEFI Secure Boot disabled on virtual OVMF firmware (SecureBoot=0)".to_string()
                } else {
                    "UEFI Secure Boot disabled in firmware (SecureBoot=0)".to_string()
                },
            },
            other => SecureBootStatus {
                uefi_present: true,
                secure_boot_enabled: None,
                state: SecureBootState::UnsupportedFirmware,
                severity: AkrepSeverity::Attention,
                availability: Availability::Unavailable,
                detail: format!(
                    "Unexpected UEFI SecureBoot efivar byte 0x{:02x}; Secure Boot unverified",
                    other
                ),
            },
        },
        _ => SecureBootStatus {
            uefi_present: true,
            secure_boot_enabled: None,
            state: SecureBootState::UnsupportedFirmware,
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
            detail: if is_virtual_ovmf {
                "Virtual OVMF UEFI firmware without SecureBoot variable; Secure Boot unavailable"
                    .to_string()
            } else {
                "UEFI firmware present but SecureBoot efivar unreadable or unsupported".to_string()
            },
        },
    }
}

pub fn inspect_tpm_at(sys_root: &Path, dev_root: &Path) -> TpmStatus {
    let tpm0_dir = sys_root.join("class/tpm/tpm0");
    let ver_major = fs::read_to_string(tpm0_dir.join("tpm_version_major"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let present = tpm0_dir.exists() || dev_root.join("tpmrm0").exists() || ver_major.is_some();

    if ver_major.as_deref() == Some("2") {
        TpmStatus {
            present: true,
            version_major: Some("2".to_string()),
            tpm2_available: true,
            severity: AkrepSeverity::Protected,
            availability: Availability::Available,
            detail: "TPM 2.0 hardware security module available".to_string(),
        }
    } else if let Some(v) = ver_major {
        TpmStatus {
            present,
            version_major: Some(v.clone()),
            tpm2_available: false,
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
            detail: format!("Legacy TPM {}.x detected; TPM 2.0 unavailable", v),
        }
    } else {
        TpmStatus {
            present: false,
            version_major: None,
            tpm2_available: false,
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
            detail: "TPM 2.0 device not present".to_string(),
        }
    }
}

pub fn inspect_kernel_lockdown_at(proc_root: &Path, sys_root: &Path) -> KernelLockdownStatus {
    let cmdline = fs::read_to_string(proc_root.join("cmdline")).unwrap_or_default();
    let cmdline_sig = cmdline
        .split_whitespace()
        .any(|tok| tok == "module.sig_enforce=1" || tok == "module.sig_enforce=y");

    let sysfs_sig_raw = fs::read_to_string(sys_root.join("module/module/parameters/sig_enforce"))
        .ok()
        .map(|s| s.trim().to_ascii_uppercase())
        .unwrap_or_default();
    let sysfs_sig = matches!(sysfs_sig_raw.as_str(), "Y" | "1");
    let module_sig_enforced = cmdline_sig || sysfs_sig;

    let lockdown_mode = match fs::read_to_string(sys_root.join("kernel/security/lockdown")) {
        Ok(raw) => {
            if raw.contains("[confidentiality]") {
                "confidentiality".to_string()
            } else if raw.contains("[integrity]") {
                "integrity".to_string()
            } else if raw.contains("[none]") {
                "none".to_string()
            } else {
                let t = raw.trim();
                if t.is_empty() {
                    "unavailable".to_string()
                } else {
                    t.to_string()
                }
            }
        }
        Err(_) => "unavailable".to_string(),
    };

    let iommu_active = fs::read_dir(sys_root.join("class/iommu"))
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);

    let severity = if lockdown_mode == "unavailable" && !module_sig_enforced {
        AkrepSeverity::Unavailable
    } else if module_sig_enforced
        && matches!(lockdown_mode.as_str(), "integrity" | "confidentiality")
    {
        AkrepSeverity::Protected
    } else {
        AkrepSeverity::Attention
    };

    let detail = format!(
        "module_sig_enforced={}, lockdown={}, iommu={}",
        module_sig_enforced, lockdown_mode, iommu_active
    );

    KernelLockdownStatus {
        cmdline_module_sig_enforce: cmdline_sig,
        sysfs_module_sig_enforce: sysfs_sig,
        module_sig_enforced,
        lockdown_mode,
        iommu_active,
        severity,
        detail,
    }
}

pub fn inspect_ld_so_preload_file(preload_path: &Path) -> (bool, String) {
    if !preload_path.exists() {
        return (true, "Absent (/etc/ld.so.preload not present)".to_string());
    }
    let Ok(raw) = fs::read_to_string(preload_path) else {
        return (
            false,
            "Unreadable /etc/ld.so.preload requires manual inspection".to_string(),
        );
    };
    let entries: Vec<&str> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    if entries.is_empty() {
        return (
            true,
            "Empty (/etc/ld.so.preload has zero entries)".to_string(),
        );
    }
    let all_allowed = entries
        .iter()
        .all(|e| *e == "/usr/lib64/libsolar_brand.so" || e.ends_with("/libsolar_brand.so"));
    if all_allowed {
        (
            true,
            "Verified (only sanctioned libsolar_brand.so present)".to_string(),
        )
    } else {
        (
            false,
            format!(
                "Unexpected ld.so.preload entry detected: {}",
                entries.join(", ")
            ),
        )
    }
}

pub fn inspect_runtime_integrity_at(etc_root: &Path, canary_path: &Path) -> RuntimeIntegrityStatus {
    let engine = SentinelEngine::new();
    let posture = evaluate_security_posture(&engine, false);
    let (ld_clean, ld_detail) = inspect_ld_so_preload_file(&etc_root.join("ld.so.preload"));
    let root_canary_present = canary_path.exists();
    let root_canary_intact = if root_canary_present {
        is_canary_intact(canary_path)
    } else {
        false
    };

    let severity =
        if !ld_clean || !posture.integrity_ok || (root_canary_present && !root_canary_intact) {
            AkrepSeverity::Critical
        } else if posture.passed < posture.total {
            AkrepSeverity::Attention
        } else {
            AkrepSeverity::Protected
        };

    RuntimeIntegrityStatus {
        posture,
        ld_so_preload_clean: ld_clean,
        ld_so_preload_detail: ld_detail,
        root_canary_present,
        root_canary_intact,
        severity,
    }
}

fn read_trimmed_opt(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

pub fn inspect_usb_devices_at(sys_root: &Path) -> UsbAuditReport {
    let usb_dir = sys_root.join("bus/usb/devices");
    let Ok(dir_iter) = fs::read_dir(&usb_dir) else {
        return UsbAuditReport {
            devices: Vec::new(),
            unauthorized_count: 0,
            anomaly_count: 0,
            severity: AkrepSeverity::Protected,
        };
    };

    let mut raw_entries: Vec<(String, PathBuf)> = dir_iter
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().to_string(), e.path()))
        .collect();
    raw_entries.sort_by(|a, b| a.0.cmp(&b.0));

    let mut devices = Vec::new();
    let mut unauthorized_count = 0usize;
    let mut anomaly_count = 0usize;

    for (name, dev_path) in &raw_entries {
        if name.contains(':') {
            continue;
        }
        let Some(id_vendor) = read_trimmed_opt(&dev_path.join("idVendor")) else {
            continue;
        };
        let Some(id_product) = read_trimmed_opt(&dev_path.join("idProduct")) else {
            continue;
        };
        let manufacturer = read_trimmed_opt(&dev_path.join("manufacturer"));
        let product = read_trimmed_opt(&dev_path.join("product"));
        let device_class = read_trimmed_opt(&dev_path.join("bDeviceClass"))
            .unwrap_or_else(|| "00".to_string())
            .to_ascii_lowercase();
        let authorized = read_trimmed_opt(&dev_path.join("authorized"))
            .map(|v| v != "0")
            .unwrap_or(true);

        let iface_prefix = format!("{}:", name);
        let mut iface_set = BTreeSet::new();
        for (sub_name, sub_path) in &raw_entries {
            if sub_name.starts_with(&iface_prefix) {
                if let Some(ic) = read_trimmed_opt(&sub_path.join("bInterfaceClass")) {
                    iface_set.insert(ic.to_ascii_lowercase());
                }
            }
        }
        let interface_classes: Vec<String> = iface_set.into_iter().collect();

        let mut anomalies = Vec::new();
        if !authorized {
            unauthorized_count += 1;
            anomalies.push("unauthorized_usb_device".to_string());
        }

        let has_hid = device_class == "03" || interface_classes.iter().any(|c| c == "03");
        let has_storage_or_net = matches!(device_class.as_str(), "08" | "02" | "0a" | "e0")
            || interface_classes
                .iter()
                .any(|c| matches!(c.as_str(), "08" | "02" | "0a" | "e0"));
        let combined_label = format!(
            "{} {}",
            manufacturer.as_deref().unwrap_or_default(),
            product.as_deref().unwrap_or_default()
        )
        .to_ascii_lowercase();
        let storage_or_spoof_label = [
            "mass storage",
            "flash drive",
            "usb disk",
            "sd card",
            "ethernet",
            "rubber ducky",
            "badusb",
            "bash bunny",
        ]
        .iter()
        .any(|kw| combined_label.contains(kw));

        if (has_hid && (has_storage_or_net || storage_or_spoof_label))
            || (id_vendor == "0000" && id_product == "0000")
        {
            anomalies.push("hid_spoof_anomaly".to_string());
        }

        if !anomalies.is_empty() {
            anomaly_count += 1;
        }

        devices.push(UsbDeviceEntry {
            sysfs_name: name.clone(),
            id_vendor,
            id_product,
            manufacturer,
            product,
            device_class,
            interface_classes,
            authorized,
            anomalies,
        });
    }

    let severity = if unauthorized_count > 0 || anomaly_count > 0 {
        AkrepSeverity::Attention
    } else {
        AkrepSeverity::Protected
    };

    UsbAuditReport {
        devices,
        unauthorized_count,
        anomaly_count,
        severity,
    }
}

pub fn parse_rpm_signature_lines(raw: &str) -> (usize, usize, Vec<String>) {
    let mut total = 0usize;
    let mut signed = 0usize;
    let mut unsigned = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut parts = trimmed.splitn(3, '|');
        let Some(name) = parts.next().map(str::trim) else {
            continue;
        };
        if name.is_empty() || name == "gpg-pubkey" {
            continue;
        }
        let rsa = parts.next().map(str::trim).unwrap_or("(none)");
        let openpgp = parts.next().map(str::trim).unwrap_or("(none)");
        total += 1;

        let has_sig = (!rsa.is_empty() && !rsa.eq_ignore_ascii_case("(none)"))
            || (!openpgp.is_empty() && !openpgp.eq_ignore_ascii_case("(none)"));
        if has_sig {
            signed += 1;
        } else {
            unsigned.push(name.to_string());
        }
    }
    unsigned.sort();
    unsigned.dedup();
    (total, signed, unsigned)
}

pub fn parse_rpm_verify_anomalies(pkg: &str, raw: &str) -> Vec<String> {
    let mut anomalies = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // Ignore pure %config or %doc mtime/size tweaks unless a binary/library/polkit/sudoers digest or mode changed
        let is_config_line = trimmed.contains(" c /");
        let flags = trimmed.split_whitespace().next().unwrap_or_default();
        let critical_flag =
            flags == "missing" || flags.contains('5') || flags.contains('M') || flags.contains('L');
        if critical_flag
            && (!is_config_line
                || trimmed.contains("/etc/sudoers")
                || trimmed.contains("/polkit-1/"))
        {
            anomalies.push(format!("{}: {}", pkg, redact_secrets(trimmed)));
        }
    }
    anomalies
}

fn resolve_rpm_binary() -> Option<PathBuf> {
    resolve_binary(&["/bin/rpm", "/usr/bin/rpm"])
}

fn resolve_mokutil_binary() -> Option<PathBuf> {
    resolve_binary(&["/usr/bin/mokutil", "/bin/mokutil"])
}

pub fn parse_mokutil_sb_state(raw: &str) -> Option<bool> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("secureboot enabled") {
        Some(true)
    } else if lower.contains("secureboot disabled") {
        Some(false)
    } else {
        None
    }
}

pub fn parse_mokutil_enrolled_count(raw: &str) -> Option<usize> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("is empty") {
        return Some(0);
    }
    let key_headers = raw
        .lines()
        .filter(|line| line.trim_start().to_ascii_lowercase().starts_with("[key "))
        .count();
    if key_headers > 0 {
        return Some(key_headers);
    }
    let sha1_lines = raw
        .lines()
        .filter(|line| {
            line.trim_start()
                .to_ascii_lowercase()
                .starts_with("sha1 fingerprint:")
        })
        .count();
    if sha1_lines > 0 {
        Some(sha1_lines)
    } else {
        None
    }
}

pub fn inspect_driver_mok_at(
    sys_root: &Path,
    secure_boot: &SecureBootStatus,
    mokutil_bin: Option<&Path>,
    rpm_bin: Option<&Path>,
) -> DriverMokSummary {
    let nvidia_module_loaded = sys_root.join("module/nvidia").exists();

    let mok_sb_state = mokutil_bin.and_then(|bin| {
        let mut cmd = command_c_locale(bin);
        cmd.arg("--sb-state");
        let out =
            run_command_with_timeout(cmd, Duration::from_secs(5), "mokutil --sb-state").ok()?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        parse_mokutil_sb_state(&stdout)
    });
    let secure_boot_enabled = mok_sb_state.or(secure_boot.secure_boot_enabled);

    let mok_enrolled_count = mokutil_bin.and_then(|bin| {
        let mut cmd = command_c_locale(bin);
        cmd.arg("--list-enrolled");
        let out = run_command_with_timeout(cmd, Duration::from_secs(5), "mokutil --list-enrolled")
            .ok()?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.to_ascii_lowercase().contains("is empty")
            || stdout.to_ascii_lowercase().contains("is empty")
        {
            return Some(0);
        }
        if let Some(count) = parse_mokutil_enrolled_count(&stdout) {
            return Some(count);
        }
        if out.status.success() && stdout.trim().is_empty() {
            Some(0)
        } else {
            None
        }
    });

    let akmod_nvidia_installed = rpm_bin.is_some_and(|bin| {
        let mut cmd = command_c_locale(bin);
        cmd.args(["-q", "--", "akmod-nvidia"]);
        run_command_with_timeout(cmd, Duration::from_secs(5), "rpm -q akmod-nvidia")
            .map(|o| o.status.success())
            .unwrap_or(false)
    });

    let unsigned_risk = secure_boot_enabled == Some(true)
        && akmod_nvidia_installed
        && (!nvidia_module_loaded || mok_enrolled_count == Some(0));

    let (guidance_tr, guidance_en) = if unsigned_risk {
        (
            "Secure Boot etkin ancak akmod-nvidia modülü imzasız veya MOK kaydı eksik görünüyor. 'mokutil --import /etc/pki/akmods/certs/public_key.der' ile anahtarı içe aktarıp yeniden başlatın.".to_string(),
            "Secure Boot is enabled while akmod-nvidia appears unsigned or missing MOK enrollment. Run 'mokutil --import /etc/pki/akmods/certs/public_key.der' and reboot to enroll the MOK key.".to_string(),
        )
    } else if secure_boot_enabled == Some(true) {
        let mok_note_tr = mok_enrolled_count
            .map(|c| format!(" ({} kayıtlı MOK)", c))
            .unwrap_or_default();
        let mok_note_en = mok_enrolled_count
            .map(|c| format!(" ({} enrolled MOK key(s))", c))
            .unwrap_or_default();
        let nv_tr = if nvidia_module_loaded {
            "NVIDIA modülü yüklü"
        } else {
            "üçüncü parti NVIDIA modülü yüklü değil"
        };
        let nv_en = if nvidia_module_loaded {
            "NVIDIA module loaded"
        } else {
            "no third-party NVIDIA module loaded"
        };
        (
            format!("Secure Boot etkin{}; {}.", mok_note_tr, nv_tr),
            format!("Secure Boot enabled{}; {}.", mok_note_en, nv_en),
        )
    } else if secure_boot_enabled == Some(false) {
        (
            "Secure Boot kapalı; MOK imza zorlaması devre dışı.".to_string(),
            "Secure Boot is disabled; kernel module MOK enforcement is inactive.".to_string(),
        )
    } else {
        (
            "UEFI Secure Boot/MOK bilgisi kullanılamıyor.".to_string(),
            "UEFI Secure Boot/MOK state is unavailable.".to_string(),
        )
    };

    DriverMokSummary {
        secure_boot_enabled,
        mok_enrolled_count,
        nvidia_module_loaded,
        akmod_nvidia_installed,
        unsigned_risk,
        guidance_tr,
        guidance_en,
    }
}

pub fn inspect_system_rpm_integrity() -> RpmIntegrityStatus {
    let Some(rpm_bin) = resolve_rpm_binary() else {
        return RpmIntegrityStatus {
            total_packages: 0,
            signed_packages: 0,
            unsigned_packages: Vec::new(),
            verified_critical_packages: Vec::new(),
            verification_anomalies: Vec::new(),
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
        };
    };

    let qa_out = command_c_locale(&rpm_bin)
        .args([
            "-qa",
            "--qf",
            "%{NAME}|%{RSAHEADER:pgpsig}|%{OPENPGP:pgpsig}\n",
        ])
        .output();

    let Ok(qa) = qa_out else {
        return RpmIntegrityStatus {
            total_packages: 0,
            signed_packages: 0,
            unsigned_packages: Vec::new(),
            verified_critical_packages: Vec::new(),
            verification_anomalies: Vec::new(),
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
        };
    };

    if !qa.status.success() {
        return RpmIntegrityStatus {
            total_packages: 0,
            signed_packages: 0,
            unsigned_packages: Vec::new(),
            verified_critical_packages: Vec::new(),
            verification_anomalies: Vec::new(),
            severity: AkrepSeverity::Unavailable,
            availability: Availability::Unavailable,
        };
    }

    let stdout_str = String::from_utf8_lossy(&qa.stdout);
    let (total_packages, signed_packages, unsigned_packages) =
        parse_rpm_signature_lines(&stdout_str);

    let mut verified_critical_packages = Vec::new();
    let mut verification_anomalies = Vec::new();

    for &pkg in CRITICAL_SECURITY_PACKAGES {
        let q_ok = command_c_locale(&rpm_bin)
            .args(["-q", "--", pkg])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !q_ok {
            continue;
        }
        verified_critical_packages.push(pkg.to_string());
        if let Ok(v_out) = command_c_locale(&rpm_bin)
            .args(["-V", "--nodeps", "--noscripts", "--", pkg])
            .output()
        {
            if !v_out.status.success() {
                let v_stdout = String::from_utf8_lossy(&v_out.stdout);
                verification_anomalies.extend(parse_rpm_verify_anomalies(pkg, &v_stdout));
            }
        }
    }

    let severity = if !verification_anomalies.is_empty() {
        AkrepSeverity::Critical
    } else if !unsigned_packages.is_empty() {
        AkrepSeverity::Attention
    } else {
        AkrepSeverity::Protected
    };

    RpmIntegrityStatus {
        total_packages,
        signed_packages,
        unsigned_packages,
        verified_critical_packages,
        verification_anomalies,
        severity,
        availability: Availability::Available,
    }
}

pub struct LinuxDeviceSecurityOps {
    sys_root: PathBuf,
    proc_root: PathBuf,
    dev_root: PathBuf,
    etc_root: PathBuf,
    canary_path: PathBuf,
}

impl Default for LinuxDeviceSecurityOps {
    fn default() -> Self {
        Self::new_system()
    }
}

impl LinuxDeviceSecurityOps {
    pub fn new_system() -> Self {
        Self {
            sys_root: PathBuf::from("/sys"),
            proc_root: PathBuf::from("/proc"),
            dev_root: PathBuf::from("/dev"),
            etc_root: PathBuf::from("/etc"),
            canary_path: PathBuf::from(ROOT_CANARY_PATH),
        }
    }

    pub fn with_roots(
        sys_root: PathBuf,
        proc_root: PathBuf,
        dev_root: PathBuf,
        etc_root: PathBuf,
        canary_path: PathBuf,
    ) -> Self {
        Self {
            sys_root,
            proc_root,
            dev_root,
            etc_root,
            canary_path,
        }
    }
}

impl DeviceSecurityOps for LinuxDeviceSecurityOps {
    fn inspect(&self) -> Result<DeviceSecurityStatus, AkrepError> {
        let secure_boot = inspect_secure_boot_at(&self.sys_root);
        let tpm = inspect_tpm_at(&self.sys_root, &self.dev_root);
        let kernel_lockdown = inspect_kernel_lockdown_at(&self.proc_root, &self.sys_root);
        let runtime_integrity = inspect_runtime_integrity_at(&self.etc_root, &self.canary_path);
        let usb_audit = inspect_usb_devices_at(&self.sys_root);
        let rpm_integrity = inspect_system_rpm_integrity();
        let mokutil_bin = resolve_mokutil_binary();
        let rpm_bin = resolve_rpm_binary();
        let driver_mok = inspect_driver_mok_at(
            &self.sys_root,
            &secure_boot,
            mokutil_bin.as_deref(),
            rpm_bin.as_deref(),
        );

        let overall_severity = secure_boot
            .severity
            .combine(tpm.severity)
            .combine(kernel_lockdown.severity)
            .combine(runtime_integrity.severity)
            .combine(usb_audit.severity)
            .combine(rpm_integrity.severity);

        Ok(DeviceSecurityStatus {
            overall_severity,
            secure_boot,
            tpm,
            kernel_lockdown,
            runtime_integrity,
            usb_audit,
            rpm_integrity,
            inspected_at: Utc::now().to_rfc3339(),
            driver_mok: Some(driver_mok),
        })
    }
}

pub struct MockDeviceSecurityOps {
    status: Mutex<DeviceSecurityStatus>,
}

impl MockDeviceSecurityOps {
    pub fn new(status: DeviceSecurityStatus) -> Self {
        Self {
            status: Mutex::new(status),
        }
    }

    pub fn new_healthy_uefi() -> Self {
        let engine = SentinelEngine::new();
        let mut posture = evaluate_security_posture(&engine, false);
        posture.passed = posture.total;
        posture.integrity_ok = true;
        posture.verified = true;

        Self::new(DeviceSecurityStatus {
            overall_severity: AkrepSeverity::Protected,
            secure_boot: SecureBootStatus {
                uefi_present: true,
                secure_boot_enabled: Some(true),
                state: SecureBootState::Enabled,
                severity: AkrepSeverity::Protected,
                availability: Availability::Available,
                detail: "UEFI Secure Boot enabled (mock)".to_string(),
            },
            tpm: TpmStatus {
                present: true,
                version_major: Some("2".to_string()),
                tpm2_available: true,
                severity: AkrepSeverity::Protected,
                availability: Availability::Available,
                detail: "TPM 2.0 available (mock)".to_string(),
            },
            kernel_lockdown: KernelLockdownStatus {
                cmdline_module_sig_enforce: true,
                sysfs_module_sig_enforce: true,
                module_sig_enforced: true,
                lockdown_mode: "integrity".to_string(),
                iommu_active: true,
                severity: AkrepSeverity::Protected,
                detail: "module_sig_enforced=true, lockdown=integrity, iommu=true".to_string(),
            },
            runtime_integrity: RuntimeIntegrityStatus {
                posture,
                ld_so_preload_clean: true,
                ld_so_preload_detail: "Absent (/etc/ld.so.preload not present)".to_string(),
                root_canary_present: true,
                root_canary_intact: true,
                severity: AkrepSeverity::Protected,
            },
            usb_audit: UsbAuditReport {
                devices: Vec::new(),
                unauthorized_count: 0,
                anomaly_count: 0,
                severity: AkrepSeverity::Protected,
            },
            rpm_integrity: RpmIntegrityStatus {
                total_packages: 420,
                signed_packages: 420,
                unsigned_packages: Vec::new(),
                verified_critical_packages: vec![
                    "blaze-release".to_string(),
                    "sudo".to_string(),
                    "polkit".to_string(),
                ],
                verification_anomalies: Vec::new(),
                severity: AkrepSeverity::Protected,
                availability: Availability::Available,
            },
            inspected_at: Utc::now().to_rfc3339(),
            driver_mok: Some(DriverMokSummary {
                secure_boot_enabled: Some(true),
                mok_enrolled_count: Some(1),
                nvidia_module_loaded: false,
                akmod_nvidia_installed: false,
                unsigned_risk: false,
                guidance_tr: "Secure Boot etkin (1 kayıtlı MOK); üçüncü parti NVIDIA modülü yüklü değil.".to_string(),
                guidance_en: "Secure Boot enabled (1 enrolled MOK key(s)); no third-party NVIDIA module loaded.".to_string(),
            }),
        })
    }

    pub fn set_status(&self, status: DeviceSecurityStatus) {
        *self.status.lock().expect("mock device security mutex") = status;
    }
}

impl DeviceSecurityOps for MockDeviceSecurityOps {
    fn inspect(&self) -> Result<DeviceSecurityStatus, AkrepError> {
        Ok(self
            .status
            .lock()
            .expect("mock device security mutex")
            .clone())
    }
}

#[derive(Clone)]
pub struct DeviceSecurityAdapter {
    ops: Arc<dyn DeviceSecurityOps>,
}

impl DeviceSecurityAdapter {
    pub fn new(ops: Arc<dyn DeviceSecurityOps>) -> Self {
        Self { ops }
    }

    pub fn new_linux() -> Self {
        Self::new(Arc::new(LinuxDeviceSecurityOps::new_system()))
    }

    pub async fn inspect_device_security(&self) -> Result<DeviceSecurityStatus, AkrepError> {
        let ops = Arc::clone(&self.ops);
        tokio::task::spawn_blocking(move || ops.inspect())
            .await
            .map_err(|e| AkrepError::Internal(format!("device_security task join error: {}", e)))?
    }
}
