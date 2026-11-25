use anyhow::{Context, Result};
use blaze_akrep_common::{AKREP_PRIV_BUS_NAME, AKREP_PRIV_OBJECT_PATH};
use blaze_akrep_privd::{
    CompositeMutationBackend, IdleTracker, PrivilegedManager, SystemPolkitAuthorizer,
    TransactionEngine, DEFAULT_AKREP_STATE_DIR,
};
use blaze_sentinel_core::AKREP_VERSION;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use tracing::info;
use zbus::Connection;

const DEFAULT_IDLE_TIMEOUT_SECS: u64 = 60;

struct CliConfig {
    idle_timeout_secs: u64,
    state_dir: PathBuf,
    use_session_bus: bool,
}

fn print_help() {
    println!(
        "akrep-privd {} — On-demand Polkit-authenticated transaction daemon for BlazeOS Akrep",
        AKREP_VERSION
    );
    println!(
        "Usage: akrep-privd [--idle-timeout-secs <N>] [--state-dir <PATH>] [--session] [--version] [--help]"
    );
}

fn parse_args() -> Result<Option<CliConfig>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut idle_timeout_secs = DEFAULT_IDLE_TIMEOUT_SECS;
    let mut state_dir = PathBuf::from(DEFAULT_AKREP_STATE_DIR);
    let mut use_session_bus = false;

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
            other => {
                anyhow::bail!("Unknown argument: {}", other);
            }
        }
    }

    Ok(Some(CliConfig {
        idle_timeout_secs,
        state_dir,
        use_session_bus,
    }))
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
                if idle.is_idle_expired(Duration::from_secs(cfg.idle_timeout_secs)) {
                    info!(
                        idle_timeout_secs = cfg.idle_timeout_secs,
                        "akrep-privd idle timeout reached with zero active calls, exiting cleanly"
                    );
                    break;
                }
            }
        }
    }

    Ok(())
}
