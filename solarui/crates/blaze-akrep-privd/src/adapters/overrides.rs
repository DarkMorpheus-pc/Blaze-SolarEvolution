use crate::transaction_engine::{
    write_json_atomic_0600, ApplyOutcome, MutationBackend, PreviewOutcome, DEFAULT_AKREP_STATE_DIR,
};
use async_trait::async_trait;
use blaze_akrep_common::{
    generate_uuid_v4, redact_secrets, validate_container_dir_path, validate_nofile_limits,
    validate_override_duration, ActionRequest, AkrepError, CompatPackageGroup, OverrideDuration,
    OverrideSubsystem, PlanDiffItem, RiskLevel, SelinuxDesiredMode, SelinuxRuntimeMode,
    StateFingerprint, TransactionRecord,
};
use blaze_sentinel_core::sha256_hex;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const DEFAULT_PUBLIC_OVERRIDES_PATH: &str = "/var/cache/akrep/active_overrides.json";
pub const PRIVATE_OVERRIDES_FILENAME: &str = "overrides.json";
pub const OVERRIDES_LOCK_FILENAME: &str = "overrides.lock";
pub const SELINUX_CONFIG_PATH: &str = "/etc/selinux/config";
pub const SELINUX_ENFORCE_SYSFS_PATH: &str = "/sys/fs/selinux/enforce";
pub const PROC_BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
pub const NOFILE_PAM_DROPIN_PATH: &str = "/etc/security/limits.d/90-akrep-dev-nofile.conf";
pub const NOFILE_SYSTEMD_DROPIN_PATH: &str = "/etc/systemd/user.conf.d/90-akrep-nofile.conf";
pub const SYSTEMD_FIREWALLD_UNIT: &str = "firewalld.service";
pub const SYSTEMD_OOMD_UNIT: &str = "systemd-oomd.service";
pub const AKREP_PRIVD_BIN_PATH: &str = "/usr/bin/akrep-privd";
pub const CHCON_TIMEOUT_SECS: u64 = 60;
const SYSTEMCTL_TIMEOUT_SECS: u64 = 30;
const DNF5_TIMEOUT_SECS: u64 = 300;
const UDEVADM_TIMEOUT_SECS: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideEntry {
    pub subsystem: OverrideSubsystem,
    pub relaxed: bool,
    pub duration: OverrideDuration,
    pub generation_id: String,
    pub started_at: String,
    pub expires_at: Option<String>,
    pub timer_unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideStoreDocument {
    pub schema_version: u32,
    pub boot_id: String,
    pub entries: BTreeMap<String, OverrideEntry>,
}

impl OverrideStoreDocument {
    pub fn empty(boot_id: String) -> Self {
        Self {
            schema_version: 1,
            boot_id,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileOverridePreState {
    pub selinux_restored: bool,
    pub selinux_prev_runtime: Option<SelinuxRuntimeMode>,
    pub selinux_prev_config_raw: Option<String>,
    pub selinux_prev_override: Option<OverrideEntry>,
    pub firewalld_restored: bool,
    pub firewalld_prev_active: Option<bool>,
    pub firewalld_prev_enabled: Option<bool>,
    pub firewalld_prev_override: Option<OverrideEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SelinuxModeSnapshot {
    prev_runtime: SelinuxRuntimeMode,
    prev_config_raw: Option<String>,
    prev_override: Option<OverrideEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ServiceOverrideSnapshot {
    unit: String,
    prev_active: bool,
    prev_enabled: bool,
    prev_override: Option<OverrideEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NofileLimitsSnapshot {
    prev_pam_content: Option<String>,
    prev_systemd_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContainerDirLabelSnapshot {
    canonical_path: String,
    recursive: bool,
    previous_context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CompatPackageSnapshot {
    package_group: CompatPackageGroup,
    previously_installed: bool,
}

#[derive(Debug, Clone, Copy)]
struct ServiceOverrideSpec {
    subsystem: OverrideSubsystem,
    unit: &'static str,
    fp_target: &'static str,
    label_tr: &'static str,
    label_en: &'static str,
}

const FIREWALLD_SPEC: ServiceOverrideSpec = ServiceOverrideSpec {
    subsystem: OverrideSubsystem::Firewalld,
    unit: SYSTEMD_FIREWALLD_UNIT,
    fp_target: "firewall.service_state",
    label_tr: "Güvenlik duvarı (firewalld)",
    label_en: "Firewall (firewalld)",
};

const OOMD_SPEC: ServiceOverrideSpec = ServiceOverrideSpec {
    subsystem: OverrideSubsystem::Oomd,
    unit: SYSTEMD_OOMD_UNIT,
    fp_target: "system.oomd_state",
    label_tr: "Bellek yöneticisi (systemd-oomd)",
    label_en: "Memory manager (systemd-oomd)",
};

pub trait OverrideOps: Send + Sync {
    fn read_boot_id(&self) -> Result<String, AkrepError>;
    fn read_selinux_runtime_mode(&self) -> Result<SelinuxRuntimeMode, AkrepError>;
    fn setenforce(&self, enforcing: bool) -> Result<(), AkrepError>;
    fn read_selinux_config_raw(&self) -> Result<Option<String>, AkrepError>;
    fn write_selinux_config_raw(&self, content: &str) -> Result<(), AkrepError>;
    fn is_unit_active(&self, unit: &str) -> Result<bool, AkrepError>;
    fn is_unit_enabled(&self, unit: &str) -> Result<bool, AkrepError>;
    fn start_unit(&self, unit: &str) -> Result<(), AkrepError>;
    fn stop_unit(&self, unit: &str) -> Result<(), AkrepError>;
    fn enable_unit(&self, unit: &str) -> Result<(), AkrepError>;
    fn disable_unit(&self, unit: &str) -> Result<(), AkrepError>;
    fn daemon_reload(&self) -> Result<(), AkrepError>;
    fn schedule_revert_timer(
        &self,
        subsystem: OverrideSubsystem,
        delay_secs: u64,
        generation_id: &str,
        state_dir: &Path,
    ) -> Result<String, AkrepError>;
    fn cancel_revert_timer(&self, subsystem: OverrideSubsystem) -> Result<(), AkrepError>;
    fn read_nofile_dropins(&self) -> Result<(Option<String>, Option<String>), AkrepError>;
    fn write_nofile_dropins(
        &self,
        pam_content: &str,
        systemd_content: &str,
    ) -> Result<(), AkrepError>;
    fn remove_nofile_dropins(&self) -> Result<(), AkrepError>;
    fn restore_nofile_dropins(
        &self,
        pam_content: Option<&str>,
        systemd_content: Option<&str>,
    ) -> Result<(), AkrepError>;
    fn resolve_container_dir(&self, path: &Path) -> Result<PathBuf, AkrepError>;
    fn read_path_selinux_context(&self, path: &Path) -> Result<String, AkrepError>;
    fn chcon_container_dir(&self, canonical_dir: &Path, recursive: bool) -> Result<(), AkrepError>;
    fn restorecon_container_dir(
        &self,
        canonical_dir: &Path,
        recursive: bool,
    ) -> Result<(), AkrepError>;
    fn is_packages_installed(&self, packages: &[&str]) -> Result<bool, AkrepError>;
    fn install_compat_packages(&self, group: CompatPackageGroup) -> Result<(), AkrepError>;
    fn remove_compat_packages(&self, group: CompatPackageGroup) -> Result<(), AkrepError>;
}

fn command_c_locale<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("LC_ALL", "C").env("LANG", "C");
    cmd
}

pub(crate) fn resolve_binary(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
}

pub(crate) fn run_command_with_timeout(
    mut cmd: Command,
    timeout: Duration,
    op_label: &str,
) -> Result<Output, AkrepError> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        AkrepError::BackendUnavailable(format!(
            "Failed to launch {}: {}",
            op_label,
            redact_secrets(&e.to_string())
        ))
    })?;

    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AkrepError::ApplyFailed(format!(
                        "{} timed out after {}s",
                        op_label,
                        timeout.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AkrepError::IoError(format!(
                    "Failed waiting on {}: {}",
                    op_label,
                    redact_secrets(&e.to_string())
                )));
            }
        }
    };

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn ensure_dir_mode_nofollow(dir: &Path, mode: u32) -> Result<(), AkrepError> {
    if let Ok(meta) = fs::symlink_metadata(dir) {
        if meta.file_type().is_symlink() {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Refusing symlinked directory '{}'",
                dir.display()
            )));
        }
        if !meta.file_type().is_dir() {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Expected directory at '{}'",
                dir.display()
            )));
        }
        fs::set_permissions(dir, fs::Permissions::from_mode(mode))?;
        return Ok(());
    }
    fs::create_dir_all(dir)?;
    fs::set_permissions(dir, fs::Permissions::from_mode(mode))?;
    Ok(())
}

fn reject_existing_symlink(path: &Path) -> Result<(), AkrepError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Refusing symlinked target '{}'",
                    path.display()
                )));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AkrepError::from(e)),
    }
}

fn read_file_nofollow_utf8(path: &Path) -> Result<Option<String>, AkrepError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Refusing to read symlinked file '{}'",
                    path.display()
                )));
            }
            if !meta.file_type().is_file() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Target '{}' is not a regular file",
                    path.display()
                )));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(AkrepError::from(e)),
    }

    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| {
            if e.raw_os_error() == Some(libc::ELOOP) {
                AkrepError::ForbiddenOperation(format!(
                    "Refusing symlinked file '{}'",
                    path.display()
                ))
            } else {
                AkrepError::from(e)
            }
        })?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    Ok(Some(content))
}

fn atomic_write_bytes_nofollow(
    target: &Path,
    bytes: &[u8],
    file_mode: u32,
    parent_mode: u32,
) -> Result<(), AkrepError> {
    let parent = target.parent().ok_or_else(|| {
        AkrepError::Internal(format!(
            "Target path '{}' has no parent directory",
            target.display()
        ))
    })?;
    ensure_dir_mode_nofollow(parent, parent_mode)?;
    reject_existing_symlink(target)?;

    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AkrepError::Internal("Invalid atomic target file name".to_string()))?;
    let nonce = generate_uuid_v4()?;
    let tmp_path = parent.join(format!(".{}.{}.tmp", file_name, nonce));

    let mut tmp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .mode(file_mode)
        .open(&tmp_path)?;

    if let Err(err) = tmp
        .set_permissions(fs::Permissions::from_mode(file_mode))
        .and_then(|_| tmp.write_all(bytes))
        .and_then(|_| tmp.sync_all())
    {
        let _ = fs::remove_file(&tmp_path);
        return Err(AkrepError::from(err));
    }
    drop(tmp);

    if let Err(err) = fs::rename(&tmp_path, target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(AkrepError::from(err));
    }
    fs::set_permissions(target, fs::Permissions::from_mode(file_mode))?;
    let dir_file = File::open(parent)?;
    dir_file.sync_all()?;
    Ok(())
}

fn remove_file_nofollow(path: &Path) -> Result<(), AkrepError> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Refusing to remove symlinked file '{}'",
                    path.display()
                )));
            }
            if !meta.file_type().is_file() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Refusing to remove non-regular file '{}'",
                    path.display()
                )));
            }
            fs::remove_file(path)?;
            if let Some(parent) = path.parent() {
                if let Ok(dir_file) = File::open(parent) {
                    let _ = dir_file.sync_all();
                }
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AkrepError::from(e)),
    }
}

/// Validates and canonicalizes an on-disk directory path without traversing symlinks.
pub fn resolve_real_directory_nofollow(path: &Path) -> Result<PathBuf, AkrepError> {
    if !path.is_absolute() {
        return Err(AkrepError::InvalidInput(format!(
            "Target directory '{}' must be an absolute path",
            path.display()
        )));
    }

    let mut cur = PathBuf::from("/");
    for comp in path.components() {
        match comp {
            Component::RootDir => {}
            Component::Normal(seg) => {
                cur.push(seg);
                let meta = fs::symlink_metadata(&cur).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        AkrepError::InvalidInput(format!(
                            "Directory '{}' does not exist",
                            path.display()
                        ))
                    } else {
                        AkrepError::from(e)
                    }
                })?;
                if meta.file_type().is_symlink() {
                    return Err(AkrepError::ForbiddenOperation(format!(
                        "Symlink component '{}' is forbidden",
                        cur.display()
                    )));
                }
            }
            Component::ParentDir | Component::CurDir | Component::Prefix(_) => {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Path traversal in '{}' is forbidden",
                    path.display()
                )));
            }
        }
    }

    let meta = fs::symlink_metadata(path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            AkrepError::InvalidInput(format!("Directory '{}' does not exist", path.display()))
        } else {
            AkrepError::from(e)
        }
    })?;
    if meta.file_type().is_symlink() {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Symlink target '{}' is forbidden",
            path.display()
        )));
    }
    if !meta.file_type().is_dir() {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Target '{}' must be a directory",
            path.display()
        )));
    }

    Ok(fs::canonicalize(path)?)
}

pub fn parse_selinux_config_mode(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = trimmed.split_once('=') {
            if key.trim() == "SELINUX" {
                return Some(val.trim().to_ascii_lowercase());
            }
        }
    }
    None
}

pub fn update_selinux_config_content(
    raw: &str,
    mode: SelinuxDesiredMode,
) -> Result<String, AkrepError> {
    let mode_val = match mode {
        SelinuxDesiredMode::Enforcing => "enforcing",
        SelinuxDesiredMode::Permissive => "permissive",
    };

    let had_trailing_newline = raw.ends_with('\n');
    let mut out_lines: Vec<String> = Vec::new();
    let mut replaced = false;

    for line in raw.lines() {
        let line_clean = line.strip_suffix('\r').unwrap_or(line);
        let trimmed = line_clean.trim();
        let is_active_selinux = !trimmed.is_empty()
            && !trimmed.starts_with('#')
            && trimmed
                .split_once('=')
                .is_some_and(|(k, _)| k.trim() == "SELINUX");
        if is_active_selinux {
            if !replaced {
                out_lines.push(format!("SELINUX={}", mode_val));
                replaced = true;
            }
        } else {
            out_lines.push(line_clean.to_string());
        }
    }

    if !replaced {
        out_lines.push(format!("SELINUX={}", mode_val));
    }

    let mut updated = out_lines.join("\n");
    if had_trailing_newline || !updated.is_empty() {
        updated.push('\n');
    }

    debug_assert_ne!(
        parse_selinux_config_mode(&updated).as_deref(),
        Some("disabled")
    );
    Ok(updated)
}

pub fn build_container_chcon_args(recursive: bool, canonical_dir: &Path) -> Vec<OsString> {
    let mut args = vec![OsString::from("--no-dereference")];
    if recursive {
        args.push(OsString::from("-R"));
    }
    args.push(OsString::from("-t"));
    args.push(OsString::from("container_file_t"));
    args.push(OsString::from("--"));
    args.push(canonical_dir.as_os_str().to_os_string());
    args
}

/// Builds `restorecon` arguments for container directory rollback.
///
/// Fedora `policycoreutils` `restorecon` treats `-h` as usage help (and always traverses via
/// `FTW_PHYS` without dereferencing symlinks), while `-F` is required to reset `container_file_t`
/// because it is listed in `/etc/selinux/targeted/contexts/customizable_types`.
pub fn build_container_restorecon_args(recursive: bool, canonical_dir: &Path) -> Vec<OsString> {
    let mut args = vec![OsString::from("-F")];
    if recursive {
        args.push(OsString::from("-R"));
        args.push(OsString::from("-x"));
    }
    args.push(OsString::from("--"));
    args.push(canonical_dir.as_os_str().to_os_string());
    args
}

fn validate_managed_systemd_unit(unit: &str) -> Result<(), AkrepError> {
    if unit == SYSTEMD_FIREWALLD_UNIT || unit == SYSTEMD_OOMD_UNIT {
        return Ok(());
    }
    Err(AkrepError::ForbiddenOperation(format!(
        "Unmanaged systemd unit '{}' is forbidden",
        unit
    )))
}

fn validate_generation_token(generation_id: &str) -> Result<(), AkrepError> {
    if generation_id.is_empty()
        || generation_id.len() > 128
        || generation_id.contains("..")
        || !generation_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(AkrepError::InvalidInput(
            "Invalid override generation_id".to_string(),
        ));
    }
    Ok(())
}

pub fn format_pam_nofile_dropin(soft_limit: u32, hard_limit: u32) -> String {
    format!(
        "* soft nofile {}\n* hard nofile {}\n",
        soft_limit, hard_limit
    )
}

pub fn format_systemd_nofile_dropin(soft_limit: u32, hard_limit: u32) -> String {
    format!(
        "[Manager]\nDefaultLimitNOFILE={}:{}\n",
        soft_limit, hard_limit
    )
}

