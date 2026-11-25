use async_trait::async_trait;
use blaze_akrep_common::{redact_secrets, AkrepError};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use zbus::zvariant::Value;
use zbus::Connection;

const DBUS_DEST: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_IFACE: &str = "org.freedesktop.DBus";

const POLKIT_DEST: &str = "org.freedesktop.PolicyKit1";
const POLKIT_PATH: &str = "/org/freedesktop/PolicyKit1/Authority";
const POLKIT_IFACE: &str = "org.freedesktop.PolicyKit1.Authority";
const POLKIT_FLAG_ALLOW_USER_INTERACTION: u32 = 0x0000_0001;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerIdentity {
    pub uid: u32,
    pub bus_name: String,
}

#[async_trait]
pub trait Authorizer: Send + Sync {
    async fn verify_caller(
        &self,
        caller_bus_name: &str,
        action_id: &str,
        allow_interaction: bool,
    ) -> Result<CallerIdentity, AkrepError>;
}

pub fn validate_unique_bus_name(caller_bus_name: &str) -> Result<(), AkrepError> {
    let trimmed = caller_bus_name.trim();
    if trimmed.is_empty() || !trimmed.starts_with(':') || trimmed.len() > 255 {
        return Err(AkrepError::Unauthorized(
            "Missing or invalid D-Bus unique sender name".to_string(),
        ));
    }
    if !trimmed[1..]
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err(AkrepError::Unauthorized(
            "Malformed D-Bus unique sender name".to_string(),
        ));
    }
    Ok(())
}

type PolkitSubject<'a> = (&'a str, HashMap<&'a str, Value<'a>>);
type PolkitAuthResult = (bool, bool, HashMap<String, String>);

pub fn build_polkit_subject(caller_bus_name: &str) -> PolkitSubject<'_> {
    let mut subject_details = HashMap::new();
    subject_details.insert("name", Value::from(caller_bus_name));
    ("system-bus-name", subject_details)
}

pub struct SystemPolkitAuthorizer {
    conn: Connection,
}

