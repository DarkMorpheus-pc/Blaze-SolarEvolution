pub mod caelestia;
pub mod end4_pc;
pub mod noctalia;
pub mod we10x;

use crate::actions::ShellAction;
use anyhow::Result;
use async_trait::async_trait;

pub use caelestia::CaelestiaProvider;
pub use end4_pc::End4PcProvider;
pub use noctalia::NoctaliaProvider;
pub use we10x::We10xProvider;

#[async_trait]
pub trait ProcessExt {
    async fn checked_status(&mut self) -> Result<()>;
    async fn output_timeout(&mut self) -> Result<std::process::Output>;
}

#[async_trait]
impl ProcessExt for tokio::process::Command {
    async fn checked_status(&mut self) -> Result<()> {
        let output = self.output_timeout().await?;
        anyhow::ensure!(
            output.status.success(),
            "Command failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        anyhow::ensure!(
            !output.stdout.starts_with(b"error:"),
            "IPC command failed: {}",
            String::from_utf8_lossy(&output.stdout)
        );
        Ok(())
    }
    async fn output_timeout(&mut self) -> Result<std::process::Output> {
        use anyhow::Context;
        self.kill_on_drop(true);
        tokio::time::timeout(std::time::Duration::from_secs(2), self.output())
            .await
            .context("Shell IPC command timed out")?
            .context("Cannot run shell command")
    }
}

pub(crate) fn find_uid_process_pids(matches_proc: impl Fn(&str, &str) -> bool) -> Vec<i32> {
    let uid = solar_common::current_uid();
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<i32>().ok()?;
            let metadata = std::fs::metadata(entry.path()).ok()?;
            use std::os::unix::fs::MetadataExt;
            if metadata.uid() != uid {
                return None;
            }
            let comm = std::fs::read_to_string(entry.path().join("comm")).ok()?;
            let status = std::fs::read_to_string(entry.path().join("status")).unwrap_or_default();
            if status.lines().any(|l| l.starts_with("State:") && (l.contains("Z (zombie)") || l.contains("X (dead)"))) {
                return None;
            }
            let cmdline = std::fs::read(entry.path().join("cmdline")).ok()?;
            let text = String::from_utf8_lossy(&cmdline).replace('\0', " ");
            matches_proc(comm.trim(), &text).then_some(pid)
        })
        .collect()
}

#[async_trait]
pub trait ShellProvider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn is_installed(&self) -> bool;
    async fn start(&self) -> Result<()>;
    async fn stop(&self) -> Result<()>;
    async fn health_check(&self) -> Result<bool>;
    async fn process_alive(&self) -> bool {
        self.health_check().await.unwrap_or(false)
    }
    async fn dispatch(&self, action: ShellAction) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn failed_commands_and_ipc_errors_are_not_success() {
        assert!(tokio::process::Command::new("false")
            .checked_status()
            .await
            .is_err());
        assert!(tokio::process::Command::new("printf")
            .arg("error: unavailable\n")
            .checked_status()
            .await
            .is_err());
        assert!(tokio::process::Command::new("true")
            .checked_status()
            .await
            .is_ok());
    }
}
