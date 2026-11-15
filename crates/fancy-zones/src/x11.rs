use crate::geometry::Rect;
use anyhow::{Context, Result};
use tracing::info;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt, PropMode};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

/// Get the currently focused X11 window ID.
pub fn get_focused_window() -> Result<u32> {
    let (conn, screen_num) = RustConnection::connect(None).context("failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let focus = conn
        .get_input_focus()
        .context("get_input_focus request failed")?
        .reply()
        .context("get_input_focus reply failed")?;

    let window = focus.focus;
    if window == root || window == 0 {
        anyhow::bail!("no focused window found");
    }
    Ok(window)
}

/// Get the root window (screen) dimensions.
/// For MVP this returns the full virtual desktop size (single-monitor assumption).
pub fn get_screen_geometry() -> Result<(u32, u32)> {
    let (conn, screen_num) = RustConnection::connect(None).context("failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];
    Ok((
        screen.width_in_pixels as u32,
        screen.height_in_pixels as u32,
    ))
}

/// Move and resize a window to the given absolute pixel coordinates.
///
/// Uses `_NET_MOVERESIZE_WINDOW` EWMH client message which is better than
/// raw `configure_window` because the window manager handles frame offsets.
/// The rect is the outer (frame) rect; `_NET_FRAME_EXTENTS` are subtracted from
/// the requested client size so decorated windows fit the zone exactly.
pub fn move_resize_window(window: u32, x: i32, y: i32, w: u32, h: u32) -> Result<()> {
    let wm = Wm::connect()?;
    wm.move_resize(window, Rect::new(x, y, w as i32, h as i32))
}

/// Get the absolute position of a window on the virtual desktop.
pub fn get_window_position(window: u32) -> Result<(i32, i32)> {
    let (conn, screen_num) = RustConnection::connect(None).context("failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // translate_coordinates gives us position relative to root (= absolute)
    let coords = conn
        .translate_coordinates(window, root, 0, 0)
        .context("translate_coordinates request failed")?
        .reply()
        .context("translate_coordinates reply failed")?;

    Ok((coords.dst_x as i32, coords.dst_y as i32))
}

/// Find which monitor a window is on, based on its top-left corner.
pub fn find_monitor_for_window(window: u32) -> Result<mpt_common::monitor::Monitor> {
    let (wx, wy) = get_window_position(window)?;
    info!("Window {window} position: ({wx}, {wy})");
    let monitors = mpt_common::monitor::detect_monitors()?;

    // Find the monitor whose rect contains the window's top-left corner
    if let Some(m) = monitors
        .iter()
        .find(|m| wx >= m.x && wx < m.x + m.width as i32 && wy >= m.y && wy < m.y + m.height as i32)
    {
        return Ok(m.clone());
    }

    // Fallback: primary or first monitor
    monitors
        .iter()
        .find(|m| m.is_primary)
        .or(monitors.first())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("no monitors detected"))
}

/// Get the WM_CLASS strings (instance, class) of a window.
pub fn get_wm_class(window: u32) -> Result<Vec<String>> {
    Wm::connect()?.wm_class(window)
}

fn intern_atom(conn: &RustConnection, name: &[u8]) -> Result<u32> {
    let reply = conn
        .intern_atom(false, name)
        .context("intern_atom request failed")?
        .reply()
        .context("intern_atom reply failed")?;
    Ok(reply.atom)
}

/// Interned atoms used by FancyZones.
#[derive(Debug, Clone, Copy)]
pub struct Atoms {
    pub net_active_window: u32,
    pub net_client_list: u32,
    pub net_wm_state: u32,
    pub net_wm_state_max_h: u32,
    pub net_wm_state_max_v: u32,
    pub net_moveresize_window: u32,
    pub net_wm_window_opacity: u32,
    pub net_frame_extents: u32,
    pub net_wm_window_type: u32,
    pub net_wm_window_type_normal: u32,
    pub net_wm_window_type_dialog: u32,
}

/// A connection to the X server with EWMH helpers. One per thread.
pub struct Wm {
    pub conn: RustConnection,
    pub screen_num: usize,
    pub root: u32,
    pub atoms: Atoms,
}

/// Pointer state from `QueryPointer`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pointer {
    pub x: i32,
    pub y: i32,
    pub button1: bool,
    pub button2: bool,
    pub button3: bool,
    pub shift: bool,
    pub ctrl: bool,
}

impl Wm {
    pub fn connect() -> Result<Self> {
        let (conn, screen_num) =
            RustConnection::connect(None).context("failed to connect to X11")?;
        let root = conn.setup().roots[screen_num].root;
        let a = |n: &[u8]| intern_atom(&conn, n);
        let atoms = Atoms {
            net_active_window: a(b"_NET_ACTIVE_WINDOW")?,
            net_client_list: a(b"_NET_CLIENT_LIST")?,
            net_wm_state: a(b"_NET_WM_STATE")?,
            net_wm_state_max_h: a(b"_NET_WM_STATE_MAXIMIZED_HORZ")?,
            net_wm_state_max_v: a(b"_NET_WM_STATE_MAXIMIZED_VERT")?,
            net_moveresize_window: a(b"_NET_MOVERESIZE_WINDOW")?,
            net_wm_window_opacity: a(b"_NET_WM_WINDOW_OPACITY")?,
            net_frame_extents: a(b"_NET_FRAME_EXTENTS")?,
            net_wm_window_type: a(b"_NET_WM_WINDOW_TYPE")?,
            net_wm_window_type_normal: a(b"_NET_WM_WINDOW_TYPE_NORMAL")?,
            net_wm_window_type_dialog: a(b"_NET_WM_WINDOW_TYPE_DIALOG")?,
        };
        Ok(Self {
            conn,
            screen_num,
            root,
            atoms,
        })
    }

    pub fn screen(&self) -> &xproto::Screen {
        &self.conn.setup().roots[self.screen_num]
    }

    fn cardinals(&self, window: u32, prop: u32, ty: impl Into<u32>) -> Vec<u32> {
        self.conn
            .get_property(false, window, prop, ty, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    /// The active top-level client (`_NET_ACTIVE_WINDOW`, else input focus).
    pub fn active_window(&self) -> Option<u32> {
        let w = self
            .cardinals(self.root, self.atoms.net_active_window, AtomEnum::WINDOW)
            .first()
            .copied()
            .filter(|&w| w != 0 && w != self.root);
        w.or_else(|| {
            self.conn
                .get_input_focus()
                .ok()?
                .reply()
                .ok()
                .map(|f| f.focus)
                .filter(|&w| w > 1 && w != self.root)
        })
    }

    /// Managed client windows (`_NET_CLIENT_LIST`).
    pub fn client_list(&self) -> Vec<u32> {
        self.cardinals(self.root, self.atoms.net_client_list, AtomEnum::WINDOW)
    }

    pub fn wm_class(&self, window: u32) -> Result<Vec<String>> {
        let reply = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)?
            .reply()?;
        Ok(reply
            .value
            .split(|&b| b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect())
    }

    /// Normal application window: type NORMAL (or unset) and not transient.
    pub fn is_normal_window(&self, window: u32) -> bool {
        let types = self.cardinals(window, self.atoms.net_wm_window_type, AtomEnum::ATOM);
        let type_ok = types.is_empty() || types.contains(&self.atoms.net_wm_window_type_normal);
        let transient = !self
            .cardinals(window, AtomEnum::WM_TRANSIENT_FOR.into(), AtomEnum::WINDOW)
            .is_empty();
        type_ok && !transient && !types.contains(&self.atoms.net_wm_window_type_dialog)
    }

    /// Client rect in root coordinates.
    pub fn geometry(&self, window: u32) -> Option<Rect> {
        let g = self.conn.get_geometry(window).ok()?.reply().ok()?;
        let t = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some(Rect::new(
            t.dst_x as i32,
            t.dst_y as i32,
            g.width as i32,
            g.height as i32,
        ))
    }

    /// `_NET_FRAME_EXTENTS` (left, right, top, bottom); zeros if unknown.
    pub fn frame_extents(&self, window: u32) -> (i32, i32, i32, i32) {
        match self.cardinals(window, self.atoms.net_frame_extents, AtomEnum::CARDINAL)[..] {
            [l, r, t, b, ..] => (l as i32, r as i32, t as i32, b as i32),
            _ => (0, 0, 0, 0),
        }
    }

    /// Outer rect (client + decorations) in root coordinates.
    pub fn outer_geometry(&self, window: u32) -> Option<Rect> {
        let c = self.geometry(window)?;
        let (l, r, t, b) = self.frame_extents(window);
        Some(Rect::new(c.x - l, c.y - t, c.w + l + r, c.h + t + b))
    }

    pub fn pointer(&self) -> Option<Pointer> {
        let p = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        let m = u16::from(p.mask);
        let has = |b: xproto::KeyButMask| m & u16::from(b) != 0;
        Some(Pointer {
            x: p.root_x as i32,
            y: p.root_y as i32,
            button1: has(xproto::KeyButMask::BUTTON1),
            button2: has(xproto::KeyButMask::BUTTON2),
            button3: has(xproto::KeyButMask::BUTTON3),
            shift: has(xproto::KeyButMask::SHIFT),
            ctrl: has(xproto::KeyButMask::CONTROL),
        })
    }

    fn send_root_message(&self, window: u32, ty: u32, data: [u32; 5]) -> Result<()> {
        let event = xproto::ClientMessageEvent::new(32, window, ty, data);
        self.conn.send_event(
            false,
            self.root,
            xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
            event,
        )?;
        Ok(())
    }

    /// Remove _NET_WM_STATE_MAXIMIZED_HORZ/VERT so the window can be resized.
    pub fn unmaximize(&self, window: u32) -> Result<()> {
        self.send_root_message(
            window,
            self.atoms.net_wm_state,
            [
                0,
                self.atoms.net_wm_state_max_h,
                self.atoms.net_wm_state_max_v,
                1,
                0,
            ],
        )
        .context("failed to send unmaximize")
    }

    /// Unmaximize and move/resize so the window's outer frame fills `outer`.
    pub fn move_resize(&self, window: u32, outer: Rect) -> Result<()> {
        self.unmaximize(window)?;
        let (l, r, t, b) = self.frame_extents(window);
        let w = (outer.w - l - r).max(1) as u32;
        let h = (outer.h - t - b).max(1) as u32;
        // Flags: gravity=0 (window's own, normally NorthWest), x/y/w/h present
        // (bits 8-11), source=2 (pager).
        let flags: u32 = (0xF << 8) | (2 << 12);
        self.send_root_message(
            window,
            self.atoms.net_moveresize_window,
            [flags, outer.x as u32, outer.y as u32, w, h],
        )
        .context("failed to send _NET_MOVERESIZE_WINDOW")?;
        self.conn
            .flush()
            .context("failed to flush X11 connection")?;
        info!(
            "Moved window {window} to ({}, {}) {w}x{h}",
            outer.x, outer.y
        );
        Ok(())
    }

    /// Resize the client area only, keeping the current position.
    pub fn resize_client(&self, window: u32, w: u32, h: u32) -> Result<()> {
        self.unmaximize(window)?;
        let flags: u32 = (0b1100 << 8) | (2 << 12);
        self.send_root_message(
            window,
            self.atoms.net_moveresize_window,
            [flags, 0, 0, w.max(1), h.max(1)],
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Top-level ancestor (WM frame) of a client window, or the window itself.
    pub fn frame_of(&self, window: u32) -> u32 {
        let mut w = window;
        for _ in 0..16 {
            let Some(tree) = self.conn.query_tree(w).ok().and_then(|c| c.reply().ok()) else {
                break;
            };
            if tree.parent == tree.root || tree.parent == 0 {
                break;
            }
            w = tree.parent;
        }
        w
    }

    /// Set (`Some(0.0..=1.0)`) or clear (`None`) `_NET_WM_WINDOW_OPACITY` on the
    /// client and its frame (compositors read either).
    pub fn set_opacity(&self, window: u32, opacity: Option<f32>) {
        let frame = self.frame_of(window);
        for w in [window, frame] {
            match opacity {
                Some(o) => {
                    let v = (o.clamp(0.0, 1.0) as f64 * u32::MAX as f64) as u32;
                    let _ = self.conn.change_property32(
                        PropMode::REPLACE,
                        w,
                        self.atoms.net_wm_window_opacity,
                        AtomEnum::CARDINAL,
                        &[v],
                    );
                }
                None => {
                    let _ = self
                        .conn
                        .delete_property(w, self.atoms.net_wm_window_opacity);
                }
            }
            if frame == window {
                break;
            }
        }
        let _ = self.conn.flush();
    }

    /// Subscribe to root PropertyNotify (for `_NET_CLIENT_LIST` changes).
    pub fn watch_root_properties(&self) -> Result<()> {
        let attrs =
            xproto::ChangeWindowAttributesAux::new().event_mask(xproto::EventMask::PROPERTY_CHANGE);
        self.conn.change_window_attributes(self.root, &attrs)?;
        self.conn.flush()?;
        Ok(())
    }
}
