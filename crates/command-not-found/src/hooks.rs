//! Shell integration: a clearly marked block in `~/.bashrc` / `~/.zshrc` and a
//! fish function file that call `mpt-command-not-found`.
//!
//! The Linux equivalent of upstream installing its PowerShell 7 module.
//! Installing is idempotent (the block between the marker lines is replaced),
//! uninstalling removes only the marked block.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use tracing::{info, warn};

use crate::config::CommandNotFoundConfig;
use crate::lookup::find_in_path;

pub const BEGIN_MARKER: &str = "# >>> my-power-toys command-not-found >>>";
pub const END_MARKER: &str = "# <<< my-power-toys command-not-found <<<";
pub const BIN_NAME: &str = "mpt-command-not-found";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

/// Quote for POSIX shells (bash/zsh): `'...'` with `'` -> `'\''`.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Quote for fish: `'...'` with `\` and `'` backslash-escaped.
fn fish_quote(s: &str) -> String {
    format!("'{}'", s.replace('\\', r"\\").replace('\'', r"\'"))
}

/// The marked block (including marker lines and a trailing newline).
pub fn hook_block(shell: Shell, bin: &Path) -> String {
    let bin = bin.to_string_lossy();
    let body = match shell {
        Shell::Bash => {
            let q = sh_quote(&bin);
            format!(
                "command_not_found_handle() {{\n    \
                 if [ -x {q} ]; then\n        \
                 {q} -- \"$1\"\n    \
                 else\n        \
                 printf 'bash: %s: command not found\\n' \"$1\" >&2\n    \
                 fi\n    \
                 return 127\n\
                 }}\n"
            )
        }
        Shell::Zsh => {
            let q = sh_quote(&bin);
            format!(
                "command_not_found_handler() {{\n    \
                 if [ -x {q} ]; then\n        \
                 {q} -- \"$1\"\n    \
                 else\n        \
                 printf 'zsh: command not found: %s\\n' \"$1\" >&2\n    \
                 fi\n    \
                 return 127\n\
                 }}\n"
            )
        }
        Shell::Fish => {
            let q = fish_quote(&bin);
            format!(
                "function fish_command_not_found\n    \
                 if test -x {q}\n        \
                 {q} -- $argv[1]\n    \
                 else\n        \
                 __fish_default_command_not_found_handler $argv\n    \
                 end\n    \
                 return 127\n\
                 end\n"
            )
        }
    };
    format!("{BEGIN_MARKER}\n{body}{END_MARKER}\n")
}

/// Byte range of the marked block (from the begin marker line to the end of
/// the end marker line, including its newline).
fn block_range(content: &str) -> Option<(usize, usize)> {
    let mut offset = 0;
    let mut start = None;
    for line in content.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);
        if start.is_none() && trimmed == BEGIN_MARKER {
            start = Some(offset);
        } else if let Some(s) = start
            && trimmed == END_MARKER
        {
            return Some((s, offset + line.len()));
        }
        offset += line.len();
    }
    None
}

