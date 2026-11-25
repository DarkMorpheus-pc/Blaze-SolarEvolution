use crate::transaction_engine::{
    write_json_atomic_0600, ApplyOutcome, MutationBackend, PreviewOutcome,
};
use async_trait::async_trait;
use blaze_akrep_common::{
    build_cidr_port_rich_rule, build_cidr_service_rich_rule, redact_secrets,
    validate_network_interface, validate_persistence, validate_port_spec, validate_service_name,
    validate_zone_name, ActionRequest, AkrepError, FirewallProto, FirewallStateSnapshot,
    FirewallZoneState, PlanDiffItem, RulePersistence, StateFingerprint, TransactionRecord,
};
use blaze_sentinel_core::sha256_hex;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use zbus::zvariant::OwnedObjectPath;
use zbus::Connection;

const FW_DEST: &str = "org.fedoraproject.FirewallD1";
const FW_PATH: &str = "/org/fedoraproject/FirewallD1";
const FW_IFACE: &str = "org.fedoraproject.FirewallD1";
const FW_ZONE_IFACE: &str = "org.fedoraproject.FirewallD1.zone";
const FW_CONFIG_PATH: &str = "/org/fedoraproject/FirewallD1/config";
const FW_CONFIG_IFACE: &str = "org.fedoraproject.FirewallD1.config";
const FW_CONFIG_ZONE_IFACE: &str = "org.fedoraproject.FirewallD1.config.zone";

const DBUS_DEST: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_IFACE: &str = "org.freedesktop.DBus";

#[async_trait]
pub trait FirewallBackend: Send + Sync {
    async fn is_running(&self) -> Result<bool, AkrepError>;
    async fn get_default_zone(&self) -> Result<String, AkrepError>;
    async fn list_zones(&self) -> Result<Vec<String>, AkrepError>;
    async fn get_active_zones(&self) -> Result<BTreeMap<String, String>, AkrepError>;
    async fn get_zone_state(&self, zone: &str) -> Result<FirewallZoneState, AkrepError>;

    async fn add_runtime_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
        timeout_secs: u32,
    ) -> Result<(), AkrepError>;
    async fn remove_runtime_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError>;
    async fn add_permanent_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError>;
    async fn remove_permanent_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError>;

    async fn add_runtime_service(
        &self,
        zone: &str,
        service: &str,
        timeout_secs: u32,
    ) -> Result<(), AkrepError>;
    async fn remove_runtime_service(&self, zone: &str, service: &str) -> Result<(), AkrepError>;
    async fn add_permanent_service(&self, zone: &str, service: &str) -> Result<(), AkrepError>;
    async fn remove_permanent_service(&self, zone: &str, service: &str) -> Result<(), AkrepError>;

    async fn add_runtime_rich_rule(
        &self,
        zone: &str,
        rule: &str,
        timeout_secs: u32,
    ) -> Result<(), AkrepError>;
    async fn remove_runtime_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError>;
    async fn add_permanent_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError>;
    async fn remove_permanent_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError>;

    async fn bind_runtime_interface(&self, zone: &str, interface: &str) -> Result<(), AkrepError>;
    async fn unbind_runtime_interface(&self, zone: &str, interface: &str)
        -> Result<(), AkrepError>;
    async fn bind_permanent_interface(&self, zone: &str, interface: &str)
        -> Result<(), AkrepError>;
    async fn unbind_permanent_interface(
        &self,
        zone: &str,
        interface: &str,
    ) -> Result<(), AkrepError>;
}

fn map_fw_dbus_error(err: zbus::Error) -> AkrepError {
    let raw = redact_secrets(&err.to_string());
    if raw.contains("ServiceUnknown")
        || raw.contains("NameHasNoOwner")
        || raw.contains("Disconnected")
        || raw.contains("NoReply")
    {
        return AkrepError::BackendUnavailable(format!(
            "firewalld is inactive or unreachable on system D-Bus: {}",
            raw
        ));
    }
    if raw.contains("INVALID_ZONE")
        || raw.contains("INVALID_PORT")
        || raw.contains("INVALID_PROTOCOL")
        || raw.contains("INVALID_SERVICE")
        || raw.contains("INVALID_RULE")
        || raw.contains("INVALID_INTERFACE")
    {
        return AkrepError::InvalidInput(format!("firewalld rejected parameter: {}", raw));
    }
    AkrepError::ApplyFailed(format!("firewalld D-Bus error: {}", raw))
}

fn is_already_enabled_err(err: &zbus::Error) -> bool {
    err.to_string().contains("ALREADY_ENABLED")
}

fn is_not_enabled_err(err: &zbus::Error) -> bool {
    err.to_string().contains("NOT_ENABLED")
}

fn normalize_zone_state(mut z: FirewallZoneState) -> FirewallZoneState {
    z.runtime_interfaces.sort();
    z.runtime_interfaces.dedup();
    z.permanent_interfaces.sort();
    z.permanent_interfaces.dedup();

    z.runtime_services.sort();
    z.runtime_services.dedup();
    z.permanent_services.sort();
    z.permanent_services.dedup();

    z.runtime_ports.sort();
    z.runtime_ports.dedup();
    z.permanent_ports.sort();
    z.permanent_ports.dedup();

    z.runtime_rich_rules.sort();
    z.runtime_rich_rules.dedup();
    z.permanent_rich_rules.sort();
    z.permanent_rich_rules.dedup();

    z.active = !z.runtime_interfaces.is_empty() || z.is_default;
    z
}

pub struct DbusFirewallBackend {
    conn: Connection,
}

impl DbusFirewallBackend {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }

    async fn resolve_config_zone_path(&self, zone: &str) -> Result<OwnedObjectPath, AkrepError> {
        validate_zone_name(zone)?;
        let reply = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_CONFIG_PATH,
                Some(FW_CONFIG_IFACE),
                "getZoneByName",
                &(zone,),
            )
            .await
            .map_err(map_fw_dbus_error)?;
        reply
            .body()
            .deserialize::<OwnedObjectPath>()
            .map_err(map_fw_dbus_error)
    }

    pub fn decode_ports_reply(
        reply: &zbus::message::Message,
    ) -> Result<Vec<(String, String)>, AkrepError> {
        if let Ok(nested) = reply.body().deserialize::<Vec<Vec<String>>>() {
            let mut out = Vec::new();
            for item in nested {
                if item.len() >= 2 {
                    out.push((item[0].clone(), item[1].to_ascii_lowercase()));
                }
            }
            return Ok(out);
        }
        let pairs = reply
            .body()
            .deserialize::<Vec<(String, String)>>()
            .map_err(map_fw_dbus_error)?;
        Ok(pairs
            .into_iter()
            .map(|(p, pr)| (p, pr.to_ascii_lowercase()))
            .collect())
    }
}

#[async_trait]
impl FirewallBackend for DbusFirewallBackend {
    async fn is_running(&self) -> Result<bool, AkrepError> {
        let reply = self
            .conn
            .call_method(
                Some(DBUS_DEST),
                DBUS_PATH,
                Some(DBUS_IFACE),
                "NameHasOwner",
                &(FW_DEST,),
            )
            .await;
        match reply {
            Ok(msg) => Ok(msg.body().deserialize::<bool>().unwrap_or(false)),
            Err(_) => Ok(false),
        }
    }

