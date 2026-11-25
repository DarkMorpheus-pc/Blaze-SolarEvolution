use crate::errors::{redact_secrets, AkrepError};
use crate::ini_override::{
    filesystem_token_base, resolve_override_spec, FlatpakOverrideDocument, OverrideTokenDiff,
    SECTION_CONTEXT, SECTION_SESSION_BUS, SECTION_SYSTEM_BUS,
};
use crate::models::{
    generate_uuid_v4, is_forbidden_blanket_filesystem_grant, is_forbidden_bus_policy_grant,
    validate_flatpak_app_id, validate_flatpak_override_changes, FlatpakScope, PlanDiffItem,
    RiskLevel, StateFingerprint,
};
use crate::transactions::TransactionState;
use blaze_sentinel_core::sha256_hex;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

pub const DEFAULT_SYSTEM_FLATPAK_ROOT: &str = "/var/lib/flatpak";
pub const DEFAULT_SYSTEM_APPLICATIONS_DIR: &str = "/usr/share/applications";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionSourceLayer {
    Manifest,
    GlobalOverride,
    AppOverride,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilesystemAccessMode {
    ReadWrite,
    ReadOnly,
    Create,
    Revoked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilesystemGrant {
    pub path_spec: String,
    pub raw_token: String,
    pub mode: FilesystemAccessMode,
    pub source: PermissionSourceLayer,
    pub is_broad_warning: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityGrantState {
    pub granted: bool,
    pub source: Option<PermissionSourceLayer>,
    pub raw_token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraPermissionStatus {
    pub direct_devices_all: bool,
    pub portal_mediated: bool,
    pub warning: bool,
    pub revoked_all: bool,
    pub source: Option<PermissionSourceLayer>,
}

impl Default for CameraPermissionStatus {
    fn default() -> Self {
        Self {
            direct_devices_all: false,
            portal_mediated: true,
            warning: false,
            revoked_all: false,
            source: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BusKind {
    Session,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BusPolicyEntry {
    pub bus_kind: BusKind,
    pub bus_name: String,
    pub policy: String,
    pub source: PermissionSourceLayer,
    pub is_sensitive_warning: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuredAppPermissions {
    pub filesystems: Vec<FilesystemGrant>,
    pub network: CapabilityGrantState,
    pub ipc: CapabilityGrantState,
    pub microphone: CapabilityGrantState,
    pub camera: CameraPermissionStatus,
    pub usb: CapabilityGrantState,
    pub bluetooth: CapabilityGrantState,
    pub gpu: CapabilityGrantState,
    pub wayland: CapabilityGrantState,
    pub x11: CapabilityGrantState,
    pub bus_policies: Vec<BusPolicyEntry>,
}

impl StructuredAppPermissions {
    pub fn has_filesystem_access(&self, path_spec: &str) -> bool {
        let normalized = filesystem_token_base(path_spec);
        self.filesystems
            .iter()
            .any(|f| f.path_spec == normalized && f.mode != FilesystemAccessMode::Revoked)
    }

    pub fn filesystem_grant(&self, path_spec: &str) -> Option<&FilesystemGrant> {
        let normalized = filesystem_token_base(path_spec);
        self.filesystems.iter().find(|f| f.path_spec == normalized)
    }

    pub fn bus_policy(&self, bus_kind: BusKind, bus_name: &str) -> Option<&BusPolicyEntry> {
        self.bus_policies
            .iter()
            .find(|b| b.bus_kind == bus_kind && b.bus_name == bus_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppOrigin {
    Flatpak {
        app_id: String,
        scope: FlatpakScope,
        arch: String,
        branch: String,
        runtime: String,
        sandbox_supported: bool,
    },
    NativePackage {
        desktop_id: String,
        exec_bin: String,
        sandbox_supported: bool,
    },
}

impl AppOrigin {
    pub fn sandbox_supported(&self) -> bool {
        match self {
            Self::Flatpak {
                sandbox_supported, ..
            }
            | Self::NativePackage {
                sandbox_supported, ..
            } => *sandbox_supported,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakAppRecord {
    pub app_id: String,
    pub display_name: String,
    pub version: String,
    pub arch: String,
    pub branch: String,
    pub runtime: String,
    pub scope: FlatpakScope,
    pub origin: AppOrigin,
    pub manifest_permissions: StructuredAppPermissions,
    pub global_override_raw: Option<String>,
    pub app_override_raw: Option<String>,
    pub effective_permissions: StructuredAppPermissions,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeAppRecord {
    pub desktop_id: String,
    pub display_name: String,
    pub exec_bin: String,
    pub origin: AppOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlatpakInstallationSnapshot {
    pub scope: FlatpakScope,
    pub root_path: String,
    pub global_override_present: bool,
    pub global_override_tokens: Vec<(String, String)>,
    pub apps: Vec<FlatpakAppRecord>,
    pub native_apps: Vec<NativeAppRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnifiedAppInventory {
    pub user_apps: Vec<FlatpakAppRecord>,
    pub system_apps: Vec<FlatpakAppRecord>,
    pub native_apps: Vec<NativeAppRecord>,
    pub user_global_override_tokens: Vec<(String, String)>,
    pub system_global_override_tokens: Vec<(String, String)>,
}

impl UnifiedAppInventory {
    pub fn find_flatpak(&self, app_id: &str, scope: FlatpakScope) -> Option<&FlatpakAppRecord> {
        match scope {
            FlatpakScope::User => self.user_apps.iter().find(|a| a.app_id == app_id),
            FlatpakScope::System => self.system_apps.iter().find(|a| a.app_id == app_id),
        }
    }

    pub fn find_native(&self, desktop_or_app_id: &str) -> Option<&NativeAppRecord> {
        let stripped = desktop_or_app_id
            .strip_suffix(".desktop")
            .unwrap_or(desktop_or_app_id);
        self.native_apps.iter().find(|n| {
            let n_base = n
                .desktop_id
                .strip_suffix(".desktop")
                .unwrap_or(&n.desktop_id);
            n.desktop_id == desktop_or_app_id || n_base == stripped
        })
    }
}

pub trait FlatpakRuntimeProbe: Send + Sync {
    fn running_app_ids(&self) -> BTreeSet<String>;

    fn is_app_running(&self, app_id: &str) -> bool {
        self.running_app_ids().contains(app_id)
    }
}

pub struct ProcessFlatpakRuntimeProbe {
    flatpak_bin: PathBuf,
    run_user_root: PathBuf,
}

impl Default for ProcessFlatpakRuntimeProbe {
    fn default() -> Self {
        Self {
            flatpak_bin: PathBuf::from("/usr/bin/flatpak"),
            run_user_root: PathBuf::from("/run/user"),
        }
    }
}

impl ProcessFlatpakRuntimeProbe {
    pub fn new() -> Self {
        Self::default()
    }

    fn parse_bwrap_info_dir(info_root: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = fs::read_dir(info_root) else {
            return;
        };
        for entry in entries.flatten() {
            let info_path = entry.path().join("info");
            let Ok(meta) = fs::symlink_metadata(&info_path) else {
                continue;
            };
            if meta.file_type().is_symlink() || !meta.file_type().is_file() {
                continue;
            }
            if let Ok(raw) = fs::read_to_string(&info_path) {
                if let Ok(doc) = FlatpakOverrideDocument::parse(&raw) {
                    if let Some(app_name) = doc.get_scalar("Application", "name") {
                        if !app_name.is_empty() {
                            out.insert(app_name);
                        }
                    }
                }
            }
        }
    }
}

impl FlatpakRuntimeProbe for ProcessFlatpakRuntimeProbe {
    fn running_app_ids(&self) -> BTreeSet<String> {
        let mut active = BTreeSet::new();

        if self.flatpak_bin.is_file() {
            if let Ok(output) = Command::new(&self.flatpak_bin)
                .args(["ps", "--columns=application:full"])
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .output()
            {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for line in stdout.lines() {
                        let candidate = line.trim();
                        if candidate.is_empty()
                            || candidate.eq_ignore_ascii_case("application")
                            || candidate.contains(' ')
                        {
                            continue;
                        }
                        if validate_flatpak_app_id(candidate).is_ok() {
                            active.insert(candidate.to_string());
                        }
                    }
                }
            }
        }

        if let Ok(xdg_rt) = std::env::var("XDG_RUNTIME_DIR") {
            let trimmed = xdg_rt.trim();
            if !trimmed.is_empty() {
                Self::parse_bwrap_info_dir(&Path::new(trimmed).join(".flatpak"), &mut active);
            }
        }

        if let Ok(uid_dirs) = fs::read_dir(&self.run_user_root) {
            for uid_entry in uid_dirs.flatten() {
                let Ok(meta) = fs::symlink_metadata(uid_entry.path()) else {
                    continue;
                };
                if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
                    continue;
                }
                Self::parse_bwrap_info_dir(&uid_entry.path().join(".flatpak"), &mut active);
            }
        }

        active
    }
}

#[derive(Default)]
pub struct MockFlatpakRuntimeProbe {
    running: Mutex<BTreeSet<String>>,
}

impl MockFlatpakRuntimeProbe {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_running(&self, app_id: &str, is_running: bool) {
        let mut guard = self.running.lock().unwrap();
        if is_running {
            guard.insert(app_id.to_string());
        } else {
            guard.remove(app_id);
        }
    }
}

impl FlatpakRuntimeProbe for MockFlatpakRuntimeProbe {
    fn running_app_ids(&self) -> BTreeSet<String> {
        self.running.lock().unwrap().clone()
    }
}

fn parse_fs_grant_token(token: &str, source: PermissionSourceLayer) -> Option<FilesystemGrant> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path_spec = filesystem_token_base(trimmed).to_string();
    if path_spec.is_empty() {
        return None;
    }

    let mode = if trimmed.starts_with('!') {
        FilesystemAccessMode::Revoked
    } else if trimmed.ends_with(":ro") {
        FilesystemAccessMode::ReadOnly
    } else if trimmed.ends_with(":create") {
        FilesystemAccessMode::Create
    } else {
        FilesystemAccessMode::ReadWrite
    };

    let is_broad_warning =
        mode != FilesystemAccessMode::Revoked && is_forbidden_blanket_filesystem_grant(trimmed);

    Some(FilesystemGrant {
        path_spec,
        raw_token: trimmed.to_string(),
        mode,
        source,
        is_broad_warning,
    })
}

fn apply_boolean_token_layer(
    state: &mut CapabilityGrantState,
    tokens: &[String],
    positive_names: &[&str],
    negative_names: &[&str],
    layer: PermissionSourceLayer,
) {
    for tok in tokens {
        let trimmed = tok.trim();
        if positive_names.contains(&trimmed) {
            state.granted = true;
            state.source = Some(layer);
            state.raw_token = Some(trimmed.to_string());
        } else if negative_names.contains(&trimmed) {
            state.granted = false;
            state.source = Some(layer);
            state.raw_token = Some(trimmed.to_string());
        }
    }
}

pub fn resolve_layered_permissions(
    layers: &[(PermissionSourceLayer, &FlatpakOverrideDocument)],
) -> StructuredAppPermissions {
    let mut fs_order: Vec<String> = Vec::new();
    let mut fs_map: BTreeMap<String, FilesystemGrant> = BTreeMap::new();

    let mut network = CapabilityGrantState::default();
    let mut ipc = CapabilityGrantState::default();
    let mut microphone = CapabilityGrantState::default();
    let mut camera = CameraPermissionStatus::default();
    let mut usb = CapabilityGrantState::default();
    let mut bluetooth = CapabilityGrantState::default();
    let mut gpu = CapabilityGrantState::default();
    let mut wayland = CapabilityGrantState::default();
    let mut x11 = CapabilityGrantState::default();

    let mut bus_order: Vec<(BusKind, String)> = Vec::new();
    let mut bus_map: BTreeMap<(BusKind, String), BusPolicyEntry> = BTreeMap::new();

    for (layer, doc) in layers {
        let fs_tokens = doc.get_tokens(SECTION_CONTEXT, "filesystems");
        for tok in &fs_tokens {
            if let Some(grant) = parse_fs_grant_token(tok, *layer) {
                if !fs_order.contains(&grant.path_spec) {
                    fs_order.push(grant.path_spec.clone());
                }
                fs_map.insert(grant.path_spec.clone(), grant);
            }
        }

        let shared = doc.get_tokens(SECTION_CONTEXT, "shared");
        apply_boolean_token_layer(&mut network, &shared, &["network"], &["!network"], *layer);
        apply_boolean_token_layer(&mut ipc, &shared, &["ipc"], &["!ipc"], *layer);

        let sockets = doc.get_tokens(SECTION_CONTEXT, "sockets");
        apply_boolean_token_layer(
            &mut microphone,
            &sockets,
            &["pulseaudio"],
            &["!pulseaudio"],
            *layer,
        );
        apply_boolean_token_layer(&mut wayland, &sockets, &["wayland"], &["!wayland"], *layer);
        apply_boolean_token_layer(
            &mut x11,
            &sockets,
            &["x11", "fallback-x11"],
            &["!x11", "!fallback-x11"],
            *layer,
        );

        let devices = doc.get_tokens(SECTION_CONTEXT, "devices");
        for dev_tok in &devices {
            let clean = dev_tok.trim();
            match clean {
                "all" => {
                    camera.direct_devices_all = true;
                    camera.portal_mediated = false;
                    camera.warning = true;
                    camera.revoked_all = false;
                    camera.source = Some(*layer);

                    usb.granted = true;
                    usb.source = Some(*layer);
                    usb.raw_token = Some("all".to_string());

                    gpu.granted = true;
                    gpu.source = Some(*layer);
                    gpu.raw_token = Some("all".to_string());
                }
                "!all" => {
                    camera.direct_devices_all = false;
                    camera.portal_mediated = true;
                    camera.warning = false;
                    camera.revoked_all = true;
                    camera.source = Some(*layer);

                    if usb.raw_token.as_deref() == Some("all") {
                        usb.granted = false;
                        usb.source = Some(*layer);
                        usb.raw_token = Some("!all".to_string());
                    }
                    if gpu.raw_token.as_deref() == Some("all") {
                        gpu.granted = false;
                        gpu.source = Some(*layer);
                        gpu.raw_token = Some("!all".to_string());
                    }
                }
                "usb" => {
                    usb.granted = true;
                    usb.source = Some(*layer);
                    usb.raw_token = Some("usb".to_string());
                }
                "!usb" => {
                    usb.granted = false;
                    usb.source = Some(*layer);
                    usb.raw_token = Some("!usb".to_string());
                }
                "dri" => {
                    gpu.granted = true;
                    gpu.source = Some(*layer);
                    gpu.raw_token = Some("dri".to_string());
                }
                "!dri" => {
                    gpu.granted = false;
                    gpu.source = Some(*layer);
                    gpu.raw_token = Some("!dri".to_string());
                }
                _ => {}
            }
        }

        let mut bt_tokens = doc.get_tokens(SECTION_CONTEXT, "features");
        if let Some(allow_raw) = doc.get_scalar(SECTION_CONTEXT, "allow") {
            bt_tokens.extend(
                allow_raw
                    .split(';')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string),
            );
        }
        apply_boolean_token_layer(
            &mut bluetooth,
            &bt_tokens,
            &["bluetooth"],
            &["!bluetooth"],
            *layer,
        );

        for (sec_name, bus_kind) in [
            (SECTION_SESSION_BUS, BusKind::Session),
            (SECTION_SYSTEM_BUS, BusKind::System),
        ] {
            for (bus_name, policy_val) in doc.section_entries(sec_name) {
                let key = (bus_kind, bus_name.clone());
                if !bus_order.contains(&key) {
                    bus_order.push(key.clone());
                }
                let sensitive = is_forbidden_bus_policy_grant(sec_name, &bus_name, &policy_val);
                bus_map.insert(
                    key,
                    BusPolicyEntry {
                        bus_kind,
                        bus_name,
                        policy: policy_val,
                        source: *layer,
                        is_sensitive_warning: sensitive,
                    },
                );
            }
        }
    }

    let filesystems = fs_order
        .into_iter()
        .filter_map(|k| fs_map.remove(&k))
        .collect();
    let bus_policies = bus_order
        .into_iter()
        .filter_map(|k| bus_map.remove(&k))
        .collect();

    StructuredAppPermissions {
        filesystems,
        network,
        ipc,
        microphone,
        camera,
        usb,
        bluetooth,
        gpu,
        wayland,
        x11,
        bus_policies,
    }
}

pub fn extract_override_summary_tokens(doc: &FlatpakOverrideDocument) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for key in [
        "shared",
        "sockets",
        "devices",
        "filesystems",
        "features",
        "persistent",
        "unset-environment",
    ] {
        for tok in doc.get_tokens(SECTION_CONTEXT, key) {
            out.push((format!("Context.{}", key), tok));
        }
    }
    for (bus_name, policy) in doc.section_entries(SECTION_SESSION_BUS) {
        out.push((format!("{}.{}", SECTION_SESSION_BUS, bus_name), policy));
    }
    for (bus_name, policy) in doc.section_entries(SECTION_SYSTEM_BUS) {
        out.push((format!("{}.{}", SECTION_SYSTEM_BUS, bus_name), policy));
    }
    out
}

pub fn ensure_not_symlink(path: &Path, context_label: &str) -> Result<(), AkrepError> {
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Refusing to traverse symlink at {} ('{}')",
                context_label,
                path.file_name().unwrap_or_default().to_string_lossy()
            )));
        }
    }
    Ok(())
}

pub fn open_read_nofollow(path: &Path, context_label: &str) -> Result<String, AkrepError> {
    ensure_not_symlink(path, context_label)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| {
            if e.raw_os_error() == Some(libc::ELOOP) {
                AkrepError::ForbiddenOperation(format!(
                    "Refusing to open symlink target at {}",
                    context_label
                ))
            } else {
                AkrepError::from(e)
            }
        })?;
    let meta = file.metadata()?;
    if !meta.file_type().is_file() {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Target at {} is not a regular file",
            context_label
        )));
    }
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    String::from_utf8(buf).map_err(|_| {
        AkrepError::InvalidInput(format!("File at {} is not valid UTF-8", context_label))
    })
}

pub fn read_override_file_nofollow(
    flatpak_root: &Path,
    app_or_global: &str,
) -> Result<Option<String>, AkrepError> {
    ensure_not_symlink(flatpak_root, "flatpak root")?;
    let overrides_dir = flatpak_root.join("overrides");
    if !overrides_dir.exists() {
        return Ok(None);
    }
    ensure_not_symlink(&overrides_dir, "flatpak overrides directory")?;
    let target = overrides_dir.join(app_or_global);
    match fs::symlink_metadata(&target) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Refusing to read symlinked Flatpak override '{}'",
                    app_or_global
                )));
            }
            if !meta.file_type().is_file() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Flatpak override '{}' is not a regular file",
                    app_or_global
                )));
            }
            open_read_nofollow(&target, "flatpak override file").map(Some)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(AkrepError::from(e)),
    }
}

