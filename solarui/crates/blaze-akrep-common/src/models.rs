use crate::errors::AkrepError;
use crate::ini_override::resolve_override_spec;
use crate::protocol::{
    DEFAULT_PLAN_TTL_SECS, POLKIT_FIREWALL_MANAGE, POLKIT_FLATPAK_SYSTEM_MANAGE,
    POLKIT_PACKAGES_INSTALL, POLKIT_PROFILE_APPLY, POLKIT_QUARANTINE_MANAGE, POLKIT_SELINUX_MANAGE,
};
use crate::rich_rule::{parse_and_validate_cidr, validate_port_spec, validate_service_name};
use blaze_sentinel_core::sha256_hex;
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::{Component, Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FirewallProto {
    Tcp,
    Udp,
}

impl FirewallProto {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RulePersistence {
    Runtime,
    TimedRuntime { seconds: u32 },
    Permanent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlatpakScope {
    User,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanDiffItem {
    pub target: String,
    pub current_value: String,
    pub desired_value: String,
    pub description_tr: String,
    pub description_en: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateFingerprint {
    pub target: String,
    pub sha256: String,
    pub captured_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionRequest {
    SelinuxSetBoolean {
        name: String,
        value: bool,
        persistent: bool,
    },
    SelinuxRestorecon {
        target_path: String,
        recursive: bool,
    },
    FirewallAddPort {
        zone: String,
        port: String,
        proto: FirewallProto,
        mode: RulePersistence,
        source_cidr: Option<String>,
    },
    FirewallRemovePort {
        zone: String,
        port: String,
        proto: FirewallProto,
        permanent: bool,
        source_cidr: Option<String>,
    },
    FirewallAddService {
        zone: String,
        service: String,
        mode: RulePersistence,
        source_cidr: Option<String>,
    },
    FirewallRemoveService {
        zone: String,
        service: String,
        permanent: bool,
        source_cidr: Option<String>,
    },
    FirewallBindInterface {
        zone: String,
        interface: String,
        permanent: bool,
    },
    FlatpakSetOverride {
        app_id: String,
        scope: FlatpakScope,
        additions: Vec<(String, String)>,
        removals: Vec<(String, String)>,
    },
    QuarantineFile {
        path: String,
        threat_name: String,
    },
    RestoreQuarantine {
        quarantine_id: String,
    },
    DeleteQuarantine {
        quarantine_id: String,
        confirm_permanent_delete: bool,
    },
    ApplyProfile {
        profile_id: String,
    },
    InstallOptionalScanner,
    #[cfg(any(test, feature = "test-support"))]
    NoopTestAction {
        key: String,
        target_value: String,
        simulate_fail_step: Option<String>,
    },
}

fn contains_shell_metacharacters(value: &str) -> bool {
    value.chars().any(|c| {
        c.is_control()
            || matches!(
                c,
                ';' | '&' | '|' | '`' | '$' | '(' | ')' | '<' | '>' | '\\' | '"' | '\''
            )
    })
}

fn reject_shell_metacharacters(field: &str, value: &str) -> Result<(), AkrepError> {
    if contains_shell_metacharacters(value) {
        return Err(AkrepError::InvalidInput(format!(
            "Field '{}' contains forbidden shell metacharacters or control bytes",
            field
        )));
    }
    Ok(())
}

fn validate_safe_absolute_path(field: &str, path_str: &str) -> Result<(), AkrepError> {
    if path_str.is_empty() {
        return Err(AkrepError::InvalidInput(format!(
            "Field '{}' must not be empty",
            field
        )));
    }
    reject_shell_metacharacters(field, path_str)?;
    let path = Path::new(path_str);
    if !path.is_absolute() {
        return Err(AkrepError::InvalidInput(format!(
            "Field '{}' must be an absolute path",
            field
        )));
    }
    for comp in path.components() {
        match comp {
            Component::ParentDir | Component::CurDir => {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Path traversal ('..' or '.') is forbidden in '{}'",
                    field
                )));
            }
            _ => {}
        }
    }
    if path == Path::new("/") {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Root filesystem '/' is forbidden as target in '{}'",
            field
        )));
    }
    Ok(())
}

