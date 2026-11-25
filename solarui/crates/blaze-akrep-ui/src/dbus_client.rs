use blaze_akrep_common::{
    redact_secrets, ActionPlan, ActionRequest, AkrepError, DeviceSecurityStatus,
    FirewallStateSnapshot, FlatpakInstallationSnapshot, PortalPrivacyAction,
    PrivacySessionController, PrivacySessionSnapshot, ProfileCatalogResponse, QuarantineInventory,
    ScanJobState, ScanProgress, ScanRequest, ScannerStatusReport, SelinuxDiagnoseQuery,
    SelinuxDiagnoseResponse, SessionPortalPipewireBackend, TransactionRecord, UnifiedAppInventory,
    UserFlatpakManager, UserOverrideTransaction, AKREP_PRIV_BUS_NAME, AKREP_PRIV_INTERFACE_NAME,
    AKREP_PRIV_OBJECT_PATH,
};
use blaze_sentinel_core::AkrepSnapshot;
use futures_util::stream::StreamExt;
use serde::de::DeserializeOwned;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock};
use zbus::{Connection, MatchRule, MessageStream};

pub const AKREP_BUS_NAME: &str = "tr.com.blazeos.Akrep1";
pub const AKREP_OBJECT_PATH: &str = "/tr/com/blazeos/Akrep1";
pub const AKREP_INTERFACE_NAME: &str = "tr.com.blazeos.Akrep1.Manager";

#[derive(Debug, Clone)]
pub enum UiCommand {
    Refresh,
    CreatePlan {
        request: ActionRequest,
    },
    PreviewProfile {
        profile_id: String,
    },
    CommitPlan {
        plan_id: String,
        plan_sha256: String,
    },
    RollbackTransaction {
        tx_id: String,
    },
    LoadHistory {
        limit: u32,
    },
    DiagnoseSelinux {
        query: SelinuxDiagnoseQuery,
    },
    InspectFirewall {
        zone_filter: String,
    },
    InspectSystemFlatpaks,
    InspectDeviceSecurity,
    InspectScanner,
    StartScan {
        request: ScanRequest,
    },
    CancelScan {
        job_id: String,
    },
    ListQuarantine,
    ListProfiles,
    QueryPrivVersion,
    RefreshUserApplications,
    ApplyUserFlatpakOverride {
        app_id: String,
        additions: Vec<(String, String)>,
        removals: Vec<(String, String)>,
    },
    RollbackUserFlatpakTx {
        tx_id: String,
    },
    RefreshPrivacySession,
    TerminatePipewireStream {
        node_id: u32,
    },
    RevokePortalPermission {
        table: String,
        resource_id: String,
        app_id: String,
    },
    Stop,
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    Snapshot(Box<AkrepSnapshot>),
    DaemonOffline(String),
    PlanStaged {
        plan: Box<ActionPlan>,
        is_profile_preview: bool,
    },
    TransactionCommitted(Box<TransactionRecord>),
    TransactionRolledBack(Box<TransactionRecord>),
    HistoryLoaded(Vec<TransactionRecord>),
    SelinuxDiagnosed(Box<SelinuxDiagnoseResponse>),
    FirewallInspected(Box<FirewallStateSnapshot>),
    SystemFlatpaksInspected(Box<FlatpakInstallationSnapshot>),
    DeviceSecurityInspected(Box<DeviceSecurityStatus>),
    ScannerInspected(Box<ScannerStatusReport>),
    ScanProgressUpdated(Box<ScanProgress>),
    QuarantineLoaded(Box<QuarantineInventory>),
    ProfilesLoaded(Box<ProfileCatalogResponse>),
    PrivVersionLoaded(String),
    UserApplicationsLoaded {
        inventory: Box<UnifiedAppInventory>,
        user_txs: Vec<UserOverrideTransaction>,
    },
    UserFlatpakOverrideApplied(Box<UserOverrideTransaction>),
    UserFlatpakTxRolledBack(Box<UserOverrideTransaction>),
    PrivacySessionLoaded(Box<PrivacySessionSnapshot>),
    OperationInfo {
        tr: String,
        en: String,
    },
    OperationError {
        tr: String,
        en: String,
    },
}

