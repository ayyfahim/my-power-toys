use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

pub const DEFAULT_HOTKEY: &str = "Super+Ctrl+W";

fn default_hotkey() -> String {
    DEFAULT_HOTKEY.to_string()
}

/// Order of the workspace list in the editor (upstream `SortBy`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortBy {
    /// Most recently launched first; never-launched ones last (by creation).
    #[default]
    #[serde(alias = "LastLaunched")]
    LastLaunched,
    /// Newest first.
    #[serde(alias = "Created")]
    Created,
    /// Alphabetical, case-insensitive.
    #[serde(alias = "Name")]
    Name,
}

impl SortBy {
    pub const ALL: [SortBy; 3] = [SortBy::LastLaunched, SortBy::Created, SortBy::Name];

    pub fn label(self) -> &'static str {
        match self {
            SortBy::LastLaunched => "Last launched",
            SortBy::Created => "Created",
            SortBy::Name => "Name",
        }
    }
}

/// Top-level configuration for the Workspaces module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspacesConfig {
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    /// Hotkey that opens the editor (upstream `Hotkey`).
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Editor list order (upstream `SortBy`).
    #[serde(default)]
    pub sort_by: SortBy,
}

impl Default for WorkspacesConfig {
    fn default() -> Self {
        Self {
            workspaces: Vec::new(),
            hotkey: default_hotkey(),
            sort_by: SortBy::default(),
        }
    }
}

/// Parse an RFC 3339 timestamp; `None` if missing or malformed.
pub fn parse_timestamp(s: &str) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    chrono::DateTime::parse_from_rfc3339(s.trim()).ok()
}

/// Human-readable local time for a stored timestamp (falls back to the raw
/// string when it can't be parsed).
pub fn format_timestamp(s: &str) -> String {
    match parse_timestamp(s) {
        Some(dt) => dt
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        None => s.to_string(),
    }
}

impl WorkspacesConfig {
    /// Indices into `workspaces` in display order for `sort_by`.
    pub fn sorted_indices(&self) -> Vec<usize> {
        sorted_indices(&self.workspaces, self.sort_by)
    }
}

/// Indices of `workspaces` ordered by `sort_by`. Stable, so equal keys keep
/// their stored order.
pub fn sorted_indices(workspaces: &[Workspace], sort_by: SortBy) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..workspaces.len()).collect();
    let created = |i: usize| parse_timestamp(&workspaces[i].created_at);
    let launched = |i: usize| {
        workspaces[i]
            .last_launched
            .as_deref()
            .and_then(parse_timestamp)
    };
    match sort_by {
        SortBy::Name => idx.sort_by_cached_key(|&i| workspaces[i].name.to_lowercase()),
        // Option ordering puts None first, so Reverse puts it last.
        SortBy::Created => idx.sort_by_key(|&i| std::cmp::Reverse(created(i))),
        SortBy::LastLaunched => idx.sort_by_key(|&i| {
            (
                std::cmp::Reverse(launched(i)),
                std::cmp::Reverse(created(i)),
            )
        }),
    }
    idx
}

/// A saved workspace: a named collection of apps with positions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub apps: Vec<AppEntry>,
    pub created_at: String,
    pub last_launched: Option<String>,
    #[serde(default = "default_true")]
    pub move_existing: bool,
    #[serde(default)]
    pub create_shortcut: bool,
}

impl Workspace {
    pub fn new(name: impl Into<String>, apps: Vec<AppEntry>) -> Self {
        Self {
            name: name.into(),
            apps,
            created_at: chrono::Local::now().to_rfc3339(),
            last_launched: None,
            move_existing: true,
            create_shortcut: false,
        }
    }

    pub fn enabled_apps(&self) -> impl Iterator<Item = &AppEntry> {
        self.apps.iter().filter(|a| a.enabled)
    }

    pub fn app_count(&self) -> usize {
        self.apps.len()
    }
}

/// A single application entry within a workspace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppEntry {
    pub name: String,
    pub wm_class: String,
    pub exec: String,
    #[serde(default)]
    pub args: Vec<String>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub monitor: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub minimized: bool,
}

/// Resolve executable path from a PID via /proc.
pub fn resolve_exec_from_pid(pid: u32) -> Option<String> {
    std::fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}

