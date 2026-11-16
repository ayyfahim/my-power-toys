//! `mpt-command-not-found <command>`: print packages that provide `<command>`.
//!
//! Called by the shell hooks; always exits with status 127.
//! Also: `--install-hooks`, `--uninstall-hooks`, `--help`.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use mpt_command_not_found::lookup::{is_valid_command_name, lookup};
use mpt_command_not_found::{install_shell_hooks, load_config, uninstall_shell_hooks};

const USAGE: &str = "usage: mpt-command-not-found [--] <command>\n       \
                     mpt-command-not-found --install-hooks | --uninstall-hooks";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = match args.first().map(String::as_str) {
        Some("--") => args.get(1).cloned(),
        Some("-h" | "--help") => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Some("--install-hooks") => return hooks_result(install_shell_hooks(&load_config())),
        Some("--uninstall-hooks") => return hooks_result(uninstall_shell_hooks()),
        other => other.map(str::to_string),
    };
    let Some(cmd) = cmd else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };

    // The name is passed to external tools: reject anything unusual without
    // doing any lookup.
    if !is_valid_command_name(&cmd) {
        return ExitCode::from(127);
    }

    let cfg = load_config();
    let res = lookup(
        &cmd,
        Duration::from_millis(cfg.lookup_timeout_ms.max(100)),
        cfg.fuzzy_suggestions,
    );

    let mut err = std::io::stderr().lock();
    let _ = writeln!(err, "{cmd}: command not found");
    if !res.packages.is_empty() {
        let _ = writeln!(err, "\nThe command '{cmd}' can be installed with:");
        for s in &res.packages {
            let _ = writeln!(err, "  {}", s.install);
        }
    }
    if !res.similar.is_empty() {
        let _ = writeln!(err, "\nDid you mean:");
        for s in &res.similar {
            let _ = writeln!(err, "  {s}");
        }
    }
    ExitCode::from(127)
}

fn hooks_result(r: anyhow::Result<Vec<std::path::PathBuf>>) -> ExitCode {
    match r {
        Ok(changed) if changed.is_empty() => {
            println!("No changes.");
            ExitCode::SUCCESS
        }
        Ok(changed) => {
            for f in changed {
                println!("Updated {}", f.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
