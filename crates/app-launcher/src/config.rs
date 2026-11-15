//! Launcher configuration (`~/.config/my-power-toys/app-launcher.toml`).
//!
//! Mirrors upstream `PowerLauncherProperties` plus per-plugin settings
//! (enabled / action keyword / include in global result).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Identifier of a launcher plugin. The string form is the key in the
/// `[plugins.<id>]` config table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PluginId {
    Program,
    Calculator,
    Shell,
    Folder,
    Uri,
    WebSearch,
    WindowWalker,
    System,
    TimeDate,
    UnitConverter,
    ValueGenerator,
    History,
    Indexer,
}

/// Built-in defaults for a plugin, taken from upstream `plugin.json` files.
#[derive(Debug, Clone, Copy)]
pub struct PluginDefaults {
    pub action_keyword: &'static str,
    pub include_in_global_result: bool,
    pub enabled: bool,
}

impl PluginId {
    pub const ALL: [PluginId; 13] = [
        PluginId::Program,
        PluginId::Calculator,
        PluginId::Shell,
        PluginId::Folder,
        PluginId::Uri,
        PluginId::WebSearch,
        PluginId::WindowWalker,
        PluginId::System,
        PluginId::TimeDate,
        PluginId::UnitConverter,
        PluginId::ValueGenerator,
        PluginId::History,
        PluginId::Indexer,
    ];

    pub fn key(self) -> &'static str {
        match self {
            PluginId::Program => "program",
            PluginId::Calculator => "calculator",
            PluginId::Shell => "shell",
            PluginId::Folder => "folder",
            PluginId::Uri => "uri",
            PluginId::WebSearch => "web-search",
            PluginId::WindowWalker => "window-walker",
            PluginId::System => "system",
            PluginId::TimeDate => "time-date",
            PluginId::UnitConverter => "unit-converter",
            PluginId::ValueGenerator => "value-generator",
            PluginId::History => "history",
            PluginId::Indexer => "indexer",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            PluginId::Program => "Program",
            PluginId::Calculator => "Calculator",
            PluginId::Shell => "Shell",
            PluginId::Folder => "Folder",
            PluginId::Uri => "URI Handler",
            PluginId::WebSearch => "Web Search",
            PluginId::WindowWalker => "Window Walker",
            PluginId::System => "System Commands",
            PluginId::TimeDate => "Time and Date",
            PluginId::UnitConverter => "Unit Converter",
            PluginId::ValueGenerator => "Value Generator",
            PluginId::History => "History",
            PluginId::Indexer => "File Search",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Slow plugins (external processes, X11 round trips) run only after the
    /// full search delay; the others run after the fast delay.
    pub fn is_slow(self) -> bool {
        matches!(
            self,
            PluginId::Indexer | PluginId::WindowWalker | PluginId::Folder
        )
    }

    pub fn defaults(self) -> PluginDefaults {
        let d = |kw: &'static str, global: bool| PluginDefaults {
            action_keyword: kw,
            include_in_global_result: global,
            enabled: true,
        };
        match self {
            PluginId::Program => d(".", true),
            PluginId::Calculator => d("=", true),
            PluginId::Shell => d(">", false),
            // Folder has no keyword upstream; paths starting with `/` or `~`
            // are recognised in global queries.
            PluginId::Folder => d("", true),
            PluginId::Uri => d("//", true),
            PluginId::WebSearch => d("??", true),
            PluginId::WindowWalker => d("<", true),
            PluginId::System => d("", true),
            PluginId::TimeDate => d(")", true),
            PluginId::UnitConverter => d("%%", false),
            PluginId::ValueGenerator => d("#", false),
            PluginId::History => d("!!", false),
            // Upstream Indexer is global, but spawning locate/fd on every
            // keystroke is expensive, so only the `?` keyword is on by default.
            PluginId::Indexer => d("?", false),
        }
    }
}