impl SystemPolkitAuthorizer {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

#[async_trait]
impl Authorizer for SystemPolkitAuthorizer {
    async fn verify_caller(
        &self,
        caller_bus_name: &str,
        action_id: &str,
        allow_interaction: bool,
    ) -> Result<CallerIdentity, AkrepError> {
        validate_unique_bus_name(caller_bus_name)?;
        if action_id.trim().is_empty() {
            return Err(AkrepError::InvalidInput(
                "Polkit action_id must not be empty".to_string(),
            ));
        }

        let uid_reply = self
            .conn
            .call_method(
                Some(DBUS_DEST),
                DBUS_PATH,
                Some(DBUS_IFACE),
                "GetConnectionUnixUser",
                &(caller_bus_name,),
            )
            .await
            .map_err(|e| {
                AkrepError::Unauthorized(format!(
                    "Failed to resolve D-Bus caller unix user: {}",
                    redact_secrets(&e.to_string())
                ))
            })?;

        let uid: u32 = uid_reply.body().deserialize().map_err(|e| {
            AkrepError::Unauthorized(format!(
                "Invalid GetConnectionUnixUser response: {}",
                redact_secrets(&e.to_string())
            ))
        })?;

        let subject = build_polkit_subject(caller_bus_name);
        let details: HashMap<&str, &str> = HashMap::new();
        let flags: u32 = if allow_interaction {
            POLKIT_FLAG_ALLOW_USER_INTERACTION
        } else {
            0
        };
        let cancellation_id = "";

        let auth_reply = self
            .conn
            .call_method(
                Some(POLKIT_DEST),
                POLKIT_PATH,
                Some(POLKIT_IFACE),
                "CheckAuthorization",
                &(subject, action_id, details, flags, cancellation_id),
            )
            .await
            .map_err(|e| {
                AkrepError::PolkitDenied(format!(
                    "Polkit CheckAuthorization call failed: {}",
                    redact_secrets(&e.to_string())
                ))
            })?;

        let ((is_authorized, _is_challenge, _resp_details),): (PolkitAuthResult,) = auth_reply
            .body()
            .deserialize()
            .or_else(|_| {
                auth_reply
                    .body()
                    .deserialize::<PolkitAuthResult>()
                    .map(|r| (r,))
            })
            .map_err(|e| {
                AkrepError::PolkitDenied(format!(
                    "Malformed Polkit CheckAuthorization response: {}",
                    redact_secrets(&e.to_string())
                ))
            })?;

        if !is_authorized {
            return Err(AkrepError::PolkitDenied(format!(
                "Polkit denied action '{}' for caller '{}' (uid={})",
                action_id, caller_bus_name, uid
            )));
        }

        Ok(CallerIdentity {
            uid,
            bus_name: caller_bus_name.to_string(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthCheckAttempt {
    pub caller_bus_name: String,
    pub action_id: String,
    pub allow_interaction: bool,
    pub granted: bool,
}

#[derive(Debug, Clone)]
struct MockCallerRule {
    uid: u32,
    allow_all: bool,
    allowed_actions: HashSet<String>,
}

#[derive(Default)]
pub struct MockAuthorizer {
    callers: Mutex<HashMap<String, MockCallerRule>>,
    attempts: Mutex<Vec<AuthCheckAttempt>>,
}

impl MockAuthorizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_all_for_bus(&self, bus_name: &str, uid: u32) {
        let mut guard = self.callers.lock().expect("mock callers mutex");
        guard.insert(
            bus_name.to_string(),
            MockCallerRule {
                uid,
                allow_all: true,
                allowed_actions: HashSet::new(),
            },
        );
    }

    pub fn allow_actions_for_bus<'a>(
        &self,
        bus_name: &str,
        uid: u32,
        actions: impl IntoIterator<Item = &'a str>,
    ) {
        let mut guard = self.callers.lock().expect("mock callers mutex");
        guard.insert(
            bus_name.to_string(),
            MockCallerRule {
                uid,
                allow_all: false,
                allowed_actions: actions.into_iter().map(ToString::to_string).collect(),
            },
        );
    }

    pub fn deny_all_for_bus(&self, bus_name: &str, uid: u32) {
        self.allow_actions_for_bus(bus_name, uid, std::iter::empty());
    }

    pub fn recorded_attempts(&self) -> Vec<AuthCheckAttempt> {
        self.attempts.lock().expect("mock attempts mutex").clone()
    }
}

#[async_trait]
impl Authorizer for MockAuthorizer {
    async fn verify_caller(
        &self,
        caller_bus_name: &str,
        action_id: &str,
        allow_interaction: bool,
    ) -> Result<CallerIdentity, AkrepError> {
        validate_unique_bus_name(caller_bus_name)?;

        let rule_opt = {
            let guard = self.callers.lock().expect("mock callers mutex");
            guard.get(caller_bus_name).cloned()
        };

        let Some(rule) = rule_opt else {
            self.attempts
                .lock()
                .expect("mock attempts mutex")
                .push(AuthCheckAttempt {
                    caller_bus_name: caller_bus_name.to_string(),
                    action_id: action_id.to_string(),
                    allow_interaction,
                    granted: false,
                });
            return Err(AkrepError::Unauthorized(format!(
                "Unknown D-Bus connection '{}'",
                caller_bus_name
            )));
        };

        let granted = rule.allow_all || rule.allowed_actions.contains(action_id);
        self.attempts
            .lock()
            .expect("mock attempts mutex")
            .push(AuthCheckAttempt {
                caller_bus_name: caller_bus_name.to_string(),
                action_id: action_id.to_string(),
                allow_interaction,
                granted,
            });

        if !granted {
            return Err(AkrepError::PolkitDenied(format!(
                "Polkit denied '{}' for '{}' (uid={})",
                action_id, caller_bus_name, rule.uid
            )));
        }

        Ok(CallerIdentity {
            uid: rule.uid,
            bus_name: caller_bus_name.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_polkit_wire_signatures_match_policykit1_spec() {
        let subject = build_polkit_subject(":1.94");
        assert_eq!(subject.0, "system-bus-name");
        assert!(subject.1.contains_key("name"));

        let details: HashMap<&str, &str> = HashMap::new();
        let call_msg = zbus::message::Message::method_call(POLKIT_PATH, "CheckAuthorization")
            .unwrap()
            .destination(POLKIT_DEST)
            .unwrap()
            .interface(POLKIT_IFACE)
            .unwrap()
            .build(&(
                subject,
                "tr.com.blazeos.Akrep1.firewall.manage",
                details,
                1u32,
                "",
            ))
            .unwrap();

        // In zvariant 5.x, body().signature() wraps top-level message body fields in an outer Structure
        // (`((sa{sv})sa{ss}us)`), which serializes on the D-Bus wire header as `(sa{sv})sa{ss}us`.
        let body_sig = call_msg.body().signature().to_string();
        assert_eq!(
            body_sig
                .strip_prefix('(')
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or(&body_sig),
            "(sa{sv})sa{ss}us"
        );

        // Verify Polkit single-struct out-arg ((bba{ss})) deserializes cleanly
        let fake_polkit_result: PolkitAuthResult = (true, false, HashMap::new());
        let reply_msg = zbus::message::Message::method_return(&call_msg.header())
            .unwrap()
            .build(&(fake_polkit_result,))
            .unwrap();
        let reply_sig = reply_msg.body().signature().to_string();
        assert_eq!(reply_sig, "(bba{ss})");
        let ((is_auth, is_chal, _),): (PolkitAuthResult,) = reply_msg.body().deserialize().unwrap();
        assert!(is_auth);
        assert!(!is_chal);
    }

    #[test]
    fn test_validate_unique_bus_name_rejects_well_known_or_empty() {
        assert!(validate_unique_bus_name("").is_err());
        assert!(validate_unique_bus_name("tr.com.blazeos.Fake").is_err());
        assert!(validate_unique_bus_name(":1.42").is_ok());
    }
}
