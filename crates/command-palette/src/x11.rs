//! EWMH helpers for the window walker provider.

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, Atom, AtomEnum, ConnectionExt};
use x11rb::rust_connection::RustConnection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    /// WM_CLASS class part (e.g. `firefox`, `Code`).
    pub class: String,
    pub pid: Option<u32>,
    /// `/proc/<pid>/comm`.
    pub process: Option<String>,
    pub desktop: Option<u32>,
}

fn atom(conn: &RustConnection, name: &[u8]) -> Result<Atom> {
    Ok(conn.intern_atom(false, name)?.reply()?.atom)
}

fn prop32(conn: &RustConnection, win: u32, prop: Atom, ty: impl Into<Atom>) -> Vec<u32> {
    conn.get_property(false, win, prop, ty, 0, 4096)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|v| v.collect()))
        .unwrap_or_default()
}

fn prop_bytes(conn: &RustConnection, win: u32, prop: Atom, ty: impl Into<Atom>) -> Vec<u8> {
    conn.get_property(false, win, prop, ty, 0, 1024)
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|r| r.value)
        .unwrap_or_default()
}

/// Normal application windows from `_NET_CLIENT_LIST`, excluding windows
/// owned by `exclude_pid` (the palette itself).
pub fn list_windows(exclude_pid: u32) -> Result<Vec<WindowInfo>> {
    let (conn, screen_num) = RustConnection::connect(None).context("failed to connect to X11")?;
    let root = conn.setup().roots[screen_num].root;

    let client_list = atom(&conn, b"_NET_CLIENT_LIST")?;
    let wm_type = atom(&conn, b"_NET_WM_WINDOW_TYPE")?;
    let type_normal = atom(&conn, b"_NET_WM_WINDOW_TYPE_NORMAL")?;
    let type_dialog = atom(&conn, b"_NET_WM_WINDOW_TYPE_DIALOG")?;
    let state = atom(&conn, b"_NET_WM_STATE")?;
    let skip_taskbar = atom(&conn, b"_NET_WM_STATE_SKIP_TASKBAR")?;
    let net_wm_name = atom(&conn, b"_NET_WM_NAME")?;
    let utf8 = atom(&conn, b"UTF8_STRING")?;
    let net_wm_pid = atom(&conn, b"_NET_WM_PID")?;
    let net_wm_desktop = atom(&conn, b"_NET_WM_DESKTOP")?;

    let mut out = Vec::new();
    for win in prop32(&conn, root, client_list, AtomEnum::WINDOW) {
        let types = prop32(&conn, win, wm_type, AtomEnum::ATOM);
        if !types.is_empty() && !types.contains(&type_normal) && !types.contains(&type_dialog) {
            continue;
        }
        if prop32(&conn, win, state, AtomEnum::ATOM).contains(&skip_taskbar) {
            continue;
        }
        let pid = prop32(&conn, win, net_wm_pid, AtomEnum::CARDINAL)
            .first()
            .copied();
        if pid == Some(exclude_pid) {
            continue;
        }

        let mut title =
            String::from_utf8_lossy(&prop_bytes(&conn, win, net_wm_name, utf8)).to_string();
        if title.is_empty() {
            title = String::from_utf8_lossy(&prop_bytes(
                &conn,
                win,
                AtomEnum::WM_NAME.into(),
                AtomEnum::STRING,
            ))
            .to_string();
        }
        let class = parse_wm_class(&prop_bytes(
            &conn,
            win,
            AtomEnum::WM_CLASS.into(),
            AtomEnum::STRING,
        ));
        if title.is_empty() && class.is_empty() {
            continue;
        }
        let desktop = prop32(&conn, win, net_wm_desktop, AtomEnum::CARDINAL)
            .first()
            .copied();
        let process = pid.and_then(|p| {
            std::fs::read_to_string(format!("/proc/{p}/comm"))
                .ok()
                .map(|s| s.trim().to_string())
        });

        out.push(WindowInfo {
            id: win,
            title,
            class,
            pid,
            process,
            desktop,
        });
    }
    Ok(out)
}

/// WM_CLASS is `instance\0class\0`; prefer the class part.
pub fn parse_wm_class(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let mut parts = s.split('\0').filter(|p| !p.is_empty());
    let instance = parts.next().unwrap_or_default().to_string();
    parts.next().map(str::to_string).unwrap_or(instance)
}

/// Switch to the window's desktop and activate it.
pub fn activate_window(window: u32) -> Result<()> {
    let (conn, screen_num) = RustConnection::connect(None).context("failed to connect to X11")?;
    let root = conn.setup().roots[screen_num].root;
    let mask = xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY;

    let net_wm_desktop = atom(&conn, b"_NET_WM_DESKTOP")?;
    let current_desktop = atom(&conn, b"_NET_CURRENT_DESKTOP")?;
    let win_desktop = prop32(&conn, window, net_wm_desktop, AtomEnum::CARDINAL)
        .first()
        .copied();
    let cur = prop32(&conn, root, current_desktop, AtomEnum::CARDINAL)
        .first()
        .copied();
    if let Some(d) = win_desktop
        && d != 0xFFFF_FFFF
        && Some(d) != cur
    {
        let ev = xproto::ClientMessageEvent::new(32, root, current_desktop, [d, 0, 0, 0, 0]);
        conn.send_event(false, root, mask, ev)?;
    }

    let active = atom(&conn, b"_NET_ACTIVE_WINDOW")?;
    // source indication 2 = pager, timestamp 0 = CurrentTime
    let ev = xproto::ClientMessageEvent::new(32, window, active, [2, 0, 0, 0, 0]);
    conn.send_event(false, root, mask, ev)?;
    conn.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_class_prefers_class() {
        assert_eq!(parse_wm_class(b"navigator\0firefox\0"), "firefox");
        assert_eq!(parse_wm_class(b"xterm\0"), "xterm");
        assert_eq!(parse_wm_class(b""), "");
    }
}
