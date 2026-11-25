use crate::audit::AuditLogger;
use crate::authorization::Authorizer;
use async_trait::async_trait;
use blaze_akrep_common::{
    generate_uuid_v4, redact_secrets, ActionPlan, ActionRequest, AkrepError, PlanDiffItem,
    RiskLevel, StateFingerprint, TransactionRecord, TransactionState, ALL_POLKIT_ACTIONS,
    POLKIT_PROFILE_APPLY, POLKIT_SELINUX_DIAGNOSE,
};
use chrono::{DateTime, Utc};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const DEFAULT_AKREP_STATE_DIR: &str = "/var/lib/akrep";

#[derive(Debug, Clone)]
pub struct PreviewOutcome {
    pub risk: RiskLevel,
    pub diffs: Vec<PlanDiffItem>,
    pub pre_fingerprints: Vec<StateFingerprint>,
    pub warnings_tr: Vec<String>,
    pub warnings_en: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ApplyOutcome {
    pub pre_snapshot: serde_json::Value,
    pub target_state: serde_json::Value,
    pub summary: String,
}

#[async_trait]
pub trait MutationBackend: Send + Sync {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError>;
    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError>;
    async fn verify(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError>;
    async fn current_fingerprints(
        &self,
        req: &ActionRequest,
        post_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError>;
    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError>;
}

pub struct DefaultStubBackend;

#[async_trait]
impl MutationBackend for DefaultStubBackend {
    async fn preview(&self, _req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        Err(AkrepError::BackendUnavailable(
            "Requested security domain backend is not yet wired in Sprint 0".to_string(),
        ))
    }

    async fn apply(
        &self,
        _req: &ActionRequest,
        _snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        Err(AkrepError::BackendUnavailable(
            "Requested security domain backend is not yet wired in Sprint 0".to_string(),
        ))
    }

    async fn verify(
        &self,
        _req: &ActionRequest,
        _expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        Err(AkrepError::BackendUnavailable(
            "Requested security domain backend is not yet wired in Sprint 0".to_string(),
        ))
    }

    async fn current_fingerprints(
        &self,
        _req: &ActionRequest,
        _post_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        Err(AkrepError::BackendUnavailable(
            "Requested security domain backend is not yet wired in Sprint 0".to_string(),
        ))
    }

    async fn rollback(
        &self,
        _record: &TransactionRecord,
        _snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        Err(AkrepError::BackendUnavailable(
            "Requested security domain backend is not yet wired in Sprint 0".to_string(),
        ))
    }
}

pub struct FlockGuard {
    file: File,
}

impl FlockGuard {
    pub fn try_acquire_exclusive(lock_path: &Path) -> Result<Self, AkrepError> {
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .mode(0o600)
            .open(lock_path)?;
        let _ = fs::set_permissions(lock_path, fs::Permissions::from_mode(0o600));

        // SAFETY: `file.as_raw_fd()` is a valid open descriptor owned by `file` for the lifetime of `FlockGuard`.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let os_err = std::io::Error::last_os_error();
            if os_err.raw_os_error() == Some(libc::EWOULDBLOCK) {
                return Err(AkrepError::TransactionConflict(
                    "Another Akrep privileged transaction is currently in progress".to_string(),
                ));
            }
            return Err(AkrepError::IoError(redact_secrets(&os_err.to_string())));
        }

        Ok(Self { file })
    }
}

impl Drop for FlockGuard {
    fn drop(&mut self) {
        // SAFETY: `self.file` remains open until after `Drop::drop` returns.
        unsafe {
            let _ = libc::flock(self.file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}

fn ensure_dir_0700(path: &Path) -> Result<(), AkrepError> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn sync_directory(dir: &Path) -> Result<(), AkrepError> {
    let dir_file = File::open(dir)?;
    dir_file.sync_all()?;
    Ok(())
}

pub fn write_json_atomic_0600<T: Serialize>(target: &Path, value: &T) -> Result<(), AkrepError> {
    let parent = target.parent().ok_or_else(|| {
        AkrepError::Internal("atomic target path missing parent directory".to_string())
    })?;
    let file_name = target
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AkrepError::Internal("invalid atomic target file name".to_string()))?;

    let nonce = generate_uuid_v4()?;
    let tmp_path = parent.join(format!("{}.{}.tmp", file_name, nonce));
    let payload = serde_json::to_vec_pretty(value)
        .map_err(|e| AkrepError::Internal(format!("json serialize failed: {}", e)))?;

    let mut tmp_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp_path)?;
    fs::set_permissions(&tmp_path, fs::Permissions::from_mode(0o600))?;

    if let Err(write_err) = tmp_file
        .write_all(&payload)
        .and_then(|_| tmp_file.sync_all())
    {
        let _ = fs::remove_file(&tmp_path);
        return Err(AkrepError::from(write_err));
    }
    drop(tmp_file);

    if let Err(rename_err) = fs::rename(&tmp_path, target) {
        let _ = fs::remove_file(&tmp_path);
        return Err(AkrepError::from(rename_err));
    }

    fs::set_permissions(target, fs::Permissions::from_mode(0o600))?;
    sync_directory(parent)?;
    Ok(())
}

fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, AkrepError> {
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(|e| {
        AkrepError::Internal(format!(
            "Failed to parse state file '{}': {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            redact_secrets(&e.to_string())
        ))
    })
}

fn validate_record_id(label: &str, id: &str) -> Result<(), AkrepError> {
    if id.is_empty() || id.len() > 128 {
        return Err(AkrepError::InvalidInput(format!(
            "{} must be 1..=128 characters",
            label
        )));
    }
    if id.contains("..")
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(AkrepError::InvalidInput(format!(
            "Invalid identifier for {}",
            label
        )));
    }
    Ok(())
}

fn fingerprints_match_exactly(expected: &[StateFingerprint], actual: &[StateFingerprint]) -> bool {
    if expected.len() != actual.len() {
        return false;
    }
    for exp in expected {
        let Some(found) = actual.iter().find(|a| a.target == exp.target) else {
            return false;
        };
        if found.sha256 != exp.sha256 {
            return false;
        }
    }
    true
}

pub struct TransactionEngine {
    root_dir: PathBuf,
    authorizer: Arc<dyn Authorizer>,
    backend: Arc<dyn MutationBackend>,
    audit: AuditLogger,
    allow_test_actions: bool,
}

impl TransactionEngine {
    pub fn new(
        root_dir: PathBuf,
        authorizer: Arc<dyn Authorizer>,
        backend: Arc<dyn MutationBackend>,
    ) -> Result<Self, AkrepError> {
        Self::new_with_options(root_dir, authorizer, backend, false)
    }

