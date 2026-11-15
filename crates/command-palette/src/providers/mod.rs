pub mod apps;
pub mod bookmarks;
pub mod calculator;
pub mod clipboard;
pub mod files;
pub mod packages;
pub mod settings;
pub mod shell;
pub mod system;
pub mod time_date;
pub mod web_search;
pub mod windows;

/// A result from any provider.
#[derive(Debug, Clone)]
pub struct PaletteResult {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub icon: ResultIcon,
    pub action: ResultAction,
    pub score: f64,
    pub provider_tag: &'static str,
}

#[derive(Debug, Clone)]
pub enum ResultIcon {
    Named(String),
    Emoji(char),
    BuiltinApp,
    BuiltinCalc,
    BuiltinWeb,
    BuiltinSystem,
    BuiltinFile,
    BuiltinTerminal,
    BuiltinSettings,
}

#[derive(Debug, Clone)]
pub enum ResultAction {
    LaunchExec(String),
    CopyToClipboard(String),
    OpenUrl(String),
    RunShell(String),
    SystemCommand(SystemCmd),
    OpenSettings(String),
    /// Activate an X11 window (window walker).
    FocusWindow(u32),
    /// Delete the stored clipboard history.
    ClearClipboardHistory,
    /// Nothing to do (hints / placeholders).
    None,
}

#[derive(Debug, Clone)]
pub enum SystemCmd {
    Lock,
    Logout,
    Shutdown,
    Reboot,
    Suspend,
    Hibernate,
}

/// Context passed to providers on each query.
pub struct QueryContext<'a> {
    pub raw_query: &'a str,
    pub stripped_query: &'a str,
    pub max_results: usize,
}

/// Every provider implements this trait.
pub trait Provider: Send {
    fn tag(&self) -> &'static str;
    fn matches(&self, raw_query: &str) -> bool;
    fn strip_prefix<'a>(&self, raw_query: &'a str) -> &'a str;
    fn search(&self, ctx: &QueryContext) -> Vec<PaletteResult>;
    fn initialize(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
    /// True when the query starts with this provider's activation keyword.
    /// When any provider claims a query this way, only claiming providers
    /// are searched.
    fn keyword_match(&self, _raw_query: &str) -> bool {
        false
    }
    /// True (once) when background work produced new results and the
    /// current query should be searched again.
    fn poll_updates(&self) -> bool {
        false
    }
}

/// Strip an activation `keyword` from the start of `raw` (case-insensitive).
/// Keywords ending in a letter/digit must be followed by whitespace or the
/// end of the query; symbol keywords (`<`, `)`) need no separator.
pub fn strip_keyword<'a>(raw: &'a str, keyword: &str) -> Option<&'a str> {
    let keyword = keyword.trim();
    if keyword.is_empty() {
        return None;
    }
    let raw = raw.trim_start();
    let head = raw.get(..keyword.len())?;
    if !head.eq_ignore_ascii_case(keyword) {
        return None;
    }
    let rest = &raw[keyword.len()..];
    let needs_space = keyword.chars().last().is_some_and(char::is_alphanumeric);
    if needs_space && !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    Some(rest.trim())
}

/// Simple relevance score of `query` (lower-case) against `text`.
pub fn text_score(text: &str, query: &str) -> Option<f64> {
    if query.is_empty() {
        return None;
    }
    let t = text.to_lowercase();
    if t == query {
        Some(100.0)
    } else if t.starts_with(query) {
        Some(80.0)
    } else if t
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(query))
    {
        Some(65.0)
    } else if t.contains(query) {
        Some(55.0)
    } else if is_subsequence(&t, query) {
        Some(25.0)
    } else {
        None
    }
}

fn is_subsequence(text: &str, query: &str) -> bool {
    let mut it = text.chars();
    query.chars().all(|q| it.any(|c| c == q))
}

/// Shorten `s` to at most `max` characters, adding an ellipsis.
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_stripping() {
        assert_eq!(strip_keyword("<fire", "<"), Some("fire"));
        assert_eq!(strip_keyword("< fire", "<"), Some("fire"));
        assert_eq!(strip_keyword("clip foo", "clip"), Some("foo"));
        assert_eq!(strip_keyword("CLIP", "clip"), Some(""));
        assert_eq!(strip_keyword("clipboard", "clip"), None);
        assert_eq!(strip_keyword("firefox", "<"), None);
        assert_eq!(strip_keyword("anything", ""), None);
        assert_eq!(strip_keyword("é", "pkg"), None);
    }

    #[test]
    fn scoring() {
        assert_eq!(text_score("Firefox", "firefox"), Some(100.0));
        assert_eq!(text_score("Firefox Web", "fire"), Some(80.0));
        assert_eq!(text_score("Mozilla Firefox", "fire"), Some(65.0));
        assert_eq!(text_score("abcfirex", "fire"), Some(55.0));
        assert_eq!(text_score("f-i-r-e", "fire"), Some(25.0));
        assert_eq!(text_score("chrome", "fire"), None);
    }

    #[test]
    fn truncation() {
        assert_eq!(truncate_chars("hello", 10), "hello");
        assert_eq!(truncate_chars("hello world", 6), "hello…");
    }
}
