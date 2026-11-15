//! File search plugin (`?`), the Linux stand-in for the Windows Indexer:
//! uses `plocate`, `locate` or `fd`, whichever is installed (or the one set in
//! `file_search_tool`).

use super::{LauncherResult, QueryCtx};
use crate::actions::{Action, which};
use crate::config::PluginId;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Plocate,
    Locate,
    Fd,
}

impl Tool {
    fn program(self) -> &'static str {
        match self {
            Tool::Plocate => "plocate",
            Tool::Locate => "locate",
            Tool::Fd => "fd",
        }
    }
}

pub struct Indexer {
    preference: String,
    tool: Option<Option<(Tool, String)>>,
}

impl Indexer {
    pub fn new(preference: &str) -> Self {
        Self {
            preference: preference.trim().to_lowercase(),
            tool: None,
        }
    }

    fn tool(&mut self) -> Option<(Tool, String)> {
        if self.tool.is_none() {
            let order: &[Tool] = match self.preference.as_str() {
                "plocate" => &[Tool::Plocate],
                "locate" => &[Tool::Locate],
                "fd" => &[Tool::Fd],
                _ => &[Tool::Plocate, Tool::Locate, Tool::Fd],
            };
            let found = order.iter().find_map(|t| {
                if which(t.program()).is_some() {
                    return Some((*t, t.program().to_string()));
                }
                // Debian/Ubuntu ship fd as `fdfind`.
                (*t == Tool::Fd && which("fdfind").is_some()).then(|| (Tool::Fd, "fdfind".into()))
            });
            if found.is_none() {
                tracing::warn!("App Launcher: no file search tool found (plocate, locate, fd)");
            }
            self.tool = Some(found);
        }
        self.tool.clone().flatten()
    }

    pub fn search(&mut self, ctx: &QueryCtx) -> Vec<LauncherResult> {
        let q = ctx.search.trim();
        if q.chars().count() < 2 {
            return if ctx.by_keyword {
                vec![LauncherResult::new(
                    PluginId::Indexer,
                    "Type at least 2 characters to search files",
                    Action::None,
                )]
            } else {
                Vec::new()
            };
        }
        let Some((tool, prog)) = self.tool() else {
            return if ctx.by_keyword {
                vec![
                    LauncherResult::new(
                        PluginId::Indexer,
                        "No file search tool found",
                        Action::None,
                    )
                    .subtitle("Install plocate, mlocate or fd"),
                ]
            } else {
                Vec::new()
            };
        };
        let limit = (ctx.config.max_results.max(1) * 3).to_string();
        let home = dirs::home_dir().unwrap_or_else(|| "/".into());
        let argv = build_args(tool, q, &limit, &home);
        let output = Command::new(&prog)
            .args(&argv)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let Ok(output) = output else {
            return Vec::new();
        };
        let base = if ctx.by_keyword { 250.0 } else { 20.0 };
        parse_output(&String::from_utf8_lossy(&output.stdout))
            .into_iter()
            .enumerate()
            .map(|(i, path)| {
                let p = Path::new(&path);
                let name = p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone());
                let is_dir = p.is_dir();
                LauncherResult::new(PluginId::Indexer, name, Action::Open(path.clone()))
                    .subtitle(path.clone())
                    .icon(Some(
                        if is_dir { "folder" } else { "text-x-generic" }.into(),
                    ))
                    .score(base - i as f64 * 0.1)
                    .alt(
                        "Open containing folder",
                        Action::OpenContainingFolder(p.to_path_buf()),
                    )
                    .key(format!("path:{path}"))
            })
            .collect()
    }
}

/// Command-line arguments for the selected tool; the query always follows `--`.
pub fn build_args(tool: Tool, query: &str, limit: &str, home: &Path) -> Vec<String> {
    let mut v: Vec<String> = match tool {
        Tool::Plocate | Tool::Locate => vec!["-i".into(), "-l".into(), limit.into()],
        Tool::Fd => vec![
            "--ignore-case".into(),
            "--absolute-path".into(),
            "--max-results".into(),
            limit.into(),
        ],
    };
    // locate treats multiple words as AND patterns.
    v.push("--".into());
    match tool {
        Tool::Fd => {
            v.push(regex_escape(query));
            v.push(home.to_string_lossy().into_owned());
        }
        _ => v.extend(query.split_whitespace().map(String::from)),
    }
    v
}

fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub fn parse_output(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_put_query_after_double_dash() {
        let a = build_args(Tool::Plocate, "-rf report", "10", Path::new("/home/u"));
        assert_eq!(a, vec!["-i", "-l", "10", "--", "-rf", "report"]);
        let a = build_args(Tool::Fd, "a.b", "5", Path::new("/home/u"));
        assert_eq!(a[a.len() - 3..], ["--", "a\\.b", "/home/u"]);
    }

    #[test]
    fn parses_lines() {
        assert_eq!(parse_output("/a\n\n /b \n"), vec!["/a", "/b"]);
    }
}
