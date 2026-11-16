//! Clipboard history provider (upstream CmdPal Clipboard History). Reads the
//! store written by the daemon-side watcher in [`crate::clipboard`].

use std::time::{SystemTime, UNIX_EPOCH};

use super::{
    PaletteResult, Provider, QueryContext, ResultAction, ResultIcon, strip_keyword, truncate_chars,
};
use crate::clipboard::{ClipEntry, ClipboardStore};

pub struct ClipboardProvider {
    keyword: String,
    entries: Vec<ClipEntry>,
}

impl ClipboardProvider {
    pub fn new(keyword: String) -> Self {
        Self {
            keyword,
            entries: Vec::new(),
        }
    }
}

/// "3 min ago" style age.
pub fn relative_age(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86_399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86_400),
    }
}

/// One-line preview of a clipboard entry.
pub fn preview(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or_default();
    let mut line = truncate_chars(first.trim(), 80);
    if text.trim().lines().nth(1).is_some() {
        line.push_str(" ⏎…");
    }
    line
}

impl Provider for ClipboardProvider {
    fn tag(&self) -> &'static str {
        "clipboard"
    }

    fn matches(&self, raw_query: &str) -> bool {
        self.keyword_match(raw_query)
    }

    fn keyword_match(&self, raw_query: &str) -> bool {
        strip_keyword(raw_query, &self.keyword).is_some()
    }

    fn strip_prefix<'a>(&self, raw_query: &'a str) -> &'a str {
        strip_keyword(raw_query, &self.keyword).unwrap_or_default()
    }

    fn search(&self, ctx: &QueryContext) -> Vec<PaletteResult> {
        let query = ctx.stripped_query.trim().to_lowercase();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let mut out: Vec<PaletteResult> = self
            .entries
            .iter()
            .filter(|e| query.is_empty() || e.text.to_lowercase().contains(&query))
            .take(ctx.max_results)
            .enumerate()
            .map(|(i, e)| PaletteResult {
                id: format!("clip:{i}"),
                title: preview(&e.text),
                subtitle: Some(format!(
                    "Copy · {} · {} chars",
                    relative_age(e.copied_at, now),
                    e.text.chars().count()
                )),
                icon: ResultIcon::Emoji('📋'),
                action: ResultAction::CopyToClipboard(e.text.clone()),
                // Keep newest-first order and stay above other providers.
                score: 300.0 - i as f64,
                provider_tag: "clipboard",
            })
            .collect();

        if self.entries.is_empty() && query.is_empty() {
            out.push(PaletteResult {
                id: "clip:empty".into(),
                title: "Clipboard history is empty".into(),
                subtitle: Some("Copied text appears here while the daemon is running".into()),
                icon: ResultIcon::Emoji('📋'),
                action: ResultAction::None,
                score: 0.0,
                provider_tag: "clipboard",
            });
        } else if query.is_empty() || "clear".starts_with(&query) {
            out.push(PaletteResult {
                id: "clip:clear".into(),
                title: "Clear clipboard history".into(),
                subtitle: Some(format!("Delete {} stored entries", self.entries.len())),
                icon: ResultIcon::BuiltinSystem,
                action: ResultAction::ClearClipboardHistory,
                score: if query.is_empty() { 1.0 } else { 400.0 },
                provider_tag: "clipboard",
            });
        }
        out
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        self.entries = ClipboardStore::load().entries;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages() {
        assert_eq!(relative_age(100, 110), "just now");
        assert_eq!(relative_age(0, 180), "3 min ago");
        assert_eq!(relative_age(0, 7200), "2 h ago");
        assert_eq!(relative_age(0, 200_000), "2 d ago");
        assert_eq!(relative_age(10, 0), "just now");
    }

    #[test]
    fn previews() {
        assert_eq!(preview("  hello  "), "hello");
        assert_eq!(preview("line1\nline2"), "line1 ⏎…");
    }

    #[test]
    fn filters_entries() {
        let mut p = ClipboardProvider::new("clip".into());
        p.entries = vec![
            ClipEntry {
                text: "alpha".into(),
                copied_at: 0,
            },
            ClipEntry {
                text: "beta".into(),
                copied_at: 0,
            },
        ];
        let ctx = QueryContext {
            raw_query: "clip be",
            stripped_query: "be",
            max_results: 8,
        };
        let r = p.search(&ctx);
        assert_eq!(r.len(), 1);
        assert!(matches!(&r[0].action, ResultAction::CopyToClipboard(t) if t == "beta"));
    }
}