    async fn get_default_zone(&self) -> Result<String, AkrepError> {
        let reply = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_IFACE),
                "getDefaultZone",
                &(),
            )
            .await
            .map_err(map_fw_dbus_error)?;
        reply
            .body()
            .deserialize::<String>()
            .map_err(map_fw_dbus_error)
    }

    async fn list_zones(&self) -> Result<Vec<String>, AkrepError> {
        let reply = self
            .conn
            .call_method(Some(FW_DEST), FW_PATH, Some(FW_ZONE_IFACE), "getZones", &())
            .await
            .map_err(map_fw_dbus_error)?;
        let mut zones = reply
            .body()
            .deserialize::<Vec<String>>()
            .map_err(map_fw_dbus_error)?;
        zones.sort();
        zones.dedup();
        Ok(zones)
    }

    async fn get_active_zones(&self) -> Result<BTreeMap<String, String>, AkrepError> {
        let reply = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "getActiveZones",
                &(),
            )
            .await
            .map_err(map_fw_dbus_error)?;
        let raw: HashMap<String, HashMap<String, Vec<String>>> =
            reply.body().deserialize().map_err(map_fw_dbus_error)?;

        let mut mapping = BTreeMap::new();
        for (zone_name, details) in raw {
            if let Some(ifaces) = details.get("interfaces") {
                for iface in ifaces {
                    mapping.insert(iface.clone(), zone_name.clone());
                }
            }
        }
        Ok(mapping)
    }

    async fn get_zone_state(&self, zone: &str) -> Result<FirewallZoneState, AkrepError> {
        validate_zone_name(zone)?;
        let default_zone = self.get_default_zone().await?;
        let cfg_path = self.resolve_config_zone_path(zone).await?;

        let rt_ifaces: Vec<String> = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "getInterfaces",
                &(zone,),
            )
            .await
            .map_err(map_fw_dbus_error)?
            .body()
            .deserialize()
            .map_err(map_fw_dbus_error)?;

        let rt_services: Vec<String> = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "getServices",
                &(zone,),
            )
            .await
            .map_err(map_fw_dbus_error)?
            .body()
            .deserialize()
            .map_err(map_fw_dbus_error)?;

        let rt_ports_msg = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "getPorts",
                &(zone,),
            )
            .await
            .map_err(map_fw_dbus_error)?;
        let rt_ports = Self::decode_ports_reply(&rt_ports_msg)?;

        let rt_rich: Vec<String> = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "getRichRules",
                &(zone,),
            )
            .await
            .map_err(map_fw_dbus_error)?
            .body()
            .deserialize()
            .map_err(map_fw_dbus_error)?;

        let perm_ifaces: Vec<String> = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "getInterfaces",
                &(),
            )
            .await
            .map_err(map_fw_dbus_error)?
            .body()
            .deserialize()
            .map_err(map_fw_dbus_error)?;

        let perm_services: Vec<String> = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "getServices",
                &(),
            )
            .await
            .map_err(map_fw_dbus_error)?
            .body()
            .deserialize()
            .map_err(map_fw_dbus_error)?;

        let perm_ports_msg = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "getPorts",
                &(),
            )
            .await
            .map_err(map_fw_dbus_error)?;
        let perm_ports = Self::decode_ports_reply(&perm_ports_msg)?;

        let perm_rich: Vec<String> = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "getRichRules",
                &(),
            )
            .await
            .map_err(map_fw_dbus_error)?
            .body()
            .deserialize()
            .map_err(map_fw_dbus_error)?;

        Ok(normalize_zone_state(FirewallZoneState {
            name: zone.to_string(),
            is_default: zone == default_zone,
            active: !rt_ifaces.is_empty() || zone == default_zone,
            runtime_interfaces: rt_ifaces,
            permanent_interfaces: perm_ifaces,
            runtime_services: rt_services,
            permanent_services: perm_services,
            runtime_ports: rt_ports,
            permanent_ports: perm_ports,
            runtime_rich_rules: rt_rich,
            permanent_rich_rules: perm_rich,
        }))
    }

    async fn add_runtime_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
        timeout_secs: u32,
    ) -> Result<(), AkrepError> {
        let timeout_i32 = i32::try_from(timeout_secs)
            .map_err(|_| AkrepError::InvalidInput("timeout_secs exceeds i32::MAX".to_string()))?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "addPort",
                &(zone, port, proto.as_str(), timeout_i32),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if timeout_secs == 0 && is_already_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn remove_runtime_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError> {
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "removePort",
                &(zone, port, proto.as_str()),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_not_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn add_permanent_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError> {
        let cfg_path = self.resolve_config_zone_path(zone).await?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "addPort",
                &(port, proto.as_str()),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_already_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn remove_permanent_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError> {
        let cfg_path = self.resolve_config_zone_path(zone).await?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "removePort",
                &(port, proto.as_str()),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_not_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn add_runtime_service(
        &self,
        zone: &str,
        service: &str,
        timeout_secs: u32,
    ) -> Result<(), AkrepError> {
        let timeout_i32 = i32::try_from(timeout_secs)
            .map_err(|_| AkrepError::InvalidInput("timeout_secs exceeds i32::MAX".to_string()))?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "addService",
                &(zone, service, timeout_i32),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if timeout_secs == 0 && is_already_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn remove_runtime_service(&self, zone: &str, service: &str) -> Result<(), AkrepError> {
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "removeService",
                &(zone, service),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_not_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn add_permanent_service(&self, zone: &str, service: &str) -> Result<(), AkrepError> {
        let cfg_path = self.resolve_config_zone_path(zone).await?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "addService",
                &(service,),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_already_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn remove_permanent_service(&self, zone: &str, service: &str) -> Result<(), AkrepError> {
        let cfg_path = self.resolve_config_zone_path(zone).await?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "removeService",
                &(service,),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_not_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn add_runtime_rich_rule(
        &self,
        zone: &str,
        rule: &str,
        timeout_secs: u32,
    ) -> Result<(), AkrepError> {
        let timeout_i32 = i32::try_from(timeout_secs)
            .map_err(|_| AkrepError::InvalidInput("timeout_secs exceeds i32::MAX".to_string()))?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "addRichRule",
                &(zone, rule, timeout_i32),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if timeout_secs == 0 && is_already_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn remove_runtime_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError> {
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "removeRichRule",
                &(zone, rule),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_not_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn add_permanent_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError> {
        let cfg_path = self.resolve_config_zone_path(zone).await?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "addRichRule",
                &(rule,),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_already_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn remove_permanent_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError> {
        let cfg_path = self.resolve_config_zone_path(zone).await?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "removeRichRule",
                &(rule,),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_not_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn bind_runtime_interface(&self, zone: &str, interface: &str) -> Result<(), AkrepError> {
        self.conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "changeZoneOfInterface",
                &(zone, interface),
            )
            .await
            .map_err(map_fw_dbus_error)?;
        Ok(())
    }

    async fn unbind_runtime_interface(
        &self,
        zone: &str,
        interface: &str,
    ) -> Result<(), AkrepError> {
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                FW_PATH,
                Some(FW_ZONE_IFACE),
                "removeInterface",
                &(zone, interface),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_not_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }

    async fn bind_permanent_interface(
        &self,
        zone: &str,
        interface: &str,
    ) -> Result<(), AkrepError> {
        let all_zones = self.list_zones().await?;
        for z in &all_zones {
            if z != zone {
                self.unbind_permanent_interface(z, interface).await?;
            }
        }
        let cfg_path = self.resolve_config_zone_path(zone).await?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "addInterface",
                &(interface,),
            )
            .await;
        match res {
            Ok(_) => {}
            Err(e) if is_already_enabled_err(&e) => {}
            Err(e) => return Err(map_fw_dbus_error(e)),
        }

        // Confirm prior permanent zones no longer retain this interface
        for z in &all_zones {
            if z != zone {
                let other_path = self.resolve_config_zone_path(z).await?;
                let other_ifaces: Vec<String> = self
                    .conn
                    .call_method(
                        Some(FW_DEST),
                        other_path.as_str(),
                        Some(FW_CONFIG_ZONE_IFACE),
                        "getInterfaces",
                        &(),
                    )
                    .await
                    .map_err(map_fw_dbus_error)?
                    .body()
                    .deserialize()
                    .map_err(map_fw_dbus_error)?;
                if other_ifaces.iter().any(|i| i == interface) {
                    return Err(AkrepError::ApplyFailed(format!(
                        "Interface '{}' remained permanently bound to prior zone '{}'",
                        interface, z
                    )));
                }
            }
        }
        Ok(())
    }

    async fn unbind_permanent_interface(
        &self,
        zone: &str,
        interface: &str,
    ) -> Result<(), AkrepError> {
        let cfg_path = self.resolve_config_zone_path(zone).await?;
        let res = self
            .conn
            .call_method(
                Some(FW_DEST),
                cfg_path.as_str(),
                Some(FW_CONFIG_ZONE_IFACE),
                "removeInterface",
                &(interface,),
            )
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_not_enabled_err(&e) => Ok(()),
            Err(e) => Err(map_fw_dbus_error(e)),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct CanonicalZoneFingerprintTuple<'a> {
    zone: &'a str,
    runtime_interfaces: &'a [String],
    permanent_interfaces: &'a [String],
    runtime_services: &'a [String],
    permanent_services: &'a [String],
    runtime_ports: &'a [(String, String)],
    permanent_ports: &'a [(String, String)],
    runtime_rich_rules: &'a [String],
    permanent_rich_rules: &'a [String],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FirewallSnapshotFile {
    zones: Vec<FirewallZoneState>,
}

pub struct FirewallAdapter {
    backend: Arc<dyn FirewallBackend>,
}

impl FirewallAdapter {
    pub fn new(backend: Arc<dyn FirewallBackend>) -> Self {
        Self { backend }
    }

    pub fn new_dbus(conn: Connection) -> Self {
        Self::new(Arc::new(DbusFirewallBackend::new(conn)))
    }

    pub fn backend(&self) -> &Arc<dyn FirewallBackend> {
        &self.backend
    }

    async fn ensure_running(&self) -> Result<(), AkrepError> {
        if !self.backend.is_running().await? {
            return Err(AkrepError::BackendUnavailable(
                "firewalld service is inactive or unreachable on system D-Bus".to_string(),
            ));
        }
        Ok(())
    }

    pub fn zone_fingerprint(state: &FirewallZoneState) -> Result<StateFingerprint, AkrepError> {
        let norm = normalize_zone_state(state.clone());
        let tuple = CanonicalZoneFingerprintTuple {
            zone: &norm.name,
            runtime_interfaces: &norm.runtime_interfaces,
            permanent_interfaces: &norm.permanent_interfaces,
            runtime_services: &norm.runtime_services,
            permanent_services: &norm.permanent_services,
            runtime_ports: &norm.runtime_ports,
            permanent_ports: &norm.permanent_ports,
            runtime_rich_rules: &norm.runtime_rich_rules,
            permanent_rich_rules: &norm.permanent_rich_rules,
        };
        let bytes = serde_json::to_vec(&tuple)
            .map_err(|e| AkrepError::Internal(format!("zone fingerprint encode error: {}", e)))?;
        Ok(StateFingerprint {
            target: format!("firewall.zone.{}", norm.name),
            sha256: sha256_hex(&bytes),
            captured_at: Utc::now().to_rfc3339(),
        })
    }

    async fn resolve_affected_zones(&self, req: &ActionRequest) -> Result<Vec<String>, AkrepError> {
        let mut zones = BTreeSet::new();
        match req {
            ActionRequest::FirewallAddPort { zone, .. }
            | ActionRequest::FirewallRemovePort { zone, .. }
            | ActionRequest::FirewallAddService { zone, .. }
            | ActionRequest::FirewallRemoveService { zone, .. } => {
                zones.insert(zone.clone());
            }
            ActionRequest::FirewallBindInterface {
                zone, interface, ..
            } => {
                zones.insert(zone.clone());
                let all = self.backend.list_zones().await?;
                for z in all {
                    let st = self.backend.get_zone_state(&z).await?;
                    if st.runtime_interfaces.iter().any(|i| i == interface)
                        || st.permanent_interfaces.iter().any(|i| i == interface)
                    {
                        zones.insert(z);
                    }
                }
            }
            _ => {
                return Err(AkrepError::InvalidInput(
                    "FirewallAdapter received non-Firewall ActionRequest".to_string(),
                ));
            }
        }
        Ok(zones.into_iter().collect())
    }

    pub async fn inspect_state(
        &self,
        zone_filter: Option<&str>,
    ) -> Result<FirewallStateSnapshot, AkrepError> {
        let filter = zone_filter.map(str::trim).filter(|s| !s.is_empty());
        if let Some(z) = filter {
            validate_zone_name(z)?;
        }

        if !self.backend.is_running().await? {
            return Ok(FirewallStateSnapshot {
                running: false,
                default_zone: String::new(),
                zones: Vec::new(),
                active_zones: BTreeMap::new(),
                zone_details: Vec::new(),
            });
        }

        let default_zone = self.backend.get_default_zone().await?;
        let zones = self.backend.list_zones().await?;
        let active_zones = self.backend.get_active_zones().await?;

        let mut zone_details = Vec::new();
        if let Some(target_zone) = filter {
            if !zones.iter().any(|z| z == target_zone) {
                return Err(AkrepError::InvalidInput(format!(
                    "Unknown firewall zone '{}'",
                    target_zone
                )));
            }
            zone_details.push(self.backend.get_zone_state(target_zone).await?);
        } else {
            for z in &zones {
                zone_details.push(self.backend.get_zone_state(z).await?);
            }
        }

        Ok(FirewallStateSnapshot {
            running: true,
            default_zone,
            zones,
            active_zones,
            zone_details,
        })
    }

    async fn reconcile_zone_to_snapshot(&self, prev: &FirewallZoneState) -> Result<(), AkrepError> {
        let curr = self.backend.get_zone_state(&prev.name).await?;

        // 1. Runtime ports
        for (p, pr) in &curr.runtime_ports {
            if !prev.runtime_ports.contains(&(p.clone(), pr.clone())) {
                let proto = parse_proto_str(pr)?;
                self.backend
                    .remove_runtime_port(&prev.name, p, proto)
                    .await?;
            }
        }
        for (p, pr) in &prev.runtime_ports {
            if !curr.runtime_ports.contains(&(p.clone(), pr.clone())) {
                let proto = parse_proto_str(pr)?;
                self.backend
                    .add_runtime_port(&prev.name, p, proto, 0)
                    .await?;
            }
        }

        // 2. Permanent ports
        for (p, pr) in &curr.permanent_ports {
            if !prev.permanent_ports.contains(&(p.clone(), pr.clone())) {
                let proto = parse_proto_str(pr)?;
                self.backend
                    .remove_permanent_port(&prev.name, p, proto)
                    .await?;
            }
        }
        for (p, pr) in &prev.permanent_ports {
            if !curr.permanent_ports.contains(&(p.clone(), pr.clone())) {
                let proto = parse_proto_str(pr)?;
                self.backend
                    .add_permanent_port(&prev.name, p, proto)
                    .await?;
            }
        }

        // 3. Runtime services
        for s in &curr.runtime_services {
            if !prev.runtime_services.contains(s) {
                self.backend.remove_runtime_service(&prev.name, s).await?;
            }
        }
        for s in &prev.runtime_services {
            if !curr.runtime_services.contains(s) {
                self.backend.add_runtime_service(&prev.name, s, 0).await?;
            }
        }

        // 4. Permanent services
        for s in &curr.permanent_services {
            if !prev.permanent_services.contains(s) {
                self.backend.remove_permanent_service(&prev.name, s).await?;
            }
        }
        for s in &prev.permanent_services {
            if !curr.permanent_services.contains(s) {
                self.backend.add_permanent_service(&prev.name, s).await?;
            }
        }

        // 5. Runtime rich rules
        for r in &curr.runtime_rich_rules {
            if !prev.runtime_rich_rules.contains(r) {
                self.backend.remove_runtime_rich_rule(&prev.name, r).await?;
            }
        }
        for r in &prev.runtime_rich_rules {
            if !curr.runtime_rich_rules.contains(r) {
                self.backend.add_runtime_rich_rule(&prev.name, r, 0).await?;
            }
        }

        // 6. Permanent rich rules
        for r in &curr.permanent_rich_rules {
            if !prev.permanent_rich_rules.contains(r) {
                self.backend
                    .remove_permanent_rich_rule(&prev.name, r)
                    .await?;
            }
        }
        for r in &prev.permanent_rich_rules {
            if !curr.permanent_rich_rules.contains(r) {
                self.backend.add_permanent_rich_rule(&prev.name, r).await?;
            }
        }

        Ok(())
    }
}

fn parse_proto_str(s: &str) -> Result<FirewallProto, AkrepError> {
    match s.to_ascii_lowercase().as_str() {
        "tcp" => Ok(FirewallProto::Tcp),
        "udp" => Ok(FirewallProto::Udp),
        other => Err(AkrepError::InvalidInput(format!(
            "Unsupported firewall protocol '{}'",
            other
        ))),
    }
}

#[async_trait]
impl MutationBackend for FirewallAdapter {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        req.validate_red_lines()?;
        self.ensure_running().await?;

        let affected_zones = self.resolve_affected_zones(req).await?;
        let mut fetched_states = BTreeMap::new();
        let mut pre_fingerprints = Vec::new();
        for z in &affected_zones {
            let st = self.backend.get_zone_state(z).await?;
            pre_fingerprints.push(Self::zone_fingerprint(&st)?);
            fetched_states.insert(z.clone(), st);
        }

        let risk = req.default_risk_level();
        let mut warnings_tr = Vec::new();
        let mut warnings_en = Vec::new();

        let format_presence = |in_rt: bool, in_perm: bool| -> String {
            format!(
                "runtime={}, permanent={}",
                if in_rt { "enabled" } else { "disabled" },
                if in_perm { "enabled" } else { "disabled" }
            )
        };

        let diff = match req {
            ActionRequest::FirewallAddPort {
                zone,
                port,
                proto,
                mode,
                source_cidr,
            } => {
                validate_zone_name(zone)?;
                validate_port_spec(port)?;
                validate_persistence(*mode)?;
                let st = fetched_states.get(zone).ok_or_else(|| {
                    AkrepError::Internal("missing fetched zone state".to_string())
                })?;

                if matches!(mode, RulePersistence::Permanent) && source_cidr.is_none() {
                    warnings_tr.push(format!(
                        "'{}' bolgesinde {}/{} portu kaynak IP kisitlamasi olmadan kalici olarak aciliyor.",
                        zone,
                        port,
                        proto.as_str()
                    ));
                    warnings_en.push(format!(
                        "Opening port {}/{} permanently in zone '{}' without source CIDR restriction.",
                        port,
                        proto.as_str(),
                        zone
                    ));
                }

                let (rule_desc, in_rt, in_perm) = match source_cidr {
                    Some(cidr) => {
                        let r = build_cidr_port_rich_rule(cidr, port, *proto)?;
                        let rt = st.runtime_rich_rules.contains(&r);
                        let pm = st.permanent_rich_rules.contains(&r);
                        (r, rt, pm)
                    }
                    None => {
                        let pair = (port.clone(), proto.as_str().to_string());
                        let rt = st.runtime_ports.contains(&pair);
                        let pm = st.permanent_ports.contains(&pair);
                        (format!("{}/{}", port, proto.as_str()), rt, pm)
                    }
                };
                PlanDiffItem {
                    target: format!("firewall.zone.{}", zone),
                    current_value: format_presence(in_rt, in_perm),
                    desired_value: format!("allow {} ({:?})", rule_desc, mode),
                    description_tr: format!(
                        "'{}' bolgesinde '{}' kurali etkinlestirilecek.",
                        zone, rule_desc
                    ),
                    description_en: format!(
                        "Enable rule '{}' in firewall zone '{}'.",
                        rule_desc, zone
                    ),
                }
            }
            ActionRequest::FirewallRemovePort {
                zone,
                port,
                proto,
                permanent,
                source_cidr,
            } => {
                validate_zone_name(zone)?;
                validate_port_spec(port)?;
                let st = fetched_states.get(zone).ok_or_else(|| {
                    AkrepError::Internal("missing fetched zone state".to_string())
                })?;
                let (rule_desc, in_rt, in_perm) = match source_cidr {
                    Some(cidr) => {
                        let r = build_cidr_port_rich_rule(cidr, port, *proto)?;
                        let rt = st.runtime_rich_rules.contains(&r);
                        let pm = st.permanent_rich_rules.contains(&r);
                        (r, rt, pm)
                    }
                    None => {
                        let pair = (port.clone(), proto.as_str().to_string());
                        let rt = st.runtime_ports.contains(&pair);
                        let pm = st.permanent_ports.contains(&pair);
                        (format!("{}/{}", port, proto.as_str()), rt, pm)
                    }
                };
                PlanDiffItem {
                    target: format!("firewall.zone.{}", zone),
                    current_value: format_presence(in_rt, in_perm),
                    desired_value: format!(
                        "runtime=disabled, permanent={}",
                        if *permanent {
                            "disabled"
                        } else if in_perm {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    ),
                    description_tr: format!(
                        "'{}' bolgesinden '{}' port kurali kaldirilacak.",
                        zone, rule_desc
                    ),
                    description_en: format!(
                        "Remove port rule '{}' from zone '{}'.",
                        rule_desc, zone
                    ),
                }
            }
            ActionRequest::FirewallAddService {
                zone,
                service,
                mode,
                source_cidr,
            } => {
                validate_zone_name(zone)?;
                validate_service_name(service)?;
                validate_persistence(*mode)?;
                let st = fetched_states.get(zone).ok_or_else(|| {
                    AkrepError::Internal("missing fetched zone state".to_string())
                })?;
                let (rule_desc, in_rt, in_perm) = match source_cidr {
                    Some(cidr) => {
                        let r = build_cidr_service_rich_rule(cidr, service)?;
                        let rt = st.runtime_rich_rules.contains(&r);
                        let pm = st.permanent_rich_rules.contains(&r);
                        (r, rt, pm)
                    }
                    None => {
                        let rt = st.runtime_services.contains(service);
                        let pm = st.permanent_services.contains(service);
                        (service.clone(), rt, pm)
                    }
                };
                PlanDiffItem {
                    target: format!("firewall.zone.{}", zone),
                    current_value: format_presence(in_rt, in_perm),
                    desired_value: format!("allow service {} ({:?})", rule_desc, mode),
                    description_tr: format!(
                        "'{}' bolgesinde '{}' servisine izin verilecek.",
                        zone, rule_desc
                    ),
                    description_en: format!("Allow service '{}' in zone '{}'.", rule_desc, zone),
                }
            }
            ActionRequest::FirewallRemoveService {
                zone,
                service,
                permanent,
                source_cidr,
            } => {
                validate_zone_name(zone)?;
                validate_service_name(service)?;
                let st = fetched_states.get(zone).ok_or_else(|| {
                    AkrepError::Internal("missing fetched zone state".to_string())
                })?;
                let (rule_desc, in_rt, in_perm) = match source_cidr {
                    Some(cidr) => {
                        let r = build_cidr_service_rich_rule(cidr, service)?;
                        let rt = st.runtime_rich_rules.contains(&r);
                        let pm = st.permanent_rich_rules.contains(&r);
                        (r, rt, pm)
                    }
                    None => {
                        let rt = st.runtime_services.contains(service);
                        let pm = st.permanent_services.contains(service);
                        (service.clone(), rt, pm)
                    }
                };
                PlanDiffItem {
                    target: format!("firewall.zone.{}", zone),
                    current_value: format_presence(in_rt, in_perm),
                    desired_value: format!(
                        "runtime=disabled, permanent={}",
                        if *permanent {
                            "disabled"
                        } else if in_perm {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    ),
                    description_tr: format!(
                        "'{}' bolgesinden '{}' servis izni kaldirilacak.",
                        zone, rule_desc
                    ),
                    description_en: format!("Remove service '{}' from zone '{}'.", rule_desc, zone),
                }
            }
            ActionRequest::FirewallBindInterface {
                zone,
                interface,
                permanent,
            } => {
                validate_zone_name(zone)?;
                validate_network_interface(interface)?;
                let active_map = self.backend.get_active_zones().await?;
                let prev_zone = active_map
                    .get(interface)
                    .cloned()
                    .unwrap_or_else(|| "<unbound>".to_string());
                PlanDiffItem {
                    target: format!("firewall.interface.{}", interface),
                    current_value: prev_zone,
                    desired_value: format!("{} (permanent={})", zone, permanent),
                    description_tr: format!(
                        "'{}' ag arayuzu '{}' guvenlik bolgesine atanacak.",
                        interface, zone
                    ),
                    description_en: format!(
                        "Bind network interface '{}' to firewall zone '{}'.",
                        interface, zone
                    ),
                }
            }
            _ => {
                return Err(AkrepError::InvalidInput(
                    "FirewallAdapter received non-Firewall ActionRequest".to_string(),
                ));
            }
        };

        Ok(PreviewOutcome {
            risk,
            diffs: vec![diff],
            pre_fingerprints,
            warnings_tr,
            warnings_en,
        })
    }

    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        req.validate_red_lines()?;
        self.ensure_running().await?;

        let affected_zones = self.resolve_affected_zones(req).await?;
        let mut snap_zones = Vec::new();
        for z in &affected_zones {
            snap_zones.push(self.backend.get_zone_state(z).await?);
        }
        let snap = FirewallSnapshotFile {
            zones: snap_zones.clone(),
        };
        write_json_atomic_0600(&snapshot_dir.join("firewall_zones.json"), &snap)?;

        let summary = match req {
            ActionRequest::FirewallAddPort {
                zone,
                port,
                proto,
                mode,
                source_cidr,
            } => {
                validate_persistence(*mode)?;
                match source_cidr {
                    None => match *mode {
                        RulePersistence::Runtime => {
                            self.backend.add_runtime_port(zone, port, *proto, 0).await?;
                        }
                        RulePersistence::TimedRuntime { seconds } => {
                            self.backend
                                .add_runtime_port(zone, port, *proto, seconds)
                                .await?;
                        }
                        RulePersistence::Permanent => {
                            self.backend.add_permanent_port(zone, port, *proto).await?;
                            self.backend.add_runtime_port(zone, port, *proto, 0).await?;
                        }
                    },
                    Some(cidr) => {
                        let rule = build_cidr_port_rich_rule(cidr, port, *proto)?;
                        match *mode {
                            RulePersistence::Runtime => {
                                self.backend.add_runtime_rich_rule(zone, &rule, 0).await?;
                            }
                            RulePersistence::TimedRuntime { seconds } => {
                                self.backend
                                    .add_runtime_rich_rule(zone, &rule, seconds)
                                    .await?;
                            }
                            RulePersistence::Permanent => {
                                self.backend.add_permanent_rich_rule(zone, &rule).await?;
                                self.backend.add_runtime_rich_rule(zone, &rule, 0).await?;
                            }
                        }
                    }
                }
                format!(
                    "Added port {}/{} in zone '{}' ({:?})",
                    port,
                    proto.as_str(),
                    zone,
                    mode
                )
            }
            ActionRequest::FirewallRemovePort {
                zone,
                port,
                proto,
                permanent,
                source_cidr,
            } => {
                match source_cidr {
                    None => {
                        self.backend.remove_runtime_port(zone, port, *proto).await?;
                        if *permanent {
                            self.backend
                                .remove_permanent_port(zone, port, *proto)
                                .await?;
                        }
                    }
                    Some(cidr) => {
                        let rule = build_cidr_port_rich_rule(cidr, port, *proto)?;
                        self.backend.remove_runtime_rich_rule(zone, &rule).await?;
                        if *permanent {
                            self.backend.remove_permanent_rich_rule(zone, &rule).await?;
                        }
                    }
                }
                format!(
                    "Removed port {}/{} from zone '{}' (permanent={})",
                    port,
                    proto.as_str(),
                    zone,
                    permanent
                )
            }
            ActionRequest::FirewallAddService {
                zone,
                service,
                mode,
                source_cidr,
            } => {
                validate_persistence(*mode)?;
                match source_cidr {
                    None => match *mode {
                        RulePersistence::Runtime => {
                            self.backend.add_runtime_service(zone, service, 0).await?;
                        }
                        RulePersistence::TimedRuntime { seconds } => {
                            self.backend
                                .add_runtime_service(zone, service, seconds)
                                .await?;
                        }
                        RulePersistence::Permanent => {
                            self.backend.add_permanent_service(zone, service).await?;
                            self.backend.add_runtime_service(zone, service, 0).await?;
                        }
                    },
                    Some(cidr) => {
                        let rule = build_cidr_service_rich_rule(cidr, service)?;
                        match *mode {
                            RulePersistence::Runtime => {
                                self.backend.add_runtime_rich_rule(zone, &rule, 0).await?;
                            }
                            RulePersistence::TimedRuntime { seconds } => {
                                self.backend
                                    .add_runtime_rich_rule(zone, &rule, seconds)
                                    .await?;
                            }
                            RulePersistence::Permanent => {
                                self.backend.add_permanent_rich_rule(zone, &rule).await?;
                                self.backend.add_runtime_rich_rule(zone, &rule, 0).await?;
                            }
                        }
                    }
                }
                format!(
                    "Allowed service '{}' in zone '{}' ({:?})",
                    service, zone, mode
                )
            }
            ActionRequest::FirewallRemoveService {
                zone,
                service,
                permanent,
                source_cidr,
            } => {
                match source_cidr {
                    None => {
                        self.backend.remove_runtime_service(zone, service).await?;
                        if *permanent {
                            self.backend.remove_permanent_service(zone, service).await?;
                        }
                    }
                    Some(cidr) => {
                        let rule = build_cidr_service_rich_rule(cidr, service)?;
                        self.backend.remove_runtime_rich_rule(zone, &rule).await?;
                        if *permanent {
                            self.backend.remove_permanent_rich_rule(zone, &rule).await?;
                        }
                    }
                }
                format!(
                    "Removed service '{}' from zone '{}' (permanent={})",
                    service, zone, permanent
                )
            }
            ActionRequest::FirewallBindInterface {
                zone,
                interface,
                permanent,
            } => {
                self.backend.bind_runtime_interface(zone, interface).await?;
                if *permanent {
                    self.backend
                        .bind_permanent_interface(zone, interface)
                        .await?;
                }
                format!(
                    "Bound interface '{}' to zone '{}' (permanent={})",
                    interface, zone, permanent
                )
            }
            _ => {
                return Err(AkrepError::InvalidInput(
                    "FirewallAdapter received non-Firewall ActionRequest".to_string(),
                ));
            }
        };

        Ok(ApplyOutcome {
            pre_snapshot: serde_json::to_value(&snap)
                .map_err(|e| AkrepError::Internal(e.to_string()))?,
            target_state: serde_json::json!({
                "affected_zones": affected_zones,
                "request": req,
            }),
            summary,
        })
    }

    async fn verify(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        self.ensure_running().await?;

        match req {
            ActionRequest::FirewallAddPort {
                zone,
                port,
                proto,
                mode,
                source_cidr,
            } => {
                let st = self.backend.get_zone_state(zone).await?;
                match source_cidr {
                    None => {
                        let pair = (port.clone(), proto.as_str().to_string());
                        if !st.runtime_ports.contains(&pair) {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Port {}/{} missing from runtime zone '{}' after apply",
                                port,
                                proto.as_str(),
                                zone
                            )));
                        }
                        if matches!(mode, RulePersistence::Permanent)
                            && !st.permanent_ports.contains(&pair)
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Port {}/{} missing from permanent zone '{}' after apply",
                                port,
                                proto.as_str(),
                                zone
                            )));
                        }
                    }
                    Some(cidr) => {
                        let rule = build_cidr_port_rich_rule(cidr, port, *proto)?;
                        if !st.runtime_rich_rules.contains(&rule) {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Rich rule '{}' missing from runtime zone '{}'",
                                rule, zone
                            )));
                        }
                        if matches!(mode, RulePersistence::Permanent)
                            && !st.permanent_rich_rules.contains(&rule)
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Rich rule '{}' missing from permanent zone '{}'",
                                rule, zone
                            )));
                        }
                    }
                }
            }
            ActionRequest::FirewallRemovePort {
                zone,
                port,
                proto,
                permanent,
                source_cidr,
            } => {
                let st = self.backend.get_zone_state(zone).await?;
                match source_cidr {
                    None => {
                        let pair = (port.clone(), proto.as_str().to_string());
                        if st.runtime_ports.contains(&pair)
                            || (*permanent && st.permanent_ports.contains(&pair))
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Port {}/{} still present in zone '{}' after removal",
                                port,
                                proto.as_str(),
                                zone
                            )));
                        }
                    }
                    Some(cidr) => {
                        let rule = build_cidr_port_rich_rule(cidr, port, *proto)?;
                        if st.runtime_rich_rules.contains(&rule)
                            || (*permanent && st.permanent_rich_rules.contains(&rule))
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Rich rule '{}' still present in zone '{}' after removal",
                                rule, zone
                            )));
                        }
                    }
                }
            }
            ActionRequest::FirewallAddService {
                zone,
                service,
                mode,
                source_cidr,
            } => {
                let st = self.backend.get_zone_state(zone).await?;
                match source_cidr {
                    None => {
                        if !st.runtime_services.contains(service)
                            || (matches!(mode, RulePersistence::Permanent)
                                && !st.permanent_services.contains(service))
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Service '{}' missing from zone '{}' after apply",
                                service, zone
                            )));
                        }
                    }
                    Some(cidr) => {
                        let rule = build_cidr_service_rich_rule(cidr, service)?;
                        if !st.runtime_rich_rules.contains(&rule)
                            || (matches!(mode, RulePersistence::Permanent)
                                && !st.permanent_rich_rules.contains(&rule))
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Service rich rule '{}' missing from zone '{}'",
                                rule, zone
                            )));
                        }
                    }
                }
            }
            ActionRequest::FirewallRemoveService {
                zone,
                service,
                permanent,
                source_cidr,
            } => {
                let st = self.backend.get_zone_state(zone).await?;
                match source_cidr {
                    None => {
                        if st.runtime_services.contains(service)
                            || (*permanent && st.permanent_services.contains(service))
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Service '{}' still present in zone '{}' after removal",
                                service, zone
                            )));
                        }
                    }
                    Some(cidr) => {
                        let rule = build_cidr_service_rich_rule(cidr, service)?;
                        if st.runtime_rich_rules.contains(&rule)
                            || (*permanent && st.permanent_rich_rules.contains(&rule))
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Service rich rule '{}' still present in zone '{}'",
                                rule, zone
                            )));
                        }
                    }
                }
            }
            ActionRequest::FirewallBindInterface {
                zone,
                interface,
                permanent,
            } => {
                let st = self.backend.get_zone_state(zone).await?;
                if !st.runtime_interfaces.contains(interface)
                    || (*permanent && !st.permanent_interfaces.contains(interface))
                {
                    return Err(AkrepError::VerifyFailed(format!(
                        "Interface '{}' not bound to zone '{}' after apply",
                        interface, zone
                    )));
                }
                let tracked_zones: Vec<String> = expected
                    .target_state
                    .get("affected_zones")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or_default();
                for other_zone in &tracked_zones {
                    if other_zone != zone {
                        let other_st = self.backend.get_zone_state(other_zone).await?;
                        if other_st.runtime_interfaces.contains(interface)
                            || (*permanent && other_st.permanent_interfaces.contains(interface))
                        {
                            return Err(AkrepError::VerifyFailed(format!(
                                "Interface '{}' still bound to prior zone '{}' after move",
                                interface, other_zone
                            )));
                        }
                    }
                }
            }
            _ => {
                return Err(AkrepError::InvalidInput(
                    "FirewallAdapter received non-Firewall ActionRequest".to_string(),
                ));
            }
        }

        let zones_to_fp: Vec<String> = expected
            .target_state
            .get("affected_zones")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(self.resolve_affected_zones(req).await?);

        let mut post_fps = Vec::new();
        for z in &zones_to_fp {
            let st = self.backend.get_zone_state(z).await?;
            post_fps.push(Self::zone_fingerprint(&st)?);
        }
        Ok(post_fps)
    }

    async fn current_fingerprints(
        &self,
        req: &ActionRequest,
        reference_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        self.ensure_running().await?;

        let mut target_zones: Vec<String> = reference_fps
            .iter()
            .filter_map(|fp| {
                fp.target
                    .strip_prefix("firewall.zone.")
                    .map(ToString::to_string)
            })
            .collect();
        if target_zones.is_empty() {
            target_zones = self.resolve_affected_zones(req).await?;
        }
        target_zones.sort();
        target_zones.dedup();

        let mut fps = Vec::new();
        for z in &target_zones {
            let st = self.backend.get_zone_state(z).await?;
            fps.push(Self::zone_fingerprint(&st)?);
        }
        Ok(fps)
    }

    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        self.ensure_running().await?;

        let snap_path = snapshot_dir.join("firewall_zones.json");
        let snap: FirewallSnapshotFile = if snap_path.is_file() {
            let raw = fs::read_to_string(&snap_path)?;
            serde_json::from_str(&raw).map_err(|e| {
                AkrepError::RollbackFailed(format!("corrupt firewall snapshot: {}", e))
            })?
        } else {
            serde_json::from_value(record.pre_snapshot.clone()).map_err(|e| {
                AkrepError::RollbackFailed(format!("missing firewall pre_snapshot: {}", e))
            })?
        };

        // Phase 1: Reconcile ports, services, and rich rules on each captured zone
        for prev_zone in &snap.zones {
            self.reconcile_zone_to_snapshot(prev_zone).await?;
        }

        // Phase 2: Reconcile runtime and permanent interface bindings across captured zones
        for prev_zone in &snap.zones {
            let curr = self.backend.get_zone_state(&prev_zone.name).await?;
            for iface in &curr.permanent_interfaces {
                if !prev_zone.permanent_interfaces.contains(iface) {
                    self.backend
                        .unbind_permanent_interface(&prev_zone.name, iface)
                        .await?;
                }
            }
            for iface in &curr.runtime_interfaces {
                if !prev_zone.runtime_interfaces.contains(iface) {
                    self.backend
                        .unbind_runtime_interface(&prev_zone.name, iface)
                        .await?;
                }
            }
        }
        for prev_zone in &snap.zones {
            let curr = self.backend.get_zone_state(&prev_zone.name).await?;
            for iface in &prev_zone.permanent_interfaces {
                if !curr.permanent_interfaces.contains(iface) {
                    self.backend
                        .bind_permanent_interface(&prev_zone.name, iface)
                        .await?;
                }
            }
            for iface in &prev_zone.runtime_interfaces {
                if !curr.runtime_interfaces.contains(iface) {
                    self.backend
                        .bind_runtime_interface(&prev_zone.name, iface)
                        .await?;
                }
            }
        }

        // Verify restored zone fingerprints match pre-apply snapshot
        for prev_zone in &snap.zones {
            let expected_fp = Self::zone_fingerprint(prev_zone)?;
            let actual_st = self.backend.get_zone_state(&prev_zone.name).await?;
            let actual_fp = Self::zone_fingerprint(&actual_st)?;
            if expected_fp.sha256 != actual_fp.sha256 {
                return Err(AkrepError::RollbackFailed(format!(
                    "Zone '{}' state did not converge to pre-apply snapshot after rollback",
                    prev_zone.name
                )));
            }
        }

        let names: Vec<&str> = snap.zones.iter().map(|z| z.name.as_str()).collect();
        Ok(format!(
            "Restored firewall state for zone(s): {}",
            names.join(", ")
        ))
    }

    async fn inspect_firewall(
        &self,
        zone_filter: Option<&str>,
    ) -> Result<FirewallStateSnapshot, AkrepError> {
        self.inspect_state(zone_filter).await
    }
}