    pub fn new_with_options(
        root_dir: PathBuf,
        authorizer: Arc<dyn Authorizer>,
        backend: Arc<dyn MutationBackend>,
        allow_test_actions: bool,
    ) -> Result<Self, AkrepError> {
        ensure_dir_0700(&root_dir)?;
        ensure_dir_0700(&root_dir.join("transactions"))?;
        ensure_dir_0700(&root_dir.join("snapshots"))?;
        ensure_dir_0700(&root_dir.join("quarantine"))?;
        ensure_dir_0700(&root_dir.join("plans"))?;

        let audit = AuditLogger::new(root_dir.join("audit.log"))?;
        let engine = Self {
            root_dir,
            authorizer,
            backend,
            audit,
            allow_test_actions,
        };
        let _ = engine.recover_interrupted_transactions()?;
        Ok(engine)
    }

    pub fn root_dir(&self) -> &Path {
        &self.root_dir
    }

    pub fn transactions_dir(&self) -> PathBuf {
        self.root_dir.join("transactions")
    }

    pub fn snapshots_dir(&self) -> PathBuf {
        self.root_dir.join("snapshots")
    }

    pub fn quarantine_dir(&self) -> PathBuf {
        self.root_dir.join("quarantine")
    }

    pub fn plans_dir(&self) -> PathBuf {
        self.root_dir.join("plans")
    }