pub fn parse_nofile_dropins(
    pam_raw: Option<&str>,
    systemd_raw: Option<&str>,
) -> Option<(u32, u32)> {
    let mut soft: Option<u32> = None;
    let mut hard: Option<u32> = None;

    if let Some(raw) = pam_raw {
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() == 4 && parts[0] == "*" && parts[2] == "nofile" {
                if parts[1] == "soft" {
                    soft = parts[3].parse::<u32>().ok();
                } else if parts[1] == "hard" {
                    hard = parts[3].parse::<u32>().ok();
                }
            }
        }
    }

    if let (Some(s), Some(h)) = (soft, hard) {
        return Some((s, h));
    }

    if let Some(raw) = systemd_raw {
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("DefaultLimitNOFILE=") {
                if let Some((s_str, h_str)) = val.trim().split_once(':') {
                    if let (Ok(s), Ok(h)) =
                        (s_str.trim().parse::<u32>(), h_str.trim().parse::<u32>())
                    {
                        return Some((s, h));
                    }
                }
            }
        }
    }

    None
}

#[derive(Default)]
pub struct LinuxOverrideOps;

impl LinuxOverrideOps {
    pub fn new() -> Self {
        Self
    }

    fn systemctl_bin() -> Result<PathBuf, AkrepError> {
        resolve_binary(&["/usr/bin/systemctl", "/bin/systemctl"]).ok_or_else(|| {
            AkrepError::BackendUnavailable("systemctl binary is not installed".to_string())
        })
    }

    fn run_systemctl(&self, args: &[&str], op_label: &str) -> Result<Output, AkrepError> {
        let bin = Self::systemctl_bin()?;
        let mut cmd = command_c_locale(bin);
        cmd.args(args);
        run_command_with_timeout(cmd, Duration::from_secs(SYSTEMCTL_TIMEOUT_SECS), op_label)
    }
}

impl OverrideOps for LinuxOverrideOps {
    fn read_boot_id(&self) -> Result<String, AkrepError> {
        let raw = fs::read_to_string(PROC_BOOT_ID_PATH)?;
        let trimmed = raw.trim().to_string();
        if trimmed.is_empty() {
            return Err(AkrepError::Internal(
                "Kernel boot_id at /proc/sys/kernel/random/boot_id is empty".to_string(),
            ));
        }
        Ok(trimmed)
    }

    fn read_selinux_runtime_mode(&self) -> Result<SelinuxRuntimeMode, AkrepError> {
        let enforce_path = Path::new(SELINUX_ENFORCE_SYSFS_PATH);
        if !enforce_path.exists() {
            return Ok(SelinuxRuntimeMode::Disabled);
        }
        let raw = fs::read_to_string(enforce_path)?;
        match raw.trim() {
            "1" => Ok(SelinuxRuntimeMode::Enforcing),
            "0" => Ok(SelinuxRuntimeMode::Permissive),
            other => Err(AkrepError::Internal(format!(
                "Unexpected /sys/fs/selinux/enforce value '{}'",
                other
            ))),
        }
    }