#[derive(Debug, Clone)]
struct TimedItem<T> {
    value: T,
    expires_at_secs: Option<u64>,
}

#[derive(Debug, Clone)]
struct MockZoneData {
    runtime_interfaces: BTreeSet<String>,
    permanent_interfaces: BTreeSet<String>,
    runtime_services: Vec<TimedItem<String>>,
    permanent_services: BTreeSet<String>,
    runtime_ports: Vec<TimedItem<(String, String)>>,
    permanent_ports: BTreeSet<(String, String)>,
    runtime_rich_rules: Vec<TimedItem<String>>,
    permanent_rich_rules: BTreeSet<String>,
}

impl MockZoneData {
    fn empty() -> Self {
        Self {
            runtime_interfaces: BTreeSet::new(),
            permanent_interfaces: BTreeSet::new(),
            runtime_services: Vec::new(),
            permanent_services: BTreeSet::new(),
            runtime_ports: Vec::new(),
            permanent_ports: BTreeSet::new(),
            runtime_rich_rules: Vec::new(),
            permanent_rich_rules: BTreeSet::new(),
        }
    }

    fn prune_expired(&mut self, now_secs: u64) {
        self.runtime_services
            .retain(|i| i.expires_at_secs.map(|exp| now_secs < exp).unwrap_or(true));
        self.runtime_ports
            .retain(|i| i.expires_at_secs.map(|exp| now_secs < exp).unwrap_or(true));
        self.runtime_rich_rules
            .retain(|i| i.expires_at_secs.map(|exp| now_secs < exp).unwrap_or(true));
    }

