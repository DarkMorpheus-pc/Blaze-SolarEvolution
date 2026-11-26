use blaze_akrep_common::{
    validate_container_dir_path, ActionRequest, AkrepError, CompatPackageGroup,
    DeviceSecurityStatus, MockFlatpakRuntimeProbe, OverrideDuration, OverrideSubsystem, RiskLevel,
    SelinuxDesiredMode, SelinuxRuntimeMode, TransactionState, POLKIT_SELINUX_MANAGE,
};
use blaze_akrep_privd::{
    inspect_driver_mok_at, inspect_secure_boot_at, parse_mokutil_enrolled_count,
    parse_mokutil_sb_state, parse_selinux_config_mode, CompositeMutationBackend,
    DeviceSecurityAdapter, FirewallAdapter, FirewallBackend, LinuxDeviceSecurityOps,
    MockAuthorizer, MockClamScanner, MockDeviceSecurityOps, MockFirewallBackend,
    MockScannerInstallerOps, MockSelinuxOps, OverrideAdapter, OverrideOps, OverrideStoreDocument,
    ProfileAdapter, QuarantineAdapter, ScannerAdapter, SelinuxAdapter, SelinuxSysOps,
    SystemFlatpakAdapter, TransactionEngine, EFI_SECURE_BOOT_VAR_REL, SYSTEMD_FIREWALLD_UNIT,
    SYSTEMD_OOMD_UNIT,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

fn official_profiles_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../blazeos_custom_apps/usr/share/akrep/profiles")
        .canonicalize()
        .expect("official profiles dir must exist")
}

struct BridgedOverrideOps {
    selinux_ops: Arc<MockSelinuxOps>,
    firewall_backend: Arc<MockFirewallBackend>,
    selinux_config: Mutex<Option<String>>,
    units: Mutex<HashMap<String, (bool, bool)>>,
    timers: Mutex<HashMap<String, (u64, String)>>,
    nofile_pam: Mutex<Option<String>>,
    nofile_systemd: Mutex<Option<String>>,
    container_dirs: Mutex<HashMap<String, (PathBuf, String)>>,
    installed_packages: Mutex<HashSet<String>>,
    recorded_ops: Mutex<Vec<String>>,
}

impl BridgedOverrideOps {
    fn new(selinux_ops: Arc<MockSelinuxOps>, firewall_backend: Arc<MockFirewallBackend>) -> Self {
        let mut units = HashMap::new();
        units.insert(SYSTEMD_FIREWALLD_UNIT.to_string(), (true, true));
        units.insert(SYSTEMD_OOMD_UNIT.to_string(), (true, true));
        let default_cfg = "\
# SELinux configuration
#     permissive - SELinux prints warnings instead of enforcing.
SELINUX=enforcing
SELINUXTYPE=targeted
";
        Self {
            selinux_ops,
            firewall_backend,
            selinux_config: Mutex::new(Some(default_cfg.to_string())),
            units: Mutex::new(units),
            timers: Mutex::new(HashMap::new()),
            nofile_pam: Mutex::new(None),
            nofile_systemd: Mutex::new(None),
            container_dirs: Mutex::new(HashMap::new()),
            installed_packages: Mutex::new(HashSet::new()),
            recorded_ops: Mutex::new(Vec::new()),
        }
    }

    fn register_container_dir(&self, logical_path: &str, backing_dir: PathBuf, ctx: &str) {
        self.container_dirs
            .lock()
            .unwrap()
            .insert(logical_path.to_string(), (backing_dir, ctx.to_string()));
    }

    fn selinux_config_text(&self) -> Option<String> {
        self.selinux_config.lock().unwrap().clone()
    }

    fn has_timer(&self, subsys: OverrideSubsystem) -> bool {
        let key = format!("akrep-revert-{}.timer", subsys.as_str());
        self.timers.lock().unwrap().contains_key(&key)
    }

    fn recorded_ops(&self) -> Vec<String> {
        self.recorded_ops.lock().unwrap().clone()
    }
}

impl OverrideOps for BridgedOverrideOps {
    fn read_boot_id(&self) -> Result<String, AkrepError> {
        Ok("boot-tx-test-1".to_string())
    }

    fn read_selinux_runtime_mode(&self) -> Result<SelinuxRuntimeMode, AkrepError> {
        self.selinux_ops.read_runtime_mode()
    }

