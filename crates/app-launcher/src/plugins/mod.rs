//! PowerToys Run style plugin engine.
//!
//! A query is routed either to the plugins whose action keyword prefixes it
//! (longest keyword wins, e.g. `??` before `?`) or, when no keyword matches,
//! to every enabled plugin that has `include_in_global_result` set.

pub mod calculator;
pub mod folder;
pub mod history;
pub mod indexer;
pub mod program;
pub mod shell;
pub mod system;
pub mod time_date;
pub mod unit_converter;
pub mod uri;
pub mod value_generator;
pub mod web_search;
pub mod window_walker;

use crate::actions::Action;
use crate::config::{AppLauncherConfig, PluginId, ResolvedPlugin};
use crate::desktop::{self, DesktopEntry};
use history::HistoryStore;

/// One row in the result list.
#[derive(Debug, Clone)]
pub struct LauncherResult {
    pub plugin: PluginId,
    pub title: String,
    pub subtitle: String,
    /// Freedesktop icon name or absolute icon path.
    pub icon: Option<String>,
    pub score: f64,
    /// Run with Enter.
    pub action: Action,
    /// Run with Ctrl+Enter / Shift+Enter, with a short label for the footer.
    pub alt_action: Option<(String, Action)>,
    /// Stable identifier used for history/click boosting. Empty = not tracked.
    pub key: String,
}

impl LauncherResult {
    pub fn new(plugin: PluginId, title: impl Into<String>, action: Action) -> Self {
        Self {
            plugin,
            title: title.into(),
            subtitle: String::new(),
            icon: None,
            score: 0.0,
            action,
            alt_action: None,
            key: String::new(),
        }
    }

    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.subtitle = s.into();
        self
    }

    pub fn score(mut self, s: f64) -> Self {
        self.score = s;
        self
    }

    pub fn icon(mut self, icon: Option<String>) -> Self {
        self.icon = icon;
        self
    }

    pub fn alt(mut self, label: impl Into<String>, action: Action) -> Self {
        self.alt_action = Some((label.into(), action));
        self
    }

    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }
}

/// The plugins a query is dispatched to and the text each one receives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route<'a> {
    pub plugin: PluginId,
    pub search: &'a str,
    pub by_keyword: bool,
}

/// Route a raw query to plugins.
pub fn route<'a>(query: &'a str, plugins: &[ResolvedPlugin]) -> Vec<Route<'a>> {
    let q = query.trim_start();
    if q.trim().is_empty() {
        return Vec::new();
    }

    let best_len = plugins
        .iter()
        .filter(|p| p.enabled && !p.action_keyword.is_empty() && q.starts_with(&p.action_keyword))
        .map(|p| p.action_keyword.len())
        .max();

    if let Some(len) = best_len {
        return plugins
            .iter()
            .filter(|p| {
                p.enabled && p.action_keyword.len() == len && q.starts_with(&p.action_keyword)
            })
            .map(|p| Route {
                plugin: p.id,
                search: q[len..].trim(),
                by_keyword: true,
            })
            .collect();
    }

    plugins
        .iter()
        .filter(|p| p.enabled && p.include_in_global_result)
        .map(|p| Route {
            plugin: p.id,
            search: q.trim(),
            by_keyword: false,
        })
        .collect()
}

/// Context handed to each plugin.
pub struct QueryCtx<'a> {
    pub search: &'a str,
    pub by_keyword: bool,
    pub config: &'a AppLauncherConfig,
}

/// Owns plugin state (app index, history, X11 connection) and runs queries.
pub struct Engine {
    config: AppLauncherConfig,
    apps: Vec<DesktopEntry>,
    history: HistoryStore,
    windows: window_walker::WindowWalker,
    indexer: indexer::Indexer,
}

impl Engine {
    /// Create an engine without indexing applications yet.
    pub fn new(config: AppLauncherConfig) -> Self {
        let indexer = indexer::Indexer::new(&config.file_search_tool);
        Self {
            config,
            apps: Vec::new(),
            history: HistoryStore::load(),
            windows: window_walker::WindowWalker::default(),
            indexer,
        }
    }

    /// Scan `.desktop` files for the Program plugin.
    pub fn load_apps(&mut self) {
        self.apps = desktop::scan_desktop_entries();
        tracing::info!("App Launcher: indexed {} applications", self.apps.len());
    }

    pub fn set_apps(&mut self, apps: Vec<DesktopEntry>) {
        self.apps = apps;
    }

    pub fn set_history(&mut self, history: HistoryStore) {
        self.history = history;
    }

    pub fn config(&self) -> &AppLauncherConfig {
        &self.config
    }

