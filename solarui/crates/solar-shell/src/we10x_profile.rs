use anyhow::Result;
use serde::{Deserialize, Serialize};
use solar_common::SolarConfig;
use solar_niri::niri_ipc::{
    Action, PositionChange, Request, Response, SizeChange, WorkspaceReferenceArg,
};
use solar_niri::NiriClient;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

pub const WE10X_PROFILE_MARKER: &str = "// SOLARUI-WE10X-PROFILE-ACTIVE";
pub const WE10X_GTK_CSS_MARKER: &str = "/* SOLARUI-WE10X-GTK-CSS */";
pub const WE10X_WALLPAPER_DARK: &str = "/usr/share/backgrounds/blazeos/we10x/We10XOS-dark.png";
pub const WE10X_WALLPAPER_LIGHT: &str = "/usr/share/backgrounds/blazeos/we10x/We10XOS-light.png";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PreWe10xSnapshot {
    pub niri_kdl: String,
    pub gtk_icon_theme: Option<String>,
    pub gtk_theme: Option<String>,
    pub color_scheme: Option<String>,
    pub button_layout: Option<String>,
    pub wallpaper_uri: Option<String>,
    pub wallpaper_uri_dark: Option<String>,
    pub gtk3_settings_ini: Option<String>,
    pub gtk4_settings_ini: Option<String>,
    #[serde(default)]
    pub gtk3_css: Option<String>,
    #[serde(default)]
    pub gtk4_css: Option<String>,
    #[serde(default)]
    pub display_wallpaper: Option<String>,
    #[serde(default)]
    pub noctalia_wallpaper: Option<String>,
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/liveuser"))
}

pub fn snapshot_path() -> PathBuf {
    home_dir().join(".local/state/solarui/pre-we10x-snapshot.json")
}

fn show_desktop_state_path() -> PathBuf {
    home_dir().join(".local/state/solarui/we10x-show-desktop.json")
}

fn user_niri_kdl_path() -> PathBuf {
    home_dir().join(".config/solarui/niri.kdl")
}

fn read_gsetting(schema: &str, key: &str) -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let val = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!val.is_empty()).then_some(val)
}

fn set_gsetting(schema: &str, key: &str, value: &str) {
    let _ = std::process::Command::new("gsettings")
        .args(["set", schema, key, value])
        .status();
}

fn reload_niri_config() {
    let _ = std::process::Command::new("niri")
        .args(["msg", "action", "load-config-file"])
        .status();
}

/// Extracts top-level `input { ... }`, `output ... { ... }`, and `environment { ... }` blocks
/// from the user's baseline `niri.kdl` so keyboard layouts, display scales, and environment vars
/// are preserved across shell switches.
fn extract_preserved_blocks(base_kdl: &str) -> (String, String, String) {
    let mut input_block = String::new();
    let mut output_blocks = String::new();
    let mut env_block = String::new();

    let lines: Vec<&str> = base_kdl.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let target = if trimmed.starts_with("input ") || trimmed == "input{" || trimmed == "input {" {
            Some(&mut input_block)
        } else if trimmed.starts_with("output ") {
            Some(&mut output_blocks)
        } else if trimmed.starts_with("environment ")
            || trimmed == "environment{"
            || trimmed == "environment {"
        {
            Some(&mut env_block)
        } else {
            None
        };

        if let Some(buf) = target {
            let mut depth: i32 = 0;
            let mut started = false;
            while i < lines.len() {
                let line = lines[i];
                buf.push_str(line);
                buf.push('\n');
                for ch in line.chars() {
                    if ch == '{' {
                        depth += 1;
                        started = true;
                    } else if ch == '}' {
                        depth -= 1;
                    }
                }
                i += 1;
                if started && depth <= 0 {
                    buf.push('\n');
                    break;
                }
            }
            continue;
        }
        i += 1;
    }

    if input_block.trim().is_empty() {
        input_block = r#"input {
    keyboard {
        xkb {
            layout "tr,us"
            options "grp:alt_shift_toggle"
        }
    }
    touchpad {
        tap
        dwt
        natural-scroll
        accel-speed 0.2
    }
    mouse {
        accel-profile "flat"
        accel-speed 0.0
    }
}
"#
        .to_string();
    }

    if env_block.trim().is_empty() {
        env_block = r#"environment {
    XDG_SESSION_TYPE "wayland"
    ELECTRON_OZONE_PLATFORM_HINT "wayland"
    MOZ_ENABLE_WAYLAND "1"
}
"#
        .to_string();
    }

    (input_block, output_blocks, env_block)
}

pub fn normalize_hex_color(raw: &str) -> String {
    let hex = raw.trim().trim_start_matches('#');
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("#{}", hex.to_ascii_lowercase())
    } else {
        "#0078d7".to_string()
    }
}

/// Builds a complete Niri KDL configuration tailored for `We10X (Tanıdık Klasik Düzen)`.
pub fn build_we10x_niri_kdl(base_kdl: &str, theme_mode: &str) -> String {
    build_we10x_niri_kdl_with_options(base_kdl, theme_mode, "#0078d7", false)
}

