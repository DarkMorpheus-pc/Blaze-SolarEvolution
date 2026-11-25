use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "code", content = "message", rename_all = "snake_case")]
pub enum AkrepError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("polkit authorization denied: {0}")]
    PolkitDenied(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("forbidden operation: {0}")]
    ForbiddenOperation(String),
    #[error("plan mismatch: {0}")]
    PlanMismatch(String),
    #[error("plan expired: {0}")]
    PlanExpired(String),
    #[error("transaction conflict: {0}")]
    TransactionConflict(String),
    #[error("external drift detected: {0}")]
    ExternalDriftDetected(String),
    #[error("backend unavailable: {0}")]
    BackendUnavailable(String),
    #[error("apply failed: {0}")]
    ApplyFailed(String),
    #[error("verification failed: {0}")]
    VerifyFailed(String),
    #[error("rollback failed: {0}")]
    RollbackFailed(String),
    #[error("io error: {0}")]
    IoError(String),
    #[error("internal error: {0}")]
    Internal(String),
}

impl AkrepError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized(_) => "unauthorized",
            Self::PolkitDenied(_) => "polkit_denied",
            Self::InvalidInput(_) => "invalid_input",
            Self::ForbiddenOperation(_) => "forbidden_operation",
            Self::PlanMismatch(_) => "plan_mismatch",
            Self::PlanExpired(_) => "plan_expired",
            Self::TransactionConflict(_) => "transaction_conflict",
            Self::ExternalDriftDetected(_) => "external_drift_detected",
            Self::BackendUnavailable(_) => "backend_unavailable",
            Self::ApplyFailed(_) => "apply_failed",
            Self::VerifyFailed(_) => "verify_failed",
            Self::RollbackFailed(_) => "rollback_failed",
            Self::IoError(_) => "io_error",
            Self::Internal(_) => "internal",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Unauthorized(m)
            | Self::PolkitDenied(m)
            | Self::InvalidInput(m)
            | Self::ForbiddenOperation(m)
            | Self::PlanMismatch(m)
            | Self::PlanExpired(m)
            | Self::TransactionConflict(m)
            | Self::ExternalDriftDetected(m)
            | Self::BackendUnavailable(m)
            | Self::ApplyFailed(m)
            | Self::VerifyFailed(m)
            | Self::RollbackFailed(m)
            | Self::IoError(m)
            | Self::Internal(m) => m,
        }
    }

    pub fn redacted(&self) -> Self {
        let clean = redact_secrets(self.message());
        match self {
            Self::Unauthorized(_) => Self::Unauthorized(clean),
            Self::PolkitDenied(_) => Self::PolkitDenied(clean),
            Self::InvalidInput(_) => Self::InvalidInput(clean),
            Self::ForbiddenOperation(_) => Self::ForbiddenOperation(clean),
            Self::PlanMismatch(_) => Self::PlanMismatch(clean),
            Self::PlanExpired(_) => Self::PlanExpired(clean),
            Self::TransactionConflict(_) => Self::TransactionConflict(clean),
            Self::ExternalDriftDetected(_) => Self::ExternalDriftDetected(clean),
            Self::BackendUnavailable(_) => Self::BackendUnavailable(clean),
            Self::ApplyFailed(_) => Self::ApplyFailed(clean),
            Self::VerifyFailed(_) => Self::VerifyFailed(clean),
            Self::RollbackFailed(_) => Self::RollbackFailed(clean),
            Self::IoError(_) => Self::IoError(clean),
            Self::Internal(_) => Self::Internal(clean),
        }
    }

    pub fn to_redacted_json(&self) -> String {
        let safe = self.redacted();
        serde_json::to_string(&safe).unwrap_or_else(|_| {
            format!(
                "{{\"code\":\"{}\",\"message\":\"[REDACTED]\"}}",
                safe.code()
            )
        })
    }
}

impl From<std::io::Error> for AkrepError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(redact_secrets(&err.to_string()))
    }
}

struct RedactionRules {
    json_kv: Regex,
    assign_kv: Regex,
    bearer: Regex,
    api_prefixes: Regex,
}

fn redaction_rules() -> &'static RedactionRules {
    static RULES: OnceLock<RedactionRules> = OnceLock::new();
    RULES.get_or_init(|| RedactionRules {
        json_kv: Regex::new(
            r#"(?i)"(password|passwd|pwd|token|access_token|refresh_token|api[_-]?key|apikey|secret|client_secret|x-berp-token)"(\s*:\s*)"[^"]*""#,
        )
        .expect("valid json secret regex"),
        assign_kv: Regex::new(
            r#"(?i)\b(password|passwd|pwd|token|access_token|refresh_token|api[_-]?key|apikey|secret|client_secret|x-berp-token)(\s*[=:]\s*)("[^"]*"|'[^']*'|[^\s,;"'}\]]+)"#,
        )
        .expect("valid kv secret regex"),
        bearer: Regex::new(r"(?i)\b(Bearer\s+)[A-Za-z0-9._~+/=-]+")
            .expect("valid bearer regex"),
        api_prefixes: Regex::new(
            r"\b(sk-[A-Za-z0-9_-]{12,}|ghp_[A-Za-z0-9]{16,}|AIza[0-9A-Za-z_-]{20,})\b",
        )
        .expect("valid api key prefix regex"),
    })
}

pub fn redact_secrets(input: &str) -> String {
    let rules = redaction_rules();
    let step1 = rules.json_kv.replace_all(input, "\"$1\"$2\"[REDACTED]\"");
    let step2 = rules.assign_kv.replace_all(&step1, "$1$2[REDACTED]");
    let step3 = rules.bearer.replace_all(&step2, "$1[REDACTED]");
    rules
        .api_prefixes
        .replace_all(&step3, "[REDACTED]")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_secrets_covers_passwords_tokens_bearer_and_api_keys() {
        let raw = "auth failed: password=supersecret token=abc12345 Bearer eyJhbGciOiJIUzI1NiJ9 X-BERP-Token: tok_9988 secret=mysec api_key=sk-1234567890abcdef";
        let scrubbed = redact_secrets(raw);
        assert!(!scrubbed.contains("supersecret"));
        assert!(!scrubbed.contains("abc12345"));
        assert!(!scrubbed.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(!scrubbed.contains("tok_9988"));
        assert!(!scrubbed.contains("mysec"));
        assert!(!scrubbed.contains("sk-1234567890abcdef"));
        assert!(scrubbed.contains("password=[REDACTED]"));
        assert!(scrubbed.contains("token=[REDACTED]"));
        assert!(scrubbed.contains("Bearer [REDACTED]"));
        assert!(scrubbed.contains("X-BERP-Token: [REDACTED]"));
    }

    #[test]
    fn test_akrep_error_structured_json_roundtrip() {
        let err = AkrepError::PolkitDenied("caller denied with token=leak123".to_string());
        let json = err.to_redacted_json();
        assert!(!json.contains("leak123"));
        assert!(json.contains("[REDACTED]"));
        let decoded: AkrepError = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.code(), "polkit_denied");
    }
}
