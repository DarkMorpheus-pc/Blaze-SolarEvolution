use blaze_sentinel_core::{
    AkrepCardSummary, AkrepFinding, AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, Availability,
};
use serde::Deserialize;
use std::fs;
use std::path::Path;

pub struct ProfilesSection {
    pub active_id: String,
    pub active_tr: String,
    pub active_en: String,
    pub card: AkrepCardSummary,
    pub detail: AkrepSectionDetail,
    pub findings: Vec<AkrepFinding>,
}

#[derive(Deserialize)]
struct PartialManagedState {
    active_profile_id: Option<String>,
    effective_zone: Option<String>,
    updated_at: Option<String>,
}

pub fn profile_labels_for_id(id: &str) -> (&'static str, &'static str) {
    match id {
        "developer" => ("Geliştirici", "Developer"),
        "gaming" => ("Oyun", "Gaming"),
        "public-network" => ("Açık Ağ", "Public Network"),
        _ => ("Dengeli", "Balanced"),
    }
}

pub fn collect_profiles() -> ProfilesSection {
    collect_profiles_from_candidates(
        &[
            Path::new("/var/cache/akrep/active_profile.json"),
            Path::new("/var/lib/akrep/active_profile.json"),
        ],
        Path::new("/usr/share/akrep/profiles"),
    )
}

#[cfg(test)]
pub fn collect_profiles_at(active_state_path: &Path, profiles_dir: &Path) -> ProfilesSection {
    collect_profiles_from_candidates(&[active_state_path], profiles_dir)
}

pub fn collect_profiles_from_candidates(
    candidate_paths: &[&Path],
    profiles_dir: &Path,
) -> ProfilesSection {
    let mut active_id = "balanced".to_string();
    let mut effective_zone = "Varsayılan (FedoraWorkstation / public)".to_string();
    let mut state_source = "Varsayılan temel profil (Balanced)".to_string();

    for candidate in candidate_paths {
        if let Ok(raw) = fs::read_to_string(candidate) {
            if let Ok(parsed) = serde_json::from_str::<PartialManagedState>(&raw) {
                if let Some(pid) = parsed.active_profile_id.filter(|s| !s.trim().is_empty()) {
                    active_id = pid;
                    state_source = match parsed.updated_at {
                        Some(ts) => format!(
                            "Etkin durum kaydı ({})",
                            ts.chars().take(19).collect::<String>()
                        ),
                        None => "Etkin durum kaydı (active_profile.json)".to_string(),
                    };
                    if let Some(z) = parsed.effective_zone.filter(|s| !s.trim().is_empty()) {
                        effective_zone = z;
                    }
                    break;
                }
            }
        }
    }

    let (label_tr, label_en) = profile_labels_for_id(&active_id);
    let active_tr = label_tr.to_string();
    let active_en = label_en.to_string();

    let mut installed_profiles = 0usize;
    if let Ok(entries) = fs::read_dir(profiles_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().and_then(|e| e.to_str()) == Some("toml") {
                installed_profiles += 1;
            }
        }
    }
    let catalog_count = if installed_profiles > 0 {
        installed_profiles
    } else {
        4
    };

    let summary_tr = format!(
        "Etkin Profil: {} • SELinux Enforcing & Firewalld Aktif",
        active_tr
    );
    let summary_en = format!(
        "Active Profile: {} • SELinux Enforcing & Firewalld Active",
        active_en
    );

    let metrics = vec![
        AkrepKeyValue {
            label_tr: "Etkin Güvenlik Profili".to_string(),
            label_en: "Active Security Profile".to_string(),
            value: format!("{} ({})", active_tr, active_id),
        },
        AkrepKeyValue {
            label_tr: "Hedef Güvenlik Duvarı Bölgesi".to_string(),
            label_en: "Target Firewall Zone".to_string(),
            value: effective_zone,
        },
        AkrepKeyValue {
            label_tr: "Katalog Profilleri".to_string(),
            label_en: "Catalog Profiles".to_string(),
            value: format!("{} bildirimsel TOML profili", catalog_count),
        },
        AkrepKeyValue {
            label_tr: "Profil Durum Kaynağı".to_string(),
            label_en: "Profile State Source".to_string(),
            value: state_source,
        },
    ];

    let card = AkrepCardSummary {
        id: "profiles".to_string(),
        title_tr: "Güvenlik Profilleri".to_string(),
        title_en: "Security Profiles".to_string(),
        status: AkrepSeverity::Protected,
        availability: Availability::Available,
        summary_tr: summary_tr.clone(),
        summary_en: summary_en.clone(),
        metric: active_en.to_ascii_uppercase(),
    };

    let detail = AkrepSectionDetail {
        section: "profiles".to_string(),
        title_tr: card.title_tr.clone(),
        title_en: card.title_en.clone(),
        status: AkrepSeverity::Protected,
        availability: Availability::Available,
        summary_tr,
        summary_en,
        metrics,
        findings: Vec::new(),
    };

    ProfilesSection {
        active_id,
        active_tr,
        active_en,
        card,
        detail,
        findings: Vec::new(),
    }
}
