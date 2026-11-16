//! Screen capture of the monitor under the cursor.
//!
//! X11: direct `GetImage` on the root window (no external tools), falling back
//! to scrot/maim/gnome-screenshot. Wayland: grim (with geometry), then
//! gnome-screenshot/spectacle. Based on `crates/color-picker/src/screenshot.rs`.

use crate::screen::{self, MonitorGeom, SessionType};
use anyhow::{Context, Result};
use image::RgbaImage;
use std::path::Path;
use std::process::{Command, Stdio};
use tracing::{info, warn};

/// A frozen screenshot of one monitor.
pub struct MonitorCapture {
    pub image: RgbaImage,
    pub monitor: MonitorGeom,
    /// Cursor position relative to the monitor's top-left (in image pixels).
    pub cursor: Option<(f32, f32)>,
}

pub fn capture_monitor_under_cursor() -> Result<MonitorCapture> {
    let (monitor, cursor) = screen::monitor_under_cursor()?;
    info!("Capturing monitor {monitor:?}");
    let image = match screen::detect_session() {
        SessionType::Wayland => capture_wayland(&monitor)?,
        _ => match capture_x11_getimage(&monitor) {
            Ok(img) => img,
            Err(e) => {
                warn!("X11 GetImage failed ({e}), trying screenshot tools");
                crop_full(capture_x11_tools()?, &monitor)
            }
        },
    };
    // Tools may capture at a different scale (HiDPI); map cursor accordingly.
    let sx = image.width() as f32 / monitor.width.max(1) as f32;
    let sy = image.height() as f32 / monitor.height.max(1) as f32;
    let cursor =
        cursor.map(|(cx, cy)| ((cx - monitor.x) as f32 * sx, (cy - monitor.y) as f32 * sy));
    Ok(MonitorCapture {
        image,
        monitor,
        cursor,
    })
}

fn capture_x11_getimage(m: &MonitorGeom) -> Result<RgbaImage> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConnectionExt, ImageFormat, ImageOrder};

    let (conn, screen_num) = x11rb::connect(None).context("connect to X11")?;
    let setup = conn.setup();
    let screen = &setup.roots[screen_num];
    let depth = screen.root_depth;
    let bpp = setup
        .pixmap_formats
        .iter()
        .find(|f| f.depth == depth)
        .map(|f| f.bits_per_pixel)
        .unwrap_or(32);
    anyhow::ensure!(bpp == 32, "unsupported X11 pixmap format ({bpp} bpp)");
    let lsb = setup.image_byte_order == ImageOrder::LSB_FIRST;

    let (w, h) = (m.width.min(u16::MAX as u32), m.height.min(u16::MAX as u32));
    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            screen.root,
            m.x as i16,
            m.y as i16,
            w as u16,
            h as u16,
            !0,
        )?
        .reply()
        .context("GetImage")?;
    let data = reply.data;
    anyhow::ensure!(
        data.len() >= (w * h * 4) as usize,
        "short GetImage reply ({} bytes)",
        data.len()
    );
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in data.chunks_exact(4).take((w * h) as usize) {
        if lsb {
            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
        } else {
            rgba.extend_from_slice(&[px[1], px[2], px[3], 255]);
        }
    }
    RgbaImage::from_raw(w, h, rgba).context("invalid image buffer")
}

fn capture_x11_tools() -> Result<RgbaImage> {
    let tmp = tmp_path();
    let p = tmp.to_string_lossy().to_string();
    for (cmd, args) in [
        ("scrot", vec!["-o", p.as_str()]),
        ("maim", vec![p.as_str()]),
        ("gnome-screenshot", vec!["-f", p.as_str()]),
    ] {
        if run_ok(cmd, &args) {
            info!("Screenshot captured via {cmd}");
            return load_and_remove(&tmp);
        }
    }
    anyhow::bail!("could not capture the screen on X11 (install scrot or maim)")
}

fn capture_wayland(m: &MonitorGeom) -> Result<RgbaImage> {
    let tmp = tmp_path();
    let p = tmp.to_string_lossy().to_string();
    let geom = format!("{},{} {}x{}", m.x, m.y, m.width, m.height);
    if run_ok("grim", &["-g", &geom, &p]) {
        return load_and_remove(&tmp);
    }
    for (cmd, args) in [
        ("grim", vec![p.as_str()]),
        ("gnome-screenshot", vec!["-f", p.as_str()]),
        ("spectacle", vec!["-b", "-n", "-f", "-o", p.as_str()]),
    ] {
        if run_ok(cmd, &args) {
            info!("Screenshot captured via {cmd}");
            return Ok(crop_full(load_and_remove(&tmp)?, m));
        }
    }
    anyhow::bail!(
        "could not capture the screen on Wayland (install grim, gnome-screenshot or spectacle)"
    )
}

/// Crop a full virtual-desktop capture to `m`, scaling for HiDPI if the
/// capture size doesn't match the monitor layout. Returns the input if the
/// crop would be empty or the capture is already monitor-sized.
fn crop_full(full: RgbaImage, m: &MonitorGeom) -> RgbaImage {
    if full.width() == m.width && full.height() == m.height {
        return full;
    }
    let (x, y) = (m.x.max(0) as u32, m.y.max(0) as u32);
    if x + m.width <= full.width() && y + m.height <= full.height() {
        return image::imageops::crop_imm(&full, x, y, m.width, m.height).to_image();
    }
    full
}

fn run_ok(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn tmp_path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("mpt-zoom-it-{}.png", std::process::id()))
}

fn load_and_remove(path: &Path) -> Result<RgbaImage> {
    let img = image::open(path).context("open screenshot")?.to_rgba8();
    let _ = std::fs::remove_file(path);
    Ok(img)
}
