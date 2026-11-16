//! Small X11 (EWMH/ICCCM) helpers shared by the module and the GUI binary.

use anyhow::{Context, Result, bail};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

/// WM_CLASS (instance and class) used for every window this module creates.
/// Used to recognise our own cropped windows so they are not cropped again.
pub const APP_CLASS: &str = "mpt-crop-and-lock";

/// Suffix appended to the source title, as upstream does ("<title> (Cropped)").
pub const TITLE_SUFFIX: &str = " (Cropped)";

/// Connect to the X server. Fails with a clear message on pure Wayland.
pub fn connect() -> Result<(RustConnection, usize)> {
    if std::env::var_os("DISPLAY").is_none() {
        bail!("Crop And Lock needs an X11 session (DISPLAY is not set)");
    }
    RustConnection::connect(None).context("failed to connect to the X server")
}

pub fn intern_atom(conn: &RustConnection, name: &[u8]) -> Result<u32> {
    Ok(conn
        .intern_atom(false, name)
        .context("intern_atom request failed")?
        .reply()
        .context("intern_atom reply failed")?
        .atom)
}

/// The currently active window from `_NET_ACTIVE_WINDOW`, falling back to the
/// input focus when the window manager does not support EWMH.
pub fn active_window(conn: &RustConnection, root: Window) -> Result<Window> {
    let net_active = intern_atom(conn, b"_NET_ACTIVE_WINDOW")?;
    let reply = conn
        .get_property(false, root, net_active, AtomEnum::WINDOW, 0, 1)?
        .reply()
        .context("_NET_ACTIVE_WINDOW reply failed")?;
    if let Some(win) = reply.value32().and_then(|mut v| v.next())
        && win != 0
        && win != root
    {
        return Ok(win);
    }

    let focus = conn.get_input_focus()?.reply()?.focus;
    // PointerRoot = 1, None = 0
    if focus <= 1 || focus == root {
        bail!("no active window");
    }
    Ok(focus)
}

/// Window title from `_NET_WM_NAME` (UTF-8) or `WM_NAME`.
pub fn window_title(conn: &RustConnection, window: Window) -> String {
    let utf8 = intern_atom(conn, b"UTF8_STRING").ok();
    let net_wm_name = intern_atom(conn, b"_NET_WM_NAME").ok();
    if let (Some(utf8), Some(name)) = (utf8, net_wm_name)
        && let Ok(cookie) = conn.get_property(false, window, name, utf8, 0, 1024)
        && let Ok(reply) = cookie.reply()
        && !reply.value.is_empty()
    {
        return String::from_utf8_lossy(&reply.value).into_owned();
    }
    if let Ok(cookie) = conn.get_property(false, window, AtomEnum::WM_NAME, AtomEnum::ANY, 0, 1024)
        && let Ok(reply) = cookie.reply()
    {
        return String::from_utf8_lossy(&reply.value).into_owned();
    }
    String::new()
}

/// Whether `window` is one of our own cropped windows (by WM_CLASS).
pub fn is_own_window(conn: &RustConnection, window: Window) -> bool {
    let Ok(cookie) = conn.get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
    else {
        return false;
    };
    let Ok(reply) = cookie.reply() else {
        return false;
    };
    String::from_utf8_lossy(&reply.value)
        .split('\0')
        .any(|part| part == APP_CLASS)
}

/// Geometry of a window in root (screen) coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootGeometry {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub border_width: u32,
}

pub fn root_geometry(conn: &RustConnection, window: Window) -> Result<RootGeometry> {
    let geom = conn
        .get_geometry(window)?
        .reply()
        .context("window no longer exists")?;
    let translated = conn
        .translate_coordinates(window, geom.root, 0, 0)?
        .reply()
        .context("translate_coordinates failed")?;
    Ok(RootGeometry {
        x: i32::from(translated.dst_x),
        y: i32::from(translated.dst_y),
        width: u32::from(geom.width),
        height: u32::from(geom.height),
        border_width: u32::from(geom.border_width),
    })
}

/// Ask the window manager to activate (raise + focus) a window.
pub fn activate_window(conn: &RustConnection, root: Window, window: Window) -> Result<()> {
    let net_active = intern_atom(conn, b"_NET_ACTIVE_WINDOW")?;
    // data: source indication (2 = pager/user action), timestamp, current active
    let event = xproto::ClientMessageEvent::new(32, window, net_active, [2u32, 0, 0, 0, 0]);
    conn.send_event(
        false,
        root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )?;
    conn.flush()?;
    Ok(())
}
