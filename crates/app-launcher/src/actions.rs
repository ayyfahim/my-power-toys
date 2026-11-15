//! Result actions and how they are executed.
//!
//! Every external program is started with explicit argv (no shell string
//! interpolation). The only exception is the Shell plugin, whose purpose is
//! running the user's command line through their shell; that command is passed
//! as a single `-c` argument.

use anyhow::{Context, Result, bail};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemCommand {
    Lock,
    Logout,
    Shutdown,
    Restart,
    Suspend,
    Hibernate,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Do nothing (hint rows).
    None,
    /// Launch a desktop entry `Exec` line (field codes already stripped).
    Launch {
        exec: String,
        terminal: bool,
    },
    /// Run a command line through the user's shell.
    RunCommand {
        command: String,
        in_terminal: bool,
        keep_open: bool,
    },
    /// Open a file, directory or URL with `xdg-open`.
    Open(String),
    /// Open the parent directory of a path in the file manager.
    OpenContainingFolder(PathBuf),
    /// Open a terminal in a directory.
    OpenTerminalAt(PathBuf),
    /// Copy text to the clipboard.
    Copy(String),
    /// Activate an X11 window.
    ActivateWindow(u32),
    /// Close an X11 window (`_NET_CLOSE_WINDOW`).
    CloseWindow(u32),
    System(SystemCommand),
    /// Replace the search text (keeps the launcher open).
    SetQuery(String),
}

impl Action {
    /// Whether the launcher should stay open after running this action.
    pub fn keeps_open(&self) -> bool {
        matches!(self, Action::SetQuery(_) | Action::None)
    }
}

/// Settings needed to execute actions.
#[derive(Debug, Clone, Default)]
pub struct ExecEnv {
    /// Preferred terminal (empty = auto-detect).
    pub terminal: String,
}

pub fn execute(action: &Action, env: &ExecEnv) -> Result<()> {
    match action {
        Action::None | Action::SetQuery(_) => Ok(()),
        Action::Launch { exec, terminal } => {
            let argv = split_exec(exec);
            if argv.is_empty() {
                bail!("empty Exec line");
            }
            if *terminal {
                spawn_in_terminal(env, &argv, None)
            } else {
                spawn_detached(&argv, None)
            }
        }
        Action::RunCommand {
            command,
            in_terminal,
            keep_open,
        } => {
            if command.trim().is_empty() {
                return Ok(());
            }
            let shell = user_shell();
            if *in_terminal {
                let argv = shell_argv(&shell, command, *keep_open);
                spawn_in_terminal(env, &argv, None)
            } else {
                spawn_detached(&[shell, "-c".into(), command.clone()], None)
            }
        }
        Action::Open(target) => {
            if target.is_empty() {
                return Ok(());
            }
            spawn_detached(&["xdg-open".into(), target.clone()], None)
        }
        Action::OpenContainingFolder(path) => {
            let dir = path.parent().unwrap_or(Path::new("/"));
            spawn_detached(
                &["xdg-open".into(), dir.to_string_lossy().into_owned()],
                None,
            )
        }
        Action::OpenTerminalAt(dir) => {
            let term = find_terminal(env).context("no terminal emulator found")?;
            spawn_detached(&[term], Some(dir))
        }
        Action::Copy(text) => copy_to_clipboard(text),
        Action::ActivateWindow(id) => crate::x11::X11::connect()?.activate(*id),
        Action::CloseWindow(id) => crate::x11::X11::connect()?.close(*id),
        Action::System(cmd) => {
            let argv = system_argv(*cmd);
            info!("App Launcher: system command {argv:?}");
            spawn_detached(&argv, None)
        }
    }
}

fn spawn_detached(argv: &[String], cwd: Option<&Path>) -> Result<()> {
    let (prog, args) = argv.split_first().context("empty command")?;
    let mut cmd = Command::new(prog);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // New process group so the child survives the launcher exiting.
        .process_group(0);
    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    } else if let Some(home) = dirs::home_dir() {
        cmd.current_dir(home);
    }
    cmd.spawn()
        .with_context(|| format!("failed to start {prog}"))?;
    Ok(())
}

fn spawn_in_terminal(env: &ExecEnv, argv: &[String], cwd: Option<&Path>) -> Result<()> {
    let term = find_terminal(env).context("no terminal emulator found")?;
    spawn_detached(&terminal_argv(&term, argv), cwd)
}

fn user_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into())
}

/// argv that runs `command` in `shell`, optionally leaving an interactive
/// shell open afterwards. The command is passed as a positional argument,
/// never spliced into the script.
pub fn shell_argv(shell: &str, command: &str, keep_open: bool) -> Vec<String> {
    if keep_open {
        vec![
            "/bin/sh".into(),
            "-c".into(),
            r#""$0" -c "$1"; exec "$0""#.into(),
            shell.into(),
            command.into(),
        ]
    } else {
        vec![shell.into(), "-c".into(), command.into()]
    }
}

const TERMINALS: &[&str] = &[
    "x-terminal-emulator",
    "gnome-terminal",
    "konsole",
    "xfce4-terminal",
    "kitty",
    "alacritty",
    "wezterm",
    "foot",
    "tilix",
    "terminator",
    "xterm",
];

