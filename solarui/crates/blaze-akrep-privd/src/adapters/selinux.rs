use crate::transaction_engine::{
    write_json_atomic_0600, ApplyOutcome, MutationBackend, PreviewOutcome,
};
use async_trait::async_trait;
use blaze_akrep_common::{
    redact_secrets, validate_restorecon_path, validate_selinux_boolean_name, ActionRequest,
    AkrepError, AvcDenialGroup, FileContextEntry, PlanDiffItem, RestoreconPreviewEntry, RiskLevel,
    SelinuxBooleanEntry, SelinuxBooleanMeta, SelinuxDiagnoseQuery, SelinuxDiagnoseResponse,
    SelinuxRuntimeMode, StateFingerprint, SuggestedSelinuxFix, TransactionRecord,
};
use blaze_sentinel_core::sha256_hex;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

pub const MAX_RESTORECON_WALK_ENTRIES: usize = 2_000;
pub const MAX_AUDIT_LOG_TAIL_BYTES: u64 = 8 * 1024 * 1024;
const PERSISTENT_CACHE_TTL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersistentSource {
    Authoritative,
    LocalOverridesOnly,
}

#[derive(Debug, Clone)]
pub struct PersistentBooleanMap {
    pub source: PersistentSource,
    pub values: HashMap<String, bool>,
}

