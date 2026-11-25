mod collectors;
mod dbus;

use anyhow::Result;
use blaze_sentinel_core::AKREP_VERSION;
use futures_util::stream::StreamExt;
use std::time::Duration;
use tokio::signal::unix::{signal, SignalKind};
use tracing::{info, warn};
use zbus::{Connection, MatchRule, MessageStream};

use crate::collectors::collect_snapshot;
use crate::dbus::{AkrepManager, AKREP_BUS_NAME, AKREP_OBJECT_PATH};

fn print_help() {
    println!(
        "akrepd {} — Strictly read-only BlazeOS Akrep D-Bus service",
        AKREP_VERSION
    );
    println!("Usage: akrepd [--oneshot-json | --session | --version | --help]");
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("akrepd {}", AKREP_VERSION);
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();

    if args.iter().any(|a| a == "--oneshot-json") {
        let conn = Connection::system().await.ok();
        let snap = collect_snapshot(conn.as_ref()).await;
        println!("{}", serde_json::to_string_pretty(&snap)?);
        return Ok(());
    }

    let use_session = args.iter().any(|a| a == "--session");
    let conn = if use_session {
        Connection::session().await?
    } else {
        Connection::system().await?
    };

    let manager = AkrepManager::new(conn.clone()).await;
    conn.object_server().at(AKREP_OBJECT_PATH, manager).await?;
    conn.request_name(AKREP_BUS_NAME).await?;
    info!(
        bus = AKREP_BUS_NAME,
        path = AKREP_OBJECT_PATH,
        read_only = true,
        "akrepd ready (strictly read-only)"
    );

    // Event-driven watch on FirewallD1 NameOwnerChanged + DefaultZoneChanged (zero hot polling loop)
    let fw_owner_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.DBus")?
        .member("NameOwnerChanged")?
        .arg(0, "org.fedoraproject.FirewallD1")?
        .build();
    let fw_zone_rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.fedoraproject.FirewallD1")?
        .build();

    let mut owner_stream = MessageStream::for_match_rule(fw_owner_rule, &conn, Some(8))
        .await
        .ok();
    let mut zone_stream = MessageStream::for_match_rule(fw_zone_rule, &conn, Some(8))
        .await
        .ok();

    let obj_server = conn.object_server();
    let iface_ref = obj_server
        .interface::<_, AkrepManager>(AKREP_OBJECT_PATH)
        .await?;

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    loop {
        tokio::select! {
            _ = sigterm.recv() => {
                info!("akrepd received SIGTERM, shutting down cleanly");
                break;
            }
            _ = sigint.recv() => {
                info!("akrepd received SIGINT, shutting down cleanly");
                break;
            }
            msg = async {
                match owner_stream.as_mut() {
                    Some(s) => s.next().await,
                    None => std::future::pending().await,
                }
            } => {
                if msg.is_some() {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    let iface = iface_ref.get().await;
                    iface.refresh_snapshot(true, Some(iface_ref.signal_emitter())).await;
                } else {
                    warn!("D-Bus NameOwnerChanged watch ended");
                    owner_stream = None;
                }
            }
            msg = async {
                match zone_stream.as_mut() {
                    Some(s) => s.next().await,
                    None => std::future::pending().await,
                }
            } => {
                if msg.is_some() {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    let iface = iface_ref.get().await;
                    iface.refresh_snapshot(true, Some(iface_ref.signal_emitter())).await;
                } else {
                    zone_stream = None;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_collect_snapshot_offline_read_only_invariants() {
        let snap = collect_snapshot(None).await;
        assert!(snap.read_only);
        assert!(!snap.mutators_enabled);
        assert_eq!(snap.posture.total, 7);
        assert_eq!(snap.cards.len(), 7);
        assert_eq!(snap.sections.len(), 7);
        assert!(snap.capabilities.read_only);
        assert!(!snap.capabilities.mutators_enabled);

        let selinux_sec = snap
            .sections
            .iter()
            .find(|s| s.section == "selinux")
            .expect("selinux section");
        assert!(selinux_sec
            .metrics
            .iter()
            .any(|m| m.label_en == "Policy Booleans"));

        let fw_sec = snap
            .sections
            .iter()
            .find(|s| s.section == "firewall")
            .expect("firewall section");
        assert!(fw_sec.metrics.iter().any(|m| m.label_en == "Rich Rules"));
    }

    #[test]
    fn test_count_sysfs_booleans_readonly_helper() {
        let dir = std::env::temp_dir().join(format!("akrep_sysfs_bools_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("virt_use_kvm"), "1   1\n").unwrap();
        std::fs::write(dir.join("httpd_can_network_connect"), "0   0\n").unwrap();
        std::fs::write(dir.join("container_manage_cgroup"), "1   1\n").unwrap();

        let counts = crate::collectors::selinux::count_sysfs_booleans_at(&dir);
        assert_eq!(counts, Some((2, 3)));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