    pub fn lock_path(&self) -> PathBuf {
        self.root_dir.join("tx.lock")
    }

    pub fn audit(&self) -> &AuditLogger {
        &self.audit
    }

    pub fn authorizer(&self) -> &Arc<dyn Authorizer> {
        &self.authorizer
    }

    pub fn recover_interrupted_transactions(&self) -> Result<usize, AkrepError> {
        let _lock = FlockGuard::try_acquire_exclusive(&self.lock_path())?;
        let tx_dir = self.transactions_dir();
        if !tx_dir.exists() {
            return Ok(0);
        }

        let mut recovered = 0usize;
        for entry in fs::read_dir(&tx_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let mut record: TransactionRecord = read_json_file(&path)?;
            if record.state.is_in_flight() {
                let prev_state = record.state;
                let now = Utc::now().to_rfc3339();
                let note = format!(
                    "Recovered interrupted transaction from in-flight state {:?}",
                    prev_state
                );
                record.state = TransactionState::Interrupted;
                record.updated_at = now.clone();
                record.completed_at = Some(now);
                record.error_message = Some(match record.error_message.take() {
                    Some(prev) => format!("{}; {}", redact_secrets(&prev), note),
                    None => note.clone(),
                });
                write_json_atomic_0600(&path, &record)?;
                self.audit.log(
                    "transaction_interrupted",
                    Some(&record.tx_id),
                    Some(&record.plan_id),
                    Some(record.actor_uid),
                    Some(&record.actor_bus_name),
                    Some(&record.polkit_action),
                    "interrupted",
                    &note,
                )?;
                recovered += 1;
            }
        }
        Ok(recovered)
    }

    pub async fn create_plan_from_json(
        &self,
        caller_bus_name: &str,
        request_json: &str,
    ) -> Result<ActionPlan, AkrepError> {
        if request_json.len() > 65_536 || request_json.contains('\0') {
            return Err(AkrepError::InvalidInput(
                "ActionRequest JSON payload is invalid or exceeds 64 KiB".to_string(),
            ));
        }
        let req: ActionRequest = serde_json::from_str(request_json).map_err(|e| {
            AkrepError::InvalidInput(format!(
                "Malformed ActionRequest JSON: {}",
                redact_secrets(&e.to_string())
            ))
        })?;
        self.create_plan(caller_bus_name, req).await
    }

    pub async fn create_plan(
        &self,
        caller_bus_name: &str,
        req: ActionRequest,
    ) -> Result<ActionPlan, AkrepError> {
        if req.is_test_only() && !self.allow_test_actions {
            return Err(AkrepError::ForbiddenOperation(
                "Test-only actions are disabled on production D-Bus".to_string(),
            ));
        }

        let polkit_action = req.required_polkit_action();
        let caller = match self
            .authorizer
            .verify_caller(caller_bus_name, polkit_action, true)
            .await
        {
            Ok(c) => c,
            Err(err) => {
                let _ = self.audit.log(
                    "plan_denied",
                    None,
                    None,
                    None,
                    Some(caller_bus_name),
                    Some(polkit_action),
                    "denied",
                    err.message(),
                );
                return Err(err.redacted());
            }
        };

        if let Err(err) = req.validate_red_lines() {
            let _ = self.audit.log(
                "plan_rejected_redline",
                None,
                None,
                Some(caller.uid),
                Some(&caller.bus_name),
                Some(polkit_action),
                "rejected",
                err.message(),
            );
            return Err(err.redacted());
        }

        let preview = self.backend.preview(&req).await.map_err(|e| e.redacted())?;
        let plan = ActionPlan::new_signed(
            caller.uid,
            req,
            preview.risk,
            preview.diffs,
            preview.pre_fingerprints,
            preview.warnings_tr,
            preview.warnings_en,
        )?;

        let plan_path = self.plans_dir().join(format!("{}.json", plan.plan_id));
        write_json_atomic_0600(&plan_path, &plan)?;
        self.audit.log(
            "plan_created",
            None,
            Some(&plan.plan_id),
            Some(caller.uid),
            Some(&caller.bus_name),
            Some(&plan.polkit_action),
            "ok",
            "ActionPlan staged and signed",
        )?;

        Ok(plan)
    }

