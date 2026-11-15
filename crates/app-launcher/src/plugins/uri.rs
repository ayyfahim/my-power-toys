//! URI plugin (`//`, global): open http(s)/ftp/file/mailto URLs and bare
//! domains like `example.com`.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::config::PluginId;

const SCHEMES: &[&str] = &["http://", "https://", "ftp://", "file://", "mailto:"];

/// Returns `(url_to_open, has_explicit_scheme)` if the query looks like a URI.
pub fn parse_uri(q: &str) -> Option<(String, bool)> {
    let q = q.trim();
    if q.is_empty() || q.contains(char::is_whitespace) {
        return None;
    }
    let lower = q.to_lowercase();
    if let Some(s) = SCHEMES.iter().find(|s| lower.starts_with(*s)) {
        return (q.len() > s.len()).then(|| (q.to_string(), true));
    }
    // localhost[:port][/path]
    let host_part = q.split(['/', '?', '#']).next().unwrap_or("");
    let host = host_part.split(':').next().unwrap_or("");
    if let Some(port) = host_part.split(':').nth(1)
        && (port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return Some((format!("http://{q}"), false));
    }
    // IPv4
    let octets: Vec<&str> = host.split('.').collect();
    if octets.len() == 4 && octets.iter().all(|o| o.parse::<u8>().is_ok()) {
        return Some((format!("http://{q}"), false));
    }
    // domain.tld: labels of [a-z0-9-], TLD alphabetic and >= 2 chars
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return None;
    }
    let valid_label = |l: &&str| {
        !l.is_empty()
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    let tld = labels.last()?;
    if labels.iter().all(valid_label)
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
    {
        return Some((format!("https://{q}"), false));
    }
    None
}

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let Some((url, explicit)) = parse_uri(ctx.search) else {
        return Vec::new();
    };
    let score = if ctx.by_keyword || explicit {
        250.0
    } else {
        70.0
    };
    vec![
        LauncherResult::new(PluginId::Uri, url.clone(), Action::Open(url.clone()))
            .subtitle("Open in default handler")
            .icon(Some("web-browser".into()))
            .score(score)
            .alt("Copy URL", Action::Copy(url)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_uris() {
        assert_eq!(
            parse_uri("https://example.com/x"),
            Some(("https://example.com/x".into(), true))
        );
        assert_eq!(
            parse_uri("example.com"),
            Some(("https://example.com".into(), false))
        );
        assert_eq!(
            parse_uri("localhost:8080/api"),
            Some(("http://localhost:8080/api".into(), false))
        );
        assert_eq!(
            parse_uri("192.168.1.1"),
            Some(("http://192.168.1.1".into(), false))
        );
        assert!(parse_uri("file:///etc/hosts").is_some());
    }

    #[test]
    fn rejects_non_uris() {
        assert_eq!(parse_uri("firefox"), None);
        assert_eq!(parse_uri("2.5"), None);
        assert_eq!(parse_uri("hello world.com"), None);
        assert_eq!(parse_uri("https://"), None);
        assert_eq!(parse_uri("a.b1"), None);
    }
}