    fn setenforce(&self, enforcing: bool) -> Result<(), AkrepError> {
        if self.selinux_ops.read_runtime_mode()? == SelinuxRuntimeMode::Disabled {
            return Err(AkrepError::BackendUnavailable(
                "SELinux is disabled".to_string(),
            ));
        }
        self.selinux_ops.set_runtime_mode(if enforcing {
            SelinuxRuntimeMode::Enforcing
        } else {
            SelinuxRuntimeMode::Permissive
        });
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
        let parsed = parse_selinux_config_mode(content);
        if parsed.as_deref() == Some("disabled") {
            return Err(AkrepError::ForbiddenOperation(
                "SELINUX=disabled is forbidden".to_string(),
            ));
        }
        *self.selinux_config.lock().unwrap() = Some(content.to_string());
        self.selinux_ops
            .set_config(parsed.as_deref(), Some("targeted"));
        self.recorded_ops
            .lock()
            .unwrap()
            .push("write_selinux_config".to_string());
        Ok(())
    }

    fn is_unit_active(&self, unit: &str) -> Result<bool, AkrepError> {
        if unit == SYSTEMD_FIREWALLD_UNIT {
            return Ok(self.firewall_backend.is_running_sync());
        }
        Ok(self
            .units
            .lock()
            .unwrap()
            .get(unit)
            .map(|(active, _)| *active)
            .unwrap_or(false))
    }

    fn is_unit_enabled(&self, unit: &str) -> Result<bool, AkrepError> {
        Ok(self
            .units
            .lock()
            .unwrap()
            .get(unit)
            .map(|(_, enabled)| *enabled)
            .unwrap_or(false))
    }

    fn start_unit(&self, unit: &str) -> Result<(), AkrepError> {
        if unit == SYSTEMD_FIREWALLD_UNIT {
            self.firewall_backend.set_running(true);
        }
        if let Some((active, _)) = self.units.lock().unwrap().get_mut(unit) {
            *active = true;
        }
        self.recorded_ops
            .lock()
            .unwrap()
            .push(format!("systemctl start {}", unit));
        Ok(())
    }

    fn stop_unit(&self, unit: &str) -> Result<(), AkrepError> {
        if unit == SYSTEMD_FIREWALLD_UNIT {
            self.firewall_backend.set_running(false);
        }
        if let Some((active, _)) = self.units.lock().unwrap().get_mut(unit) {
            *active = false;
        }
        self.recorded_ops
            .lock()
            .unwrap()
            .push(format!("systemctl stop {}", unit));
        Ok(())
    }

    fn enable_unit(&self, unit: &str) -> Result<(), AkrepError> {
        if let Some((_, enabled)) = self.units.lock().unwrap().get_mut(unit) {
            *enabled = true;
        }
        self.recorded_ops
            .lock()
            .unwrap()
            .push(format!("systemctl enable {}", unit));
        Ok(())
    }

    fn disable_unit(&self, unit: &str) -> Result<(), AkrepError> {
        if let Some((_, enabled)) = self.units.lock().unwrap().get_mut(unit) {
            *enabled = false;
        }
        self.recorded_ops
            .lock()
            .unwrap()
            .push(format!("systemctl disable {}", unit));
        Ok(())
    }

    fn daemon_reload(&self) -> Result<(), AkrepError> {
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
        Ok(unit)
    }

    fn cancel_revert_timer(&self, subsystem: OverrideSubsystem) -> Result<(), AkrepError> {
        let unit = format!("akrep-revert-{}.timer", subsystem.as_str());
        self.timers.lock().unwrap().remove(&unit);
        Ok(())
    }

    fn read_nofile_dropins(&self) -> Result<(Option<String>, Option<String>), AkrepError> {
        Ok((
            self.nofile_pam.lock().unwrap().clone(),
            self.nofile_systemd.lock().unwrap().clone(),
        ))
    }

    fn write_nofile_dropins(&self, pam: &str, systemd: &str) -> Result<(), AkrepError> {
        *self.nofile_pam.lock().unwrap() = Some(pam.to_string());
        *self.nofile_systemd.lock().unwrap() = Some(systemd.to_string());
        Ok(())
    }

    fn remove_nofile_dropins(&self) -> Result<(), AkrepError> {
        *self.nofile_pam.lock().unwrap() = None;
        *self.nofile_systemd.lock().unwrap() = None;
        Ok(())
    }

