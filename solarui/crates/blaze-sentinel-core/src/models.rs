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
}
