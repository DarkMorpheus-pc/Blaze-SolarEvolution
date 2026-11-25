use crate::adapters::firewalld::FirewallAdapter;
use crate::adapters::selinux::{RawBooleanState, SelinuxAdapter};
use crate::transaction_engine::{
    write_json_atomic_0600, ApplyOutcome, MutationBackend, PreviewOutcome,
};
use async_trait::async_trait;
use blaze_akrep_common::{
    build_cidr_port_rich_rule, generate_uuid_v4, redact_secrets, ActionRequest, AkrepError,
    ManagedBooleanBaseline, ManagedInterfaceMove, ManagedProfileState, ManagedRemovedService,
    ManagedRichRuleEntry, PlanDiffItem, ProfileCatalogResponse, RiskLevel, SecurityProfileSpec,
    SecurityProfileSummary, SelinuxRuntimeMode, StateFingerprint, TransactionRecord,
    TransactionState, OFFICIAL_PROFILE_IDS,
};
use blaze_sentinel_core::sha256_hex;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DEFAULT_PROFILES_DIR: &str = "/usr/share/akrep/profiles";
pub const DEFAULT_PUBLIC_PROFILE_STATUS_PATH: &str = "/var/cache/akrep/active_profile.json";

#[derive(Debug, Serialize)]
struct PublicProfileStatusPayload<'a> {
    active_profile_id: &'a Option<String>,
    effective_zone: &'a Option<String>,
    updated_at: &'a Option<String>,
}

#[derive(Debug, Serialize)]
struct CanonicalManagedProfileTuple<'a> {
    active_profile_id: &'a Option<String>,
    effective_zone: &'a Option<String>,
    managed_booleans: &'a [ManagedBooleanBaseline],
    managed_rich_rules: &'a [ManagedRichRuleEntry],
    moved_interfaces: &'a [ManagedInterfaceMove],
    removed_runtime_services: &'a [ManagedRemovedService],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileTxSnapshot {
    previous_managed_state: ManagedProfileState,
    target_profile_id: String,
    effective_zone: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProfileAppliedProgress {
    prev_reverted: bool,
    applied_so_far: ManagedProfileState,
}

fn format_boolean_fp(state: &RawBooleanState) -> StateFingerprint {
    let p_str = match state.persistent {
        Some(true) => "some_true",
        Some(false) => "some_false",
        None => "unknown_default",
    };
    let canonical = format!("{}|{}", state.current, p_str);
    StateFingerprint {
        target: format!("selinux.boolean.{}", state.name),
        sha256: sha256_hex(canonical.as_bytes()),
        captured_at: Utc::now().to_rfc3339(),
    }
}

fn read_file_nofollow_utf8(path: &Path) -> Result<String, AkrepError> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| {
            if e.raw_os_error() == Some(libc::ELOOP) {
                AkrepError::ForbiddenOperation(format!(
                    "Symlink profile file '{}' is forbidden",
                    path.display()
                ))
            } else {
                AkrepError::from(e)
            }
        })?;
    let meta = f.metadata()?;
    if !meta.file_type().is_file() {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Profile path '{}' must be a regular file",
            path.display()
        )));
    }
    if meta.len() > 65_536 {
        return Err(AkrepError::InvalidInput(
            "Profile TOML exceeds 64 KiB limit".to_string(),
        ));
    }
    let mut s = String::new();
    f.read_to_string(&mut s)?;
    Ok(s)
}

fn write_public_profile_status_atomic_0644(
    target: &Path,
    state: &ManagedProfileState,
) -> Result<(), AkrepError> {
    let parent = target.parent().ok_or_else(|| {
        AkrepError::Internal("public profile status path missing parent".to_string())
    })?;
    if let Ok(meta) = fs::symlink_metadata(parent) {
        if meta.file_type().is_symlink() {
            return Err(AkrepError::ForbiddenOperation(
                "Refusing to write public profile status inside symlinked directory".to_string(),
            ));
        }
    } else {
        fs::create_dir_all(parent)?;
        let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o755));
    }

    if let Ok(meta) = fs::symlink_metadata(target) {
        if meta.file_type().is_symlink() {
            return Err(AkrepError::ForbiddenOperation(
                "Refusing to overwrite symlinked public profile status file".to_string(),
            ));
        }
    }

    let payload_obj = PublicProfileStatusPayload {
        active_profile_id: &state.active_profile_id,
        effective_zone: &state.effective_zone,
        updated_at: &state.updated_at,
    };
    let bytes = serde_json::to_vec_pretty(&payload_obj)
        .map_err(|e| AkrepError::Internal(format!("public profile json encode error: {}", e)))?;

    let nonce = generate_uuid_v4()?;
    let tmp_path = parent.join(format!(".active_profile.{}.tmp", nonce));
    let mut tmp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .mode(0o644)
        .open(&tmp_path)?;
    if let Err(err) = tmp
        .set_permissions(fs::Permissions::from_mode(0o644))
        .and_then(|_| tmp.write_all(&bytes))
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
    let _ = fs::set_permissions(target, fs::Permissions::from_mode(0o644));
    if let Ok(dir_f) = File::open(parent) {
        let _ = dir_f.sync_all();
    }
    Ok(())
}

