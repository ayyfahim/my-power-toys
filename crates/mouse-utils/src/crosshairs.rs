use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::ConnectionExt as XfixesConnectionExt;
use x11rb::protocol::xinput::{self, ConnectionExt as XiConnectionExt};
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;

use crate::config::{CrosshairOrientation, MouseUtilsConfig};
use crate::x11util;

/// Spawn the crosshairs overlay thread.
/// Creates thin line windows that follow the cursor.
pub fn spawn(config: MouseUtilsConfig, stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    spawn_with_toggle(config, Arc::new(AtomicBool::new(true)), stop)
}

/// Like [`spawn`], but the crosshairs are only shown while `active` is true
/// (toggled by the activation shortcut).
pub fn spawn_with_toggle(
    config: MouseUtilsConfig,
    active: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("crosshairs".into())
        .spawn(move || {
            if let Err(e) = run(config, active, stop) {
                warn!("Crosshairs stopped: {e}");
            }
        })
        .expect("failed to spawn crosshairs thread")
}

/// Visual parameters for a set of crosshair lines.
#[derive(Debug, Clone)]
pub(crate) struct LineStyle {
    pub color: u32,
    pub opacity: f32,
    pub thickness: u32,
    pub border_color: u32,
    pub border_size: u32,
    pub orientation: CrosshairOrientation,
    pub fixed_length: Option<u32>,
}

impl LineStyle {
    pub fn from_config(config: &MouseUtilsConfig) -> Self {
        Self {
            color: parse_hex_color(&config.crosshair_color),
            opacity: config.crosshair_opacity.clamp(0.0, 1.0),
            thickness: config.crosshair_thickness.max(1),
            border_color: parse_hex_color_or(&config.crosshair_border_color, 0xFFFFFF),
            border_size: config.crosshair_border_size,
            orientation: config.crosshair_orientation,
            fixed_length: config
                .crosshair_fixed_length
                .then_some(config.crosshair_fixed_length_px.max(1)),
        }
    }
}

/// Window rectangle `(x, y, w, h)`.
type Rect = (i32, i32, u32, u32);

/// Geometry of the horizontal line (with `pad` extra pixels on every side,
/// used for the border) centred on the pointer.
fn horizontal_rect(style: &LineStyle, sw: u16, cx: i16, cy: i16, pad: u32) -> Rect {
    let t = style.thickness + 2 * pad;
    let (x, w) = match style.fixed_length {
        Some(len) => {
            let len = len + 2 * pad;
            (cx as i32 - (len / 2) as i32, len)
        }
        None => (0, sw as u32),
    };
    (x, cy as i32 - (t / 2) as i32, w, t)
}

fn vertical_rect(style: &LineStyle, sh: u16, cx: i16, cy: i16, pad: u32) -> Rect {
    let t = style.thickness + 2 * pad;
    let (y, h) = match style.fixed_length {
        Some(len) => {
            let len = len + 2 * pad;
            (cy as i32 - (len / 2) as i32, len)
        }
        None => (0, sh as u32),
    };
    (cx as i32 - (t / 2) as i32, y, t, h)
}

/// A set of override-redirect line windows (plus optional border windows)
/// that can be moved to follow a point. Shared by Crosshairs and Gliding Cursor.
pub(crate) struct CrosshairLines {
    style: LineStyle,
    sw: u16,
    sh: u16,
    /// (line window, border window) per axis.
    horizontal: Option<(u32, Option<u32>)>,
    vertical: Option<(u32, Option<u32>)>,
    visible: bool,
    last_pos: Option<(i16, i16)>,
}

impl CrosshairLines {
    pub fn create(conn: &RustConnection, screen_num: usize, style: LineStyle) -> Result<Self> {
        let screen = &conn.setup().roots[screen_num];
        let (root, depth) = (screen.root, screen.root_depth);
        let (sw, sh) = (screen.width_in_pixels, screen.height_in_pixels);
        let opacity_atom = x11util::intern_atom(conn, b"_NET_WM_WINDOW_OPACITY")?;

        let make_pair = || -> Result<(u32, Option<u32>)> {
            // Border first so the line is created (and stacked) above it.
            let border = if style.border_size > 0 {
                Some(x11util::create_overlay_window(
                    conn,
                    root,
                    depth,
                    style.border_color,
                    opacity_atom,
                    style.opacity,
                    (0, 0, 1, 1),
                    false,
                )?)
            } else {
                None
            };
            let line = x11util::create_overlay_window(
                conn,
                root,
                depth,
                style.color,
                opacity_atom,
                style.opacity,
                (0, 0, 1, 1),
                false,
            )?;
            Ok((line, border))
        };

        let horizontal = if style.orientation != CrosshairOrientation::Vertical {
            Some(make_pair()?)
        } else {
            None
        };
        let vertical = if style.orientation != CrosshairOrientation::Horizontal {
            Some(make_pair()?)
        } else {
            None
        };

        Ok(Self {
            style,
            sw,
            sh,
            horizontal,
            vertical,
            visible: false,
            last_pos: None,
        })
    }