    /// Run a query. With `include_slow == false` only fast plugins run.
    pub fn query(&mut self, query: &str, include_slow: bool) -> Vec<LauncherResult> {
        let plugins = self.config.resolved_plugins();
        let routes = route(query, &plugins);
        let mut results = Vec::new();

        for r in routes {
            if !include_slow && r.plugin.is_slow() {
                continue;
            }
            let ctx = QueryCtx {
                search: r.search,
                by_keyword: r.by_keyword,
                config: &self.config,
            };
            let mut out = match r.plugin {
                PluginId::Program => program::search(&ctx, &self.apps),
                PluginId::Calculator => calculator::search(&ctx),
                PluginId::Shell => shell::search(&ctx),
                PluginId::Folder => folder::search(&ctx),
                PluginId::Uri => uri::search(&ctx),
                PluginId::WebSearch => web_search::search(&ctx),
                PluginId::WindowWalker => self.windows.search(&ctx),
                PluginId::System => system::search(&ctx),
                PluginId::TimeDate => time_date::search(&ctx),
                PluginId::UnitConverter => unit_converter::search(&ctx),
                PluginId::ValueGenerator => value_generator::search(&ctx),
                PluginId::History => self.history.search(&ctx),
                PluginId::Indexer => self.indexer.search(&ctx),
            };
            results.append(&mut out);
        }

        let weight = self.config.search_clicked_item_weight;
        for r in &mut results {
            if !r.key.is_empty() {
                r.score += self.history.boost(&r.key) * weight;
            }
        }

        results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        results.truncate(self.config.max_results.max(1));
        results
    }
}

/// Case-insensitive match score shared by plugins: exact > prefix > word
/// prefix > substring > initials. Returns `None` when nothing matches.
pub fn text_score(haystack: &str, needle_lower: &str) -> Option<f64> {
    if needle_lower.is_empty() {
        return None;
    }
    let h = haystack.to_lowercase();
    if h == needle_lower {
        Some(100.0)
    } else if h.starts_with(needle_lower) {
        Some(80.0)
    } else if h
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(needle_lower))
    {
        Some(65.0)
    } else if h.contains(needle_lower) {
        Some(55.0)
    } else if needle_lower.len() >= 2 && initials(&h).starts_with(needle_lower) {
        Some(50.0)
    } else {
        None
    }
}

fn initials(s: &str) -> String {
    s.split(|c: char| !c.is_alphanumeric())
        .filter_map(|w| w.chars().next())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_plugins() -> Vec<ResolvedPlugin> {
        AppLauncherConfig::default().resolved_plugins()
    }

    #[test]
    fn longest_keyword_wins() {
        let p = cfg_plugins();
        let r = route("?? rust", &p);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].plugin, PluginId::WebSearch);
        assert_eq!(r[0].search, "rust");
        let r = route("?readme", &p);
        assert_eq!(r[0].plugin, PluginId::Indexer);
        assert_eq!(r[0].search, "readme");
    }

    #[test]
    fn keyword_without_space() {
        let p = cfg_plugins();
        let r = route(">ls -la", &p);
        assert_eq!(r[0].plugin, PluginId::Shell);
        assert_eq!(r[0].search, "ls -la");
        assert!(r[0].by_keyword);
    }

    #[test]
    fn global_query_hits_global_plugins_only() {
        let p = cfg_plugins();
        let r = route("firefox", &p);
        let ids: Vec<_> = r.iter().map(|r| r.plugin).collect();
        assert!(ids.contains(&PluginId::Program));
        assert!(ids.contains(&PluginId::System));
        assert!(!ids.contains(&PluginId::Shell));
        assert!(!ids.contains(&PluginId::UnitConverter));
        assert!(r.iter().all(|r| !r.by_keyword));
    }

    #[test]
    fn disabled_plugin_keyword_is_ignored() {
        let mut cfg = AppLauncherConfig::default();
        cfg.plugins.get_mut("shell").unwrap().enabled = Some(false);
        let r = route(">ls", &cfg.resolved_plugins());
        assert!(r.iter().all(|r| r.plugin != PluginId::Shell));
        assert!(r.iter().all(|r| !r.by_keyword));
    }

    #[test]
    fn empty_query_routes_nowhere() {
        assert!(route("   ", &cfg_plugins()).is_empty());
    }

    #[test]
    fn text_score_orders() {
        assert_eq!(text_score("Firefox", "firefox"), Some(100.0));
        assert_eq!(text_score("Firefox", "fire"), Some(80.0));
        assert_eq!(text_score("GNU Image Manipulation", "image"), Some(65.0));
        assert_eq!(text_score("Visual Studio Code", "vsc"), Some(50.0));
        assert_eq!(text_score("Firefox", "chrome"), None);
    }

    #[test]
    fn engine_ranks_and_truncates() {
        let mut engine = Engine::new(AppLauncherConfig::default());
        engine.set_history(HistoryStore::default());
        let res = engine.query("=2+3", false);
        assert_eq!(res[0].plugin, PluginId::Calculator);
        assert_eq!(res[0].title, "5");
    }
}
