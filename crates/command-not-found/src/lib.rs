//! Command Not Found: when a shell command is missing, suggest packages that
//! provide it (Linux port of the PowerToys PowerShell/WinGet module).
//!
//! The module installs shell hooks (bash `command_not_found_handle`, zsh
//! `command_not_found_handler`, fish `fish_command_not_found`) that call the
//! `mpt-command-not-found` binary.
//!
//! `stop()` intentionally does NOT remove the hooks: silently editing the
//! user's rc files whenever the daemon exits would be surprising, and the hook
//! falls back to the shell's default message if the binary is missing. To
//! remove them, trigger the named action `"uninstall"` (or run
//! `mpt-command-not-found --uninstall-hooks`).

use anyhow::Result;
use mpt_common::module::PowerModule;
use tracing::{info, warn};

pub mod config;
pub mod hooks;
pub mod lookup;

pub use config::CommandNotFoundConfig;
pub use hooks::{install_shell_hooks, uninstall_shell_hooks};

pub const MODULE_ID: &str = "command-not-found";

/// Load the module config, falling back to defaults on error.
pub fn load_config() -> CommandNotFoundConfig {
    mpt_common::config::load_module_config::<CommandNotFoundConfig>(MODULE_ID).unwrap_or_else(|e| {
        warn!("Failed to load {MODULE_ID} config, using defaults: {e}");
        CommandNotFoundConfig::default()
    })
}

pub struct CommandNotFound {
    running: bool,
}

impl CommandNotFound {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for CommandNotFound {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for CommandNotFound {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn name(&self) -> &'static str {
        "Command Not Found"
    }

    fn description(&self) -> &'static str {
        "Suggest packages that provide a missing shell command"
    }

    fn start(&mut self) -> Result<()> {
        let cfg = load_config();
        if cfg.install_shell_hooks {
            // A failure here (e.g. binary not installed) should not keep the
            // daemon from starting the module.
            if let Err(e) = install_shell_hooks(&cfg) {
                warn!("Could not install command-not-found shell hooks: {e:#}");
            }
        } else {
            info!("Command Not Found: shell hook installation disabled in config");
        }
        self.running = true;
        Ok(())
    }

    /// Does not remove the shell hooks; see the crate docs and the
    /// `"uninstall"` named action.
    fn stop(&mut self) -> Result<()> {
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    /// `"install"`: (re)install hooks per config toggles.
    /// `"uninstall"`: remove the marked blocks from all shells.
    fn on_named_action(&mut self, action: &str) -> Result<()> {
        match action {
            "install" => {
                install_shell_hooks(&load_config())?;
            }
            "uninstall" => {
                uninstall_shell_hooks()?;
            }
            other => warn!("Command Not Found: unknown action {other:?}"),
        }
        Ok(())
    }
}
