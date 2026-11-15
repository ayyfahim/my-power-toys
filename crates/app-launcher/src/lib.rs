pub mod actions;
pub mod calculator;
pub mod config;
pub mod desktop;
pub mod frecency;
#[cfg(feature = "gui")]
pub mod gui;
pub mod placement;
pub mod plugins;
pub mod search;
pub mod x11;

use anyhow::Result;
use mpt_common::hotkey::Hotkey;
use mpt_common::module::PowerModule;
use tracing::{info, warn};

pub use config::{AppLauncherConfig, LauncherPosition, PluginId, PluginSettings, ResolvedPlugin};
pub use plugins::{Engine, LauncherResult};

pub struct AppLauncher {
    running: bool,
    index: search::SearchIndex,
    config: AppLauncherConfig,
}

impl AppLauncher {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("app-launcher").unwrap_or_default();
        Self {
            running: false,
            index: search::SearchIndex::new(),
            config,
        }
    }

    pub fn config(&self) -> &AppLauncherConfig {
        &self.config
    }

    /// Headless search over the indexed applications (legacy API).
    pub fn search(&self, query: &str) -> Vec<search::SearchResult> {
        self.index
            .search(query, self.config.max_results, self.config.show_calculator)
    }
}

impl Default for AppLauncher {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for AppLauncher {
    fn id(&self) -> &'static str {
        "app-launcher"
    }

    fn name(&self) -> &'static str {
        "App Launcher"
    }

    fn description(&self) -> &'static str {
        "PowerToys Run style launcher: apps, calculator, shell, files, windows, web search and more"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        // Alt+Space (the upstream PowerToys Run default) belongs to Command
        // Palette; users can bind App Launcher to any hotkey in settings.
        None
    }

    fn start(&mut self) -> Result<()> {
        self.config = mpt_common::config::load_module_config("app-launcher").unwrap_or_default();
        info!("App Launcher: indexing .desktop files...");
        self.index.refresh()?;
        info!(
            "App Launcher: indexed {} applications",
            self.index.app_count()
        );
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("App Launcher stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        self.config = mpt_common::config::load_module_config("app-launcher").unwrap_or_default();
        let bin = find_gui_binary();
        let result = std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        match result {
            Ok(child) => info!("App Launcher GUI spawned (pid={})", child.id()),
            Err(e) => warn!("Failed to spawn App Launcher GUI ({bin:?}): {e}"),
        }
        Ok(())
    }
}

/// Find the mpt-app-launcher binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-app-launcher");
        if candidate.exists() {
            return candidate;
        }
    }
    // Fallback: rely on PATH
    "mpt-app-launcher".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrip() {
        let config = AppLauncherConfig::default();
        let s = toml::to_string_pretty(&config).unwrap();
        let parsed: AppLauncherConfig = toml::from_str(&s).unwrap();
        assert_eq!(parsed.max_results, 8);
        assert!(parsed.show_calculator);
    }

    #[test]
    fn empty_toml_gives_defaults() {
        let parsed: AppLauncherConfig = toml::from_str("").unwrap();
        assert_eq!(parsed.max_results, 8);
        assert!(parsed.show_calculator);
    }

    #[test]
    fn no_default_hotkey_to_avoid_command_palette_clash() {
        let l = AppLauncher {
            running: false,
            index: search::SearchIndex::new(),
            config: AppLauncherConfig::default(),
        };
        assert_eq!(l.default_hotkey(), None);
    }
}
