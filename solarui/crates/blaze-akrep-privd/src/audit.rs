use blaze_akrep_common::{redact_secrets, AkrepError};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEvent {
    pub timestamp: String,
    pub event: String,
    pub tx_id: Option<String>,
    pub plan_id: Option<String>,
    pub actor_uid: Option<u32>,
    pub actor_bus_name: Option<String>,
    pub polkit_action: Option<String>,
    pub outcome: String,
    pub message: String,
}

pub struct AuditLogger {
    path: PathBuf,
    write_lock: Mutex<()>,
}

impl AuditLogger {
    pub fn new(path: PathBuf) -> Result<Self, AkrepError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        file.sync_all()?;

        Ok(Self {
            path,
            write_lock: Mutex::new(()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log(
        &self,
        event: &str,
        tx_id: Option<&str>,
        plan_id: Option<&str>,
        actor_uid: Option<u32>,
        actor_bus_name: Option<&str>,
        polkit_action: Option<&str>,
        outcome: &str,
        message: &str,
    ) -> Result<(), AkrepError> {
        let entry = AuditEvent {
            timestamp: Utc::now().to_rfc3339(),
            event: redact_secrets(event),
            tx_id: tx_id.map(redact_secrets),
            plan_id: plan_id.map(redact_secrets),
            actor_uid,
            actor_bus_name: actor_bus_name.map(redact_secrets),
            polkit_action: polkit_action.map(redact_secrets),
            outcome: redact_secrets(outcome),
            message: redact_secrets(message),
        };

        let mut line = serde_json::to_string(&entry)
            .map_err(|e| AkrepError::Internal(format!("audit json encode failed: {}", e)))?;
        line.push('\n');

        let _guard = self
            .write_lock
            .lock()
            .map_err(|_| AkrepError::Internal("audit write mutex poisoned".to_string()))?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&self.path)?;
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))?;
        file.write_all(line.as_bytes())?;
        file.sync_data()?;
        Ok(())
    }

    pub fn read_events(&self) -> Result<Vec<AuditEvent>, AkrepError> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let raw = fs::read_to_string(&self.path)?;
        let mut events = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let parsed: AuditEvent = serde_json::from_str(trimmed)
                .map_err(|e| AkrepError::Internal(format!("corrupt audit line: {}", e)))?;
            events.push(parsed);
        }
        Ok(events)
    }
}