/// Get command-line arguments from a PID via /proc.
pub fn resolve_args_from_pid(pid: u32) -> Vec<String> {
    std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
        .ok()
        .map(|s| {
            s.split('\0')
                .skip(1) // skip executable itself
                .filter(|a| !a.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Create a `.desktop` file for a workspace so it appears in the app menu.
#[cfg(feature = "gui")]
pub fn create_desktop_shortcut(workspace_name: &str, workspace_idx: usize) -> anyhow::Result<()> {
    let app_dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("no home dir"))?
        .join(".local/share/applications");
    std::fs::create_dir_all(&app_dir)?;

    let slug = workspace_name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "-");
    let filename = format!("mpt-workspace-{slug}.desktop");

    let content = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Workspace: {name}\n\
         Comment=Launch MyPowerToys workspace \"{name}\"\n\
         Exec=mpt-workspaces --launch {idx}\n\
         Icon=my-power-toys\n\
         Terminal=false\n\
         Categories=Utility;System;\n\
         NoDisplay=false\n",
        name = workspace_name,
        idx = workspace_idx,
    );

    std::fs::write(app_dir.join(&filename), content)?;
    tracing::info!("Created desktop shortcut: {filename}");
    Ok(())
}

/// Remove a desktop shortcut for a workspace.
#[cfg(feature = "gui")]
pub fn remove_desktop_shortcut(workspace_name: &str) {
    let slug = workspace_name
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric(), "-");
    let filename = format!("mpt-workspace-{slug}.desktop");

    if let Some(home) = dirs::home_dir() {
        let path = home.join(".local/share/applications").join(&filename);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            tracing::info!("Removed desktop shortcut: {filename}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_empty() {
        let config = WorkspacesConfig::default();
        assert!(config.workspaces.is_empty());
        assert_eq!(config.hotkey, DEFAULT_HOTKEY);
        assert_eq!(config.sort_by, SortBy::LastLaunched);
    }

    fn ws(name: &str, created: &str, launched: Option<&str>) -> Workspace {
        Workspace {
            created_at: created.into(),
            last_launched: launched.map(String::from),
            ..Workspace::new(name, vec![])
        }
    }

    fn sample() -> Vec<Workspace> {
        vec![
            ws("beta", "2025-01-02T10:00:00+00:00", None),
            ws(
                "Alpha",
                "2025-01-01T10:00:00+00:00",
                Some("2025-02-01T10:00:00+00:00"),
            ),
            ws(
                "gamma",
                "2025-01-03T10:00:00+00:00",
                Some("2025-01-15T10:00:00+00:00"),
            ),
            ws("delta", "garbage", None),
        ]
    }

    #[test]
    fn sort_by_name() {
        assert_eq!(sorted_indices(&sample(), SortBy::Name), vec![1, 0, 3, 2]);
    }

    #[test]
    fn sort_by_created_newest_first() {
        assert_eq!(sorted_indices(&sample(), SortBy::Created), vec![2, 0, 1, 3]);
    }

    #[test]
    fn sort_by_last_launched() {
        assert_eq!(
            sorted_indices(&sample(), SortBy::LastLaunched),
            vec![1, 2, 0, 3]
        );
    }

    #[test]
    fn legacy_config_parses() {
        let cfg: WorkspacesConfig = toml::from_str("workspaces = []\n").unwrap();
        assert_eq!(cfg.hotkey, DEFAULT_HOTKEY);
        let cfg: WorkspacesConfig = toml::from_str("sort_by = \"Name\"\n").unwrap();
        assert_eq!(cfg.sort_by, SortBy::Name);
        let cfg: WorkspacesConfig = toml::from_str("sort_by = \"created\"\n").unwrap();
        assert_eq!(cfg.sort_by, SortBy::Created);
    }

    #[test]
    fn workspace_new_sets_timestamp() {
        let ws = Workspace::new("Test", vec![]);
        assert!(!ws.created_at.is_empty());
        assert!(ws.last_launched.is_none());
        assert!(ws.move_existing);
    }

    #[test]
    fn workspace_enabled_apps_filters() {
        let apps = vec![
            AppEntry {
                name: "Firefox".into(),
                wm_class: "firefox".into(),
                exec: "/usr/bin/firefox".into(),
                args: vec![],
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                monitor: "DP-1".into(),
                enabled: true,
                minimized: false,
            },
            AppEntry {
                name: "Code".into(),
                wm_class: "code".into(),
                exec: "/usr/bin/code".into(),
                args: vec![],
                x: 0,
                y: 0,
                width: 960,
                height: 1080,
                monitor: "DP-1".into(),
                enabled: false,
                minimized: false,
            },
        ];
        let ws = Workspace::new("Dev", apps);
        assert_eq!(ws.app_count(), 2);
        assert_eq!(ws.enabled_apps().count(), 1);
    }

    #[test]
    fn roundtrip_toml() {
        let config = WorkspacesConfig {
            hotkey: DEFAULT_HOTKEY.into(),
            sort_by: SortBy::Name,
            workspaces: vec![Workspace::new(
                "Dev",
                vec![AppEntry {
                    name: "Firefox".into(),
                    wm_class: "firefox".into(),
                    exec: "/usr/bin/firefox".into(),
                    args: vec!["--profile".into(), "dev".into()],
                    x: 100,
                    y: 200,
                    width: 1920,
                    height: 1080,
                    monitor: "DP-1".into(),
                    enabled: true,
                    minimized: false,
                }],
            )],
        };
        let toml_str = toml::to_string(&config).unwrap();
        let parsed: WorkspacesConfig = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.workspaces.len(), 1);
        assert_eq!(parsed.workspaces[0].name, "Dev");
        assert_eq!(parsed.workspaces[0].apps[0].args, vec!["--profile", "dev"]);
    }
}