/// Non-blocking UI command dispatcher safe to call from any GTK4 widget callback.
#[inline]
pub fn dispatch_cmd(cmd_tx: &async_channel::Sender<UiCommand>, cmd: UiCommand) {
    let _ = cmd_tx.try_send(cmd);
}

pub fn format_dbus_or_akrep_error(err: &zbus::Error) -> (String, String) {
    let raw = err.to_string();
    if let Some(start) = raw.find("{\"code\":") {
        if let Some(end) = raw[start..].rfind('}') {
            let slice = &raw[start..=start + end];
            if let Ok(parsed) = serde_json::from_str::<AkrepError>(slice) {
                let clean = redact_secrets(parsed.message());
                return match parsed {
                    AkrepError::PolkitDenied(_) | AkrepError::Unauthorized(_) => (
                        format!(
                            "Yönetici kimlik doğrulaması reddedildi veya iptal edildi: {}",
                            clean
                        ),
                        format!(
                            "Administrator authentication denied or cancelled: {}",
                            clean
                        ),
                    ),
                    AkrepError::BackendUnavailable(_) => (
                        format!(
                            "İlgili güvenlik bileşeni bu sistemde kullanılamıyor: {}",
                            clean
                        ),
                        format!(
                            "Requested security component is unavailable on this host: {}",
                            clean
                        ),
                    ),
                    AkrepError::ForbiddenOperation(_) => (
                        format!(
                            "Güvenlik kırmızı çizgi koruması işlemi engelledi: {}",
                            clean
                        ),
                        format!("Security red-line guardrail blocked operation: {}", clean),
                    ),
                    AkrepError::ExternalDriftDetected(_) => (
                        format!(
                            "Dış değişiklik (drift) algılandı; lütfen durumu yenileyip tekrar deneyin: {}",
                            clean
                        ),
                        format!(
                            "External state drift detected; please refresh and retry: {}",
                            clean
                        ),
                    ),
                    _ => (
                        format!("İşlem hatası ({}): {}", parsed.code(), clean),
                        format!("Operation error ({}): {}", parsed.code(), clean),
                    ),
                };
            }
        }
    }
    let clean = redact_secrets(&raw);
    (
        format!("D-Bus çağrısı başarısız oldu: {}", clean),
        format!("D-Bus call failed: {}", clean),
    )
}

pub fn format_akrep_error(err: &AkrepError) -> (String, String) {
    let clean = redact_secrets(err.message());
    match err {
        AkrepError::ForbiddenOperation(_) => (
            format!(
                "Güvenlik kırmızı çizgi koruması işlemi engelledi: {}",
                clean
            ),
            format!("Security red-line guardrail blocked operation: {}", clean),
        ),
        AkrepError::BackendUnavailable(_) => (
            format!("Oturum bileşeni kullanılamıyor: {}", clean),
            format!("Session backend unavailable: {}", clean),
        ),
        _ => (
            format!("Hata ({}): {}", err.code(), clean),
            format!("Error ({}): {}", err.code(), clean),
        ),
    }
}

async fn connect_bus() -> zbus::Result<Connection> {
    if std::env::var("BLAZE_AKREP_BUS").ok().as_deref() == Some("session") {
        Connection::session().await
    } else {
        Connection::system().await
    }
}

async fn get_or_connect_bus(conn_slot: &Arc<RwLock<Option<Connection>>>) -> Option<Connection> {
    if let Some(existing) = conn_slot.read().await.clone() {
        return Some(existing);
    }
    let mut write_guard = conn_slot.write().await;
    if let Some(existing) = write_guard.clone() {
        return Some(existing);
    }
    let fresh = connect_bus().await.ok();
    *write_guard = fresh.clone();
    fresh
}

pub async fn fetch_snapshot(conn: &Connection) -> Result<AkrepSnapshot, String> {
    let proxy = zbus::Proxy::new(
        conn,
        AKREP_BUS_NAME,
        AKREP_OBJECT_PATH,
        AKREP_INTERFACE_NAME,
    )
    .await
    .map_err(|e| {
        format!(
            "D-Bus proxy error ({}): {}",
            AKREP_BUS_NAME,
            redact_secrets(&e.to_string())
        )
    })?;
    let json_str: String = proxy.call("GetSnapshot", &()).await.map_err(|e| {
        format!(
            "{} unreachable: {}",
            AKREP_BUS_NAME,
            redact_secrets(&e.to_string())
        )
    })?;
    serde_json::from_str::<AkrepSnapshot>(&json_str).map_err(|e| {
        format!(
            "Snapshot JSON parse error: {}",
            redact_secrets(&e.to_string())
        )
    })
}

