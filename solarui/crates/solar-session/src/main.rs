use anyhow::{Context, Result};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{error, info};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "solar_session=info".into()),
        )
        .init();

    info!("=========================================");
    info!("   Starting BlazeOS SolarUI Session      ");
    info!("=========================================");

    // 1. Setup Wayland & Desktop environment variables
    std::env::set_var("XDG_CURRENT_DESKTOP", "SolarUI");
    std::env::set_var("XDG_SESSION_TYPE", "wayland");
    std::env::set_var("XDG_SESSION_DESKTOP", "SolarUI");
    std::env::set_var("DESKTOP_SESSION", "SolarUI");
    std::env::set_var("MOZ_ENABLE_WAYLAND", "1");
    std::env::set_var("QT_QPA_PLATFORM", "wayland;xcb");
    std::env::set_var("CLUTTER_BACKEND", "wayland");

    // ChromeOS Ash Performance & Low-Power Idle Optimizations
    let is_vm = std::path::Path::new("/sys/class/dmi/id/product_name")
        .exists()
        && std::fs::read_to_string("/sys/class/dmi/id/product_name")
            .map(|s| {
                let lower = s.to_lowercase();
                lower.contains("virtualbox")
                    || lower.contains("vmware")
                    || lower.contains("qemu")
                    || lower.contains("kvm")
                    || lower.contains("bochs")
            })
            .unwrap_or(false);

    if is_vm {
        info!("Virtual Machine environment detected; configuring rock-solid basic QtQuick render loop.");
        std::env::set_var("QSG_RENDER_LOOP", "basic");
    } else if std::env::var("QSG_RENDER_LOOP").is_err() {
        std::env::set_var("QSG_RENDER_LOOP", "threaded");
    }
    std::env::set_var("QT_QUICK_BACKEND", "rhi");
    std::env::set_var("QSG_RHI_BACKEND", "opengl");

    // Initialize portal configs & systemd graphical session target for PipeWire / OBS screencast
    setup_desktop_portals();
    reconcile_gtk_icon_theme();

    // Branding belongs in shell assets; do not inject GTK/Pango overrides into
    // every application launched by this session.

    // Configure Noctalia to use SolarUI branded assets (Signature SolarUI translations & icons)
    if let Some(assets_dir) = find_noctalia_assets_dir() {
        info!("Using SolarUI Noctalia assets directory: {:?}", assets_dir);
        std::env::set_var("NOCTALIA_ASSETS_DIR", &assets_dir);
    }

    // 2. Prepare Solar runtime directories
    let runtime_dir = solar_common::get_solar_runtime_dir();
    let _ = std::fs::create_dir_all(&runtime_dir);

    // 3. Locate Solar Niri config
    let config_path = find_solar_niri_config();
    info!("Using Niri configuration: {:?}", config_path);

    // 4. Start SolarCore in background if not already running via systemd
    let solar_core_bin =
        find_binary("solar-core").unwrap_or_else(|| PathBuf::from("/usr/bin/solar-core"));
    if solar_core_bin.exists() {
        info!(
            "Spawning SolarCore platform daemon from {:?}",
            solar_core_bin
        );
        let _ = Command::new(&solar_core_bin).spawn();
    }

    // 4.5. Synchronize display scale if configured (multi-monitor & per-output)
    let solar_cfg = solar_common::SolarConfig::load();
    let global_scale = solar_cfg.display.scale;
    let custom_outputs = solar_cfg.display.outputs.clone();
    let has_custom = !custom_outputs.is_empty();
    let has_scale_change = (global_scale - 1.0).abs() > 0.001 || has_custom;

    if has_scale_change {
        info!(
            "Applying persistent display scale (global: {}, custom outputs: {})",
            global_scale,
            custom_outputs.len()
        );
        std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(std::time::Duration::from_millis(200));
                let out = Command::new("niri")
                    .args(["msg", "--json", "outputs"])
                    .output();
                if let Ok(output) = out {
                    if output.status.success() {
                        let text = String::from_utf8_lossy(&output.stdout);
                        let global_scale_str = format!("{:.2}", global_scale);
                        for token in text.split('"') {
                            if token == "Virtual-1"
                                || token.starts_with("eDP")
                                || token.starts_with("HDMI")
                                || token.starts_with("DP-")
                            {
                                if let Some(m) = custom_outputs.iter().find(|o| o.name == token) {
                                    if let Some(ref mode_str) = m.mode {
                                        let subcmd = if m.custom_mode { "custom-mode" } else { "mode" };
                                        let _ = Command::new("niri")
                                            .args(["msg", "output", token, subcmd, mode_str])
                                            .status();
                                    }
                                }
                                let scale_to_apply = if let Some(m) = custom_outputs.iter().find(|o| o.name == token) {
                                    format!("{:.2}", m.scale)
                                } else {
                                    global_scale_str.clone()
                                };
                                let _ = Command::new("niri")
                                    .args(["msg", "output", token, "scale", &scale_to_apply])
                                    .status();
                            }
                        }
                        let _ = Command::new("gsettings")
                            .args([
                                "set",
                                "org.gnome.desktop.interface",
                                "text-scaling-factor",
                                &global_scale_str,
                            ])
                            .status();
                        break;
                    }
                }
            }
        });
    }

    // 5. Exec into Niri compositor
    let niri_bin = find_binary("niri").unwrap_or_else(|| PathBuf::from("/usr/bin/niri"));
    if !niri_bin.exists() {
        error!("Fatal: niri compositor binary not found at {:?}", niri_bin);
        anyhow::bail!("Niri compositor is required to run SolarUI. Please install niri.");
    }

    info!("Executing Niri Wayland session: {:?}", niri_bin);

    let mut cmd = Command::new(&niri_bin);
    // --session bayragi her zaman ekle: systemd ve D-Bus ortamina
    // WAYLAND_DISPLAY, XDG_CURRENT_DESKTOP vb. degiskenleri kayit eder.
    // solar-session-wrapper da ek olarak import-environment yapar,
    // iki kez import zararsizdir ama eksik import sorun yaratir.
    cmd.arg("--session");

    if let Some(cfg) = config_path {
        cmd.arg("-c").arg(cfg);
    }

    // exec replaces current process with Niri so signals and exit codes are cleanly passed
    let err = cmd.exec();
    error!("Failed to exec into Niri: {}", err);
    Err(err).context("Failed to exec niri")
}

