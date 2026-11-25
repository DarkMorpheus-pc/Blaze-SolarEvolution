pub mod firewalld;
pub mod flatpak;
pub mod history;
pub mod kernel;
pub mod packages;
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

pub async fn collect_snapshot(dbus_conn: Option<&Connection>) -> AkrepSnapshot {
    // Run CPU/blocking procfs & sysfs inspection off the tokio reactor cleanly
    let (
        posture,
        selinux_sec,
        flatpak_sec,
        packages_sec,
        history_sec,
    ) = tokio::task::spawn_blocking(|| {
        let engine = SentinelEngine::new();
        let posture = evaluate_security_posture(&engine, false);
        let selinux_sec = selinux::collect_selinux();
        let flatpak_sec = flatpak::collect_flatpak();
        let packages_sec = packages::collect_packages();
        let history_sec = history::collect_history();
        (posture, selinux_sec, flatpak_sec, packages_sec, history_sec)
    })
    .await
    .expect("akrep collector blocking task should never panic");

    let fw_sec = firewalld::collect_firewalld(dbus_conn).await;
    let dev_sec = kernel::collect_device_and_kernel(&posture);

    // Threat & Integrity card built directly from shared blaze-sentinel-core posture
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

    let threat_status = if posture.integrity_ok {
        AkrepSeverity::Normal
    } else {
        AkrepSeverity::Critical
    };

    let threat_card = AkrepCardSummary {
        id: "threat_integrity".to_string(),
        title_tr: "Tehdit Koruması & Bütünlük".to_string(),
        title_en: "Threat & Runtime Integrity".to_string(),
        status: threat_status,
        availability: Availability::Available,
        summary_tr: if posture.integrity_ok {
            format!("{} • Sentinel v{} temiz", preload_state, SENTINEL_VERSION)
        } else {
            format!("{} çalışma zamanı bütünlük alarmı!", posture.alerts.len())
        },
        summary_en: if posture.integrity_ok {
            format!("Sentinel v{} integrity verified ({}/{})", SENTINEL_VERSION, posture.passed, posture.total)
        } else {
            format!("{} runtime integrity alert(s) detected", posture.alerts.len())
        },
        metric: if posture.integrity_ok { "TEMİZ".to_string() } else { "ALARM".to_string() },
    };

    let mut all_findings = findings_from_posture(&posture);
    all_findings.extend(selinux_sec.findings);
    all_findings.extend(fw_sec.findings);
    all_findings.extend(flatpak_sec.findings);
    all_findings.extend(dev_sec.findings);
    all_findings.extend(packages_sec.findings);
    all_findings.extend(history_sec.findings);

    let threat_findings: Vec<_> = all_findings
        .iter()
        .filter(|f| f.section == "threat_integrity")
        .cloned()
        .collect();

    let threat_detail = AkrepSectionDetail {
        section: "threat_integrity".to_string(),
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
                value: format!("v{} ({}/{} kontrol aktif)", SENTINEL_VERSION, posture.passed, posture.total),
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
                value: "Akrep v0.1 kapsamında devre dışı (Salt Okunur teşhis)".to_string(),
            },
        ],
        findings: threat_findings,
    };

    let cards = vec![
        selinux_sec.card,
        fw_sec.card,
        flatpak_sec.card,
        dev_sec.card,
        threat_card,
        packages_sec.card,
        history_sec.card,
    ];

    let sections = vec![
        selinux_sec.detail,
        fw_sec.detail,
        flatpak_sec.detail,
        dev_sec.detail,
        threat_detail,
        packages_sec.detail,
        history_sec.detail,
    ];

    let overall_status = cards
        .iter()
        .fold(AkrepSeverity::Normal, |acc, c| acc.combine(c.status));

    let (headline_tr, headline_en) = match overall_status {
        AkrepSeverity::Normal => (
            "Sistem Güvenlik Duruşu Sağlıklı (Salt Okunur Denetim)".to_string(),
            "System Security Posture Healthy (Read-Only Telemetry)".to_string(),
        ),
        AkrepSeverity::Warning | AkrepSeverity::Unknown | AkrepSeverity::Unavailable => (
            "Bazı Alanlar İnceleme Gerektiriyor".to_string(),
            "Some Security Areas Need Review".to_string(),
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
        mutators_enabled: false,
        sections: sections.iter().map(|s| s.section.clone()).collect(),
        future_polkit_actions: vec!["tr.com.blazeos.Akrep1.manage".to_string()],
    };

    AkrepSnapshot {
        schema_version: AKREP_SCHEMA_VERSION,
        version: AKREP_VERSION.to_string(),
        sentinel_version: SENTINEL_VERSION.to_string(),
        read_only: true,
        mutators_enabled: false,
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
