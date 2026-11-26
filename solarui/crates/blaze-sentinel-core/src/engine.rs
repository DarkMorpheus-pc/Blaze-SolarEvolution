use anyhow::Result;
use std::fs;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{
    PostureReport, SecurityControl, SentinelConfig, ROOT_CANARY_DIR, ROOT_CANARY_PATH,
};

pub const SENTINEL_VERSION: &str = "0.3.1";
const CANARY_PREFIX: &str = "BLAZE_CANARY_V2:";

pub fn generate_random_canary_token() -> Result<String> {
    let mut buf = [0u8; 32];
    let mut f = fs::File::open("/dev/urandom")?;
    f.read_exact(&mut buf)?;
    let hex: String = buf.iter().map(|b| format!("{:02x}", b)).collect();
    Ok(format!("{}{}", CANARY_PREFIX, hex))
}

pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    for chunk in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[4 * i],
                chunk[4 * i + 1],
                chunk[4 * i + 2],
                chunk[4 * i + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|v| format!("{:08x}", v)).collect()
}

pub fn expected_canary_hash_path(canary: &Path) -> PathBuf {
    let name = canary
        .file_name()
        .map(|n| format!("{}.sha256", n.to_string_lossy()))
        .unwrap_or_else(|| ".blaze_canary.sha256".to_string());
    canary.with_file_name(name)
}

/// Fail-closed canary integrity verification: requires both the canary file and
/// its SHA-256 digest file to exist and match. Missing digest = integrity failure.
pub fn is_canary_intact(canary: &Path) -> bool {
    let Ok(actual) = fs::read_to_string(canary) else {
        return false;
    };
    let Ok(expected_hash) = fs::read_to_string(expected_canary_hash_path(canary)) else {
        return false;
    };
    let actual_trimmed = actual.trim();
    let expected_trimmed = expected_hash.trim();
    !actual_trimmed.is_empty()
        && expected_trimmed.len() == 64
        && sha256_hex(actual_trimmed.as_bytes()) == expected_trimmed
}

pub fn is_root_user() -> bool {
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

pub struct SentinelEngine {
    config: SentinelConfig,
}

impl Default for SentinelEngine {
    fn default() -> Self {
        Self::new()
    }
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
        if root {
            let _ = fs::remove_file("/var/tmp/.blaze_canary");
        }

        for p in &self.config.canary_paths {
            if p == Path::new(ROOT_CANARY_PATH) && !root {
                continue;
            }
            if let Some(parent) = p.parent() {
                if fs::create_dir_all(parent).is_ok() && parent == Path::new(ROOT_CANARY_DIR) {
                    let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
                }
            }
            let hash_path = expected_canary_hash_path(p);
            if !p.exists() {
                if let Ok(token) = generate_random_canary_token() {
                    let digest = sha256_hex(token.as_bytes());
                    if fs::write(p, &token).is_ok() {
                        let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
                        if fs::write(&hash_path, &digest).is_ok() {
                            let _ =
                                fs::set_permissions(&hash_path, fs::Permissions::from_mode(0o400));
                        }
                    }
                }
            } else {
                let _ = fs::set_permissions(p, fs::Permissions::from_mode(0o600));
            }
        }
    }

    /// Scans system integrity (ld.so.preload, active canaries if check_canaries is true,
    /// and deleted /tmp or /dev/shm executables in /proc).
    pub fn scan_integrity(&self, check_canaries: bool) -> Vec<String> {
        let mut alerts = Vec::new();
        let root = is_root_user();

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

        if check_canaries {
            for canary in &self.config.canary_paths {
                if canary == Path::new(ROOT_CANARY_PATH) && !root {
                    continue;
                }
                if !canary.exists() {
                    alerts.push(format!(
                        "RANSOMWARE UYARISI: Güvenlik koruma canarisi silindi: {:?}",
                        canary
                    ));
                } else if !is_canary_intact(canary) {
                    alerts.push(format!(
                        "RANSOMWARE / BOZULMA UYARISI: Güvenlik canarisi tahrif edildi: {:?}",
                        canary
                    ));
                }
            }
        }

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
}

pub fn read_sysctl_int(proc_path: &str) -> Option<i64> {
    fs::read_to_string(proc_path)
        .ok()
        .and_then(|s| s.trim().parse::<i64>().ok())
}

pub fn check_mac_control() -> SecurityControl {
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
        level: if passed {
            "pass".to_string()
        } else {
            "fail".to_string()
        },
        critical: true,
        detail,
    }
}

pub fn check_firewall_control() -> SecurityControl {
    let firewalld_active = Command::new("systemctl")
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
        level: if passed {
            "pass".to_string()
        } else {
            "fail".to_string()
        },
        critical: true,
        detail,
    }
}

pub fn check_sysctl_control(
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
        level: if passed {
            "pass".to_string()
        } else {
            "warn".to_string()
        },
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