#[derive(Clone)]
pub struct ProfileAdapter {
    profiles_dir: PathBuf,
    state_dir: PathBuf,
    public_status_path: Option<PathBuf>,
    selinux: Arc<SelinuxAdapter>,
    firewall: Arc<FirewallAdapter>,
}

impl ProfileAdapter {
    pub fn with_paths(
        profiles_dir: PathBuf,
        state_dir: PathBuf,
        selinux: Arc<SelinuxAdapter>,
        firewall: Arc<FirewallAdapter>,
    ) -> Self {
        Self {
            profiles_dir,
            state_dir,
            public_status_path: None,
            selinux,
            firewall,
        }
    }

    pub fn with_public_status_path(mut self, path: PathBuf) -> Self {
        self.public_status_path = Some(path);
        self
    }

    pub fn new_system(
        state_dir: PathBuf,
        selinux: Arc<SelinuxAdapter>,
        firewall: Arc<FirewallAdapter>,
    ) -> Self {
        let public_status_path = if state_dir == Path::new("/var/lib/akrep") {
            Some(PathBuf::from(DEFAULT_PUBLIC_PROFILE_STATUS_PATH))
        } else {
            Some(state_dir.join("public_active_profile.json"))
        };
        Self {
            profiles_dir: PathBuf::from(DEFAULT_PROFILES_DIR),
            state_dir,
            public_status_path,
            selinux,
            firewall,
        }
    }

    pub fn active_profile_state_path(&self) -> PathBuf {
        self.state_dir.join("active_profile.json")
    }

    pub fn load_managed_state(&self) -> Result<ManagedProfileState, AkrepError> {
        let p = self.active_profile_state_path();
        if !p.exists() {
            return Ok(ManagedProfileState::default());
        }
        let raw = read_file_nofollow_utf8(&p)?;
        serde_json::from_str::<ManagedProfileState>(&raw).map_err(|e| {
            AkrepError::Internal(format!(
                "Corrupt active_profile.json: {}",
                redact_secrets(&e.to_string())
            ))
        })
    }

    fn save_managed_state(&self, state: &ManagedProfileState) -> Result<(), AkrepError> {
        fs::create_dir_all(&self.state_dir)?;
        write_json_atomic_0600(&self.active_profile_state_path(), state)?;
        if let Some(pub_path) = &self.public_status_path {
            let _ = write_public_profile_status_atomic_0644(pub_path, state);
        }
        Ok(())
    }

    fn managed_state_fingerprint(
        state: &ManagedProfileState,
    ) -> Result<StateFingerprint, AkrepError> {
        let tuple = CanonicalManagedProfileTuple {
            active_profile_id: &state.active_profile_id,
            effective_zone: &state.effective_zone,
            managed_booleans: &state.managed_booleans,
            managed_rich_rules: &state.managed_rich_rules,
            moved_interfaces: &state.moved_interfaces,
            removed_runtime_services: &state.removed_runtime_services,
        };
        let bytes = serde_json::to_vec(&tuple)
            .map_err(|e| AkrepError::Internal(format!("managed profile encode error: {}", e)))?;
        Ok(StateFingerprint {
            target: "profile.managed_state".to_string(),
            sha256: sha256_hex(&bytes),
            captured_at: Utc::now().to_rfc3339(),
        })
    }

