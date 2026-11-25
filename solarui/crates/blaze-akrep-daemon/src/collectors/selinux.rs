use blaze_sentinel_core::{
    AkrepCardSummary, AkrepFinding, AkrepKeyValue, AkrepSectionDetail, AkrepSeverity, Availability,
};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::process::Command;

const MAX_READONLY_AUDIT_TAIL_BYTES: u64 = 8 * 1024 * 1024;

pub struct SelinuxSection {
    pub card: AkrepCardSummary,
    pub detail: AkrepSectionDetail,
    pub findings: Vec<AkrepFinding>,
}

fn parse_selinux_config_kv() -> (Option<String>, Option<String>) {
    let Ok(raw) = fs::read_to_string("/etc/selinux/config") else {
        return (None, None);
    };
    let mut mode = None;
    let mut policy = None;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(v) = line.strip_prefix("SELINUX=") {
            mode = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("SELINUXTYPE=") {
            policy = Some(v.trim().to_string());
        }
    }
    (mode, policy)
}

pub fn count_avc_denials_in_audit_file(path: &Path) -> std::io::Result<usize> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut buf = Vec::new();
    if file_len > MAX_READONLY_AUDIT_TAIL_BYTES {
        file.seek(SeekFrom::Start(file_len - MAX_READONLY_AUDIT_TAIL_BYTES))?;
        file.read_to_end(&mut buf)?;
        if let Some(nl_pos) = buf.iter().position(|&b| b == b'\n') {
            buf.drain(..=nl_pos);
        }
    } else {
        file.read_to_end(&mut buf)?;
    }
    let raw = String::from_utf8_lossy(&buf);
    Ok(raw
        .lines()
        .filter(|l| l.contains("avc:") && l.contains("denied"))
        .count())
}

fn check_recent_avc_readonly() -> (Availability, Option<usize>, String) {
    // Try audit.log only if directly readable by current service account
    if let Ok(count) = count_avc_denials_in_audit_file(Path::new("/var/log/audit/audit.log")) {
        return (
            Availability::Available,
            Some(count),
            format!("{} reddedilen AVC olayı (/var/log/audit/audit.log)", count),
        );
    }

    // Try journalctl for setroubleshoot/audit entries if accessible (returns Err/permission error when restricted)
    if let Ok(out) = Command::new("journalctl")
        .env("LC_ALL", "C")
        .env("LANG", "C")
        .args([
            "-t",
            "setroubleshoot",
            "--since",
            "24 hours ago",
            "--no-pager",
            "-n",
            "25",
            "-o",
            "cat",
        ])
        .output()
    {
        if out.status.success() {
            let txt = String::from_utf8_lossy(&out.stdout);
            let lines: Vec<&str> = txt.lines().filter(|l| !l.trim().is_empty()).collect();
            return (
                Availability::Available,
                Some(lines.len()),
                format!("{} setroubleshoot uyarısı (son 24 saat)", lines.len()),
            );
        }
    }

    (
        Availability::Unavailable,
        None,
        "Denetim günlüğü yalnızca yetkili yönetici için okunabilir (Salt Okunur)".to_string(),
    )
}

pub fn count_sysfs_booleans_at(bool_dir: &Path) -> Option<(usize, usize)> {
    let entries = fs::read_dir(bool_dir).ok()?;
    let mut total = 0usize;
    let mut enabled = 0usize;
    for entry in entries.flatten() {
        let Ok(raw) = fs::read_to_string(entry.path()) else {
            continue;
        };
        total += 1;
        if raw.split_whitespace().next() == Some("1") {
            enabled += 1;
        }
    }
    Some((enabled, total))
}

fn summarize_selinux_booleans_readonly() -> String {
    match count_sysfs_booleans_at(Path::new("/sys/fs/selinux/booleans")) {
        Some((enabled, total)) => format!("{} etkin / {} toplam", enabled, total),
        None => "Kullanılamıyor (/sys/fs/selinux yok)".to_string(),
    }
}

