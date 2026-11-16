//! Execution of side-effect actions (launch app, open URI, run command).

use std::path::PathBuf;
use std::process::{Command, Stdio};

use tracing::{info, warn};

use crate::engine::Action;

/// Expand a leading `~/` to the home directory.
fn expand_home(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(p)
}

/// Build the command for an action without spawning it.
pub fn build_command(action: &Action) -> Command {
    match action {
        // The model documents `RunCommand` as "Execute a shell command", so the
        // user-authored string is the shell program itself (nothing is
        // interpolated into it).
        Action::RunCommand(cmd) => {
            let mut c = Command::new("sh");
            c.arg("-c").arg(cmd);
            c
        }
        Action::LaunchApp {
            path,
            args,
            working_dir,
            elevated,
        } => {
            let program = expand_home(path);
            let mut c = if *elevated {
                let mut c = Command::new("pkexec");
                c.arg(&program);
                c
            } else {
                Command::new(&program)
            };
            c.args(args);
            if let Some(dir) = working_dir.as_deref().filter(|d| !d.trim().is_empty()) {
                c.current_dir(expand_home(dir));
            }
            c
        }
        Action::OpenUri(uri) => {
            let mut c = Command::new("xdg-open");
            c.arg(uri);
            c
        }
    }
}

/// Spawn the action detached; the child is reaped on a helper thread.
pub fn run(action: &Action) {
    let mut cmd = build_command(action);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match cmd.spawn() {
        Ok(mut child) => {
            info!("Key Manager: started {:?}", cmd.get_program());
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(e) => warn!("Key Manager: failed to run {:?}: {e}", cmd.get_program()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_app_args_are_separate() {
        let c = build_command(&Action::LaunchApp {
            path: "/usr/bin/foo".into(),
            args: vec!["a b".into(), "; rm".into()],
            working_dir: Some("/tmp".into()),
            elevated: false,
        });
        assert_eq!(c.get_program(), "/usr/bin/foo");
        let args: Vec<_> = c.get_args().collect();
        assert_eq!(args, vec!["a b", "; rm"]);
        assert_eq!(c.get_current_dir(), Some(std::path::Path::new("/tmp")));
    }

    #[test]
    fn elevated_uses_pkexec() {
        let c = build_command(&Action::LaunchApp {
            path: "gparted".into(),
            args: vec![],
            working_dir: None,
            elevated: true,
        });
        assert_eq!(c.get_program(), "pkexec");
        assert_eq!(c.get_args().collect::<Vec<_>>(), vec!["gparted"]);
    }

    #[test]
    fn open_uri_uses_xdg_open() {
        let c = build_command(&Action::OpenUri("https://x.y/?a=1&b=2".into()));
        assert_eq!(c.get_program(), "xdg-open");
        assert_eq!(
            c.get_args().collect::<Vec<_>>(),
            vec!["https://x.y/?a=1&b=2"]
        );
    }

    #[test]
    fn run_command_uses_shell() {
        let c = build_command(&Action::RunCommand("echo hi".into()));
        assert_eq!(c.get_program(), "sh");
        assert_eq!(c.get_args().collect::<Vec<_>>(), vec!["-c", "echo hi"]);
    }
}