fn validate_identifier_ascii(
    field: &str,
    value: &str,
    max_len: usize,
    allow_dots: bool,
) -> Result<(), AkrepError> {
    if value.is_empty() || value.len() > max_len {
        return Err(AkrepError::InvalidInput(format!(
            "Field '{}' length must be in 1..={}",
            field, max_len
        )));
    }
    reject_shell_metacharacters(field, value)?;
    if value.contains("..") {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Field '{}' must not contain '..'",
            field
        )));
    }
    let valid = value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || (allow_dots && b == b'.'));
    if !valid {
        return Err(AkrepError::InvalidInput(format!(
            "Field '{}' contains invalid characters",
            field
        )));
    }
    Ok(())
}

fn validate_zone_name(zone: &str) -> Result<(), AkrepError> {
    validate_identifier_ascii("zone", zone, 64, false)
}

fn validate_persistence(mode: RulePersistence) -> Result<(), AkrepError> {
    if let RulePersistence::TimedRuntime { seconds } = mode {
        if seconds == 0 || seconds > 604_800 {
            return Err(AkrepError::InvalidInput(
                "TimedRuntime seconds must be within 1..=604800".to_string(),
            ));
        }
    }
    Ok(())
}

fn validate_flatpak_app_id(app_id: &str) -> Result<(), AkrepError> {
    let trimmed = app_id.trim();
    if trimmed.eq_ignore_ascii_case("global") {
        return Err(AkrepError::ForbiddenOperation(
            "Global Flatpak overrides ('global') are strictly forbidden".to_string(),
        ));
    }
    if trimmed.is_empty() || trimmed.len() > 255 {
        return Err(AkrepError::InvalidInput(
            "Flatpak app_id must be 1..=255 characters".to_string(),
        ));
    }
    reject_shell_metacharacters("app_id", trimmed)?;
    if trimmed.contains('/') || trimmed.contains("..") {
        return Err(AkrepError::ForbiddenOperation(
            "Flatpak app_id must not contain path separators or '..'".to_string(),
        ));
    }
    let segments: Vec<&str> = trimmed.split('.').collect();
    if segments.len() < 3 {
        return Err(AkrepError::InvalidInput(format!(
            "Flatpak app_id '{}' must have at least 3 dot-separated segments",
            trimmed
        )));
    }
    for seg in segments {
        if seg.is_empty()
            || !seg
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(AkrepError::InvalidInput(format!(
                "Invalid segment '{}' in Flatpak app_id '{}'",
                seg, trimmed
            )));
        }
    }
    Ok(())
}

fn is_forbidden_blanket_filesystem_grant(token: &str) -> bool {
    if token.starts_with('!') {
        return false;
    }
    let base = token
        .strip_suffix(":rw")
        .or_else(|| token.strip_suffix(":ro"))
        .or_else(|| token.strip_suffix(":create"))
        .unwrap_or(token)
        .trim_end_matches('/');

    matches!(
        base,
        "" | "/"
            | "~"
            | "host"
            | "host-os"
            | "host-etc"
            | "home"
            | "/home"
            | "/etc"
            | "/usr"
            | "/var"
            | "/root"
            | "/boot"
            | "/proc"
            | "/sys"
            | "/dev"
            | "xdg-run/flatpak"
            | "xdg-data/flatpak"
    )
}