    fn windows(&self) -> impl Iterator<Item = u32> + '_ {
        [self.horizontal, self.vertical]
            .into_iter()
            .flatten()
            .flat_map(|(line, border)| border.into_iter().chain(std::iter::once(line)))
    }

    /// Move the lines so they cross at `(cx, cy)`, re-raising them above other windows.
    pub fn update(&mut self, conn: &RustConnection, cx: i16, cy: i16) -> Result<()> {
        if !self.visible {
            for w in self.windows().collect::<Vec<_>>() {
                conn.map_window(w)?;
            }
            self.visible = true;
        }
        let moved = self.last_pos != Some((cx, cy));
        let pad = self.style.border_size;
        let place = |win: u32, (x, y, w, h): Rect| {
            let mut aux = xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE);
            if moved {
                aux = aux.x(x).y(y).width(w.max(1)).height(h.max(1));
            }
            conn.configure_window(win, &aux).map(|_| ())
        };
        if let Some((line, border)) = self.horizontal {
            if let Some(b) = border {
                place(b, horizontal_rect(&self.style, self.sw, cx, cy, pad))?;
            }
            place(line, horizontal_rect(&self.style, self.sw, cx, cy, 0))?;
        }
        if let Some((line, border)) = self.vertical {
            if let Some(b) = border {
                place(b, vertical_rect(&self.style, self.sh, cx, cy, pad))?;
            }
            place(line, vertical_rect(&self.style, self.sh, cx, cy, 0))?;
        }
        self.last_pos = Some((cx, cy));
        Ok(())
    }

    pub fn hide(&mut self, conn: &RustConnection) -> Result<()> {
        if self.visible {
            for w in self.windows().collect::<Vec<_>>() {
                conn.unmap_window(w)?;
            }
            self.visible = false;
        }
        Ok(())
    }

    pub fn destroy(self, conn: &RustConnection) {
        for w in self.windows() {
            let _ = conn.destroy_window(w);
        }
        let _ = conn.flush();
    }
}

/// Auto-hide decision (upstream: hide the crosshairs while the pointer is
/// hidden). Linux has no global "pointer hidden" flag, so we combine two signals:
/// the current cursor image is fully transparent (apps / unclutter hid it), or
/// the user is typing (Windows hides the pointer while typing) and the
/// pointer has not moved since.
fn should_auto_hide(cursor_invisible: bool, typing_since_move: bool) -> bool {
    cursor_invisible || typing_since_move
}

/// Whether an ARGB cursor image is effectively invisible.
fn cursor_image_invisible(width: u16, height: u16, pixels: &[u32]) -> bool {
    width == 0 || height == 0 || pixels.iter().all(|p| p >> 24 == 0)
}

fn modifier_keycodes(conn: &RustConnection) -> HashSet<u8> {
    conn.get_modifier_mapping()
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.keycodes.into_iter().filter(|&k| k != 0).collect())
        .unwrap_or_default()
}

