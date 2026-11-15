//! Window walker: switch to an open window (upstream CmdPal Window Walker).

use super::{
    PaletteResult, Provider, QueryContext, ResultAction, ResultIcon, strip_keyword, text_score,
};
use crate::x11::{self, WindowInfo};

pub struct WindowsProvider {
    keyword: String,
    global: bool,
    windows: Vec<WindowInfo>,
}

impl WindowsProvider {
    pub fn new(keyword: String, global: bool) -> Self {
        Self {
            keyword,
            global,
            windows: Vec::new(),
        }
    }

    #[cfg(test)]
    fn with_windows(keyword: &str, global: bool, windows: Vec<WindowInfo>) -> Self {
        Self {
            keyword: keyword.into(),
            global,
            windows,
        }
    }
}

fn window_score(w: &WindowInfo, query: &str) -> Option<f64> {
    let title = text_score(&w.title, query);
    let class = text_score(&w.class, query).map(|s| s - 10.0);
    let process = w
        .process
        .as_deref()
        .and_then(|p| text_score(p, query))
        .map(|s| s - 15.0);
    [title, class, process]
        .into_iter()
        .flatten()
        .fold(None, |best, s| Some(best.map_or(s, |b: f64| b.max(s))))
}

impl Provider for WindowsProvider {
    fn tag(&self) -> &'static str {
        "window"
    }

    fn matches(&self, raw_query: &str) -> bool {
        self.keyword_match(raw_query) || (self.global && !raw_query.trim().is_empty())
    }

    fn keyword_match(&self, raw_query: &str) -> bool {
        strip_keyword(raw_query, &self.keyword).is_some()
    }

    fn strip_prefix<'a>(&self, raw_query: &'a str) -> &'a str {
        strip_keyword(raw_query, &self.keyword).unwrap_or(raw_query.trim())
    }

    fn search(&self, ctx: &QueryContext) -> Vec<PaletteResult> {
        let by_keyword = self.keyword_match(ctx.raw_query);
        let query = ctx.stripped_query.trim().to_lowercase();

        let mut scored: Vec<(f64, &WindowInfo)> = if query.is_empty() {
            if !by_keyword {
                return Vec::new();
            }
            // Keyword alone: list every window in stacking order.
            self.windows
                .iter()
                .enumerate()
                .map(|(i, w)| (150.0 - i as f64 * 0.01, w))
                .collect()
        } else {
            self.windows
                .iter()
                .filter_map(|w| window_score(w, &query).map(|s| (s, w)))
                // Keyword queries rank above everything; global results sit
                // just below app matches.
                .map(|(s, w)| (if by_keyword { s + 100.0 } else { s - 20.0 }, w))
                .collect()
        };

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored
            .into_iter()
            .take(ctx.max_results)
            .map(|(score, w)| {
                let mut subtitle = w.class.clone();
                if let Some(p) = &w.process
                    && !p.eq_ignore_ascii_case(&w.class)
                {
                    subtitle.push_str(&format!(" · {p}"));
                }
                if let Some(d) = w.desktop
                    && d != 0xFFFF_FFFF
                {
                    subtitle.push_str(&format!(" · desktop {}", d + 1));
                }
                PaletteResult {
                    id: format!("{}", w.id),
                    title: if w.title.is_empty() {
                        w.class.clone()
                    } else {
                        w.title.clone()
                    },
                    subtitle: Some(format!("Switch to window — {subtitle}")),
                    icon: ResultIcon::Named(w.class.to_lowercase()),
                    action: ResultAction::FocusWindow(w.id),
                    score,
                    provider_tag: "window",
                }
            })
            .collect()
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        if std::env::var_os("DISPLAY").is_none() {
            anyhow::bail!("window walker needs X11 (DISPLAY not set)");
        }
        self.windows = x11::list_windows(std::process::id())?;
        tracing::info!("Command Palette: {} open window(s)", self.windows.len());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u32, title: &str, class: &str) -> WindowInfo {
        WindowInfo {
            id,
            title: title.into(),
            class: class.into(),
            pid: None,
            process: None,
            desktop: Some(0),
        }
    }

    fn ctx<'a>(raw: &'a str, stripped: &'a str) -> QueryContext<'a> {
        QueryContext {
            raw_query: raw,
            stripped_query: stripped,
            max_results: 8,
        }
    }

    #[test]
    fn keyword_lists_all() {
        let p = WindowsProvider::with_windows(
            "<",
            true,
            vec![win(1, "Mail", "thunderbird"), win(2, "Docs", "firefox")],
        );
        assert!(p.keyword_match("<"));
        let r = p.search(&ctx("<", p.strip_prefix("<")));
        assert_eq!(r.len(), 2);
        assert!(matches!(r[0].action, ResultAction::FocusWindow(1)));
    }

    #[test]
    fn global_match_by_class() {
        let p = WindowsProvider::with_windows(
            "<",
            true,
            vec![win(1, "Mail", "thunderbird"), win(2, "Docs", "firefox")],
        );
        let r = p.search(&ctx("fire", "fire"));
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Docs");
        assert!(p.search(&ctx("", "")).is_empty());
    }

    #[test]
    fn global_disabled() {
        let p = WindowsProvider::with_windows("<", false, vec![win(1, "Mail", "thunderbird")]);
        assert!(!p.matches("mail"));
        assert!(p.matches("<mail"));
    }
}
