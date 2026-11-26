use crate::errors::AkrepError;
use crate::ini_override::resolve_override_spec;
use crate::protocol::{
    DEFAULT_PLAN_TTL_SECS, POLKIT_FIREWALL_MANAGE, POLKIT_FLATPAK_SYSTEM_MANAGE,
    POLKIT_PACKAGES_INSTALL, POLKIT_PROFILE_APPLY, POLKIT_QUARANTINE_MANAGE, POLKIT_SELINUX_MANAGE,
    POLKIT_SYSTEM_COMPAT_MANAGE,
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

/// Protection subsystem supporting timed or manual overrides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideSubsystem {
    Selinux,
    Firewalld,
    Oomd,
}

impl OverrideSubsystem {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Selinux => "selinux",
            Self::Firewalld => "firewalld",
            Self::Oomd => "oomd",
        }
    }

    pub fn from_str_id(s: &str) -> Option<Self> {
        match s {
            "selinux" => Some(Self::Selinux),
            "firewalld" => Some(Self::Firewalld),
            "oomd" => Some(Self::Oomd),
            _ => None,
        }
    }
}

/// Duration policy for temporarily or manually relaxing a protection subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OverrideDuration {
    /// Automatically revert after `minutes` via transient systemd timer (validated: 1..=1440).
    Timed { minutes: u32 },
    /// Runtime-only; cleared automatically on next reboot (verified via kernel boot_id).
    UntilReboot,
    /// Persistent across reboots until the user explicitly re-enables protection.
    UntilChanged,
}

/// Allowed target SELinux modes (`disabled` is intentionally absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelinuxDesiredMode {
    Enforcing,
    Permissive,
}

/// Fixed allowlisted compatibility package groups for Android/ROM workflows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompatPackageGroup {
    AndroidUdevRules,
    AndroidLegacyLibs,
}

impl CompatPackageGroup {
    pub fn rpm_packages(&self) -> &'static [&'static str] {
        match self {
            Self::AndroidUdevRules => &["android-udev-rules"],
            Self::AndroidLegacyLibs => &["ncurses-compat-libs"],
        }
    }
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
    SelinuxSetMode {
        mode: SelinuxDesiredMode,
        duration: OverrideDuration,
    },
    FirewallSetState {
        enabled: bool,
        duration: OverrideDuration,
    },
    OomdSetState {
        enabled: bool,
        duration: OverrideDuration,
    },
    DevLimitsSetNofile {
        enabled: bool,
        soft_limit: u32,
        hard_limit: u32,
    },
    SelinuxLabelContainerDir {
        target_path: String,
        recursive: bool,
    },
    InstallCompatibilityPackage {
        package_group: CompatPackageGroup,
    },
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

pub fn validate_zone_name(zone: &str) -> Result<(), AkrepError> {
    validate_identifier_ascii("zone", zone, 64, false)
}

pub fn validate_persistence(mode: RulePersistence) -> Result<(), AkrepError> {
    if let RulePersistence::TimedRuntime { seconds } = mode {
        if seconds == 0 || seconds > 86_400 {
            return Err(AkrepError::InvalidInput(
                "TimedRuntime seconds must be within 1..=86400".to_string(),
            ));
        }
    }
    Ok(())
}

pub fn validate_selinux_boolean_name(name: &str) -> Result<(), AkrepError> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("selinux=disabled")
        || lower == "disabled"
        || lower.contains("audit2allow")
        || lower.contains("setenforce")
        || lower.contains("semodule")
    {
        return Err(AkrepError::ForbiddenOperation(
            "Disabling SELinux or invoking audit2allow/semodule is forbidden".to_string(),
        ));
    }
    reject_shell_metacharacters("boolean.name", name)?;
    if name.contains("..") {
        return Err(AkrepError::ForbiddenOperation(
            "Field 'boolean.name' must not contain '..'".to_string(),
        ));
    }
    if !(2..=96).contains(&name.len()) {
        return Err(AkrepError::InvalidInput(
            "SELinux boolean name length must be in 2..=96".to_string(),
        ));
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_lowercase()
        || !bytes[1..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'_')
    {
        return Err(AkrepError::InvalidInput(format!(
            "Invalid SELinux boolean name '{}' (expected ^[a-z][a-z0-9_]{{1,95}}$)",
            name
        )));
    }
    Ok(())
}