    fn is_plan_already_consumed(&self, plan_id: &str) -> Result<bool, AkrepError> {
        let tx_dir = self.transactions_dir();
        if !tx_dir.exists() {
            return Ok(false);
        }
        for entry in fs::read_dir(&tx_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let rec: TransactionRecord = read_json_file(&path)?;
            if rec.plan_id == plan_id {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn commit_plan(
        &self,
        caller_bus_name: &str,
        plan_id: &str,
        expected_plan_sha256: &str,
    ) -> Result<TransactionRecord, AkrepError> {
        validate_record_id("plan_id", plan_id)?;
        let plan_path = self.plans_dir().join(format!("{}.json", plan_id));
        if !plan_path.exists() {
            return Err(AkrepError::InvalidInput(format!(
                "Unknown plan_id '{}'",
                plan_id
            )));
        }

        let plan: ActionPlan = read_json_file(&plan_path)?;
        if plan.request.is_test_only() && !self.allow_test_actions {
            return Err(AkrepError::ForbiddenOperation(
                "Test-only actions are disabled on production D-Bus".to_string(),
            ));
        }

        let required_action = plan.request.required_polkit_action();
        if plan.polkit_action != required_action {
            return Err(AkrepError::PlanMismatch(
                "Stored plan polkit_action does not match request requirement".to_string(),
            ));
        }

        let caller = match self
            .authorizer
            .verify_caller(caller_bus_name, required_action, true)
            .await
        {
            Ok(c) => c,
            Err(err) => {
                let _ = self.audit.log(
                    "commit_denied",
                    None,
                    Some(plan_id),
                    None,
                    Some(caller_bus_name),
                    Some(required_action),
                    "denied",
                    err.message(),
                );
                return Err(err.redacted());
            }
        };

        if caller.uid != plan.created_by_uid {
            let msg = format!(
                "Plan '{}' was created by uid {} and cannot be committed by uid {}",
                plan_id, plan.created_by_uid, caller.uid
            );
            let _ = self.audit.log(
                "commit_uid_mismatch",
                None,
                Some(plan_id),
                Some(caller.uid),
                Some(&caller.bus_name),
                Some(required_action),
                "denied",
                &msg,
            );
            return Err(AkrepError::Unauthorized(msg));
        }

        plan.request.validate_red_lines()?;
        plan.verify_hash(expected_plan_sha256)?;

        let expires_at = DateTime::parse_from_rfc3339(&plan.expires_at)
            .map_err(|_| {
                AkrepError::PlanMismatch("Malformed plan expires_at timestamp".to_string())
            })?
            .with_timezone(&Utc);
        if Utc::now() > expires_at {
            let _ = self.audit.log(
                "commit_expired",
                None,
                Some(plan_id),
                Some(caller.uid),
                Some(&caller.bus_name),
                Some(required_action),
                "expired",
                "ActionPlan TTL expired before CommitPlan",
            );
            return Err(AkrepError::PlanExpired(format!(
                "ActionPlan '{}' has expired",
                plan_id
            )));
        }

        let _tx_lock = FlockGuard::try_acquire_exclusive(&self.lock_path())?;

        if self.is_plan_already_consumed(plan_id)? {
            return Err(AkrepError::PlanMismatch(format!(
                "ActionPlan '{}' has already been consumed and cannot be replayed",
                plan_id
            )));
        }

        let tx_id = generate_uuid_v4()?;
        let tx_path = self.transactions_dir().join(format!("{}.json", tx_id));
        let snapshot_dir = self.snapshots_dir().join(&tx_id);
        ensure_dir_0700(&snapshot_dir)?;

        let now = Utc::now().to_rfc3339();
        let mut record = TransactionRecord {
            tx_id: tx_id.clone(),
            plan_id: plan.plan_id.clone(),
            plan_sha256: plan.plan_sha256.clone(),
            actor_uid: caller.uid,
            actor_bus_name: caller.bus_name.clone(),
            polkit_action: plan.polkit_action.clone(),
            polkit_authorized: true,
            request: plan.request.clone(),
            state: TransactionState::Prepared,
            pre_snapshot: serde_json::Value::Null,
            target_state: serde_json::Value::Null,
            pre_fingerprints: plan.pre_fingerprints.clone(),
            post_fingerprints: Vec::new(),
            apply_result: None,
            rollback_result: None,
            error_message: None,
            created_at: now.clone(),
            updated_at: now,
            completed_at: None,
        };

        write_json_atomic_0600(&tx_path, &record)?;

        record.state = TransactionState::Applying;
        record.updated_at = Utc::now().to_rfc3339();
        write_json_atomic_0600(&tx_path, &record)?;

        let apply_outcome = match self.backend.apply(&record.request, &snapshot_dir).await {
            Ok(outcome) => outcome,
            Err(apply_err) => {
                let clean_err = apply_err.redacted();
                return Err(self
                    .execute_automatic_rollback(&mut record, &tx_path, &snapshot_dir, clean_err)
                    .await);
            }
        };

        record.pre_snapshot = apply_outcome.pre_snapshot.clone();
        record.target_state = apply_outcome.target_state.clone();
        record.apply_result = Some(redact_secrets(&apply_outcome.summary));
        record.state = TransactionState::Verifying;
        record.updated_at = Utc::now().to_rfc3339();
        write_json_atomic_0600(&tx_path, &record)?;

        match self.backend.verify(&record.request, &apply_outcome).await {
            Ok(post_fps) => {
                let done_at = Utc::now().to_rfc3339();
                record.post_fingerprints = post_fps;
                record.state = TransactionState::Committed;
                record.updated_at = done_at.clone();
                record.completed_at = Some(done_at);
                write_json_atomic_0600(&tx_path, &record)?;
                self.audit.log(
                    "transaction_committed",
                    Some(&record.tx_id),
                    Some(&record.plan_id),
                    Some(record.actor_uid),
                    Some(&record.actor_bus_name),
                    Some(&record.polkit_action),
                    "committed",
                    record.apply_result.as_deref().unwrap_or("committed"),
                )?;
                Ok(record)
            }
            Err(verify_err) => {
                let clean_err = match verify_err {
                    AkrepError::VerifyFailed(m) => AkrepError::VerifyFailed(redact_secrets(&m)),
                    other => AkrepError::VerifyFailed(redact_secrets(other.message())),
                };
                Err(self
                    .execute_automatic_rollback(&mut record, &tx_path, &snapshot_dir, clean_err)
                    .await)
            }
        }
    }

    async fn execute_automatic_rollback(
        &self,
        record: &mut TransactionRecord,
        tx_path: &Path,
        snapshot_dir: &Path,
        cause: AkrepError,
    ) -> AkrepError {
        let redacted_cause = redact_secrets(cause.message());
        record.state = TransactionState::RollingBack;
        record.error_message = Some(redacted_cause.clone());
        record.updated_at = Utc::now().to_rfc3339();
        let _ = write_json_atomic_0600(tx_path, record);

        match self.backend.rollback(record, snapshot_dir).await {
            Ok(rb_summary) => {
                let done_at = Utc::now().to_rfc3339();
                let clean_rb = redact_secrets(&rb_summary);
                record.state = TransactionState::RolledBack;
                record.rollback_result = Some(clean_rb.clone());
                record.updated_at = done_at.clone();
                record.completed_at = Some(done_at);
                let _ = write_json_atomic_0600(tx_path, record);
                let _ = self.audit.log(
                    "transaction_rolled_back",
                    Some(&record.tx_id),
                    Some(&record.plan_id),
                    Some(record.actor_uid),
                    Some(&record.actor_bus_name),
                    Some(&record.polkit_action),
                    "rolled_back",
                    &format!("cause={}; rollback={}", redacted_cause, clean_rb),
                );
                match cause {
                    AkrepError::VerifyFailed(_) => AkrepError::VerifyFailed(redacted_cause),
                    _ => AkrepError::ApplyFailed(redacted_cause),
                }
            }
            Err(rb_err) => {
                let done_at = Utc::now().to_rfc3339();
                let clean_rb_err = redact_secrets(rb_err.message());
                record.state = TransactionState::RollbackFailed;
                record.rollback_result = Some(clean_rb_err.clone());
                record.updated_at = done_at.clone();
                record.completed_at = Some(done_at);
                let _ = write_json_atomic_0600(tx_path, record);
                let _ = self.audit.log(
                    "transaction_rollback_failed",
                    Some(&record.tx_id),
                    Some(&record.plan_id),
                    Some(record.actor_uid),
                    Some(&record.actor_bus_name),
                    Some(&record.polkit_action),
                    "rollback_failed",
                    &format!("cause={}; rollback_error={}", redacted_cause, clean_rb_err),
                );
                AkrepError::RollbackFailed(format!(
                    "operation failed ({}) and rollback failed ({})",
                    redacted_cause, clean_rb_err
                ))
            }
        }
    }

    pub async fn rollback_transaction(
        &self,
        caller_bus_name: &str,
        tx_id: &str,
    ) -> Result<TransactionRecord, AkrepError> {
        validate_record_id("tx_id", tx_id)?;
        let tx_path = self.transactions_dir().join(format!("{}.json", tx_id));
        if !tx_path.exists() {
            return Err(AkrepError::InvalidInput(format!(
                "Unknown tx_id '{}'",
                tx_id
            )));
        }

        let initial_record: TransactionRecord = read_json_file(&tx_path)?;
        let caller = self
            .authorizer
            .verify_caller(caller_bus_name, &initial_record.polkit_action, true)
            .await
            .map_err(|e| e.redacted())?;

        let _tx_lock = FlockGuard::try_acquire_exclusive(&self.lock_path())?;
        let mut record: TransactionRecord = read_json_file(&tx_path)?;

        if record.state != TransactionState::Committed {
            return Err(AkrepError::InvalidInput(format!(
                "Transaction '{}' is in state {:?}; only Committed transactions can be rolled back",
                tx_id, record.state
            )));
        }

        let current_fps = self
            .backend
            .current_fingerprints(&record.request, &record.post_fingerprints)
            .await
            .map_err(|e| e.redacted())?;

        if !fingerprints_match_exactly(&record.post_fingerprints, &current_fps) {
            let msg = format!(
                "Underlying state for transaction '{}' has drifted since commit; refusing blind rollback",
                tx_id
            );
            let _ = self.audit.log(
                "external_drift_detected",
                Some(&record.tx_id),
                Some(&record.plan_id),
                Some(caller.uid),
                Some(&caller.bus_name),
                Some(&record.polkit_action),
                "blocked",
                &msg,
            );
            return Err(AkrepError::ExternalDriftDetected(msg));
        }

        let snapshot_dir = self.snapshots_dir().join(&record.tx_id);
        record.state = TransactionState::RollingBack;
        record.updated_at = Utc::now().to_rfc3339();
        write_json_atomic_0600(&tx_path, &record)?;

        match self.backend.rollback(&record, &snapshot_dir).await {
            Ok(rb_msg) => {
                let done_at = Utc::now().to_rfc3339();
                let clean_rb = redact_secrets(&rb_msg);
                record.state = TransactionState::RolledBack;
                record.rollback_result = Some(clean_rb.clone());
                record.updated_at = done_at.clone();
                record.completed_at = Some(done_at);
                write_json_atomic_0600(&tx_path, &record)?;
                self.audit.log(
                    "transaction_rolled_back_manual",
                    Some(&record.tx_id),
                    Some(&record.plan_id),
                    Some(caller.uid),
                    Some(&caller.bus_name),
                    Some(&record.polkit_action),
                    "rolled_back",
                    &clean_rb,
                )?;
                Ok(record)
            }
            Err(rb_err) => {
                let done_at = Utc::now().to_rfc3339();
                let clean_err = redact_secrets(rb_err.message());
                record.state = TransactionState::RollbackFailed;
                record.error_message = Some(clean_err.clone());
                record.updated_at = done_at.clone();
                record.completed_at = Some(done_at);
                write_json_atomic_0600(&tx_path, &record)?;
                let _ = self.audit.log(
                    "transaction_rollback_failed_manual",
                    Some(&record.tx_id),
                    Some(&record.plan_id),
                    Some(caller.uid),
                    Some(&caller.bus_name),
                    Some(&record.polkit_action),
                    "rollback_failed",
                    &clean_err,
                );
                Err(AkrepError::RollbackFailed(clean_err))
            }
        }
    }

    pub async fn get_transaction(
        &self,
        caller_bus_name: &str,
        tx_id: &str,
    ) -> Result<TransactionRecord, AkrepError> {
        validate_record_id("tx_id", tx_id)?;
        let tx_path = self.transactions_dir().join(format!("{}.json", tx_id));
        if !tx_path.exists() {
            return Err(AkrepError::InvalidInput(format!(
                "Unknown tx_id '{}'",
                tx_id
            )));
        }
        let record: TransactionRecord = read_json_file(&tx_path)?;
        self.authorizer
            .verify_caller(caller_bus_name, &record.polkit_action, true)
            .await
            .map_err(|e| e.redacted())?;
        Ok(record)
    }

    pub async fn get_history(
        &self,
        caller_bus_name: &str,
        limit: u32,
    ) -> Result<Vec<TransactionRecord>, AkrepError> {
        let mut authorized = false;
        for action in ALL_POLKIT_ACTIONS {
            if self
                .authorizer
                .verify_caller(caller_bus_name, action, false)
                .await
                .is_ok()
            {
                authorized = true;
                break;
            }
        }
        if !authorized {
            self.authorizer
                .verify_caller(caller_bus_name, POLKIT_PROFILE_APPLY, true)
                .await
                .map_err(|e| e.redacted())?;
        }

        self.list_records_sorted(limit as usize)
    }

    pub fn list_records_sorted(&self, limit: usize) -> Result<Vec<TransactionRecord>, AkrepError> {
        let tx_dir = self.transactions_dir();
        if !tx_dir.exists() || limit == 0 {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(&tx_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            records.push(read_json_file::<TransactionRecord>(&path)?);
        }
        records.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        records.truncate(limit.min(500));
        Ok(records)
    }

    pub async fn diagnose_selinux(
        &self,
        caller_bus_name: &str,
        query_json: &str,
    ) -> Result<String, AkrepError> {
        if query_json.len() > 32_768 || query_json.contains('\0') {
            return Err(AkrepError::InvalidInput(
                "Invalid SELinux diagnosis query payload".to_string(),
            ));
        }
        let caller = self
            .authorizer
            .verify_caller(caller_bus_name, POLKIT_SELINUX_DIAGNOSE, true)
            .await
            .map_err(|e| e.redacted())?;

        let _: serde_json::Value = serde_json::from_str(query_json).map_err(|e| {
            AkrepError::InvalidInput(format!(
                "Malformed SELinux diagnosis query JSON: {}",
                redact_secrets(&e.to_string())
            ))
        })?;

        let _ = self.audit.log(
            "selinux_diagnose_requested",
            None,
            None,
            Some(caller.uid),
            Some(&caller.bus_name),
            Some(POLKIT_SELINUX_DIAGNOSE),
            "unavailable",
            "SELinux diagnosis adapter stubbed in Sprint 0",
        );

        Err(AkrepError::BackendUnavailable(
            "SELinux diagnosis backend will be activated in Sprint 1".to_string(),
        ))
    }
}
