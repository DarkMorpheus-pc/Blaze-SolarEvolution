use super::{ProcessExt, ShellProvider};
use crate::actions::ShellAction;
use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

/// Supervised We10X (Windows 10 Familiar Classic Layout) Quickshell provider.
///
/// Selects its QML directory by an explicit path (`/usr/share/quickshell/we10x`)
/// and isolates PIDs by command-line path so it never collides with Caelestia or end4-pC.
pub struct We10xProvider {
    qs_bin: PathBuf,
    config_dir: PathBuf,
}

impl Default for We10xProvider {
    fn default() -> Self {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/liveuser".to_string());
        let checkout = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("../../vendor/we10x-shell")));
        let candidates = [
            std::env::var_os("SOLARUI_WE10X_DIR").map(PathBuf::from),
            Some(PathBuf::from("/usr/share/quickshell/we10x")),
            Some(PathBuf::from("/etc/xdg/quickshell/we10x")),
            Some(PathBuf::from(&home).join(".config/quickshell/we10x")),
            checkout,
        ];
        let config_dir = candidates
            .into_iter()
            .flatten()
            .find(|path| path.join("shell.qml").is_file())
            .unwrap_or_else(|| PathBuf::from("/usr/share/quickshell/we10x"));

        let qs_bin = [
            PathBuf::from("/usr/bin/quickshell-fedora"),
            PathBuf::from("/usr/bin/qs"),
            PathBuf::from("/usr/bin/quickshell"),
        ]
        .into_iter()
        .find(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("qs"));

        Self { qs_bin, config_dir }
    }
}

impl We10xProvider {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cmd(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.qs_bin);
        cmd.env_remove("LD_LIBRARY_PATH")
            .env_remove("QT_PLUGIN_PATH")
            .env_remove("QT_QPA_PLATFORM_PLUGIN_PATH")
            .env_remove("QML2_IMPORT_PATH")
            .env_remove("QML_IMPORT_PATH")
            .args(["--path"])
            .arg(&self.config_dir)
            .env("XDG_CURRENT_DESKTOP", "SolarUI:Niri")
            .env("XDG_SESSION_DESKTOP", "SolarUI:Niri")
            .env("QS_ICON_THEME", "We10X")
            .env("QT_QPA_PLATFORM", "wayland");
        if let Ok(value) = std::env::var("WAYLAND_DISPLAY") {
            cmd.env("WAYLAND_DISPLAY", value);
        }
        if let Ok(value) = std::env::var("XDG_RUNTIME_DIR") {
            cmd.env("XDG_RUNTIME_DIR", value);
        }
        if let Ok(sock) = solar_niri::find_niri_socket() {
            cmd.env("NIRI_SOCKET", sock);
        }
        cmd
    }

    async fn get_pids(&self) -> Vec<i32> {
        let needle = self.config_dir.to_string_lossy();
        super::find_uid_process_pids(|_, text| {
            (text.contains("qs") || text.contains("quickshell"))
                && text.contains(needle.as_ref())
                && !text.contains(" ipc ")
        })
    }

    pub async fn ipc(&self, target: &str, function: &str) -> Result<std::process::Output> {
        self.cmd()
            .args(["ipc", "call", target, function])
            .output_timeout()
            .await
    }

    pub async fn ipc_checked(&self, target: &str, function: &str) -> Result<()> {
        let output = self.ipc(target, function).await?;
        anyhow::ensure!(
            output.status.success(),
            "We10X IPC failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        Ok(())
    }

    pub fn ipc_target(action: ShellAction) -> (&'static str, &'static str) {
        match action {
            ShellAction::Launcher => ("startMenu", "toggle"),
            ShellAction::ControlCenter | ShellAction::Dashboard => ("actionCenter", "toggle"),
            ShellAction::WindowSwitcher => ("altTab", "toggle"),
            ShellAction::Overview => ("taskView", "toggle"),
            ShellAction::Session => ("powerDialog", "toggle"),
            ShellAction::Settings => ("settings", "toggle"),
        }
    }
}

