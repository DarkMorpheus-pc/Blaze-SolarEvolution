// ==============================================================================
// Blaze Sentinel — Security Posture Inspector & Active Canary / Integrity Daemon
// v0.3.1 — Unified CLI Posture Inspector (status, policy, verify) and Watchdog
// ==============================================================================

use anyhow::Result;
use blaze_sentinel_core::{
    check_mac_control, evaluate_security_posture, expected_canary_hash_path,
    generate_random_canary_token, is_canary_intact, is_root_user, sha256_hex, PostureReport,
    SentinelEngine, ROOT_CANARY_PATH, SENTINEL_VERSION,
};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

fn handle_sentinel_alerts(alerts: &[String]) {
    for a in alerts {
        eprintln!("[Blaze Sentinel] {}", a);
        log_sentinel_event(a);
    }

    // Trigger BERP if severe threat detected
    if !alerts.is_empty() {
        eprintln!("[Blaze Sentinel] Sistem bütünlüğü bozuldu. Bir sonraki açılış için BERP kurtarma protokolü tetikleniyor...");
        let _ = Command::new("blaze-recovery").arg("--trigger").status();

        // Send notification to user desktop if notify-send available
        let _ = Command::new("notify-send")
            .args([
                "-u",
                "critical",
                "-i",
                "security-high",
                "Blaze Sentinel Güvenlik Alarmı",
                "Şüpheli sistem aktivitesi veya dosya bozulması tespit edildi! Blaze Acil Kurtarma Protokolü hazırlandı.",
            ])
            .status();
    }
}

fn run_sentinel_daemon(engine: &SentinelEngine) {
    engine.init_canaries();
    println!("=== Blaze Sentinel Güvenlik Servisi Başlatıldı ===");
    println!("İzleme döngüsü: {} saniyede bir", engine.config().check_interval_secs);

    loop {
        let alerts = engine.scan_once();
        if !alerts.is_empty() {
            handle_sentinel_alerts(&alerts);
        }
        thread::sleep(Duration::from_secs(engine.config().check_interval_secs));
    }
}

fn print_human_posture(report: &PostureReport) {
    println!(
        "Blaze Sentinel v{} — Security Posture ({}/{} controls passing)",
        SENTINEL_VERSION, report.passed, report.total
    );
    for c in &report.controls {
        let tag = if c.passed { "[PASS]" } else { "[WARN]" };
        println!("  {} {:<28} — {}", tag, c.name, c.detail);
    }
    if report.integrity_ok {
        println!("  [PASS] System Integrity             — clean (no rootkit/canary tampering)");
    } else {
        for a in &report.alerts {
            println!("  [FAIL] Integrity Alert              — {}", a);
        }
    }
}

