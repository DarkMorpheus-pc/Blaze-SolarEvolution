use anyhow::{bail, Result};
use solar_common::{ShellEngine, SolarConfig};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::{info, warn};

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/liveuser"))
}

fn stamp_path() -> PathBuf {
    home_dir().join(".local/state/solarui/last-wallpaper.txt")
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h1 = (bytes[i + 1] as char).to_digit(16);
            let h2 = (bytes[i + 2] as char).to_digit(16);
            if let (Some(a), Some(b)) = (h1, h2) {
                out.push(((a << 4) | b) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode_file_uri(abs_path: &str) -> String {
    let mut out = String::from("file://");
    for b in abs_path.bytes() {
        match b {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'/'
            | b'-'
            | b'_'
            | b'.'
            | b'~'
            | b'('
            | b')' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

pub fn normalize_wallpaper_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches('\'').trim_matches('"').trim();
    if trimmed.is_empty() {
        return None;
    }
    let without_scheme = if let Some(rest) = trimmed.strip_prefix("file://") {
        percent_decode(rest)
    } else {
        trimmed.to_string()
    };
    let expanded = if let Some(rest) = without_scheme.strip_prefix("~/") {
        home_dir().join(rest).to_string_lossy().into_owned()
    } else {
        without_scheme
    };
    let p = Path::new(&expanded);
    if !p.is_file() {
        return None;
    }
    Some(
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .into_owned(),
    )
}

pub fn current_effective_wallpaper() -> String {
    let cfg = SolarConfig::load();
    if cfg.shell.engine == ShellEngine::We10x {
        if !cfg.shell.we10x_wallpaper.trim().is_empty()
            && Path::new(&cfg.shell.we10x_wallpaper).is_file()
        {
            return cfg.shell.we10x_wallpaper;
        }
        if !cfg.display.wallpaper.trim().is_empty() && Path::new(&cfg.display.wallpaper).is_file() {
            return cfg.display.wallpaper;
        }
        return if cfg.shell.we10x_theme_mode.eq_ignore_ascii_case("light") {
            crate::we10x_profile::WE10X_WALLPAPER_LIGHT.to_string()
        } else {
            crate::we10x_profile::WE10X_WALLPAPER_DARK.to_string()
        };
    }

    if !cfg.display.wallpaper.trim().is_empty() && Path::new(&cfg.display.wallpaper).is_file() {
        return cfg.display.wallpaper;
    }

    for fallback in [
        "/usr/share/backgrounds/blazeos/default.png",
        "/usr/share/backgrounds/blazeos/Wallpaperdefault.jpg",
    ] {
        if Path::new(fallback).is_file() {
            return fallback.to_string();
        }
    }
    String::new()
}

fn friendly_wallpaper_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Duvar Kağıdı");
    match stem {
        "default" => "BlazeOS Varsayılan (Koyu Kaya)".to_string(),
        "Wallpaperdefault" => "BlazeOS Çöl Ufku".to_string(),
        "galaxy" => "BlazeOS Kozmik Galaksi".to_string(),
        "Wallpaper1s" => "BlazeOS Kanyon Kemeri".to_string(),
        "Wallpaper2s" => "BlazeOS Kumul Tepeleri".to_string(),
        "Wallpaper3s" => "BlazeOS Okyanus Kıyısı".to_string(),
        "Wallpaper4s" => "BlazeOS Zümrüt Zirve".to_string(),
        "Wallpaper5s" => "BlazeOS Gün Batımı Kumulu".to_string(),
        "wallpapergreen" => "BlazeOS Botanik Yeşil".to_string(),
        "wallpaperplant" => "BlazeOS Gece Yaprakları".to_string(),
        "wallpaperyellow" => "BlazeOS Kehribar Doku".to_string(),
        "We10XOS-dark" => "We10X Klasik Koyu (Mavi)".to_string(),
        "We10XOS-light" => "We10X Klasik Açık (Buz Mavisi)".to_string(),
        "We10XOS-black" => "We10X Koyu Antrasit / OLED".to_string(),
        "We10XOS-green" => "We10X Zümrüt Yeşili".to_string(),
        "We10XOS-purple" => "We10X Kraliyet Moru".to_string(),
        other => other.replace(['_', '-'], " "),
    }
}

pub fn discover_available_wallpapers() -> Vec<(String, String)> {
    let home = home_dir();
    let dirs = [
        PathBuf::from("/usr/share/backgrounds/blazeos"),
        PathBuf::from("/usr/share/backgrounds/blazeos/we10x"),
        home.join("Resimler/Wallpapers"),
        home.join("Pictures/Wallpapers"),
    ];

    let mut seen_names = std::collections::HashSet::new();
    let mut results = Vec::new();

    for dir in &dirs {
        let Ok(rd) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut files: Vec<PathBuf> = rd
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|x| x.to_str())
                        .map(|ext| {
                            matches!(
                                ext.to_ascii_lowercase().as_str(),
                                "png" | "jpg" | "jpeg" | "webp"
                            )
                        })
                        .unwrap_or(false)
            })
            .collect();
        files.sort();
        for f in files {
            let fname = f
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();
            if fname.is_empty() || !seen_names.insert(fname) {
                continue;
            }
            let label = friendly_wallpaper_name(&f);
            results.push((label, f.to_string_lossy().into_owned()));
        }
    }
    results
}

pub(crate) fn is_builtin_we10x_wallpaper(path: &str) -> bool {
    path.trim().starts_with("/usr/share/backgrounds/blazeos/we10x/")
}

pub(crate) fn read_noctalia_wallpaper_toml(home: &Path) -> Option<String> {
    for rel in [
        ".local/state/noctalia/settings.toml",
        ".config/noctalia/settings.toml",
    ] {
        let p = home.join(rel);
        if let Ok(content) = std::fs::read_to_string(&p) {
            if let Ok(doc) = content.parse::<toml::Value>() {
                if let Some(wp) = doc
                    .get("wallpaper")
                    .and_then(|w| w.get("default"))
                    .and_then(|d| d.get("path"))
                    .and_then(|v| v.as_str())
                {
                    let trimmed = wp.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn update_noctalia_wallpaper_toml(toml_path: &Path, clean_path: &str) {
    let mut doc = std::fs::read_to_string(toml_path)
        .ok()
        .and_then(|c| c.parse::<toml::Value>().ok())
        .unwrap_or_else(|| toml::Value::Table(toml::map::Map::new()));

    if !doc.is_table() {
        doc = toml::Value::Table(toml::map::Map::new());
    }
    if let Some(root) = doc.as_table_mut() {
        let wp = root
            .entry("wallpaper".to_string())
            .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
        if let Some(wpt) = wp.as_table_mut() {
            wpt.entry("enabled".to_string())
                .or_insert(toml::Value::Boolean(true));
            let def = wpt
                .entry("default".to_string())
                .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
            if let Some(deft) = def.as_table_mut() {
                deft.insert(
                    "path".to_string(),
                    toml::Value::String(clean_path.to_string()),
                );
            }
        }
    }

    if let Some(parent) = toml_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = toml::to_string_pretty(&doc) {
        let _ = solar_common::persistence::atomic_write(toml_path, s.as_bytes());
    }
}

fn sync_caelestia_wallpaper_files(home: &Path, clean_path: &str) {
    let wp_dir = home.join(".local/state/caelestia/wallpaper");
    if std::fs::create_dir_all(&wp_dir).is_ok() {
        let _ = solar_common::persistence::atomic_write(
            &wp_dir.join("path.txt"),
            clean_path.as_bytes(),
        );
        let cur = wp_dir.join("current");
        let _ = std::fs::remove_file(&cur);
        let _ = std::os::unix::fs::symlink(clean_path, &cur);
    }
}

fn sync_end4_wallpaper_json(home: &Path, clean_path: &str) {
    let cfg_path = home.join(".config/illogical-impulse/config.json");
    if !cfg_path.exists() {
        return;
    }
    if let Ok(raw) = std::fs::read_to_string(&cfg_path) {
        if let Ok(mut val) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(obj) = val.as_object_mut() {
                let bg = obj
                    .entry("background".to_string())
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(bgo) = bg.as_object_mut() {
                    bgo.insert(
                        "wallpaperPath".to_string(),
                        serde_json::Value::String(clean_path.to_string()),
                    );
                }
                if let Ok(out) = serde_json::to_string_pretty(&val) {
                    let _ = solar_common::persistence::atomic_write(&cfg_path, out.as_bytes());
                }
            }
        }
    }
}

pub fn apply_wallpaper_everywhere(raw_path: &str, update_gsettings: bool) -> Result<String> {
    let Some(clean_path) = normalize_wallpaper_path(raw_path) else {
        bail!("Geçerli bir duvar kağıdı dosyası bulunamadı: {}", raw_path);
    };

    let home = home_dir();
    let stamp = stamp_path();
    if let Some(parent) = stamp.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = solar_common::persistence::atomic_write(&stamp, clean_path.as_bytes());

    SolarConfig::update(|c| {
        c.display.wallpaper = clean_path.clone();
        c.shell.we10x_wallpaper = clean_path.clone();
    })?;

    let file_uri = percent_encode_file_uri(&clean_path);

    // If We10X pre-snapshot exists, update its stored wallpaper URI too so switching back to
    // Noctalia/Caelestia/end4-pC keeps the user's newly chosen wallpaper!
    let snap_file = crate::we10x_profile::snapshot_path();
    if snap_file.is_file() && !is_builtin_we10x_wallpaper(&clean_path) {
        if let Ok(bytes) = std::fs::read(&snap_file) {
            if let Ok(mut snap) =
                serde_json::from_slice::<crate::we10x_profile::PreWe10xSnapshot>(&bytes)
            {
                let quoted = format!("'{}'", file_uri);
                snap.wallpaper_uri = Some(quoted.clone());
                snap.wallpaper_uri_dark = Some(quoted);
                snap.display_wallpaper = Some(clean_path.clone());
                snap.noctalia_wallpaper = Some(clean_path.clone());
                if let Ok(updated) = serde_json::to_vec_pretty(&snap) {
                    let _ = solar_common::persistence::atomic_write(&snap_file, &updated);
                }
            }
        }
    }

    // 1. Update Noctalia config files & live daemon (if active)
    update_noctalia_wallpaper_toml(&home.join(".local/state/noctalia/settings.toml"), &clean_path);
    update_noctalia_wallpaper_toml(&home.join(".config/noctalia/settings.toml"), &clean_path);
    let uid = solar_common::current_uid();
    let uid_str = uid.to_string();
    let rt_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", uid));
    let dbus_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| format!("unix:path={}/bus", rt_dir));

    if Command::new("pgrep")
        .args(["-u", &uid_str, "-x", "noctalia"])
        .stdout(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
    {
        let mut ncmd = Command::new("noctalia");
        ncmd.env("XDG_RUNTIME_DIR", &rt_dir)
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
            .args(["msg", "wallpaper-set", &clean_path]);
        if let Ok(wd) = std::env::var("WAYLAND_DISPLAY") {
            ncmd.env("WAYLAND_DISPLAY", wd);
        } else if let Ok(rd) = std::fs::read_dir(&rt_dir) {
            if let Some(sock) = rd.flatten().find_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                (n.starts_with("wayland-") && !n.ends_with(".lock")).then_some(n)
            }) {
                ncmd.env("WAYLAND_DISPLAY", sock);
            }
        }
        let _ = ncmd.status();
    }

    // 2. Update We10X live QML surface (if active)
    let cfg = SolarConfig::load();
    if cfg.shell.engine == ShellEngine::We10x {
        let _ = Command::new("solar-shell")
            .env("XDG_RUNTIME_DIR", &rt_dir)
            .args(["we10x-ipc", "reloadConfig", "reload"])
            .status();
    }

    // 3. Update Caelestia state & live shell (if active)
    sync_caelestia_wallpaper_files(&home, &clean_path);
    if cfg.shell.engine == ShellEngine::Caelestia {
        let _ = Command::new("caelestia")
            .env("XDG_RUNTIME_DIR", &rt_dir)
            .args(["wallpaper", "-f", &clean_path])
            .status();
    }

    // 4. Update end4-pC state & live shell (if active)
    sync_end4_wallpaper_json(&home, &clean_path);
    if cfg.shell.engine == ShellEngine::End4Pc {
        for sw in [
            "/usr/share/quickshell/end4-pC/scripts/colors/switchwall.sh",
            "/etc/xdg/quickshell/end4-pC/scripts/colors/switchwall.sh",
        ] {
            if Path::new(sw).is_file() {
                let _ = Command::new("bash")
                    .env("XDG_RUNTIME_DIR", &rt_dir)
                    .args([sw, &clean_path])
                    .spawn();
                break;
            }
        }
    }

    // 5. Sync GSettings (for Nautilus, portals, BlazeHouse, and lockscreen)
    if update_gsettings {
        for (schema, key) in [
            ("org.gnome.desktop.background", "picture-uri"),
            ("org.gnome.desktop.background", "picture-uri-dark"),
            ("org.gnome.desktop.screensaver", "picture-uri"),
        ] {
            let _ = Command::new("gsettings")
                .env("XDG_RUNTIME_DIR", &rt_dir)
                .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                .args(["set", schema, key, &file_uri])
                .status();
        }
    }

    info!("Applied desktop wallpaper across SolarUI engines: {}", clean_path);
    Ok(clean_path)
}

fn handle_external_gsetting_uri(raw_val: &str) {
    let Some(clean) = normalize_wallpaper_path(raw_val) else {
        return;
    };
    if is_builtin_we10x_wallpaper(&clean) {
        return;
    }
    let last = std::fs::read_to_string(stamp_path())
        .unwrap_or_default()
        .trim()
        .to_string();
    if clean == last {
        return;
    }
    if let Err(e) = apply_wallpaper_everywhere(&clean, false) {
        warn!("GSettings wallpaper sync failed for {}: {}", clean, e);
    }
}

/// Spawns a background listener watching `org.gnome.desktop.background` (line-buffered via `stdbuf -oL`
/// PLUS zero-overhead `~/.config/dconf/user` mtime polling) so setting a wallpaper from Nautilus
/// ("Set as Wallpaper"), BlazeHouse, or any portal immediately updates Noctalia, We10X, Caelestia,
/// and end4-pC live!
pub fn spawn_gsettings_wallpaper_watcher() {
    let uid = solar_common::current_uid();
    let rt_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| format!("/run/user/{}", uid));
    let dbus_addr = std::env::var("DBUS_SESSION_BUS_ADDRESS")
        .unwrap_or_else(|_| format!("unix:path={}/bus", rt_dir));

    // Seed stamp file with initial GSettings picture-uri so initial startup doesn't spuriously overwrite default.png
    if !stamp_path().exists() {
        if let Ok(out) = Command::new("gsettings")
            .env("XDG_RUNTIME_DIR", &rt_dir)
            .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
            .args(["get", "org.gnome.desktop.background", "picture-uri"])
            .output()
        {
            if out.status.success() {
                if let Some(init_clean) =
                    normalize_wallpaper_path(&String::from_utf8_lossy(&out.stdout))
                {
                    if let Some(parent) = stamp_path().parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = solar_common::persistence::atomic_write(
                        &stamp_path(),
                        init_clean.as_bytes(),
                    );
                }
            }
        }
    }

    // 1. Real-time line-buffered `stdbuf -oL gsettings monitor` stream
    let rt1 = rt_dir.clone();
    let db1 = dbus_addr.clone();
    std::thread::spawn(move || {
        let mut cmd = if Path::new("/usr/bin/stdbuf").is_file() {
            let mut c = Command::new("/usr/bin/stdbuf");
            c.args(["-oL", "gsettings", "monitor", "org.gnome.desktop.background"]);
            c
        } else {
            let mut c = Command::new("gsettings");
            c.args(["monitor", "org.gnome.desktop.background"]);
            c
        };
        cmd.env("XDG_RUNTIME_DIR", &rt1)
            .env("DBUS_SESSION_BUS_ADDRESS", &db1)
            .stdout(Stdio::piped())
            .stderr(Stdio::null());

        let Ok(mut child) = cmd.spawn() else {
            warn!("Could not start gsettings monitor for org.gnome.desktop.background");
            return;
        };
        let Some(stdout) = child.stdout.take() else {
            return;
        };
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let trimmed = line.trim();
            let raw_val = if let Some(rest) = trimmed.strip_prefix("picture-uri-dark:") {
                Some(rest.trim())
            } else {
                trimmed.strip_prefix("picture-uri:").map(|r| r.trim())
            };
            if let Some(val) = raw_val {
                handle_external_gsetting_uri(val);
            }
        }
    });

    // 2. Deterministic dconf database mtime fallback (catches updates even if D-Bus wasn't up at t=0)
    std::thread::spawn(move || {
        let dconf_file = home_dir().join(".config/dconf/user");
        let mut last_mtime = std::fs::metadata(&dconf_file)
            .and_then(|m| m.modified())
            .ok();
        loop {
            std::thread::sleep(std::time::Duration::from_millis(5000));
            let cur_mtime = std::fs::metadata(&dconf_file)
                .and_then(|m| m.modified())
                .ok();
            if cur_mtime.is_some() && cur_mtime != last_mtime {
                last_mtime = cur_mtime;
                for key in ["picture-uri-dark", "picture-uri"] {
                    if let Ok(out) = Command::new("gsettings")
                        .env("XDG_RUNTIME_DIR", &rt_dir)
                        .env("DBUS_SESSION_BUS_ADDRESS", &dbus_addr)
                        .args(["get", "org.gnome.desktop.background", key])
                        .output()
                    {
                        if out.status.success() {
                            handle_external_gsetting_uri(&String::from_utf8_lossy(&out.stdout));
                        }
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_wallpaper_handles_file_uris_and_spaces() {
        let dir = std::env::temp_dir().join(format!("blaze_wp_t1_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("My Cool Wallpaper.png");
        std::fs::write(&img, b"PNG").unwrap();

        let uri = format!(
            "file://{}",
            img.to_string_lossy().replace(' ', "%20")
        );
        let resolved = normalize_wallpaper_path(&format!("'{}'", uri)).unwrap();
        assert_eq!(
            PathBuf::from(resolved),
            std::fs::canonicalize(&img).unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_noctalia_wallpaper_toml_sets_default_path() {
        let dir = std::env::temp_dir().join(format!("blaze_wp_t2_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.toml");
        update_noctalia_wallpaper_toml(&p, "/usr/share/backgrounds/blazeos/galaxy.png");
        let s = std::fs::read_to_string(&p).unwrap();
        assert!(s.contains("/usr/share/backgrounds/blazeos/galaxy.png"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn builtin_we10x_wallpapers_detected_accurately() {
        assert!(is_builtin_we10x_wallpaper(
            "/usr/share/backgrounds/blazeos/we10x/We10XOS-dark.png"
        ));
        assert!(!is_builtin_we10x_wallpaper(
            "/usr/share/backgrounds/blazeos/default.png"
        ));
    }
}