impl ActionRequest {
    pub fn required_polkit_action(&self) -> &'static str {
        match self {
            Self::SelinuxSetBoolean { .. } | Self::SelinuxRestorecon { .. } => {
                POLKIT_SELINUX_MANAGE
            }
            Self::FirewallAddPort { .. }
            | Self::FirewallRemovePort { .. }
            | Self::FirewallAddService { .. }
            | Self::FirewallRemoveService { .. }
            | Self::FirewallBindInterface { .. } => POLKIT_FIREWALL_MANAGE,
            Self::FlatpakSetOverride { .. } => POLKIT_FLATPAK_SYSTEM_MANAGE,
            Self::QuarantineFile { .. }
            | Self::RestoreQuarantine { .. }
            | Self::DeleteQuarantine { .. } => POLKIT_QUARANTINE_MANAGE,
            Self::ApplyProfile { .. } => POLKIT_PROFILE_APPLY,
            Self::InstallOptionalScanner => POLKIT_PACKAGES_INSTALL,
            #[cfg(any(test, feature = "test-support"))]
            Self::NoopTestAction { .. } => POLKIT_PROFILE_APPLY,
        }
    }

    pub fn default_risk_level(&self) -> RiskLevel {
        match self {
            Self::SelinuxRestorecon { .. } => RiskLevel::Low,
            Self::SelinuxSetBoolean { persistent, .. } => {
                if *persistent {
                    RiskLevel::Medium
                } else {
                    RiskLevel::Low
                }
            }
            Self::FirewallAddPort {
                mode, source_cidr, ..
            }
            | Self::FirewallAddService {
                mode, source_cidr, ..
            } => match (mode, source_cidr) {
                (RulePersistence::Permanent, None) => RiskLevel::High,
                (RulePersistence::Permanent, Some(_)) => RiskLevel::Medium,
                _ => RiskLevel::Low,
            },
            Self::FirewallRemovePort { .. }
            | Self::FirewallRemoveService { .. }
            | Self::FirewallBindInterface { .. } => RiskLevel::Medium,
            Self::FlatpakSetOverride { scope, .. } => match scope {
                FlatpakScope::System => RiskLevel::Medium,
                FlatpakScope::User => RiskLevel::Low,
            },
            Self::QuarantineFile { .. } | Self::RestoreQuarantine { .. } => RiskLevel::Medium,
            Self::DeleteQuarantine { .. } => RiskLevel::High,
            Self::ApplyProfile { .. } => RiskLevel::Medium,
            Self::InstallOptionalScanner => RiskLevel::Medium,
            #[cfg(any(test, feature = "test-support"))]
            Self::NoopTestAction { .. } => RiskLevel::Low,
        }
    }

    pub fn is_test_only(&self) -> bool {
        #[cfg(any(test, feature = "test-support"))]
        {
            matches!(self, Self::NoopTestAction { .. })
        }
        #[cfg(not(any(test, feature = "test-support")))]
        {
            false
        }
    }

    pub fn validate_red_lines(&self) -> Result<(), AkrepError> {
        match self {
            Self::SelinuxSetBoolean { name, .. } => {
                let lower = name.to_ascii_lowercase();
                if lower.contains("selinux=disabled")
                    || lower == "disabled"
                    || lower.contains("audit2allow")
                    || lower.contains("setenforce")
                {
                    return Err(AkrepError::ForbiddenOperation(
                        "Disabling SELinux or invoking audit2allow is forbidden".to_string(),
                    ));
                }
                validate_identifier_ascii("boolean.name", name, 128, true)?;
                if !name
                    .as_bytes()
                    .first()
                    .map(|b| b.is_ascii_alphabetic())
                    .unwrap_or(false)
                {
                    return Err(AkrepError::InvalidInput(
                        "SELinux boolean name must start with an ASCII letter".to_string(),
                    ));
                }
                Ok(())
            }
            Self::SelinuxRestorecon { target_path, .. } => {
                let lower = target_path.to_ascii_lowercase();
                if lower.contains("audit2allow") || lower.contains("selinux=disabled") {
                    return Err(AkrepError::ForbiddenOperation(
                        "Forbidden SELinux directive in restorecon path".to_string(),
                    ));
                }
                validate_safe_absolute_path("target_path", target_path)?;
                for virtual_fs in ["/proc", "/sys", "/dev"] {
                    if target_path == virtual_fs
                        || target_path.starts_with(&format!("{}/", virtual_fs))
                    {
                        return Err(AkrepError::ForbiddenOperation(format!(
                            "restorecon on pseudo-filesystem '{}' is forbidden",
                            virtual_fs
                        )));
                    }
                }
                Ok(())
            }
            Self::FirewallAddPort {
                zone,
                port,
                mode,
                source_cidr,
                ..
            } => {
                validate_zone_name(zone)?;
                validate_port_spec(port)?;
                validate_persistence(*mode)?;
                if let Some(cidr) = source_cidr {
                    let _ = parse_and_validate_cidr(cidr)?;
                }
                Ok(())
            }
            Self::FirewallRemovePort {
                zone,
                port,
                source_cidr,
                ..
            } => {
                validate_zone_name(zone)?;
                validate_port_spec(port)?;
                if let Some(cidr) = source_cidr {
                    let _ = parse_and_validate_cidr(cidr)?;
                }
                Ok(())
            }
            Self::FirewallAddService {
                zone,
                service,
                mode,
                source_cidr,
            } => {
                validate_zone_name(zone)?;
                validate_service_name(service)?;
                validate_persistence(*mode)?;
                if let Some(cidr) = source_cidr {
                    let _ = parse_and_validate_cidr(cidr)?;
                }
                Ok(())
            }
            Self::FirewallRemoveService {
                zone,
                service,
                source_cidr,
                ..
            } => {
                validate_zone_name(zone)?;
                validate_service_name(service)?;
                if let Some(cidr) = source_cidr {
                    let _ = parse_and_validate_cidr(cidr)?;
                }
                Ok(())
            }
            Self::FirewallBindInterface {
                zone, interface, ..
            } => {
                validate_zone_name(zone)?;
                if interface.is_empty() || interface.len() > 15 {
                    return Err(AkrepError::InvalidInput(
                        "Network interface name must be 1..=15 characters".to_string(),
                    ));
                }
                reject_shell_metacharacters("interface", interface)?;
                if interface.contains('/') || interface.contains("..") {
                    return Err(AkrepError::ForbiddenOperation(
                        "Invalid characters in network interface name".to_string(),
                    ));
                }
                if !interface
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'))
                {
                    return Err(AkrepError::InvalidInput(
                        "Invalid network interface identifier".to_string(),
                    ));
                }
                Ok(())
            }
            Self::FlatpakSetOverride {
                app_id,
                additions,
                removals,
                ..
            } => {
                validate_flatpak_app_id(app_id)?;
                if additions.is_empty() && removals.is_empty() {
                    return Err(AkrepError::InvalidInput(
                        "FlatpakSetOverride requires at least one addition or removal".to_string(),
                    ));
                }

                for (spec, tok) in additions {
                    reject_shell_metacharacters("flatpak.addition.key", spec)?;
                    reject_shell_metacharacters("flatpak.addition.token", tok)?;
                    if tok.contains("..") {
                        return Err(AkrepError::ForbiddenOperation(
                            "Path traversal '..' is forbidden in Flatpak override token"
                                .to_string(),
                        ));
                    }
                    let (sec, key) = resolve_override_spec(spec)?;
                    let trimmed_tok = tok.trim();
                    if sec == "Context"
                        && key == "filesystems"
                        && is_forbidden_blanket_filesystem_grant(trimmed_tok)
                    {
                        return Err(AkrepError::ForbiddenOperation(format!(
                            "Blanket Flatpak filesystem grant '{}' is forbidden",
                            trimmed_tok
                        )));
                    }
                    if sec == "Context"
                        && key == "devices"
                        && !trimmed_tok.starts_with('!')
                        && trimmed_tok == "all"
                    {
                        return Err(AkrepError::ForbiddenOperation(
                            "Blanket Flatpak device grant 'devices=all' is forbidden".to_string(),
                        ));
                    }
                    if sec == "Session Bus Policy"
                        && (key == "org.freedesktop.Flatpak"
                            || key == "org.freedesktop.impl.portal.PermissionStore")
                        && matches!(trimmed_tok, "talk" | "own")
                    {
                        return Err(AkrepError::ForbiddenOperation(format!(
                            "Sandbox-escape D-Bus grant '{}={}' is forbidden",
                            key, trimmed_tok
                        )));
                    }
                }

                for (spec, tok) in removals {
                    reject_shell_metacharacters("flatpak.removal.key", spec)?;
                    reject_shell_metacharacters("flatpak.removal.token", tok)?;
                    if tok.contains("..") {
                        return Err(AkrepError::ForbiddenOperation(
                            "Path traversal '..' is forbidden in Flatpak removal token".to_string(),
                        ));
                    }
                    let _ = resolve_override_spec(spec)?;
                }
                Ok(())
            }
            Self::QuarantineFile { path, threat_name } => {
                validate_safe_absolute_path("quarantine.path", path)?;
                for protected_prefix in [
                    "/usr",
                    "/bin",
                    "/sbin",
                    "/lib",
                    "/lib64",
                    "/etc",
                    "/boot",
                    "/proc",
                    "/sys",
                    "/dev",
                    "/run",
                    "/var/lib/akrep",
                ] {
                    if path == protected_prefix
                        || path.starts_with(&format!("{}/", protected_prefix))
                    {
                        return Err(AkrepError::ForbiddenOperation(format!(
                            "Quarantining protected system path '{}' is forbidden",
                            path
                        )));
                    }
                }
                if threat_name.trim().is_empty() || threat_name.len() > 128 {
                    return Err(AkrepError::InvalidInput(
                        "threat_name must be 1..=128 characters".to_string(),
                    ));
                }
                reject_shell_metacharacters("threat_name", threat_name)?;
                Ok(())
            }
            Self::RestoreQuarantine { quarantine_id } => {
                validate_identifier_ascii("quarantine_id", quarantine_id, 128, false)
            }
            Self::DeleteQuarantine {
                quarantine_id,
                confirm_permanent_delete,
            } => {
                validate_identifier_ascii("quarantine_id", quarantine_id, 128, false)?;
                if !confirm_permanent_delete {
                    return Err(AkrepError::ForbiddenOperation(
                        "Permanent quarantine deletion requires confirm_permanent_delete=true"
                            .to_string(),
                    ));
                }
                Ok(())
            }
            Self::ApplyProfile { profile_id } => {
                let lower = profile_id.to_ascii_lowercase();
                if lower.contains("disabled") || lower.contains("audit2allow") {
                    return Err(AkrepError::ForbiddenOperation(
                        "Forbidden profile identifier".to_string(),
                    ));
                }
                validate_identifier_ascii("profile_id", profile_id, 64, false)
            }
            Self::InstallOptionalScanner => Ok(()),
            #[cfg(any(test, feature = "test-support"))]
            Self::NoopTestAction {
                key,
                target_value,
                simulate_fail_step,
            } => {
                validate_identifier_ascii("noop.key", key, 64, true)?;
                reject_shell_metacharacters("noop.target_value", target_value)?;
                if let Some(step) = simulate_fail_step {
                    validate_identifier_ascii("noop.simulate_fail_step", step, 32, false)?;
                }
                Ok(())
            }
        }
    }
}