    fn reset_runtime_from_permanent(&mut self) {
        self.runtime_interfaces = self.permanent_interfaces.clone();
        self.runtime_services = self
            .permanent_services
            .iter()
            .map(|s| TimedItem {
                value: s.clone(),
                expires_at_secs: None,
            })
            .collect();
        self.runtime_ports = self
            .permanent_ports
            .iter()
            .map(|p| TimedItem {
                value: p.clone(),
                expires_at_secs: None,
            })
            .collect();
        self.runtime_rich_rules = self
            .permanent_rich_rules
            .iter()
            .map(|r| TimedItem {
                value: r.clone(),
                expires_at_secs: None,
            })
            .collect();
    }
}

#[derive(Debug)]
struct MockFirewallInner {
    running: bool,
    default_zone: String,
    zones: BTreeMap<String, MockZoneData>,
    epoch: Instant,
    virtual_offset_secs: u64,
    simulate_stale_verify: bool,
    fail_after_rich_rule_adds: Option<usize>,
}

impl MockFirewallInner {
    fn now_secs(&self) -> u64 {
        self.epoch.elapsed().as_secs() + self.virtual_offset_secs
    }

    fn prune_all(&mut self) {
        let now = self.now_secs();
        for z in self.zones.values_mut() {
            z.prune_expired(now);
        }
    }

