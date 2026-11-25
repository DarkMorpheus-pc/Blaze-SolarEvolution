use blaze_sentinel_core::{
    AkrepCardSummary, AkrepFinding, AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, Availability,
};
use std::fs;
use std::path::Path;

pub struct HistorySection {
    pub card: AkrepCardSummary,
    pub detail: AkrepSectionDetail,
    pub findings: Vec<AkrepFinding>,
}

fn read_tail_lines(path: &str, max_lines: usize) -> Result<Vec<String>, std::io::Error> {
    let raw = fs::read_to_string(path)?;
    let lines: Vec<String> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|s| s.to_string())
        .collect();
    let start = lines.len().saturating_sub(max_lines);
    Ok(lines[start..].to_vec())
}

pub fn collect_history() -> HistorySection {
    let mut metrics = Vec::new();
    let mut avail = Availability::Available;
    let mut status = AkrepSeverity::Normal;

    match read_tail_lines("/var/log/bere-recovery.log", 5) {
        Ok(lines) => {
            metrics.push(AkrepKeyValue {
                label_tr: "BERE Kurtarma Denetim Günlüğü".to_string(),
                label_en: "BERE Recovery Audit Log".to_string(),
                value: if lines.is_empty() {
                    "Kayıt yok".to_string()
                } else {
                    format!("{} son olay okundu", lines.len())
                },
            });
            for (idx, ln) in lines.iter().rev().enumerate() {
                metrics.push(AkrepKeyValue {
                    label_tr: format!("BERE Olay #{}", idx + 1),
                    label_en: format!("BERE Event #{}", idx + 1),
                    value: ln.chars().take(180).collect(),
                });
            }
        }
        Err(_) if Path::new("/var/log/bere-recovery.log").exists() => {
            avail = Availability::Unavailable;
            metrics.push(AkrepKeyValue {
                label_tr: "BERE Kurtarma Denetim Günlüğü".to_string(),
                label_en: "BERE Recovery Audit Log".to_string(),
                value: "Erişilemiyor — yalnızca root (0600)".to_string(),
            });
        }
        Err(_) => {
            metrics.push(AkrepKeyValue {
                label_tr: "BERE Kurtarma Denetim Günlüğü".to_string(),
                label_en: "BERE Recovery Audit Log".to_string(),
                value: "Henüz kurtarma olayı yok".to_string(),
            });
        }
    }

    match read_tail_lines("/var/log/blaze-sentinel.log", 5) {
        Ok(lines) => {
            if !lines.is_empty() {
                status = status.combine(AkrepSeverity::Warning);
            }
            metrics.push(AkrepKeyValue {
                label_tr: "Blaze Sentinel Alarm Günlüğü".to_string(),
                label_en: "Blaze Sentinel Alert Log".to_string(),
                value: if lines.is_empty() {
                    "Temiz (0 alarm)".to_string()
                } else {
                    lines.join(" | ").chars().take(200).collect()
                },
            });
        }
        Err(_) if Path::new("/var/log/blaze-sentinel.log").exists() => {
            avail = Availability::Unavailable;
            metrics.push(AkrepKeyValue {
                label_tr: "Blaze Sentinel Alarm Günlüğü".to_string(),
                label_en: "Blaze Sentinel Alert Log".to_string(),
                value: "Erişilemiyor (Salt Okunur)".to_string(),
            });
        }
        Err(_) => {
            metrics.push(AkrepKeyValue {
                label_tr: "Blaze Sentinel Alarm Günlüğü".to_string(),
                label_en: "Blaze Sentinel Alert Log".to_string(),
                value: "Temiz (0 alarm)".to_string(),
            });
        }
    }

    let summary_tr = match avail {
        Availability::Available => {
            "Kurtarma ve bütünlük günlükleri salt okunur denetlendi".to_string()
        }
        _ => "Kısıtlı denetim günlükleri root yetkisi gerektirir (Salt Okunur)".to_string(),
    };
    let summary_en = match avail {
        Availability::Available => {
            "Recovery and integrity event logs inspected read-only".to_string()
        }
        _ => "Privileged audit logs restricted under least-privilege service".to_string(),
    };

    let card = AkrepCardSummary {
        id: "history".to_string(),
        title_tr: "Koruma Geçmişi".to_string(),
        title_en: "Protection History".to_string(),
        status,
        availability: avail,
        summary_tr: summary_tr.clone(),
        summary_en: summary_en.clone(),
        metric: avail.label_en().to_string(),
    };

    let detail = AkrepSectionDetail {
        section: "history".to_string(),
        title_tr: card.title_tr.clone(),
        title_en: card.title_en.clone(),
        status,
        availability: avail,
        summary_tr,
        summary_en,
        metrics,
        findings: Vec::new(),
    };

    HistorySection {
        card,
        detail,
        findings: Vec::new(),
    }
}
