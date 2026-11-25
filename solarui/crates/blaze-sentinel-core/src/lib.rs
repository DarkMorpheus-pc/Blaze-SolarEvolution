pub mod engine;
pub mod findings;
pub mod models;
pub mod severity;

pub use engine::*;
pub use findings::*;
pub use models::*;
pub use severity::*;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn test_core_default_config_uses_root_only_canary() {
        let cfg = SentinelConfig::default();
        assert!(cfg.canary_paths.contains(&PathBuf::from(ROOT_CANARY_PATH)));
        assert!(!cfg.canary_paths.contains(&PathBuf::from("/var/tmp/.blaze_canary")));
    }

    #[test]
    fn test_core_new_engine_does_not_create_canaries_eagerly() {
        let _engine = SentinelEngine::new();
        assert!(!Path::new("/var/tmp/.blaze_canary").exists());
    }

    #[test]
    fn test_core_posture_report_has_seven_controls() {
        let engine = SentinelEngine::new();
        let report = evaluate_security_posture(&engine, false);
        assert_eq!(report.total, 7);
        assert_eq!(report.controls.len(), 7);
    }

    #[test]
    fn test_core_canary_is_fail_closed_and_sha256_verified() {
        let dir = std::env::temp_dir().join(format!("blaze_core_canary_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let canary = dir.join(".blaze_canary");
        let digest = expected_canary_hash_path(&canary);

        assert!(!is_canary_intact(&canary));
        let tok = generate_random_canary_token().unwrap();
        fs::write(&canary, &tok).unwrap();
        fs::write(&digest, sha256_hex(tok.as_bytes())).unwrap();
        assert!(is_canary_intact(&canary));

        fs::write(&canary, "TAMPERED").unwrap();
        assert!(!is_canary_intact(&canary));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_severity_ordering_and_findings_from_posture() {
        assert_eq!(AkrepSeverity::Normal.combine(AkrepSeverity::Warning), AkrepSeverity::Warning);
        assert_eq!(AkrepSeverity::Warning.combine(AkrepSeverity::Critical), AkrepSeverity::Critical);
        let report = PostureReport {
            passed: 6,
            total: 7,
            verified: true,
            integrity_ok: true,
            controls: vec![SecurityControl {
                id: "firewall".to_string(),
                name: "Firewall".to_string(),
                title: "Firewall".to_string(),
                passed: false,
                level: "fail".to_string(),
                critical: true,
                detail: "no active firewall".to_string(),
            }],
            checks: vec![],
            alerts: vec![],
        };
        let f = findings_from_posture(&report);
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].section, "firewall");
        assert_eq!(f[0].severity, AkrepSeverity::Critical);
    }
}
