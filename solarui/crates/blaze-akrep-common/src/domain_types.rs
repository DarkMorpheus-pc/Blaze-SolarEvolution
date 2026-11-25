use crate::errors::{redact_secrets, AkrepError};
use crate::models::{
    validate_restorecon_path, validate_selinux_boolean_name, validate_zone_name, FirewallProto,
    RiskLevel,
};
use crate::rich_rule::{parse_and_validate_cidr, validate_port_spec, validate_service_name};
use blaze_sentinel_core::{AkrepSeverity, Availability, PostureReport};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Component, Path, PathBuf};

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

// ============================================================================
// Sprint 3: Device Security Domain Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecureBootState {
    Enabled,
    Disabled,
    LegacyBios,
    UnsupportedFirmware,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecureBootStatus {
    pub uefi_present: bool,
    pub secure_boot_enabled: Option<bool>,
    pub state: SecureBootState,
    pub severity: AkrepSeverity,
    pub availability: Availability,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TpmStatus {
    pub present: bool,
    pub version_major: Option<String>,
    pub tpm2_available: bool,
    pub severity: AkrepSeverity,
    pub availability: Availability,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelLockdownStatus {
    pub cmdline_module_sig_enforce: bool,
    pub sysfs_module_sig_enforce: bool,
    pub module_sig_enforced: bool,
    pub lockdown_mode: String,
    pub iommu_active: bool,
    pub severity: AkrepSeverity,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeIntegrityStatus {
    pub posture: PostureReport,
    pub ld_so_preload_clean: bool,
    pub ld_so_preload_detail: String,
    pub root_canary_present: bool,
    pub root_canary_intact: bool,
    pub severity: AkrepSeverity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbDeviceEntry {
    pub sysfs_name: String,
    pub id_vendor: String,
    pub id_product: String,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub device_class: String,
    pub interface_classes: Vec<String>,
    pub authorized: bool,
    pub anomalies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbAuditReport {
    pub devices: Vec<UsbDeviceEntry>,
    pub unauthorized_count: usize,
    pub anomaly_count: usize,
    pub severity: AkrepSeverity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RpmIntegrityStatus {
    pub total_packages: usize,
    pub signed_packages: usize,
    pub unsigned_packages: Vec<String>,
    pub verified_critical_packages: Vec<String>,
    pub verification_anomalies: Vec<String>,
    pub severity: AkrepSeverity,
    pub availability: Availability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceSecurityStatus {
    pub overall_severity: AkrepSeverity,
    pub secure_boot: SecureBootStatus,
    pub tpm: TpmStatus,
    pub kernel_lockdown: KernelLockdownStatus,
    pub runtime_integrity: RuntimeIntegrityStatus,
    pub usb_audit: UsbAuditReport,
    pub rpm_integrity: RpmIntegrityStatus,
    pub inspected_at: String,
}

// ============================================================================
// Sprint 3: Optional ClamAV Scanner & Quarantine Domain Types
// ============================================================================

pub const OPTIONAL_SCANNER_INSTALL_HINT: &str = "clamav, clamd, clamav-freshclam";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScannerEngineState {
    ClamdSocket { socket_path: String },
    ClamscanCli { binary_path: String },
    Unavailable { install_hint: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignatureFreshness {
    Healthy,
    Warning,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureDbStatus {
    pub freshness: SignatureFreshness,
    pub severity: AkrepSeverity,
    pub availability: Availability,
    pub age_days: Option<u64>,
    pub db_path: Option<String>,
    pub version: Option<u32>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScanVerdict {
    Clean,
    Suspicious { reason: String },
    Infected { signature: String },
    Unknown { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanFinding {
    pub path: String,
    pub verdict: ScanVerdict,
    pub threat_name: Option<String>,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanJobState {
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanRequest {
    pub target_path: String,
    #[serde(default = "default_true")]
    pub recursive: bool,
    #[serde(default)]
    pub socket_override: Option<String>,
    #[serde(default)]
    pub synchronous: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanProgress {
    pub job_id: String,
    pub target_path: String,
    pub scanned_files: usize,
    pub total_files: usize,
    pub percent: u8,
    pub current_path: Option<String>,
    pub findings: Vec<ScanFinding>,
    pub state: ScanJobState,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanHistoryEntry {
    pub job_id: String,
    pub target_path: String,
    pub scanned_files: usize,
    pub findings_count: usize,
    pub state: ScanJobState,
    pub started_at: String,
    pub finished_at: String,
    pub actor_uid: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScannerStatusReport {
    pub engine_state: ScannerEngineState,
    pub signature_db: SignatureDbStatus,
    pub realtime_protection_active: bool,
    pub severity: AkrepSeverity,
    pub availability: Availability,
    pub recent_history: Vec<ScanHistoryEntry>,
}

fn has_shell_or_control_chars(value: &str) -> bool {
    value.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                ';' | '&' | '|' | '`' | '$' | '(' | ')' | '<' | '>' | '\\' | '"' | '\''
            )
    })
}

pub fn validate_clamd_endpoint(endpoint: &str) -> Result<PathBuf, AkrepError> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() || trimmed != endpoint || has_shell_or_control_chars(trimmed) {
        return Err(AkrepError::InvalidInput(
            "Invalid clamd socket endpoint path".to_string(),
        ));
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("tcp://")
        || lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("unix://")
        || lower.contains(":3310")
        || lower.contains("127.0.0.1")
        || lower.contains("0.0.0.0")
        || lower.contains("localhost")
        || lower.contains("[::1]")
        || lower.contains("::")
        || trimmed.contains(':')
        || trimmed.parse::<IpAddr>().is_ok()
        || trimmed.parse::<SocketAddr>().is_ok()
    {
        return Err(AkrepError::ForbiddenOperation(
            "TCP or network clamd endpoints are strictly forbidden; only local Unix domain sockets are permitted"
                .to_string(),
        ));
    }

    let p = Path::new(trimmed);
    if !p.is_absolute() {
        return Err(AkrepError::ForbiddenOperation(
            "clamd endpoint must be an absolute local Unix domain socket path".to_string(),
        ));
    }
    for comp in p.components() {
        if matches!(comp, Component::ParentDir | Component::CurDir) {
            return Err(AkrepError::ForbiddenOperation(
                "Path traversal is forbidden in clamd socket path".to_string(),
            ));
        }
    }
    Ok(p.to_path_buf())
}

pub fn validate_scan_target_path(path_str: &str) -> Result<PathBuf, AkrepError> {
    if path_str.is_empty() || has_shell_or_control_chars(path_str) {
        return Err(AkrepError::InvalidInput(
            "Scan target_path contains invalid or forbidden characters".to_string(),
        ));
    }
    let p = Path::new(path_str);
    if !p.is_absolute() {
        return Err(AkrepError::InvalidInput(
            "Scan target_path must be an absolute path".to_string(),
        ));
    }
    for comp in p.components() {
        if matches!(comp, Component::ParentDir | Component::CurDir) {
            return Err(AkrepError::ForbiddenOperation(
                "Path traversal ('..' or '.') is forbidden in scan target_path".to_string(),
            ));
        }
    }
    if p == Path::new("/") {
        return Err(AkrepError::ForbiddenOperation(
            "Scanning root filesystem '/' directly is forbidden; specify a concrete directory or file"
                .to_string(),
        ));
    }
    for forbidden in ["/proc", "/sys", "/dev", "/run", "/var/lib/akrep/quarantine"] {
        if path_str == forbidden || path_str.starts_with(&format!("{}/", forbidden)) {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Scanning virtual or quarantine path '{}' is forbidden",
                forbidden
            )));
        }
    }
    Ok(p.to_path_buf())
}

impl ScanRequest {
    pub fn validate_red_lines(&self) -> Result<(), AkrepError> {
        let _ = validate_scan_target_path(&self.target_path)?;
        if let Some(sock) = &self.socket_override {
            let _ = validate_clamd_endpoint(sock)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineMetadata {
    pub quarantine_id: String,
    pub original_path: String,
    pub original_mode: u32,
    pub original_uid: u32,
    pub original_gid: u32,
    pub selinux_context: Option<String>,
    pub sha256: String,
    pub size_bytes: u64,
    pub threat_name: String,
    pub quarantined_at: String,
    pub actor_uid: u32,
    #[serde(default)]
    pub restored: bool,
    #[serde(default)]
    pub restored_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuarantineInventory {
    pub vault_path: String,
    pub entries: Vec<QuarantineMetadata>,
}

// ============================================================================
// Sprint 4: Declarative Security Profiles (v0.5)
// ============================================================================

pub const OFFICIAL_PROFILE_IDS: [&str; 4] = ["balanced", "developer", "gaming", "public-network"];

pub const GLOBAL_FORBIDDEN_PROFILE_BOOLEANS: [&str; 3] = [
    "selinuxuser_execheap",
    "selinuxuser_execstack",
    "mmap_low_allowed",
];

pub fn is_rfc1918_ipv4_cidr(cidr: &str) -> Result<String, AkrepError> {
    let (_, normalized) = parse_and_validate_cidr(cidr)?;
    let Some((addr_str, prefix_str)) = normalized.split_once('/') else {
        return Err(AkrepError::InvalidInput(format!("Invalid CIDR '{}'", cidr)));
    };
    let v4: Ipv4Addr = addr_str.parse().map_err(|_| {
        AkrepError::ForbiddenOperation(format!(
            "Profile LAN rich rules must use RFC1918 private IPv4 subnets (got '{}')",
            cidr
        ))
    })?;
    let prefix: u8 = prefix_str
        .parse()
        .map_err(|_| AkrepError::InvalidInput(format!("Invalid CIDR prefix in '{}'", cidr)))?;
    let octets = v4.octets();
    let is_rfc1918 = (octets[0] == 10 && (8..=32).contains(&prefix))
        || (octets[0] == 172 && (16..=31).contains(&octets[1]) && (12..=32).contains(&prefix))
        || (octets[0] == 192 && octets[1] == 168 && (16..=32).contains(&prefix));

    if !is_rfc1918 {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Profile LAN rich rule CIDR '{}' is outside RFC1918 private LAN ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)",
            cidr
        )));
    }
    Ok(normalized)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileManagedBoolean {
    pub name: String,
    pub value: bool,
    #[serde(default)]
    pub persistent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileLanRichRule {
    pub cidr: String,
    pub port: String,
    pub proto: FirewallProto,
    #[serde(default)]
    pub permanent: bool,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SecurityProfileSpec {
    pub id: String,
    pub version: u32,
    pub name_tr: String,
    pub name_en: String,
    pub description_tr: String,
    pub description_en: String,
    pub selinux_required: bool,
    pub firewalld_required: bool,
    pub target_zone: String,
    #[serde(default)]
    pub fallback_zone: Option<String>,
    #[serde(default)]
    pub allow_permanent_rules: bool,
    #[serde(default = "default_true")]
    pub preserve_flatpak_sandbox: bool,
    #[serde(default = "default_true")]
    pub preserve_portal_consent: bool,
    #[serde(default)]
    pub clear_managed_rules: bool,
    #[serde(default)]
    pub lock_interfaces_to_target_zone: bool,
    #[serde(default)]
    pub remove_runtime_services_in_target_zone: Vec<String>,
    #[serde(default)]
    pub managed_booleans: Vec<ProfileManagedBoolean>,
    #[serde(default)]
    pub forbidden_booleans: Vec<String>,
    #[serde(default)]
    pub lan_rich_rules: Vec<ProfileLanRichRule>,
}

impl SecurityProfileSpec {
    pub fn parse_and_validate_toml(
        raw_toml: &str,
        expected_id: Option<&str>,
    ) -> Result<Self, AkrepError> {
        if raw_toml.trim().is_empty() || raw_toml.len() > 65_536 || raw_toml.contains('\0') {
            return Err(AkrepError::InvalidInput(
                "Profile TOML document is empty, contains NUL bytes, or exceeds 64 KiB".to_string(),
            ));
        }
        let spec: Self = toml::from_str(raw_toml).map_err(|e| {
            AkrepError::InvalidInput(format!(
                "Malformed security profile TOML: {}",
                redact_secrets(&e.to_string())
            ))
        })?;
        spec.validate(expected_id)?;
        Ok(spec)
    }

    pub fn validate(&self, expected_id: Option<&str>) -> Result<(), AkrepError> {
        if self.version != 1 {
            return Err(AkrepError::InvalidInput(format!(
                "Unsupported security profile schema version {} (expected 1)",
                self.version
            )));
        }
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(AkrepError::InvalidInput(format!(
                "Invalid profile id '{}'",
                self.id
            )));
        }
        if !OFFICIAL_PROFILE_IDS.contains(&self.id.as_str()) {
            return Err(AkrepError::InvalidInput(format!(
                "Unknown security profile id '{}'",
                self.id
            )));
        }
        if let Some(exp) = expected_id {
            if self.id != exp {
                return Err(AkrepError::InvalidInput(format!(
                    "Profile TOML id '{}' does not match expected profile '{}'",
                    self.id, exp
                )));
            }
        }
        if self.name_tr.trim().is_empty() || self.name_en.trim().is_empty() {
            return Err(AkrepError::InvalidInput(
                "Profile localized names (name_tr/name_en) must not be empty".to_string(),
            ));
        }
        if !self.selinux_required {
            return Err(AkrepError::ForbiddenOperation(
                "Security profile red line violated: selinux_required=false is strictly forbidden"
                    .to_string(),
            ));
        }
        if !self.firewalld_required {
            return Err(AkrepError::ForbiddenOperation(
                "Security profile red line violated: firewalld_required=false is strictly forbidden"
                    .to_string(),
            ));
        }
        if !self.preserve_flatpak_sandbox || !self.preserve_portal_consent {
            return Err(AkrepError::ForbiddenOperation(
                "Security profile must preserve Flatpak sandboxes and XDG Portal consent"
                    .to_string(),
            ));
        }

        validate_zone_name(&self.target_zone)?;
        if let Some(fb) = &self.fallback_zone {
            validate_zone_name(fb)?;
        }

        for fb_bool in &self.forbidden_booleans {
            validate_selinux_boolean_name(fb_bool)?;
        }

        for mb in &self.managed_booleans {
            validate_selinux_boolean_name(&mb.name)?;
            if self.forbidden_booleans.iter().any(|f| f == &mb.name) {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Profile '{}' cannot manage boolean '{}' listed in forbidden_booleans",
                    self.id, mb.name
                )));
            }
            if mb.value && GLOBAL_FORBIDDEN_PROFILE_BOOLEANS.contains(&mb.name.as_str()) {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Enabling memory-weakening SELinux boolean '{}' in a profile is forbidden",
                    mb.name
                )));
            }
        }

        for svc in &self.remove_runtime_services_in_target_zone {
            validate_service_name(svc)?;
        }

        for rule in &self.lan_rich_rules {
            if rule.permanent && !self.allow_permanent_rules {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Profile '{}' forbids permanent firewall rules",
                    self.id
                )));
            }
            let _ = is_rfc1918_ipv4_cidr(&rule.cidr)?;
            validate_port_spec(&rule.port)?;
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityProfileSummary {
    pub id: String,
    pub version: u32,
    pub name_tr: String,
    pub name_en: String,
    pub description_tr: String,
    pub description_en: String,
    pub target_zone: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedBooleanBaseline {
    pub name: String,
    pub original_runtime: bool,
    pub original_persistent: Option<bool>,
    pub applied_value: bool,
    pub applied_persistent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedRichRuleEntry {
    pub zone: String,
    pub rule: String,
    pub permanent: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedInterfaceMove {
    pub interface: String,
    pub prior_zone: String,
    pub applied_zone: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedRemovedService {
    pub zone: String,
    pub service: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManagedProfileState {
    pub active_profile_id: Option<String>,
    pub effective_zone: Option<String>,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub managed_booleans: Vec<ManagedBooleanBaseline>,
    #[serde(default)]
    pub managed_rich_rules: Vec<ManagedRichRuleEntry>,
    #[serde(default)]
    pub moved_interfaces: Vec<ManagedInterfaceMove>,
    #[serde(default)]
    pub removed_runtime_services: Vec<ManagedRemovedService>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileCatalogResponse {
    pub active_profile_id: Option<String>,
    pub managed_state: ManagedProfileState,
    pub profiles: Vec<SecurityProfileSummary>,
}
