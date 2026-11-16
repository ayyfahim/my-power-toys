//! File Locksmith: find and stop processes that are using files or folders.

#[cfg(feature = "gui")]
pub mod gui;
pub mod integration;
pub mod kill;
pub mod scan;

use anyhow::Result;
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{info, warn};

pub use integration::{install_file_manager_integration, uninstall_file_manager_integration};
pub use kill::{KillOutcome, end_all, end_process};
pub use scan::{ProcessInfo, ScanResult, Scanner, find_processes};

pub const MODULE_ID: &str = "file-locksmith";

fn default_true() -> bool {
    true
}

fn default_kill_timeout_ms() -> u64 {
    3000
}

fn default_refresh_interval_ms() -> u64 {
    2000
}

/// Settings stored in `~/.config/my-power-toys/file-locksmith.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileLocksmithConfig {
    /// Install the "Unlock with File Locksmith" entry into Nautilus, Dolphin
    /// and Thunar while the module is enabled. Loose equivalent of upstream
    /// `ExtendedContextMenuOnly` (Linux file managers have no extended menu).
    #[serde(default = "default_true")]
    pub context_menu: bool,
    /// Time to wait after SIGTERM before sending SIGKILL ("End task").
    #[serde(default = "default_kill_timeout_ms")]
    pub kill_timeout_ms: u64,
    /// How often the window re-scans to drop processes that have exited
    /// (upstream removes exited processes from the list automatically).
    /// 0 disables automatic refresh.
    #[serde(default = "default_refresh_interval_ms")]
    pub refresh_interval_ms: u64,
}

impl Default for FileLocksmithConfig {
    fn default() -> Self {
        Self {
            context_menu: true,
            kill_timeout_ms: default_kill_timeout_ms(),
            refresh_interval_ms: default_refresh_interval_ms(),
        }
    }
}

impl FileLocksmithConfig {
    pub fn load() -> Self {
        mpt_common::config::load_module_config(MODULE_ID).unwrap_or_default()
    }

    pub fn kill_timeout(&self) -> Duration {
        Duration::from_millis(self.kill_timeout_ms)
    }
}

pub struct FileLocksmith {
    running: bool,
}

impl FileLocksmith {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for FileLocksmith {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for FileLocksmith {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn name(&self) -> &'static str {
        "File Locksmith"
    }

    fn description(&self) -> &'static str {
        "Find and stop processes that are using a file or folder"
    }

    fn start(&mut self) -> Result<()> {
        let config = FileLocksmithConfig::load();
        let r = if config.context_menu {
            install_file_manager_integration()
        } else {
            uninstall_file_manager_integration()
        };
        if let Err(e) = r {
            warn!("File Locksmith: file manager integration failed: {e:#}");
        }
        self.running = true;
        info!("File Locksmith module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Err(e) = uninstall_file_manager_integration() {
            warn!("File Locksmith: failed to remove file manager integration: {e:#}");
        }
        self.running = false;
        info!("File Locksmith module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        launch_gui(&[])
    }
}

/// Find the mpt-file-locksmith binary, looking next to the current executable first.
pub fn find_gui_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-file-locksmith");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-file-locksmith".into()
}

/// Spawn the File Locksmith window for `paths` (empty = show the picker).
pub fn launch_gui(paths: &[PathBuf]) -> Result<()> {
    let bin = find_gui_binary();
    match std::process::Command::new(&bin)
        .args(paths)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            info!("File Locksmith GUI spawned (pid={})", child.id());
            Ok(())
        }
        Err(e) => {
            warn!("Failed to spawn File Locksmith GUI ({bin:?}): {e}");
            Err(e.into())
        }
    }
}

/// Environment variables a GUI needs to reach the user's display session.
const SESSION_VARS: &[&str] = &[
    "DISPLAY",
    "XAUTHORITY",
    "WAYLAND_DISPLAY",
    "XDG_RUNTIME_DIR",
    "DBUS_SESSION_BUS_ADDRESS",
];

/// Build the `pkexec` argument list that relaunches the GUI as root
/// ("Run as administrator"). pkexec clears the environment, so the session
/// variables are passed through `env`. Paths are passed as separate args.
pub fn elevated_command_args(
    bin: &std::path::Path,
    paths: &[PathBuf],
    env: impl Fn(&str) -> Option<OsString>,
) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["env".into()];
    for var in SESSION_VARS {
        if let Some(val) = env(var) {
            let mut kv = OsString::from(format!("{var}="));
            kv.push(val);
            args.push(kv);
        }
    }
    args.push(bin.as_os_str().to_owned());
    args.push("--elevated".into());
    if !paths.is_empty() {
        args.push("--".into());
        args.extend(paths.iter().map(|p| p.as_os_str().to_owned()));
    }
    args
}

/// Start `pkexec env ... mpt-file-locksmith --elevated -- <paths>`.
pub fn spawn_elevated(paths: &[PathBuf]) -> Result<std::process::Child> {
    let mut bin = find_gui_binary();
    if !bin.is_absolute()
        && let Some(found) = which(&bin)
    {
        bin = found;
    }
    let args = elevated_command_args(&bin, paths, |v| std::env::var_os(v));
    let child = std::process::Command::new("pkexec")
        .args(args)
        .stdin(std::process::Stdio::null())
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to run pkexec: {e}"))?;
    Ok(child)
}

fn which(name: &std::path::Path) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|p| {
        std::env::split_paths(&p)
            .map(|d| d.join(name))
            .find(|c| c.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_roundtrip_and_defaults() {
        let c = FileLocksmithConfig::default();
        let s = toml::to_string(&c).unwrap();
        assert_eq!(toml::from_str::<FileLocksmithConfig>(&s).unwrap(), c);
        let partial: FileLocksmithConfig = toml::from_str("context_menu = false").unwrap();
        assert!(!partial.context_menu);
        assert_eq!(partial.kill_timeout_ms, 3000);
    }

    #[test]
    fn elevated_args() {
        let args = elevated_command_args(
            std::path::Path::new("/usr/bin/mpt-file-locksmith"),
            &[PathBuf::from("/tmp/a b"), PathBuf::from("-x")],
            |v| (v == "DISPLAY").then(|| OsString::from(":0")),
        );
        let args: Vec<String> = args.into_iter().map(|a| a.into_string().unwrap()).collect();
        assert_eq!(
            args,
            [
                "env",
                "DISPLAY=:0",
                "/usr/bin/mpt-file-locksmith",
                "--elevated",
                "--",
                "/tmp/a b",
                "-x"
            ]
        );
    }
}
