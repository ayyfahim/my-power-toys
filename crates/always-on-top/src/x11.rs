use anyhow::{Context, Result};
use tracing::info;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;

/// Information about the currently focused X11 window.
pub struct FocusedWindow {
    pub id: u32,
    pub root: u32,
    pub wm_class: String,
}

/// Get the focused window with metadata needed for the toggle decision.
/// Prefers EWMH `_NET_ACTIVE_WINDOW` (the client toplevel), falling back to the input focus.
pub fn get_focused_window() -> Result<FocusedWindow> {
    let (conn, screen_num) = RustConnection::connect(None).context("failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    let window = match active_window(&conn, root) {
        Some(w) => w,
        None => {
            conn.get_input_focus()
                .context("get_input_focus request failed")?
                .reply()
                .context("get_input_focus reply failed")?
                .focus
        }
    };
    if window == root || window == 0 {
        anyhow::bail!("no focused window found");
    }

    let wm_class = get_wm_class(&conn, window).unwrap_or_default();

    Ok(FocusedWindow {
        id: window,
        root,
        wm_class,
    })
}

fn active_window(conn: &RustConnection, root: u32) -> Option<u32> {
    let atom = intern_atom(conn, b"_NET_ACTIVE_WINDOW").ok()?;
    let reply = conn
        .get_property(false, root, atom, xproto::AtomEnum::WINDOW, 0, 1)
        .ok()?
        .reply()
        .ok()?;
    reply.value32()?.next().filter(|&w| w != 0)
}

/// Whether the window is fullscreen (`_NET_WM_STATE_FULLSCREEN`, or covers the whole root).
/// Used as the Linux stand-in for Windows Game Mode.
pub fn is_fullscreen(window: u32, root: u32) -> Result<bool> {
    let (conn, _) = RustConnection::connect(None).context("failed to connect to X11")?;
    let state = intern_atom(&conn, b"_NET_WM_STATE")?;
    let fullscreen = intern_atom(&conn, b"_NET_WM_STATE_FULLSCREEN")?;
    let reply = conn
        .get_property(false, window, state, xproto::AtomEnum::ATOM, 0, 64)?
        .reply()?;
    if reply
        .value32()
        .is_some_and(|mut v| v.any(|a| a == fullscreen))
    {
        return Ok(true);
    }
    let g = conn.get_geometry(window)?.reply()?;
    let rg = conn.get_geometry(root)?.reply()?;
    Ok(g.width >= rg.width && g.height >= rg.height)
}

/// The window-manager frame of a client (the ancestor that is a direct child of root).
fn frame_of(conn: &RustConnection, window: u32, root: u32) -> Option<u32> {
    let mut current = window;
    for _ in 0..16 {
        let tree = conn.query_tree(current).ok()?.reply().ok()?;
        if tree.parent == root || tree.parent == 0 {
            return Some(current);
        }
        current = tree.parent;
    }
    None
}

/// Set `_NET_WM_WINDOW_OPACITY` on a client window (and its WM frame, for
/// compositors that read it there). `opacity >= 1.0` removes the property.
pub fn set_window_opacity(window: u32, root: u32, opacity: f32) -> Result<()> {
    let (conn, _) = RustConnection::connect(None).context("failed to connect to X11")?;
    let atom = intern_atom(&conn, b"_NET_WM_WINDOW_OPACITY")?;
    let mut targets = vec![window];
    if let Some(frame) = frame_of(&conn, window, root)
        && frame != window
    {
        targets.push(frame);
    }
    for w in targets {
        if opacity >= 1.0 {
            conn.delete_property(w, atom)?;
        } else {
            let value = (opacity.clamp(0.0, 1.0) as f64 * u32::MAX as f64) as u32;
            conn.change_property(
                xproto::PropMode::REPLACE,
                w,
                atom,
                xproto::AtomEnum::CARDINAL,
                32,
                1,
                &value.to_ne_bytes(),
            )?;
        }
    }
    conn.flush()?;
    Ok(())
}

/// Explicitly set or remove _NET_WM_STATE_ABOVE on the given window.
/// Using explicit ADD/REMOVE instead of TOGGLE avoids desync between
/// our internal state tracking and the actual X11 window state.
pub fn set_always_on_top(window: u32, root: u32, enable: bool) -> Result<()> {
    let (conn, _) = RustConnection::connect(None).context("failed to connect to X11")?;

    let net_wm_state = intern_atom(&conn, b"_NET_WM_STATE")?;
    let net_wm_state_above = intern_atom(&conn, b"_NET_WM_STATE_ABOVE")?;

    const NET_WM_STATE_REMOVE: u32 = 0;
    const NET_WM_STATE_ADD: u32 = 1;

    let action = if enable {
        NET_WM_STATE_ADD
    } else {
        NET_WM_STATE_REMOVE
    };

    let event = xproto::ClientMessageEvent::new(
        32,
        window,
        net_wm_state,
        [action, net_wm_state_above, 0, 1, 0],
    );

    conn.send_event(
        false,
        root,
        xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        event,
    )
    .context("failed to send client message")?;

    conn.flush().context("failed to flush X11 connection")?;

    let verb = if enable { "Added" } else { "Removed" };
    info!("{verb} always-on-top for window {window}");
    Ok(())
}

/// Read WM_CLASS for a window (returns the instance name, first part).
pub fn get_wm_class(conn: &RustConnection, window: u32) -> Result<String> {
    let reply = conn
        .get_property(
            false,
            window,
            xproto::AtomEnum::WM_CLASS,
            xproto::AtomEnum::STRING,
            0,
            256,
        )
        .context("get WM_CLASS failed")?
        .reply()
        .context("get WM_CLASS reply failed")?;

    if reply.value.is_empty() {
        anyhow::bail!("empty WM_CLASS");
    }

    // WM_CLASS is two null-terminated strings: instance\0class\0
    let value = String::from_utf8_lossy(&reply.value);
    let instance = value.split('\0').next().unwrap_or("");

    if instance.is_empty() {
        anyhow::bail!("empty WM_CLASS instance");
    }

    Ok(instance.to_string())
}

fn intern_atom(conn: &RustConnection, name: &[u8]) -> Result<u32> {
    let reply = conn
        .intern_atom(false, name)
        .context("intern_atom request failed")?
        .reply()
        .context("intern_atom reply failed")?;
    Ok(reply.atom)
}