pub fn collect_selinux() -> SelinuxSection {
    let enforce_sysfs = fs::read_to_string("/sys/fs/selinux/enforce")
        .ok()
        .map(|s| s.trim().to_string());
    let lsm_stack = fs::read_to_string("/sys/kernel/security/lsm")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "bilinmiyor".to_string());
    let (cfg_mode, cfg_policy) = parse_selinux_config_kv();

    let (status, mode_tr, mode_en) = match enforce_sysfs.as_deref() {
        Some("1") => (
            AkrepSeverity::Normal,
            "Enforcing (Zorunlu)".to_string(),
            "Enforcing".to_string(),
        ),
        Some("0") => (
            AkrepSeverity::Warning,
            "Permissive (İzin Veren)".to_string(),
            "Permissive".to_string(),
        ),
        _ => {
            if Path::new("/sys/fs/selinux").exists() {
                (
                    AkrepSeverity::Unknown,
                    "Bilinmiyor".to_string(),
                    "Unknown".to_string(),
                )
            } else {
                (
                    AkrepSeverity::Critical,
                    "Devre Dışı".to_string(),
                    "Disabled".to_string(),
                )
            }
        }
    };

    let (avc_avail, avc_count, avc_summary) = check_recent_avc_readonly();
    let booleans_summary = summarize_selinux_booleans_readonly();
    let policy_str = cfg_policy.unwrap_or_else(|| "targeted (varsayılan)".to_string());
    let cfg_mode_str = cfg_mode.unwrap_or_else(|| "bilinmiyor".to_string());

    let mut final_status = status;
    let mut findings = Vec::new();
    if status != AkrepSeverity::Normal {
        findings.push(AkrepFinding {
            id: "selinux.mode".to_string(),
            section: "selinux".to_string(),
            severity: status,
            availability: Availability::Available,
            title_tr: format!("SELinux Durumu: {}", mode_tr),
            title_en: format!("SELinux Mode: {}", mode_en),
            description_tr: format!("Aktif çalışma kipi '{}' olarak algılandı (yapılandırma: {}).", mode_tr, cfg_mode_str),
            description_en: format!("Active runtime mode detected as '{}' (config: {}).", mode_en, cfg_mode_str),
            remediation_hint_tr: "Akrep v0.1 salt okunurdur; kalıcı değişiklik için /etc/selinux/config dosyasını yönetici olarak doğrulayın.".to_string(),
            remediation_hint_en: "Akrep v0.1 is strictly read-only; review /etc/selinux/config as an administrator.".to_string(),
            source: "collectors/selinux".to_string(),
            read_only_note: "Salt Okunur v0.1 — setenforce / bool değişikliği yapılmaz.".to_string(),
        });
    }

    if let Some(cnt) = avc_count {
        if cnt > 0 {
            final_status = final_status.combine(AkrepSeverity::Warning);
            findings.push(AkrepFinding {
                id: "selinux.avc_recent".to_string(),
                section: "selinux".to_string(),
                severity: AkrepSeverity::Warning,
                availability: avc_avail,
                title_tr: format!("Son 24 Saatte {} AVC / SELinux Uyarısı", cnt),
                title_en: format!("{} Recent SELinux / AVC Event(s)", cnt),
                description_tr: avc_summary.clone(),
                description_en: avc_summary.clone(),
                remediation_hint_tr: "Yönetici terminalinde `ausearch -m avc -ts recent` ile etiket reddi nedenini inceleyin.".to_string(),
                remediation_hint_en: "Inspect AVC denial details using `ausearch -m avc -ts recent` as administrator.".to_string(),
                source: "collectors/selinux".to_string(),
                read_only_note: "Salt Okunur v0.1 — politika yüklemesi yapılmaz.".to_string(),
            });
        }
    }

    let metrics = vec![
        AkrepKeyValue {
            label_tr: "Çalışma Kipi".to_string(),
            label_en: "Runtime Mode".to_string(),
            value: mode_tr.clone(),
        },
        AkrepKeyValue {
            label_tr: "Politika Türü".to_string(),
            label_en: "Policy Type".to_string(),
            value: policy_str.clone(),
        },
        AkrepKeyValue {
            label_tr: "Yapılandırma (/etc/selinux/config)".to_string(),
            label_en: "Configured Mode".to_string(),
            value: cfg_mode_str,
        },
        AkrepKeyValue {
            label_tr: "Çekirdek LSM Yığını".to_string(),
            label_en: "Kernel LSM Stack".to_string(),
            value: lsm_stack,
        },
        AkrepKeyValue {
            label_tr: "Politika Anahtarları (Booleans)".to_string(),
            label_en: "Policy Booleans".to_string(),
            value: booleans_summary,
        },
        AkrepKeyValue {
            label_tr: "Son AVC Kayıtları".to_string(),
            label_en: "Recent AVC Log".to_string(),
            value: avc_summary,
        },
    ];

    let card = AkrepCardSummary {
        id: "selinux".to_string(),
        title_tr: "Sistem Koruması (SELinux)".to_string(),
        title_en: "System Protection (SELinux)".to_string(),
        status: final_status,
        availability: Availability::Available,
        summary_tr: format!("{} • Politika: {}", mode_tr, policy_str),
        summary_en: format!("{} • Policy: {}", mode_en, policy_str),
        metric: mode_en,
    };

    let detail = AkrepSectionDetail {
        section: "selinux".to_string(),
        title_tr: card.title_tr.clone(),
        title_en: card.title_en.clone(),
        status: final_status,
        availability: Availability::Available,
        summary_tr: card.summary_tr.clone(),
        summary_en: card.summary_en.clone(),
        metrics,
        findings: findings.clone(),
    };

    SelinuxSection {
        card,
        detail,
        findings,
    }
}
