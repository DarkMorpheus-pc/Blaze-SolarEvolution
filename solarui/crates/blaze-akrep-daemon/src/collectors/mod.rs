pub mod firewalld;
pub mod flatpak;
pub mod history;
pub mod kernel;
pub mod packages;
pub mod privacy;
pub mod profiles;
pub mod secureboot;
pub mod selinux;

use blaze_sentinel_core::{
    evaluate_security_posture, findings_from_posture, AkrepCapabilities, AkrepCardSummary,
    AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, AkrepSnapshot, Availability, SentinelEngine,
    AKREP_SCHEMA_VERSION, AKREP_VERSION, SENTINEL_VERSION,
};
use chrono::Utc;
use std::fs;
use std::path::Path;
use zbus::Connection;

pub const PRIVILEGED_BUS_NAME: &str = "tr.com.blazeos.Akrep1.Privileged";

pub fn default_polkit_actions() -> Vec<String> {
    vec![
        "tr.com.blazeos.Akrep1.selinux.diagnose".to_string(),
        "tr.com.blazeos.Akrep1.selinux.manage".to_string(),
        "tr.com.blazeos.Akrep1.firewall.manage".to_string(),
        "tr.com.blazeos.Akrep1.flatpak.system-manage".to_string(),
        "tr.com.blazeos.Akrep1.quarantine.manage".to_string(),
        "tr.com.blazeos.Akrep1.profile.apply".to_string(),
        "tr.com.blazeos.Akrep1.packages.install-optional".to_string(),
    ]
}

