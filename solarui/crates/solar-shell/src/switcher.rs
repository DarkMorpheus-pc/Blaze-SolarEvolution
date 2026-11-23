use crate::engine::ShellEngine;
use crate::providers::{
    CaelestiaProvider, End4PcProvider, NoctaliaProvider, ShellProvider, We10xProvider,
};
use crate::supervisor::{ActualEngine, SolarShellSupervisor};
use anyhow::{bail, Context, Result};
use std::time::Duration;
use tracing::{info, warn};

pub struct SolarShellSwitcher {
    supervisor: SolarShellSupervisor,
    noctalia: Box<dyn ShellProvider>,
    full_caelestia: Box<dyn ShellProvider>,
    end4_pc: Box<dyn ShellProvider>,
    we10x: Box<dyn ShellProvider>,
    profile_sync_enabled: bool,
}

impl Default for SolarShellSwitcher {
    fn default() -> Self {
        Self {
            supervisor: SolarShellSupervisor::new(),
            noctalia: Box::new(NoctaliaProvider::new()),
            full_caelestia: Box::new(CaelestiaProvider::for_full()),
            end4_pc: Box::new(End4PcProvider::new()),
            we10x: Box::new(We10xProvider::new()),
            profile_sync_enabled: true,
        }
    }
}

impl SolarShellSwitcher {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn switch_engine(&self, target: ShellEngine) -> Result<()> {
        self.switch(Some(target)).await
    }

    pub async fn reconcile(&self) -> Result<()> {
        self.switch(None).await
    }

    async fn actual(&self) -> ActualEngine {
        let (n, c_full, e, w) = tokio::join!(
            self.noctalia.health_check(),
            self.full_caelestia.health_check(),
            self.end4_pc.health_check(),
            self.we10x.health_check(),
        );
        match (
            n.unwrap_or(false),
            c_full.unwrap_or(false),
            e.unwrap_or(false),
            w.unwrap_or(false),
        ) {
            (true, false, false, false) => ActualEngine::Noctalia,
            (false, true, false, false) => ActualEngine::Caelestia,
            (false, false, true, false) => ActualEngine::End4Pc,
            (false, false, false, true) => ActualEngine::We10x,
            (false, false, false, false) => ActualEngine::None,
            _ => ActualEngine::Conflict,
        }
    }

