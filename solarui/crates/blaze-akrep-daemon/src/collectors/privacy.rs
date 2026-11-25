use blaze_sentinel_core::{
    AkrepCardSummary, AkrepFinding, AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, Availability,
};
use std::path::Path;

pub struct PrivacySection {
    pub card: AkrepCardSummary,
    pub detail: AkrepSectionDetail,
    pub findings: Vec<AkrepFinding>,
}

pub fn collect_privacy() -> PrivacySection {
    let pw_dump_installed = Path::new("/usr/bin/pw-dump").is_file();
    let pw_cli_installed = Path::new("/usr/bin/pw-cli").is_file();
    let portal_installed = Path::new("/usr/libexec/xdg-desktop-portal").is_file()
        || Path::new("/usr/lib/systemd/user/xdg-desktop-portal.service").is_file();

    let (status, avail, summary_tr, summary_en, metric) = if portal_installed && pw_dump_installed {
        (
            AkrepSeverity::Protected,
            Availability::Available,
            "Wayland Portal onayı ve PipeWire akış denetimi hazır".to_string(),
            "Wayland Portal consent and PipeWire stream auditing ready".to_string(),
            "PORTAL".to_string(),
        )
    } else if portal_installed || pw_dump_installed {
        (
            AkrepSeverity::Unavailable,
            Availability::Unavailable,
            "Kısmi masaüstü gizlilik altyapısı (PipeWire / XDG Portal kontrol edin)".to_string(),
            "Partial desktop privacy stack (verify PipeWire / XDG Portal)".to_string(),
            "PARTIAL".to_string(),
        )
    } else {
        (
            AkrepSeverity::Unavailable,
            Availability::Unavailable,
            "XDG Desktop Portal veya PipeWire araçları kurulu değil".to_string(),
            "XDG Desktop Portal or PipeWire tools not installed".to_string(),
            "UNAVAIL".to_string(),
        )
    };

    let metrics = vec![
        AkrepKeyValue {
            label_tr: "XDG Masaüstü Portal Altyapısı".to_string(),
            label_en: "XDG Desktop Portal Backend".to_string(),
            value: if portal_installed {
                "Kurulu (xdg-desktop-portal)".to_string()
            } else {
                "Kullanılamıyor".to_string()
            },
        },
        AkrepKeyValue {
            label_tr: "PipeWire Medya Denetimi".to_string(),
            label_en: "PipeWire Graph Inspector".to_string(),
            value: if pw_dump_installed && pw_cli_installed {
                "Hazır (pw-dump + pw-cli)".to_string()
            } else if pw_dump_installed {
                "Yalnızca pw-dump kurulu".to_string()
            } else {
                "Kullanılamıyor".to_string()
            },
        },
        AkrepKeyValue {
            label_tr: "Oturum İzolasyonu".to_string(),
            label_en: "Session Isolation".to_string(),
            value: "Canlı akışlar kullanıcı oturumunda (akrep-ui) doğrudan denetlenir".to_string(),
        },
    ];

    let card = AkrepCardSummary {
        id: "privacy".to_string(),
        title_tr: "Gizlilik & Masaüstü".to_string(),
        title_en: "Privacy & Desktop".to_string(),
        status,
        availability: avail,
        summary_tr: summary_tr.clone(),
        summary_en: summary_en.clone(),
        metric,
    };

    let detail = AkrepSectionDetail {
        section: "privacy".to_string(),
        title_tr: card.title_tr.clone(),
        title_en: card.title_en.clone(),
        status,
        availability: avail,
        summary_tr,
        summary_en,
        metrics,
        findings: Vec::new(),
    };

    PrivacySection {
        card,
        detail,
        findings: Vec::new(),
    }
}