/// Insert `block` into `content`, replacing an existing marked block if any.
pub fn upsert_block(content: &str, block: &str) -> String {
    if let Some((s, e)) = block_range(content) {
        let mut out = String::with_capacity(content.len() + block.len());
        out.push_str(&content[..s]);
        out.push_str(block);
        out.push_str(&content[e..]);
        return out;
    }
    let mut out = content.to_string();
    if !out.is_empty() {
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(block);
    out
}

/// Remove the marked block. Returns `None` if no block is present.
/// Also removes the blank separator line that [`upsert_block`] added.
pub fn remove_block(content: &str) -> Option<String> {
    let (s, e) = block_range(content)?;
    let mut before = content[..s].to_string();
    let after = &content[e..];
    if before.ends_with("\n\n") {
        before.pop();
    }
    if after.is_empty() && before == "\n" {
        before.clear();
    }
    Some(before + after)
}

/// Files the hooks are written to.
#[derive(Debug, Clone)]
pub struct HookTargets {
    pub bashrc: PathBuf,
    pub zshrc: PathBuf,
    /// Fish config directory (`~/.config/fish`).
    pub fish_dir: PathBuf,
}

impl HookTargets {
    /// Default locations for the current user (`$ZDOTDIR` respected for zsh).
    pub fn for_current_user() -> Result<Self> {
        let home = dirs::home_dir().context("could not determine home directory")?;
        let zdot = std::env::var_os("ZDOTDIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.clone());
        let config = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        Ok(Self {
            bashrc: home.join(".bashrc"),
            zshrc: zdot.join(".zshrc"),
            fish_dir: config.join("fish"),
        })
    }

    pub fn fish_function(&self) -> PathBuf {
        self.fish_dir
            .join("functions")
            .join("fish_command_not_found.fish")
    }
}

/// Locate `mpt-command-not-found` as an absolute path: next to the current
/// executable first, then on `$PATH`.
pub fn find_binary() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe() {
        if exe.file_name().is_some_and(|n| n == BIN_NAME) {
            return Some(exe);
        }
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(BIN_NAME);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    find_in_path(BIN_NAME).and_then(|p| std::fs::canonicalize(&p).ok().or(Some(p)))
}

/// Write `content` if it differs from what is on disk. Writes in place so
/// symlinked dotfiles keep pointing at their target.
fn write_if_changed(path: &Path, old: &str, new: &str) -> Result<bool> {
    if old == new {
        return Ok(false);
    }
    std::fs::write(path, new).with_context(|| format!("writing {}", path.display()))?;
    Ok(true)
}

fn install_rc(path: &Path, shell: Shell, bin: &Path) -> Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }
    let old =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let new = upsert_block(&old, &hook_block(shell, bin));
    Ok(write_if_changed(path, &old, &new)?.then(|| path.to_path_buf()))
}

/// Install hooks into the given targets. Returns the files that changed.
pub fn install_hooks_at(
    targets: &HookTargets,
    bin: &Path,
    cfg: &CommandNotFoundConfig,
) -> Result<Vec<PathBuf>> {
    if !bin.is_absolute() {
        bail!("{} path must be absolute: {}", BIN_NAME, bin.display());
    }
    let mut changed = Vec::new();
    if cfg.bash {
        changed.extend(install_rc(&targets.bashrc, Shell::Bash, bin)?);
    }
    if cfg.zsh {
        changed.extend(install_rc(&targets.zshrc, Shell::Zsh, bin)?);
    }
    if cfg.fish && targets.fish_dir.is_dir() {
        let f = targets.fish_function();
        let old = std::fs::read_to_string(&f).unwrap_or_default();
        if !old.is_empty() && block_range(&old).is_none() {
            warn!(
                "{} exists and is not managed by my-power-toys; leaving it alone",
                f.display()
            );
        } else {
            let new = upsert_block("", &hook_block(Shell::Fish, bin));
            if old != new {
                if let Some(dir) = f.parent() {
                    std::fs::create_dir_all(dir)
                        .with_context(|| format!("creating {}", dir.display()))?;
                }
                std::fs::write(&f, &new).with_context(|| format!("writing {}", f.display()))?;
                changed.push(f);
            }
        }
    }
    Ok(changed)
}

/// Remove the marked blocks from the given targets. Returns changed files.
pub fn uninstall_hooks_at(targets: &HookTargets) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    for rc in [&targets.bashrc, &targets.zshrc] {
        if let Ok(old) = std::fs::read_to_string(rc)
            && let Some(new) = remove_block(&old)
        {
            std::fs::write(rc, new).with_context(|| format!("writing {}", rc.display()))?;
            changed.push(rc.clone());
        }
    }
    let f = targets.fish_function();
    if let Ok(old) = std::fs::read_to_string(&f)
        && let Some(new) = remove_block(&old)
    {
        if new.trim().is_empty() {
            std::fs::remove_file(&f).with_context(|| format!("removing {}", f.display()))?;
        } else {
            std::fs::write(&f, new).with_context(|| format!("writing {}", f.display()))?;
        }
        changed.push(f);
    }
    Ok(changed)
}

