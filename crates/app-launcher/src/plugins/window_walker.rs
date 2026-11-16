//! Window Walker plugin (`<`): switch to an open window, using X11
//! `_NET_CLIENT_LIST` and `_NET_ACTIVE_WINDOW`. Ctrl/Shift+Enter closes it.

use super::{LauncherResult, QueryCtx, text_score};
use crate::actions::Action;
use crate::config::PluginId;
use crate::x11::{WindowInfo, X11};

#[derive(Default)]
pub struct WindowWalker {
    /// `None` = not tried yet, `Some(None)` = X11 unavailable.
    conn: Option<Option<X11>>,
}

impl WindowWalker {
    fn conn(&mut self) -> Option<&X11> {
        if self.conn.is_none() {
            let c = X11::connect()
                .map_err(|e| tracing::warn!("Window Walker unavailable: {e}"))
                .ok();
            self.conn = Some(c);
        }
        self.conn.as_ref().and_then(|c| c.as_ref())
    }

    pub fn search(&mut self, ctx: &QueryCtx) -> Vec<LauncherResult> {
        let q = ctx.search.trim().to_lowercase();
        if q.is_empty() && !ctx.by_keyword {
            return Vec::new();
        }
        let all_desktops = ctx.config.window_walker_all_desktops;
        let Some(x) = self.conn() else {
            return if ctx.by_keyword {
                vec![LauncherResult::new(
                    PluginId::WindowWalker,
                    "Window Walker needs an X11 session",
                    Action::None,
                )]
            } else {
                Vec::new()
            };
        };
        let own_pid = std::process::id();
        let windows: Vec<WindowInfo> = x
            .list_windows(all_desktops)
            .into_iter()
            .filter(|w| w.pid != Some(own_pid))
            .collect();
        let base = if ctx.by_keyword { 200.0 } else { 0.0 };
        rank(&windows, &q)
            .into_iter()
            .map(|(score, w)| {
                let subtitle = if w.class.is_empty() {
                    "Switch to window".to_string()
                } else {
                    format!("{} · Switch to window", w.class)
                };
                LauncherResult::new(
                    PluginId::WindowWalker,
                    w.title.clone(),
                    Action::ActivateWindow(w.id),
                )
                .subtitle(subtitle)
                .icon((!w.class.is_empty()).then(|| w.class.to_lowercase()))
                .score(base + score)
                .alt("Close window", Action::CloseWindow(w.id))
            })
            .collect()
    }
}

/// Score windows by title/class. Empty query keeps MRU order.
pub fn rank<'a>(windows: &'a [WindowInfo], q: &str) -> Vec<(f64, &'a WindowInfo)> {
    let mut out: Vec<(f64, &WindowInfo)> = windows
        .iter()
        .enumerate()
        .filter_map(|(i, w)| {
            // Small MRU bonus keeps recently used windows first among equals.
            let mru = 1.0 / (1.0 + i as f64);
            if q.is_empty() {
                return Some((50.0 + mru, w));
            }
            let t = text_score(&w.title, q).map(|s| s * 0.6);
            let c = text_score(&w.class, q).map(|s| s * 0.5);
            let best = t
                .into_iter()
                .chain(c)
                .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))))?;
            Some((best + mru, w))
        })
        .collect();
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(id: u32, title: &str, class: &str) -> WindowInfo {
        WindowInfo {
            id,
            title: title.into(),
            class: class.into(),
            pid: None,
            desktop: None,
        }
    }

    #[test]
    fn ranks_by_title_and_class() {
        let ws = vec![
            w(1, "README.md - Visual Studio Code", "Code"),
            w(2, "Mozilla Firefox", "firefox"),
            w(3, "Terminal", "kitty"),
        ];
        let r = rank(&ws, "firefox");
        assert_eq!(r[0].1.id, 2);
        let r = rank(&ws, "kitty");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].1.id, 3);
        let all = rank(&ws, "");
        assert_eq!(
            all.iter().map(|r| r.1.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(rank(&ws, "zzz").is_empty());
    }
}
