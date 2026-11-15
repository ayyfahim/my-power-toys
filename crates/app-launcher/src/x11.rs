//! X11/EWMH helpers: window list for Window Walker, activation, cursor and
//! focused-window geometry for window placement.

use anyhow::{Context, Result, bail};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, Atom, AtomEnum, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    /// WM_CLASS class part (e.g. "firefox").
    pub class: String,
    pub pid: Option<u32>,
    pub desktop: Option<u32>,
}

struct Atoms {
    active: Atom,
    client_list: Atom,
    stacking: Atom,
    close: Atom,
    wm_name: Atom,
    utf8: Atom,
    wm_pid: Atom,
    wm_desktop: Atom,
    current_desktop: Atom,
    wm_state: Atom,
    skip_taskbar: Atom,
    wm_type: Atom,
    type_normal: Atom,
    type_dialog: Atom,
}

pub struct X11 {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
}

const ALL_DESKTOPS: u32 = 0xFFFF_FFFF;

impl X11 {
    pub fn connect() -> Result<Self> {
        if std::env::var_os("DISPLAY").is_none() {
            bail!("DISPLAY not set (X11 required)");
        }
        let (conn, screen) = RustConnection::connect(None).context("failed to connect to X11")?;
        let root = conn.setup().roots[screen].root;
        let intern =
            |name: &[u8]| -> Result<Atom> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
        let atoms = Atoms {
            active: intern(b"_NET_ACTIVE_WINDOW")?,
            client_list: intern(b"_NET_CLIENT_LIST")?,
            stacking: intern(b"_NET_CLIENT_LIST_STACKING")?,
            close: intern(b"_NET_CLOSE_WINDOW")?,
            wm_name: intern(b"_NET_WM_NAME")?,
            utf8: intern(b"UTF8_STRING")?,
            wm_pid: intern(b"_NET_WM_PID")?,
            wm_desktop: intern(b"_NET_WM_DESKTOP")?,
            current_desktop: intern(b"_NET_CURRENT_DESKTOP")?,
            wm_state: intern(b"_NET_WM_STATE")?,
            skip_taskbar: intern(b"_NET_WM_STATE_SKIP_TASKBAR")?,
            wm_type: intern(b"_NET_WM_WINDOW_TYPE")?,
            type_normal: intern(b"_NET_WM_WINDOW_TYPE_NORMAL")?,
            type_dialog: intern(b"_NET_WM_WINDOW_TYPE_DIALOG")?,
        };
        Ok(Self { conn, root, atoms })
    }

    fn prop32(&self, window: Window, prop: Atom, ty: impl Into<Atom>) -> Vec<u32> {
        self.conn
            .get_property(false, window, prop, ty, 0, 4096)
            .ok()
            .and_then(|c| c.reply().ok())
            .and_then(|r| r.value32().map(|v| v.collect()))
            .unwrap_or_default()
    }

    fn prop_string(&self, window: Window, prop: Atom, ty: impl Into<Atom>) -> Option<String> {
        let r = self
            .conn
            .get_property(false, window, prop, ty, 0, 1024)
            .ok()?
            .reply()
            .ok()?;
        if r.value.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&r.value).into_owned())
    }

    pub fn active_window(&self) -> Option<u32> {
        self.prop32(self.root, self.atoms.active, AtomEnum::WINDOW)
            .first()
            .copied()
            .filter(|&w| w != 0 && w != self.root)
    }

    fn title(&self, w: Window) -> String {
        self.prop_string(w, self.atoms.wm_name, self.atoms.utf8)
            .or_else(|| self.prop_string(w, AtomEnum::WM_NAME.into(), AtomEnum::STRING))
            .unwrap_or_default()
    }

    fn class(&self, w: Window) -> String {
        self.prop_string(w, AtomEnum::WM_CLASS.into(), AtomEnum::STRING)
            .map(|s| {
                let mut parts = s.split('\0').filter(|p| !p.is_empty());
                let instance = parts.next().unwrap_or("").to_string();
                parts.next().map(str::to_string).unwrap_or(instance)
            })
            .unwrap_or_default()
    }

    fn is_listed(&self, w: Window) -> bool {
        let types = self.prop32(w, self.atoms.wm_type, AtomEnum::ATOM);
        if !types.is_empty()
            && !types
                .iter()
                .any(|t| *t == self.atoms.type_normal || *t == self.atoms.type_dialog)
        {
            return false;
        }
        let state = self.prop32(w, self.atoms.wm_state, AtomEnum::ATOM);
        !state.contains(&self.atoms.skip_taskbar)
    }

    /// Open client windows, most recently used first (via
    /// `_NET_CLIENT_LIST_STACKING`, falling back to `_NET_CLIENT_LIST`).
    pub fn list_windows(&self, all_desktops: bool) -> Vec<WindowInfo> {
        let mut list = self.prop32(self.root, self.atoms.stacking, AtomEnum::WINDOW);
        if list.is_empty() {
            list = self.prop32(self.root, self.atoms.client_list, AtomEnum::WINDOW);
        }
        list.reverse();
        let current = self
            .prop32(self.root, self.atoms.current_desktop, AtomEnum::CARDINAL)
            .first()
            .copied();
        list.into_iter()
            .filter(|&w| self.is_listed(w))
            .filter_map(|w| {
                let desktop = self
                    .prop32(w, self.atoms.wm_desktop, AtomEnum::CARDINAL)
                    .first()
                    .copied();
                if !all_desktops
                    && let (Some(cur), Some(d)) = (current, desktop)
                    && d != cur
                    && d != ALL_DESKTOPS
                {
                    return None;
                }
                let title = self.title(w);
                if title.is_empty() {
                    return None;
                }
                Some(WindowInfo {
                    id: w,
                    title,
                    class: self.class(w),
                    pid: self
                        .prop32(w, self.atoms.wm_pid, AtomEnum::CARDINAL)
                        .first()
                        .copied(),
                    desktop,
                })
            })
            .collect()
    }

    fn send_root_message(&self, window: Window, ty: Atom, data: [u32; 5]) -> Result<()> {
        let ev = xproto::ClientMessageEvent::new(32, window, ty, data);
        self.conn.send_event(
            false,
            self.root,
            xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        )?;
        self.conn.flush()?;
        Ok(())
    }

    /// Ask the WM to activate (switch desktop / de-iconify / raise) `window`.
    pub fn activate(&self, window: u32) -> Result<()> {
        if window == 0 {
            bail!("invalid window");
        }
        // Source indication 2 = pager/user request.
        self.send_root_message(window, self.atoms.active, [2, 0, 0, 0, 0])
    }

    /// Politely ask the WM to close `window`.
    pub fn close(&self, window: u32) -> Result<()> {
        if window == 0 {
            bail!("invalid window");
        }
        self.send_root_message(window, self.atoms.close, [0, 2, 0, 0, 0])
    }

    /// Mouse pointer position in root coordinates.
    pub fn cursor_position(&self) -> Option<(i32, i32)> {
        let r = self.conn.query_pointer(self.root).ok()?.reply().ok()?;
        Some((r.root_x as i32, r.root_y as i32))
    }

    /// Center of the focused window in root coordinates.
    pub fn focused_window_center(&self) -> Option<(i32, i32)> {
        let w = self.active_window()?;
        let geo = self.conn.get_geometry(w).ok()?.reply().ok()?;
        let t = self
            .conn
            .translate_coordinates(w, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        Some((
            t.dst_x as i32 + geo.width as i32 / 2,
            t.dst_y as i32 + geo.height as i32 / 2,
        ))
    }
}