#[derive(Serialize)]
struct CanonicalPlanPayload<'a> {
    plan_id: &'a str,
    created_by_uid: u32,
    polkit_action: &'a str,
    request: &'a ActionRequest,
    risk: RiskLevel,
    diffs: &'a [PlanDiffItem],
    pre_fingerprints: &'a [StateFingerprint],
    warnings_tr: &'a [String],
    warnings_en: &'a [String],
    created_at: &'a str,
    expires_at: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPlan {
    pub plan_id: String,
    pub plan_sha256: String,
    pub created_by_uid: u32,
    pub polkit_action: String,
    pub request: ActionRequest,
    pub risk: RiskLevel,
    pub diffs: Vec<PlanDiffItem>,
    pub pre_fingerprints: Vec<StateFingerprint>,
    pub warnings_tr: Vec<String>,
    pub warnings_en: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
}

impl ActionPlan {
    #[allow(clippy::too_many_arguments)]
    pub fn new_signed(
        created_by_uid: u32,
        request: ActionRequest,
        risk: RiskLevel,
        diffs: Vec<PlanDiffItem>,
        pre_fingerprints: Vec<StateFingerprint>,
        warnings_tr: Vec<String>,
        warnings_en: Vec<String>,
    ) -> Result<Self, AkrepError> {
        let plan_id = generate_uuid_v4()?;
        let now = Utc::now();
        let created_at = now.to_rfc3339();
        let expires_at = (now + Duration::seconds(DEFAULT_PLAN_TTL_SECS)).to_rfc3339();
        let polkit_action = request.required_polkit_action().to_string();

        let mut plan = Self {
            plan_id,
            plan_sha256: String::new(),
            created_by_uid,
            polkit_action,
            request,
            risk,
            diffs,
            pre_fingerprints,
            warnings_tr,
            warnings_en,
            created_at,
            expires_at,
        };
        plan.plan_sha256 = plan.compute_canonical_sha256()?;
        Ok(plan)
    }

