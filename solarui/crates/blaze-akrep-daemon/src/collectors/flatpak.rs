use blaze_sentinel_core::{
    AkrepCardSummary, AkrepFinding, AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, Availability,
};
use std::fs;
use std::path::Path;

pub struct FlatpakSection {
    pub card: AkrepCardSummary,
    pub detail: AkrepSectionDetail,
    pub findings: Vec<AkrepFinding>,
}

fn parse_context_permissions(raw: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut in_context = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_context = trimmed == "[Context]";
            continue;
        }
        if !in_context || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = trimmed.split_once('=') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

fn classify_elevated_tokens(perms: &[(String, String)], source_label: &str) -> Vec<String> {
    let mut alerts = Vec::new();
    for (k, v) in perms {
        for tok in v.split(';').filter(|s| !s.is_empty()) {
            let clean = tok.trim();
            if clean.starts_with('!') {
                continue;
            }
            if (k == "filesystems" && (clean == "host" || clean == "host-os" || clean == "home"))
                || (k == "devices" && clean == "all")
                || (k == "sockets"
                    && (clean == "x11" || clean == "system-bus" || clean == "session-bus"))
                || (k == "shared" && clean == "network")
            {
                alerts.push(format!("{}={} ({})", k, clean, source_label));
            }
        }
    }
    alerts
}

pub fn collect_flatpak() -> FlatpakSection {
    let app_dir = Path::new("/var/lib/flatpak/app");
    let override_dir = Path::new("/var/lib/flatpak/overrides");

    let mut apps = Vec::new();
    let mut findings = Vec::new();
    let mut metrics = Vec::new();
    let mut avail = Availability::Available;
    let mut status = AkrepSeverity::Normal;

    if app_dir.exists() {
        match fs::read_dir(app_dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let app_id = entry.file_name().to_string_lossy().to_string();
                    if app_id.starts_with('.') {
                        continue;
                    }
                    let meta_path = entry.path().join("current/active/metadata");
                    let mut elevated = Vec::new();
                    if let Ok(meta_raw) = fs::read_to_string(&meta_path) {
                        let perms = parse_context_permissions(&meta_raw);
                        elevated.extend(classify_elevated_tokens(&perms, "manifest"));
                    }
                    let ov_path = override_dir.join(&app_id);
                    if let Ok(ov_raw) = fs::read_to_string(&ov_path) {
                        let perms = parse_context_permissions(&ov_raw);
                        elevated.extend(classify_elevated_tokens(&perms, "system-override"));
                    }
                    apps.push((app_id, elevated));
                }
            }
            Err(_) => {
                avail = Availability::Unavailable;
                status = AkrepSeverity::Unknown;
            }
        }
    }

    let mut global_overrides = Vec::new();
    let global_ov_path = override_dir.join("global");
    if let Ok(raw) = fs::read_to_string(&global_ov_path) {
        let perms = parse_context_permissions(&raw);
        global_overrides = classify_elevated_tokens(&perms, "global-override");
    }

    if !global_overrides.is_empty() {
        status = status.combine(AkrepSeverity::Warning);
        findings.push(AkrepFinding {
            id: "flatpak.global_override".to_string(),
            section: "flatpak".to_string(),
            severity: AkrepSeverity::Warning,
            availability: Availability::Available,
            title_tr: "Genel Flatpak Sistem Override İzni".to_string(),
            title_en: "Global Flatpak System Override Detected".to_string(),
            description_tr: format!(
                "Tüm Flatpak uygulamalarına uygulanan yüksek izinler: {}",
                global_overrides.join(", ")
            ),
            description_en: format!(
                "Elevated global Flatpak overrides: {}",
                global_overrides.join(", ")
            ),
            remediation_hint_tr:
                "`flatpak override --show` ile kapsamı inceleyin (Akrep v0.1 izni değiştirmez)."
                    .to_string(),
            remediation_hint_en:
                "Inspect permissions using `flatpak override --show` (read-only in v0.1)."
                    .to_string(),
            source: "/var/lib/flatpak/overrides/global".to_string(),
            read_only_note: "Salt Okunur v0.1 — izin yazımı veya kısıtlama yapılmaz.".to_string(),
        });
    }

    let mut broad_host_apps = 0usize;
    for (app_id, elevated) in &apps {
        if elevated
            .iter()
            .any(|e| e.contains("filesystems=host") || e.contains("devices=all"))
        {
            broad_host_apps += 1;
            status = status.combine(AkrepSeverity::Warning);
            findings.push(AkrepFinding {
                id: format!("flatpak.broad.{}", app_id),
                section: "flatpak".to_string(),
                severity: AkrepSeverity::Warning,
                availability: Availability::Available,
                title_tr: format!("Geniş Sandbox İzni: {}", app_id),
                title_en: format!("Broad Sandbox Permission: {}", app_id),
                description_tr: format!("Algılanan yetkiler: {}", elevated.join(", ")),
                description_en: format!("Detected permissions: {}", elevated.join(", ")),
                remediation_hint_tr: format!(
                    "Gerekirse `flatpak info --show-permissions {}` ile doğrulayın.",
                    app_id
                ),
                remediation_hint_en: format!(
                    "Review with `flatpak info --show-permissions {}` if needed.",
                    app_id
                ),
                source: "collectors/flatpak".to_string(),
                read_only_note: "Salt Okunur v0.1 — override uygulanmaz.".to_string(),
            });
        }
    }

    metrics.push(AkrepKeyValue {
        label_tr: "Sistem Flatpak Uygulamaları".to_string(),
        label_en: "System Flatpak Apps".to_string(),
        value: apps.len().to_string(),
    });
    metrics.push(AkrepKeyValue {
        label_tr: "Geniş Dosya/Cihaz İzinli Uygulamalar".to_string(),
        label_en: "Broad Host/Device Access Apps".to_string(),
        value: broad_host_apps.to_string(),
    });
    metrics.push(AkrepKeyValue {
        label_tr: "Global Sistem Override".to_string(),
        label_en: "Global System Overrides".to_string(),
        value: if global_overrides.is_empty() {
            "Yok (Varsayılan sandbox)".to_string()
        } else {
            global_overrides.join(", ")
        },
    });
    metrics.push(AkrepKeyValue {
        label_tr: "Denetim Kaynağı".to_string(),
        label_en: "Inspection Scope".to_string(),
        value: "Sistem /var/lib/flatpak (Salt Okunur)".to_string(),
    });

    let summary_tr = format!(
        "{} uygulama • {} geniş yetkili",
        apps.len(),
        broad_host_apps
    );
    let summary_en = format!("{} app(s) • {} broad access", apps.len(), broad_host_apps);

    let card = AkrepCardSummary {
        id: "flatpak".to_string(),
        title_tr: "Uygulama İzinleri (Flatpak)".to_string(),
        title_en: "App Permissions (Flatpak)".to_string(),
        status,
        availability: avail,
        summary_tr: summary_tr.clone(),
        summary_en: summary_en.clone(),
        metric: apps.len().to_string(),
    };

    let detail = AkrepSectionDetail {
        section: "flatpak".to_string(),
        title_tr: card.title_tr.clone(),
        title_en: card.title_en.clone(),
        status,
        availability: avail,
        summary_tr,
        summary_en,
        metrics,
        findings: findings.clone(),
    };

    FlatpakSection {
        card,
        detail,
        findings,
    }
}
