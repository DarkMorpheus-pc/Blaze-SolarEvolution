pub mod firewalld;
pub mod selinux;

pub use firewalld::*;
pub use selinux::*;

use crate::transaction_engine::{ApplyOutcome, MutationBackend, PreviewOutcome};
use async_trait::async_trait;
use blaze_akrep_common::{
    ActionRequest, AkrepError, FirewallStateSnapshot, SelinuxDiagnoseQuery,
    SelinuxDiagnoseResponse, StateFingerprint, TransactionRecord,
};
use std::path::Path;
use std::sync::Arc;
use zbus::Connection;

pub struct CompositeMutationBackend {
    selinux: Arc<SelinuxAdapter>,
    firewall: Arc<FirewallAdapter>,
}

impl CompositeMutationBackend {
    pub fn new(selinux: Arc<SelinuxAdapter>, firewall: Arc<FirewallAdapter>) -> Self {
        Self { selinux, firewall }
    }

    pub fn new_system(conn: Connection) -> Self {
        Self::new(
            Arc::new(SelinuxAdapter::new_linux()),
            Arc::new(FirewallAdapter::new_dbus(conn)),
        )
    }

    pub fn selinux(&self) -> &Arc<SelinuxAdapter> {
        &self.selinux
    }

    pub fn firewall(&self) -> &Arc<FirewallAdapter> {
        &self.firewall
    }
}

fn is_selinux_action(req: &ActionRequest) -> bool {
    matches!(
        req,
        ActionRequest::SelinuxSetBoolean { .. } | ActionRequest::SelinuxRestorecon { .. }
    )
}

fn is_firewall_action(req: &ActionRequest) -> bool {
    matches!(
        req,
        ActionRequest::FirewallAddPort { .. }
            | ActionRequest::FirewallRemovePort { .. }
            | ActionRequest::FirewallAddService { .. }
            | ActionRequest::FirewallRemoveService { .. }
            | ActionRequest::FirewallBindInterface { .. }
    )
}

#[async_trait]
impl MutationBackend for CompositeMutationBackend {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        if is_selinux_action(req) {
            return self.selinux.preview(req).await;
        }
        if is_firewall_action(req) {
            return self.firewall.preview(req).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 2-4".to_string(),
        ))
    }

    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        if is_selinux_action(req) {
            return self.selinux.apply(req, snapshot_dir).await;
        }
        if is_firewall_action(req) {
            return self.firewall.apply(req, snapshot_dir).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 2-4".to_string(),
        ))
    }

    async fn verify(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        if is_selinux_action(req) {
            return self.selinux.verify(req, expected).await;
        }
        if is_firewall_action(req) {
            return self.firewall.verify(req, expected).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 2-4".to_string(),
        ))
    }

    async fn current_fingerprints(
        &self,
        req: &ActionRequest,
        post_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        if is_selinux_action(req) {
            return self.selinux.current_fingerprints(req, post_fps).await;
        }
        if is_firewall_action(req) {
            return self.firewall.current_fingerprints(req, post_fps).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 2-4".to_string(),
        ))
    }

    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        if is_selinux_action(&record.request) {
            return self.selinux.rollback(record, snapshot_dir).await;
        }
        if is_firewall_action(&record.request) {
            return self.firewall.rollback(record, snapshot_dir).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 2-4".to_string(),
        ))
    }

    async fn diagnose_selinux(
        &self,
        query: &SelinuxDiagnoseQuery,
    ) -> Result<SelinuxDiagnoseResponse, AkrepError> {
        self.selinux.diagnose_selinux(query).await
    }

    async fn inspect_firewall(
        &self,
        zone_filter: Option<&str>,
    ) -> Result<FirewallStateSnapshot, AkrepError> {
        self.firewall.inspect_firewall(zone_filter).await
    }
}
