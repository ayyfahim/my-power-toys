//! Shell plugin (`>`): run a command in a terminal or detached.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::config::PluginId;

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let cmd = ctx.search.trim();
    if cmd.is_empty() {
        return vec![
            LauncherResult::new(PluginId::Shell, "Type a command to run", Action::None)
                .subtitle("Runs in your $SHELL"),
        ];
    }
    let in_term = ctx.config.shell_run_in_terminal;
    let keep_open = ctx.config.shell_keep_open;
    let run = |in_terminal| Action::RunCommand {
        command: cmd.to_string(),
        in_terminal,
        keep_open,
    };
    let (main_label, alt_label) = if in_term {
        ("Run in terminal", "Run without terminal")
    } else {
        ("Run without terminal", "Run in terminal")
    };
    vec![
        LauncherResult::new(PluginId::Shell, cmd, run(in_term))
            .subtitle(main_label)
            .icon(Some("utilities-terminal".into()))
            .score(300.0)
            .alt(alt_label, run(!in_term))
            .key(format!("shell:{cmd}")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppLauncherConfig;

    #[test]
    fn alt_flips_terminal_mode() {
        let cfg = AppLauncherConfig::default();
        let ctx = QueryCtx {
            search: "htop",
            by_keyword: true,
            config: &cfg,
        };
        let r = &search(&ctx)[0];
        assert!(matches!(
            r.action,
            Action::RunCommand {
                in_terminal: true,
                ..
            }
        ));
        assert!(matches!(
            r.alt_action,
            Some((
                _,
                Action::RunCommand {
                    in_terminal: false,
                    ..
                }
            ))
        ));
    }
}