fn sync_dir(dir: &Path) -> Result<(), AkrepError> {
    let f = File::open(dir)?;
    f.sync_all()?;
    Ok(())
}

pub fn write_override_file_atomic_nofollow(
    flatpak_root: &Path,
    app_id: &str,
    content: &str,
    dir_mode: u32,
    file_mode: u32,
) -> Result<(), AkrepError> {
    validate_flatpak_app_id(app_id)?;
    ensure_not_symlink(flatpak_root, "flatpak root")?;

    let overrides_dir = flatpak_root.join("overrides");
    match fs::symlink_metadata(&overrides_dir) {
        Ok(meta) => {
            if meta.file_type().is_symlink() || !meta.file_type().is_dir() {
                return Err(AkrepError::ForbiddenOperation(
                    "Flatpak overrides path must be a real directory, not a symlink".to_string(),
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(&overrides_dir)?;
            fs::set_permissions(&overrides_dir, fs::Permissions::from_mode(dir_mode))?;
        }
        Err(e) => return Err(AkrepError::from(e)),
    }

    let target = overrides_dir.join(app_id);
    if let Ok(meta) = fs::symlink_metadata(&target) {
        if meta.file_type().is_symlink() {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Refusing to overwrite symlink in Flatpak overrides for '{}'",
                app_id
            )));
        }
        if !meta.file_type().is_file() {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Flatpak override target '{}' is not a regular file",
                app_id
            )));
        }
    }

    let nonce = generate_uuid_v4()?;
    let tmp_path = overrides_dir.join(format!(".{}.{}.tmp", app_id, nonce));
    let mut tmp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .mode(file_mode)
        .open(&tmp_path)?;

    if let Err(err) = fs::set_permissions(&tmp_path, fs::Permissions::from_mode(file_mode))
        .and_then(|_| tmp_file.write_all(content.as_bytes()))
        .and_then(|_| tmp_file.sync_all())
    {
        let _ = fs::remove_file(&tmp_path);
        return Err(AkrepError::from(err));
    }
    drop(tmp_file);

    if let Ok(meta) = fs::symlink_metadata(&target) {
        if meta.file_type().is_symlink() || meta.file_type().is_dir() {
            let _ = fs::remove_file(&tmp_path);
            return Err(AkrepError::ForbiddenOperation(format!(
                "Refusing to rename over non-regular override entry '{}'",
                app_id
            )));
        }
    }

    if let Err(err) = fs::rename(&tmp_path, &target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(AkrepError::from(err));
    }
    fs::set_permissions(&target, fs::Permissions::from_mode(file_mode))?;
    sync_dir(&overrides_dir)?;
    Ok(())
}

