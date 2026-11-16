//! Program plugin: installed applications from `.desktop` files.

use super::{LauncherResult, QueryCtx, text_score};
use crate::actions::Action;
use crate::config::PluginId;
use crate::desktop::DesktopEntry;

pub fn search(ctx: &QueryCtx, apps: &[DesktopEntry]) -> Vec<LauncherResult> {
    let q = ctx.search.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<LauncherResult> = apps
        .iter()
        .filter_map(|app| {
            let a = crate::search::match_score(app, &q);
            let b = text_score(&app.name, &q);
            let score = a
                .into_iter()
                .chain(b)
                .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))))?;
            Some(to_result(app, score))
        })
        .collect();
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(ctx.config.max_results.max(1) * 2);
    out
}

fn to_result(app: &DesktopEntry, score: f64) -> LauncherResult {
    let subtitle = app
        .generic_name
        .clone()
        .or_else(|| app.comment.clone())
        .unwrap_or_else(|| app.exec.clone());
    LauncherResult::new(
        PluginId::Program,
        app.name.clone(),
        Action::Launch {
            exec: app.exec.clone(),
            terminal: app.terminal,
        },
    )
    .subtitle(subtitle)
    .icon(app.icon.clone())
    .score(score)
    .alt(
        "Open containing folder",
        Action::OpenContainingFolder(app.path.clone()),
    )
    .key(format!("program:{}", app.name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppLauncherConfig;

    fn entry(name: &str) -> DesktopEntry {
        DesktopEntry {
            name: name.into(),
            generic_name: Some("Web Browser".into()),
            comment: None,
            exec: name.to_lowercase(),
            icon: Some(name.to_lowercase()),
            categories: vec![],
            keywords: vec![],
            terminal: false,
            no_display: false,
            path: "/usr/share/applications/x.desktop".into(),
        }
    }

    #[test]
    fn finds_by_prefix_and_initials() {
        let cfg = AppLauncherConfig::default();
        let apps = vec![entry("Firefox"), entry("Visual Studio Code")];
        let ctx = QueryCtx {
            search: "vsc",
            by_keyword: false,
            config: &cfg,
        };
        let r = search(&ctx, &apps);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].title, "Visual Studio Code");
        let ctx = QueryCtx {
            search: "fir",
            by_keyword: false,
            config: &cfg,
        };
        let r = search(&ctx, &apps);
        assert_eq!(r[0].title, "Firefox");
        assert_eq!(r[0].key, "program:Firefox");
    }
}