    fn ensure_running(&self) -> Result<(), AkrepError> {
        if !self.running {
            return Err(AkrepError::BackendUnavailable(
                "firewalld is inactive (mock)".to_string(),
            ));
        }
        Ok(())
    }

    fn zone_mut(&mut self, zone: &str) -> Result<&mut MockZoneData, AkrepError> {
        validate_zone_name(zone)?;
        self.zones
            .get_mut(zone)
            .ok_or_else(|| AkrepError::InvalidInput(format!("INVALID_ZONE: '{}'", zone)))
    }
}

pub struct MockFirewallBackend {
    inner: Mutex<MockFirewallInner>,
}

impl Default for MockFirewallBackend {
    fn default() -> Self {
        Self::new_fedora_workstation()
    }
}

impl MockFirewallBackend {
    pub fn new_fedora_workstation() -> Self {
        let mut zones = BTreeMap::new();
        for z_name in [
            "block",
            "dmz",
            "drop",
            "external",
            "FedoraWorkstation",
            "home",
            "internal",
            "public",
            "trusted",
            "work",
        ] {
            zones.insert(z_name.to_string(), MockZoneData::empty());
        }

        if let Some(fw) = zones.get_mut("FedoraWorkstation") {
            fw.permanent_interfaces.insert("wlan0".to_string());
            fw.permanent_services.insert("dhcpv6-client".to_string());
            fw.permanent_services.insert("mdns".to_string());
            fw.permanent_services.insert("samba-client".to_string());
            fw.permanent_ports
                .insert(("1025-65535".to_string(), "tcp".to_string()));
            fw.permanent_ports
                .insert(("1025-65535".to_string(), "udp".to_string()));
            fw.reset_runtime_from_permanent();
        }

        if let Some(pub_z) = zones.get_mut("public") {
            pub_z.permanent_interfaces.insert("eth0".to_string());
            pub_z.permanent_services.insert("dhcpv6-client".to_string());
            pub_z.permanent_services.insert("mdns".to_string());
            pub_z.reset_runtime_from_permanent();
        }

        Self {
            inner: Mutex::new(MockFirewallInner {
                running: true,
                default_zone: "FedoraWorkstation".to_string(),
                zones,
                epoch: Instant::now(),
                virtual_offset_secs: 0,
                simulate_stale_verify: false,
                fail_after_rich_rule_adds: None,
            }),
        }
    }