pub fn remove_override_file_nofollow(flatpak_root: &Path, app_id: &str) -> Result<(), AkrepError> {
    validate_flatpak_app_id(app_id)?;
    ensure_not_symlink(flatpak_root, "flatpak root")?;
    let overrides_dir = flatpak_root.join("overrides");
    if !overrides_dir.exists() {
        return Ok(());
    }
    ensure_not_symlink(&overrides_dir, "flatpak overrides directory")?;
    let target = overrides_dir.join(app_id);
    match fs::symlink_metadata(&target) {
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                return Err(AkrepError::ForbiddenOperation(format!(
                    "Refusing to unlink symlink override '{}'",
                    app_id
                )));
            }
            fs::remove_file(&target)?;
            sync_dir(&overrides_dir)?;
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AkrepError::from(e)),
    }
}

fn extract_gvariant_ascii_after_key(bytes: &[u8], key: &[u8]) -> Option<String> {
    let mut needle = Vec::with_capacity(key.len() + 1);
    needle.extend_from_slice(key);
    needle.push(0);

    let pos = bytes
        .windows(needle.len())
        .position(|w| w == needle.as_slice())?;
    let mut cursor = pos + needle.len();
    while cursor < bytes.len() && bytes[cursor] == 0 {
        cursor += 1;
    }
    let start = cursor;
    while cursor < bytes.len() && bytes[cursor] != 0 {
        cursor += 1;
    }
    if cursor <= start {
        return None;
    }
    let candidate = std::str::from_utf8(&bytes[start..cursor]).ok()?.trim();
    if candidate.is_empty() || candidate.chars().any(char::is_control) {
        return None;
    }
    Some(candidate.to_string())
}