    async fn switch(&self, requested: Option<ShellEngine>) -> Result<()> {
        if requested.is_some() {
            // Detach from parent process group so stopping the active shell (e.g. Caelestia/Quickshell)
            // never kills this transition process via SIGHUP before the target shell starts.
            unsafe {
                libc::setsid();
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
            }
        }
        let lock_path = solar_common::get_solar_runtime_dir().join("shell-transition.lock");
        let _lock = if requested.is_some() {
            let mut acquired = None;
            for _ in 0..50 {
                match solar_common::persistence::try_lock(&lock_path) {
                    Ok(l) => {
                        acquired = Some(l);
                        break;
                    }
                    Err(_) => {
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
            acquired
                .context("Another shell transition is in progress and did not finish in time")?
        } else {
            solar_common::persistence::try_lock(&lock_path)
                .context("Another shell transition is in progress")?
        };
        // Read desired state AFTER obtaining the cross-process lock. The watchdog
        // must never apply an old choice after a concurrent user switch completes.
        let mut desired = match self.supervisor.load_desired_state() {
            Ok(d) => d,
            Err(e) if requested.is_some() => {
                tracing::warn!(
                    "Config file had errors ({e}); healing config.toml so explicit shell switch can proceed."
                );
                let cfg_path = solar_common::SolarConfig::config_path();
                let _ = std::fs::copy(&cfg_path, cfg_path.with_extension("toml.bak"));
                let mut fresh = solar_common::SolarConfig::default();
                fresh.shell.engine = requested.unwrap();
                let content = toml::to_string_pretty(&fresh).unwrap_or_default();
                let _ = solar_common::persistence::atomic_write(&cfg_path, content.as_bytes());
                crate::supervisor::DesiredState {
                    desired_engine: requested.unwrap(),
                    desired_profile: fresh.performance.profile.as_str().to_string(),
                }
            }
            Err(e) => return Err(e),
        };
        let original_desired = desired.clone();
        let target = requested.unwrap_or(desired.desired_engine);
        let previous = self.actual().await;

        self.preflight_check(target).await?;

        desired.desired_engine = target;
        self.supervisor
            .save_desired_state(&desired)
            .context("Cannot persist shell choice")?;

        if !SolarShellSupervisor::is_reconciled(target, &previous) {
            if let Err(err) = self.transition_with_rollback(target, &previous).await {
                if original_desired.desired_engine != target {
                    let _ = self.supervisor.save_desired_state(&original_desired);
                }
                return Err(err);
            }
        } else if target == ShellEngine::We10x && self.profile_sync_enabled {
            // Ensure We10X Niri/GSettings profile stays reconciled even if Quickshell was already alive.
            if let Err(e) = crate::we10x_profile::enter_we10x_profile().await {
                warn!("Could not refresh We10X profile: {e}");
            }
        }
        Ok(())
    }

    async fn preflight_check(&self, target: ShellEngine) -> Result<()> {
        let provider = match target {
            ShellEngine::Noctalia => &self.noctalia,
            ShellEngine::Caelestia => &self.full_caelestia,
            ShellEngine::End4Pc => &self.end4_pc,
            ShellEngine::We10x => &self.we10x,
        };
        anyhow::ensure!(
            provider.is_installed().await,
            "Target shell is not installed: {}",
            provider.name()
        );
        Ok(())
    }

    async fn transition_with_rollback(
        &self,
        target: ShellEngine,
        previous: &ActualEngine,
    ) -> Result<()> {
        if let Err(error) = self.transition(target).await {
            match self.rollback_or_safe_default(previous).await {
                Ok(()) => bail!("Shell transition failed: {error:#}; previous shell restored"),
                Err(rollback) => {
                    bail!("Shell transition failed: {error:#}; rollback also failed: {rollback:#}")
                }
            }
        }
        info!("Shell transition verified: {target:?}");
        Ok(())
    }

    async fn rollback_or_safe_default(&self, previous: &ActualEngine) -> Result<()> {
        match previous {
            ActualEngine::None | ActualEngine::Failed(_) => {
                // OOBE may request an optional shell before any healthy shell
                // exists. A failure must still leave a usable desktop.
                self.transition(ShellEngine::Noctalia).await
            }
            _ => self.rollback(previous).await,
        }
    }

    async fn transition(&self, target: ShellEngine) -> Result<()> {
        match target {
            ShellEngine::Noctalia => {
                self.full_caelestia.stop().await?;
                self.end4_pc.stop().await?;
                self.we10x.stop().await?;
                if self.profile_sync_enabled {
                    let _ = crate::we10x_profile::leave_we10x_profile().await;
                }
                self.noctalia.start().await?;
            }
            ShellEngine::Caelestia => {
                let auto_scale = crate::settings_gui::detect_screen_resolution_and_auto_scale();
                crate::settings_gui::apply_display_scale_dynamically(auto_scale);

                self.noctalia.stop().await?;
                self.end4_pc.stop().await?;
                self.we10x.stop().await?;
                if self.profile_sync_enabled {
                    let _ = crate::we10x_profile::leave_we10x_profile().await;
                }
                self.full_caelestia.start().await?;
            }
            ShellEngine::End4Pc => {
                self.noctalia.stop().await?;
                self.full_caelestia.stop().await?;
                self.we10x.stop().await?;
                if self.profile_sync_enabled {
                    let _ = crate::we10x_profile::leave_we10x_profile().await;
                }
                self.end4_pc.start().await?;
            }
            ShellEngine::We10x => {
                self.noctalia.stop().await?;
                self.full_caelestia.stop().await?;
                self.end4_pc.stop().await?;
                if self.profile_sync_enabled {
                    crate::we10x_profile::enter_we10x_profile().await?;
                }
                self.we10x.start().await?;
            }
        }
        for _ in 0..100 {
            if SolarShellSupervisor::is_reconciled(target, &self.actual().await) {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        bail!("Target shell did not become healthy: {target:?}")
    }

    async fn rollback(&self, previous: &ActualEngine) -> Result<()> {
        let target = match previous {
            ActualEngine::Noctalia => ShellEngine::Noctalia,
            ActualEngine::Caelestia => ShellEngine::Caelestia,
            ActualEngine::End4Pc => ShellEngine::End4Pc,
            ActualEngine::We10x => ShellEngine::We10x,
            ActualEngine::Conflict => {
                bail!("Multiple full shells are active; no safe rollback target")
            }
            _ => bail!("No previously healthy shell is available"),
        };
        self.transition(target).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Fake {
        alive: Arc<AtomicBool>,
        fail_start: bool,
        fail_stop: bool,
        lie_about_start: bool,
    }
    #[async_trait]
    impl ShellProvider for Fake {
        fn name(&self) -> &'static str {
            "test shell"
        }
        async fn is_installed(&self) -> bool {
            true
        }
        async fn start(&self) -> Result<()> {
            anyhow::ensure!(!self.fail_start, "start failed");
            if !self.lie_about_start {
                self.alive.store(true, Ordering::SeqCst);
            }
            Ok(())
        }
        async fn stop(&self) -> Result<()> {
            anyhow::ensure!(!self.fail_stop, "stop failed");
            self.alive.store(false, Ordering::SeqCst);
            Ok(())
        }
        async fn health_check(&self) -> Result<bool> {
            Ok(self.alive.load(Ordering::SeqCst))
        }
        async fn dispatch(&self, _: crate::actions::ShellAction) -> Result<()> {
            Ok(())
        }
    }
    fn fixture(n: bool, c: bool, fail_n: bool, fail_c: bool) -> SolarShellSwitcher {
        let ns = Arc::new(AtomicBool::new(n));
        let cs = Arc::new(AtomicBool::new(c));
        let es = Arc::new(AtomicBool::new(false));
        let ws = Arc::new(AtomicBool::new(false));
        SolarShellSwitcher {
            supervisor: SolarShellSupervisor::new(),
            noctalia: Box::new(Fake {
                alive: ns,
                fail_start: fail_n,
                fail_stop: false,
                lie_about_start: false,
            }),
            full_caelestia: Box::new(Fake {
                alive: cs,
                fail_start: fail_c,
                fail_stop: false,
                lie_about_start: false,
            }),
            end4_pc: Box::new(Fake {
                alive: es,
                fail_start: false,
                fail_stop: false,
                lie_about_start: false,
            }),
            we10x: Box::new(Fake {
                alive: ws,
                fail_start: false,
                fail_stop: false,
                lie_about_start: false,
            }),
            profile_sync_enabled: false,
        }
    }
    #[tokio::test]
    async fn successful_spawn_without_health_is_not_a_successful_switch() {
        let mut switcher = fixture(false, true, false, false);
        switcher.noctalia = Box::new(Fake {
            alive: Arc::new(AtomicBool::new(false)),
            fail_start: false,
            fail_stop: false,
            lie_about_start: true,
        });
        assert!(switcher
            .transition_with_rollback(ShellEngine::Noctalia, &ActualEngine::Caelestia)
            .await
            .is_err());
        assert_eq!(switcher.actual().await, ActualEngine::Caelestia);
    }

    #[tokio::test]
    async fn failed_noctalia_switch_restores_caelestia() {
        let switcher = fixture(false, true, true, false);
        assert!(switcher
            .transition_with_rollback(ShellEngine::Noctalia, &ActualEngine::Caelestia)
            .await
            .is_err());
        assert_eq!(switcher.actual().await, ActualEngine::Caelestia);
    }
    #[tokio::test]
    async fn failed_caelestia_switch_restores_noctalia() {
        let switcher = fixture(true, false, false, true);
        assert!(switcher
            .transition_with_rollback(ShellEngine::Caelestia, &ActualEngine::Noctalia)
            .await
            .is_err());
        assert_eq!(switcher.actual().await, ActualEngine::Noctalia);
    }
    #[tokio::test]
    async fn failed_rollback_is_reported() {
        let switcher = fixture(true, false, true, true);
        let err = switcher
            .transition_with_rollback(ShellEngine::Caelestia, &ActualEngine::Noctalia)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("rollback also failed"));
    }

    #[tokio::test]
    async fn end4_pc_transition_has_single_shell_ownership() {
        let mut switcher = fixture(true, false, false, false);
        switcher.end4_pc = Box::new(Fake {
            alive: Arc::new(AtomicBool::new(false)),
            fail_start: false,
            fail_stop: false,
            lie_about_start: false,
        });
        switcher.transition(ShellEngine::End4Pc).await.unwrap();
        assert_eq!(switcher.actual().await, ActualEngine::End4Pc);
    }

    #[tokio::test]
    async fn we10x_transition_and_rollback_preserve_single_shell_ownership() {
        let mut switcher = fixture(true, false, false, false);
        switcher.transition(ShellEngine::We10x).await.unwrap();
        assert_eq!(switcher.actual().await, ActualEngine::We10x);

        // Switching back to Noctalia stops We10x and starts Noctalia
        switcher.transition(ShellEngine::Noctalia).await.unwrap();
        assert_eq!(switcher.actual().await, ActualEngine::Noctalia);

        // Failed We10x start rolls back to Noctalia cleanly
        switcher.we10x = Box::new(Fake {
            alive: Arc::new(AtomicBool::new(false)),
            fail_start: true,
            fail_stop: false,
            lie_about_start: false,
        });
        assert!(switcher
            .transition_with_rollback(ShellEngine::We10x, &ActualEngine::Noctalia)
            .await
            .is_err());
        assert_eq!(switcher.actual().await, ActualEngine::Noctalia);
    }
}