pub async fn collect_snapshot(dbus_conn: Option<&Connection>) -> AkrepSnapshot {
    // Run CPU/blocking procfs & sysfs inspection off the tokio reactor cleanly
    let (posture, selinux_sec, flatpak_sec, packages_sec, privacy_sec, profiles_sec, history_sec) =
        tokio::task::spawn_blocking(|| {
            let engine = SentinelEngine::new();
            let posture = evaluate_security_posture(&engine, false);
            let selinux_sec = selinux::collect_selinux();
            let flatpak_sec = flatpak::collect_flatpak();
            let packages_sec = packages::collect_packages();
            let privacy_sec = privacy::collect_privacy();
            let profiles_sec = profiles::collect_profiles();
            let history_sec = history::collect_history();
            (
                posture,
                selinux_sec,
                flatpak_sec,
                packages_sec,
                privacy_sec,
                profiles_sec,
                history_sec,
            )
        })
        .await
        .expect("akrep collector blocking task should never panic");

    let fw_sec = firewalld::collect_firewalld(dbus_conn).await;
    let dev_sec = kernel::collect_device_and_kernel(&posture, &packages_sec);

    let preload_state = if Path::new("/etc/ld.so.preload").exists() {
        let raw = fs::read_to_string("/etc/ld.so.preload").unwrap_or_default();
        if raw.trim().is_empty() || raw.contains("libsolar_brand.so") {
            "Doğrulandı (yalnızca libsolar_brand.so)".to_string()
        } else {
            "UYARI: Beklenmeyen ld.so.preload kaydı!".to_string()
        }
    } else {
        "Temiz (/etc/ld.so.preload yok)".to_string()
    };

    let clamav_socket_ready = Path::new("/run/clamd.scan/clamd.sock").exists()
        || Path::new("/var/run/clamd.scan/clamd.sock").exists();
    let clamav_cli_ready = Path::new("/usr/bin/clamscan").is_file()
        && (Path::new("/var/lib/clamav/daily.cld").is_file()
            || Path::new("/var/lib/clamav/daily.cvd").is_file()
            || Path::new("/var/lib/clamav/main.cld").is_file()
            || Path::new("/var/lib/clamav/main.cvd").is_file());
    let clamav_ready = clamav_socket_ready || clamav_cli_ready;

    let threat_status = if !posture.integrity_ok {
        AkrepSeverity::Critical
    } else {
        AkrepSeverity::Protected
    };

    let threat_card = AkrepCardSummary {
        id: "threat_protection".to_string(),
        title_tr: "Tehdit Koruması".to_string(),
        title_en: "Threat Protection".to_string(),
        status: threat_status,
        availability: Availability::Available,
        summary_tr: if posture.integrity_ok {
            format!(
                "{} • Sentinel v{} temiz • AV: {}",
                preload_state,
                SENTINEL_VERSION,
                if clamav_ready {
                    "Hazır"
                } else {
                    "İsteğe Bağlı Motor"
                }
            )
        } else {
            format!("{} çalışma zamanı bütünlük alarmı!", posture.alerts.len())
        },
        summary_en: if posture.integrity_ok {
            format!(
                "Sentinel v{} integrity verified ({}/{}) • AV: {}",
                SENTINEL_VERSION,
                posture.passed,
                posture.total,
                if clamav_ready {
                    "Ready"
                } else {
                    "Optional Engine"
                }
            )
        } else {
            format!(
                "{} runtime integrity alert(s) detected",
                posture.alerts.len()
            )
        },
        metric: if posture.integrity_ok {
            "PROTECTED".to_string()
        } else {
            "CRITICAL".to_string()
        },
    };

    let mut all_findings = findings_from_posture(&posture);
    all_findings.extend(selinux_sec.findings);
    all_findings.extend(fw_sec.findings);
    all_findings.extend(flatpak_sec.findings);
    all_findings.extend(dev_sec.findings);
    all_findings.extend(privacy_sec.findings);
    all_findings.extend(profiles_sec.findings);
    all_findings.extend(history_sec.findings);

    let threat_findings: Vec<_> = all_findings
        .iter()
        .filter(|f| f.section == "threat_protection" || f.section == "threat_integrity")
        .cloned()
        .collect();

    let threat_detail = AkrepSectionDetail {
        section: "threat_protection".to_string(),
        title_tr: threat_card.title_tr.clone(),
        title_en: threat_card.title_en.clone(),
        status: threat_status,
        availability: Availability::Available,
        summary_tr: threat_card.summary_tr.clone(),
        summary_en: threat_card.summary_en.clone(),
        metrics: vec![
            AkrepKeyValue {
                label_tr: "Sentinel Çekirdek Motoru".to_string(),
                label_en: "Sentinel Core Engine".to_string(),
                value: format!(
                    "v{} ({}/{} kontrol aktif)",
                    SENTINEL_VERSION, posture.passed, posture.total
                ),
            },
            AkrepKeyValue {
                label_tr: "/etc/ld.so.preload Durumu".to_string(),
                label_en: "/etc/ld.so.preload State".to_string(),
                value: preload_state,
            },
            AkrepKeyValue {
                label_tr: "Root Kanarisi Politikası".to_string(),
                label_en: "Root Canary Policy".to_string(),
                value: "blaze-sentinel.service (0600 root koruma)".to_string(),
            },
            AkrepKeyValue {
                label_tr: "İsteğe Bağlı AV Tarama Motoru".to_string(),
                label_en: "AV Scanning Engine".to_string(),
                value: if clamav_socket_ready {
                    "ClamAV Unix Soketi Hazır (Talep Üzerine Tarama)".to_string()
                } else if clamav_cli_ready {
                    "ClamAV CLI Hazır (Talep Üzerine Tarama)".to_string()
                } else {
                    "Kullanılamıyor (İsteğe bağlı: clamav, clamd, clamav-freshclam)".to_string()
                },
            },
        ],
        findings: threat_findings,
    };

    let cards = vec![
        threat_card,
        selinux_sec.card,
        fw_sec.card,
        flatpak_sec.card,
        dev_sec.card,
        privacy_sec.card,
        profiles_sec.card,
        history_sec.card,
    ];

    let sections = vec![
        threat_detail,
        selinux_sec.detail,
        fw_sec.detail,
        flatpak_sec.detail,
        dev_sec.detail,
        privacy_sec.detail,
        profiles_sec.detail,
        history_sec.detail,
    ];

    let overall_status = cards
        .iter()
        .fold(AkrepSeverity::Protected, |acc, c| acc.combine(c.status));

    let (headline_tr, headline_en) = match overall_status {
        AkrepSeverity::Protected => (
            "Sistem Güvenlik Duruşu Korunuyor".to_string(),
            "System Security Posture Protected".to_string(),
        ),
        AkrepSeverity::Attention => (
            "Bazı Güvenlik Kontrolleri İnceleme Gerektiriyor".to_string(),
            "Some Security Controls Require Attention".to_string(),
        ),
        AkrepSeverity::Unavailable => (
            "Temel Kontroller Aktif — Bazı İsteğe Bağlı Bileşenler Kullanılamıyor".to_string(),
            "Core Protections Active — Some Optional Components Unavailable".to_string(),
        ),
        AkrepSeverity::Unknown => (
            "Bazı Güvenlik Telemetri Alanları Doğrulanamadı".to_string(),
            "Some Security Telemetry Domains Are Unknown".to_string(),
        ),
        AkrepSeverity::Critical => (
            "Kritik Güvenlik Uyarısı Tespit Edildi".to_string(),
            "Critical Security Alert Detected".to_string(),
        ),
    };

    let capabilities = AkrepCapabilities {
        version: AKREP_VERSION.to_string(),
        sentinel_version: SENTINEL_VERSION.to_string(),
        schema_version: AKREP_SCHEMA_VERSION,
        read_only: true,
        read_only_daemon: true,
        mutators_enabled: true,
        privileged_bus: PRIVILEGED_BUS_NAME.to_string(),
        sections: sections.iter().map(|s| s.section.clone()).collect(),
        future_polkit_actions: default_polkit_actions(),
    };

    AkrepSnapshot {
        schema_version: AKREP_SCHEMA_VERSION,
        version: AKREP_VERSION.to_string(),
        sentinel_version: SENTINEL_VERSION.to_string(),
        read_only: true,
        read_only_daemon: true,
        mutators_enabled: true,
        active_profile_id: profiles_sec.active_id,
        active_profile_tr: profiles_sec.active_tr,
        active_profile_en: profiles_sec.active_en,
        generated_at: Utc::now().to_rfc3339(),
        overall_status,
        headline_tr,
        headline_en,
        posture,
        cards,
        findings: all_findings,
        sections,
        capabilities,
    }
}