    pub fn set_running(&self, running: bool) {
        self.inner.lock().expect("mock firewall mutex").running = running;
    }

    pub fn advance_time(&self, seconds: u64) {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.virtual_offset_secs += seconds;
        g.prune_all();
    }

    pub fn simulate_reload(&self) {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        for z in g.zones.values_mut() {
            z.reset_runtime_from_permanent();
        }
    }

    pub fn simulate_reboot(&self) {
        self.simulate_reload();
    }

    pub fn set_simulate_stale_verify(&self, stale: bool) {
        self.inner
            .lock()
            .expect("mock firewall mutex")
            .simulate_stale_verify = stale;
    }

    pub fn inject_external_runtime_port(&self, zone: &str, port: &str, proto: FirewallProto) {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        if let Some(z) = g.zones.get_mut(zone) {
            z.runtime_ports.push(TimedItem {
                value: (port.to_string(), proto.as_str().to_string()),
                expires_at_secs: None,
            });
        }
    }

    pub fn set_fail_after_rich_rule_adds(&self, count: Option<usize>) {
        self.inner
            .lock()
            .expect("mock firewall mutex")
            .fail_after_rich_rule_adds = count;
    }
}

#[async_trait]
impl FirewallBackend for MockFirewallBackend {
    async fn is_running(&self) -> Result<bool, AkrepError> {
        Ok(self.inner.lock().expect("mock firewall mutex").running)
    }