    fn restore_nofile_dropins(
        &self,
        pam_content: Option<&str>,
        systemd_content: Option<&str>,
    ) -> Result<(), AkrepError> {
        *self.nofile_pam.lock().unwrap() = pam_content.map(ToString::to_string);
        *self.nofile_systemd.lock().unwrap() = systemd_content.map(ToString::to_string);
        Ok(())
    }

    fn resolve_container_dir(&self, path: &Path) -> Result<PathBuf, AkrepError> {
        let raw = path.to_string_lossy().to_string();
        let normalized = validate_container_dir_path(&raw)?;
        let dirs = self.container_dirs.lock().unwrap();
        let Some((backing, _)) = dirs.get(&normalized) else {
            return Err(AkrepError::InvalidInput(format!(
                "Container directory '{}' does not exist",
                normalized
            )));
        };
        if !backing.is_dir() {
            return Err(AkrepError::InvalidInput(format!(
                "Backing directory for '{}' is missing",
                normalized
            )));
        }
        Ok(PathBuf::from(normalized))
    }

    fn read_path_selinux_context(&self, path: &Path) -> Result<String, AkrepError> {
        let key = path.to_string_lossy().to_string();
        self.container_dirs
            .lock()
            .unwrap()
            .get(&key)
            .map(|(_, ctx)| ctx.clone())
            .ok_or_else(|| AkrepError::InvalidInput(format!("Missing container dir '{}'", key)))
    }

    fn chcon_container_dir(&self, canonical_dir: &Path, recursive: bool) -> Result<(), AkrepError> {
        let key = canonical_dir.to_string_lossy().to_string();
        let mut dirs = self.container_dirs.lock().unwrap();
        let Some((_, ctx)) = dirs.get_mut(&key) else {
            return Err(AkrepError::InvalidInput(format!(
                "Missing container dir '{}'",
                key
            )));
        };
        *ctx = "unconfined_u:object_r:container_file_t:s0".to_string();
        self.recorded_ops.lock().unwrap().push(format!(
            "chcon --no-dereference{} -t container_file_t {}",
            if recursive { " -R" } else { "" },
            key
        ));
        Ok(())
    }

    fn restorecon_container_dir(
        &self,
        canonical_dir: &Path,
        recursive: bool,
    ) -> Result<(), AkrepError> {
        let key = canonical_dir.to_string_lossy().to_string();
        let mut dirs = self.container_dirs.lock().unwrap();
        let Some((_, ctx)) = dirs.get_mut(&key) else {
            return Err(AkrepError::InvalidInput(format!(
                "Missing container dir '{}'",
                key
            )));
        };
        *ctx = "unconfined_u:object_r:user_home_t:s0".to_string();
        self.recorded_ops.lock().unwrap().push(format!(
            "restorecon -F{} {}",
            if recursive { " -R" } else { "" },
            key
        ));
        Ok(())
    }

    fn is_packages_installed(&self, packages: &[&str]) -> Result<bool, AkrepError> {
        let installed = self.installed_packages.lock().unwrap();
        Ok(packages.iter().all(|p| installed.contains(*p)))
    }

    fn install_compat_packages(&self, group: CompatPackageGroup) -> Result<(), AkrepError> {
        let mut installed = self.installed_packages.lock().unwrap();
        for p in group.rpm_packages() {
            installed.insert((*p).to_string());
        }
        Ok(())
    }

    fn remove_compat_packages(&self, group: CompatPackageGroup) -> Result<(), AkrepError> {
        let mut installed = self.installed_packages.lock().unwrap();
        for p in group.rpm_packages() {
            installed.remove(*p);
        }
        Ok(())
    }
}

struct FlexibleTxHarness {
    root_tmp: TempDir,
    state_dir: PathBuf,
    public_overrides_path: PathBuf,
    selinux_ops: Arc<MockSelinuxOps>,
    firewall_backend: Arc<MockFirewallBackend>,
    override_ops: Arc<BridgedOverrideOps>,
    overrides: Arc<OverrideAdapter>,
    engine: TransactionEngine,
}

