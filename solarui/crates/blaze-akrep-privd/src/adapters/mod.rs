pub mod firewalld;
pub mod flatpak;
pub mod selinux;

pub use firewalld::*;
pub use flatpak::*;
pub use selinux::*;

use crate::transaction_engine::{ApplyOutcome, MutationBackend, PreviewOutcome};
use async_trait::async_trait;
use blaze_akrep_common::{
    ActionRequest, AkrepError, FirewallStateSnapshot, FlatpakInstallationSnapshot, FlatpakScope,
    SelinuxDiagnoseQuery, SelinuxDiagnoseResponse, StateFingerprint, TransactionRecord,
};
use std::path::Path;
use std::sync::Arc;
use zbus::Connection;

pub struct CompositeMutationBackend {
    selinux: Arc<SelinuxAdapter>,
    firewall: Arc<FirewallAdapter>,
    flatpak: Arc<SystemFlatpakAdapter>,
}

impl CompositeMutationBackend {
    pub fn new(selinux: Arc<SelinuxAdapter>, firewall: Arc<FirewallAdapter>) -> Self {
        Self {
            selinux,
            firewall,
            flatpak: Arc::new(SystemFlatpakAdapter::new_system()),
        }
    }

    pub fn with_flatpak(
        selinux: Arc<SelinuxAdapter>,
        firewall: Arc<FirewallAdapter>,
        flatpak: Arc<SystemFlatpakAdapter>,
    ) -> Self {
        Self {
            selinux,
            firewall,
            flatpak,
        }
    }

    pub fn new_system(conn: Connection) -> Self {
        Self::with_flatpak(
            Arc::new(SelinuxAdapter::new_linux()),
            Arc::new(FirewallAdapter::new_dbus(conn)),
            Arc::new(SystemFlatpakAdapter::new_system()),
        )
    }

    pub fn selinux(&self) -> &Arc<SelinuxAdapter> {
        &self.selinux
    }

    pub fn firewall(&self) -> &Arc<FirewallAdapter> {
        &self.firewall
    }

    pub fn flatpak(&self) -> &Arc<SystemFlatpakAdapter> {
        &self.flatpak
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
        if let ActionRequest::FlatpakSetOverride { scope, .. } = req {
            if *scope == FlatpakScope::User {
                return Err(AkrepError::ForbiddenOperation(
                    USER_SCOPE_PRIVD_REJECTION_MSG.to_string(),
                ));
            }
            return self.flatpak.preview(req).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 3-4".to_string(),
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
        if let ActionRequest::FlatpakSetOverride { scope, .. } = req {
            if *scope == FlatpakScope::User {
                return Err(AkrepError::ForbiddenOperation(
                    USER_SCOPE_PRIVD_REJECTION_MSG.to_string(),
                ));
            }
            return self.flatpak.apply(req, snapshot_dir).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 3-4".to_string(),
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
        if let ActionRequest::FlatpakSetOverride { scope, .. } = req {
            if *scope == FlatpakScope::User {
                return Err(AkrepError::ForbiddenOperation(
                    USER_SCOPE_PRIVD_REJECTION_MSG.to_string(),
                ));
            }
            return self.flatpak.verify(req, expected).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 3-4".to_string(),
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
        if let ActionRequest::FlatpakSetOverride { scope, .. } = req {
            if *scope == FlatpakScope::User {
                return Err(AkrepError::ForbiddenOperation(
                    USER_SCOPE_PRIVD_REJECTION_MSG.to_string(),
                ));
            }
            return self.flatpak.current_fingerprints(req, post_fps).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 3-4".to_string(),
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
        if let ActionRequest::FlatpakSetOverride { scope, .. } = &record.request {
            if *scope == FlatpakScope::User {
                return Err(AkrepError::ForbiddenOperation(
                    USER_SCOPE_PRIVD_REJECTION_MSG.to_string(),
                ));
            }
            return self.flatpak.rollback(record, snapshot_dir).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Requested security domain adapter is scheduled for Sprint 3-4".to_string(),
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

    async fn inspect_system_flatpaks(&self) -> Result<FlatpakInstallationSnapshot, AkrepError> {
        self.flatpak.inspect_system_flatpaks().await
    }
}