pub fn validate_restorecon_path(target_path: &str) -> Result<(), AkrepError> {
    let lower = target_path.to_ascii_lowercase();
    if lower.contains("audit2allow")
        || lower.contains("selinux=disabled")
        || lower.contains("semodule")
        || lower.contains("setenforce")
    {
        return Err(AkrepError::ForbiddenOperation(
            "Forbidden SELinux directive in restorecon path".to_string(),
        ));
    }
    validate_safe_absolute_path("target_path", target_path)?;
    for protected_prefix in [
        "/proc",
        "/sys",
        "/dev",
        "/run",
        "/boot",
        "/var/lib/akrep",
        "/etc/sudoers.d",
    ] {
        if target_path == protected_prefix
            || target_path.starts_with(&format!("{}/", protected_prefix))
        {
            return Err(AkrepError::ForbiddenOperation(format!(
                "restorecon on protected path '{}' is forbidden",
                protected_prefix
            )));
        }
    }
    if matches!(
        target_path,
        "/etc/shadow" | "/etc/gshadow" | "/etc/sudoers" | "/etc/selinux/config"
    ) {
        return Err(AkrepError::ForbiddenOperation(format!(
            "restorecon on sensitive security file '{}' is forbidden",
            target_path
        )));
    }
    Ok(())
}

pub fn validate_network_interface(interface: &str) -> Result<(), AkrepError> {
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
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(AkrepError::InvalidInput(
            "Invalid network interface identifier (expected ^[a-zA-Z0-9._-]+$)".to_string(),
        ));
    }
    Ok(())
}

