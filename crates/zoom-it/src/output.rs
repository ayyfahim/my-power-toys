//! Saving images to disk and copying them to the clipboard as PNG.

use crate::screen::{self, SessionType};
use anyhow::{Context, Result};
use image::RgbaImage;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::info;

pub fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut buf = std::io::Cursor::new(Vec::new());
    img.write_to(&mut buf, image::ImageFormat::Png)
        .context("encode PNG")?;
    Ok(buf.into_inner())
}

/// `<dir>/ZoomIt-<timestamp>.png`
pub fn timestamped_path(dir: &Path, ext: &str) -> PathBuf {
    let ts = chrono::Local::now().format("%Y-%m-%d_%H-%M-%S");
    dir.join(format!("ZoomIt-{ts}.{ext}"))
}

pub fn save_png(img: &RgbaImage, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    let path = timestamped_path(dir, "png");
    img.save_with_format(&path, image::ImageFormat::Png)
        .with_context(|| format!("save {}", path.display()))?;
    info!("Saved {}", path.display());
    Ok(path)
}

/// Copy a PNG to the clipboard via wl-copy (Wayland) or xclip (X11).
pub fn copy_png_to_clipboard(img: &RgbaImage) -> Result<()> {
    let png = encode_png(img)?;
    let attempts: &[(&str, &[&str])] = match screen::detect_session() {
        SessionType::Wayland => &[
            ("wl-copy", &["--type", "image/png"]),
            (
                "xclip",
                &["-selection", "clipboard", "-t", "image/png", "-i"],
            ),
        ],
        _ => &[
            (
                "xclip",
                &["-selection", "clipboard", "-t", "image/png", "-i"],
            ),
            ("wl-copy", &["--type", "image/png"]),
        ],
    };
    let mut last_err = None;
    for (cmd, args) in attempts {
        match pipe_to(cmd, args, &png) {
            Ok(()) => {
                info!("Copied image to clipboard via {cmd}");
                return Ok(());
            }
            Err(e) => last_err = Some(e),
        }
    }
    Err(last_err
        .unwrap_or_else(|| anyhow::anyhow!("no clipboard tool"))
        .context("copy to clipboard failed (install xclip or wl-clipboard)"))
}

fn pipe_to(cmd: &str, args: &[&str], data: &[u8]) -> Result<()> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("spawn {cmd}"))?;
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(data)
        .with_context(|| format!("write to {cmd}"))?;
    let status = child.wait()?;
    anyhow::ensure!(status.success(), "{cmd} exited with {status}");
    Ok(())
}
