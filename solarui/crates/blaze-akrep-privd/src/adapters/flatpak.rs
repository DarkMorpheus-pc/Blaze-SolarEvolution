use crate::transaction_engine::{
    write_json_atomic_0600, ApplyOutcome, MutationBackend, PreviewOutcome,
};
use async_trait::async_trait;
use blaze_akrep_common::{
    build_override_plan_diffs, compute_override_fingerprints_for_keys,
    compute_touched_override_keys, format_override_key_target, inspect_flatpak_apps_in_root,
    inspect_native_desktop_apps, prepare_override_mutation, read_override_file_nofollow,
    rollback_override_key_level_nofollow, validate_flatpak_app_id,
    verify_override_post_state_nofollow, write_override_file_atomic_nofollow, ActionRequest,
    AkrepError, FlatpakInstallationSnapshot, FlatpakOverrideDocument, FlatpakRuntimeProbe,
    FlatpakScope, OverrideSnapshotBackup, ProcCgroupFlatpakRuntimeProbe, RiskLevel,
    StateFingerprint, TransactionRecord, DEFAULT_SYSTEM_APPLICATIONS_DIR,
    DEFAULT_SYSTEM_FLATPAK_ROOT,
};
use blaze_sentinel_core::sha256_hex;
use chrono::Utc;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const USER_SCOPE_PRIVD_REJECTION_MSG: &str =
    "User-scope Flatpak overrides must execute in user session, never as root";

fn is_forbidden_home_or_root_str(rendered: &str) -> bool {
    rendered == "/home"
        || rendered.starts_with("/home/")
        || rendered == "/var/home"
        || rendered.starts_with("/var/home/")
        || rendered == "/root"
        || rendered
            .strip_prefix("/root")
            .is_some_and(|s| s.starts_with('/'))
}

fn ensure_not_home_path(path: &Path) -> Result<(), AkrepError> {
    if is_forbidden_home_or_root_str(&path.to_string_lossy()) {
        return Err(AkrepError::ForbiddenOperation(
            "akrep-privd ProtectHome invariant forbids accessing /home or /root paths".to_string(),
        ));
    }

    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if let Ok(link_target) = fs::read_link(ancestor) {
            let resolved = if link_target.is_absolute() {
                link_target
            } else if let Some(parent) = ancestor.parent() {
                parent.join(link_target)
            } else {
                link_target
            };
            if is_forbidden_home_or_root_str(&resolved.to_string_lossy()) {
                return Err(AkrepError::ForbiddenOperation(
                    "akrep-privd ProtectHome invariant forbids symlinked /home or /root paths"
                        .to_string(),
                ));
            }
        }
        if let Ok(canon) = fs::canonicalize(ancestor) {
            if is_forbidden_home_or_root_str(&canon.to_string_lossy()) {
                return Err(AkrepError::ForbiddenOperation(
                    "akrep-privd ProtectHome invariant forbids canonical /home or /root paths"
                        .to_string(),
                ));
            }
            break;
        }
    }

    Ok(())
}

pub struct SystemFlatpakAdapter {
    flatpak_root: PathBuf,
    applications_dir: PathBuf,
    runtime_probe: Arc<dyn FlatpakRuntimeProbe>,
}

type ExtractedOverrideReq<'a> = (&'a str, &'a [(String, String)], &'a [(String, String)]);

impl SystemFlatpakAdapter {
    pub fn new_system() -> Self {
        Self {
            flatpak_root: PathBuf::from(DEFAULT_SYSTEM_FLATPAK_ROOT),
            applications_dir: PathBuf::from(DEFAULT_SYSTEM_APPLICATIONS_DIR),
            runtime_probe: Arc::new(ProcCgroupFlatpakRuntimeProbe::new()),
        }
    }

    pub fn with_roots(
        flatpak_root: PathBuf,
        applications_dir: PathBuf,
        runtime_probe: Arc<dyn FlatpakRuntimeProbe>,
    ) -> Self {
        Self {
            flatpak_root,
            applications_dir,
            runtime_probe,
        }
    }

    pub fn flatpak_root(&self) -> &Path {
        &self.flatpak_root
    }

    pub fn applications_dir(&self) -> &Path {
        &self.applications_dir
    }

    pub fn runtime_probe(&self) -> &Arc<dyn FlatpakRuntimeProbe> {
        &self.runtime_probe
    }

    pub fn inspect_installation(&self) -> Result<FlatpakInstallationSnapshot, AkrepError> {
        ensure_not_home_path(&self.flatpak_root)?;
        ensure_not_home_path(&self.applications_dir)?;

        let running_ids = self.runtime_probe.running_app_ids();
        let (global_raw, global_override_tokens, apps) =
            inspect_flatpak_apps_in_root(&self.flatpak_root, FlatpakScope::System, &running_ids)?;

        let mut known_flatpaks = BTreeSet::new();
        for app in &apps {
            known_flatpaks.insert(app.app_id.clone());
        }
        let native_apps = inspect_native_desktop_apps(
            std::slice::from_ref(&self.applications_dir),
            &known_flatpaks,
        )?;

        Ok(FlatpakInstallationSnapshot {
            scope: FlatpakScope::System,
            root_path: self.flatpak_root.to_string_lossy().to_string(),
            global_override_present: global_raw.is_some(),
            global_override_tokens,
            apps,
            native_apps,
        })
    }

