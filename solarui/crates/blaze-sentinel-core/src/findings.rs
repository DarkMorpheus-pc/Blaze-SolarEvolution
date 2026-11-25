use crate::models::{AkrepFinding, PostureReport};
use crate::severity::{AkrepSeverity, Availability};

pub fn findings_from_posture(report: &PostureReport) -> Vec<AkrepFinding> {
    let mut findings = Vec::new();

    for ctrl in &report.controls {
        if ctrl.passed {
            continue;
        }
        let section = match ctrl.id.as_str() {
            "mac" => "selinux",
            "firewall" => "firewall",
            _ => "device_security",
        };
        let severity = if ctrl.critical {
            AkrepSeverity::Critical
        } else {
            AkrepSeverity::Attention
        };
        let (remedy_tr, remedy_en) = match ctrl.id.as_str() {
            "mac" => (
                "SELinux Enforcing durumu yalnızca yönetici tarafından /etc/selinux/config üzerinden etkinleştirilebilir (Akrep v0.1 salt okunurdur).",
                "SELinux Enforcing mode requires administrator review via /etc/selinux/config (Akrep v0.1 is read-only).",
            ),
            "firewall" => (
                "Güvenlik duvarı durumunu doğrulamak için `systemctl status firewalld` çıktısını inceleyin.",
                "Inspect `systemctl status firewalld` as an administrator to verify firewall protection.",
            ),
            _ => (
                "/etc/sysctl.d/99-blaze-optimization.conf sertleştirme parametrelerini kontrol edin.",
                "Verify kernel sysctl hardening settings in /etc/sysctl.d/99-blaze-optimization.conf.",
            ),
        };

        findings.push(AkrepFinding {
            id: format!("sentinel.{}", ctrl.id),
            section: section.to_string(),
            severity,
            availability: Availability::Available,
            title_tr: format!("Kontrol Uyarısı: {}", ctrl.name),
            title_en: format!("Control Alert: {}", ctrl.title),
            description_tr: ctrl.detail.clone(),
            description_en: ctrl.detail.clone(),
            remediation_hint_tr: remedy_tr.to_string(),
            remediation_hint_en: remedy_en.to_string(),
            source: "blaze-sentinel-core".to_string(),
            read_only_note: "Akrep v0.1 yalnızca teşhis sağlar; sistem ayarı değiştirmez."
                .to_string(),
        });
    }

    for (idx, alert) in report.alerts.iter().enumerate() {
        findings.push(AkrepFinding {
            id: format!("sentinel.integrity.{}", idx),
            section: "threat_integrity".to_string(),
            severity: AkrepSeverity::Critical,
            availability: Availability::Available,
            title_tr: "Çalışma Zamanı Bütünlük Uyarısı".to_string(),
            title_en: "Runtime Integrity Alert".to_string(),
            description_tr: alert.clone(),
            description_en: alert.clone(),
            remediation_hint_tr:
                "BERP kurtarma konsolunu ve /var/log/blaze-sentinel.log kayıtlarını inceleyin."
                    .to_string(),
            remediation_hint_en:
                "Inspect BERP recovery logs and /var/log/blaze-sentinel.log as administrator."
                    .to_string(),
            source: "blaze-sentinel-core".to_string(),
            read_only_note: "Akrep v0.1 yalnızca teşhis sağlar; müdahale etmez.".to_string(),
        });
    }

    findings
}
