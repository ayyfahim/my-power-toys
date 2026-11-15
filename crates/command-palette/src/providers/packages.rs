//! Package search (upstream CmdPal WinGet extension, Linux flavour):
//! `apt-cache search`, `dnf search`, `pacman -Ss`, `zypper search` and
//! `flatpak search`. Searches run on a background thread after a short
//! debounce; the palette re-queries when results arrive
//! ([`Provider::poll_updates`]). Activating a result copies the install
//! command (Flatpak results open the software center via `appstream://`).

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{PaletteResult, Provider, QueryContext, ResultAction, ResultIcon, strip_keyword};
use crate::PackageSearchConfig;

const DEBOUNCE: Duration = Duration::from_millis(350);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Flatpak,
}

impl Manager {
    pub fn label(self) -> &'static str {
        match self {
            Manager::Apt => "apt",
            Manager::Dnf => "dnf",
            Manager::Pacman => "pacman",
            Manager::Zypper => "zypper",
            Manager::Flatpak => "flatpak",
        }
    }

    fn program(self) -> &'static str {
        match self {
            Manager::Apt => "apt-cache",
            Manager::Dnf => "dnf",
            Manager::Pacman => "pacman",
            Manager::Zypper => "zypper",
            Manager::Flatpak => "flatpak",
        }
    }

    fn args(self, query: &str) -> Vec<String> {
        let q = query.to_string();
        match self {
            Manager::Apt => vec!["search".into(), "--names-only".into(), q],
            Manager::Dnf => vec!["search".into(), "-q".into(), q],
            Manager::Pacman => vec!["-Ss".into(), q],
            Manager::Zypper => vec![
                "--non-interactive".into(),
                "--quiet".into(),
                "search".into(),
                q,
            ],
            Manager::Flatpak => vec![
                "search".into(),
                "--columns=application,name,description".into(),
                q,
            ],
        }
    }

    fn parse(self, out: &str) -> Vec<Package> {
        let rows = match self {
            Manager::Apt => parse_apt(out),
            Manager::Dnf => parse_dnf(out),
            Manager::Pacman => parse_pacman(out),
            Manager::Zypper => parse_zypper(out),
            Manager::Flatpak => parse_flatpak(out),
        };
        rows.into_iter()
            .map(|(id, display, description)| Package {
                manager: self,
                id,
                display,
                description,
            })
            .collect()
    }

    pub fn action(self, id: &str) -> ResultAction {
        match self {
            Manager::Apt => ResultAction::CopyToClipboard(format!("sudo apt install {id}")),
            Manager::Dnf => ResultAction::CopyToClipboard(format!("sudo dnf install {id}")),
            Manager::Pacman => ResultAction::CopyToClipboard(format!("sudo pacman -S {id}")),
            Manager::Zypper => ResultAction::CopyToClipboard(format!("sudo zypper install {id}")),
            Manager::Flatpak => ResultAction::OpenUrl(format!("appstream://{id}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    pub manager: Manager,
    /// Package name / Flatpak application ID.
    pub id: String,
    pub display: String,
    pub description: String,
}

/// `name - description`
pub fn parse_apt(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .filter_map(|l| l.split_once(" - "))
        .map(|(n, d)| {
            (
                n.trim().to_string(),
                n.trim().to_string(),
                d.trim().to_string(),
            )
        })
        .filter(|(n, _, _)| !n.is_empty())
        .collect()
}

const ARCHES: &[&str] = &[
    "x86_64", "noarch", "i686", "i386", "aarch64", "ppc64le", "s390x", "armv7hl", "src",
];

fn strip_arch(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((base, arch)) if ARCHES.contains(&arch) => base,
        _ => name,
    }
}

/// dnf4 `name.arch : summary` and dnf5 ` name.arch<TAB>summary`.
pub fn parse_dnf(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .map(str::trim)
        .filter(|l| {
            !l.is_empty()
                && !l.starts_with('=')
                && !l.starts_with("Matched fields")
                && !l.starts_with("Last metadata")
                && !l.starts_with("Updating and loading")
                && !l.starts_with("Repositories loaded")
        })
        .filter_map(|l| {
            let (name, desc) = l
                .split_once(" : ")
                .or_else(|| l.split_once('\t'))
                .or_else(|| l.split_once(char::is_whitespace))?;
            let name = strip_arch(name.trim());
            (!name.is_empty())
                .then(|| (name.to_string(), name.to_string(), desc.trim().to_string()))
        })
        .collect()
}

/// `repo/name version [installed]` followed by an indented description.
pub fn parse_pacman(out: &str) -> Vec<(String, String, String)> {
    let mut rows = Vec::new();
    let mut lines = out.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with(char::is_whitespace) || line.trim().is_empty() {
            continue;
        }
        let Some(full) = line.split_whitespace().next() else {
            continue;
        };
        let name = full.rsplit_once('/').map_or(full, |(_, n)| n).to_string();
        let desc = match lines.peek() {
            Some(next) if next.starts_with(char::is_whitespace) => {
                lines.next().unwrap_or_default().trim().to_string()
            }
            _ => String::new(),
        };
        let display = if line.contains("[installed") {
            format!("{name} (installed)")
        } else {
            name.clone()
        };
        rows.push((name, display, desc));
    }
    rows
}

/// `S | Name | Summary | Type` table.
pub fn parse_zypper(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .filter(|l| l.contains('|') && !l.starts_with("--"))
        .filter_map(|l| {
            let cols: Vec<&str> = l.split('|').map(str::trim).collect();
            if cols.len() < 3 || cols[1].is_empty() || cols[1] == "Name" {
                return None;
            }
            if cols
                .get(3)
                .is_some_and(|t| !t.is_empty() && *t != "package")
            {
                return None;
            }
            let installed = cols[0].starts_with('i');
            let name = cols[1].to_string();
            let display = if installed {
                format!("{name} (installed)")
            } else {
                name.clone()
            };
            Some((name, display, cols[2].to_string()))
        })
        .collect()
}

/// `application<TAB>name<TAB>description`
pub fn parse_flatpak(out: &str) -> Vec<(String, String, String)> {
    out.lines()
        .filter_map(|l| {
            let mut cols = l.split('\t');
            let app = cols.next()?.trim();
            let name = cols.next()?.trim();
            let desc = cols.next().unwrap_or_default().trim();
            (app.contains('.') && !name.is_empty())
                .then(|| (app.to_string(), name.to_string(), desc.to_string()))
        })
        .collect()
}

/// Drop leading dashes so a query can't be read as a command-line option.
pub fn sanitize_query(q: &str) -> String {
    q.trim().trim_start_matches('-').trim().to_string()
}

fn tool_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

/// Package managers to query for `config`.
pub fn detect_managers(config: &PackageSearchConfig) -> Vec<Manager> {
    let mut out = Vec::new();
    let distro = match config.manager.trim().to_lowercase().as_str() {
        "apt" => Some(Manager::Apt),
        "dnf" => Some(Manager::Dnf),
        "pacman" => Some(Manager::Pacman),
        "zypper" => Some(Manager::Zypper),
        "none" | "flatpak" => None,
        _ => [Manager::Apt, Manager::Dnf, Manager::Pacman, Manager::Zypper]
            .into_iter()
            .find(|m| tool_exists(m.program())),
    };
    out.extend(distro);
    if config.include_flatpak && tool_exists("flatpak") {
        out.push(Manager::Flatpak);
    }
    out
}

#[derive(Default)]
struct Shared {
    /// Latest query the user typed.
    requested: String,
    /// Results of the last finished search.
    done: Option<(String, Vec<Package>)>,
}

pub struct PackagesProvider {
    keyword: String,
    config: PackageSearchConfig,
    managers: Vec<Manager>,
    shared: Arc<Mutex<Shared>>,
    updated: Arc<AtomicBool>,
}

impl PackagesProvider {
    pub fn new(keyword: String, config: PackageSearchConfig) -> Self {
        Self {
            keyword,
            config,
            managers: Vec::new(),
            shared: Arc::new(Mutex::new(Shared::default())),
            updated: Arc::new(AtomicBool::new(false)),
        }
    }

    fn start_search(&self, query: String) {
        let shared = Arc::clone(&self.shared);
        let updated = Arc::clone(&self.updated);
        let managers = self.managers.clone();
        std::thread::spawn(move || {
            std::thread::sleep(DEBOUNCE);
            if shared.lock().map(|s| s.requested != query).unwrap_or(true) {
                return; // superseded while debouncing
            }
            let mut results = Vec::new();
            for m in managers {
                match Command::new(m.program())
                    .args(m.args(&query))
                    .stdin(Stdio::null())
                    .stderr(Stdio::null())
                    .output()
                {
                    Ok(out) => results.extend(m.parse(&String::from_utf8_lossy(&out.stdout))),
                    Err(e) => tracing::warn!("Package search: {} failed: {e}", m.program()),
                }
            }
            if let Ok(mut s) = shared.lock()
                && s.requested == query
            {
                s.done = Some((query, results));
                updated.store(true, Ordering::Relaxed);
            }
        });
    }

    fn placeholder(title: String, subtitle: &str) -> PaletteResult {
        PaletteResult {
            id: "pkg:status".into(),
            title,
            subtitle: Some(subtitle.into()),
            icon: ResultIcon::Emoji('📦'),
            action: ResultAction::None,
            score: 250.0,
            provider_tag: "package",
        }
    }
}

/// Rank packages: exact name, then prefix, then tool order.
pub fn rank(packages: &[Package], query: &str) -> Vec<(f64, Package)> {
    let q = query.to_lowercase();
    let mut out: Vec<(f64, Package)> = packages
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p.id.to_lowercase();
            let display = p.display.to_lowercase();
            let bonus = if name == q || display == q {
                60.0
            } else if name.starts_with(&q) || display.starts_with(&q) {
                30.0
            } else {
                0.0
            };
            (200.0 + bonus - i as f64 * 0.01, p.clone())
        })
        .collect();
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    out
}