/// Per-plugin overrides. Missing fields fall back to [`PluginId::defaults`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_keyword: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_in_global_result: Option<bool>,
}

/// Fully resolved plugin settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPlugin {
    pub id: PluginId,
    pub enabled: bool,
    pub action_keyword: String,
    pub include_in_global_result: bool,
}

/// Where the launcher window appears (upstream `Position`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LauncherPosition {
    /// Monitor under the mouse cursor (upstream default).
    #[default]
    Cursor,
    /// Primary monitor.
    Primary,
    /// Monitor containing the focused window.
    Focused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLauncherConfig {
    /// Upstream `MaximumNumberOfResults`.
    #[serde(default = "default_max_results")]
    pub max_results: usize,

    /// Legacy switch kept for compatibility; the calculator runs only if both
    /// this and `plugins.calculator.enabled` are true.
    #[serde(default = "default_true")]
    pub show_calculator: bool,

    /// Upstream `ClearInputOnLaunch`: start with an empty search box instead
    /// of the previous query.
    #[serde(default)]
    pub clear_input_on_launch: bool,

    /// Upstream `SearchQueryResultsWithDelay`.
    #[serde(default = "default_true")]
    pub search_query_results_with_delay: bool,

    /// Upstream `SearchInputDelayFast` (ms) for fast plugins.
    #[serde(default = "default_fast_delay")]
    pub search_input_delay_fast_ms: u64,

    /// Upstream `SearchInputDelay` (ms) for slow plugins (files, windows).
    #[serde(default = "default_delay")]
    pub search_input_delay_ms: u64,

    /// Upstream `Position`.
    #[serde(default)]
    pub position: LauncherPosition,

    /// Upstream `SearchClickedItemWeight`: boost for previously chosen results.
    #[serde(default = "default_clicked_weight")]
    pub search_clicked_item_weight: f64,

    /// Number of entries kept by the History plugin.
    #[serde(default = "default_history_size")]
    pub history_size: usize,

    /// Web search engine: google, bing, duckduckgo, brave, startpage, ecosia, custom.
    #[serde(default = "default_search_engine")]
    pub web_search_engine: String,

    /// Template for `custom` engine, `{query}` is replaced with the encoded query.
    #[serde(default)]
    pub custom_search_url: Option<String>,

    /// Shell plugin: Enter runs in a terminal (true) or detached (false).
    /// Ctrl/Shift+Enter does the other.
    #[serde(default = "default_true")]
    pub shell_run_in_terminal: bool,

    /// Shell plugin (upstream `LeaveShellOpen`): keep terminal open after the command.
    #[serde(default = "default_true")]
    pub shell_keep_open: bool,

    /// Terminal emulator to use. Empty = auto-detect ($TERMINAL, x-terminal-emulator, ...).
    #[serde(default)]
    pub terminal: String,

    /// File search tool: auto, plocate, locate, fd.
    #[serde(default = "default_file_tool")]
    pub file_search_tool: String,

    /// Window Walker: include windows on other virtual desktops.
    #[serde(default = "default_true")]
    pub window_walker_all_desktops: bool,

    /// Per-plugin settings keyed by plugin id (`[plugins.shell]` etc.).
    #[serde(default = "default_plugins")]
    pub plugins: BTreeMap<String, PluginSettings>,
}

fn default_max_results() -> usize {
    8
}

fn default_true() -> bool {
    true
}

fn default_fast_delay() -> u64 {
    50
}

fn default_delay() -> u64 {
    150
}

fn default_clicked_weight() -> f64 {
    5.0
}

fn default_history_size() -> usize {
    50
}

fn default_search_engine() -> String {
    "google".into()
}

fn default_file_tool() -> String {
    "auto".into()
}

