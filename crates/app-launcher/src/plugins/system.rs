//! System commands plugin: lock, logout, shutdown, restart, suspend, hibernate.
//! Global with a low weight so applications rank above it.

use super::{LauncherResult, QueryCtx};
use crate::actions::{Action, SystemCommand};
use crate::config::PluginId;

struct Entry {
    keywords: &'static [&'static str],
    title: &'static str,
    subtitle: &'static str,
    icon: &'static str,
    cmd: SystemCommand,
}

const ENTRIES: &[Entry] = &[
    Entry {
        keywords: &["lock", "lock screen"],
        title: "Lock",
        subtitle: "Lock the session (loginctl lock-session)",
        icon: "system-lock-screen",
        cmd: SystemCommand::Lock,
    },
    Entry {
        keywords: &["logout", "log out", "sign out", "signout"],
        title: "Log out",
        subtitle: "End the session (loginctl terminate-session)",
        icon: "system-log-out",
        cmd: SystemCommand::Logout,
    },
    Entry {
        keywords: &["shutdown", "shut down", "power off", "poweroff"],
        title: "Shut down",
        subtitle: "Power off the computer (systemctl poweroff)",
        icon: "system-shutdown",
        cmd: SystemCommand::Shutdown,
    },
    Entry {
        keywords: &["restart", "reboot"],
        title: "Restart",
        subtitle: "Reboot the computer (systemctl reboot)",
        icon: "system-reboot",
        cmd: SystemCommand::Restart,
    },
    Entry {
        keywords: &["suspend", "sleep"],
        title: "Suspend",
        subtitle: "Suspend to RAM (systemctl suspend)",
        icon: "system-suspend",
        cmd: SystemCommand::Suspend,
    },
    Entry {
        keywords: &["hibernate"],
        title: "Hibernate",
        subtitle: "Suspend to disk (systemctl hibernate)",
        icon: "system-suspend-hibernate",
        cmd: SystemCommand::Hibernate,
    },
];

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let q = ctx.search.trim().to_lowercase();
    if q.len() < 2 && !ctx.by_keyword {
        return Vec::new();
    }
    ENTRIES
        .iter()
        .filter_map(|e| {
            let best = e
                .keywords
                .iter()
                .filter_map(|kw| {
                    if q.is_empty() {
                        Some(1.0)
                    } else if *kw == q {
                        Some(30.0)
                    } else if kw.starts_with(&q) {
                        Some(20.0)
                    } else {
                        None
                    }
                })
                .fold(None, |m: Option<f64>, s| Some(m.map_or(s, |m| m.max(s))))?;
            Some(
                LauncherResult::new(PluginId::System, e.title, Action::System(e.cmd))
                    .subtitle(e.subtitle)
                    .icon(Some(e.icon.into()))
                    .score(best)
                    .key(format!("system:{}", e.title)),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppLauncherConfig;

    fn run(q: &str) -> Vec<LauncherResult> {
        let cfg = AppLauncherConfig::default();
        search(&QueryCtx {
            search: q,
            by_keyword: false,
            config: &cfg,
        })
    }

    #[test]
    fn matches_prefix() {
        let r = run("shut");
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].action, Action::System(SystemCommand::Shutdown));
        assert!(r[0].score < 50.0, "system results must rank below apps");
        assert_eq!(
            run("reboot")[0].action,
            Action::System(SystemCommand::Restart)
        );
        assert!(run("l").is_empty());
        assert!(run("firefox").is_empty());
    }
}