impl Provider for PackagesProvider {
    fn tag(&self) -> &'static str {
        "package"
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
        if self.managers.is_empty() {
            return vec![Self::placeholder(
                "No package manager found".into(),
                "Looked for apt-cache, dnf, pacman, zypper and flatpak",
            )];
        }
        let query = sanitize_query(ctx.stripped_query);
        if query.chars().count() < self.config.min_query_len.max(1) {
            let names: Vec<&str> = self.managers.iter().map(|m| m.label()).collect();
            return vec![Self::placeholder(
                "Type a package name…".into(),
                &format!("Searches {}", names.join(", ")),
            )];
        }

        let Ok(mut shared) = self.shared.lock() else {
            return Vec::new();
        };
        if let Some((done_q, results)) = &shared.done
            && *done_q == query
        {
            if results.is_empty() {
                return vec![Self::placeholder(
                    format!("No packages found for \"{query}\""),
                    "Try another name",
                )];
            }
            return rank(results, &query)
                .into_iter()
                .take(ctx.max_results)
                .map(|(score, p)| PaletteResult {
                    id: format!("{}:{}", p.manager.label(), p.id),
                    title: p.display.clone(),
                    subtitle: Some(format!(
                        "[{}] {} · {}",
                        p.manager.label(),
                        p.description,
                        if p.manager == Manager::Flatpak {
                            "Enter: open in software center"
                        } else {
                            "Enter: copy install command"
                        }
                    )),
                    icon: ResultIcon::Emoji('📦'),
                    action: p.manager.action(&p.id),
                    score,
                    provider_tag: "package",
                })
                .collect();
        }