pub fn validate_flatpak_app_id(app_id: &str) -> Result<(), AkrepError> {
    let trimmed = app_id.trim();
    if trimmed != app_id {
        return Err(AkrepError::InvalidInput(
            "Flatpak app_id must not contain leading or trailing whitespace".to_string(),
        ));
    }
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
    if !trimmed.as_bytes()[0].is_ascii_alphabetic() {
        return Err(AkrepError::InvalidInput(format!(
            "Flatpak app_id '{}' must start with an ASCII letter",
            trimmed
        )));
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

fn normalize_flatpak_fs_base(raw: &str) -> String {
    let mut collapsed = String::with_capacity(raw.len());
    let mut prev_slash = false;
    for ch in raw.chars() {
        if ch == '/' {
            if !prev_slash {
                collapsed.push('/');
            }
            prev_slash = true;
        } else {
            collapsed.push(ch);
            prev_slash = false;
        }
    }
    let stripped = collapsed.trim_end_matches('/');
    if stripped.is_empty() && collapsed.starts_with('/') {
        "/".to_string()
    } else {
        stripped.to_string()
    }
}

pub fn is_forbidden_blanket_filesystem_grant(token: &str) -> bool {
    let trimmed = token.trim();
    if trimmed.starts_with('!') {
        return false;
    }
    let raw_base = trimmed.split(':').next().unwrap_or(trimmed).trim();
    if raw_base.is_empty() {
        return true;
    }
    let normalized = normalize_flatpak_fs_base(raw_base);
    let base = normalized.as_str();

    if matches!(
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
            | "/var/lib/akrep"
            | "/var/lib/flatpak"
            | "xdg-run/flatpak"
            | "xdg-data/flatpak"
            | "xdg-data/akrep"
            | "~/.local/share/flatpak"
            | "~/.local/share/akrep"
            | "home/.local/share/flatpak"
            | "home/.local/share/akrep"
    ) {
        return true;
    }

    if base.strip_prefix("/root").is_some_and(|s| s.starts_with('/')) {
        return true;
    }

    for sys_prefix in [
        "/etc/",
        "/usr/",
        "/boot/",
        "/proc/",
        "/sys/",
        "/dev/",
        "/var/lib/akrep/",
        "/var/lib/flatpak/",
        "xdg-data/flatpak/",
        "xdg-run/flatpak/",
        "xdg-data/akrep/",
        "~/.local/share/flatpak/",
        "~/.local/share/akrep/",
        "home/.local/share/flatpak/",
        "home/.local/share/akrep/",
    ] {
        if base.starts_with(sys_prefix) {
            return true;
        }
    }

    false
}

fn bus_pattern_matches(pattern: &str, target: &str) -> bool {
    if pattern == "*" || pattern == target {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return target == prefix || target.starts_with(&format!("{}.", prefix));
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return target.starts_with(prefix);
    }
    false
}

pub fn is_forbidden_bus_policy_grant(section: &str, bus_name: &str, policy: &str) -> bool {
    let norm_policy = policy.trim().to_ascii_lowercase();
    if !matches!(norm_policy.as_str(), "talk" | "own") {
        return false;
    }

    const FORBIDDEN_SESSION_BUSES: &[&str] = &[
        "org.freedesktop.Flatpak",
        "org.freedesktop.impl.portal.PermissionStore",
        "org.freedesktop.secrets",
        "tr.com.blazeos.Akrep1",
        "tr.com.blazeos.Akrep1.Privileged",
    ];

    const FORBIDDEN_SYSTEM_BUSES: &[&str] = &[
        "org.freedesktop.PolicyKit1",
        "org.freedesktop.systemd1",
        "org.freedesktop.login1",
        "org.freedesktop.Flatpak",
        "tr.com.blazeos.Akrep1.Privileged",
        "tr.com.blazeos.Akrep1",
    ];

    match section {
        "Session Bus Policy" => FORBIDDEN_SESSION_BUSES
            .iter()
            .any(|b| bus_pattern_matches(bus_name, b)),
        "System Bus Policy" => FORBIDDEN_SYSTEM_BUSES
            .iter()
            .any(|b| bus_pattern_matches(bus_name, b)),
        _ => false,
    }
}

pub fn validate_flatpak_override_changes(
    additions: &[(String, String)],
    removals: &[(String, String)],
) -> Result<(), AkrepError> {
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
                "Path traversal '..' is forbidden in Flatpak override token".to_string(),
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
        let dev_base = trimmed_tok.split(':').next().unwrap_or(trimmed_tok).trim();
        if sec == "Context"
            && key == "devices"
            && !trimmed_tok.starts_with('!')
            && dev_base.eq_ignore_ascii_case("all")
        {
            return Err(AkrepError::ForbiddenOperation(
                "Blanket Flatpak device grant 'devices=all' is forbidden".to_string(),
            ));
        }
        if is_forbidden_bus_policy_grant(&sec, &key, trimmed_tok) {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Sandbox-escape D-Bus grant '[{}].{}={}' is forbidden",
                sec, key, trimmed_tok
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

/// Validates that timed override durations stay within 1..=1440 minutes.
pub fn validate_override_duration(duration: OverrideDuration) -> Result<(), AkrepError> {
    if let OverrideDuration::Timed { minutes } = duration {
        if minutes == 0 || minutes > 1_440 {
            return Err(AkrepError::InvalidInput(
                "Timed override duration minutes must be within 1..=1440".to_string(),
            ));
        }
    }
    Ok(())
}

/// Validates developer `nofile` limits (`1024 <= soft_limit <= hard_limit <= 1_048_576`).
pub fn validate_nofile_limits(soft_limit: u32, hard_limit: u32) -> Result<(), AkrepError> {
    if soft_limit < 1_024 {
        return Err(AkrepError::InvalidInput(
            "nofile soft_limit must be at least 1024".to_string(),
        ));
    }
    if hard_limit > 1_048_576 {
        return Err(AkrepError::InvalidInput(
            "nofile hard_limit must not exceed 1048576".to_string(),
        ));
    }
    if soft_limit > hard_limit {
        return Err(AkrepError::InvalidInput(
            "nofile soft_limit must not exceed hard_limit".to_string(),
        ));
    }
    Ok(())
}

/// Validates and normalizes a container/ROM workspace directory path to user-owned subfolders.
pub fn validate_container_dir_path(path: &str) -> Result<String, AkrepError> {
    if path.is_empty() || path.len() > 4_096 {
        return Err(AkrepError::InvalidInput(
            "Container directory path length must be in 1..=4096".to_string(),
        ));
    }
    if path.trim() != path {
        return Err(AkrepError::InvalidInput(
            "Container directory path must not contain leading or trailing whitespace"
                .to_string(),
        ));
    }
    let lower = path.to_ascii_lowercase();
    if lower.contains("audit2allow")
        || lower.contains("selinux=disabled")
        || lower.contains("semodule")
        || lower.contains("setenforce")
    {
        return Err(AkrepError::ForbiddenOperation(
            "Forbidden SELinux directive in container directory path".to_string(),
        ));
    }
    validate_safe_absolute_path("target_path", path)?;
    if path.split('/').any(|seg| seg == "." || seg == "..") {
        return Err(AkrepError::ForbiddenOperation(
            "Path traversal ('.' or '..') is forbidden in container directory path".to_string(),
        ));
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.iter().any(|s| s.trim().is_empty()) {
        return Err(AkrepError::InvalidInput(
            "Container directory path contains whitespace-only path segment".to_string(),
        ));
    }
    let allowed = match segments.as_slice() {
        ["home", user, _, ..] if !user.is_empty() => true,
        ["mnt", _, ..] => true,
        ["run", "media", user, _, ..] if !user.is_empty() => true,
        _ => false,
    };
    if !allowed {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Container directory '{}' must be a subfolder under /home/<user>/..., /mnt/..., or /run/media/<user>/...",
            path
        )));
    }
    Ok(format!("/{}", segments.join("/")))
}

impl ActionRequest {
    pub fn required_polkit_action(&self) -> &'static str {
        match self {
            Self::SelinuxSetBoolean { .. }
            | Self::SelinuxRestorecon { .. }
            | Self::SelinuxSetMode { .. }
            | Self::SelinuxLabelContainerDir { .. } => POLKIT_SELINUX_MANAGE,
            Self::FirewallAddPort { .. }
            | Self::FirewallRemovePort { .. }
            | Self::FirewallAddService { .. }
            | Self::FirewallRemoveService { .. }
            | Self::FirewallBindInterface { .. }
            | Self::FirewallSetState { .. } => POLKIT_FIREWALL_MANAGE,
            Self::OomdSetState { .. } | Self::DevLimitsSetNofile { .. } => {
                POLKIT_SYSTEM_COMPAT_MANAGE
            }
            Self::FlatpakSetOverride { .. } => POLKIT_FLATPAK_SYSTEM_MANAGE,
            Self::QuarantineFile { .. }
            | Self::RestoreQuarantine { .. }
            | Self::DeleteQuarantine { .. } => POLKIT_QUARANTINE_MANAGE,
            Self::ApplyProfile { .. } => POLKIT_PROFILE_APPLY,
            Self::InstallOptionalScanner | Self::InstallCompatibilityPackage { .. } => {
                POLKIT_PACKAGES_INSTALL
            }
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
            Self::SelinuxSetMode { mode, duration } => match (mode, duration) {
                (SelinuxDesiredMode::Enforcing, _) => RiskLevel::Low,
                (SelinuxDesiredMode::Permissive, OverrideDuration::UntilChanged) => RiskLevel::High,
                (SelinuxDesiredMode::Permissive, _) => RiskLevel::Medium,
            },
            Self::SelinuxLabelContainerDir { .. } => RiskLevel::Medium,
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
            Self::FirewallSetState { enabled, duration } => {
                if *enabled {
                    RiskLevel::Low
                } else if matches!(duration, OverrideDuration::UntilChanged) {
                    RiskLevel::High
                } else {
                    RiskLevel::Medium
                }
            }
            Self::OomdSetState { enabled, .. } => {
                if *enabled {
                    RiskLevel::Low
                } else {
                    RiskLevel::Medium
                }
            }
            Self::DevLimitsSetNofile { .. } => RiskLevel::Low,
            Self::FlatpakSetOverride { scope, .. } => match scope {
                FlatpakScope::System => RiskLevel::Medium,
                FlatpakScope::User => RiskLevel::Low,
            },
            Self::QuarantineFile { .. } | Self::RestoreQuarantine { .. } => RiskLevel::Medium,
            Self::DeleteQuarantine { .. } => RiskLevel::High,
            Self::ApplyProfile { .. } => RiskLevel::Medium,
            Self::InstallOptionalScanner => RiskLevel::Medium,
            Self::InstallCompatibilityPackage { .. } => RiskLevel::Low,
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
            Self::SelinuxSetBoolean { name, .. } => validate_selinux_boolean_name(name),
            Self::SelinuxRestorecon { target_path, .. } => validate_restorecon_path(target_path),
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
                validate_network_interface(interface)
            }
            Self::FlatpakSetOverride {
                app_id,
                additions,
                removals,
                ..
            } => {
                validate_flatpak_app_id(app_id)?;
                validate_flatpak_override_changes(additions, removals)
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
            Self::SelinuxSetMode { duration, .. }
            | Self::FirewallSetState { duration, .. }
            | Self::OomdSetState { duration, .. } => validate_override_duration(*duration),
            Self::DevLimitsSetNofile {
                soft_limit,
                hard_limit,
                ..
            } => validate_nofile_limits(*soft_limit, *hard_limit),
            Self::SelinuxLabelContainerDir { target_path, .. } => {
                validate_container_dir_path(target_path).map(|_| ())
            }
            Self::InstallCompatibilityPackage { package_group } => {
                if package_group.rpm_packages().is_empty() {
                    Err(AkrepError::InvalidInput(
                        "CompatPackageGroup has no RPM packages".to_string(),
                    ))
                } else {
                    Ok(())
                }
            }
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

        let bracketed_spaced_host = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("[ Context ].filesystems".to_string(), "host".to_string())],
            removals: vec![],
        };
        assert!(bracketed_spaced_host.validate_red_lines().is_err());

        let env_ld_preload = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![(
                "[Environment].LD_PRELOAD".to_string(),
                "/tmp/x.so".to_string(),
            )],
            removals: vec![],
        };
        assert!(env_ld_preload.validate_red_lines().is_err());

        let chained_suffix_host = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("filesystems".to_string(), "host:ro:rw".to_string())],
            removals: vec![],
        };
        assert!(chained_suffix_host.validate_red_lines().is_err());

        let chained_suffix_home = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("filesystems".to_string(), "/home:ro:rw".to_string())],
            removals: vec![],
        };
        assert!(chained_suffix_home.validate_red_lines().is_err());

        let system_bus_polkit = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![(
                "[System Bus Policy].org.freedesktop.PolicyKit1".to_string(),
                "talk".to_string(),
            )],
            removals: vec![],
        };
        assert!(system_bus_polkit.validate_red_lines().is_err());

        let system_bus_akrep_priv = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![(
                "System Bus Policy.tr.com.blazeos.Akrep1.Privileged".to_string(),
                "own".to_string(),
            )],
            removals: vec![],
        };
        assert!(system_bus_akrep_priv.validate_red_lines().is_err());

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

        // Sandbox-escape Flatpak override subpaths MUST be rejected
        for escape_fs in [
            "xdg-data/flatpak/overrides:rw",
            "xdg-run/flatpak/app:rw",
            "~/.local/share/flatpak/overrides:create",
            "home/.local/share/flatpak:rw",
            "home/.local/share/flatpak/overrides:rw",
            "/var/lib/flatpak:rw",
            "/var/lib/flatpak/overrides:rw",
            "xdg-data//flatpak///overrides/:rw",
            "~///.local//share/flatpak/overrides:rw",
        ] {
            let bad_escape = ActionRequest::FlatpakSetOverride {
                app_id: "org.example.ChatApp".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.filesystems".to_string(), escape_fs.to_string())],
                removals: vec![],
            };
            assert!(
                bad_escape.validate_red_lines().is_err(),
                "Expected sandbox-escape token '{}' to be rejected",
                escape_fs
            );
        }

        // Tightening / removing broad permissions MUST succeed
        let valid_tightening = ActionRequest::FlatpakSetOverride {
            app_id: "org.example.ChatApp".to_string(),
            scope: FlatpakScope::System,
            additions: vec![("Context.filesystems".to_string(), "!home".to_string())],
            removals: vec![("Context.filesystems".to_string(), "host".to_string())],
        };
        assert!(valid_tightening.validate_red_lines().is_ok());
    }

    #[test]
    fn test_new_flexible_action_requests_validation_and_polkit() {
        assert_eq!(OverrideSubsystem::Selinux.as_str(), "selinux");
        assert_eq!(OverrideSubsystem::Firewalld.as_str(), "firewalld");
        assert_eq!(OverrideSubsystem::Oomd.as_str(), "oomd");
        assert_eq!(
            OverrideSubsystem::from_str_id("selinux"),
            Some(OverrideSubsystem::Selinux)
        );
        assert_eq!(
            OverrideSubsystem::from_str_id("firewalld"),
            Some(OverrideSubsystem::Firewalld)
        );
        assert_eq!(
            OverrideSubsystem::from_str_id("oomd"),
            Some(OverrideSubsystem::Oomd)
        );
        assert_eq!(OverrideSubsystem::from_str_id("unknown"), None);

        assert_eq!(
            CompatPackageGroup::AndroidUdevRules.rpm_packages(),
            &["android-udev-rules"]
        );
        assert_eq!(
            CompatPackageGroup::AndroidLegacyLibs.rpm_packages(),
            &["ncurses-compat-libs"]
        );

        assert!(crate::protocol::ALL_POLKIT_ACTIONS.contains(&POLKIT_SYSTEM_COMPAT_MANAGE));

        let selinux_enforce = ActionRequest::SelinuxSetMode {
            mode: SelinuxDesiredMode::Enforcing,
            duration: OverrideDuration::UntilChanged,
        };
        assert!(selinux_enforce.validate_red_lines().is_ok());
        assert_eq!(
            selinux_enforce.required_polkit_action(),
            "tr.com.blazeos.Akrep1.selinux.manage"
        );
        assert_eq!(selinux_enforce.default_risk_level(), RiskLevel::Low);

        let selinux_timed = ActionRequest::SelinuxSetMode {
            mode: SelinuxDesiredMode::Permissive,
            duration: OverrideDuration::Timed { minutes: 15 },
        };
        assert!(selinux_timed.validate_red_lines().is_ok());
        assert_eq!(
            selinux_timed.required_polkit_action(),
            "tr.com.blazeos.Akrep1.selinux.manage"
        );
        assert_eq!(selinux_timed.default_risk_level(), RiskLevel::Medium);

        let selinux_until_changed = ActionRequest::SelinuxSetMode {
            mode: SelinuxDesiredMode::Permissive,
            duration: OverrideDuration::UntilChanged,
        };
        assert!(selinux_until_changed.validate_red_lines().is_ok());
        assert_eq!(
            selinux_until_changed.required_polkit_action(),
            "tr.com.blazeos.Akrep1.selinux.manage"
        );
        assert_eq!(selinux_until_changed.default_risk_level(), RiskLevel::High);

        let fw_enable = ActionRequest::FirewallSetState {
            enabled: true,
            duration: OverrideDuration::UntilChanged,
        };
        assert!(fw_enable.validate_red_lines().is_ok());
        assert_eq!(
            fw_enable.required_polkit_action(),
            "tr.com.blazeos.Akrep1.firewall.manage"
        );
        assert_eq!(fw_enable.default_risk_level(), RiskLevel::Low);

        let fw_timed = ActionRequest::FirewallSetState {
            enabled: false,
            duration: OverrideDuration::Timed { minutes: 60 },
        };
        assert!(fw_timed.validate_red_lines().is_ok());
        assert_eq!(
            fw_timed.required_polkit_action(),
            "tr.com.blazeos.Akrep1.firewall.manage"
        );
        assert_eq!(fw_timed.default_risk_level(), RiskLevel::Medium);

        let fw_until_changed = ActionRequest::FirewallSetState {
            enabled: false,
            duration: OverrideDuration::UntilChanged,
        };
        assert!(fw_until_changed.validate_red_lines().is_ok());
        assert_eq!(
            fw_until_changed.required_polkit_action(),
            "tr.com.blazeos.Akrep1.firewall.manage"
        );
        assert_eq!(fw_until_changed.default_risk_level(), RiskLevel::High);

        let oomd_enable = ActionRequest::OomdSetState {
            enabled: true,
            duration: OverrideDuration::UntilChanged,
        };
        assert!(oomd_enable.validate_red_lines().is_ok());
        assert_eq!(
            oomd_enable.required_polkit_action(),
            "tr.com.blazeos.Akrep1.system.compat-manage"
        );
        assert_eq!(oomd_enable.default_risk_level(), RiskLevel::Low);

        let oomd_pause = ActionRequest::OomdSetState {
            enabled: false,
            duration: OverrideDuration::UntilReboot,
        };
        assert!(oomd_pause.validate_red_lines().is_ok());
        assert_eq!(
            oomd_pause.required_polkit_action(),
            "tr.com.blazeos.Akrep1.system.compat-manage"
        );
        assert_eq!(oomd_pause.default_risk_level(), RiskLevel::Medium);

        let nofile = ActionRequest::DevLimitsSetNofile {
            enabled: true,
            soft_limit: 65536,
            hard_limit: 524288,
        };
        assert!(nofile.validate_red_lines().is_ok());
        assert_eq!(
            nofile.required_polkit_action(),
            "tr.com.blazeos.Akrep1.system.compat-manage"
        );
        assert_eq!(nofile.default_risk_level(), RiskLevel::Low);

        let container_dir = ActionRequest::SelinuxLabelContainerDir {
            target_path: "/home/alice/android".to_string(),
            recursive: true,
        };
        assert!(container_dir.validate_red_lines().is_ok());
        assert_eq!(
            container_dir.required_polkit_action(),
            "tr.com.blazeos.Akrep1.selinux.manage"
        );
        assert_eq!(container_dir.default_risk_level(), RiskLevel::Medium);
        assert_eq!(
            validate_container_dir_path("/mnt/aosp/").unwrap(),
            "/mnt/aosp"
        );
        assert_eq!(
            validate_container_dir_path("/run/media/alice/ssd").unwrap(),
            "/run/media/alice/ssd"
        );

        let pkg_udev = ActionRequest::InstallCompatibilityPackage {
            package_group: CompatPackageGroup::AndroidUdevRules,
        };
        assert!(pkg_udev.validate_red_lines().is_ok());
        assert_eq!(
            pkg_udev.required_polkit_action(),
            "tr.com.blazeos.Akrep1.packages.install-optional"
        );
        assert_eq!(pkg_udev.default_risk_level(), RiskLevel::Low);
    }

    #[test]
    fn test_flexible_action_red_lines_rejected() {
        for bad_minutes in [0u32, 1441u32] {
            let sel_bad = ActionRequest::SelinuxSetMode {
                mode: SelinuxDesiredMode::Permissive,
                duration: OverrideDuration::Timed {
                    minutes: bad_minutes,
                },
            };
            assert!(matches!(
                sel_bad.validate_red_lines(),
                Err(AkrepError::InvalidInput(_))
            ));

            let fw_bad = ActionRequest::FirewallSetState {
                enabled: false,
                duration: OverrideDuration::Timed {
                    minutes: bad_minutes,
                },
            };
            assert!(matches!(
                fw_bad.validate_red_lines(),
                Err(AkrepError::InvalidInput(_))
            ));

            let oomd_bad = ActionRequest::OomdSetState {
                enabled: false,
                duration: OverrideDuration::Timed {
                    minutes: bad_minutes,
                },
            };
            assert!(matches!(
                oomd_bad.validate_red_lines(),
                Err(AkrepError::InvalidInput(_))
            ));
        }

        let low_soft = ActionRequest::DevLimitsSetNofile {
            enabled: true,
            soft_limit: 500,
            hard_limit: 65536,
        };
        assert!(matches!(
            low_soft.validate_red_lines(),
            Err(AkrepError::InvalidInput(_))
        ));

        let soft_gt_hard = ActionRequest::DevLimitsSetNofile {
            enabled: true,
            soft_limit: 131_072,
            hard_limit: 65536,
        };
        assert!(matches!(
            soft_gt_hard.validate_red_lines(),
            Err(AkrepError::InvalidInput(_))
        ));

        let hard_too_high = ActionRequest::DevLimitsSetNofile {
            enabled: true,
            soft_limit: 65536,
            hard_limit: 1_048_577,
        };
        assert!(matches!(
            hard_too_high.validate_red_lines(),
            Err(AkrepError::InvalidInput(_))
        ));

        for bad_dir in [
            "/",
            "/home",
            "/home/alice",
            "/home/alice/",
            "/etc/foo",
            "/home/alice/../etc/shadow",
            "/home/alice/./android",
            "/mnt",
            "/run/media/alice",
            "/root",
            "/var/tmp/android",
            "relative/android",
            "/home/alice/android;id",
        ] {
            let req = ActionRequest::SelinuxLabelContainerDir {
                target_path: bad_dir.to_string(),
                recursive: true,
            };
            let res = req.validate_red_lines();
            assert!(
                matches!(
                    res,
                    Err(AkrepError::InvalidInput(_)) | Err(AkrepError::ForbiddenOperation(_))
                ),
                "Expected '{}' to be rejected with InvalidInput or ForbiddenOperation, got {:?}",
                bad_dir,
                res
            );
        }

        // SELINUX=disabled cannot even be deserialized into SelinuxDesiredMode
        let disabled_mode_json = r#"{"kind":"selinux_set_mode","mode":"disabled","duration":{"kind":"until_changed"}}"#;
        assert!(serde_json::from_str::<ActionRequest>(disabled_mode_json).is_err());
    }
}
