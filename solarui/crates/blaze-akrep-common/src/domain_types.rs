use crate::errors::AkrepError;
use crate::models::{validate_restorecon_path, validate_selinux_boolean_name, RiskLevel};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelinuxRuntimeMode {
    Enforcing,
    Permissive,
    Disabled,
}

impl SelinuxRuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Enforcing => "enforcing",
            Self::Permissive => "permissive",
            Self::Disabled => "disabled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelinuxBooleanMeta {
    pub name: String,
    pub category: String,
    pub risk: RiskLevel,
    pub summary_tr: String,
    pub summary_en: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelinuxBooleanEntry {
    pub name: String,
    pub current: bool,
    pub pending: bool,
    pub persistent: bool,
    #[serde(default = "default_true")]
    pub persistent_known: bool,
    pub cataloged: bool,
    pub category: String,
    pub risk: RiskLevel,
    pub summary_tr: String,
    pub summary_en: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SuggestedSelinuxFix {
    Restorecon {
        target_path: String,
        recursive: bool,
        expected_context: Option<String>,
    },
    SetBoolean {
        boolean_name: String,
        recommended_value: bool,
        risk: RiskLevel,
    },
    ManualPolicyReview {
        guidance_tr: String,
        guidance_en: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvcDenialGroup {
    pub signature_id: String,
    pub app_comm: String,
    pub source_domain: String,
    pub target_type: String,
    pub target_class: String,
    pub permissions: Vec<String>,
    pub count: usize,
    pub last_seen: String,
    pub sample_path: Option<String>,
    pub severity: RiskLevel,
    pub suggested_remediation_kind: SuggestedSelinuxFix,
    pub explanation_tr: String,
    pub explanation_en: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreconPreviewEntry {
    pub path: String,
    pub from_context: String,
    pub to_context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContextEntry {
    pub path: String,
    pub context: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelinuxDiagnoseQuery {
    #[serde(default = "default_true")]
    pub include_booleans: bool,
    #[serde(default = "default_true")]
    pub include_avc: bool,
    #[serde(default)]
    pub boolean_filter: Option<String>,
    #[serde(default)]
    pub inspect_path: Option<String>,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub action: Option<String>,
}

impl Default for SelinuxDiagnoseQuery {
    fn default() -> Self {
        Self {
            include_booleans: true,
            include_avc: true,
            boolean_filter: None,
            inspect_path: None,
            recursive: false,
            action: None,
        }
    }
}

fn check_forbidden_selinux_directive_str(s: &str) -> Result<(), AkrepError> {
    let lower = s.to_ascii_lowercase();
    for forbidden in ["audit2allow", "semodule", "setenforce", "selinux=disabled"] {
        if lower.contains(forbidden) {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Forbidden SELinux directive '{}' in diagnosis query",
                forbidden
            )));
        }
    }
    Ok(())
}

pub fn reject_forbidden_selinux_directives_in_json_value(
    val: &serde_json::Value,
) -> Result<(), AkrepError> {
    match val {
        serde_json::Value::String(s) => check_forbidden_selinux_directive_str(s),
        serde_json::Value::Array(arr) => {
            for item in arr {
                reject_forbidden_selinux_directives_in_json_value(item)?;
            }
            Ok(())
        }
        serde_json::Value::Object(map) => {
            for (k, v) in map {
                check_forbidden_selinux_directive_str(k)?;
                reject_forbidden_selinux_directives_in_json_value(v)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

impl SelinuxDiagnoseQuery {
    pub fn validate_red_lines(&self, raw_json: &str) -> Result<(), AkrepError> {
        check_forbidden_selinux_directive_str(raw_json)?;
        if let Some(act) = &self.action {
            check_forbidden_selinux_directive_str(act)?;
        }
        if let Some(filter) = &self.boolean_filter {
            check_forbidden_selinux_directive_str(filter)?;
        }
        if let Some(path) = &self.inspect_path {
            check_forbidden_selinux_directive_str(path)?;
        }

        if let Some(act) = &self.action {
            let norm = act.trim().to_ascii_lowercase();
            if !norm.is_empty()
                && !matches!(
                    norm.as_str(),
                    "full" | "booleans" | "avc" | "inspect_path" | "status"
                )
            {
                return Err(AkrepError::InvalidInput(format!(
                    "Unsupported SELinux diagnosis action '{}'",
                    act
                )));
            }
            if norm == "inspect_path"
                && self
                    .inspect_path
                    .as_deref()
                    .map(str::trim)
                    .unwrap_or_default()
                    .is_empty()
            {
                return Err(AkrepError::InvalidInput(
                    "SelinuxDiagnoseQuery action 'inspect_path' requires a non-empty inspect_path"
                        .to_string(),
                ));
            }
        }

        if let Some(filter) = &self.boolean_filter {
            let f = filter.trim();
            if f.len() > 96
                || f.chars()
                    .any(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'))
            {
                return Err(AkrepError::InvalidInput(
                    "Invalid SELinux boolean_filter (expected lowercase ASCII/digits/'_')"
                        .to_string(),
                ));
            }
            if f.len() >= 2 && f.as_bytes()[0].is_ascii_lowercase() {
                validate_selinux_boolean_name(f)?;
            }
        }

        if let Some(path) = &self.inspect_path {
            validate_restorecon_path(path)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelinuxDiagnoseResponse {
    pub runtime_mode: SelinuxRuntimeMode,
    pub configured_mode: Option<String>,
    pub policy_type: Option<String>,
    pub booleans: Vec<SelinuxBooleanEntry>,
    pub avc_groups: Vec<AvcDenialGroup>,
    pub restorecon_preview: Vec<RestoreconPreviewEntry>,
    pub current_contexts: Vec<FileContextEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallZoneState {
    pub name: String,
    pub is_default: bool,
    pub active: bool,
    pub runtime_interfaces: Vec<String>,
    pub permanent_interfaces: Vec<String>,
    pub runtime_services: Vec<String>,
    pub permanent_services: Vec<String>,
    pub runtime_ports: Vec<(String, String)>,
    pub permanent_ports: Vec<(String, String)>,
    pub runtime_rich_rules: Vec<String>,
    pub permanent_rich_rules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallStateSnapshot {
    pub running: bool,
    pub default_zone: String,
    pub zones: Vec<String>,
    pub active_zones: BTreeMap<String, String>,
    pub zone_details: Vec<FirewallZoneState>,
}
