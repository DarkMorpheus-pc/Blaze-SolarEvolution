use blaze_sentinel_core::{
    AkrepCardSummary, AkrepFinding, AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, Availability,
    PostureReport,
};
use std::fs;

use crate::collectors::secureboot::collect_secureboot_and_tpm;

pub struct DeviceSecuritySection {
    pub card: AkrepCardSummary,
    pub detail: AkrepSectionDetail,
    pub findings: Vec<AkrepFinding>,
}

fn read_lockdown_state() -> String {
    let Ok(raw) = fs::read_to_string("/sys/kernel/security/lockdown") else {
        return "Bilinmiyor".to_string();
    };
    if raw.contains("[confidentiality]") {
        "confidentiality".to_string()
    } else if raw.contains("[integrity]") {
        "integrity".to_string()
    } else if raw.contains("[none]") {
        "none (devre dışı)".to_string()
    } else {
        raw.trim().to_string()
    }
}

pub fn collect_device_and_kernel(posture: &PostureReport) -> DeviceSecuritySection {
    let sb = collect_secureboot_and_tpm();
    let lockdown = read_lockdown_state();
    let bpf_unpriv = fs::read_to_string("/proc/sys/kernel/unprivileged_bpf_disabled")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "bilinmiyor".to_string());

    let sysctl_controls: Vec<_> = posture
        .controls
        .iter()
        .filter(|c| c.id != "mac" && c.id != "firewall")
        .collect();
    let sysctl_passed = sysctl_controls.iter().filter(|c| c.passed).count();
    let sysctl_total = sysctl_controls.len();

    let mut status = if sysctl_passed == sysctl_total {
        AkrepSeverity::Normal
    } else {
        AkrepSeverity::Warning
    };

    let mut findings = Vec::new();
    if sb.uefi_boot && sb.secure_boot_enabled == Some(false) {
        status = status.combine(AkrepSeverity::Warning);
        findings.push(AkrepFinding {
            id: "device.secure_boot_off".to_string(),
            section: "device_security".to_string(),
            severity: AkrepSeverity::Warning,
            availability: Availability::Available,
            title_tr: "UEFI Secure Boot Devre Dışı".to_string(),
            title_en: "UEFI Secure Boot Disabled".to_string(),
            description_tr: "Önyükleme zinciri imzasız çekirdek/modüllere karşı donanım doğrulaması uygulamıyor.".to_string(),
            description_en: "Boot firmware does not currently enforce UEFI Secure Boot signature verification.".to_string(),
            remediation_hint_tr: "Üçüncü taraf imzasız sürücü gerekmiyorsa UEFI ayarlarından Secure Boot etkinleştirilebilir.".to_string(),
            remediation_hint_en: "Enable Secure Boot in firmware settings when signed kernel/drivers are used.".to_string(),
            source: "collectors/secureboot".to_string(),
            read_only_note: "Salt Okunur v0.1 — UEFI değişkenleri değiştirilmez.".to_string(),
        });
    }

    let mut metrics = sb.metrics;
    metrics.push(AkrepKeyValue {
        label_tr: "Kernel Lockdown".to_string(),
        label_en: "Kernel Lockdown".to_string(),
        value: lockdown,
    });
    metrics.push(AkrepKeyValue {
        label_tr: "Sysctl Sertleştirme Puanı".to_string(),
        label_en: "Kernel Sysctl Baseline".to_string(),
        value: format!("{}/{} aktif", sysctl_passed, sysctl_total),
    });
    metrics.push(AkrepKeyValue {
        label_tr: "unprivileged_bpf_disabled".to_string(),
        label_en: "unprivileged_bpf_disabled".to_string(),
        value: bpf_unpriv,
    });
    for c in sysctl_controls {
        metrics.push(AkrepKeyValue {
            label_tr: c.name.clone(),
            label_en: c.title.clone(),
            value: c.detail.clone(),
        });
    }

    let summary_tr = format!(
        "Sysctl {}/{} • {} • {}",
        sysctl_passed,
        sysctl_total,
        if sb.uefi_boot {
            match sb.secure_boot_enabled {
                Some(true) => "Secure Boot Açık",
                Some(false) => "Secure Boot Kapalı",
                None => "Secure Boot Bilinmiyor",
            }
        } else {
            "Legacy BIOS"
        },
        sb.tpm_version_tr
    );
    let summary_en = format!("Sysctl {}/{} • {}", sysctl_passed, sysctl_total, sb.tpm_version_en);

    let card = AkrepCardSummary {
        id: "device_security".to_string(),
        title_tr: "Cihaz Güvenliği (Kernel & Donanım)".to_string(),
        title_en: "Device Security (Kernel & Hardware)".to_string(),
        status,
        availability: Availability::Available,
        summary_tr: summary_tr.clone(),
        summary_en: summary_en.clone(),
        metric: format!("{}/{}", sysctl_passed, sysctl_total),
    };

    let detail = AkrepSectionDetail {
        section: "device_security".to_string(),
        title_tr: card.title_tr.clone(),
        title_en: card.title_en.clone(),
        status,
        availability: Availability::Available,
        summary_tr,
        summary_en,
        metrics,
        findings: findings.clone(),
    };

    DeviceSecuritySection { card, detail, findings }
}
