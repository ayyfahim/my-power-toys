//! Web search plugin (`??`). In global results it appears as a low-ranked
//! fallback, like upstream.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::config::PluginId;
use crate::plugins::value_generator::url_encode;

pub fn engine_name(engine: &str) -> &'static str {
    match engine {
        "bing" => "Bing",
        "duckduckgo" => "DuckDuckGo",
        "brave" => "Brave Search",
        "startpage" => "Startpage",
        "ecosia" => "Ecosia",
        "custom" => "custom engine",
        _ => "Google",
    }
}

/// Build the search URL for `query` with the given engine.
pub fn search_url(engine: &str, custom: Option<&str>, query: &str) -> String {
    let q = url_encode(query);
    match engine {
        "bing" => format!("https://www.bing.com/search?q={q}"),
        "duckduckgo" => format!("https://duckduckgo.com/?q={q}"),
        "brave" => format!("https://search.brave.com/search?q={q}"),
        "startpage" => format!("https://www.startpage.com/do/search?q={q}"),
        "ecosia" => format!("https://www.ecosia.org/search?q={q}"),
        "custom" if custom.is_some_and(|c| c.contains("{query}")) => {
            custom.unwrap_or_default().replace("{query}", &q)
        }
        _ => format!("https://www.google.com/search?q={q}"),
    }
}

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let q = ctx.search.trim();
    let engine = ctx.config.web_search_engine.as_str();
    let name = engine_name(engine);
    if q.is_empty() {
        if ctx.by_keyword {
            return vec![
                LauncherResult::new(PluginId::WebSearch, "Type to search the web", Action::None)
                    .subtitle(format!("Searches with {name}")),
            ];
        }
        return Vec::new();
    }
    let url = search_url(engine, ctx.config.custom_search_url.as_deref(), q);
    let score = if ctx.by_keyword { 300.0 } else { 0.5 };
    vec![
        LauncherResult::new(
            PluginId::WebSearch,
            format!("Search the web for \"{q}\""),
            Action::Open(url.clone()),
        )
        .subtitle(format!("{name} · Ctrl+Enter copies the URL"))
        .icon(Some("web-browser".into()))
        .score(score)
        .alt("Copy URL", Action::Copy(url)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_urls() {
        assert_eq!(
            search_url("google", None, "rust lang"),
            "https://www.google.com/search?q=rust%20lang"
        );
        assert_eq!(
            search_url("duckduckgo", None, "a&b"),
            "https://duckduckgo.com/?q=a%26b"
        );
        assert_eq!(
            search_url("custom", Some("https://x.org/?s={query}"), "hi"),
            "https://x.org/?s=hi"
        );
        // custom without template falls back to Google
        assert!(search_url("custom", None, "x").starts_with("https://www.google.com"));
    }
}
