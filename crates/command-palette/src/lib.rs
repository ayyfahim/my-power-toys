pub mod clipboard;
pub mod frecency;
#[cfg(feature = "gui")]
pub mod gui;
pub mod providers;
pub mod search;
pub mod x11;

#[cfg(test)]
mod parity_tests;

use anyhow::Result;
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use tracing::{info, warn};

// ── Configuration ───────────────────────────────────────────────────────────

fn default_max_results() -> usize {
    8
}

fn default_search_engine() -> String {
    "google".into()
}

fn default_file_tool() -> String {
    "auto".into()
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default = "default_true")]
    pub apps: bool,
    #[serde(default = "default_true")]
    pub calculator: bool,
    #[serde(default = "default_true")]
    pub web_search: bool,
    #[serde(default = "default_true")]
    pub shell_commands: bool,
    #[serde(default = "default_true")]
    pub system_commands: bool,
    #[serde(default = "default_true")]
    pub file_search: bool,
    #[serde(default = "default_true")]
    pub settings: bool,
    /// Switch to open windows (upstream Window Walker).
    #[serde(default = "default_true")]
    pub window_walker: bool,
    /// Recent text clipboard entries (upstream Clipboard History). Off by
    /// default for privacy; enabling it also starts the background watcher
    /// in the daemon.
    #[serde(default)]
    pub clipboard_history: bool,
    /// User-defined bookmarks (`bookmarks` list).
    #[serde(default = "default_true")]
    pub bookmarks: bool,
    /// Current time/date in several formats (upstream Time and Date).
    #[serde(default = "default_true")]
    pub time_date: bool,
    /// Distro/Flatpak package search (upstream WinGet extension).
    #[serde(default = "default_true")]
    pub packages: bool,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            apps: true,
            calculator: true,
            web_search: true,
            shell_commands: true,
            system_commands: true,
            file_search: true,
            settings: true,
            window_walker: true,
            clipboard_history: false,
            bookmarks: true,
            time_date: true,
            packages: true,
        }
    }
}

/// Activation keywords for the keyword-driven providers. A keyword made of
/// letters must be followed by a space (`clip foo`); symbol keywords can be
/// typed directly before the query (`<firefox`). An empty keyword disables
/// keyword activation for that provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderKeywords {
    #[serde(default = "kw_window_walker")]
    pub window_walker: String,
    #[serde(default = "kw_clipboard")]
    pub clipboard_history: String,
    #[serde(default = "kw_bookmarks")]
    pub bookmarks: String,
    #[serde(default = "kw_time_date")]
    pub time_date: String,
    #[serde(default = "kw_packages")]
    pub packages: String,
}

fn kw_window_walker() -> String {
    "<".into()
}
fn kw_clipboard() -> String {
    "clip".into()
}
fn kw_bookmarks() -> String {
    "bm".into()
}
fn kw_time_date() -> String {
    ")".into()
}
fn kw_packages() -> String {
    "pkg".into()
}

impl Default for ProviderKeywords {
    fn default() -> Self {
        Self {
            window_walker: kw_window_walker(),
            clipboard_history: kw_clipboard(),
            bookmarks: kw_bookmarks(),
            time_date: kw_time_date(),
            packages: kw_packages(),
        }
    }
}

/// Whether providers also contribute results to plain (keyword-less) queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalResults {
    #[serde(default = "default_true")]
    pub window_walker: bool,
    #[serde(default = "default_true")]
    pub bookmarks: bool,
    /// Plain queries `time`, `date`, `now`, `today`, `week`.
    #[serde(default = "default_true")]
    pub time_date: bool,
}

impl Default for GlobalResults {
    fn default() -> Self {
        Self {
            window_walker: true,
            bookmarks: true,
            time_date: true,
        }
    }
}

fn default_clip_entries() -> usize {
    25
}
fn default_clip_poll() -> u64 {
    1000
}
fn default_clip_bytes() -> usize {
    64 * 1024
}

/// Clipboard history store settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardHistoryConfig {
    /// Number of text entries kept (newest first).
    #[serde(default = "default_clip_entries")]
    pub max_entries: usize,
    /// How often the daemon-side watcher checks the clipboard.
    #[serde(default = "default_clip_poll")]
    pub poll_interval_ms: u64,
    /// Larger clipboard texts are not stored.
    #[serde(default = "default_clip_bytes")]
    pub max_entry_bytes: usize,
}

impl Default for ClipboardHistoryConfig {
    fn default() -> Self {
        Self {
            max_entries: default_clip_entries(),
            poll_interval_ms: default_clip_poll(),
            max_entry_bytes: default_clip_bytes(),
        }
    }
}

/// A named URL or file path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub name: String,
    /// URL (`https://…`) or path (`~/Documents`, `/etc/hosts`).
    pub target: String,
}

fn default_pkg_manager() -> String {
    "auto".into()
}
fn default_pkg_min_len() -> usize {
    2
}