fn setup_flexible_tx_harness() -> FlexibleTxHarness {
    let root_tmp = TempDir::new().expect("tempdir");
    let state_dir = root_tmp.path().join("var_lib_akrep");
    let public_cache_dir = root_tmp.path().join("var_cache_akrep");
    let public_overrides_path = public_cache_dir.join("active_overrides.json");
    let public_profile_path = public_cache_dir.join("active_profile.json");
    let sys_fp = root_tmp.path().join("var_lib_flatpak");
    let sys_apps = root_tmp.path().join("usr_share_applications");
    fs::create_dir_all(&sys_fp).unwrap();
    fs::create_dir_all(&sys_apps).unwrap();

    let auth = Arc::new(MockAuthorizer::new());
    auth.allow_all_for_bus(":1.40", 1000);

    let selinux_ops = Arc::new(MockSelinuxOps::new_fedora_enforcing());
    let firewall_backend = Arc::new(MockFirewallBackend::new_fedora_workstation());
    let override_ops = Arc::new(BridgedOverrideOps::new(
        selinux_ops.clone(),
        firewall_backend.clone(),
    ));
    let overrides = Arc::new(OverrideAdapter::new_with_ops(
        state_dir.clone(),
        public_overrides_path.clone(),
        override_ops.clone(),
    ));

    let selinux = Arc::new(SelinuxAdapter::new(selinux_ops.clone()));
    let firewall = Arc::new(FirewallAdapter::new(firewall_backend.clone()));
    let flatpak = Arc::new(SystemFlatpakAdapter::with_roots(
        sys_fp,
        sys_apps,
        Arc::new(MockFlatpakRuntimeProbe::new()),
    ));
    let device_security = Arc::new(DeviceSecurityAdapter::new(Arc::new(
        MockDeviceSecurityOps::new_healthy_uefi(),
    )));
    let scanner = Arc::new(ScannerAdapter::with_options(
        state_dir.clone(),
        Arc::new(MockClamScanner::new_healthy()),
        Arc::new(MockScannerInstallerOps::new(false)),
        true,
    ));
    let quarantine = Arc::new(QuarantineAdapter::new_lazy(state_dir.clone()));
    let profiles = Arc::new(
        ProfileAdapter::with_paths(
            official_profiles_dir(),
            state_dir.clone(),
            selinux.clone(),
            firewall.clone(),
        )
        .with_public_status_path(public_profile_path)
        .with_overrides(overrides.clone()),
    );

    let composite = Arc::new(CompositeMutationBackend::new_with_overrides(
        selinux,
        firewall,
        flatpak,
        device_security,
        scanner,
        quarantine,
        profiles,
        overrides.clone(),
    ));

    let engine =
        TransactionEngine::new_with_options(state_dir.clone(), auth, composite, true).unwrap();

    FlexibleTxHarness {
        root_tmp,
        state_dir,
        public_overrides_path,
        selinux_ops,
        firewall_backend,
        override_ops,
        overrides,
        engine,
    }
}

#[tokio::test]
async fn test_tx_selinux_mode_preview_commit_and_rollback() {
    let h = setup_flexible_tx_harness();

    let req = ActionRequest::SelinuxSetMode {
        mode: SelinuxDesiredMode::Permissive,
        duration: OverrideDuration::UntilChanged,
    };
    let plan = h.engine.create_plan(":1.40", req).await.unwrap();
    assert_eq!(plan.polkit_action, POLKIT_SELINUX_MANAGE);
    assert_eq!(plan.risk, RiskLevel::High);
    assert!(plan.diffs.iter().any(|d| d.target == "selinux.mode"));

    let committed = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(committed.state, TransactionState::Committed);
    assert_eq!(
        h.selinux_ops.read_runtime_mode().unwrap(),
        SelinuxRuntimeMode::Permissive
    );
    let cfg_after_commit = h.override_ops.selinux_config_text().unwrap();
    assert_eq!(
        parse_selinux_config_mode(&cfg_after_commit).as_deref(),
        Some("permissive")
    );
    assert!(cfg_after_commit.contains("# SELinux configuration"));
    assert!(cfg_after_commit.contains("SELINUXTYPE=targeted"));

    let pub_doc: OverrideStoreDocument =
        serde_json::from_str(&fs::read_to_string(&h.public_overrides_path).unwrap()).unwrap();
    assert!(pub_doc.entries.contains_key("selinux"));

    let rolled = h
        .engine
        .rollback_transaction(":1.40", &committed.tx_id)
        .await
        .unwrap();
    assert_eq!(rolled.state, TransactionState::RolledBack);
    assert_eq!(
        h.selinux_ops.read_runtime_mode().unwrap(),
        SelinuxRuntimeMode::Enforcing
    );
    let cfg_after_rb = h.override_ops.selinux_config_text().unwrap();
    assert_eq!(
        parse_selinux_config_mode(&cfg_after_rb).as_deref(),
        Some("enforcing")
    );
    let pub_doc_after_rb: OverrideStoreDocument =
        serde_json::from_str(&fs::read_to_string(&h.public_overrides_path).unwrap()).unwrap();
    assert!(!pub_doc_after_rb.entries.contains_key("selinux"));
}

