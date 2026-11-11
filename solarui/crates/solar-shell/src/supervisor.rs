use anyhow::Result;
use serde::{Deserialize, Serialize};
use solar_common::SolarConfig;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::engine::ShellEngine;
use crate::providers::{CaelestiaProvider, End4PcProvider, NoctaliaProvider, ShellProvider};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DesiredState {
    #[serde(default)]
    pub desired_engine: ShellEngine,
    #[serde(default = "default_profile")]
    pub desired_profile: String,
}

fn default_profile() -> String {
    "balanced".to_string()
}

impl Default for DesiredState {
    fn default() -> Self {
        Self {
            desired_engine: ShellEngine::Noctalia,
            desired_profile: default_profile(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActualEngine {
    Noctalia,
    Caelestia,
    End4Pc,
    Conflict,
    None,
    Failed(String),
}

impl ActualEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            ActualEngine::Noctalia => "noctalia",
            ActualEngine::Caelestia => "caelestia",
            ActualEngine::End4Pc => "end4-pc",
            ActualEngine::Conflict => "conflict",
            ActualEngine::None => "none",
            ActualEngine::Failed(_) => "failed",
        }
    }
}

pub struct SolarShellSupervisor {
    state_file: PathBuf,
    noctalia: NoctaliaProvider,
    caelestia: CaelestiaProvider,
    end4_pc: End4PcProvider,
}

impl Default for SolarShellSupervisor {
    fn default() -> Self {
        let state_file = solar_common::get_solar_config_dir().join("shell_state.json");
        Self {
            state_file,
            noctalia: NoctaliaProvider::new(),
            caelestia: CaelestiaProvider::new(),
            end4_pc: End4PcProvider::new(),
        }
    }
}

impl SolarShellSupervisor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Single Source-of-Truth: SolarConfig üzerinden istenen durumu okur.
    pub fn load_desired_state(&self) -> Result<DesiredState> {
        let cfg = SolarConfig::try_load()?;
        Ok(DesiredState {
            desired_engine: cfg.shell.engine,
            desired_profile: cfg.performance.profile.as_str().to_string(),
        })
    }

    pub fn save_desired_state(&self, state: &DesiredState) -> Result<()> {
        let profile = state
            .desired_profile
            .parse()
            .map_err(|e: String| anyhow::anyhow!(e))?;
        let matching_provider = match state.desired_engine {
            ShellEngine::Noctalia => solar_common::SurfaceProvider::Noctalia,
            ShellEngine::Caelestia => solar_common::SurfaceProvider::Caelestia,
            ShellEngine::End4Pc => solar_common::SurfaceProvider::End4Pc,
        };
        SolarConfig::update(|cfg| {
            cfg.shell.engine = state.desired_engine;
            cfg.shell.launcher_provider = matching_provider;
            cfg.shell.dashboard_provider = matching_provider;
            cfg.shell.bar_provider = matching_provider;
            cfg.shell.notifications_provider = matching_provider;
            cfg.shell.osd_provider = matching_provider;
            cfg.performance.profile = profile;
        })?;
        Self::sync_noctalia_state_file(state.desired_engine, matching_provider);
        // Config is authoritative; a diagnostic mirror failure must not undo a
        // successful transition or make the watchdog rewrite the configuration.
        if let Err(e) = solar_common::persistence::atomic_write(
            &self.state_file,
            &serde_json::to_vec_pretty(state)?,
        ) {
            warn!("Could not update diagnostic shell state: {e}");
        }
        Ok(())
    }

    fn sync_noctalia_state_file(engine: ShellEngine, provider: solar_common::SurfaceProvider) {
        let Ok(home) = std::env::var("HOME") else {
            return;
        };
        let path = PathBuf::from(home).join(".local/state/noctalia/settings.toml");
        if let Ok(content) = std::fs::read_to_string(&path) {
            if let Ok(mut doc) = content.parse::<toml::Value>() {
                if let Some(table) = doc.as_table_mut() {
                    if !table.contains_key("solarui") {
                        table.insert(
                            "solarui".to_string(),
                            toml::Value::Table(toml::map::Map::new()),
                        );
                    }
                    if let Some(s) = table.get_mut("solarui").and_then(|v| v.as_table_mut()) {
                        s.insert(
                            "shell_engine".to_string(),
                            toml::Value::String(engine.as_str().to_string()),
                        );
                        s.insert(
                            "launcher_provider".to_string(),
                            toml::Value::String(provider.as_str().to_string()),
                        );
                        s.insert(
                            "dashboard_provider".to_string(),
                            toml::Value::String(provider.as_str().to_string()),
                        );
                    }
                }
                if let Ok(serialized) = toml::to_string_pretty(&doc) {
                    let _ = std::fs::write(&path, serialized);
                }
            }
        }
    }

    pub fn reconcile_niri_config_symlink() {
        let Ok(home) = std::env::var("HOME") else {
            return;
        };
        let home_path = PathBuf::from(&home);
        let solar_cfg = home_path.join(".config/solarui/niri.kdl");
        let niri_dir = home_path.join(".config/niri");
        let std_cfg = niri_dir.join("config.kdl");

        if !solar_cfg.exists() {
            return;
        }

        if let Ok(meta) = std::fs::symlink_metadata(&std_cfg) {
            if !meta.file_type().is_symlink() {
                let std_mtime = meta.modified().ok();
                let solar_mtime = std::fs::metadata(&solar_cfg).ok().and_then(|m| m.modified().ok());
                if let (Some(n_time), Some(s_time)) = (std_mtime, solar_mtime) {
                    if n_time > s_time {
                        info!("Detected user edit to ~/.config/niri/config.kdl; syncing to ~/.config/solarui/niri.kdl");
                        let _ = std::fs::copy(&std_cfg, &solar_cfg);
                        let _ = std::process::Command::new("niri")
                            .args(["msg", "action", "load-config-file"])
                            .status();
                    }
                }
                let _ = std::fs::remove_file(&std_cfg);
                let _ = std::os::unix::fs::symlink("../solarui/niri.kdl", &std_cfg);
            }
        } else {
            let _ = std::fs::create_dir_all(&niri_dir);
            let _ = std::os::unix::fs::symlink("../solarui/niri.kdl", &std_cfg);
        }
    }