fn find_binary(name: &str) -> Option<PathBuf> {
    // 1. Check same directory as current exe (for dev/testing)
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let candidate = parent.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 2. Check PATH
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    // 3. Common system locations
    for loc in &["/usr/local/bin", "/usr/bin"] {
        let candidate = Path::new(loc).join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn find_solar_niri_config() -> Option<PathBuf> {
    // 1. User config: ~/.config/solarui/niri.kdl (reconciled with ~/.config/niri/config.kdl)
    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(&home);
        let solar_dir = home_path.join(".config/solarui");
        let niri_dir = home_path.join(".config/niri");
        let user_cfg = solar_dir.join("niri.kdl");
        let std_niri_cfg = niri_dir.join("config.kdl");

        // Ensure ~/.config/solarui/niri.kdl exists (seed from /etc/solarui/niri.kdl if missing)
        if !user_cfg.exists() {
            let _ = std::fs::create_dir_all(&solar_dir);
            if std_niri_cfg.exists() && !std_niri_cfg.is_symlink() {
                let _ = std::fs::copy(&std_niri_cfg, &user_cfg);
            } else if Path::new("/etc/solarui/niri.kdl").exists() {
                let _ = std::fs::copy("/etc/solarui/niri.kdl", &user_cfg);
            }
        }

        // If user edited ~/.config/niri/config.kdl as a regular file, sync the newer file into ~/.config/solarui/niri.kdl
        if let Ok(meta) = std::fs::symlink_metadata(&std_niri_cfg) {
            if !meta.file_type().is_symlink() {
                let std_mtime = meta.modified().ok();
                let solar_mtime = std::fs::metadata(&user_cfg).ok().and_then(|m| m.modified().ok());
                if let (Some(n_time), Some(s_time)) = (std_mtime, solar_mtime) {
                    if n_time > s_time {
                        info!("Detected newer user edits in ~/.config/niri/config.kdl; syncing to ~/.config/solarui/niri.kdl");
                        let _ = std::fs::copy(&std_niri_cfg, &user_cfg);
                    }
                }
                if user_cfg.exists() {
                    let _ = std::fs::remove_file(&std_niri_cfg);
                    let _ = std::os::unix::fs::symlink("../solarui/niri.kdl", &std_niri_cfg);
                }
            }
        } else if user_cfg.exists() {
            let _ = std::fs::create_dir_all(&niri_dir);
            let _ = std::os::unix::fs::symlink("../solarui/niri.kdl", &std_niri_cfg);
        }

        if user_cfg.exists() {
            return Some(user_cfg);
        }
    }

    // 2. System config: /etc/solarui/niri.kdl or /usr/share/solarui/niri.kdl
    let sys_cfg = PathBuf::from("/etc/solarui/niri.kdl");
    if sys_cfg.exists() {
        return Some(sys_cfg);
    }
    let share_cfg = PathBuf::from("/usr/share/solarui/niri.kdl");
    if share_cfg.exists() {
        return Some(share_cfg);
    }

    // 3. Workspace config in project
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let workspace_cfg = parent.join("../../data/niri.kdl");
            if workspace_cfg.exists() {
                return Some(workspace_cfg);
            }
        }
    }

    None
}

