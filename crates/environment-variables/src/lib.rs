//! Environment Variables: edit user/system variables (systemd environment.d)
//! and switch between named profiles. Linux port of the PowerToys module.

pub mod envd;
#[cfg(feature = "gui")]
pub mod gui;
pub mod listvalue;
pub mod manager;
pub mod profiles;
pub mod session;
pub mod store;

use anyhow::Result;
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub const MODULE_ID: &str = "environment-variables";

fn default_true() -> bool {
    true
}

/// Settings stored in `~/.config/my-power-toys/environment-variables.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvironmentVariablesConfig {
    /// Upstream `LaunchAdministrator` (default true). On Linux the editor never
    /// runs as root; instead this enables editing system variables, which are
    /// written to `/etc/environment.d/50-my-power-toys-system.conf` via pkexec.
    #[serde(default = "default_true")]
    pub launch_administrator: bool,
    /// After saving, push changes into the running session with
    /// `dbus-update-activation-environment --systemd` / `systemctl --user
    /// set-environment` so newly launched apps see them.
    #[serde(default = "default_true")]
    pub update_session: bool,
}

impl Default for EnvironmentVariablesConfig {
    fn default() -> Self {
        Self {
            launch_administrator: true,
            update_session: true,
        }
    }
}

pub fn load_config() -> EnvironmentVariablesConfig {
    mpt_common::config::load_module_config(MODULE_ID).unwrap_or_else(|e| {
        warn!("Environment Variables: failed to load config: {e:#}");
        EnvironmentVariablesConfig::default()
    })
}

pub struct EnvironmentVariables {
    running: bool,
}

impl EnvironmentVariables {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for EnvironmentVariables {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for EnvironmentVariables {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn name(&self) -> &'static str {
        "Environment Variables"
    }

    fn description(&self) -> &'static str {
        "Manage user environment variables and profiles"
    }

    fn start(&mut self) -> Result<()> {
        self.running = true;
        info!("Environment Variables module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Environment Variables module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        launch_gui()
    }
}

/// Spawn the `mpt-environment-variables` editor window.
pub fn launch_gui() -> Result<()> {
    let bin = find_gui_binary();
    match std::process::Command::new(&bin)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(child) => {
            info!("Environment Variables GUI spawned (pid={})", child.id());
            Ok(())
        }
        Err(e) => {
            warn!("Failed to spawn Environment Variables GUI ({bin:?}): {e}");
            Err(e.into())
        }
    }
}

/// Find the mpt-environment-variables binary, looking next to the current
/// executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-environment-variables");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-environment-variables".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults_and_roundtrip() {
        let c: EnvironmentVariablesConfig = toml::from_str("").unwrap();
        assert_eq!(c, EnvironmentVariablesConfig::default());
        assert!(c.launch_administrator);
        let c2 = EnvironmentVariablesConfig {
            launch_administrator: false,
            update_session: true,
        };
        let s = toml::to_string(&c2).unwrap();
        assert_eq!(
            toml::from_str::<EnvironmentVariablesConfig>(&s).unwrap(),
            c2
        );
    }
}
