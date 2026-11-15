//! History plugin (`!!`) and the persistent launch history used for
//! click boosting (upstream `SearchClickedItemWeight`) and for restoring the
//! previous query when `clear_input_on_launch` is off.
//!
//! Stored in `~/.local/share/my-power-toys/app-launcher-history.json`.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::config::PluginId;
use crate::frecency::FrecencyStore;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryItem {
    pub query: String,
    pub title: String,
    #[serde(default)]
    pub plugin: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HistoryStore {
    /// Most recent first.
    #[serde(default)]
    pub items: Vec<HistoryItem>,
    #[serde(default)]
    pub clicks: FrecencyStore,
    #[serde(default)]
    pub last_query: String,
}

pub fn history_path() -> Option<PathBuf> {
    Some(
        dirs::data_dir()?
            .join("my-power-toys")
            .join("app-launcher-history.json"),
    )
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl HistoryStore {
    pub fn load() -> Self {
        history_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        let path = history_path().ok_or_else(|| anyhow::anyhow!("no data directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    /// Record a launched result for `query`, keeping at most `max` items.
    pub fn record(&mut self, query: &str, result: &LauncherResult, max: usize) {
        let query = query.trim();
        if !result.key.is_empty() {
            self.clicks.record_launch(&result.key);
        }
        if query.is_empty() || result.plugin == PluginId::History {
            return;
        }
        self.items
            .retain(|i| !(i.query == query && i.title == result.title));
        self.items.insert(
            0,
            HistoryItem {
                query: query.to_string(),
                title: result.title.clone(),
                plugin: result.plugin.key().to_string(),
                timestamp: now(),
            },
        );
        self.items.truncate(max);
    }

    /// Click boost for a result key (0 if never chosen).
    pub fn boost(&self, key: &str) -> f64 {
        self.clicks.score(key)
    }

    pub fn search(&self, ctx: &QueryCtx) -> Vec<LauncherResult> {
        let q = ctx.search.trim().to_lowercase();
        let now = now();
        self.items
            .iter()
            .filter(|i| {
                q.is_empty()
                    || i.query.to_lowercase().contains(&q)
                    || i.title.to_lowercase().contains(&q)
            })
            .enumerate()
            .map(|(n, i)| {
                LauncherResult::new(
                    PluginId::History,
                    i.query.clone(),
                    Action::SetQuery(i.query.clone()),
                )
                .subtitle(format!(
                    "{} · {}",
                    i.title,
                    ago(now.saturating_sub(i.timestamp))
                ))
                .icon(Some("document-open-recent".into()))
                .score(300.0 - n as f64 * 0.1)
            })
            .collect()
    }
}

fn ago(secs: u64) -> String {
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86400 => format!("{} h ago", secs / 3600),
        _ => format!("{} days ago", secs / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppLauncherConfig;

    fn res(title: &str) -> LauncherResult {
        LauncherResult::new(PluginId::Program, title, Action::None).key(format!("program:{title}"))
    }

    #[test]
    fn records_dedupes_and_caps() {
        let mut h = HistoryStore::default();
        h.record("fire", &res("Firefox"), 2);
        h.record("code", &res("Code"), 2);
        h.record("fire", &res("Firefox"), 2);
        assert_eq!(h.items.len(), 2);
        assert_eq!(h.items[0].query, "fire");
        h.record("term", &res("Terminal"), 2);
        assert_eq!(h.items.len(), 2);
        assert!(h.boost("program:Firefox") > h.boost("program:Code"));
        assert_eq!(h.boost("program:Nope"), 0.0);
    }

    #[test]
    fn search_filters_and_sets_query() {
        let mut h = HistoryStore::default();
        h.record("fire", &res("Firefox"), 10);
        h.record("code", &res("Code"), 10);
        let cfg = AppLauncherConfig::default();
        let r = h.search(&QueryCtx {
            search: "fox",
            by_keyword: true,
            config: &cfg,
        });
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].action, Action::SetQuery("fire".into()));
    }

    #[test]
    fn json_roundtrip() {
        let mut h = HistoryStore::default();
        h.record("q", &res("T"), 5);
        h.last_query = "abc".into();
        let s = serde_json::to_string(&h).unwrap();
        let p: HistoryStore = serde_json::from_str(&s).unwrap();
        assert_eq!(p.items, h.items);
        assert_eq!(p.last_query, "abc");
    }

    #[test]
    fn ago_formats() {
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(120), "2 min ago");
        assert_eq!(ago(7200), "2 h ago");
    }
}