    pub fn compute_canonical_sha256(&self) -> Result<String, AkrepError> {
        let canonical = CanonicalPlanPayload {
            plan_id: &self.plan_id,
            created_by_uid: self.created_by_uid,
            polkit_action: &self.polkit_action,
            request: &self.request,
            risk: self.risk,
            diffs: &self.diffs,
            pre_fingerprints: &self.pre_fingerprints,
            warnings_tr: &self.warnings_tr,
            warnings_en: &self.warnings_en,
            created_at: &self.created_at,
            expires_at: &self.expires_at,
        };
        let bytes = serde_json::to_vec(&canonical)
            .map_err(|e| AkrepError::Internal(format!("canonical plan encode failed: {}", e)))?;
        Ok(sha256_hex(&bytes))
    }

    pub fn verify_hash(&self, expected_sha256: &str) -> Result<(), AkrepError> {
        let canonical = self.compute_canonical_sha256()?;
        if self.plan_sha256 != canonical || expected_sha256 != canonical {
            return Err(AkrepError::PlanMismatch(
                "ActionPlan SHA-256 digest does not match canonical payload".to_string(),
            ));
        }
        Ok(())
    }
}

pub fn generate_uuid_v4() -> Result<String, AkrepError> {
    let mut b = [0u8; 16];
    let mut f = File::open("/dev/urandom")?;
    f.read_exact(&mut b)?;
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tx_10_arbitrary_or_redline_inputs_rejected() {
        let traversal = ActionRequest::SelinuxRestorecon {
            target_path: "../etc/shadow".to_string(),
            recursive: false,
        };
        assert!(traversal.validate_red_lines().is_err());

        let abs_traversal = ActionRequest::QuarantineFile {
            path: "/tmp/../etc/shadow".to_string(),
            threat_name: "EICAR".to_string(),
        };
        assert!(abs_traversal.validate_red_lines().is_err());

        let shell_meta = ActionRequest::SelinuxSetBoolean {
            name: "httpd_can_network_connect; rm -rf /".to_string(),
            value: true,
            persistent: false,
        };
        assert!(shell_meta.validate_red_lines().is_err());

        let selinux_disabled = ActionRequest::SelinuxSetBoolean {
            name: "SELINUX=disabled".to_string(),
            value: true,
            persistent: true,
        };
        assert!(selinux_disabled.validate_red_lines().is_err());

        let audit2allow = ActionRequest::SelinuxSetBoolean {
            name: "audit2allow_custom".to_string(),
            value: true,
            persistent: false,
        };
        assert!(audit2allow.validate_red_lines().is_err());

        let blanket_home = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("Context.filesystems".to_string(), "/home".to_string())],
            removals: vec![],
        };
        assert!(blanket_home.validate_red_lines().is_err());

        let blanket_host = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("filesystems".to_string(), "host".to_string())],
            removals: vec![],
        };
        assert!(blanket_host.validate_red_lines().is_err());

        let blanket_devices = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("devices".to_string(), "all".to_string())],
            removals: vec![],
        };
        assert!(blanket_devices.validate_red_lines().is_err());

        let global_flatpak = ActionRequest::FlatpakSetOverride {
            app_id: "global".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("filesystems".to_string(), "!home".to_string())],
            removals: vec![],
        };
        assert!(global_flatpak.validate_red_lines().is_err());

        let port_zero = ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "0".to_string(),
            proto: FirewallProto::Tcp,
            mode: RulePersistence::Runtime,
            source_cidr: None,
        };
        assert!(port_zero.validate_red_lines().is_err());

        let port_high = ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "70000".to_string(),
            proto: FirewallProto::Udp,
            mode: RulePersistence::Runtime,
            source_cidr: None,
        };
        assert!(port_high.validate_red_lines().is_err());

        let unconfirmed_delete = ActionRequest::DeleteQuarantine {
            quarantine_id: "q_001".to_string(),
            confirm_permanent_delete: false,
        };
        assert!(unconfirmed_delete.validate_red_lines().is_err());

        // Tightening / removing broad permissions MUST succeed
        let valid_tightening = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("Context.filesystems".to_string(), "!home".to_string())],
            removals: vec![("Context.filesystems".to_string(), "host".to_string())],
        };
        assert!(valid_tightening.validate_red_lines().is_ok());
    }
}
