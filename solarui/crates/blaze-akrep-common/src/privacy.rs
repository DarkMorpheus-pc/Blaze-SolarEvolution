use crate::errors::{redact_secrets, AkrepError};
use async_trait::async_trait;
use blaze_sentinel_core::{AkrepSeverity, Availability};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex, OnceLock};
use zbus::zvariant::OwnedValue;
use zbus::Connection;

pub const PORTAL_DESKTOP_BUS_NAME: &str = "org.freedesktop.portal.Desktop";
pub const PORTAL_DESKTOP_OBJECT_PATH: &str = "/org/freedesktop/portal/desktop";

pub const PERMISSION_STORE_BUS_NAME: &str = "org.freedesktop.impl.portal.PermissionStore";
pub const PERMISSION_STORE_OBJECT_PATH: &str = "/org/freedesktop/impl/portal/desktop";
pub const PERMISSION_STORE_INTERFACE: &str = "org.freedesktop.impl.portal.PermissionStore";

pub const IFACE_PORTAL_SCREENCAST: &str = "org.freedesktop.portal.ScreenCast";
pub const IFACE_PORTAL_REMOTE_DESKTOP: &str = "org.freedesktop.portal.RemoteDesktop";
pub const IFACE_PORTAL_GLOBAL_SHORTCUTS: &str = "org.freedesktop.portal.GlobalShortcuts";
pub const IFACE_PORTAL_CAMERA: &str = "org.freedesktop.portal.Camera";
pub const IFACE_PORTAL_USB: &str = "org.freedesktop.portal.Usb";

pub const PERMISSION_STORE_TABLES: &[&str] = &[
    "camera",
    "microphone",
    "screencast",
    "remote-desktop",
    "devices",
    "notifications",
];

const DBUS_DEST: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_IFACE: &str = "org.freedesktop.DBus";
const DBUS_INTROSPECTABLE_IFACE: &str = "org.freedesktop.DBus.Introspectable";
const DBUS_PROPERTIES_IFACE: &str = "org.freedesktop.DBus.Properties";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenCastSourceType {
    Monitor,
    Window,
    Virtual,
}

