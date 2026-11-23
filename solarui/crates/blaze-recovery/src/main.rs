// ==============================================================================
// Blaze Emergency Recovery Protocol (BERP)
// Console & TUI System Recovery, Snapshot Time Machine & Preserved Reset Engine
// ==============================================================================

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::collections::HashSet;
use std::ffi::{CStr, CString};
use std::fs;
use std::io::{stdin, stdout, IsTerminal, Write};
use std::os::raw::c_char;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

pub const BERP_VERSION: &str = "5.05";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryReason {
    Normal,
    BootFailure,
    SentinelAlert,
    ManualTrigger,
    KernelCmdline,
}

#[derive(Debug, Clone)]
pub struct SnapshotEntry {
    pub id: u64,
    pub path: PathBuf,
    pub name: String,
    pub date: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct PreservedFolder {
    pub name: String,
    pub username: String,
    pub folder_name: String,
    pub path: PathBuf,
    pub size_human: String,
    pub selected: bool,
}

pub struct BerpApp {
    pub target_root: PathBuf,
    pub root_dev: String,
    pub root_btrfs: bool,
    pub root_uuid: String,
    pub active_subvol: String,
    pub boot_counter: u32,
    pub recovery_requested: bool,
    pub recovery_reason: RecoveryReason,
    pub snapshots: Vec<SnapshotEntry>,
    pub preserved_folders: Vec<PreservedFolder>,
}

impl Default for BerpApp {
    fn default() -> Self {
        Self::new()
    }
}

pub fn remount_filesystems_rw() {
    let _ = Command::new("mount")
        .args(["-o", "remount,rw", "/"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if Path::new("/etc/fstab").is_file()
        && !Path::new("/run/initramfs/live").exists()
        && !Path::new("/etc/initrd-release").exists()
    {
        let _ = Command::new("mount")
            .arg("-a")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    for mp in ["/", "/home", "/boot", "/boot/efi", "/mnt/bere_target"] {
        if Path::new(mp).exists() {
            let _ = Command::new("mount")
                .args(["-o", "remount,rw", mp])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
}

/// Strips Btrfs subvolume suffix `[...]` from `findmnt -n -o SOURCE /` output,
/// e.g. `/dev/sda3[/root]` -> `/dev/sda3`.
pub fn strip_btrfs_subvol_suffix(dev: &str) -> String {
    dev.split('[').next().unwrap_or("").trim().to_string()
}

/// Normalizes a subvolume path from `btrfs subvolume list /` relative to the active FSROOT.
pub fn normalize_subvol_snapshot_path(raw_subvol_path: &str, active_fsroot: &str) -> Option<PathBuf> {
    let trimmed = raw_subvol_path.trim().trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let fsroot_clean = active_fsroot.trim().trim_start_matches('/').trim_end_matches('/');
    let rel = if !fsroot_clean.is_empty() {
        let prefix = format!("{}/", fsroot_clean);
        trimmed.strip_prefix(&prefix).unwrap_or(trimmed)
    } else {
        trimmed
    };

    // Never include container roots or /home snapshots in root rollback targets
    if rel == ".snapshots"
        || rel == ".blaze-snapshots"
        || rel == "home"
        || rel == "home/.snapshots"
        || rel.starts_with("home/.snapshots/")
        || rel.ends_with("_home")
    {
        return None;
    }

    if rel.starts_with(".snapshots/")
        || rel.starts_with(".blaze-snapshots/")
        || rel.starts_with("@snapshots/")
        || rel.contains("snapshot")
    {
        Some(PathBuf::from(format!("/{}", rel)))
    } else {
        None
    }
}

pub fn detect_active_root_subvol_at(mount_point: &Path) -> String {
    let mp = mount_point.to_str().unwrap_or("/");
    if let Ok(out) = Command::new("findmnt").args(["-n", "-o", "FSROOT", mp]).output() {
        if out.status.success() {
            let fsroot = String::from_utf8_lossy(&out.stdout)
                .trim()
                .trim_start_matches('/')
                .to_string();
            if !fsroot.is_empty() {
                return fsroot;
            }
        }
    }
    if let Ok(out) = Command::new("findmnt").args(["-n", "-o", "OPTIONS", mp]).output() {
        if out.status.success() {
            let opts = String::from_utf8_lossy(&out.stdout);
            for opt in opts.trim().split(',') {
                if let Some(sub) = opt.strip_prefix("subvol=") {
                    let clean = sub.trim_start_matches('/').to_string();
                    if !clean.is_empty() {
                        return clean;
                    }
                }
            }
        }
    }
    "root".to_string()
}

pub fn detect_active_root_subvol() -> String {
    detect_active_root_subvol_at(Path::new("/"))
}

pub fn discover_snapshots(root_btrfs: bool, active_subvol: &str) -> Vec<SnapshotEntry> {
    discover_snapshots_at(Path::new("/"), root_btrfs, active_subvol)
}

pub fn discover_snapshots_at(
    target_root: &Path,
    root_btrfs: bool,
    active_subvol: &str,
) -> Vec<SnapshotEntry> {
    let mut list = Vec::new();
    let mut seen_paths: HashSet<PathBuf> = HashSet::new();
    let target_str = target_root.to_str().unwrap_or("/");

    // 1. Scan btrfs subvolumes if available
    if root_btrfs {
        if let Ok(output) = Command::new("btrfs")
            .args(["subvolume", "list", target_str])
            .output()
        {
            if output.status.success() {
                let out_str = String::from_utf8_lossy(&output.stdout);
                for line in out_str.lines() {
                    if line.contains("snapshot") || line.contains(".snapshots") || line.contains(".blaze-snapshots") {
                        let parts: Vec<&str> = line.split_whitespace().collect();
                        if parts.len() >= 9 {
                            let id = parts[1].parse::<u64>().unwrap_or(0);
                            let path_str = if let Some(idx) = line.find(" path ") {
                                &line[idx + 6..]
                            } else {
                                parts[parts.len() - 1]
                            };
                            if let Some(norm_path) = normalize_subvol_snapshot_path(path_str, active_subvol) {
                                let disk_check = if target_root == Path::new("/") {
                                    norm_path.clone()
                                } else {
                                    target_root.join(norm_path.to_string_lossy().trim_start_matches('/'))
                                };
                                let canonical = fs::canonicalize(&disk_check).unwrap_or(norm_path.clone());
                                if seen_paths.insert(canonical) {
                                    let name = norm_path
                                        .file_name()
                                        .map(|s| s.to_string_lossy().to_string())
                                        .unwrap_or_else(|| norm_path.to_string_lossy().to_string());
                                    let date_str = fs::metadata(&disk_check)
                                        .ok()
                                        .and_then(|m| m.modified().ok())
                                        .map(|t| {
                                            let dt: chrono::DateTime<chrono::Local> = t.into();
                                            dt.format("%Y-%m-%d %H:%M").to_string()
                                        })
                                        .unwrap_or_else(|| "Sistem Kaydı".to_string());
                                    list.push(SnapshotEntry {
                                        id,
                                        path: norm_path,
                                        name: name.clone(),
                                        date: date_str,
                                        description: if name.contains("pre-update") || name.contains("dnf") {
                                            "Güncelleme Öncesi Otomatik Yedek".to_string()
                                        } else if name.contains("sentinel") {
                                            "Güvenlik Uyarısı Öncesi Durum".to_string()
                                        } else if name.contains("golden") || name.contains("factory") {
                                            "BlazeOS Orijinal Fabrika İmajı".to_string()
                                        } else {
                                            "Kullanıcı / Periyodik Snapshot".to_string()
                                        },
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Scan standard snapshot directories (including /.blaze-snapshots)
    let search_dirs = [
        ".blaze-snapshots",
        ".snapshots",
        "@snapshots",
        "var/snapshots",
        "boot/snapshots",
    ];
    for sdir in search_dirs {
        let p = target_root.join(sdir);
        if p.is_dir() {
            if let Ok(entries) = fs::read_dir(&p) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        let fname = path.file_name().unwrap_or_default().to_string_lossy().to_string();
                        if fname.ends_with("_home") {
                            continue;
                        }
                        let logical_path = PathBuf::from(format!("/{}/{}", sdir, fname));
                        let canonical = fs::canonicalize(&path).unwrap_or_else(|_| logical_path.clone());
                        if seen_paths.insert(canonical) {
                            let metadata = entry.metadata().ok();
                            let date_str = metadata
                                .and_then(|m| m.modified().ok())
                                .map(|t| {
                                    let dt: chrono::DateTime<chrono::Local> = t.into();
                                    dt.format("%Y-%m-%d %H:%M").to_string()
                                })
                                .unwrap_or_else(|| "Bilinmiyor".to_string());

                            list.push(SnapshotEntry {
                                id: list.len() as u64 + 1,
                                path: logical_path,
                                name: fname.clone(),
                                date: date_str,
                                description: if fname.contains("golden") || fname.contains("factory") {
                                    "BlazeOS Orijinal Fabrika İmajı".to_string()
                                } else {
                                    "Btrfs Kurtarma Noktası".to_string()
                                },
                            });
                        }
                    }
                }
            }
        }
    }

    // Only include /@golden_rootfs if it actually exists on disk
    if list.is_empty() && target_root.join("@golden_rootfs").exists() {
        list.push(SnapshotEntry {
            id: 1,
            path: PathBuf::from("/@golden_rootfs"),
            name: "@golden_rootfs (Fabrika Varsayılanı)".to_string(),
            date: "Sistem Kurulumu".to_string(),
            description: "İlk Kurulum Temiz Sistem İmajı".to_string(),
        });
    }

    list
}

impl BerpApp {
    pub fn new() -> Self {
        Self::new_with_remount(true)
    }

    pub fn new_readonly() -> Self {
        Self::new_with_remount(false)
    }

    fn new_with_remount(do_remount: bool) -> Self {
        if do_remount {
            remount_filesystems_rw();
        }
        let (target_root, root_dev, root_btrfs, root_uuid, active_subvol) =
            resolve_target_root(do_remount);
        let (boot_counter, recovery_requested, recovery_reason) = detect_recovery_reason();
        let mut app = Self {
            target_root,
            root_dev,
            root_btrfs,
            root_uuid,
            active_subvol,
            boot_counter,
            recovery_requested,
            recovery_reason,
            snapshots: Vec::new(),
            preserved_folders: Vec::new(),
        };
        app.refresh_snapshots();
        app.refresh_preserved_folders();
        app
    }

    pub fn refresh_snapshots(&mut self) {
        self.snapshots =
            discover_snapshots_at(&self.target_root, self.root_btrfs, &self.active_subvol);
    }

    pub fn refresh_preserved_folders(&mut self) {
        self.preserved_folders.clear();
        let mut candidates = Vec::new();

        let home_dir = self.target_root.join("home");
        if home_dir.is_dir() {
            if let Ok(users) = fs::read_dir(&home_dir) {
                for u in users.flatten() {
                    let upath = u.path();
                    let uname = upath.file_name().unwrap_or_default().to_string_lossy().to_string();
                    if upath.is_dir()
                        && uname != "lost+found"
                        && uname != "liveuser"
                        && uname != "blaze-setup"
                        && uname != "preserved_data"
                    {
                        let targets = [
                            ("İndirilenler (Downloads)", "Downloads", true),
                            ("İndirilenler (Türkçe)", "İndirilenler", true),
                            ("Belgeler (Documents)", "Documents", true),
                            ("Belgeler (Türkçe)", "Belgeler", true),
                            ("Masaüstü (Desktop)", "Desktop", true),
                            ("Masaüstü (Türkçe)", "Masaüstü", true),
                            ("Resimler (Pictures)", "Pictures", true),
                            ("Resimler (Türkçe)", "Resimler", true),
                            ("Videolar (Videos)", "Videos", true),
                            ("Projeler (Projects)", "Projects", true),
                            ("Kod Geliştirme (Workspace)", "workspace", true),
                            ("SSH Anahtarları (.ssh)", ".ssh", true),
                            ("GPG Anahtarları (.gnupg)", ".gnupg", true),
                            ("Tarayıcı Profilleri (.config)", ".config", false),
                        ];

                        for (title, sub, def_sel) in targets {
                            let p = upath.join(sub);
                            if p.exists() {
                                let size_str = get_folder_size_str(&p);
                                candidates.push(PreservedFolder {
                                    name: format!("{}: {}", uname, title),
                                    username: uname.clone(),
                                    folder_name: sub.to_string(),
                                    path: p,
                                    size_human: size_str,
                                    selected: def_sel,
                                });
                            }
                        }
                    }
                }
            }
        }

        if candidates.is_empty() {
            candidates.push(PreservedFolder {
                name: "Tüm /home Kullanıcı Alanı".to_string(),
                username: "shared".to_string(),
                folder_name: "home".to_string(),
                path: home_dir.clone(),
                size_human: get_folder_size_str(&home_dir),
                selected: true,
            });
        }

        self.preserved_folders = candidates;
    }
}

fn parse_blkid_export_blocks(raw: &str) -> Vec<(String, String, String, String)> {
    let mut blocks = Vec::new();
    for block in raw.split("\n\n") {
        let mut dev = String::new();
        let mut fstype = String::new();
        let mut uuid = String::new();
        let mut label = String::new();
        for line in block.lines() {
            if let Some(v) = line.strip_prefix("DEVNAME=") {
                dev = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("TYPE=") {
                fstype = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("UUID=") {
                uuid = v.trim().to_string();
            } else if let Some(v) = line.strip_prefix("LABEL=") {
                label = v.trim().to_string();
            }
        }
        if !dev.is_empty() && !fstype.is_empty() {
            blocks.push((dev, fstype, uuid, label));
        }
    }
    blocks
}

/// Resolves the active OS root target:
/// - When booted on a persistent installed disk, returns `("/", root_dev, is_btrfs, uuid, subvol)`.
/// - When booted from Live ISO (`overlay`/`LiveOS_rootfs`) or `initramfs` (`/etc/initrd-release`),
///   scans block devices via `blkid -o export` and auto-mounts the installed Btrfs/ext4 OS at `/mnt/bere_target`.
fn resolve_target_root(allow_mount: bool) -> (PathBuf, String, bool, String, String) {
    let mut root_fstype = String::new();
    let mut root_dev = String::new();
    if let Ok(content) = fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == "/" {
                root_dev = strip_btrfs_subvol_suffix(parts[0]);
                root_fstype = parts[2].to_string();
                break;
            }
        }
    }
    if let Ok(out) = Command::new("findmnt").args(["-n", "-o", "SOURCE", "/"]).output() {
        if out.status.success() {
            let s = strip_btrfs_subvol_suffix(&String::from_utf8_lossy(&out.stdout));
            if !s.is_empty() {
                root_dev = s;
            }
        }
    }

    let is_ephemeral_root = Path::new("/etc/initrd-release").exists()
        || matches!(
            root_fstype.as_str(),
            "overlay" | "rootfs" | "tmpfs" | "iso9660" | "squashfs"
        )
        || root_dev == "LiveOS_rootfs";

    if !is_ephemeral_root {
        let (is_btrfs, uuid) = detect_root_fs_at(Path::new("/"));
        let subvol = detect_active_root_subvol_at(Path::new("/"));
        return (PathBuf::from("/"), root_dev, is_btrfs, uuid, subvol);
    }

    let target_mp = PathBuf::from("/mnt/bere_target");
    if allow_mount {
        // Check if already mounted at /mnt/bere_target
        let (already_btrfs, already_uuid) = detect_root_fs_at(&target_mp);
        if already_uuid != "Bilinmiyor" {
            let subvol = detect_active_root_subvol_at(&target_mp);
            let dev = Command::new("findmnt")
                .args(["-n", "-o", "SOURCE", "/mnt/bere_target"])
                .output()
                .ok()
                .map(|o| strip_btrfs_subvol_suffix(&String::from_utf8_lossy(&o.stdout)))
                .unwrap_or_default();
            return (target_mp, dev, already_btrfs, already_uuid, subvol);
        }

        if let Ok(blkid_out) = Command::new("blkid").args(["-o", "export"]).output() {
            let blocks = parse_blkid_export_blocks(&String::from_utf8_lossy(&blkid_out.stdout));

            // Prefer Btrfs installed volumes first
            for (dev, fstype, uuid, label) in &blocks {
                if fstype != "btrfs" || label == "Blaze-SE-5" {
                    continue;
                }
                let probe_mp = Path::new("/mnt/berp_btrfs_probe");
                let _ = fs::create_dir_all(probe_mp);
                if Command::new("mount")
                    .args(["-o", "subvolid=5", dev, "/mnt/berp_btrfs_probe"])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
                {
                    let chosen_subvol = if probe_mp.join("root").is_dir() {
                        Some("root")
                    } else if probe_mp.join("@").is_dir() {
                        Some("@")
                    } else if probe_mp.join("@_active").is_dir() {
                        Some("@_active")
                    } else {
                        None
                    };
                    let has_home = probe_mp.join("home").is_dir();
                    let _ = Command::new("umount").arg("/mnt/berp_btrfs_probe").status();

                    let _ = fs::create_dir_all(&target_mp);
                    let mounted = if let Some(sv) = chosen_subvol {
                        Command::new("mount")
                            .args(["-o", &format!("subvol={}", sv), dev, "/mnt/bere_target"])
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false)
                    } else {
                        Command::new("mount")
                            .args([dev, "/mnt/bere_target"])
                            .status()
                            .map(|s| s.success())
                            .unwrap_or(false)
                    };

                    if mounted {
                        if has_home {
                            let home_mp = target_mp.join("home");
                            let _ = fs::create_dir_all(&home_mp);
                            let _ = Command::new("mount")
                                .args(["-o", "subvol=home", dev, "/mnt/bere_target/home"])
                                .status();
                        }
                        mount_companion_boot_partitions(&blocks, &target_mp);
                        let sub = chosen_subvol.unwrap_or("root").to_string();
                        return (
                            target_mp,
                            dev.clone(),
                            true,
                            if uuid.is_empty() { "Bilinmiyor".to_string() } else { uuid.clone() },
                            sub,
                        );
                    }
                }
            }

            // Fallback: ext4 / xfs installed root partition
            for (dev, fstype, uuid, label) in &blocks {
                if !matches!(fstype.as_str(), "ext4" | "xfs")
                    || label == "Blaze-SE-5"
                    || dev.starts_with("/dev/loop")
                {
                    continue;
                }
                let probe_mp = Path::new("/mnt/berp_ext_probe");
                let _ = fs::create_dir_all(probe_mp);
                if Command::new("mount")
                    .args([dev.as_str(), "/mnt/berp_ext_probe"])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
                {
                    let looks_like_boot = probe_mp.join("grub2").is_dir()
                        && !probe_mp.join("usr").is_dir()
                        && !probe_mp.join("home").is_dir();
                    let _ = Command::new("umount").arg("/mnt/berp_ext_probe").status();
                    if looks_like_boot {
                        continue;
                    }
                    let _ = fs::create_dir_all(&target_mp);
                    if Command::new("mount")
                        .args([dev.as_str(), "/mnt/bere_target"])
                        .status()
                        .map(|s| s.success())
                        .unwrap_or(false)
                    {
                        mount_companion_boot_partitions(&blocks, &target_mp);
                        return (
                            target_mp,
                            dev.clone(),
                            false,
                            if uuid.is_empty() { "Bilinmiyor".to_string() } else { uuid.clone() },
                            "root".to_string(),
                        );
                    }
                }
            }
        }
    }

    let (is_btrfs, uuid) = detect_root_fs_at(Path::new("/"));
    let subvol = detect_active_root_subvol_at(Path::new("/"));
    (PathBuf::from("/"), root_dev, is_btrfs, uuid, subvol)
}

fn mount_companion_boot_partitions(
    blocks: &[(String, String, String, String)],
    target_mp: &Path,
) {
    let boot_mp = target_mp.join("boot");
    for (dev, fstype, _, label) in blocks {
        if !matches!(fstype.as_str(), "ext4" | "xfs")
            || label == "Blaze-SE-5"
            || dev.starts_with("/dev/loop")
        {
            continue;
        }
        let probe_mp = Path::new("/mnt/berp_boot_probe");
        let _ = fs::create_dir_all(probe_mp);
        if Command::new("mount")
            .args([dev.as_str(), "/mnt/berp_boot_probe"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            let is_boot = probe_mp.join("grub2").is_dir() || probe_mp.join("loader").is_dir();
            let _ = Command::new("umount").arg("/mnt/berp_boot_probe").status();
            if is_boot {
                let _ = fs::create_dir_all(&boot_mp);
                let _ = Command::new("mount")
                    .args([dev.as_str(), boot_mp.to_str().unwrap_or("/mnt/bere_target/boot")])
                    .status();
                break;
            }
        }
    }
}

fn detect_root_fs_at(mount_point: &Path) -> (bool, String) {
    let mp_str = mount_point.to_str().unwrap_or("/");
    let mut is_btrfs = false;
    let mut uuid = "Bilinmiyor".to_string();

    if let Ok(content) = fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 && parts[1] == mp_str {
                if parts[2] == "btrfs" {
                    is_btrfs = true;
                }
                break;
            }
        }
    }

    if let Ok(output) = Command::new("findmnt").args(["-n", "-o", "UUID", mp_str]).output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                uuid = s;
            }
        }
    }

    (is_btrfs, uuid)
}

pub fn detect_recovery_reason() -> (u32, bool, RecoveryReason) {
    let mut counter: u32 = 0;
    let mut requested = false;
    let mut reason = RecoveryReason::Normal;

    // 1. Check grubenv if accessible
    if Path::new("/boot/grub2/grubenv").exists() {
        if let Ok(output) = Command::new("grub2-editenv")
            .args(["/boot/grub2/grubenv", "list"])
            .output()
        {
            if output.status.success() {
                let s = String::from_utf8_lossy(&output.stdout);
                for line in s.lines() {
                    if let Some(val) = line.strip_prefix("boot_counter=") {
                        counter = val.trim().parse().unwrap_or(0);
                    }
                    if line.starts_with("blaze_recovery=alert") {
                        requested = true;
                        reason = RecoveryReason::SentinelAlert;
                    } else if line.starts_with("blaze_recovery=1") {
                        requested = true;
                        if reason == RecoveryReason::Normal {
                            reason = RecoveryReason::ManualTrigger;
                        }
                    }
                }
            }
        }
    }

    // 2. Check /var/lib/blaze/berp-counter
    if let Ok(cnt_str) = fs::read_to_string("/var/lib/blaze/berp-counter") {
        if let Ok(berp_cnt) = cnt_str.trim().parse::<u32>() {
            if berp_cnt > counter {
                counter = berp_cnt;
            }
        }
    }
    if counter >= 3 {
        requested = true;
        reason = RecoveryReason::BootFailure;
    }

    // 3. Check sentinel & recovery trigger files
    if Path::new("/etc/blaze_recovery_alert").exists()
        || Path::new("/run/blaze_recovery_alert").exists()
    {
        requested = true;
        reason = RecoveryReason::SentinelAlert;
    }

    if Path::new("/var/run/blaze_recovery_trigger").exists()
        || Path::new("/run/blaze_recovery_trigger").exists()
        || Path::new("/var/lib/blaze/berp-trigger").exists()
        || Path::new("/boot/bere/recovery_triggered").exists()
    {
        requested = true;
        if reason == RecoveryReason::Normal {
            reason = RecoveryReason::ManualTrigger;
        }
    }

    // 4. Check /proc/cmdline for berp.mode=recovery
    if let Ok(cmdline) = fs::read_to_string("/proc/cmdline") {
        if cmdline.split_whitespace().any(|tok| {
            tok == "berp.mode=recovery"
                || tok == "systemd.unit=blaze-recovery.target"
                || tok == "systemd.unit=blaze-recovery-gui.target"
        }) {
            requested = true;
            if reason == RecoveryReason::Normal {
                reason = RecoveryReason::KernelCmdline;
            }
        }
    }

    (counter, requested, reason)
}

fn get_folder_size_str(path: &Path) -> String {
    if let Ok(output) = Command::new("du").args(["-sh", path.to_str().unwrap_or("")]).output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Some(first) = s.split_whitespace().next() {
                return first.to_string();
            }
        }
    }
    "~".to_string()
}

// -----------------------------------------------------------------------------
// Offline Root / Wheel Admin Authentication Gate (Policy A1)
// -----------------------------------------------------------------------------

unsafe extern "C" {
    fn crypt(phrase: *const c_char, setting: *const c_char) -> *const c_char;
}

static CRYPT_CALL_LOCK: Mutex<()> = Mutex::new(());

pub fn verify_crypt_password(password: &str, shadow_hash: &str) -> bool {
    if password.is_empty()
        || shadow_hash.is_empty()
        || shadow_hash.starts_with('!')
        || shadow_hash.starts_with('*')
        || shadow_hash == "x"
    {
        return false;
    }
    let Ok(c_pw) = CString::new(password) else {
        return false;
    };
    let Ok(c_salt) = CString::new(shadow_hash) else {
        return false;
    };
    let _guard = CRYPT_CALL_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let out_ptr = unsafe { crypt(c_pw.as_ptr(), c_salt.as_ptr()) };
    if out_ptr.is_null() {
        return false;
    }
    let hashed = unsafe { CStr::from_ptr(out_ptr) }.to_bytes();
    let expected = shadow_hash.as_bytes();
    if hashed.len() != expected.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (&a, &b) in hashed.iter().zip(expected.iter()) {
        diff |= a ^ b;
    }
    diff == 0
}

pub fn collect_admin_hashes_at(root: &Path) -> Vec<(String, String)> {
    let shadow_path = root.join("etc/shadow");
    if !shadow_path.is_file() {
        return Vec::new();
    }

    let mut admin_users = HashSet::new();
    admin_users.insert("root".to_string());
    let mut wheel_gid = None;

    if let Ok(group_txt) = fs::read_to_string(root.join("etc/group")) {
        for line in group_txt.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 4 && parts[0].trim() == "wheel" {
                let gid = parts[2].trim();
                if !gid.is_empty() {
                    wheel_gid = Some(gid.to_string());
                }
                for m in parts[3].split(',') {
                    let trimmed = m.trim();
                    if !trimmed.is_empty() {
                        admin_users.insert(trimmed.to_string());
                    }
                }
            }
        }
    }

    if let Some(ref wgid) = wheel_gid {
        if let Ok(passwd_txt) = fs::read_to_string(root.join("etc/passwd")) {
            for line in passwd_txt.lines() {
                let parts: Vec<&str> = line.split(':').collect();
                if parts.len() >= 4 && parts[3].trim() == wgid {
                    let uname = parts[0].trim();
                    if !uname.is_empty() {
                        admin_users.insert(uname.to_string());
                    }
                }
            }
        }
    }

    let mut entries = Vec::new();
    if let Ok(shadow_txt) = fs::read_to_string(&shadow_path) {
        for line in shadow_txt.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 2 {
                continue;
            }
            let user = parts[0].trim();
            let hash = parts[1].trim();
            if admin_users.contains(user)
                && !hash.is_empty()
                && !hash.starts_with('!')
                && !hash.starts_with('*')
                && hash != "x"
            {
                entries.push((user.to_string(), hash.to_string()));
            }
        }
    }
    entries
}

pub fn verify_admin_password_at(root: &Path, password: &str) -> Option<String> {
    if password.is_empty() {
        return None;
    }
    for (user, hash) in collect_admin_hashes_at(root) {
        if verify_crypt_password(password, &hash) {
            return Some(user);
        }
    }
    None
}

fn resolve_auth_target_root(preferred: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = preferred {
        if p.join("etc/shadow").is_file() {
            return Some(p.to_path_buf());
        }
    }
    for cand in [Path::new("/mnt/bere_target"), Path::new("/sysroot")] {
        if cand.join("etc/shadow").is_file() {
            return Some(cand.to_path_buf());
        }
    }
    let ephemeral_root = Path::new("/etc/initrd-release").exists()
        || Path::new("/run/initramfs/live").exists()
        || Path::new("/dev/mapper/live-base").exists();
    if !ephemeral_root && Path::new("/etc/shadow").is_file() {
        return Some(PathBuf::from("/"));
    }
    if preferred.is_none() {
        let (mounted_target, _, _, _, _) = resolve_target_root(true);
        if mounted_target != Path::new("/") && mounted_target.join("etc/shadow").is_file() {
            return Some(mounted_target);
        }
    }
    None
}

fn read_masked_password(prompt: &str) -> Result<Option<String>> {
    print!("{}", prompt);
    stdout().flush()?;

    if stdin().is_terminal() && stdout().is_terminal() && terminal::enable_raw_mode().is_ok() {
        let mut pw = String::new();
        let mut out = stdout();
        let res = loop {
            match event::read()? {
                Event::Key(key) => match key.code {
                    KeyCode::Enter => {
                        execute!(out, Print("\r\n"))?;
                        out.flush()?;
                        break Some(pw);
                    }
                    KeyCode::Esc => {
                        execute!(out, Print("\r\n"))?;
                        out.flush()?;
                        break None;
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        execute!(out, Print("\r\n"))?;
                        out.flush()?;
                        break None;
                    }
                    KeyCode::Backspace => {
                        if pw.pop().is_some() {
                            execute!(out, Print("\x08 \x08"))?;
                            out.flush()?;
                        }
                    }
                    KeyCode::Char(c) => {
                        pw.push(c);
                        execute!(out, Print("*"))?;
                        out.flush()?;
                    }
                    _ => {}
                },
                _ => {}
            }
        };
        let _ = terminal::disable_raw_mode();
        return Ok(res);
    }

    let mut line = String::new();
    let n = std::io::stdin().read_line(&mut line)?;
    println!();
    if n == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim_end_matches(&['\r', '\n'][..]).to_string()))
}

pub fn prompt_and_verify_admin_auth(preferred_root: Option<&Path>, banner: &str) -> Result<bool> {
    let Some(auth_root) = resolve_auth_target_root(preferred_root) else {
        println!("  [BİLGİ] Kurulu sistem hesabı (/etc/shadow) bulunamadı; kurtarma erişimi açık.");
        return Ok(true);
    };
    let entries = collect_admin_hashes_at(&auth_root);
    if entries.is_empty() {
        println!("  [BİLGİ] Kurulu sistemde aktif root/wheel şifresi bulunamadı; erişim açık.");
        return Ok(true);
    }

    println!("\n=======================================================");
    println!("  {}", banner);
    println!("  Yetkili Kullanıcı (wheel) veya Root Parolası Gerekli");
    println!("=======================================================");

    for attempt in 1..=5 {
        let Some(pw) = read_masked_password("  Parola (İptal: Esc): ")? else {
            println!("  [İPTAL] Kimlik doğrulaması iptal edildi.");
            return Ok(false);
        };
        if verify_admin_password_at(&auth_root, &pw).is_some() {
            println!("  [DOĞRULANDI] Yetkili erişim onaylandı.\n");
            return Ok(true);
        }
        std::thread::sleep(std::time::Duration::from_millis(350));
        println!("  [REDDEDİLDİ] Hatalı parola (Kalan deneme: {}).", 5 - attempt);
    }
    println!("  [ENGEL] Çok fazla hatalı parola denemesi!");
    Ok(false)
}

fn prompt_auth_in_tui(app: &BerpApp, banner: &str) -> Result<bool> {
    let mut out = stdout();
    let _ = execute!(out, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    let ok = prompt_and_verify_admin_auth(Some(&app.target_root), banner)?;
    if !ok {
        std::thread::sleep(std::time::Duration::from_millis(900));
    }
    terminal::enable_raw_mode()?;
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;
    Ok(ok)
}

// -----------------------------------------------------------------------------
// Interactive UI Engine
// -----------------------------------------------------------------------------

pub fn run_plain_text_menu(app: &mut BerpApp) -> Result<()> {
    if !prompt_and_verify_admin_auth(Some(&app.target_root), "BERP KURTARMA GÜVENLİK KİLİDİ")? {
        anyhow::bail!("BERP kimlik doğrulaması başarısız.");
    }
    run_plain_text_menu_loop(app)
}

fn run_plain_text_menu_loop(app: &mut BerpApp) -> Result<()> {
    loop {
        println!("\n=======================================================");
        println!("       BLAZE EMERGENCY RECOVERY PROTOCOL (BERP)        ");
        println!("=======================================================");
        println!("  Kök Bölümü: UUID={} | Btrfs: {}", app.root_uuid, app.root_btrfs);
        println!("  Bulunan Snapshot: {} adet\n", app.snapshots.len());
        println!("  [1] Zaman Makinesi (Snapshot Geri Yükleme)");
        println!("  [2] Verilerimi Koru ve Sıfırla (Akıllı Yedekleme)");
        println!("  [3] Fabrika Ayarlarına Tam Sıfırlama");
        println!("  [4] Sistem & Donanım Teşhisi");
        println!("  [5] Acil Durum Terminali (Root Shell)");
        println!("  [R] Sistemi Yeniden Başlat");
        println!("  [Q] Bilgisayarı Kapat");
        print!("\nSeçiminiz (1-5, R, Q): ");
        std::io::stdout().flush()?;

        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
            break;
        }
        match line.trim().to_uppercase().as_str() {
            "1" => {
                if app.snapshots.is_empty() {
                    println!("Kayıtlı snapshot bulunamadı.");
                } else {
                    for (i, s) in app.snapshots.iter().enumerate() {
                        println!("  [{}] {} ({}) - {}", i + 1, s.name, s.date, s.description);
                    }
                    print!("Geri yüklemek istediğiniz numara (veya 0 iptal): ");
                    std::io::stdout().flush()?;
                    let mut pick = String::new();
                    std::io::stdin().read_line(&mut pick)?;
                    if let Ok(num) = pick.trim().parse::<usize>() {
                        if num > 0 && num <= app.snapshots.len() {
                            if !prompt_and_verify_admin_auth(
                                Some(&app.target_root),
                                "SNAPSHOT GERİ YÜKLEME PAROLA ONAYI",
                            )? {
                                continue;
                            }
                            let snap = app.snapshots[num - 1].clone();
                            execute_snapshot_rollback(app, &snap)?;
                        }
                    }
                }
            }
            "2" => {
                if !prompt_and_verify_admin_auth(
                    Some(&app.target_root),
                    "VERİLERİ KORU VE SIFIRLA — PAROLA ONAYI",
                )? {
                    continue;
                }
                println!("Korunacak klasörler:");
                for (i, f) in app.preserved_folders.iter().enumerate() {
                    println!("  [{}] {} ({})", i + 1, f.name, f.size_human);
                }
                execute_preserved_reset(&app.target_root, &app.preserved_folders)?;
            }
            "3" => {
                println!("UYARI: Tüm veriler silinecektir! Onaylamak için SIFIRLA yazın:");
                let mut c = String::new();
                std::io::stdin().read_line(&mut c)?;
                let trimmed = c.trim().to_lowercase();
                if trimmed == "sifirla" || trimmed == "sıfırla" || trimmed == "reset" {
                    if !prompt_and_verify_admin_auth(
                        Some(&app.target_root),
                        "FABRİKA SIFIRLAMASI — ZORUNLU PAROLA ONAYI",
                    )? {
                        continue;
                    }
                    println!("Fabrika ayarlarına sıfırlama başlatılıyor...");
                    execute_factory_reset_core_at(&app.target_root)?;
                    println!("Sistem sıfırlandı. Yeniden başlatılıyor...");
                    do_reboot()?;
                    break;
                } else {
                    println!("Hatalı onay kelimesi. İşlem iptal edildi.");
                }
            }
            "4" => {
                println!("Donanım ve dosya sistemi taranıyor...");
                let _ = Command::new("lsblk").args(["-d", "-o", "NAME,SIZE,MODEL,TYPE"]).status();
                if app.root_btrfs {
                    let _ = Command::new("btrfs").args(["device", "stats", "/"]).status();
                    let _ = Command::new("btrfs").args(["scrub", "status", "/"]).status();
                }
                let _ = Command::new("blaze-sentinel").arg("status").status();
            }
            "5" => {
                handle_emergency_shell()?;
            }
            "R" => {
                do_reboot()?;
                break;
            }
            "Q" => {
                do_poweroff()?;
                break;
            }
            _ => println!("Geçersiz seçim."),
        }
    }
    Ok(())
}

pub fn run_interactive_tui(mut app: BerpApp) -> Result<()> {
    if !prompt_and_verify_admin_auth(Some(&app.target_root), "BERP KURTARMA GÜVENLİK KİLİDİ")? {
        anyhow::bail!("BERP kimlik doğrulaması başarısız.");
    }
    if !stdin().is_terminal() || !stdout().is_terminal() {
        return run_plain_text_menu_loop(&mut app);
    }
    if let Err(e) = terminal::enable_raw_mode() {
        eprintln!("Raw mode failed: {}. Line-buffered konsol moduna geçiliyor...", e);
        return run_plain_text_menu_loop(&mut app);
    }
    let mut stdout = stdout();
    execute!(
        stdout,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        Clear(ClearType::All)
    )?;

    let mut current_menu_index: usize = 0;
    let menu_items = [
        ("1", "Zaman Makinesi (Btrfs Snapshot Geri Yükleme)", "Sistemi en son kararlı çalıştığı snapshot anına döndürür"),
        ("2", "Verilerimi Koru ve Sıfırla (Akıllı Sıfırlama)", "Seçtiğin klasörleri güvene alarak sistemi fabrika durumuna sıfırlar"),
        ("3", "Fabrika Ayarlarına Tam Sıfırlama (Factory Wipe)", "Tüm sistem ve kullanıcı alanını ilk kurulum durumuna sıfırlar"),
        ("4", "Sistem & Donanım Teşhisi (Diagnostics)", "RAM, NVMe SMART durumu ve Btrfs dosya sistemi bütünlük taraması"),
        ("5", "Acil Durum Terminali (Root Rescue Shell)", "Sorun giderme için kök izinli komut satırını açar"),
        ("R", "Sistemi Yeniden Başlat (Reboot)", "Bilgisayarı normal önyükleme ile baştan başlatır"),
        ("Q", "Bilgisayarı Kapat (Power Off)", "Sistemi güvenli bir şekilde kapatır"),
    ];

    loop {
        render_main_screen(&app, current_menu_index, &menu_items)?;

        if event::poll(std::time::Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up => {
                        if current_menu_index > 0 {
                            current_menu_index -= 1;
                        } else {
                            current_menu_index = menu_items.len() - 1;
                        }
                    }
                    KeyCode::Down => {
                        if current_menu_index + 1 < menu_items.len() {
                            current_menu_index += 1;
                        } else {
                            current_menu_index = 0;
                        }
                    }
                    KeyCode::Char('1') => {
                        handle_time_machine(&mut app)?;
                    }
                    KeyCode::Char('2') => {
                        handle_preserve_and_reset(&mut app)?;
                    }
                    KeyCode::Char('3') => {
                        handle_factory_reset(&mut app)?;
                    }
                    KeyCode::Char('4') => {
                        handle_diagnostics(&app)?;
                    }
                    KeyCode::Char('5') => {
                        handle_emergency_shell()?;
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        do_reboot()?;
                        break;
                    }
                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                        do_poweroff()?;
                        break;
                    }
                    KeyCode::Enter => {
                        match current_menu_index {
                            0 => handle_time_machine(&mut app)?,
                            1 => handle_preserve_and_reset(&mut app)?,
                            2 => handle_factory_reset(&mut app)?,
                            3 => handle_diagnostics(&app)?,
                            4 => handle_emergency_shell()?,
                            5 => {
                                do_reboot()?;
                                break;
                            }
                            6 => {
                                do_poweroff()?;
                                break;
                            }
                            _ => {}
                        }
                    }
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    execute!(stdout, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

fn render_main_screen(
    app: &BerpApp,
    selected_idx: usize,
    menu_items: &[(&str, &str, &str)],
) -> Result<()> {
    let mut out = stdout();
    execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;

    // Header
    execute!(
        out,
        SetForegroundColor(Color::Black),
        SetBackgroundColor(Color::Cyan),
        Print("  === BLAZE EMERGENCY RECOVERY PROTOCOL (BERP) ===  \r\n"),
        ResetColor,
        SetForegroundColor(Color::DarkGrey),
        Print("  Blaze SolarEvolution — Bağımsız Sistem Kurtarma ve Güvenlik Konsolu\r\n\r\n"),
        ResetColor
    )?;

    // Status Card
    execute!(
        out,
        SetForegroundColor(Color::White),
        Print("  [ Sistem Durumu ]\r\n"),
        SetForegroundColor(Color::DarkCyan),
        Print(format!("    * Kök Bölümü:        UUID={} (subvol={})\r\n", app.root_uuid, app.active_subvol)),
        Print(format!(
            "    * Dosya Sistemi:     {} (CoW & Snapshot Desteği)\r\n",
            if app.root_btrfs { "Btrfs Aktif" } else { "Standart Linux Dosya Sistemi" }
        )),
        Print(format!(
            "    * Önyükleme Durumu:  {} ({:?}) | Başarısız Sayaç: {}\r\n",
            if app.recovery_requested { "ACİL KURTARMA TETİKLENDİ" } else { "Normal" },
            app.recovery_reason,
            app.boot_counter
        )),
        Print(format!("    * Bulunan Snapshot:  {} adet kayıtlı durum\r\n\r\n", app.snapshots.len())),
        ResetColor
    )?;

    // Menu options
    execute!(
        out,
        SetForegroundColor(Color::Yellow),
        Print("  Lütfen gerçekleştirmek istediğiniz kurtarma eylemini seçin:\r\n\r\n"),
        ResetColor
    )?;

    for (i, (key, label, desc)) in menu_items.iter().enumerate() {
        if i == selected_idx {
            execute!(
                out,
                SetForegroundColor(Color::Black),
                SetBackgroundColor(Color::White),
                Print(format!("   > [{}] {:<48} \r\n", key, label)),
                ResetColor,
                SetForegroundColor(Color::Cyan),
                Print(format!("       └── {}\r\n\r\n", desc)),
                ResetColor
            )?;
        } else {
            execute!(
                out,
                SetForegroundColor(Color::Green),
                Print(format!("     [{}] ", key)),
                SetForegroundColor(Color::White),
                Print(format!("{:<48}\r\n", label)),
                SetForegroundColor(Color::DarkGrey),
                Print(format!("       └── {}\r\n\r\n", desc)),
                ResetColor
            )?;
        }
    }

    execute!(
        out,
        SetForegroundColor(Color::DarkGrey),
        Print("  [↑ / ↓] Seçimi Değiştir  |  [Enter / Rakam] Onayla  |  [Ctrl+C] Çık\r\n"),
        ResetColor
    )?;

    out.flush()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Submenu: Time Machine (Snapshot Rollback)
// -----------------------------------------------------------------------------

fn handle_time_machine(app: &mut BerpApp) -> Result<()> {
    let mut out = stdout();
    let mut sel = 0;

    loop {
        execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
        execute!(
            out,
            SetForegroundColor(Color::Black),
            SetBackgroundColor(Color::Yellow),
            Print("  === ZAMAN MAKİNESİ: SNAPSHOT GERİ YÜKLEME ===  \r\n"),
            ResetColor,
            Print("\r\n  Geri dönmek istediğiniz sistem anını seçin ve Enter'a basın:\r\n\r\n")
        )?;

        if app.snapshots.is_empty() {
            execute!(
                out,
                SetForegroundColor(Color::Red),
                Print("  Sistemde kayıtlı Btrfs snapshot bulunamadı.\r\n"),
                ResetColor,
                Print("\r\n  [Esc / Enter] Ana Menüye Dön\r\n")
            )?;
            out.flush()?;
            wait_for_escape_or_enter()?;
            return Ok(());
        }

        for (i, snap) in app.snapshots.iter().enumerate() {
            if i == sel {
                execute!(
                    out,
                    SetForegroundColor(Color::Black),
                    SetBackgroundColor(Color::Cyan),
                    Print(format!("   > [{}] {:<35} | {:<16} | {}\r\n", snap.id, snap.name, snap.date, snap.description)),
                    ResetColor
                )?;
            } else {
                execute!(
                    out,
                    SetForegroundColor(Color::White),
                    Print(format!("     [{}] {:<35} | {:<16} | {}\r\n", snap.id, snap.name, snap.date, snap.description)),
                    ResetColor
                )?;
            }
        }

        execute!(
            out,
            SetForegroundColor(Color::DarkGrey),
            Print("\r\n  [↑ / ↓] Gezin  |  [Enter] Seçilen Duruma Geri Dön  |  [Esc] İptal\r\n"),
            ResetColor
        )?;
        out.flush()?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up => {
                    sel = sel.saturating_sub(1);
                }
                KeyCode::Down => {
                    if sel + 1 < app.snapshots.len() {
                        sel += 1;
                    }
                }
                KeyCode::Esc => return Ok(()),
                KeyCode::Enter => {
                    let target = app.snapshots[sel].clone();
                    return execute_snapshot_rollback(app, &target);
                }
                _ => {}
            }
        }
    }
}

/// Resolves the snapshot source path inside `top_mount` after `active_subvol` was renamed to `old_subvol_name`.
pub fn resolve_btrfs_snapshot_source(
    top_mount: &Path,
    active_subvol: &str,
    old_subvol_path: &Path,
    snap_path: &Path,
) -> PathBuf {
    let snap_rel = snap_path.to_string_lossy().trim_start_matches('/').to_string();
    let stripped_rel = snap_rel
        .strip_prefix(&format!("{}/", active_subvol))
        .unwrap_or(&snap_rel);

    let candidates = [
        old_subvol_path.join(stripped_rel),
        old_subvol_path.join(&snap_rel),
        top_mount.join(&snap_rel),
        top_mount.join(stripped_rel),
        snap_path.to_path_buf(),
    ];

    for c in &candidates {
        if c.exists() {
            return c.clone();
        }
    }
    candidates[0].clone()
}

/// Moves nested `.snapshots` and `.blaze-snapshots` subvolumes or directories from `old_subvol_path`
/// into the newly restored `target_subvol` so snapshots are preserved after rollback.
pub fn preserve_nested_snapshot_containers(old_subvol_path: &Path, target_subvol: &Path) {
    for nested_name in [".blaze-snapshots", ".snapshots"] {
        let old_nested = old_subvol_path.join(nested_name);
        let new_nested = target_subvol.join(nested_name);
        if !old_nested.exists() {
            continue;
        }
        // When a Btrfs snapshot is created, nested subvolumes appear as empty stub directories
        // in the new snapshot. Remove the empty stub directory and move the real nested subvolume/dir.
        if new_nested.exists() {
            let _ = fs::remove_dir(&new_nested);
        }
        if !new_nested.exists() {
            let _ = fs::rename(&old_nested, &new_nested);
        } else if let Ok(entries) = fs::read_dir(&old_nested) {
            for entry in entries.flatten() {
                let dest = new_nested.join(entry.file_name());
                if dest.is_dir() {
                    let is_empty = fs::read_dir(&dest).map(|mut i| i.next().is_none()).unwrap_or(false);
                    if is_empty {
                        let _ = fs::remove_dir(&dest);
                    }
                }
                if !dest.exists() {
                    let _ = fs::rename(entry.path(), dest);
                }
            }
        }
    }
}

fn execute_snapshot_rollback(app: &BerpApp, snap: &SnapshotEntry) -> Result<()> {
    let mut out = stdout();
    execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    execute!(
        out,
        SetForegroundColor(Color::Red),
        Print("  [ ONAY GEREKLİ ]\r\n"),
        SetForegroundColor(Color::White),
        Print(format!(
            "  Sistem aşağıdaki snapshot durumuna geri döndürülecektir:\r\n    Hedef: {}\r\n    Açıklama: {}\r\n\r\n",
            snap.name, snap.description
        )),
        SetForegroundColor(Color::Yellow),
        Print("  Bu işlem geçerli kök sistemini hedef snapshot ile değiştirecektir.\r\n"),
        Print("  Devam etmek için [E]vet, iptal etmek için [H]ayır basınız: "),
        ResetColor
    )?;
    out.flush()?;

    loop {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if !prompt_auth_in_tui(app, "SNAPSHOT GERİ YÜKLEME — ZORUNLU PAROLA ONAYI")? {
                        return Ok(());
                    }
                    execute!(
                        out,
                        Print("\r\n\r\n  -> Snapshot geri yükleme başlatılıyor...\r\n")
                    )?;
                    out.flush()?;

                    let snap_str = snap.path.to_string_lossy();
                    if snap_str.contains("/home/.snapshots") || snap_str.ends_with("_home") {
                        execute!(
                            out,
                            SetForegroundColor(Color::Red),
                            Print("  [HATA] Kullanıcı (/home) yedeği kök dizine (/) geri yüklenemez!\r\n"),
                            ResetColor,
                            Print("\r\n  [Enter] veya [Esc] ile ana menüye dönebilirsiniz...\r\n")
                        )?;
                        out.flush()?;
                        wait_for_escape_or_enter()?;
                        return Ok(());
                    }

                    remount_filesystems_rw();

                    let mut rollback_success = false;

                    if app.root_btrfs {
                        let root_dev = if !app.root_dev.is_empty() {
                            app.root_dev.clone()
                        } else if let Ok(findmnt_out) = Command::new("findmnt")
                            .args(["-n", "-o", "SOURCE", app.target_root.to_str().unwrap_or("/")])
                            .output()
                        {
                            strip_btrfs_subvol_suffix(&String::from_utf8_lossy(&findmnt_out.stdout))
                        } else {
                            String::new()
                        };

                        if !root_dev.is_empty() {
                            let top_mount = Path::new("/mnt/bere_btrfs_top");
                            let _ = fs::create_dir_all(top_mount);
                            if Command::new("mount")
                                .args(["-o", "subvolid=5", &root_dev, "/mnt/bere_btrfs_top"])
                                .stdout(Stdio::null())
                                .stderr(Stdio::null())
                                .status()
                                .map(|s| s.success())
                                .unwrap_or(false)
                            {
                                let detected = app.active_subvol.clone();
                                let active_subvol = if !detected.contains('/') && top_mount.join(&detected).exists() {
                                    detected
                                } else if top_mount.join("root").exists() {
                                    "root".to_string()
                                } else if top_mount.join("@").exists() {
                                    "@".to_string()
                                } else if top_mount.join("@_active").exists() {
                                    "@_active".to_string()
                                } else {
                                    "root".to_string()
                                };

                                let target_subvol = top_mount.join(&active_subvol);
                                let ts = chrono::Local::now().timestamp();
                                let old_subvol_name = format!("{}_old_{}", active_subvol, ts);
                                let old_subvol_path = top_mount.join(&old_subvol_name);

                                let renamed = if target_subvol.exists() {
                                    fs::rename(&target_subvol, &old_subvol_path).is_ok()
                                } else {
                                    false
                                };

                                let source_path = resolve_btrfs_snapshot_source(
                                    top_mount,
                                    &active_subvol,
                                    if renamed { &old_subvol_path } else { &target_subvol },
                                    &snap.path,
                                );

                                let snap_status = Command::new("btrfs")
                                    .args([
                                        "subvolume",
                                        "snapshot",
                                        source_path.to_str().unwrap_or(""),
                                        target_subvol.to_str().unwrap_or(""),
                                    ])
                                    .stdout(Stdio::null())
                                    .stderr(Stdio::null())
                                    .status();

                                if snap_status.map(|s| s.success()).unwrap_or(false) {
                                    // Ensure the restored root subvolume is read-write (ro=false)
                                    let _ = Command::new("btrfs")
                                        .args([
                                            "property",
                                            "set",
                                            "-ts",
                                            target_subvol.to_str().unwrap_or(""),
                                            "ro",
                                            "false",
                                        ])
                                        .stdout(Stdio::null())
                                        .stderr(Stdio::null())
                                        .status();

                                    // Preserve nested .snapshots and .blaze-snapshots into the new root subvol
                                    if renamed {
                                        preserve_nested_snapshot_containers(&old_subvol_path, &target_subvol);
                                    }
                                    rollback_success = true;
                                } else if renamed && !target_subvol.exists() {
                                    // Restore original root subvolume on failure
                                    let _ = fs::rename(&old_subvol_path, &target_subvol);
                                }

                                let _ = Command::new("umount")
                                    .arg("/mnt/bere_btrfs_top")
                                    .stdout(Stdio::null())
                                    .stderr(Stdio::null())
                                    .status();
                            }
                        }
                    } else {
                        // Ext4 / XFS veya dizin yedeği: rsync ile güvenli eşitleme
                        let src_dir = if app.target_root == Path::new("/") {
                            snap.path.clone()
                        } else {
                            app.target_root.join(snap.path.to_string_lossy().trim_start_matches('/'))
                        };
                        let dst_dir = format!("{}/", app.target_root.to_string_lossy().trim_end_matches('/'));
                        let rsync_status = Command::new("rsync")
                            .args([
                                "-aHAX",
                                "--delete",
                                "--exclude=/boot",
                                "--exclude=/dev",
                                "--exclude=/proc",
                                "--exclude=/sys",
                                "--exclude=/run",
                                "--exclude=/mnt",
                                "--exclude=/tmp",
                                "--exclude=/var/tmp",
                                &format!("{}/", src_dir.display()),
                                &dst_dir,
                            ])
                            .status();
                        rollback_success = rsync_status.map(|s| s.success()).unwrap_or(false);
                    }

                    if rollback_success {
                        execute!(
                            out,
                            SetForegroundColor(Color::Green),
                            Print(format!(
                                "  [BAŞARILI] Sistem '{}' durumuna başarıyla döndürüldü!\r\n",
                                snap.name
                            )),
                            Print("  Önyükleme bayrakları temizlendi.\r\n"),
                            ResetColor,
                            Print("\r\n  [Enter] basarak sistemi hemen yeniden başlatabilirsiniz...\r\n")
                        )?;
                        clear_boot_triggers();
                        out.flush()?;
                        wait_for_escape_or_enter()?;
                        do_reboot()?;
                        return Ok(());
                    } else {
                        execute!(
                            out,
                            SetForegroundColor(Color::Red),
                            Print("  [HATA] Geri yükleme işlemi tamamlanamadı!\r\n"),
                            ResetColor,
                            Print("\r\n  [Enter] veya [Esc] ile ana menüye dönebilirsiniz...\r\n")
                        )?;
                        out.flush()?;
                        wait_for_escape_or_enter()?;
                        return Ok(());
                    }
                }
                KeyCode::Char('h')
                | KeyCode::Char('H')
                | KeyCode::Char('n')
                | KeyCode::Char('N')
                | KeyCode::Esc => {
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Submenu: Preserve Data & Reset ("Verilerimi Koru ve Sıfırla")
// -----------------------------------------------------------------------------

fn handle_preserve_and_reset(app: &mut BerpApp) -> Result<()> {
    let mut out = stdout();
    let mut sel = 0;

    loop {
        execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
        execute!(
            out,
            SetForegroundColor(Color::Black),
            SetBackgroundColor(Color::Magenta),
            Print("  === VERİLERİMİ KORU VE SIFIRLA (AKILLI YEDEKLEME) ===  \r\n"),
            ResetColor,
            SetForegroundColor(Color::Yellow),
            Print("\r\n  Sıfırlama sırasında korunmasını istediğiniz klasörleri seçin:\r\n"),
            Print("  (Boşluk tuşu ile işaretleyin/kaldırın, Enter ile sıfırlamayı başlatın)\r\n\r\n"),
            ResetColor
        )?;

        for (i, folder) in app.preserved_folders.iter().enumerate() {
            let check = if folder.selected { "[X]" } else { "[ ]" };
            if i == sel {
                execute!(
                    out,
                    SetForegroundColor(Color::Black),
                    SetBackgroundColor(Color::White),
                    Print(format!(
                        "   > {} {:<32} ({}) [{}]\r\n",
                        check,
                        folder.name,
                        folder.size_human,
                        folder.path.display()
                    )),
                    ResetColor
                )?;
            } else {
                execute!(
                    out,
                    SetForegroundColor(if folder.selected { Color::Green } else { Color::DarkGrey }),
                    Print(format!("     {} ", check)),
                    SetForegroundColor(Color::White),
                    Print(format!("{:<32} ", folder.name)),
                    SetForegroundColor(Color::DarkCyan),
                    Print(format!("({}) ", folder.size_human)),
                    SetForegroundColor(Color::DarkGrey),
                    Print(format!("[{}]\r\n", folder.path.display())),
                    ResetColor
                )?;
            }
        }

        execute!(
            out,
            SetForegroundColor(Color::DarkGrey),
            Print("\r\n  [Boşluk] Seç / Kaldır  |  [A] Tümünü Seç  |  [Enter] Sıfırlamayı Başlat  |  [Esc] İptal\r\n"),
            ResetColor
        )?;
        out.flush()?;

        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Up => {
                    sel = sel.saturating_sub(1);
                }
                KeyCode::Down => {
                    if sel + 1 < app.preserved_folders.len() {
                        sel += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    app.preserved_folders[sel].selected = !app.preserved_folders[sel].selected;
                }
                KeyCode::Char('a') | KeyCode::Char('A') => {
                    let all_sel = app.preserved_folders.iter().all(|f| f.selected);
                    for f in &mut app.preserved_folders {
                        f.selected = !all_sel;
                    }
                }
                KeyCode::Esc => return Ok(()),
                KeyCode::Enter => {
                    if !prompt_auth_in_tui(app, "VERİLERİ KORU VE SIFIRLA — ZORUNLU PAROLA ONAYI")? {
                        return Ok(());
                    }
                    return execute_preserved_reset(&app.target_root, &app.preserved_folders);
                }
                _ => {}
            }
        }
    }
}

/// Enforces strict permissions on `/home/preserved_data` (`0755`) and any `.ssh` (`0700`, files `0600`)
/// or `.gnupg` (`0700`) directories inside it. Never sets `0777`.
pub fn secure_preserved_data_permissions(dest_preserved: &Path) {
    if !dest_preserved.exists() {
        return;
    }
    let _ = fs::set_permissions(dest_preserved, fs::Permissions::from_mode(0o755));
    if let Ok(users) = fs::read_dir(dest_preserved) {
        for u in users.flatten() {
            let user_dir = u.path();
            if user_dir.is_dir() {
                let _ = fs::set_permissions(&user_dir, fs::Permissions::from_mode(0o755));
                let ssh_dir = user_dir.join(".ssh");
                if ssh_dir.is_dir() {
                    let _ = fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700));
                    if let Ok(files) = fs::read_dir(&ssh_dir) {
                        for f in files.flatten() {
                            let fp = f.path();
                            if fp.is_file() {
                                let fname = fp.file_name().unwrap_or_default().to_string_lossy();
                                let mode = if fname.ends_with(".pub") { 0o644 } else { 0o600 };
                                let _ = fs::set_permissions(&fp, fs::Permissions::from_mode(mode));
                            }
                        }
                    }
                }
                let gnupg_dir = user_dir.join(".gnupg");
                if gnupg_dir.is_dir() {
                    let _ = fs::set_permissions(&gnupg_dir, fs::Permissions::from_mode(0o700));
                    if let Ok(files) = fs::read_dir(&gnupg_dir) {
                        for f in files.flatten() {
                            let fp = f.path();
                            if fp.is_file() {
                                let _ = fs::set_permissions(&fp, fs::Permissions::from_mode(0o600));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn execute_preserved_reset(target_root: &Path, folders: &[PreservedFolder]) -> Result<()> {
    let mut out = stdout();
    let selected_count = folders.iter().filter(|f| f.selected).count();

    execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    execute!(
        out,
        SetForegroundColor(Color::Red),
        SetBackgroundColor(Color::Black),
        Print("  [ KRİTİK İŞLEM ONAYI ]\r\n\r\n"),
        ResetColor,
        SetForegroundColor(Color::White),
        Print(format!("  * Seçilen korunacak klasör sayısı: {}\r\n", selected_count)),
        Print("  * Sistem adımları:\r\n"),
        Print("     1. Seçilen kullanıcı klasörleri korumalı yedekleme alanına (/var/tmp/preserved_stage/<kullanıcı>) alınacak.\r\n"),
        Print("     2. İşletim sistemi kök dizini temiz fabrika durumuna sıfırlanacak.\r\n"),
        Print("     3. Korunan verileriniz /home/preserved_data/<kullanıcı>/<klasör> altına güvenli izinlerle geri yüklenecek.\r\n"),
        Print("     4. Kullanıcı yetkileri ve SELinux etiketleri otomatik onarılacak.\r\n\r\n"),
        SetForegroundColor(Color::Yellow),
        Print("  Sıfırlama işlemini başlatmak için [E]vet, vazgeçmek için [H]ayır basınız: "),
        ResetColor
    )?;
    out.flush()?;

    loop {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('e') | KeyCode::Char('E') | KeyCode::Char('y') | KeyCode::Char('Y') => {
                    execute!(
                        out,
                        SetForegroundColor(Color::Cyan),
                        Print("\r\n\r\n  [1/4] Seçilen veriler güvene alınıyor (/var/tmp/preserved_stage)...\r\n")
                    )?;
                    out.flush()?;

                    let stage_dir = Path::new("/var/tmp/preserved_stage");
                    let _ = fs::remove_dir_all(stage_dir);
                    let _ = fs::create_dir_all(stage_dir);
                    let _ = fs::set_permissions(stage_dir, fs::Permissions::from_mode(0o700));

                    for folder in folders.iter().filter(|f| f.selected) {
                        execute!(out, Print(format!("         -> Kopyalanıyor: {}\r\n", folder.name)))?;
                        out.flush()?;
                        let user_stage = stage_dir.join(&folder.username);
                        let _ = fs::create_dir_all(&user_stage);
                        let target = user_stage.join(&folder.folder_name);
                        let _ = Command::new("cp")
                            .args([
                                "-a",
                                folder.path.to_str().unwrap_or(""),
                                target.to_str().unwrap_or(""),
                            ])
                            .status();
                    }

                    execute!(
                        out,
                        Print("  [2/4] İşletim sistemi fabrika durumuna sıfırlanıyor...\r\n")
                    )?;
                    out.flush()?;
                    execute_factory_reset_core_at(target_root)?;

                    execute!(
                        out,
                        Print("  [3/4] Korunan veriler kullanıcı alanına taşınıyor (/home/preserved_data)...\r\n")
                    )?;
                    out.flush()?;
                    let dest_preserved = target_root.join("home/preserved_data");
                    let _ = fs::create_dir_all(&dest_preserved);
                    let _ = fs::set_permissions(&dest_preserved, fs::Permissions::from_mode(0o755));
                    let _ = Command::new("cp")
                        .args([
                            "-a",
                            "/var/tmp/preserved_stage/.",
                            dest_preserved.to_str().unwrap_or("/home/preserved_data"),
                        ])
                        .status();
                    let _ = fs::remove_dir_all("/var/tmp/preserved_stage");
                    secure_preserved_data_permissions(&dest_preserved);

                    execute!(
                        out,
                        Print("  [4/4] SELinux ve kullanıcı sahiplik etiketleri onarılıyor...\r\n")
                    )?;
                    let home_target = target_root.join("home");
                    let _ = Command::new("restorecon")
                        .args(["-Rv", home_target.to_str().unwrap_or("/home")])
                        .status();
                    let _ = Command::new("sync").status();
                    clear_boot_triggers();

                    execute!(
                        out,
                        SetForegroundColor(Color::Green),
                        Print("\r\n  [TAMAMLANDI] Sistem başarıyla sıfırlandı ve kişisel verileriniz korundu!\r\n"),
                        ResetColor,
                        Print("\r\n  [Enter] tuşuna basarak sistemi yeni ve temiz haliyle başlatabilirsiniz...\r\n")
                    )?;
                    out.flush()?;
                    wait_for_escape_or_enter()?;
                    do_reboot()?;
                    return Ok(());
                }
                KeyCode::Char('h')
                | KeyCode::Char('H')
                | KeyCode::Char('n')
                | KeyCode::Char('N')
                | KeyCode::Esc => {
                    return Ok(());
                }
                _ => {}
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Submenu: Factory Reset & OOBE Restoration
// -----------------------------------------------------------------------------

fn clean_dir_contents(path: &Path) {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let fname = entry.file_name();
            let fname_str = fname.to_string_lossy();
            if matches!(
                fname_str.as_ref(),
                ".snapshots" | ".blaze-snapshots" | ".blaze-isos" | "lost+found"
            ) {
                continue;
            }
            let p = entry.path();
            if p.is_dir() {
                let _ = fs::remove_dir_all(&p);
            } else {
                let _ = fs::remove_file(&p);
            }
        }
    }
}

fn filter_user_from_colon_file(file_path: &str, users_to_remove: &HashSet<String>) {
    if let Ok(content) = fs::read_to_string(file_path) {
        let mut filtered = Vec::new();
        for line in content.lines() {
            let uname = line.split(':').next().unwrap_or("");
            if !users_to_remove.contains(uname) {
                filtered.push(line);
            }
        }
        let _ = fs::write(file_path, filtered.join("\n") + "\n");
    }
}

pub fn wipe_human_users() -> Result<()> {
    wipe_human_users_at(Path::new("/"))
}

pub fn wipe_human_users_at(target_root: &Path) -> Result<()> {
    let mut human_users = HashSet::new();
    let passwd_path = target_root.join("etc/passwd");
    if let Ok(content) = fs::read_to_string(&passwd_path) {
        for line in content.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 3 {
                if let Ok(uid) = parts[2].parse::<u32>() {
                    let user = parts[0];
                    if (1000..60000).contains(&uid)
                        && user != "nobody"
                        && user != "nfsnobody"
                        && user != "liveuser"
                        && user != "blaze-setup"
                    {
                        human_users.insert(user.to_string());
                    }
                }
            }
        }
    }

    for user in &human_users {
        if target_root == Path::new("/") {
            let _ = Command::new("userdel").args(["-rf", user]).status();
        } else {
            let _ = fs::remove_dir_all(target_root.join("home").join(user));
        }
        let _ = fs::remove_file(target_root.join(format!("etc/sudoers.d/{}", user)));
        let _ = fs::remove_file(target_root.join(format!("var/lib/AccountsService/users/{}", user)));
    }

    if !human_users.is_empty() {
        for db in [
            "etc/passwd",
            "etc/shadow",
            "etc/group",
            "etc/gshadow",
            "etc/subuid",
            "etc/subgid",
        ] {
            let db_p = target_root.join(db);
            if let Some(s) = db_p.to_str() {
                filter_user_from_colon_file(s, &human_users);
            }
        }
    }
    Ok(())
}

pub fn execute_factory_reset_core() -> Result<()> {
    execute_factory_reset_core_at(Path::new("/"))
}

pub fn execute_factory_reset_core_at(target_root: &Path) -> Result<()> {
    remount_filesystems_rw();
    let _ = wipe_human_users_at(target_root);
    clean_dir_contents(&target_root.join("home"));
    clean_dir_contents(&target_root.join("tmp"));
    clean_dir_contents(&target_root.join("var/log"));
    clean_dir_contents(&target_root.join("var/cache/dnf"));
    clean_dir_contents(&target_root.join("var/cache/flatpak"));
    prepare_oobe_reset_at(target_root)?;
    let home_p = target_root.join("home");
    let _ = Command::new("restorecon")
        .args(["-Rv", home_p.to_str().unwrap_or("/home")])
        .status();
    let _ = Command::new("sync").status();
    clear_boot_triggers();
    Ok(())
}

fn handle_factory_reset(app: &mut BerpApp) -> Result<()> {
    let mut out = stdout();
    execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    execute!(
        out,
        SetForegroundColor(Color::White),
        SetBackgroundColor(Color::Red),
        Print("  === DİKKAT: FABRİKA AYARLARINA TAM SIFIRLAMA ===  \r\n"),
        ResetColor,
        SetForegroundColor(Color::Red),
        Print("\r\n  UYARI: Bu işlem diskteki tüm kullanıcı verilerini ve ayarları silecek;\r\n"),
        Print("  sistemi ilk kurulum anındaki orijinal durumuna döndürecektir.\r\n\r\n"),
        SetForegroundColor(Color::Yellow),
        Print("  Tüm verileri silip sıfırlamak istediğinizden kesinlikle emin misiniz?\r\n"),
        Print("  Onaylamak için [S][I][F][I][R][L][A] yazın veya vazgeçmek için [Esc] basınız: "),
        ResetColor
    )?;
    out.flush()?;

    let mut input = String::new();
    loop {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc => return Ok(()),
                KeyCode::Backspace => {
                    if !input.is_empty() {
                        input.pop();
                        execute!(out, cursor::MoveLeft(1), Print(" "), cursor::MoveLeft(1))?;
                        out.flush()?;
                    }
                }
                KeyCode::Enter => {
                    if input.to_lowercase() == "sifirla"
                        || input.to_lowercase() == "sıfırla"
                        || input.to_lowercase() == "reset"
                    {
                        if !prompt_auth_in_tui(app, "FABRİKA SIFIRLAMASI — ZORUNLU PAROLA ONAYI")? {
                            return Ok(());
                        }
                        execute!(
                            out,
                            SetForegroundColor(Color::Red),
                            Print("\r\n\r\n  -> Fabrika ayarlarına sıfırlama işlemi başlatılıyor...\r\n")
                        )?;
                        out.flush()?;
                        execute_factory_reset_core_at(&app.target_root)?;
                        execute!(
                            out,
                            SetForegroundColor(Color::Green),
                            Print("  [BAŞARILI] Sistem fabrika ayarlarına döndürüldü.\r\n"),
                            ResetColor,
                            Print("\r\n  [Enter] ile yeniden başlatabilirsiniz...\r\n")
                        )?;
                        out.flush()?;
                        wait_for_escape_or_enter()?;
                        do_reboot()?;
                        return Ok(());
                    } else {
                        execute!(
                            out,
                            SetForegroundColor(Color::Red),
                            Print("\r\n  Hatalı onay kelimesi. İptal edildi.\r\n"),
                            ResetColor
                        )?;
                        out.flush()?;
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                        return Ok(());
                    }
                }
                KeyCode::Char(c) => {
                    input.push(c);
                    execute!(out, Print(c))?;
                    out.flush()?;
                }
                _ => {}
            }
        }
    }
}

/// Return the installed system to the first-boot OOBE.
/// Recreates `/etc/sudoers.d/99-blaze-setup` (`0440`), configures `/etc/sddm.conf.d/autologin.conf`
/// and `00-blaze-oobe.conf`, and removes firstboot completion markers.
pub fn prepare_oobe_reset() -> Result<()> {
    prepare_oobe_reset_at(Path::new("/"))
}

pub fn prepare_oobe_reset_at(target_root: &Path) -> Result<()> {
    for marker in [
        "etc/blazeos-setup-completed",
        "etc/blaze-firstboot-done",
        "var/lib/solar-firstboot/done",
    ] {
        let _ = fs::remove_file(target_root.join(marker));
    }

    // Ensure sudoers.d rule for blaze-setup with mode 0440
    let sudoers_dir = target_root.join("etc/sudoers.d");
    fs::create_dir_all(&sudoers_dir)?;
    let sudoers_file = sudoers_dir.join("99-blaze-setup");
    fs::write(&sudoers_file, "blaze-setup ALL=(ALL) NOPASSWD: ALL\n")?;
    let _ = fs::set_permissions(&sudoers_file, fs::Permissions::from_mode(0o440));

    // Configure SDDM autologin to blaze-setup / blaze-oobe.desktop and clear stale state.conf
    let _ = fs::remove_file(target_root.join("var/lib/sddm/state.conf"));
    let _ = fs::remove_file(target_root.join("etc/sddm.conf.d/autologin.conf"));
    let sddm_dir = target_root.join("etc/sddm.conf.d");
    fs::create_dir_all(&sddm_dir)?;
    let oobe_content =
        "[Autologin]\nUser=blaze-setup\nSession=blaze-oobe.desktop\nRelogin=false\n\n[Users]\nMinimumUid=900\nRememberLastUser=false\nRememberLastSession=false\n";
    fs::write(sddm_dir.join("00-blaze-oobe.conf"), oobe_content)?;

    let oobe_template = target_root.join("usr/share/blaze-setup/blaze-oobe.desktop");
    let oobe_session = target_root.join("usr/share/wayland-sessions/blaze-oobe.desktop");
    if oobe_template.is_file() {
        if let Some(parent) = oobe_session.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(&oobe_template, &oobe_session);
    }

    let setup_home = target_root.join("var/lib/blaze-setup");
    let _ = fs::create_dir_all(&setup_home);

    // Ensure blaze-setup is unlocked in /etc/shadow and uses /bin/bash in /etc/passwd
    let passwd_p = target_root.join("etc/passwd");
    if let Ok(pw_txt) = fs::read_to_string(&passwd_p) {
        let updated_pw: Vec<String> = pw_txt
            .lines()
            .map(|l| {
                if l.starts_with("blaze-setup:") {
                    let mut cols: Vec<&str> = l.split(':').collect();
                    if cols.len() >= 7 {
                        cols[6] = "/bin/bash";
                        return cols.join(":");
                    }
                }
                l.to_string()
            })
            .collect();
        let _ = fs::write(&passwd_p, updated_pw.join("\n") + "\n");
    }

    let shadow_p = target_root.join("etc/shadow");
    if let Ok(sh_txt) = fs::read_to_string(&shadow_p) {
        let mut found_bs = false;
        let mut updated_sh: Vec<String> = sh_txt
            .lines()
            .map(|l| {
                if l.starts_with("blaze-setup:") {
                    found_bs = true;
                    "blaze-setup::20726::::::".to_string()
                } else {
                    l.to_string()
                }
            })
            .collect();
        if !found_bs {
            updated_sh.push("blaze-setup::20726::::::".to_string());
        }
        let _ = fs::write(&shadow_p, updated_sh.join("\n") + "\n");
        let _ = fs::set_permissions(&shadow_p, fs::Permissions::from_mode(0o000));
    }

    if target_root == Path::new("/") {
        if Command::new("id")
            .args(["-u", "blaze-setup"])
            .status()
            .map(|s| !s.success())
            .unwrap_or(true)
        {
            let _ = Command::new("useradd")
                .args([
                    "--system",
                    "--uid",
                    "961",
                    "--create-home",
                    "--home-dir",
                    "/var/lib/blaze-setup",
                    "--shell",
                    "/bin/bash",
                    "blaze-setup",
                ])
                .status();
        }
        let _ = Command::new("usermod")
            .args(["-s", "/bin/bash", "-aG", "wheel,video,audio,input,render", "blaze-setup"])
            .status();
        let _ = Command::new("chown")
            .args(["-R", "961:961", "/var/lib/blaze-setup"])
            .status();
        let _ = Command::new("systemctl").args(["enable", "sddm.service"]).status();
    }
    Ok(())
}

// -----------------------------------------------------------------------------
// Submenu: Diagnostics (Real Read-Only Hardware & FS Checks)
// -----------------------------------------------------------------------------

fn handle_diagnostics(app: &BerpApp) -> Result<()> {
    let mut out = stdout();
    execute!(out, cursor::MoveTo(0, 0), Clear(ClearType::All))?;
    execute!(
        out,
        SetForegroundColor(Color::Black),
        SetBackgroundColor(Color::Blue),
        Print("  === SİSTEM VE DONANIM SAĞLIK TEŞHİSİ ===  \r\n"),
        ResetColor,
        Print("\r\n  Donanım, dosya sistemi ve güvenlik durumu gerçek zamanlı taranıyor...\r\n\r\n")
    )?;
    out.flush()?;

    // 1. RAM info
    let mem_info = fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let mut mem_total = "Bilinmiyor".to_string();
    let mut mem_avail = "Bilinmiyor".to_string();
    for l in mem_info.lines() {
        if l.starts_with("MemTotal:") {
            mem_total = l.split_whitespace().nth(1).unwrap_or("").to_string() + " kB";
        }
        if l.starts_with("MemAvailable:") {
            mem_avail = l.split_whitespace().nth(1).unwrap_or("").to_string() + " kB";
        }
    }

    execute!(
        out,
        SetForegroundColor(Color::Green),
        Print("  [1] Bellek (RAM) Durumu:\r\n"),
        SetForegroundColor(Color::White),
        Print(format!("      * Toplam RAM:       {}\r\n", mem_total)),
        Print(format!("      * Kullanılabilir:   {}\r\n\r\n", mem_avail)),
        ResetColor
    )?;
    out.flush()?;

    // 2. Storage & SMART check
    execute!(
        out,
        SetForegroundColor(Color::Green),
        Print("  [2] Depolama Aygıtları & SMART Durumu:\r\n"),
        SetForegroundColor(Color::White)
    )?;
    if let Ok(disks) = Command::new("lsblk")
        .args(["-d", "-n", "-o", "NAME,SIZE,MODEL,TYPE"])
        .output()
    {
        let s = String::from_utf8_lossy(&disks.stdout);
        for line in s.lines() {
            execute!(out, Print(format!("      * {}\r\n", line)))?;
            let dev_name = line.split_whitespace().next().unwrap_or("");
            if !dev_name.is_empty() && !dev_name.starts_with("loop") && !dev_name.starts_with("zram") {
                let dev_path = format!("/dev/{}", dev_name);
                if let Ok(smart) = Command::new("smartctl").args(["-H", &dev_path]).output() {
                    let smart_out = String::from_utf8_lossy(&smart.stdout);
                    for sline in smart_out.lines() {
                        if sline.contains("SMART overall-health") || sline.contains("SMART Health Status") {
                            execute!(out, Print(format!("        -> {}\r\n", sline.trim())))?;
                        }
                    }
                }
            }
        }
    }
    execute!(out, Print("\r\n"), ResetColor)?;
    out.flush()?;

    // 3. Filesystem Check (Btrfs device stats + scrub status, or dumpe2fs on ext4)
    execute!(
        out,
        SetForegroundColor(Color::Green),
        Print("  [3] Dosya Sistemi Bütünlük Denetimi:\r\n"),
        SetForegroundColor(Color::White)
    )?;
    if app.root_btrfs {
        if let Ok(stats) = Command::new("btrfs").args(["device", "stats", "/"]).output() {
            let stats_str = String::from_utf8_lossy(&stats.stdout);
            for line in stats_str.lines().take(6) {
                execute!(out, Print(format!("      * {}\r\n", line)))?;
            }
        }
        if let Ok(scrub) = Command::new("btrfs").args(["scrub", "status", "/"]).output() {
            let scrub_str = String::from_utf8_lossy(&scrub.stdout);
            for line in scrub_str.lines().take(4) {
                execute!(out, Print(format!("      * {}\r\n", line)))?;
            }
        }
    } else if let Ok(findmnt_out) = Command::new("findmnt")
        .args(["-n", "-o", "FSTYPE,SOURCE", "/"])
        .output()
    {
        let out_str = String::from_utf8_lossy(&findmnt_out.stdout);
        let parts: Vec<&str> = out_str.split_whitespace().collect();
        let fstype = parts.first().copied().unwrap_or("unknown");
        let root_dev = strip_btrfs_subvol_suffix(parts.get(1).copied().unwrap_or(""));
        execute!(
            out,
            Print(format!("      * Kök dosya sistemi: {} ({})\r\n", fstype, root_dev))
        )?;
        if fstype == "ext4" && !root_dev.is_empty() {
            if let Ok(d2fs) = Command::new("dumpe2fs").args(["-h", &root_dev]).output() {
                let d_str = String::from_utf8_lossy(&d2fs.stdout);
                for line in d_str.lines() {
                    if line.starts_with("Filesystem state:") || line.starts_with("Errors behavior:") {
                        execute!(out, Print(format!("      * {}\r\n", line)))?;
                    }
                }
            }
        }
    }
    execute!(out, Print("\r\n"), ResetColor)?;

    // 4. Blaze Sentinel Security Posture
    execute!(
        out,
        SetForegroundColor(Color::Green),
        Print("  [4] Blaze Sentinel Güvenlik Durumu:\r\n"),
        SetForegroundColor(Color::White)
    )?;
    if let Ok(sent) = Command::new("blaze-sentinel").arg("status").output() {
        let s_str = String::from_utf8_lossy(&sent.stdout);
        for line in s_str.lines() {
            execute!(out, Print(format!("      {}\r\n", line)))?;
        }
    } else {
        execute!(out, Print("      * blaze-sentinel ikilisi çalıştırılamadı.\r\n"))?;
    }
    execute!(out, Print("\r\n"), ResetColor)?;

    execute!(
        out,
        SetForegroundColor(Color::Cyan),
        Print("  [Teşhis Tamamlandı]\r\n\r\n"),
        SetForegroundColor(Color::DarkGrey),
        Print("  [Esc / Enter] Ana Menüye Dön\r\n"),
        ResetColor
    )?;
    out.flush()?;
    wait_for_escape_or_enter()?;
    Ok(())
}

// -----------------------------------------------------------------------------
// Submenu: Emergency Shell
// -----------------------------------------------------------------------------

fn handle_emergency_shell() -> Result<()> {
    let mut out = stdout();
    execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;
    terminal::disable_raw_mode()?;

    println!("\n=======================================================");
    println!("  BLAZE EMERGENCY RECOVERY ROOT SHELL");
    println!("  Kurtarma konsolundan çıkıp BERP menüsüne dönmek için: exit");
    println!("=======================================================\n");

    let shell = if Path::new("/bin/bash").is_file() {
        "/bin/bash"
    } else {
        "/bin/sh"
    };
    let _ = Command::new(shell).status();

    terminal::enable_raw_mode()?;
    execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;
    Ok(())
}

pub fn clear_boot_triggers() {
    for genv in ["/boot/grub2/grubenv", "/mnt/bere_target/boot/grub2/grubenv"] {
        if Path::new(genv).exists() {
            for args in [
                [genv, "set", "boot_counter=0"],
                [genv, "set", "boot_success=1"],
                [genv, "set", "blaze_recovery=-"],
                [genv, "unset", "blaze_recovery"],
            ] {
                let _ = Command::new("grub2-editenv")
                    .args(args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
        }
    }

    for trigger in [
        "/var/run/blaze_recovery_trigger",
        "/run/blaze_recovery_trigger",
        "/var/lib/blaze/berp-trigger",
        "/boot/bere/recovery_triggered",
        "/etc/blaze_recovery_alert",
        "/run/blaze_recovery_alert",
    ] {
        let _ = fs::remove_file(trigger);
    }

    if Path::new("/var/lib/blaze").exists() || Path::new("/var/lib/blaze/berp-counter").exists() {
        let _ = fs::create_dir_all("/var/lib/blaze");
        let _ = fs::set_permissions("/var/lib/blaze", fs::Permissions::from_mode(0o700));
        let _ = fs::write("/var/lib/blaze/berp-counter", "0\n");
    }
}

fn do_reboot() -> Result<()> {
    clear_boot_triggers();
    let mut out = stdout();
    let _ = execute!(out, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    println!("Sistem yeniden başlatılıyor...");
    let _ = Command::new("sync").status();
    if Path::new("/mnt/bere_target").is_dir() {
        let _ = Command::new("umount").args(["-R", "/mnt/bere_target"]).status();
    }
    let _ = Command::new("systemctl").arg("reboot").status();
    let _ = Command::new("systemctl").args(["reboot", "--force"]).status();
    let _ = Command::new("reboot").arg("-f").status();
    Ok(())
}

fn do_poweroff() -> Result<()> {
    clear_boot_triggers();
    let mut out = stdout();
    let _ = execute!(out, cursor::Show, terminal::LeaveAlternateScreen);
    let _ = terminal::disable_raw_mode();
    println!("Sistem kapatılıyor...");
    let _ = Command::new("sync").status();
    if Path::new("/mnt/bere_target").is_dir() {
        let _ = Command::new("umount").args(["-R", "/mnt/bere_target"]).status();
    }
    let _ = Command::new("systemctl").arg("poweroff").status();
    let _ = Command::new("systemctl").args(["poweroff", "--force"]).status();
    let _ = Command::new("poweroff").arg("-f").status();
    Ok(())
}

fn wait_for_escape_or_enter() -> Result<()> {
    loop {
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => return Ok(()),
                _ => {}
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Main Entrypoint
// -----------------------------------------------------------------------------

fn force_tty1_text_mode() {
    unsafe extern "C" {
        fn ioctl(fd: std::os::raw::c_int, request: std::os::raw::c_ulong, ...) -> std::os::raw::c_int;
    }
    const KDSETMODE: std::os::raw::c_ulong = 0x4B3A;
    const KD_TEXT: std::os::raw::c_int = 0x00;
    const VT_ACTIVATE: std::os::raw::c_ulong = 0x5606;
    const VT_WAITACTIVE: std::os::raw::c_ulong = 0x5607;

    use std::os::unix::io::AsRawFd;
    for path in ["/dev/tty1", "/dev/tty0", "/dev/console"] {
        if let Ok(f) = fs::OpenOptions::new().read(true).write(true).open(path) {
            unsafe {
                let _ = ioctl(f.as_raw_fd(), KDSETMODE, KD_TEXT);
                let _ = ioctl(f.as_raw_fd(), VT_ACTIVATE, 1 as std::os::raw::c_int);
                let _ = ioctl(f.as_raw_fd(), VT_WAITACTIVE, 1 as std::os::raw::c_int);
            }
        }
    }
    unsafe {
        let _ = ioctl(0, KDSETMODE, KD_TEXT);
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        match args[1].as_str() {
            "--kd-text" => {
                force_tty1_text_mode();
                return Ok(());
            }
            "--version" | "-V" | "version" => {
                println!("blaze-recovery {}", BERP_VERSION);
                return Ok(());
            }
            "--status" | "status" => {
                let app = BerpApp::new_readonly();
                println!(
                    "BERP Status: Btrfs={}, Subvol={}, UUID={}, BootCounter={}, RecoveryRequested={}, Reason={:?}, Snapshots={}",
                    app.root_btrfs,
                    app.active_subvol,
                    app.root_uuid,
                    app.boot_counter,
                    app.recovery_requested,
                    app.recovery_reason,
                    app.snapshots.len()
                );
                return Ok(());
            }
            "--list-snapshots" | "list-snapshots" => {
                let app = BerpApp::new_readonly();
                if app.snapshots.is_empty() {
                    println!("No snapshots found.");
                } else {
                    for s in &app.snapshots {
                        println!(
                            "[{}] {} | {} | {} | {}",
                            s.id,
                            s.name,
                            s.path.display(),
                            s.date,
                            s.description
                        );
                    }
                }
                return Ok(());
            }
            "--verify" | "verify" => {
                let app = BerpApp::new_readonly();
                println!(
                    "BERP Verify: version={}, root_btrfs={}, active_subvol={}, root_uuid={}, snapshots={}, recovery_reason={:?}",
                    BERP_VERSION,
                    app.root_btrfs,
                    app.active_subvol,
                    app.root_uuid,
                    app.snapshots.len(),
                    app.recovery_reason
                );
                return Ok(());
            }
            "--verify-auth" | "verify-auth" => {
                let mut sysroot_arg: Option<PathBuf> = None;
                let mut i = 2;
                while i < args.len() {
                    if args[i] == "--sysroot" && i + 1 < args.len() {
                        sysroot_arg = Some(PathBuf::from(&args[i + 1]));
                        i += 2;
                        continue;
                    } else if let Some(rest) = args[i].strip_prefix("--sysroot=") {
                        sysroot_arg = Some(PathBuf::from(rest));
                    }
                    i += 1;
                }
                let ok = prompt_and_verify_admin_auth(
                    sysroot_arg.as_deref(),
                    "BLAZEOS ACİL DURUM KÖK KONSOLU KİLİDİ",
                )?;
                if !ok {
                    std::process::exit(1);
                }
                return Ok(());
            }
            "--trigger" | "trigger" => {
                if Path::new("/boot/grub2/grubenv").exists() {
                    let _ = Command::new("grub2-editenv")
                        .args(["/boot/grub2/grubenv", "set", "blaze_recovery=alert"])
                        .status();
                }
                let _ = fs::write("/var/run/blaze_recovery_trigger", "1\n");
                if fs::create_dir_all("/var/lib/blaze").is_ok() {
                    let _ = fs::set_permissions("/var/lib/blaze", fs::Permissions::from_mode(0o700));
                    let _ = fs::write("/var/lib/blaze/berp-trigger", "1\n");
                }
                println!("BERP: Acil kurtarma bir sonraki açılış için tetiklendi.");
                return Ok(());
            }
            "--clear-trigger" | "clear-trigger" => {
                clear_boot_triggers();
                println!("BERP: Kurtarma bayrakları ve sayaçlar sıfırlandı.");
                return Ok(());
            }
            "--help" | "-h" | "help" => {
                println!("Blaze Emergency Recovery Protocol (BERP) v{}", BERP_VERSION);
                println!("Usage: blaze-recovery [--status | --list-snapshots | --verify | --trigger | --clear-trigger | --version | --help]");
                return Ok(());
            }
            other => {
                eprintln!("blaze-recovery: unknown command or option: {}", other);
                eprintln!("Usage: blaze-recovery [--status | --list-snapshots | --verify | --trigger | --clear-trigger | --version | --help]");
                std::process::exit(2);
            }
        }
    }

    let app = BerpApp::new();
    run_interactive_tui(app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_btrfs_subvol_suffix() {
        assert_eq!(strip_btrfs_subvol_suffix("/dev/sda3[/root]"), "/dev/sda3");
        assert_eq!(strip_btrfs_subvol_suffix("/dev/nvme0n1p2[/@]"), "/dev/nvme0n1p2");
        assert_eq!(strip_btrfs_subvol_suffix("/dev/vda2"), "/dev/vda2");
    }

    #[test]
    fn test_normalize_subvol_snapshot_path() {
        assert_eq!(
            normalize_subvol_snapshot_path("root/.snapshots/snap1", "root"),
            Some(PathBuf::from("/.snapshots/snap1"))
        );
        assert_eq!(
            normalize_subvol_snapshot_path("root/.blaze-snapshots/20261026T120000Z", "root"),
            Some(PathBuf::from("/.blaze-snapshots/20261026T120000Z"))
        );
        // Never allow home snapshots as root rollback targets
        assert_eq!(
            normalize_subvol_snapshot_path("home/.snapshots/snap1_home", "root"),
            None
        );
        assert_eq!(
            normalize_subvol_snapshot_path("root/.snapshots/snap1_home", "root"),
            None
        );
        // Never allow container subvolume itself
        assert_eq!(
            normalize_subvol_snapshot_path("root/.blaze-snapshots", "root"),
            None
        );
    }

    #[test]
    fn test_resolve_btrfs_snapshot_source_nested() {
        let tmp = std::env::temp_dir().join(format!("berp_test_{}", std::process::id()));
        let old_sub = tmp.join("root_old_12345");
        let nested_snap = old_sub.join(".blaze-snapshots/20261026T120000Z");
        fs::create_dir_all(&nested_snap).unwrap();

        let resolved = resolve_btrfs_snapshot_source(
            &tmp,
            "root",
            &old_sub,
            Path::new("/.blaze-snapshots/20261026T120000Z"),
        );
        assert_eq!(resolved, nested_snap);

        let new_root = tmp.join("root");
        fs::create_dir_all(new_root.join(".blaze-snapshots")).unwrap();
        preserve_nested_snapshot_containers(&old_sub, &new_root);
        assert!(new_root.join(".blaze-snapshots/20261026T120000Z").exists());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_parse_blkid_export_and_external_snapshot_discovery() {
        let sample = "DEVNAME=/dev/vda1\nUUID=1111-2222\nTYPE=vfat\n\nDEVNAME=/dev/vda3\nUUID=abcd-ef01\nLABEL=fedora\nTYPE=btrfs\n";
        let blocks = parse_blkid_export_blocks(sample);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[1].0, "/dev/vda3");
        assert_eq!(blocks[1].1, "btrfs");
        assert_eq!(blocks[1].2, "abcd-ef01");

        let tmp = std::env::temp_dir().join(format!("berp_ext_{}", std::process::id()));
        fs::create_dir_all(tmp.join(".blaze-snapshots/golden-init")).unwrap();
        fs::create_dir_all(tmp.join(".snapshots/user_home")).unwrap();
        let found = discover_snapshots_at(&tmp, false, "root");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "golden-init");
        assert_eq!(found[0].path, PathBuf::from("/.blaze-snapshots/golden-init"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_collect_and_verify_admin_hashes_root_and_wheel_policy_a1() {
        let tmp = std::env::temp_dir().join(format!("berp_auth_{}", std::process::id()));
        let etc_dir = tmp.join("etc");
        fs::create_dir_all(&etc_dir).unwrap();

        let wheel_pw = "BlazeWheel#2026";
        let root_pw = "BlazeRoot#2026";
        let non_admin_pw = "Guest#2026";

        // Generate valid SHA-512 crypt hashes via libc/libcrypt
        let hash_of = |pw: &str, salt: &str| -> String {
            let c_pw = CString::new(pw).unwrap();
            let c_salt = CString::new(salt).unwrap();
            let _g = CRYPT_CALL_LOCK.lock().unwrap();
            unsafe { CStr::from_ptr(crypt(c_pw.as_ptr(), c_salt.as_ptr())) }
                .to_string_lossy()
                .into_owned()
        };

        let wheel_hash = hash_of(wheel_pw, "$6$blazetest1$");
        let root_hash = hash_of(root_pw, "$6$blazetest2$");
        let guest_hash = hash_of(non_admin_pw, "$6$blazetest3$");

        fs::write(
            etc_dir.join("group"),
            "root:x:0:\nwheel:x:10:blazeuser\nusers:x:100:\n",
        )
        .unwrap();
        fs::write(
            etc_dir.join("passwd"),
            "root:x:0:0:root:/root:/bin/bash\nblazeuser:x:1000:1000:Blaze:/home/blazeuser:/bin/bash\nguest:x:1001:100:Guest:/home/guest:/bin/bash\n",
        )
        .unwrap();

        // Case 1: OOBE locked root (!), unlocked wheel user -> accepts wheel_pw ONLY
        fs::write(
            etc_dir.join("shadow"),
            format!(
                "root:!:19000:0:99999:7:::\nblazeuser:{}:19000:0:99999:7:::\nguest:{}:19000:0:99999:7:::\n",
                wheel_hash, guest_hash
            ),
        )
        .unwrap();

        let hashes = collect_admin_hashes_at(&tmp);
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[0].0, "blazeuser");
        assert_eq!(verify_admin_password_at(&tmp, wheel_pw).as_deref(), Some("blazeuser"));
        assert_eq!(verify_admin_password_at(&tmp, non_admin_pw), None);
        assert_eq!(verify_admin_password_at(&tmp, "wrong"), None);
        assert_eq!(verify_admin_password_at(&tmp, ""), None);

        // Case 2: Both root and wheel active -> accepts both
        fs::write(
            etc_dir.join("shadow"),
            format!(
                "root:{}:19000:0:99999:7:::\nblazeuser:{}:19000:0:99999:7:::\n",
                root_hash, wheel_hash
            ),
        )
        .unwrap();
        assert_eq!(verify_admin_password_at(&tmp, root_pw).as_deref(), Some("root"));
        assert_eq!(verify_admin_password_at(&tmp, wheel_pw).as_deref(), Some("blazeuser"));

        let _ = fs::remove_dir_all(&tmp);
    }
}
