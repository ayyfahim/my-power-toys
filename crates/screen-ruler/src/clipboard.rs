use anyhow::{Context, Result};
use std::io::Write;
use std::process::{Command, Stdio};

fn pipe_to(cmd: &str, args: &[&str], text: &str) -> Result<()> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to run {cmd}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    let status = child.wait()?;
    anyhow::ensure!(status.success(), "{cmd} exited with {status}");
    Ok(())
}

/// Copy text to the clipboard via wl-copy (Wayland) or xclip (X11).
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let wl = || pipe_to("wl-copy", &[], text);
    let x = || pipe_to("xclip", &["-selection", "clipboard", "-i"], text);
    let result = if wayland {
        wl().or_else(|_| x())
    } else {
        x().or_else(|_| wl())
    };
    result.context("could not copy to clipboard: install wl-copy (Wayland) or xclip (X11)")
}