impl PersistentBooleanMap {
    pub fn resolve_for(&self, name: &str, current: bool) -> Option<bool> {
        match self.source {
            PersistentSource::Authoritative => {
                Some(self.values.get(name).copied().unwrap_or(current))
            }
            PersistentSource::LocalOverridesOnly => self.values.get(name).copied(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawBooleanState {
    pub name: String,
    pub current: bool,
    pub pending: bool,
    pub persistent: Option<bool>,
}

impl RawBooleanState {
    pub fn is_persistent_enabled(&self) -> bool {
        self.persistent == Some(true)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelinuxConfigState {
    pub configured_mode: Option<String>,
    pub policy_type: Option<String>,
}

struct CatalogStaticEntry {
    name: &'static str,
    category: &'static str,
    risk: RiskLevel,
    summary_tr: &'static str,
    summary_en: &'static str,
}

const CURATED_SELINUX_BOOLEANS: &[CatalogStaticEntry] = &[
    CatalogStaticEntry {
        name: "container_manage_cgroup",
        category: "container",
        risk: RiskLevel::Low,
        summary_tr: "Kapsayicilarin kendi cgroup kaynak sinirlarini yonetmesine izin verir (rootless Podman/systemd kapsayicilari icin yaygin).",
        summary_en: "Allows containers to manage their own cgroup hierarchies (common for systemd-in-container workloads).",
    },
    CatalogStaticEntry {
        name: "container_use_devices",
        category: "container",
        risk: RiskLevel::Medium,
        summary_tr: "Kapsayicilarin atanmis blok ve karakter aygit dugumlerini dogrudan kullanmasina izin verir.",
        summary_en: "Allows containers to directly access assigned character and block device nodes.",
    },
    CatalogStaticEntry {
        name: "container_use_cephfs",
        category: "container",
        risk: RiskLevel::Low,
        summary_tr: "Kapsayici sureclerinin CephFS uzerindeki dosyalari okumasina ve yazmasina izin verir.",
        summary_en: "Allows container processes to read and write files stored on CephFS mounts.",
    },
    CatalogStaticEntry {
        name: "container_connect_any",
        category: "container",
        risk: RiskLevel::Medium,
        summary_tr: "Kapsayici sureclerinin sistemdeki tum TCP/UDP port turlerine giden baglanti kurmasina izin verir.",
        summary_en: "Allows container processes to initiate outbound connections to arbitrary TCP/UDP port types.",
    },
    CatalogStaticEntry {
        name: "virt_use_kvm",
        category: "virtualization",
        risk: RiskLevel::Low,
        summary_tr: "Sanal makinelerin donanim destekli KVM sanallastirma hizlandirmasini kullanmasina izin verir.",
        summary_en: "Allows virtual machines to utilize hardware-assisted KVM virtualization.",
    },
    CatalogStaticEntry {
        name: "virt_use_usb",
        category: "virtualization",
        risk: RiskLevel::Medium,
        summary_tr: "Sanal makinelerin ana makinedeki USB aygitlarina dogrudan gecis (passthrough) erisimi saglar.",
        summary_en: "Allows virtual machines to access host USB devices via passthrough.",
    },
    CatalogStaticEntry {
        name: "virt_use_nfs",
        category: "virtualization",
        risk: RiskLevel::Low,
        summary_tr: "Sanal makine disk imajlarinin NFS ag paylasimlari uzerinde barindirilmasina izin verir.",
        summary_en: "Allows virtual machine disk images to reside on NFS network shares.",
    },
    CatalogStaticEntry {
        name: "virt_use_samba",
        category: "virtualization",
        risk: RiskLevel::Low,
        summary_tr: "Sanal makine disk imajlarinin CIFS/SMB ag paylasimlari uzerinde kullanilmasina izin verir.",
        summary_en: "Allows virtual machine disk images to reside on CIFS/SMB network shares.",
    },
    CatalogStaticEntry {
        name: "virt_sandbox_use_all_caps",
        category: "virtualization",
        risk: RiskLevel::High,
        summary_tr: "Sanallastirma kum havuzlarinin tum Linux yeteneklerini (capabilities) kullanmasina izin verir; saldiri yuzeyini genisletir.",
        summary_en: "Allows virtualization sandboxes to retain all Linux capabilities; significantly widens attack surface.",
    },
    CatalogStaticEntry {
        name: "httpd_can_network_connect",
        category: "web_network",
        risk: RiskLevel::Medium,
        summary_tr: "Web sunucusu (httpd/nginx) betik ve modullerinin dis ag TCP soketlerine baglanmasina izin verir.",
        summary_en: "Allows web server scripts and modules to initiate outbound TCP network connections.",
    },
    CatalogStaticEntry {
        name: "httpd_can_network_connect_db",
        category: "web_network",
        risk: RiskLevel::Low,
        summary_tr: "Web sunucusu sureclerinin yalnizca bilinen veritabani portlarina ag uzerinden baglanmasina izin verir.",
        summary_en: "Allows web server processes to connect over the network strictly to database ports.",
    },
    CatalogStaticEntry {
        name: "httpd_enable_homedirs",
        category: "web_network",
        risk: RiskLevel::Medium,
        summary_tr: "Web sunucusunun kullanici ev dizinlerindeki public_html alt dizinlerine erismesini etkinlestirir.",
        summary_en: "Allows the web server to traverse user home directories for public_html content.",
    },
    CatalogStaticEntry {
        name: "httpd_read_user_content",
        category: "web_network",
        risk: RiskLevel::Medium,
        summary_tr: "Web sunucusunun tum kullanici icerik etiketli dosyalari okumasina izin verir.",
        summary_en: "Allows the web server to read generic user-content labeled files.",
    },
    CatalogStaticEntry {
        name: "httpd_unified",
        category: "web_network",
        risk: RiskLevel::High,
        summary_tr: "Tum web sunucusu icerik turlerini birlestirerek salt okunur ve yazilabilir web etiket ayrimini zayiflatir.",
        summary_en: "Unifies all web content types, weakening separation between read-only code and writable web data.",
    },
    CatalogStaticEntry {
        name: "samba_enable_home_dirs",
        category: "file_sharing",
        risk: RiskLevel::Medium,
        summary_tr: "Samba dosya sunucusunun kullanici dizinlerini SMB uzerinden paylasmasina izin verir.",
        summary_en: "Allows the Samba daemon to share user directories over SMB.",
    },
    CatalogStaticEntry {
        name: "samba_export_all_ro",
        category: "file_sharing",
        risk: RiskLevel::Medium,
        summary_tr: "Samba'nin rastgele dosya ve dizinleri salt okunur olarak disa aktarmasina izin verir.",
        summary_en: "Allows Samba to export arbitrary files and directories read-only.",
    },
    CatalogStaticEntry {
        name: "samba_export_all_rw",
        category: "file_sharing",
        risk: RiskLevel::High,
        summary_tr: "Samba'nin sistemdeki etiketlerden bagimsiz dosya/dizinleri okuma-yazma olarak paylasmasina izin verir.",
        summary_en: "Allows Samba to export arbitrary files and directories read-write regardless of SELinux file type.",
    },
    CatalogStaticEntry {
        name: "use_nfs_home_dirs",
        category: "file_sharing",
        risk: RiskLevel::Low,
        summary_tr: "NFS uzerinden baglanan uzak kullanici dizinlerinin oturum surecleri tarafindan kullanilmasini destekler.",
        summary_en: "Supports user home directories mounted remotely over NFS.",
    },
    CatalogStaticEntry {
        name: "use_samba_home_dirs",
        category: "file_sharing",
        risk: RiskLevel::Low,
        summary_tr: "CIFS/SMB uzerinden baglanan uzak kullanici dizinlerinin oturum surecleri tarafindan kullanilmasini destekler.",
        summary_en: "Supports user home directories mounted remotely over CIFS/SMB.",
    },
    CatalogStaticEntry {
        name: "ftpd_full_access",
        category: "file_sharing",
        risk: RiskLevel::High,
        summary_tr: "FTP sunucusuna dosya sistemi genelinde tam erisim tanir; yalnizca izole ortamlarda kullanilmalidir.",
        summary_en: "Grants the FTP daemon broad filesystem access across non-FTP contexts.",
    },
    CatalogStaticEntry {
        name: "nis_enabled",
        category: "auth_directory",
        risk: RiskLevel::Medium,
        summary_tr: "Sistem servislerinin NIS/ypbind dizin kimlik dogrulamasi icin ek ag portlari acmasina izin verir.",
        summary_en: "Allows system services to use NIS/ypbind authentication and dynamic RPC ports.",
    },
    CatalogStaticEntry {
        name: "authlogin_nsswitch_use_ldap",
        category: "auth_directory",
        risk: RiskLevel::Low,
        summary_tr: "Oturum acma ve ad cozumleme servislerinin kurumsal LDAP sunucularina baglanmasina izin verir.",
        summary_en: "Allows authentication and NSS services to query enterprise LDAP servers.",
    },
    CatalogStaticEntry {
        name: "ssh_sysadm_login",
        category: "auth_directory",
        risk: RiskLevel::High,
        summary_tr: "SSH uzerinden dogrudan sysadm_r yonetici SELinux rolu ile giris yapilmasina izin verir.",
        summary_en: "Allows SSH logins to map directly into the privileged sysadm_r SELinux role.",
    },
    CatalogStaticEntry {
        name: "selinuxuser_execheap",
        category: "memory_hardening",
        risk: RiskLevel::Critical,
        summary_tr: "KRITIK: Kullanici sureclerinin yigin (heap) bellegini calistirilabilir yapmasina izin verir; bellek istismar korumasini devre disi birakir.",
        summary_en: "CRITICAL: Allows user processes to make heap memory executable, weakening W^X exploit mitigations.",
    },
    CatalogStaticEntry {
        name: "selinuxuser_execmod",
        category: "memory_hardening",
        risk: RiskLevel::High,
        summary_tr: "Kullanici sureclerinin metin yer degistirmeli (textrel) eski paylasimli kutuphaneleri calistirmasina izin verir.",
        summary_en: "Allows user processes to execute modified file-backed memory mappings (legacy textrel libraries).",
    },
    CatalogStaticEntry {
        name: "selinuxuser_execstack",
        category: "memory_hardening",
        risk: RiskLevel::Critical,
        summary_tr: "KRITIK: Kullanici sureclerinin calistirilabilir yigit (executable stack) kullanmasina izin verir; arabellek tasma istismar riskini artirir.",
        summary_en: "CRITICAL: Allows user processes to execute code on the stack, weakening stack exploit protections.",
    },
    CatalogStaticEntry {
        name: "mmap_low_allowed",
        category: "memory_hardening",
        risk: RiskLevel::Critical,
        summary_tr: "KRITIK: Sureclerin dusuk sanal bellek adreslerini (NULL dereference bolgesi) eslemesine izin verir.",
        summary_en: "CRITICAL: Allows processes to map low virtual memory addresses, increasing kernel NULL-dereference exploitability.",
    },
    CatalogStaticEntry {
        name: "deny_ptrace",
        category: "desktop_debug",
        risk: RiskLevel::Medium,
        summary_tr: "Etkinlestirildiginde kullanici alanindaki hata ayiklayicilarin (gdb/strace) diger surecleri ptrace ile incelemesini kisitlar.",
        summary_en: "When enabled, restricts ptrace process inspection across user-space domains (affects gdb/strace).",
    },
    CatalogStaticEntry {
        name: "selinuxuser_rw_noexattrfile",
        category: "gaming_wine",
        risk: RiskLevel::Low,
        summary_tr: "Kullanici sureclerinin genisletilmis oznitelik desteklemeyen (FAT/exFAT harici diskler) dosyalara yazmasina izin verir.",
        summary_en: "Allows user sessions to read and write files on filesystems lacking extended attributes (FAT/exFAT).",
    },
    CatalogStaticEntry {
        name: "wine_mmap_zero_ignore",
        category: "gaming_wine",
        risk: RiskLevel::Medium,
        summary_tr: "Wine uyumluluk katmaninin sifir adres bellek esleme denemelerinin sessizce yoksayilmasini saglar.",
        summary_en: "Silently ignores Wine attempts to mmap the zero address without granting low-memory mapping.",
    },
];

pub fn curated_boolean_catalog() -> Vec<SelinuxBooleanMeta> {
    CURATED_SELINUX_BOOLEANS
        .iter()
        .map(|e| SelinuxBooleanMeta {
            name: e.name.to_string(),
            category: e.category.to_string(),
            risk: e.risk,
            summary_tr: e.summary_tr.to_string(),
            summary_en: e.summary_en.to_string(),
        })
        .collect()
}

pub fn lookup_boolean_meta(name: &str) -> (bool, SelinuxBooleanMeta) {
    if let Some(found) = CURATED_SELINUX_BOOLEANS.iter().find(|e| e.name == name) {
        return (
            true,
            SelinuxBooleanMeta {
                name: found.name.to_string(),
                category: found.category.to_string(),
                risk: found.risk,
                summary_tr: found.summary_tr.to_string(),
                summary_en: found.summary_en.to_string(),
            },
        );
    }

    (
        false,
        SelinuxBooleanMeta {
            name: name.to_string(),
            category: "uncataloged".to_string(),
            risk: RiskLevel::Medium,
            summary_tr: format!(
                "Katalog disi SELinux politika anahtari ('{}'); degisiklik oncesi hedef servisin gereksinimini dogrulayin.",
                name
            ),
            summary_en: format!(
                "Uncataloged SELinux policy boolean ('{}'); verify target service requirements before toggling.",
                name
            ),
        },
    )
}

pub trait SelinuxSysOps: Send + Sync {
    fn read_runtime_mode(&self) -> Result<SelinuxRuntimeMode, AkrepError>;
    fn read_config(&self) -> Result<SelinuxConfigState, AkrepError>;
    fn list_booleans(&self) -> Result<Vec<RawBooleanState>, AkrepError>;
    fn read_boolean(&self, name: &str) -> Result<Option<RawBooleanState>, AkrepError>;
    fn set_boolean(&self, name: &str, value: bool, persistent: bool) -> Result<(), AkrepError>;
    fn preview_restorecon(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<RestoreconPreviewEntry>, AkrepError>;
    fn read_file_contexts(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<FileContextEntry>, AkrepError>;
    fn apply_restorecon(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<RestoreconPreviewEntry>, AkrepError>;
    fn restore_file_context(&self, path: &Path, context: &str) -> Result<(), AkrepError>;
    fn read_audit_avc_lines(&self) -> Result<Vec<String>, AkrepError>;
    fn read_journal_avc_lines(&self) -> Result<Vec<String>, AkrepError>;
}

fn resolve_binary(candidates: &[&str]) -> Option<PathBuf> {
    candidates.iter().map(PathBuf::from).find(|p| p.is_file())
}

pub fn command_c_locale<S: AsRef<OsStr>>(program: S) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("LC_ALL", "C").env("LANG", "C");
    cmd
}

pub fn validate_selinux_policy_type(policy: &str) -> Result<&str, AkrepError> {
    let trimmed = policy.trim();
    if trimmed.is_empty() || trimmed.len() > 64 || trimmed == "." || trimmed.contains("..") {
        return Err(AkrepError::InvalidInput(format!(
            "Invalid SELinux policy type '{}'",
            policy
        )));
    }
    if !trimmed
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(AkrepError::InvalidInput(format!(
            "Invalid characters in SELinux policy type '{}'",
            policy
        )));
    }
    Ok(trimmed)
}

pub fn resolve_and_validate_restorecon_target(
    path: &Path,
    recursive: bool,
) -> Result<PathBuf, AkrepError> {
    let path_str = path
        .to_str()
        .ok_or_else(|| AkrepError::InvalidInput("Target path must be valid UTF-8".to_string()))?;
    validate_restorecon_path(path_str)?;

    let leaf_meta = fs::symlink_metadata(path).map_err(|_| {
        AkrepError::InvalidInput(format!("Target path '{}' does not exist", path_str))
    })?;

    if recursive && leaf_meta.file_type().is_symlink() {
        return Err(AkrepError::ForbiddenOperation(format!(
            "Recursive restorecon on symlink target '{}' is forbidden",
            path_str
        )));
    }

    let parent = path.parent().ok_or_else(|| {
        AkrepError::ForbiddenOperation("Target path must have a valid parent directory".to_string())
    })?;
    let canon_parent = fs::canonicalize(parent).map_err(|e| {
        AkrepError::InvalidInput(format!(
            "Failed to canonicalize parent directory for '{}': {}",
            path_str,
            redact_secrets(&e.to_string())
        ))
    })?;

    let resolved = match path.file_name() {
        Some(name) => canon_parent.join(name),
        None => canon_parent,
    };
    let resolved_str = resolved.to_str().ok_or_else(|| {
        AkrepError::InvalidInput("Canonicalized target path is not valid UTF-8".to_string())
    })?;
    validate_restorecon_path(resolved_str)?;

    if leaf_meta.file_type().is_dir() {
        let canon_dir = fs::canonicalize(&resolved).map_err(|e| {
            AkrepError::InvalidInput(format!(
                "Failed to canonicalize target directory '{}': {}",
                path_str,
                redact_secrets(&e.to_string())
            ))
        })?;
        let canon_dir_str = canon_dir.to_str().ok_or_else(|| {
            AkrepError::InvalidInput("Canonicalized directory is not valid UTF-8".to_string())
        })?;
        validate_restorecon_path(canon_dir_str)?;
        return Ok(canon_dir);
    }

    Ok(resolved)
}

pub fn walk_restorecon_paths_bounded(
    path: &Path,
    recursive: bool,
    max_entries: usize,
) -> Result<Vec<PathBuf>, AkrepError> {
    let resolved = resolve_and_validate_restorecon_target(path, recursive)?;
    let root_meta = fs::symlink_metadata(&resolved)?;

    if !recursive || !root_meta.file_type().is_dir() {
        return Ok(vec![resolved]);
    }

    let root_dev = root_meta.dev();
    let mut collected = Vec::new();
    let mut stack = vec![resolved.clone()];

    while let Some(cur_dir) = stack.pop() {
        if collected.len() >= max_entries {
            return Err(AkrepError::InvalidInput(format!(
                "Target directory '{}' exceeds maximum entry limit ({}) for transactional restorecon",
                resolved.display(),
                max_entries
            )));
        }
        collected.push(cur_dir.clone());

        let mut children: Vec<PathBuf> = fs::read_dir(&cur_dir)?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|e| e.path())
            .collect();
        children.sort();

        for child in children.into_iter().rev() {
            let Some(child_str) = child.to_str() else {
                continue;
            };
            if validate_restorecon_path(child_str).is_err() {
                continue;
            }
            let child_meta = fs::symlink_metadata(&child)?;
            // Never traverse directory symlinks or cross filesystem mount boundaries into pseudo-mounts
            if child_meta.file_type().is_dir() {
                if child_meta.dev() != root_dev {
                    continue;
                }
                stack.push(child);
            } else {
                if collected.len() >= max_entries {
                    return Err(AkrepError::InvalidInput(format!(
                        "Target directory '{}' exceeds maximum entry limit ({}) for transactional restorecon",
                        resolved.display(),
                        max_entries
                    )));
                }
                collected.push(child);
            }
        }
    }

    collected.sort();
    Ok(collected)
}

pub fn read_tail_lossy_string(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let file_len = file.metadata()?.len();
    let mut buf = Vec::new();
    if file_len > max_bytes {
        file.seek(SeekFrom::Start(file_len - max_bytes))?;
        file.read_to_end(&mut buf)?;
        if let Some(nl_pos) = buf.iter().position(|&b| b == b'\n') {
            buf.drain(..=nl_pos);
        }
    } else {
        file.read_to_end(&mut buf)?;
    }
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

pub fn extract_avc_lines_from_audit_file(path: &Path) -> Result<Vec<String>, AkrepError> {
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let Ok(raw) = read_tail_lossy_string(path, MAX_AUDIT_LOG_TAIL_BYTES) else {
        return Ok(Vec::new());
    };
    let mut lines: Vec<String> = raw
        .lines()
        .filter(|l| is_avc_denied_line(l))
        .map(ToString::to_string)
        .collect();
    if lines.len() > 1_000 {
        lines = lines.split_off(lines.len() - 1_000);
    }
    Ok(lines)
}

pub fn parse_semanage_boolean_output(raw: &str) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some((left, rest)) = trimmed.split_once('(') else {
            continue;
        };
        let name = left.trim();
        let Some((inside, _desc)) = rest.split_once(')') else {
            continue;
        };
        let mut cols = inside.split(',');
        let _active_col = cols.next();
        let Some(default_col) = cols.next() else {
            continue;
        };
        let norm = default_col.trim().to_ascii_lowercase();
        let val = match norm.as_str() {
            "on" | "1" | "true" => Some(true),
            "off" | "0" | "false" => Some(false),
            _ => None,
        };
        if let Some(v) = val {
            if !name.is_empty() {
                map.insert(name.to_string(), v);
            }
        }
    }
    map
}

pub fn parse_booleans_local_file(raw: &str) -> HashMap<String, bool> {
    let mut map = HashMap::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let pair = trimmed
            .split_once('=')
            .or_else(|| trimmed.split_once(char::is_whitespace));
        let Some((k, v)) = pair else {
            continue;
        };
        let name = k.trim();
        let val_s = v.trim().to_ascii_lowercase();
        let parsed = match val_s.as_str() {
            "1" | "on" | "true" => Some(true),
            "0" | "off" | "false" => Some(false),
            _ => None,
        };
        if let Some(b) = parsed {
            if !name.is_empty() {
                map.insert(name.to_string(), b);
            }
        }
    }
    map
}

pub fn parse_restorecon_output(raw: &str) -> Vec<RestoreconPreviewEntry> {
    let mut entries = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        let body = trimmed
            .strip_prefix("Would relabel ")
            .or_else(|| trimmed.strip_prefix("Relabeled "));
        let Some(rest) = body else {
            continue;
        };
        let Some((path_part, ctx_part)) = rest.rsplit_once(" from ") else {
            continue;
        };
        let Some((from_ctx, to_ctx)) = ctx_part.split_once(" to ") else {
            continue;
        };
        entries.push(RestoreconPreviewEntry {
            path: path_part.trim().to_string(),
            from_context: from_ctx.trim().to_string(),
            to_context: to_ctx.trim().to_string(),
        });
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    entries
}

fn is_avc_denied_line(line: &str) -> bool {
    if !line.contains("denied") {
        return false;
    }
    line.contains("type=AVC")
        || line.contains("type=USER_AVC")
        || line.contains("type=SELINUX_ERR")
        || line.contains("type=USER_SELINUX_ERR")
        || line.contains("avc:")
}

#[derive(Debug, Clone)]
struct CachedPersistentBooleans {
    policy_type: String,
    stamp: Option<(u64, SystemTime)>,
    loaded_at: Instant,
    map: PersistentBooleanMap,
}

pub struct LinuxSelinuxOps {
    persistent_cache: Mutex<Option<CachedPersistentBooleans>>,
}

impl LinuxSelinuxOps {
    pub fn new() -> Self {
        Self {
            persistent_cache: Mutex::new(None),
        }
    }

    fn policy_store_stamp(policy_type: &str) -> Option<(u64, SystemTime)> {
        let commit_path = PathBuf::from(format!(
            "/var/lib/selinux/{}/active/commit_num",
            policy_type
        ));
        let local_path = PathBuf::from(format!(
            "/var/lib/selinux/{}/active/booleans.local",
            policy_type
        ));
        let meta = fs::metadata(&commit_path)
            .or_else(|_| fs::metadata(&local_path))
            .ok()?;
        let mtime = meta.modified().ok()?;
        Some((meta.len(), mtime))
    }

    fn invalidate_persistent_cache(&self) {
        if let Ok(mut guard) = self.persistent_cache.lock() {
            *guard = None;
        }
    }

    fn read_persistent_map(
        &self,
        raw_policy_type: &str,
    ) -> Result<PersistentBooleanMap, AkrepError> {
        let policy_type = validate_selinux_policy_type(raw_policy_type)?;
        let current_stamp = Self::policy_store_stamp(policy_type);

        if let Ok(guard) = self.persistent_cache.lock() {
            if let Some(cached) = guard.as_ref() {
                if cached.policy_type == policy_type
                    && cached.stamp == current_stamp
                    && cached.loaded_at.elapsed() < PERSISTENT_CACHE_TTL
                {
                    return Ok(cached.map.clone());
                }
            }
        }

        let loaded = if let Some(semanage) =
            resolve_binary(&["/usr/sbin/semanage", "/sbin/semanage", "/usr/bin/semanage"])
        {
            match command_c_locale(semanage)
                .args(["boolean", "-l", "-n"])
                .output()
            {
                Ok(out) if out.status.success() => {
                    let parsed =
                        parse_semanage_boolean_output(&String::from_utf8_lossy(&out.stdout));
                    if !parsed.is_empty() {
                        Some(PersistentBooleanMap {
                            source: PersistentSource::Authoritative,
                            values: parsed,
                        })
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };

        let map = match loaded {
            Some(authoritative) => authoritative,
            None => {
                let local_path = PathBuf::from(format!(
                    "/var/lib/selinux/{}/active/booleans.local",
                    policy_type
                ));
                let local_vals = fs::read_to_string(local_path)
                    .map(|raw| parse_booleans_local_file(&raw))
                    .unwrap_or_default();
                PersistentBooleanMap {
                    source: PersistentSource::LocalOverridesOnly,
                    values: local_vals,
                }
            }
        };

        if let Ok(mut guard) = self.persistent_cache.lock() {
            *guard = Some(CachedPersistentBooleans {
                policy_type: policy_type.to_string(),
                stamp: current_stamp,
                loaded_at: Instant::now(),
                map: map.clone(),
            });
        }

        Ok(map)
    }

    fn parse_sysfs_bool_file(path: &Path) -> Result<(bool, bool), AkrepError> {
        let raw = fs::read_to_string(path)?;
        let mut parts = raw.split_whitespace();
        let cur = parts
            .next()
            .ok_or_else(|| AkrepError::Internal("Malformed sysfs boolean file".to_string()))?;
        let pend = parts.next().unwrap_or(cur);
        Ok((cur == "1", pend == "1"))
    }

    fn read_single_file_context(path: &Path) -> Result<String, AkrepError> {
        if let Some(getfattr) = resolve_binary(&["/usr/bin/getfattr", "/bin/getfattr"]) {
            if let Ok(out) = command_c_locale(getfattr)
                .args([
                    "-h",
                    "-n",
                    "security.selinux",
                    "--only-values",
                    "--absolute-names",
                    "--",
                ])
                .arg(path)
                .output()
            {
                if out.status.success() {
                    let s = String::from_utf8_lossy(&out.stdout)
                        .trim_matches(|c: char| c == '\0' || c.is_whitespace())
                        .to_string();
                    if !s.is_empty() {
                        return Ok(s);
                    }
                }
            }
        }

        let stat_bin = resolve_binary(&["/usr/bin/stat", "/bin/stat"]).ok_or_else(|| {
            AkrepError::BackendUnavailable(
                "Missing 'stat' utility for SELinux context inspection".to_string(),
            )
        })?;
        let out = command_c_locale(stat_bin)
            .args(["-c", "%C", "--"])
            .arg(path)
            .output()?;
        if !out.status.success() {
            return Err(AkrepError::IoError(redact_secrets(
                &String::from_utf8_lossy(&out.stderr),
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    }
}

impl Default for LinuxSelinuxOps {
    fn default() -> Self {
        Self::new()
    }
}

impl SelinuxSysOps for LinuxSelinuxOps {
    fn read_runtime_mode(&self) -> Result<SelinuxRuntimeMode, AkrepError> {
        let selinux_root = Path::new("/sys/fs/selinux");
        if !selinux_root.exists() {
            return Ok(SelinuxRuntimeMode::Disabled);
        }
        let enforce_raw = fs::read_to_string(selinux_root.join("enforce"))?;
        match enforce_raw.trim() {
            "1" => Ok(SelinuxRuntimeMode::Enforcing),
            "0" => Ok(SelinuxRuntimeMode::Permissive),
            other => Err(AkrepError::Internal(format!(
                "Unexpected /sys/fs/selinux/enforce value '{}'",
                other
            ))),
        }
    }

    fn read_config(&self) -> Result<SelinuxConfigState, AkrepError> {
        let cfg_path = Path::new("/etc/selinux/config");
        if !cfg_path.exists() {
            return Ok(SelinuxConfigState {
                configured_mode: None,
                policy_type: None,
            });
        }
        let raw = fs::read_to_string(cfg_path)?;
        let mut configured_mode = None;
        let mut policy_type = None;
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some(v) = trimmed.strip_prefix("SELINUX=") {
                configured_mode = Some(v.trim().to_string());
            } else if let Some(v) = trimmed.strip_prefix("SELINUXTYPE=") {
                let candidate = v.trim();
                validate_selinux_policy_type(candidate)?;
                policy_type = Some(candidate.to_string());
            }
        }
        Ok(SelinuxConfigState {
            configured_mode,
            policy_type,
        })
    }

    fn list_booleans(&self) -> Result<Vec<RawBooleanState>, AkrepError> {
        let bool_dir = Path::new("/sys/fs/selinux/booleans");
        if !bool_dir.is_dir() {
            return Ok(Vec::new());
        }
        let cfg = self.read_config()?;
        let policy = cfg.policy_type.as_deref().unwrap_or("targeted");
        let persistent_map = self.read_persistent_map(policy)?;

        let mut list = Vec::new();
        for entry in fs::read_dir(bool_dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok((current, pending)) = Self::parse_sysfs_bool_file(&entry.path()) else {
                continue;
            };
            let persistent = persistent_map.resolve_for(&name, current);
            list.push(RawBooleanState {
                name,
                current,
                pending,
                persistent,
            });
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    fn read_boolean(&self, name: &str) -> Result<Option<RawBooleanState>, AkrepError> {
        validate_selinux_boolean_name(name)?;
        let path = Path::new("/sys/fs/selinux/booleans").join(name);
        if !path.is_file() {
            return Ok(None);
        }
        let (current, pending) = Self::parse_sysfs_bool_file(&path)?;
        let cfg = self.read_config()?;
        let policy = cfg.policy_type.as_deref().unwrap_or("targeted");
        let persistent_map = self.read_persistent_map(policy)?;
        let persistent = persistent_map.resolve_for(name, current);
        Ok(Some(RawBooleanState {
            name: name.to_string(),
            current,
            pending,
            persistent,
        }))
    }

    fn set_boolean(&self, name: &str, value: bool, persistent: bool) -> Result<(), AkrepError> {
        validate_selinux_boolean_name(name)?;
        let bin = resolve_binary(&[
            "/usr/sbin/setsebool",
            "/sbin/setsebool",
            "/usr/bin/setsebool",
        ])
        .ok_or_else(|| {
            AkrepError::BackendUnavailable("setsebool binary is not installed".to_string())
        })?;

        let val_arg = if value { "on" } else { "off" };
        let mut cmd = command_c_locale(bin);
        if persistent {
            cmd.args(["-P", name, val_arg]);
        } else {
            cmd.args([name, val_arg]);
        }
        let out = cmd.output()?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "setsebool failed for '{}': {}",
                name,
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        if persistent {
            self.invalidate_persistent_cache();
        }
        Ok(())
    }

    fn preview_restorecon(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<RestoreconPreviewEntry>, AkrepError> {
        let resolved = resolve_and_validate_restorecon_target(path, recursive)?;
        let bin = resolve_binary(&[
            "/sbin/restorecon",
            "/usr/sbin/restorecon",
            "/usr/bin/restorecon",
        ])
        .ok_or_else(|| {
            AkrepError::BackendUnavailable("restorecon binary is not installed".to_string())
        })?;

        let mut cmd = command_c_locale(bin);
        cmd.args(["-n", "-v", "-h"]);
        if recursive {
            cmd.arg("-R");
        }
        cmd.arg("--").arg(&resolved);
        let out = cmd.output()?;
        if !out.status.success() {
            return Err(AkrepError::IoError(format!(
                "restorecon preview failed: {}",
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(parse_restorecon_output(&combined))
    }

    fn read_file_contexts(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<FileContextEntry>, AkrepError> {
        let paths = walk_restorecon_paths_bounded(path, recursive, MAX_RESTORECON_WALK_ENTRIES)?;
        let mut results = Vec::with_capacity(paths.len());
        for p in paths {
            let ctx = Self::read_single_file_context(&p)?;
            results.push(FileContextEntry {
                path: p.to_string_lossy().to_string(),
                context: ctx,
            });
        }
        results.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(results)
    }

    fn apply_restorecon(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<RestoreconPreviewEntry>, AkrepError> {
        let resolved = resolve_and_validate_restorecon_target(path, recursive)?;
        let bin = resolve_binary(&[
            "/sbin/restorecon",
            "/usr/sbin/restorecon",
            "/usr/bin/restorecon",
        ])
        .ok_or_else(|| {
            AkrepError::BackendUnavailable("restorecon binary is not installed".to_string())
        })?;

        let mut cmd = command_c_locale(bin);
        cmd.args(["-v", "-h"]);
        if recursive {
            cmd.arg("-R");
        }
        cmd.arg("--").arg(&resolved);
        let out = cmd.output()?;
        if !out.status.success() {
            return Err(AkrepError::ApplyFailed(format!(
                "restorecon failed: {}",
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        let combined = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(parse_restorecon_output(&combined))
    }

    fn restore_file_context(&self, path: &Path, context: &str) -> Result<(), AkrepError> {
        let resolved = resolve_and_validate_restorecon_target(path, false)?;
        if context.trim().is_empty() || context.chars().any(|c| c.is_whitespace() || c.is_control())
        {
            return Err(AkrepError::InvalidInput(
                "Invalid SELinux context string for chcon rollback".to_string(),
            ));
        }
        let bin = resolve_binary(&["/usr/bin/chcon", "/bin/chcon", "/usr/sbin/chcon"]).ok_or_else(
            || AkrepError::BackendUnavailable("chcon binary is not installed".to_string()),
        )?;
        let out = command_c_locale(bin)
            .args(["-h", context, "--"])
            .arg(&resolved)
            .output()?;
        if !out.status.success() {
            return Err(AkrepError::RollbackFailed(format!(
                "chcon rollback failed on '{}': {}",
                resolved.display(),
                redact_secrets(String::from_utf8_lossy(&out.stderr).trim())
            )));
        }
        Ok(())
    }

    fn read_audit_avc_lines(&self) -> Result<Vec<String>, AkrepError> {
        extract_avc_lines_from_audit_file(Path::new("/var/log/audit/audit.log"))
    }

    fn read_journal_avc_lines(&self) -> Result<Vec<String>, AkrepError> {
        let Some(journalctl) = resolve_binary(&["/usr/bin/journalctl", "/bin/journalctl"]) else {
            return Ok(Vec::new());
        };

        let mut collected = Vec::new();
        for transport in ["_TRANSPORT=audit", "_TRANSPORT=kernel"] {
            if let Ok(out) = command_c_locale(&journalctl)
                .args([
                    "--output=json",
                    "--no-pager",
                    "--since",
                    "24 hours ago",
                    "-n",
                    "500",
                    transport,
                ])
                .output()
            {
                if out.status.success() {
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    for line in stdout.lines() {
                        let trimmed = line.trim();
                        if !trimmed.is_empty() {
                            collected.push(trimmed.to_string());
                        }
                    }
                }
            }
        }
        Ok(collected)
    }
}

fn extract_audit_stamp(msg: &str) -> Option<(&str, &str)> {
    let mut rest = msg;
    while let Some(idx) = rest.find("audit(") {
        let after = &rest[idx + "audit(".len()..];
        if let Some(end) = after.find(')') {
            let inside = &after[..end];
            if let Some((ts, serial)) = inside.split_once(':') {
                let valid_ts = !ts.is_empty()
                    && ts.contains('.')
                    && ts.bytes().all(|b| b.is_ascii_digit() || b == b'.');
                let valid_serial = !serial.is_empty() && serial.bytes().all(|b| b.is_ascii_digit());
                if valid_ts && valid_serial {
                    return Some((ts, serial));
                }
            }
        }
        rest = after;
    }
    None
}

fn extract_denied_permissions(msg: &str) -> Option<Vec<String>> {
    let idx = msg.find("denied")?;
    let after = msg[idx + "denied".len()..].trim_start();
    let inside_start = after.strip_prefix('{')?;
    let end = inside_start.find('}')?;
    let mut perms: Vec<String> = inside_start[..end]
        .split_whitespace()
        .map(ToString::to_string)
        .collect();
    perms.sort();
    perms.dedup();
    if perms.is_empty() {
        None
    } else {
        Some(perms)
    }
}

fn extract_audit_kv(msg: &str, key: &str) -> Option<String> {
    let needle = format!("{}=", key);
    let mut offset = 0usize;
    while let Some(rel) = msg[offset..].find(&needle) {
        let abs = offset + rel;
        let boundary_ok = abs == 0
            || msg[..abs]
                .chars()
                .next_back()
                .map(|c| c.is_whitespace())
                .unwrap_or(true);
        let val_start = abs + needle.len();
        if boundary_ok && val_start <= msg.len() {
            let rem = &msg[val_start..];
            if let Some(quoted) = rem.strip_prefix('"') {
                if let Some(q_end) = quoted.find('"') {
                    return Some(quoted[..q_end].to_string());
                }
            } else {
                let end = rem.find(char::is_whitespace).unwrap_or(rem.len());
                if end > 0 {
                    return Some(rem[..end].to_string());
                }
            }
        }
        offset = val_start;
    }
    None
}

#[derive(Debug, Clone)]
struct ParsedAvcEvent {
    dedup_key: String,
    timestamp_rfc3339: String,
    app_comm: String,
    source_domain: String,
    target_type: String,
    target_class: String,
    permissions: Vec<String>,
    sample_path: Option<String>,
}

fn extract_selinux_type_field(ctx: &str) -> String {
    let parts: Vec<&str> = ctx.split(':').collect();
    if parts.len() >= 3 && !parts[2].is_empty() {
        parts[2].to_string()
    } else {
        ctx.to_string()
    }
}

fn parse_single_avc_message(
    raw_msg: &str,
    fallback_audit_id: Option<&str>,
    fallback_realtime_usec: Option<&str>,
) -> Option<ParsedAvcEvent> {
    if !is_avc_denied_line(raw_msg) {
        return None;
    }
    let permissions = extract_denied_permissions(raw_msg)?;
    let app_comm = extract_audit_kv(raw_msg, "comm").unwrap_or_else(|| "unknown".to_string());
    let source_domain = extract_audit_kv(raw_msg, "scontext")
        .map(|s| extract_selinux_type_field(&s))
        .unwrap_or_else(|| "unknown_t".to_string());
    let target_type = extract_audit_kv(raw_msg, "tcontext")
        .map(|s| extract_selinux_type_field(&s))
        .unwrap_or_else(|| "unknown_t".to_string());
    let target_class = extract_audit_kv(raw_msg, "tclass").unwrap_or_else(|| "unknown".to_string());
    let sample_path =
        extract_audit_kv(raw_msg, "path").filter(|p| validate_restorecon_path(p).is_ok());

    let (stamp_id, timestamp_rfc3339) = if let Some((ts_str, serial)) = extract_audit_stamp(raw_msg)
    {
        let secs = ts_str
            .split('.')
            .next()
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);
        let dt = DateTime::from_timestamp(secs, 0)
            .unwrap_or_else(|| DateTime::from_timestamp(0, 0).expect("unix epoch"))
            .to_rfc3339();
        (format!("{}:{}", ts_str, serial), dt)
    } else {
        let dt = fallback_realtime_usec
            .and_then(|u| u.parse::<i64>().ok())
            .and_then(|usec| DateTime::from_timestamp(usec / 1_000_000, 0))
            .unwrap_or_else(|| DateTime::from_timestamp(0, 0).expect("unix epoch"))
            .to_rfc3339();
        let id = fallback_audit_id
            .map(|a| format!("journal_audit:{}", a))
            .unwrap_or_else(|| sha256_hex(raw_msg.trim().as_bytes())[..24].to_string());
        (id, dt)
    };

    let dedup_key = format!(
        "{}|{}|{}|{}|{}",
        stamp_id,
        source_domain,
        target_type,
        target_class,
        permissions.join(",")
    );

    Some(ParsedAvcEvent {
        dedup_key,
        timestamp_rfc3339,
        app_comm,
        source_domain,
        target_type,
        target_class,
        permissions,
        sample_path,
    })
}

fn parse_journal_line(line: &str) -> Option<ParsedAvcEvent> {
    if line.trim_start().starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(line).ok()?;
        let msg = match v.get("MESSAGE")? {
            serde_json::Value::String(s) => s.clone(),
            serde_json::Value::Array(bytes) => {
                let raw: Vec<u8> = bytes
                    .iter()
                    .filter_map(|b| b.as_u64().map(|n| n as u8))
                    .collect();
                String::from_utf8_lossy(&raw).into_owned()
            }
            _ => return None,
        };
        let audit_id = v.get("_AUDIT_ID").and_then(|x| x.as_str());
        let rt_ts = v.get("__REALTIME_TIMESTAMP").and_then(|x| x.as_str());
        parse_single_avc_message(&msg, audit_id, rt_ts)
    } else {
        parse_single_avc_message(line, None, None)
    }
}

fn match_safe_boolean_for_denial(
    source_domain: &str,
    target_type: &str,
    target_class: &str,
    permissions: &[String],
    known_booleans: &HashSet<String>,
) -> Option<(String, RiskLevel)> {
    let has_perm = |p: &str| permissions.iter().any(|x| x == p);

    let candidate: Option<&str> = if source_domain.starts_with("httpd_")
        && target_class == "tcp_socket"
        && has_perm("name_connect")
    {
        if matches!(
            target_type,
            "postgresql_port_t" | "mysqld_port_t" | "redis_port_t" | "mssql_port_t"
        ) {
            Some("httpd_can_network_connect_db")
        } else {
            Some("httpd_can_network_connect")
        }
    } else if source_domain.starts_with("httpd_")
        && matches!(target_type, "user_home_t" | "user_home_dir_t")
    {
        Some("httpd_enable_homedirs")
    } else if source_domain.starts_with("smbd_")
        && matches!(target_type, "user_home_t" | "user_home_dir_t")
    {
        Some("samba_enable_home_dirs")
    } else if source_domain.starts_with("container_") && target_type == "cgroup_t" {
        Some("container_manage_cgroup")
    } else if source_domain.starts_with("container_")
        && matches!(target_class, "chr_file" | "blk_file")
    {
        Some("container_use_devices")
    } else if (source_domain.starts_with("svirt_") || source_domain.starts_with("virtqemud_"))
        && target_type == "usb_device_t"
    {
        Some("virt_use_usb")
    } else if (source_domain.starts_with("svirt_") || source_domain.starts_with("virtqemud_"))
        && target_type == "nfs_t"
    {
        Some("virt_use_nfs")
    } else if (source_domain.starts_with("svirt_") || source_domain.starts_with("virtqemud_"))
        && target_type == "cifs_t"
    {
        Some("virt_use_samba")
    } else {
        None
    };

    let bool_name = candidate?;
    if !known_booleans.is_empty() && !known_booleans.contains(bool_name) {
        return None;
    }
    let (_, meta) = lookup_boolean_meta(bool_name);
    if matches!(meta.risk, RiskLevel::High | RiskLevel::Critical) {
        return None;
    }
    Some((bool_name.to_string(), meta.risk))
}

fn classify_avc_severity(target_class: &str, permissions: &[String]) -> RiskLevel {
    if permissions
        .iter()
        .any(|p| matches!(p.as_str(), "execheap" | "execstack" | "mmap_zero"))
    {
        return RiskLevel::Critical;
    }
    if permissions.iter().any(|p| {
        matches!(
            p.as_str(),
            "execmod" | "ptrace" | "sys_admin" | "module_load"
        )
    }) {
        return RiskLevel::High;
    }
    if matches!(
        target_class,
        "tcp_socket" | "udp_socket" | "chr_file" | "blk_file"
    ) {
        return RiskLevel::Medium;
    }
    RiskLevel::Low
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BooleanSnapshotFile {
    name: String,
    prev_runtime: bool,
    prev_persistent: Option<bool>,
    was_persistent_op: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RestoreconSnapshotFile {
    target_path: String,
    recursive: bool,
    previous_contexts: Vec<FileContextEntry>,
}

fn format_opt_persistent(p: Option<bool>) -> &'static str {
    match p {
        Some(true) => "on",
        Some(false) => "off",
        None => "policy_default",
    }
}

#[derive(Clone)]
pub struct SelinuxAdapter {
    ops: Arc<dyn SelinuxSysOps>,
}

impl SelinuxAdapter {
    pub fn new(ops: Arc<dyn SelinuxSysOps>) -> Self {
        Self { ops }
    }

    pub fn new_linux() -> Self {
        Self::new(Arc::new(LinuxSelinuxOps::new()))
    }

    pub fn ops(&self) -> &Arc<dyn SelinuxSysOps> {
        &self.ops
    }

    fn ensure_selinux_enabled(&self) -> Result<SelinuxRuntimeMode, AkrepError> {
        let mode = self.ops.read_runtime_mode()?;
        if mode == SelinuxRuntimeMode::Disabled {
            return Err(AkrepError::BackendUnavailable(
                "SELinux is disabled on this system; mutations are unavailable".to_string(),
            ));
        }
        Ok(mode)
    }

    fn boolean_fingerprint(state: &RawBooleanState) -> StateFingerprint {
        let p_str = match state.persistent {
            Some(true) => "some_true",
            Some(false) => "some_false",
            None => "unknown_default",
        };
        let canonical = format!("{}|{}", state.current, p_str);
        StateFingerprint {
            target: format!("selinux.boolean.{}", state.name),
            sha256: sha256_hex(canonical.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn contexts_fingerprint(target_path: &str, contexts: &[FileContextEntry]) -> StateFingerprint {
        let mut sorted = contexts.to_vec();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));
        let mut buf = String::new();
        for item in &sorted {
            buf.push_str(&item.path);
            buf.push('=');
            buf.push_str(&item.context);
            buf.push('\n');
        }
        StateFingerprint {
            target: format!("selinux.context.{}", target_path),
            sha256: sha256_hex(buf.as_bytes()),
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn collect_grouped_avc_denials(
        &self,
        known_booleans: &HashSet<String>,
    ) -> Result<Vec<AvcDenialGroup>, AkrepError> {
        let audit_lines = self.ops.read_audit_avc_lines()?;
        let journal_lines = self.ops.read_journal_avc_lines()?;

        let mut seen_dedup: HashSet<String> = HashSet::new();
        let mut unique_events: Vec<ParsedAvcEvent> = Vec::new();

        for line in &audit_lines {
            if let Some(ev) = parse_single_avc_message(line, None, None) {
                if seen_dedup.insert(ev.dedup_key.clone()) {
                    unique_events.push(ev);
                }
            }
        }
        for line in &journal_lines {
            if let Some(ev) = parse_journal_line(line) {
                if seen_dedup.insert(ev.dedup_key.clone()) {
                    unique_events.push(ev);
                }
            }
        }

        type GroupKey = (String, String, String, String);
        struct Accumulator {
            permissions: BTreeSet<String>,
            count: usize,
            last_seen: String,
            sample_path: Option<String>,
        }

        let mut grouped: BTreeMap<GroupKey, Accumulator> = BTreeMap::new();
        for ev in unique_events {
            let key = (
                ev.app_comm.clone(),
                ev.source_domain.clone(),
                ev.target_type.clone(),
                ev.target_class.clone(),
            );
            let entry = grouped.entry(key).or_insert_with(|| Accumulator {
                permissions: BTreeSet::new(),
                count: 0,
                last_seen: ev.timestamp_rfc3339.clone(),
                sample_path: None,
            });
            entry.count += 1;
            for p in ev.permissions {
                entry.permissions.insert(p);
            }
            if ev.timestamp_rfc3339 > entry.last_seen {
                entry.last_seen = ev.timestamp_rfc3339;
            }
            if entry.sample_path.is_none() && ev.sample_path.is_some() {
                entry.sample_path = ev.sample_path;
            }
        }

        let mut out = Vec::new();
        for ((app_comm, source_domain, target_type, target_class), acc) in grouped {
            let permissions: Vec<String> = acc.permissions.into_iter().collect();
            let sig_seed = format!(
                "{}|{}|{}|{}",
                app_comm, source_domain, target_type, target_class
            );
            let signature_id = sha256_hex(sig_seed.as_bytes())[..16].to_string();
            let severity = classify_avc_severity(&target_class, &permissions);

            let mut suggested_fix = None;
            if let Some(ref p_str) = acc.sample_path {
                if matches!(
                    target_class.as_str(),
                    "file" | "dir" | "lnk_file" | "sock_file" | "fifo_file"
                ) {
                    if let Ok(relabels) = self.ops.preview_restorecon(Path::new(p_str), false) {
                        if let Some(first) = relabels.first() {
                            suggested_fix = Some(SuggestedSelinuxFix::Restorecon {
                                target_path: p_str.clone(),
                                recursive: target_class == "dir",
                                expected_context: Some(first.to_context.clone()),
                            });
                        }
                    }
                }
            }

            if suggested_fix.is_none() {
                if let Some((bool_name, bool_risk)) = match_safe_boolean_for_denial(
                    &source_domain,
                    &target_type,
                    &target_class,
                    &permissions,
                    known_booleans,
                ) {
                    suggested_fix = Some(SuggestedSelinuxFix::SetBoolean {
                        boolean_name: bool_name,
                        recommended_value: true,
                        risk: bool_risk,
                    });
                }
            }

            let suggested_remediation_kind = suggested_fix.unwrap_or_else(|| {
                SuggestedSelinuxFix::ManualPolicyReview {
                    guidance_tr: "Otomatik genis kapsamli politika uretimi guvenlik nedeniyle engellenmistir; hedef dizin etiketlerini ve servis yapilandirmasini yonetici olarak inceleyin.".to_string(),
                    guidance_en: "Automatic broad policy generation is disabled by security policy; review target file labels and service configuration manually as an administrator.".to_string(),
                }
            });

            let perms_joined = permissions.join(", ");
            let explanation_tr = format!(
                "'{}' sureci ({}) '{}' etiketli '{}' nesnesi uzerinde {{{}}} izni isterken SELinux tarafindan engellendi ({} kez).",
                app_comm, source_domain, target_type, target_class, perms_joined, acc.count
            );
            let explanation_en = format!(
                "Process '{}' ({}) was denied {{{}}} access on '{}' object labeled '{}' ({} occurrence(s)).",
                app_comm, source_domain, perms_joined, target_class, target_type, acc.count
            );

            out.push(AvcDenialGroup {
                signature_id,
                app_comm,
                source_domain,
                target_type,
                target_class,
                permissions,
                count: acc.count,
                last_seen: acc.last_seen,
                sample_path: acc.sample_path,
                severity,
                suggested_remediation_kind,
                explanation_tr,
                explanation_en,
            });
        }

        out.sort_by(|a, b| {
            b.last_seen
                .cmp(&a.last_seen)
                .then_with(|| a.signature_id.cmp(&b.signature_id))
        });
        Ok(out)
    }

    pub fn diagnose(
        &self,
        query: &SelinuxDiagnoseQuery,
    ) -> Result<SelinuxDiagnoseResponse, AkrepError> {
        query.validate_red_lines("{}")?;

        let action_mode = query
            .action
            .as_deref()
            .map(|s| s.trim().to_ascii_lowercase())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "full".to_string());

        let want_booleans = match action_mode.as_str() {
            "status" | "avc" | "inspect_path" => false,
            "booleans" => true,
            _ => query.include_booleans,
        };
        let want_avc = match action_mode.as_str() {
            "status" | "booleans" | "inspect_path" => false,
            "avc" => true,
            _ => query.include_avc,
        };
        let want_inspect_path = match action_mode.as_str() {
            "status" | "booleans" | "avc" => false,
            "inspect_path" => true,
            _ => query.inspect_path.is_some(),
        };

        let runtime_mode = self.ops.read_runtime_mode()?;
        let cfg = self.ops.read_config()?;

        let raw_bools =
            if runtime_mode == SelinuxRuntimeMode::Disabled || (!want_booleans && !want_avc) {
                Vec::new()
            } else {
                self.ops.list_booleans()?
            };

        let known_bool_names: HashSet<String> = raw_bools.iter().map(|b| b.name.clone()).collect();

        let booleans = if want_booleans {
            let filter = query
                .boolean_filter
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty());
            raw_bools
                .into_iter()
                .filter(|b| match filter {
                    Some(sub) => b.name.contains(sub),
                    None => true,
                })
                .map(|b| {
                    let (cataloged, meta) = lookup_boolean_meta(&b.name);
                    SelinuxBooleanEntry {
                        name: b.name,
                        current: b.current,
                        pending: b.pending,
                        persistent: b.persistent.unwrap_or(b.current),
                        persistent_known: b.persistent.is_some(),
                        cataloged,
                        category: meta.category,
                        risk: meta.risk,
                        summary_tr: meta.summary_tr,
                        summary_en: meta.summary_en,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };

        let avc_groups = if want_avc {
            self.collect_grouped_avc_denials(&known_bool_names)?
        } else {
            Vec::new()
        };

        let (current_contexts, restorecon_preview) = if want_inspect_path {
            let target = query.inspect_path.as_deref().ok_or_else(|| {
                AkrepError::InvalidInput(
                    "inspect_path must be provided for action 'inspect_path'".to_string(),
                )
            })?;
            validate_restorecon_path(target)?;
            let path = Path::new(target);
            let cur = self.ops.read_file_contexts(path, query.recursive)?;
            let prev = self.ops.preview_restorecon(path, query.recursive)?;
            (cur, prev)
        } else {
            (Vec::new(), Vec::new())
        };

        Ok(SelinuxDiagnoseResponse {
            runtime_mode,
            configured_mode: cfg.configured_mode,
            policy_type: cfg.policy_type,
            booleans,
            avc_groups,
            restorecon_preview,
            current_contexts,
        })
    }

    fn preview_sync(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        req.validate_red_lines()?;
        self.ensure_selinux_enabled()?;

        match req {
            ActionRequest::SelinuxSetBoolean {
                name,
                value,
                persistent,
            } => {
                validate_selinux_boolean_name(name)?;
                let state = self.ops.read_boolean(name)?.ok_or_else(|| {
                    AkrepError::InvalidInput(format!(
                        "SELinux boolean '{}' does not exist in active policy",
                        name
                    ))
                })?;

                let (cataloged, meta) = lookup_boolean_meta(name);
                let base_risk = if *persistent {
                    combine_risk(meta.risk, RiskLevel::Medium)
                } else {
                    meta.risk
                };

                let mut warnings_tr = Vec::new();
                let mut warnings_en = Vec::new();
                if !cataloged {
                    warnings_tr.push(format!(
                        "'{}' yerlesik katalogda yer almayan bir politika anahtaridir; etki alanini dogrulayin.",
                        name
                    ));
                    warnings_en.push(format!(
                        "'{}' is an uncataloged policy boolean; verify its domain impact before applying.",
                        name
                    ));
                }
                if matches!(meta.risk, RiskLevel::High | RiskLevel::Critical) && *value {
                    warnings_tr.push(format!(
                        "Yuksek riskli SELinux anahtari ('{}') etkinlestiriliyor: {}",
                        name, meta.summary_tr
                    ));
                    warnings_en.push(format!(
                        "Enabling high-risk SELinux boolean ('{}'): {}",
                        name, meta.summary_en
                    ));
                }

                let desired_persistent = if *persistent {
                    Some(*value)
                } else {
                    state.persistent
                };
                let diff = PlanDiffItem {
                    target: format!("selinux.boolean.{}", name),
                    current_value: format!(
                        "runtime={}, persistent={}",
                        if state.current { "on" } else { "off" },
                        format_opt_persistent(state.persistent)
                    ),
                    desired_value: format!(
                        "runtime={}, persistent={}",
                        if *value { "on" } else { "off" },
                        format_opt_persistent(desired_persistent)
                    ),
                    description_tr: meta.summary_tr,
                    description_en: meta.summary_en,
                };

                Ok(PreviewOutcome {
                    risk: base_risk,
                    diffs: vec![diff],
                    pre_fingerprints: vec![Self::boolean_fingerprint(&state)],
                    warnings_tr,
                    warnings_en,
                })
            }
            ActionRequest::SelinuxRestorecon {
                target_path,
                recursive,
            } => {
                validate_restorecon_path(target_path)?;
                let path = Path::new(target_path);
                let contexts = self.ops.read_file_contexts(path, *recursive)?;
                let preview_changes = self.ops.preview_restorecon(path, *recursive)?;

                let diffs = if preview_changes.is_empty() {
                    vec![PlanDiffItem {
                        target: target_path.clone(),
                        current_value: "policy_default".to_string(),
                        desired_value: "policy_default".to_string(),
                        description_tr: format!(
                            "'{}' yolu zaten varsayilan SELinux dosya baglami ile uyumlu.",
                            target_path
                        ),
                        description_en: format!(
                            "Path '{}' already matches the default SELinux file context policy.",
                            target_path
                        ),
                    }]
                } else {
                    preview_changes
                        .iter()
                        .map(|c| PlanDiffItem {
                            target: c.path.clone(),
                            current_value: c.from_context.clone(),
                            desired_value: c.to_context.clone(),
                            description_tr: format!(
                                "'{}' dosyasinin SELinux etiketi varsayilan politikaya onarilacak.",
                                c.path
                            ),
                            description_en: format!(
                                "Restore default SELinux security context on '{}'.",
                                c.path
                            ),
                        })
                        .collect()
                };

                Ok(PreviewOutcome {
                    risk: RiskLevel::Low,
                    diffs,
                    pre_fingerprints: vec![Self::contexts_fingerprint(target_path, &contexts)],
                    warnings_tr: Vec::new(),
                    warnings_en: Vec::new(),
                })
            }
            _ => Err(AkrepError::InvalidInput(
                "SelinuxAdapter received non-SELinux ActionRequest".to_string(),
            )),
        }
    }

    fn apply_sync(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        req.validate_red_lines()?;
        self.ensure_selinux_enabled()?;

        match req {
            ActionRequest::SelinuxSetBoolean {
                name,
                value,
                persistent,
            } => {
                validate_selinux_boolean_name(name)?;
                let before = self.ops.read_boolean(name)?.ok_or_else(|| {
                    AkrepError::InvalidInput(format!(
                        "SELinux boolean '{}' does not exist in active policy",
                        name
                    ))
                })?;

                let snap = BooleanSnapshotFile {
                    name: name.clone(),
                    prev_runtime: before.current,
                    prev_persistent: before.persistent,
                    was_persistent_op: *persistent,
                };
                write_json_atomic_0600(&snapshot_dir.join("selinux_bool.json"), &snap)?;

                self.ops.set_boolean(name, *value, *persistent)?;

                let desired_persistent: Option<bool> = if *persistent {
                    Some(*value)
                } else {
                    before.persistent
                };
                Ok(ApplyOutcome {
                    pre_snapshot: serde_json::to_value(&snap)
                        .map_err(|e| AkrepError::Internal(e.to_string()))?,
                    target_state: serde_json::json!({
                        "name": name,
                        "desired_runtime": value,
                        "desired_persistent": desired_persistent,
                    }),
                    summary: format!(
                        "SELinux boolean '{}' set to {} (persistent={})",
                        name,
                        if *value { "on" } else { "off" },
                        persistent
                    ),
                })
            }
            ActionRequest::SelinuxRestorecon {
                target_path,
                recursive,
            } => {
                validate_restorecon_path(target_path)?;
                let path = Path::new(target_path);
                let prev_contexts = self.ops.read_file_contexts(path, *recursive)?;
                let snap = RestoreconSnapshotFile {
                    target_path: target_path.clone(),
                    recursive: *recursive,
                    previous_contexts: prev_contexts,
                };
                write_json_atomic_0600(&snapshot_dir.join("selinux_contexts.json"), &snap)?;

                let relabeled = self.ops.apply_restorecon(path, *recursive)?;
                Ok(ApplyOutcome {
                    pre_snapshot: serde_json::to_value(&snap)
                        .map_err(|e| AkrepError::Internal(e.to_string()))?,
                    target_state: serde_json::json!({
                        "target_path": target_path,
                        "recursive": recursive,
                        "relabeled_count": relabeled.len(),
                    }),
                    summary: format!(
                        "Restored SELinux file contexts on '{}' ({} relabeled)",
                        target_path,
                        relabeled.len()
                    ),
                })
            }
            _ => Err(AkrepError::InvalidInput(
                "SelinuxAdapter received non-SELinux ActionRequest".to_string(),
            )),
        }
    }

    fn verify_sync(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        match req {
            ActionRequest::SelinuxSetBoolean {
                name,
                value,
                persistent,
            } => {
                let after = self.ops.read_boolean(name)?.ok_or_else(|| {
                    AkrepError::VerifyFailed(format!(
                        "SELinux boolean '{}' disappeared after apply",
                        name
                    ))
                })?;
                if after.current != *value || after.pending != *value {
                    return Err(AkrepError::VerifyFailed(format!(
                        "SELinux boolean '{}' runtime state mismatch (expected {}, got current={} pending={})",
                        name, value, after.current, after.pending
                    )));
                }
                let expected_persistent: Option<bool> = expected
                    .target_state
                    .get("desired_persistent")
                    .and_then(|v| v.as_bool());
                if *persistent && after.persistent != Some(*value) {
                    return Err(AkrepError::VerifyFailed(format!(
                        "SELinux boolean '{}' persistent state mismatch (expected Some({}), got {:?})",
                        name, value, after.persistent
                    )));
                }
                if !*persistent && after.persistent != expected_persistent {
                    return Err(AkrepError::VerifyFailed(format!(
                        "SELinux boolean '{}' unexpected persistent policy mutation (expected {:?}, got {:?})",
                        name, expected_persistent, after.persistent
                    )));
                }
                Ok(vec![Self::boolean_fingerprint(&after)])
            }
            ActionRequest::SelinuxRestorecon {
                target_path,
                recursive,
            } => {
                let path = Path::new(target_path);
                let remaining = self.ops.preview_restorecon(path, *recursive)?;
                if !remaining.is_empty() {
                    return Err(AkrepError::VerifyFailed(format!(
                        "restorecon verification failed: {} path(s) still differ from policy default",
                        remaining.len()
                    )));
                }
                let after_contexts = self.ops.read_file_contexts(path, *recursive)?;
                Ok(vec![Self::contexts_fingerprint(
                    target_path,
                    &after_contexts,
                )])
            }
            _ => Err(AkrepError::InvalidInput(
                "SelinuxAdapter received non-SELinux ActionRequest".to_string(),
            )),
        }
    }

    fn current_fingerprints_sync(
        &self,
        req: &ActionRequest,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        self.ensure_selinux_enabled()?;
        match req {
            ActionRequest::SelinuxSetBoolean { name, .. } => {
                let cur = self.ops.read_boolean(name)?.ok_or_else(|| {
                    AkrepError::InvalidInput(format!("SELinux boolean '{}' does not exist", name))
                })?;
                Ok(vec![Self::boolean_fingerprint(&cur)])
            }
            ActionRequest::SelinuxRestorecon {
                target_path,
                recursive,
            } => {
                let contexts = self
                    .ops
                    .read_file_contexts(Path::new(target_path), *recursive)?;
                Ok(vec![Self::contexts_fingerprint(target_path, &contexts)])
            }
            _ => Err(AkrepError::InvalidInput(
                "SelinuxAdapter received non-SELinux ActionRequest".to_string(),
            )),
        }
    }

    fn rollback_sync(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        match &record.request {
            ActionRequest::SelinuxSetBoolean { name, .. } => {
                let snap_path = snapshot_dir.join("selinux_bool.json");
                let snap: BooleanSnapshotFile = if snap_path.is_file() {
                    let raw = fs::read_to_string(&snap_path)?;
                    serde_json::from_str(&raw)
                        .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
                } else {
                    serde_json::from_value(record.pre_snapshot.clone()).map_err(|e| {
                        AkrepError::RollbackFailed(format!(
                            "Missing SELinux boolean rollback snapshot for '{}': {}",
                            name, e
                        ))
                    })?
                };

                if snap.was_persistent_op {
                    let target_p = snap.prev_persistent.unwrap_or(snap.prev_runtime);
                    self.ops.set_boolean(&snap.name, target_p, true)?;
                    if snap.prev_runtime != target_p {
                        self.ops.set_boolean(&snap.name, snap.prev_runtime, false)?;
                    }
                } else {
                    self.ops.set_boolean(&snap.name, snap.prev_runtime, false)?;
                }

                let restored = self.ops.read_boolean(&snap.name)?.ok_or_else(|| {
                    AkrepError::RollbackFailed(format!(
                        "SELinux boolean '{}' missing after rollback",
                        snap.name
                    ))
                })?;
                let expected_restored_p = if snap.was_persistent_op {
                    Some(snap.prev_persistent.unwrap_or(snap.prev_runtime))
                } else {
                    snap.prev_persistent
                };
                if restored.current != snap.prev_runtime
                    || restored.persistent != expected_restored_p
                {
                    return Err(AkrepError::RollbackFailed(format!(
                        "SELinux boolean '{}' rollback state verification failed",
                        snap.name
                    )));
                }

                Ok(format!(
                    "Restored SELinux boolean '{}' to runtime={}, persistent={}",
                    snap.name,
                    if snap.prev_runtime { "on" } else { "off" },
                    format_opt_persistent(expected_restored_p)
                ))
            }
            ActionRequest::SelinuxRestorecon { target_path, .. } => {
                let snap_path = snapshot_dir.join("selinux_contexts.json");
                let snap: RestoreconSnapshotFile = if snap_path.is_file() {
                    let raw = fs::read_to_string(&snap_path)?;
                    serde_json::from_str(&raw)
                        .map_err(|e| AkrepError::RollbackFailed(e.to_string()))?
                } else {
                    serde_json::from_value(record.pre_snapshot.clone()).map_err(|e| {
                        AkrepError::RollbackFailed(format!(
                            "Missing restorecon snapshot for '{}': {}",
                            target_path, e
                        ))
                    })?
                };

                for item in &snap.previous_contexts {
                    self.ops
                        .restore_file_context(Path::new(&item.path), &item.context)?;
                }

                // Re-verify restored file contexts match pre-apply snapshot
                let restored_contexts = self
                    .ops
                    .read_file_contexts(Path::new(&snap.target_path), snap.recursive)?;
                let restored_map: HashMap<&str, &str> = restored_contexts
                    .iter()
                    .map(|c| (c.path.as_str(), c.context.as_str()))
                    .collect();
                for expected_item in &snap.previous_contexts {
                    if restored_map.get(expected_item.path.as_str()).copied()
                        != Some(expected_item.context.as_str())
                    {
                        return Err(AkrepError::RollbackFailed(format!(
                            "File context on '{}' did not match pre-snapshot after rollback",
                            expected_item.path
                        )));
                    }
                }

                Ok(format!(
                    "Restored {} SELinux file context(s) under '{}'",
                    snap.previous_contexts.len(),
                    snap.target_path
                ))
            }
            _ => Err(AkrepError::RollbackFailed(
                "SelinuxAdapter cannot roll back non-SELinux transaction".to_string(),
            )),
        }
    }
}

fn combine_risk(a: RiskLevel, b: RiskLevel) -> RiskLevel {
    let rank = |r: RiskLevel| -> u8 {
        match r {
            RiskLevel::Low => 0,
            RiskLevel::Medium => 1,
            RiskLevel::High => 2,
            RiskLevel::Critical => 3,
        }
    };
    if rank(a) >= rank(b) {
        a
    } else {
        b
    }
}

async fn run_blocking<F, T>(f: F) -> Result<T, AkrepError>
where
    F: FnOnce() -> Result<T, AkrepError> + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AkrepError::Internal(format!("blocking SELinux task panicked: {}", e)))?
}

#[async_trait]
impl MutationBackend for SelinuxAdapter {
    async fn preview(&self, req: &ActionRequest) -> Result<PreviewOutcome, AkrepError> {
        let this = self.clone();
        let req_cloned = req.clone();
        run_blocking(move || this.preview_sync(&req_cloned)).await
    }

    async fn apply(
        &self,
        req: &ActionRequest,
        snapshot_dir: &Path,
    ) -> Result<ApplyOutcome, AkrepError> {
        let this = self.clone();
        let req_cloned = req.clone();
        let dir_cloned = snapshot_dir.to_path_buf();
        run_blocking(move || this.apply_sync(&req_cloned, &dir_cloned)).await
    }

    async fn verify(
        &self,
        req: &ActionRequest,
        expected: &ApplyOutcome,
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let this = self.clone();
        let req_cloned = req.clone();
        let exp_cloned = expected.clone();
        run_blocking(move || this.verify_sync(&req_cloned, &exp_cloned)).await
    }

    async fn current_fingerprints(
        &self,
        req: &ActionRequest,
        _post_fps: &[StateFingerprint],
    ) -> Result<Vec<StateFingerprint>, AkrepError> {
        let this = self.clone();
        let req_cloned = req.clone();
        run_blocking(move || this.current_fingerprints_sync(&req_cloned)).await
    }

    async fn rollback(
        &self,
        record: &TransactionRecord,
        snapshot_dir: &Path,
    ) -> Result<String, AkrepError> {
        let this = self.clone();
        let rec_cloned = record.clone();
        let dir_cloned = snapshot_dir.to_path_buf();
        run_blocking(move || this.rollback_sync(&rec_cloned, &dir_cloned)).await
    }

    async fn diagnose_selinux(
        &self,
        query: &SelinuxDiagnoseQuery,
    ) -> Result<SelinuxDiagnoseResponse, AkrepError> {
        let this = self.clone();
        let q_cloned = query.clone();
        run_blocking(move || this.diagnose(&q_cloned)).await
    }
}

#[derive(Debug, Clone)]
struct MockBooleanData {
    name: String,
    current: bool,
    pending: bool,
    base_default: bool,
    local_override: Option<bool>,
}

impl MockBooleanData {
    fn to_raw_state(&self, local_overrides_only: bool) -> RawBooleanState {
        let persistent = if local_overrides_only {
            self.local_override
        } else {
            Some(self.local_override.unwrap_or(self.base_default))
        };
        RawBooleanState {
            name: self.name.clone(),
            current: self.current,
            pending: self.pending,
            persistent,
        }
    }
}

#[derive(Debug, Clone)]
struct MockFileLabel {
    current_context: String,
    policy_default_context: String,
}

#[derive(Debug)]
struct MockSelinuxInner {
    runtime_mode: SelinuxRuntimeMode,
    configured_mode: Option<String>,
    policy_type: Option<String>,
    local_overrides_only: bool,
    booleans: BTreeMap<String, MockBooleanData>,
    files: BTreeMap<String, MockFileLabel>,
    audit_avc_lines: Vec<String>,
    journal_avc_lines: Vec<String>,
    recorded_commands: Vec<Vec<String>>,
    simulate_stale_verify: bool,
}

pub struct MockSelinuxOps {
    inner: Mutex<MockSelinuxInner>,
}

impl Default for MockSelinuxOps {
    fn default() -> Self {
        Self::new_fedora_enforcing()
    }
}

impl MockSelinuxOps {
    pub fn new_fedora_enforcing() -> Self {
        let mut booleans = BTreeMap::new();
        for entry in CURATED_SELINUX_BOOLEANS {
            let default_on = matches!(entry.name, "virt_use_kvm" | "selinuxuser_rw_noexattrfile");
            booleans.insert(
                entry.name.to_string(),
                MockBooleanData {
                    name: entry.name.to_string(),
                    current: default_on,
                    pending: default_on,
                    base_default: default_on,
                    local_override: None,
                },
            );
        }

        let mut files = BTreeMap::new();
        files.insert(
            "/srv/www/index.html".to_string(),
            MockFileLabel {
                current_context: "unconfined_u:object_r:user_tmp_t:s0".to_string(),
                policy_default_context: "system_u:object_r:httpd_sys_content_t:s0".to_string(),
            },
        );

        Self {
            inner: Mutex::new(MockSelinuxInner {
                runtime_mode: SelinuxRuntimeMode::Enforcing,
                configured_mode: Some("enforcing".to_string()),
                policy_type: Some("targeted".to_string()),
                local_overrides_only: false,
                booleans,
                files,
                audit_avc_lines: Vec::new(),
                journal_avc_lines: Vec::new(),
                recorded_commands: Vec::new(),
                simulate_stale_verify: false,
            }),
        }
    }

    pub fn set_runtime_mode(&self, mode: SelinuxRuntimeMode) {
        self.inner.lock().expect("mock selinux mutex").runtime_mode = mode;
    }

    pub fn set_config(&self, configured_mode: Option<&str>, policy_type: Option<&str>) {
        let mut g = self.inner.lock().expect("mock selinux mutex");
        g.configured_mode = configured_mode.map(ToString::to_string);
        g.policy_type = policy_type.map(ToString::to_string);
    }

    pub fn set_local_overrides_only_mode(&self, enabled: bool) {
        self.inner
            .lock()
            .expect("mock selinux mutex")
            .local_overrides_only = enabled;
    }

    pub fn insert_boolean(&self, name: &str, current: bool, persistent: bool) {
        self.inner
            .lock()
            .expect("mock selinux mutex")
            .booleans
            .insert(
                name.to_string(),
                MockBooleanData {
                    name: name.to_string(),
                    current,
                    pending: current,
                    base_default: persistent,
                    local_override: None,
                },
            );
    }

    pub fn set_external_boolean_drift(&self, name: &str, current: bool, persistent: bool) {
        let mut g = self.inner.lock().expect("mock selinux mutex");
        if let Some(entry) = g.booleans.get_mut(name) {
            entry.current = current;
            entry.pending = current;
            entry.local_override = Some(persistent);
        } else {
            g.booleans.insert(
                name.to_string(),
                MockBooleanData {
                    name: name.to_string(),
                    current,
                    pending: current,
                    base_default: persistent,
                    local_override: Some(persistent),
                },
            );
        }
    }

    pub fn insert_file_context(
        &self,
        path: &str,
        current_context: &str,
        policy_default_context: &str,
    ) {
        self.inner.lock().expect("mock selinux mutex").files.insert(
            path.to_string(),
            MockFileLabel {
                current_context: current_context.to_string(),
                policy_default_context: policy_default_context.to_string(),
            },
        );
    }

    pub fn set_avc_sources(&self, audit_lines: Vec<String>, journal_lines: Vec<String>) {
        let mut g = self.inner.lock().expect("mock selinux mutex");
        g.audit_avc_lines = audit_lines;
        g.journal_avc_lines = journal_lines;
    }

    pub fn set_simulate_stale_verify(&self, stale: bool) {
        self.inner
            .lock()
            .expect("mock selinux mutex")
            .simulate_stale_verify = stale;
    }

    pub fn simulate_reboot(&self) {
        let mut g = self.inner.lock().expect("mock selinux mutex");
        for b in g.booleans.values_mut() {
            let boot_val = b.local_override.unwrap_or(b.base_default);
            b.current = boot_val;
            b.pending = boot_val;
        }
        if let Some(ref cfg_m) = g.configured_mode {
            g.runtime_mode = match cfg_m.as_str() {
                "enforcing" => SelinuxRuntimeMode::Enforcing,
                "permissive" => SelinuxRuntimeMode::Permissive,
                "disabled" => SelinuxRuntimeMode::Disabled,
                _ => g.runtime_mode,
            };
        }
    }

    pub fn recorded_commands(&self) -> Vec<Vec<String>> {
        self.inner
            .lock()
            .expect("mock selinux mutex")
            .recorded_commands
            .clone()
    }
}

impl SelinuxSysOps for MockSelinuxOps {
    fn read_runtime_mode(&self) -> Result<SelinuxRuntimeMode, AkrepError> {
        Ok(self.inner.lock().expect("mock selinux mutex").runtime_mode)
    }

    fn read_config(&self) -> Result<SelinuxConfigState, AkrepError> {
        let g = self.inner.lock().expect("mock selinux mutex");
        if let Some(ref pt) = g.policy_type {
            validate_selinux_policy_type(pt)?;
        }
        Ok(SelinuxConfigState {
            configured_mode: g.configured_mode.clone(),
            policy_type: g.policy_type.clone(),
        })
    }

    fn list_booleans(&self) -> Result<Vec<RawBooleanState>, AkrepError> {
        let g = self.inner.lock().expect("mock selinux mutex");
        let local_only = g.local_overrides_only;
        Ok(g.booleans
            .values()
            .map(|b| b.to_raw_state(local_only))
            .collect())
    }

    fn read_boolean(&self, name: &str) -> Result<Option<RawBooleanState>, AkrepError> {
        validate_selinux_boolean_name(name)?;
        let g = self.inner.lock().expect("mock selinux mutex");
        let local_only = g.local_overrides_only;
        Ok(g.booleans.get(name).map(|b| b.to_raw_state(local_only)))
    }

    fn set_boolean(&self, name: &str, value: bool, persistent: bool) -> Result<(), AkrepError> {
        validate_selinux_boolean_name(name)?;
        let mut g = self.inner.lock().expect("mock selinux mutex");
        let val_str = if value { "on" } else { "off" };
        let argv = if persistent {
            vec![
                "/usr/sbin/setsebool".to_string(),
                "-P".to_string(),
                name.to_string(),
                val_str.to_string(),
            ]
        } else {
            vec![
                "/usr/sbin/setsebool".to_string(),
                name.to_string(),
                val_str.to_string(),
            ]
        };
        g.recorded_commands.push(argv);

        let stale = g.simulate_stale_verify;
        let entry = g.booleans.get_mut(name).ok_or_else(|| {
            AkrepError::InvalidInput(format!("Unknown SELinux boolean '{}'", name))
        })?;
        if !stale {
            entry.current = value;
            entry.pending = value;
            if persistent {
                entry.local_override = Some(value);
            }
        }
        Ok(())
    }

    fn preview_restorecon(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<RestoreconPreviewEntry>, AkrepError> {
        let target = path.to_string_lossy().to_string();
        validate_restorecon_path(&target)?;
        let g = self.inner.lock().expect("mock selinux mutex");
        let prefix = format!("{}/", target.trim_end_matches('/'));
        let mut out = Vec::new();
        for (p, lbl) in &g.files {
            let matched = p == &target || (recursive && p.starts_with(&prefix));
            if matched && lbl.current_context != lbl.policy_default_context {
                out.push(RestoreconPreviewEntry {
                    path: p.clone(),
                    from_context: lbl.current_context.clone(),
                    to_context: lbl.policy_default_context.clone(),
                });
            }
        }
        Ok(out)
    }

    fn read_file_contexts(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<FileContextEntry>, AkrepError> {
        let target = path.to_string_lossy().to_string();
        validate_restorecon_path(&target)?;
        let g = self.inner.lock().expect("mock selinux mutex");
        let prefix = format!("{}/", target.trim_end_matches('/'));
        let mut out = Vec::new();
        for (p, lbl) in &g.files {
            if p == &target || (recursive && p.starts_with(&prefix)) {
                out.push(FileContextEntry {
                    path: p.clone(),
                    context: lbl.current_context.clone(),
                });
            }
        }
        if out.is_empty() {
            return Err(AkrepError::InvalidInput(format!(
                "Target path '{}' does not exist",
                target
            )));
        }
        if out.len() > MAX_RESTORECON_WALK_ENTRIES {
            return Err(AkrepError::InvalidInput(format!(
                "Target path '{}' exceeds maximum entry limit ({})",
                target, MAX_RESTORECON_WALK_ENTRIES
            )));
        }
        Ok(out)
    }

    fn apply_restorecon(
        &self,
        path: &Path,
        recursive: bool,
    ) -> Result<Vec<RestoreconPreviewEntry>, AkrepError> {
        let target = path.to_string_lossy().to_string();
        validate_restorecon_path(&target)?;
        let mut g = self.inner.lock().expect("mock selinux mutex");
        let mut argv = vec![
            "/sbin/restorecon".to_string(),
            "-v".to_string(),
            "-h".to_string(),
        ];
        if recursive {
            argv.push("-R".to_string());
        }
        argv.push("--".to_string());
        argv.push(target.clone());
        g.recorded_commands.push(argv);

        let stale = g.simulate_stale_verify;
        let prefix = format!("{}/", target.trim_end_matches('/'));
        let mut changed = Vec::new();
        for (p, lbl) in &mut g.files {
            let matched = p == &target || (recursive && p.starts_with(&prefix));
            if matched && lbl.current_context != lbl.policy_default_context {
                changed.push(RestoreconPreviewEntry {
                    path: p.clone(),
                    from_context: lbl.current_context.clone(),
                    to_context: lbl.policy_default_context.clone(),
                });
                if !stale {
                    lbl.current_context = lbl.policy_default_context.clone();
                }
            }
        }
        Ok(changed)
    }

    fn restore_file_context(&self, path: &Path, context: &str) -> Result<(), AkrepError> {
        let target = path.to_string_lossy().to_string();
        validate_restorecon_path(&target)?;
        let mut g = self.inner.lock().expect("mock selinux mutex");
        g.recorded_commands.push(vec![
            "/usr/bin/chcon".to_string(),
            "-h".to_string(),
            context.to_string(),
            "--".to_string(),
            target.clone(),
        ]);
        let entry = g.files.get_mut(&target).ok_or_else(|| {
            AkrepError::RollbackFailed(format!("Missing target file '{}' during chcon", target))
        })?;
        entry.current_context = context.to_string();
        Ok(())
    }

    fn read_audit_avc_lines(&self) -> Result<Vec<String>, AkrepError> {
        Ok(self
            .inner
            .lock()
            .expect("mock selinux mutex")
            .audit_avc_lines
            .clone())
    }

    fn read_journal_avc_lines(&self) -> Result<Vec<String>, AkrepError> {
        Ok(self
            .inner
            .lock()
            .expect("mock selinux mutex")
            .journal_avc_lines
            .clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::symlink;
    use tempfile::TempDir;

    #[test]
    fn test_semanage_and_booleans_local_parsers_and_c_locale() {
        let cmd = command_c_locale("/usr/bin/true");
        let envs: HashMap<_, _> = cmd
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect();
        assert_eq!(envs.get("LC_ALL").map(String::as_str), Some("C"));
        assert_eq!(envs.get("LANG").map(String::as_str), Some("C"));

        let semanage_sample = "\
httpd_can_network_connect      (off  ,  off)  Allow HTTPD scripts to connect
virt_use_usb                   (on   ,   on)  Allow virt to use USB
container_manage_cgroup        (on   ,  off)  Allow container cgroups
";
        let m = parse_semanage_boolean_output(semanage_sample);
        assert_eq!(m.get("httpd_can_network_connect"), Some(&false));
        assert_eq!(m.get("virt_use_usb"), Some(&true));
        assert_eq!(m.get("container_manage_cgroup"), Some(&false));

        let local_sample =
            "# Managed by setsebool\nvirt_use_usb=1\nhttpd_can_network_connect = 0\n";
        let ml = parse_booleans_local_file(local_sample);
        assert_eq!(ml.get("virt_use_usb"), Some(&true));
        assert_eq!(ml.get("httpd_can_network_connect"), Some(&false));

        let local_map = PersistentBooleanMap {
            source: PersistentSource::LocalOverridesOnly,
            values: ml,
        };
        assert_eq!(local_map.resolve_for("virt_use_usb", false), Some(true));
        assert_eq!(local_map.resolve_for("uncustomized_bool", true), None);
    }

    #[test]
    fn test_parse_restorecon_preview_lines() {
        let sample = "Would relabel /srv/www/index.html from unconfined_u:object_r:user_tmp_t:s0 to system_u:object_r:httpd_sys_content_t:s0\n";
        let parsed = parse_restorecon_output(sample);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, "/srv/www/index.html");
        assert_eq!(
            parsed[0].from_context,
            "unconfined_u:object_r:user_tmp_t:s0"
        );
        assert_eq!(
            parsed[0].to_context,
            "system_u:object_r:httpd_sys_content_t:s0"
        );
    }

    #[test]
    fn test_restorecon_canonicalization_blocks_symlink_escape_and_enforces_max_entries() {
        let tmp = TempDir::new().unwrap();
        let proc_link = tmp.path().join("proc_escape");
        symlink("/proc", &proc_link).unwrap();

        // Ancestor symlink escaping into /proc must be rejected
        let escaped_file = proc_link.join("version");
        let err_anc = resolve_and_validate_restorecon_target(&escaped_file, false).unwrap_err();
        assert!(matches!(err_anc, AkrepError::ForbiddenOperation(_)));

        // Directory symlink with recursive=true must be rejected
        let root_link = tmp.path().join("root_escape");
        symlink("/", &root_link).unwrap();
        let err_rec = resolve_and_validate_restorecon_target(&root_link, true).unwrap_err();
        assert!(matches!(err_rec, AkrepError::ForbiddenOperation(_)));

        // Symlink ancestor pointing to /etc/shadow must be rejected
        let etc_link = tmp.path().join("etc_link");
        symlink("/etc", &etc_link).unwrap();
        let shadow_via_link = etc_link.join("shadow");
        if Path::new("/etc/shadow").exists() {
            let err_sh =
                resolve_and_validate_restorecon_target(&shadow_via_link, false).unwrap_err();
            assert!(matches!(err_sh, AkrepError::ForbiddenOperation(_)));
        }

        // Bounded walk returns InvalidInput instead of silent truncation above max_entries
        let many_dir = tmp.path().join("many");
        fs::create_dir_all(&many_dir).unwrap();
        for idx in 0..6 {
            fs::write(many_dir.join(format!("f_{}.txt", idx)), b"ok").unwrap();
        }
        let max_err = walk_restorecon_paths_bounded(&many_dir, true, 4).unwrap_err();
        assert!(matches!(max_err, AkrepError::InvalidInput(_)));
        let ok_walk = walk_restorecon_paths_bounded(&many_dir, true, 20).unwrap();
        assert_eq!(ok_walk.len(), 7); // directory + 6 files
    }

    #[test]
    fn test_lossy_audit_log_reader_handles_non_utf8_bytes_and_tail_bound() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("audit.log");
        let mut raw_bytes = Vec::new();
        raw_bytes.extend_from_slice(b"type=PROCTITLE msg=audit(1732550000.050:199): proctitle=");
        raw_bytes.extend_from_slice(&[0xff, 0xfe, 0xfd]);
        raw_bytes.extend_from_slice(b"\n");
        raw_bytes.extend_from_slice(
            br#"type=AVC msg=audit(1732550001.100:200): avc:  denied  { read } for  pid=900 comm="nginx" path="/srv/www/index.html" scontext=system_u:system_r:httpd_t:s0 tcontext=unconfined_u:object_r:user_tmp_t:s0 tclass=file permissive=0"#,
        );
        raw_bytes.extend_from_slice(b"\n");

        let mut f = File::create(&log_path).unwrap();
        f.write_all(&raw_bytes).unwrap();
        f.sync_all().unwrap();

        let lines = extract_avc_lines_from_audit_file(&log_path).unwrap();
        assert_eq!(lines.len(), 1);
        let parsed = parse_single_avc_message(&lines[0], None, None).expect("parsed avc");
        assert_eq!(parsed.app_comm, "nginx");
        assert_eq!(parsed.permissions, vec!["read".to_string()]);
    }

    #[test]
    fn test_policy_type_validation_blocks_path_traversal() {
        assert!(validate_selinux_policy_type("targeted").is_ok());
        assert!(validate_selinux_policy_type("mls").is_ok());
        assert!(validate_selinux_policy_type("../etc").is_err());
        assert!(validate_selinux_policy_type("targeted/evil").is_err());
        assert!(validate_selinux_policy_type("").is_err());
    }
}
