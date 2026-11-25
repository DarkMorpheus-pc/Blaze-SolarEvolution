use blaze_akrep_common::{
    build_cidr_port_rich_rule, build_cidr_service_rich_rule, ActionRequest, AkrepError,
    FirewallProto, FirewallStateSnapshot, RiskLevel, RulePersistence, SelinuxDiagnoseResponse,
    SelinuxRuntimeMode, SuggestedSelinuxFix, TransactionState, POLKIT_FIREWALL_MANAGE,
    POLKIT_SELINUX_DIAGNOSE, POLKIT_SELINUX_MANAGE,
};
use blaze_akrep_privd::{
    curated_boolean_catalog, lookup_boolean_meta, CompositeMutationBackend, FirewallAdapter,
    FirewallBackend, MockAuthorizer, MockFirewallBackend, MockSelinuxOps, SelinuxAdapter,
    SelinuxSysOps, TransactionEngine,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use tempfile::TempDir;

struct Sprint1Harness {
    _dir: TempDir,
    auth: Arc<MockAuthorizer>,
    selinux_ops: Arc<MockSelinuxOps>,
    firewall_backend: Arc<MockFirewallBackend>,
    engine: TransactionEngine,
}

fn setup_sprint1_harness() -> Sprint1Harness {
    let dir = TempDir::new().expect("tempdir");
    let auth = Arc::new(MockAuthorizer::new());
    auth.allow_all_for_bus(":1.20", 1000);

    let selinux_ops = Arc::new(MockSelinuxOps::new_fedora_enforcing());
    let firewall_backend = Arc::new(MockFirewallBackend::new_fedora_workstation());

    let selinux_adapter = Arc::new(SelinuxAdapter::new(selinux_ops.clone()));
    let firewall_adapter = Arc::new(FirewallAdapter::new(firewall_backend.clone()));
    let composite = Arc::new(CompositeMutationBackend::new(
        selinux_adapter,
        firewall_adapter,
    ));

    let engine = TransactionEngine::new(dir.path().to_path_buf(), auth.clone(), composite)
        .expect("engine init");

    Sprint1Harness {
        _dir: dir,
        auth,
        selinux_ops,
        firewall_backend,
        engine,
    }
}

// ============================================================================
// SELinux Acceptance Tests (SEL-01 .. SEL-10)
// ============================================================================

#[tokio::test]
async fn sel_01_read_enforcing_mode() {
    let h = setup_sprint1_harness();

    // 1. Default Enforcing mode + targeted policy
    let raw_enforcing = h.engine.diagnose_selinux(":1.20", "{}").await.unwrap();
    let resp1: SelinuxDiagnoseResponse = serde_json::from_str(&raw_enforcing).unwrap();
    assert_eq!(resp1.runtime_mode, SelinuxRuntimeMode::Enforcing);
    assert_eq!(resp1.configured_mode.as_deref(), Some("enforcing"));
    assert_eq!(resp1.policy_type.as_deref(), Some("targeted"));

    // 2. Permissive mode
    h.selinux_ops
        .set_runtime_mode(SelinuxRuntimeMode::Permissive);
    h.selinux_ops
        .set_config(Some("permissive"), Some("targeted"));
    let raw_permissive = h
        .engine
        .diagnose_selinux(":1.20", r#"{"include_booleans":false,"include_avc":false}"#)
        .await
        .unwrap();
    let resp2: SelinuxDiagnoseResponse = serde_json::from_str(&raw_permissive).unwrap();
    assert_eq!(resp2.runtime_mode, SelinuxRuntimeMode::Permissive);
    assert_eq!(resp2.configured_mode.as_deref(), Some("permissive"));

    // 3. Disabled mode reports Disabled cleanly and blocks boolean mutations
    h.selinux_ops.set_runtime_mode(SelinuxRuntimeMode::Disabled);
    let raw_disabled = h.engine.diagnose_selinux(":1.20", "{}").await.unwrap();
    let resp3: SelinuxDiagnoseResponse = serde_json::from_str(&raw_disabled).unwrap();
    assert_eq!(resp3.runtime_mode, SelinuxRuntimeMode::Disabled);
    assert!(resp3.booleans.is_empty());

    let err = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::SelinuxSetBoolean {
                name: "virt_use_nfs".to_string(),
                value: true,
                persistent: false,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, AkrepError::BackendUnavailable(_)));
}

#[tokio::test]
async fn sel_02_boolean_inventory_typed_and_cataloged() {
    let h = setup_sprint1_harness();
    let catalog = curated_boolean_catalog();
    assert!(
        catalog.len() >= 25,
        "curated SELinux boolean catalog must contain >= 25 entries, got {}",
        catalog.len()
    );

    // Check key mandatory catalog entries and their risk levels
    for (expected_name, expected_risk) in [
        ("container_manage_cgroup", RiskLevel::Low),
        ("container_use_devices", RiskLevel::Medium),
        ("virt_use_kvm", RiskLevel::Low),
        ("virt_use_usb", RiskLevel::Medium),
        ("httpd_can_network_connect", RiskLevel::Medium),
        ("samba_enable_home_dirs", RiskLevel::Medium),
        ("nis_enabled", RiskLevel::Medium),
        ("selinuxuser_execheap", RiskLevel::Critical),
        ("selinuxuser_execmod", RiskLevel::High),
        ("selinuxuser_execstack", RiskLevel::Critical),
        ("mmap_low_allowed", RiskLevel::Critical),
        ("deny_ptrace", RiskLevel::Medium),
    ] {
        let (is_cat, meta) = lookup_boolean_meta(expected_name);
        assert!(is_cat, "expected '{}' in curated catalog", expected_name);
        assert_eq!(
            meta.risk, expected_risk,
            "risk mismatch for {}",
            expected_name
        );
        assert!(!meta.summary_tr.is_empty());
        assert!(!meta.summary_en.is_empty());
    }

    // Seed an uncataloged policy boolean and verify it receives Medium risk + generic bilingual description + preview warning
    h.selinux_ops
        .insert_boolean("custom_vendor_daemon_can_network", false, false);

    let raw = h
        .engine
        .diagnose_selinux(
            ":1.20",
            r#"{"boolean_filter":"custom_vendor_daemon_can_network","include_avc":false}"#,
        )
        .await
        .unwrap();
    let resp: SelinuxDiagnoseResponse = serde_json::from_str(&raw).unwrap();
    assert_eq!(resp.booleans.len(), 1);
    let custom_entry = &resp.booleans[0];
    assert!(!custom_entry.cataloged);
    assert_eq!(custom_entry.risk, RiskLevel::Medium);
    assert!(custom_entry
        .summary_en
        .contains("custom_vendor_daemon_can_network"));
    assert!(custom_entry
        .summary_tr
        .contains("custom_vendor_daemon_can_network"));

    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::SelinuxSetBoolean {
                name: "custom_vendor_daemon_can_network".to_string(),
                value: true,
                persistent: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(plan.risk, RiskLevel::Medium);
    assert!(!plan.warnings_tr.is_empty());
    assert!(!plan.warnings_en.is_empty());
}

#[tokio::test]
async fn sel_03_runtime_boolean_toggle() {
    let h = setup_sprint1_harness();

    let before = h
        .selinux_ops
        .read_boolean("httpd_can_network_connect")
        .unwrap()
        .unwrap();
    assert!(!before.current);
    assert!(!before.persistent);

    let req = ActionRequest::SelinuxSetBoolean {
        name: "httpd_can_network_connect".to_string(),
        value: true,
        persistent: false,
    };
    let plan = h.engine.create_plan(":1.20", req).await.unwrap();
    assert_eq!(plan.polkit_action, POLKIT_SELINUX_MANAGE);
    assert_eq!(plan.pre_fingerprints.len(), 1);

    let rec = h
        .engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);
    assert_eq!(rec.post_fingerprints.len(), 1);
    assert_ne!(
        rec.pre_fingerprints[0].sha256,
        rec.post_fingerprints[0].sha256
    );

    let after = h
        .selinux_ops
        .read_boolean("httpd_can_network_connect")
        .unwrap()
        .unwrap();
    assert!(after.current);
    assert!(
        !after.persistent,
        "runtime-only toggle must not mutate persistent policy"
    );

    let cmds = h.selinux_ops.recorded_commands();
    assert_eq!(
        cmds,
        vec![vec![
            "/usr/sbin/setsebool".to_string(),
            "httpd_can_network_connect".to_string(),
            "on".to_string()
        ]]
    );

    let snap_file = h
        .engine
        .snapshots_dir()
        .join(&rec.tx_id)
        .join("selinux_bool.json");
    assert!(snap_file.is_file());
    assert_eq!(
        fs::metadata(&snap_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[tokio::test]
async fn sel_04_runtime_boolean_reverts_on_simulated_reboot() {
    let h = setup_sprint1_harness();

    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::SelinuxSetBoolean {
                name: "container_manage_cgroup".to_string(),
                value: true,
                persistent: false,
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();

    assert!(
        h.selinux_ops
            .read_boolean("container_manage_cgroup")
            .unwrap()
            .unwrap()
            .current
    );

    h.selinux_ops.simulate_reboot();

    let post_reboot = h
        .selinux_ops
        .read_boolean("container_manage_cgroup")
        .unwrap()
        .unwrap();
    assert!(
        !post_reboot.current,
        "runtime-only boolean must revert to persistent default after reboot"
    );
    assert!(!post_reboot.persistent);
}

#[tokio::test]
async fn sel_05_persistent_boolean_survives_simulated_reboot() {
    let h = setup_sprint1_harness();

    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::SelinuxSetBoolean {
                name: "virt_use_usb".to_string(),
                value: true,
                persistent: true,
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    let cmds = h.selinux_ops.recorded_commands();
    assert_eq!(
        cmds,
        vec![vec![
            "/usr/sbin/setsebool".to_string(),
            "-P".to_string(),
            "virt_use_usb".to_string(),
            "on".to_string()
        ]]
    );

    h.selinux_ops.simulate_reboot();

    let post_reboot = h.selinux_ops.read_boolean("virt_use_usb").unwrap().unwrap();
    assert!(post_reboot.current);
    assert!(post_reboot.persistent);
}

#[tokio::test]
async fn sel_06_boolean_transaction_rollback() {
    let h = setup_sprint1_harness();

    // 1. Commit persistent boolean change and manually roll back
    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::SelinuxSetBoolean {
                name: "samba_enable_home_dirs".to_string(),
                value: true,
                persistent: true,
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert!(
        h.selinux_ops
            .read_boolean("samba_enable_home_dirs")
            .unwrap()
            .unwrap()
            .persistent
    );

    let rb = h
        .engine
        .rollback_transaction(":1.20", &rec.tx_id)
        .await
        .unwrap();
    assert_eq!(rb.state, TransactionState::RolledBack);
    let restored = h
        .selinux_ops
        .read_boolean("samba_enable_home_dirs")
        .unwrap()
        .unwrap();
    assert!(!restored.current);
    assert!(!restored.persistent);

    // 2. Also verify SelinuxRestorecon commit + chcon rollback
    let rc_plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::SelinuxRestorecon {
                target_path: "/srv/www/index.html".to_string(),
                recursive: false,
            },
        )
        .await
        .unwrap();
    let rc_rec = h
        .engine
        .commit_plan(":1.20", &rc_plan.plan_id, &rc_plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rc_rec.state, TransactionState::Committed);
    assert_eq!(
        h.selinux_ops
            .read_file_contexts(Path::new("/srv/www/index.html"), false)
            .unwrap()[0]
            .context,
        "system_u:object_r:httpd_sys_content_t:s0"
    );

    let rc_rb = h
        .engine
        .rollback_transaction(":1.20", &rc_rec.tx_id)
        .await
        .unwrap();
    assert_eq!(rc_rb.state, TransactionState::RolledBack);
    assert_eq!(
        h.selinux_ops
            .read_file_contexts(Path::new("/srv/www/index.html"), false)
            .unwrap()[0]
            .context,
        "unconfined_u:object_r:user_tmp_t:s0"
    );

    // 3. External drift on boolean after commit blocks blind rollback
    let p_drift = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::SelinuxSetBoolean {
                name: "virt_use_nfs".to_string(),
                value: true,
                persistent: false,
            },
        )
        .await
        .unwrap();
    let rec_drift = h
        .engine
        .commit_plan(":1.20", &p_drift.plan_id, &p_drift.plan_sha256)
        .await
        .unwrap();
    h.selinux_ops
        .set_external_boolean_drift("virt_use_nfs", true, true);
    let drift_err = h
        .engine
        .rollback_transaction(":1.20", &rec_drift.tx_id)
        .await
        .unwrap_err();
    assert!(matches!(drift_err, AkrepError::ExternalDriftDetected(_)));
}

#[tokio::test]
async fn sel_07_avc_dual_source_grouping_and_dedup() {
    let h = setup_sprint1_harness();

    // Seed:
    // Event A (duplicate across audit.log and journalctl -> must count as 1, file context mismatch -> Restorecon)
    // Event B (audit.log + second distinct serial in journalctl -> must count as 2, safe boolean -> SetBoolean httpd_can_network_connect)
    // Event C (journalctl only, critical execheap -> ManualPolicyReview, NEVER audit2allow)
    let audit_lines = vec![
        r#"type=AVC msg=audit(1732550000.100:201): avc:  denied  { read open } for  pid=1100 comm="nginx" path="/srv/www/index.html" dev="dm-0" ino=42 scontext=system_u:system_r:httpd_t:s0 tcontext=unconfined_u:object_r:user_tmp_t:s0 tclass=file permissive=0"#.to_string(),
        r#"type=AVC msg=audit(1732550010.200:202): avc:  denied  { name_connect } for  pid=1101 comm="httpd" dest=8080 scontext=system_u:system_r:httpd_t:s0 tcontext=system_u:object_r:http_cache_port_t:s0 tclass=tcp_socket permissive=0"#.to_string(),
    ];

    let journal_lines = vec![
        // Exact duplicate of audit(1732550000.100:201)
        serde_json::json!({
            "_AUDIT_ID": "201",
            "__REALTIME_TIMESTAMP": "1732550000100000",
            "MESSAGE": r#"AVC avc:  denied  { open read } for  pid=1100 comm="nginx" path="/srv/www/index.html" dev="dm-0" ino=42 scontext=system_u:system_r:httpd_t:s0 tcontext=unconfined_u:object_r:user_tmp_t:s0 tclass=file permissive=0 msg=audit(1732550000.100:201):"#
        })
        .to_string(),
        // Distinct second occurrence of httpd name_connect (serial 203)
        serde_json::json!({
            "_AUDIT_ID": "203",
            "__REALTIME_TIMESTAMP": "1732550020000000",
            "MESSAGE": r#"type=AVC msg=audit(1732550020.000:203): avc:  denied  { name_connect } for  pid=1102 comm="httpd" dest=8080 scontext=system_u:system_r:httpd_t:s0 tcontext=system_u:object_r:http_cache_port_t:s0 tclass=tcp_socket permissive=0"#
        })
        .to_string(),
        // Journal-only critical memory protection denial (serial 204)
        serde_json::json!({
            "_AUDIT_ID": "204",
            "__REALTIME_TIMESTAMP": "1732550030000000",
            "MESSAGE": r#"type=AVC msg=audit(1732550030.000:204): avc:  denied  { execheap } for  pid=2200 comm="legacy_bin" scontext=unconfined_u:unconfined_r:unconfined_t:s0 tcontext=unconfined_u:unconfined_r:unconfined_t:s0 tclass=process permissive=0"#
        })
        .to_string(),
    ];

    h.selinux_ops.set_avc_sources(audit_lines, journal_lines);

    let raw = h
        .engine
        .diagnose_selinux(":1.20", r#"{"include_booleans":false,"include_avc":true}"#)
        .await
        .unwrap();
    let resp: SelinuxDiagnoseResponse = serde_json::from_str(&raw).unwrap();
    assert_eq!(resp.avc_groups.len(), 3);

    // Priority 1: nginx /srv/www/index.html context mismatch -> Restorecon, deduplicated count == 1
    let grp_nginx = resp
        .avc_groups
        .iter()
        .find(|g| g.app_comm == "nginx")
        .expect("nginx avc group");
    assert_eq!(
        grp_nginx.count, 1,
        "duplicate audit+journal event must be counted once"
    );
    assert_eq!(
        grp_nginx.permissions,
        vec!["open".to_string(), "read".to_string()]
    );
    match &grp_nginx.suggested_remediation_kind {
        SuggestedSelinuxFix::Restorecon {
            target_path,
            recursive,
            expected_context,
        } => {
            assert_eq!(target_path, "/srv/www/index.html");
            assert!(!recursive);
            assert_eq!(
                expected_context.as_deref(),
                Some("system_u:object_r:httpd_sys_content_t:s0")
            );
        }
        other => panic!("expected Restorecon remediation, got {:?}", other),
    }

    // Priority 2: httpd tcp_socket name_connect -> SetBoolean(httpd_can_network_connect), count == 2
    let grp_httpd = resp
        .avc_groups
        .iter()
        .find(|g| g.app_comm == "httpd")
        .expect("httpd avc group");
    assert_eq!(grp_httpd.count, 2);
    match &grp_httpd.suggested_remediation_kind {
        SuggestedSelinuxFix::SetBoolean {
            boolean_name,
            recommended_value,
            risk,
        } => {
            assert_eq!(boolean_name, "httpd_can_network_connect");
            assert!(*recommended_value);
            assert_eq!(*risk, RiskLevel::Medium);
        }
        other => panic!("expected SetBoolean remediation, got {:?}", other),
    }

    // Priority 3: legacy_bin execheap -> Critical severity, ManualPolicyReview (never auto-enable execheap or audit2allow)
    let grp_execheap = resp
        .avc_groups
        .iter()
        .find(|g| g.app_comm == "legacy_bin")
        .expect("legacy_bin avc group");
    assert_eq!(grp_execheap.severity, RiskLevel::Critical);
    match &grp_execheap.suggested_remediation_kind {
        SuggestedSelinuxFix::ManualPolicyReview {
            guidance_tr,
            guidance_en,
        } => {
            assert!(!guidance_tr.to_ascii_lowercase().contains("audit2allow"));
            assert!(!guidance_en.to_ascii_lowercase().contains("audit2allow"));
        }
        other => panic!("expected ManualPolicyReview, got {:?}", other),
    }
}

#[tokio::test]
async fn sel_08_unauthorized_boolean_change_denied_by_polkit() {
    let h = setup_sprint1_harness();
    h.auth.deny_all_for_bus(":1.77", 1005);

    // Denied on SELinux boolean plan creation
    let bool_err = h
        .engine
        .create_plan(
            ":1.77",
            ActionRequest::SelinuxSetBoolean {
                name: "virt_use_usb".to_string(),
                value: true,
                persistent: false,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(bool_err, AkrepError::PolkitDenied(_)));
    assert!(h.selinux_ops.recorded_commands().is_empty());

    // Denied on DiagnoseSelinux (POLKIT_SELINUX_DIAGNOSE)
    let diag_err = h.engine.diagnose_selinux(":1.77", "{}").await.unwrap_err();
    assert!(matches!(diag_err, AkrepError::PolkitDenied(_)));

    // Granting ONLY POLKIT_SELINUX_DIAGNOSE still denies POLKIT_SELINUX_MANAGE mutations
    h.auth
        .allow_actions_for_bus(":1.78", 1006, [POLKIT_SELINUX_DIAGNOSE]);
    assert!(h.engine.diagnose_selinux(":1.78", "{}").await.is_ok());
    let mut_err = h
        .engine
        .create_plan(
            ":1.78",
            ActionRequest::SelinuxSetBoolean {
                name: "virt_use_usb".to_string(),
                value: true,
                persistent: false,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(mut_err, AkrepError::PolkitDenied(_)));
}

#[tokio::test]
async fn sel_09_nonexistent_or_malformed_boolean_rejected() {
    let h = setup_sprint1_harness();

    let malformed_names = [
        "",
        "a",
        "UpperCaseBool",
        "1starts_with_digit",
        "hyphen-not-allowed",
        "dot.not.allowed",
        "virt_use_usb;id",
        &"x".repeat(97),
    ];

    for bad in malformed_names {
        let err = h
            .engine
            .create_plan(
                ":1.20",
                ActionRequest::SelinuxSetBoolean {
                    name: bad.to_string(),
                    value: true,
                    persistent: false,
                },
            )
            .await
            .unwrap_err();
        assert!(
            matches!(err, AkrepError::InvalidInput(_)),
            "expected InvalidInput for '{}', got {:?}",
            bad,
            err
        );
    }

    // Syntactically valid name that does not exist in /sys/fs/selinux/booleans/<name>
    let nonexistent_err = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::SelinuxSetBoolean {
                name: "nonexistent_fedora_boolean_xyz".to_string(),
                value: true,
                persistent: false,
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(nonexistent_err, AkrepError::InvalidInput(_)));
    assert!(h.selinux_ops.recorded_commands().is_empty());
}

#[tokio::test]
async fn sel_10_audit2allow_and_global_disable_forbidden() {
    let h = setup_sprint1_harness();

    let forbidden_requests = vec![
        ActionRequest::SelinuxSetBoolean {
            name: "audit2allow_auto".to_string(),
            value: true,
            persistent: false,
        },
        ActionRequest::SelinuxSetBoolean {
            name: "SELINUX=disabled".to_string(),
            value: true,
            persistent: true,
        },
        ActionRequest::SelinuxSetBoolean {
            name: "setenforce_0".to_string(),
            value: true,
            persistent: false,
        },
        ActionRequest::SelinuxSetBoolean {
            name: "semodule_install".to_string(),
            value: true,
            persistent: false,
        },
        ActionRequest::SelinuxRestorecon {
            target_path: "/proc/1/attr/current".to_string(),
            recursive: false,
        },
        ActionRequest::SelinuxRestorecon {
            target_path: "/sys/fs/selinux/enforce".to_string(),
            recursive: false,
        },
        ActionRequest::SelinuxRestorecon {
            target_path: "/dev/null".to_string(),
            recursive: false,
        },
        ActionRequest::SelinuxRestorecon {
            target_path: "/".to_string(),
            recursive: true,
        },
        ActionRequest::SelinuxRestorecon {
            target_path: "/srv/../etc/shadow".to_string(),
            recursive: false,
        },
    ];

    for req in forbidden_requests {
        let err = h.engine.create_plan(":1.20", req).await.unwrap_err();
        assert!(matches!(err, AkrepError::ForbiddenOperation(_)));
    }

    // DiagnoseSelinux queries attempting audit2allow / semodule / setenforce MUST return ForbiddenOperation
    for bad_query in [
        r#"{"action":"audit2allow"}"#,
        r#"{"inspect_path":"/srv/www","note":"semodule -i custom.pp"}"#,
        r#"{"action":"setenforce 0"}"#,
        r#"{"inspect_path":"/proc/self"}"#,
    ] {
        let err = h
            .engine
            .diagnose_selinux(":1.20", bad_query)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AkrepError::ForbiddenOperation(_)),
            "expected ForbiddenOperation for query '{}', got {:?}",
            bad_query,
            err
        );
    }
}

// ============================================================================
// Firewalld Acceptance Tests (FW-01 .. FW-12)
// ============================================================================

#[tokio::test]
async fn fw_01_read_zone_list() {
    let h = setup_sprint1_harness();

    let raw = h.engine.inspect_firewall(":1.20", "").await.unwrap();
    let snap: FirewallStateSnapshot = serde_json::from_str(&raw).unwrap();
    assert!(snap.running);
    assert_eq!(snap.default_zone, "FedoraWorkstation");
    for expected_zone in [
        "FedoraWorkstation",
        "public",
        "home",
        "work",
        "drop",
        "block",
        "trusted",
    ] {
        assert!(
            snap.zones.contains(&expected_zone.to_string()),
            "missing zone {}",
            expected_zone
        );
    }
    assert_eq!(snap.zone_details.len(), snap.zones.len());

    // Single-zone filtered query
    let filtered_raw = h.engine.inspect_firewall(":1.20", "public").await.unwrap();
    let filtered: FirewallStateSnapshot = serde_json::from_str(&filtered_raw).unwrap();
    assert_eq!(filtered.zone_details.len(), 1);
    assert_eq!(filtered.zone_details[0].name, "public");
}

#[tokio::test]
async fn fw_02_active_zone_interface_mapping() {
    let h = setup_sprint1_harness();

    let initial: FirewallStateSnapshot =
        serde_json::from_str(&h.engine.inspect_firewall(":1.20", "").await.unwrap()).unwrap();
    assert_eq!(
        initial.active_zones.get("wlan0").map(String::as_str),
        Some("FedoraWorkstation")
    );
    assert_eq!(
        initial.active_zones.get("eth0").map(String::as_str),
        Some("public")
    );

    // Move eth0 from public -> work persistently
    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallBindInterface {
                zone: "work".to_string(),
                interface: "eth0".to_string(),
                permanent: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(plan.polkit_action, POLKIT_FIREWALL_MANAGE);
    assert_eq!(
        plan.pre_fingerprints.len(),
        2,
        "both source ('public') and destination ('work') zones must be fingerprinted"
    );

    let rec = h
        .engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    let updated: FirewallStateSnapshot =
        serde_json::from_str(&h.engine.inspect_firewall(":1.20", "").await.unwrap()).unwrap();
    assert_eq!(
        updated.active_zones.get("eth0").map(String::as_str),
        Some("work")
    );
}

#[tokio::test]
async fn fw_03_open_tcp_port_runtime() {
    let h = setup_sprint1_harness();

    // 1. Standard runtime TCP port
    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "8443".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::Runtime,
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    let pub_state = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(pub_state
        .runtime_ports
        .contains(&("8443".to_string(), "tcp".to_string())));
    assert!(!pub_state
        .permanent_ports
        .contains(&("8443".to_string(), "tcp".to_string())));

    // 2. CIDR-restricted TCP port (Minecraft 25565/tcp restricted to LAN 192.168.1.0/24)
    let cidr_plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "25565".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::Runtime,
                source_cidr: Some("192.168.1.0/24".to_string()),
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.20", &cidr_plan.plan_id, &cidr_plan.plan_sha256)
        .await
        .unwrap();

    let expected_rich =
        build_cidr_port_rich_rule("192.168.1.0/24", "25565", FirewallProto::Tcp).unwrap();
    let pub_after = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(pub_after.runtime_rich_rules.contains(&expected_rich));
    assert!(!pub_after.permanent_rich_rules.contains(&expected_rich));
}

#[tokio::test]
async fn fw_04_open_udp_port_runtime() {
    let h = setup_sprint1_harness();

    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "51820".to_string(),
                proto: FirewallProto::Udp,
                mode: RulePersistence::Runtime,
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    let st = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(st
        .runtime_ports
        .contains(&("51820".to_string(), "udp".to_string())));
    assert!(!st
        .permanent_ports
        .contains(&("51820".to_string(), "udp".to_string())));
}

#[tokio::test]
async fn fw_05_timed_port_expires() {
    let h = setup_sprint1_harness();

    // Out-of-bounds TimedRuntime durations (0 or > 86400) must be rejected
    for bad_secs in [0u32, 86_401u32] {
        let err = h
            .engine
            .create_plan(
                ":1.20",
                ActionRequest::FirewallAddPort {
                    zone: "public".to_string(),
                    port: "8080".to_string(),
                    proto: FirewallProto::Tcp,
                    mode: RulePersistence::TimedRuntime { seconds: bad_secs },
                    source_cidr: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AkrepError::InvalidInput(_)));
    }

    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "8080".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::TimedRuntime { seconds: 60 },
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();

    // Active before timeout
    h.firewall_backend.advance_time(30);
    assert!(h
        .firewall_backend
        .get_zone_state("public")
        .await
        .unwrap()
        .runtime_ports
        .contains(&("8080".to_string(), "tcp".to_string())));

    // Automatically expired once timeout elapses
    h.firewall_backend.advance_time(31);
    let expired_state = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(!expired_state
        .runtime_ports
        .contains(&("8080".to_string(), "tcp".to_string())));
    assert!(!expired_state
        .permanent_ports
        .contains(&("8080".to_string(), "tcp".to_string())));
}

#[tokio::test]
async fn fw_06_service_allow_in_zone() {
    let h = setup_sprint1_harness();

    // 1. Standard service allow in 'home'
    let p1 = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddService {
                zone: "home".to_string(),
                service: "kdeconnect".to_string(),
                mode: RulePersistence::Runtime,
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.20", &p1.plan_id, &p1.plan_sha256)
        .await
        .unwrap();
    assert!(h
        .firewall_backend
        .get_zone_state("home")
        .await
        .unwrap()
        .runtime_services
        .contains(&"kdeconnect".to_string()));

    // 2. CIDR-restricted permanent service in 'work'
    let p2 = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddService {
                zone: "work".to_string(),
                service: "cockpit".to_string(),
                mode: RulePersistence::Permanent,
                source_cidr: Some("10.20.0.0/16".to_string()),
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.20", &p2.plan_id, &p2.plan_sha256)
        .await
        .unwrap();

    let expected_rule = build_cidr_service_rich_rule("10.20.0.0/16", "cockpit").unwrap();
    let work_st = h.firewall_backend.get_zone_state("work").await.unwrap();
    assert!(work_st.runtime_rich_rules.contains(&expected_rule));
    assert!(work_st.permanent_rich_rules.contains(&expected_rule));

    // 3. Remove service from 'home'
    let p3 = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallRemoveService {
                zone: "home".to_string(),
                service: "kdeconnect".to_string(),
                permanent: false,
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.20", &p3.plan_id, &p3.plan_sha256)
        .await
        .unwrap();
    assert!(!h
        .firewall_backend
        .get_zone_state("home")
        .await
        .unwrap()
        .runtime_services
        .contains(&"kdeconnect".to_string()));
}

#[tokio::test]
async fn fw_07_permanent_rule_survives_reload_and_reboot() {
    let h = setup_sprint1_harness();

    // Permanent port 443/tcp (must apply immediately to BOTH runtime and permanent)
    let perm_plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "443".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::Permanent,
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(perm_plan.risk, RiskLevel::High);
    assert!(!perm_plan.warnings_tr.is_empty());
    h.engine
        .commit_plan(":1.20", &perm_plan.plan_id, &perm_plan.plan_sha256)
        .await
        .unwrap();

    // Runtime-only port 9090/tcp
    let rt_plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "9090".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::Runtime,
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    h.engine
        .commit_plan(":1.20", &rt_plan.plan_id, &rt_plan.plan_sha256)
        .await
        .unwrap();

    let immediate = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(immediate
        .runtime_ports
        .contains(&("443".to_string(), "tcp".to_string())));
    assert!(immediate
        .permanent_ports
        .contains(&("443".to_string(), "tcp".to_string())));
    assert!(immediate
        .runtime_ports
        .contains(&("9090".to_string(), "tcp".to_string())));

    // Reload clears runtime-only 9090/tcp while keeping permanent 443/tcp active
    h.firewall_backend.simulate_reload();
    let after_reload = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(after_reload
        .runtime_ports
        .contains(&("443".to_string(), "tcp".to_string())));
    assert!(!after_reload
        .runtime_ports
        .contains(&("9090".to_string(), "tcp".to_string())));

    // Simulated reboot also keeps 443/tcp active in both runtime and permanent
    h.firewall_backend.simulate_reboot();
    let after_reboot = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(after_reboot
        .runtime_ports
        .contains(&("443".to_string(), "tcp".to_string())));
    assert!(after_reboot
        .permanent_ports
        .contains(&("443".to_string(), "tcp".to_string())));
}

#[tokio::test]
async fn fw_08_firewall_transaction_rollback() {
    let h = setup_sprint1_harness();

    let pre_public = h.firewall_backend.get_zone_state("public").await.unwrap();
    let pre_work = h.firewall_backend.get_zone_state("work").await.unwrap();

    // 1. Add permanent CIDR rich rule port in 'public' and roll back
    let p_port = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "5432".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::Permanent,
                source_cidr: Some("192.168.50.0/24".to_string()),
            },
        )
        .await
        .unwrap();
    let rec_port = h
        .engine
        .commit_plan(":1.20", &p_port.plan_id, &p_port.plan_sha256)
        .await
        .unwrap();
    let rb_port = h
        .engine
        .rollback_transaction(":1.20", &rec_port.tx_id)
        .await
        .unwrap();
    assert_eq!(rb_port.state, TransactionState::RolledBack);
    assert_eq!(
        h.firewall_backend.get_zone_state("public").await.unwrap(),
        pre_public
    );

    // 2. Bind eth0 from 'public' -> 'work' (permanent=true) and roll back
    let p_iface = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallBindInterface {
                zone: "work".to_string(),
                interface: "eth0".to_string(),
                permanent: true,
            },
        )
        .await
        .unwrap();
    let rec_iface = h
        .engine
        .commit_plan(":1.20", &p_iface.plan_id, &p_iface.plan_sha256)
        .await
        .unwrap();
    assert!(h
        .firewall_backend
        .get_zone_state("work")
        .await
        .unwrap()
        .runtime_interfaces
        .contains(&"eth0".to_string()));

    let rb_iface = h
        .engine
        .rollback_transaction(":1.20", &rec_iface.tx_id)
        .await
        .unwrap();
    assert_eq!(rb_iface.state, TransactionState::RolledBack);
    assert_eq!(
        h.firewall_backend.get_zone_state("public").await.unwrap(),
        pre_public
    );
    assert_eq!(
        h.firewall_backend.get_zone_state("work").await.unwrap(),
        pre_work
    );

    // 3. Simulated post-apply verification failure triggers automatic rollback
    let p_fail = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddService {
                zone: "public".to_string(),
                service: "https".to_string(),
                mode: RulePersistence::Runtime,
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    h.firewall_backend.set_simulate_stale_verify(true);
    let err_fail = h
        .engine
        .commit_plan(":1.20", &p_fail.plan_id, &p_fail.plan_sha256)
        .await
        .unwrap_err();
    h.firewall_backend.set_simulate_stale_verify(false);
    assert!(matches!(err_fail, AkrepError::VerifyFailed(_)));
    assert_eq!(
        h.firewall_backend.get_zone_state("public").await.unwrap(),
        pre_public
    );
}

#[tokio::test]
async fn fw_09_invalid_port_and_cidr_rejected() {
    let h = setup_sprint1_harness();

    let invalid_requests = vec![
        ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "0".to_string(),
            proto: FirewallProto::Tcp,
            mode: RulePersistence::Runtime,
            source_cidr: None,
        },
        ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "65536".to_string(),
            proto: FirewallProto::Tcp,
            mode: RulePersistence::Runtime,
            source_cidr: None,
        },
        ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "22;rm".to_string(),
            proto: FirewallProto::Tcp,
            mode: RulePersistence::Runtime,
            source_cidr: None,
        },
        ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "5000-4000".to_string(),
            proto: FirewallProto::Udp,
            mode: RulePersistence::Runtime,
            source_cidr: None,
        },
        ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "8080".to_string(),
            proto: FirewallProto::Tcp,
            mode: RulePersistence::Runtime,
            source_cidr: Some("0.0.0.0/0".to_string()),
        },
        ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "8080".to_string(),
            proto: FirewallProto::Tcp,
            mode: RulePersistence::Runtime,
            source_cidr: Some("::/0".to_string()),
        },
        ActionRequest::FirewallAddPort {
            zone: "public".to_string(),
            port: "8080".to_string(),
            proto: FirewallProto::Tcp,
            mode: RulePersistence::Runtime,
            source_cidr: Some("192.168.1.0/33".to_string()),
        },
        ActionRequest::FirewallBindInterface {
            zone: "public".to_string(),
            interface: "../eth0".to_string(),
            permanent: false,
        },
        ActionRequest::FirewallBindInterface {
            zone: "public".to_string(),
            interface: "interface_name_16".to_string(),
            permanent: false,
        },
    ];

    for bad in invalid_requests {
        assert!(h.engine.create_plan(":1.20", bad).await.is_err());
    }
    assert!(h.engine.list_records_sorted(50).unwrap().is_empty());
}

#[tokio::test]
async fn fw_10_unauthorized_caller_denied_by_polkit() {
    let h = setup_sprint1_harness();
    h.auth.deny_all_for_bus(":1.88", 1008);

    let req = ActionRequest::FirewallAddPort {
        zone: "public".to_string(),
        port: "8443".to_string(),
        proto: FirewallProto::Tcp,
        mode: RulePersistence::Runtime,
        source_cidr: None,
    };
    let err = h
        .engine
        .create_plan(":1.88", req.clone())
        .await
        .unwrap_err();
    assert!(matches!(err, AkrepError::PolkitDenied(_)));

    let insp_err = h.engine.inspect_firewall(":1.88", "").await.unwrap_err();
    assert!(matches!(insp_err, AkrepError::PolkitDenied(_)));

    // Revoking authorization between CreatePlan and CommitPlan also aborts cleanly
    let staged = h.engine.create_plan(":1.20", req).await.unwrap();
    h.auth.deny_all_for_bus(":1.20", 1000);
    let commit_err = h
        .engine
        .commit_plan(":1.20", &staged.plan_id, &staged.plan_sha256)
        .await
        .unwrap_err();
    assert!(matches!(commit_err, AkrepError::PolkitDenied(_)));
    assert!(h.engine.list_records_sorted(50).unwrap().is_empty());
}

#[tokio::test]
async fn fw_11_firewalld_inactive_aborts_cleanly() {
    let h = setup_sprint1_harness();

    let req = ActionRequest::FirewallAddPort {
        zone: "public".to_string(),
        port: "8443".to_string(),
        proto: FirewallProto::Tcp,
        mode: RulePersistence::Runtime,
        source_cidr: None,
    };

    // 1. Inactive during CreatePlan -> BackendUnavailable with zero plan/tx files created
    h.firewall_backend.set_running(false);
    let err_plan = h
        .engine
        .create_plan(":1.20", req.clone())
        .await
        .unwrap_err();
    assert!(matches!(err_plan, AkrepError::BackendUnavailable(_)));
    assert!(h.engine.list_records_sorted(50).unwrap().is_empty());

    // InspectFirewall returns running=false snapshot cleanly when service is stopped
    let snap: FirewallStateSnapshot =
        serde_json::from_str(&h.engine.inspect_firewall(":1.20", "").await.unwrap()).unwrap();
    assert!(!snap.running);

    // 2. Active during CreatePlan, then firewalld stops before CommitPlan -> BackendUnavailable, zero tx/snapshot files
    h.firewall_backend.set_running(true);
    let plan = h.engine.create_plan(":1.20", req).await.unwrap();
    h.firewall_backend.set_running(false);

    let err_commit = h
        .engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap_err();
    assert!(matches!(err_commit, AkrepError::BackendUnavailable(_)));
    assert!(
        h.engine.list_records_sorted(50).unwrap().is_empty(),
        "no transaction record should be created when firewalld is offline before apply"
    );
}

#[tokio::test]
async fn fw_12_external_firewall_modification_triggers_drift_conflict() {
    let h = setup_sprint1_harness();

    let plan = h
        .engine
        .create_plan(
            ":1.20",
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "8080".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::Runtime,
                source_cidr: None,
            },
        )
        .await
        .unwrap();
    let rec = h
        .engine
        .commit_plan(":1.20", &plan.plan_id, &plan.plan_sha256)
        .await
        .unwrap();
    assert_eq!(rec.state, TransactionState::Committed);

    // Simulate external administrator opening port 9091/tcp out-of-band in 'public'
    h.firewall_backend
        .inject_external_runtime_port("public", "9091", FirewallProto::Tcp);

    let drift_err = h
        .engine
        .rollback_transaction(":1.20", &rec.tx_id)
        .await
        .unwrap_err();
    assert!(matches!(drift_err, AkrepError::ExternalDriftDetected(_)));

    // Both ports remain untouched (no blind overwrite)
    let st = h.firewall_backend.get_zone_state("public").await.unwrap();
    assert!(st
        .runtime_ports
        .contains(&("8080".to_string(), "tcp".to_string())));
    assert!(st
        .runtime_ports
        .contains(&("9091".to_string(), "tcp".to_string())));

    let events = h.engine.audit().read_events().unwrap();
    assert!(events.iter().any(|e| e.event == "external_drift_detected"));
}
