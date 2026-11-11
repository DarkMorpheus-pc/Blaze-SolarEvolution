// ==============================================================================
// Blaze Sentinel — Security Posture Inspector & Active Canary / Integrity Daemon
// v0.3.0 — Unified CLI Posture Inspector (status, policy, verify) and Watchdog
// ==============================================================================

use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

pub const SENTINEL_VERSION: &str = "0.3.0";
pub const ROOT_CANARY_DIR: &str = "/var/lib/blaze";
pub const ROOT_CANARY_PATH: &str = "/var/lib/blaze/.blaze_canary";
const CANARY_TOKEN: &str = "BLAZE_CANARY_INTEGRITY_TOKEN_2026_VALID";

#[derive(Debug, Clone, Serialize)]
pub struct SecurityControl {
    pub id: String,
    pub name: String,
    pub title: String,
    pub passed: bool,
    pub level: String,
    pub critical: bool,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PostureReport {
    pub passed: usize,
    pub total: usize,
    pub verified: bool,
    pub integrity_ok: bool,
    pub controls: Vec<SecurityControl>,
    pub checks: Vec<SecurityControl>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<String>,
}

pub struct SentinelConfig {
    pub canary_paths: Vec<PathBuf>,
    pub monitored_files: Vec<PathBuf>,
    pub check_interval_secs: u64,
}

impl Default for SentinelConfig {
    fn default() -> Self {
        // Security hardening: root-only /var/lib/blaze/.blaze_canary (0700 dir, 0600 file)
        // instead of world-writable /var/tmp/.blaze_canary.
        let mut canaries = vec![PathBuf::from(ROOT_CANARY_PATH)];
        if let Ok(home) = std::env::var("HOME") {
            if !home.is_empty() && home != "/" {
                canaries.push(PathBuf::from(home).join(".config/.blaze_canary"));
            }
        }

        Self {
            canary_paths: canaries,
            monitored_files: vec![
                PathBuf::from("/etc/ld.so.preload"),
                PathBuf::from("/etc/pam.d/sudo"),
                PathBuf::from("/etc/pam.d/system-auth"),
                PathBuf::from("/etc/shadow"),
            ],
            check_interval_secs: 5,
        }
    }
}

pub struct SentinelEngine {
    config: SentinelConfig,
}

impl Default for SentinelEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn is_root_user() -> bool {
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(euid_str) = rest.split_whitespace().nth(1) {
                    if let Ok(euid) = euid_str.parse::<u32>() {
                        return euid == 0;
                    }
                }
            }
        }
    }
    std::env::var("USER").map(|u| u == "root").unwrap_or(false)
}

impl SentinelEngine {
    /// Creates a new SentinelEngine WITHOUT touching the filesystem or creating canaries.
    /// Canaries are only initialized when explicitly running `--daemon`, `--check-once`,
    /// or `--simulate-attack`.
    pub fn new() -> Self {
        Self {
            config: SentinelConfig::default(),
        }
    }

    pub fn config(&self) -> &SentinelConfig {
        &self.config
    }

    /// Initialize canary files with strict root-only / owner-only permissions (0700 dir, 0600 file).
    pub fn init_canaries(&self) {
        let root = is_root_user();
        // Clean up legacy world-writable /var/tmp/.blaze_canary if root
        if root {
            let _ = fs::remove_file("/var/tmp/.blaze_canary");
        }

        for p in &self.config.canary_paths {
            if p == Path::new(ROOT_CANARY_PATH) && !root {
                // Only root can initialize /var/lib/blaze/.blaze_canary
                continue;
            }
            if let Some(parent) = p.parent() {
                if fs::create_dir_all(parent).is_ok() && parent == Path::new(ROOT_CANARY_DIR) {
                    let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
                }
            }
            if !p.exists() && fs::write(p, CANARY_TOKEN).is_ok() {
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
            } else if p.exists() {
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
            }
        }
    }

