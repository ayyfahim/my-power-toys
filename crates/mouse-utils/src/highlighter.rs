use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as XiConnectionExt};
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;

use crate::config::{HighlightMode, MouseUtilsConfig, parse_argb};
use crate::x11util;

/// Spawn the mouse highlighter thread.
/// Watches for mouse clicks and shows colored circles at click positions.
pub fn spawn(config: MouseUtilsConfig, stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    spawn_with_toggle(config, Arc::new(AtomicBool::new(true)), stop)
}

/// Like [`spawn`], but highlighting is only shown while `active` is true
/// (toggled by the activation shortcut).
pub fn spawn_with_toggle(
    config: MouseUtilsConfig,
    active: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("mouse-highlighter".into())
        .spawn(move || {
            if let Err(e) = run(config, active, stop) {
                warn!("Mouse Highlighter stopped: {e}");
            }
        })
        .expect("failed to spawn mouse-highlighter thread")
}

/// A circle overlay that fades out after a delay.
struct HighlightCircle {
    window: u32,
    created_at: Instant,
    alpha: f32,
}

/// Resolved colors/opacities for the highlighter.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Palette {
    primary: (u32, f32),
    secondary: (u32, f32),
    /// Always-on pointer highlight (None when disabled / fully transparent).
    always: Option<(u32, f32)>,
    /// Dim layer used in spotlight mode.
    spotlight: (u32, f32),
}

fn palette(config: &MouseUtilsConfig) -> Palette {
    let opacity = config.highlighter_opacity.clamp(0.0, 1.0);
    let scale = |(rgb, a): (u32, f32)| (rgb, a * opacity);
    let primary = scale(parse_argb(&config.highlighter_primary_color).unwrap_or((0xFFFF00, 1.0)));
    let secondary =
        scale(parse_argb(&config.highlighter_secondary_color).unwrap_or((0x0000FF, 1.0)));
    let always = parse_argb(&config.highlighter_always_color)
        .filter(|(_, a)| *a > 0.0)
        .map(scale);
    // Upstream dims the screen with the always color; fall back to black so
    // spotlight mode does something even without an always color.
    let spotlight = always.unwrap_or((0x000000, opacity));
    Palette {
        primary,
        secondary,
        always,
        spotlight,
    }
}

/// Persistent overlays shown while the highlighter is active.
struct PointerOverlays {
    /// Always-on circle following the pointer (circle mode).
    always: Option<u32>,
    /// Fullscreen dim layer with a hole at the pointer (spotlight mode).
    spotlight: Option<u32>,
    always_visible: bool,
    last_spot: Option<(i16, i16, u32)>,
}