pub fn decode_screencast_source_types(mask: u32) -> Vec<ScreenCastSourceType> {
    let mut out = Vec::new();
    if (mask & 1) != 0 {
        out.push(ScreenCastSourceType::Monitor);
    }
    if (mask & 2) != 0 {
        out.push(ScreenCastSourceType::Window);
    }
    if (mask & 4) != 0 {
        out.push(ScreenCastSourceType::Virtual);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenCastCursorMode {
    Hidden,
    Embedded,
    Metadata,
}

pub fn decode_screencast_cursor_modes(mask: u32) -> Vec<ScreenCastCursorMode> {
    let mut out = Vec::new();
    if (mask & 1) != 0 {
        out.push(ScreenCastCursorMode::Hidden);
    }
    if (mask & 2) != 0 {
        out.push(ScreenCastCursorMode::Embedded);
    }
    if (mask & 4) != 0 {
        out.push(ScreenCastCursorMode::Metadata);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteDesktopDeviceType {
    Keyboard,
    Pointer,
    Touchscreen,
}

pub fn decode_remote_desktop_device_types(mask: u32) -> Vec<RemoteDesktopDeviceType> {
    let mut out = Vec::new();
    if (mask & 1) != 0 {
        out.push(RemoteDesktopDeviceType::Keyboard);
    }
    if (mask & 2) != 0 {
        out.push(RemoteDesktopDeviceType::Pointer);
    }
    if (mask & 4) != 0 {
        out.push(RemoteDesktopDeviceType::Touchscreen);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenCastPortalStatus {
    pub availability: Availability,
    pub severity: AkrepSeverity,
    pub version: Option<u32>,
    pub available_source_types_mask: Option<u32>,
    pub source_types: Vec<ScreenCastSourceType>,
    pub available_cursor_modes_mask: Option<u32>,
    pub cursor_modes: Vec<ScreenCastCursorMode>,
}

impl ScreenCastPortalStatus {
    pub fn unavailable() -> Self {
        Self {
            availability: Availability::Unavailable,
            severity: AkrepSeverity::Unavailable,
            version: None,
            available_source_types_mask: None,
            source_types: Vec::new(),
            available_cursor_modes_mask: None,
            cursor_modes: Vec::new(),
        }
    }

    pub fn unknown() -> Self {
        Self {
            availability: Availability::Unknown,
            severity: AkrepSeverity::Unknown,
            version: None,
            available_source_types_mask: None,
            source_types: Vec::new(),
            available_cursor_modes_mask: None,
            cursor_modes: Vec::new(),
        }
    }

    pub fn from_properties(
        version: u32,
        source_mask: Option<u32>,
        cursor_mask: Option<u32>,
    ) -> Self {
        let source_types = source_mask
            .map(decode_screencast_source_types)
            .unwrap_or_default();
        let cursor_modes = cursor_mask
            .map(decode_screencast_cursor_modes)
            .unwrap_or_default();
        let severity = if version > 0 && !source_types.is_empty() {
            AkrepSeverity::Protected
        } else {
            AkrepSeverity::Attention
        };
        Self {
            availability: Availability::Available,
            severity,
            version: Some(version),
            available_source_types_mask: source_mask,
            source_types,
            available_cursor_modes_mask: cursor_mask,
            cursor_modes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteDesktopPortalStatus {
    pub availability: Availability,
    pub severity: AkrepSeverity,
    pub version: Option<u32>,
    pub available_device_types_mask: Option<u32>,
    pub device_types: Vec<RemoteDesktopDeviceType>,
}

impl RemoteDesktopPortalStatus {
    pub fn unavailable() -> Self {
        Self {
            availability: Availability::Unavailable,
            severity: AkrepSeverity::Unavailable,
            version: None,
            available_device_types_mask: None,
            device_types: Vec::new(),
        }
    }

    pub fn unknown() -> Self {
        Self {
            availability: Availability::Unknown,
            severity: AkrepSeverity::Unknown,
            version: None,
            available_device_types_mask: None,
            device_types: Vec::new(),
        }
    }

    pub fn from_properties(version: u32, device_mask: Option<u32>) -> Self {
        let device_types = device_mask
            .map(decode_remote_desktop_device_types)
            .unwrap_or_default();
        let severity = if version > 0 && !device_types.is_empty() {
            AkrepSeverity::Protected
        } else {
            AkrepSeverity::Attention
        };
        Self {
            availability: Availability::Available,
            severity,
            version: Some(version),
            available_device_types_mask: device_mask,
            device_types,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GlobalShortcutsPortalStatus {
    pub availability: Availability,
    pub severity: AkrepSeverity,
    pub version: Option<u32>,
}

impl GlobalShortcutsPortalStatus {
    pub fn unavailable() -> Self {
        Self {
            availability: Availability::Unavailable,
            severity: AkrepSeverity::Unavailable,
            version: None,
        }
    }

    pub fn unknown() -> Self {
        Self {
            availability: Availability::Unknown,
            severity: AkrepSeverity::Unknown,
            version: None,
        }
    }

    pub fn from_version(version: u32) -> Self {
        Self {
            availability: Availability::Available,
            severity: if version > 0 {
                AkrepSeverity::Protected
            } else {
                AkrepSeverity::Unknown
            },
            version: Some(version),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CameraPortalStatus {
    pub availability: Availability,
    pub severity: AkrepSeverity,
    pub version: Option<u32>,
    pub is_camera_present: Option<bool>,
}

impl CameraPortalStatus {
    pub fn unavailable() -> Self {
        Self {
            availability: Availability::Unavailable,
            severity: AkrepSeverity::Unavailable,
            version: None,
            is_camera_present: None,
        }
    }

    pub fn unknown() -> Self {
        Self {
            availability: Availability::Unknown,
            severity: AkrepSeverity::Unknown,
            version: None,
            is_camera_present: None,
        }
    }

    pub fn from_properties(version: u32, is_camera_present: Option<bool>) -> Self {
        Self {
            availability: Availability::Available,
            severity: if version > 0 {
                AkrepSeverity::Protected
            } else {
                AkrepSeverity::Unknown
            },
            version: Some(version),
            is_camera_present,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsbPortalStatus {
    pub availability: Availability,
    pub severity: AkrepSeverity,
    pub version: Option<u32>,
}

impl UsbPortalStatus {
    pub fn unavailable() -> Self {
        Self {
            availability: Availability::Unavailable,
            severity: AkrepSeverity::Unavailable,
            version: None,
        }
    }

    pub fn unknown() -> Self {
        Self {
            availability: Availability::Unknown,
            severity: AkrepSeverity::Unknown,
            version: None,
        }
    }

    pub fn from_version(version: u32) -> Self {
        Self {
            availability: Availability::Available,
            severity: if version > 0 {
                AkrepSeverity::Protected
            } else {
                AkrepSeverity::Unknown
            },
            version: Some(version),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalDiscoveryReport {
    pub portal_service_availability: Availability,
    pub discovered_interfaces: Vec<String>,
    pub screencast: ScreenCastPortalStatus,
    pub remote_desktop: RemoteDesktopPortalStatus,
    pub global_shortcuts: GlobalShortcutsPortalStatus,
    pub camera: CameraPortalStatus,
    pub usb: UsbPortalStatus,
}

impl PortalDiscoveryReport {
    pub fn unavailable() -> Self {
        Self {
            portal_service_availability: Availability::Unavailable,
            discovered_interfaces: Vec::new(),
            screencast: ScreenCastPortalStatus::unavailable(),
            remote_desktop: RemoteDesktopPortalStatus::unavailable(),
            global_shortcuts: GlobalShortcutsPortalStatus::unavailable(),
            camera: CameraPortalStatus::unavailable(),
            usb: UsbPortalStatus::unavailable(),
        }
    }

    pub fn core_wayland_portals_severity(&self) -> AkrepSeverity {
        if self.portal_service_availability != Availability::Available {
            return match self.portal_service_availability {
                Availability::Unavailable => AkrepSeverity::Unavailable,
                Availability::Unknown => AkrepSeverity::Unknown,
                Availability::Available => AkrepSeverity::Protected,
            };
        }
        self.screencast
            .severity
            .combine(self.remote_desktop.severity)
            .combine(self.camera.severity)
    }
}

pub fn parse_portal_introspection_interfaces(xml: &str) -> BTreeSet<String> {
    static IFACE_RE: OnceLock<Regex> = OnceLock::new();
    let re = IFACE_RE.get_or_init(|| {
        Regex::new(r#"<interface\s+name\s*=\s*"([^"]+)"\s*/?>"#).expect("valid interface xml regex")
    });
    let mut set = BTreeSet::new();
    for cap in re.captures_iter(xml) {
        if let Some(m) = cap.get(1) {
            set.insert(m.as_str().trim().to_string());
        }
    }
    set
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionStoreEntry {
    pub table: String,
    pub resource_id: String,
    pub app_id: String,
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionTableReport {
    pub table: String,
    pub availability: Availability,
    pub entries: Vec<PermissionStoreEntry>,
}

pub fn validate_permission_store_table(table: &str) -> Result<(), AkrepError> {
    let trimmed = table.trim();
    if !PERMISSION_STORE_TABLES.contains(&trimmed) {
        return Err(AkrepError::InvalidInput(format!(
            "Unsupported XDG PermissionStore table '{}' (allowed: {})",
            table,
            PERMISSION_STORE_TABLES.join(", ")
        )));
    }
    Ok(())
}

fn validate_permission_store_token(field: &str, value: &str) -> Result<(), AkrepError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 255 || trimmed.chars().any(char::is_control) {
        return Err(AkrepError::InvalidInput(format!(
            "Invalid PermissionStore {} parameter",
            field
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivacyStreamKind {
    Microphone,
    Camera,
    ScreenCast,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivePrivacyStream {
    pub node_id: u32,
    pub kind: PrivacyStreamKind,
    pub state: String,
    pub application_name: String,
    pub application_id: Option<String>,
    pub process_binary: Option<String>,
    pub process_id: Option<u32>,
    pub media_name: String,
    pub source_node_id: u32,
    pub source_node_name: String,
    pub owner_uid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipewireNodeSummary {
    pub node_id: u32,
    pub media_class: String,
    pub state: String,
    pub node_name: String,
    pub media_name: String,
    pub media_role: Option<String>,
    pub device_api: Option<String>,
    pub application_name: Option<String>,
    pub application_id: Option<String>,
    pub process_binary: Option<String>,
    pub process_id: Option<u32>,
    pub owner_uid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipewireGraphInspection {
    pub availability: Availability,
    pub nodes: BTreeMap<u32, PipewireNodeSummary>,
    pub active_streams: Vec<ActivePrivacyStream>,
}

fn json_str_prop(props: &serde_json::Value, key: &str) -> Option<String> {
    props
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn json_u32_prop(props: &serde_json::Value, key: &str) -> Option<u32> {
    if let Some(u) = props.get(key).and_then(|v| v.as_u64()) {
        return u32::try_from(u).ok();
    }
    if let Some(s) = props.get(key).and_then(|v| v.as_str()) {
        return s.trim().parse::<u32>().ok();
    }
    None
}

fn classify_video_source(
    source: &PipewireNodeSummary,
    stream: &PipewireNodeSummary,
) -> PrivacyStreamKind {
    if let Some(role) = source.media_role.as_deref() {
        let lower = role.to_ascii_lowercase();
        if lower.contains("screen") || lower.contains("desktop") {
            return PrivacyStreamKind::ScreenCast;
        }
        if lower.contains("camera") {
            return PrivacyStreamKind::Camera;
        }
    }
    if let Some(api) = source.device_api.as_deref() {
        let lower = api.to_ascii_lowercase();
        if lower == "v4l2" || lower == "libcamera" {
            return PrivacyStreamKind::Camera;
        }
    }

    let combined = format!(
        "{} {} {} {}",
        source.node_name.to_ascii_lowercase(),
        source.media_name.to_ascii_lowercase(),
        source
            .application_name
            .as_deref()
            .unwrap_or_default()
            .to_ascii_lowercase(),
        stream.media_name.to_ascii_lowercase()
    );
    for screen_kw in [
        "portal",
        "screencast",
        "screen-cast",
        "xdp",
        "niri",
        "mutter",
        "kwin",
        "xdg-desktop-portal",
        "monitor",
        "screen",
    ] {
        if combined.contains(screen_kw) {
            return PrivacyStreamKind::ScreenCast;
        }
    }
    PrivacyStreamKind::Camera
}

pub fn parse_pipewire_dump_json(raw_json: &str) -> Result<PipewireGraphInspection, AkrepError> {
    let items: Vec<serde_json::Value> = serde_json::from_str(raw_json).map_err(|e| {
        AkrepError::InvalidInput(format!(
            "Failed to parse pw-dump JSON array: {}",
            redact_secrets(&e.to_string())
        ))
    })?;

    let mut nodes: BTreeMap<u32, PipewireNodeSummary> = BTreeMap::new();
    let mut links: Vec<(u32, u32)> = Vec::new();

    for obj in &items {
        let Some(id_u64) = obj.get("id").and_then(|v| v.as_u64()) else {
            continue;
        };
        let Ok(obj_id) = u32::try_from(id_u64) else {
            continue;
        };
        let obj_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or_default();

        if obj_type == "PipeWire:Interface:Node" {
            let info = obj.get("info").unwrap_or(&serde_json::Value::Null);
            let props = info
                .get("props")
                .or_else(|| obj.get("props"))
                .unwrap_or(&serde_json::Value::Null);

            let state = info
                .get("state")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_ascii_lowercase())
                .or_else(|| json_str_prop(props, "node.state").map(|s| s.to_ascii_lowercase()))
                .or_else(|| {
                    obj.get("state")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_ascii_lowercase())
                })
                .unwrap_or_else(|| "idle".to_string());

            let media_class = json_str_prop(props, "media.class").unwrap_or_default();
            let node_name = json_str_prop(props, "node.name")
                .or_else(|| json_str_prop(props, "node.description"))
                .unwrap_or_else(|| format!("node-{}", obj_id));
            let media_name =
                json_str_prop(props, "media.name").unwrap_or_else(|| node_name.clone());
            let media_role = json_str_prop(props, "media.role");
            let device_api = json_str_prop(props, "device.api");
            let application_name = json_str_prop(props, "application.name");
            let application_id = json_str_prop(props, "application.id");
            let process_binary = json_str_prop(props, "application.process.binary");
            let process_id = json_u32_prop(props, "application.process.id");
            let owner_uid = json_u32_prop(props, "application.process.uid")
                .or_else(|| json_u32_prop(props, "client.uid"));

            nodes.insert(
                obj_id,
                PipewireNodeSummary {
                    node_id: obj_id,
                    media_class,
                    state,
                    node_name,
                    media_name,
                    media_role,
                    device_api,
                    application_name,
                    application_id,
                    process_binary,
                    process_id,
                    owner_uid,
                },
            );
        } else if obj_type == "PipeWire:Interface:Link" {
            let info = obj.get("info").unwrap_or(&serde_json::Value::Null);
            let props = info
                .get("props")
                .or_else(|| obj.get("props"))
                .unwrap_or(&serde_json::Value::Null);
            let link_state = info
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("active")
                .to_ascii_lowercase();
            if link_state == "error" {
                continue;
            }
            let out_node = json_u32_prop(info, "output-node-id")
                .or_else(|| json_u32_prop(props, "link.output.node"));
            let in_node = json_u32_prop(info, "input-node-id")
                .or_else(|| json_u32_prop(props, "link.input.node"));
            if let (Some(src), Some(dst)) = (out_node, in_node) {
                if !links.contains(&(src, dst)) {
                    links.push((src, dst));
                }
            }
        }
    }

    let mut active_streams = Vec::new();
    for (stream_id, stream_node) in &nodes {
        if stream_node.state != "running" {
            continue;
        }
        let is_audio_capture = stream_node.media_class == "Stream/Input/Audio";
        let is_video_capture = stream_node.media_class == "Stream/Input/Video";
        if !is_audio_capture && !is_video_capture {
            continue;
        }

        let linked_source = links.iter().find_map(|(src_id, dst_id)| {
            if dst_id != stream_id {
                return None;
            }
            let src_node = nodes.get(src_id)?;
            if (is_audio_capture && src_node.media_class.starts_with("Audio/Source"))
                || (is_video_capture && src_node.media_class.starts_with("Video/Source"))
            {
                Some(src_node)
            } else {
                None
            }
        });

        let Some(src_node) = linked_source else {
            continue;
        };

        let kind = if is_audio_capture {
            PrivacyStreamKind::Microphone
        } else {
            classify_video_source(src_node, stream_node)
        };

        let application_name = stream_node
            .application_name
            .clone()
            .or_else(|| stream_node.process_binary.clone())
            .unwrap_or_else(|| stream_node.node_name.clone());

        active_streams.push(ActivePrivacyStream {
            node_id: *stream_id,
            kind,
            state: stream_node.state.clone(),
            application_name,
            application_id: stream_node.application_id.clone(),
            process_binary: stream_node.process_binary.clone(),
            process_id: stream_node.process_id,
            media_name: stream_node.media_name.clone(),
            source_node_id: src_node.node_id,
            source_node_name: src_node.node_name.clone(),
            owner_uid: stream_node.owner_uid,
        });
    }

    Ok(PipewireGraphInspection {
        availability: Availability::Available,
        nodes,
        active_streams,
    })
}

pub fn validate_terminable_stream_node(
    graph: &PipewireGraphInspection,
    node_id: u32,
    caller_uid: u32,
) -> Result<ActivePrivacyStream, AkrepError> {
    if node_id == 0 {
        return Err(AkrepError::InvalidInput(
            "PipeWire node_id 0 (core) cannot be destroyed".to_string(),
        ));
    }
    let Some(node) = graph.nodes.get(&node_id) else {
        return Err(AkrepError::InvalidInput(format!(
            "PipeWire node {} does not exist in the current user session graph",
            node_id
        )));
    };

    if !matches!(
        node.media_class.as_str(),
        "Stream/Input/Audio" | "Stream/Input/Video"
    ) {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Refusing to destroy non-capture PipeWire node {} (media.class='{}'); hardware sources, sinks, and compositor nodes are protected",
            node_id, node.media_class
        )));
    }

    if let Some(uid) = node.owner_uid {
        if uid != caller_uid {
            return Err(AkrepError::ForbiddenOperation(format!(
                "PipeWire stream node {} belongs to uid {}, not current session uid {}",
                node_id, uid, caller_uid
            )));
        }
    }

    if node.state != "running" {
        return Err(AkrepError::InvalidInput(format!(
            "PipeWire stream node {} is in state '{}' (only active 'running' capture streams may be terminated)",
            node_id, node.state
        )));
    }

    graph
        .active_streams
        .iter()
        .find(|s| s.node_id == node_id)
        .cloned()
        .ok_or_else(|| {
            AkrepError::InvalidInput(format!(
                "PipeWire stream node {} is not linked to an active capture/sharing source",
                node_id
            ))
        })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PortalPrivacyAction {
    RevokeStoredPermission {
        table: String,
        resource_id: String,
        app_id: String,
        user_confirmed: bool,
    },
    TerminateActiveStream {
        node_id: u32,
        user_confirmed: bool,
    },
    SilentGrantPermission {
        table: String,
        resource_id: String,
        app_id: String,
        permissions: Vec<String>,
    },
    BypassInteractiveConsent {
        portal_interface: String,
        app_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrivacySessionSnapshot {
    pub portals: PortalDiscoveryReport,
    pub permission_tables: Vec<PermissionTableReport>,
    pub pipewire_availability: Availability,
    pub active_streams: Vec<ActivePrivacyStream>,
}

#[async_trait]
pub trait PortalPrivacyBackend: Send + Sync {
    async fn discover_portals(&self) -> Result<PortalDiscoveryReport, AkrepError>;
    async fn query_permission_table(
        &self,
        table: &str,
    ) -> Result<PermissionTableReport, AkrepError>;
    async fn revoke_permission(
        &self,
        table: &str,
        resource_id: &str,
        app_id: &str,
        user_confirmed: bool,
    ) -> Result<(), AkrepError>;
    async fn inspect_pipewire_graph(&self) -> Result<PipewireGraphInspection, AkrepError>;
    async fn terminate_stream(&self, node_id: u32, user_confirmed: bool) -> Result<(), AkrepError>;
}

pub struct PrivacySessionController {
    backend: Arc<dyn PortalPrivacyBackend>,
}

impl PrivacySessionController {
    pub fn new(backend: Arc<dyn PortalPrivacyBackend>) -> Self {
        Self { backend }
    }

    pub fn backend(&self) -> &Arc<dyn PortalPrivacyBackend> {
        &self.backend
    }

    pub async fn snapshot(&self) -> Result<PrivacySessionSnapshot, AkrepError> {
        let portals = self.backend.discover_portals().await?;
        let mut permission_tables = Vec::with_capacity(PERMISSION_STORE_TABLES.len());
        for table in PERMISSION_STORE_TABLES {
            permission_tables.push(self.backend.query_permission_table(table).await?);
        }
        let pw = self.backend.inspect_pipewire_graph().await?;
        Ok(PrivacySessionSnapshot {
            portals,
            permission_tables,
            pipewire_availability: pw.availability,
            active_streams: pw.active_streams,
        })
    }

    pub async fn execute_action(&self, action: PortalPrivacyAction) -> Result<(), AkrepError> {
        match action {
            PortalPrivacyAction::SilentGrantPermission {
                table, app_id, ..
            } => Err(AkrepError::ForbiddenOperation(format!(
                "Wayland user consent invariant: silent Portal permission grant on '{}' for '{}' is strictly forbidden",
                table, app_id
            ))),
            PortalPrivacyAction::BypassInteractiveConsent {
                portal_interface,
                app_id,
            } => Err(AkrepError::ForbiddenOperation(format!(
                "Wayland user consent invariant: bypassing interactive consent for '{}' ('{}') is strictly forbidden",
                portal_interface, app_id
            ))),
            PortalPrivacyAction::RevokeStoredPermission {
                table,
                resource_id,
                app_id,
                user_confirmed,
            } => {
                self.backend
                    .revoke_permission(&table, &resource_id, &app_id, user_confirmed)
                    .await
            }
            PortalPrivacyAction::TerminateActiveStream {
                node_id,
                user_confirmed,
            } => self.backend.terminate_stream(node_id, user_confirmed).await,
        }
    }
}

pub struct SessionPortalPipewireBackend {
    conn: Connection,
    pw_dump_bin: PathBuf,
    pw_cli_bin: PathBuf,
}

impl SessionPortalPipewireBackend {
    pub async fn connect_session() -> Result<Self, AkrepError> {
        let conn = Connection::session().await.map_err(|e| {
            AkrepError::BackendUnavailable(format!(
                "Session D-Bus connection unavailable: {}",
                redact_secrets(&e.to_string())
            ))
        })?;
        Ok(Self::with_connection(conn))
    }

    pub fn with_connection(conn: Connection) -> Self {
        Self {
            conn,
            pw_dump_bin: PathBuf::from("/usr/bin/pw-dump"),
            pw_cli_bin: PathBuf::from("/usr/bin/pw-cli"),
        }
    }

    async fn name_has_owner(&self, bus_name: &str) -> bool {
        let Ok(reply) = self
            .conn
            .call_method(
                Some(DBUS_DEST),
                DBUS_PATH,
                Some(DBUS_IFACE),
                "NameHasOwner",
                &(bus_name,),
            )
            .await
        else {
            return false;
        };
        reply.body().deserialize::<bool>().unwrap_or(false)
    }

    async fn get_u32_prop(&self, iface: &str, prop: &str) -> Option<u32> {
        let reply = self
            .conn
            .call_method(
                Some(PORTAL_DESKTOP_BUS_NAME),
                PORTAL_DESKTOP_OBJECT_PATH,
                Some(DBUS_PROPERTIES_IFACE),
                "Get",
                &(iface, prop),
            )
            .await
            .ok()?;
        let val = reply.body().deserialize::<OwnedValue>().ok()?;
        u32::try_from(val).ok()
    }

    async fn get_bool_prop(&self, iface: &str, prop: &str) -> Option<bool> {
        let reply = self
            .conn
            .call_method(
                Some(PORTAL_DESKTOP_BUS_NAME),
                PORTAL_DESKTOP_OBJECT_PATH,
                Some(DBUS_PROPERTIES_IFACE),
                "Get",
                &(iface, prop),
            )
            .await
            .ok()?;
        let val = reply.body().deserialize::<OwnedValue>().ok()?;
        bool::try_from(val).ok()
    }

    fn current_uid() -> u32 {
        // SAFETY: `libc::getuid()` is thread-safe and infallible on Linux.
        unsafe { libc::getuid() }
    }
}

fn is_permission_table_missing_err(err_msg: &str) -> bool {
    let lower = err_msg.to_ascii_lowercase();
    lower.contains("notfound")
        || lower.contains("not found")
        || lower.contains("no such table")
        || lower.contains("does not exist")
}

#[async_trait]
impl PortalPrivacyBackend for SessionPortalPipewireBackend {
    async fn discover_portals(&self) -> Result<PortalDiscoveryReport, AkrepError> {
        if !self.name_has_owner(PORTAL_DESKTOP_BUS_NAME).await {
            return Ok(PortalDiscoveryReport::unavailable());
        }

        let introspect_reply = self
            .conn
            .call_method(
                Some(PORTAL_DESKTOP_BUS_NAME),
                PORTAL_DESKTOP_OBJECT_PATH,
                Some(DBUS_INTROSPECTABLE_IFACE),
                "Introspect",
                &(),
            )
            .await;

        let Ok(reply) = introspect_reply else {
            return Ok(PortalDiscoveryReport {
                portal_service_availability: Availability::Unknown,
                discovered_interfaces: Vec::new(),
                screencast: ScreenCastPortalStatus::unknown(),
                remote_desktop: RemoteDesktopPortalStatus::unknown(),
                global_shortcuts: GlobalShortcutsPortalStatus::unknown(),
                camera: CameraPortalStatus::unknown(),
                usb: UsbPortalStatus::unknown(),
            });
        };

        let Ok(xml) = reply.body().deserialize::<String>() else {
            return Ok(PortalDiscoveryReport::unavailable());
        };
        let ifaces = parse_portal_introspection_interfaces(&xml);

        let screencast = if ifaces.contains(IFACE_PORTAL_SCREENCAST) {
            match self.get_u32_prop(IFACE_PORTAL_SCREENCAST, "version").await {
                Some(ver) => {
                    let src_mask = self
                        .get_u32_prop(IFACE_PORTAL_SCREENCAST, "AvailableSourceTypes")
                        .await;
                    let cur_mask = self
                        .get_u32_prop(IFACE_PORTAL_SCREENCAST, "AvailableCursorModes")
                        .await;
                    ScreenCastPortalStatus::from_properties(ver, src_mask, cur_mask)
                }
                None => ScreenCastPortalStatus::unknown(),
            }
        } else {
            ScreenCastPortalStatus::unavailable()
        };

        let remote_desktop = if ifaces.contains(IFACE_PORTAL_REMOTE_DESKTOP) {
            match self
                .get_u32_prop(IFACE_PORTAL_REMOTE_DESKTOP, "version")
                .await
            {
                Some(ver) => {
                    let dev_mask = self
                        .get_u32_prop(IFACE_PORTAL_REMOTE_DESKTOP, "AvailableDeviceTypes")
                        .await;
                    RemoteDesktopPortalStatus::from_properties(ver, dev_mask)
                }
                None => RemoteDesktopPortalStatus::unknown(),
            }
        } else {
            RemoteDesktopPortalStatus::unavailable()
        };

        let global_shortcuts = if ifaces.contains(IFACE_PORTAL_GLOBAL_SHORTCUTS) {
            match self
                .get_u32_prop(IFACE_PORTAL_GLOBAL_SHORTCUTS, "version")
                .await
            {
                Some(ver) => GlobalShortcutsPortalStatus::from_version(ver),
                None => GlobalShortcutsPortalStatus::unknown(),
            }
        } else {
            GlobalShortcutsPortalStatus::unavailable()
        };

        let camera = if ifaces.contains(IFACE_PORTAL_CAMERA) {
            match self.get_u32_prop(IFACE_PORTAL_CAMERA, "version").await {
                Some(ver) => {
                    let present = self
                        .get_bool_prop(IFACE_PORTAL_CAMERA, "IsCameraPresent")
                        .await;
                    CameraPortalStatus::from_properties(ver, present)
                }
                None => CameraPortalStatus::unknown(),
            }
        } else {
            CameraPortalStatus::unavailable()
        };

        let usb = if ifaces.contains(IFACE_PORTAL_USB) {
            match self.get_u32_prop(IFACE_PORTAL_USB, "version").await {
                Some(ver) => UsbPortalStatus::from_version(ver),
                None => UsbPortalStatus::unknown(),
            }
        } else {
            UsbPortalStatus::unavailable()
        };

        let portal_ifaces: Vec<String> = ifaces
            .into_iter()
            .filter(|i| i.starts_with("org.freedesktop.portal."))
            .collect();

        Ok(PortalDiscoveryReport {
            portal_service_availability: Availability::Available,
            discovered_interfaces: portal_ifaces,
            screencast,
            remote_desktop,
            global_shortcuts,
            camera,
            usb,
        })
    }

    async fn query_permission_table(
        &self,
        table: &str,
    ) -> Result<PermissionTableReport, AkrepError> {
        validate_permission_store_table(table)?;

        if !self.name_has_owner(PERMISSION_STORE_BUS_NAME).await {
            return Ok(PermissionTableReport {
                table: table.to_string(),
                availability: Availability::Unavailable,
                entries: Vec::new(),
            });
        }

        let list_reply = self
            .conn
            .call_method(
                Some(PERMISSION_STORE_BUS_NAME),
                PERMISSION_STORE_OBJECT_PATH,
                Some(PERMISSION_STORE_INTERFACE),
                "List",
                &(table,),
            )
            .await;

        let ids: Vec<String> = match list_reply {
            Ok(msg) => msg.body().deserialize::<Vec<String>>().map_err(|e| {
                AkrepError::Internal(format!(
                    "PermissionStore List decode error: {}",
                    redact_secrets(&e.to_string())
                ))
            })?,
            Err(err) => {
                let err_s = redact_secrets(&err.to_string());
                if is_permission_table_missing_err(&err_s) {
                    return Ok(PermissionTableReport {
                        table: table.to_string(),
                        availability: Availability::Unknown,
                        entries: Vec::new(),
                    });
                }
                return Ok(PermissionTableReport {
                    table: table.to_string(),
                    availability: Availability::Unavailable,
                    entries: Vec::new(),
                });
            }
        };

        let mut entries = Vec::new();
        for resource_id in ids {
            let lookup_res = self
                .conn
                .call_method(
                    Some(PERMISSION_STORE_BUS_NAME),
                    PERMISSION_STORE_OBJECT_PATH,
                    Some(PERMISSION_STORE_INTERFACE),
                    "Lookup",
                    &(table, resource_id.as_str()),
                )
                .await;
            let Ok(lookup_msg) = lookup_res else {
                continue;
            };
            let Ok((app_map, _data)) = lookup_msg
                .body()
                .deserialize::<(HashMap<String, Vec<String>>, OwnedValue)>()
            else {
                continue;
            };
            let mut sorted_apps: Vec<(String, Vec<String>)> = app_map.into_iter().collect();
            sorted_apps.sort_by(|a, b| a.0.cmp(&b.0));
            for (app_id, permissions) in sorted_apps {
                entries.push(PermissionStoreEntry {
                    table: table.to_string(),
                    resource_id: resource_id.clone(),
                    app_id,
                    permissions,
                });
            }
        }

        Ok(PermissionTableReport {
            table: table.to_string(),
            availability: Availability::Available,
            entries,
        })
    }

    async fn revoke_permission(
        &self,
        table: &str,
        resource_id: &str,
        app_id: &str,
        user_confirmed: bool,
    ) -> Result<(), AkrepError> {
        if !user_confirmed {
            return Err(AkrepError::ForbiddenOperation(
                "User confirmation is required to revoke stored portal permission".to_string(),
            ));
        }
        validate_permission_store_table(table)?;
        validate_permission_store_token("resource_id", resource_id)?;
        validate_permission_store_token("app_id", app_id)?;

        self.conn
            .call_method(
                Some(PERMISSION_STORE_BUS_NAME),
                PERMISSION_STORE_OBJECT_PATH,
                Some(PERMISSION_STORE_INTERFACE),
                "DeletePermission",
                &(table.trim(), resource_id.trim(), app_id.trim()),
            )
            .await
            .map_err(|e| {
                let msg = redact_secrets(&e.to_string());
                if is_permission_table_missing_err(&msg) {
                    AkrepError::InvalidInput(format!(
                        "PermissionStore entry not found in table '{}' for '{}'",
                        table, app_id
                    ))
                } else {
                    AkrepError::ApplyFailed(format!(
                        "PermissionStore DeletePermission failed: {}",
                        msg
                    ))
                }
            })?;
        Ok(())
    }

    async fn inspect_pipewire_graph(&self) -> Result<PipewireGraphInspection, AkrepError> {
        if !self.pw_dump_bin.is_file() {
            return Ok(PipewireGraphInspection {
                availability: Availability::Unavailable,
                nodes: BTreeMap::new(),
                active_streams: Vec::new(),
            });
        }

        let pw_dump = self.pw_dump_bin.clone();
        let output = tokio::task::spawn_blocking(move || {
            Command::new(&pw_dump)
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .output()
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("pw-dump task join failed: {}", e)))?
        .map_err(AkrepError::from)?;

        if !output.status.success() {
            return Ok(PipewireGraphInspection {
                availability: Availability::Unavailable,
                nodes: BTreeMap::new(),
                active_streams: Vec::new(),
            });
        }

        let raw_json = String::from_utf8_lossy(&output.stdout);
        parse_pipewire_dump_json(&raw_json)
    }

    async fn terminate_stream(&self, node_id: u32, user_confirmed: bool) -> Result<(), AkrepError> {
        if !user_confirmed {
            return Err(AkrepError::ForbiddenOperation(
                "User confirmation is required before terminating an active PipeWire stream"
                    .to_string(),
            ));
        }
        let graph = self.inspect_pipewire_graph().await?;
        if graph.availability != Availability::Available {
            return Err(AkrepError::BackendUnavailable(
                "PipeWire session graph is unavailable".to_string(),
            ));
        }

        let uid = Self::current_uid();
        let _verified = validate_terminable_stream_node(&graph, node_id, uid)?;

        if !self.pw_cli_bin.is_file() {
            return Err(AkrepError::BackendUnavailable(
                "/usr/bin/pw-cli is not installed".to_string(),
            ));
        }

        let pw_cli = self.pw_cli_bin.clone();
        let node_arg = node_id.to_string();
        let output = tokio::task::spawn_blocking(move || {
            Command::new(&pw_cli)
                .args(["destroy", &node_arg])
                .env("LC_ALL", "C")
                .env("LANG", "C")
                .output()
        })
        .await
        .map_err(|e| AkrepError::Internal(format!("pw-cli task join failed: {}", e)))?
        .map_err(AkrepError::from)?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(AkrepError::ApplyFailed(format!(
                "pw-cli destroy {} failed: {}",
                node_id,
                redact_secrets(stderr.trim())
            )));
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
enum MockTableState {
    Missing,
    Present(Vec<PermissionStoreEntry>),
}

struct MockPrivacyState {
    portal_report: PortalDiscoveryReport,
    permission_store_online: bool,
    tables: BTreeMap<String, MockTableState>,
    pw_dump_json: Option<String>,
    session_uid: u32,
    destroyed_nodes: Vec<u32>,
    revoked_permissions: Vec<(String, String, String)>,
}

pub struct MockPortalPrivacyBackend {
    state: Mutex<MockPrivacyState>,
}

impl MockPortalPrivacyBackend {
    pub fn new_wayland_desktop(session_uid: u32) -> Self {
        let portal_report = PortalDiscoveryReport {
            portal_service_availability: Availability::Available,
            discovered_interfaces: vec![
                IFACE_PORTAL_CAMERA.to_string(),
                IFACE_PORTAL_GLOBAL_SHORTCUTS.to_string(),
                IFACE_PORTAL_REMOTE_DESKTOP.to_string(),
                IFACE_PORTAL_SCREENCAST.to_string(),
            ],
            screencast: ScreenCastPortalStatus::from_properties(
                5,
                Some(1 | 2 | 4),
                Some(1 | 2 | 4),
            ),
            remote_desktop: RemoteDesktopPortalStatus::from_properties(2, Some(1 | 2 | 4)),
            global_shortcuts: GlobalShortcutsPortalStatus::from_version(2),
            camera: CameraPortalStatus::from_properties(1, Some(true)),
            usb: UsbPortalStatus::unavailable(),
        };

        let mut tables = BTreeMap::new();
        for t in [
            "camera",
            "microphone",
            "screencast",
            "remote-desktop",
            "notifications",
        ] {
            tables.insert(t.to_string(), MockTableState::Present(Vec::new()));
        }
        tables.insert("devices".to_string(), MockTableState::Missing);

        let default_pw = serde_json::json!([
            {
                "id": 40,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "state": "running",
                    "props": {
                        "media.class": "Audio/Source",
                        "node.name": "alsa_input.pci-0000_00_1f.3.analog-stereo",
                        "media.name": "Built-in Audio Analog Stereo"
                    }
                }
            },
            {
                "id": 50,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "state": "running",
                    "props": {
                        "media.class": "Video/Source",
                        "media.role": "Screen",
                        "node.name": "xdg-desktop-portal-gnome",
                        "media.name": "xdp-screencast"
                    }
                }
            },
            {
                "id": 60,
                "type": "PipeWire:Interface:Node",
                "info": {
                    "state": "running",
                    "props": {
                        "media.class": "Video/Source",
                        "media.role": "Camera",
                        "device.api": "v4l2",
                        "node.name": "v4l2_input.usb-webcam",
                        "media.name": "Integrated Camera"
                    }
                }
            }
        ])
        .to_string();

        Self {
            state: Mutex::new(MockPrivacyState {
                portal_report,
                permission_store_online: true,
                tables,
                pw_dump_json: Some(default_pw),
                session_uid,
                destroyed_nodes: Vec::new(),
                revoked_permissions: Vec::new(),
            }),
        }
    }

    pub fn set_portal_report(&self, report: PortalDiscoveryReport) {
        self.state.lock().unwrap().portal_report = report;
    }

    pub fn set_portal_from_introspection_xml(
        &self,
        xml: &str,
        screencast_props: Option<(u32, Option<u32>, Option<u32>)>,
        remote_desktop_props: Option<(u32, Option<u32>)>,
        global_shortcuts_ver: Option<u32>,
        camera_props: Option<(u32, Option<bool>)>,
        usb_ver: Option<u32>,
    ) {
        let ifaces = parse_portal_introspection_interfaces(xml);
        let screencast = if ifaces.contains(IFACE_PORTAL_SCREENCAST) {
            match screencast_props {
                Some((v, sm, cm)) => ScreenCastPortalStatus::from_properties(v, sm, cm),
                None => ScreenCastPortalStatus::unknown(),
            }
        } else {
            ScreenCastPortalStatus::unavailable()
        };
        let remote_desktop = if ifaces.contains(IFACE_PORTAL_REMOTE_DESKTOP) {
            match remote_desktop_props {
                Some((v, dm)) => RemoteDesktopPortalStatus::from_properties(v, dm),
                None => RemoteDesktopPortalStatus::unknown(),
            }
        } else {
            RemoteDesktopPortalStatus::unavailable()
        };
        let global_shortcuts = if ifaces.contains(IFACE_PORTAL_GLOBAL_SHORTCUTS) {
            match global_shortcuts_ver {
                Some(v) => GlobalShortcutsPortalStatus::from_version(v),
                None => GlobalShortcutsPortalStatus::unknown(),
            }
        } else {
            GlobalShortcutsPortalStatus::unavailable()
        };
        let camera = if ifaces.contains(IFACE_PORTAL_CAMERA) {
            match camera_props {
                Some((v, p)) => CameraPortalStatus::from_properties(v, p),
                None => CameraPortalStatus::unknown(),
            }
        } else {
            CameraPortalStatus::unavailable()
        };
        let usb = if ifaces.contains(IFACE_PORTAL_USB) {
            match usb_ver {
                Some(v) => UsbPortalStatus::from_version(v),
                None => UsbPortalStatus::unknown(),
            }
        } else {
            UsbPortalStatus::unavailable()
        };

        let discovered_interfaces = ifaces
            .into_iter()
            .filter(|i| i.starts_with("org.freedesktop.portal."))
            .collect();

        self.set_portal_report(PortalDiscoveryReport {
            portal_service_availability: Availability::Available,
            discovered_interfaces,
            screencast,
            remote_desktop,
            global_shortcuts,
            camera,
            usb,
        });
    }

    pub fn set_permission_table_entries(&self, table: &str, entries: Vec<PermissionStoreEntry>) {
        self.state
            .lock()
            .unwrap()
            .tables
            .insert(table.to_string(), MockTableState::Present(entries));
    }

    pub fn set_permission_table_missing(&self, table: &str) {
        self.state
            .lock()
            .unwrap()
            .tables
            .insert(table.to_string(), MockTableState::Missing);
    }

    pub fn set_pipewire_dump_json(&self, json: Option<String>) {
        self.state.lock().unwrap().pw_dump_json = json;
    }

    pub fn destroyed_nodes(&self) -> Vec<u32> {
        self.state.lock().unwrap().destroyed_nodes.clone()
    }

    pub fn revoked_permissions(&self) -> Vec<(String, String, String)> {
        self.state.lock().unwrap().revoked_permissions.clone()
    }
}

#[async_trait]
impl PortalPrivacyBackend for MockPortalPrivacyBackend {
    async fn discover_portals(&self) -> Result<PortalDiscoveryReport, AkrepError> {
        Ok(self.state.lock().unwrap().portal_report.clone())
    }

    async fn query_permission_table(
        &self,
        table: &str,
    ) -> Result<PermissionTableReport, AkrepError> {
        validate_permission_store_table(table)?;
        let guard = self.state.lock().unwrap();
        if !guard.permission_store_online {
            return Ok(PermissionTableReport {
                table: table.to_string(),
                availability: Availability::Unavailable,
                entries: Vec::new(),
            });
        }
        match guard.tables.get(table) {
            Some(MockTableState::Present(entries)) => Ok(PermissionTableReport {
                table: table.to_string(),
                availability: Availability::Available,
                entries: entries.clone(),
            }),
            Some(MockTableState::Missing) | None => Ok(PermissionTableReport {
                table: table.to_string(),
                availability: Availability::Unknown,
                entries: Vec::new(),
            }),
        }
    }

    async fn revoke_permission(
        &self,
        table: &str,
        resource_id: &str,
        app_id: &str,
        user_confirmed: bool,
    ) -> Result<(), AkrepError> {
        if !user_confirmed {
            return Err(AkrepError::ForbiddenOperation(
                "User confirmation is required to revoke stored portal permission".to_string(),
            ));
        }
        validate_permission_store_table(table)?;
        validate_permission_store_token("resource_id", resource_id)?;
        validate_permission_store_token("app_id", app_id)?;

        let mut guard = self.state.lock().unwrap();
        if !guard.permission_store_online {
            return Err(AkrepError::BackendUnavailable(
                "PermissionStore is offline".to_string(),
            ));
        }
        match guard.tables.get_mut(table) {
            Some(MockTableState::Present(entries)) => {
                let before_len = entries.len();
                entries.retain(|e| !(e.resource_id == resource_id && e.app_id == app_id));
                if entries.len() == before_len {
                    return Err(AkrepError::InvalidInput(format!(
                        "Permission entry not found in '{}' for app '{}'",
                        table, app_id
                    )));
                }
                guard.revoked_permissions.push((
                    table.to_string(),
                    resource_id.to_string(),
                    app_id.to_string(),
                ));
                Ok(())
            }
            Some(MockTableState::Missing) | None => Err(AkrepError::InvalidInput(format!(
                "PermissionStore table '{}' does not exist",
                table
            ))),
        }
    }

    async fn inspect_pipewire_graph(&self) -> Result<PipewireGraphInspection, AkrepError> {
        let raw_opt = self.state.lock().unwrap().pw_dump_json.clone();
        let Some(raw) = raw_opt else {
            return Ok(PipewireGraphInspection {
                availability: Availability::Unavailable,
                nodes: BTreeMap::new(),
                active_streams: Vec::new(),
            });
        };
        parse_pipewire_dump_json(&raw)
    }

    async fn terminate_stream(&self, node_id: u32, user_confirmed: bool) -> Result<(), AkrepError> {
        if !user_confirmed {
            return Err(AkrepError::ForbiddenOperation(
                "User confirmation is required before terminating an active PipeWire stream"
                    .to_string(),
            ));
        }
        let graph = self.inspect_pipewire_graph().await?;
        if graph.availability != Availability::Available {
            return Err(AkrepError::BackendUnavailable(
                "PipeWire session graph is unavailable".to_string(),
            ));
        }

        let mut guard = self.state.lock().unwrap();
        let uid = guard.session_uid;
        let _verified = validate_terminable_stream_node(&graph, node_id, uid)?;

        if let Some(ref raw) = guard.pw_dump_json {
            if let Ok(mut arr) = serde_json::from_str::<Vec<serde_json::Value>>(raw) {
                arr.retain(|obj| {
                    let obj_id = obj.get("id").and_then(|v| v.as_u64());
                    if obj_id == Some(u64::from(node_id)) {
                        return false;
                    }
                    let info = obj.get("info").unwrap_or(&serde_json::Value::Null);
                    let in_node = json_u32_prop(info, "input-node-id");
                    in_node != Some(node_id)
                });
                guard.pw_dump_json = Some(serde_json::to_string(&arr).unwrap_or_default());
            }
        }
        guard.destroyed_nodes.push(node_id);
        Ok(())
    }
}