/// Package search settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSearchConfig {
    /// `auto`, `apt`, `dnf`, `pacman`, `zypper` or `none`.
    #[serde(default = "default_pkg_manager")]
    pub manager: String,
    /// Also search Flatpak remotes.
    #[serde(default = "default_true")]
    pub include_flatpak: bool,
    /// Shortest query that starts a search.
    #[serde(default = "default_pkg_min_len")]
    pub min_query_len: usize,
}

impl Default for PackageSearchConfig {
    fn default() -> Self {
        Self {
            manager: default_pkg_manager(),
            include_flatpak: true,
            min_query_len: default_pkg_min_len(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandPaletteConfig {
    #[serde(default = "default_max_results")]
    pub max_results: usize,

    #[serde(default = "default_search_engine")]
    pub search_engine: String,

    #[serde(default)]
    pub custom_search_url: Option<String>,

    #[serde(default)]
    pub providers: ProviderConfig,

    #[serde(default = "default_file_tool")]
    pub file_search_tool: String,

    #[serde(default = "default_true")]
    pub show_provider_tags: bool,

    #[serde(default)]
    pub keywords: ProviderKeywords,

    #[serde(default)]
    pub global_results: GlobalResults,

    #[serde(default)]
    pub clipboard_history: ClipboardHistoryConfig,

    #[serde(default)]
    pub bookmarks: Vec<Bookmark>,

    #[serde(default)]
    pub package_search: PackageSearchConfig,
}

impl Default for CommandPaletteConfig {
    fn default() -> Self {
        Self {
            max_results: default_max_results(),
            search_engine: default_search_engine(),
            custom_search_url: None,
            providers: ProviderConfig::default(),
            file_search_tool: default_file_tool(),
            show_provider_tags: true,
            keywords: ProviderKeywords::default(),
            global_results: GlobalResults::default(),
            clipboard_history: ClipboardHistoryConfig::default(),
            bookmarks: Vec::new(),
            package_search: PackageSearchConfig::default(),
        }
    }
}

// ── Module ──────────────────────────────────────────────────────────────────

pub struct CommandPalette {
    running: bool,
    config: CommandPaletteConfig,
    clip_stop: Arc<AtomicBool>,
    clip_thread: Option<JoinHandle<()>>,
}

impl CommandPalette {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("command-palette").unwrap_or_default();
        Self {
            running: false,
            config,
            clip_stop: Arc::new(AtomicBool::new(false)),
            clip_thread: None,
        }
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for CommandPalette {
    fn id(&self) -> &'static str {
        "command-palette"
    }

    fn name(&self) -> &'static str {
        "Command Palette"
    }

    fn description(&self) -> &'static str {
        "Spotlight-like search: apps, calculator, web search, system commands and more"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(Hotkey::new(vec![Modifier::Alt], "Space"))
    }

    fn start(&mut self) -> Result<()> {
        if let Ok(cfg) = mpt_common::config::load_module_config("command-palette") {
            self.config = cfg;
        }
        // The watcher re-reads the config and stays idle while clipboard
        // history is disabled, so toggling it needs no restart.
        self.clip_stop.store(false, Ordering::Relaxed);
        self.clip_thread = clipboard::spawn_watcher(Arc::clone(&self.clip_stop));
        self.running = true;
        info!(
            "Command Palette module started (clipboard history {})",
            if self.config.providers.clipboard_history {
                "on"
            } else {
                "off"
            }
        );
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.clip_stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.clip_thread.take() {
            let _ = handle.join();
        }
        self.running = false;
        info!("Command Palette module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        info!("Command Palette: launching GUI");

        let bin = find_gui_binary();
        let result = std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(child) => {
                info!("Command Palette GUI spawned (pid={})", child.id());
            }
            Err(e) => {
                warn!("Failed to spawn Command Palette GUI ({bin:?}): {e}");
            }
        }
        Ok(())
    }
}

fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-command-palette");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-command-palette".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_parses_with_new_defaults() {
        let cfg: CommandPaletteConfig =
            toml::from_str("max_results = 5\n[providers]\napps = false\n").unwrap();
        assert_eq!(cfg.max_results, 5);
        assert!(!cfg.providers.apps);
        assert!(cfg.providers.window_walker);
        assert!(!cfg.providers.clipboard_history);
        assert_eq!(cfg.keywords.window_walker, "<");
        assert_eq!(cfg.clipboard_history.poll_interval_ms, 1000);
        assert!(cfg.bookmarks.is_empty());
    }

    #[test]
    fn bookmarks_parse() {
        let cfg: CommandPaletteConfig =
            toml::from_str("[[bookmarks]]\nname = \"Docs\"\ntarget = \"~/Documents\"\n").unwrap();
        assert_eq!(
            cfg.bookmarks,
            vec![Bookmark {
                name: "Docs".into(),
                target: "~/Documents".into()
            }]
        );
    }
}
