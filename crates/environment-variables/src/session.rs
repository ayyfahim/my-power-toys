//! Push changes into the running session so newly started apps see them.
//!
//! environment.d files are only read when the systemd user manager starts
//! (i.e. at login), so after saving we also update the manager and the D-Bus
//! activation environment with explicit `KEY=VALUE` pairs. Shells and apps that
//! are already running keep their old environment; a log out/in may still be
//! needed for everything to pick the change up.

use crate::store::which;
use std::process::Command;
use tracing::{info, warn};

/// Variables that must never be unset from the session, even if they were
/// removed from our files (they normally come from login/PAM).
const PROTECTED: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "TERM",
    "DISPLAY",
    "WAYLAND_DISPLAY",
    "DBUS_SESSION_BUS_ADDRESS",
    "XAUTHORITY",
];

pub fn is_protected(name: &str) -> bool {
    PROTECTED.contains(&name) || name.starts_with("XDG_")
}

#[derive(Debug, Default, Clone)]
pub struct SessionReport {
    pub messages: Vec<String>,
    pub warnings: Vec<String>,
}

/// Build the argument list `KEY=VALUE` for the given pairs.
pub fn assignment_args(set: &[(String, String)]) -> Vec<String> {
    set.iter().map(|(k, v)| format!("{k}={v}")).collect()
}

/// Split `unset` into names to unset and names that are protected.
pub fn partition_unset(unset: &[String]) -> (Vec<String>, Vec<String>) {
    unset.iter().cloned().partition(|n| !is_protected(n))
}

/// Set and unset variables in the systemd user manager and D-Bus activation
/// environment. Missing tools produce warnings, not errors.
pub fn update_session(set: &[(String, String)], unset: &[String]) -> SessionReport {
    let mut report = SessionReport::default();
    let systemctl = which("systemctl");
    let dbus_update = which("dbus-update-activation-environment");

    if !set.is_empty() {
        let args = assignment_args(set);
        let mut done = false;
        if let Some(tool) = &dbus_update {
            // --systemd also updates the systemd user manager.
            done |= run(&mut report, Command::new(tool).arg("--systemd").args(&args));
        }
        if let Some(tool) = &systemctl {
            done |= run(
                &mut report,
                Command::new(tool)
                    .arg("--user")
                    .arg("set-environment")
                    .args(&args),
            );
        }
        if done {
            report
                .messages
                .push(format!("Updated {} variable(s) in the session.", set.len()));
        }
    }

    let (to_unset, protected) = partition_unset(unset);
    if !protected.is_empty() {
        report.warnings.push(format!(
            "Not unsetting {} in the running session; log out and back in to apply.",
            protected.join(", ")
        ));
    }
    if !to_unset.is_empty() {
        if let Some(tool) = &systemctl {
            if run(
                &mut report,
                Command::new(tool)
                    .arg("--user")
                    .arg("unset-environment")
                    .args(&to_unset),
            ) {
                report
                    .messages
                    .push(format!("Removed {} from the session.", to_unset.join(", ")));
            }
        } else {
            report
                .warnings
                .push("systemctl not found: removed variables stay set until you log out.".into());
        }
    }

    if systemctl.is_none() && dbus_update.is_none() && (!set.is_empty() || !unset.is_empty()) {
        report.warnings.push(
            "Neither systemctl nor dbus-update-activation-environment was found; \
             changes apply after you log out and back in."
                .into(),
        );
    }
    report.messages.push(
        "Already running apps and terminals keep their old environment. \
         Log out and back in if a change does not show up."
            .into(),
    );
    for w in &report.warnings {
        warn!("Environment Variables: {w}");
    }
    report
}

fn run(report: &mut SessionReport, cmd: &mut Command) -> bool {
    let program = cmd.get_program().to_string_lossy().to_string();
    match cmd.output() {
        Ok(out) if out.status.success() => {
            info!("Environment Variables: ran {program}");
            true
        }
        Ok(out) => {
            report.warnings.push(format!(
                "{program} failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ));
            false
        }
        Err(e) => {
            report
                .warnings
                .push(format!("Failed to run {program}: {e}"));
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_names_are_not_unset() {
        let (u, p) = partition_unset(&["FOO".into(), "PATH".into(), "XDG_DATA_DIRS".into()]);
        assert_eq!(u, vec!["FOO".to_string()]);
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn assignment_args_format() {
        assert_eq!(
            assignment_args(&[("A".into(), "x y".into())]),
            vec!["A=x y".to_string()]
        );
    }
}
