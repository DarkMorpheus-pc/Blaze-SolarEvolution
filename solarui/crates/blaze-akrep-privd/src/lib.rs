pub mod audit;
pub mod authorization;
pub mod dbus;
pub mod transaction_engine;

pub use audit::*;
pub use authorization::*;
pub use dbus::*;
pub use transaction_engine::*;

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use blaze_akrep_common::{
        ActionRequest, AkrepError, FirewallProto, FlatpakScope, PlanDiffItem, RiskLevel,
        RulePersistence, StateFingerprint, TransactionRecord, TransactionState,
    };
    use blaze_sentinel_core::sha256_hex;
    use chrono::{Duration as ChronoDuration, Utc};
    use std::collections::HashMap;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Default)]
    struct TestKvBackend {
        kv: Mutex<HashMap<String, String>>,
        secret_error_override: Mutex<Option<String>>,
        apply_call_count: Mutex<usize>,
    }

    impl TestKvBackend {
        fn new() -> Self {
            let mut map = HashMap::new();
            map.insert("firewall.zone".to_string(), "public".to_string());
            map.insert("profile.active".to_string(), "standard".to_string());
            Self {
                kv: Mutex::new(map),
                secret_error_override: Mutex::new(None),
                apply_call_count: Mutex::new(0),
            }
        }

        fn get(&self, key: &str) -> Option<String> {
            self.kv.lock().unwrap().get(key).cloned()
        }

        fn set_external_drift(&self, key: &str, drifted_value: &str) {
            self.kv
                .lock()
                .unwrap()
                .insert(key.to_string(), drifted_value.to_string());
        }

        fn set_secret_error(&self, msg: &str) {
            *self.secret_error_override.lock().unwrap() = Some(msg.to_string());
        }

        fn apply_calls(&self) -> usize {
            *self.apply_call_count.lock().unwrap()
        }

        fn extract_kv(req: &ActionRequest) -> (String, String, Option<String>) {
            match req {
                ActionRequest::NoopTestAction {
                    key,
                    target_value,
                    simulate_fail_step,
                } => (
                    key.clone(),
                    target_value.clone(),
                    simulate_fail_step.clone(),
                ),
                ActionRequest::ApplyProfile { profile_id } => {
                    ("profile.active".to_string(), profile_id.clone(), None)
                }
                _ => (
                    "generic.target".to_string(),
                    serde_json::to_string(req).unwrap_or_default(),
                    None,
                ),
            }
        }

        fn fingerprint_for(&self, key: &str) -> StateFingerprint {
            let val = self.get(key).unwrap_or_else(|| "<unset>".to_string());
            StateFingerprint {
                target: key.to_string(),
                sha256: sha256_hex(val.as_bytes()),
                captured_at: Utc::now().to_rfc3339(),
            }
        }
    }

    #[async_trait]
    impl MutationBackend for TestKvBackend {
        async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
            let (key, desired, _) = Self::extract_kv(req);
            let current = self.get(&key).unwrap_or_else(|| "<unset>".to_string());
            Ok(PreviewOutcome {
                risk: RiskLevel::Low,
                diffs: vec![PlanDiffItem {
                    target: key.clone(),
                    current_value: current,
                    desired_value: desired,
                    description_tr: "Test durum degisikligi".to_string(),
                    description_en: "Test state mutation".to_string(),
                }],
                pre_fingerprints: vec![self.fingerprint_for(&key)],
                warnings_tr: vec![],
                warnings_en: vec![],
            })
        }

        async fn apply(
            &self,
            req: &ActionRequest,
            snapshot_dir: &Path,
        ) -> Result<ApplyOutcome, AkrepError> {
            *self.apply_call_count.lock().unwrap() += 1;
            let (key, desired, fail_step) = Self::extract_kv(req);
            let prev = self.get(&key);

            let snap_payload = serde_json::json!({
                "key": key,
                "previous": prev,
            });
            fs::write(
                snapshot_dir.join("kv_backup.json"),
                serde_json::to_vec(&snap_payload).unwrap(),
            )?;

            if fail_step.as_deref() == Some("apply") {
                if let Some(sec_msg) = self.secret_error_override.lock().unwrap().clone() {
                    return Err(AkrepError::ApplyFailed(sec_msg));
                }
                return Err(AkrepError::ApplyFailed(
                    "simulated mid-apply failure".to_string(),
                ));
            }

            self.kv.lock().unwrap().insert(key.clone(), desired.clone());
            Ok(ApplyOutcome {
                pre_snapshot: snap_payload,
                target_state: serde_json::json!({ "key": key, "desired": desired }),
                summary: format!("updated {} to {}", key, desired),
            })
        }

        async fn verify(
            &self,
            req: &ActionRequest,
            _expected: &ApplyOutcome,
        ) -> Result<Vec<StateFingerprint>, AkrepError> {
            let (key, _, fail_step) = Self::extract_kv(req);
            if fail_step.as_deref() == Some("verify") {
                if let Some(sec_msg) = self.secret_error_override.lock().unwrap().clone() {
                    return Err(AkrepError::VerifyFailed(sec_msg));
                }
                return Err(AkrepError::VerifyFailed(
                    "simulated post-apply verification failure".to_string(),
                ));
            }
            Ok(vec![self.fingerprint_for(&key)])
        }

        async fn current_fingerprints(
            &self,
            req: &ActionRequest,
            _post_fps: &[StateFingerprint],
        ) -> Result<Vec<StateFingerprint>, AkrepError> {
            let (key, _, _) = Self::extract_kv(req);
            Ok(vec![self.fingerprint_for(&key)])
        }

        async fn rollback(
            &self,
            record: &TransactionRecord,
            snapshot_dir: &Path,
        ) -> Result<String, AkrepError> {
            let (_, _, fail_step) = Self::extract_kv(&record.request);
            if fail_step.as_deref() == Some("rollback") {
                return Err(AkrepError::RollbackFailed(
                    "simulated rollback failure".to_string(),
                ));
            }

            let snap_file = snapshot_dir.join("kv_backup.json");
            let snap: serde_json::Value = if snap_file.exists() {
                serde_json::from_slice(&fs::read(snap_file)?).unwrap()
            } else {
                record.pre_snapshot.clone()
            };

            if let Some(key) = snap.get("key").and_then(|v| v.as_str()) {
                match snap.get("previous").and_then(|v| v.as_str()) {
                    Some(prev_val) => {
                        self.kv
                            .lock()
                            .unwrap()
                            .insert(key.to_string(), prev_val.to_string());
                    }
                    None => {
                        self.kv.lock().unwrap().remove(key);
                    }
                }
                return Ok(format!("restored key {}", key));
            }
            Ok("nothing to restore".to_string())
        }
    }

    fn setup_harness() -> (
        TempDir,
        Arc<MockAuthorizer>,
        Arc<TestKvBackend>,
        TransactionEngine,
    ) {
        let dir = TempDir::new().expect("tempdir");
        let auth = Arc::new(MockAuthorizer::new());
        auth.allow_all_for_bus(":1.10", 1000);
        let backend = Arc::new(TestKvBackend::new());
        let engine = TransactionEngine::new_with_options(
            dir.path().to_path_buf(),
            auth.clone(),
            backend.clone(),
            true,
        )
        .expect("engine init");
        (dir, auth, backend, engine)
    }

    #[tokio::test]
    async fn tx_01_normal_commit() {
        let (dir, _auth, backend, engine) = setup_harness();

        for sub in ["transactions", "snapshots", "quarantine"] {
            let mode = fs::metadata(dir.path().join(sub))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o700, "directory {} must be 0700", sub);
        }

        let req = ActionRequest::NoopTestAction {
            key: "firewall.zone".to_string(),
            target_value: "drop".to_string(),
            simulate_fail_step: None,
        };
        let plan = engine.create_plan(":1.10", req).await.expect("create_plan");
        let record = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .expect("commit_plan");

        assert_eq!(record.state, TransactionState::Committed);
        assert_eq!(record.actor_uid, 1000);
        assert_eq!(record.post_fingerprints.len(), 1);
        assert_ne!(
            record.pre_fingerprints[0].sha256,
            record.post_fingerprints[0].sha256
        );
        assert_eq!(backend.get("firewall.zone").as_deref(), Some("drop"));

        let tx_file = engine
            .transactions_dir()
            .join(format!("{}.json", record.tx_id));
        assert!(tx_file.is_file());
        let tx_mode = fs::metadata(&tx_file).unwrap().permissions().mode() & 0o777;
        assert_eq!(tx_mode, 0o600);
    }

    #[tokio::test]
    async fn tx_02_polkit_denied_leaves_zero_changes() {
        let (_dir, auth, backend, engine) = setup_harness();
        auth.deny_all_for_bus(":1.99", 1002);

        let req = ActionRequest::NoopTestAction {
            key: "firewall.zone".to_string(),
            target_value: "trusted".to_string(),
            simulate_fail_step: None,
        };
        let err = engine.create_plan(":1.99", req.clone()).await.unwrap_err();
        assert!(matches!(err, AkrepError::PolkitDenied(_)));
        assert_eq!(backend.apply_calls(), 0);
        assert_eq!(backend.get("firewall.zone").as_deref(), Some("public"));
        assert!(engine.list_records_sorted(50).unwrap().is_empty());

        // Also verify revocation between CreatePlan and CommitPlan leaves zero changes
        let plan = engine.create_plan(":1.10", req).await.unwrap();
        auth.deny_all_for_bus(":1.10", 1000);
        let commit_err = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap_err();
        assert!(matches!(commit_err, AkrepError::PolkitDenied(_)));
        assert_eq!(backend.apply_calls(), 0);
        assert_eq!(backend.get("firewall.zone").as_deref(), Some("public"));
        assert!(engine.list_records_sorted(50).unwrap().is_empty());
    }

    #[tokio::test]
    async fn tx_03_mid_apply_or_verify_failure_triggers_rollback() {
        let (_dir, _auth, backend, engine) = setup_harness();

        let req = ActionRequest::NoopTestAction {
            key: "firewall.zone".to_string(),
            target_value: "work".to_string(),
            simulate_fail_step: Some("verify".to_string()),
        };
        let plan = engine.create_plan(":1.10", req).await.unwrap();
        let err = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap_err();

        assert!(matches!(err, AkrepError::VerifyFailed(_)));
        assert_eq!(
            backend.get("firewall.zone").as_deref(),
            Some("public"),
            "automatic rollback must restore pre-apply value"
        );

        let history = engine.get_history(":1.10", 10).await.unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].state, TransactionState::RolledBack);
        assert!(history[0].rollback_result.is_some());
    }

    #[tokio::test]
    async fn tx_04_disk_write_failure_fails_safely() {
        let (_dir, _auth, backend, engine) = setup_harness();

        let req = ActionRequest::NoopTestAction {
            key: "firewall.zone".to_string(),
            target_value: "internal".to_string(),
            simulate_fail_step: None,
        };
        let plan = engine.create_plan(":1.10", req).await.unwrap();

        // Replace transactions directory with a regular file (and 0400 mode) so creating
        // any transaction record fails deterministically (even under CAP_DAC_OVERRIDE) BEFORE backend.apply.
        let tx_dir = engine.transactions_dir();
        fs::remove_dir_all(&tx_dir).unwrap();
        fs::write(&tx_dir, b"unwritable_blocker").unwrap();
        fs::set_permissions(&tx_dir, fs::Permissions::from_mode(0o400)).unwrap();

        let res = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await;

        // Restore cleanup permissions before assertions
        fs::set_permissions(&tx_dir, fs::Permissions::from_mode(0o600)).unwrap();
        fs::remove_file(&tx_dir).unwrap();
        fs::create_dir_all(&tx_dir).unwrap();

        assert!(res.is_err());
        assert!(matches!(res.unwrap_err(), AkrepError::IoError(_)));
        assert_eq!(
            backend.apply_calls(),
            0,
            "backend.apply must never run if transaction record persistence fails"
        );
        assert_eq!(backend.get("firewall.zone").as_deref(), Some("public"));
    }

    #[tokio::test]
    async fn tx_05_crash_recovery_marks_interrupted() {
        let dir = TempDir::new().unwrap();
        let tx_dir = dir.path().join("transactions");
        fs::create_dir_all(&tx_dir).unwrap();

        let crashed_record = TransactionRecord {
            tx_id: "tx-crash-001".to_string(),
            plan_id: "plan-crash-001".to_string(),
            plan_sha256: "a".repeat(64),
            actor_uid: 1000,
            actor_bus_name: ":1.10".to_string(),
            polkit_action: "tr.com.blazeos.Akrep1.profile.apply".to_string(),
            polkit_authorized: true,
            request: ActionRequest::ApplyProfile {
                profile_id: "hardened".to_string(),
            },
            state: TransactionState::Applying,
            pre_snapshot: serde_json::json!({"profile": "standard"}),
            target_state: serde_json::json!({"profile": "hardened"}),
            pre_fingerprints: vec![],
            post_fingerprints: vec![],
            apply_result: None,
            rollback_result: None,
            error_message: None,
            created_at: Utc::now().to_rfc3339(),
            updated_at: Utc::now().to_rfc3339(),
            completed_at: None,
        };
        fs::write(
            tx_dir.join("tx-crash-001.json"),
            serde_json::to_vec_pretty(&crashed_record).unwrap(),
        )
        .unwrap();

        let auth = Arc::new(MockAuthorizer::new());
        auth.allow_all_for_bus(":1.10", 1000);
        let backend = Arc::new(TestKvBackend::new());

        let engine =
            TransactionEngine::new_with_options(dir.path().to_path_buf(), auth, backend, true)
                .unwrap();

        let recovered = engine
            .get_transaction(":1.10", "tx-crash-001")
            .await
            .unwrap();
        assert_eq!(recovered.state, TransactionState::Interrupted);
        assert!(recovered.completed_at.is_some());
        assert!(recovered
            .error_message
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .contains("interrupted"));

        let events = engine.audit().read_events().unwrap();
        assert!(events.iter().any(|e| e.event == "transaction_interrupted"));
    }

    #[tokio::test]
    async fn tx_06_restart_preserves_transaction_history() {
        let dir = TempDir::new().unwrap();
        let auth = Arc::new(MockAuthorizer::new());
        auth.allow_all_for_bus(":1.10", 1000);
        let backend = Arc::new(TestKvBackend::new());

        let committed_tx_id;
        {
            let engine1 = TransactionEngine::new_with_options(
                dir.path().to_path_buf(),
                auth.clone(),
                backend.clone(),
                true,
            )
            .unwrap();

            let p1 = engine1
                .create_plan(
                    ":1.10",
                    ActionRequest::NoopTestAction {
                        key: "profile.active".to_string(),
                        target_value: "developer".to_string(),
                        simulate_fail_step: None,
                    },
                )
                .await
                .unwrap();
            let rec1 = engine1
                .commit_plan(":1.10", &p1.plan_id, &p1.plan_sha256)
                .await
                .unwrap();
            committed_tx_id = rec1.tx_id;

            let p2 = engine1
                .create_plan(
                    ":1.10",
                    ActionRequest::NoopTestAction {
                        key: "firewall.zone".to_string(),
                        target_value: "dmz".to_string(),
                        simulate_fail_step: Some("verify".to_string()),
                    },
                )
                .await
                .unwrap();
            let _ = engine1
                .commit_plan(":1.10", &p2.plan_id, &p2.plan_sha256)
                .await;
        }

        let engine2 =
            TransactionEngine::new_with_options(dir.path().to_path_buf(), auth, backend, true)
                .unwrap();

        let history = engine2.get_history(":1.10", 20).await.unwrap();
        assert_eq!(history.len(), 2);
        assert!(history
            .iter()
            .any(|r| r.tx_id == committed_tx_id && r.state == TransactionState::Committed));
        assert!(history
            .iter()
            .any(|r| r.state == TransactionState::RolledBack));
    }

    #[tokio::test]
    async fn tx_07_concurrent_transaction_conflict() {
        let (_dir, _auth, backend, engine) = setup_harness();

        let plan = engine
            .create_plan(
                ":1.10",
                ActionRequest::NoopTestAction {
                    key: "firewall.zone".to_string(),
                    target_value: "block".to_string(),
                    simulate_fail_step: None,
                },
            )
            .await
            .unwrap();

        let held_lock = FlockGuard::try_acquire_exclusive(&engine.lock_path()).unwrap();
        let err = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap_err();
        assert!(matches!(err, AkrepError::TransactionConflict(_)));
        assert_eq!(backend.apply_calls(), 0);

        drop(held_lock);
        let ok_rec = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap();
        assert_eq!(ok_rec.state, TransactionState::Committed);
    }

    #[tokio::test]
    async fn tx_08_external_drift_blocks_blind_rollback() {
        let (_dir, _auth, backend, engine) = setup_harness();

        let plan = engine
            .create_plan(
                ":1.10",
                ActionRequest::NoopTestAction {
                    key: "firewall.zone".to_string(),
                    target_value: "work".to_string(),
                    simulate_fail_step: None,
                },
            )
            .await
            .unwrap();
        let committed = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap();
        assert_eq!(backend.get("firewall.zone").as_deref(), Some("work"));

        // Simulate external CLI/admin modifying firewall.zone after commit
        backend.set_external_drift("firewall.zone", "custom_admin_zone");

        let drift_err = engine
            .rollback_transaction(":1.10", &committed.tx_id)
            .await
            .unwrap_err();
        assert!(matches!(drift_err, AkrepError::ExternalDriftDetected(_)));
        assert_eq!(
            backend.get("firewall.zone").as_deref(),
            Some("custom_admin_zone"),
            "rollback must not overwrite externally modified state"
        );
    }

    #[tokio::test]
    async fn tx_09_spoofed_uid_ignored_in_favor_of_bus_identity() {
        let (_dir, auth, _backend, engine) = setup_harness();
        auth.allow_all_for_bus(":1.50", 1000);
        auth.allow_all_for_bus(":1.51", 1001);

        let spoofed_json = r#"{
            "kind": "apply_profile",
            "profile_id": "workstation",
            "uid": 0,
            "actor_uid": 0,
            "created_by_uid": 0
        }"#;

        let plan = engine
            .create_plan_from_json(":1.50", spoofed_json)
            .await
            .unwrap();
        assert_eq!(
            plan.created_by_uid, 1000,
            "D-Bus connection UID must override any forged JSON UID"
        );

        let cross_uid_err = engine
            .commit_plan(":1.51", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap_err();
        assert!(matches!(cross_uid_err, AkrepError::Unauthorized(_)));

        let committed = engine
            .commit_plan(":1.50", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap();
        assert_eq!(committed.actor_uid, 1000);
        assert_eq!(committed.actor_bus_name, ":1.50");
    }

    #[tokio::test]
    async fn tx_10_arbitrary_or_redline_inputs_rejected() {
        let (_dir, auth, backend, engine) = setup_harness();

        let bad_requests = vec![
            ActionRequest::SelinuxRestorecon {
                target_path: "../etc/shadow".to_string(),
                recursive: true,
            },
            ActionRequest::SelinuxSetBoolean {
                name: "virt_use_nfs; id".to_string(),
                value: true,
                persistent: false,
            },
            ActionRequest::FlatpakSetOverride {
                app_id: "org.mozilla.firefox".to_string(),
                scope: FlatpakScope::System,
                additions: vec![("Context.filesystems".to_string(), "/home".to_string())],
                removals: vec![],
            },
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "0".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::Runtime,
                source_cidr: None,
            },
            ActionRequest::FirewallAddPort {
                zone: "public".to_string(),
                port: "70000".to_string(),
                proto: FirewallProto::Tcp,
                mode: RulePersistence::Runtime,
                source_cidr: None,
            },
            ActionRequest::DeleteQuarantine {
                quarantine_id: "quarantine_item_1".to_string(),
                confirm_permanent_delete: false,
            },
        ];

        for bad in bad_requests {
            assert!(bad.validate_red_lines().is_err());
            assert!(engine.create_plan(":1.10", bad).await.is_err());
        }
        assert_eq!(backend.apply_calls(), 0);

        // Verify production mode rejects NoopTestAction even for authenticated caller
        let prod_dir = TempDir::new().unwrap();
        let prod_engine =
            TransactionEngine::new(prod_dir.path().to_path_buf(), auth, backend).unwrap();
        let test_action_err = prod_engine
            .create_plan(
                ":1.10",
                ActionRequest::NoopTestAction {
                    key: "k".to_string(),
                    target_value: "v".to_string(),
                    simulate_fail_step: None,
                },
            )
            .await
            .unwrap_err();
        assert!(matches!(test_action_err, AkrepError::ForbiddenOperation(_)));
    }

    #[tokio::test]
    async fn tx_11_replay_and_tampered_hash_rejected() {
        let (_dir, _auth, _backend, engine) = setup_harness();

        let plan = engine
            .create_plan(
                ":1.10",
                ActionRequest::NoopTestAction {
                    key: "profile.active".to_string(),
                    target_value: "strict".to_string(),
                    simulate_fail_step: None,
                },
            )
            .await
            .unwrap();

        // 1. Tampered expected hash rejected
        let bad_hash = "0".repeat(64);
        let hash_err = engine
            .commit_plan(":1.10", &plan.plan_id, &bad_hash)
            .await
            .unwrap_err();
        assert!(matches!(hash_err, AkrepError::PlanMismatch(_)));

        // 2. First valid commit succeeds
        let rec = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap();
        assert_eq!(rec.state, TransactionState::Committed);

        // 3. Replay of the exact same plan_id fails
        let replay_err = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap_err();
        assert!(matches!(replay_err, AkrepError::PlanMismatch(_)));

        // 4. Expired plan rejected
        let mut expired_plan = engine
            .create_plan(
                ":1.10",
                ActionRequest::NoopTestAction {
                    key: "profile.active".to_string(),
                    target_value: "office".to_string(),
                    simulate_fail_step: None,
                },
            )
            .await
            .unwrap();
        expired_plan.expires_at = (Utc::now() - ChronoDuration::seconds(10)).to_rfc3339();
        expired_plan.plan_sha256 = expired_plan.compute_canonical_sha256().unwrap();
        write_json_atomic_0600(
            &engine
                .plans_dir()
                .join(format!("{}.json", expired_plan.plan_id)),
            &expired_plan,
        )
        .unwrap();

        let exp_err = engine
            .commit_plan(":1.10", &expired_plan.plan_id, &expired_plan.plan_sha256)
            .await
            .unwrap_err();
        assert!(matches!(exp_err, AkrepError::PlanExpired(_)));
    }

    #[tokio::test]
    async fn tx_12_sensitive_secrets_redacted_in_logs_and_records() {
        let (_dir, _auth, backend, engine) = setup_harness();
        backend.set_secret_error(
            "backend auth failed: password=supersecret token=abc12345 Bearer jwtSecretValue99",
        );

        let plan = engine
            .create_plan(
                ":1.10",
                ActionRequest::NoopTestAction {
                    key: "firewall.zone".to_string(),
                    target_value: "dmz".to_string(),
                    simulate_fail_step: Some("verify".to_string()),
                },
            )
            .await
            .unwrap();

        let err = engine
            .commit_plan(":1.10", &plan.plan_id, &plan.plan_sha256)
            .await
            .unwrap_err();
        let err_json = err.to_redacted_json();
        for forbidden in ["supersecret", "abc12345", "jwtSecretValue99"] {
            assert!(!err.message().contains(forbidden));
            assert!(!err_json.contains(forbidden));
        }
        assert!(err.message().contains("[REDACTED]"));

        let history = engine.get_history(":1.10", 10).await.unwrap();
        assert_eq!(history.len(), 1);
        let raw_tx_disk = fs::read_to_string(
            engine
                .transactions_dir()
                .join(format!("{}.json", history[0].tx_id)),
        )
        .unwrap();
        let raw_audit_disk = fs::read_to_string(engine.audit().path()).unwrap();

        for forbidden in ["supersecret", "abc12345", "jwtSecretValue99"] {
            assert!(
                !raw_tx_disk.contains(forbidden),
                "transaction JSON leaked {}",
                forbidden
            );
            assert!(
                !raw_audit_disk.contains(forbidden),
                "audit.log leaked {}",
                forbidden
            );
        }
        assert!(raw_tx_disk.contains("[REDACTED]"));
        assert!(raw_audit_disk.contains("[REDACTED]"));

        let audit_mode = fs::metadata(engine.audit().path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(audit_mode, 0o600);
    }
}
