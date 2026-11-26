use anyhow::{Context, Result};
use blaze_akrep_common::{
    redact_secrets, AkrepError, OverrideSubsystem, AKREP_PRIV_BUS_NAME, AKREP_PRIV_OBJECT_PATH,
};
use blaze_akrep_privd::{
    AuditLogger, CompositeMutationBackend, IdleTracker, OverrideAdapter, PrivilegedManager,
    SystemPolkitAuthorizer, TransactionEngine, DEFAULT_AKREP_STATE_DIR,
};
use blaze_sentinel_core::AKREP_VERSION;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use tracing::info;
use zbus::Connection;

const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
struct RevertOverrideCli {
    subsystem: OverrideSubsystem,
    generation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CliConfig {
    idle_timeout_secs: u64,
    state_dir: PathBuf,
    use_session_bus: bool,
    revert_override: Option<RevertOverrideCli>,
}

fn print_help() {
    println!(
        "akrep-privd {} - On-demand Polkit-authenticated transaction daemon for BlazeOS Akrep",
        AKREP_VERSION
    );
    println!(
        "Usage: akrep-privd [--idle-timeout-secs <N>] [--state-dir <PATH>] [--session] [--version] [--help]"
    );
    println!(
        "       akrep-privd --revert-override <selinux|firewalld|oomd> --generation <UUID> [--state-dir <PATH>]"
    );
}

fn parse_args_from(args: &[String]) -> Result<Option<CliConfig>> {
    let mut idle_timeout_secs = DEFAULT_IDLE_TIMEOUT_SECS;
    let mut state_dir = PathBuf::from(DEFAULT_AKREP_STATE_DIR);
    let mut use_session_bus = false;
    let mut revert_subsys: Option<OverrideSubsystem> = None;
    let mut generation_id: Option<String> = None;

    let mut idx = 0usize;
    while idx < args.len() {
        match args[idx].as_str() {
            "--help" | "-h" => {
                print_help();
                return Ok(None);
            }
            "--version" | "-V" => {
                println!("akrep-privd {}", AKREP_VERSION);
                return Ok(None);
            }
            "--session" => {
                use_session_bus = true;
                idx += 1;
            }
            "--idle-timeout-secs" => {
                let val = args
                    .get(idx + 1)
                    .context("Missing value for --idle-timeout-secs")?;
                idle_timeout_secs = val
                    .parse::<u64>()
                    .context("Invalid integer for --idle-timeout-secs")?;
                idx += 2;
            }
            "--state-dir" => {
                let val = args.get(idx + 1).context("Missing value for --state-dir")?;
                state_dir = PathBuf::from(val);
                idx += 2;
            }
            "--revert-override" => {
                let val = args
                    .get(idx + 1)
                    .context("Missing value for --revert-override")?;
                let subsys = OverrideSubsystem::from_str_id(val).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Invalid subsystem '{}' for --revert-override (expected selinux|firewalld|oomd)",
                        val
                    )
                })?;
                revert_subsys = Some(subsys);
                idx += 2;
            }
            "--generation" => {
                let val = args
                    .get(idx + 1)
                    .context("Missing value for --generation")?;
                if val.is_empty()
                    || val.len() > 128
                    || val.contains("..")
                    || !val
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                {
                    anyhow::bail!("Invalid generation UUID '{}'", val);
                }
                generation_id = Some(val.clone());
                idx += 2;
            }
            other => {
                anyhow::bail!("Unknown argument: {}", other);
            }
        }
    }

    let revert_override = match (revert_subsys, generation_id) {
        (Some(subsystem), Some(generation_id)) => Some(RevertOverrideCli {
            subsystem,
            generation_id,
        }),
        (Some(_), None) => {
            anyhow::bail!("--revert-override requires --generation <UUID>");
        }
        (None, Some(_)) => {
            anyhow::bail!("--generation requires --revert-override <selinux|firewalld|oomd>");
        }
        (None, None) => None,
    };

    Ok(Some(CliConfig {
        idle_timeout_secs,
        state_dir,
        use_session_bus,
        revert_override,
    }))
}

fn parse_args() -> Result<Option<CliConfig>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    parse_args_from(&args)
}