    pub fn load_profile_spec(&self, profile_id: &str) -> Result<SecurityProfileSpec, AkrepError> {
        let trimmed = profile_id.trim();
        let lower = trimmed.to_ascii_lowercase();
        if lower.contains("disabled")
            || lower.contains("audit2allow")
            || lower.contains("permissive")
        {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Forbidden security profile identifier '{}'",
                profile_id
            )));
        }
        if !OFFICIAL_PROFILE_IDS.contains(&trimmed) {
            return Err(AkrepError::InvalidInput(format!(
                "Unknown security profile '{}'",
                profile_id
            )));
        }

        let toml_path = self.profiles_dir.join(format!("{}.toml", trimmed));
        if !toml_path.exists() {
            return Err(AkrepError::InvalidInput(format!(
                "Security profile file '{}.toml' is missing in '{}'",
                trimmed,
                self.profiles_dir.display()
            )));
        }
        let raw_toml = read_file_nofollow_utf8(&toml_path)?;
        SecurityProfileSpec::parse_and_validate_toml(&raw_toml, Some(trimmed))
    }

    pub fn list_profiles(&self) -> Result<ProfileCatalogResponse, AkrepError> {
        let managed_state = self.load_managed_state()?;
        let active_id = managed_state.active_profile_id.clone();
        let mut profiles = Vec::with_capacity(OFFICIAL_PROFILE_IDS.len());

        for &id in &OFFICIAL_PROFILE_IDS {
            let spec = self.load_profile_spec(id)?;
            profiles.push(SecurityProfileSummary {
                id: spec.id.clone(),
                version: spec.version,
                name_tr: spec.name_tr,
                name_en: spec.name_en,
                description_tr: spec.description_tr,
                description_en: spec.description_en,
                target_zone: spec.target_zone,
                active: active_id.as_deref() == Some(id),
            });
        }

        Ok(ProfileCatalogResponse {
            active_profile_id: active_id,
            managed_state,
            profiles,
        })
    }

    async fn ensure_prerequisites_and_resolve_zone(
        &self,
        spec: &SecurityProfileSpec,
    ) -> Result<String, AkrepError> {
        if spec.selinux_required {
            let mode = self.selinux.ops().read_runtime_mode()?;
            if mode != SelinuxRuntimeMode::Enforcing {
                return Err(AkrepError::BackendUnavailable(format!(
                    "Profile '{}' requires SELinux in Enforcing mode (current={})",
                    spec.id,
                    mode.as_str()
                )));
            }
        }
        if spec.firewalld_required && !self.firewall.backend().is_running().await? {
            return Err(AkrepError::BackendUnavailable(format!(
                "Profile '{}' requires active firewalld service",
                spec.id
            )));
        }

        let zones = self.firewall.backend().list_zones().await?;
        if zones.iter().any(|z| z == &spec.target_zone) {
            return Ok(spec.target_zone.clone());
        }
        if let Some(fb) = &spec.fallback_zone {
            if zones.iter().any(|z| z == fb) {
                return Ok(fb.clone());
            }
        }
        Err(AkrepError::InvalidInput(format!(
            "Neither target_zone '{}' nor fallback_zone is available in firewalld",
            spec.target_zone
        )))
    }

    async fn collect_touched_targets(
        &self,
        prev_state: &ManagedProfileState,
        spec: &SecurityProfileSpec,
        effective_zone: &str,
    ) -> Result<(BTreeSet<String>, BTreeSet<String>), AkrepError> {
        let mut bool_names = BTreeSet::new();
        for b in &prev_state.managed_booleans {
            bool_names.insert(b.name.clone());
        }
        for b in &spec.managed_booleans {
            bool_names.insert(b.name.clone());
        }
        for fb in &spec.forbidden_booleans {
            if self.selinux.ops().read_boolean(fb)?.is_some() {
                bool_names.insert(fb.clone());
            }
        }

        let mut zone_names = BTreeSet::new();
        zone_names.insert(effective_zone.to_string());
        if let Some(ez) = &prev_state.effective_zone {
            let all = self.firewall.backend().list_zones().await?;
            if all.iter().any(|z| z == ez) {
                zone_names.insert(ez.clone());
            }
        }
        for mr in &prev_state.managed_rich_rules {
            zone_names.insert(mr.zone.clone());
        }
        for mv in &prev_state.moved_interfaces {
            zone_names.insert(mv.prior_zone.clone());
            zone_names.insert(mv.applied_zone.clone());
        }
        for rs in &prev_state.removed_runtime_services {
            zone_names.insert(rs.zone.clone());
        }
        if spec.lock_interfaces_to_target_zone {
            let all_zones = self.firewall.backend().list_zones().await?;
            for z in all_zones {
                let st = self.firewall.backend().get_zone_state(&z).await?;
                if !st.runtime_interfaces.is_empty() {
                    zone_names.insert(z);
                }
            }
        }

        Ok((bool_names, zone_names))
    }

    async fn capture_fingerprints_for_sets(
        &self,
        managed: &ManagedProfileState,
        bool_names: &BTreeSet<String>,
        zone_names: &BTreeSet<String>,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let mut fps = Vec::with_capacity(1 + bool_names.len() + zone_names.len());
        fps.push(Self::managed_state_fingerprint(managed)?);

        for bname in bool_names {
            let st = self.selinux.ops().read_boolean(bname)?.ok_or_else(|| {
                AkrepError::InvalidInput(format!(
                    "SELinux boolean '{}' does not exist in active policy",
                    bname
                ))
            })?;
            fps.push(format_boolean_fp(&st));
        }

        for zname in zone_names {
            let zst = self.firewall.backend().get_zone_state(zname).await?;
            fps.push(FirewallAdapter::zone_fingerprint(&zst)?);
        }

        Ok(fps)
    }

    async fn revert_managed_items(&self, state: &ManagedProfileState) -> Result<(), AkrepError> {
        // 1. Remove ONLY Akrep-managed rich rules (never touch user-created rules)
        for mr in &state.managed_rich_rules {
            self.firewall
                .backend()
                .remove_runtime_rich_rule(&mr.zone, &mr.rule)
                .await?;
            if mr.permanent {
                self.firewall
                    .backend()
                    .remove_permanent_rich_rule(&mr.zone, &mr.rule)
                    .await?;
            }
        }

        // 2. Restore runtime services removed by Akrep (e.g. ssh/mdns in public-network)
        for rs in &state.removed_runtime_services {
            self.firewall
                .backend()
                .add_runtime_service(&rs.zone, &rs.service, 0)
                .await?;
        }

        // 3. Move network interfaces back to their recorded prior runtime zones
        for mv in &state.moved_interfaces {
            self.firewall
                .backend()
                .bind_runtime_interface(&mv.prior_zone, &mv.interface)
                .await?;
        }

        // 4. Restore Akrep-managed SELinux booleans back to their recorded original baselines
        for mb in &state.managed_booleans {
            if mb.applied_persistent {
                let orig_p = mb.original_persistent.unwrap_or(mb.original_runtime);
                self.selinux.ops().set_boolean(&mb.name, orig_p, true)?;
                if mb.original_runtime != orig_p {
                    self.selinux
                        .ops()
                        .set_boolean(&mb.name, mb.original_runtime, false)?;
                }
            } else {
                self.selinux
                    .ops()
                    .set_boolean(&mb.name, mb.original_runtime, false)?;
            }
        }

        Ok(())
    }

    async fn rehydrate_managed_state(&self, state: &ManagedProfileState) -> Result<(), AkrepError> {
        for mb in &state.managed_booleans {
            self.selinux
                .ops()
                .set_boolean(&mb.name, mb.applied_value, mb.applied_persistent)?;
        }
        for mr in &state.managed_rich_rules {
            if mr.permanent {
                self.firewall
                    .backend()
                    .add_permanent_rich_rule(&mr.zone, &mr.rule)
                    .await?;
            }
            self.firewall
                .backend()
                .add_runtime_rich_rule(&mr.zone, &mr.rule, 0)
                .await?;
        }
        for mv in &state.moved_interfaces {
            self.firewall
                .backend()
                .bind_runtime_interface(&mv.applied_zone, &mv.interface)
                .await?;
        }
        for rs in &state.removed_runtime_services {
            self.firewall
                .backend()
                .remove_runtime_service(&rs.zone, &rs.service)
                .await?;
        }
        Ok(())
    }
}

