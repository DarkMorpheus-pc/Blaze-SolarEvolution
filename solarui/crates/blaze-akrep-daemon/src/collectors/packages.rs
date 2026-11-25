use blaze_sentinel_core::{
    AkrepCardSummary, AkrepFinding, AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, Availability,
};
use std::collections::HashMap;
use std::fs;
use std::process::Command;

pub struct PackagesSection {
    pub card: AkrepCardSummary,
    pub detail: AkrepSectionDetail,
    pub findings: Vec<AkrepFinding>,
}

fn parse_env_kv(path: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(raw) = fs::read_to_string(path) else {
        return map;
    };
    for line in raw.lines() {
        let l = line.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = l.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
        }
    }
    map
}

pub fn collect_packages() -> PackagesSection {
    let osr = parse_env_kv("/etc/os-release");
    let pretty = osr
        .get("PRETTY_NAME")
        .cloned()
        .unwrap_or_else(|| "Bilinmiyor".to_string());
    let base_id = osr.get("BLAZE_BASE_ID").cloned().unwrap_or_else(|| {
        osr.get("ID_LIKE")
            .cloned()
            .unwrap_or_else(|| "unknown".to_string())
    });
    let base_ver = osr
        .get("BLAZE_BASE_VERSION_ID")
        .cloned()
        .unwrap_or_else(|| "unknown".to_string());

    let repo_raw = fs::read_to_string("/usr/share/dnf5/repos.d/blaze.repo")
        .or_else(|_| fs::read_to_string("/etc/yum.repos.d/blaze.repo"))
        .ok();
    let (gpg_ok, repo_state) = match &repo_raw {
        Some(r) => {
            let pkg_gpg = r.contains("gpgcheck=1");
            let meta_gpg = r.contains("repo_gpgcheck=1");
            if pkg_gpg && meta_gpg {
                (true, "gpgcheck=1 + repo_gpgcheck=1 aktif".to_string())
            } else {
                (
                    false,
                    format!("gpgcheck={} repo_gpgcheck={}", pkg_gpg, meta_gpg),
                )
            }
        }
        None => (false, "Blaze DNF5 repo tanımı bulunamadı".to_string()),
    };

    let blaze_rel_rpm = Command::new("/bin/rpm")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .args([
            "-q",
            "--qf",
            "%{NAME}-%{VERSION}-%{RELEASE}",
            "blaze-release",
        ])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| "kurulu değil / okunamadı".to_string());

    let channel_cfg = parse_env_kv("/etc/blazeos/update-channel.conf");
    let channel = channel_cfg
        .get("CHANNEL")
        .cloned()
        .unwrap_or_else(|| "stable".to_string());

    let mut findings = Vec::new();
    let status = if gpg_ok {
        AkrepSeverity::Normal
    } else {
        findings.push(AkrepFinding {
            id: "packages.repo_gpg".to_string(),
            section: "packages".to_string(),
            severity: AkrepSeverity::Warning,
            availability: Availability::Available,
            title_tr: "Blaze DNF5 Depo İmza Doğrulaması Eksik".to_string(),
            title_en: "Blaze DNF5 Repository GPG Check Incomplete".to_string(),
            description_tr: repo_state.clone(),
            description_en: repo_state.clone(),
            remediation_hint_tr: "/usr/share/dnf5/repos.d/blaze.repo dosyasında gpgcheck=1 ve repo_gpgcheck=1 ayarlarını doğrulayın.".to_string(),
            remediation_hint_en: "Verify gpgcheck=1 and repo_gpgcheck=1 in /usr/share/dnf5/repos.d/blaze.repo.".to_string(),
            source: "collectors/packages".to_string(),
            read_only_note: "Salt Okunur v0.1 — dnf işlemi başlatılmaz.".to_string(),
        });
        AkrepSeverity::Warning
    };

    let metrics = vec![
        AkrepKeyValue {
            label_tr: "İşletim Sistemi Kimliği".to_string(),
            label_en: "OS Identity".to_string(),
            value: pretty.clone(),
        },
        AkrepKeyValue {
            label_tr: "Üst Akış Tabanı".to_string(),
            label_en: "Upstream Base".to_string(),
            value: format!("{}:{}", base_id, base_ver),
        },
        AkrepKeyValue {
            label_tr: "DNF5 İmza Politikası".to_string(),
            label_en: "DNF5 GPG Policy".to_string(),
            value: repo_state,
        },
        AkrepKeyValue {
            label_tr: "blaze-release RPM".to_string(),
            label_en: "blaze-release RPM".to_string(),
            value: blaze_rel_rpm,
        },
        AkrepKeyValue {
            label_tr: "Güncelleme Kanalı".to_string(),
            label_en: "Update Channel".to_string(),
            value: channel,
        },
    ];

    let card = AkrepCardSummary {
        id: "packages".to_string(),
        title_tr: "Paket & Tedarik Zinciri Bütünlüğü".to_string(),
        title_en: "Supply Chain & Package Integrity".to_string(),
        status,
        availability: Availability::Available,
        summary_tr: format!(
            "{} ({}:{}) • GPG {}",
            pretty,
            base_id,
            base_ver,
            if gpg_ok { "Aktif" } else { "Uyarı" }
        ),
        summary_en: format!(
            "{} ({}:{}) • GPG {}",
            pretty,
            base_id,
            base_ver,
            if gpg_ok { "Active" } else { "Warn" }
        ),
        metric: if gpg_ok {
            "GPG".to_string()
        } else {
            "WARN".to_string()
        },
    };

    let detail = AkrepSectionDetail {
        section: "packages".to_string(),
        title_tr: card.title_tr.clone(),
        title_en: card.title_en.clone(),
        status,
        availability: Availability::Available,
        summary_tr: card.summary_tr.clone(),
        summary_en: card.summary_en.clone(),
        metrics,
        findings: findings.clone(),
    };

    PackagesSection {
        card,
        detail,
        findings,
    }
}