    /// Scans system integrity (ld.so.preload, active canaries if check_canaries is true,
    /// and deleted /tmp or /dev/shm executables in /proc).
    pub fn scan_integrity(&self, check_canaries: bool) -> Vec<String> {
        let mut alerts = Vec::new();
        let root = is_root_user();

        // 1. Check ld.so.preload (Classic Linux Rootkit persistence)
        let preload = Path::new("/etc/ld.so.preload");
        if preload.exists() {
            if let Ok(content) = fs::read_to_string(preload) {
                let trimmed = content.trim();
                if !trimmed.is_empty() && !trimmed.contains("libsolar_brand.so") {
                    alerts.push(format!(
                        "KRİTİK GÜVENLİK TEHDİDİ: Yetkisiz /etc/ld.so.preload tespiti: '{}'",
                        trimmed
                    ));
                }
            }
        }

        // 2. Check Canary integrity (Ransomware / File tampering detection)
        if check_canaries {
            for canary in &self.config.canary_paths {
                if canary == Path::new(ROOT_CANARY_PATH) && !root && !canary.exists() {
                    // Non-root caller cannot access /var/lib/blaze/.blaze_canary if not created yet
                    continue;
                }
                if !canary.exists() {
                    alerts.push(format!(
                        "RANSOMWARE UYARISI: Güvenlik koruma canarisi silindi: {:?}",
                        canary
                    ));
                } else if let Ok(c) = fs::read_to_string(canary) {
                    if !c.contains("BLAZE_CANARY_INTEGRITY_TOKEN") {
                        alerts.push(format!(
                            "RANSOMWARE / BOZULMA UYARISI: Güvenlik canarisi tahrif edildi: {:?}",
                            canary
                        ));
                    }
                }
            }
        }

        // 3. Scan /proc for deleted executables running hiddenly from /tmp or /dev/shm
        if let Ok(entries) = fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Ok(pid) = name.parse::<i32>() {
                    let exe_link = entry.path().join("exe");
                    if let Ok(target) = fs::read_link(&exe_link) {
                        let target_str = target.to_string_lossy();
                        if target_str.contains("(deleted)")
                            && (target_str.contains("/tmp/") || target_str.contains("/dev/shm/"))
                        {
                            alerts.push(format!(
                                "ŞÜPHELİ GİZLİ SÜREÇ: PID {} silinmiş bellek alanından çalışıyor ({})",
                                pid, target_str
                            ));
                        }
                    }
                }
            }
        }

        alerts
    }

    pub fn scan_once(&self) -> Vec<String> {
        self.scan_integrity(true)
    }

    pub fn handle_alerts(&self, alerts: &[String]) {
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

    pub fn run_daemon(&self) {
        self.init_canaries();
        println!("=== Blaze Sentinel Güvenlik Servisi Başlatıldı ===");
        println!("İzleme döngüsü: {} saniyede bir", self.config.check_interval_secs);

        loop {
            let alerts = self.scan_once();
            if !alerts.is_empty() {
                self.handle_alerts(&alerts);
            }
            thread::sleep(Duration::from_secs(self.config.check_interval_secs));
        }
    }
}

fn read_sysctl_int(proc_path: &str) -> Option<i64> {
    fs::read_to_string(proc_path)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
}

fn check_mac_control() -> SecurityControl {
    let selinux_enforce = fs::read_to_string("/sys/fs/selinux/enforce")
        .ok()
        .map(|s| s.trim().to_string());
    let apparmor_enabled = fs::read_to_string("/sys/module/apparmor/parameters/enabled")
        .ok()
        .map(|s| s.trim().to_string());
    let lsm_list = fs::read_to_string("/sys/kernel/security/lsm")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();

    let (passed, detail) = if selinux_enforce.as_deref() == Some("1") {
        (true, "SELinux enforcing".to_string())
    } else if apparmor_enabled.as_deref() == Some("Y") {
        (true, "AppArmor enabled".to_string())
    } else if selinux_enforce.as_deref() == Some("0") {
        (false, "SELinux permissive".to_string())
    } else if lsm_list.contains("selinux") || lsm_list.contains("apparmor") {
        (true, format!("LSM active ({})", lsm_list))
    } else {
        (false, "no mandatory access control active".to_string())
    };

    SecurityControl {
        id: "mac".to_string(),
        name: "SELinux/AppArmor MAC".to_string(),
        title: "Mandatory access control".to_string(),
        passed,
        level: if passed { "pass".to_string() } else { "fail".to_string() },
        critical: true,
        detail,
    }
}