        if shared.requested != query {
            shared.requested = query.clone();
            drop(shared);
            self.start_search(query.clone());
        }
        vec![Self::placeholder(
            format!("Searching packages for \"{query}\"…"),
            "Results appear when the search finishes",
        )]
    }

    fn initialize(&mut self) -> anyhow::Result<()> {
        self.managers = detect_managers(&self.config);
        tracing::info!("Command Palette: package search via {:?}", self.managers);
        Ok(())
    }

    fn poll_updates(&self) -> bool {
        self.updated.swap(false, Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apt_output() {
        let rows = parse_apt("firefox - Safe and easy web browser\nfirefox-l10n-de - German\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "firefox");
        assert_eq!(rows[0].2, "Safe and easy web browser");
    }

    #[test]
    fn dnf4_and_dnf5_output() {
        let dnf4 =
            "=== Name Exactly Matched: firefox ===\nfirefox.x86_64 : Mozilla Firefox Web browser\n";
        assert_eq!(
            parse_dnf(dnf4),
            vec![(
                "firefox".into(),
                "firefox".into(),
                "Mozilla Firefox Web browser".into()
            )]
        );
        let dnf5 = "Matched fields: name (exact)\n firefox.x86_64\tMozilla Firefox Web browser\n";
        assert_eq!(parse_dnf(dnf5)[0].0, "firefox");
    }

    #[test]
    fn pacman_output() {
        let out = "extra/firefox 125.0-1 [installed]\n    Fast, Private & Safe Web Browser\nextra/firefox-i18n-de 125.0-1\n    German language pack\n";
        let rows = parse_pacman(out);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "firefox");
        assert_eq!(rows[0].1, "firefox (installed)");
        assert_eq!(rows[1].2, "German language pack");
    }

    #[test]
    fn zypper_output() {
        let out = "S | Name    | Summary         | Type\n--+---------+-----------------+--------\ni | firefox | Mozilla Firefox | package\n  | firefox | Mozilla Firefox | srcpackage\n";
        let rows = parse_zypper(out);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].1, "firefox (installed)");
    }

    #[test]
    fn flatpak_output() {
        let out =
            "org.mozilla.firefox\tFirefox\tFast, Private & Safe Web Browser\nNo matches found\n";
        let rows = parse_flatpak(out);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "org.mozilla.firefox");
        assert!(matches!(
            Manager::Flatpak.action(&rows[0].0),
            ResultAction::OpenUrl(u) if u == "appstream://org.mozilla.firefox"
        ));
    }

    #[test]
    fn query_sanitizing() {
        assert_eq!(sanitize_query("  --help "), "help");
        assert_eq!(sanitize_query("vim"), "vim");
    }

    #[test]
    fn ranking_prefers_exact() {
        let pkgs: Vec<Package> = ["vim-tiny", "neovim", "vim"]
            .iter()
            .map(|n| Package {
                manager: Manager::Apt,
                id: n.to_string(),
                display: n.to_string(),
                description: String::new(),
            })
            .collect();
        let ranked = rank(&pkgs, "vim");
        assert_eq!(ranked[0].1.id, "vim");
        assert_eq!(ranked[1].1.id, "vim-tiny");
        assert_eq!(ranked[2].1.id, "neovim");
    }

    #[test]
    fn explicit_none_manager() {
        let cfg = PackageSearchConfig {
            manager: "none".into(),
            include_flatpak: false,
            min_query_len: 2,
        };
        assert!(detect_managers(&cfg).is_empty());
    }
}