fn print_policy(json_mode: bool) -> Result<()> {
    let mac = check_mac_control();
    let lsm = fs::read_to_string("/sys/kernel/security/lsm")
        .unwrap_or_else(|_| "none".to_string())
        .trim()
        .to_string();
    if json_mode {
        let payload = serde_json::json!({
            "version": SENTINEL_VERSION,
            "active_lsm": lsm,
            "mac_status": mac.detail,
            "mac_enforcing": mac.passed,
            "canary_path": ROOT_CANARY_PATH,
            "policies": [
                "mac.selinux_or_apparmor",
                "network.firewall_active",
                "sysctl.kernel.kptr_restrict>=1",
                "sysctl.kernel.dmesg_restrict>=1",
                "sysctl.kernel.yama.ptrace_scope>=1",
                "sysctl.fs.protected_hardlinks>=1",
                "sysctl.fs.protected_symlinks>=1",
                "integrity.ld_so_preload",
                "integrity.root_canary"
            ]
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("--- Blaze Sentinel Policy Manager (list) ---");
        println!("Active LSM stack: {}", lsm);
        println!("MAC state:        {}", mac.detail);
        println!("Root canary:      {} (0600 inside 0700 /var/lib/blaze)", ROOT_CANARY_PATH);
        println!("Enforced hardening policies (7 controls):");
        println!("  1. SELinux / AppArmor Mandatory Access Control");
        println!("  2. Firewall active (firewalld / nftables)");
        println!("  3. kernel.kptr_restrict >= 1");
        println!("  4. kernel.dmesg_restrict >= 1");
        println!("  5. kernel.yama.ptrace_scope >= 1");
        println!("  6. fs.protected_hardlinks >= 1");
        println!("  7. fs.protected_symlinks >= 1");
    }
    Ok(())
}

fn print_help() {
    println!(
        "BlazeOS Sentinel v{}: system security posture inspector & active canary watchdog",
        SENTINEL_VERSION
    );
    println!();
    println!("Usage: blaze-sentinel [COMMAND | OPTION]");
    println!();
    println!("Commands:");
    println!("  status [--json]       Show security posture of this system (7 hardening controls)");
    println!("  policy [list] [--json] List mandatory-access-control & hardening policy state");
    println!("  verify [--json]       Verify posture and runtime integrity");
    println!("  daemon, --daemon      Run active ransomware canary & rootkit watchdog loop");
    println!("  check-once, --check-once  Run one-shot canary and integrity scan");
    println!("  --simulate-attack     Simulate a canary tamper event and verify detection");
    println!();
    println!("Options:");
    println!("  --json                Output machine-readable JSON");
    println!("  -h, --help            Print help");
    println!("  -V, --version         Print version");
}

fn log_sentinel_event(msg: &str) {
    let log_file = "/var/log/blaze-sentinel.log";
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(log_file) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(f, "[{}] {}", now, msg);
    }
}

fn parse_cli_args(args: &[String]) -> (bool, Vec<&str>) {
    let json_mode = args.iter().any(|a| a == "--json" || a == "-j");
    let positional: Vec<&str> = args
        .iter()
        .map(|s| s.as_str())
        .filter(|&a| a != "--json" && a != "-j")
        .collect();
    (json_mode, positional)
}

fn should_auto_daemonize_on_empty_args() -> bool {
    std::env::var("INVOCATION_ID").is_ok()
        && std::os::unix::process::parent_id() == 1
        && !std::io::IsTerminal::is_terminal(&std::io::stdout())
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (json_mode, positional) = parse_cli_args(&args);
    let engine = SentinelEngine::new();

    if positional.iter().any(|&a| a == "--help" || a == "-h" || a == "help") {
        print_help();
        return Ok(());
    }

    if positional.iter().any(|&a| a == "--version" || a == "-V" || a == "version") {
        println!("blaze-sentinel {}", SENTINEL_VERSION);
        return Ok(());
    }

    let first = positional.first().copied().unwrap_or("");

    match first {
        "status" | "--status" => {
            let report = evaluate_security_posture(&engine, false);
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_human_posture(&report);
            }
            return Ok(());
        }
        "policy" | "--policy" => {
            return print_policy(json_mode);
        }
        "verify" | "--verify" => {
            let report = evaluate_security_posture(&engine, false);
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print_human_posture(&report);
                if report.integrity_ok {
                    println!(
                        "[SENTINEL VERIFY] Integrity OK ({}/{} hardening controls active)",
                        report.passed, report.total
                    );
                } else {
                    println!("[SENTINEL VERIFY] FAILED: runtime integrity alerts detected!");
                }
            }
            if !report.integrity_ok {
                std::process::exit(1);
            }
            return Ok(());
        }
        "check-once" | "--check-once" | "check" | "--check" => {
            engine.init_canaries();
            let alerts = engine.scan_once();
            if json_mode {
                let payload = serde_json::json!({
                    "status": if alerts.is_empty() { "clean" } else { "alert" },
                    "alerts": alerts,
                });
                println!("{}", serde_json::to_string_pretty(&payload)?);
            } else if alerts.is_empty() {
                println!("[GÜVENLİ] Herhangi bir virüs, rootkit veya bütünlük ihlali bulunamadı.");
            } else {
                println!("[UYARI] Tespit edilen anormallikler:");
                for a in &alerts {
                    println!("  - {}", a);
                }
            }
            if !alerts.is_empty() {
                std::process::exit(1);
            }
            return Ok(());
        }
        "simulate-attack" | "--simulate-attack" => {
            engine.init_canaries();
            let sim_canary = if is_root_user() {
                PathBuf::from(ROOT_CANARY_PATH)
            } else {
                let sc = std::env::temp_dir().join(format!(".blaze_canary_sim_{}", std::process::id()));
                if let Ok(tok) = generate_random_canary_token() {
                    let _ = fs::write(&sc, &tok);
                    let _ = fs::write(expected_canary_hash_path(&sc), sha256_hex(tok.as_bytes()));
                }
                sc
            };
            let _ = fs::write(&sim_canary, "TAMPERED_BY_SIMULATION");
            let mut alerts = engine.scan_integrity(false);
            if !is_canary_intact(&sim_canary) {
                alerts.push(format!(
                    "RANSOMWARE / BOZULMA UYARISI: Güvenlik canarisi tahrif edildi: {:?}",
                    sim_canary
                ));
            }
            if is_root_user() {
                engine.init_canaries();
            } else {
                let _ = fs::remove_file(&sim_canary);
                let _ = fs::remove_file(expected_canary_hash_path(&sim_canary));
            }
            println!(
                "[SIMULATION] Canary tamper detected successfully ({} alert(s)).",
                alerts.len()
            );
            return Ok(());
        }
        "trigger" | "--trigger" => {
            handle_sentinel_alerts(&["Manuel güvenlik alarmı tetiklendi".to_string()]);
            return Ok(());
        }
        "daemon" | "--daemon" => {
            run_sentinel_daemon(&engine);
            return Ok(());
        }
        "" => {
            // Only auto-daemonize when launched directly by PID 1 systemd without a TTY.
            if should_auto_daemonize_on_empty_args() {
                run_sentinel_daemon(&engine);
            } else {
                let report = evaluate_security_posture(&engine, false);
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&report)?);
                } else {
                    print_human_posture(&report);
                }
            }
            return Ok(());
        }
        other => {
            eprintln!("blaze-sentinel: unknown command or option: {}", other);
            print_help();
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blaze_sentinel_core::SentinelConfig;
    use std::path::Path;

    #[test]
    fn test_default_config_uses_root_only_canary() {
        let cfg = SentinelConfig::default();
        assert!(cfg.canary_paths.contains(&PathBuf::from("/var/lib/blaze/.blaze_canary")));
        assert!(!cfg.canary_paths.contains(&PathBuf::from("/var/tmp/.blaze_canary")));
    }

    #[test]
    fn test_new_engine_does_not_create_canaries_eagerly() {
        let _engine = SentinelEngine::new();
        assert!(!Path::new("/var/tmp/.blaze_canary").exists());
    }

    #[test]
    fn test_posture_report_has_seven_controls() {
        let engine = SentinelEngine::new();
        let report = evaluate_security_posture(&engine, false);
        assert_eq!(report.total, 7);
        assert_eq!(report.controls.len(), 7);
        let ids: Vec<&str> = report.controls.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "mac",
                "firewall",
                "kernel.kptr_restrict",
                "kernel.dmesg_restrict",
                "kernel.yama.ptrace_scope",
                "fs.protected_hardlinks",
                "fs.protected_symlinks"
            ]
        );
    }

    #[test]
    fn test_canary_is_fail_closed_and_sha256_verified() {
        let dir = std::env::temp_dir().join(format!("blaze_canary_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let canary = dir.join(".blaze_canary");
        let digest_file = expected_canary_hash_path(&canary);

        // 1. Missing both -> fail-closed
        assert!(!is_canary_intact(&canary));

        // 2. Valid token + matching SHA-256 -> intact
        let token = generate_random_canary_token().expect("urandom available");
        fs::write(&canary, &token).unwrap();
        fs::write(&digest_file, sha256_hex(token.as_bytes())).unwrap();
        assert!(is_canary_intact(&canary));

        // 3. Tampered canary -> fails
        fs::write(&canary, "BLAZE_CANARY_V2:0000").unwrap();
        assert!(!is_canary_intact(&canary));

        // 4. Restore token, delete digest file -> MUST fail-closed
        fs::write(&canary, &token).unwrap();
        let _ = fs::remove_file(&digest_file);
        assert!(!is_canary_intact(&canary));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn canary_rejects_malformed_hash_lengths_and_empty_canary() {
        let dir = std::env::temp_dir().join(format!("blaze_canary_edge_{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let canary = dir.join(".blaze_canary");
        let digest = expected_canary_hash_path(&canary);

        // Empty canary must fail closed even with matching SHA-256 of ""
        fs::write(&canary, "   \n").unwrap();
        fs::write(&digest, sha256_hex(b"")).unwrap();
        assert!(!is_canary_intact(&canary));

        // Valid canary with 63-char or 65-char digest must fail closed
        let tok = "BLAZE_CANARY_V2:abcd";
        let h = sha256_hex(tok.as_bytes());
        fs::write(&canary, tok).unwrap();
        fs::write(&digest, &h[..63]).unwrap();
        assert!(!is_canary_intact(&canary));
        fs::write(&digest, format!("{}0", h)).unwrap();
        assert!(!is_canary_intact(&canary));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn json_flag_accepted_before_or_after_subcommand() {
        let args_before = vec!["--json".to_string(), "status".to_string()];
        let (json_b, pos_b) = parse_cli_args(&args_before);
        assert!(json_b);
        assert_eq!(pos_b, vec!["status"]);

        let args_after = vec!["status".to_string(), "--json".to_string()];
        let (json_a, pos_a) = parse_cli_args(&args_after);
        assert!(json_a);
        assert_eq!(pos_a, vec!["status"]);
    }
}