    async fn get_default_zone(&self) -> Result<String, AkrepError> {
        let g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        Ok(g.default_zone.clone())
    }

    async fn list_zones(&self) -> Result<Vec<String>, AkrepError> {
        let g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        Ok(g.zones.keys().cloned().collect())
    }

    async fn get_active_zones(&self) -> Result<BTreeMap<String, String>, AkrepError> {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        g.prune_all();
        let mut out = BTreeMap::new();
        for (z_name, z_data) in &g.zones {
            for iface in &z_data.runtime_interfaces {
                out.insert(iface.clone(), z_name.clone());
            }
        }
        Ok(out)
    }

    async fn get_zone_state(&self, zone: &str) -> Result<FirewallZoneState, AkrepError> {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        g.prune_all();
        let is_default = g.default_zone == zone;
        let z = g.zone_mut(zone)?;

        Ok(normalize_zone_state(FirewallZoneState {
            name: zone.to_string(),
            is_default,
            active: !z.runtime_interfaces.is_empty() || is_default,
            runtime_interfaces: z.runtime_interfaces.iter().cloned().collect(),
            permanent_interfaces: z.permanent_interfaces.iter().cloned().collect(),
            runtime_services: z.runtime_services.iter().map(|i| i.value.clone()).collect(),
            permanent_services: z.permanent_services.iter().cloned().collect(),
            runtime_ports: z.runtime_ports.iter().map(|i| i.value.clone()).collect(),
            permanent_ports: z.permanent_ports.iter().cloned().collect(),
            runtime_rich_rules: z
                .runtime_rich_rules
                .iter()
                .map(|i| i.value.clone())
                .collect(),
            permanent_rich_rules: z.permanent_rich_rules.iter().cloned().collect(),
        }))
    }

