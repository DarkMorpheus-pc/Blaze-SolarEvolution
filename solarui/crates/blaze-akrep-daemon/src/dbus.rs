use blaze_sentinel_core::{AkrepSnapshot, AKREP_VERSION};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use zbus::object_server::SignalEmitter;
use zbus::Connection;

use crate::collectors::collect_snapshot;

pub const AKREP_BUS_NAME: &str = "tr.com.blazeos.Akrep1";
pub const AKREP_OBJECT_PATH: &str = "/tr/com/blazeos/Akrep1";
#[allow(dead_code)]
pub const AKREP_INTERFACE_NAME: &str = "tr.com.blazeos.Akrep1.Manager";

pub struct CachedTelemetry {
    pub snapshot: AkrepSnapshot,
    pub last_collected: Instant,
    pub known_findings: HashMap<String, (String, String)>, // id -> (severity, title_tr)
}

pub struct AkrepManager {
    conn: Connection,
    cache: Arc<Mutex<CachedTelemetry>>,
    refresh_lock: Arc<Mutex<()>>,
}

impl AkrepManager {
    pub async fn new(conn: Connection) -> Self {
        let initial = collect_snapshot(Some(&conn)).await;
        let known_findings = initial
            .findings
            .iter()
            .map(|f| {
                (
                    f.id.clone(),
                    (f.severity.as_str().to_string(), f.title_tr.clone()),
                )
            })
            .collect();
        Self {
            conn,
            cache: Arc::new(Mutex::new(CachedTelemetry {
                snapshot: initial,
                last_collected: Instant::now(),
                known_findings,
            })),
            refresh_lock: Arc::new(Mutex::new(())),
        }
    }

    pub async fn refresh_snapshot(
        &self,
        force: bool,
        emitter: Option<&SignalEmitter<'_>>,
    ) -> AkrepSnapshot {
        if !force {
            let guard = self.cache.lock().await;
            if guard.last_collected.elapsed() < Duration::from_secs(2) {
                return guard.snapshot.clone();
            }
        }

        let _serial = self.refresh_lock.lock().await;
        {
            let guard = self.cache.lock().await;
            if !force && guard.last_collected.elapsed() < Duration::from_secs(2) {
                return guard.snapshot.clone();
            }
        }

        let fresh = collect_snapshot(Some(&self.conn)).await;
        let fresh_map: HashMap<String, (String, String)> = fresh
            .findings
            .iter()
            .map(|f| {
                (
                    f.id.clone(),
                    (f.severity.as_str().to_string(), f.title_tr.clone()),
                )
            })
            .collect();

        let (prev_overall, added, resolved) = {
            let mut guard = self.cache.lock().await;
            let prev_overall = guard.snapshot.overall_status;
            let prev_ids: HashSet<String> = guard.known_findings.keys().cloned().collect();
            let curr_ids: HashSet<String> = fresh_map.keys().cloned().collect();

            let mut added = Vec::new();
            for id in curr_ids.difference(&prev_ids) {
                if let Some((sev, title)) = fresh_map.get(id) {
                    added.push((id.clone(), sev.clone(), title.clone()));
                }
            }
            let resolved: Vec<String> = prev_ids.difference(&curr_ids).cloned().collect();

            guard.snapshot = fresh.clone();
            guard.last_collected = Instant::now();
            guard.known_findings = fresh_map;
            (prev_overall, added, resolved)
        };

        if let Some(em) = emitter {
            if prev_overall != fresh.overall_status {
                let _ = Self::status_changed(em, fresh.overall_status.as_str(), &fresh.headline_tr)
                    .await;
            }
            for (fid, sev, title) in added {
                let _ = Self::finding_added(em, &fid, &sev, &title).await;
            }
            for fid in resolved {
                let _ = Self::finding_resolved(em, &fid).await;
            }
        }

        fresh
    }
}

#[zbus::interface(name = "tr.com.blazeos.Akrep1.Manager")]
impl AkrepManager {
    async fn get_version(&self) -> zbus::fdo::Result<String> {
        Ok(AKREP_VERSION.to_string())
    }

    async fn get_capabilities(&self) -> zbus::fdo::Result<String> {
        let guard = self.cache.lock().await;
        serde_json::to_string(&guard.snapshot.capabilities)
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_snapshot(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        let snap = self.refresh_snapshot(false, Some(&emitter)).await;
        serde_json::to_string(&snap).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_findings(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        let snap = self.refresh_snapshot(false, Some(&emitter)).await;
        serde_json::to_string(&snap.findings).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    async fn get_section_detail(
        &self,
        section: &str,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<String> {
        let snap = self.refresh_snapshot(false, Some(&emitter)).await;
        let Some(detail) = snap.sections.iter().find(|s| s.section == section) else {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "Unknown Akrep read-only section: {}",
                section
            )));
        };
        serde_json::to_string(detail).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }

    #[zbus(signal)]
    pub async fn status_changed(
        emitter: &SignalEmitter<'_>,
        overall_status: &str,
        summary: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn finding_added(
        emitter: &SignalEmitter<'_>,
        finding_id: &str,
        severity: &str,
        title: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn finding_resolved(
        emitter: &SignalEmitter<'_>,
        finding_id: &str,
    ) -> zbus::Result<()>;
}