    fn setenforce(&self, enforcing: bool) -> Result<(), AkrepError> {
        let bin = resolve_binary(&[
            "/usr/sbin/setenforce",
            "/sbin/setenforce",
            "/usr/bin/setenforce",
        ])
        .ok_or_else(|| {
            AkrepError::BackendUnavailable("setenforce binary is not installed".to_string())
        })?;
        let arg = if enforcing { "1" } else { "0" };
        let mut cmd = command_c_locale(bin);
        cmd.arg(arg);
        let out = run_command_with_timeout(cmd, Duration::from_secs(15), "setenforce")?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "setenforce {} failed: {}",
                arg,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn read_selinux_config_raw(&self) -> Result<Option<String>, AkrepError> {
        read_file_nofollow_utf8(Path::new(SELINUX_CONFIG_PATH))
    }

    fn write_selinux_config_raw(&self, content: &str) -> Result<(), AkrepError> {
        if let Some(mode) = parse_selinux_config_mode(content) {
            if mode != "enforcing" && mode != "permissive" {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Refusing to write disallowed SELINUX={} to {}",
                    mode, SELINUX_CONFIG_PATH
                )));
            }
        } else {
            return Err(AkrepError::InvalidInput(
                "SELinux config payload missing SELINUX= directive".to_string(),
            ));
        }
        atomic_write_bytes_nofollow(
            Path::new(SELINUX_CONFIG_PATH),
            content.as_bytes(),
            0o644,
            0o755,
        )
    }

    fn is_unit_active(&self, unit: &str) -> Result<bool, AkrepError> {
        validate_managed_systemd_unit(unit)?;
        let out = self.run_systemctl(&["is-active", "--quiet", unit], "systemctl is-active")?;
        Ok(out.status.success())
    }

    fn is_unit_enabled(&self, unit: &str) -> Result<bool, AkrepError> {
        validate_managed_systemd_unit(unit)?;
        let out = self.run_systemctl(&["is-enabled", unit], "systemctl is-enabled")?;
        let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(out.status.success()
            || matches!(
                state.as_str(),
                "enabled" | "enabled-runtime" | "static" | "indirect"
            ))
    }

    fn start_unit(&self, unit: &str) -> Result<(), AkrepError> {
        validate_managed_systemd_unit(unit)?;
        let out = self.run_systemctl(&["start", unit], "systemctl start")?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "systemctl start {} failed: {}",
                unit,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn stop_unit(&self, unit: &str) -> Result<(), AkrepError> {
        validate_managed_systemd_unit(unit)?;
        let out = self.run_systemctl(&["stop", unit], "systemctl stop")?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "systemctl stop {} failed: {}",
                unit,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn enable_unit(&self, unit: &str) -> Result<(), AkrepError> {
        validate_managed_systemd_unit(unit)?;
        let out = self.run_systemctl(&["enable", unit], "systemctl enable")?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "systemctl enable {} failed: {}",
                unit,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn disable_unit(&self, unit: &str) -> Result<(), AkrepError> {
        validate_managed_systemd_unit(unit)?;
        let out = self.run_systemctl(&["disable", unit], "systemctl disable")?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "systemctl disable {} failed: {}",
                unit,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn daemon_reload(&self) -> Result<(), AkrepError> {
        let out = self.run_systemctl(&["daemon-reload"], "systemctl daemon-reload")?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "systemctl daemon-reload failed: {}",
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn schedule_revert_timer(
        &self,
        subsystem: OverrideSubsystem,
        delay_secs: u64,
        generation_id: &str,
        state_dir: &Path,
    ) -> Result<String, AkrepError> {
        if delay_secs == 0 {
            return Err(AkrepError::InvalidInput(
                "Timer delay_secs must be greater than zero".to_string(),
            ));
        }
        validate_generation_token(generation_id)?;
        let _ = self.cancel_revert_timer(subsystem);

        let systemd_run = resolve_binary(&["/usr/bin/systemd-run", "/bin/systemd-run"])
            .ok_or_else(|| {
                AkrepError::BackendUnavailable("systemd-run binary is not installed".to_string())
            })?;

        let unit_base = format!("akrep-revert-{}", subsystem.as_str());
        let timer_unit = format!("{}.timer", unit_base);
        let unit_arg = format!("--unit={}", unit_base);
        let on_active_arg = format!("--on-active={}s", delay_secs);
        let desc_arg = format!("--description=Akrep auto-revert {}", subsystem.as_str());

        let mut cmd = command_c_locale(systemd_run);
        cmd.args([
            unit_arg.as_str(),
            on_active_arg.as_str(),
            "--timer-property=AccuracySec=1s",
            desc_arg.as_str(),
            "--",
            AKREP_PRIVD_BIN_PATH,
            "--revert-override",
            subsystem.as_str(),
            "--generation",
            generation_id,
        ]);
        if state_dir != Path::new(DEFAULT_AKREP_STATE_DIR) {
            cmd.arg("--state-dir").arg(state_dir);
        }

        let out = run_command_with_timeout(
            cmd,
            Duration::from_secs(SYSTEMCTL_TIMEOUT_SECS),
            "systemd-run",
        )?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "systemd-run failed for '{}': {}",
                timer_unit,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(timer_unit)
    }

    fn cancel_revert_timer(&self, subsystem: OverrideSubsystem) -> Result<(), AkrepError> {
        let unit_base = format!("akrep-revert-{}", subsystem.as_str());
        let timer_unit = format!("{}.timer", unit_base);
        let service_unit = format!("{}.service", unit_base);
        let _ = self.run_systemctl(
            &["stop", timer_unit.as_str(), service_unit.as_str()],
            "systemctl stop revert timer",
        )?;
        let _ = self.run_systemctl(
            &["reset-failed", timer_unit.as_str(), service_unit.as_str()],
            "systemctl reset-failed revert timer",
        )?;
        Ok(())
    }

    fn read_nofile_dropins(&self) -> Result<(Option<String>, Option<String>), AkrepError> {
        let pam = read_file_nofollow_utf8(Path::new(NOFILE_PAM_DROPIN_PATH))?;
        let sysd = read_file_nofollow_utf8(Path::new(NOFILE_SYSTEMD_DROPIN_PATH))?;
        Ok((pam, sysd))
    }

    fn write_nofile_dropins(
        &self,
        pam_content: &str,
        systemd_content: &str,
    ) -> Result<(), AkrepError> {
        let pam_path = Path::new(NOFILE_PAM_DROPIN_PATH);
        let sysd_path = Path::new(NOFILE_SYSTEMD_DROPIN_PATH);
        let prev_pam = read_file_nofollow_utf8(pam_path)?;

        atomic_write_bytes_nofollow(pam_path, pam_content.as_bytes(), 0o644, 0o755)?;
        if let Err(err) =
            atomic_write_bytes_nofollow(sysd_path, systemd_content.as_bytes(), 0o644, 0o755)
        {
            match prev_pam {
                Some(old) => {
                    let _ = atomic_write_bytes_nofollow(pam_path, old.as_bytes(), 0o644, 0o755);
                }
                None => {
                    let _ = remove_file_nofollow(pam_path);
                }
            }
            return Err(err);
        }
        Ok(())
    }

    fn remove_nofile_dropins(&self) -> Result<(), AkrepError> {
        remove_file_nofollow(Path::new(NOFILE_PAM_DROPIN_PATH))?;
        remove_file_nofollow(Path::new(NOFILE_SYSTEMD_DROPIN_PATH))?;
        Ok(())
    }

    fn restore_nofile_dropins(
        &self,
        pam_content: Option<&str>,
        systemd_content: Option<&str>,
    ) -> Result<(), AkrepError> {
        match pam_content {
            Some(pam) => atomic_write_bytes_nofollow(
                Path::new(NOFILE_PAM_DROPIN_PATH),
                pam.as_bytes(),
                0o644,
                0o755,
            )?,
            None => remove_file_nofollow(Path::new(NOFILE_PAM_DROPIN_PATH))?,
        }
        match systemd_content {
            Some(sysd) => atomic_write_bytes_nofollow(
                Path::new(NOFILE_SYSTEMD_DROPIN_PATH),
                sysd.as_bytes(),
                0o644,
                0o755,
            )?,
            None => remove_file_nofollow(Path::new(NOFILE_SYSTEMD_DROPIN_PATH))?,
        }
        Ok(())
    }

    fn resolve_container_dir(&self, path: &Path) -> Result<PathBuf, AkrepError> {
        let path_str = path.to_str().ok_or_else(|| {
            AkrepError::InvalidInput("Container directory path must be valid UTF-8".to_string())
        })?;
        let normalized = validate_container_dir_path(path_str)?;
        let canonical = resolve_real_directory_nofollow(Path::new(&normalized))?;
        let canonical_str = canonical.to_str().ok_or_else(|| {
            AkrepError::InvalidInput("Canonical container directory path is not UTF-8".to_string())
        })?;
        let _ = validate_container_dir_path(canonical_str)?;
        Ok(canonical)
    }

    fn read_path_selinux_context(&self, path: &Path) -> Result<String, AkrepError> {
        if let Some(getfattr) = resolve_binary(&["/usr/bin/getfattr", "/bin/getfattr"]) {
            let mut cmd = command_c_locale(getfattr);
            cmd.args([
                "-h",
                "-n",
                "security.selinux",
                "--only-values",
                "--absolute-names",
                "--",
            ])
            .arg(path);
            if let Ok(out) = run_command_with_timeout(cmd, Duration::from_secs(15), "getfattr") {
                if out.status.success() {
                    let ctx = String::from_utf8_lossy(&out.stdout)
                        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                        .to_string();
                    if !ctx.is_empty() {
                        return Ok(ctx);
                    }
                }
            }
        }

        let stat_bin = resolve_binary(&["/usr/bin/stat", "/bin/stat"]).ok_or_else(|| {
            AkrepError::BackendUnavailable(
                "Missing 'stat' utility for SELinux context inspection".to_string(),
            )
        })?;
        let mut cmd = command_c_locale(stat_bin);
        cmd.args(["-c", "%C", "--"]).arg(path);
        let out = run_command_with_timeout(cmd, Duration::from_secs(15), "stat")?;
        if !out.status.success() {
            return Err(AkrepError::IoError(redact_secrets(
                String::from_utf8_lossy(&out.stderr).trim(),
            )));
        }
        let ctx = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if ctx.is_empty() || ctx == "?" {
            return Err(AkrepError::BackendUnavailable(format!(
                "SELinux context is unavailable for '{}'",
                path.display()
            )));
        }
        Ok(ctx)
    }

    fn chcon_container_dir(&self, canonical_dir: &Path, recursive: bool) -> Result<(), AkrepError> {
        let resolved = self.resolve_container_dir(canonical_dir)?;
        let bin = resolve_binary(&["/usr/bin/chcon", "/bin/chcon", "/usr/sbin/chcon"]).ok_or_else(
            || AkrepError::BackendUnavailable("chcon binary is not installed".to_string()),
        )?;
        let mut cmd = command_c_locale(bin);
        cmd.args(build_container_chcon_args(recursive, &resolved));
        let out = run_command_with_timeout(
            cmd,
            Duration::from_secs(CHCON_TIMEOUT_SECS),
            "chcon container_file_t",
        )?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "chcon failed on '{}': {}",
                resolved.display(),
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn restorecon_container_dir(
        &self,
        canonical_dir: &Path,
        recursive: bool,
    ) -> Result<(), AkrepError> {
        let resolved = self.resolve_container_dir(canonical_dir)?;
        let bin = resolve_binary(&[
            "/sbin/restorecon",
            "/usr/sbin/restorecon",
            "/usr/bin/restorecon",
        ])
        .ok_or_else(|| {
            AkrepError::BackendUnavailable("restorecon binary is not installed".to_string())
        })?;
        let mut cmd = command_c_locale(bin);
        cmd.args(build_container_restorecon_args(recursive, &resolved));
        let out = run_command_with_timeout(
            cmd,
            Duration::from_secs(CHCON_TIMEOUT_SECS),
            "restorecon container dir",
        )?;
        if !out.status.success() {
            return Err(AkrepError::RollbackFailed(format!(
                "restorecon failed on '{}': {}",
                resolved.display(),
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn is_packages_installed(&self, packages: &[&str]) -> Result<bool, AkrepError> {
        let rpm_bin = resolve_binary(&["/usr/bin/rpm", "/bin/rpm"]).ok_or_else(|| {
            AkrepError::BackendUnavailable("rpm binary is not installed".to_string())
        })?;
        let mut cmd = command_c_locale(rpm_bin);
        cmd.arg("-q").args(packages);
        let out = run_command_with_timeout(cmd, Duration::from_secs(15), "rpm -q")?;
        Ok(out.status.success())
    }

    fn install_compat_packages(&self, group: CompatPackageGroup) -> Result<(), AkrepError> {
        let dnf_bin = resolve_binary(&["/usr/bin/dnf5"]).ok_or_else(|| {
            AkrepError::BackendUnavailable(
                "/usr/bin/dnf5 is not available on this system".to_string(),
            )
        })?;
        let pkgs = group.rpm_packages();
        let mut cmd = command_c_locale(dnf_bin);
        cmd.args(["install", "-y"]).args(pkgs);
        let out = run_command_with_timeout(
            cmd,
            Duration::from_secs(DNF5_TIMEOUT_SECS),
            "dnf5 install compatibility package",
        )?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "dnf5 install failed for {:?}: {}",
                pkgs,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }

        if matches!(group, CompatPackageGroup::AndroidUdevRules) {
            let udevadm =
                resolve_binary(&["/usr/bin/udevadm", "/sbin/udevadm", "/usr/sbin/udevadm"])
                    .ok_or_else(|| {
                        AkrepError::BackendUnavailable(
                            "udevadm binary is not installed".to_string(),
                        )
                    })?;

            let mut reload_cmd = command_c_locale(&udevadm);
            reload_cmd.args(["control", "--reload-rules"]);
            let reload_out = run_command_with_timeout(
                reload_cmd,
                Duration::from_secs(UDEVADM_TIMEOUT_SECS),
                "udevadm control --reload-rules",
            )?;
            if !reload_out.status.success() {
                return Err(AkrepError::ApplyFailed(format!(
                    "udevadm control --reload-rules failed: {}",
                    redact_secrets(String::from_utf8_lossy(&reload_out.stderr).trim())
                )));
            }

            let mut trig_cmd = command_c_locale(&udevadm);
            trig_cmd.arg("trigger");
            let trig_out = run_command_with_timeout(
                trig_cmd,
                Duration::from_secs(UDEVADM_TIMEOUT_SECS),
                "udevadm trigger",
            )?;
            if !trig_out.status.success() {
                return Err(AkrepError::ApplyFailed(format!(
                    "udevadm trigger failed: {}",
                    redact_secrets(String::from_utf8_lossy(&trig_out.stderr).trim())
                )));
            }
        }
        Ok(())
    }

    fn remove_compat_packages(&self, group: CompatPackageGroup) -> Result<(), AkrepError> {
        let dnf_bin = resolve_binary(&["/usr/bin/dnf5"]).ok_or_else(|| {
            AkrepError::BackendUnavailable(
                "/usr/bin/dnf5 is not available for rollback".to_string(),
            )
        })?;
        let pkgs = group.rpm_packages();
        let mut cmd = command_c_locale(dnf_bin);
        cmd.args(["remove", "-y"]).args(pkgs);
        let out = run_command_with_timeout(
            cmd,
            Duration::from_secs(DNF5_TIMEOUT_SECS),
            "dnf5 remove compatibility package",
        )?;
        if !out.status.success() {
            return Err(AkrepError::RollbackFailed(format!(
                "dnf5 remove failed for {:?}: {}",
                pkgs,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        if matches!(group, CompatPackageGroup::AndroidUdevRules) {
            if let Some(udevadm) =
                resolve_binary(&["/usr/bin/udevadm", "/sbin/udevadm", "/usr/sbin/udevadm"])
            {
                let mut reload_cmd = command_c_locale(&udevadm);
                reload_cmd.args(["control", "--reload-rules"]);
                let _ = run_command_with_timeout(
                    reload_cmd,
                    Duration::from_secs(UDEVADM_TIMEOUT_SECS),
                    "udevadm control --reload-rules",
                );
            }
        }
        Ok(())
    }
}

struct BlockingLockGuard {
    file: File,
}

impl BlockingLockGuard {
    fn acquire_exclusive(lock_path: &Path) -> Result<Self, AkrepError> {
        reject_existing_symlink(lock_path)?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .mode(0o600)
            .open(lock_path)?;
        let _ = fs::set_permissions(lock_path, fs::Permissions::from_mode(0o600));
        // SAFETY: `file.as_raw_fd()` is valid and remains open until `Drop::drop`.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
        if rc != 0 {
            return Err(AkrepError::IoError(redact_secrets(
                &std::io::Error::last_os_error().to_string(),
            )));
        }
        Ok(Self { file })
    }
}

impl Drop for BlockingLockGuard {
    fn drop(&mut self) {
        // SAFETY: `self.file` remains open until after `Drop::drop` finishes.
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[derive(Clone)]
pub struct OverrideAdapter {
    state_dir: PathBuf,
    public_cache_path: PathBuf,
    ops: Arc<dyn OverrideOps>,
    store_mutex: Arc<Mutex<()>>,
}

impl OverrideAdapter {
    pub fn new_system(state_dir: PathBuf) -> Self {
        Self::new_with_ops(
            state_dir,
            PathBuf::from(DEFAULT_PUBLIC_OVERRIDES_PATH),
            Arc::new(LinuxOverrideOps::new()),
        )
    }

    pub fn new_with_ops(
        state_dir: PathBuf,
        public_cache_path: PathBuf,
        ops: Arc<dyn OverrideOps>,
    ) -> Self {
        Self {
            state_dir,
            public_cache_path,
            ops,
            store_mutex: Arc::new(Mutex::new(())),
        }
    }

    pub fn state_dir(&self) -> &Path {
        &self.state_dir
    }

    pub fn public_cache_path(&self) -> &Path {
        &self.public_cache_path
    }

    pub fn private_store_path(&self) -> PathBuf {
        self.state_dir.join(PRIVATE_OVERRIDES_FILENAME)
    }

    pub fn ops(&self) -> &Arc<dyn OverrideOps> {
        &self.ops
    }

    fn lock_store(&self) -> Result<(std::sync::MutexGuard<'_, ()>, BlockingLockGuard), AkrepError> {
        let guard = self
            .store_mutex
            .lock()
            .map_err(|_| AkrepError::Internal("override store mutex poisoned".to_string()))?;
        ensure_dir_mode_nofollow(&self.state_dir, 0o700)?;
        let flock =
            BlockingLockGuard::acquire_exclusive(&self.state_dir.join(OVERRIDES_LOCK_FILENAME))?;
        Ok((guard, flock))
    }

    fn load_store_unlocked(&self) -> Result<OverrideStoreDocument, AkrepError> {
        let current_boot_id = self.ops.read_boot_id()?;
        let store_path = self.private_store_path();
        let Some(raw) = read_file_nofollow_utf8(&store_path)? else {
            return Ok(OverrideStoreDocument::empty(current_boot_id));
        };
        let mut doc: OverrideStoreDocument = serde_json::from_str(&raw).map_err(|e| {
            AkrepError::Internal(format!(
                "Corrupt {}: {}",
                PRIVATE_OVERRIDES_FILENAME,
                redact_secrets(&e.to_string())
            ))
        })?;
        if doc.schema_version != 1 {
            return Err(AkrepError::Internal(format!(
                "Unsupported {} schema_version {}",
                PRIVATE_OVERRIDES_FILENAME, doc.schema_version
            )));
        }
        if doc.boot_id != current_boot_id {
            // Timed and UntilReboot entries belong only to the boot that created them.
            doc.entries
                .retain(|_, entry| matches!(entry.duration, OverrideDuration::UntilChanged));
            doc.boot_id = current_boot_id;
        }
        Ok(doc)
    }

    fn save_store_unlocked(&self, doc: &OverrideStoreDocument) -> Result<(), AkrepError> {
        ensure_dir_mode_nofollow(&self.state_dir, 0o700)?;
        let store_path = self.private_store_path();
        reject_existing_symlink(&store_path)?;
        write_json_atomic_0600(&store_path, doc)?;

        let pub_bytes = serde_json::to_vec_pretty(doc)
            .map_err(|e| AkrepError::Internal(format!("override cache serialize failed: {}", e)))?;
        atomic_write_bytes_nofollow(&self.public_cache_path, &pub_bytes, 0o644, 0o755)?;
        Ok(())
    }

    pub fn active_override(
        &self,
        subsystem: OverrideSubsystem,
    ) -> Result<Option<OverrideEntry>, AkrepError> {
        let _locks = self.lock_store()?;
        let doc = self.load_store_unlocked()?;
        Ok(doc.entries.get(subsystem.as_str()).cloned())
    }

    fn build_override_entry(
        subsystem: OverrideSubsystem,
        duration: OverrideDuration,
        generation_id: String,
        timer_unit: Option<String>,
    ) -> OverrideEntry {
        let now = Utc::now();
        let expires_at = match duration {
            OverrideDuration::Timed { minutes } => {
                Some((now + ChronoDuration::seconds(i64::from(minutes) * 60)).to_rfc3339())
            }
            OverrideDuration::UntilReboot | OverrideDuration::UntilChanged => None,
        };
        OverrideEntry {
            subsystem,
            relaxed: true,
            duration,
            generation_id,
            started_at: now.to_rfc3339(),
            expires_at,
            timer_unit,
        }
    }

    fn restore_previous_entry(
        &self,
        subsystem: OverrideSubsystem,
        prev_override: Option<OverrideEntry>,
        doc: &mut OverrideStoreDocument,
    ) -> Result<(), AkrepError> {
        self.ops.cancel_revert_timer(subsystem)?;
        let key = subsystem.as_str().to_string();
        match prev_override {
            None => {
                doc.entries.remove(&key);
            }
            Some(mut entry) => {
                if let OverrideDuration::Timed { .. } = entry.duration {
                    let remaining_secs = entry
                        .expires_at
                        .as_deref()
                        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                        .map(|exp| (exp.with_timezone(&Utc) - Utc::now()).num_seconds())
                        .unwrap_or(0);
                    if remaining_secs > 0 {
                        let timer_unit = self.ops.schedule_revert_timer(
                            subsystem,
                            remaining_secs as u64,
                            &entry.generation_id,
                            &self.state_dir,
                        )?;
                        entry.timer_unit = Some(timer_unit);
                        doc.entries.insert(key, entry);
                    } else {
                        doc.entries.remove(&key);
                    }
                } else {
                    entry.timer_unit = None;
                    entry.expires_at = None;
                    doc.entries.insert(key, entry);
                }
            }
        }
        Ok(())
    }

    fn format_duration_label(duration: OverrideDuration) -> String {
        match duration {
            OverrideDuration::Timed { minutes } => format!("timed ({}m)", minutes),
            OverrideDuration::UntilReboot => "until_reboot".to_string(),
            OverrideDuration::UntilChanged => "until_changed".to_string(),
        }
    }

    fn selinux_mode_fingerprint(
        runtime: SelinuxRuntimeMode,
        configured_mode: Option<&str>,
        entry: Option<&OverrideEntry>,
    ) -> StateFingerprint {
        let ov_part = match entry {
            Some(e) => format!(
                "{}:{}",
                Self::format_duration_label(e.duration),
                e.generation_id
            ),
            None => "none".to_string(),
        };
        let payload = format!(
            "runtime={}|config={}|override={}",
            runtime.as_str(),
            configured_mode.unwrap_or("none"),
            ov_part
        );
        StateFingerprint {
            target: "selinux.mode".to_string(),
            sha256: sha256_hex(payload.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn service_fingerprint(
        spec: ServiceOverrideSpec,
        active: bool,
        enabled: bool,
        entry: Option<&OverrideEntry>,
    ) -> StateFingerprint {
        let ov_part = match entry {
            Some(e) => format!(
                "{}:{}",
                Self::format_duration_label(e.duration),
                e.generation_id
            ),
            None => "none".to_string(),
        };
        let payload = format!("active={}|enabled={}|override={}", active, enabled, ov_part);
        StateFingerprint {
            target: spec.fp_target.to_string(),
            sha256: sha256_hex(payload.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn nofile_fingerprint(pam: Option<&str>, systemd: Option<&str>) -> StateFingerprint {
        let payload = format!(
            "pam={}|systemd={}",
            pam.unwrap_or("<absent>"),
            systemd.unwrap_or("<absent>")
        );
        StateFingerprint {
            target: "system.limits.nofile".to_string(),
            sha256: sha256_hex(payload.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn container_dir_fingerprint(canonical_dir: &Path, context: &str) -> StateFingerprint {
        let payload = format!("{}={}", canonical_dir.display(), context);
        StateFingerprint {
            target: format!("selinux.container_dir.{}", canonical_dir.display()),
            sha256: sha256_hex(payload.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn compat_package_fingerprint(group: CompatPackageGroup, installed: bool) -> StateFingerprint {
        let pkgs = group.rpm_packages().join(",");
        let payload = format!("pkgs={}|installed={}", pkgs, installed);
        StateFingerprint {
            target: format!("packages.compat.{}", pkgs),
            sha256: sha256_hex(payload.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn ensure_selinux_not_disabled(&self) -> Result<SelinuxRuntimeMode, AkrepError> {
        let runtime = self.ops.read_selinux_runtime_mode()?;
        if runtime == SelinuxRuntimeMode::Disabled {
            return Err(AkrepError::BackendUnavailable(
                "SELinux is disabled on this system; mode changes are unavailable".to_string(),
            ));
        }
        Ok(runtime)
    }

    fn ensure_selinux_config_mode(&self, desired: SelinuxDesiredMode) -> Result<(), AkrepError> {
        let desired_str = match desired {
            SelinuxDesiredMode::Enforcing => "enforcing",
            SelinuxDesiredMode::Permissive => "permissive",
        };
        match self.ops.read_selinux_config_raw()? {
            Some(raw) => {
                if parse_selinux_config_mode(&raw).as_deref() != Some(desired_str) {
                    let updated = update_selinux_config_content(&raw, desired)?;
                    self.ops.write_selinux_config_raw(&updated)?;
                }
                Ok(())
            }
            None => {
                if desired == SelinuxDesiredMode::Permissive {
                    Err(AkrepError::BackendUnavailable(format!(
                        "{} does not exist; cannot persist SELINUX=permissive",
                        SELINUX_CONFIG_PATH
                    )))
                } else {
                    Ok(())
                }
            }
        }
    }

    pub fn preview_selinux_mode(
        &self,
        mode: SelinuxDesiredMode,
        duration: OverrideDuration,
    ) -> Result<PreviewOutcome, AkrepError> {
        validate_override_duration(duration)?;
        let runtime = self.ensure_selinux_not_disabled()?;
        let raw_cfg = self.ops.read_selinux_config_raw()?;
        let cfg_mode = raw_cfg.as_deref().and_then(parse_selinux_config_mode);

        let _locks = self.lock_store()?;
        let doc = self.load_store_unlocked()?;
        let cur_entry = doc.entries.get(OverrideSubsystem::Selinux.as_str());

        let cur_desc = format!(
            "runtime={}, config={}{}",
            runtime.as_str(),
            cfg_mode.as_deref().unwrap_or("absent"),
            cur_entry
                .map(|e| format!(" ({})", Self::format_duration_label(e.duration)))
                .unwrap_or_default()
        );

        let (desired_desc, risk, mut warnings_tr, mut warnings_en) = match mode {
            SelinuxDesiredMode::Enforcing => (
                "runtime=enforcing, config=enforcing (override cleared)".to_string(),
                RiskLevel::Low,
                Vec::new(),
                Vec::new(),
            ),
            SelinuxDesiredMode::Permissive => {
                let desired_cfg = if matches!(duration, OverrideDuration::UntilChanged) {
                    "permissive"
                } else {
                    "enforcing"
                };
                let (risk, w_tr, w_en) = match duration {
                    OverrideDuration::Timed { minutes } => (
                        RiskLevel::Medium,
                        vec![format!(
                            "SELinux {} dakika boyunca Permissive modda kalacak ve süre dolduğunda otomatik olarak Enforcing moduna dönecek.",
                            minutes
                        )],
                        vec![format!(
                            "SELinux will run in Permissive mode for {} minute(s) and automatically revert to Enforcing.",
                            minutes
                        )],
                    ),
                    OverrideDuration::UntilReboot => (
                        RiskLevel::Medium,
                        vec![
                            "SELinux yalnızca çalışma zamanında Permissive moda geçecek; bir sonraki yeniden başlatmada Enforcing geri yüklenecek."
                                .to_string(),
                        ],
                        vec![
                            "SELinux will remain Permissive until the next reboot, preserving SELINUX=enforcing on disk."
                                .to_string(),
                        ],
                    ),
                    OverrideDuration::UntilChanged => (
                        RiskLevel::High,
                        vec![
                            "Yüksek risk: SELinux siz tekrar Enforcing seçilene kadar yeniden başlatmalar arasında da Permissive kalacak (dosya etiketleri korunur, autorelabel gerekmez)."
                                .to_string(),
                        ],
                        vec![
                            "High risk: SELinux will persist in Permissive mode across reboots until manually re-enabled (file labels remain intact)."
                                .to_string(),
                        ],
                    ),
                };
                (
                    format!(
                        "runtime=permissive, config={} ({})",
                        desired_cfg,
                        Self::format_duration_label(duration)
                    ),
                    risk,
                    w_tr,
                    w_en,
                )
            }
        };

        let mut diffs = vec![PlanDiffItem {
            target: "selinux.mode".to_string(),
            current_value: cur_desc,
            desired_value: desired_desc,
            description_tr: match mode {
                SelinuxDesiredMode::Enforcing => {
                    "SELinux zorlayıcı (Enforcing) moda geri döndürülecek ve etkin gevşetme temizlenecek."
                        .to_string()
                }
                SelinuxDesiredMode::Permissive => {
                    "SELinux modu esnek denetim (Permissive) olarak ayarlanacak.".to_string()
                }
            },
            description_en: match mode {
                SelinuxDesiredMode::Enforcing => {
                    "Restore SELinux to Enforcing mode and clear any active override.".to_string()
                }
                SelinuxDesiredMode::Permissive => {
                    "Switch SELinux mode to Permissive.".to_string()
                }
            },
        }];

        // Surface explicit config reconciliation diff when switching UntilChanged -> Timed/UntilReboot
        if mode == SelinuxDesiredMode::Permissive
            && !matches!(duration, OverrideDuration::UntilChanged)
            && cfg_mode.as_deref() == Some("permissive")
        {
            diffs.push(PlanDiffItem {
                target: SELINUX_CONFIG_PATH.to_string(),
                current_value: "SELINUX=permissive".to_string(),
                desired_value: "SELINUX=enforcing".to_string(),
                description_tr:
                    "Kalıcı yapılandırma SELINUX=enforcing olarak eşitlenecek ki zamanlayıcı veya yeniden başlatma korumayı geri getirsin."
                        .to_string(),
                description_en:
                    "Reconcile persistent config back to SELINUX=enforcing so timer or reboot restores protection."
                        .to_string(),
            });
            warnings_tr.push(
                "Önceki kalıcı Permissive ayarı /etc/selinux/config üzerinde Enforcing olarak eşitlenecek."
                    .to_string(),
            );
            warnings_en.push(
                "Previous persistent Permissive setting in /etc/selinux/config will be reconciled back to Enforcing."
                    .to_string(),
            );
        }

        let fp = Self::selinux_mode_fingerprint(runtime, cfg_mode.as_deref(), cur_entry);
        Ok(PreviewOutcome {
            risk,
            diffs,
            pre_fingerprints: vec![fp],
            warnings_tr,
            warnings_en,
        })
    }

    pub fn apply_selinux_mode(
        &self,
        mode: SelinuxDesiredMode,
        duration: OverrideDuration,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        validate_override_duration(duration)?;
        let prev_runtime = self.ensure_selinux_not_disabled()?;
        let prev_config_raw = self.ops.read_selinux_config_raw()?;

        let _locks = self.lock_store()?;
        let mut doc = self.load_store_unlocked()?;
        let key = OverrideSubsystem::Selinux.as_str().to_string();
        let prev_override = doc.entries.get(&key).cloned();

        let snap = SelinuxModeSnapshot {
            prev_runtime,
            prev_config_raw,
            prev_override,
        };
        write_json_atomic_0600(&snapshot_dir.join("selinux_mode.json"), &snap)?;

        self.ops.cancel_revert_timer(OverrideSubsystem::Selinux)?;

        let (generation_id, expected_cfg_mode) = match mode {
            SelinuxDesiredMode::Enforcing => {
                // Re-enforcing always ignores duration and restores persistent enforcing state.
                self.ensure_selinux_config_mode(SelinuxDesiredMode::Enforcing)?;
                self.ops.setenforce(true)?;
                doc.entries.remove(&key);
                self.save_store_unlocked(&doc)?;
                (None, "enforcing")
            }
            SelinuxDesiredMode::Permissive => {
                let desired_cfg = match duration {
                    OverrideDuration::UntilChanged => SelinuxDesiredMode::Permissive,
                    OverrideDuration::Timed { .. } | OverrideDuration::UntilReboot => {
                        SelinuxDesiredMode::Enforcing
                    }
                };
                self.ensure_selinux_config_mode(desired_cfg)?;
                self.ops.setenforce(false)?;

                let gen_id = generate_uuid_v4()?;
                let timer_unit = match duration {
                    OverrideDuration::Timed { minutes } => {
                        let secs = u64::from(minutes) * 60;
                        let unit = self.ops.schedule_revert_timer(
                            OverrideSubsystem::Selinux,
                            secs,
                            &gen_id,
                            &self.state_dir,
                        )?;
                        Some(unit)
                    }
                    OverrideDuration::UntilReboot | OverrideDuration::UntilChanged => None,
                };

                let entry = Self::build_override_entry(
                    OverrideSubsystem::Selinux,
                    duration,
                    gen_id.clone(),
                    timer_unit,
                );
                doc.entries.insert(key, entry);
                self.save_store_unlocked(&doc)?;
                (
                    Some(gen_id),
                    if desired_cfg == SelinuxDesiredMode::Permissive {
                        "permissive"
                    } else {
                        "enforcing"
                    },
                )
            }
        };

        Ok(ApplyOutcome {
            pre_snapshot: serde_json::to_value(&snap)
                .map_err(|e| AkrepError::Internal(e.to_string()))?,
            target_state: serde_json::json!({
                "subsystem": "selinux",
                "mode": mode,
                "duration": duration,
                "expected_config_mode": expected_cfg_mode,
                "generation_id": generation_id,
            }),
            summary: format!(
                "SELinux mode set to {:?} (duration={})",
                mode,
                Self::format_duration_label(duration)
            ),
        })
    }

    pub fn verify_selinux_mode(
        &self,
        mode: SelinuxDesiredMode,
        duration: OverrideDuration,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let runtime = self.ops.read_selinux_runtime_mode()?;
        let expected_runtime = match mode {
            SelinuxDesiredMode::Enforcing => SelinuxRuntimeMode::Enforcing,
            SelinuxDesiredMode::Permissive => SelinuxRuntimeMode::Permissive,
        };
        if runtime != expected_runtime {
            return Err(AkrepError::VerifyFailed(format!(
                "SELinux runtime mode mismatch (expected {:?}, got {:?})",
                expected_runtime, runtime
            )));
        }

        let raw_cfg = self.ops.read_selinux_config_raw()?;
        let cfg_mode = raw_cfg.as_deref().and_then(parse_selinux_config_mode);
        if let Some(exp_cfg) = expected
            .target_state
            .get("expected_config_mode")
            .and_then(|v| v.as_str())
        {
            if raw_cfg.is_some() && cfg_mode.as_deref() != Some(exp_cfg) {
                return Err(AkrepError::VerifyFailed(format!(
                    "{} mode mismatch (expected {}, got {:?})",
                    SELINUX_CONFIG_PATH, exp_cfg, cfg_mode
                )));
            }
        }

        let _locks = self.lock_store()?;
        let doc = self.load_store_unlocked()?;
        let entry = doc.entries.get(OverrideSubsystem::Selinux.as_str());
        match mode {
            SelinuxDesiredMode::Enforcing => {
                if entry.is_some() {
                    return Err(AkrepError::VerifyFailed(
                        "SELinux override entry remained in store after Enforcing restore"
                            .to_string(),
                    ));
                }
            }
            SelinuxDesiredMode::Permissive => {
                let Some(e) = entry else {
                    return Err(AkrepError::VerifyFailed(
                        "Missing SELinux override entry in store after Permissive apply"
                            .to_string(),
                    ));
                };
                let exp_gen = expected
                    .target_state
                    .get("generation_id")
                    .and_then(|v| v.as_str());
                if e.duration != duration
                    || (exp_gen.is_some() && Some(e.generation_id.as_str()) != exp_gen)
                {
                    return Err(AkrepError::VerifyFailed(
                        "SELinux override entry duration or generation_id mismatch".to_string(),
                    ));
                }
            }
        }

        Ok(vec![Self::selinux_mode_fingerprint(
            runtime,
            cfg_mode.as_deref(),
            entry,
        )])
    }

    pub fn current_selinux_mode_fingerprints(&self) -> Result<Vec<StateFingerprint>, AkrepError> {
        let runtime = self.ops.read_selinux_runtime_mode()?;
        let raw_cfg = self.ops.read_selinux_config_raw()?;
        let cfg_mode = raw_cfg.as_deref().and_then(parse_selinux_config_mode);
        let _locks = self.lock_store()?;
        let doc = self.load_store_unlocked()?;
        let entry = doc.entries.get(OverrideSubsystem::Selinux.as_str());
        Ok(vec![Self::selinux_mode_fingerprint(
            runtime,
            cfg_mode.as_deref(),
            entry,
        )])
    }

    pub fn rollback_selinux_mode(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let snap_path = snapshot_dir.join("selinux_mode.json");
        if !snap_path.is_file() && record.pre_snapshot.is_null() {
            return Ok("SELinux mode mutation aborted before snapshot".to_string());
        }
        let snap: SelinuxModeSnapshot = if snap_path.is_file() {
            serde_json::from_str(&fs::read_to_string(&snap_path)?)
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        } else {
            serde_json::from_value(record.pre_snapshot.clone())
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        };

        let _locks = self.lock_store()?;
        let mut doc = self.load_store_unlocked()?;

        if let Some(prev_cfg) = &snap.prev_config_raw {
            self.ops.write_selinux_config_raw(prev_cfg)?;
        }
        match snap.prev_runtime {
            SelinuxRuntimeMode::Enforcing => self.ops.setenforce(true)?,
            SelinuxRuntimeMode::Permissive => self.ops.setenforce(false)?,
            SelinuxRuntimeMode::Disabled => {}
        }
        self.restore_previous_entry(
            OverrideSubsystem::Selinux,
            snap.prev_override.clone(),
            &mut doc,
        )?;
        self.save_store_unlocked(&doc)?;
        Ok(format!(
            "Rolled back SELinux mode to {:?}",
            snap.prev_runtime
        ))
    }

    fn preview_service_override(
        &self,
        spec: ServiceOverrideSpec,
        enabled: bool,
        duration: OverrideDuration,
    ) -> Result<PreviewOutcome, AkrepError> {
        validate_override_duration(duration)?;
        let cur_active = self.ops.is_unit_active(spec.unit)?;
        let cur_enabled = self.ops.is_unit_enabled(spec.unit)?;

        let _locks = self.lock_store()?;
        let doc = self.load_store_unlocked()?;
        let cur_entry = doc.entries.get(spec.subsystem.as_str());

        let cur_desc = format!(
            "active={}, enabled={}{}",
            cur_active,
            cur_enabled,
            cur_entry
                .map(|e| format!(" ({})", Self::format_duration_label(e.duration)))
                .unwrap_or_default()
        );

        let (desired_desc, risk, mut warnings_tr, mut warnings_en) = if enabled {
            (
                "active=true, enabled=true (override cleared)".to_string(),
                RiskLevel::Low,
                Vec::new(),
                Vec::new(),
            )
        } else {
            let desired_enabled = !matches!(duration, OverrideDuration::UntilChanged);
            let is_fw = spec.subsystem == OverrideSubsystem::Firewalld;
            let (risk, w_tr, w_en) = match duration {
                OverrideDuration::Timed { minutes } => (
                    RiskLevel::Medium,
                    vec![format!(
                        "{} {} dakika boyunca duraklatılacak ve süre sonunda otomatik yeniden başlatılacak.",
                        spec.label_tr, minutes
                    )],
                    vec![format!(
                        "{} will be paused for {} minute(s) and automatically re-enabled.",
                        spec.label_en, minutes
                    )],
                ),
                OverrideDuration::UntilReboot => (
                    RiskLevel::Medium,
                    vec![format!(
                        "{} yalnızca bu oturumda duraklatılacak; bir sonraki yeniden başlatmada yeniden açılacak.",
                        spec.label_tr
                    )],
                    vec![format!(
                        "{} will remain paused until next reboot and remain enabled on boot.",
                        spec.label_en
                    )],
                ),
                OverrideDuration::UntilChanged => (
                    if is_fw {
                        RiskLevel::High
                    } else {
                        RiskLevel::Medium
                    },
                    vec![format!(
                        "{} siz tekrar etkinleştirene kadar yeniden başlatmalar arasında da kapalı kalacak.",
                        spec.label_tr
                    )],
                    vec![format!(
                        "{} will remain disabled across reboots until manually re-enabled.",
                        spec.label_en
                    )],
                ),
            };
            (
                format!(
                    "active=false, enabled={} ({})",
                    desired_enabled,
                    Self::format_duration_label(duration)
                ),
                risk,
                w_tr,
                w_en,
            )
        };

        let mut diffs = vec![PlanDiffItem {
            target: spec.fp_target.to_string(),
            current_value: cur_desc,
            desired_value: desired_desc,
            description_tr: if enabled {
                format!(
                    "{} etkinleştirilecek ve gevşetme temizlenecek.",
                    spec.label_tr
                )
            } else {
                format!("{} duraklatılacak.", spec.label_tr)
            },
            description_en: if enabled {
                format!("Enable {} and clear active override.", spec.label_en)
            } else {
                format!("Pause {}.", spec.label_en)
            },
        }];

        if !enabled && !matches!(duration, OverrideDuration::UntilChanged) && !cur_enabled {
            diffs.push(PlanDiffItem {
                target: format!("systemd.unit.{}", spec.unit),
                current_value: "disabled".to_string(),
                desired_value: "enabled".to_string(),
                description_tr: format!(
                    "{} kalıcı servis durumu etkinleştirilerek zamanlayıcı/yeniden başlatmada geri dönüş güvence altına alınacak.",
                    spec.unit
                ),
                description_en: format!(
                    "Re-enable {} persistent unit so timer or reboot restores protection.",
                    spec.unit
                ),
            });
            warnings_tr.push(format!(
                "{} yeniden başlatma sonrasında açılması için kalıcı olarak etkinleştirilecek.",
                spec.unit
            ));
            warnings_en.push(format!(
                "{} will be persistently enabled for restoration on reboot.",
                spec.unit
            ));
        }

        let fp = Self::service_fingerprint(spec, cur_active, cur_enabled, cur_entry);
        Ok(PreviewOutcome {
            risk,
            diffs,
            pre_fingerprints: vec![fp],
            warnings_tr,
            warnings_en,
        })
    }

    fn apply_service_override(
        &self,
        spec: ServiceOverrideSpec,
        enabled: bool,
        duration: OverrideDuration,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        validate_override_duration(duration)?;
        let prev_active = self.ops.is_unit_active(spec.unit)?;
        let prev_enabled = self.ops.is_unit_enabled(spec.unit)?;

        let _locks = self.lock_store()?;
        let mut doc = self.load_store_unlocked()?;
        let key = spec.subsystem.as_str().to_string();
        let prev_override = doc.entries.get(&key).cloned();

        let snap = ServiceOverrideSnapshot {
            unit: spec.unit.to_string(),
            prev_active,
            prev_enabled,
            prev_override,
        };
        let snap_filename = format!("service_{}.json", spec.subsystem.as_str());
        write_json_atomic_0600(&snapshot_dir.join(snap_filename), &snap)?;

        self.ops.cancel_revert_timer(spec.subsystem)?;

        let (generation_id, desired_enabled) = if enabled {
            // Re-enabling always ignores duration and restores persistent + runtime state.
            if !prev_enabled {
                self.ops.enable_unit(spec.unit)?;
            }
            if !prev_active {
                self.ops.start_unit(spec.unit)?;
            }
            doc.entries.remove(&key);
            self.save_store_unlocked(&doc)?;
            (None, true)
        } else {
            let persist_enabled = !matches!(duration, OverrideDuration::UntilChanged);
            if persist_enabled && !prev_enabled {
                self.ops.enable_unit(spec.unit)?;
            } else if !persist_enabled && prev_enabled {
                self.ops.disable_unit(spec.unit)?;
            }
            if prev_active {
                self.ops.stop_unit(spec.unit)?;
            }

            let gen_id = generate_uuid_v4()?;
            let timer_unit = match duration {
                OverrideDuration::Timed { minutes } => {
                    let secs = u64::from(minutes) * 60;
                    let unit = self.ops.schedule_revert_timer(
                        spec.subsystem,
                        secs,
                        &gen_id,
                        &self.state_dir,
                    )?;
                    Some(unit)
                }
                OverrideDuration::UntilReboot | OverrideDuration::UntilChanged => None,
            };

            let entry =
                Self::build_override_entry(spec.subsystem, duration, gen_id.clone(), timer_unit);
            doc.entries.insert(key, entry);
            self.save_store_unlocked(&doc)?;
            (Some(gen_id), persist_enabled)
        };

        Ok(ApplyOutcome {
            pre_snapshot: serde_json::to_value(&snap)
                .map_err(|e| AkrepError::Internal(e.to_string()))?,
            target_state: serde_json::json!({
                "subsystem": spec.subsystem.as_str(),
                "unit": spec.unit,
                "desired_active": enabled,
                "desired_enabled": desired_enabled,
                "duration": duration,
                "generation_id": generation_id,
            }),
            summary: format!(
                "{} {} (duration={})",
                spec.unit,
                if enabled { "enabled" } else { "paused" },
                Self::format_duration_label(duration)
            ),
        })
    }

    fn verify_service_override(
        &self,
        spec: ServiceOverrideSpec,
        enabled: bool,
        duration: OverrideDuration,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let active = self.ops.is_unit_active(spec.unit)?;
        let unit_enabled = self.ops.is_unit_enabled(spec.unit)?;

        if active != enabled {
            return Err(AkrepError::VerifyFailed(format!(
                "{} active state mismatch (expected {}, got {})",
                spec.unit, enabled, active
            )));
        }
        let exp_enabled = expected
            .target_state
            .get("desired_enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(!matches!(duration, OverrideDuration::UntilChanged) || enabled);
        if unit_enabled != exp_enabled {
            return Err(AkrepError::VerifyFailed(format!(
                "{} enabled state mismatch (expected {}, got {})",
                spec.unit, exp_enabled, unit_enabled
            )));
        }

        let _locks = self.lock_store()?;
        let doc = self.load_store_unlocked()?;
        let entry = doc.entries.get(spec.subsystem.as_str());
        if enabled {
            if entry.is_some() {
                return Err(AkrepError::VerifyFailed(format!(
                    "{} override entry remained after re-enable",
                    spec.subsystem.as_str()
                )));
            }
        } else {
            let Some(e) = entry else {
                return Err(AkrepError::VerifyFailed(format!(
                    "Missing {} override entry after pause",
                    spec.subsystem.as_str()
                )));
            };
            let exp_gen = expected
                .target_state
                .get("generation_id")
                .and_then(|v| v.as_str());
            if e.duration != duration
                || (exp_gen.is_some() && Some(e.generation_id.as_str()) != exp_gen)
            {
                return Err(AkrepError::VerifyFailed(format!(
                    "{} override duration or generation_id mismatch",
                    spec.subsystem.as_str()
                )));
            }
        }

        Ok(vec![Self::service_fingerprint(
            spec,
            active,
            unit_enabled,
            entry,
        )])
    }

    fn current_service_override_fingerprints(
        &self,
        spec: ServiceOverrideSpec,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let active = self.ops.is_unit_active(spec.unit)?;
        let enabled = self.ops.is_unit_enabled(spec.unit)?;
        let _locks = self.lock_store()?;
        let doc = self.load_store_unlocked()?;
        let entry = doc.entries.get(spec.subsystem.as_str());
        Ok(vec![Self::service_fingerprint(
            spec, active, enabled, entry,
        )])
    }

    fn rollback_service_override(
        &self,
        spec: ServiceOverrideSpec,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let snap_path = snapshot_dir.join(format!("service_{}.json", spec.subsystem.as_str()));
        if !snap_path.is_file() && record.pre_snapshot.is_null() {
            return Ok(format!("{} mutation aborted before snapshot", spec.unit));
        }
        let snap: ServiceOverrideSnapshot = if snap_path.is_file() {
            serde_json::from_str(&fs::read_to_string(&snap_path)?)
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        } else {
            serde_json::from_value(record.pre_snapshot.clone())
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        };

        let _locks = self.lock_store()?;
        let mut doc = self.load_store_unlocked()?;

        if snap.prev_enabled {
            self.ops.enable_unit(spec.unit)?;
        } else {
            self.ops.disable_unit(spec.unit)?;
        }
        if snap.prev_active {
            self.ops.start_unit(spec.unit)?;
        } else {
            self.ops.stop_unit(spec.unit)?;
        }

        self.restore_previous_entry(spec.subsystem, snap.prev_override.clone(), &mut doc)?;
        self.save_store_unlocked(&doc)?;
        Ok(format!(
            "Rolled back {} (active={}, enabled={})",
            spec.unit, snap.prev_active, snap.prev_enabled
        ))
    }

    pub fn preview_firewall_state(
        &self,
        enabled: bool,
        duration: OverrideDuration,
    ) -> Result<PreviewOutcome, AkrepError> {
        self.preview_service_override(FIREWALLD_SPEC, enabled, duration)
    }

    pub fn apply_firewall_state(
        &self,
        enabled: bool,
        duration: OverrideDuration,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        self.apply_service_override(FIREWALLD_SPEC, enabled, duration, snapshot_dir)
    }

    pub fn verify_firewall_state(
        &self,
        enabled: bool,
        duration: OverrideDuration,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        self.verify_service_override(FIREWALLD_SPEC, enabled, duration, expected)
    }

    pub fn current_firewall_state_fingerprints(&self) -> Result<Vec<StateFingerprint>, AkrepError> {
        self.current_service_override_fingerprints(FIREWALLD_SPEC)
    }

    pub fn rollback_firewall_state(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        self.rollback_service_override(FIREWALLD_SPEC, record, snapshot_dir)
    }

    pub fn preview_oomd_state(
        &self,
        enabled: bool,
        duration: OverrideDuration,
    ) -> Result<PreviewOutcome, AkrepError> {
        self.preview_service_override(OOMD_SPEC, enabled, duration)
    }

    pub fn apply_oomd_state(
        &self,
        enabled: bool,
        duration: OverrideDuration,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        self.apply_service_override(OOMD_SPEC, enabled, duration, snapshot_dir)
    }

    pub fn verify_oomd_state(
        &self,
        enabled: bool,
        duration: OverrideDuration,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        self.verify_service_override(OOMD_SPEC, enabled, duration, expected)
    }

    pub fn current_oomd_state_fingerprints(&self) -> Result<Vec<StateFingerprint>, AkrepError> {
        self.current_service_override_fingerprints(OOMD_SPEC)
    }

    pub fn rollback_oomd_state(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        self.rollback_service_override(OOMD_SPEC, record, snapshot_dir)
    }

    pub fn preview_nofile_limits(
        &self,
        enabled: bool,
        soft_limit: u32,
        hard_limit: u32,
    ) -> Result<PreviewOutcome, AkrepError> {
        validate_nofile_limits(soft_limit, hard_limit)?;
        let (pam_raw, sysd_raw) = self.ops.read_nofile_dropins()?;
        let cur_desc = match parse_nofile_dropins(pam_raw.as_deref(), sysd_raw.as_deref()) {
            Some((s, h)) => format!("{}:{} (managed)", s, h),
            None if pam_raw.is_some() || sysd_raw.is_some() => "custom".to_string(),
            None => "default".to_string(),
        };

        let mut warnings_tr = Vec::new();
        let mut warnings_en = Vec::new();
        if enabled && hard_limit < 524_288 {
            warnings_tr.push(
                "Fedora varsayılan sert sınırı (524288) altına inmek Wine/Proton esync performansını etkileyebilir."
                    .to_string(),
            );
            warnings_en.push(
                "Lowering hard_limit below Fedora's 524288 default may affect Wine/Proton esync compatibility."
                    .to_string(),
            );
        }

        let desired_desc = if enabled {
            format!("{}:{} (PAM + systemd user)", soft_limit, hard_limit)
        } else {
            "default (drop-ins removed)".to_string()
        };

        let fp = Self::nofile_fingerprint(pam_raw.as_deref(), sysd_raw.as_deref());
        Ok(PreviewOutcome {
            risk: RiskLevel::Low,
            diffs: vec![PlanDiffItem {
                target: "system.limits.nofile".to_string(),
                current_value: cur_desc,
                desired_value: desired_desc,
                description_tr: if enabled {
                    format!(
                        "Açık dosya sınırı drop-in dosyaları ({}/{}) yazılacak ve systemd daemon-reload çalıştırılacak.",
                        soft_limit, hard_limit
                    )
                } else {
                    "Akrep açık dosya sınırı drop-in dosyaları kaldırılacak ve systemd varsayılanına dönülecek."
                        .to_string()
                },
                description_en: if enabled {
                    format!(
                        "Write managed nofile drop-ins ({}/{}) and run systemctl daemon-reload.",
                        soft_limit, hard_limit
                    )
                } else {
                    "Remove managed nofile drop-in files and run systemctl daemon-reload."
                        .to_string()
                },
            }],
            pre_fingerprints: vec![fp],
            warnings_tr,
            warnings_en,
        })
    }

    pub fn apply_nofile_limits(
        &self,
        enabled: bool,
        soft_limit: u32,
        hard_limit: u32,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        validate_nofile_limits(soft_limit, hard_limit)?;
        let (prev_pam_content, prev_systemd_content) = self.ops.read_nofile_dropins()?;
        let snap = NofileLimitsSnapshot {
            prev_pam_content,
            prev_systemd_content,
        };
        write_json_atomic_0600(&snapshot_dir.join("nofile_limits.json"), &snap)?;

        if enabled {
            let pam = format_pam_nofile_dropin(soft_limit, hard_limit);
            let sysd = format_systemd_nofile_dropin(soft_limit, hard_limit);
            self.ops.write_nofile_dropins(&pam, &sysd)?;
        } else {
            self.ops.remove_nofile_dropins()?;
        }
        self.ops.daemon_reload()?;

        Ok(ApplyOutcome {
            pre_snapshot: serde_json::to_value(&snap)
                .map_err(|e| AkrepError::Internal(e.to_string()))?,
            target_state: serde_json::json!({
                "enabled": enabled,
                "soft_limit": soft_limit,
                "hard_limit": hard_limit,
            }),
            summary: if enabled {
                format!(
                    "Configured managed nofile limits {}:{}",
                    soft_limit, hard_limit
                )
            } else {
                "Removed managed nofile limits drop-ins".to_string()
            },
        })
    }

    pub fn verify_nofile_limits(
        &self,
        enabled: bool,
        soft_limit: u32,
        hard_limit: u32,
        _expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let (pam_raw, sysd_raw) = self.ops.read_nofile_dropins()?;
        if enabled {
            let exp_pam = format_pam_nofile_dropin(soft_limit, hard_limit);
            let exp_sysd = format_systemd_nofile_dropin(soft_limit, hard_limit);
            if pam_raw.as_deref() != Some(exp_pam.as_str())
                || sysd_raw.as_deref() != Some(exp_sysd.as_str())
            {
                return Err(AkrepError::VerifyFailed(
                    "nofile drop-in contents mismatch after apply".to_string(),
                ));
            }
        } else if pam_raw.is_some() || sysd_raw.is_some() {
            return Err(AkrepError::VerifyFailed(
                "nofile drop-in files still exist after removal".to_string(),
            ));
        }
        Ok(vec![Self::nofile_fingerprint(
            pam_raw.as_deref(),
            sysd_raw.as_deref(),
        )])
    }

    pub fn current_nofile_limits_fingerprints(&self) -> Result<Vec<StateFingerprint>, AkrepError> {
        let (pam_raw, sysd_raw) = self.ops.read_nofile_dropins()?;
        Ok(vec![Self::nofile_fingerprint(
            pam_raw.as_deref(),
            sysd_raw.as_deref(),
        )])
    }

    pub fn rollback_nofile_limits(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let snap_path = snapshot_dir.join("nofile_limits.json");
        if !snap_path.is_file() && record.pre_snapshot.is_null() {
            return Ok("nofile limits mutation aborted before snapshot".to_string());
        }
        let snap: NofileLimitsSnapshot = if snap_path.is_file() {
            serde_json::from_str(&fs::read_to_string(&snap_path)?)
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        } else {
            serde_json::from_value(record.pre_snapshot.clone())
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        };

        self.ops.restore_nofile_dropins(
            snap.prev_pam_content.as_deref(),
            snap.prev_systemd_content.as_deref(),
        )?;
        self.ops.daemon_reload()?;
        Ok("Rolled back managed nofile drop-in configuration".to_string())
    }

    pub fn preview_container_dir_label(
        &self,
        target_path: &str,
        recursive: bool,
    ) -> Result<PreviewOutcome, AkrepError> {
        let _ = self.ensure_selinux_not_disabled()?;
        let normalized = validate_container_dir_path(target_path)?;
        let canonical = self.ops.resolve_container_dir(Path::new(&normalized))?;
        let current_ctx = self.ops.read_path_selinux_context(&canonical)?;
        let fp = Self::container_dir_fingerprint(&canonical, &current_ctx);

        Ok(PreviewOutcome {
            risk: RiskLevel::Medium,
            diffs: vec![PlanDiffItem {
                target: canonical.to_string_lossy().to_string(),
                current_value: current_ctx,
                desired_value: format!("type=container_file_t (recursive={})", recursive),
                description_tr: format!(
                    "'{}' çalışma klasörü SELinux konteyner/derleme bağlamı (container_file_t) ile etiketlenecek (sembolik bağlar izlenmez).",
                    canonical.display()
                ),
                description_en: format!(
                    "Label workspace directory '{}' as container_file_t without following symlinks.",
                    canonical.display()
                ),
            }],
            pre_fingerprints: vec![fp],
            warnings_tr: Vec::new(),
            warnings_en: Vec::new(),
        })
    }

    pub fn apply_container_dir_label(
        &self,
        target_path: &str,
        recursive: bool,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        let _ = self.ensure_selinux_not_disabled()?;
        let normalized = validate_container_dir_path(target_path)?;
        let canonical = self.ops.resolve_container_dir(Path::new(&normalized))?;
        let previous_context = self.ops.read_path_selinux_context(&canonical)?;

        let snap = ContainerDirLabelSnapshot {
            canonical_path: canonical.to_string_lossy().to_string(),
            recursive,
            previous_context,
        };
        write_json_atomic_0600(&snapshot_dir.join("container_dir_label.json"), &snap)?;

        self.ops.chcon_container_dir(&canonical, recursive)?;

        Ok(ApplyOutcome {
            pre_snapshot: serde_json::to_value(&snap)
                .map_err(|e| AkrepError::Internal(e.to_string()))?,
            target_state: serde_json::json!({
                "canonical_path": canonical.to_string_lossy().to_string(),
                "recursive": recursive,
                "target_type": "container_file_t",
            }),
            summary: format!(
                "Labeled container workspace '{}' with container_file_t (recursive={})",
                canonical.display(),
                recursive
            ),
        })
    }

    pub fn verify_container_dir_label(
        &self,
        target_path: &str,
        _recursive: bool,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let canonical = if let Some(p) = expected
            .target_state
            .get("canonical_path")
            .and_then(|v| v.as_str())
        {
            self.ops.resolve_container_dir(Path::new(p))?
        } else {
            let normalized = validate_container_dir_path(target_path)?;
            self.ops.resolve_container_dir(Path::new(&normalized))?
        };
        let after_ctx = self.ops.read_path_selinux_context(&canonical)?;
        let selinux_type = after_ctx.split(':').nth(2).unwrap_or("");
        if selinux_type != "container_file_t" {
            return Err(AkrepError::VerifyFailed(format!(
                "Expected SELinux type 'container_file_t' on '{}', got '{}'",
                canonical.display(),
                after_ctx
            )));
        }
        // Strictly fingerprint ONLY the canonical root directory context (never child files).
        Ok(vec![Self::container_dir_fingerprint(
            &canonical, &after_ctx,
        )])
    }

    pub fn current_container_dir_label_fingerprints(
        &self,
        target_path: &str,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let normalized = validate_container_dir_path(target_path)?;
        let canonical = self.ops.resolve_container_dir(Path::new(&normalized))?;
        let ctx = self.ops.read_path_selinux_context(&canonical)?;
        Ok(vec![Self::container_dir_fingerprint(&canonical, &ctx)])
    }

    pub fn rollback_container_dir_label(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let snap_path = snapshot_dir.join("container_dir_label.json");
        if !snap_path.is_file() && record.pre_snapshot.is_null() {
            return Ok("Container dir label mutation aborted before snapshot".to_string());
        }
        let snap: ContainerDirLabelSnapshot = if snap_path.is_file() {
            serde_json::from_str(&fs::read_to_string(&snap_path)?)
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        } else {
            serde_json::from_value(record.pre_snapshot.clone())
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        };

        let normalized = validate_container_dir_path(&snap.canonical_path)?;
        let canonical = self.ops.resolve_container_dir(Path::new(&normalized))?;
        self.ops
            .restorecon_container_dir(&canonical, snap.recursive)?;
        Ok(format!(
            "Restored default SELinux contexts on '{}' (recursive={})",
            canonical.display(),
            snap.recursive
        ))
    }

    pub fn preview_compat_package(
        &self,
        package_group: CompatPackageGroup,
    ) -> Result<PreviewOutcome, AkrepError> {
        let pkgs = package_group.rpm_packages();
        let installed = self.ops.is_packages_installed(pkgs)?;
        let fp = Self::compat_package_fingerprint(package_group, installed);
        let joined = pkgs.join(", ");

        let (desc_tr, desc_en) = match package_group {
            CompatPackageGroup::AndroidUdevRules => (
                "Android ADB/Fastboot cihaz kuralları (android-udev-rules) DNF5 ile kurulacak ve udev kuralları yenilenecek."
                    .to_string(),
                "Install android-udev-rules via DNF5 and reload/trigger udev rules.".to_string(),
            ),
            CompatPackageGroup::AndroidLegacyLibs => (
                "Eski Android derleme araçları için ncurses-compat-libs paketi DNF5 ile kurulacak."
                    .to_string(),
                "Install ncurses-compat-libs via DNF5 for legacy Android build tools.".to_string(),
            ),
        };

        Ok(PreviewOutcome {
            risk: RiskLevel::Low,
            diffs: vec![PlanDiffItem {
                target: format!("packages.compat.{}", joined),
                current_value: if installed {
                    "installed".to_string()
                } else {
                    "absent".to_string()
                },
                desired_value: format!("installed ({})", joined),
                description_tr: desc_tr,
                description_en: desc_en,
            }],
            pre_fingerprints: vec![fp],
            warnings_tr: Vec::new(),
            warnings_en: Vec::new(),
        })
    }

    pub fn apply_compat_package(
        &self,
        package_group: CompatPackageGroup,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        let pkgs = package_group.rpm_packages();
        let previously_installed = self.ops.is_packages_installed(pkgs)?;
        let snap = CompatPackageSnapshot {
            package_group,
            previously_installed,
        };
        write_json_atomic_0600(&snapshot_dir.join("compat_package.json"), &snap)?;

        if !previously_installed {
            self.ops.install_compat_packages(package_group)?;
        }

        Ok(ApplyOutcome {
            pre_snapshot: serde_json::to_value(&snap)
                .map_err(|e| AkrepError::Internal(e.to_string()))?,
            target_state: serde_json::json!({
                "package_group": package_group,
                "packages": pkgs,
                "installed": true,
            }),
            summary: format!(
                "Ensured compatibility package(s) installed: {}",
                pkgs.join(", ")
            ),
        })
    }

    pub fn verify_compat_package(
        &self,
        package_group: CompatPackageGroup,
        _expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let pkgs = package_group.rpm_packages();
        let installed = self.ops.is_packages_installed(pkgs)?;
        if !installed {
            return Err(AkrepError::VerifyFailed(format!(
                "Compatibility package(s) {} not installed after apply",
                pkgs.join(", ")
            )));
        }
        Ok(vec![Self::compat_package_fingerprint(package_group, true)])
    }

    pub fn current_compat_package_fingerprints(
        &self,
        package_group: CompatPackageGroup,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let pkgs = package_group.rpm_packages();
        let installed = self.ops.is_packages_installed(pkgs)?;
        Ok(vec![Self::compat_package_fingerprint(
            package_group,
            installed,
        )])
    }

    pub fn rollback_compat_package(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let snap_path = snapshot_dir.join("compat_package.json");
        if !snap_path.is_file() && record.pre_snapshot.is_null() {
            return Ok("Compatibility package install aborted before snapshot".to_string());
        }
        let snap: CompatPackageSnapshot = if snap_path.is_file() {
            serde_json::from_str(&fs::read_to_string(&snap_path)?)
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        } else {
            serde_json::from_value(record.pre_snapshot.clone())
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        };
        if snap.previously_installed {
            return Ok(
                "Compatibility packages were already installed prior to transaction; keeping installed"
                    .to_string(),
            );
        }
        self.ops.remove_compat_packages(snap.package_group)?;
        Ok(format!(
            "Removed compatibility package(s) {:?} on rollback",
            snap.package_group.rpm_packages()
        ))
    }

    pub fn force_restore_for_profile(
        &self,
        restore_selinux: bool,
        restore_firewalld: bool,
    ) -> Result<ProfileOverridePreState, AkrepError> {
        let _locks = self.lock_store()?;

        let selinux_prev_runtime = if restore_selinux {
            Some(self.ensure_selinux_not_disabled()?)
        } else {
            None
        };
        let selinux_prev_config_raw = if restore_selinux {
            self.ops.read_selinux_config_raw()?
        } else {
            None
        };
        let firewalld_prev_active = if restore_firewalld {
            Some(self.ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT)?)
        } else {
            None
        };
        let firewalld_prev_enabled = if restore_firewalld {
            Some(self.ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT)?)
        } else {
            None
        };

        let mut doc = self.load_store_unlocked()?;
        let selinux_prev_override = if restore_selinux {
            doc.entries
                .get(OverrideSubsystem::Selinux.as_str())
                .cloned()
        } else {
            None
        };
        let firewalld_prev_override = if restore_firewalld {
            doc.entries
                .get(OverrideSubsystem::Firewalld.as_str())
                .cloned()
        } else {
            None
        };

        let selinux_needs_restore = restore_selinux
            && (selinux_prev_runtime == Some(SelinuxRuntimeMode::Permissive)
                || selinux_prev_config_raw
                    .as_deref()
                    .and_then(parse_selinux_config_mode)
                    .as_deref()
                    == Some("permissive")
                || selinux_prev_override.is_some());

        let firewalld_needs_restore = restore_firewalld
            && (firewalld_prev_active == Some(false)
                || firewalld_prev_enabled == Some(false)
                || firewalld_prev_override.is_some());

        let mut pre = ProfileOverridePreState {
            selinux_restored: false,
            selinux_prev_runtime,
            selinux_prev_config_raw,
            selinux_prev_override,
            firewalld_restored: false,
            firewalld_prev_active,
            firewalld_prev_enabled,
            firewalld_prev_override,
        };

        if selinux_needs_restore {
            pre.selinux_restored = true;
            let sel_res = (|| -> Result<(), AkrepError> {
                self.ops.cancel_revert_timer(OverrideSubsystem::Selinux)?;
                self.ensure_selinux_config_mode(SelinuxDesiredMode::Enforcing)?;
                if selinux_prev_runtime != Some(SelinuxRuntimeMode::Enforcing) {
                    self.ops.setenforce(true)?;
                }
                Ok(())
            })();
            if let Err(err) = sel_res {
                let _ = self.rollback_profile_override_restore_unlocked(&pre, &mut doc);
                return Err(err);
            }
            doc.entries.remove(OverrideSubsystem::Selinux.as_str());
        }

        if firewalld_needs_restore {
            pre.firewalld_restored = true;
            let fw_res = (|| -> Result<(), AkrepError> {
                self.ops.cancel_revert_timer(OverrideSubsystem::Firewalld)?;
                if firewalld_prev_enabled == Some(false) {
                    self.ops.enable_unit(SYSTEMD_FIREWALLD_UNIT)?;
                }
                if firewalld_prev_active == Some(false) {
                    self.ops.start_unit(SYSTEMD_FIREWALLD_UNIT)?;
                }
                Ok(())
            })();
            if let Err(err) = fw_res {
                let _ = self.rollback_profile_override_restore_unlocked(&pre, &mut doc);
                return Err(err);
            }
            doc.entries.remove(OverrideSubsystem::Firewalld.as_str());
        }

        if pre.selinux_restored || pre.firewalld_restored {
            if let Err(err) = self.save_store_unlocked(&doc) {
                let _ = self.rollback_profile_override_restore_unlocked(&pre, &mut doc);
                return Err(err);
            }
        }
        Ok(pre)
    }

    fn rollback_profile_override_restore_unlocked(
        &self,
        pre: &ProfileOverridePreState,
        doc: &mut OverrideStoreDocument,
    ) -> Result<(), AkrepError> {
        if !pre.selinux_restored && !pre.firewalld_restored {
            return Ok(());
        }

        let mut first_err: Option<AkrepError> = None;
        let mut record_err = |res: Result<(), AkrepError>| {
            if let Err(err) = res {
                if first_err.is_none() {
                    first_err = Some(err);
                }
            }
        };

        if pre.firewalld_restored {
            if let Some(was_enabled) = pre.firewalld_prev_enabled {
                if self.ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT).ok() != Some(was_enabled) {
                    record_err(if was_enabled {
                        self.ops.enable_unit(SYSTEMD_FIREWALLD_UNIT)
                    } else {
                        self.ops.disable_unit(SYSTEMD_FIREWALLD_UNIT)
                    });
                }
            }
            if let Some(was_active) = pre.firewalld_prev_active {
                if self.ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT).ok() != Some(was_active) {
                    record_err(if was_active {
                        self.ops.start_unit(SYSTEMD_FIREWALLD_UNIT)
                    } else {
                        self.ops.stop_unit(SYSTEMD_FIREWALLD_UNIT)
                    });
                }
            }
            record_err(self.restore_previous_entry(
                OverrideSubsystem::Firewalld,
                pre.firewalld_prev_override.clone(),
                doc,
            ));
        }

        if pre.selinux_restored {
            if let Some(prev_cfg) = &pre.selinux_prev_config_raw {
                if self.ops.read_selinux_config_raw().ok().flatten().as_deref()
                    != Some(prev_cfg.as_str())
                {
                    record_err(self.ops.write_selinux_config_raw(prev_cfg));
                }
            }
            if let Some(prev_rt) = pre.selinux_prev_runtime {
                if self.ops.read_selinux_runtime_mode().ok() != Some(prev_rt) {
                    match prev_rt {
                        SelinuxRuntimeMode::Enforcing => record_err(self.ops.setenforce(true)),
                        SelinuxRuntimeMode::Permissive => record_err(self.ops.setenforce(false)),
                        SelinuxRuntimeMode::Disabled => {}
                    }
                }
            }
            record_err(self.restore_previous_entry(
                OverrideSubsystem::Selinux,
                pre.selinux_prev_override.clone(),
                doc,
            ));
        }

        record_err(self.save_store_unlocked(doc));
        match first_err {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    pub fn rollback_profile_override_restore(
        &self,
        pre: &ProfileOverridePreState,
    ) -> Result<(), AkrepError> {
        if !pre.selinux_restored && !pre.firewalld_restored {
            return Ok(());
        }

        let _locks = self.lock_store()?;
        let mut doc = self.load_store_unlocked()?;
        self.rollback_profile_override_restore_unlocked(pre, &mut doc)
    }

    /// Reverts a timed override only if `generation_id` matches the active `Timed` entry.
    ///
    /// Returns `Ok(false)` as a race-safe no-op when the override was already cleared or switched
    /// to another duration/generation (e.g. `UntilChanged`).
    pub fn handle_timer_revert(
        &self,
        subsystem: OverrideSubsystem,
        generation_id: &str,
    ) -> Result<bool, AkrepError> {
        validate_generation_token(generation_id)?;
        let _locks = self.lock_store()?;
        let mut doc = self.load_store_unlocked()?;
        let key = subsystem.as_str().to_string();

        let Some(entry) = doc.entries.get(&key) else {
            return Ok(false);
        };
        if entry.generation_id != generation_id
            || !matches!(entry.duration, OverrideDuration::Timed { .. })
        {
            return Ok(false);
        }

        match subsystem {
            OverrideSubsystem::Selinux => {
                self.ensure_selinux_config_mode(SelinuxDesiredMode::Enforcing)?;
                if self.ops.read_selinux_runtime_mode()? != SelinuxRuntimeMode::Disabled {
                    self.ops.setenforce(true)?;
                }
            }
            OverrideSubsystem::Firewalld => {
                if !self.ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT)? {
                    self.ops.enable_unit(SYSTEMD_FIREWALLD_UNIT)?;
                }
                if !self.ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT)? {
                    self.ops.start_unit(SYSTEMD_FIREWALLD_UNIT)?;
                }
            }
            OverrideSubsystem::Oomd => {
                if !self.ops.is_unit_enabled(SYSTEMD_OOMD_UNIT)? {
                    self.ops.enable_unit(SYSTEMD_OOMD_UNIT)?;
                }
                if !self.ops.is_unit_active(SYSTEMD_OOMD_UNIT)? {
                    self.ops.start_unit(SYSTEMD_OOMD_UNIT)?;
                }
            }
        }

        doc.entries.remove(&key);
        self.save_store_unlocked(&doc)?;
        Ok(true)
    }
}

#[async_trait]
impl MutationBackend for OverrideAdapter {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        req.validate_red_lines()?;
        let this = self.clone();
        let req_c = req.clone();
        tokio::task::spawn_blocking(move || match &req_c {
            ActionRequest::SelinuxSetMode { mode, duration } => {
                this.preview_selinux_mode(*mode, *duration)
            }
            ActionRequest::FirewallSetState { enabled, duration } => {
                this.preview_firewall_state(*enabled, *duration)
            }
            ActionRequest::OomdSetState { enabled, duration } => {
                this.preview_oomd_state(*enabled, *duration)
            }
            ActionRequest::DevLimitsSetNofile {
                enabled,
                soft_limit,
                hard_limit,
            } => this.preview_nofile_limits(*enabled, *soft_limit, *hard_limit),
            ActionRequest::SelinuxLabelContainerDir {
                target_path,
                recursive,
            } => this.preview_container_dir_label(target_path, *recursive),
            ActionRequest::InstallCompatibilityPackage { package_group } => {
                this.preview_compat_package(*package_group)
            }
            _ => Err(AkrepError::InvalidInput(
                "OverrideAdapter received unsupported ActionRequest".to_string(),
            )),
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("override preview join error: {}", e)))?
    }

    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        req.validate_red_lines()?;
        let this = self.clone();
        let req_c = req.clone();
        let snap_dir = snapshot_dir.to_path_buf();
        tokio::task::spawn_blocking(move || match &req_c {
            ActionRequest::SelinuxSetMode { mode, duration } => {
                this.apply_selinux_mode(*mode, *duration, &snap_dir)
            }
            ActionRequest::FirewallSetState { enabled, duration } => {
                this.apply_firewall_state(*enabled, *duration, &snap_dir)
            }
            ActionRequest::OomdSetState { enabled, duration } => {
                this.apply_oomd_state(*enabled, *duration, &snap_dir)
            }
            ActionRequest::DevLimitsSetNofile {
                enabled,
                soft_limit,
                hard_limit,
            } => this.apply_nofile_limits(*enabled, *soft_limit, *hard_limit, &snap_dir),
            ActionRequest::SelinuxLabelContainerDir {
                target_path,
                recursive,
            } => this.apply_container_dir_label(target_path, *recursive, &snap_dir),
            ActionRequest::InstallCompatibilityPackage { package_group } => {
                this.apply_compat_package(*package_group, &snap_dir)
            }
            _ => Err(AkrepError::InvalidInput(
                "OverrideAdapter received unsupported ActionRequest".to_string(),
            )),
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("override apply join error: {}", e)))?
    }

    async fn verify(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let this = self.clone();
        let req_c = req.clone();
        let exp_c = expected.clone();
        tokio::task::spawn_blocking(move || match &req_c {
            ActionRequest::SelinuxSetMode { mode, duration } => {
                this.verify_selinux_mode(*mode, *duration, &exp_c)
            }
            ActionRequest::FirewallSetState { enabled, duration } => {
                this.verify_firewall_state(*enabled, *duration, &exp_c)
            }
            ActionRequest::OomdSetState { enabled, duration } => {
                this.verify_oomd_state(*enabled, *duration, &exp_c)
            }
            ActionRequest::DevLimitsSetNofile {
                enabled,
                soft_limit,
                hard_limit,
            } => this.verify_nofile_limits(*enabled, *soft_limit, *hard_limit, &exp_c),
            ActionRequest::SelinuxLabelContainerDir {
                target_path,
                recursive,
            } => this.verify_container_dir_label(target_path, *recursive, &exp_c),
            ActionRequest::InstallCompatibilityPackage { package_group } => {
                this.verify_compat_package(*package_group, &exp_c)
            }
            _ => Err(AkrepError::InvalidInput(
                "OverrideAdapter received unsupported ActionRequest".to_string(),
            )),
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("override verify join error: {}", e)))?
    }

    async fn current_fingerprints(
        &self,
        req: &ActionRequest,
        _post_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let this = self.clone();
        let req_c = req.clone();
        tokio::task::spawn_blocking(move || match &req_c {
            ActionRequest::SelinuxSetMode { .. } => this.current_selinux_mode_fingerprints(),
            ActionRequest::FirewallSetState { .. } => this.current_firewall_state_fingerprints(),
            ActionRequest::OomdSetState { .. } => this.current_oomd_state_fingerprints(),
            ActionRequest::DevLimitsSetNofile { .. } => this.current_nofile_limits_fingerprints(),
            ActionRequest::SelinuxLabelContainerDir { target_path, .. } => {
                this.current_container_dir_label_fingerprints(target_path)
            }
            ActionRequest::InstallCompatibilityPackage { package_group } => {
                this.current_compat_package_fingerprints(*package_group)
            }
            _ => Err(AkrepError::InvalidInput(
                "OverrideAdapter received unsupported ActionRequest".to_string(),
            )),
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("override fingerprint join error: {}", e)))?
    }

    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let this = self.clone();
        let rec_c = record.clone();
        let snap_dir = snapshot_dir.to_path_buf();
        tokio::task::spawn_blocking(move || match &rec_c.request {
            ActionRequest::SelinuxSetMode { .. } => this.rollback_selinux_mode(&rec_c, &snap_dir),
            ActionRequest::FirewallSetState { .. } => {
                this.rollback_firewall_state(&rec_c, &snap_dir)
            }
            ActionRequest::OomdSetState { .. } => this.rollback_oomd_state(&rec_c, &snap_dir),
            ActionRequest::DevLimitsSetNofile { .. } => {
                this.rollback_nofile_limits(&rec_c, &snap_dir)
            }
            ActionRequest::SelinuxLabelContainerDir { .. } => {
                this.rollback_container_dir_label(&rec_c, &snap_dir)
            }
            ActionRequest::InstallCompatibilityPackage { .. } => {
                this.rollback_compat_package(&rec_c, &snap_dir)
            }
            _ => Err(AkrepError::InvalidInput(
                "OverrideAdapter received unsupported ActionRequest".to_string(),
            )),
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("override rollback join error: {}", e)))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blaze_akrep_common::{
        CompatPackageGroup, OverrideDuration, OverrideSubsystem, SelinuxDesiredMode,
        SelinuxRuntimeMode, TransactionState,
    };
    use std::collections::{HashMap, HashSet};
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    use std::sync::Arc;
    use tempfile::TempDir;

    struct FakeUnitState {
        active: bool,
        enabled: bool,
    }

    struct FakeOverrideOps {
        boot_id: Mutex<String>,
        selinux_runtime: Mutex<SelinuxRuntimeMode>,
        selinux_config: Mutex<Option<String>>,
        units: Mutex<HashMap<String, FakeUnitState>>,
        timers: Mutex<HashMap<String, (u64, String)>>,
        nofile_pam: Mutex<Option<String>>,
        nofile_systemd: Mutex<Option<String>>,
        symlink_dirs: Mutex<HashSet<String>>,
        real_dirs: Mutex<HashMap<String, (String, Vec<String>)>>,
        installed_packages: Mutex<HashSet<String>>,
        daemon_reloads: Mutex<usize>,
        recorded_ops: Mutex<Vec<String>>,
        fail_setenforce_true: Mutex<Option<String>>,
        fail_start_unit: Mutex<Option<String>>,
    }

    impl FakeOverrideOps {
        fn new() -> Self {
            let mut units = HashMap::new();
            units.insert(
                SYSTEMD_FIREWALLD_UNIT.to_string(),
                FakeUnitState {
                    active: true,
                    enabled: true,
                },
            );
            units.insert(
                SYSTEMD_OOMD_UNIT.to_string(),
                FakeUnitState {
                    active: true,
                    enabled: true,
                },
            );
            let default_cfg = "\
# Comment preserved
#     permissive - SELinux prints warnings instead of enforcing.

SELINUX=enforcing
# Type of policy
SELINUXTYPE=targeted
";
            Self {
                boot_id: Mutex::new("boot-test-1".to_string()),
                selinux_runtime: Mutex::new(SelinuxRuntimeMode::Enforcing),
                selinux_config: Mutex::new(Some(default_cfg.to_string())),
                units: Mutex::new(units),
                timers: Mutex::new(HashMap::new()),
                nofile_pam: Mutex::new(None),
                nofile_systemd: Mutex::new(None),
                symlink_dirs: Mutex::new(HashSet::new()),
                real_dirs: Mutex::new(HashMap::new()),
                installed_packages: Mutex::new(HashSet::new()),
                daemon_reloads: Mutex::new(0),
                recorded_ops: Mutex::new(Vec::new()),
                fail_setenforce_true: Mutex::new(None),
                fail_start_unit: Mutex::new(None),
            }
        }

        fn set_fail_setenforce_true(&self, msg: Option<&str>) {
            *self.fail_setenforce_true.lock().unwrap() = msg.map(ToString::to_string);
        }

        fn set_fail_start_unit(&self, msg: Option<&str>) {
            *self.fail_start_unit.lock().unwrap() = msg.map(ToString::to_string);
        }

        fn set_boot_id(&self, id: &str) {
            *self.boot_id.lock().unwrap() = id.to_string();
        }

        fn set_selinux_runtime(&self, mode: SelinuxRuntimeMode) {
            *self.selinux_runtime.lock().unwrap() = mode;
        }

        fn has_scheduled_timer(&self, subsys: OverrideSubsystem, secs: u64) -> bool {
            let timers = self.timers.lock().unwrap();
            let unit = format!("akrep-revert-{}.timer", subsys.as_str());
            timers.get(&unit).is_some_and(|(s, _)| *s == secs)
        }

        fn has_any_timer(&self, subsys: OverrideSubsystem) -> bool {
            let timers = self.timers.lock().unwrap();
            let unit = format!("akrep-revert-{}.timer", subsys.as_str());
            timers.contains_key(&unit)
        }

        fn daemon_reload_count(&self) -> usize {
            *self.daemon_reloads.lock().unwrap()
        }

        fn register_symlink_dir(&self, path: &str) {
            self.symlink_dirs.lock().unwrap().insert(path.to_string());
        }

        fn register_real_dir(&self, path: &str, ctx: &str, children: &[&str]) {
            self.real_dirs.lock().unwrap().insert(
                path.to_string(),
                (
                    ctx.to_string(),
                    children.iter().map(|c| c.to_string()).collect(),
                ),
            );
        }

        fn add_child_file(&self, dir: &str, rel: &str) {
            if let Some((_, children)) = self.real_dirs.lock().unwrap().get_mut(dir) {
                children.push(rel.to_string());
            }
        }

        fn recorded_ops(&self) -> Vec<String> {
            self.recorded_ops.lock().unwrap().clone()
        }
    }

    impl OverrideOps for FakeOverrideOps {
        fn read_boot_id(&self) -> Result<String, AkrepError> {
            Ok(self.boot_id.lock().unwrap().clone())
        }

        fn read_selinux_runtime_mode(&self) -> Result<SelinuxRuntimeMode, AkrepError> {
            Ok(*self.selinux_runtime.lock().unwrap())
        }

        fn setenforce(&self, enforcing: bool) -> Result<(), AkrepError> {
            if enforcing {
                if let Some(msg) = self.fail_setenforce_true.lock().unwrap().clone() {
                    return Err(AkrepError::ApplyFailed(msg));
                }
            }
            let mut cur = self.selinux_runtime.lock().unwrap();
            if *cur == SelinuxRuntimeMode::Disabled {
                return Err(AkrepError::BackendUnavailable(
                    "SELinux is disabled".to_string(),
                ));
            }
            *cur = if enforcing {
                SelinuxRuntimeMode::Enforcing
            } else {
                SelinuxRuntimeMode::Permissive
            };
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("setenforce {}", if enforcing { 1 } else { 0 }));
            Ok(())
        }

        fn read_selinux_config_raw(&self) -> Result<Option<String>, AkrepError> {
            Ok(self.selinux_config.lock().unwrap().clone())
        }

        fn write_selinux_config_raw(&self, content: &str) -> Result<(), AkrepError> {
            if parse_selinux_config_mode(content).as_deref() == Some("disabled") {
                return Err(AkrepError::ForbiddenOperation(
                    "SELINUX=disabled forbidden".to_string(),
                ));
            }
            *self.selinux_config.lock().unwrap() = Some(content.to_string());
            self.recorded_ops
                .lock()
                .unwrap()
                .push("write_selinux_config".to_string());
            Ok(())
        }

        fn is_unit_active(&self, unit: &str) -> Result<bool, AkrepError> {
            Ok(self
                .units
                .lock()
                .unwrap()
                .get(unit)
                .map(|u| u.active)
                .unwrap_or(false))
        }

        fn is_unit_enabled(&self, unit: &str) -> Result<bool, AkrepError> {
            Ok(self
                .units
                .lock()
                .unwrap()
                .get(unit)
                .map(|u| u.enabled)
                .unwrap_or(false))
        }

        fn start_unit(&self, unit: &str) -> Result<(), AkrepError> {
            if let Some(msg) = self.fail_start_unit.lock().unwrap().clone() {
                return Err(AkrepError::ApplyFailed(msg));
            }
            if let Some(u) = self.units.lock().unwrap().get_mut(unit) {
                u.active = true;
            }
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("systemctl start {}", unit));
            Ok(())
        }

        fn stop_unit(&self, unit: &str) -> Result<(), AkrepError> {
            if let Some(u) = self.units.lock().unwrap().get_mut(unit) {
                u.active = false;
            }
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("systemctl stop {}", unit));
            Ok(())
        }

        fn enable_unit(&self, unit: &str) -> Result<(), AkrepError> {
            if let Some(u) = self.units.lock().unwrap().get_mut(unit) {
                u.enabled = true;
            }
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("systemctl enable {}", unit));
            Ok(())
        }

        fn disable_unit(&self, unit: &str) -> Result<(), AkrepError> {
            if let Some(u) = self.units.lock().unwrap().get_mut(unit) {
                u.enabled = false;
            }
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("systemctl disable {}", unit));
            Ok(())
        }

        fn daemon_reload(&self) -> Result<(), AkrepError> {
            *self.daemon_reloads.lock().unwrap() += 1;
            self.recorded_ops
                .lock()
                .unwrap()
                .push("systemctl daemon-reload".to_string());
            Ok(())
        }

        fn schedule_revert_timer(
            &self,
            subsystem: OverrideSubsystem,
            delay_secs: u64,
            generation_id: &str,
            _state_dir: &Path,
        ) -> Result<String, AkrepError> {
            let unit = format!("akrep-revert-{}.timer", subsystem.as_str());
            self.timers
                .lock()
                .unwrap()
                .insert(unit.clone(), (delay_secs, generation_id.to_string()));
            self.recorded_ops.lock().unwrap().push(format!(
                "systemd-run --unit=akrep-revert-{} --on-active={}s --generation={}",
                subsystem.as_str(),
                delay_secs,
                generation_id
            ));
            Ok(unit)
        }

        fn cancel_revert_timer(&self, subsystem: OverrideSubsystem) -> Result<(), AkrepError> {
            let unit = format!("akrep-revert-{}.timer", subsystem.as_str());
            self.timers.lock().unwrap().remove(&unit);
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("cancel_timer {}", unit));
            Ok(())
        }

        fn read_nofile_dropins(&self) -> Result<(Option<String>, Option<String>), AkrepError> {
            Ok((
                self.nofile_pam.lock().unwrap().clone(),
                self.nofile_systemd.lock().unwrap().clone(),
            ))
        }

        fn write_nofile_dropins(
            &self,
            pam_content: &str,
            systemd_content: &str,
        ) -> Result<(), AkrepError> {
            *self.nofile_pam.lock().unwrap() = Some(pam_content.to_string());
            *self.nofile_systemd.lock().unwrap() = Some(systemd_content.to_string());
            self.recorded_ops
                .lock()
                .unwrap()
                .push("write_nofile_dropins".to_string());
            Ok(())
        }

        fn remove_nofile_dropins(&self) -> Result<(), AkrepError> {
            *self.nofile_pam.lock().unwrap() = None;
            *self.nofile_systemd.lock().unwrap() = None;
            self.recorded_ops
                .lock()
                .unwrap()
                .push("remove_nofile_dropins".to_string());
            Ok(())
        }

        fn restore_nofile_dropins(
            &self,
            pam_content: Option<&str>,
            systemd_content: Option<&str>,
        ) -> Result<(), AkrepError> {
            *self.nofile_pam.lock().unwrap() = pam_content.map(ToString::to_string);
            *self.nofile_systemd.lock().unwrap() = systemd_content.map(ToString::to_string);
            self.recorded_ops
                .lock()
                .unwrap()
                .push("restore_nofile_dropins".to_string());
            Ok(())
        }

        fn resolve_container_dir(&self, path: &Path) -> Result<PathBuf, AkrepError> {
            let raw = path.to_string_lossy().to_string();
            let normalized = validate_container_dir_path(&raw)?;
            if self.symlink_dirs.lock().unwrap().contains(&normalized) {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Symlink target '{}' is forbidden",
                    normalized
                )));
            }
            if !self.real_dirs.lock().unwrap().contains_key(&normalized) {
                return Err(AkrepError::InvalidInput(format!(
                    "Container directory '{}' does not exist",
                    normalized
                )));
            }
            Ok(PathBuf::from(normalized))
        }

        fn read_path_selinux_context(&self, path: &Path) -> Result<String, AkrepError> {
            let key = path.to_string_lossy().to_string();
            self.real_dirs
                .lock()
                .unwrap()
                .get(&key)
                .map(|(ctx, _)| ctx.clone())
                .ok_or_else(|| AkrepError::InvalidInput(format!("Missing dir '{}'", key)))
        }

        fn chcon_container_dir(
            &self,
            canonical_dir: &Path,
            recursive: bool,
        ) -> Result<(), AkrepError> {
            let key = canonical_dir.to_string_lossy().to_string();
            let args = build_container_chcon_args(recursive, canonical_dir);
            let rendered: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().to_string())
                .collect();
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("chcon {}", rendered.join(" ")));
            if let Some((ctx, _)) = self.real_dirs.lock().unwrap().get_mut(&key) {
                *ctx = "unconfined_u:object_r:container_file_t:s0".to_string();
                Ok(())
            } else {
                Err(AkrepError::InvalidInput(format!("Missing dir '{}'", key)))
            }
        }

        fn restorecon_container_dir(
            &self,
            canonical_dir: &Path,
            recursive: bool,
        ) -> Result<(), AkrepError> {
            let key = canonical_dir.to_string_lossy().to_string();
            let args = build_container_restorecon_args(recursive, canonical_dir);
            let rendered: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().to_string())
                .collect();
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("restorecon {}", rendered.join(" ")));
            if let Some((ctx, _)) = self.real_dirs.lock().unwrap().get_mut(&key) {
                *ctx = "unconfined_u:object_r:user_home_t:s0".to_string();
                Ok(())
            } else {
                Err(AkrepError::InvalidInput(format!("Missing dir '{}'", key)))
            }
        }

        fn is_packages_installed(&self, packages: &[&str]) -> Result<bool, AkrepError> {
            let installed = self.installed_packages.lock().unwrap();
            Ok(packages.iter().all(|p| installed.contains(*p)))
        }

        fn install_compat_packages(&self, group: CompatPackageGroup) -> Result<(), AkrepError> {
            let mut installed = self.installed_packages.lock().unwrap();
            for pkg in group.rpm_packages() {
                installed.insert((*pkg).to_string());
            }
            self.recorded_ops.lock().unwrap().push(format!(
                "dnf5 install -y {}",
                group.rpm_packages().join(" ")
            ));
            if matches!(group, CompatPackageGroup::AndroidUdevRules) {
                self.recorded_ops
                    .lock()
                    .unwrap()
                    .push("udevadm control --reload-rules && udevadm trigger".to_string());
            }
            Ok(())
        }

        fn remove_compat_packages(&self, group: CompatPackageGroup) -> Result<(), AkrepError> {
            let mut installed = self.installed_packages.lock().unwrap();
            for pkg in group.rpm_packages() {
                installed.remove(*pkg);
            }
            self.recorded_ops
                .lock()
                .unwrap()
                .push(format!("dnf5 remove -y {}", group.rpm_packages().join(" ")));
            Ok(())
        }
    }

    fn dummy_tx_record(request: ActionRequest, outcome: ApplyOutcome) -> TransactionRecord {
        let now = Utc::now().to_rfc3339();
        TransactionRecord {
            tx_id: "tx-test-1".to_string(),
            plan_id: "plan-test-1".to_string(),
            plan_sha256: "a".repeat(64),
            actor_uid: 1000,
            actor_bus_name: ":1.10".to_string(),
            polkit_action: request.required_polkit_action().to_string(),
            polkit_authorized: true,
            request,
            state: TransactionState::Committed,
            pre_snapshot: outcome.pre_snapshot,
            target_state: outcome.target_state,
            pre_fingerprints: Vec::new(),
            post_fingerprints: Vec::new(),
            apply_result: Some(outcome.summary),
            rollback_result: None,
            error_message: None,
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
        }
    }

    #[test]
    fn test_selinux_mode_timed_schedules_timer_writes_cache_and_reverts() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache.clone(), ops.clone());

        let snap_dir = dir.path().join("snap1");
        fs::create_dir_all(&snap_dir).unwrap();

        let preview = adapter
            .preview_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
            )
            .unwrap();
        assert_eq!(preview.pre_fingerprints.len(), 1);

        let outcome = adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
                &snap_dir,
            )
            .unwrap();
        let post_fps = adapter
            .verify_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
                &outcome,
            )
            .unwrap();
        assert_eq!(post_fps.len(), 1);

        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Permissive
        );
        assert!(ops
            .read_selinux_config_raw()
            .unwrap()
            .unwrap()
            .contains("SELINUX=enforcing"));
        assert!(ops.has_scheduled_timer(OverrideSubsystem::Selinux, 900));

        let priv_store = dir.path().join("overrides.json");
        assert_eq!(
            fs::metadata(&priv_store).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(&pub_cache).unwrap().permissions().mode() & 0o777,
            0o644
        );

        let parsed: OverrideStoreDocument =
            serde_json::from_str(&fs::read_to_string(&pub_cache).unwrap()).unwrap();
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.boot_id, "boot-test-1");
        let entry = parsed.entries.get("selinux").unwrap();
        assert!(entry.relaxed);
        assert_eq!(entry.duration, OverrideDuration::Timed { minutes: 15 });
        assert_eq!(
            entry.timer_unit.as_deref(),
            Some("akrep-revert-selinux.timer")
        );
        assert!(entry.expires_at.is_some());

        let reverted = adapter
            .handle_timer_revert(OverrideSubsystem::Selinux, &entry.generation_id)
            .unwrap();
        assert!(reverted);
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Enforcing
        );

        let cleared: OverrideStoreDocument =
            serde_json::from_str(&fs::read_to_string(&pub_cache).unwrap()).unwrap();
        assert!(cleared.entries.is_empty());
    }

    #[test]
    fn test_timer_revert_ignores_stale_generation_after_until_changed() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache.clone(), ops.clone());

        let snap1 = dir.path().join("snap1");
        let snap2 = dir.path().join("snap2");
        fs::create_dir_all(&snap1).unwrap();
        fs::create_dir_all(&snap2).unwrap();

        let _ = adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
                &snap1,
            )
            .unwrap();
        let doc1: OverrideStoreDocument =
            serde_json::from_str(&fs::read_to_string(&pub_cache).unwrap()).unwrap();
        let old_gen = doc1.entries["selinux"].generation_id.clone();

        let _ = adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::UntilChanged,
                &snap2,
            )
            .unwrap();

        let cfg = ops.read_selinux_config_raw().unwrap().unwrap();
        assert!(cfg.contains("SELINUX=permissive"));
        assert!(cfg.contains("SELINUXTYPE=targeted"));
        assert!(cfg.contains("# Comment preserved"));
        assert!(!cfg.contains("SELINUX=disabled"));
        assert!(!ops.has_any_timer(OverrideSubsystem::Selinux));

        let reverted = adapter
            .handle_timer_revert(OverrideSubsystem::Selinux, &old_gen)
            .unwrap();
        assert!(!reverted);
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Permissive
        );
        let doc2: OverrideStoreDocument =
            serde_json::from_str(&fs::read_to_string(&pub_cache).unwrap()).unwrap();
        assert_eq!(
            doc2.entries["selinux"].duration,
            OverrideDuration::UntilChanged
        );
        assert!(doc2.entries["selinux"].expires_at.is_none());
        assert!(doc2.entries["selinux"].timer_unit.is_none());
    }

    #[test]
    fn test_switching_until_changed_back_to_timed_reconciles_persistent_config() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache.clone(), ops.clone());

        let snap = dir.path().join("snap");
        fs::create_dir_all(&snap).unwrap();

        adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::UntilChanged,
                &snap,
            )
            .unwrap();
        assert!(ops
            .read_selinux_config_raw()
            .unwrap()
            .unwrap()
            .contains("SELINUX=permissive"));

        adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
                &snap,
            )
            .unwrap();
        assert!(ops
            .read_selinux_config_raw()
            .unwrap()
            .unwrap()
            .contains("SELINUX=enforcing"));
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Permissive
        );
        assert!(ops.has_scheduled_timer(OverrideSubsystem::Selinux, 900));

        adapter
            .apply_firewall_state(false, OverrideDuration::UntilChanged, &snap)
            .unwrap();
        assert!(!ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(!ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT).unwrap());

        adapter
            .apply_firewall_state(false, OverrideDuration::UntilReboot, &snap)
            .unwrap();
        assert!(ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(!ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT).unwrap());

        // Re-enforcing SELinux and re-enabling Firewalld must ignore duration and clear overrides.
        adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Enforcing,
                OverrideDuration::UntilChanged,
                &snap,
            )
            .unwrap();
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Enforcing
        );
        assert!(ops
            .read_selinux_config_raw()
            .unwrap()
            .unwrap()
            .contains("SELINUX=enforcing"));

        adapter
            .apply_firewall_state(true, OverrideDuration::UntilChanged, &snap)
            .unwrap();
        assert!(ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT).unwrap());

        let doc: OverrideStoreDocument =
            serde_json::from_str(&fs::read_to_string(&pub_cache).unwrap()).unwrap();
        assert!(doc.entries.is_empty());
    }

    #[test]
    fn test_nofile_limits_apply_and_rollback_preserves_524288_hard_ceiling() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache, ops.clone());
        let snap = dir.path().join("snap");
        fs::create_dir_all(&snap).unwrap();

        let preview = adapter
            .preview_nofile_limits(true, 65_536, 524_288)
            .unwrap();
        assert_eq!(preview.risk, RiskLevel::Low);
        assert!(preview.warnings_en.is_empty());

        let outcome = adapter
            .apply_nofile_limits(true, 65_536, 524_288, &snap)
            .unwrap();
        let post_fps = adapter
            .verify_nofile_limits(true, 65_536, 524_288, &outcome)
            .unwrap();
        assert_eq!(post_fps.len(), 1);

        let (pam, sysd) = ops.read_nofile_dropins().unwrap();
        let pam = pam.unwrap();
        let sysd = sysd.unwrap();
        assert!(pam.contains("* soft nofile 65536"));
        assert!(pam.contains("* hard nofile 524288"));
        assert!(sysd.contains("DefaultLimitNOFILE=65536:524288"));
        assert!(ops.daemon_reload_count() >= 1);

        let record = dummy_tx_record(
            ActionRequest::DevLimitsSetNofile {
                enabled: true,
                soft_limit: 65_536,
                hard_limit: 524_288,
            },
            outcome,
        );
        adapter.rollback_nofile_limits(&record, &snap).unwrap();
        let (pam_after, sysd_after) = ops.read_nofile_dropins().unwrap();
        assert!(pam_after.is_none());
        assert!(sysd_after.is_none());
        assert!(ops.daemon_reload_count() >= 2);
    }

    #[test]
    fn test_container_dir_label_roots_only_fingerprint_and_symlink_rejection() {
        let dir = TempDir::new().unwrap();
        let real_subdir = dir.path().join("workspace");
        fs::create_dir_all(&real_subdir).unwrap();
        let real_symlink = dir.path().join("workspace_link");
        symlink(&real_subdir, &real_symlink).unwrap();
        assert!(matches!(
            resolve_real_directory_nofollow(&real_symlink),
            Err(AkrepError::ForbiddenOperation(_))
        ));

        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        ops.register_symlink_dir("/home/alice/symlink_dir");
        ops.register_real_dir(
            "/home/alice/android",
            "unconfined_u:object_r:user_home_t:s0",
            &["a.txt", "b.txt"],
        );
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache, ops.clone());

        let symlink_err = adapter
            .preview_container_dir_label("/home/alice/symlink_dir", true)
            .unwrap_err();
        assert!(matches!(symlink_err, AkrepError::ForbiddenOperation(_)));

        let snap = dir.path().join("snap");
        fs::create_dir_all(&snap).unwrap();
        let outcome = adapter
            .apply_container_dir_label("/home/alice/android", true, &snap)
            .unwrap();
        let post_fps = adapter
            .verify_container_dir_label("/home/alice/android", true, &outcome)
            .unwrap();
        assert_eq!(post_fps.len(), 1);
        assert!(ops.recorded_ops().iter().any(|op| op
            .contains("chcon --no-dereference -R -t container_file_t -- /home/alice/android")));

        ops.add_child_file("/home/alice/android", "out/system.img");
        let cur_fps = adapter
            .current_container_dir_label_fingerprints("/home/alice/android")
            .unwrap();
        assert_eq!(cur_fps.len(), 1);
        assert_eq!(cur_fps[0].sha256, post_fps[0].sha256);

        let record = dummy_tx_record(
            ActionRequest::SelinuxLabelContainerDir {
                target_path: "/home/alice/android".to_string(),
                recursive: true,
            },
            outcome,
        );
        adapter
            .rollback_container_dir_label(&record, &snap)
            .unwrap();
        assert!(ops
            .recorded_ops()
            .iter()
            .any(|op| op.contains("restorecon -F -R -x -- /home/alice/android")));
        assert_eq!(
            ops.read_path_selinux_context(Path::new("/home/alice/android"))
                .unwrap(),
            "unconfined_u:object_r:user_home_t:s0"
        );
    }

    #[test]
    fn test_oomd_pause_resume_and_compat_package_install() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache.clone(), ops.clone());
        let snap1 = dir.path().join("snap1");
        let snap2 = dir.path().join("snap2");
        let snap3 = dir.path().join("snap3");
        let snap4 = dir.path().join("snap4");
        let snap5 = dir.path().join("snap5");
        for s in [&snap1, &snap2, &snap3, &snap4, &snap5] {
            fs::create_dir_all(s).unwrap();
        }

        let oomd_out = adapter
            .apply_oomd_state(false, OverrideDuration::Timed { minutes: 60 }, &snap1)
            .unwrap();
        assert!(!ops.is_unit_active(SYSTEMD_OOMD_UNIT).unwrap());
        assert!(ops.is_unit_enabled(SYSTEMD_OOMD_UNIT).unwrap());
        assert!(ops.has_scheduled_timer(OverrideSubsystem::Oomd, 3600));
        let _ = adapter
            .verify_oomd_state(false, OverrideDuration::Timed { minutes: 60 }, &oomd_out)
            .unwrap();

        adapter
            .apply_oomd_state(false, OverrideDuration::UntilChanged, &snap2)
            .unwrap();
        assert!(!ops.is_unit_enabled(SYSTEMD_OOMD_UNIT).unwrap());
        assert!(!ops.has_any_timer(OverrideSubsystem::Oomd));

        adapter
            .apply_oomd_state(true, OverrideDuration::UntilChanged, &snap3)
            .unwrap();
        assert!(ops.is_unit_active(SYSTEMD_OOMD_UNIT).unwrap());
        assert!(ops.is_unit_enabled(SYSTEMD_OOMD_UNIT).unwrap());

        let oomd_rec = dummy_tx_record(
            ActionRequest::OomdSetState {
                enabled: false,
                duration: OverrideDuration::Timed { minutes: 60 },
            },
            oomd_out,
        );
        adapter.rollback_oomd_state(&oomd_rec, &snap1).unwrap();
        assert!(ops.is_unit_active(SYSTEMD_OOMD_UNIT).unwrap());
        assert!(ops.is_unit_enabled(SYSTEMD_OOMD_UNIT).unwrap());

        let udev_out = adapter
            .apply_compat_package(CompatPackageGroup::AndroidUdevRules, &snap4)
            .unwrap();
        let udev_fps = adapter
            .verify_compat_package(CompatPackageGroup::AndroidUdevRules, &udev_out)
            .unwrap();
        assert_eq!(udev_fps.len(), 1);
        assert!(ops
            .recorded_ops()
            .iter()
            .any(|op| op.contains("dnf5 install -y android-udev-rules")));
        assert!(ops.recorded_ops().iter().any(|op| op.contains("udevadm")));

        let libs_out = adapter
            .apply_compat_package(CompatPackageGroup::AndroidLegacyLibs, &snap5)
            .unwrap();
        let _ = adapter
            .verify_compat_package(CompatPackageGroup::AndroidLegacyLibs, &libs_out)
            .unwrap();
        assert!(ops
            .recorded_ops()
            .iter()
            .any(|op| op.contains("dnf5 install -y ncurses-compat-libs")));

        let udev_rec = dummy_tx_record(
            ActionRequest::InstallCompatibilityPackage {
                package_group: CompatPackageGroup::AndroidUdevRules,
            },
            udev_out,
        );
        adapter.rollback_compat_package(&udev_rec, &snap4).unwrap();
        assert!(!ops.is_packages_installed(&["android-udev-rules"]).unwrap());
    }

    #[test]
    fn test_profile_force_restore_and_boot_id_pruning() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache.clone(), ops.clone());
        let snap = dir.path().join("snap");
        fs::create_dir_all(&snap).unwrap();

        adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
                &snap,
            )
            .unwrap();
        adapter
            .apply_firewall_state(false, OverrideDuration::UntilChanged, &snap)
            .unwrap();

        let pre = adapter.force_restore_for_profile(true, true).unwrap();
        assert!(pre.selinux_restored);
        assert!(pre.firewalld_restored);
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Enforcing
        );
        assert!(ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(adapter
            .active_override(OverrideSubsystem::Selinux)
            .unwrap()
            .is_none());
        assert!(adapter
            .active_override(OverrideSubsystem::Firewalld)
            .unwrap()
            .is_none());

        adapter.rollback_profile_override_restore(&pre).unwrap();
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Permissive
        );
        assert!(!ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(!ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT).unwrap());

        // Simulate reboot: Timed SELinux entry is dropped while UntilChanged Firewalld persists.
        ops.set_boot_id("boot-test-2");
        assert!(adapter
            .active_override(OverrideSubsystem::Selinux)
            .unwrap()
            .is_none());
        assert_eq!(
            adapter
                .active_override(OverrideSubsystem::Firewalld)
                .unwrap()
                .map(|e| e.duration),
            Some(OverrideDuration::UntilChanged)
        );

        // Disabled SELinux must fail closed on force_restore_for_profile.
        ops.set_selinux_runtime(SelinuxRuntimeMode::Disabled);
        let err = adapter.force_restore_for_profile(true, true).unwrap_err();
        assert!(matches!(err, AkrepError::BackendUnavailable(_)));
    }

    #[test]
    fn test_force_restore_partial_failure_selinux_rolls_back_config_and_timer() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache.clone(), ops.clone());
        let snap1 = dir.path().join("snap1");
        let snap2 = dir.path().join("snap2");
        fs::create_dir_all(&snap1).unwrap();
        fs::create_dir_all(&snap2).unwrap();

        // Case 1: SELinux UntilChanged -> force_restore_for_profile fails at setenforce(true)
        // after rewriting /etc/selinux/config; config and override entry must be rolled back.
        adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::UntilChanged,
                &snap1,
            )
            .unwrap();
        ops.set_fail_setenforce_true(Some("simulated setenforce 1 failure"));

        let err = adapter.force_restore_for_profile(true, false).unwrap_err();
        assert!(matches!(err, AkrepError::ApplyFailed(_)));
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Permissive
        );
        assert!(ops
            .read_selinux_config_raw()
            .unwrap()
            .unwrap()
            .contains("SELINUX=permissive"));
        assert_eq!(
            adapter
                .active_override(OverrideSubsystem::Selinux)
                .unwrap()
                .map(|e| e.duration),
            Some(OverrideDuration::UntilChanged)
        );

        // Case 2: SELinux Timed(15) -> force_restore_for_profile fails at setenforce(true)
        // after canceling the timer; timer and override entry must be restored cleanly.
        ops.set_fail_setenforce_true(None);
        adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
                &snap2,
            )
            .unwrap();
        let pre_entry = adapter
            .active_override(OverrideSubsystem::Selinux)
            .unwrap()
            .unwrap();
        ops.set_fail_setenforce_true(Some("simulated setenforce 1 failure"));

        let err2 = adapter.force_restore_for_profile(true, false).unwrap_err();
        assert!(matches!(err2, AkrepError::ApplyFailed(_)));
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Permissive
        );
        assert!(ops.has_any_timer(OverrideSubsystem::Selinux));
        assert_eq!(
            adapter
                .active_override(OverrideSubsystem::Selinux)
                .unwrap()
                .map(|e| e.generation_id),
            Some(pre_entry.generation_id)
        );
    }

    #[test]
    fn test_force_restore_partial_failure_firewalld_rolls_back_with_and_without_selinux() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache.clone(), ops.clone());
        let snap1 = dir.path().join("snap1");
        let snap2 = dir.path().join("snap2");
        let snap3 = dir.path().join("snap3");
        fs::create_dir_all(&snap1).unwrap();
        fs::create_dir_all(&snap2).unwrap();
        fs::create_dir_all(&snap3).unwrap();

        // Case 1: SELinux stays Enforcing (selinux_restored=false), Firewalld is UntilChanged.
        // start_unit fails after cancel_revert_timer + enable_unit succeeded.
        adapter
            .apply_firewall_state(false, OverrideDuration::UntilChanged, &snap1)
            .unwrap();
        ops.set_fail_start_unit(Some("simulated firewalld start failure"));

        let err1 = adapter.force_restore_for_profile(true, true).unwrap_err();
        assert!(matches!(err1, AkrepError::ApplyFailed(_)));
        assert!(!ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(!ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert_eq!(
            adapter
                .active_override(OverrideSubsystem::Firewalld)
                .unwrap()
                .map(|e| e.duration),
            Some(OverrideDuration::UntilChanged)
        );

        // Case 2: SELinux is Timed(15) AND Firewalld is Timed(15).
        // SELinux succeeds, then Firewalld fails at start_unit; BOTH SELinux and Firewalld
        // runtime, config, timers, and store entries must be rolled back.
        ops.set_fail_start_unit(None);
        adapter
            .apply_selinux_mode(
                SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
                &snap2,
            )
            .unwrap();
        adapter
            .apply_firewall_state(false, OverrideDuration::Timed { minutes: 15 }, &snap3)
            .unwrap();
        let sel_gen = adapter
            .active_override(OverrideSubsystem::Selinux)
            .unwrap()
            .unwrap()
            .generation_id;
        let fw_gen = adapter
            .active_override(OverrideSubsystem::Firewalld)
            .unwrap()
            .unwrap()
            .generation_id;

        ops.set_fail_start_unit(Some("simulated firewalld start failure"));
        let err2 = adapter.force_restore_for_profile(true, true).unwrap_err();
        assert!(matches!(err2, AkrepError::ApplyFailed(_)));

        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Permissive
        );
        assert!(ops.has_any_timer(OverrideSubsystem::Selinux));
        assert!(!ops.is_unit_active(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(ops.is_unit_enabled(SYSTEMD_FIREWALLD_UNIT).unwrap());
        assert!(ops.has_any_timer(OverrideSubsystem::Firewalld));
        assert_eq!(
            adapter
                .active_override(OverrideSubsystem::Selinux)
                .unwrap()
                .map(|e| e.generation_id),
            Some(sel_gen)
        );
        assert_eq!(
            adapter
                .active_override(OverrideSubsystem::Firewalld)
                .unwrap()
                .map(|e| e.generation_id),
            Some(fw_gen)
        );
    }

    #[test]
    fn test_override_store_rejects_unsupported_schema_version() {
        let dir = TempDir::new().unwrap();
        let ops = Arc::new(FakeOverrideOps::new());
        let adapter = OverrideAdapter::new_with_ops(
            dir.path().to_path_buf(),
            dir.path().join("active_overrides.json"),
            ops,
        );
        fs::write(
            dir.path().join(PRIVATE_OVERRIDES_FILENAME),
            r#"{"schema_version":99,"boot_id":"boot-uuid-1","entries":{}}"#,
        )
        .unwrap();
        let err = adapter
            .active_override(OverrideSubsystem::Selinux)
            .unwrap_err();
        assert!(matches!(err, AkrepError::Internal(_)));
    }
}