fn find_terminal(env: &ExecEnv) -> Option<String> {
    if !env.terminal.trim().is_empty() {
        return Some(env.terminal.trim().to_string());
    }
    if let Ok(t) = std::env::var("TERMINAL")
        && !t.is_empty()
        && which(&t).is_some()
    {
        return Some(t);
    }
    TERMINALS
        .iter()
        .find(|t| which(t).is_some())
        .map(|t| t.to_string())
}

/// Locate an executable on PATH (or accept an existing absolute path).
pub fn which(prog: &str) -> Option<PathBuf> {
    if prog.contains('/') {
        let p = PathBuf::from(prog);
        return p.is_file().then_some(p);
    }
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|d| d.join(prog))
            .find(|p| p.is_file())
    })
}

/// Build the argv that makes `term` execute `argv`.
pub fn terminal_argv(term: &str, argv: &[String]) -> Vec<String> {
    let name = Path::new(term)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut out = vec![term.to_string()];
    match name.as_str() {
        "gnome-terminal" | "kgx" | "tilix" | "ptyxis" => out.push("--".into()),
        "xfce4-terminal" | "terminator" => out.push("-x".into()),
        "wezterm" => {
            out.push("start".into());
            out.push("--".into());
        }
        "kitty" | "foot" => {}
        _ => out.push("-e".into()),
    }
    out.extend(argv.iter().cloned());
    out
}

/// Split a desktop-entry `Exec` value into argv, honouring double quotes
/// and backslash escapes (Desktop Entry Spec, "The Exec key").
pub fn split_exec(exec: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    let mut chars = exec.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true;
            }
            '\\' if in_quotes => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    out.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

pub fn system_argv(cmd: SystemCommand) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let session = std::env::var("XDG_SESSION_ID")
        .ok()
        .filter(|s| !s.is_empty());
    match cmd {
        SystemCommand::Lock => match session {
            Some(id) => v(&["loginctl", "lock-session", &id]),
            None => v(&["loginctl", "lock-session"]),
        },
        SystemCommand::Logout => match session {
            Some(id) => v(&["loginctl", "terminate-session", &id]),
            None => {
                let user = std::env::var("USER").unwrap_or_default();
                v(&["loginctl", "terminate-user", &user])
            }
        },
        SystemCommand::Shutdown => v(&["systemctl", "poweroff"]),
        SystemCommand::Restart => v(&["systemctl", "reboot"]),
        SystemCommand::Suspend => v(&["systemctl", "suspend"]),
        SystemCommand::Hibernate => v(&["systemctl", "hibernate"]),
    }
}

/// Copy text via wl-copy (Wayland) or xclip/xsel (X11).
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    use std::io::Write;
    let candidates: &[&[&str]] = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        &[
            &["wl-copy"],
            &["xclip", "-selection", "clipboard"],
            &["xsel", "-ib"],
        ]
    } else {
        &[
            &["xclip", "-selection", "clipboard"],
            &["xsel", "-ib"],
            &["wl-copy"],
        ]
    };
    for argv in candidates {
        let Ok(mut child) = Command::new(argv[0])
            .args(&argv[1..])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        }
        // xclip/xsel fork to serve the selection; wait only for the parent.
        let _ = child.wait();
        return Ok(());
    }
    warn!("No clipboard tool found (wl-copy, xclip, xsel)");
    bail!("no clipboard tool available (install wl-clipboard, xclip or xsel)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_exec_quotes() {
        assert_eq!(
            split_exec("firefox --new-window"),
            vec!["firefox", "--new-window"]
        );
        assert_eq!(
            split_exec(r#""/opt/My App/app" --flag "a \"b\"""#),
            vec!["/opt/My App/app", "--flag", r#"a "b""#]
        );
        assert_eq!(split_exec(r#"sh -c """#), vec!["sh", "-c", ""]);
        assert!(split_exec("   ").is_empty());
    }

    #[test]
    fn terminal_argv_flags() {
        let cmd = vec!["htop".to_string()];
        assert_eq!(terminal_argv("xterm", &cmd), vec!["xterm", "-e", "htop"]);
        assert_eq!(
            terminal_argv("/usr/bin/gnome-terminal", &cmd),
            vec!["/usr/bin/gnome-terminal", "--", "htop"]
        );
        assert_eq!(terminal_argv("kitty", &cmd), vec!["kitty", "htop"]);
        assert_eq!(
            terminal_argv("wezterm", &cmd),
            vec!["wezterm", "start", "--", "htop"]
        );
    }

    #[test]
    fn shell_argv_does_not_splice_command() {
        let argv = shell_argv("/bin/bash", "echo $HOME; rm -i x", true);
        assert_eq!(argv[0], "/bin/sh");
        assert_eq!(argv[3], "/bin/bash");
        assert_eq!(argv[4], "echo $HOME; rm -i x");
        assert!(!argv[2].contains("echo"));
        assert_eq!(
            shell_argv("/bin/zsh", "ls", false),
            vec!["/bin/zsh", "-c", "ls"]
        );
    }

    #[test]
    fn system_commands() {
        assert_eq!(
            system_argv(SystemCommand::Shutdown),
            vec!["systemctl", "poweroff"]
        );
        assert_eq!(
            system_argv(SystemCommand::Restart),
            vec!["systemctl", "reboot"]
        );
        assert_eq!(system_argv(SystemCommand::Lock)[0], "loginctl");
    }

    #[test]
    fn keeps_open() {
        assert!(Action::SetQuery("x".into()).keeps_open());
        assert!(!Action::Copy("x".into()).keeps_open());
    }
}
