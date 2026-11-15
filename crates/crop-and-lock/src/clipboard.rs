//! Copy a PNG image to the clipboard using wl-copy (Wayland) or xclip (X11).

use anyhow::{Context, Result, bail};
use image::RgbaImage;
use std::io::Write;
use std::process::{Command, Stdio};
use tracing::{info, warn};

pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .context("failed to encode PNG")?;
    Ok(buf.into_inner())
}

/// Copy `img` to the clipboard as `image/png`. Tries the tool matching the
/// session first. Returns an error if no tool is available.
pub fn copy_png(img: &RgbaImage) -> Result<()> {
    let png = encode_png(img)?;

    let wl_copy: (&str, &[&str]) = ("wl-copy", &["--type", "image/png"]);
    let xclip: (&str, &[&str]) = (
        "xclip",
        &["-selection", "clipboard", "-t", "image/png", "-i"],
    );
    let tools = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        [wl_copy, xclip]
    } else {
        [xclip, wl_copy]
    };

    for (cmd, args) in tools {
        match pipe_to(cmd, args, &png) {
            Ok(()) => {
                info!("Copied cropped screenshot to clipboard via {cmd}");
                return Ok(());
            }
            Err(e) => warn!("{cmd}: {e:#}"),
        }
    }
    bail!("could not copy image to clipboard: install xclip (X11) or wl-clipboard (Wayland)")
}

fn pipe_to(cmd: &str, args: &[&str], data: &[u8]) -> Result<()> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("failed to run {cmd}"))?;
    {
        let mut stdin = child.stdin.take().context("no stdin")?;
        stdin
            .write_all(data)
            .context("failed to write image data")?;
    }
    // Both tools fork a background process that serves the selection; the
    // foreground process exits once stdin is consumed. Reap it off-thread.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
