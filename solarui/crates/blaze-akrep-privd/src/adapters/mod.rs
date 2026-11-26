pub mod device_security;
pub mod firewalld;
pub mod flatpak;
pub mod overrides;
pub mod profiles;
pub mod quarantine;
pub mod scanner;
pub mod selinux;

pub use device_security::*;
pub use firewalld::*;
pub use flatpak::*;
pub use overrides::*;
pub use profiles::*;
pub use quarantine::*;
pub use scanner::*;
pub use selinux::*;

use crate::transaction_engine::{
    ApplyOutcome, MutationBackend, PreviewOutcome, DEFAULT_AKREP_STATE_DIR,
};
use async_trait::async_trait;
use blaze_akrep_common::{
    ActionRequest, AkrepError, DeviceSecurityStatus, FirewallStateSnapshot,
    FlatpakInstallationSnapshot, FlatpakScope, ProfileCatalogResponse, QuarantineInventory,
    ScanProgress, ScanRequest, ScannerStatusReport, SelinuxDiagnoseQuery, SelinuxDiagnoseResponse,
    StateFingerprint, TransactionRecord,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zbus::Connection;

pub struct CompositeMutationBackend {
    selinux: Arc<SelinuxAdapter>,
    firewall: Arc<FirewallAdapter>,
    flatpak: Arc<SystemFlatpakAdapter>,
    device_security: Arc<DeviceSecurityAdapter>,
    scanner: Arc<ScannerAdapter>,
    quarantine: Arc<QuarantineAdapter>,
    profiles: Arc<ProfileAdapter>,
}

impl CompositeMutationBackend {
    pub fn new(selinux: Arc<SelinuxAdapter>, firewall: Arc<FirewallAdapter>) -> Self {
        Self::with_flatpak(
            selinux,
            firewall,
            Arc::new(SystemFlatpakAdapter::new_system()),
        )
    }

    pub fn with_flatpak(
        selinux: Arc<SelinuxAdapter>,
        firewall: Arc<FirewallAdapter>,
        flatpak: Arc<SystemFlatpakAdapter>,
    ) -> Self {
        let state_dir = PathBuf::from(DEFAULT_AKREP_STATE_DIR);
        let profiles = Arc::new(ProfileAdapter::new_system(
            state_dir.clone(),
            Arc::clone(&selinux),
            Arc::clone(&firewall),
        ));
        Self {
            selinux,
            firewall,
            flatpak,
            device_security: Arc::new(DeviceSecurityAdapter::new_linux()),
            scanner: Arc::new(ScannerAdapter::new_system(state_dir.clone())),
            quarantine: Arc::new(QuarantineAdapter::new_lazy(state_dir)),
            profiles,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_full(
        selinux: Arc<SelinuxAdapter>,
        firewall: Arc<FirewallAdapter>,
        flatpak: Arc<SystemFlatpakAdapter>,
        device_security: Arc<DeviceSecurityAdapter>,
        scanner: Arc<ScannerAdapter>,
        quarantine: Arc<QuarantineAdapter>,
        profiles: Arc<ProfileAdapter>,
    ) -> Self {
        Self {
            selinux,
            firewall,
            flatpak,
            device_security,
            scanner,
            quarantine,
            profiles,
        }
    }

    pub fn new_system(conn: Connection) -> Self {
        Self::new_system_with_state_dir(conn, PathBuf::from(DEFAULT_AKREP_STATE_DIR))
    }

    pub fn new_system_with_state_dir(conn: Connection, state_dir: PathBuf) -> Self {
        let selinux = Arc::new(SelinuxAdapter::new_linux());
        let firewall = Arc::new(FirewallAdapter::new_dbus(conn));
        let flatpak = Arc::new(SystemFlatpakAdapter::new_system());
        let device_security = Arc::new(DeviceSecurityAdapter::new_linux());
        let scanner = Arc::new(ScannerAdapter::new_system(state_dir.clone()));
        let quarantine = Arc::new(QuarantineAdapter::new_lazy(state_dir.clone()));
        let profiles = Arc::new(ProfileAdapter::new_system(
            state_dir,
            Arc::clone(&selinux),
            Arc::clone(&firewall),
        ));
        Self::new_full(
            selinux,
            firewall,
            flatpak,
            device_security,
            scanner,
            quarantine,
            profiles,
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

    pub fn device_security(&self) -> &Arc<DeviceSecurityAdapter> {
        &self.device_security
    }

    pub fn scanner(&self) -> &Arc<ScannerAdapter> {
        &self.scanner
    }

    pub fn quarantine(&self) -> &Arc<QuarantineAdapter> {
        &self.quarantine
    }

    pub fn profiles(&self) -> &Arc<ProfileAdapter> {
        &self.profiles
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

fn is_quarantine_action(req: &ActionRequest) -> bool {
    matches!(
        req,
        ActionRequest::QuarantineFile { .. }
            | ActionRequest::RestoreQuarantine { .. }
            | ActionRequest::DeleteQuarantine { .. }
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
        if is_quarantine_action(req) {
            return self.quarantine.preview(req).await;
        }
        if matches!(req, ActionRequest::InstallOptionalScanner) {
            return self.scanner.preview(req).await;
        }
        if matches!(req, ActionRequest::ApplyProfile { .. }) {
            return self.profiles.preview(req).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Unsupported ActionRequest on CompositeMutationBackend".to_string(),
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
        if is_quarantine_action(req) {
            return self.quarantine.apply(req, snapshot_dir).await;
        }
        if matches!(req, ActionRequest::InstallOptionalScanner) {
            return self.scanner.apply(req, snapshot_dir).await;
        }
        if matches!(req, ActionRequest::ApplyProfile { .. }) {
            return self.profiles.apply(req, snapshot_dir).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Unsupported ActionRequest on CompositeMutationBackend".to_string(),
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
        if is_quarantine_action(req) {
            return self.quarantine.verify(req, expected).await;
        }
        if matches!(req, ActionRequest::InstallOptionalScanner) {
            return self.scanner.verify(req, expected).await;
        }
        if matches!(req, ActionRequest::ApplyProfile { .. }) {
            return self.profiles.verify(req, expected).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Unsupported ActionRequest on CompositeMutationBackend".to_string(),
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
        if is_quarantine_action(req) {
            return self.quarantine.current_fingerprints(req, post_fps).await;
        }
        if matches!(req, ActionRequest::InstallOptionalScanner) {
            return self.scanner.current_fingerprints(req, post_fps).await;
        }
        if matches!(req, ActionRequest::ApplyProfile { .. }) {
            return self.profiles.current_fingerprints(req, post_fps).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Unsupported ActionRequest on CompositeMutationBackend".to_string(),
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
        if is_quarantine_action(&record.request) {
            return self.quarantine.rollback(record, snapshot_dir).await;
        }
        if matches!(record.request, ActionRequest::InstallOptionalScanner) {
            return self.scanner.rollback(record, snapshot_dir).await;
        }
        if matches!(record.request, ActionRequest::ApplyProfile { .. }) {
            return self.profiles.rollback(record, snapshot_dir).await;
        }
        Err(AkrepError::BackendUnavailable(
            "Unsupported ActionRequest on CompositeMutationBackend".to_string(),
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

    async fn inspect_device_security(&self) -> Result<DeviceSecurityStatus, AkrepError> {
        self.device_security.inspect_device_security().await
    }

    async fn inspect_scanner(&self) -> Result<ScannerStatusReport, AkrepError> {
        let scanner = Arc::clone(&self.scanner);
        tokio::task::spawn_blocking(move || scanner.inspect_status())
            .await
            .map_err(|e| AkrepError::Internal(format!("scanner inspect join error: {}", e)))?
    }

    async fn start_scan(
        &self,
        req: ScanRequest,
        actor_uid: u32,
    ) -> Result<ScanProgress, AkrepError> {
        self.scanner.start_scan(req, actor_uid).await
    }

    async fn get_scan_status(&self, job_id: &str) -> Result<ScanProgress, AkrepError> {
        self.scanner.get_scan_status(job_id)
    }

    async fn cancel_scan(&self, job_id: &str) -> Result<ScanProgress, AkrepError> {
        self.scanner.cancel_scan(job_id)
    }

    async fn list_quarantine(&self) -> Result<QuarantineInventory, AkrepError> {
        let q = Arc::clone(&self.quarantine);
        tokio::task::spawn_blocking(move || q.list_inventory())
            .await
            .map_err(|e| AkrepError::Internal(format!("quarantine list join error: {}", e)))?
    }

    async fn list_profiles(&self) -> Result<ProfileCatalogResponse, AkrepError> {
        let p = Arc::clone(&self.profiles);
        tokio::task::spawn_blocking(move || p.list_profiles())
            .await
            .map_err(|e| AkrepError::Internal(format!("profiles list join error: {}", e)))?
    }

    fn has_active_background_jobs(&self) -> bool {
        self.scanner.has_active_scans()
    }
}