pub async fn call_priv_json<B, R>(
    conn: &Connection,
    method: &str,
    body: &B,
) -> Result<R, (String, String)>
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
    R: DeserializeOwned,
{
    let proxy = zbus::Proxy::new(
        conn,
        AKREP_PRIV_BUS_NAME,
        AKREP_PRIV_OBJECT_PATH,
        AKREP_PRIV_INTERFACE_NAME,
    )
    .await
    .map_err(|e| format_dbus_or_akrep_error(&e))?;
    let raw_json: String = proxy
        .call(method, body)
        .await
        .map_err(|e| format_dbus_or_akrep_error(&e))?;
    serde_json::from_str::<R>(&raw_json).map_err(|e| {
        let clean = redact_secrets(&e.to_string());
        (
            format!("{} yanıtı çözümlenemedi: {}", method, clean),
            format!("Failed to decode {} response: {}", method, clean),
        )
    })
}

async fn load_user_apps_blocking(
) -> Result<(UnifiedAppInventory, Vec<UserOverrideTransaction>), (String, String)> {
    tokio::task::spawn_blocking(|| {
        let mgr = UserFlatpakManager::from_xdg_env().map_err(|e| format_akrep_error(&e))?;
        let inv = mgr.inspect_unified().map_err(|e| format_akrep_error(&e))?;
        let txs = mgr.list_user_transactions(50).unwrap_or_default();
        Ok((inv, txs))
    })
    .await
    .map_err(|e| {
        (
            format!("Kullanıcı uygulama envanteri görev hatası: {}", e),
            format!("User application inventory task error: {}", e),
        )
    })?
}

async fn load_privacy_session() -> Result<PrivacySessionSnapshot, (String, String)> {
    let backend = SessionPortalPipewireBackend::connect_session()
        .await
        .map_err(|e| format_akrep_error(&e))?;
    let controller = PrivacySessionController::new(Arc::new(backend));
    controller
        .snapshot()
        .await
        .map_err(|e| format_akrep_error(&e))
}