#[async_trait]
impl ShellProvider for We10xProvider {
    fn name(&self) -> &'static str {
        "we10x"
    }

    async fn is_installed(&self) -> bool {
        self.config_dir.join("shell.qml").is_file()
            && (self.qs_bin.is_file() || self.qs_bin == Path::new("qs"))
    }

    async fn start(&self) -> Result<()> {
        if self.health_check().await.unwrap_or(false) {
            return Ok(());
        }
        if !self.get_pids().await.is_empty() {
            self.stop().await?;
        }
        anyhow::ensure!(
            self.is_installed().await,
            "We10X shell or Quickshell is not installed"
        );

        info!("Starting We10X shell from {:?}", self.config_dir);
        self.cmd()
            .arg("--daemonize")
            .spawn()
            .context("Cannot start We10X shell")?;

        for _ in 0..300 {
            sleep(Duration::from_millis(100)).await;
            if self.health_check().await.unwrap_or(false) {
                return Ok(());
            }
        }
        let diagnostics = self
            .cmd()
            .arg("log")
            .output_timeout()
            .await
            .map(|out| {
                format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                )
            })
            .unwrap_or_else(|error| format!("log unavailable: {error:#}"));
        let _ = self.stop().await;
        let tail = diagnostics
            .lines()
            .rev()
            .take(20)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("\n");
        bail!("We10X shell did not become IPC-ready within 30 seconds\n{tail}")
    }

    async fn stop(&self) -> Result<()> {
        let pids = self.get_pids().await;
        for pid in &pids {
            let _ = tokio::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .status()
                .await;
        }
        for _ in 0..20 {
            if self.get_pids().await.is_empty() {
                return Ok(());
            }
            sleep(Duration::from_millis(100)).await;
        }
        warn!("We10X shell ignored SIGTERM; terminating owned PIDs");
        for pid in self.get_pids().await {
            let _ = tokio::process::Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status()
                .await;
        }
        anyhow::ensure!(
            self.get_pids().await.is_empty(),
            "We10X processes could not be stopped"
        );
        Ok(())
    }

    async fn process_alive(&self) -> bool {
        !self.get_pids().await.is_empty()
    }

    async fn health_check(&self) -> Result<bool> {
        if self.get_pids().await.is_empty() {
            return Ok(false);
        }
        let output = match self.ipc("solarui", "ping").await {
            Ok(output) => output,
            Err(_) => return Ok(false),
        };
        Ok(output.status.success()
            && String::from_utf8_lossy(&output.stdout)
                .to_ascii_lowercase()
                .contains("ready"))
    }

    async fn dispatch(&self, action: ShellAction) -> Result<()> {
        let (target, function) = Self::ipc_target(action);
        self.ipc_checked(target, function).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solar_actions_have_stable_we10x_ipc_targets() {
        assert_eq!(
            We10xProvider::ipc_target(ShellAction::Launcher),
            ("startMenu", "toggle")
        );
        assert_eq!(
            We10xProvider::ipc_target(ShellAction::ControlCenter),
            ("actionCenter", "toggle")
        );
        assert_eq!(
            We10xProvider::ipc_target(ShellAction::Dashboard),
            ("actionCenter", "toggle")
        );
        assert_eq!(
            We10xProvider::ipc_target(ShellAction::WindowSwitcher),
            ("altTab", "toggle")
        );
        assert_eq!(
            We10xProvider::ipc_target(ShellAction::Overview),
            ("taskView", "toggle")
        );
        assert_eq!(
            We10xProvider::ipc_target(ShellAction::Session),
            ("powerDialog", "toggle")
        );
        assert_eq!(
            We10xProvider::ipc_target(ShellAction::Settings),
            ("settings", "toggle")
        );
    }

    #[test]
    fn cmd_sanitizes_caelestia_qt_and_ld_env_vars() {
        let provider = We10xProvider::new();
        let std_cmd = provider.cmd().into_std();
        let envs: Vec<(&std::ffi::OsStr, Option<&std::ffi::OsStr>)> = std_cmd.get_envs().collect();
        for key in [
            "LD_LIBRARY_PATH",
            "QT_PLUGIN_PATH",
            "QT_QPA_PLATFORM_PLUGIN_PATH",
            "QML2_IMPORT_PATH",
            "QML_IMPORT_PATH",
        ] {
            assert!(
                envs.iter()
                    .any(|(k, v)| *k == std::ffi::OsStr::new(key) && v.is_none()),
                "Expected {key} to be explicitly removed in We10xProvider::cmd()"
            );
        }
        assert!(
            envs.iter().any(|(k, v)| {
                *k == std::ffi::OsStr::new("QS_ICON_THEME")
                    && *v == Some(std::ffi::OsStr::new("We10X"))
            }),
            "Expected QS_ICON_THEME=We10X in We10xProvider::cmd()"
        );
    }
}