/// Install the shell hooks for the current user according to `cfg`
/// (per-shell toggles). Only existing rc files are modified; the fish function
/// is created only if the fish config directory exists.
pub fn install_shell_hooks(cfg: &CommandNotFoundConfig) -> Result<Vec<PathBuf>> {
    let bin = find_binary().with_context(|| format!("{BIN_NAME} binary not found"))?;
    let changed = install_hooks_at(&HookTargets::for_current_user()?, &bin, cfg)?;
    for f in &changed {
        info!("Installed command-not-found hook in {}", f.display());
    }
    Ok(changed)
}

/// Remove the shell hooks (only the marked blocks) for the current user.
pub fn uninstall_shell_hooks() -> Result<Vec<PathBuf>> {
    let changed = uninstall_hooks_at(&HookTargets::for_current_user()?)?;
    for f in &changed {
        info!("Removed command-not-found hook from {}", f.display());
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIN: &str = "/usr/bin/mpt-command-not-found";

    #[test]
    fn upsert_is_idempotent_and_remove_restores() {
        let block = hook_block(Shell::Bash, Path::new(BIN));
        for original in ["", "export A=1\n", "export A=1", "alias x=y\n\n# end\n"] {
            let once = upsert_block(original, &block);
            let twice = upsert_block(&once, &block);
            assert_eq!(once, twice, "not idempotent for {original:?}");
            assert_eq!(once.matches(BEGIN_MARKER).count(), 1);
            let removed = remove_block(&once).unwrap();
            let expected = if original.is_empty() || original.ends_with('\n') {
                original.to_string()
            } else {
                format!("{original}\n")
            };
            assert_eq!(removed, expected);
            assert!(remove_block(&removed).is_none());
        }
    }

    #[test]
    fn upsert_replaces_existing_block_in_place() {
        let old = hook_block(Shell::Zsh, Path::new("/old/path"));
        let content = format!("before\n\n{old}after\n");
        let new = hook_block(Shell::Zsh, Path::new(BIN));
        let updated = upsert_block(&content, &new);
        assert_eq!(updated, format!("before\n\n{new}after\n"));
        assert!(!updated.contains("/old/path"));
        assert_eq!(remove_block(&updated).unwrap(), "before\nafter\n");
    }

    #[test]
    fn remove_ignores_unterminated_block() {
        let content = format!("x\n{BEGIN_MARKER}\nfoo\n");
        assert!(remove_block(&content).is_none());
    }

    #[test]
    fn blocks_contain_handlers() {
        let b = hook_block(Shell::Bash, Path::new(BIN));
        assert!(b.contains("command_not_found_handle()"));
        assert!(b.contains("return 127"));
        assert!(b.contains("'/usr/bin/mpt-command-not-found' -- \"$1\""));
        let z = hook_block(Shell::Zsh, Path::new(BIN));
        assert!(z.contains("command_not_found_handler()"));
        let f = hook_block(Shell::Fish, Path::new(BIN));
        assert!(f.contains("function fish_command_not_found"));
        assert!(f.contains("return 127"));
    }

    #[test]
    fn quoting() {
        assert_eq!(sh_quote("/a b/it's"), r"'/a b/it'\''s'");
        assert_eq!(fish_quote(r"/a\b/it's"), r"'/a\\b/it\'s'");
    }

    #[test]
    fn install_uninstall_files() {
        let dir = std::env::temp_dir().join(format!("mpt-cnf-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("fish")).unwrap();
        std::fs::write(dir.join("bashrc"), "export A=1\n").unwrap();
        // zshrc does not exist -> must not be created
        let t = HookTargets {
            bashrc: dir.join("bashrc"),
            zshrc: dir.join("zshrc"),
            fish_dir: dir.join("fish"),
        };
        let cfg = CommandNotFoundConfig::default();
        let changed = install_hooks_at(&t, Path::new(BIN), &cfg).unwrap();
        assert_eq!(changed, vec![t.bashrc.clone(), t.fish_function()]);
        assert!(!t.zshrc.exists());
        assert!(
            install_hooks_at(&t, Path::new(BIN), &cfg)
                .unwrap()
                .is_empty()
        );

        let changed = uninstall_hooks_at(&t).unwrap();
        assert_eq!(changed.len(), 2);
        assert_eq!(std::fs::read_to_string(&t.bashrc).unwrap(), "export A=1\n");
        assert!(!t.fish_function().exists());
        assert!(install_hooks_at(&t, Path::new("relative"), &cfg).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