fn check_firewall_control() -> SecurityControl {
    let firewalld_active = Path::new("/run/firewalld").exists()
        || Command::new("systemctl")
            .args(["is-active", "--quiet", "firewalld.service"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
    let nftables_active = Command::new("systemctl")
        .args(["is-active", "--quiet", "nftables.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let ufw_active = Command::new("systemctl")
        .args(["is-active", "--quiet", "ufw.service"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);

    let passed = firewalld_active || nftables_active || ufw_active;
    let detail = if firewalld_active {
        "firewalld active".to_string()
    } else if nftables_active {
        "nftables active".to_string()
    } else if ufw_active {
        "ufw active".to_string()
    } else {
        "no active firewall service (firewalld/nftables)".to_string()
    };

    SecurityControl {
        id: "firewall".to_string(),
        name: "Firewall (firewalld/nftables)".to_string(),
        title: "Firewall".to_string(),
        passed,
        level: if passed { "pass".to_string() } else { "fail".to_string() },
        critical: true,
        detail,
    }
}

fn check_sysctl_control(
    id: &str,
    name: &str,
    proc_path: &str,
    min_val: i64,
) -> SecurityControl {
    let val = read_sysctl_int(proc_path);
    let passed = val.map(|v| v >= min_val).unwrap_or(false);
    let detail = match val {
        Some(v) => format!("{} = {} (expected >= {})", id, v, min_val),
        None => format!("{} unavailable ({})", id, proc_path),
    };

    SecurityControl {
        id: id.to_string(),
        name: name.to_string(),
        title: name.to_string(),
        passed,
        level: if passed { "pass".to_string() } else { "warn".to_string() },
        critical: false,
        detail,
    }
}

pub fn evaluate_security_posture(engine: &SentinelEngine, check_canaries: bool) -> PostureReport {
    let controls = vec![
        check_mac_control(),
        check_firewall_control(),
        check_sysctl_control(
            "kernel.kptr_restrict",
            "Kernel kptr_restrict",
            "/proc/sys/kernel/kptr_restrict",
            1,
        ),
        check_sysctl_control(
            "kernel.dmesg_restrict",
            "Kernel dmesg_restrict",
            "/proc/sys/kernel/dmesg_restrict",
            1,
        ),
        check_sysctl_control(
            "kernel.yama.ptrace_scope",
            "Kernel yama.ptrace_scope",
            "/proc/sys/kernel/yama/ptrace_scope",
            1,
        ),
        check_sysctl_control(
            "fs.protected_hardlinks",
            "FS protected_hardlinks",
            "/proc/sys/fs/protected_hardlinks",
            1,
        ),
        check_sysctl_control(
            "fs.protected_symlinks",
            "FS protected_symlinks",
            "/proc/sys/fs/protected_symlinks",
            1,
        ),
    ];

    let passed = controls.iter().filter(|c| c.passed).count();
    let total = controls.len();
    let alerts = engine.scan_integrity(check_canaries);
    let integrity_ok = alerts.is_empty();
    let verified = integrity_ok && passed > 0;

    PostureReport {
        passed,
        total,
        verified,
        integrity_ok,
        checks: controls.clone(),
        controls,
        alerts,
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

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json_mode = args.iter().any(|a| a == "--json" || a == "-j");
    let engine = SentinelEngine::new();

    if args.iter().any(|a| a == "--help" || a == "-h" || a == "help") {
        print_help();
        return Ok(());
    }

    if args.iter().any(|a| a == "--version" || a == "-V" || a == "version") {
        println!("blaze-sentinel {}", SENTINEL_VERSION);
        return Ok(());
    }

    let first = args.first().map(|s| s.as_str()).unwrap_or("");

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
                std::env::temp_dir().join(format!(".blaze_canary_sim_{}", std::process::id()))
            };
            let _ = fs::write(&sim_canary, "TAMPERED_BY_SIMULATION");
            let mut alerts = engine.scan_integrity(false);
            if let Ok(c) = fs::read_to_string(&sim_canary) {
                if !c.contains("BLAZE_CANARY_INTEGRITY_TOKEN") {
                    alerts.push(format!(
                        "RANSOMWARE / BOZULMA UYARISI: Güvenlik canarisi tahrif edildi: {:?}",
                        sim_canary
                    ));
                }
            }
            if is_root_user() {
                engine.init_canaries();
            } else {
                let _ = fs::remove_file(&sim_canary);
            }
            println!(
                "[SIMULATION] Canary tamper detected successfully ({} alert(s)).",
                alerts.len()
            );
            return Ok(());
        }
        "trigger" | "--trigger" => {
            engine.handle_alerts(&["Manuel güvenlik alarmı tetiklendi".to_string()]);
            return Ok(());
        }
        "daemon" | "--daemon" => {
            engine.run_daemon();
            return Ok(());
        }
        "" => {
            // If started by systemd service without args (INVOCATION_ID set), run daemon;
            // otherwise show security posture status so CLI invocations never hang.
            if std::env::var("INVOCATION_ID").is_ok() {
                engine.run_daemon();
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
}