fn parse_deploy_hints(deploy_path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let Ok(bytes) = fs::read(deploy_path) else {
        return (None, None, None);
    };
    if let Ok(txt) = std::str::from_utf8(&bytes) {
        if !txt.contains('\0') {
            let mut disp = None;
            let mut ver = None;
            let mut br = None;
            for line in txt.lines() {
                let trimmed = line.trim();
                if let Some((k, v)) = trimmed.split_once('=') {
                    match k.trim() {
                        "display_name" | "appdata-name" | "name" if !v.trim().is_empty() => {
                            disp = Some(v.trim().to_string());
                        }
                        "version" | "appdata-version" if !v.trim().is_empty() => {
                            ver = Some(v.trim().to_string());
                        }
                        "branch" if !v.trim().is_empty() => {
                            br = Some(v.trim().to_string());
                        }
                        _ => {}
                    }
                }
            }
            return (disp, ver, br);
        }
    }

    let disp = extract_gvariant_ascii_after_key(&bytes, b"appdata-name");
    let ver = extract_gvariant_ascii_after_key(&bytes, b"appdata-version");
    (disp, ver, None)
}

#[derive(Default)]
struct DesktopEntryHints {
    name: Option<String>,
    exec: Option<String>,
    version: Option<String>,
    is_application: bool,
    no_display: bool,
    is_flatpak_export: bool,
}

fn parse_desktop_entry(raw: &str) -> DesktopEntryHints {
    let mut hints = DesktopEntryHints::default();
    let mut in_desktop_entry = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_desktop_entry = trimmed == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        let Some((raw_k, raw_v)) = trimmed.split_once('=') else {
            continue;
        };
        let key = raw_k.trim();
        let val = raw_v.trim();
        match key {
            "Type" => hints.is_application = val == "Application",
            "Name" if hints.name.is_none() && !val.is_empty() => {
                hints.name = Some(val.to_string());
            }
            "Exec" if !val.is_empty() => {
                if val.contains("flatpak run") || val.contains("/flatpak ") {
                    hints.is_flatpak_export = true;
                }
                hints.exec = Some(val.to_string());
            }
            "X-Flatpak" | "X-Flatpak-Tags" => {
                hints.is_flatpak_export = true;
            }
            "X-Flatpak-Version" if !val.is_empty() => {
                hints.version = Some(val.to_string());
            }
            "NoDisplay" | "Hidden" if val.eq_ignore_ascii_case("true") => {
                hints.no_display = true;
            }
            _ => {}
        }
    }
    hints
}

fn extract_exec_binary(exec_line: &str) -> String {
    for token in exec_line.split_whitespace() {
        let unquoted = token.trim_matches('"').trim_matches('\'');
        if unquoted.is_empty() || unquoted == "env" {
            continue;
        }
        if unquoted.contains('=') && !unquoted.starts_with('/') {
            continue;
        }
        return unquoted.to_string();
    }
    exec_line.trim().to_string()
}

fn resolve_active_deployment_dir(
    canonical_flatpak_root: &Path,
    app_dir: &Path,
) -> Option<(PathBuf, String, String)> {
    let current_link = app_dir.join("current");
    if current_link.exists() {
        let mut arch = "unknown".to_string();
        let mut branch = "stable".to_string();
        if let Ok(link_target) = fs::read_link(&current_link) {
            let comps: Vec<String> = link_target
                .components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(s) => Some(s.to_string_lossy().to_string()),
                    _ => None,
                })
                .collect();
            if comps.len() >= 2 {
                arch = comps[comps.len() - 2].clone();
                branch = comps[comps.len() - 1].clone();
            }
        }
        let active_dir = current_link.join("active");
        if let Ok(canon_active) = fs::canonicalize(&active_dir) {
            if canon_active.starts_with(canonical_flatpak_root)
                && canon_active.join("metadata").is_file()
            {
                return Some((canon_active, arch, branch));
            }
        }
    }

    let Ok(arch_entries) = fs::read_dir(app_dir) else {
        return None;
    };
    for arch_entry in arch_entries.flatten() {
        let arch_name = arch_entry.file_name().to_string_lossy().to_string();
        if arch_name.starts_with('.') || arch_name == "current" {
            continue;
        }
        let Ok(branch_entries) = fs::read_dir(arch_entry.path()) else {
            continue;
        };
        for branch_entry in branch_entries.flatten() {
            let branch_name = branch_entry.file_name().to_string_lossy().to_string();
            if branch_name.starts_with('.') {
                continue;
            }
            let active_dir = branch_entry.path().join("active");
            if let Ok(canon_active) = fs::canonicalize(&active_dir) {
                if canon_active.starts_with(canonical_flatpak_root)
                    && canon_active.join("metadata").is_file()
                {
                    return Some((canon_active, arch_name, branch_name));
                }
            }
        }
    }
    None
}

pub fn is_flatpak_app_installed(flatpak_root: &Path, app_id: &str) -> Result<bool, AkrepError> {
    validate_flatpak_app_id(app_id)?;
    ensure_not_symlink(flatpak_root, "flatpak root")?;
    if !flatpak_root.exists() {
        return Ok(false);
    }
    let canon_root = fs::canonicalize(flatpak_root)?;
    let app_dir = flatpak_root.join("app").join(app_id);
    if !app_dir.exists() {
        return Ok(false);
    }
    ensure_not_symlink(&app_dir, "flatpak app directory")?;
    Ok(resolve_active_deployment_dir(&canon_root, &app_dir).is_some())
}

pub type FlatpakRootInspection = (Option<String>, Vec<(String, String)>, Vec<FlatpakAppRecord>);