    pub fn is_reconciled(desired: ShellEngine, actual: &ActualEngine) -> bool {
        matches!(
            (desired, actual),
            (ShellEngine::Noctalia, ActualEngine::Noctalia)
                | (ShellEngine::Caelestia, ActualEngine::Caelestia)
                | (ShellEngine::End4Pc, ActualEngine::End4Pc)
        )
    }

    pub async fn detect_actual_engine(&self) -> ActualEngine {
        let n_alive = self.noctalia.health_check().await.unwrap_or(false);
        let c_alive = self.caelestia.health_check().await.unwrap_or(false);
        let e_alive = self.end4_pc.health_check().await.unwrap_or(false);

        match (n_alive, c_alive, e_alive) {
            (true, false, false) => ActualEngine::Noctalia,
            (false, true, false) => ActualEngine::Caelestia,
            (false, false, true) => ActualEngine::End4Pc,
            (false, false, false) => ActualEngine::None,
            _ => ActualEngine::Conflict,
        }
    }

    pub async fn reconcile(&self) -> Result<()> {
        Self::reconcile_niri_config_symlink();
        let desired = self.load_desired_state()?;
        let actual = self.detect_actual_engine().await;
        if Self::is_reconciled(desired.desired_engine, &actual) {
            return Ok(());
        }
        crate::switcher::SolarShellSwitcher::new().reconcile().await
    }

    pub fn is_gamezone_active() -> bool {
        let runtime_lock = solar_common::get_solar_runtime_dir().join("gamezone-active.lock");
        if let Ok(raw_pid) = std::fs::read_to_string(&runtime_lock) {
            if let Ok(pid) = raw_pid.trim().parse::<u32>() {
                if pid > 1 && std::path::Path::new(&format!("/proc/{}/stat", pid)).exists() {
                    return true;
                }
            }
            let _ = std::fs::remove_file(&runtime_lock);
        }
        let _ = std::fs::remove_file("/tmp/solar-gamezone-active.lock");
        false
    }

    pub fn start_watchdog_loop(self: std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            info!("SolarShell watchdog started");
            let mut delay = 3;
            loop {
                // Delay after completion: do not replay missed ticks in a restart storm.
                tokio::time::sleep(std::time::Duration::from_secs(delay)).await;

                // Anaconda kurulum sureci calısıyorsa shell gecislerini durdur.
                // Kurulum sırasında health_check geçici basarısız donebilir ve
                // watchdog'un yanlis engine'e gecmesine yol acar.
                // /proc dosya sistemi uzerinden kontrol: fork/exec gerektirmez.
                let anaconda_running = std::path::Path::new("/run/anaconda.pid").exists()
                    || std::fs::read_dir("/proc")
                        .map(|entries| {
                            entries.flatten().any(|e| {
                                let name = e.file_name();
                                if name.to_str().map(|s| s.chars().all(|c| c.is_ascii_digit())).unwrap_or(false) {
                                    std::fs::read_to_string(e.path().join("comm"))
                                        .map(|c| c.trim() == "anaconda")
                                        .unwrap_or(false)
                                } else {
                                    false
                                }
                            })
                        })
                        .unwrap_or(false);

                // GameZone bağımsız oyun kabuğu modu aktifse desktop shell denetimini askıya al.
                // Bu sayede oyun sırasında arka planda Noctalia/Caelestia çalıştırılmaz,
                // RAM ve GPU kaynakları tamamen oyuna tahsis edilir.
                if Self::is_gamezone_active() {
                    tracing::debug!("GameZone standalone game shell is active; watchdog desktop shell reconciliation suspended.");
                    delay = 3;
                    continue;
                }

                if anaconda_running {
                    tracing::debug!("Anaconda kurulumu devam ediyor; watchdog shell gecisi askiya alindi.");
                    delay = 5;
                    continue;
                }

                match self.reconcile().await {
                    Ok(()) => delay = 3,
                    Err(e) => {
                        tracing::error!("Watchdog reconciliation failed: {e:#}");
                        delay = (delay * 2).min(60);
                    }
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_reconciled_matching() {
        assert!(SolarShellSupervisor::is_reconciled(
            ShellEngine::Noctalia,
            &ActualEngine::Noctalia
        ));
        assert!(SolarShellSupervisor::is_reconciled(
            ShellEngine::Caelestia,
            &ActualEngine::Caelestia
        ));
        assert!(SolarShellSupervisor::is_reconciled(
            ShellEngine::End4Pc,
            &ActualEngine::End4Pc
        ));
        // Uyuşmazlık durumları
        assert!(!SolarShellSupervisor::is_reconciled(
            ShellEngine::Noctalia,
            &ActualEngine::Caelestia
        ));
        assert!(!SolarShellSupervisor::is_reconciled(
            ShellEngine::Caelestia,
            &ActualEngine::Noctalia
        ));
        assert!(!SolarShellSupervisor::is_reconciled(
            ShellEngine::Noctalia,
            &ActualEngine::None
        ));
    }
}