fn run(config: MouseUtilsConfig, active: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> Result<()> {
    let (conn, screen_num) =
        RustConnection::connect(None).context("Crosshairs: failed to connect to X11")?;
    let root = conn.setup().roots[screen_num].root;

    let style = LineStyle::from_config(&config);
    let auto_hide = config.crosshair_auto_hide;

    // Auto-hide inputs: XFixes cursor image + XI2 raw key presses.
    let xfixes_ok = auto_hide
        && conn
            .xfixes_query_version(5, 0)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some();
    if auto_hide {
        let event_mask = xinput::EventMask {
            deviceid: 3,
            mask: vec![(1u32 << 13).into()], // XI_RawKeyPress
        };
        if let Err(e) = conn
            .xinput_xi_select_events(root, &[event_mask])
            .map_err(anyhow::Error::from)
            .and_then(|c| c.check().map_err(anyhow::Error::from))
        {
            warn!("Crosshairs: cannot watch keyboard for auto-hide: {e}");
        }
    }
    let mod_keys = modifier_keycodes(&conn);

    let mut lines: Option<CrosshairLines> = None;
    let mut typing_since_move = false;
    let mut typing_pos: Option<(i16, i16)> = None;
    let mut cursor_invisible = false;
    let mut last_cursor_check = Instant::now() - Duration::from_secs(1);

    info!(
        "Crosshairs ready (orientation={:?}, thickness={}px, border={}px, auto_hide={})",
        style.orientation, style.thickness, style.border_size, auto_hide
    );

    while !stop.load(Ordering::Relaxed) {
        let is_active = active.load(Ordering::Relaxed);

        // Drain events (raw key presses for typing detection).
        let mut key_pressed = false;
        while let Some(event) = conn.poll_for_event()? {
            if let x11rb::protocol::Event::XinputRawKeyPress(e) = &event
                && !mod_keys.contains(&(e.detail as u8))
            {
                key_pressed = true;
            }
        }

        if !is_active {
            if let Some(l) = lines.take() {
                info!("Crosshairs deactivated");
                l.destroy(&conn);
            }
            thread::sleep(Duration::from_millis(50));
            continue;
        }
        if lines.is_none() {
            info!("Crosshairs activated");
            lines = Some(CrosshairLines::create(&conn, screen_num, style.clone())?);
        }

        let reply = conn.query_pointer(root)?.reply()?;
        let (cx, cy) = (reply.root_x, reply.root_y);

        if auto_hide {
            // Typing only counts without Ctrl/Alt/Super held, so shortcuts don't hide.
            let held = u16::from(reply.mask);
            let shortcut_mods = u16::from(xproto::KeyButMask::CONTROL)
                | u16::from(xproto::KeyButMask::MOD1)
                | u16::from(xproto::KeyButMask::MOD4);
            if key_pressed && held & shortcut_mods == 0 {
                typing_since_move = true;
                typing_pos = Some((cx, cy));
            } else if typing_since_move && typing_pos != Some((cx, cy)) {
                typing_since_move = false;
            }
            if xfixes_ok && last_cursor_check.elapsed() >= Duration::from_millis(150) {
                last_cursor_check = Instant::now();
                if let Ok(c) = conn.xfixes_get_cursor_image()
                    && let Ok(img) = c.reply()
                {
                    cursor_invisible =
                        cursor_image_invisible(img.width, img.height, &img.cursor_image);
                }
            }
        }

        let l = lines.as_mut().expect("lines created above");
        if auto_hide && should_auto_hide(cursor_invisible, typing_since_move) {
            if l.visible {
                debug!("Crosshairs: auto-hidden");
            }
            l.hide(&conn)?;
        } else {
            l.update(&conn, cx, cy)?;
        }

        conn.flush()?;
        thread::sleep(Duration::from_millis(16)); // ~60fps
    }

    if let Some(l) = lines.take() {
        l.destroy(&conn);
    }
    info!("Crosshairs overlay stopped");
    Ok(())
}

fn parse_hex_color(hex: &str) -> u32 {
    parse_hex_color_or(hex, 0xFF0000)
}

fn parse_hex_color_or(hex: &str, fallback: u32) -> u32 {
    crate::config::parse_argb(hex)
        .map(|(rgb, _)| rgb)
        .unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(fixed: Option<u32>) -> LineStyle {
        LineStyle {
            color: 0xFF0000,
            opacity: 1.0,
            thickness: 4,
            border_color: 0xFFFFFF,
            border_size: 2,
            orientation: CrosshairOrientation::Both,
            fixed_length: fixed,
        }
    }

    #[test]
    fn line_geometry_with_border() {
        let s = style(None);
        assert_eq!(horizontal_rect(&s, 1920, 100, 50, 0), (0, 48, 1920, 4));
        assert_eq!(horizontal_rect(&s, 1920, 100, 50, 2), (0, 46, 1920, 8));
        assert_eq!(vertical_rect(&s, 1080, 100, 50, 0), (98, 0, 4, 1080));
        let s = style(Some(200));
        assert_eq!(horizontal_rect(&s, 1920, 100, 50, 0), (0, 48, 200, 4));
        assert_eq!(horizontal_rect(&s, 1920, 100, 50, 2), (-2, 46, 204, 8));
    }

    #[test]
    fn auto_hide_rules() {
        assert!(cursor_image_invisible(0, 0, &[]));
        assert!(cursor_image_invisible(2, 1, &[0x00FF_FFFF, 0]));
        assert!(!cursor_image_invisible(2, 1, &[0xFF00_0000, 0]));
        assert!(should_auto_hide(true, false));
        assert!(should_auto_hide(false, true));
        assert!(!should_auto_hide(false, false));
    }

    #[test]
    fn style_from_config() {
        let cfg = MouseUtilsConfig {
            crosshair_border_size: 3,
            crosshair_border_color: "#00FF00".into(),
            ..Default::default()
        };
        let s = LineStyle::from_config(&cfg);
        assert_eq!(s.border_size, 3);
        assert_eq!(s.border_color, 0x00FF00);
        assert_eq!(s.color, 0xFF0000);
        assert_eq!(s.fixed_length, None);
    }
}