    async fn add_runtime_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
        timeout_secs: u32,
    ) -> Result<(), AkrepError> {
        validate_port_spec(port)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        g.prune_all();
        if g.simulate_stale_verify {
            return Ok(());
        }
        let now = g.now_secs();
        let expires = if timeout_secs == 0 {
            None
        } else {
            Some(now + u64::from(timeout_secs))
        };
        let pair = (port.to_string(), proto.as_str().to_string());
        let z = g.zone_mut(zone)?;
        z.runtime_ports.retain(|i| i.value != pair);
        z.runtime_ports.push(TimedItem {
            value: pair,
            expires_at_secs: expires,
        });
        Ok(())
    }

    async fn remove_runtime_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError> {
        validate_port_spec(port)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        g.prune_all();
        if g.simulate_stale_verify {
            return Ok(());
        }
        let pair = (port.to_string(), proto.as_str().to_string());
        let z = g.zone_mut(zone)?;
        z.runtime_ports.retain(|i| i.value != pair);
        Ok(())
    }

    async fn add_permanent_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError> {
        validate_port_spec(port)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        if g.simulate_stale_verify {
            return Ok(());
        }
        let z = g.zone_mut(zone)?;
        z.permanent_ports
            .insert((port.to_string(), proto.as_str().to_string()));
        Ok(())
    }

    async fn remove_permanent_port(
        &self,
        zone: &str,
        port: &str,
        proto: FirewallProto,
    ) -> Result<(), AkrepError> {
        validate_port_spec(port)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        if g.simulate_stale_verify {
            return Ok(());
        }
        let z = g.zone_mut(zone)?;
        z.permanent_ports
            .remove(&(port.to_string(), proto.as_str().to_string()));
        Ok(())
    }

    async fn add_runtime_service(
        &self,
        zone: &str,
        service: &str,
        timeout_secs: u32,
    ) -> Result<(), AkrepError> {
        validate_service_name(service)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        g.prune_all();
        if g.simulate_stale_verify {
            return Ok(());
        }
        let now = g.now_secs();
        let expires = if timeout_secs == 0 {
            None
        } else {
            Some(now + u64::from(timeout_secs))
        };
        let z = g.zone_mut(zone)?;
        z.runtime_services.retain(|i| i.value != service);
        z.runtime_services.push(TimedItem {
            value: service.to_string(),
            expires_at_secs: expires,
        });
        Ok(())
    }

    async fn remove_runtime_service(&self, zone: &str, service: &str) -> Result<(), AkrepError> {
        validate_service_name(service)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        g.prune_all();
        if g.simulate_stale_verify {
            return Ok(());
        }
        let z = g.zone_mut(zone)?;
        z.runtime_services.retain(|i| i.value != service);
        Ok(())
    }

    async fn add_permanent_service(&self, zone: &str, service: &str) -> Result<(), AkrepError> {
        validate_service_name(service)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        if g.simulate_stale_verify {
            return Ok(());
        }
        let z = g.zone_mut(zone)?;
        z.permanent_services.insert(service.to_string());
        Ok(())
    }

    async fn remove_permanent_service(&self, zone: &str, service: &str) -> Result<(), AkrepError> {
        validate_service_name(service)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        if g.simulate_stale_verify {
            return Ok(());
        }
        let z = g.zone_mut(zone)?;
        z.permanent_services.remove(service);
        Ok(())
    }

    async fn add_runtime_rich_rule(
        &self,
        zone: &str,
        rule: &str,
        timeout_secs: u32,
    ) -> Result<(), AkrepError> {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        g.prune_all();
        if g.simulate_stale_verify {
            return Ok(());
        }
        if let Some(rem) = g.fail_after_rich_rule_adds {
            if rem == 0 {
                g.fail_after_rich_rule_adds = None;
                return Err(AkrepError::ApplyFailed(
                    "Simulated mid-apply rich rule failure".to_string(),
                ));
            }
            g.fail_after_rich_rule_adds = Some(rem - 1);
        }
        let now = g.now_secs();
        let expires = if timeout_secs == 0 {
            None
        } else {
            Some(now + u64::from(timeout_secs))
        };
        let z = g.zone_mut(zone)?;
        z.runtime_rich_rules.retain(|i| i.value != rule);
        z.runtime_rich_rules.push(TimedItem {
            value: rule.to_string(),
            expires_at_secs: expires,
        });
        Ok(())
    }

    async fn remove_runtime_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError> {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        g.prune_all();
        if g.simulate_stale_verify {
            return Ok(());
        }
        let z = g.zone_mut(zone)?;
        z.runtime_rich_rules.retain(|i| i.value != rule);
        Ok(())
    }

    async fn add_permanent_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError> {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        if g.simulate_stale_verify {
            return Ok(());
        }
        let z = g.zone_mut(zone)?;
        z.permanent_rich_rules.insert(rule.to_string());
        Ok(())
    }

    async fn remove_permanent_rich_rule(&self, zone: &str, rule: &str) -> Result<(), AkrepError> {
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        if g.simulate_stale_verify {
            return Ok(());
        }
        let z = g.zone_mut(zone)?;
        z.permanent_rich_rules.remove(rule);
        Ok(())
    }

    async fn bind_runtime_interface(&self, zone: &str, interface: &str) -> Result<(), AkrepError> {
        validate_network_interface(interface)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        if !g.zones.contains_key(zone) {
            return Err(AkrepError::InvalidInput(format!(
                "INVALID_ZONE: '{}'",
                zone
            )));
        }
        if g.simulate_stale_verify {
            return Ok(());
        }
        for z_data in g.zones.values_mut() {
            z_data.runtime_interfaces.remove(interface);
        }
        g.zones
            .get_mut(zone)
            .expect("verified zone exists")
            .runtime_interfaces
            .insert(interface.to_string());
        Ok(())
    }

    async fn unbind_runtime_interface(
        &self,
        zone: &str,
        interface: &str,
    ) -> Result<(), AkrepError> {
        validate_network_interface(interface)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        let z = g.zone_mut(zone)?;
        z.runtime_interfaces.remove(interface);
        Ok(())
    }

    async fn bind_permanent_interface(
        &self,
        zone: &str,
        interface: &str,
    ) -> Result<(), AkrepError> {
        validate_network_interface(interface)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        if !g.zones.contains_key(zone) {
            return Err(AkrepError::InvalidInput(format!(
                "INVALID_ZONE: '{}'",
                zone
            )));
        }
        if g.simulate_stale_verify {
            return Ok(());
        }
        for z_data in g.zones.values_mut() {
            z_data.permanent_interfaces.remove(interface);
        }
        g.zones
            .get_mut(zone)
            .expect("verified zone exists")
            .permanent_interfaces
            .insert(interface.to_string());
        Ok(())
    }

    async fn unbind_permanent_interface(
        &self,
        zone: &str,
        interface: &str,
    ) -> Result<(), AkrepError> {
        validate_network_interface(interface)?;
        let mut g = self.inner.lock().expect("mock firewall mutex");
        g.ensure_running()?;
        let z = g.zone_mut(zone)?;
        z.permanent_interfaces.remove(interface);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_firewalld_dbus_wire_signatures_match_upstream_spec() {
        let add_port_msg = zbus::message::Message::method_call(FW_PATH, "addPort")
            .unwrap()
            .destination(FW_DEST)
            .unwrap()
            .interface(FW_ZONE_IFACE)
            .unwrap()
            .build(&("public", "8443", "tcp", 60i32))
            .unwrap();
        let sig = add_port_msg.body().signature().to_string();
        assert_eq!(
            sig.strip_prefix('(')
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or(&sig),
            "sssi"
        );

        let cfg_port_msg = zbus::message::Message::method_call(
            "/org/fedoraproject/FirewallD1/config/zone/0",
            "addPort",
        )
        .unwrap()
        .destination(FW_DEST)
        .unwrap()
        .interface(FW_CONFIG_ZONE_IFACE)
        .unwrap()
        .build(&("443", "tcp"))
        .unwrap();
        let cfg_sig = cfg_port_msg.body().signature().to_string();
        assert_eq!(
            cfg_sig
                .strip_prefix('(')
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or(&cfg_sig),
            "ss"
        );
    }
}
