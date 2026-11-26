use blaze_sentinel_core::{
    ActiveOverrideEntry, AkrepFinding, AkrepSeverity, AkrepSnapshot, Availability,
    DriverMokSummary, SystemCompatStatus,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub const DEFAULT_PUBLIC_OVERRIDES_PATH: &str = "/var/cache/akrep/active_overrides.json";

const PROC_BOOT_ID_PATH: &str = "/proc/sys/kernel/random/boot_id";
const NOFILE_PAM_DROPIN_PATH: &str = "/etc/security/limits.d/90-akrep-dev-nofile.conf";
const NOFILE_SYSTEMD_DROPIN_PATH: &str = "/etc/systemd/user.conf.d/90-akrep-nofile.conf";
const EFI_DIR_REL: &str = "firmware/efi";
const EFI_SECURE_BOOT_VAR_REL: &str =
    "firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";
const NVIDIA_SYSFS_MODULE_PATH: &str = "/sys/module/nvidia";
const SYSTEMD_OOMD_UNIT: &str = "systemd-oomd.service";

const SUBPROCESS_CACHE_TTL: Duration = Duration::from_secs(15);
const SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(3);
const DEFAULT_NOFILE_SOFT: u32 = 1024;
const DEFAULT_NOFILE_HARD: u32 = 524_288;

#[derive(Debug, Clone, Copy)]
struct CompatRpmCache {
    android_udev_installed: bool,
    ncurses_compat_installed: bool,
}

#[derive(Debug, Clone, Copy)]
struct MokSubprocessCache {
    akmod_nvidia_installed: bool,
    mok_enrolled_count: Option<usize>,
    mok_sb_state: Option<bool>,
}

static COMPAT_RPM_CACHE: OnceLock<Mutex<Option<(Instant, CompatRpmCache)>>> = OnceLock::new();
static MOK_SUBPROCESS_CACHE: OnceLock<Mutex<Option<(Instant, MokSubprocessCache)>>> =
    OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WireOverrideDuration {
    Timed { minutes: u32 },
    UntilReboot,
    UntilChanged,
}

#[derive(Debug, Deserialize)]
struct WireOverrideEntry {
    subsystem: String,
    relaxed: bool,
    duration: WireOverrideDuration,
    started_at: String,
    expires_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WireOverrideDocument {
    #[serde(default)]
    schema_version: Option<u32>,
    boot_id: String,
    #[serde(default)]
    entries: BTreeMap<String, WireOverrideEntry>,
}

fn resolve_binary(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
}

fn run_command_with_timeout(mut cmd: Command, timeout: Duration) -> Option<Output> {
    cmd.env("LC_ALL", "C")
        .env("LANG", "C")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().ok()?;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut pipe) = stdout_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(ref mut pipe) = stderr_pipe {
            let _ = pipe.read_to_end(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };

    let stdout = stdout_handle.join().unwrap_or_default();
    let stderr = stderr_handle.join().unwrap_or_default();
    Some(Output {
        status,
        stdout,
        stderr,
    })
}

fn cached_with_ttl<T: Copy>(
    slot: &OnceLock<Mutex<Option<(Instant, T)>>>,
    ttl: Duration,
    probe: impl FnOnce() -> T,
) -> T {
    let lock = slot.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = lock.lock() {
        if let Some((captured_at, val)) = *guard {
            if captured_at.elapsed() < ttl {
                return val;
            }
        }
    }

    let fresh = probe();
    if let Ok(mut guard) = lock.lock() {
        *guard = Some((Instant::now(), fresh));
    }
    fresh
}

fn query_rpm_installed(rpm_bin: &Path, pkg: &str) -> bool {
    let mut cmd = Command::new(rpm_bin);
    cmd.args(["-q", "--", pkg]);
    run_command_with_timeout(cmd, SUBPROCESS_TIMEOUT)
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Reads the kernel's ephemeral boot UUID used to discard stale `Timed` / `UntilReboot` overrides.
pub fn read_current_boot_id() -> String {
    fs::read_to_string(PROC_BOOT_ID_PATH)
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Parses `/var/cache/akrep/active_overrides.json` and filters out `!relaxed`, mismatched-boot
/// `Timed`/`UntilReboot`, and expired `Timed` entries while preserving `UntilChanged` entries.
pub fn collect_active_overrides(
    cache_path: &Path,
    current_boot_id: &str,
    now: DateTime<Utc>,
) -> Vec<ActiveOverrideEntry> {
    let Ok(raw) = fs::read_to_string(cache_path) else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_str::<WireOverrideDocument>(&raw) else {
        return Vec::new();
    };
    if doc.schema_version.unwrap_or(1) != 1 {
        return Vec::new();
    }

    let doc_boot = doc.boot_id.trim();
    let cur_boot = current_boot_id.trim();
    let boot_matches = !cur_boot.is_empty() && doc_boot == cur_boot;

    let mut out: Vec<ActiveOverrideEntry> = Vec::new();
    for (map_key, entry) in doc.entries {
        if !entry.relaxed {
            continue;
        }
        let subsystem = if entry.subsystem.trim().is_empty() {
            map_key
        } else {
            entry.subsystem.trim().to_string()
        };
        if subsystem.is_empty() || out.iter().any(|existing| existing.subsystem == subsystem) {
            continue;
        }

        match entry.duration {
            WireOverrideDuration::Timed { minutes } => {
                if !boot_matches {
                    continue;
                }
                let Some(exp_str) = entry.expires_at.as_deref() else {
                    continue;
                };
                let Ok(exp_dt) = DateTime::parse_from_rfc3339(exp_str) else {
                    continue;
                };
                let exp_utc = exp_dt.with_timezone(&Utc);
                if exp_utc <= now {
                    continue;
                }
                let remaining_secs = exp_utc.signed_duration_since(now).num_seconds().max(1);
                out.push(ActiveOverrideEntry {
                    subsystem,
                    duration_kind: "timed".to_string(),
                    minutes: Some(minutes),
                    started_at: entry.started_at,
                    expires_at: entry.expires_at,
                    remaining_secs: Some(remaining_secs),
                    boot_id: doc_boot.to_string(),
                });
            }
            WireOverrideDuration::UntilReboot => {
                if !boot_matches {
                    continue;
                }
                out.push(ActiveOverrideEntry {
                    subsystem,
                    duration_kind: "until_reboot".to_string(),
                    minutes: None,
                    started_at: entry.started_at,
                    expires_at: None,
                    remaining_secs: None,
                    boot_id: doc_boot.to_string(),
                });
            }
            WireOverrideDuration::UntilChanged => {
                out.push(ActiveOverrideEntry {
                    subsystem,
                    duration_kind: "until_changed".to_string(),
                    minutes: None,
                    started_at: entry.started_at,
                    expires_at: None,
                    remaining_secs: None,
                    boot_id: doc_boot.to_string(),
                });
            }
        }
    }

    out
}

pub fn parse_nofile_dropin_pair(
    pam_raw: Option<&str>,
    systemd_raw: Option<&str>,
) -> Option<(u32, u32)> {
    let mut soft: Option<u32> = None;
    let mut hard: Option<u32> = None;

    if let Some(raw) = pam_raw {
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = trimmed.split_whitespace().collect();
            if parts.len() == 4 && parts[0] == "*" && parts[2] == "nofile" {
                if parts[1] == "soft" {
                    soft = parts[3].parse::<u32>().ok();
                } else if parts[1] == "hard" {
                    hard = parts[3].parse::<u32>().ok();
                }
            }
        }
    }

    if let (Some(s), Some(h)) = (soft, hard) {
        return Some((s, h));
    }

    if let Some(raw) = systemd_raw {
        for line in raw.lines() {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("DefaultLimitNOFILE=") {
                if let Some((s_str, h_str)) = val.trim().split_once(':') {
                    if let (Ok(s), Ok(h)) =
                        (s_str.trim().parse::<u32>(), h_str.trim().parse::<u32>())
                    {
                        return Some((s, h));
                    }
                }
            }
        }
    }

    None
}

fn collect_nofile_status_at(pam_path: &Path, systemd_path: &Path) -> (bool, u32, u32) {
    let pam_raw = fs::read_to_string(pam_path).ok();
    let systemd_raw = fs::read_to_string(systemd_path).ok();
    match parse_nofile_dropin_pair(pam_raw.as_deref(), systemd_raw.as_deref()) {
        Some((soft, hard)) => (true, soft, hard),
        None => (false, DEFAULT_NOFILE_SOFT, DEFAULT_NOFILE_HARD),
    }
}

fn query_systemd_unit_state(systemctl_bin: &Path, unit: &str) -> (bool, bool) {
    let mut active_cmd = Command::new(systemctl_bin);
    active_cmd.args(["is-active", "--quiet", unit]);
    let is_active = run_command_with_timeout(active_cmd, SUBPROCESS_TIMEOUT)
        .map(|o| o.status.success())
        .unwrap_or(false);

    let mut enabled_cmd = Command::new(systemctl_bin);
    enabled_cmd.args(["is-enabled", unit]);
    let is_enabled = run_command_with_timeout(enabled_cmd, SUBPROCESS_TIMEOUT)
        .map(|o| {
            let state = String::from_utf8_lossy(&o.stdout).trim().to_string();
            o.status.success()
                || matches!(
                    state.as_str(),
                    "enabled" | "enabled-runtime" | "static" | "indirect"
                )
        })
        .unwrap_or(false);

    (is_active, is_enabled)
}

/// Reads live `systemd-oomd` and `nofile` drop-in state alongside 15s TTL-cached optional RPM queries.
pub fn collect_system_compat_cached() -> SystemCompatStatus {
    let (oomd_active, oomd_enabled) = resolve_binary(&["/usr/bin/systemctl", "/bin/systemctl"])
        .map(|bin| query_systemd_unit_state(&bin, SYSTEMD_OOMD_UNIT))
        .unwrap_or((false, false));

    let (nofile_enabled, nofile_soft, nofile_hard) = collect_nofile_status_at(
        Path::new(NOFILE_PAM_DROPIN_PATH),
        Path::new(NOFILE_SYSTEMD_DROPIN_PATH),
    );

    let rpm_cache = cached_with_ttl(&COMPAT_RPM_CACHE, SUBPROCESS_CACHE_TTL, || {
        let Some(rpm_bin) = resolve_binary(&["/usr/bin/rpm", "/bin/rpm"]) else {
            return CompatRpmCache {
                android_udev_installed: false,
                ncurses_compat_installed: false,
            };
        };
        CompatRpmCache {
            android_udev_installed: query_rpm_installed(&rpm_bin, "android-udev-rules"),
            ncurses_compat_installed: query_rpm_installed(&rpm_bin, "ncurses-compat-libs"),
        }
    });

    SystemCompatStatus {
        oomd_active,
        oomd_enabled,
        nofile_enabled,
        nofile_soft,
        nofile_hard,
        android_udev_installed: rpm_cache.android_udev_installed,
        ncurses_compat_installed: rpm_cache.ncurses_compat_installed,
    }
}

pub fn read_secure_boot_efivar_at(sys_root: &Path) -> Option<bool> {
    if !sys_root.join(EFI_DIR_REL).exists() {
        return None;
    }
    fs::read(sys_root.join(EFI_SECURE_BOOT_VAR_REL))
        .ok()
        .and_then(|bytes| {
            bytes.last().and_then(|b| match *b {
                1 => Some(true),
                0 => Some(false),
                _ => None,
            })
        })
}

pub fn parse_mokutil_sb_state(raw: &str) -> Option<bool> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("secureboot enabled") {
        Some(true)
    } else if lower.contains("secureboot disabled") {
        Some(false)
    } else {
        None
    }
}

pub fn parse_mokutil_enrolled_count(raw: &str) -> Option<usize> {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("is empty") {
        return Some(0);
    }
    let key_headers = raw
        .lines()
        .filter(|line| line.trim_start().to_ascii_lowercase().starts_with("[key "))
        .count();
    if key_headers > 0 {
        return Some(key_headers);
    }
    let sha1_lines = raw
        .lines()
        .filter(|line| {
            line.trim_start()
                .to_ascii_lowercase()
                .starts_with("sha1 fingerprint:")
        })
        .count();
    if sha1_lines > 0 {
        Some(sha1_lines)
    } else {
        None
    }
}

pub fn build_driver_mok_summary(
    secure_boot_enabled: Option<bool>,
    mok_enrolled_count: Option<usize>,
    nvidia_module_loaded: bool,
    akmod_nvidia_installed: bool,
) -> DriverMokSummary {
    let unsigned_risk = secure_boot_enabled == Some(true)
        && akmod_nvidia_installed
        && (!nvidia_module_loaded || mok_enrolled_count == Some(0));

    let (guidance_tr, guidance_en) = if unsigned_risk {
        (
            "Secure Boot etkin ancak akmod-nvidia modülü imzasız veya MOK kaydı eksik görünüyor; 'mokutil --import /etc/pki/akmods/certs/public_key.der' ile anahtarı içe aktarıp yeniden başlatın".to_string(),
            "Secure Boot is enabled while akmod-nvidia appears unsigned or missing MOK enrollment; run 'mokutil --import /etc/pki/akmods/certs/public_key.der' and reboot to enroll the key".to_string(),
        )
    } else if secure_boot_enabled == Some(true) {
        let mok_note_tr = mok_enrolled_count
            .map(|c| format!(" ({} kayıtlı MOK)", c))
            .unwrap_or_default();
        let mok_note_en = mok_enrolled_count
            .map(|c| format!(" ({} enrolled MOK key(s))", c))
            .unwrap_or_default();
        let nv_tr = if nvidia_module_loaded {
            "NVIDIA modülü yüklü"
        } else if akmod_nvidia_installed {
            "akmod-nvidia kurulu"
        } else {
            "üçüncü parti NVIDIA modülü yüklü değil"
        };
        let nv_en = if nvidia_module_loaded {
            "NVIDIA module loaded"
        } else if akmod_nvidia_installed {
            "akmod-nvidia installed"
        } else {
            "no third-party NVIDIA module loaded"
        };
        (
            format!("Secure Boot etkin{}; {}", mok_note_tr, nv_tr),
            format!("Secure Boot enabled{}; {}", mok_note_en, nv_en),
        )
    } else if secure_boot_enabled == Some(false) {
        (
            "Secure Boot kapalı; çekirdek modülü MOK imza zorlaması devre dışı".to_string(),
            "Secure Boot is disabled; kernel module MOK enforcement is inactive".to_string(),
        )
    } else {
        (
            "UEFI Secure Boot veya MOK bilgisi kullanılamıyor".to_string(),
            "UEFI Secure Boot or MOK state is unavailable".to_string(),
        )
    };

    DriverMokSummary {
        secure_boot_enabled,
        mok_enrolled_count,
        nvidia_module_loaded,
        akmod_nvidia_installed,
        unsigned_risk,
        guidance_tr,
        guidance_en,
    }
}

/// Collects read-only Secure Boot, MOK enrollment, and NVIDIA module state with 15s TTL caching.
pub fn collect_driver_mok_cached() -> DriverMokSummary {
    let sys_root = Path::new("/sys");
    let efivar_sb = read_secure_boot_efivar_at(sys_root);
    let uefi_present = sys_root.join(EFI_DIR_REL).exists();
    let nvidia_module_loaded = Path::new(NVIDIA_SYSFS_MODULE_PATH).exists();

    let mok_cache = cached_with_ttl(&MOK_SUBPROCESS_CACHE, SUBPROCESS_CACHE_TTL, || {
        let akmod_nvidia_installed = resolve_binary(&["/usr/bin/rpm", "/bin/rpm"])
            .map(|rpm_bin| query_rpm_installed(&rpm_bin, "akmod-nvidia"))
            .unwrap_or(false);

        let (mok_enrolled_count, mok_sb_state) = if uefi_present {
            if let Some(mokutil_bin) = resolve_binary(&["/usr/bin/mokutil", "/bin/mokutil"]) {
                let enrolled = run_command_with_timeout(
                    {
                        let mut cmd = Command::new(&mokutil_bin);
                        cmd.arg("--list-enrolled");
                        cmd
                    },
                    SUBPROCESS_TIMEOUT,
                )
                .and_then(|out| {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    if stderr.to_ascii_lowercase().contains("is empty")
                        || stdout.to_ascii_lowercase().contains("is empty")
                    {
                        return Some(0);
                    }
                    if let Some(count) = parse_mokutil_enrolled_count(&stdout) {
                        return Some(count);
                    }
                    if out.status.success() && stdout.trim().is_empty() {
                        Some(0)
                    } else {
                        None
                    }
                });

                let sb_state = if efivar_sb.is_none() {
                    run_command_with_timeout(
                        {
                            let mut cmd = Command::new(&mokutil_bin);
                            cmd.arg("--sb-state");
                            cmd
                        },
                        SUBPROCESS_TIMEOUT,
                    )
                    .and_then(|out| parse_mokutil_sb_state(&String::from_utf8_lossy(&out.stdout)))
                } else {
                    None
                };
                (enrolled, sb_state)
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        MokSubprocessCache {
            akmod_nvidia_installed,
            mok_enrolled_count,
            mok_sb_state,
        }
    });

    let secure_boot_enabled = efivar_sb.or(mok_cache.mok_sb_state);
    build_driver_mok_summary(
        secure_boot_enabled,
        mok_cache.mok_enrolled_count,
        nvidia_module_loaded,
        mok_cache.akmod_nvidia_installed,
    )
}

fn is_override_live_relaxed(snapshot: &AkrepSnapshot, entry: &ActiveOverrideEntry) -> bool {
    match entry.subsystem.as_str() {
        "selinux" => {
            let card = snapshot.cards.iter().find(|c| c.id == "system_protection");
            match card {
                Some(c)
                    if c.metric.eq_ignore_ascii_case("Enforcing")
                        || c.metric.eq_ignore_ascii_case("Disabled") =>
                {
                    false
                }
                Some(c) if c.status == AkrepSeverity::Protected => snapshot
                    .findings
                    .iter()
                    .any(|f| f.id == "selinux.mode" || f.id == "sentinel.mac"),
                _ => true,
            }
        }
        "firewalld" => {
            let card = snapshot.cards.iter().find(|c| c.id == "firewall");
            match card {
                Some(c)
                    if c.status == AkrepSeverity::Protected
                        && !c.metric.eq_ignore_ascii_case("OFF") =>
                {
                    snapshot
                        .findings
                        .iter()
                        .any(|f| f.id == "firewall.inactive" || f.id == "sentinel.firewall")
                }
                _ => true,
            }
        }
        "oomd" => match &snapshot.system_compat {
            Some(compat) => !compat.oomd_active,
            None => true,
        },
        _ => false,
    }
}

fn format_remaining_labels(entry: &ActiveOverrideEntry) -> (String, String) {
    let rem_secs = entry
        .remaining_secs
        .unwrap_or_else(|| i64::from(entry.minutes.unwrap_or(1)) * 60)
        .max(1);
    if rem_secs >= 60 {
        let mins = (rem_secs + 59) / 60;
        (
            format!("{} dk kaldı", mins),
            format!("{} min remaining", mins),
        )
    } else {
        (
            format!("{} sn kaldı", rem_secs),
            format!("{}s remaining", rem_secs),
        )
    }
}

struct ReconcileCopy {
    section_id: &'static str,
    preferred_finding_id: &'static str,
    duplicate_finding_id: &'static str,
    target_severity: AkrepSeverity,
    card_summary_tr: String,
    card_summary_en: String,
    title_tr: String,
    title_en: String,
    description_tr: String,
    description_en: String,
    remediation_tr: String,
    remediation_en: String,
}

fn build_reconcile_copy(entry: &ActiveOverrideEntry) -> Option<ReconcileCopy> {
    match entry.subsystem.as_str() {
        "selinux" => {
            let (card_summary_tr, card_summary_en, title_tr, title_en, description_tr, description_en) =
                match entry.duration_kind.as_str() {
                    "timed" => {
                        let (rem_tr, rem_en) = format_remaining_labels(entry);
                        (
                            format!("Permissive · {} (otomatik dönüş)", rem_tr),
                            format!("Permissive · {} (auto-revert)", rem_en),
                            "SELinux geçici olarak Permissive modda".to_string(),
                            "SELinux temporarily in Permissive mode".to_string(),
                            format!(
                                "Akrep üzerinden SELinux geçici olarak Permissive moduna alındı ({}; dosya etiketleri korunuyor, süre sonunda Enforcing moduna dönülür)",
                                rem_tr
                            ),
                            format!(
                                "SELinux was temporarily set to Permissive via Akrep ({}; file labels remain intact and Enforcing restores automatically)",
                                rem_en
                            ),
                        )
                    }
                    "until_reboot" => (
                        "Permissive · Yeniden başlatmaya kadar".to_string(),
                        "Permissive · Until reboot".to_string(),
                        "SELinux yeniden başlatmaya kadar Permissive modda".to_string(),
                        "SELinux in Permissive mode until reboot".to_string(),
                        "Akrep üzerinden SELinux bu oturumda Permissive moduna alındı; dosya etiketleri ve kalıcı Enforcing ayarı korunuyor".to_string(),
                        "SELinux is running in Permissive mode until the next reboot; file labels and persistent Enforcing configuration remain intact".to_string(),
                    ),
                    _ => (
                        "Permissive · Elle açılana kadar".to_string(),
                        "Permissive · Until manually enabled".to_string(),
                        "SELinux elle açılana kadar Permissive modda".to_string(),
                        "SELinux in Permissive mode until manually enabled".to_string(),
                        "Akrep üzerinden SELinux siz tekrar Enforcing seçilene kadar Permissive moduna alındı; dosya etiketleri korunuyor ve denetleme sürüyor".to_string(),
                        "SELinux was set to Permissive via Akrep until manually re-enabled; file labels remain intact and auditing continues".to_string(),
                    ),
                };
            Some(ReconcileCopy {
                section_id: "system_protection",
                preferred_finding_id: "selinux.mode",
                duplicate_finding_id: "sentinel.mac",
                target_severity: AkrepSeverity::Attention,
                card_summary_tr,
                card_summary_en,
                title_tr,
                title_en,
                description_tr,
                description_en,
                remediation_tr: "Sistem Koruması sayfasından veya Genel Bakış kartından Enforcing moduna dönebilirsiniz".to_string(),
                remediation_en: "Restore Enforcing mode anytime from System Protection or the Overview active overrides card".to_string(),
            })
        }
        "firewalld" => {
            let target_severity = if entry.duration_kind == "until_changed" {
                AkrepSeverity::Critical
            } else {
                AkrepSeverity::Attention
            };
            let (card_summary_tr, card_summary_en, title_tr, title_en, description_tr, description_en) =
                match entry.duration_kind.as_str() {
                    "timed" => {
                        let (rem_tr, rem_en) = format_remaining_labels(entry);
                        (
                            format!("Duraklatıldı · {} (otomatik dönüş)", rem_tr),
                            format!("Paused · {} (auto-revert)", rem_en),
                            "Güvenlik duvarı geçici olarak duraklatıldı".to_string(),
                            "Firewall temporarily paused".to_string(),
                            format!(
                                "Akrep üzerinden firewalld geçici olarak duraklatıldı ({}; süre dolduğunda otomatik olarak yeniden etkinleştirilir)",
                                rem_tr
                            ),
                            format!(
                                "firewalld was temporarily paused via Akrep ({}; protection will re-enable automatically when the timer expires)",
                                rem_en
                            ),
                        )
                    }
                    "until_reboot" => (
                        "Duraklatıldı · Yeniden başlatmaya kadar".to_string(),
                        "Paused · Until reboot".to_string(),
                        "Güvenlik duvarı yeniden başlatmaya kadar duraklatıldı".to_string(),
                        "Firewall paused until reboot".to_string(),
                        "Akrep üzerinden firewalld yeniden başlatmaya kadar (bu oturum için) duraklatıldı; bir sonraki açılışta otomatik olarak etkinleştirilir".to_string(),
                        "firewalld is paused for this session and will automatically re-enable on the next reboot".to_string(),
                    ),
                    _ => (
                        "Kapalı · Elle açılana kadar (gelen koruma devre dışı)".to_string(),
                        "Disabled · Until manually enabled (incoming protection off)".to_string(),
                        "Güvenlik duvarı elle açılana kadar kapalı".to_string(),
                        "Firewall disabled until manually enabled".to_string(),
                        "Akrep üzerinden firewalld siz tekrar etkinleştirene kadar kapatıldı; gelen ağ bağlantısı koruması şu anda devre dışı".to_string(),
                        "firewalld was disabled via Akrep until manually re-enabled; incoming network connection protection is currently off".to_string(),
                    ),
                };
            Some(ReconcileCopy {
                section_id: "firewall",
                preferred_finding_id: "firewall.inactive",
                duplicate_finding_id: "sentinel.firewall",
                target_severity,
                card_summary_tr,
                card_summary_en,
                title_tr,
                title_en,
                description_tr,
                description_en,
                remediation_tr: "Güvenlik Duvarı sayfasından veya Genel Bakış kartından firewalld servisini yeniden etkinleştirebilirsiniz".to_string(),
                remediation_en: "Re-enable firewalld anytime from the Firewall page or the Overview active overrides card".to_string(),
            })
        }
        _ => None,
    }
}

fn apply_copy_to_finding(f: &mut AkrepFinding, copy: &ReconcileCopy) {
    f.severity = copy.target_severity;
    f.availability = Availability::Available;
    f.title_tr = copy.title_tr.clone();
    f.title_en = copy.title_en.clone();
    f.description_tr = copy.description_tr.clone();
    f.description_en = copy.description_en.clone();
    f.remediation_hint_tr = copy.remediation_tr.clone();
    f.remediation_hint_en = copy.remediation_en.clone();
}

fn reconcile_finding_list(findings: &mut Vec<AkrepFinding>, copy: &ReconcileCopy) {
    let has_preferred = findings.iter().any(|f| f.id == copy.preferred_finding_id);
    let keep_id = if has_preferred {
        copy.preferred_finding_id
    } else {
        copy.duplicate_finding_id
    };

    let mut kept_one = false;
    findings.retain_mut(|f| {
        if f.id == copy.preferred_finding_id || f.id == copy.duplicate_finding_id {
            if f.id == keep_id && !kept_one {
                kept_one = true;
                apply_copy_to_finding(f, copy);
                true
            } else {
                false
            }
        } else {
            true
        }
    });

    if !kept_one {
        findings.push(AkrepFinding {
            id: copy.preferred_finding_id.to_string(),
            section: copy.section_id.to_string(),
            severity: copy.target_severity,
            availability: Availability::Available,
            title_tr: copy.title_tr.clone(),
            title_en: copy.title_en.clone(),
            description_tr: copy.description_tr.clone(),
            description_en: copy.description_en.clone(),
            remediation_hint_tr: copy.remediation_tr.clone(),
            remediation_hint_en: copy.remediation_en.clone(),
            source: "collectors/compat".to_string(),
            read_only_note:
                "akrepd salt okunurdur; geçici ve kalıcı değişiklikler akrep-privd ile uygulanır."
                    .to_string(),
        });
    }
}

/// Returns localized sentence-case headline strings matching overall status and override presence.
pub fn headline_for_status(status: AkrepSeverity, has_active_overrides: bool) -> (String, String) {
    let (tr, en) = match (status, has_active_overrides) {
        (AkrepSeverity::Protected, _) => (
            "Sistem güvenlik duruşu korunuyor",
            "System security posture protected",
        ),
        (AkrepSeverity::Attention, true) => (
            "Etkin koruma gevşetmesi sürüyor; bazı kontroller dikkat gerektiriyor",
            "Active protection override in effect; some controls require attention",
        ),
        (AkrepSeverity::Attention, false) => (
            "Bazı güvenlik kontrolleri inceleme gerektiriyor",
            "Some security controls require attention",
        ),
        (AkrepSeverity::Unavailable, _) => (
            "Temel korumalar etkin, bazı isteğe bağlı bileşenler kullanılamıyor",
            "Core protections active, some optional components unavailable",
        ),
        (AkrepSeverity::Unknown, _) => (
            "Bazı güvenlik telemetri alanları doğrulanamadı",
            "Some security telemetry domains are unknown",
        ),
        (AkrepSeverity::Critical, true) => (
            "Kritik güvenlik uyarısı: kalıcı koruma gevşetmesi veya açık tespit edildi",
            "Critical security alert: persistent protection relaxation or exposure detected",
        ),
        (AkrepSeverity::Critical, false) => (
            "Kritik güvenlik uyarısı tespit edildi",
            "Critical security alert detected",
        ),
    };
    (tr.to_string(), en.to_string())
}

/// Cross-checks overrides against live telemetry, deduplicates/annotates findings, and recomputes overall status.
pub fn reconcile_snapshot_with_overrides(
    snapshot: &mut AkrepSnapshot,
    overrides: &[ActiveOverrideEntry],
) {
    let live_overrides: Vec<ActiveOverrideEntry> = overrides
        .iter()
        .filter(|e| is_override_live_relaxed(snapshot, e))
        .cloned()
        .collect();

    for entry in &live_overrides {
        let Some(copy) = build_reconcile_copy(entry) else {
            continue;
        };

        reconcile_finding_list(&mut snapshot.findings, &copy);

        let mut section_status = copy.target_severity;
        if let Some(section) = snapshot
            .sections
            .iter_mut()
            .find(|s| s.section == copy.section_id)
        {
            reconcile_finding_list(&mut section.findings, &copy);
            section_status = section
                .findings
                .iter()
                .fold(copy.target_severity, |acc, f| acc.combine(f.severity));
            section.status = section_status;
            section.availability = Availability::Available;
            section.summary_tr = copy.card_summary_tr.clone();
            section.summary_en = copy.card_summary_en.clone();
        }

        if let Some(card) = snapshot.cards.iter_mut().find(|c| c.id == copy.section_id) {
            card.status = section_status;
            card.availability = Availability::Available;
            card.summary_tr = copy.card_summary_tr;
            card.summary_en = copy.card_summary_en;
        }
    }

    snapshot.active_overrides = live_overrides;
    snapshot.overall_status = snapshot
        .cards
        .iter()
        .map(|c| c.status)
        .chain(snapshot.findings.iter().map(|f| f.severity))
        .fold(AkrepSeverity::Protected, |acc, s| acc.combine(s));

    let (headline_tr, headline_en) = headline_for_status(
        snapshot.overall_status,
        !snapshot.active_overrides.is_empty(),
    );
    snapshot.headline_tr = headline_tr;
    snapshot.headline_en = headline_en;
}

#[cfg(test)]
mod tests {
    use super::*;
    use blaze_sentinel_core::{
        AkrepCapabilities, AkrepCardSummary, AkrepKeyValue, AkrepSectionDetail, PostureReport,
        SecurityControl,
    };
    use chrono::Duration as ChronoDuration;

    fn temp_cache_file(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "akrep_compat_test_{}_{}_{}",
            tag,
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("active_overrides.json");
        (dir, file)
    }

    fn make_base_snapshot(
        selinux_status: AkrepSeverity,
        selinux_metric: &str,
        fw_status: AkrepSeverity,
        fw_metric: &str,
        extra_findings: Vec<AkrepFinding>,
    ) -> AkrepSnapshot {
        let mut selinux_findings = Vec::new();
        if selinux_status != AkrepSeverity::Protected {
            selinux_findings.push(AkrepFinding {
                id: "selinux.mode".to_string(),
                section: "system_protection".to_string(),
                severity: selinux_status,
                availability: Availability::Available,
                title_tr: "SELinux Durumu".to_string(),
                title_en: "SELinux Mode".to_string(),
                description_tr: "Permissive".to_string(),
                description_en: "Permissive".to_string(),
                remediation_hint_tr: "Enforcing yapın".to_string(),
                remediation_hint_en: "Restore Enforcing".to_string(),
                source: "collectors/selinux".to_string(),
                read_only_note: "ro".to_string(),
            });
        }

        let mut fw_findings = Vec::new();
        if fw_status != AkrepSeverity::Protected {
            fw_findings.push(AkrepFinding {
                id: "firewall.inactive".to_string(),
                section: "firewall".to_string(),
                severity: fw_status,
                availability: Availability::Available,
                title_tr: "Güvenlik Duvarı Etkin Değil".to_string(),
                title_en: "Firewall Not Active".to_string(),
                description_tr: "firewalld aktif değil".to_string(),
                description_en: "firewalld inactive".to_string(),
                remediation_hint_tr: "firewalld başlatın".to_string(),
                remediation_hint_en: "Start firewalld".to_string(),
                source: "collectors/firewalld".to_string(),
                read_only_note: "ro".to_string(),
            });
        }

        let mut all_findings = extra_findings;
        all_findings.extend(selinux_findings.clone());
        all_findings.extend(fw_findings.clone());

        let selinux_card = AkrepCardSummary {
            id: "system_protection".to_string(),
            title_tr: "Sistem Koruması (SELinux)".to_string(),
            title_en: "System Protection (SELinux)".to_string(),
            status: selinux_status,
            availability: Availability::Available,
            summary_tr: "SELinux".to_string(),
            summary_en: "SELinux".to_string(),
            metric: selinux_metric.to_string(),
        };
        let fw_card = AkrepCardSummary {
            id: "firewall".to_string(),
            title_tr: "Güvenlik Duvarı (Firewalld)".to_string(),
            title_en: "Firewall & Network (Firewalld)".to_string(),
            status: fw_status,
            availability: Availability::Available,
            summary_tr: "Firewalld".to_string(),
            summary_en: "Firewalld".to_string(),
            metric: fw_metric.to_string(),
        };

        let selinux_detail = AkrepSectionDetail {
            section: "system_protection".to_string(),
            title_tr: selinux_card.title_tr.clone(),
            title_en: selinux_card.title_en.clone(),
            status: selinux_status,
            availability: Availability::Available,
            summary_tr: selinux_card.summary_tr.clone(),
            summary_en: selinux_card.summary_en.clone(),
            metrics: vec![AkrepKeyValue {
                label_tr: "Politika Anahtarları (Booleans)".to_string(),
                label_en: "Policy Booleans".to_string(),
                value: "10 etkin / 20 toplam".to_string(),
            }],
            findings: selinux_findings,
        };
        let fw_detail = AkrepSectionDetail {
            section: "firewall".to_string(),
            title_tr: fw_card.title_tr.clone(),
            title_en: fw_card.title_en.clone(),
            status: fw_status,
            availability: Availability::Available,
            summary_tr: fw_card.summary_tr.clone(),
            summary_en: fw_card.summary_en.clone(),
            metrics: vec![AkrepKeyValue {
                label_tr: "Zengin Kurallar (Rich Rules)".to_string(),
                label_en: "Rich Rules".to_string(),
                value: "0 kural".to_string(),
            }],
            findings: fw_findings,
        };

        let overall_status = selinux_status.combine(fw_status);
        AkrepSnapshot {
            schema_version: 2,
            version: "1.0.0".to_string(),
            sentinel_version: "0.3.1".to_string(),
            read_only: true,
            read_only_daemon: true,
            mutators_enabled: true,
            active_profile_id: "balanced".to_string(),
            active_profile_tr: "Dengeli".to_string(),
            active_profile_en: "Balanced".to_string(),
            generated_at: Utc::now().to_rfc3339(),
            overall_status,
            headline_tr: "Test".to_string(),
            headline_en: "Test".to_string(),
            posture: PostureReport {
                passed: 5,
                total: 7,
                verified: true,
                integrity_ok: true,
                controls: vec![
                    SecurityControl {
                        id: "mac".to_string(),
                        name: "SELinux".to_string(),
                        title: "MAC".to_string(),
                        passed: selinux_status == AkrepSeverity::Protected,
                        level: "fail".to_string(),
                        critical: true,
                        detail: "SELinux permissive".to_string(),
                    },
                    SecurityControl {
                        id: "firewall".to_string(),
                        name: "Firewall".to_string(),
                        title: "Firewall".to_string(),
                        passed: fw_status == AkrepSeverity::Protected,
                        level: "fail".to_string(),
                        critical: true,
                        detail: "firewalld inactive".to_string(),
                    },
                ],
                checks: Vec::new(),
                alerts: Vec::new(),
            },
            cards: vec![selinux_card, fw_card],
            findings: all_findings,
            sections: vec![selinux_detail, fw_detail],
            capabilities: AkrepCapabilities {
                version: "1.0.0".to_string(),
                sentinel_version: "0.3.1".to_string(),
                schema_version: 2,
                read_only: true,
                read_only_daemon: true,
                mutators_enabled: true,
                privileged_bus: "tr.com.blazeos.Akrep1.Privileged".to_string(),
                sections: vec!["system_protection".to_string(), "firewall".to_string()],
                future_polkit_actions: Vec::new(),
            },
            active_overrides: Vec::new(),
            system_compat: None,
            driver_mok: None,
        }
    }

    #[test]
    fn test_collect_active_overrides_drops_stale_boot_id_and_expired_timed() {
        let (dir, cache_path) = temp_cache_file("stale_boot_and_expired");
        let now = Utc::now();
        let past_expired = (now - ChronoDuration::minutes(5)).to_rfc3339();
        let started = (now - ChronoDuration::minutes(20)).to_rfc3339();

        // Case 1: boot_id differs from current_boot_id -> Timed and UntilReboot drop; UntilChanged survives.
        let doc_json = serde_json::json!({
            "schema_version": 1,
            "boot_id": "boot-old",
            "entries": {
                "selinux": {
                    "subsystem": "selinux",
                    "relaxed": true,
                    "duration": { "kind": "timed", "minutes": 15 },
                    "generation_id": "gen-1",
                    "started_at": started,
                    "expires_at": past_expired,
                    "timer_unit": "akrep-revert-selinux.timer"
                },
                "firewalld": {
                    "subsystem": "firewalld",
                    "relaxed": true,
                    "duration": { "kind": "until_reboot" },
                    "generation_id": "gen-2",
                    "started_at": started,
                    "expires_at": null,
                    "timer_unit": null
                },
                "oomd": {
                    "subsystem": "oomd",
                    "relaxed": true,
                    "duration": { "kind": "until_changed" },
                    "generation_id": "gen-3",
                    "started_at": started,
                    "expires_at": null,
                    "timer_unit": null
                }
            }
        });
        fs::write(&cache_path, serde_json::to_vec_pretty(&doc_json).unwrap()).unwrap();

        let survived = collect_active_overrides(&cache_path, "boot-new", now);
        assert_eq!(survived.len(), 1);
        assert_eq!(survived[0].subsystem, "oomd");
        assert_eq!(survived[0].duration_kind, "until_changed");
        assert_eq!(survived[0].remaining_secs, None);

        // Case 2: even with matching boot_id, expired Timed (expires_at <= now) and !relaxed must drop.
        let doc2_json = serde_json::json!({
            "schema_version": 1,
            "boot_id": "boot-new",
            "entries": {
                "selinux": {
                    "subsystem": "selinux",
                    "relaxed": true,
                    "duration": { "kind": "timed", "minutes": 15 },
                    "generation_id": "gen-4",
                    "started_at": started,
                    "expires_at": past_expired,
                    "timer_unit": "akrep-revert-selinux.timer"
                },
                "firewalld": {
                    "subsystem": "firewalld",
                    "relaxed": false,
                    "duration": { "kind": "until_changed" },
                    "generation_id": "gen-5",
                    "started_at": started,
                    "expires_at": null,
                    "timer_unit": null
                }
            }
        });
        fs::write(&cache_path, serde_json::to_vec_pretty(&doc2_json).unwrap()).unwrap();
        let survived2 = collect_active_overrides(&cache_path, "boot-new", now);
        assert!(survived2.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_collect_active_overrides_computes_remaining_secs_for_live_timed() {
        let (dir, cache_path) = temp_cache_file("live_timed");
        let now = Utc::now();
        let expires = (now + ChronoDuration::minutes(15)).to_rfc3339();
        let started = now.to_rfc3339();

        let doc_json = serde_json::json!({
            "schema_version": 1,
            "boot_id": "boot-live-1",
            "entries": {
                "selinux": {
                    "subsystem": "selinux",
                    "relaxed": true,
                    "duration": { "kind": "timed", "minutes": 15 },
                    "generation_id": "gen-live-1",
                    "started_at": started,
                    "expires_at": expires,
                    "timer_unit": "akrep-revert-selinux.timer"
                },
                "firewalld": {
                    "subsystem": "firewalld",
                    "relaxed": true,
                    "duration": { "kind": "until_reboot" },
                    "generation_id": "gen-live-2",
                    "started_at": started,
                    "expires_at": null,
                    "timer_unit": null
                }
            }
        });
        fs::write(&cache_path, serde_json::to_vec_pretty(&doc_json).unwrap()).unwrap();

        let active = collect_active_overrides(&cache_path, "boot-live-1", now);
        assert_eq!(active.len(), 2);

        let sel = active.iter().find(|e| e.subsystem == "selinux").unwrap();
        assert_eq!(sel.duration_kind, "timed");
        assert_eq!(sel.minutes, Some(15));
        assert!(sel.remaining_secs.unwrap_or(0) > 0);
        assert!(sel.remaining_secs.unwrap_or(0) <= 900);

        let fw = active.iter().find(|e| e.subsystem == "firewalld").unwrap();
        assert_eq!(fw.duration_kind, "until_reboot");
        assert_eq!(fw.remaining_secs, None);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_reconcile_downgrades_timed_firewall_and_selinux_findings_to_attention_and_keeps_until_changed_firewall_critical(
    ) {
        let sentinel_mac = AkrepFinding {
            id: "sentinel.mac".to_string(),
            section: "system_protection".to_string(),
            severity: AkrepSeverity::Critical,
            availability: Availability::Available,
            title_tr: "Kontrol Uyarısı: SELinux/AppArmor MAC".to_string(),
            title_en: "Control Alert: Mandatory access control".to_string(),
            description_tr: "SELinux permissive".to_string(),
            description_en: "SELinux permissive".to_string(),
            remediation_hint_tr: "Doğrulayın".to_string(),
            remediation_hint_en: "Verify".to_string(),
            source: "blaze-sentinel-core".to_string(),
            read_only_note: "ro".to_string(),
        };
        let sentinel_fw = AkrepFinding {
            id: "sentinel.firewall".to_string(),
            section: "firewall".to_string(),
            severity: AkrepSeverity::Critical,
            availability: Availability::Available,
            title_tr: "Kontrol Uyarısı: Firewall".to_string(),
            title_en: "Control Alert: Firewall".to_string(),
            description_tr: "no active firewall service".to_string(),
            description_en: "no active firewall service".to_string(),
            remediation_hint_tr: "İnceleyin".to_string(),
            remediation_hint_en: "Inspect".to_string(),
            source: "blaze-sentinel-core".to_string(),
            read_only_note: "ro".to_string(),
        };

        // 1) Timed overrides on SELinux and Firewalld -> both Attention, deduplicated.
        let mut snap_timed = make_base_snapshot(
            AkrepSeverity::Attention,
            "Permissive",
            AkrepSeverity::Attention,
            "OFF",
            vec![sentinel_mac.clone(), sentinel_fw.clone()],
        );
        let timed_overrides = vec![
            ActiveOverrideEntry {
                subsystem: "selinux".to_string(),
                duration_kind: "timed".to_string(),
                minutes: Some(15),
                started_at: "2026-11-26T13:30:00Z".to_string(),
                expires_at: Some("2026-11-26T13:45:00Z".to_string()),
                remaining_secs: Some(900),
                boot_id: "boot-1".to_string(),
            },
            ActiveOverrideEntry {
                subsystem: "firewalld".to_string(),
                duration_kind: "timed".to_string(),
                minutes: Some(15),
                started_at: "2026-11-26T13:30:00Z".to_string(),
                expires_at: Some("2026-11-26T13:45:00Z".to_string()),
                remaining_secs: Some(840),
                boot_id: "boot-1".to_string(),
            },
        ];

        reconcile_snapshot_with_overrides(&mut snap_timed, &timed_overrides);

        assert_eq!(snap_timed.active_overrides.len(), 2);
        assert_eq!(snap_timed.overall_status, AkrepSeverity::Attention);
        assert!(!snap_timed.headline_tr.contains('!'));

        let sys_card = snap_timed
            .cards
            .iter()
            .find(|c| c.id == "system_protection")
            .unwrap();
        assert_eq!(sys_card.status, AkrepSeverity::Attention);

        let fw_card = snap_timed
            .cards
            .iter()
            .find(|c| c.id == "firewall")
            .unwrap();
        assert_eq!(fw_card.status, AkrepSeverity::Attention);

        let sys_findings: Vec<_> = snap_timed
            .findings
            .iter()
            .filter(|f| f.section == "system_protection")
            .collect();
        assert_eq!(
            sys_findings.len(),
            1,
            "duplicate sentinel.mac / selinux.mode must be deduplicated"
        );
        assert_eq!(sys_findings[0].severity, AkrepSeverity::Attention);
        assert!(sys_findings[0].description_tr.contains("15 dk"));

        let fw_findings: Vec<_> = snap_timed
            .findings
            .iter()
            .filter(|f| f.section == "firewall")
            .collect();
        assert_eq!(
            fw_findings.len(),
            1,
            "duplicate sentinel.firewall / firewall.inactive must be deduplicated"
        );
        assert_eq!(fw_findings[0].severity, AkrepSeverity::Attention);
        assert!(fw_findings[0].description_tr.contains("14 dk"));

        // 2) UntilChanged on SELinux (Attention) and Firewalld (Critical)
        let mut snap_until_changed = make_base_snapshot(
            AkrepSeverity::Attention,
            "Permissive",
            AkrepSeverity::Attention,
            "OFF",
            vec![sentinel_mac.clone(), sentinel_fw.clone()],
        );
        let until_changed_overrides = vec![
            ActiveOverrideEntry {
                subsystem: "selinux".to_string(),
                duration_kind: "until_changed".to_string(),
                minutes: None,
                started_at: "2026-11-26T13:30:00Z".to_string(),
                expires_at: None,
                remaining_secs: None,
                boot_id: "boot-1".to_string(),
            },
            ActiveOverrideEntry {
                subsystem: "firewalld".to_string(),
                duration_kind: "until_changed".to_string(),
                minutes: None,
                started_at: "2026-11-26T13:30:00Z".to_string(),
                expires_at: None,
                remaining_secs: None,
                boot_id: "boot-1".to_string(),
            },
        ];

        reconcile_snapshot_with_overrides(&mut snap_until_changed, &until_changed_overrides);

        assert_eq!(snap_until_changed.overall_status, AkrepSeverity::Critical);
        assert_eq!(
            snap_until_changed
                .cards
                .iter()
                .find(|c| c.id == "system_protection")
                .unwrap()
                .status,
            AkrepSeverity::Attention
        );
        assert_eq!(
            snap_until_changed
                .cards
                .iter()
                .find(|c| c.id == "firewall")
                .unwrap()
                .status,
            AkrepSeverity::Critical
        );
        let fw_uc_findings: Vec<_> = snap_until_changed
            .findings
            .iter()
            .filter(|f| f.section == "firewall")
            .collect();
        assert_eq!(fw_uc_findings.len(), 1);
        assert_eq!(fw_uc_findings[0].severity, AkrepSeverity::Critical);

        // 3) UntilReboot on SELinux and Firewalld -> both Attention with reboot note, deduplicated.
        let mut snap_until_reboot = make_base_snapshot(
            AkrepSeverity::Attention,
            "Permissive",
            AkrepSeverity::Unknown,
            "OFF",
            vec![sentinel_mac, sentinel_fw],
        );
        let until_reboot_overrides = vec![
            ActiveOverrideEntry {
                subsystem: "selinux".to_string(),
                duration_kind: "until_reboot".to_string(),
                minutes: None,
                started_at: "2026-11-26T13:30:00Z".to_string(),
                expires_at: None,
                remaining_secs: None,
                boot_id: "boot-1".to_string(),
            },
            ActiveOverrideEntry {
                subsystem: "firewalld".to_string(),
                duration_kind: "until_reboot".to_string(),
                minutes: None,
                started_at: "2026-11-26T13:30:00Z".to_string(),
                expires_at: None,
                remaining_secs: None,
                boot_id: "boot-1".to_string(),
            },
        ];

        reconcile_snapshot_with_overrides(&mut snap_until_reboot, &until_reboot_overrides);
        assert_eq!(snap_until_reboot.overall_status, AkrepSeverity::Attention);
        assert_eq!(
            snap_until_reboot
                .cards
                .iter()
                .find(|c| c.id == "firewall")
                .unwrap()
                .status,
            AkrepSeverity::Attention
        );
        assert_eq!(
            snap_until_reboot
                .findings
                .iter()
                .filter(|f| f.section == "firewall")
                .count(),
            1
        );
        assert!(snap_until_reboot
            .findings
            .iter()
            .find(|f| f.section == "firewall")
            .unwrap()
            .description_tr
            .contains("yeniden başlatmaya kadar"));
    }

    #[tokio::test]
    async fn test_polkit_actions_and_snapshot_populate_compat_and_mok() {
        let actions = crate::collectors::default_polkit_actions();
        assert!(
            actions
                .iter()
                .any(|a| a == "tr.com.blazeos.Akrep1.system.compat-manage"),
            "missing system.compat-manage polkit action"
        );

        let snap = crate::collectors::collect_snapshot(None).await;
        assert!(snap.system_compat.is_some());
        assert!(snap.driver_mok.is_some());
        assert!(snap
            .capabilities
            .future_polkit_actions
            .iter()
            .any(|a| a == "tr.com.blazeos.Akrep1.system.compat-manage"));
    }

    #[test]
    fn test_reconcile_drops_phantom_overrides_when_live_state_restored() {
        let mut snap = make_base_snapshot(
            AkrepSeverity::Protected,
            "Enforcing",
            AkrepSeverity::Protected,
            "FedoraWorkstation",
            Vec::new(),
        );
        snap.system_compat = Some(SystemCompatStatus {
            oomd_active: true,
            oomd_enabled: true,
            nofile_enabled: false,
            nofile_soft: 1024,
            nofile_hard: 524_288,
            android_udev_installed: false,
            ncurses_compat_installed: false,
        });

        let stale_overrides = vec![
            ActiveOverrideEntry {
                subsystem: "selinux".to_string(),
                duration_kind: "until_changed".to_string(),
                minutes: None,
                started_at: "2026-11-26T13:00:00Z".to_string(),
                expires_at: None,
                remaining_secs: None,
                boot_id: "boot-1".to_string(),
            },
            ActiveOverrideEntry {
                subsystem: "firewalld".to_string(),
                duration_kind: "until_reboot".to_string(),
                minutes: None,
                started_at: "2026-11-26T13:00:00Z".to_string(),
                expires_at: None,
                remaining_secs: None,
                boot_id: "boot-1".to_string(),
            },
            ActiveOverrideEntry {
                subsystem: "oomd".to_string(),
                duration_kind: "timed".to_string(),
                minutes: Some(60),
                started_at: "2026-11-26T13:00:00Z".to_string(),
                expires_at: Some("2026-11-26T14:00:00Z".to_string()),
                remaining_secs: Some(3600),
                boot_id: "boot-1".to_string(),
            },
        ];

        reconcile_snapshot_with_overrides(&mut snap, &stale_overrides);
        assert!(
            snap.active_overrides.is_empty(),
            "phantom overrides must be cleared when live state is restored"
        );
        assert_eq!(snap.overall_status, AkrepSeverity::Protected);

        // When oomd_active is false, oomd override stays active without altering SELinux/Firewalld status.
        snap.system_compat.as_mut().unwrap().oomd_active = false;
        reconcile_snapshot_with_overrides(&mut snap, &stale_overrides);
        assert_eq!(snap.active_overrides.len(), 1);
        assert_eq!(snap.active_overrides[0].subsystem, "oomd");
        assert_eq!(snap.overall_status, AkrepSeverity::Protected);
    }

    #[test]
    fn test_parse_nofile_dropin_and_mokutil_helpers() {
        let pam_raw = "# Akrep managed\n* soft nofile 65536\n* hard nofile 524288\n";
        assert_eq!(
            parse_nofile_dropin_pair(Some(pam_raw), None),
            Some((65536, 524_288))
        );

        let sysd_raw = "[Manager]\nDefaultLimitNOFILE=65536:524288\n";
        assert_eq!(
            parse_nofile_dropin_pair(None, Some(sysd_raw)),
            Some((65536, 524_288))
        );
        assert_eq!(parse_nofile_dropin_pair(None, None), None);

        let enrolled_sample =
            "[key 1]\nSHA1 Fingerprint: aa:bb\n[key 2]\nSHA1 Fingerprint: cc:dd\n";
        assert_eq!(parse_mokutil_enrolled_count(enrolled_sample), Some(2));
        assert_eq!(parse_mokutil_enrolled_count("MokListRT is empty"), Some(0));
        assert_eq!(parse_mokutil_sb_state("SecureBoot enabled\n"), Some(true));
        assert_eq!(parse_mokutil_sb_state("SecureBoot disabled\n"), Some(false));

        let risky = build_driver_mok_summary(Some(true), Some(0), false, true);
        assert!(risky.unsigned_risk);
        assert!(risky.guidance_tr.contains("modülü"));
        assert!(!risky.guidance_tr.contains('!'));

        let safe = build_driver_mok_summary(Some(true), Some(1), true, true);
        assert!(!safe.unsigned_risk);
        assert!(safe.guidance_tr.contains("kayıtlı MOK"));
        assert!(!safe.guidance_tr.contains('!'));
    }

    #[test]
    fn test_collect_system_compat_and_driver_mok_cached_fast_and_graceful() {
        let _ = collect_system_compat_cached();
        let _ = collect_driver_mok_cached();

        let started = Instant::now();
        let compat = collect_system_compat_cached();
        let mok = collect_driver_mok_cached();
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_millis(100),
            "cached compat/mok collectors took too long: {:?}",
            elapsed
        );
        assert!(compat.nofile_hard >= compat.nofile_soft);
        assert!(!mok.guidance_tr.is_empty());
        assert!(!mok.guidance_tr.contains('!'));
    }
}
