use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::severity::{AkrepSeverity, Availability};

pub const AKREP_SCHEMA_VERSION: u32 = 2;
pub const AKREP_VERSION: &str = "1.0.0";
pub const ROOT_CANARY_DIR: &str = "/var/lib/blaze";
pub const ROOT_CANARY_PATH: &str = "/var/lib/blaze/.blaze_canary";

fn default_true() -> bool {
    true
}

fn default_active_profile_id() -> String {
    "balanced".to_string()
}

fn default_active_profile_tr() -> String {
    "Dengeli".to_string()
}

fn default_active_profile_en() -> String {
    "Balanced".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityControl {
    pub id: String,
    pub name: String,
    pub title: String,
    pub passed: bool,
    pub level: String,
    pub critical: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostureReport {
    pub passed: usize,
    pub total: usize,
    pub verified: bool,
    pub integrity_ok: bool,
    pub controls: Vec<SecurityControl>,
    pub checks: Vec<SecurityControl>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SentinelConfig {
    pub canary_paths: Vec<PathBuf>,
    pub monitored_files: Vec<PathBuf>,
    pub check_interval_secs: u64,
}

impl Default for SentinelConfig {
    fn default() -> Self {
        // Security hardening: root-only /var/lib/blaze/.blaze_canary (0700 dir, 0600 file)
        // instead of world-writable /var/tmp/.blaze_canary.
        let mut canaries = vec![PathBuf::from(ROOT_CANARY_PATH)];
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() && home != "/" {
                canaries.push(PathBuf::from(home).join(".config/.blaze_canary"));
            }
        }

        Self {
            canary_paths: canaries,
            monitored_files: vec![
                PathBuf::from("/etc/ld.so.preload"),
                PathBuf::from("/etc/pam.d/sudo"),
                PathBuf::from("/etc/pam.d/system-auth"),
                PathBuf::from("/etc/shadow"),
            ],
            check_interval_secs: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AkrepKeyValue {
    pub label_tr: String,
    pub label_en: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AkrepCardSummary {
    pub id: String,
    pub title_tr: String,
    pub title_en: String,
    pub status: AkrepSeverity,
    pub availability: Availability,
    pub summary_tr: String,
    pub summary_en: String,
    pub metric: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AkrepFinding {
    pub id: String,
    pub section: String,
    pub severity: AkrepSeverity,
    pub availability: Availability,
    pub title_tr: String,
    pub title_en: String,
    pub description_tr: String,
    pub description_en: String,
    pub remediation_hint_tr: String,
    pub remediation_hint_en: String,
    pub source: String,
    pub read_only_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AkrepSectionDetail {
    pub section: String,
    pub title_tr: String,
    pub title_en: String,
    pub status: AkrepSeverity,
    pub availability: Availability,
    pub summary_tr: String,
    pub summary_en: String,
    pub metrics: Vec<AkrepKeyValue>,
    pub findings: Vec<AkrepFinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AkrepCapabilities {
    pub version: String,
    pub sentinel_version: String,
    pub schema_version: u32,
    pub read_only: bool,
    #[serde(default = "default_true")]
    pub read_only_daemon: bool,
    pub mutators_enabled: bool,
    #[serde(default)]
    pub privileged_bus: String,
    pub sections: Vec<String>,
    pub future_polkit_actions: Vec<String>,
}

/// Active temporary or manual override entry surfaced read-only in `AkrepSnapshot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveOverrideEntry {
    pub subsystem: String,
    pub duration_kind: String,
    pub minutes: Option<u32>,
    pub started_at: String,
    pub expires_at: Option<String>,
    pub remaining_secs: Option<i64>,
    pub boot_id: String,
}

/// Read-only developer and build compatibility status (`systemd-oomd`, `nofile`, Android packages).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemCompatStatus {
    pub oomd_active: bool,
    pub oomd_enabled: bool,
    pub nofile_enabled: bool,
    pub nofile_soft: u32,
    pub nofile_hard: u32,
    pub android_udev_installed: bool,
    pub ncurses_compat_installed: bool,
}

/// Read-only Secure Boot, MOK enrollment, and NVIDIA kernel module compatibility summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverMokSummary {
    pub secure_boot_enabled: Option<bool>,
    pub mok_enrolled_count: Option<usize>,
    pub nvidia_module_loaded: bool,
    pub akmod_nvidia_installed: bool,
    pub unsigned_risk: bool,
    pub guidance_tr: String,
    pub guidance_en: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AkrepSnapshot {
    pub schema_version: u32,
    pub version: String,
    pub sentinel_version: String,
    pub read_only: bool,
    #[serde(default = "default_true")]
    pub read_only_daemon: bool,
    pub mutators_enabled: bool,
    #[serde(default = "default_active_profile_id")]
    pub active_profile_id: String,
    #[serde(default = "default_active_profile_tr")]
    pub active_profile_tr: String,
    #[serde(default = "default_active_profile_en")]
    pub active_profile_en: String,
    pub generated_at: String,
    pub overall_status: AkrepSeverity,
    pub headline_tr: String,
    pub headline_en: String,
    pub posture: PostureReport,
    pub cards: Vec<AkrepCardSummary>,
    pub findings: Vec<AkrepFinding>,
    pub sections: Vec<AkrepSectionDetail>,
    pub capabilities: AkrepCapabilities,
    #[serde(default)]
    pub active_overrides: Vec<ActiveOverrideEntry>,
    #[serde(default)]
    pub system_compat: Option<SystemCompatStatus>,
    #[serde(default)]
    pub driver_mok: Option<DriverMokSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_akrep_snapshot_deserializes_legacy_json_without_new_fields() {
        let legacy_json = r#"{
            "schema_version": 2,
            "version": "1.0.0",
            "sentinel_version": "0.1.0",
            "read_only": true,
            "mutators_enabled": true,
            "active_profile_id": "balanced",
            "active_profile_tr": "Dengeli",
            "active_profile_en": "Balanced",
            "generated_at": "2026-11-26T12:00:00Z",
            "overall_status": "protected",
            "headline_tr": "Korunuyor",
            "headline_en": "Protected",
            "posture": {
                "passed": 7,
                "total": 7,
                "verified": true,
                "integrity_ok": true,
                "controls": [],
                "checks": []
            },
            "cards": [],
            "findings": [],
            "sections": [],
            "capabilities": {
                "version": "1.0.0",
                "sentinel_version": "0.1.0",
                "schema_version": 2,
                "read_only": true,
                "mutators_enabled": true,
                "sections": [],
                "future_polkit_actions": []
            }
        }"#;

        let snap: AkrepSnapshot =
            serde_json::from_str(legacy_json).expect("legacy schema v2 JSON must deserialize");
        assert_eq!(snap.schema_version, 2);
        assert!(snap.active_overrides.is_empty());
        assert!(snap.system_compat.is_none());
        assert!(snap.driver_mok.is_none());

        let override_entry = ActiveOverrideEntry {
            subsystem: "selinux".to_string(),
            duration_kind: "timed".to_string(),
            minutes: Some(15),
            started_at: "2026-11-26T12:00:00Z".to_string(),
            expires_at: Some("2026-11-26T12:15:00Z".to_string()),
            remaining_secs: Some(900),
            boot_id: "boot-123".to_string(),
        };
        let compat = SystemCompatStatus {
            oomd_active: true,
            oomd_enabled: true,
            nofile_enabled: true,
            nofile_soft: 65536,
            nofile_hard: 524288,
            android_udev_installed: false,
            ncurses_compat_installed: true,
        };
        let mok = DriverMokSummary {
            secure_boot_enabled: Some(true),
            mok_enrolled_count: Some(1),
            nvidia_module_loaded: false,
            akmod_nvidia_installed: false,
            unsigned_risk: false,
            guidance_tr: "Hazir".to_string(),
            guidance_en: "Ready".to_string(),
        };
        assert_eq!(override_entry.minutes, Some(15));
        assert_eq!(compat.nofile_hard, 524288);
        assert_eq!(mok.mok_enrolled_count, Some(1));
    }
}