#[tokio::test]
async fn test_tx_raw_json_still_blocks_audit2allow_semodule_disabled_and_free_text_setenforce() {
    let h = setup_flexible_tx_harness();

    // 1. Typed SelinuxSetBoolean with "setenforce 0" or "selinux=disabled" MUST be blocked.
    for bad_req in [
        ActionRequest::SelinuxSetBoolean {
            name: "setenforce 0".to_string(),
            value: true,
            persistent: false,
        },
        ActionRequest::SelinuxSetBoolean {
            name: "selinux=disabled".to_string(),
            value: true,
            persistent: true,
        },
        ActionRequest::SelinuxSetBoolean {
            name: "audit2allow_rule".to_string(),
            value: true,
            persistent: false,
        },
        ActionRequest::SelinuxSetBoolean {
            name: "semodule_custom".to_string(),
            value: true,
            persistent: false,
        },
    ] {
        let err = h.engine.create_plan(":1.40", bad_req).await.unwrap_err();
        assert!(matches!(err, AkrepError::ForbiddenOperation(_)));
    }

    // 2. Raw JSON payloads carrying audit2allow / semodule / selinux=disabled / free-text setenforce
    // (including extra fields on selinux_set_mode and unicode escapes) MUST be blocked.
    for bad_json in [
        r#"{"kind":"selinux_set_boolean","name":"setenforce 0","value":true,"persistent":false}"#,
        r#"{"kind":"selinux_set_boolean","name":"selinux=disabled","value":true,"persistent":false}"#,
        r#"{"kind":"selinux_set_mode","mode":"permissive","duration":{"kind":"until_changed"},"note":"setenforce 0"}"#,
        r#"{"kind":"selinux_set_mode","mode":"permissive","duration":{"kind":"until_changed"},"note":"selinux=disabled"}"#,
        r#"{"kind":"apply_profile","profile_id":"balanced","extra":"audit\u0032allow"}"#,
        r#"{"kind":"apply_profile","profile_id":"balanced","extra":"semodule -i p.pp"}"#,
    ] {
        let err = h
            .engine
            .create_plan_from_json(":1.40", bad_json)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AkrepError::ForbiddenOperation(_)),
            "expected ForbiddenOperation for payload '{}', got {:?}",
            bad_json,
            err
        );
    }

    // DiagnoseSelinux continues rejecting "setenforce".
    let diag_err = h
        .engine
        .diagnose_selinux(":1.40", r#"{"action":"setenforce 0"}"#)
        .await
        .unwrap_err();
    assert!(matches!(diag_err, AkrepError::ForbiddenOperation(_)));

    // 3. Typed ActionRequest::SelinuxSetMode (both direct and via JSON) MUST succeed.
    let typed_req = ActionRequest::SelinuxSetMode {
        mode: SelinuxDesiredMode::Permissive,
        duration: OverrideDuration::Timed { minutes: 15 },
    };
    let typed_json = serde_json::to_string(&typed_req).unwrap();
    let json_plan = h
        .engine
        .create_plan_from_json(":1.40", &typed_json)
        .await
        .unwrap();
    assert!(matches!(
        json_plan.request,
        ActionRequest::SelinuxSetMode {
            mode: SelinuxDesiredMode::Permissive,
            duration: OverrideDuration::Timed { minutes: 15 },
        }
    ));
}