fn find_noctalia_assets_dir() -> Option<PathBuf> {
    // 1. User local directory: ~/.local/share/solarui/noctalia-assets
    if let Ok(home) = std::env::var("HOME") {
        let user_assets = PathBuf::from(home).join(".local/share/solarui/noctalia-assets");
        if user_assets.exists() && user_assets.join("translations/en.json").exists() {
            return Some(user_assets);
        }
    }

    // 2. System directory: /usr/share/solarui/noctalia-assets
    let sys_assets = PathBuf::from("/usr/share/solarui/noctalia-assets");
    if sys_assets.exists() && sys_assets.join("translations/en.json").exists() {
        return Some(sys_assets);
    }

    // 3. Workspace directory: <root>/data/noctalia-assets
    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let ws_assets = parent.join("../../data/noctalia-assets");
            if ws_assets.exists() && ws_assets.join("translations/en.json").exists() {
                return Some(ws_assets);
            }
        }
    }

    None
}

fn setup_desktop_portals() {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(home);

        // 1. Ensure ~/.config/xdg-desktop-portal/solarui-portals.conf uses gtk + blazeos wallpaper portal
        let portal_dir = home_path.join(".config/xdg-desktop-portal");
        let _ = std::fs::create_dir_all(&portal_dir);
        let portal_conf = portal_dir.join("solarui-portals.conf");
        let desired_portals = "[preferred]\ndefault=gtk;\norg.freedesktop.impl.portal.FileChooser=gtk;\norg.freedesktop.impl.portal.AppChooser=gtk;\norg.freedesktop.impl.portal.Print=gtk;\norg.freedesktop.impl.portal.Access=gtk;\norg.freedesktop.impl.portal.Notification=gtk;\norg.freedesktop.impl.portal.Secret=gnome-keyring;\norg.freedesktop.impl.portal.ScreenCast=gnome;\norg.freedesktop.impl.portal.Screenshot=gnome;\norg.freedesktop.impl.portal.Wallpaper=blazeos;\n";
        let should_update = if !portal_conf.exists() {
            true
        } else if let Ok(existing) = std::fs::read_to_string(&portal_conf) {
            existing.contains("default=gnome;gtk;") && !existing.contains("org.freedesktop.impl.portal.Wallpaper=")
        } else {
            false
        };
        if should_update {
            let _ = std::fs::write(&portal_conf, desired_portals);
        }

        // 2. Ensure ~/.config/systemd/user/solarui-session.service exists
        let systemd_dir = home_path.join(".config/systemd/user");
        let _ = std::fs::create_dir_all(&systemd_dir);
        let service_file = systemd_dir.join("solarui-session.service");
        if !service_file.exists() {
            let content = "[Unit]\nDescription=SolarUI Session Manager\nBindsTo=graphical-session.target\nBefore=graphical-session.target\n\n[Service]\nType=oneshot\nRemainAfterExit=yes\nExecStart=/usr/bin/true\nNice=-10\nOOMScoreAdjust=-500\n";
            let _ = std::fs::write(&service_file, content);
        }
    }

    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    let _ = Command::new("systemctl")
        .args([
            "--user",
            "import-environment",
            "WAYLAND_DISPLAY",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_TYPE",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
        ])
        .status();
    let _ = Command::new("dbus-update-activation-environment")
        .args([
            "--systemd",
            "WAYLAND_DISPLAY",
            "XDG_CURRENT_DESKTOP",
            "XDG_SESSION_TYPE",
            "XDG_SESSION_DESKTOP",
            "DESKTOP_SESSION",
        ])
        .status();
    let _ = Command::new("systemctl")
        .args(["--user", "start", "solarui-session.service"])
        .status();
}

