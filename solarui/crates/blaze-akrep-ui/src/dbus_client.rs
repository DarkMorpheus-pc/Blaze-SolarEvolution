use blaze_sentinel_core::AkrepSnapshot;
use futures_util::stream::StreamExt;
use std::time::Duration;
use zbus::{Connection, MatchRule, MessageStream};

pub const AKREP_BUS_NAME: &str = "tr.com.blazeos.Akrep1";
pub const AKREP_OBJECT_PATH: &str = "/tr/com/blazeos/Akrep1";
pub const AKREP_INTERFACE_NAME: &str = "tr.com.blazeos.Akrep1.Manager";

#[derive(Debug, Clone)]
pub enum UiCommand {
    Refresh,
    Stop,
}

#[derive(Debug, Clone)]
pub enum UiEvent {
    Snapshot(Box<AkrepSnapshot>),
    DaemonOffline(String),
}

async fn connect_bus() -> zbus::Result<Connection> {
    if std::env::var("BLAZE_AKREP_BUS").ok().as_deref() == Some("session") {
        Connection::session().await
    } else {
        Connection::system().await
    }
}

async fn fetch_snapshot(conn: &Connection) -> Result<AkrepSnapshot, String> {
    let proxy = zbus::Proxy::new(conn, AKREP_BUS_NAME, AKREP_OBJECT_PATH, AKREP_INTERFACE_NAME)
        .await
        .map_err(|e| format!("D-Bus proxy error ({}): {}", AKREP_BUS_NAME, e))?;
    let json_str: String = proxy
        .call("GetSnapshot", &())
        .await
        .map_err(|e| format!("{} unreachable: {}", AKREP_BUS_NAME, e))?;
    serde_json::from_str::<AkrepSnapshot>(&json_str)
        .map_err(|e| format!("Snapshot JSON parse error: {}", e))
}

pub fn spawn_dbus_worker() -> (async_channel::Sender<UiCommand>, async_channel::Receiver<UiEvent>) {
    let (cmd_tx, cmd_rx) = async_channel::bounded::<UiCommand>(16);
    let (evt_tx, evt_rx) = async_channel::bounded::<UiEvent>(16);

    std::thread::Builder::new()
        .name("akrep-dbus-worker".to_string())
        .spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                let _ = evt_tx.send_blocking(UiEvent::DaemonOffline(
                    "Tokio çalışma zamanı başlatılamadı.".to_string(),
                ));
                return;
            };

            rt.block_on(async move {
                let mut conn_opt: Option<Connection> = connect_bus().await.ok();

                let push_once = |conn_opt: &mut Option<Connection>, evt_tx: &async_channel::Sender<UiEvent>| {
                    let _ = conn_opt;
                    let _ = evt_tx;
                };
                let _ = push_once;

                // Initial fetch
                match &conn_opt {
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

                // Subscribe to Akrep1 signals + NameOwnerChanged when bus available
                let mut akrep_sig_stream = if let Some(c) = &conn_opt {
                    if let Ok(rule) = MatchRule::builder()
                        .msg_type(zbus::message::Type::Signal)
                        .interface(AKREP_INTERFACE_NAME)
                    {
                        MessageStream::for_match_rule(rule.build(), c, Some(8)).await.ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                let mut owner_sig_stream = if let Some(c) = &conn_opt {
                    if let Ok(rule) = MatchRule::builder()
                        .msg_type(zbus::message::Type::Signal)
                        .interface("org.freedesktop.DBus")
                        .and_then(|b| b.member("NameOwnerChanged"))
                        .and_then(|b| b.arg(0, AKREP_BUS_NAME))
                    {
                        MessageStream::for_match_rule(rule.build(), c, Some(8)).await.ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                loop {
                    tokio::select! {
                        cmd = cmd_rx.recv() => {
                            match cmd {
                                Ok(UiCommand::Refresh) => {
                                    if conn_opt.is_none() {
                                        conn_opt = connect_bus().await.ok();
                                    }
                                    if let Some(c) = &conn_opt {
                                        match fetch_snapshot(c).await {
                                            Ok(snap) => {
                                                let _ = evt_tx.send(UiEvent::Snapshot(Box::new(snap))).await;
                                            }
                                            Err(e) => {
                                                let _ = evt_tx.send(UiEvent::DaemonOffline(e)).await;
                                            }
                                        }
                                    } else {
                                        let _ = evt_tx.send(UiEvent::DaemonOffline(
                                            "Sistem D-Bus bağlantısı kurulamadı.".to_string(),
                                        )).await;
                                    }
                                }
                                Ok(UiCommand::Stop) | Err(_) => break,
                            }
                        }
                        msg = async {
                            match akrep_sig_stream.as_mut() {
                                Some(s) => s.next().await,
                                None => std::future::pending().await,
                            }
                        } => {
                            if msg.is_some() {
                                if let Some(c) = &conn_opt {
                                    if let Ok(snap) = fetch_snapshot(c).await {
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
                                if let Some(c) = &conn_opt {
                                    match fetch_snapshot(c).await {
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
        .expect("spawn akrep dbus thread");

    (cmd_tx, evt_rx)
}