fn run(config: MouseUtilsConfig, active: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> Result<()> {
    let (conn, screen_num) =
        RustConnection::connect(None).context("Highlighter: failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let depth = screen.root_depth;
    let (sw, sh) = (screen.width_in_pixels, screen.height_in_pixels);

    // Select XI2 raw button events
    // XI_RawButtonPress = bit 15, XI_RawButtonRelease = bit 16
    let mask: u32 = (1 << 15) | (1 << 16);
    let event_mask = xinput::EventMask {
        deviceid: 3,
        mask: vec![mask.into()],
    };
    conn.xinput_xi_select_events(root, &[event_mask])?
        .check()
        .context("Highlighter: failed to select XInput2 events")?;
    conn.flush()?;

    let opacity_atom = x11util::intern_atom(&conn, b"_NET_WM_WINDOW_OPACITY")?;

    let colors = palette(&config);
    let spotlight_mode = config.highlighter_mode == HighlightMode::Spotlight;
    let radius = config.highlighter_radius.max(5);
    let diameter = (radius * 2) as u16;
    let fade_delay = Duration::from_millis(config.highlighter_fade_delay_ms as u64);
    let fade_duration = Duration::from_millis(config.highlighter_fade_duration_ms as u64);
    let total_lifetime = fade_delay + fade_duration;

    info!(
        "Mouse Highlighter ready (mode={:?}, radius={}px, fade={}+{}ms, always={})",
        config.highlighter_mode,
        radius,
        config.highlighter_fade_delay_ms,
        config.highlighter_fade_duration_ms,
        colors.always.is_some()
    );

    let mut circles: Vec<HighlightCircle> = Vec::new();
    let mut overlays = PointerOverlays {
        always: None,
        spotlight: None,
        always_visible: false,
        last_spot: None,
    };
    let mut was_active = false;
    let (mut left_down, mut right_down) = (false, false);

    while !stop.load(Ordering::Relaxed) {
        let is_active = active.load(Ordering::Relaxed);
        if is_active != was_active {
            if is_active {
                info!("Mouse Highlighter activated");
                if spotlight_mode {
                    overlays.spotlight = Some(x11util::create_overlay_window(
                        &conn,
                        root,
                        depth,
                        colors.spotlight.0,
                        opacity_atom,
                        colors.spotlight.1,
                        (0, 0, sw, sh),
                        false,
                    )?);
                } else if let Some((rgb, a)) = colors.always {
                    let win = x11util::create_overlay_window(
                        &conn,
                        root,
                        depth,
                        rgb,
                        opacity_atom,
                        a,
                        (0, 0, diameter, diameter),
                        false,
                    )?;
                    let r = radius as i16;
                    x11util::shape_circle(&conn, win, diameter, diameter, r, r, radius, false)?;
                    overlays.always = Some(win);
                }
            } else {
                info!("Mouse Highlighter deactivated");
                for c in circles.drain(..) {
                    let _ = conn.destroy_window(c.window);
                }
                for win in [overlays.always.take(), overlays.spotlight.take()]
                    .into_iter()
                    .flatten()
                {
                    let _ = conn.destroy_window(win);
                }
                overlays.always_visible = false;
                overlays.last_spot = None;
            }
            was_active = is_active;
        }

        // Process events
        while let Some(event) = conn.poll_for_event()? {
            match &event {
                x11rb::protocol::Event::XinputRawButtonPress(e)
                    if e.detail == 1 || e.detail == 3 =>
                {
                    let button = e.detail;
                    if button == 1 {
                        left_down = true;
                    } else {
                        right_down = true;
                    }
                    // In spotlight mode a press shrinks the spotlight instead of
                    // drawing a circle (upstream SpotlightAnimatePress).
                    if !is_active || spotlight_mode {
                        continue;
                    }
                    let (color, alpha) = if button == 1 {
                        colors.primary
                    } else {
                        colors.secondary
                    };
                    let cursor = conn.query_pointer(root)?.reply()?;
                    let (cx, cy) = (cursor.root_x, cursor.root_y);
                    debug!("Highlighter: click at ({cx}, {cy}), button={button}");
                    if let Ok(win) = create_circle_window(
                        &conn,
                        root,
                        depth,
                        color,
                        opacity_atom,
                        alpha,
                        cx,
                        cy,
                        radius,
                    ) {
                        circles.push(HighlightCircle {
                            window: win,
                            created_at: Instant::now(),
                            alpha,
                        });
                    }
                }
                x11rb::protocol::Event::XinputRawButtonRelease(e) => match e.detail {
                    1 => left_down = false,
                    3 => right_down = false,
                    _ => {}
                },
                _ => {}
            }
        }

        if is_active {
            // Clean up expired circles
            let now = Instant::now();
            circles.retain(|c| {
                if now.duration_since(c.created_at) > total_lifetime {
                    let _ = conn.destroy_window(c.window);
                    false
                } else {
                    true
                }
            });

            // Update opacity for fading circles
            for circle in &circles {
                let elapsed = now.duration_since(circle.created_at);
                if elapsed > fade_delay {
                    let alpha = circle.alpha * fade_alpha(elapsed - fade_delay, fade_duration);
                    let _ = x11util::set_opacity(&conn, opacity_atom, circle.window, alpha);
                }
            }

            if overlays.always.is_some() || overlays.spotlight.is_some() {
                let p = conn.query_pointer(root)?.reply()?;
                let pressed = left_down || right_down;
                if let Some(win) = overlays.always {
                    // Upstream hides the always-on highlight while a button is held.
                    if pressed {
                        if overlays.always_visible {
                            conn.unmap_window(win)?;
                            overlays.always_visible = false;
                        }
                    } else {
                        let r = radius as i32;
                        conn.configure_window(
                            win,
                            &xproto::ConfigureWindowAux::new()
                                .x(p.root_x as i32 - r)
                                .y(p.root_y as i32 - r)
                                .stack_mode(xproto::StackMode::ABOVE),
                        )?;
                        if !overlays.always_visible {
                            conn.map_window(win)?;
                            overlays.always_visible = true;
                        }
                    }
                }
                if let Some(win) = overlays.spotlight {
                    let r = if pressed { (radius / 2).max(3) } else { radius };
                    let key = (p.root_x, p.root_y, r);
                    if overlays.last_spot != Some(key) {
                        x11util::shape_circle(&conn, win, sw, sh, p.root_x, p.root_y, r, true)?;
                        if overlays.last_spot.is_none() {
                            x11util::raise(&conn, win)?;
                            conn.map_window(win)?;
                        }
                        overlays.last_spot = Some(key);
                    }
                }
            }
        }

        let _ = conn.flush();
        thread::sleep(Duration::from_millis(16));
    }

    // Cleanup
    for circle in &circles {
        let _ = conn.destroy_window(circle.window);
    }
    for win in [overlays.always, overlays.spotlight].into_iter().flatten() {
        let _ = conn.destroy_window(win);
    }
    let _ = conn.flush();

    info!("Mouse Highlighter stopped");
    Ok(())
}

/// Remaining alpha factor (1.0 → 0.0) during the fade-out phase.
fn fade_alpha(elapsed_in_fade: Duration, fade_duration: Duration) -> f32 {
    if fade_duration.is_zero() {
        return 0.0;
    }
    (1.0 - elapsed_in_fade.as_secs_f32() / fade_duration.as_secs_f32()).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn create_circle_window(
    conn: &RustConnection,
    root: u32,
    depth: u8,
    color: u32,
    opacity_atom: u32,
    alpha: f32,
    cx: i16,
    cy: i16,
    radius: u32,
) -> Result<u32> {
    let r = radius as i16;
    let diameter = (radius * 2) as u16;
    let wid = x11util::create_overlay_window(
        conn,
        root,
        depth,
        color,
        opacity_atom,
        alpha,
        (cx - r, cy - r, diameter, diameter),
        false,
    )?;
    x11util::shape_circle(conn, wid, diameter, diameter, r, r, radius, false)?;
    x11util::raise(conn, wid)?;
    conn.map_window(wid)?;
    Ok(wid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_applies_opacity_and_always_color() {
        let mut cfg = MouseUtilsConfig {
            highlighter_opacity: 0.5,
            ..Default::default()
        };
        let p = palette(&cfg);
        assert_eq!(p.primary, (0xFFFF00, 0.5));
        assert!(p.always.is_none());
        assert_eq!(p.spotlight, (0x000000, 0.5));

        // Upstream default always color "#00FF0000" is fully transparent = disabled.
        cfg.highlighter_always_color = "#00FF0000".into();
        assert!(palette(&cfg).always.is_none());

        cfg.highlighter_always_color = "#FF0000".into();
        let p = palette(&cfg);
        assert_eq!(p.always, Some((0xFF0000, 0.5)));
        assert_eq!(p.spotlight, (0xFF0000, 0.5));
    }

    #[test]
    fn fade_alpha_ranges() {
        let d = Duration::from_millis(200);
        assert_eq!(fade_alpha(Duration::ZERO, d), 1.0);
        assert!((fade_alpha(Duration::from_millis(100), d) - 0.5).abs() < 1e-3);
        assert_eq!(fade_alpha(Duration::from_millis(400), d), 0.0);
        assert_eq!(fade_alpha(Duration::ZERO, Duration::ZERO), 0.0);
    }
}
