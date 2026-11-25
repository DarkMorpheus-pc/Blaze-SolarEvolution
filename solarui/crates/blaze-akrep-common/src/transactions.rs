use crate::models::{ActionRequest, StateFingerprint};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    Draft,
    AwaitingAuthorization,
    Prepared,
    Applying,
    Verifying,
    Committed,
    RollingBack,
    RolledBack,
    RollbackFailed,
    Interrupted,
}

impl TransactionState {
    pub fn is_in_flight(self) -> bool {
        matches!(self, Self::Applying | Self::Verifying | Self::RollingBack)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub tx_id: String,
    pub plan_id: String,
    pub plan_sha256: String,
    pub actor_uid: u32,
    pub actor_bus_name: String,
    pub polkit_action: String,
    pub polkit_authorized: bool,
    pub request: ActionRequest,
    pub state: TransactionState,
    pub pre_snapshot: serde_json::Value,
    pub target_state: serde_json::Value,
    pub pre_fingerprints: Vec<StateFingerprint>,
    pub post_fingerprints: Vec<StateFingerprint>,
    pub apply_result: Option<String>,
    pub rollback_result: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}