fn is_icon_theme_installed(theme_name: &str) -> bool {
    let clean = theme_name.trim().trim_matches('\'').trim_matches('"');
    if clean.is_empty() {
        return false;
    }
    let mut dirs = vec![
        PathBuf::from("/usr/share/icons"),
        PathBuf::from("/usr/local/share/icons"),
    ];
    if let Ok(home) = std::env::var("HOME") {
        let hp = PathBuf::from(home);
        dirs.push(hp.join(".local/share/icons"));
        dirs.push(hp.join(".icons"));
    }
    dirs.into_iter()
        .any(|base| base.join(clean).join("index.theme").is_file())
}

fn reconcile_gtk_icon_theme() {
    let fallback_theme = if is_icon_theme_installed("Papirus-Dark") {
        "Papirus-Dark"
    } else {
        "Adwaita"
    };

    if let Ok(out) = Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "icon-theme"])
        .output()
    {
        if out.status.success() {
            let raw = String::from_utf8_lossy(&out.stdout);
            let current = raw.trim().trim_matches('\'').trim_matches('"');
            if !is_icon_theme_installed(current) {
                info!(
                    "Configured GTK icon theme '{}' is not installed; switching to '{}'",
                    current, fallback_theme
                );
                let _ = Command::new("gsettings")
                    .args([
                        "set",
                        "org.gnome.desktop.interface",
                        "icon-theme",
                        fallback_theme,
                    ])
                    .status();
            }
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let hp = PathBuf::from(home);
        for rel in [".config/gtk-3.0/settings.ini", ".config/gtk-4.0/settings.ini"] {
            let ini_path = hp.join(rel);
            let Ok(content) = std::fs::read_to_string(&ini_path) else {
                continue;
            };
            let mut changed = false;
            let mut out_lines: Vec<String> = Vec::new();
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("gtk-icon-theme-name=") {
                    let theme_val = rest.trim();
                    if !is_icon_theme_installed(theme_val) {
                        out_lines.push(format!("gtk-icon-theme-name={}", fallback_theme));
                        changed = true;
                        continue;
                    }
                }
                out_lines.push(line.to_string());
            }
            if changed {
                let mut rebuilt = out_lines.join("\n");
                rebuilt.push('\n');
                let _ = std::fs::write(&ini_path, rebuilt);
            }
        }
    }
}