pub fn inspect_flatpak_apps_in_root(
    flatpak_root: &Path,
    scope: FlatpakScope,
    running_ids: &BTreeSet<String>,
) -> Result<FlatpakRootInspection, AkrepError> {
    if !flatpak_root.exists() {
        return Ok((None, Vec::new(), Vec::new()));
    }
    ensure_not_symlink(flatpak_root, "flatpak root")?;
    let canon_root = fs::canonicalize(flatpak_root)?;

    let global_override_raw = read_override_file_nofollow(flatpak_root, "global")?;
    let global_doc = match &global_override_raw {
        Some(raw) => Some(FlatpakOverrideDocument::parse(raw)?),
        None => None,
    };
    let global_tokens = global_doc
        .as_ref()
        .map(extract_override_summary_tokens)
        .unwrap_or_default();

    let app_parent = flatpak_root.join("app");
    if !app_parent.exists() {
        return Ok((global_override_raw, global_tokens, Vec::new()));
    }
    ensure_not_symlink(&app_parent, "flatpak app root")?;

    let mut records = Vec::new();
    for entry in fs::read_dir(&app_parent)? {
        let entry = entry?;
        let app_id = entry.file_name().to_string_lossy().to_string();
        if app_id.starts_with('.') || validate_flatpak_app_id(&app_id).is_err() {
            continue;
        }
        let app_dir = entry.path();
        let Ok(app_meta) = fs::symlink_metadata(&app_dir) else {
            continue;
        };
        if app_meta.file_type().is_symlink() || !app_meta.file_type().is_dir() {
            continue;
        }

        let Some((active_dir, mut arch, mut branch)) =
            resolve_active_deployment_dir(&canon_root, &app_dir)
        else {
            continue;
        };

        let meta_path = active_dir.join("metadata");
        let Ok(meta_raw) = fs::read_to_string(&meta_path) else {
            continue;
        };
        let Ok(manifest_doc) = FlatpakOverrideDocument::parse(&meta_raw) else {
            continue;
        };

        let runtime = manifest_doc
            .get_scalar("Application", "runtime")
            .or_else(|| manifest_doc.get_scalar("Runtime", "runtime"))
            .unwrap_or_else(|| "unknown".to_string());
        if arch == "unknown" {
            let parts: Vec<&str> = runtime.split('/').collect();
            if parts.len() >= 2 && !parts[1].is_empty() {
                arch = parts[1].to_string();
            }
        }
        if let Some(meta_branch) = manifest_doc.get_scalar("Application", "branch") {
            if !meta_branch.is_empty() {
                branch = meta_branch;
            }
        }

        let (deploy_name, deploy_ver, deploy_branch) =
            parse_deploy_hints(&active_dir.join("deploy"));
        if let Some(db) = deploy_branch {
            branch = db;
        }

        let exported_desktop = active_dir
            .join("export/share/applications")
            .join(format!("{}.desktop", app_id));
        let desktop_hints = fs::read_to_string(&exported_desktop)
            .ok()
            .map(|raw| parse_desktop_entry(&raw))
            .unwrap_or_default();

        let display_name = deploy_name
            .or(desktop_hints.name)
            .or_else(|| manifest_doc.get_scalar("Application", "display_name"))
            .unwrap_or_else(|| app_id.clone());

        let version = deploy_ver
            .or(desktop_hints.version)
            .or_else(|| manifest_doc.get_scalar("Application", "version"))
            .unwrap_or_else(|| branch.clone());

        let app_override_raw = read_override_file_nofollow(flatpak_root, &app_id)?;
        let app_override_doc = match &app_override_raw {
            Some(raw) => Some(FlatpakOverrideDocument::parse(raw)?),
            None => None,
        };

        let manifest_permissions =
            resolve_layered_permissions(&[(PermissionSourceLayer::Manifest, &manifest_doc)]);

        let mut layers: Vec<(PermissionSourceLayer, &FlatpakOverrideDocument)> =
            vec![(PermissionSourceLayer::Manifest, &manifest_doc)];
        if let Some(ref gdoc) = global_doc {
            layers.push((PermissionSourceLayer::GlobalOverride, gdoc));
        }
        if let Some(ref adoc) = app_override_doc {
            layers.push((PermissionSourceLayer::AppOverride, adoc));
        }
        let effective_permissions = resolve_layered_permissions(&layers);

        let origin = AppOrigin::Flatpak {
            app_id: app_id.clone(),
            scope,
            arch: arch.clone(),
            branch: branch.clone(),
            runtime: runtime.clone(),
            sandbox_supported: true,
        };

        records.push(FlatpakAppRecord {
            running: running_ids.contains(&app_id),
            app_id,
            display_name,
            version,
            arch,
            branch,
            runtime,
            scope,
            origin,
            manifest_permissions,
            global_override_raw: global_override_raw.clone(),
            app_override_raw,
            effective_permissions,
        });
    }

    records.sort_by(|a, b| a.app_id.cmp(&b.app_id));
    Ok((global_override_raw, global_tokens, records))
}

pub fn inspect_native_desktop_apps(
    applications_dirs: &[PathBuf],
    known_flatpak_ids: &BTreeSet<String>,
) -> Result<Vec<NativeAppRecord>, AkrepError> {
    let mut seen_ids = BTreeSet::new();
    let mut native_apps = Vec::new();

    for dir in applications_dirs {
        if !dir.exists() {
            continue;
        }
        let Ok(canon_dir) = fs::canonicalize(dir) else {
            continue;
        };
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let file_name = entry.file_name().to_string_lossy().to_string();
            if file_name.starts_with('.') || !file_name.ends_with(".desktop") {
                continue;
            }
            let base_id = file_name.trim_end_matches(".desktop");
            if known_flatpak_ids.contains(base_id) || seen_ids.contains(&file_name) {
                continue;
            }

            let path = entry.path();
            let Ok(canon_file) = fs::canonicalize(&path) else {
                continue;
            };
            if !canon_file.starts_with(&canon_dir) {
                continue;
            }
            let Ok(raw) = fs::read_to_string(&canon_file) else {
                continue;
            };
            let hints = parse_desktop_entry(&raw);
            if !hints.is_application || hints.no_display || hints.is_flatpak_export {
                continue;
            }
            let exec_bin = hints
                .exec
                .as_deref()
                .map(extract_exec_binary)
                .unwrap_or_default();
            if exec_bin.is_empty() {
                continue;
            }
            let display_name = hints.name.unwrap_or_else(|| base_id.to_string());
            seen_ids.insert(file_name.clone());
            native_apps.push(NativeAppRecord {
                desktop_id: file_name.clone(),
                display_name,
                exec_bin: exec_bin.clone(),
                origin: AppOrigin::NativePackage {
                    desktop_id: file_name,
                    exec_bin,
                    sandbox_supported: false,
                },
            });
        }
    }

    native_apps.sort_by(|a, b| a.desktop_id.cmp(&b.desktop_id));
    Ok(native_apps)
}

pub fn compute_touched_override_keys(
    additions: &[(String, String)],
    removals: &[(String, String)],
) -> Result<Vec<(String, String)>, AkrepError> {
    let mut keys = BTreeSet::new();
    for (spec, _) in additions.iter().chain(removals.iter()) {
        let (sec, key) = resolve_override_spec(spec)?;
        keys.insert((sec, key));
    }
    Ok(keys.into_iter().collect())
}

pub fn format_override_key_target(
    scope: FlatpakScope,
    app_id: &str,
    section: &str,
    key: &str,
) -> String {
    let scope_str = match scope {
        FlatpakScope::System => "system",
        FlatpakScope::User => "user",
    };
    format!("flatpak:{}:{}:[{}].{}", scope_str, app_id, section, key)
}

pub fn compute_override_fingerprints_for_keys(
    scope: FlatpakScope,
    app_id: &str,
    doc: &FlatpakOverrideDocument,
    keys: &[(String, String)],
) -> Vec<StateFingerprint> {
    let now = Utc::now().to_rfc3339();
    keys.iter()
        .map(|(sec, key)| {
            let tokens = doc.get_tokens(sec, key);
            let canonical = serde_json::to_vec(&tokens).unwrap_or_default();
            StateFingerprint {
                target: format_override_key_target(scope, app_id, sec, key),
                sha256: sha256_hex(&canonical),
                captured_at: now.clone(),
            }
        })
        .collect()
}

