//! `mpt-crop-and-lock --mode thumbnail|reparent|screenshot --window <xid>`
//!
//! Each invocation handles one cropped window, so several can be open at
//! once (as upstream). Only the selection step is single-instance.

use mpt_crop_and_lock::capture::WindowCapturer;
use mpt_crop_and_lock::gui::{cropped, selection};
use mpt_crop_and_lock::x11::{self, TITLE_SUFFIX};
use mpt_crop_and_lock::{CropMode, clipboard, parse_cli_args, reparent};
use std::fs;
use std::path::PathBuf;
use tracing::{error, info, warn};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_crop_and_lock=info".into()),
        )
        .init();

    let args = match parse_cli_args(std::env::args().skip(1)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e:#}");
            eprintln!(
                "usage: mpt-crop-and-lock --mode thumbnail|reparent|screenshot --window <xid>"
            );
            std::process::exit(2);
        }
    };
    if let Err(e) = run(args.mode, args.window) {
        error!("Crop And Lock failed: {e:#}");
        std::process::exit(1);
    }
}

fn run(mode: CropMode, window: u32) -> anyhow::Result<()> {
    let (conn, _) = x11::connect()?;
    if x11::is_own_window(&conn, window) {
        info!("Target is already a cropped window, nothing to do");
        return Ok(());
    }
    let title = format!("{}{TITLE_SUFFIX}", x11::window_title(&conn, window));
    let geom = x11::root_geometry(&conn, window)?;
    drop(conn);

    let capturer = WindowCapturer::new(window)?;
    let full = capturer.capture(None)?;

    let rect = {
        let Some(_lock) = SelectionLock::acquire() else {
            info!("Another region selection is in progress, exiting");
            return Ok(());
        };
        selection::run_selection(full.clone(), geom)
    };
    let Some(rect) = rect else {
        info!("Selection cancelled");
        return Ok(());
    };
    info!(
        "Selected {}x{} at {},{} (mode={})",
        rect.width,
        rect.height,
        rect.x,
        rect.y,
        mode.as_str()
    );

    match mode {
        CropMode::Thumbnail => cropped::run_thumbnail(capturer, rect, &title),
        CropMode::Screenshot => {
            // Re-capture so the overlay itself is never in the shot.
            let img = capturer
                .capture(Some(rect))
                .or_else(|_| crop_from(&full, rect))?;
            drop(capturer);
            if let Err(e) = clipboard::copy_png(&img) {
                warn!("{e:#}");
            }
            cropped::run_snapshot(img, rect, &title, window);
        }
        CropMode::Reparent => {
            // Undo the Composite redirection before moving the window.
            drop(capturer);
            match reparent::run(window, rect, &title) {
                Ok(end) => info!("Reparent session ended: {end:?}"),
                Err(e) => {
                    warn!("Reparenting failed ({e:#}); falling back to thumbnail mode");
                    cropped::run_thumbnail(WindowCapturer::new(window)?, rect, &title);
                }
            }
        }
    }
    Ok(())
}

fn crop_from(
    full: &image::RgbaImage,
    rect: mpt_crop_and_lock::geometry::CropRect,
) -> anyhow::Result<image::RgbaImage> {
    let r = rect
        .clamp_to(full.width(), full.height())
        .ok_or_else(|| anyhow::anyhow!("crop region outside captured image"))?;
    Ok(image::imageops::crop_imm(full, r.x, r.y, r.width, r.height).to_image())
}

/// PID lock held while the selection overlay is open, so two hotkey presses
/// do not stack overlays (upstream resets its single overlay instead).
struct SelectionLock(PathBuf);

impl SelectionLock {
    fn acquire() -> Option<Self> {
        let path = std::env::temp_dir().join("mpt-crop-and-lock-select.lock");
        if let Ok(pid_str) = fs::read_to_string(&path)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
            && pid != std::process::id()
            && std::path::Path::new(&format!("/proc/{pid}")).exists()
        {
            return None;
        }
        let _ = fs::write(&path, std::process::id().to_string());
        Some(Self(path))
    }
}

impl Drop for SelectionLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