async fn refresh_post_mutation_domains(
    conn: &Connection,
    req: &ActionRequest,
    evt_tx: &async_channel::Sender<UiEvent>,
) {
    if let Ok(snap) = fetch_snapshot(conn).await {
        let _ = evt_tx.send(UiEvent::Snapshot(Box::new(snap))).await;
    }
    match req {
        ActionRequest::SelinuxSetBoolean { .. } | ActionRequest::SelinuxRestorecon { .. } => {
            if let Ok(q_json) = serde_json::to_string(&SelinuxDiagnoseQuery::default()) {
                if let Ok(diag) = call_priv_json::<_, SelinuxDiagnoseResponse>(
                    conn,
                    "DiagnoseSelinux",
                    &(q_json.as_str(),),
                )
                .await
                {
                    let _ = evt_tx.send(UiEvent::SelinuxDiagnosed(Box::new(diag))).await;
                }
            }
        }
        ActionRequest::FirewallAddPort { .. }
        | ActionRequest::FirewallRemovePort { .. }
        | ActionRequest::FirewallAddService { .. }
        | ActionRequest::FirewallRemoveService { .. }
        | ActionRequest::FirewallBindInterface { .. } => {
            if let Ok(fw) =
                call_priv_json::<_, FirewallStateSnapshot>(conn, "InspectFirewall", &("",)).await
            {
                let _ = evt_tx.send(UiEvent::FirewallInspected(Box::new(fw))).await;
            }
        }
        ActionRequest::FlatpakSetOverride { .. } => {
            if let Ok((inv, txs)) = load_user_apps_blocking().await {
                let _ = evt_tx
                    .send(UiEvent::UserApplicationsLoaded {
                        inventory: Box::new(inv),
                        user_txs: txs,
                    })
                    .await;
            }
            if let Ok(sys_snap) =
                call_priv_json::<_, FlatpakInstallationSnapshot>(conn, "InspectSystemFlatpaks", &())
                    .await
            {
                let _ = evt_tx
                    .send(UiEvent::SystemFlatpaksInspected(Box::new(sys_snap)))
                    .await;
            }
        }
        ActionRequest::QuarantineFile { .. }
        | ActionRequest::RestoreQuarantine { .. }
        | ActionRequest::DeleteQuarantine { .. } => {
            if let Ok(q) =
                call_priv_json::<_, QuarantineInventory>(conn, "ListQuarantine", &()).await
            {
                let _ = evt_tx.send(UiEvent::QuarantineLoaded(Box::new(q))).await;
            }
        }
        ActionRequest::ApplyProfile { .. } => {
            if let Ok(cat) =
                call_priv_json::<_, ProfileCatalogResponse>(conn, "ListProfiles", &()).await
            {
                let _ = evt_tx.send(UiEvent::ProfilesLoaded(Box::new(cat))).await;
            }
            if let Ok(fw) =
                call_priv_json::<_, FirewallStateSnapshot>(conn, "InspectFirewall", &("",)).await
            {
                let _ = evt_tx.send(UiEvent::FirewallInspected(Box::new(fw))).await;
            }
        }
        ActionRequest::InstallOptionalScanner => {
            if let Ok(sc) =
                call_priv_json::<_, ScannerStatusReport>(conn, "InspectScanner", &()).await
            {
                let _ = evt_tx.send(UiEvent::ScannerInspected(Box::new(sc))).await;
            }
        }
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

async fn handle_ui_command(
    cmd: UiCommand,
    conn_slot: Arc<RwLock<Option<Connection>>>,
    active_scan_job: Arc<Mutex<Option<String>>>,
    evt_tx: async_channel::Sender<UiEvent>,
) {
    let send_no_bus = |evt_tx: &async_channel::Sender<UiEvent>| {
        let tx = evt_tx.clone();
        async move {
            let _ = tx
                .send(UiEvent::OperationError {
                    tr: "Sistem D-Bus bağlantısı yok.".to_string(),
                    en: "System D-Bus connection unavailable.".to_string(),
                })
                .await;
        }
    };

    match cmd {
        UiCommand::Stop => {}
        UiCommand::Refresh => match get_or_connect_bus(&conn_slot).await {
            Some(c) => match fetch_snapshot(&c).await {
                Ok(snap) => {
                    let _ = evt_tx.send(UiEvent::Snapshot(Box::new(snap))).await;
                }
                Err(e) => {
                    let _ = evt_tx.send(UiEvent::DaemonOffline(e)).await;
                }
            },
            None => {
                let _ = evt_tx
                    .send(UiEvent::DaemonOffline(
                        "Sistem D-Bus bağlantısı kurulamadı.".to_string(),
                    ))
                    .await;
            }
        },
        UiCommand::CreatePlan { request } => {
            if let Err(err) = request.validate_red_lines() {
                let (tr, en) = format_akrep_error(&err);
                let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                return;
            }
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            let req_json = match serde_json::to_string(&request) {
                Ok(j) => j,
                Err(e) => {
                    let _ = evt_tx
                        .send(UiEvent::OperationError {
                            tr: format!("İstek serileştirme hatası: {}", e),
                            en: format!("Request encode error: {}", e),
                        })
                        .await;
                    return;
                }
            };
            match call_priv_json::<_, ActionPlan>(&c, "CreatePlan", &(req_json.as_str(),)).await {
                Ok(plan) => {
                    let _ = evt_tx
                        .send(UiEvent::PlanStaged {
                            plan: Box::new(plan),
                            is_profile_preview: false,
                        })
                        .await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::PreviewProfile { profile_id } => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, ActionPlan>(&c, "PreviewProfile", &(profile_id.as_str(),))
                .await
            {
                Ok(plan) => {
                    let _ = evt_tx
                        .send(UiEvent::PlanStaged {
                            plan: Box::new(plan),
                            is_profile_preview: true,
                        })
                        .await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::CommitPlan {
            plan_id,
            plan_sha256,
        } => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, TransactionRecord>(
                &c,
                "CommitPlan",
                &(plan_id.as_str(), plan_sha256.as_str()),
            )
            .await
            {
                Ok(rec) => {
                    let req_clone = rec.request.clone();
                    let _ = evt_tx
                        .send(UiEvent::TransactionCommitted(Box::new(rec)))
                        .await;
                    refresh_post_mutation_domains(&c, &req_clone, &evt_tx).await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::RollbackTransaction { tx_id } => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, TransactionRecord>(
                &c,
                "RollbackTransaction",
                &(tx_id.as_str(),),
            )
            .await
            {
                Ok(rec) => {
                    let req_clone = rec.request.clone();
                    let _ = evt_tx
                        .send(UiEvent::TransactionRolledBack(Box::new(rec)))
                        .await;
                    refresh_post_mutation_domains(&c, &req_clone, &evt_tx).await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::LoadHistory { limit } => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, Vec<TransactionRecord>>(&c, "GetHistory", &(limit,)).await {
                Ok(list) => {
                    let _ = evt_tx.send(UiEvent::HistoryLoaded(list)).await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::DiagnoseSelinux { query } => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            let Ok(q_json) = serde_json::to_string(&query) else {
                return;
            };
            if let Err(err) = query.validate_red_lines(&q_json) {
                let (tr, en) = format_akrep_error(&err);
                let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                return;
            }
            match call_priv_json::<_, SelinuxDiagnoseResponse>(
                &c,
                "DiagnoseSelinux",
                &(q_json.as_str(),),
            )
            .await
            {
                Ok(diag) => {
                    let _ = evt_tx.send(UiEvent::SelinuxDiagnosed(Box::new(diag))).await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::InspectFirewall { zone_filter } => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, FirewallStateSnapshot>(
                &c,
                "InspectFirewall",
                &(zone_filter.as_str(),),
            )
            .await
            {
                Ok(fw) => {
                    let _ = evt_tx.send(UiEvent::FirewallInspected(Box::new(fw))).await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::InspectSystemFlatpaks => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, FlatpakInstallationSnapshot>(&c, "InspectSystemFlatpaks", &())
                .await
            {
                Ok(snap) => {
                    let _ = evt_tx
                        .send(UiEvent::SystemFlatpaksInspected(Box::new(snap)))
                        .await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::InspectDeviceSecurity => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, DeviceSecurityStatus>(&c, "InspectDeviceSecurity", &()).await
            {
                Ok(status) => {
                    let _ = evt_tx
                        .send(UiEvent::DeviceSecurityInspected(Box::new(status)))
                        .await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::InspectScanner => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, ScannerStatusReport>(&c, "InspectScanner", &()).await {
                Ok(rep) => {
                    let _ = evt_tx.send(UiEvent::ScannerInspected(Box::new(rep))).await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::StartScan { request } => {
            if let Err(err) = request.validate_red_lines() {
                let (tr, en) = format_akrep_error(&err);
                let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                return;
            }
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            let Ok(req_json) = serde_json::to_string(&request) else {
                return;
            };
            match call_priv_json::<_, ScanProgress>(&c, "StartScan", &(req_json.as_str(),)).await {
                Ok(prog) => {
                    {
                        let mut jguard = active_scan_job.lock().await;
                        if prog.state == ScanJobState::Running {
                            *jguard = Some(prog.job_id.clone());
                        } else {
                            *jguard = None;
                        }
                    }
                    let _ = evt_tx
                        .send(UiEvent::ScanProgressUpdated(Box::new(prog)))
                        .await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::CancelScan { job_id } => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                return;
            };
            match call_priv_json::<_, ScanProgress>(&c, "CancelScan", &(job_id.as_str(),)).await {
                Ok(prog) => {
                    *active_scan_job.lock().await = None;
                    let _ = evt_tx
                        .send(UiEvent::ScanProgressUpdated(Box::new(prog)))
                        .await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::ListQuarantine => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, QuarantineInventory>(&c, "ListQuarantine", &()).await {
                Ok(inv) => {
                    let _ = evt_tx.send(UiEvent::QuarantineLoaded(Box::new(inv))).await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::ListProfiles => {
            let Some(c) = get_or_connect_bus(&conn_slot).await else {
                send_no_bus(&evt_tx).await;
                return;
            };
            match call_priv_json::<_, ProfileCatalogResponse>(&c, "ListProfiles", &()).await {
                Ok(cat) => {
                    let _ = evt_tx.send(UiEvent::ProfilesLoaded(Box::new(cat))).await;
                }
                Err((tr, en)) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::QueryPrivVersion => {
            if let Some(c) = get_or_connect_bus(&conn_slot).await {
                if let Ok(proxy) = zbus::Proxy::new(
                    &c,
                    AKREP_PRIV_BUS_NAME,
                    AKREP_PRIV_OBJECT_PATH,
                    AKREP_PRIV_INTERFACE_NAME,
                )
                .await
                {
                    if let Ok(ver) = proxy.call::<_, _, String>("GetVersion", &()).await {
                        let _ = evt_tx.send(UiEvent::PrivVersionLoaded(ver)).await;
                    }
                }
            }
        }
        UiCommand::RefreshUserApplications => match load_user_apps_blocking().await {
            Ok((inv, txs)) => {
                let _ = evt_tx
                    .send(UiEvent::UserApplicationsLoaded {
                        inventory: Box::new(inv),
                        user_txs: txs,
                    })
                    .await;
            }
            Err((tr, en)) => {
                let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
            }
        },
        UiCommand::ApplyUserFlatpakOverride {
            app_id,
            additions,
            removals,
        } => {
            let res = tokio::task::spawn_blocking(move || {
                let mgr = UserFlatpakManager::from_xdg_env().map_err(|e| format_akrep_error(&e))?;
                let tx = mgr
                    .apply_user_override(&app_id, &additions, &removals)
                    .map_err(|e| format_akrep_error(&e))?;
                let inv = mgr.inspect_unified().map_err(|e| format_akrep_error(&e))?;
                let txs = mgr.list_user_transactions(50).unwrap_or_default();
                Ok::<_, (String, String)>((tx, inv, txs))
            })
            .await;

            match res {
                Ok(Ok((tx, inv, txs))) => {
                    let _ = evt_tx
                        .send(UiEvent::UserFlatpakOverrideApplied(Box::new(tx)))
                        .await;
                    let _ = evt_tx
                        .send(UiEvent::UserApplicationsLoaded {
                            inventory: Box::new(inv),
                            user_txs: txs,
                        })
                        .await;
                }
                Ok(Err((tr, en))) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
                Err(join_err) => {
                    let _ = evt_tx
                        .send(UiEvent::OperationError {
                            tr: format!("Kullanıcı izin görevi başarısız: {}", join_err),
                            en: format!("User override task failed: {}", join_err),
                        })
                        .await;
                }
            }
        }
        UiCommand::RollbackUserFlatpakTx { tx_id } => {
            let res = tokio::task::spawn_blocking(move || {
                let mgr = UserFlatpakManager::from_xdg_env().map_err(|e| format_akrep_error(&e))?;
                let tx = mgr
                    .rollback_user_override(&tx_id)
                    .map_err(|e| format_akrep_error(&e))?;
                let inv = mgr.inspect_unified().map_err(|e| format_akrep_error(&e))?;
                let txs = mgr.list_user_transactions(50).unwrap_or_default();
                Ok::<_, (String, String)>((tx, inv, txs))
            })
            .await;

            match res {
                Ok(Ok((tx, inv, txs))) => {
                    let _ = evt_tx
                        .send(UiEvent::UserFlatpakTxRolledBack(Box::new(tx)))
                        .await;
                    let _ = evt_tx
                        .send(UiEvent::UserApplicationsLoaded {
                            inventory: Box::new(inv),
                            user_txs: txs,
                        })
                        .await;
                }
                Ok(Err((tr, en))) => {
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
                Err(join_err) => {
                    let _ = evt_tx
                        .send(UiEvent::OperationError {
                            tr: format!("Geri alma görevi başarısız: {}", join_err),
                            en: format!("User rollback task failed: {}", join_err),
                        })
                        .await;
                }
            }
        }
        UiCommand::RefreshPrivacySession => match load_privacy_session().await {
            Ok(snap) => {
                let _ = evt_tx
                    .send(UiEvent::PrivacySessionLoaded(Box::new(snap)))
                    .await;
            }
            Err((tr, en)) => {
                let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
            }
        },
        UiCommand::TerminatePipewireStream { node_id } => {
            match SessionPortalPipewireBackend::connect_session().await {
                Ok(backend) => {
                    let controller = PrivacySessionController::new(Arc::new(backend));
                    match controller
                        .execute_action(PortalPrivacyAction::TerminateActiveStream {
                            node_id,
                            user_confirmed: true,
                        })
                        .await
                    {
                        Ok(()) => {
                            let _ = evt_tx
                                .send(UiEvent::OperationInfo {
                                    tr: format!("PipeWire akış düğümü #{} sonlandırıldı.", node_id),
                                    en: format!(
                                        "Terminated PipeWire capture stream node #{}.",
                                        node_id
                                    ),
                                })
                                .await;
                            if let Ok(snap) = controller.snapshot().await {
                                let _ = evt_tx
                                    .send(UiEvent::PrivacySessionLoaded(Box::new(snap)))
                                    .await;
                            }
                        }
                        Err(err) => {
                            let (tr, en) = format_akrep_error(&err);
                            let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                        }
                    }
                }
                Err(err) => {
                    let (tr, en) = format_akrep_error(&err);
                    let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                }
            }
        }
        UiCommand::RevokePortalPermission {
            table,
            resource_id,
            app_id,
        } => match SessionPortalPipewireBackend::connect_session().await {
            Ok(backend) => {
                let controller = PrivacySessionController::new(Arc::new(backend));
                match controller
                    .execute_action(PortalPrivacyAction::RevokeStoredPermission {
                        table: table.clone(),
                        resource_id: resource_id.clone(),
                        app_id: app_id.clone(),
                        user_confirmed: true,
                    })
                    .await
                {
                    Ok(()) => {
                        let _ = evt_tx
                            .send(UiEvent::OperationInfo {
                                tr: format!("{} için '{}' portal izni geri alındı.", app_id, table),
                                en: format!(
                                    "Revoked stored '{}' portal grant for {}.",
                                    table, app_id
                                ),
                            })
                            .await;
                        if let Ok(snap) = controller.snapshot().await {
                            let _ = evt_tx
                                .send(UiEvent::PrivacySessionLoaded(Box::new(snap)))
                                .await;
                        }
                    }
                    Err(err) => {
                        let (tr, en) = format_akrep_error(&err);
                        let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
                    }
                }
            }
            Err(err) => {
                let (tr, en) = format_akrep_error(&err);
                let _ = evt_tx.send(UiEvent::OperationError { tr, en }).await;
            }
        },
    }
}

pub fn spawn_dbus_worker() -> (
    async_channel::Sender<UiCommand>,
    async_channel::Receiver<UiEvent>,
) {
    let (cmd_tx, cmd_rx) = async_channel::unbounded::<UiCommand>();
    let (evt_tx, evt_rx) = async_channel::unbounded::<UiEvent>();

    std::thread::Builder::new()
        .name("akrep-dbus-worker".to_string())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                let _ = evt_tx.try_send(UiEvent::DaemonOffline(
                    "Tokio çalışma zamanı başlatılamadı.".to_string(),
                ));
                return;
            };

            rt.block_on(async move {
                let initial_conn = connect_bus().await.ok();
                let conn_slot = Arc::new(RwLock::new(initial_conn.clone()));
                let active_scan_job = Arc::new(Mutex::new(None::<String>));

                match &initial_conn {
                    Some(c) => match fetch_snapshot(c).await {
                        Ok(snap) => {
                            let _ = evt_tx.send(UiEvent::Snapshot(Box::new(snap))).await;
                        }
                        Err(e) => {
                            let _ = evt_tx.send(UiEvent::DaemonOffline(e)).await;
                        }
                    },
                    None => {
                        let _ = evt_tx
                            .send(UiEvent::DaemonOffline(
                                "Sistem D-Bus bağlantısı kurulamadı.".to_string(),
                            ))
                            .await;
                    }
                }

                if let Ok((inv, txs)) = load_user_apps_blocking().await {
                    let _ = evt_tx
                        .send(UiEvent::UserApplicationsLoaded {
                            inventory: Box::new(inv),
                            user_txs: txs,
                        })
                        .await;
                }
                if let Ok(priv_snap) = load_privacy_session().await {
                    let _ = evt_tx
                        .send(UiEvent::PrivacySessionLoaded(Box::new(priv_snap)))
                        .await;
                }

                let mut akrep_sig_stream = if let Some(c) = &initial_conn {
                    if let Ok(rule) = MatchRule::builder()
                        .msg_type(zbus::message::Type::Signal)
                        .interface(AKREP_INTERFACE_NAME)
                    {
                        MessageStream::for_match_rule(rule.build(), c, Some(16))
                            .await
                            .ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                let mut owner_sig_stream = if let Some(c) = &initial_conn {
                    if let Ok(rule) = MatchRule::builder()
                        .msg_type(zbus::message::Type::Signal)
                        .interface("org.freedesktop.DBus")
                        .and_then(|b| b.member("NameOwnerChanged"))
                        .and_then(|b| b.arg(0, AKREP_BUS_NAME))
                    {
                        MessageStream::for_match_rule(rule.build(), c, Some(16))
                            .await
                            .ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                let mut poll_interval = tokio::time::interval(Duration::from_millis(600));
                poll_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    tokio::select! {
                        cmd = cmd_rx.recv() => {
                            let Ok(cmd) = cmd else { break };
                            if matches!(cmd, UiCommand::Stop) {
                                break;
                            }
                            let c_slot = Arc::clone(&conn_slot);
                            let s_slot = Arc::clone(&active_scan_job);
                            let tx = evt_tx.clone();
                            tokio::spawn(async move {
                                handle_ui_command(cmd, c_slot, s_slot, tx).await;
                            });
                        }
                        _ = poll_interval.tick() => {
                            let job_opt = active_scan_job.lock().await.clone();
                            if let Some(jid) = job_opt {
                                if let Some(c) = get_or_connect_bus(&conn_slot).await {
                                    let s_slot = Arc::clone(&active_scan_job);
                                    let tx = evt_tx.clone();
                                    tokio::spawn(async move {
                                        match call_priv_json::<_, ScanProgress>(&c, "GetScanStatus", &(jid.as_str(),)).await {
                                            Ok(prog) => {
                                                if prog.state != ScanJobState::Running {
                                                    let mut g = s_slot.lock().await;
                                                    if g.as_deref() == Some(jid.as_str()) {
                                                        *g = None;
                                                    }
                                                }
                                                let _ = tx.send(UiEvent::ScanProgressUpdated(Box::new(prog))).await;
                                            }
                                            Err(_) => {
                                                let mut g = s_slot.lock().await;
                                                if g.as_deref() == Some(jid.as_str()) {
                                                    *g = None;
                                                }
                                            }
                                        }
                                    });
                                }
                            }
                        }
                        msg = async {
                            match akrep_sig_stream.as_mut() {
                                Some(s) => s.next().await,
                                None => std::future::pending().await,
                            }
                        } => {
                            if msg.is_some() {
                                if let Some(c) = get_or_connect_bus(&conn_slot).await {
                                    if let Ok(snap) = fetch_snapshot(&c).await {
                                        let _ = evt_tx.send(UiEvent::Snapshot(Box::new(snap))).await;
                                    }
                                }
                            } else {
                                akrep_sig_stream = None;
                            }
                        }
                        msg = async {
                            match owner_sig_stream.as_mut() {
                                Some(s) => s.next().await,
                                None => std::future::pending().await,
                            }
                        } => {
                            if msg.is_some() {
                                tokio::time::sleep(Duration::from_millis(120)).await;
                                if let Some(c) = get_or_connect_bus(&conn_slot).await {
                                    match fetch_snapshot(&c).await {
                                        Ok(snap) => {
                                            let _ = evt_tx.send(UiEvent::Snapshot(Box::new(snap))).await;
                                        }
                                        Err(e) => {
                                            let _ = evt_tx.send(UiEvent::DaemonOffline(e)).await;
                                        }
                                    }
                                }
                            } else {
                                owner_sig_stream = None;
                            }
                        }
                    }
                }
            });
        })
        .expect("spawn akrep dbus worker");

    (cmd_tx, evt_rx)
}