pub fn build_override_plan_diffs(
    scope: FlatpakScope,
    app_id: &str,
    before_doc: &FlatpakOverrideDocument,
    after_doc: &FlatpakOverrideDocument,
    touched_keys: &[(String, String)],
) -> Vec<PlanDiffItem> {
    let mut diffs = Vec::new();
    for (sec, key) in touched_keys {
        let old_tokens = before_doc.get_tokens(sec, key);
        let new_tokens = after_doc.get_tokens(sec, key);
        let old_str = if old_tokens.is_empty() {
            "<unset>".to_string()
        } else {
            old_tokens.join(";")
        };
        let new_str = if new_tokens.is_empty() {
            "<unset>".to_string()
        } else {
            new_tokens.join(";")
        };
        diffs.push(PlanDiffItem {
            target: format_override_key_target(scope, app_id, sec, key),
            current_value: old_str,
            desired_value: new_str,
            description_tr: format!(
                "{} uygulamasi icin [{}].{} kum havuzu izni guncellemesi",
                app_id, sec, key
            ),
            description_en: format!(
                "Flatpak sandbox override update on [{}].{} for {}",
                sec, key, app_id
            ),
        });
    }
    diffs
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverrideSnapshotBackup {
    pub app_id: String,
    pub scope: FlatpakScope,
    pub existed_before: bool,
    pub raw_before: String,
    pub raw_before_sha256: String,
    pub pre_sections: Vec<String>,
    pub touched_keys: Vec<(String, String)>,
    pub pre_key_tokens: BTreeMap<String, Vec<String>>,
    pub post_key_tokens: BTreeMap<String, Vec<String>>,
    pub diffs: Vec<OverrideTokenDiff>,
    pub restart_required: bool,
}

impl OverrideSnapshotBackup {
    pub fn key_map_id(section: &str, key: &str) -> String {
        format!("[{}].{}", section, key)
    }
}

pub fn prepare_override_mutation(
    flatpak_root: &Path,
    scope: FlatpakScope,
    app_id: &str,
    additions: &[(String, String)],
    removals: &[(String, String)],
    restart_required: bool,
) -> Result<
    (
        OverrideSnapshotBackup,
        FlatpakOverrideDocument,
        FlatpakOverrideDocument,
    ),
    AkrepError,
> {
    validate_flatpak_app_id(app_id)?;
    validate_flatpak_override_changes(additions, removals)?;

    let existing_raw = read_override_file_nofollow(flatpak_root, app_id)?;
    let existed_before = existing_raw.is_some();
    let raw_before = existing_raw.unwrap_or_default();
    let raw_before_sha256 = sha256_hex(raw_before.as_bytes());

    let before_doc = FlatpakOverrideDocument::parse(&raw_before)?;
    let pre_sections = before_doc.sections();
    let touched_keys = compute_touched_override_keys(additions, removals)?;

    let mut after_doc = before_doc.clone();
    let diffs = after_doc.apply_changes(additions, removals)?;

    let mut pre_key_tokens = BTreeMap::new();
    let mut post_key_tokens = BTreeMap::new();
    for (sec, key) in &touched_keys {
        let map_id = OverrideSnapshotBackup::key_map_id(sec, key);
        pre_key_tokens.insert(map_id.clone(), before_doc.get_tokens(sec, key));
        post_key_tokens.insert(map_id, after_doc.get_tokens(sec, key));
    }

    let backup = OverrideSnapshotBackup {
        app_id: app_id.to_string(),
        scope,
        existed_before,
        raw_before,
        raw_before_sha256,
        pre_sections,
        touched_keys,
        pre_key_tokens,
        post_key_tokens,
        diffs,
        restart_required,
    };

    Ok((backup, before_doc, after_doc))
}

pub fn verify_override_post_state_nofollow(
    flatpak_root: &Path,
    backup: &OverrideSnapshotBackup,
    expected_file_mode: Option<u32>,
) -> Result<Vec<StateFingerprint>, AkrepError> {
    let target = flatpak_root.join("overrides").join(&backup.app_id);
    let meta = fs::symlink_metadata(&target).map_err(|e| {
        AkrepError::VerifyFailed(format!(
            "Override file missing after apply for '{}': {}",
            backup.app_id,
            redact_secrets(&e.to_string())
        ))
    })?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(AkrepError::VerifyFailed(format!(
            "Override target for '{}' is not a regular file",
            backup.app_id
        )));
    }
    if let Some(exp_mode) = expected_file_mode {
        let actual_mode = meta.permissions().mode() & 0o777;
        if actual_mode != exp_mode {
            return Err(AkrepError::VerifyFailed(format!(
                "Override file permissions {:04o} did not match expected {:04o}",
                actual_mode, exp_mode
            )));
        }
    }

    let raw_on_disk =
        read_override_file_nofollow(flatpak_root, &backup.app_id)?.ok_or_else(|| {
            AkrepError::VerifyFailed(format!(
                "Override file disappeared during verification for '{}'",
                backup.app_id
            ))
        })?;
    let doc = FlatpakOverrideDocument::parse(&raw_on_disk).map_err(|e| {
        AkrepError::VerifyFailed(format!(
            "Override INI failed read-back parse for '{}': {}",
            backup.app_id,
            e.message()
        ))
    })?;

    for (sec, key) in &backup.touched_keys {
        let map_id = OverrideSnapshotBackup::key_map_id(sec, key);
        let expected_tokens = backup
            .post_key_tokens
            .get(&map_id)
            .cloned()
            .unwrap_or_default();
        let actual_tokens = doc.get_tokens(sec, key);
        if actual_tokens != expected_tokens {
            return Err(AkrepError::VerifyFailed(format!(
                "Post-apply token mismatch on {} for '{}'",
                map_id, backup.app_id
            )));
        }
    }

    Ok(compute_override_fingerprints_for_keys(
        backup.scope,
        &backup.app_id,
        &doc,
        &backup.touched_keys,
    ))
}

pub fn rollback_override_key_level_nofollow(
    flatpak_root: &Path,
    backup: &OverrideSnapshotBackup,
    require_post_drift_check: bool,
    dir_mode: u32,
    file_mode: u32,
) -> Result<String, AkrepError> {
    let live_raw_opt = read_override_file_nofollow(flatpak_root, &backup.app_id)?;

    if !require_post_drift_check {
        if !backup.existed_before {
            remove_override_file_nofollow(flatpak_root, &backup.app_id)?;
            return Ok(format!(
                "Removed newly created override file for '{}'",
                backup.app_id
            ));
        }
        write_override_file_atomic_nofollow(
            flatpak_root,
            &backup.app_id,
            &backup.raw_before,
            dir_mode,
            file_mode,
        )?;
        return Ok(format!(
            "Restored pre-apply override snapshot for '{}'",
            backup.app_id
        ));
    }

    let Some(live_raw) = live_raw_opt else {
        return Err(AkrepError::ExternalDriftDetected(format!(
            "Override file for '{}' was deleted externally after commit; refusing blind rollback",
            backup.app_id
        )));
    };
    let mut live_doc = FlatpakOverrideDocument::parse(&live_raw).map_err(|_| {
        AkrepError::ExternalDriftDetected(format!(
            "Override file for '{}' was modified into invalid INI externally; refusing rollback",
            backup.app_id
        ))
    })?;

    for (sec, key) in &backup.touched_keys {
        let map_id = OverrideSnapshotBackup::key_map_id(sec, key);
        let expected_post = backup
            .post_key_tokens
            .get(&map_id)
            .cloned()
            .unwrap_or_default();
        let current_live = live_doc.get_tokens(sec, key);
        if current_live != expected_post {
            return Err(AkrepError::ExternalDriftDetected(format!(
                "External drift detected on {} for '{}'; refusing to overwrite external changes",
                map_id, backup.app_id
            )));
        }
    }

    live_doc.revert_diffs(&backup.diffs, &backup.pre_sections)?;

    if !backup.existed_before && !live_doc.has_any_entries() {
        remove_override_file_nofollow(flatpak_root, &backup.app_id)?;
        return Ok(format!(
            "Reverted key-level override changes and removed empty override file for '{}'",
            backup.app_id
        ));
    }

    write_override_file_atomic_nofollow(
        flatpak_root,
        &backup.app_id,
        &live_doc.to_ini_string(),
        dir_mode,
        file_mode,
    )?;
    Ok(format!(
        "Reverted {} key-level override diff(s) for '{}' while preserving unrelated keys",
        backup.diffs.len(),
        backup.app_id
    ))
}