#[async_trait]
impl MutationBackend for ProfileAdapter {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        req.validate_red_lines()?;
        let ActionRequest::ApplyProfile { profile_id } = req else {
            return Err(AkrepError::InvalidInput(
                "ProfileAdapter received non-ApplyProfile ActionRequest".to_string(),
            ));
        };

        let spec = self.load_profile_spec(profile_id)?;
        let effective_zone = self.ensure_prerequisites_and_resolve_zone(&spec).await?;
        let prev_managed = self.load_managed_state()?;

        let (bool_names, zone_names) = self
            .collect_touched_targets(&prev_managed, &spec, &effective_zone)
            .await?;
        let pre_fps = self
            .capture_fingerprints_for_sets(&prev_managed, &bool_names, &zone_names)
            .await?;

        let mut diffs = Vec::new();
        diffs.push(PlanDiffItem {
            target: "profile.active".to_string(),
            current_value: prev_managed
                .active_profile_id
                .clone()
                .unwrap_or_else(|| "none (baseline)".to_string()),
            desired_value: format!("{} (zone={})", spec.id, effective_zone),
            description_tr: spec.description_tr.clone(),
            description_en: spec.description_en.clone(),
        });

        for mb in &spec.managed_booleans {
            let cur = self.selinux.ops().read_boolean(&mb.name)?.ok_or_else(|| {
                AkrepError::InvalidInput(format!(
                    "SELinux boolean '{}' does not exist in active policy",
                    mb.name
                ))
            })?;
            diffs.push(PlanDiffItem {
                target: format!("selinux.boolean.{}", mb.name),
                current_value: format!("runtime={}", if cur.current { "on" } else { "off" }),
                desired_value: format!(
                    "runtime={}, persistent={}",
                    if mb.value { "on" } else { "off" },
                    mb.persistent
                ),
                description_tr: format!(
                    "Profil kapsaminda '{}' SELinux anahtari guncellenecek.",
                    mb.name
                ),
                description_en: format!(
                    "Configure SELinux boolean '{}' for profile '{}'.",
                    mb.name, spec.id
                ),
            });
        }