pub fn build_we10x_niri_kdl_with_options(
    base_kdl: &str,
    theme_mode: &str,
    accent_color: &str,
    colorize_surfaces: bool,
) -> String {
    let accent = normalize_hex_color(accent_color);
    let is_light = theme_mode.eq_ignore_ascii_case("light");
    let is_oled = theme_mode.eq_ignore_ascii_case("oled") || theme_mode.eq_ignore_ascii_case("black");
    let (mut tb_active_bg, tb_inactive_bg, mut tb_active_fg, tb_inactive_fg, border_inactive) =
        if is_light {
            ("#ffffff", "#f2f2f2", "#111111", "#767676", "#cccccc")
        } else if is_oled {
            ("#101010", "#181818", "#ffffff", "#8e8e93", "#2c2c31")
        } else {
            ("#1f1f1f", "#2b2b2b", "#ffffff", "#9a9a9a", "#3f3f46")
        };

    if colorize_surfaces {
        tb_active_bg = accent.as_str();
        tb_active_fg = "#ffffff";
    }

    let (input_block, output_blocks, env_block) = extract_preserved_blocks(base_kdl);

    format!(
        r##"{marker}
// BlazeOS SolarUI - We10X (Tanıdık Klasik Masaüstü / Windows 10 Modu)
// Otomatik olarak SolarUI We10X Profil Yöneticisi tarafından üretilmiştir.
// Başka bir kabuğa geçildiğinde önceki niri.kdl yapılandırmanız eksiksiz geri yüklenir.

spawn-at-startup "/usr/libexec/blazeos-wayland-session-ready"
spawn-at-startup "solar-shell" "autostart"

super-tap-spawn "solar-shell" "route" "launcher"

hotkey-overlay {{
    skip-at-startup
}}

{input_block}
{output_blocks}
layout {{
    gaps 6
    center-focused-column "never"
    background-color "transparent"

    preset-column-widths {{
        proportion 0.33333
        proportion 0.5
        proportion 0.66667
        proportion 1.0
    }}

    default-column-width {{ proportion 0.5; }}

    focus-ring {{
        off
    }}

    border {{
        on
        width 1
        active-color "{accent}"
        inactive-color "{border_inactive}"
    }}

    struts {{
        top 0
        left 0
        right 0
        bottom 44
    }}

    server-titlebar {{
        on
        height 30
        active-color "{tb_active_bg}"
        inactive-color "{tb_inactive_bg}"
        active-text-color "{tb_active_fg}"
        inactive-text-color "{tb_inactive_fg}"
        aero-snap true
        allow-minimize true
    }}
}}

animations {{
    workspace-switch {{
        spring damping-ratio=1.0 stiffness=1100 epsilon=0.0001
    }}
    window-open {{
        duration-ms 150
        curve "ease-out-quad"
    }}
    window-close {{
        duration-ms 130
        curve "ease-out-cubic"
    }}
    horizontal-view-movement {{
        spring damping-ratio=1.0 stiffness=1000 epsilon=0.0001
    }}
    window-movement {{
        spring damping-ratio=1.0 stiffness=950 epsilon=0.0001
    }}
    window-resize {{
        spring damping-ratio=1.0 stiffness=1100 epsilon=0.0001
    }}
    overview-open-close {{
        spring damping-ratio=1.0 stiffness=950 epsilon=0.0001
    }}
}}

// Keskin Windows 10 pencere köşeleri (radius=0) ve yüzen masaüstü pencere davranışı
window-rule {{
    geometry-corner-radius 0
    clip-to-geometry true
    open-floating true
}}

// Kendi CSD başlık çubuğunu çizmeyen terminal, FireHub ve araçlara 30px Windows 10 başlık çubuğu (─ □ ✕) ekle
window-rule {{
    match app-id=r#"^(kitty|Alacritty|org\.Alacritty|foot|mpv|imv|feh)$"#
    open-floating true
    server-titlebar true
    default-column-width {{ fixed 980; }}
    default-window-height {{ fixed 540; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

window-rule {{
    match app-id=r#"^(FireHub|firehub)$"#
    open-floating true
    server-titlebar true
    default-column-width {{ proportion 0.80; }}
    default-window-height {{ proportion 0.82; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

window-rule {{
    match app-id=r#"^(anaconda|org\.fedoraproject\.Anaconda.*|Anaconda|liveinst)$"#
    open-floating true
    open-focused true
    default-column-width {{ proportion 0.85; }}
    default-window-height {{ proportion 0.85; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

window-rule {{
    match app-id=r#"(org\.blazeos\.setup|blaze-setup)"#
    open-floating true
    open-focused true
    open-fullscreen true
    server-titlebar false
}}

window-rule {{
    match app-id="^solar-settings$"
    open-floating true
    server-titlebar false
    default-column-width {{ fixed 1020; }}
    default-window-height {{ fixed 620; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

window-rule {{
    match app-id="^solar-welcome$"
    open-floating true
    server-titlebar false
    default-column-width {{ fixed 750; }}
    default-window-height {{ fixed 590; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

window-rule {{
    match app-id="^solar-installer-gui$"
    open-floating true
    server-titlebar false
    default-column-width {{ fixed 920; }}
    default-window-height {{ fixed 640; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

window-rule {{
    match app-id="^solar-omnibar$"
    open-floating true
    server-titlebar false
    default-column-width {{ fixed 720; }}
    default-window-height {{ fixed 490; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

window-rule {{
    match app-id=r#"^(solar-snap|solar-clock|solar-shell|dev\.noctalia\.Noctalia)$"#
    open-floating true
    server-titlebar false
    geometry-corner-radius 0
}}

window-rule {{
    match app-id="^solar-gamezone$"
    open-fullscreen true
    server-titlebar false
    geometry-corner-radius 0
    clip-to-geometry false
    background-effect {{
        blur false
    }}
}}

window-rule {{
    match app-id=r#"^(org\.blazeos\.control|blazeos-welcome)$"#
    open-floating true
    server-titlebar false
    default-column-width {{ proportion 0.80; }}
    default-window-height {{ proportion 0.82; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

window-rule {{
    match app-id=r#"^(firefox|google-chrome.*|chromium.*)$"#
    open-floating true
    default-column-width {{ proportion 0.82; }}
    default-window-height {{ proportion 0.84; }}
    geometry-corner-radius 0
    clip-to-geometry true
}}

workspace "1"
workspace "background"

layer-rule {{
    match namespace="^we10x-wallpaper$"
    place-within-backdrop false
}}

binds {{
    // ── Windows 10 Alışkanlıkları & Aero Snap Kısayolları ──────────────
    Mod+D             {{ spawn "solar-shell" "show-desktop"; }}
    Mod+Left          {{ spawn "solar-shell" "aero-snap" "left"; }}
    Mod+Right         {{ spawn "solar-shell" "aero-snap" "right"; }}
    Mod+Up            {{ spawn "solar-shell" "aero-snap" "up"; }}
    Mod+Down          {{ spawn "solar-shell" "aero-snap" "down"; }}
    Alt+Tab           {{ spawn "solar-shell" "route" "window-switcher"; }}
    Mod+Tab           {{ spawn "solar-shell" "route" "overview"; }}
    Alt+F4            {{ close-window; }}
    Mod+Q             {{ close-window; }}
    Ctrl+Shift+Escape {{ spawn "blaze-system-monitor"; }}
    Ctrl+Alt+Delete   {{ spawn "solar-shell" "route" "session"; }}

    // ── Windows 10 Merkezleri & Hızlı Başlatıcılar ─────────────────────
    Mod+A             {{ spawn "solar-shell" "route" "control-center"; }}
    Mod+S             {{ spawn "solar-shell" "we10x-ipc" "searchBox" "toggle"; }}
    Mod+E             {{ spawn "nautilus"; }}
    Mod+I             {{ spawn "solar-shell" "settings"; }}
    Mod+R             {{ spawn "solar-shell" "omnibar" "--force"; }}
    Mod+Space         {{ spawn "solar-shell" "omnibar" "--force"; }}
    Mod+L             {{ spawn "solar-lock"; }}
    Mod+G             {{ spawn "solar-shell" "gamezone"; }}
    Mod+H             {{ spawn "firehub"; }}
    Mod+F1            {{ spawn "solar-shell" "welcome"; }}
    Mod+Alt+R         {{ spawn "solar-shell" "supervisor"; }}

    Mod+Return        {{ spawn "sh" "-c" "command -v kitty >/dev/null && exec kitty || exec alacritty"; }}
    Ctrl+Alt+T        {{ spawn "sh" "-c" "command -v kitty >/dev/null && exec kitty || exec alacritty"; }}
    Mod+B             {{ spawn "sh" "-c" "command -v google-chrome-stable >/dev/null && exec google-chrome-stable || command -v firefox >/dev/null && exec firefox || exec xdg-open https://www.google.com"; }}

    // ── Çalışma Alanları ───────────────────────────────────────────────
    Mod+1             {{ focus-workspace 1; }}
    Mod+2             {{ focus-workspace 2; }}
    Mod+3             {{ focus-workspace 3; }}
    Mod+4             {{ focus-workspace 4; }}
    Mod+Ctrl+Left     {{ focus-workspace-up; }}
    Mod+Ctrl+Right    {{ focus-workspace-down; }}

    // ── Ses, Parlaklık & Ekran Görüntüsü ───────────────────────────────
    XF86AudioRaiseVolume allow-when-locked=true {{ spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "5%+"; }}
    XF86AudioLowerVolume allow-when-locked=true {{ spawn "wpctl" "set-volume" "@DEFAULT_AUDIO_SINK@" "5%-"; }}
    XF86AudioMute        allow-when-locked=true {{ spawn "wpctl" "set-mute" "@DEFAULT_AUDIO_SINK@" "toggle"; }}
    XF86AudioMicMute     allow-when-locked=true {{ spawn "wpctl" "set-mute" "@DEFAULT_AUDIO_SOURCE@" "toggle"; }}

    XF86MonBrightnessUp   allow-when-locked=true {{ spawn "brightnessctl" "set" "+5%"; }}
    XF86MonBrightnessDown allow-when-locked=true {{ spawn "brightnessctl" "set" "5%-"; }}

    Print             {{ spawn "sh" "-c" "IMG=$(mktemp --suffix=.png) && grim -g \"$(slurp)\" \"$IMG\" && wl-copy < \"$IMG\" && swappy -f \"$IMG\"; rm -f \"$IMG\""; }}
    Mod+Shift+S       {{ spawn "sh" "-c" "IMG=$(mktemp --suffix=.png) && grim -g \"$(slurp)\" \"$IMG\" && wl-copy < \"$IMG\" && swappy -f \"$IMG\"; rm -f \"$IMG\""; }}
    Alt+Print         {{ spawn "sh" "-c" "IMG=$(mktemp --suffix=.png) && grim \"$IMG\" && wl-copy < \"$IMG\" && swappy -f \"$IMG\"; rm -f \"$IMG\""; }}

    Mod+Shift+E       {{ quit; }}
}}

{env_block}"##,
        marker = WE10X_PROFILE_MARKER,
        input_block = input_block,
        output_blocks = output_blocks,
        accent = accent,
        border_inactive = border_inactive,
        tb_active_bg = tb_active_bg,
        tb_inactive_bg = tb_inactive_bg,
        tb_active_fg = tb_active_fg,
        tb_inactive_fg = tb_inactive_fg,
        env_block = env_block,
    )
}

fn write_gtk_settings_and_css_for_we10x(home: &Path, is_light: bool) {
    let gtk_theme = if is_light { "Adwaita" } else { "Adwaita-dark" };
    let prefer_dark = if is_light { "false" } else { "true" };
    let ini = format!(
        "[Settings]\ngtk-theme-name={gtk_theme}\ngtk-application-prefer-dark-theme={prefer_dark}\ngtk-icon-theme-name=We10X\ngtk-decoration-layout=appmenu:minimize,maximize,close\ngtk-font-name=Inter 10\n"
    );
    let css = format!(
        "{marker}\nwindow, window.csd, window.background, .background, .background.csd, headerbar, .titlebar {{\n    border-radius: 0px;\n}}\nwindowcontrols > button.close:hover,\nwindowcontrols > button.close:hover > image,\nheaderbar button.titlebutton.close:hover {{\n    background-color: #e81123;\n    color: #ffffff;\n    border-radius: 0px;\n}}\n",
        marker = WE10X_GTK_CSS_MARKER,
    );
    for ver in ["gtk-3.0", "gtk-4.0"] {
        let dir = home.join(".config").join(ver);
        let _ = std::fs::create_dir_all(&dir);
        let _ = solar_common::persistence::atomic_write(&dir.join("settings.ini"), ini.as_bytes());
        let _ = solar_common::persistence::atomic_write(&dir.join("gtk.css"), css.as_bytes());
    }
}

fn remove_if_we10x_managed_ini(path: &Path) {
    if let Ok(content) = std::fs::read_to_string(path) {
        if content.contains("gtk-icon-theme-name=We10X") {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn remove_if_we10x_managed_css(path: &Path) {
    if let Ok(content) = std::fs::read_to_string(path) {
        if content.contains(WE10X_GTK_CSS_MARKER) {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn enter_we10x_profile_in_paths(
    home: &Path,
    niri_cfg_path: &Path,
    snap_path: &Path,
    fallback_system_niri: &Path,
    theme_mode: &str,
    apply_system_side_effects: bool,
) -> Result<()> {
    enter_we10x_profile_in_paths_with_options(
        home,
        niri_cfg_path,
        snap_path,
        fallback_system_niri,
        theme_mode,
        "#0078d7",
        false,
        "",
        apply_system_side_effects,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn enter_we10x_profile_in_paths_with_options(
    home: &Path,
    niri_cfg_path: &Path,
    snap_path: &Path,
    fallback_system_niri: &Path,
    theme_mode: &str,
    accent_color: &str,
    colorize_surfaces: bool,
    custom_wallpaper: &str,
    apply_system_side_effects: bool,
) -> Result<()> {
    let existing_niri = std::fs::read_to_string(niri_cfg_path)
        .or_else(|_| std::fs::read_to_string(fallback_system_niri))
        .unwrap_or_default();

    let is_already_we10x = existing_niri.contains(WE10X_PROFILE_MARKER);

    let baseline_niri = if !is_already_we10x {
        let gtk3_path = home.join(".config/gtk-3.0/settings.ini");
        let gtk4_path = home.join(".config/gtk-4.0/settings.ini");
        let gtk3_css_path = home.join(".config/gtk-3.0/gtk.css");
        let gtk4_css_path = home.join(".config/gtk-4.0/gtk.css");

        let raw_icon_theme = if apply_system_side_effects {
            read_gsetting("org.gnome.desktop.interface", "icon-theme")
        } else {
            None
        };
        let clean_icon_theme = raw_icon_theme.map(|v| {
            if v.trim_matches('\'').eq_ignore_ascii_case("We10X") {
                "'Papirus-Dark'".to_string()
            } else {
                v
            }
        });

        let raw_btn_layout = if apply_system_side_effects {
            read_gsetting("org.gnome.desktop.wm.preferences", "button-layout")
        } else {
            None
        };
        let clean_btn_layout =
            raw_btn_layout.or_else(|| Some("'appmenu:maximize,close'".to_string()));

        let clean_ini = |p: &Path| -> Option<String> {
            let s = std::fs::read_to_string(p).ok()?;
            if s.contains("gtk-icon-theme-name=We10X") {
                None
            } else {
                Some(s)
            }
        };
        let clean_css = |p: &Path| -> Option<String> {
            let s = std::fs::read_to_string(p).ok()?;
            if s.contains(WE10X_GTK_CSS_MARKER) {
                None
            } else {
                Some(s)
            }
        };

        let read_display_wp = || -> Option<String> {
            let candidate = if apply_system_side_effects {
                SolarConfig::load().display.wallpaper
            } else {
                let p = home.join(".config/solarui/config.toml");
                std::fs::read_to_string(&p)
                    .ok()
                    .and_then(|s| toml::from_str::<SolarConfig>(&s).ok())
                    .map(|c| c.display.wallpaper)
                    .unwrap_or_default()
            };
            let t = candidate.trim();
            (!t.is_empty() && !crate::wallpaper::is_builtin_we10x_wallpaper(t))
                .then(|| t.to_string())
        };
        let filter_wp_uri = |raw: Option<String>| -> Option<String> {
            raw.filter(|u| !u.contains("/backgrounds/blazeos/we10x/"))
        };
        let noctalia_wp = crate::wallpaper::read_noctalia_wallpaper_toml(home)
            .filter(|w| !crate::wallpaper::is_builtin_we10x_wallpaper(w));

        let snapshot = PreWe10xSnapshot {
            niri_kdl: existing_niri.clone(),
            gtk_icon_theme: clean_icon_theme,
            gtk_theme: if apply_system_side_effects {
                read_gsetting("org.gnome.desktop.interface", "gtk-theme")
            } else {
                None
            },
            color_scheme: if apply_system_side_effects {
                read_gsetting("org.gnome.desktop.interface", "color-scheme")
            } else {
                None
            },
            button_layout: clean_btn_layout,
            wallpaper_uri: if apply_system_side_effects {
                filter_wp_uri(read_gsetting("org.gnome.desktop.background", "picture-uri"))
            } else {
                None
            },
            wallpaper_uri_dark: if apply_system_side_effects {
                filter_wp_uri(read_gsetting("org.gnome.desktop.background", "picture-uri-dark"))
            } else {
                None
            },
            gtk3_settings_ini: clean_ini(&gtk3_path),
            gtk4_settings_ini: clean_ini(&gtk4_path),
            gtk3_css: clean_css(&gtk3_css_path),
            gtk4_css: clean_css(&gtk4_css_path),
            display_wallpaper: read_display_wp(),
            noctalia_wallpaper: noctalia_wp,
        };
        if let Some(parent) = snap_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let bytes = serde_json::to_vec_pretty(&snapshot)?;
        solar_common::persistence::atomic_write(snap_path, &bytes)?;
        existing_niri
    } else if let Ok(raw) = std::fs::read(snap_path) {
        serde_json::from_slice::<PreWe10xSnapshot>(&raw)
            .map(|s| s.niri_kdl)
            .unwrap_or(existing_niri)
    } else {
        existing_niri
    };

    let we10x_kdl = build_we10x_niri_kdl_with_options(
        &baseline_niri,
        theme_mode,
        accent_color,
        colorize_surfaces,
    );
    if let Some(parent) = niri_cfg_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    solar_common::persistence::atomic_write(niri_cfg_path, we10x_kdl.as_bytes())?;

    let is_light = theme_mode.eq_ignore_ascii_case("light");
    write_gtk_settings_and_css_for_we10x(home, is_light);

    if apply_system_side_effects {
        crate::supervisor::SolarShellSupervisor::reconcile_niri_config_symlink();
        let _ = std::process::Command::new("gsettings")
            .args([
                "set",
                "org.gnome.desktop.wm.preferences",
                "button-layout",
                "appmenu:minimize,maximize,close",
            ])
            .status();
        let _ = std::process::Command::new("gsettings")
            .args(["set", "org.gnome.desktop.interface", "icon-theme", "We10X"])
            .status();
        let (gtk_theme, scheme) = if is_light {
            ("Adwaita", "prefer-light")
        } else {
            ("Adwaita-dark", "prefer-dark")
        };
        let _ = custom_wallpaper;
        let _ = std::process::Command::new("gsettings")
            .args(["set", "org.gnome.desktop.interface", "gtk-theme", gtk_theme])
            .status();
        let _ = std::process::Command::new("gsettings")
            .args(["set", "org.gnome.desktop.interface", "color-scheme", scheme])
            .status();
        reload_niri_config();
    }

    Ok(())
}

/// Offline OOBE helper (`blaze-setup`): provisions a newly created user's home directory so
/// their very first login boots straight into We10X `niri.kdl` + sharp GTK CSS while preserving
/// a clean stock SolarUI `PreWe10xSnapshot` for future switches to Noctalia/Caelestia/end4-pC.
pub fn provision_user_we10x_home(
    home: &Path,
    fallback_system_niri: &Path,
    theme_mode: &str,
) -> Result<()> {
    let niri_cfg = home.join(".config/solarui/niri.kdl");
    let snap = home.join(".local/state/solarui/pre-we10x-snapshot.json");
    enter_we10x_profile_in_paths(home, &niri_cfg, &snap, fallback_system_niri, theme_mode, false)?;

    let niri_dir = home.join(".config/niri");
    std::fs::create_dir_all(&niri_dir)?;
    let std_cfg = niri_dir.join("config.kdl");
    let _ = std::fs::remove_file(&std_cfg);
    let _ = std::os::unix::fs::symlink("../solarui/niri.kdl", &std_cfg);
    Ok(())
}

pub fn leave_we10x_profile_in_paths(
    home: &Path,
    niri_cfg_path: &Path,
    snap_path: &Path,
    fallback_system_niri: &Path,
    apply_system_side_effects: bool,
) -> Result<()> {
    let snap_opt: Option<PreWe10xSnapshot> = std::fs::read(snap_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok());

    let gtk3_ini_path = home.join(".config/gtk-3.0/settings.ini");
    let gtk4_ini_path = home.join(".config/gtk-4.0/settings.ini");
    let gtk3_css_path = home.join(".config/gtk-3.0/gtk.css");
    let gtk4_css_path = home.join(".config/gtk-4.0/gtk.css");

    let current_niri = std::fs::read_to_string(niri_cfg_path).unwrap_or_default();
    if let Some(ref snap) = snap_opt {
        if !snap.niri_kdl.trim().is_empty() {
            if let Some(parent) = niri_cfg_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            solar_common::persistence::atomic_write(niri_cfg_path, snap.niri_kdl.as_bytes())?;
        }
        if let Some(ref ini3) = snap.gtk3_settings_ini {
            let _ = solar_common::persistence::atomic_write(&gtk3_ini_path, ini3.as_bytes());
        } else {
            remove_if_we10x_managed_ini(&gtk3_ini_path);
        }
        if let Some(ref ini4) = snap.gtk4_settings_ini {
            let _ = solar_common::persistence::atomic_write(&gtk4_ini_path, ini4.as_bytes());
        } else {
            remove_if_we10x_managed_ini(&gtk4_ini_path);
        }
        if let Some(ref css3) = snap.gtk3_css {
            let _ = solar_common::persistence::atomic_write(&gtk3_css_path, css3.as_bytes());
        } else {
            remove_if_we10x_managed_css(&gtk3_css_path);
        }
        if let Some(ref css4) = snap.gtk4_css {
            let _ = solar_common::persistence::atomic_write(&gtk4_css_path, css4.as_bytes());
        } else {
            remove_if_we10x_managed_css(&gtk4_css_path);
        }
        if let Some(ref nw) = snap.noctalia_wallpaper {
            if !crate::wallpaper::is_builtin_we10x_wallpaper(nw) {
                let cur_nw = crate::wallpaper::read_noctalia_wallpaper_toml(home).unwrap_or_default();
                if cur_nw.is_empty() || crate::wallpaper::is_builtin_we10x_wallpaper(&cur_nw) {
                    crate::wallpaper::update_noctalia_wallpaper_toml(
                        &home.join(".local/state/noctalia/settings.toml"),
                        nw,
                    );
                    crate::wallpaper::update_noctalia_wallpaper_toml(
                        &home.join(".config/noctalia/settings.toml"),
                        nw,
                    );
                }
            }
        }
        if apply_system_side_effects {
            if let Some(ref dw) = snap.display_wallpaper {
                if !crate::wallpaper::is_builtin_we10x_wallpaper(dw) {
                    let _ = SolarConfig::update(|c| {
                        if crate::wallpaper::is_builtin_we10x_wallpaper(&c.display.wallpaper)
                            || c.display.wallpaper.trim().is_empty()
                        {
                            c.display.wallpaper = dw.clone();
                        }
                    });
                }
            }
            if let Some(ref v) = snap.gtk_icon_theme {
                set_gsetting("org.gnome.desktop.interface", "icon-theme", v);
            } else {
                set_gsetting("org.gnome.desktop.interface", "icon-theme", "'Papirus-Dark'");
            }
            if let Some(ref v) = snap.gtk_theme {
                set_gsetting("org.gnome.desktop.interface", "gtk-theme", v);
            }
            if let Some(ref v) = snap.color_scheme {
                set_gsetting("org.gnome.desktop.interface", "color-scheme", v);
            }
            if let Some(ref v) = snap.button_layout {
                set_gsetting("org.gnome.desktop.wm.preferences", "button-layout", v);
            } else {
                set_gsetting(
                    "org.gnome.desktop.wm.preferences",
                    "button-layout",
                    "'appmenu:maximize,close'",
                );
            }
            if let Some(ref v) = snap.wallpaper_uri {
                set_gsetting("org.gnome.desktop.background", "picture-uri", v);
            }
            if let Some(ref v) = snap.wallpaper_uri_dark {
                set_gsetting("org.gnome.desktop.background", "picture-uri-dark", v);
            }
        }
        let _ = std::fs::remove_file(snap_path);
    } else if current_niri.contains(WE10X_PROFILE_MARKER) {
        if let Ok(sys_kdl) = std::fs::read_to_string(fallback_system_niri) {
            let _ = solar_common::persistence::atomic_write(niri_cfg_path, sys_kdl.as_bytes());
        }
        remove_if_we10x_managed_ini(&gtk3_ini_path);
        remove_if_we10x_managed_ini(&gtk4_ini_path);
        remove_if_we10x_managed_css(&gtk3_css_path);
        remove_if_we10x_managed_css(&gtk4_css_path);
        if apply_system_side_effects {
            let _ = std::process::Command::new("gsettings")
                .args([
                    "set",
                    "org.gnome.desktop.interface",
                    "icon-theme",
                    "Papirus-Dark",
                ])
                .status();
            let _ = std::process::Command::new("gsettings")
                .args([
                    "set",
                    "org.gnome.desktop.wm.preferences",
                    "button-layout",
                    "appmenu:maximize,close",
                ])
                .status();
        }
    }

    if apply_system_side_effects {
        reload_niri_config();
    }
    Ok(())
}

/// Enters the We10X classic Windows 10 desktop profile for the active user session.
pub async fn enter_we10x_profile() -> Result<()> {
    let cfg = SolarConfig::load();
    let home = home_dir();
    let niri_path = user_niri_kdl_path();
    let snap = snapshot_path();
    let custom_wp = if !cfg.shell.we10x_wallpaper.trim().is_empty() {
        cfg.shell.we10x_wallpaper.as_str()
    } else {
        cfg.display.wallpaper.as_str()
    };
    enter_we10x_profile_in_paths_with_options(
        &home,
        &niri_path,
        &snap,
        Path::new("/etc/solarui/niri.kdl"),
        &cfg.shell.we10x_theme_mode,
        &cfg.shell.we10x_accent_color,
        cfg.shell.we10x_colorize_surfaces,
        custom_wp,
        true,
    )?;
    info!(
        "Applied We10X profile (theme_mode={}, accent={}, colorize={}, start_icon={})",
        cfg.shell.we10x_theme_mode,
        cfg.shell.we10x_accent_color,
        cfg.shell.we10x_colorize_surfaces,
        cfg.shell.we10x_start_icon
    );
    Ok(())
}

/// Restores all windows parked on Niri's `"background"` workspace back to visible workspaces
/// so zero minimized windows remain hidden when switching away from `We10X`.
pub async fn unminimize_all_windows_with_client(client: &NiriClient) -> Result<usize> {
    let Response::Workspaces(workspaces) = client.send_request(Request::Workspaces).await? else {
        anyhow::bail!("Unexpected Workspaces reply from Niri");
    };
    let Some(bg_ws_id) = workspaces
        .iter()
        .find(|ws| {
            ws.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case("background"))
        })
        .map(|ws| ws.id)
    else {
        return Ok(0);
    };

    let fallback_ws_id = workspaces
        .iter()
        .find(|ws| ws.is_focused && ws.id != bg_ws_id)
        .or_else(|| {
            workspaces
                .iter()
                .find(|ws| ws.name.as_deref() == Some("1") && ws.id != bg_ws_id)
        })
        .or_else(|| workspaces.iter().find(|ws| ws.id != bg_ws_id))
        .map(|ws| ws.id);

    let Some(target_fallback) = fallback_ws_id else {
        return Ok(0);
    };

    let stored_records = solar_common::MinimizedStore::load();
    let valid_ws_ids: Vec<u64> = workspaces
        .iter()
        .filter(|ws| ws.id != bg_ws_id)
        .map(|ws| ws.id)
        .collect();

    let windows = client.get_windows().await?;
    let mut restored_count = 0usize;

    for win in windows {
        if win.workspace_id == Some(bg_ws_id) {
            let dest = stored_records
                .get(&win.id)
                .map(|r| r.orig_workspace_id)
                .filter(|id| valid_ws_ids.contains(id))
                .unwrap_or(target_fallback);

            client
                .move_window_to_workspace(Some(win.id), WorkspaceReferenceArg::Id(dest), false)
                .await?;
            if let Ok(mut tx) = solar_common::MinimizedStore::transaction() {
                let _ = tx.remove(win.id);
            }
            restored_count += 1;
        }
    }

    let _ = std::fs::remove_file(show_desktop_state_path());
    Ok(restored_count)
}

/// Leaves the We10X profile: unminimizes every parked window and restores previous SolarUI config & theme.
pub async fn leave_we10x_profile() -> Result<()> {
    if let Ok(client) = NiriClient::new() {
        match unminimize_all_windows_with_client(&client).await {
            Ok(n) if n > 0 => info!("Restored {n} minimized window(s) from background workspace"),
            Ok(_) => {}
            Err(e) => warn!("Could not unminimize windows while leaving We10X: {e}"),
        }
    }

    let home = home_dir();
    let niri_path = user_niri_kdl_path();
    let snap = snapshot_path();
    leave_we10x_profile_in_paths(
        &home,
        &niri_path,
        &snap,
        Path::new("/etc/solarui/niri.kdl"),
        true,
    )?;
    info!("Restored pre-We10X SolarUI profile and theme");
    Ok(())
}

/// Implements Windows 10 `Win+D` / rightmost 5px Show Desktop toggle.
pub async fn toggle_show_desktop() -> Result<()> {
    let cfg = SolarConfig::load();
    if cfg.shell.engine != crate::engine::ShellEngine::We10x {
        tracing::info!(
            "Show Desktop skipped outside We10X mode (active={:?})",
            cfg.shell.engine
        );
        return Ok(());
    }
    let client = NiriClient::new()?;
    let Response::Workspaces(workspaces) = client.send_request(Request::Workspaces).await? else {
        return Ok(());
    };
    let Some(bg_ws_id) = workspaces
        .iter()
        .find(|ws| {
            ws.name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case("background"))
        })
        .map(|ws| ws.id)
    else {
        return Ok(());
    };
    let Some(active_ws_id) = workspaces
        .iter()
        .find(|ws| ws.is_focused && ws.id != bg_ws_id)
        .or_else(|| workspaces.iter().find(|ws| ws.is_active && ws.id != bg_ws_id))
        .map(|ws| ws.id)
    else {
        return Ok(());
    };

    let windows = client.get_windows().await?;
    let visible_on_active: Vec<u64> = windows
        .iter()
        .filter(|w| w.workspace_id == Some(active_ws_id))
        .map(|w| w.id)
        .collect();

    let state_file = show_desktop_state_path();
    if !visible_on_active.is_empty() {
        for &win_id in &visible_on_active {
            let _ = solar_niri::minimized::minimize(&client, win_id).await;
        }
        if let Some(parent) = state_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec(&visible_on_active) {
            let _ = solar_common::persistence::atomic_write(&state_file, &bytes);
        }
    } else {
        let saved: Vec<u64> = std::fs::read(&state_file)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let bg_ids: Vec<u64> = windows
            .iter()
            .filter(|w| w.workspace_id == Some(bg_ws_id))
            .map(|w| w.id)
            .collect();
        let to_restore: Vec<u64> = if !saved.is_empty() {
            let filtered: Vec<u64> = saved.into_iter().filter(|id| bg_ids.contains(id)).collect();
            if filtered.is_empty() {
                bg_ids
            } else {
                filtered
            }
        } else {
            bg_ids
        };
        let mut last_restored = None;
        for win_id in to_restore {
            if client
                .move_window_to_workspace(
                    Some(win_id),
                    WorkspaceReferenceArg::Id(active_ws_id),
                    false,
                )
                .await
                .is_ok()
            {
                if let Ok(mut tx) = solar_common::MinimizedStore::transaction() {
                    let _ = tx.remove(win_id);
                }
                last_restored = Some(win_id);
            }
        }
        if let Some(id) = last_restored {
            let _ = client.focus_window(id).await;
        }
        let _ = std::fs::remove_file(&state_file);
    }
    Ok(())
}

/// Implements Windows 10 `Win + Arrow` Aero Snap (`left`, `right`, `up`, `down`).
pub async fn aero_snap_focused(direction: &str) -> Result<()> {
    let client = NiriClient::new()?;
    aero_snap_focused_with_client(&client, direction).await
}

pub async fn aero_snap_focused_with_client(client: &NiriClient, direction: &str) -> Result<()> {
    let Response::Windows(raw_windows) = client.send_request(Request::Windows).await? else {
        return Ok(());
    };
    let Some(focused) = raw_windows.into_iter().find(|w| w.is_focused) else {
        return Ok(());
    };
    let id = focused.id;

    let (work_w, work_h) = match client.send_request(Request::FocusedOutput).await {
        Ok(Response::FocusedOutput(Some(out))) => {
            if let Some(logical) = out.logical {
                (logical.width as f64, (logical.height as f64 - 44.0).max(200.0))
            } else {
                (1920.0, 1036.0)
            }
        }
        _ => (1920.0, 1036.0),
    };

    let tile_w = focused.layout.tile_size.0;
    let tile_h = focused.layout.tile_size.1;
    let is_maximized = tile_w >= work_w * 0.95 && tile_h >= work_h * 0.92;

    match direction.to_ascii_lowercase().trim() {
        "left" | "right" => {
            let is_left = direction.trim().eq_ignore_ascii_case("left");
            if is_maximized {
                client
                    .send_request(Request::Action(Action::MaximizeWindowToEdges {
                        id: Some(id),
                    }))
                    .await?;
            }
            if !focused.is_floating {
                client
                    .send_request(Request::Action(Action::MoveWindowToFloating {
                        id: Some(id),
                    }))
                    .await?;
            }
            // Note: Niri IPC `SizeChange::SetProportion` and `PositionChange::SetProportion`
            // expect percentages in `0.0..=100.0` (see `niri/src/layout/floating.rs`).
            client
                .send_request(Request::Action(Action::SetWindowWidth {
                    id: Some(id),
                    change: SizeChange::SetProportion(50.0),
                }))
                .await?;
            client
                .send_request(Request::Action(Action::SetWindowHeight {
                    id: Some(id),
                    change: SizeChange::SetProportion(100.0),
                }))
                .await?;
            let x_prop = if is_left { 0.0 } else { 50.0 };
            client
                .send_request(Request::Action(Action::MoveFloatingWindow {
                    id: Some(id),
                    x: PositionChange::SetProportion(x_prop),
                    y: PositionChange::SetProportion(0.0),
                }))
                .await?;
        }
        "up" | "maximize" => {
            let already_maximized = tile_w >= work_w * 0.96 && tile_h >= work_h * 0.94;
            if !already_maximized {
                client
                    .send_request(Request::Action(Action::MaximizeWindowToEdges {
                        id: Some(id),
                    }))
                    .await?;
            }
        }
        "down" | "restore-or-minimize" => {
            if is_maximized {
                // Restore from maximized to centered 72%x75% floating window
                client
                    .send_request(Request::Action(Action::MaximizeWindowToEdges {
                        id: Some(id),
                    }))
                    .await?;
                if focused.is_floating {
                    let _ = client
                        .send_request(Request::Action(Action::SetWindowWidth {
                            id: Some(id),
                            change: SizeChange::SetProportion(72.0),
                        }))
                        .await;
                    let _ = client
                        .send_request(Request::Action(Action::SetWindowHeight {
                            id: Some(id),
                            change: SizeChange::SetProportion(75.0),
                        }))
                        .await;
                    let _ = client
                        .send_request(Request::Action(Action::MoveFloatingWindow {
                            id: Some(id),
                            x: PositionChange::SetProportion(14.0),
                            y: PositionChange::SetProportion(12.5),
                        }))
                        .await;
                }
            } else if SolarConfig::load().shell.engine == crate::engine::ShellEngine::We10x {
                solar_niri::minimized::minimize(client, id).await?;
            }
        }
        other => anyhow::bail!(
            "Unknown Aero Snap direction '{}'. Valid: left, right, up, down",
            other
        ),
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solar_niri::niri_ipc::{LogicalOutput, Output, Reply, Transform, Window, WindowLayout, Workspace};
    use std::collections::HashMap;
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        net::UnixListener,
    };

    use std::sync::atomic::{AtomicU64, Ordering};

    struct TestDir(PathBuf);
    impl TestDir {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!(
                "solarui-we10x-test-{}-{}-{}",
                tag,
                std::process::id(),
                seq
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn we10x_niri_profile_preserves_user_input_and_restores_cleanly() {
        let dir = TestDir::new("profile");
        let home = dir.path();
        let niri_path = home.join(".config/solarui/niri.kdl");
        let snap_path = home.join(".local/state/solarui/pre-we10x-snapshot.json");
        let fallback_path = home.join("system-niri.kdl");

        let original_kdl = r#"
input {
    keyboard {
        xkb {
            layout "de,tr"
        }
    }
}

output "DP-2" {
    scale 1.25
}

window-rule {
    geometry-corner-radius 20
}
"#;
        std::fs::create_dir_all(niri_path.parent().unwrap()).unwrap();
        std::fs::write(&niri_path, original_kdl).unwrap();
        std::fs::write(&fallback_path, original_kdl).unwrap();

        enter_we10x_profile_in_paths(home, &niri_path, &snap_path, &fallback_path, "dark", false)
            .unwrap();
        let active_kdl = std::fs::read_to_string(&niri_path).unwrap();
        assert!(active_kdl.contains(WE10X_PROFILE_MARKER));
        assert!(active_kdl.contains(r#"layout "de,tr""#));
        assert!(active_kdl.contains(r#"output "DP-2""#));
        assert!(active_kdl.contains("scale 1.25"));
        assert!(active_kdl.contains(r##"active-color "#0078d7""##));
        assert!(active_kdl.contains("geometry-corner-radius 0"));
        assert!(!active_kdl.contains("geometry-corner-radius 20"));
        assert!(active_kdl.contains("server-titlebar {"));
        assert!(active_kdl.contains("super-tap-spawn"));

        // GTK sharp-corner + #e81123 CSS and We10X settings.ini must exist while in We10X.
        assert!(home.join(".config/gtk-4.0/settings.ini").exists());
        let gtk4_css = std::fs::read_to_string(home.join(".config/gtk-4.0/gtk.css")).unwrap();
        assert!(gtk4_css.contains(WE10X_GTK_CSS_MARKER));
        assert!(gtk4_css.contains("#e81123"));

        // Idempotent second call (e.g. switching dark -> light) must NOT overwrite original snapshot!
        enter_we10x_profile_in_paths(home, &niri_path, &snap_path, &fallback_path, "light", false)
            .unwrap();
        let light_kdl = std::fs::read_to_string(&niri_path).unwrap();
        assert!(light_kdl.contains(r##"active-color "#ffffff""##));

        // Leaving We10X must restore the exact original KDL, delete We10X-created GTK files, and delete the snapshot.
        leave_we10x_profile_in_paths(home, &niri_path, &snap_path, &fallback_path, false).unwrap();
        let restored_kdl = std::fs::read_to_string(&niri_path).unwrap();
        assert_eq!(restored_kdl, original_kdl);
        assert!(!snap_path.exists());
        assert!(!home.join(".config/gtk-3.0/settings.ini").exists());
        assert!(!home.join(".config/gtk-4.0/settings.ini").exists());
        assert!(!home.join(".config/gtk-3.0/gtk.css").exists());
        assert!(!home.join(".config/gtk-4.0/gtk.css").exists());
    }

    #[test]
    fn provision_user_we10x_home_preserves_clean_stock_rollback_snapshot() {
        let dir = TestDir::new("oobe-prov");
        let home = dir.path();
        let fallback_path = home.join("system-niri.kdl");
        let stock_kdl = "window-rule {\n    geometry-corner-radius 20\n}\n";
        std::fs::write(&fallback_path, stock_kdl).unwrap();

        provision_user_we10x_home(home, &fallback_path, "dark").unwrap();

        let niri_path = home.join(".config/solarui/niri.kdl");
        let snap_path = home.join(".local/state/solarui/pre-we10x-snapshot.json");
        assert!(std::fs::read_to_string(&niri_path)
            .unwrap()
            .contains(WE10X_PROFILE_MARKER));
        assert!(home.join(".config/niri/config.kdl").is_symlink());
        assert!(snap_path.exists());

        // Switching away later restores stock SolarUI rounded KDL and cleans up We10X GTK files.
        leave_we10x_profile_in_paths(home, &niri_path, &snap_path, &fallback_path, false).unwrap();
        assert_eq!(std::fs::read_to_string(&niri_path).unwrap(), stock_kdl);
        assert!(!home.join(".config/gtk-4.0/settings.ini").exists());
        assert!(!home.join(".config/gtk-4.0/gtk.css").exists());
    }

    #[tokio::test]
    async fn aero_snap_focused_sends_percentage_proportions_and_unmaximizes_first() {
        let dir = TestDir::new("aerosnap");
        let sock_path = dir.path().join("niri.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let mock_out = Output {
            name: "Virtual-1".into(),
            make: "QEMU".into(),
            model: "VirtIO".into(),
            serial: None,
            physical_size: None,
            modes: vec![],
            current_mode: None,
            is_custom_mode: false,
            vrr_supported: false,
            vrr_enabled: false,
            logical: Some(LogicalOutput {
                x: 0,
                y: 0,
                width: 1280,
                height: 720,
                scale: 1.0,
                transform: Transform::Normal,
            }),
        };

        // Focused window is currently maximized (1280x676) and floating -> snapping "right" must:
        // 1. Toggle MaximizeWindowToEdges off
        // 2. SetWindowWidth(SetProportion(50.0))  (NOT 0.5!)
        // 3. SetWindowHeight(SetProportion(100.0)) (NOT 1.0!)
        // 4. MoveFloatingWindow(SetProportion(50.0), SetProportion(0.0))
        let replies: Vec<Reply> = vec![
            Ok(Response::Windows(vec![Window {
                id: 77,
                title: Some("Kitty".into()),
                app_id: Some("kitty".into()),
                pid: Some(900),
                workspace_id: Some(1),
                is_focused: true,
                is_floating: true,
                is_urgent: false,
                layout: WindowLayout {
                    pos_in_scrolling_layout: None,
                    tile_size: (1280.0, 676.0),
                    window_size: (1278, 644),
                    tile_pos_in_workspace_view: Some((0.0, 0.0)),
                    window_offset_in_tile: (1.0, 31.0),
                },
                focus_timestamp: None,
            }])),
            Ok(Response::FocusedOutput(Some(mock_out))),
            Ok(Response::Handled),
            Ok(Response::Handled),
            Ok(Response::Handled),
            Ok(Response::Handled),
        ];

        let _ = HashMap::<u8, u8>::new();
        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut line = String::new();
                BufReader::new(&mut stream)
                    .read_line(&mut line)
                    .await
                    .unwrap();
                seen.push(serde_json::from_str::<Request>(&line).unwrap());
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(b'\n');
                stream.write_all(&bytes).await.unwrap();
            }
            seen
        });

        let client = NiriClient::with_socket_path(sock_path);
        aero_snap_focused_with_client(&client, "right")
            .await
            .unwrap();

        let requests = server.await.unwrap();
        assert!(matches!(
            requests[2],
            Request::Action(Action::MaximizeWindowToEdges { id: Some(77) })
        ));
        assert!(matches!(
            requests[3],
            Request::Action(Action::SetWindowWidth {
                id: Some(77),
                change: SizeChange::SetProportion(p),
            }) if (p - 50.0).abs() < 1e-6
        ));
        assert!(matches!(
            requests[4],
            Request::Action(Action::SetWindowHeight {
                id: Some(77),
                change: SizeChange::SetProportion(p),
            }) if (p - 100.0).abs() < 1e-6
        ));
        assert!(matches!(
            requests[5],
            Request::Action(Action::MoveFloatingWindow {
                id: Some(77),
                x: PositionChange::SetProportion(px),
                y: PositionChange::SetProportion(py),
            }) if (px - 50.0).abs() < 1e-6 && py.abs() < 1e-6
        ));
    }

    #[tokio::test]
    async fn unminimize_all_windows_moves_every_background_window_back() {
        let dir = TestDir::new("unmin");
        let sock_path = dir.path().join("niri.sock");
        let listener = UnixListener::bind(&sock_path).unwrap();

        let replies: Vec<Reply> = vec![
            Ok(Response::Workspaces(vec![
                Workspace {
                    id: 10,
                    idx: 1,
                    name: Some("1".into()),
                    output: Some("Virtual-1".into()),
                    is_urgent: false,
                    is_active: true,
                    is_focused: true,
                    active_window_id: None,
                },
                Workspace {
                    id: 99,
                    idx: 2,
                    name: Some("background".into()),
                    output: Some("Virtual-1".into()),
                    is_urgent: false,
                    is_active: false,
                    is_focused: false,
                    active_window_id: Some(501),
                },
            ])),
            Ok(Response::Windows(vec![
                Window {
                    id: 501,
                    title: Some("Files".into()),
                    app_id: Some("org.gnome.Nautilus".into()),
                    pid: Some(1234),
                    workspace_id: Some(99),
                    is_focused: false,
                    is_floating: true,
                    is_urgent: false,
                    layout: WindowLayout {
                        pos_in_scrolling_layout: None,
                        tile_size: (800.0, 600.0),
                        window_size: (800, 600),
                        tile_pos_in_workspace_view: Some((100.0, 100.0)),
                        window_offset_in_tile: (0.0, 0.0),
                    },
                    focus_timestamp: None,
                },
                Window {
                    id: 502,
                    title: Some("Kitty".into()),
                    app_id: Some("kitty".into()),
                    pid: Some(1235),
                    workspace_id: Some(10),
                    is_focused: true,
                    is_floating: true,
                    is_urgent: false,
                    layout: WindowLayout {
                        pos_in_scrolling_layout: None,
                        tile_size: (980.0, 570.0),
                        window_size: (978, 538),
                        tile_pos_in_workspace_view: Some((50.0, 50.0)),
                        window_offset_in_tile: (1.0, 31.0),
                    },
                    focus_timestamp: None,
                },
            ])),
            Ok(Response::Handled),
        ];

        let server = tokio::spawn(async move {
            let mut seen = Vec::new();
            for reply in replies {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut line = String::new();
                BufReader::new(&mut stream)
                    .read_line(&mut line)
                    .await
                    .unwrap();
                seen.push(serde_json::from_str::<Request>(&line).unwrap());
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(b'\n');
                stream.write_all(&bytes).await.unwrap();
            }
            seen
        });

        let client = NiriClient::with_socket_path(sock_path);
        let count = unminimize_all_windows_with_client(&client).await.unwrap();
        assert_eq!(count, 1);

        let requests = server.await.unwrap();
        assert!(matches!(
            requests[2],
            Request::Action(Action::MoveWindowToWorkspace {
                window_id: Some(501),
                reference: WorkspaceReferenceArg::Id(10),
                focus: false,
            })
        ));
    }

    #[test]
    fn generated_we10x_niri_kdl_passes_compositor_validate() {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let niri_bin = repo_root.join("blazeos_custom_apps/usr/bin/niri");
        let base_kdl_path = repo_root.join("blazeos_custom_apps/etc/solarui/niri.kdl");
        if !niri_bin.is_file() || !base_kdl_path.is_file() {
            return;
        }
        let base_kdl = std::fs::read_to_string(&base_kdl_path).unwrap();
        let dir = TestDir::new("validate");
        for mode in ["dark", "light"] {
            let kdl = build_we10x_niri_kdl(&base_kdl, mode);
            let out_file = dir.path().join(format!("we10x-{mode}.kdl"));
            std::fs::write(&out_file, kdl).unwrap();
            let output = std::process::Command::new(&niri_bin)
                .arg("validate")
                .arg("-c")
                .arg(&out_file)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "niri validate failed for {mode} We10X KDL:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    #[test]
    fn switching_to_we10x_and_back_preserves_noctalia_wallpaper() {
        let dir = TestDir::new("wp-roundtrip");
        let home = dir.path();
        let niri_path = home.join(".config/solarui/niri.kdl");
        let snap_path = home.join(".local/state/solarui/pre-we10x-snapshot.json");
        let fallback_path = home.join("system-niri.kdl");
        let noctalia_state_toml = home.join(".local/state/noctalia/settings.toml");

        std::fs::create_dir_all(noctalia_state_toml.parent().unwrap()).unwrap();
        std::fs::write(
            &noctalia_state_toml,
            "[wallpaper]\nenabled = true\n\n[wallpaper.default]\npath = \"/usr/share/backgrounds/blazeos/default.png\"\n",
        )
        .unwrap();
        std::fs::write(&fallback_path, "layout { gaps 14; }\n").unwrap();

        enter_we10x_profile_in_paths(home, &niri_path, &snap_path, &fallback_path, "dark", false)
            .unwrap();

        let snap: PreWe10xSnapshot =
            serde_json::from_slice(&std::fs::read(&snap_path).unwrap()).unwrap();
        assert_eq!(
            snap.noctalia_wallpaper.as_deref(),
            Some("/usr/share/backgrounds/blazeos/default.png")
        );

        // Simulate a stray We10X wallpaper write into noctalia TOML while in We10X
        crate::wallpaper::update_noctalia_wallpaper_toml(&noctalia_state_toml, WE10X_WALLPAPER_DARK);

        leave_we10x_profile_in_paths(home, &niri_path, &snap_path, &fallback_path, false).unwrap();

        assert_eq!(
            crate::wallpaper::read_noctalia_wallpaper_toml(home).as_deref(),
            Some("/usr/share/backgrounds/blazeos/default.png")
        );
    }
}