struct UserFlockGuard {
    file: File,
}

impl UserFlockGuard {
    fn acquire_exclusive(lock_path: &Path) -> Result<Self, AkrepError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .mode(0o600)
            .open(lock_path)?;
        let _ = fs::set_permissions(lock_path, fs::Permissions::from_mode(0o600));
        // SAFETY: `file.as_raw_fd()` is valid and owned by `UserFlockGuard` until drop.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let os_err = std::io::Error::last_os_error();
            if os_err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Err(AkrepError::TransactionConflict(
                    "Another user-scope Flatpak override transaction is in progress".to_string(),
                ));
            }
            return Err(AkrepError::IoError(redact_secrets(&os_err.to_string())));
        }
        Ok(Self { file })
    }
}

impl Drop for UserFlockGuard {
    fn drop(&mut self) {
        // SAFETY: `self.file` remains open throughout `drop`.
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserOverridePreview {
    pub app_id: String,
    pub scope: FlatpakScope,
    pub risk: RiskLevel,
    pub diffs: Vec<PlanDiffItem>,
    pub pre_fingerprints: Vec<StateFingerprint>,
    pub restart_required: bool,
    pub warnings_tr: Vec<String>,
    pub warnings_en: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserOverrideTransaction {
    pub tx_id: String,
    pub app_id: String,
    pub scope: FlatpakScope,
    pub state: TransactionState,
    pub additions: Vec<(String, String)>,
    pub removals: Vec<(String, String)>,
    pub backup: OverrideSnapshotBackup,
    pub pre_fingerprints: Vec<StateFingerprint>,
    pub post_fingerprints: Vec<StateFingerprint>,
    pub restart_required: bool,
    pub created_at: String,
    pub updated_at: String,
    pub rollback_result: Option<String>,
}

pub struct UserFlatpakManager {
    user_flatpak_root: PathBuf,
    system_flatpak_root: PathBuf,
    applications_dirs: Vec<PathBuf>,
    user_tx_dir: PathBuf,
    runtime_probe: Arc<dyn FlatpakRuntimeProbe>,
}

impl UserFlatpakManager {
    pub fn from_xdg_env() -> Result<Self, AkrepError> {
        let data_home = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            let t = xdg.trim();
            if !t.is_empty() && Path::new(t).is_absolute() {
                PathBuf::from(t)
            } else {
                Self::fallback_home_data_dir()?
            }
        } else {
            Self::fallback_home_data_dir()?
        };

        let user_flatpak_root = data_home.join("flatpak");
        let system_flatpak_root = PathBuf::from(DEFAULT_SYSTEM_FLATPAK_ROOT);
        let applications_dirs = vec![
            data_home.join("applications"),
            PathBuf::from(DEFAULT_SYSTEM_APPLICATIONS_DIR),
        ];
        let user_tx_dir = data_home.join("akrep/user-transactions");

        Ok(Self {
            user_flatpak_root,
            system_flatpak_root,
            applications_dirs,
            user_tx_dir,
            runtime_probe: Arc::new(ProcessFlatpakRuntimeProbe::new()),
        })
    }

    fn fallback_home_data_dir() -> Result<PathBuf, AkrepError> {
        let home = std::env::var("HOME").map_err(|_| {
            AkrepError::InvalidInput("Neither XDG_DATA_HOME nor HOME is set".to_string())
        })?;
        let trimmed = home.trim();
        if trimmed.is_empty() || !Path::new(trimmed).is_absolute() {
            return Err(AkrepError::InvalidInput(
                "HOME environment variable must be a non-empty absolute path".to_string(),
            ));
        }
        Ok(PathBuf::from(trimmed).join(".local/share"))
    }

    pub fn with_paths(
        user_flatpak_root: PathBuf,
        system_flatpak_root: PathBuf,
        applications_dirs: Vec<PathBuf>,
        user_tx_dir: PathBuf,
        runtime_probe: Arc<dyn FlatpakRuntimeProbe>,
    ) -> Self {
        Self {
            user_flatpak_root,
            system_flatpak_root,
            applications_dirs,
            user_tx_dir,
            runtime_probe,
        }
    }

    pub fn user_flatpak_root(&self) -> &Path {
        &self.user_flatpak_root
    }

    pub fn system_flatpak_root(&self) -> &Path {
        &self.system_flatpak_root
    }

    pub fn user_tx_dir(&self) -> &Path {
        &self.user_tx_dir
    }

    pub fn inspect_unified(&self) -> Result<UnifiedAppInventory, AkrepError> {
        let running_ids = self.runtime_probe.running_app_ids();
        let (_, user_global_override_tokens, user_apps) = inspect_flatpak_apps_in_root(
            &self.user_flatpak_root,
            FlatpakScope::User,
            &running_ids,
        )?;
        let (_, system_global_override_tokens, system_apps) = inspect_flatpak_apps_in_root(
            &self.system_flatpak_root,
            FlatpakScope::System,
            &running_ids,
        )?;

        let mut known_flatpaks = BTreeSet::new();
        for app in user_apps.iter().chain(system_apps.iter()) {
            known_flatpaks.insert(app.app_id.clone());
        }
        let native_apps = inspect_native_desktop_apps(&self.applications_dirs, &known_flatpaks)?;

        Ok(UnifiedAppInventory {
            user_apps,
            system_apps,
            native_apps,
            user_global_override_tokens,
            system_global_override_tokens,
        })
    }

    fn ensure_target_is_installed_user_flatpak(&self, app_id: &str) -> Result<(), AkrepError> {
        if app_id.trim().eq_ignore_ascii_case("global") {
            return Err(AkrepError::ForbiddenOperation(
                "Global Flatpak overrides ('global') are strictly forbidden".to_string(),
            ));
        }

        let inv = self.inspect_unified()?;
        if inv.find_flatpak(app_id, FlatpakScope::User).is_some() {
            validate_flatpak_app_id(app_id)?;
            return Ok(());
        }
        if inv.find_native(app_id).is_some() {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Application '{}' is a native host package (sandbox_supported=false); Flatpak sandbox overrides are unsupported",
                app_id
            )));
        }
        validate_flatpak_app_id(app_id)?;
        if inv.find_flatpak(app_id, FlatpakScope::System).is_some() {
            return Err(AkrepError::InvalidInput(format!(
                "Flatpak application '{}' is installed in System scope, not User scope",
                app_id
            )));
        }
        Err(AkrepError::InvalidInput(format!(
            "Flatpak application '{}' is not installed in User scope",
            app_id
        )))
    }

    pub fn preview_user_override(
        &self,
        app_id: &str,
        additions: &[(String, String)],
        removals: &[(String, String)],
    ) -> Result<UserOverridePreview, AkrepError> {
        self.ensure_target_is_installed_user_flatpak(app_id)?;
        let restart_required = self.runtime_probe.is_app_running(app_id);
        let (backup, before_doc, after_doc) = prepare_override_mutation(
            &self.user_flatpak_root,
            FlatpakScope::User,
            app_id,
            additions,
            removals,
            restart_required,
        )?;

        let diffs = build_override_plan_diffs(
            FlatpakScope::User,
            app_id,
            &before_doc,
            &after_doc,
            &backup.touched_keys,
        );
        let pre_fingerprints = compute_override_fingerprints_for_keys(
            FlatpakScope::User,
            app_id,
            &before_doc,
            &backup.touched_keys,
        );

        let mut warnings_tr = Vec::new();
        let mut warnings_en = Vec::new();
        if restart_required {
            warnings_tr.push(format!(
                "{} su anda calisiyor; yeni izinlerin etkinlesmesi icin uygulamayi yeniden baslatin.",
                app_id
            ));
            warnings_en.push(format!(
                "{} is currently running; restart the application for sandbox changes to take effect.",
                app_id
            ));
        }

        Ok(UserOverridePreview {
            app_id: app_id.to_string(),
            scope: FlatpakScope::User,
            risk: RiskLevel::Low,
            diffs,
            pre_fingerprints,
            restart_required,
            warnings_tr,
            warnings_en,
        })
    }

    fn ensure_tx_dir(&self) -> Result<(), AkrepError> {
        ensure_not_symlink(&self.user_tx_dir, "user transaction directory")?;
        fs::create_dir_all(&self.user_tx_dir)?;
        fs::set_permissions(&self.user_tx_dir, fs::Permissions::from_mode(0o700))?;
        Ok(())
    }

    fn persist_user_tx(&self, tx: &UserOverrideTransaction) -> Result<(), AkrepError> {
        self.ensure_tx_dir()?;
        let target = self.user_tx_dir.join(format!("{}.json", tx.tx_id));
        ensure_not_symlink(&target, "user transaction record")?;
        let nonce = generate_uuid_v4()?;
        let tmp_path = self
            .user_tx_dir
            .join(format!(".{}.{}.tmp", tx.tx_id, nonce));
        let payload = serde_json::to_vec_pretty(tx)
            .map_err(|e| AkrepError::Internal(format!("user tx serialize error: {}", e)))?;

        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .mode(0o600)
            .open(&tmp_path)?;
        fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))?;
        f.write_all(&payload)?;
        f.sync_all()?;
        drop(f);

        fs::rename(&tmp_path, &target)?;
        fs::set_permissions(&target, fs::Permissions::from_mode(0o600))?;
        sync_dir(&self.user_tx_dir)?;
        Ok(())
    }

    pub fn apply_user_override(
        &self,
        app_id: &str,
        additions: &[(String, String)],
        removals: &[(String, String)],
    ) -> Result<UserOverrideTransaction, AkrepError> {
        self.ensure_target_is_installed_user_flatpak(app_id)?;
        self.ensure_tx_dir()?;
        let _lock = UserFlockGuard::acquire_exclusive(&self.user_tx_dir.join("tx.lock"))?;

        let restart_required = self.runtime_probe.is_app_running(app_id);
        let (backup, before_doc, after_doc) = prepare_override_mutation(
            &self.user_flatpak_root,
            FlatpakScope::User,
            app_id,
            additions,
            removals,
            restart_required,
        )?;
        let pre_fingerprints = compute_override_fingerprints_for_keys(
            FlatpakScope::User,
            app_id,
            &before_doc,
            &backup.touched_keys,
        );

        write_override_file_atomic_nofollow(
            &self.user_flatpak_root,
            app_id,
            &after_doc.to_ini_string(),
            0o700,
            0o644,
        )?;

        let post_fingerprints = match verify_override_post_state_nofollow(
            &self.user_flatpak_root,
            &backup,
            Some(0o644),
        ) {
            Ok(fps) => fps,
            Err(verify_err) => {
                let _ = rollback_override_key_level_nofollow(
                    &self.user_flatpak_root,
                    &backup,
                    false,
                    0o700,
                    0o644,
                );
                return Err(verify_err);
            }
        };

        let now = Utc::now().to_rfc3339();
        let tx = UserOverrideTransaction {
            tx_id: generate_uuid_v4()?,
            app_id: app_id.to_string(),
            scope: FlatpakScope::User,
            state: TransactionState::Committed,
            additions: additions.to_vec(),
            removals: removals.to_vec(),
            backup,
            pre_fingerprints,
            post_fingerprints,
            restart_required,
            created_at: now.clone(),
            updated_at: now,
            rollback_result: None,
        };
        self.persist_user_tx(&tx)?;
        Ok(tx)
    }

    pub fn rollback_user_override(
        &self,
        tx_id: &str,
    ) -> Result<UserOverrideTransaction, AkrepError> {
        if tx_id.is_empty()
            || tx_id.len() > 128
            || tx_id.contains("..")
            || !tx_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(AkrepError::InvalidInput("Invalid user tx_id".to_string()));
        }
        self.ensure_tx_dir()?;
        let _lock = UserFlockGuard::acquire_exclusive(&self.user_tx_dir.join("tx.lock"))?;

        let tx_path = self.user_tx_dir.join(format!("{}.json", tx_id));
        if !tx_path.exists() {
            return Err(AkrepError::InvalidInput(format!(
                "Unknown user override transaction '{}'",
                tx_id
            )));
        }
        let raw = open_read_nofollow(&tx_path, "user transaction file")?;
        let mut tx: UserOverrideTransaction = serde_json::from_str(&raw).map_err(|e| {
            AkrepError::Internal(format!(
                "Corrupt user override transaction '{}': {}",
                tx_id,
                redact_secrets(&e.to_string())
            ))
        })?;

        if tx.state != TransactionState::Committed {
            return Err(AkrepError::InvalidInput(format!(
                "User transaction '{}' is in state {:?} and cannot be rolled back",
                tx_id, tx.state
            )));
        }

        let summary = rollback_override_key_level_nofollow(
            &self.user_flatpak_root,
            &tx.backup,
            true,
            0o700,
            0o644,
        )?;
        tx.state = TransactionState::RolledBack;
        tx.updated_at = Utc::now().to_rfc3339();
        tx.rollback_result = Some(summary);
        self.persist_user_tx(&tx)?;
        Ok(tx)
    }

    pub fn list_user_transactions(
        &self,
        limit: usize,
    ) -> Result<Vec<UserOverrideTransaction>, AkrepError> {
        if !self.user_tx_dir.exists() || limit == 0 {
            return Ok(Vec::new());
        }
        ensure_not_symlink(&self.user_tx_dir, "user transaction directory")?;
        let mut out = Vec::new();
        for entry in fs::read_dir(&self.user_tx_dir)? {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(raw) = open_read_nofollow(&path, "user transaction file") {
                if let Ok(tx) = serde_json::from_str::<UserOverrideTransaction>(&raw) {
                    out.push(tx);
                }
            }
        }
        out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        out.truncate(limit.min(500));
        Ok(out)
    }
}