        for lr in &spec.lan_rich_rules {
            let rule_str = build_cidr_port_rich_rule(&lr.cidr, &lr.port, lr.proto)?;
            diffs.push(PlanDiffItem {
                target: format!("firewall.zone.{}.rich_rule", effective_zone),
                current_value: "absent".to_string(),
                desired_value: rule_str,
                description_tr: format!(
                    "RFC1918 yerel ag ({}) icin {}/{} izni eklenecek.",
                    lr.cidr,
                    lr.port,
                    lr.proto.as_str()
                ),
                description_en: format!(
                    "Allow {}/{} strictly from RFC1918 subnet {}.",
                    lr.port,
                    lr.proto.as_str(),
                    lr.cidr
                ),
            });
        }

        if spec.lock_interfaces_to_target_zone {
            let all_zones = self.firewall.backend().list_zones().await?;
            for z in all_zones {
                if z == effective_zone {
                    continue;
                }
                let st = self.firewall.backend().get_zone_state(&z).await?;
                for iface in st.runtime_interfaces {
                    diffs.push(PlanDiffItem {
                        target: format!("firewall.interface.{}", iface),
                        current_value: z.clone(),
                        desired_value: effective_zone.clone(),
                        description_tr: format!(
                            "'{}' ag arayuzu calisma zamaninda '{}' bolgesine tasinacak.",
                            iface, effective_zone
                        ),
                        description_en: format!(
                            "Move network interface '{}' to runtime zone '{}'.",
                            iface, effective_zone
                        ),
                    });
                }
            }
        }

        if !spec.remove_runtime_services_in_target_zone.is_empty() {
            let zst = self
                .firewall
                .backend()
                .get_zone_state(&effective_zone)
                .await?;
            for svc in &spec.remove_runtime_services_in_target_zone {
                if zst.runtime_services.contains(svc) {
                    diffs.push(PlanDiffItem {
                        target: format!("firewall.zone.{}.service.{}", effective_zone, svc),
                        current_value: "enabled (runtime)".to_string(),
                        desired_value: "closed (runtime)".to_string(),
                        description_tr: format!(
                            "'{}' bolgesinde gelen '{}' servisi calisma zamaninda kapatilacak.",
                            effective_zone, svc
                        ),
                        description_en: format!(
                            "Close runtime service '{}' in zone '{}' while on untrusted network.",
                            svc, effective_zone
                        ),
                    });
                }
            }
        }

        let risk = if spec.lock_interfaces_to_target_zone || !spec.managed_booleans.is_empty() {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };

