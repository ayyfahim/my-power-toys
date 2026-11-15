use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, KeyButMask};
use x11rb::rust_connection::RustConnection;

use crate::config::{CursorWrapActivation, CursorWrapMode, MouseUtilsConfig};

/// Margin: how close to the edge before wrapping (in pixels)
const MARGIN: i16 = 1;

/// Spawn the cursor wrap thread.
/// When the cursor hits a screen edge, it wraps to the opposite side.
/// Wrapping only happens while `active` is true (toggled by the activation shortcut).
pub fn spawn(
    config: MouseUtilsConfig,
    active: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("cursor-wrap".into())
        .spawn(move || {
            if let Err(e) = run(config, active, stop) {
                warn!("Cursor Wrap stopped: {e}");
            }
        })
        .expect("failed to spawn cursor-wrap thread")
}

/// Where the pointer should be warped to, if anywhere.
pub fn wrap_target(
    (cx, cy): (i16, i16),
    (sw, sh): (i16, i16),
    mode: CursorWrapMode,
) -> Option<(i16, i16)> {
    let mut nx = cx;
    let mut ny = cy;

    if mode != CursorWrapMode::VerticalOnly {
        if cx <= MARGIN {
            nx = sw - MARGIN - 2;
        } else if cx >= sw - MARGIN - 1 {
            nx = MARGIN + 1;
        }
    }
    if mode != CursorWrapMode::HorizontalOnly {
        if cy <= MARGIN {
            ny = sh - MARGIN - 2;
        } else if cy >= sh - MARGIN - 1 {
            ny = MARGIN + 1;
        }
    }
    (nx != cx || ny != cy).then_some((nx, ny))
}

/// Whether the modifier/button state allows wrapping right now.
pub fn wrap_allowed(
    mask: KeyButMask,
    activation: CursorWrapActivation,
    disable_during_drag: bool,
) -> bool {
    if disable_during_drag && mask.contains(KeyButMask::BUTTON1) {
        return false;
    }
    match activation {
        CursorWrapActivation::Always => true,
        CursorWrapActivation::HoldingCtrl => mask.contains(KeyButMask::CONTROL),
        CursorWrapActivation::HoldingShift => mask.contains(KeyButMask::SHIFT),
    }
}

fn monitor_count() -> usize {
    mpt_common::monitor::detect_monitors()
        .map(|m| m.len())
        .unwrap_or(1)
}

fn run(config: MouseUtilsConfig, active: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> Result<()> {
    let (conn, screen_num) =
        RustConnection::connect(None).context("Cursor Wrap: failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let sw = screen.width_in_pixels as i16;
    let sh = screen.height_in_pixels as i16;

    let mode = config.cursor_wrap_mode;
    let activation = config.cursor_wrap_activation_mode;
    let single_monitor_check = config.cursor_wrap_disable_on_single_monitor;
    let mut single_monitor = single_monitor_check && monitor_count() <= 1;
    let mut last_monitor_check = Instant::now();

    info!(
        "Cursor Wrap ready (screen {sw}x{sh}, mode={mode:?}, activation={activation:?}, drag_off={}, single_monitor_off={})",
        config.cursor_wrap_disable_during_drag, single_monitor_check
    );

    while !stop.load(Ordering::Relaxed) {
        if !active.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(100));
            continue;
        }

        // Monitors can be hot-plugged; re-check occasionally.
        if single_monitor_check && last_monitor_check.elapsed() > Duration::from_secs(5) {
            single_monitor = monitor_count() <= 1;
            last_monitor_check = Instant::now();
        }
        if single_monitor {
            thread::sleep(Duration::from_millis(250));
            continue;
        }

        let reply = conn.query_pointer(root)?.reply()?;
        let (cx, cy) = (reply.root_x, reply.root_y);

        if wrap_allowed(
            reply.mask,
            activation,
            config.cursor_wrap_disable_during_drag,
        ) && let Some((nx, ny)) = wrap_target((cx, cy), (sw, sh), mode)
        {
            debug!("Cursor Wrap: ({cx},{cy}) -> ({nx},{ny})");
            conn.warp_pointer(0u32, root, 0, 0, 0, 0, nx, ny)?;
            conn.flush()?;
        }

        thread::sleep(Duration::from_millis(8)); // ~120Hz polling for responsive wrapping
    }

    info!("Cursor Wrap stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: (i16, i16) = (1920, 1080);

    #[test]
    fn wraps_per_mode() {
        assert_eq!(
            wrap_target((0, 500), SCREEN, CursorWrapMode::Both),
            Some((1917, 500))
        );
        assert_eq!(
            wrap_target((1919, 500), SCREEN, CursorWrapMode::Both),
            Some((2, 500))
        );
        assert_eq!(
            wrap_target((0, 500), SCREEN, CursorWrapMode::VerticalOnly),
            None
        );
        assert_eq!(
            wrap_target((500, 0), SCREEN, CursorWrapMode::VerticalOnly),
            Some((500, 1077))
        );
        assert_eq!(
            wrap_target((500, 1079), SCREEN, CursorWrapMode::HorizontalOnly),
            None
        );
        assert_eq!(wrap_target((500, 500), SCREEN, CursorWrapMode::Both), None);
    }

    #[test]
    fn activation_and_drag_rules() {
        let none = KeyButMask::default();
        assert!(wrap_allowed(none, CursorWrapActivation::Always, true));
        assert!(!wrap_allowed(
            KeyButMask::BUTTON1,
            CursorWrapActivation::Always,
            true
        ));
        assert!(wrap_allowed(
            KeyButMask::BUTTON1,
            CursorWrapActivation::Always,
            false
        ));
        assert!(!wrap_allowed(none, CursorWrapActivation::HoldingCtrl, true));
        assert!(wrap_allowed(
            KeyButMask::CONTROL,
            CursorWrapActivation::HoldingCtrl,
            true
        ));
        assert!(wrap_allowed(
            KeyButMask::SHIFT,
            CursorWrapActivation::HoldingShift,
            true
        ));
    }
}
