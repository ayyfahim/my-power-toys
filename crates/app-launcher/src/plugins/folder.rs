//! Folder plugin: browse the file system by typing a path starting with `/`
//! or `~`. `~/Doc` lists entries of `~` starting with "Doc"; a trailing `/`
//! lists the directory. Hidden entries are shown only when the typed name
//! starts with a dot.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::config::PluginId;
use std::path::{Path, PathBuf};

const MAX_ENTRIES: usize = 200;

/// Expand a typed path. Returns `None` unless it starts with `/` or `~`.
pub fn expand(q: &str, home: &Path) -> Option<PathBuf> {
    let q = q.trim();
    if q == "~" {
        return Some(home.to_path_buf());
    }
    if let Some(rest) = q.strip_prefix("~/") {
        return Some(home.join(rest));
    }
    q.starts_with('/').then(|| PathBuf::from(q))
}

/// Split into (directory to list, partial name filter).
pub fn split(typed: &str, expanded: &Path) -> (PathBuf, String) {
    if typed.ends_with('/') || typed.trim() == "~" {
        return (expanded.to_path_buf(), String::new());
    }
    let parent = expanded.parent().unwrap_or(Path::new("/")).to_path_buf();
    let name = expanded
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    (parent, name)
}

/// Rank a directory entry name against the partial filter.
pub fn name_score(name: &str, partial: &str) -> Option<f64> {
    if partial.is_empty() {
        return (!name.starts_with('.')).then_some(50.0);
    }
    if name.starts_with('.') && !partial.starts_with('.') {
        return None;
    }
    let n = name.to_lowercase();
    let p = partial.to_lowercase();
    if n == p {
        Some(100.0)
    } else if n.starts_with(&p) {
        Some(80.0)
    } else if n.contains(&p) {
        Some(55.0)
    } else {
        None
    }
}

fn path_result(path: &Path, title: String, score: f64) -> LauncherResult {
    let is_dir = path.is_dir();
    let p = path.to_string_lossy().into_owned();
    let r = LauncherResult::new(PluginId::Folder, title, Action::Open(p.clone()))
        .subtitle(p.clone())
        .score(score)
        .key(format!("path:{p}"));
    if is_dir {
        r.icon(Some("folder".into())).alt(
            "Open in terminal",
            Action::OpenTerminalAt(path.to_path_buf()),
        )
    } else {
        r.icon(Some("text-x-generic".into())).alt(
            "Open containing folder",
            Action::OpenContainingFolder(path.to_path_buf()),
        )
    }
}

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let typed = ctx.search.trim();
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let Some(expanded) = expand(typed, &home) else {
        return Vec::new();
    };
    // Paths beat everything else when the query is clearly a path.
    let base = 200.0;
    let mut out = Vec::new();

    if expanded.exists() && !typed.ends_with('/') && typed != "/" {
        out.push(path_result(&expanded, typed.to_string(), base + 110.0));
    }

    let (dir, partial) = split(typed, &expanded);
    let Ok(read) = std::fs::read_dir(&dir) else {
        return out;
    };
    let mut entries: Vec<(f64, bool, String, PathBuf)> = read
        .flatten()
        .take(5000)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let score = name_score(&name, &partial)?;
            let path = e.path();
            if path == expanded && !typed.ends_with('/') {
                return None; // already added as the exact match
            }
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            Some((score, is_dir, name, path))
        })
        .collect();
    // Higher score, then directories first, then alphabetical.
    entries.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.1.cmp(&a.1))
            .then(a.2.to_lowercase().cmp(&b.2.to_lowercase()))
    });
    for (i, (score, is_dir, name, path)) in entries.into_iter().take(MAX_ENTRIES).enumerate() {
        let title = if is_dir { format!("{name}/") } else { name };
        out.push(path_result(&path, title, base + score - i as f64 * 0.01));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_paths() {
        let home = Path::new("/home/u");
        assert_eq!(expand("~", home), Some(PathBuf::from("/home/u")));
        assert_eq!(expand("~/Doc", home), Some(PathBuf::from("/home/u/Doc")));
        assert_eq!(expand("/etc", home), Some(PathBuf::from("/etc")));
        assert_eq!(expand("firefox", home), None);
        assert_eq!(expand("~user", home), None);
    }

    #[test]
    fn splits_dir_and_partial() {
        assert_eq!(
            split("/usr/lo", Path::new("/usr/lo")),
            (PathBuf::from("/usr"), "lo".into())
        );
        assert_eq!(
            split("/usr/", Path::new("/usr/")),
            (PathBuf::from("/usr/"), String::new())
        );
        assert_eq!(
            split("/", Path::new("/")),
            (PathBuf::from("/"), String::new())
        );
    }

    #[test]
    fn scores_names() {
        assert_eq!(name_score("Documents", "doc"), Some(80.0));
        assert_eq!(name_score(".bashrc", ""), None);
        assert_eq!(name_score(".bashrc", ".ba"), Some(80.0));
        assert_eq!(name_score("Music", "doc"), None);
    }

    #[test]
    fn lists_root() {
        let cfg = crate::config::AppLauncherConfig::default();
        let r = search(&QueryCtx {
            search: "/",
            by_keyword: false,
            config: &cfg,
        });
        assert!(r.iter().any(|r| r.title == "etc/" || r.title == "usr/"));
    }
}