    fn extract_system_override_request<'a>(
        &self,
        req: &'a ActionRequest,
    ) -> Result<ExtractedOverrideReq<'a>, AkrepError> {
        ensure_not_home_path(&self.flatpak_root)?;
        ensure_not_home_path(&self.applications_dir)?;

        let ActionRequest::FlatpakSetOverride {
            app_id,
            scope,
            additions,
            removals,
        } = req
        else {
            return Err(AkrepError::InvalidInput(
                "SystemFlatpakAdapter received non-Flatpak ActionRequest".to_string(),
            ));
        };

        if *scope == FlatpakScope::User {
            return Err(AkrepError::ForbiddenOperation(
                USER_SCOPE_PRIVD_REJECTION_MSG.to_string(),
            ));
        }

        if app_id.trim().eq_ignore_ascii_case("global") {
            return Err(AkrepError::ForbiddenOperation(
                "Global Flatpak overrides ('global') are strictly forbidden".to_string(),
            ));
        }

        let snap = self.inspect_installation()?;
        if snap.apps.iter().any(|a| a.app_id == app_id.as_str()) {
            req.validate_red_lines()?;
            return Ok((app_id.as_str(), additions.as_slice(), removals.as_slice()));
        }

        let stripped = app_id.strip_suffix(".desktop").unwrap_or(app_id.as_str());
        if snap.native_apps.iter().any(|n| {
            let n_base = n
                .desktop_id
                .strip_suffix(".desktop")
                .unwrap_or(&n.desktop_id);
            n.desktop_id == app_id.as_str() || n_base == stripped
        }) {
            return Err(AkrepError::ForbiddenOperation(format!(
                "Application '{}' is a native host package (sandbox_supported=false); Flatpak sandbox overrides are unsupported",
                app_id
            )));
        }

        validate_flatpak_app_id(app_id)?;
        req.validate_red_lines()?;
        Err(AkrepError::InvalidInput(format!(
            "Flatpak application '{}' is not installed in System scope",
            app_id
        )))
    }
}

#[async_trait]
impl MutationBackend for SystemFlatpakAdapter {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        let (app_id, additions, removals) = self.extract_system_override_request(req)?;
        let restart_required = self.runtime_probe.is_app_running(app_id);

        let (backup, before_doc, after_doc) = prepare_override_mutation(
            &self.flatpak_root,
            FlatpakScope::System,
            app_id,
            additions,
            removals,
            restart_required,
        )?;

        let diffs = build_override_plan_diffs(
            FlatpakScope::System,
            app_id,
            &before_doc,
            &after_doc,
            &backup.touched_keys,
        );
        let pre_fingerprints = compute_override_fingerprints_for_keys(
            FlatpakScope::System,
            app_id,
            &before_doc,
            &backup.touched_keys,
        );

        let mut warnings_tr = Vec::new();
        let mut warnings_en = Vec::new();
        if restart_required {
            warnings_tr.push(format!(
                "{} su anda calisiyor; yeni sistem kum havuzu izinlerinin uygulanmasi icin uygulamayi yeniden baslatin.",
                app_id
            ));
            warnings_en.push(format!(
                "{} is currently running; restart the application for system sandbox overrides to take effect.",
                app_id
            ));
        }

