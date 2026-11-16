//! File locations, reading/writing of variable sources and the merged
//! ("applied") view.

use crate::envd::{self, EnvFile};
use crate::listvalue;
use crate::profiles::ProfileStore;
use anyhow::{Context, Result, bail};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{info, warn};

pub const USER_FILE_NAME: &str = "50-my-power-toys.conf";
pub const PROFILE_FILE_NAME: &str = "60-my-power-toys-profile.conf";
/// System file written via pkexec. It deliberately has a different basename
/// from the user file: systemd lets a file in `~/.config/environment.d` mask a
/// file with the same name in `/etc/environment.d`. "-system" sorts before
/// ".conf", so user variables still override system ones.
pub const SYSTEM_FILE_NAME: &str = "50-my-power-toys-system.conf";
pub const PROFILES_FILE_NAME: &str = "environment-variables-profiles.toml";

/// System-wide environment.d directories, highest priority first.
const SYSTEM_ENVD_DIRS: &[&str] = &[
    "/etc/environment.d",
    "/run/environment.d",
    "/usr/local/lib/environment.d",
    "/usr/lib/environment.d",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Scope {
    System,
    User,
    Profile,
    /// Only present in the running process environment.
    Session,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Scope::System => "System",
            Scope::User => "User",
            Scope::Profile => "Profile",
            Scope::Session => "Session",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Paths {
    pub user_envd_dir: PathBuf,
    pub user_file: PathBuf,
    pub profile_file: PathBuf,
    pub profiles_toml: PathBuf,
    pub etc_environment: PathBuf,
    pub system_envd_dirs: Vec<PathBuf>,
    pub system_file: PathBuf,
}

impl Paths {
    pub fn detect() -> Result<Self> {
        // dirs::config_dir honours $XDG_CONFIG_HOME, like systemd.
        let config = dirs::config_dir().context("could not determine config directory")?;
        let user_envd_dir = config.join("environment.d");
        let system_envd_dirs: Vec<PathBuf> = SYSTEM_ENVD_DIRS.iter().map(PathBuf::from).collect();
        Ok(Self {
            user_file: user_envd_dir.join(USER_FILE_NAME),
            profile_file: user_envd_dir.join(PROFILE_FILE_NAME),
            user_envd_dir,
            profiles_toml: mpt_common::config::config_dir()?.join(PROFILES_FILE_NAME),
            etc_environment: PathBuf::from("/etc/environment"),
            system_file: system_envd_dirs[0].join(SYSTEM_FILE_NAME),
            system_envd_dirs,
        })
    }
}

/// A parsed source file with its scope.
#[derive(Debug, Clone)]
pub struct Source {
    pub path: PathBuf,
    pub scope: Scope,
    /// True for files this module manages (and may write).
    pub managed: bool,
    /// pam_env's /etc/environment does not expand `$` references.
    pub expands: bool,
    pub vars: Vec<(String, String)>,
}

pub fn read_env_file(path: &Path) -> Result<EnvFile> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(EnvFile::parse(&s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(EnvFile::default()),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Write a file atomically (temp file + rename), creating parent dirs.
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let dir = path.parent().context("path has no parent")?;
    fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let tmp = dir.join(format!(
        ".{}.tmp-{}",
        path.file_name().and_then(|n| n.to_str()).unwrap_or("env"),
        std::process::id()
    ));
    fs::write(&tmp, content).with_context(|| format!("failed to write {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("failed to replace {}", path.display()))?;
    Ok(())
}

pub fn remove_file_if_exists(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("failed to remove {}", path.display())),
    }
}

pub fn load_profiles(path: &Path) -> Result<ProfileStore> {
    match fs::read_to_string(path) {
        Ok(s) => {
            ProfileStore::parse(&s).with_context(|| format!("failed to parse {}", path.display()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProfileStore::default()),
        Err(e) => Err(e).with_context(|| format!("failed to read {}", path.display())),
    }
}

pub fn save_profiles(path: &Path, store: &ProfileStore) -> Result<()> {
    write_atomic(path, &store.to_toml()?)
}

/// List `*.conf` files of environment.d directories in systemd order: sorted
/// by basename, a basename in an earlier (higher-priority) directory masking
/// the same basename in later ones. Returns (path, index of directory).
pub fn list_envd_files(dirs: &[PathBuf]) -> Vec<(PathBuf, usize)> {
    let mut by_name: BTreeMap<String, (PathBuf, usize)> = BTreeMap::new();
    for (idx, dir) in dirs.iter().enumerate() {
        let Ok(rd) = fs::read_dir(dir) else { continue };
        for entry in rd.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.ends_with(".conf") || name.starts_with('.') {
                continue;
            }
            by_name.entry(name).or_insert((entry.path(), idx));
        }
    }
    by_name.into_values().collect()
}

/// Collect every source in the order systemd applies them.
pub fn collect_sources(paths: &Paths) -> Vec<Source> {
    let mut sources = Vec::new();
    if let Ok(f) = read_env_file(&paths.etc_environment)
        && !f.vars().is_empty()
    {
        sources.push(Source {
            path: paths.etc_environment.clone(),
            scope: Scope::System,
            managed: false,
            expands: false,
            vars: f.vars(),
        });
    }
    let mut dirs = vec![paths.user_envd_dir.clone()];
    dirs.extend(paths.system_envd_dirs.iter().cloned());
    for (path, idx) in list_envd_files(&dirs) {
        let file = match read_env_file(&path) {
            Ok(f) => f,
            Err(e) => {
                warn!("Environment Variables: {e:#}");
                continue;
            }
        };
        let scope = if path == paths.profile_file {
            Scope::Profile
        } else if idx == 0 {
            Scope::User
        } else {
            Scope::System
        };
        let managed =
            path == paths.user_file || path == paths.profile_file || path == paths.system_file;
        sources.push(Source {
            path,
            scope,
            managed,
            expands: true,
            vars: file.vars(),
        });
    }
    sources
}

/// One row of the merged view.
#[derive(Debug, Clone)]
pub struct EffectiveVar {
    pub name: String,
    /// Value as written (references unexpanded).
    pub value: String,
    pub expanded: String,
    pub scope: Scope,
    pub path: Option<PathBuf>,
    /// Earlier definitions that this one overrides.
    pub overrides: Vec<(Scope, PathBuf)>,
}

/// Merge sources (in order) on top of `base` (usually the process
/// environment, used for expanding references). Variables only in `base`
/// are returned with [`Scope::Session`] when `include_session` is set.
pub fn merge(
    sources: &[Source],
    base: &BTreeMap<String, String>,
    include_session: bool,
) -> Vec<EffectiveVar> {
    let mut env = base.clone();
    let mut rows: BTreeMap<String, EffectiveVar> = BTreeMap::new();
    for src in sources {
        for (k, v) in &src.vars {
            let expanded = if src.expands {
                let snapshot = &env;
                envd::expand(v, &|n| snapshot.get(n).cloned())
            } else {
                v.clone()
            };
            let expanded = if listvalue::is_list_variable(k) {
                listvalue::dedup(&expanded)
            } else {
                expanded
            };
            env.insert(k.clone(), expanded.clone());
            let mut overrides = Vec::new();
            if let Some(prev) = rows.remove(k) {
                overrides = prev.overrides;
                if let Some(p) = prev.path {
                    overrides.push((prev.scope, p));
                }
            }
            rows.insert(
                k.clone(),
                EffectiveVar {
                    name: k.clone(),
                    value: v.clone(),
                    expanded,
                    scope: src.scope,
                    path: Some(src.path.clone()),
                    overrides,
                },
            );
        }
    }
    if include_session {
        for (k, v) in base {
            rows.entry(k.clone()).or_insert_with(|| EffectiveVar {
                name: k.clone(),
                value: v.clone(),
                expanded: v.clone(),
                scope: Scope::Session,
                path: None,
                overrides: Vec::new(),
            });
        }
    }
    rows.into_values().collect()
}

pub fn process_env() -> BTreeMap<String, String> {
    std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect()
}

/// Find an executable in $PATH.
pub fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(program))
        .find(|p| p.is_file())
}

/// Write the system-managed file. Tries a direct write first (e.g. when
/// running as root) and falls back to `pkexec install`.
pub fn write_system_file(path: &Path, content: &str) -> Result<()> {
    match write_atomic(path, content) {
        Ok(()) => return Ok(()),
        Err(e) => info!(
            "Direct write of {} failed ({e:#}), using pkexec",
            path.display()
        ),
    }
    let Some(pkexec) = which("pkexec") else {
        bail!(
            "pkexec is not installed; cannot write {} (install polkit or edit as root)",
            path.display()
        );
    };
    let tmp = std::env::temp_dir().join(format!("mpt-env-system-{}.conf", std::process::id()));
    fs::write(&tmp, content).with_context(|| format!("failed to write {}", tmp.display()))?;
    let status = Command::new(pkexec)
        .arg("install")
        .arg("-D")
        .arg("-m")
        .arg("0644")
        .arg(&tmp)
        .arg(path)
        .status();
    let _ = fs::remove_file(&tmp);
    let status = status.context("failed to run pkexec")?;
    if !status.success() {
        bail!("pkexec failed or was cancelled ({status})");
    }
    Ok(())
}

/// Names defined in the files this module manages.
pub fn managed_names(sources: &[Source]) -> HashSet<String> {
    sources
        .iter()
        .filter(|s| s.managed)
        .flat_map(|s| s.vars.iter().map(|(k, _)| k.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(scope: Scope, path: &str, vars: &[(&str, &str)]) -> Source {
        Source {
            path: PathBuf::from(path),
            scope,
            managed: scope != Scope::System,
            expands: true,
            vars: vars
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        }
    }

    #[test]
    fn merge_labels_and_overrides() {
        let base: BTreeMap<String, String> = [("HOME", "/h"), ("PATH", "/usr/bin"), ("TERM", "x")]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        let sources = vec![
            src(
                Scope::System,
                "/etc/environment.d/10.conf",
                &[("EDITOR", "vi")],
            ),
            src(
                Scope::User,
                "/u/50.conf",
                &[("EDITOR", "nano"), ("PATH", "$HOME/bin:$PATH")],
            ),
            src(Scope::Profile, "/u/60.conf", &[("EDITOR", "nvim")]),
        ];
        let rows = merge(&sources, &base, true);
        let get = |n: &str| rows.iter().find(|r| r.name == n).unwrap();
        assert_eq!(get("EDITOR").scope, Scope::Profile);
        assert_eq!(get("EDITOR").overrides.len(), 2);
        assert_eq!(get("PATH").expanded, "/h/bin:/usr/bin");
        assert_eq!(get("PATH").scope, Scope::User);
        assert_eq!(get("TERM").scope, Scope::Session);
        assert!(
            merge(&sources, &base, false)
                .iter()
                .all(|r| r.name != "TERM")
        );
    }

    #[test]
    fn merge_dedups_list_variables() {
        let base: BTreeMap<String, String> = [("PATH", "/h/bin:/usr/bin")]
            .into_iter()
            .map(|(k, v)| (k.into(), v.into()))
            .collect();
        let sources = vec![src(Scope::User, "/u", &[("PATH", "/h/bin:$PATH")])];
        let rows = merge(&sources, &base, false);
        assert_eq!(rows[0].expanded, "/h/bin:/usr/bin");
    }

    #[test]
    fn envd_listing_masks_by_basename() {
        let tmp = std::env::temp_dir().join(format!("mpt-envd-test-{}", std::process::id()));
        let a = tmp.join("a");
        let b = tmp.join("b");
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("20-x.conf"), "A=1\n").unwrap();
        fs::write(b.join("20-x.conf"), "A=2\n").unwrap();
        fs::write(b.join("10-y.conf"), "B=1\n").unwrap();
        fs::write(b.join("ignored.txt"), "C=1\n").unwrap();
        let files = list_envd_files(&[a.clone(), b.clone()]);
        assert_eq!(
            files,
            vec![(b.join("10-y.conf"), 1), (a.join("20-x.conf"), 0)]
        );
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn system_file_sorts_before_user_file() {
        assert!(SYSTEM_FILE_NAME < USER_FILE_NAME);
        assert!(USER_FILE_NAME < PROFILE_FILE_NAME);
    }
}