        Ok(PreviewOutcome {
            risk,
            diffs,
            pre_fingerprints: pre_fps,
            warnings_tr: Vec::new(),
            warnings_en: Vec::new(),
        })
    }

    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        req.validate_red_lines()?;
        let ActionRequest::ApplyProfile { profile_id } = req else {
            return Err(AkrepError::InvalidInput(
                "ProfileAdapter received non-ApplyProfile ActionRequest".to_string(),
            ));
        };

        let spec = self.load_profile_spec(profile_id)?;
        let effective_zone = self.ensure_prerequisites_and_resolve_zone(&spec).await?;
        let prev_managed = self.load_managed_state()?;

        let mut pre_forbidden_booleans: BTreeMap<String, bool> = BTreeMap::new();
        for fb in &spec.forbidden_booleans {
            if let Some(st) = self.selinux.ops().read_boolean(fb)? {
                pre_forbidden_booleans.insert(fb.clone(), st.current);
            }
        }

        let tx_snap = ProfileTxSnapshot {
            previous_managed_state: prev_managed.clone(),
            target_profile_id: spec.id.clone(),
            effective_zone: effective_zone.clone(),
        };
        write_json_atomic_0600(&snapshot_dir.join("profile_tx.json"), &tx_snap)?;

        // Preserve original boolean baselines across multi-hop profile switches
        let mut prior_bool_baselines: BTreeMap<String, (bool, Option<bool>)> = BTreeMap::new();
        for b in &prev_managed.managed_booleans {
            prior_bool_baselines
                .insert(b.name.clone(), (b.original_runtime, b.original_persistent));
        }

        // Step 1: Revert all previously Akrep-managed items cleanly without touching user rules
        self.revert_managed_items(&prev_managed).await?;

        let applied_progress_path = snapshot_dir.join("profile_applied.json");
        let mut new_managed = ManagedProfileState {
            active_profile_id: Some(spec.id.clone()),
            effective_zone: Some(effective_zone.clone()),
            updated_at: Some(Utc::now().to_rfc3339()),
            managed_booleans: Vec::new(),
            managed_rich_rules: Vec::new(),
            moved_interfaces: Vec::new(),
            removed_runtime_services: Vec::new(),
        };
        write_json_atomic_0600(
            &applied_progress_path,
            &ProfileAppliedProgress {
                prev_reverted: true,
                applied_so_far: new_managed.clone(),
            },
        )?;

        if !spec.clear_managed_rules {
            // Step 2a: Apply managed SELinux booleans
            for mb in &spec.managed_booleans {
                let raw = self.selinux.ops().read_boolean(&mb.name)?.ok_or_else(|| {
                    AkrepError::InvalidInput(format!(
                        "SELinux boolean '{}' missing in policy",
                        mb.name
                    ))
                })?;
                let (orig_rt, orig_perm) = prior_bool_baselines
                    .get(&mb.name)
                    .cloned()
                    .unwrap_or((raw.current, raw.persistent));
                self.selinux
                    .ops()
                    .set_boolean(&mb.name, mb.value, mb.persistent)?;
                new_managed.managed_booleans.push(ManagedBooleanBaseline {
                    name: mb.name.clone(),
                    original_runtime: orig_rt,
                    original_persistent: orig_perm,
                    applied_value: mb.value,
                    applied_persistent: mb.persistent,
                });
                write_json_atomic_0600(
                    &applied_progress_path,
                    &ProfileAppliedProgress {
                        prev_reverted: true,
                        applied_so_far: new_managed.clone(),
                    },
                )?;
            }

            // Step 2b: Apply RFC1918 LAN rich rules (claiming ownership ONLY of rules added by Akrep)
            let pre_zone_state = self
                .firewall
                .backend()
                .get_zone_state(&effective_zone)
                .await?;
            for lr in &spec.lan_rich_rules {
                let rule_str = build_cidr_port_rich_rule(&lr.cidr, &lr.port, lr.proto)?;
                let already_user_owned = pre_zone_state.runtime_rich_rules.contains(&rule_str);
                if !already_user_owned {
                    if lr.permanent {
                        self.firewall
                            .backend()
                            .add_permanent_rich_rule(&effective_zone, &rule_str)
                            .await?;
                    }
                    if let Err(rt_err) = self
                        .firewall
                        .backend()
                        .add_runtime_rich_rule(&effective_zone, &rule_str, 0)
                        .await
                    {
                        if lr.permanent {
                            let _ = self
                                .firewall
                                .backend()
                                .remove_permanent_rich_rule(&effective_zone, &rule_str)
                                .await;
                        }
                        return Err(rt_err);
                    }
                    new_managed.managed_rich_rules.push(ManagedRichRuleEntry {
                        zone: effective_zone.clone(),
                        rule: rule_str,
                        permanent: lr.permanent,
                    });
                    write_json_atomic_0600(
                        &applied_progress_path,
                        &ProfileAppliedProgress {
                            prev_reverted: true,
                            applied_so_far: new_managed.clone(),
                        },
                    )?;
                }
            }

            // Step 2c: Lock active runtime interfaces to target_zone if requested (e.g. public-network)
            if spec.lock_interfaces_to_target_zone {
                let all_zones = self.firewall.backend().list_zones().await?;
                for z in all_zones {
                    if z == effective_zone {
                        continue;
                    }
                    let st = self.firewall.backend().get_zone_state(&z).await?;
                    for iface in st.runtime_interfaces {
                        self.firewall
                            .backend()
                            .bind_runtime_interface(&effective_zone, &iface)
                            .await?;
                        new_managed.moved_interfaces.push(ManagedInterfaceMove {
                            interface: iface,
                            prior_zone: z.clone(),
                            applied_zone: effective_zone.clone(),
                        });
                        write_json_atomic_0600(
                            &applied_progress_path,
                            &ProfileAppliedProgress {
                                prev_reverted: true,
                                applied_so_far: new_managed.clone(),
                            },
                        )?;
                    }
                }
            }

            // Step 2d: Remove configured runtime services in target_zone if currently enabled
            if !spec.remove_runtime_services_in_target_zone.is_empty() {
                let zst = self
                    .firewall
                    .backend()
                    .get_zone_state(&effective_zone)
                    .await?;
                for svc in &spec.remove_runtime_services_in_target_zone {
                    if zst.runtime_services.contains(svc) {
                        self.firewall
                            .backend()
                            .remove_runtime_service(&effective_zone, svc)
                            .await?;
                        new_managed
                            .removed_runtime_services
                            .push(ManagedRemovedService {
                                zone: effective_zone.clone(),
                                service: svc.clone(),
                            });
                        write_json_atomic_0600(
                            &applied_progress_path,
                            &ProfileAppliedProgress {
                                prev_reverted: true,
                                applied_so_far: new_managed.clone(),
                            },
                        )?;
                    }
                }
            }
        }

        new_managed
            .managed_booleans
            .sort_by(|a, b| a.name.cmp(&b.name));
        new_managed
            .managed_rich_rules
            .sort_by(|a, b| (&a.zone, &a.rule).cmp(&(&b.zone, &b.rule)));
        new_managed
            .moved_interfaces
            .sort_by(|a, b| a.interface.cmp(&b.interface));
        new_managed
            .removed_runtime_services
            .sort_by(|a, b| (&a.zone, &a.service).cmp(&(&b.zone, &b.service)));

        write_json_atomic_0600(
            &applied_progress_path,
            &ProfileAppliedProgress {
                prev_reverted: true,
                applied_so_far: new_managed.clone(),
            },
        )?;
        self.save_managed_state(&new_managed)?;

        let (touched_bools, touched_zones) = self
            .collect_touched_targets(&prev_managed, &spec, &effective_zone)
            .await?;
        let touched_bools_vec: Vec<String> = touched_bools.into_iter().collect();
        let touched_zones_vec: Vec<String> = touched_zones.into_iter().collect();

        Ok(ApplyOutcome {
            pre_snapshot: serde_json::to_value(&tx_snap)
                .map_err(|e| AkrepError::Internal(e.to_string()))?,
            target_state: serde_json::json!({
                "profile_id": spec.id,
                "effective_zone": effective_zone,
                "touched_booleans": touched_bools_vec,
                "touched_zones": touched_zones_vec,
                "pre_forbidden_booleans": pre_forbidden_booleans,
            }),
            summary: format!(
                "Applied security profile '{}' (effective_zone={})",
                spec.id, effective_zone
            ),
        })
    }

    async fn verify(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let ActionRequest::ApplyProfile { profile_id } = req else {
            return Err(AkrepError::InvalidInput(
                "ProfileAdapter received non-ApplyProfile ActionRequest".to_string(),
            ));
        };

        let spec = self.load_profile_spec(profile_id)?;
        let effective_zone = expected
            .target_state
            .get("effective_zone")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                AkrepError::VerifyFailed("Missing effective_zone in target_state".to_string())
            })?;

        if spec.selinux_required {
            let mode = self.selinux.ops().read_runtime_mode()?;
            if mode != SelinuxRuntimeMode::Enforcing {
                return Err(AkrepError::VerifyFailed(
                    "SELinux is not in Enforcing mode after profile apply".to_string(),
                ));
            }
        }
        if spec.firewalld_required && !self.firewall.backend().is_running().await? {
            return Err(AkrepError::VerifyFailed(
                "firewalld is not running after profile apply".to_string(),
            ));
        }

        let live_managed = self.load_managed_state()?;
        if live_managed.active_profile_id.as_deref() != Some(spec.id.as_str()) {
            return Err(AkrepError::VerifyFailed(
                "Persisted active_profile_id mismatch after apply".to_string(),
            ));
        }

        for mb in &spec.managed_booleans {
            let st = self.selinux.ops().read_boolean(&mb.name)?.ok_or_else(|| {
                AkrepError::VerifyFailed(format!("SELinux boolean '{}' missing", mb.name))
            })?;
            if st.current != mb.value {
                return Err(AkrepError::VerifyFailed(format!(
                    "SELinux boolean '{}' runtime value mismatch (expected {})",
                    mb.name, mb.value
                )));
            }
            if mb.persistent && st.persistent == Some(!mb.value) {
                return Err(AkrepError::VerifyFailed(format!(
                    "SELinux boolean '{}' persistent value mismatch",
                    mb.name
                )));
            }
        }

        for fb in &spec.forbidden_booleans {
            if live_managed
                .managed_booleans
                .iter()
                .any(|mb| mb.name == *fb && mb.applied_value)
            {
                return Err(AkrepError::VerifyFailed(format!(
                    "Forbidden SELinux boolean '{}' is managed as enabled!",
                    fb
                )));
            }
            let was_enabled_before = expected
                .target_state
                .get("pre_forbidden_booleans")
                .and_then(|m| m.get(fb.as_str()))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if let Some(st) = self.selinux.ops().read_boolean(fb)? {
                if st.current && !was_enabled_before {
                    return Err(AkrepError::VerifyFailed(format!(
                        "Forbidden SELinux boolean '{}' was enabled during profile apply!",
                        fb
                    )));
                }
            }
        }

        let eff_zone_st = self
            .firewall
            .backend()
            .get_zone_state(effective_zone)
            .await?;
        for lr in &spec.lan_rich_rules {
            let rule_str = build_cidr_port_rich_rule(&lr.cidr, &lr.port, lr.proto)?;
            if !eff_zone_st.runtime_rich_rules.contains(&rule_str) {
                return Err(AkrepError::VerifyFailed(format!(
                    "Expected LAN rich rule '{}' missing from zone '{}'",
                    rule_str, effective_zone
                )));
            }
        }

        if spec.lock_interfaces_to_target_zone {
            let all_zones = self.firewall.backend().list_zones().await?;
            for z in all_zones {
                if z == effective_zone {
                    continue;
                }
                let st = self.firewall.backend().get_zone_state(&z).await?;
                if !st.runtime_interfaces.is_empty() {
                    return Err(AkrepError::VerifyFailed(format!(
                        "Zone '{}' still holds active runtime interface(s) {:?}",
                        z, st.runtime_interfaces
                    )));
                }
            }
        }

        for svc in &spec.remove_runtime_services_in_target_zone {
            if eff_zone_st.runtime_services.contains(svc) {
                return Err(AkrepError::VerifyFailed(format!(
                    "Runtime service '{}' remained active in zone '{}'",
                    svc, effective_zone
                )));
            }
        }

        let touched_bools: BTreeSet<String> = expected
            .target_state
            .get("touched_booleans")
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .unwrap_or_default()
            .into_iter()
            .collect();
        let touched_zones: BTreeSet<String> = expected
            .target_state
            .get("touched_zones")
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .unwrap_or_default()
            .into_iter()
            .collect();

        self.capture_fingerprints_for_sets(&live_managed, &touched_bools, &touched_zones)
            .await
    }

    async fn current_fingerprints(
        &self,
        _req: &ActionRequest,
        reference_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let live_managed = self.load_managed_state()?;
        let mut out = Vec::with_capacity(reference_fps.len());

        for fp in reference_fps {
            if fp.target == "profile.managed_state" {
                out.push(Self::managed_state_fingerprint(&live_managed)?);
            } else if let Some(bname) = fp.target.strip_prefix("selinux.boolean.") {
                let st = self.selinux.ops().read_boolean(bname)?.ok_or_else(|| {
                    AkrepError::InvalidInput(format!(
                        "SELinux boolean '{}' missing during fingerprint check",
                        bname
                    ))
                })?;
                out.push(format_boolean_fp(&st));
            } else if let Some(zname) = fp.target.strip_prefix("firewall.zone.") {
                let zst = self.firewall.backend().get_zone_state(zname).await?;
                out.push(FirewallAdapter::zone_fingerprint(&zst)?);
            } else {
                return Err(AkrepError::Internal(format!(
                    "Unknown profile fingerprint target '{}'",
                    fp.target
                )));
            }
        }

        Ok(out)
    }

    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let snap_path = snapshot_dir.join("profile_tx.json");
        if !snap_path.is_file() && record.pre_snapshot.is_null() {
            return Ok("Profile apply aborted before state mutation".to_string());
        }
        let snap: ProfileTxSnapshot = if snap_path.is_file() {
            serde_json::from_str(&fs::read_to_string(&snap_path)?)
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        } else {
            serde_json::from_value(record.pre_snapshot.clone())
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
        };

        let applied_snap_path = snapshot_dir.join("profile_applied.json");
        if applied_snap_path.is_file() {
            let raw = fs::read_to_string(&applied_snap_path)
                .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?;
            if let Ok(progress) = serde_json::from_str::<ProfileAppliedProgress>(&raw) {
                if progress.prev_reverted {
                    self.revert_managed_items(&progress.applied_so_far)
                        .await
                        .map_err(|e| AkrepError::RollbackFailed(e.message().to_string()))?;
                }
            } else if let Ok(legacy_state) = serde_json::from_str::<ManagedProfileState>(&raw) {
                self.revert_managed_items(&legacy_state)
                    .await
                    .map_err(|e| AkrepError::RollbackFailed(e.message().to_string()))?;
            } else {
                return Err(AkrepError::RollbackFailed(
                    "Corrupt profile_applied.json in transaction snapshot".to_string(),
                ));
            }
        } else if record.state != TransactionState::RollingBack || record.apply_result.is_some() {
            // Only revert live_managed if apply had actually completed; if Step 1 failed before
            // profile_applied.json existed, rehydrating previous_managed_state below restores Step 1 cleanly.
            if let Ok(live_managed) = self.load_managed_state() {
                self.revert_managed_items(&live_managed)
                    .await
                    .map_err(|e| AkrepError::RollbackFailed(e.message().to_string()))?;
            }
        }

        self.rehydrate_managed_state(&snap.previous_managed_state)
            .await
            .map_err(|e| AkrepError::RollbackFailed(e.message().to_string()))?;

        self.save_managed_state(&snap.previous_managed_state)
            .map_err(|e| AkrepError::RollbackFailed(e.message().to_string()))?;

        let prev_label = snap
            .previous_managed_state
            .active_profile_id
            .as_deref()
            .unwrap_or("baseline");
        Ok(format!(
            "Rolled back profile '{}' to '{}' while preserving user rules",
            snap.target_profile_id, prev_label
        ))
    }
}
