use crate::transaction_engine::TransactionEngine;
use blaze_akrep_common::AkrepError;
use blaze_sentinel_core::AKREP_VERSION;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct IdleTracker {
    epoch: Instant,
    last_activity_ms: AtomicU64,
    active_calls: AtomicUsize,
}

impl Default for IdleTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl IdleTracker {
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            last_activity_ms: AtomicU64::new(0),
            active_calls: AtomicUsize::new(0),
        }
    }

    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }

    pub fn touch(&self) {
        self.last_activity_ms.store(self.now_ms(), Ordering::SeqCst);
    }

    pub fn begin_call(self: &Arc<Self>) -> ActiveCallGuard {
        self.active_calls.fetch_add(1, Ordering::SeqCst);
        self.touch();
        ActiveCallGuard {
            tracker: Arc::clone(self),
        }
    }

    pub fn active_calls(&self) -> usize {
        self.active_calls.load(Ordering::SeqCst)
    }

    pub fn idle_elapsed(&self) -> Duration {
        let last = self.last_activity_ms.load(Ordering::SeqCst);
        let now = self.now_ms();
        Duration::from_millis(now.saturating_sub(last))
    }

    pub fn is_idle_expired(&self, timeout: Duration) -> bool {
        self.active_calls() == 0 && self.idle_elapsed() >= timeout
    }
}

pub struct ActiveCallGuard {
    tracker: Arc<IdleTracker>,
}

impl Drop for ActiveCallGuard {
    fn drop(&mut self) {
        self.tracker.touch();
        self.tracker.active_calls.fetch_sub(1, Ordering::SeqCst);
    }
}

pub fn map_akrep_error_to_fdo(err: AkrepError) -> zbus::fdo::Error {
    let payload = err.to_redacted_json();
    match err {
        AkrepError::Unauthorized(_) | AkrepError::PolkitDenied(_) => {
            zbus::fdo::Error::AccessDenied(payload)
        }
        AkrepError::InvalidInput(_)
        | AkrepError::ForbiddenOperation(_)
        | AkrepError::PlanMismatch(_)
        | AkrepError::PlanExpired(_) => zbus::fdo::Error::InvalidArgs(payload),
        AkrepError::BackendUnavailable(_) => zbus::fdo::Error::NotSupported(payload),
        AkrepError::TransactionConflict(_)
        | AkrepError::ExternalDriftDetected(_)
        | AkrepError::ApplyFailed(_)
        | AkrepError::VerifyFailed(_)
        | AkrepError::RollbackFailed(_)
        | AkrepError::IoError(_)
        | AkrepError::Internal(_) => zbus::fdo::Error::Failed(payload),
    }
}

pub fn extract_caller_bus_name(header: &zbus::message::Header<'_>) -> zbus::fdo::Result<String> {
    let Some(sender) = header.sender() else {
        return Err(map_akrep_error_to_fdo(AkrepError::Unauthorized(
            "Missing D-Bus message sender header".to_string(),
        )));
    };
    Ok(sender.as_str().to_string())
}

pub struct PrivilegedManager {
    engine: Arc<TransactionEngine>,
    idle: Arc<IdleTracker>,
}

impl PrivilegedManager {
    pub fn new(engine: Arc<TransactionEngine>, idle: Arc<IdleTracker>) -> Self {
        Self { engine, idle }
    }
}

#[zbus::interface(name = "tr.com.blazeos.Akrep1.PrivilegedManager")]
impl PrivilegedManager {
    async fn get_version(&self) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        Ok(AKREP_VERSION.to_string())
    }

    async fn create_plan(
        &self,
        request_json: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        let caller = extract_caller_bus_name(&header)?;
        let plan = self
            .engine
            .create_plan_from_json(&caller, request_json)
            .await
            .map_err(map_akrep_error_to_fdo)?;
        serde_json::to_string(&plan).map_err(|e| {
            map_akrep_error_to_fdo(AkrepError::Internal(format!("plan serialize error: {}", e)))
        })
    }

    async fn commit_plan(
        &self,
        plan_id: &str,
        plan_sha256: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        let caller = extract_caller_bus_name(&header)?;
        let record = self
            .engine
            .commit_plan(&caller, plan_id, plan_sha256)
            .await
            .map_err(map_akrep_error_to_fdo)?;
        serde_json::to_string(&record).map_err(|e| {
            map_akrep_error_to_fdo(AkrepError::Internal(format!(
                "transaction serialize error: {}",
                e
            )))
        })
    }

    async fn get_transaction(
        &self,
        tx_id: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        let caller = extract_caller_bus_name(&header)?;
        let record = self
            .engine
            .get_transaction(&caller, tx_id)
            .await
            .map_err(map_akrep_error_to_fdo)?;
        serde_json::to_string(&record).map_err(|e| {
            map_akrep_error_to_fdo(AkrepError::Internal(format!(
                "transaction serialize error: {}",
                e
            )))
        })
    }

    async fn rollback_transaction(
        &self,
        tx_id: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        let caller = extract_caller_bus_name(&header)?;
        let record = self
            .engine
            .rollback_transaction(&caller, tx_id)
            .await
            .map_err(map_akrep_error_to_fdo)?;
        serde_json::to_string(&record).map_err(|e| {
            map_akrep_error_to_fdo(AkrepError::Internal(format!(
                "transaction serialize error: {}",
                e
            )))
        })
    }

    async fn get_history(
        &self,
        limit: u32,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        let caller = extract_caller_bus_name(&header)?;
        let history = self
            .engine
            .get_history(&caller, limit)
            .await
            .map_err(map_akrep_error_to_fdo)?;
        serde_json::to_string(&history).map_err(|e| {
            map_akrep_error_to_fdo(AkrepError::Internal(format!(
                "history serialize error: {}",
                e
            )))
        })
    }

    async fn diagnose_selinux(
        &self,
        query_json: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        let caller = extract_caller_bus_name(&header)?;
        self.engine
            .diagnose_selinux(&caller, query_json)
            .await
            .map_err(map_akrep_error_to_fdo)
    }

    async fn inspect_firewall(
        &self,
        zone_filter: &str,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        let caller = extract_caller_bus_name(&header)?;
        self.engine
            .inspect_firewall(&caller, zone_filter)
            .await
            .map_err(map_akrep_error_to_fdo)
    }

    async fn inspect_system_flatpaks(
        &self,
        #[zbus(header)] header: zbus::message::Header<'_>,
    ) -> zbus::fdo::Result<String> {
        let _guard = self.idle.begin_call();
        let caller = extract_caller_bus_name(&header)?;
        self.engine
            .inspect_system_flatpaks(&caller)
            .await
            .map_err(map_akrep_error_to_fdo)
    }
}