fn execute_revert_override_with_adapter(
    adapter: &OverrideAdapter,
    state_dir: &Path,
    subsystem: OverrideSubsystem,
    generation_id: &str,
) -> std::result::Result<bool, AkrepError> {
    let reverted = adapter.handle_timer_revert(subsystem, generation_id)?;
    if reverted {
        let audit = AuditLogger::new(state_dir.join("audit.log"))?;
        audit.log(
            "override_auto_reverted",
            None,
            None,
            Some(0),
            None,
            None,
            "reverted",
            &format!(
                "Auto-reverted timed override for '{}' (generation={})",
                subsystem.as_str(),
                redact_secrets(generation_id)
            ),
        )?;
    }
    Ok(reverted)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let Some(cfg) = parse_args()? else {
        return Ok(());
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    if let Some(revert) = cfg.revert_override {
        let adapter = OverrideAdapter::new_system(cfg.state_dir.clone());
        let reverted = execute_revert_override_with_adapter(
            &adapter,
            &cfg.state_dir,
            revert.subsystem,
            &revert.generation_id,
        )
        .map_err(|e| anyhow::anyhow!("{}", e.to_redacted_json()))?;
        info!(
            subsystem = revert.subsystem.as_str(),
            generation_id = %revert.generation_id,
            reverted = reverted,
            "Completed --revert-override handler"
        );
        return Ok(());
    }

    let conn = if cfg.use_session_bus {
        Connection::session().await?
    } else {
        Connection::system().await?
    };

    let authorizer = Arc::new(SystemPolkitAuthorizer::new(conn.clone()));
    let backend = Arc::new(CompositeMutationBackend::new_system_with_state_dir(
        conn.clone(),
        cfg.state_dir.clone(),
    ));
    let engine = Arc::new(
        TransactionEngine::new(cfg.state_dir.clone(), authorizer, backend)
            .map_err(|e| anyhow::anyhow!("{}", e.to_redacted_json()))?,
    );
    let idle = Arc::new(IdleTracker::new());

    let manager = PrivilegedManager::new(Arc::clone(&engine), Arc::clone(&idle));
    conn.object_server()
        .at(AKREP_PRIV_OBJECT_PATH, manager)
        .await?;
    conn.request_name(AKREP_PRIV_BUS_NAME).await?;

    info!(
        bus = AKREP_PRIV_BUS_NAME,
        path = AKREP_PRIV_OBJECT_PATH,
        idle_timeout_secs = cfg.idle_timeout_secs,
        state_dir = %cfg.state_dir.display(),
        "akrep-privd ready (Polkit system-bus-name authenticated)"
    );

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("akrep-privd received SIGTERM, exiting cleanly");
                break;
            }
            _ = sigint.recv() => {
                info!("akrep-privd received SIGINT, exiting cleanly");
                break;
            }
            _ = ticker.tick(), if cfg.idle_timeout_secs > 0 => {
                if engine.has_active_background_jobs() {
                    idle.touch();
                } else if idle.is_idle_expired(Duration::from_secs(cfg.idle_timeout_secs)) {
                    info!(
                        idle_timeout_secs = cfg.idle_timeout_secs,
                        "akrep-privd idle timeout reached with zero active calls or background jobs, exiting cleanly"
                    );
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use blaze_akrep_common::{CompatPackageGroup, OverrideDuration, SelinuxRuntimeMode};
    use blaze_akrep_privd::{OverrideOps, OverrideStoreDocument};
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    struct StubCliOverrideOps {
        selinux_mode: Mutex<SelinuxRuntimeMode>,
        selinux_cfg: Mutex<String>,
    }

    impl StubCliOverrideOps {
        fn new() -> Self {
            Self {
                selinux_mode: Mutex::new(SelinuxRuntimeMode::Enforcing),
                selinux_cfg: Mutex::new("SELINUX=enforcing\nSELINUXTYPE=targeted\n".to_string()),
            }
        }
    }

    impl OverrideOps for StubCliOverrideOps {
        fn read_boot_id(&self) -> Result<String, AkrepError> {
            Ok("boot-cli-1".to_string())
        }
        fn read_selinux_runtime_mode(&self) -> Result<SelinuxRuntimeMode, AkrepError> {
            Ok(*self.selinux_mode.lock().unwrap())
        }
        fn setenforce(&self, enforcing: bool) -> Result<(), AkrepError> {
            *self.selinux_mode.lock().unwrap() = if enforcing {
                SelinuxRuntimeMode::Enforcing
            } else {
                SelinuxRuntimeMode::Permissive
            };
            Ok(())
        }
        fn read_selinux_config_raw(&self) -> Result<Option<String>, AkrepError> {
            Ok(Some(self.selinux_cfg.lock().unwrap().clone()))
        }
        fn write_selinux_config_raw(&self, content: &str) -> Result<(), AkrepError> {
            *self.selinux_cfg.lock().unwrap() = content.to_string();
            Ok(())
        }
        fn is_unit_active(&self, _unit: &str) -> Result<bool, AkrepError> {
            Ok(true)
        }
        fn is_unit_enabled(&self, _unit: &str) -> Result<bool, AkrepError> {
            Ok(true)
        }
        fn start_unit(&self, _unit: &str) -> Result<(), AkrepError> {
            Ok(())
        }
        fn stop_unit(&self, _unit: &str) -> Result<(), AkrepError> {
            Ok(())
        }
        fn enable_unit(&self, _unit: &str) -> Result<(), AkrepError> {
            Ok(())
        }
        fn disable_unit(&self, _unit: &str) -> Result<(), AkrepError> {
            Ok(())
        }
        fn daemon_reload(&self) -> Result<(), AkrepError> {
            Ok(())
        }
        fn schedule_revert_timer(
            &self,
            subsystem: OverrideSubsystem,
            _delay_secs: u64,
            _generation_id: &str,
            _state_dir: &Path,
        ) -> Result<String, AkrepError> {
            Ok(format!("akrep-revert-{}.timer", subsystem.as_str()))
        }
        fn cancel_revert_timer(&self, _subsystem: OverrideSubsystem) -> Result<(), AkrepError> {
            Ok(())
        }
        fn read_nofile_dropins(&self) -> Result<(Option<String>, Option<String>), AkrepError> {
            Ok((None, None))
        }
        fn write_nofile_dropins(&self, _pam: &str, _sysd: &str) -> Result<(), AkrepError> {
            Ok(())
        }
        fn remove_nofile_dropins(&self) -> Result<(), AkrepError> {
            Ok(())
        }
        fn restore_nofile_dropins(
            &self,
            _pam_content: Option<&str>,
            _systemd_content: Option<&str>,
        ) -> Result<(), AkrepError> {
            Ok(())
        }
        fn resolve_container_dir(&self, path: &Path) -> Result<PathBuf, AkrepError> {
            Ok(path.to_path_buf())
        }
        fn read_path_selinux_context(&self, _path: &Path) -> Result<String, AkrepError> {
            Ok("unconfined_u:object_r:user_home_t:s0".to_string())
        }
        fn chcon_container_dir(&self, _dir: &Path, _recursive: bool) -> Result<(), AkrepError> {
            Ok(())
        }
        fn restorecon_container_dir(
            &self,
            _dir: &Path,
            _recursive: bool,
        ) -> Result<(), AkrepError> {
            Ok(())
        }
        fn is_packages_installed(&self, _pkgs: &[&str]) -> Result<bool, AkrepError> {
            Ok(true)
        }
        fn install_compat_packages(&self, _group: CompatPackageGroup) -> Result<(), AkrepError> {
            Ok(())
        }
        fn remove_compat_packages(&self, _group: CompatPackageGroup) -> Result<(), AkrepError> {
            Ok(())
        }
    }

    #[test]
    fn test_cli_parse_revert_override_args() {
        let parsed = parse_args_from(&[
            "--revert-override".to_string(),
            "selinux".to_string(),
            "--generation".to_string(),
            "11111111-2222-4333-8444-555555555555".to_string(),
            "--state-dir".to_string(),
            "/var/lib/akrep".to_string(),
        ])
        .unwrap()
        .unwrap();
        assert_eq!(
            parsed.revert_override,
            Some(RevertOverrideCli {
                subsystem: OverrideSubsystem::Selinux,
                generation_id: "11111111-2222-4333-8444-555555555555".to_string(),
            })
        );

        assert!(
            parse_args_from(&["--revert-override".to_string(), "selinux".to_string(),]).is_err()
        );
        assert!(parse_args_from(&[
            "--revert-override".to_string(),
            "invalid".to_string(),
            "--generation".to_string(),
            "abc".to_string(),
        ])
        .is_err());
    }

    #[test]
    fn test_cli_revert_override_writes_audit_log_only_on_match() {
        let dir = TempDir::new().unwrap();
        let pub_cache = dir.path().join("cache/active_overrides.json");
        let ops = Arc::new(StubCliOverrideOps::new());
        let adapter =
            OverrideAdapter::new_with_ops(dir.path().to_path_buf(), pub_cache.clone(), ops.clone());
        let snap = dir.path().join("snap");
        fs::create_dir_all(&snap).unwrap();

        adapter
            .apply_selinux_mode(
                blaze_akrep_common::SelinuxDesiredMode::Permissive,
                OverrideDuration::Timed { minutes: 15 },
                &snap,
            )
            .unwrap();
        let doc: OverrideStoreDocument =
            serde_json::from_str(&fs::read_to_string(&pub_cache).unwrap()).unwrap();
        let gen_id = doc.entries["selinux"].generation_id.clone();

        // Stale generation does not revert and does not log override_auto_reverted.
        let skipped = execute_revert_override_with_adapter(
            &adapter,
            dir.path(),
            OverrideSubsystem::Selinux,
            "stale-gen-id",
        )
        .unwrap();
        assert!(!skipped);
        assert!(!dir.path().join("audit.log").exists());

        // Matching generation reverts to Enforcing and logs override_auto_reverted.
        let reverted = execute_revert_override_with_adapter(
            &adapter,
            dir.path(),
            OverrideSubsystem::Selinux,
            &gen_id,
        )
        .unwrap();
        assert!(reverted);
        assert_eq!(
            ops.read_selinux_runtime_mode().unwrap(),
            SelinuxRuntimeMode::Enforcing
        );

        let audit = AuditLogger::new(dir.path().join("audit.log")).unwrap();
        let events = audit.read_events().unwrap();
        assert!(events.iter().any(|e| e.event == "override_auto_reverted"));
    }
}
