//! Small X11 helpers shared by the mouse utilities (overlay windows, active window queries).

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::shape;
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;

pub fn intern_atom(conn: &RustConnection, name: &[u8]) -> Result<u32> {
    Ok(conn
        .intern_atom(false, name)
        .context("intern_atom request failed")?
        .reply()
        .context("intern_atom reply failed")?
        .atom)
}

/// The active toplevel window (`_NET_ACTIVE_WINDOW`), falling back to the input focus.
pub fn active_window(conn: &RustConnection, root: u32) -> Option<u32> {
    if let Ok(atom) = intern_atom(conn, b"_NET_ACTIVE_WINDOW")
        && let Ok(cookie) = conn.get_property(false, root, atom, xproto::AtomEnum::WINDOW, 0, 1)
        && let Ok(reply) = cookie.reply()
        && let Some(win) = reply.value32().and_then(|mut v| v.next())
        && win != 0
    {
        return Some(win);
    }
    let focus = conn.get_input_focus().ok()?.reply().ok()?.focus;
    (focus != 0 && focus != root).then_some(focus)
}

/// Both WM_CLASS parts (instance, class) of a window. Empty when unavailable.
pub fn wm_class(conn: &RustConnection, window: u32) -> Vec<String> {
    let Ok(cookie) = conn.get_property(
        false,
        window,
        xproto::AtomEnum::WM_CLASS,
        xproto::AtomEnum::STRING,
        0,
        256,
    ) else {
        return Vec::new();
    };
    let Ok(reply) = cookie.reply() else {
        return Vec::new();
    };
    String::from_utf8_lossy(&reply.value)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether a window is fullscreen: `_NET_WM_STATE_FULLSCREEN` set, or its
/// geometry covers the whole root window. Used as the Linux stand-in for
/// Windows "Game Mode".
pub fn is_fullscreen(conn: &RustConnection, root: u32, window: u32) -> bool {
    if let (Ok(state), Ok(fs)) = (
        intern_atom(conn, b"_NET_WM_STATE"),
        intern_atom(conn, b"_NET_WM_STATE_FULLSCREEN"),
    ) && let Ok(cookie) = conn.get_property(false, window, state, xproto::AtomEnum::ATOM, 0, 64)
        && let Ok(reply) = cookie.reply()
        && let Some(mut atoms) = reply.value32()
        && atoms.any(|a| a == fs)
    {
        return true;
    }
    let (Ok(g), Ok(rg)) = (
        conn.get_geometry(window).map(|c| c.reply()),
        conn.get_geometry(root).map(|c| c.reply()),
    ) else {
        return false;
    };
    match (g, rg) {
        (Ok(g), Ok(rg)) => g.width >= rg.width && g.height >= rg.height,
        _ => false,
    }
}

/// Set `_NET_WM_WINDOW_OPACITY` (0.0–1.0) on a window.
pub fn set_opacity(conn: &RustConnection, atom: u32, window: u32, opacity: f32) -> Result<()> {
    let value = (opacity.clamp(0.0, 1.0) as f64 * u32::MAX as f64) as u32;
    conn.change_property(
        xproto::PropMode::REPLACE,
        window,
        atom,
        xproto::AtomEnum::CARDINAL,
        32,
        1,
        &value.to_ne_bytes(),
    )?;
    Ok(())
}

/// Give a window an empty input shape so clicks pass through it.
pub fn make_click_through(conn: &RustConnection, window: u32) -> Result<()> {
    shape::rectangles(
        conn,
        shape::SO::SET,
        shape::SK::INPUT,
        xproto::ClipOrdering::UNSORTED,
        window,
        0,
        0,
        &[],
    )?;
    Ok(())
}

/// Create a mapped, override-redirect, click-through, solid-colored window.
#[allow(clippy::too_many_arguments)]
pub fn create_overlay_window(
    conn: &RustConnection,
    root: u32,
    depth: u8,
    color: u32,
    opacity_atom: u32,
    opacity: f32,
    (x, y, w, h): (i16, i16, u16, u16),
    map: bool,
) -> Result<u32> {
    let wid = conn.generate_id()?;
    let values = xproto::CreateWindowAux::new()
        .background_pixel(color)
        .override_redirect(1)
        .event_mask(xproto::EventMask::EXPOSURE);
    conn.create_window(
        depth,
        wid,
        root,
        x,
        y,
        w.max(1),
        h.max(1),
        0,
        xproto::WindowClass::INPUT_OUTPUT,
        0,
        &values,
    )?;
    set_opacity(conn, opacity_atom, wid, opacity)?;
    make_click_through(conn, wid)?;
    if map {
        conn.configure_window(
            wid,
            &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
        )?;
        conn.map_window(wid)?;
    }
    Ok(wid)
}

/// Shape a window's bounding region as a filled circle (`hole = false`) or as
/// the whole window minus a circle (`hole = true`). Circle center is in window coordinates.
#[allow(clippy::too_many_arguments)]
pub fn shape_circle(
    conn: &RustConnection,
    window: u32,
    w: u16,
    h: u16,
    cx: i16,
    cy: i16,
    radius: u32,
    hole: bool,
) -> Result<()> {
    let r = radius.min(i16::MAX as u32 / 2) as i16;
    let pixmap = conn.generate_id()?;
    conn.create_pixmap(1, pixmap, window, w.max(1), h.max(1))?;
    let gc_bg = conn.generate_id()?;
    conn.create_gc(
        gc_bg,
        pixmap,
        &xproto::CreateGCAux::new().foreground(u32::from(hole)),
    )?;
    let gc_fg = conn.generate_id()?;
    conn.create_gc(
        gc_fg,
        pixmap,
        &xproto::CreateGCAux::new().foreground(u32::from(!hole)),
    )?;
    conn.poly_fill_rectangle(
        pixmap,
        gc_bg,
        &[xproto::Rectangle {
            x: 0,
            y: 0,
            width: w.max(1),
            height: h.max(1),
        }],
    )?;
    conn.poly_fill_arc(
        pixmap,
        gc_fg,
        &[xproto::Arc {
            x: cx.saturating_sub(r),
            y: cy.saturating_sub(r),
            width: (r as u16).saturating_mul(2),
            height: (r as u16).saturating_mul(2),
            angle1: 0,
            angle2: 360 * 64,
        }],
    )?;
    shape::mask(
        conn,
        shape::SO::SET,
        shape::SK::BOUNDING,
        window,
        0,
        0,
        pixmap,
    )?;
    conn.free_gc(gc_bg)?;
    conn.free_gc(gc_fg)?;
    conn.free_pixmap(pixmap)?;
    Ok(())
}

pub fn raise(conn: &RustConnection, window: u32) -> Result<()> {
    conn.configure_window(
        window,
        &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
    )?;
    Ok(())
}
