#[cfg(feature = "gui")]
pub mod gui;
pub mod integration;
pub mod templates;

use anyhow::Result;
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{info, warn};

pub use templates::{Template, create_from_template, list_templates};

pub const MODULE_ID: &str = "new-plus";

fn default_template_location() -> String {
    "~/.local/share/my-power-toys/new-plus-templates".into()
}

fn default_true() -> bool {
    true
}

/// Configuration for New+ (`~/.config/my-power-toys/new-plus.toml`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewPlusConfig {
    /// Templates folder; a leading `~/` is expanded to the home directory.
    #[serde(default = "default_template_location")]
    pub template_location: String,
    /// Hide file extensions in menus.
    #[serde(default = "default_true")]
    pub hide_file_extension: bool,
    /// Strip leading ordering digits ("01. ") from menu entries and created names.
    #[serde(default = "default_true")]
    pub hide_starting_digits: bool,
    /// Resolve `$YYYY`, `%USERNAME%`, `$PARENT_FOLDER_NAME`, ... in created names.
    #[serde(default)]
    pub replace_variables: bool,
    /// Generate Nautilus scripts and a Dolphin service menu.
    #[serde(default = "default_true")]
    pub context_menu: bool,
    /// Symlink templates into the XDG Templates folder.
    #[serde(default)]
    pub sync_xdg_templates: bool,
}

impl Default for NewPlusConfig {
    fn default() -> Self {
        Self {
            template_location: default_template_location(),
            hide_file_extension: true,
            hide_starting_digits: true,
            replace_variables: false,
            context_menu: true,
            sync_xdg_templates: false,
        }
    }
}

impl NewPlusConfig {
    pub fn load() -> Self {
        mpt_common::config::load_module_config(MODULE_ID).unwrap_or_else(|e| {
            warn!("New+: failed to load config: {e:#}");
            Self::default()
        })
    }

    /// Resolved templates folder path.
    pub fn templates_dir(&self) -> PathBuf {
        let loc = self.template_location.trim();
        let loc = if loc.is_empty() {
            default_template_location()
        } else {
            loc.to_string()
        };
        if loc == "~" {
            return dirs::home_dir().unwrap_or_default();
        }
        if let Some(rest) = loc.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(rest);
        }
        PathBuf::from(loc)
    }
}

/// Reload config, make sure the templates folder exists and regenerate file-manager integration.
pub fn refresh() -> Result<()> {
    let config = NewPlusConfig::load();
    if let Err(e) = templates::ensure_templates_dir(&config.templates_dir()) {
        warn!("New+: {e:#}");
    }
    integration::apply(&config, &find_gui_binary())
}

pub struct NewPlus {
    running: bool,
}

impl NewPlus {
    pub fn new() -> Self {
        Self { running: false }
    }

    /// Regenerate menus after templates or config changed.
    pub fn refresh(&mut self) -> Result<()> {
        refresh()
    }
}

impl Default for NewPlus {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for NewPlus {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn name(&self) -> &'static str {
        "New+"
    }

    fn description(&self) -> &'static str {
        "Create files and folders from personal templates"
    }

    fn start(&mut self) -> Result<()> {
        if let Err(e) = refresh() {
            warn!("New+: refresh failed: {e:#}");
        }
        self.running = true;
        info!("New+ module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        integration::remove_all();
        self.running = false;
        info!("New+ module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        // Pick up template/config changes in the menus as well.
        if let Err(e) = refresh() {
            warn!("New+: refresh failed: {e:#}");
        }
        let bin = find_gui_binary();
        match std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => info!("New+ GUI spawned (pid={})", child.id()),
            Err(e) => warn!("Failed to spawn New+ GUI ({bin:?}): {e}"),
        }
        Ok(())
    }
}

/// Find the mpt-new-plus binary, looking next to the current executable first.
pub fn find_gui_binary() -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-new-plus");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-new-plus".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trip() {
        let cfg = NewPlusConfig {
            replace_variables: true,
            ..Default::default()
        };
        let s = toml::to_string(&cfg).unwrap();
        assert_eq!(toml::from_str::<NewPlusConfig>(&s).unwrap(), cfg);
        assert_eq!(
            toml::from_str::<NewPlusConfig>("").unwrap(),
            NewPlusConfig::default()
        );
    }
}