fn default_plugins() -> BTreeMap<String, PluginSettings> {
    PluginId::ALL
        .into_iter()
        .map(|id| {
            let d = id.defaults();
            (
                id.key().to_string(),
                PluginSettings {
                    enabled: Some(d.enabled),
                    action_keyword: Some(d.action_keyword.to_string()),
                    include_in_global_result: Some(d.include_in_global_result),
                },
            )
        })
        .collect()
}

impl Default for AppLauncherConfig {
    fn default() -> Self {
        Self {
            max_results: default_max_results(),
            show_calculator: default_true(),
            clear_input_on_launch: false,
            search_query_results_with_delay: true,
            search_input_delay_fast_ms: default_fast_delay(),
            search_input_delay_ms: default_delay(),
            position: LauncherPosition::default(),
            search_clicked_item_weight: default_clicked_weight(),
            history_size: default_history_size(),
            web_search_engine: default_search_engine(),
            custom_search_url: None,
            shell_run_in_terminal: true,
            shell_keep_open: true,
            terminal: String::new(),
            file_search_tool: default_file_tool(),
            window_walker_all_desktops: true,
            plugins: default_plugins(),
        }
    }
}

impl AppLauncherConfig {
    /// Resolve settings for one plugin, applying built-in defaults.
    pub fn plugin(&self, id: PluginId) -> ResolvedPlugin {
        let d = id.defaults();
        let s = self.plugins.get(id.key());
        let mut enabled = s.and_then(|s| s.enabled).unwrap_or(d.enabled);
        if id == PluginId::Calculator && !self.show_calculator {
            enabled = false;
        }
        ResolvedPlugin {
            id,
            enabled,
            action_keyword: s
                .and_then(|s| s.action_keyword.clone())
                .unwrap_or_else(|| d.action_keyword.to_string())
                .trim()
                .to_string(),
            include_in_global_result: s
                .and_then(|s| s.include_in_global_result)
                .unwrap_or(d.include_in_global_result),
        }
    }

    /// All plugins, resolved.
    pub fn resolved_plugins(&self) -> Vec<ResolvedPlugin> {
        PluginId::ALL
            .into_iter()
            .map(|id| self.plugin(id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_keys_roundtrip() {
        for id in PluginId::ALL {
            assert_eq!(PluginId::from_key(id.key()), Some(id));
        }
    }

    #[test]
    fn partial_plugin_table_uses_defaults() {
        let cfg: AppLauncherConfig = toml::from_str(
            r#"
            [plugins.shell]
            action_keyword = "$"
            [plugins.uri]
            enabled = false
            "#,
        )
        .unwrap();
        let shell = cfg.plugin(PluginId::Shell);
        assert_eq!(shell.action_keyword, "$");
        assert!(shell.enabled);
        assert!(!shell.include_in_global_result);
        assert!(!cfg.plugin(PluginId::Uri).enabled);
        // Plugins absent from a user-provided table still get defaults.
        assert_eq!(cfg.plugin(PluginId::WebSearch).action_keyword, "??");
    }

    #[test]
    fn full_config_roundtrip() {
        let mut cfg = AppLauncherConfig {
            position: LauncherPosition::Focused,
            ..Default::default()
        };
        cfg.plugins.insert(
            "history".into(),
            PluginSettings {
                enabled: Some(false),
                ..Default::default()
            },
        );
        let s = toml::to_string_pretty(&cfg).unwrap();
        let parsed: AppLauncherConfig = toml::from_str(&s).unwrap();
        assert_eq!(parsed.position, LauncherPosition::Focused);
        assert!(!parsed.plugin(PluginId::History).enabled);
        assert_eq!(parsed.plugin(PluginId::History).action_keyword, "!!");
        assert_eq!(parsed.search_input_delay_ms, 150);
    }

    #[test]
    fn show_calculator_false_disables_plugin() {
        let cfg = AppLauncherConfig {
            show_calculator: false,
            ..Default::default()
        };
        assert!(!cfg.plugin(PluginId::Calculator).enabled);
    }
}