#[tokio::test]
async fn test_tx_apply_profile_auto_restores_permissive_selinux_and_paused_firewalld() {
    let h = setup_flexible_tx_harness();

    // Put SELinux in Permissive (Timed 15m) and firewalld in stopped (Timed 60m) via TransactionEngine.
    let se_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::SelinuxSetMode {
                mode: SelinuxDesiredMode::Permissive,
                duration: OverrideDuration::Timed { minutes: 15 },
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.40", &se_plan.plan_id, &se_plan.plan_sha256)
        .await
        .unwrap();

    let fw_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::FirewallSetState {
                enabled: false,
                duration: OverrideDuration::Timed { minutes: 60 },
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.40", &fw_plan.plan_id, &fw_plan.plan_sha256)
        .await
        .unwrap();

    assert_eq!(
        h.selinux_ops.read_runtime_mode().unwrap(),
        SelinuxRuntimeMode::Permissive
    );
    assert!(!h.firewall_backend.is_running().await.unwrap());
    assert!(h.override_ops.has_timer(OverrideSubsystem::Selinux));
    assert!(h.override_ops.has_timer(OverrideSubsystem::Firewalld));

    // Previewing "balanced" while firewalld is stopped and SELinux is Permissive MUST succeed
    // and surface explicit auto-restore diffs.
    let preview_json = h.engine.preview_profile(":1.40", "balanced").await.unwrap();
    let prof_plan: blaze_akrep_common::ActionPlan = serde_json::from_str(&preview_json).unwrap();
    assert!(
        prof_plan.diffs.iter().any(|d| d.target == "selinux.mode"),
        "preview diffs must include selinux.mode restore: {:?}",
        prof_plan.diffs
    );
    assert!(
        prof_plan
            .diffs
            .iter()
            .any(|d| d.target == "firewall.service_state"),
        "preview diffs must include firewall.service_state restore: {:?}",
        prof_plan.diffs
    );

    // Committing "balanced" MUST restore SELinux Enforcing, start firewalld, and clear overrides + timers.
    let committed = h
        .engine
        .commit_plan(":1.40", &prof_plan.plan_id, &prof_plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(committed.state, TransactionState::Committed);
    assert_eq!(
        h.selinux_ops.read_runtime_mode().unwrap(),
        SelinuxRuntimeMode::Enforcing
    );
    assert!(h.firewall_backend.is_running().await.unwrap());
    assert!(!h.override_ops.has_timer(OverrideSubsystem::Selinux));
    assert!(!h.override_ops.has_timer(OverrideSubsystem::Firewalld));
    assert!(h
        .overrides
        .active_override(OverrideSubsystem::Selinux)
        .unwrap()
        .is_none());
    assert!(h
        .overrides
        .active_override(OverrideSubsystem::Firewalld)
        .unwrap()
        .is_none());

    let priv_doc: OverrideStoreDocument =
        serde_json::from_str(&fs::read_to_string(h.state_dir.join("overrides.json")).unwrap())
            .unwrap();
    assert!(priv_doc.entries.is_empty());

    // Rolling back the profile transaction MUST cleanly restore Permissive SELinux & paused firewalld.
    let rolled = h
        .engine
        .rollback_transaction(":1.40", &committed.tx_id)
        .await
        .unwrap();
    assert_eq!(rolled.state, TransactionState::RolledBack);
    assert_eq!(
        h.selinux_ops.read_runtime_mode().unwrap(),
        SelinuxRuntimeMode::Permissive
    );
    assert!(!h.firewall_backend.is_running().await.unwrap());
    assert!(h
        .overrides
        .active_override(OverrideSubsystem::Selinux)
        .unwrap()
        .is_some());
    assert!(h
        .overrides
        .active_override(OverrideSubsystem::Firewalld)
        .unwrap()
        .is_some());

    // SELinux Disabled MUST STILL fail closed with BackendUnavailable!
    h.selinux_ops.set_runtime_mode(SelinuxRuntimeMode::Disabled);
    let disabled_err = h
        .engine
        .preview_profile(":1.40", "balanced")
        .await
        .unwrap_err();
    assert!(matches!(disabled_err, AkrepError::BackendUnavailable(_)));
}

#[tokio::test]
async fn test_tx_container_dir_rollback_succeeds_after_child_file_changes() {
    let h = setup_flexible_tx_harness();

    let workspace_backing = h.root_tmp.path().join("alice_aosp");
    fs::create_dir_all(&workspace_backing).unwrap();
    fs::write(workspace_backing.join("Android.bp"), b"initial\n").unwrap();

    let logical_dir = "/home/alice/aosp";
    h.override_ops.register_container_dir(
        logical_dir,
        workspace_backing.clone(),
        "unconfined_u:object_r:user_home_t:s0",
    );

    let plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::SelinuxLabelContainerDir {
                target_path: logical_dir.to_string(),
                recursive: true,
            },
        )
        .await
        .unwrap();
    let committed = h
        .engine
        .commit_plan(":1.40", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(committed.state, TransactionState::Committed);
    assert_eq!(committed.post_fingerprints.len(), 1);
    assert_eq!(
        h.override_ops
            .read_path_selinux_context(Path::new(logical_dir))
            .unwrap(),
        "unconfined_u:object_r:container_file_t:s0"
    );

    // Mutate and create child files inside the directory (simulating AOSP/container build activity).
    fs::write(
        workspace_backing.join("Android.bp"),
        b"modified during build\n",
    )
    .unwrap();
    fs::create_dir_all(workspace_backing.join("out/target/product")).unwrap();
    fs::write(
        workspace_backing.join("out/target/product/system.img"),
        vec![0xAB; 4096],
    )
    .unwrap();

    // Rollback MUST succeed with RolledBack (NEVER ExternalDriftDetected).
    let rolled = h
        .engine
        .rollback_transaction(":1.40", &committed.tx_id)
        .await
        .unwrap();
    assert_eq!(rolled.state, TransactionState::RolledBack);
    assert_eq!(
        h.override_ops
            .read_path_selinux_context(Path::new(logical_dir))
            .unwrap(),
        "unconfined_u:object_r:user_home_t:s0"
    );
    assert!(h
        .override_ops
        .recorded_ops()
        .iter()
        .any(|op| op.starts_with("restorecon")));
}

#[tokio::test]
async fn test_device_security_audit_populates_mok_nvidia_summary_and_degrades_gracefully() {
    let tmp = TempDir::new().unwrap();
    let sys = tmp.path().join("sys");
    let proc_dir = tmp.path().join("proc");
    let dev_dir = tmp.path().join("dev");
    let etc_dir = tmp.path().join("etc");
    let canary = tmp.path().join(".blaze_canary");
    fs::create_dir_all(sys.join("firmware/efi/efivars")).unwrap();
    fs::create_dir_all(sys.join("module/nvidia")).unwrap();
    fs::create_dir_all(&proc_dir).unwrap();
    fs::create_dir_all(&dev_dir).unwrap();
    fs::create_dir_all(&etc_dir).unwrap();

    let sb_var_path = sys.join(EFI_SECURE_BOOT_VAR_REL);
    fs::write(&sb_var_path, [0x06, 0x00, 0x00, 0x00, 0x01]).unwrap();

    // Unit parsing checks for mokutil outputs
    assert_eq!(parse_mokutil_sb_state("SecureBoot enabled\n"), Some(true));
    assert_eq!(parse_mokutil_sb_state("SecureBoot disabled\n"), Some(false));
    assert_eq!(
        parse_mokutil_sb_state("EFI variables are not supported on this system\n"),
        None
    );

    let mok_sample = "\
[key 1]
SHA1 Fingerprint: aa:bb:cc:dd
Certificate:
    Data:
[key 2]
SHA1 Fingerprint: 11:22:33:44
Certificate:
    Data:
";
    assert_eq!(parse_mokutil_enrolled_count(mok_sample), Some(2));
    assert_eq!(
        parse_mokutil_enrolled_count("MokListRT is empty\n"),
        Some(0)
    );

    let sb = inspect_secure_boot_at(&sys);
    let mok = inspect_driver_mok_at(&sys, &sb, None, None);
    assert_eq!(mok.secure_boot_enabled, Some(true));
    assert!(mok.nvidia_module_loaded);
    assert!(!mok.akmod_nvidia_installed);
    assert!(!mok.unsigned_risk);

    // Full LinuxDeviceSecurityOps inspect MUST succeed without panicking even if mokutil is absent
    let linux_ops = LinuxDeviceSecurityOps::with_roots(sys, proc_dir, dev_dir, etc_dir, canary);
    let status = DeviceSecurityAdapter::new(Arc::new(linux_ops))
        .inspect_device_security()
        .await
        .unwrap();
    let driver_mok = status
        .driver_mok
        .as_ref()
        .expect("driver_mok must be populated");
    assert!(driver_mok.nvidia_module_loaded);

    // Backward compatibility: legacy JSON without driver_mok still deserializes to None.
    let mut val = serde_json::to_value(&status).unwrap();
    val.as_object_mut().unwrap().remove("driver_mok");
    let legacy_decoded: DeviceSecurityStatus = serde_json::from_value(val).unwrap();
    assert!(legacy_decoded.driver_mok.is_none());
}

#[tokio::test]
async fn test_tx_oomd_nofile_compat_package_and_public_network_auto_restore() {
    let h = setup_flexible_tx_harness();

    // 1. OomdSetState pause + rollback via TransactionEngine
    let oomd_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::OomdSetState {
                enabled: false,
                duration: OverrideDuration::UntilReboot,
            },
        )
        .await
        .unwrap();
    let oomd_tx = h
        .engine
        .commit_plan(":1.40", &oomd_plan.plan_id, &oomd_plan.plan_sha256)
        .await
        .unwrap();
    assert!(!h.override_ops.is_unit_active(SYSTEMD_OOMD_UNIT).unwrap());
    let oomd_rb = h
        .engine
        .rollback_transaction(":1.40", &oomd_tx.tx_id)
        .await
        .unwrap();
    assert_eq!(oomd_rb.state, TransactionState::RolledBack);
    assert!(h.override_ops.is_unit_active(SYSTEMD_OOMD_UNIT).unwrap());

    // 2. DevLimitsSetNofile enable + rollback via TransactionEngine
    let nofile_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::DevLimitsSetNofile {
                enabled: true,
                soft_limit: 65_536,
                hard_limit: 524_288,
            },
        )
        .await
        .unwrap();
    let nofile_tx = h
        .engine
        .commit_plan(":1.40", &nofile_plan.plan_id, &nofile_plan.plan_sha256)
        .await
        .unwrap();
    let (pam_after, sysd_after) = h.override_ops.read_nofile_dropins().unwrap();
    assert!(pam_after.unwrap().contains("* hard nofile 524288"));
    assert!(sysd_after
        .unwrap()
        .contains("DefaultLimitNOFILE=65536:524288"));
    let nofile_rb = h
        .engine
        .rollback_transaction(":1.40", &nofile_tx.tx_id)
        .await
        .unwrap();
    assert_eq!(nofile_rb.state, TransactionState::RolledBack);
    assert_eq!(h.override_ops.read_nofile_dropins().unwrap(), (None, None));

    // 3. InstallCompatibilityPackage apply + rollback via TransactionEngine
    let pkg_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::InstallCompatibilityPackage {
                package_group: CompatPackageGroup::AndroidUdevRules,
            },
        )
        .await
        .unwrap();
    let pkg_tx = h
        .engine
        .commit_plan(":1.40", &pkg_plan.plan_id, &pkg_plan.plan_sha256)
        .await
        .unwrap();
    assert!(h
        .override_ops
        .is_packages_installed(&["android-udev-rules"])
        .unwrap());
    let pkg_rb = h
        .engine
        .rollback_transaction(":1.40", &pkg_tx.tx_id)
        .await
        .unwrap();
    assert_eq!(pkg_rb.state, TransactionState::RolledBack);
    assert!(!h
        .override_ops
        .is_packages_installed(&["android-udev-rules"])
        .unwrap());

    // 4. public-network profile preview/commit/rollback while firewalld is stopped
    let fw_pause_plan = h
        .engine
        .create_plan(
            ":1.40",
            ActionRequest::FirewallSetState {
                enabled: false,
                duration: OverrideDuration::UntilChanged,
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.40", &fw_pause_plan.plan_id, &fw_pause_plan.plan_sha256)
        .await
        .unwrap();
    assert!(!h.firewall_backend.is_running().await.unwrap());

    let pub_preview = h
        .engine
        .preview_profile(":1.40", "public-network")
        .await
        .unwrap();
    let pub_plan: blaze_akrep_common::ActionPlan = serde_json::from_str(&pub_preview).unwrap();
    assert!(pub_plan
        .diffs
        .iter()
        .any(|d| d.target == "firewall.service_state"));
    assert!(pub_plan
        .diffs
        .iter()
        .any(|d| d.target == "firewall.interfaces"));

    let pub_tx = h
        .engine
        .commit_plan(":1.40", &pub_plan.plan_id, &pub_plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(pub_tx.state, TransactionState::Committed);
    assert!(h.firewall_backend.is_running().await.unwrap());
    let pub_zone = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(pub_zone.runtime_interfaces.contains(&"wlan0".to_string()));
    assert!(!pub_zone.runtime_services.contains(&"mdns".to_string()));

    let pub_rb = h
        .engine
        .rollback_transaction(":1.40", &pub_tx.tx_id)
        .await
        .unwrap();
    assert_eq!(pub_rb.state, TransactionState::RolledBack);
    assert!(!h.firewall_backend.is_running().await.unwrap());
}