        Ok(PreviewOutcome {
            risk: RiskLevel::Medium,
            diffs,
            pre_fingerprints,
            warnings_tr,
            warnings_en,
        })
    }

    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        let (app_id, additions, removals) = self.extract_system_override_request(req)?;
        let restart_required = self.runtime_probe.is_app_running(app_id);

        let (backup, _before_doc, after_doc) = prepare_override_mutation(
            &self.flatpak_root,
            FlatpakScope::System,
            app_id,
            additions,
            removals,
            restart_required,
        )?;

        let backup_path = snapshot_dir.join("override_backup.json");
        write_json_atomic_0600(&backup_path, &backup)?;

        write_override_file_atomic_nofollow(
            &self.flatpak_root,
            app_id,
            &after_doc.to_ini_string(),
            0o755,
            0o644,
        )?;

        let pre_snapshot = serde_json::to_value(&backup)
            .map_err(|e| AkrepError::Internal(format!("backup serialize failed: {}", e)))?;
        let target_state = serde_json::json!({
            "app_id": app_id,
            "scope": "system",
            "restart_required": restart_required,
            "diffs": backup.diffs,
            "post_key_tokens": backup.post_key_tokens,
        });
        let summary = format!(
            "Applied system Flatpak override for {} (restart_required={})",
            app_id, restart_required
        );

        Ok(ApplyOutcome {
            pre_snapshot,
            target_state,
            summary,
        })
    }

    async fn verify(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let (_app_id, _additions, _removals) = self.extract_system_override_request(req)?;
        let backup: OverrideSnapshotBackup = serde_json::from_value(expected.pre_snapshot.clone())
            .map_err(|e| {
                AkrepError::VerifyFailed(format!("Invalid Flatpak pre_snapshot metadata: {}", e))
            })?;

        verify_override_post_state_nofollow(&self.flatpak_root, &backup, Some(0o644))
    }

    async fn current_fingerprints(
        &self,
        req: &ActionRequest,
        _post_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        ensure_not_home_path(&self.flatpak_root)?;
        let ActionRequest::FlatpakSetOverride {
            app_id,
            scope,
            additions,
            removals,
        } = req
        else {
            return Err(AkrepError::InvalidInput(
                "SystemFlatpakAdapter received non-Flatpak ActionRequest".to_string(),
            ));
        };
        if *scope == FlatpakScope::User {
            return Err(AkrepError::ForbiddenOperation(
                USER_SCOPE_PRIVD_REJECTION_MSG.to_string(),
            ));
        }
        validate_flatpak_app_id(app_id)?;
        let touched_keys = compute_touched_override_keys(additions, removals)?;

        let raw_opt = match read_override_file_nofollow(&self.flatpak_root, app_id) {
            Ok(v) => v,
            Err(_) => {
                let now = Utc::now().to_rfc3339();
                return Ok(touched_keys
                    .iter()
                    .map(|(sec, key)| StateFingerprint {
                        target: format_override_key_target(FlatpakScope::System, app_id, sec, key),
                        sha256: sha256_hex(b"<unreadable_or_symlink_drift>"),
                        captured_at: now.clone(),
                    })
                    .collect());
            }
        };

        let raw = raw_opt.unwrap_or_default();
        let Ok(doc) = FlatpakOverrideDocument::parse(&raw) else {
            let now = Utc::now().to_rfc3339();
            return Ok(touched_keys
                .iter()
                .map(|(sec, key)| StateFingerprint {
                    target: format_override_key_target(FlatpakScope::System, app_id, sec, key),
                    sha256: sha256_hex(b"<corrupt_ini_drift>"),
                    captured_at: now.clone(),
                })
                .collect());
        };

        Ok(compute_override_fingerprints_for_keys(
            FlatpakScope::System,
            app_id,
            &doc,
            &touched_keys,
        ))
    }

    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        ensure_not_home_path(&self.flatpak_root)?;
        if let ActionRequest::FlatpakSetOverride {
            scope: FlatpakScope::User,
            ..
        } = &record.request
        {
            return Err(AkrepError::ForbiddenOperation(
                USER_SCOPE_PRIVD_REJECTION_MSG.to_string(),
            ));
        }

        let backup_file = snapshot_dir.join("override_backup.json");
        let backup: OverrideSnapshotBackup = if backup_file.is_file() {
            let raw = fs::read_to_string(&backup_file)?;
            serde_json::from_str(&raw).map_err(|e| {
                AkrepError::RollbackFailed(format!("Corrupt override_backup.json: {}", e))
            })?
        } else {
            serde_json::from_value(record.pre_snapshot.clone()).map_err(|e| {
                AkrepError::RollbackFailed(format!(
                    "Missing Flatpak override backup snapshot: {}",
                    e
                ))
            })?
        };

        let require_post_drift_check = !record.post_fingerprints.is_empty();
        rollback_override_key_level_nofollow(
            &self.flatpak_root,
            &backup,
            require_post_drift_check,
            0o755,
            0o644,
        )
    }

    async fn inspect_system_flatpaks(&self) -> Result<FlatpakInstallationSnapshot, AkrepError> {
        self.inspect_installation()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    #[test]
    fn test_ensure_not_home_path_blocks_symlinks_into_home_or_root() {
        assert!(ensure_not_home_path(Path::new("/var/lib/flatpak")).is_ok());
        assert!(ensure_not_home_path(&Path::new("/root").join("flatpak")).is_err());

        let tmp = TempDir::new().unwrap();
        let sym_to_home = tmp.path().join("sneaky_home_link");
        symlink("/home", &sym_to_home).unwrap();
        assert!(ensure_not_home_path(&sym_to_home).is_err());
        assert!(ensure_not_home_path(&sym_to_home.join("nested/flatpak")).is_err());

        let sym_to_root = tmp.path().join("sneaky_root_link");
        symlink("/root", &sym_to_root).unwrap();
        assert!(ensure_not_home_path(&sym_to_root).is_err());
        assert!(ensure_not_home_path(&sym_to_root.join("nested/flatpak")).is_err());
    }
}
