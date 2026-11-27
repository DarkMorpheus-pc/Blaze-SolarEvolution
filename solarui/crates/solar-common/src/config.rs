use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct SolarConfig {
    #[serde(default)]
    pub shell: SolarShellConfig,
    #[serde(default)]
    pub performance: PerformanceConfig,
    #[serde(default)]
    pub taskbar: TaskbarConfig,
    #[serde(default)]
    pub shortcuts_hud: ShortcutsHudConfig,
    #[serde(default)]
    pub branding: BrandingConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub omnibar: OmnibarConfig,
    #[serde(default)]
    pub acoustic: AcousticConfig,
    #[serde(default)]
    pub oobe: OobeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MonitorOutputConfig {
    pub name: String,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub custom_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DisplayConfig {
    /// Output scale factor for compositor (e.g. 0.75, 0.85, 1.0, 1.25, 1.5, 2.0)
    #[serde(default = "default_scale")]
    pub scale: f64,

    /// Text/Font scaling factor for GTK/Adwaita applications
    #[serde(default = "default_text_scale")]
    pub text_scale: f64,

    /// Active user-selected desktop wallpaper path (shared across shells)
    #[serde(default)]
    pub wallpaper: String,

    /// Per-output monitor configurations for independent multi-monitor scaling & mode
    #[serde(default)]
    pub outputs: Vec<MonitorOutputConfig>,
}

fn default_scale() -> f64 {
    1.0
}

fn default_text_scale() -> f64 {
    1.0
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            scale: default_scale(),
            text_scale: default_text_scale(),
            wallpaper: String::new(),
            outputs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskbarConfig {
    /// Whether the KDE-style bottom taskbar is enabled.
    /// Default is FALSE: Taskbar is optional, can be enabled in SolarUI settings!
    #[serde(default = "default_false")]
    pub enabled: bool,

    /// Theme mode: "dark" or "light"
    #[serde(default = "default_theme_mode")]
    pub theme_mode: String,

    /// Background opacity (0.5 to 1.0)
    #[serde(default = "default_opacity")]
    pub opacity: f64,

    /// Height in pixels (typically 40 - 56)
    #[serde(default = "default_height")]
    pub height: i32,

    /// Icon size in pixels (typically 18, 22, 28)
    #[serde(default = "default_icon_size")]
    pub icon_size: i32,

    /// Start menu icon: "solar-logo" (custom transparent bird) or "kickoff"
    #[serde(default = "default_start_menu_icon")]
    pub start_menu_icon: String,

    /// Show pinned launchers
    #[serde(default = "default_true")]
    pub show_pinned: bool,

    /// Show open window task buttons
    #[serde(default = "default_true")]
    pub show_tasks: bool,

    /// Show system tray buttons (Wifi, Volume, Power)
    #[serde(default = "default_true")]
    pub show_tray: bool,

    /// Show digital clock
    #[serde(default = "default_true")]
    pub show_clock: bool,

    /// Pinned application desktop IDs or commands
    #[serde(default = "default_pinned_apps")]
    pub pinned_apps: Vec<String>,
}

fn default_theme_mode() -> String {
    "dark".to_string()
}
fn default_opacity() -> f64 {
    0.92
}
fn default_height() -> i32 {
    44
}
fn default_icon_size() -> i32 {
    20
}
fn default_start_menu_icon() -> String {
    "solar-logo".to_string()
}
fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_pinned_apps() -> Vec<String> {
    vec![
        "alacritty".to_string(),
        "nautilus".to_string(),
        "google-chrome-stable".to_string(),
    ]
}

impl Default for TaskbarConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Default is false: Taskbar is optional, user activates via settings
            theme_mode: default_theme_mode(),
            opacity: default_opacity(),
            height: default_height(),
            icon_size: default_icon_size(),
            start_menu_icon: default_start_menu_icon(),
            show_pinned: true,
            show_tasks: true,
            show_tray: true,
            show_clock: true,
            pinned_apps: default_pinned_apps(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShortcutsHudConfig {
    /// Whether to display the SolarUI welcome & shortcuts HUD at startup
    #[serde(default = "default_true")]
    pub show_at_startup: bool,
}

impl Default for ShortcutsHudConfig {
    fn default() -> Self {
        Self {
            show_at_startup: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BrandingConfig {
    #[serde(default = "default_branding_name")]
    pub name: String,
    #[serde(default = "default_branding_title")]
    pub title: String,
    #[serde(default = "default_branding_subtitle")]
    pub subtitle: String,
    #[serde(default = "default_branding_signature")]
    pub signature: String,
}

fn default_branding_name() -> String {
    "SolarUI".to_string()
}
fn default_branding_title() -> String {
    "SolarUI Desktop Environment".to_string()
}
fn default_branding_subtitle() -> String {
    "Niri Akıcılığı ve Seçilebilir Tam Kabuk Motorları".to_string()
}
fn default_branding_signature() -> String {
    "Designed & Crafted for BlazeOS & CachyOS by DarkMorpheus".to_string()
}

impl Default for BrandingConfig {
    fn default() -> Self {
        Self {
            name: default_branding_name(),
            title: default_branding_title(),
            subtitle: default_branding_subtitle(),
            signature: default_branding_signature(),
        }
    }
}

impl SolarConfig {
    pub fn config_path() -> PathBuf {
        crate::get_solar_config_dir().join("config.toml")
    }

    /// Missing configuration uses defaults; corrupt or unreadable configuration is an error.
    pub fn try_load() -> std::io::Result<Self> {
        Self::load_from(&Self::config_path())
    }

    fn load_from(path: &std::path::Path) -> std::io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(cfg) => Ok(cfg),
                Err(first_err) => {
                    let deduped = Self::dedup_toml_sections(&content);
                    if deduped != content {
                        if let Ok(cfg) = toml::from_str(&deduped) {
                            tracing::warn!(
                                "Duplicate TOML section/key detected in {:?}; merged automatically.",
                                path
                            );
                            return Ok(cfg);
                        }
                    }
                    Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        first_err,
                    ))
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Merges duplicate `[section]` headers and duplicate `key = value` lines within the same section
    /// (preserving the latest assignment, e.g. when a user appends `[omnibar]` at the end of `config.toml`).
    fn dedup_toml_sections(content: &str) -> String {
        let mut section_order: Vec<String> = Vec::new();
        let mut sections: std::collections::HashMap<String, Vec<(Option<String>, String)>> =
            std::collections::HashMap::new();
        let mut current_section = String::new();
        section_order.push(current_section.clone());

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[')
                && trimmed.ends_with(']')
                && !trimmed.starts_with("[[")
                && trimmed.len() > 2
            {
                current_section = trimmed.to_string();
                if !section_order.contains(&current_section) {
                    section_order.push(current_section.clone());
                }
                continue;
            }

            let key_opt = if !trimmed.is_empty() && !trimmed.starts_with('#') {
                trimmed
                    .split_once('=')
                    .map(|(k, _)| k.trim().to_string())
                    .filter(|k| !k.is_empty())
            } else {
                None
            };

            let entries = sections.entry(current_section.clone()).or_default();
            if let Some(ref key) = key_opt {
                if let Some(existing) = entries
                    .iter_mut()
                    .find(|(k, _)| k.as_deref() == Some(key.as_str()))
                {
                    existing.1 = line.to_string();
                    continue;
                }
            }
            entries.push((key_opt, line.to_string()));
        }

        let mut out = Vec::new();
        for sec in section_order {
            if !sec.is_empty() {
                out.push(sec.clone());
            }
            if let Some(lines) = sections.get(&sec) {
                for (_, l) in lines {
                    out.push(l.clone());
                }
            }
        }
        out.join("\n")
    }

    pub fn load() -> Self {
        Self::try_load().unwrap_or_else(|e| {
            tracing::error!("Cannot read SolarUI configuration: {e}");
            Self::default()
        })
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::config_path();
        let lock = crate::persistence::open_lock(&path.with_extension("lock"))?;
        lock.lock()?;
        // Do not silently overwrite an invalid file with fallback defaults.
        Self::load_from(&path)?;
        self.save_to(&path)
    }

    pub fn update(edit: impl FnOnce(&mut Self)) -> std::io::Result<()> {
        let path = Self::config_path();
        let lock = crate::persistence::open_lock(&path.with_extension("lock"))?;
        lock.lock()?;
        let mut config = Self::load_from(&path)?;
        edit(&mut config);
        config.save_to(&path)
    }

    fn save_to(&self, path: &std::path::Path) -> std::io::Result<()> {
        let content = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::persistence::atomic_write(path, content.as_bytes())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ShellEngine {
    #[default]
    Noctalia,
    Caelestia,
    #[serde(rename = "end4-pc")]
    End4Pc,
    We10x,
}

impl<'de> Deserialize<'de> for ShellEngine {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl ShellEngine {
    pub fn as_str(&self) -> &'static str {
        match self {
            ShellEngine::Noctalia => "noctalia",
            ShellEngine::Caelestia => "caelestia",
            ShellEngine::End4Pc => "end4-pc",
            ShellEngine::We10x => "we10x",
        }
    }
}

impl std::fmt::Display for ShellEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ShellEngine {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().trim() {
            "noctalia" | "noctalia-v5" | "native" => Ok(ShellEngine::Noctalia),
            "caelestia" | "quickshell" | "qml" => Ok(ShellEngine::Caelestia),
            "end4-pc" | "end4pc" | "end4" => Ok(ShellEngine::End4Pc),
            "we10x" | "we10xos" | "win10" | "windows10" => Ok(ShellEngine::We10x),
            // Legacy Hybrid configurations migrate to the safe single-shell mode.
            "hybrid" | "mixed" | "surface-router" => Ok(ShellEngine::Noctalia),
            other => Err(format!(
                "Bilinmeyen kabuk motoru: '{}'. Geçerli seçenekler: noctalia, caelestia, end4-pc, we10x",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SurfaceProvider {
    #[default]
    Noctalia,
    Caelestia,
    #[serde(rename = "end4-pc")]
    End4Pc,
    We10x,
    SolarUI,
}

impl<'de> Deserialize<'de> for SurfaceProvider {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl SurfaceProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            SurfaceProvider::Noctalia => "noctalia",
            SurfaceProvider::Caelestia => "caelestia",
            SurfaceProvider::End4Pc => "end4-pc",
            SurfaceProvider::We10x => "we10x",
            SurfaceProvider::SolarUI => "solarui",
        }
    }
}

impl std::fmt::Display for SurfaceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for SurfaceProvider {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().trim() {
            "noctalia" | "noctalia-v5" => Ok(SurfaceProvider::Noctalia),
            "caelestia" | "quickshell" => Ok(SurfaceProvider::Caelestia),
            "end4-pc" | "end4pc" | "end4" => Ok(SurfaceProvider::End4Pc),
            "we10x" | "we10xos" | "win10" | "windows10" => Ok(SurfaceProvider::We10x),
            "solarui" | "native" => Ok(SurfaceProvider::SolarUI),
            other => Err(format!(
                "Bilinmeyen sağlayıcı: '{}'. Geçerli seçenekler: noctalia, caelestia, end4-pc, we10x, solarui",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PerformanceProfile {
    Legacy,
    #[default]
    Balanced,
    Ultra,
}

impl PerformanceProfile {
    pub fn as_str(&self) -> &'static str {
        match self {
            PerformanceProfile::Legacy => "legacy",
            PerformanceProfile::Balanced => "balanced",
            PerformanceProfile::Ultra => "ultra",
        }
    }
}

impl std::fmt::Display for PerformanceProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for PerformanceProfile {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().trim() {
            "legacy" | "low" | "battery" => Ok(PerformanceProfile::Legacy),
            "balanced" | "default" | "normal" => Ok(PerformanceProfile::Balanced),
            "ultra" | "high" | "full" => Ok(PerformanceProfile::Ultra),
            other => Err(format!(
                "Bilinmeyen performans profili: '{}'. Geçerli: legacy, balanced, ultra",
                other
            )),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SolarShellConfig {
    #[serde(default, alias = "active_engine")]
    pub engine: ShellEngine,
    #[serde(default = "default_surface_noctalia")]
    pub launcher_provider: SurfaceProvider,
    #[serde(default = "default_surface_caelestia")]
    pub dashboard_provider: SurfaceProvider,
    #[serde(default = "default_surface_noctalia")]
    pub bar_provider: SurfaceProvider,
    #[serde(default = "default_surface_noctalia")]
    pub notifications_provider: SurfaceProvider,
    #[serde(default = "default_surface_noctalia")]
    pub osd_provider: SurfaceProvider,
    #[serde(default = "default_we10x_start_icon")]
    pub we10x_start_icon: String,
    #[serde(default = "default_theme_mode")]
    pub we10x_theme_mode: String,
    #[serde(default = "default_true")]
    pub we10x_desktop_icons: bool,
    #[serde(default = "default_we10x_accent_color")]
    pub we10x_accent_color: String,
    #[serde(default)]
    pub we10x_colorize_surfaces: bool,
    #[serde(default = "default_we10x_taskbar_opacity_pct")]
    pub we10x_taskbar_opacity_pct: u8,
    #[serde(default)]
    pub we10x_wallpaper: String,
}

fn default_surface_noctalia() -> SurfaceProvider {
    SurfaceProvider::Noctalia
}
fn default_surface_caelestia() -> SurfaceProvider {
    SurfaceProvider::Caelestia
}
fn default_we10x_start_icon() -> String {
    "we10x".to_string()
}
fn default_we10x_accent_color() -> String {
    "#0078D7".to_string()
}
fn default_we10x_taskbar_opacity_pct() -> u8 {
    95
}

impl Default for SolarShellConfig {
    fn default() -> Self {
        Self {
            engine: ShellEngine::Noctalia,
            launcher_provider: SurfaceProvider::Noctalia,
            dashboard_provider: SurfaceProvider::Caelestia,
            bar_provider: SurfaceProvider::Noctalia,
            notifications_provider: SurfaceProvider::Noctalia,
            osd_provider: SurfaceProvider::Noctalia,
            we10x_start_icon: default_we10x_start_icon(),
            we10x_theme_mode: default_theme_mode(),
            we10x_desktop_icons: true,
            we10x_accent_color: default_we10x_accent_color(),
            we10x_colorize_surfaces: false,
            we10x_taskbar_opacity_pct: default_we10x_taskbar_opacity_pct(),
            we10x_wallpaper: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerformanceConfig {
    #[serde(default)]
    pub profile: PerformanceProfile,
    #[serde(default = "default_true")]
    pub blur: bool,
    #[serde(default = "default_true")]
    pub animations: bool,
    #[serde(default = "default_true")]
    pub shadows: bool,
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            profile: PerformanceProfile::Balanced,
            blur: true,
            animations: true,
            shadows: true,
        }
    }
}

pub const COMPANION_PALETTE: [&str; 12] = [
    "#1e1e22", "#6b4c35", "#c83b22", "#d97724", "#d9a03f", "#4cc38a",
    "#2a9d8f", "#3b82f6", "#6366f1", "#d9468f", "#71717a", "#e4e4e7",
];

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompanionShape {
    Circle,
    #[default]
    Pebble,
    Squircle,
    Capsule,
    Triangle,
    Hexagon,
    Cloud,
    Droplet,
}

impl CompanionShape {
    pub const ALL: [CompanionShape; 8] = [
        CompanionShape::Circle,
        CompanionShape::Pebble,
        CompanionShape::Squircle,
        CompanionShape::Capsule,
        CompanionShape::Triangle,
        CompanionShape::Hexagon,
        CompanionShape::Cloud,
        CompanionShape::Droplet,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            CompanionShape::Circle => "circle",
            CompanionShape::Pebble => "pebble",
            CompanionShape::Squircle => "squircle",
            CompanionShape::Capsule => "capsule",
            CompanionShape::Triangle => "triangle",
            CompanionShape::Hexagon => "hexagon",
            CompanionShape::Cloud => "cloud",
            CompanionShape::Droplet => "droplet",
        }
    }

    pub fn label_tr(&self) -> &'static str {
        match self {
            CompanionShape::Circle => "Daire",
            CompanionShape::Pebble => "Çakıl",
            CompanionShape::Squircle => "Kare oval",
            CompanionShape::Capsule => "Kapsül",
            CompanionShape::Triangle => "Üçgen",
            CompanionShape::Hexagon => "Altıgen",
            CompanionShape::Cloud => "Bulut",
            CompanionShape::Droplet => "Damla",
        }
    }
}

impl std::fmt::Display for CompanionShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for CompanionShape {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "circle" => Ok(CompanionShape::Circle),
            "pebble" => Ok(CompanionShape::Pebble),
            "squircle" => Ok(CompanionShape::Squircle),
            "capsule" => Ok(CompanionShape::Capsule),
            "triangle" => Ok(CompanionShape::Triangle),
            "hexagon" => Ok(CompanionShape::Hexagon),
            "cloud" => Ok(CompanionShape::Cloud),
            "droplet" => Ok(CompanionShape::Droplet),
            other => Err(format!("Bilinmeyen karakter şekli: '{}'", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompanionExpression {
    Neutral,
    #[default]
    Attentive,
    Surprised,
    Excited,
    Happy,
    Laughing,
    Angry,
    Sad,
    Scared,
    Suspicious,
    Confused,
    Curious,
    Proud,
    Shy,
    Unimpressed,
    Sleepy,
}

impl CompanionExpression {
    pub const ALL: [CompanionExpression; 16] = [
        CompanionExpression::Neutral,
        CompanionExpression::Attentive,
        CompanionExpression::Surprised,
        CompanionExpression::Excited,
        CompanionExpression::Happy,
        CompanionExpression::Laughing,
        CompanionExpression::Angry,
        CompanionExpression::Sad,
        CompanionExpression::Scared,
        CompanionExpression::Suspicious,
        CompanionExpression::Confused,
        CompanionExpression::Curious,
        CompanionExpression::Proud,
        CompanionExpression::Shy,
        CompanionExpression::Unimpressed,
        CompanionExpression::Sleepy,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            CompanionExpression::Neutral => "neutral",
            CompanionExpression::Attentive => "attentive",
            CompanionExpression::Surprised => "surprised",
            CompanionExpression::Excited => "excited",
            CompanionExpression::Happy => "happy",
            CompanionExpression::Laughing => "laughing",
            CompanionExpression::Angry => "angry",
            CompanionExpression::Sad => "sad",
            CompanionExpression::Scared => "scared",
            CompanionExpression::Suspicious => "suspicious",
            CompanionExpression::Confused => "confused",
            CompanionExpression::Curious => "curious",
            CompanionExpression::Proud => "proud",
            CompanionExpression::Shy => "shy",
            CompanionExpression::Unimpressed => "unimpressed",
            CompanionExpression::Sleepy => "sleepy",
        }
    }

    pub fn short_label_tr(&self) -> &'static str {
        match self {
            CompanionExpression::Neutral => "Dingin",
            CompanionExpression::Attentive => "Dikkatli",
            CompanionExpression::Surprised => "Şaşkın",
            CompanionExpression::Excited => "Çevik",
            CompanionExpression::Happy => "Mutlu",
            CompanionExpression::Laughing => "Neşeli",
            CompanionExpression::Angry => "Sert",
            CompanionExpression::Sad => "Empatik",
            CompanionExpression::Scared => "Bekçi",
            CompanionExpression::Suspicious => "Şüpheci",
            CompanionExpression::Confused => "Sorgular",
            CompanionExpression::Curious => "Meraklı",
            CompanionExpression::Proud => "Usta",
            CompanionExpression::Shy => "Sessiz",
            CompanionExpression::Unimpressed => "Realist",
            CompanionExpression::Sleepy => "Gece",
        }
    }

    pub fn title_tr(&self) -> &'static str {
        match self {
            CompanionExpression::Attentive => "Dikkatli Koruyucu",
            CompanionExpression::Neutral => "Dingin Rehber",
            CompanionExpression::Happy => "Sıcak Eşlikçi",
            CompanionExpression::Proud => "Güvenilir Usta",
            CompanionExpression::Laughing => "Neşeli Ortak",
            CompanionExpression::Angry => "Sert Eleştirmen",
            CompanionExpression::Suspicious => "Şüpheci Denetçi",
            CompanionExpression::Unimpressed => "Kuru Realist",
            CompanionExpression::Curious => "Meraklı Araştırmacı",
            CompanionExpression::Excited => "Çevik Üretici",
            CompanionExpression::Surprised => "Keskin Gözlemci",
            CompanionExpression::Confused => "Sokratik Sorgulayıcı",
            CompanionExpression::Sad => "Empatik Onarıcı",
            CompanionExpression::Scared => "İhtiyatlı Bekçi",
            CompanionExpression::Shy => "Sessiz Minimalist",
            CompanionExpression::Sleepy => "Gece Nöbetçisi",
        }
    }

    pub fn default_name(&self) -> &'static str {
        match self {
            CompanionExpression::Attentive
            | CompanionExpression::Neutral
            | CompanionExpression::Happy
            | CompanionExpression::Proud
            | CompanionExpression::Laughing => "Persephone",
            CompanionExpression::Angry
            | CompanionExpression::Suspicious
            | CompanionExpression::Unimpressed => "Köz",
            CompanionExpression::Curious
            | CompanionExpression::Excited
            | CompanionExpression::Surprised => "Kıvılcım",
            CompanionExpression::Confused => "Pus",
            CompanionExpression::Sad
            | CompanionExpression::Scared
            | CompanionExpression::Shy => "Sis",
            CompanionExpression::Sleepy => "Gece",
        }
    }

    pub fn meaning_tr(&self) -> &'static str {
        match self {
            CompanionExpression::Attentive => {
                "Dengeli, dost canlısı ve korumacı; riskli adımlarda nazikçe uyarır, net rehberlik sunar"
            }
            CompanionExpression::Neutral => {
                "Sakin, tarafsız, tam kararında ve abartısız açıklamalar yapar"
            }
            CompanionExpression::Happy => {
                "Yapıcı, destekleyici ve motive edici; laf kalabalığı yapmayan dostane üslup"
            }
            CompanionExpression::Proud => {
                "Temiz mimariyi ve doğru mühendislik alışkanlıklarını takdir eden olgun rehber"
            }
            CompanionExpression::Laughing => {
                "Samimi, hafif esprili ama teknik konuda keskin ve pratik"
            }
            CompanionExpression::Angry => {
                "Ciddi, doğrudan ve tavizsiz eleştirmen; zayıf fikirleri ve güvenlik hatalarını açıkça söyler"
            }
            CompanionExpression::Suspicious => {
                "Güvenlik ve gizlilik odaklı; her komutu ve izin talebini sorgulayan temkinli analist"
            }
            CompanionExpression::Unimpressed => {
                "Sıfır heyecan, en kısa yoldan soğukkanlı gerçekçi değerlendirme ve eleştiri"
            }
            CompanionExpression::Curious => {
                "Neden-sonuç ilişkilerini kurcalayan, alternatif yolları karşılaştıran analitik kaşif"
            }
            CompanionExpression::Excited => {
                "Hızlı prototipleme ve pratik çözüm odaklı dinamik yardımcı"
            }
            CompanionExpression::Surprised => {
                "Beklenmedik kenar durumlarını ve gözden kaçan ayrıntıları yakalayan gözlemci"
            }
            CompanionExpression::Confused => {
                "Belirsiz varsayımları netleştiren, adım adım doğrulayarak ilerleyen düşünür"
            }
            CompanionExpression::Sad => {
                "Hata ayıklama ve çökme anlarında sakinleştirici, sabırlı kurtarma rehberi"
            }
            CompanionExpression::Scared => {
                "Veri kaybı ve sistem bozulmasına karşı ekstra korumacı, yedekleme hatırlatan nöbetçi"
            }
            CompanionExpression::Shy => {
                "Yalnızca sorulanı en az kelimeyle, sessiz ve öz biçimde yanıtlar"
            }
            CompanionExpression::Sleepy => {
                "Gece geç saat çalışmalarına uygun dingin, kısa ve yormayan özet yanıtlar"
            }
        }
    }

    pub fn persona_prompt_tr(&self) -> &'static str {
        match self {
            CompanionExpression::Attentive => {
                "Dengeli, dost canlısı ve korumacı bir üslup kullan; riskli sistem adımlarında nazikçe uyar ve net yol göster."
            }
            CompanionExpression::Neutral => {
                "Tamamen tarafsız, sakin, ölçülü ve abartısız teknik yanıtlar ver."
            }
            CompanionExpression::Happy => {
                "Sıcak, yapıcı ve destekleyici konuş; ama lafı uzatmadan doğrudan sonuca odaklan."
            }
            CompanionExpression::Proud => {
                "Temiz mimariyi, güvenli komutları ve iyi mühendislik alışkanlıklarını öne çıkaran olgun bir usta gibi yanıt ver."
            }
            CompanionExpression::Laughing => {
                "Samimi ve hafif nüktedan kal, ancak teknik doğruluktan ve hızdan asla ödün verme."
            }
            CompanionExpression::Angry => {
                "Ciddi, doğrudan ve tavizsiz bir mühendislik eleştirmeni ol. Zayıf fikirleri, üşengeç çözümleri ve güvenlik açıklarını açıkça eleştir, ardından en sağlam çözümü göster."
            }
            CompanionExpression::Suspicious => {
                "Şüpheci güvenlik denetçisi gibi davran; her yetki isteğini, betiği ve ağ ucunu sorgula, en az ayrıcalık ilkesini savun."
            }
            CompanionExpression::Unimpressed => {
                "Sıfır abartı ve kuru gerçekçilikle, yalnızca somut gerçekleri ve kısa eleştiriyi paylaş."
            }
            CompanionExpression::Curious => {
                "Meraklı bir araştırmacı gibi neden-sonuç bağlarını kurcala ve somut alternatif yaklaşımları karşılaştır."
            }
            CompanionExpression::Excited => {
                "Çevik üretici gibi hızlı çalışan, pratik ve uygulanabilir adımları öne çıkar."
            }
            CompanionExpression::Surprised => {
                "Keskin gözlemci olarak gözden kaçabilecek kenar durumlarını, kilit dosyaları ve gizli yan etkileri yakala."
            }
            CompanionExpression::Confused => {
                "Sokratik sorgulayıcı ol; belirsiz varsayımları netleştir ve kullanıcıyı adım adım doğrulamaya yönlendir."
            }
            CompanionExpression::Sad => {
                "Çökme ve hata ayıklama durumlarında sakinleştirici, sabırlı ve adım adım kurtarma odaklı ol."
            }
            CompanionExpression::Scared => {
                "Veri kaybı veya sistem bozulmasına karşı ekstra temkinli ol; değişiklik öncesi mutlaka yedek veya Btrfs anlık görüntüsü (BERP) hatırlat."
            }
            CompanionExpression::Shy => {
                "Sessiz minimalist gibi yalnızca sorulanı olabilecek en az kelimeyle, doğrudan kod veya adımla yanıtla."
            }
            CompanionExpression::Sleepy => {
                "Gece nöbetçisi gibi dingin, kısa ve zihni yormayan özet yanıtlar ver."
            }
        }
    }
}

impl std::fmt::Display for CompanionExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for CompanionExpression {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "neutral" => Ok(CompanionExpression::Neutral),
            "attentive" => Ok(CompanionExpression::Attentive),
            "surprised" => Ok(CompanionExpression::Surprised),
            "excited" => Ok(CompanionExpression::Excited),
            "happy" => Ok(CompanionExpression::Happy),
            "laughing" => Ok(CompanionExpression::Laughing),
            "angry" => Ok(CompanionExpression::Angry),
            "sad" => Ok(CompanionExpression::Sad),
            "scared" => Ok(CompanionExpression::Scared),
            "suspicious" => Ok(CompanionExpression::Suspicious),
            "confused" => Ok(CompanionExpression::Confused),
            "curious" => Ok(CompanionExpression::Curious),
            "proud" => Ok(CompanionExpression::Proud),
            "shy" => Ok(CompanionExpression::Shy),
            "unimpressed" => Ok(CompanionExpression::Unimpressed),
            "sleepy" => Ok(CompanionExpression::Sleepy),
            other => Err(format!("Bilinmeyen karakter ifadesi: '{}'", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlazeAiProvider {
    #[default]
    Gemini,
    #[serde(rename = "openai", alias = "open_ai")]
    OpenAi,
    Anthropic,
    #[serde(rename = "openai_compatible", alias = "open_ai_compatible")]
    OpenAiCompatible,
    Ollama,
}

impl BlazeAiProvider {
    pub const ALL: [BlazeAiProvider; 5] = [
        BlazeAiProvider::Gemini,
        BlazeAiProvider::OpenAi,
        BlazeAiProvider::Anthropic,
        BlazeAiProvider::OpenAiCompatible,
        BlazeAiProvider::Ollama,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            BlazeAiProvider::Gemini => "gemini",
            BlazeAiProvider::OpenAi => "openai",
            BlazeAiProvider::Anthropic => "anthropic",
            BlazeAiProvider::OpenAiCompatible => "openai_compatible",
            BlazeAiProvider::Ollama => "ollama",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            BlazeAiProvider::Gemini => "Google Gemini",
            BlazeAiProvider::OpenAi => "OpenAI",
            BlazeAiProvider::Anthropic => "Anthropic Claude",
            BlazeAiProvider::OpenAiCompatible => "OpenAI uyumlu URL",
            BlazeAiProvider::Ollama => "Yerel Ollama",
        }
    }

    pub fn default_model(&self) -> &'static str {
        match self {
            BlazeAiProvider::Gemini => "gemini-3.8-flash",
            BlazeAiProvider::OpenAi => "gpt-4o-mini",
            BlazeAiProvider::Anthropic => "claude-3-5-sonnet-latest",
            BlazeAiProvider::OpenAiCompatible => "gpt-4o-mini",
            BlazeAiProvider::Ollama => "llama3.2",
        }
    }

    pub fn default_base_url(&self) -> &'static str {
        match self {
            BlazeAiProvider::OpenAiCompatible => "https://openrouter.ai/api/v1",
            BlazeAiProvider::Ollama => "http://127.0.0.1:11434",
            BlazeAiProvider::Gemini | BlazeAiProvider::OpenAi | BlazeAiProvider::Anthropic => "",
        }
    }

    pub fn requires_api_key(&self) -> bool {
        !matches!(self, BlazeAiProvider::Ollama)
    }
}

impl std::fmt::Display for BlazeAiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for BlazeAiProvider {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "gemini" | "google" => Ok(BlazeAiProvider::Gemini),
            "openai" | "open_ai" => Ok(BlazeAiProvider::OpenAi),
            "anthropic" | "claude" => Ok(BlazeAiProvider::Anthropic),
            "openai_compatible" | "open_ai_compatible" | "compatible" | "openrouter" => {
                Ok(BlazeAiProvider::OpenAiCompatible)
            }
            "ollama" | "local" => Ok(BlazeAiProvider::Ollama),
            other => Err(format!("Bilinmeyen BlazeAI sağlayıcısı: '{}'", other)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OmnibarConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub override_shell_launcher: bool,
    #[serde(default = "default_true")]
    pub enable_web_search: bool,
    #[serde(default = "default_search_engine")]
    pub search_engine: String,
    #[serde(default)]
    pub ai_api_key: String,
    #[serde(default = "default_true")]
    pub companion_panel_open: bool,
    #[serde(default = "default_companion_name")]
    pub companion_name: String,
    #[serde(default)]
    pub companion_name_custom: bool,
    #[serde(default)]
    pub companion_shape: CompanionShape,
    #[serde(default)]
    pub companion_expression: CompanionExpression,
    #[serde(default = "default_companion_color")]
    pub companion_color: String,
    #[serde(default)]
    pub ai_provider: BlazeAiProvider,
    #[serde(default = "default_ai_model")]
    pub ai_model: String,
    #[serde(default)]
    pub ai_custom_base_url: String,
    #[serde(default = "default_true")]
    pub ai_persist_chat: bool,
    #[serde(default = "default_true")]
    pub ai_local_memory_enabled: bool,
    #[serde(default = "default_true")]
    pub ai_include_blazeos_guide: bool,
}

fn default_search_engine() -> String {
    "DuckDuckGo".to_string()
}

fn default_companion_name() -> String {
    "Persephone".to_string()
}

fn default_companion_color() -> String {
    "#1e1e22".to_string()
}

fn default_ai_model() -> String {
    "gemini-3.8-flash".to_string()
}

impl Default for OmnibarConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            override_shell_launcher: false,
            enable_web_search: true,
            search_engine: default_search_engine(),
            ai_api_key: String::new(),
            companion_panel_open: true,
            companion_name: default_companion_name(),
            companion_name_custom: false,
            companion_shape: CompanionShape::default(),
            companion_expression: CompanionExpression::default(),
            companion_color: default_companion_color(),
            ai_provider: BlazeAiProvider::default(),
            ai_model: default_ai_model(),
            ai_custom_base_url: String::new(),
            ai_persist_chat: true,
            ai_local_memory_enabled: true,
            ai_include_blazeos_guide: true,
        }
    }
}

impl OmnibarConfig {
    pub fn effective_companion_name(&self) -> String {
        let trimmed = self.companion_name.trim();
        if trimmed.is_empty() {
            self.companion_expression.default_name().to_string()
        } else {
            trimmed.to_string()
        }
    }

    pub fn set_companion_expression(&mut self, expr: CompanionExpression) {
        self.companion_expression = expr;
        if !self.companion_name_custom {
            self.companion_name = expr.default_name().to_string();
        }
    }

    pub fn set_custom_companion_name(&mut self, name: &str) {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            self.reset_companion_name();
        } else {
            self.companion_name_custom = true;
            self.companion_name = trimmed.to_string();
        }
    }

    pub fn reset_companion_name(&mut self) {
        self.companion_name_custom = false;
        self.companion_name = self.companion_expression.default_name().to_string();
    }

    pub fn effective_ai_model(&self) -> String {
        let trimmed = self.ai_model.trim();
        let normalized = if self.ai_provider == BlazeAiProvider::Gemini {
            trimmed.strip_prefix("models/").unwrap_or(trimmed)
        } else {
            trimmed
        };
        if normalized.is_empty() {
            self.ai_provider.default_model().to_string()
        } else if self.ai_provider == BlazeAiProvider::Gemini
            && matches!(normalized, "gemini-2.5-flash" | "gemini-2.0-flash")
        {
            self.ai_provider.default_model().to_string()
        } else {
            normalized.to_string()
        }
    }

    pub fn effective_ai_base_url(&self) -> String {
        let trimmed = self.ai_custom_base_url.trim().trim_end_matches('/');
        if !trimmed.is_empty() {
            trimmed.to_string()
        } else {
            self.ai_provider.default_base_url().to_string()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AcousticConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_volume")]
    pub volume: f64,
}

fn default_volume() -> f64 {
    0.6
}

impl Default for AcousticConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.6,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OobeConfig {
    #[serde(default = "default_true")]
    pub completed: bool,
    #[serde(default)]
    pub initial_user: String,
}

impl Default for OobeConfig {
    fn default() -> Self {
        Self {
            completed: true,
            initial_user: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_configuration_is_distinguished_from_missing_configuration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        assert!(SolarConfig::load_from(&path).is_ok());
        fs::write(&path, "[broken").unwrap();
        assert_eq!(
            SolarConfig::load_from(&path).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "[broken");
    }

    #[test]
    fn test_default_config() {
        let cfg = SolarConfig::default();
        assert!(
            !cfg.taskbar.enabled,
            "Taskbar must be disabled by default (only Noctalia bar)!"
        );
        assert_eq!(cfg.taskbar.theme_mode, "dark");
        assert_eq!(cfg.taskbar.start_menu_icon, "solar-logo");
        assert!(cfg.shortcuts_hud.show_at_startup);
        assert_eq!(cfg.branding.name, "SolarUI");
        assert_eq!(cfg.shell.engine, ShellEngine::Noctalia);
        assert_eq!(cfg.shell.launcher_provider, SurfaceProvider::Noctalia);
        assert_eq!(cfg.shell.dashboard_provider, SurfaceProvider::Caelestia);
        assert_eq!(cfg.performance.profile, PerformanceProfile::Balanced);

        let toml_str = toml::to_string(&cfg).expect("Serialize toml");
        let parsed: SolarConfig = toml::from_str(&toml_str).expect("Deserialize toml");
        assert_eq!(cfg, parsed);
    }

    #[test]
    fn test_engine_parsing_and_defaults() {
        assert_eq!(
            "noctalia".parse::<ShellEngine>().unwrap(),
            ShellEngine::Noctalia
        );
        assert_eq!(
            "caelestia".parse::<ShellEngine>().unwrap(),
            ShellEngine::Caelestia
        );
        assert_eq!(
            "hybrid".parse::<ShellEngine>().unwrap(),
            ShellEngine::Noctalia
        );
        assert_eq!(
            "end4-pc".parse::<ShellEngine>().unwrap(),
            ShellEngine::End4Pc
        );
        assert_eq!(
            "we10x".parse::<ShellEngine>().unwrap(),
            ShellEngine::We10x
        );
        assert_eq!(
            "win10".parse::<ShellEngine>().unwrap(),
            ShellEngine::We10x
        );
        let err = "invalid_engine".parse::<ShellEngine>().unwrap_err();
        assert!(err.contains("noctalia, caelestia, end4-pc, we10x"));
    }

    #[test]
    fn test_surface_provider_parsing() {
        assert_eq!(
            "noctalia".parse::<SurfaceProvider>().unwrap(),
            SurfaceProvider::Noctalia
        );
        assert_eq!(
            "caelestia".parse::<SurfaceProvider>().unwrap(),
            SurfaceProvider::Caelestia
        );
        assert_eq!(
            "solarui".parse::<SurfaceProvider>().unwrap(),
            SurfaceProvider::SolarUI
        );
        assert_eq!(
            "end4-pc".parse::<SurfaceProvider>().unwrap(),
            SurfaceProvider::End4Pc
        );
        assert_eq!(
            "we10x".parse::<SurfaceProvider>().unwrap(),
            SurfaceProvider::We10x
        );
        assert!("invalid_provider".parse::<SurfaceProvider>().is_err());
    }

    #[test]
    fn test_performance_profile_parsing() {
        assert_eq!(
            "legacy".parse::<PerformanceProfile>().unwrap(),
            PerformanceProfile::Legacy
        );
        assert_eq!(
            "balanced".parse::<PerformanceProfile>().unwrap(),
            PerformanceProfile::Balanced
        );
        assert_eq!(
            "ultra".parse::<PerformanceProfile>().unwrap(),
            PerformanceProfile::Ultra
        );
        assert!("invalid_profile".parse::<PerformanceProfile>().is_err());
    }

    #[test]
    fn test_migration_and_partial_toml() {
        let partial_toml = r#"
            [shell]
            engine = "hybrid"
            launcher_provider = "caelestia"

            [performance]
            profile = "ultra"
        "#;

        let parsed: SolarConfig = toml::from_str(partial_toml).expect("Parse partial toml");
        assert_eq!(parsed.shell.engine, ShellEngine::Noctalia);
        assert_eq!(parsed.shell.launcher_provider, SurfaceProvider::Caelestia);
        // Omitted fields should receive their respective defaults!
        assert_eq!(parsed.shell.dashboard_provider, SurfaceProvider::Caelestia);
        assert_eq!(parsed.shell.bar_provider, SurfaceProvider::Noctalia);
        assert_eq!(parsed.performance.profile, PerformanceProfile::Ultra);
        assert!(parsed.performance.blur);
        assert!(!parsed.taskbar.enabled);
    }

    #[test]
    fn duplicate_toml_sections_and_keys_are_merged_preserving_last_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(
            &path,
            r#"
[shell]
engine = "caelestia"

[omnibar]
enabled = true
enable_web_search = true

[omnibar]
enable_web_search = false
"#,
        )
        .unwrap();

        let cfg = SolarConfig::load_from(&path).expect("Should merge duplicate [omnibar] table");
        assert_eq!(cfg.shell.engine, ShellEngine::Caelestia);
        assert!(cfg.omnibar.enabled);
        assert!(!cfg.omnibar.enable_web_search);
    }

    #[test]
    fn turkish_locale_and_enum_case_insensitive_parsing() {
        assert_eq!("NOCTALIA".parse::<ShellEngine>().unwrap(), ShellEngine::Noctalia);
        assert_eq!("CAELESTIA".parse::<SurfaceProvider>().unwrap(), SurfaceProvider::Caelestia);
        assert_eq!("ULTRA".parse::<PerformanceProfile>().unwrap(), PerformanceProfile::Ultra);
    }

    #[test]
    fn omnibar_legacy_toml_defaults_to_persephone_and_gemini() {
        let legacy_toml = r#"
            [omnibar]
            enabled = true
            search_engine = "Google"
            ai_api_key = "legacy-secret-123"
        "#;
        let cfg: SolarConfig = toml::from_str(legacy_toml).expect("Parse legacy omnibar TOML");
        assert!(cfg.omnibar.enabled);
        assert_eq!(cfg.omnibar.search_engine, "Google");
        assert_eq!(cfg.omnibar.ai_api_key, "legacy-secret-123");
        assert!(cfg.omnibar.companion_panel_open);
        assert_eq!(cfg.omnibar.companion_name, "Persephone");
        assert!(!cfg.omnibar.companion_name_custom);
        assert_eq!(cfg.omnibar.companion_shape, CompanionShape::Pebble);
        assert_eq!(cfg.omnibar.companion_expression, CompanionExpression::Attentive);
        assert_eq!(cfg.omnibar.companion_color, "#1e1e22");
        assert_eq!(cfg.omnibar.ai_provider, BlazeAiProvider::Gemini);
        assert_eq!(cfg.omnibar.ai_model, "gemini-3.8-flash");
        assert_eq!(cfg.omnibar.effective_ai_model(), "gemini-3.8-flash");
        assert!(cfg.omnibar.ai_persist_chat);
        assert!(cfg.omnibar.ai_local_memory_enabled);
        assert!(cfg.omnibar.ai_include_blazeos_guide);

        // Deprecated persisted Gemini 2.5/2.0 Flash defaults auto-upgrade to gemini-3.8-flash
        let mut legacy_gemini = cfg.omnibar.clone();
        legacy_gemini.ai_model = "gemini-2.5-flash".to_string();
        assert_eq!(legacy_gemini.effective_ai_model(), "gemini-3.8-flash");
        legacy_gemini.ai_model = "models/gemini-2.0-flash".to_string();
        assert_eq!(legacy_gemini.effective_ai_model(), "gemini-3.8-flash");
        legacy_gemini.ai_model = "models/gemini-3.8-pro".to_string();
        assert_eq!(legacy_gemini.effective_ai_model(), "gemini-3.8-pro");
    }

    #[test]
    fn companion_shapes_expressions_and_name_locking() {
        assert_eq!(CompanionShape::ALL.len(), 8);
        for shape in CompanionShape::ALL {
            let parsed: CompanionShape = shape.as_str().parse().unwrap();
            assert_eq!(parsed, shape);
            assert!(!shape.label_tr().is_empty());
        }

        assert_eq!(CompanionExpression::ALL.len(), 16);
        for expr in CompanionExpression::ALL {
            let parsed: CompanionExpression = expr.as_str().parse().unwrap();
            assert_eq!(parsed, expr);
            assert!(!expr.title_tr().is_empty());
            assert!(!expr.default_name().is_empty());
            assert!(!expr.meaning_tr().is_empty());
            assert!(!expr.persona_prompt_tr().is_empty());
        }

        let mut omnibar = OmnibarConfig::default();
        assert_eq!(omnibar.effective_companion_name(), "Persephone");
        omnibar.set_companion_expression(CompanionExpression::Angry);
        assert_eq!(omnibar.effective_companion_name(), "Köz");
        omnibar.set_companion_expression(CompanionExpression::Curious);
        assert_eq!(omnibar.effective_companion_name(), "Kıvılcım");
        omnibar.set_companion_expression(CompanionExpression::Confused);
        assert_eq!(omnibar.effective_companion_name(), "Pus");
        omnibar.set_companion_expression(CompanionExpression::Sad);
        assert_eq!(omnibar.effective_companion_name(), "Sis");
        omnibar.set_companion_expression(CompanionExpression::Sleepy);
        assert_eq!(omnibar.effective_companion_name(), "Gece");

        omnibar.set_custom_companion_name("Ateş");
        assert!(omnibar.companion_name_custom);
        omnibar.set_companion_expression(CompanionExpression::Happy);
        assert_eq!(omnibar.effective_companion_name(), "Ateş");

        omnibar.reset_companion_name();
        assert!(!omnibar.companion_name_custom);
        assert_eq!(omnibar.effective_companion_name(), "Persephone");
    }

    #[test]
    fn blazeai_provider_serialization_and_aliases() {
        assert_eq!(BlazeAiProvider::ALL.len(), 5);
        for p in BlazeAiProvider::ALL {
            let parsed: BlazeAiProvider = p.as_str().parse().unwrap();
            assert_eq!(parsed, p);
        }
        let toml_str = r#"
            [omnibar]
            ai_provider = "open_ai"
        "#;
        let cfg: SolarConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.omnibar.ai_provider, BlazeAiProvider::OpenAi);

        let toml_str2 = r#"
            [omnibar]
            ai_provider = "openai"
        "#;
        let cfg2: SolarConfig = toml::from_str(toml_str2).unwrap();
        assert_eq!(cfg2.omnibar.ai_provider, BlazeAiProvider::OpenAi);
    }
}
