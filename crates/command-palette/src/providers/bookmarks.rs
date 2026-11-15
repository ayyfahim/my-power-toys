//! User-defined bookmarks (name → URL or path) from the `bookmarks` list.

use super::{
    PaletteResult, Provider, QueryContext, ResultAction, ResultIcon, strip_keyword, text_score,
};
use crate::Bookmark;

pub struct BookmarksProvider {
    keyword: String,
    global: bool,
    bookmarks: Vec<Bookmark>,
}

impl BookmarksProvider {
    pub fn new(keyword: String, global: bool, bookmarks: Vec<Bookmark>) -> Self {
        Self {
            keyword,
            global,
            bookmarks,
        }
    }
}

pub fn is_url(target: &str) -> bool {
    let t = target.trim();
    t.split_once("://")
        .is_some_and(|(scheme, _)| !scheme.is_empty() && scheme.chars().all(char::is_alphanumeric))
        || t.starts_with("mailto:")
}

/// Expand `~/` for path targets; URLs are returned unchanged.
pub fn resolve_target(target: &str) -> String {
    let t = target.trim();
    if !is_url(t)
        && let Some(rest) = t.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).to_string_lossy().to_string();
    }
    if t == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home.to_string_lossy().to_string();
    }
    t.to_string()
}

impl Provider for BookmarksProvider {
    fn tag(&self) -> &'static str {
        "bookmark"
    }

    fn matches(&self, raw_query: &str) -> bool {
        !self.bookmarks.is_empty()
            && (self.keyword_match(raw_query) || (self.global && !raw_query.trim().is_empty()))
    }

    fn keyword_match(&self, raw_query: &str) -> bool {
        !self.bookmarks.is_empty() && strip_keyword(raw_query, &self.keyword).is_some()
    }

    fn strip_prefix<'a>(&self, raw_query: &'a str) -> &'a str {
        strip_keyword(raw_query, &self.keyword).unwrap_or(raw_query.trim())
    }

    fn search(&self, ctx: &QueryContext) -> Vec<PaletteResult> {
        let by_keyword = self.keyword_match(ctx.raw_query);
        let query = ctx.stripped_query.trim().to_lowercase();

        let mut scored: Vec<(f64, &Bookmark)> = self
            .bookmarks
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                if query.is_empty() {
                    return by_keyword.then_some((150.0 - i as f64 * 0.01, b));
                }
                let s = text_score(&b.name, &query)
                    .or_else(|| text_score(&b.target, &query).map(|s| s - 20.0))?;
                Some((if by_keyword { s + 100.0 } else { s - 5.0 }, b))
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        scored
            .into_iter()
            .take(ctx.max_results)
            .map(|(score, b)| {
                let url = is_url(&b.target);
                PaletteResult {
                    id: b.name.clone(),
                    title: b.name.clone(),
                    subtitle: Some(b.target.clone()),
                    icon: if url {
                        ResultIcon::BuiltinWeb
                    } else {
                        ResultIcon::BuiltinFile
                    },
                    action: ResultAction::OpenUrl(resolve_target(&b.target)),
                    score,
                    provider_tag: "bookmark",
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(global: bool) -> BookmarksProvider {
        BookmarksProvider::new(
            "bm".into(),
            global,
            vec![
                Bookmark {
                    name: "Rust docs".into(),
                    target: "https://doc.rust-lang.org".into(),
                },
                Bookmark {
                    name: "Projects".into(),
                    target: "/srv/projects".into(),
                },
            ],
        )
    }

    #[test]
    fn url_detection() {
        assert!(is_url("https://example.com"));
        assert!(is_url("mailto:a@b.c"));
        assert!(!is_url("~/Documents"));
        assert!(!is_url("/etc/hosts"));
    }

    #[test]
    fn keyword_lists_all_and_filters() {
        let p = provider(false);
        let all = p.search(&QueryContext {
            raw_query: "bm",
            stripped_query: "",
            max_results: 8,
        });
        assert_eq!(all.len(), 2);
        let r = p.search(&QueryContext {
            raw_query: "bm rust",
            stripped_query: "rust",
            max_results: 8,
        });
        assert_eq!(r.len(), 1);
        assert!(
            matches!(&r[0].action, ResultAction::OpenUrl(u) if u == "https://doc.rust-lang.org")
        );
    }

    #[test]
    fn global_toggle() {
        assert!(!provider(false).matches("proj"));
        assert!(provider(true).matches("proj"));
        assert!(provider(false).matches("bm proj"));
    }
}
