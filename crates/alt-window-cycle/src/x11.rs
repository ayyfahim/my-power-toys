//! X11/EWMH queries and activation for Alt Window Cycle.

use anyhow::{Context, Result, bail};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, Atom, AtomEnum, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

/// Identity of the application owning a window.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppKey {
    /// WM_CLASS class part, lowercased (falls back to instance part).
    pub class: String,
    /// Resolved executable of `_NET_WM_PID` (empty if unknown).
    pub exe: String,
}

impl AppKey {
    /// Two windows belong to the same app when their WM_CLASS matches, or
    /// (if either class is missing) when their executables match.
    pub fn same_app(&self, other: &AppKey) -> bool {
        if !self.class.is_empty() && !other.class.is_empty() {
            return self.class == other.class;
        }
        !self.exe.is_empty() && self.exe == other.exe
    }
}

struct Atoms {
    active: Atom,
    stacking: Atom,
    client_list: Atom,
    wm_desktop: Atom,
    current_desktop: Atom,
    wm_pid: Atom,
    wm_state: Atom,
    hidden: Atom,
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

/// Filtering options for collecting the cycle set.
pub struct Filter {
    pub include_minimized: bool,
    pub all_desktops: bool,
}

const ALL_DESKTOPS: u32 = 0xFFFF_FFFF;

impl X11 {
    pub fn connect() -> Result<Self> {
        let (conn, screen) = RustConnection::connect(None).context("failed to connect to X11")?;
        let root = conn.setup().roots[screen].root;
        let intern =
            |name: &[u8]| -> Result<Atom> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
        let atoms = Atoms {
            active: intern(b"_NET_ACTIVE_WINDOW")?,
            stacking: intern(b"_NET_CLIENT_LIST_STACKING")?,
            client_list: intern(b"_NET_CLIENT_LIST")?,
            wm_desktop: intern(b"_NET_WM_DESKTOP")?,
            current_desktop: intern(b"_NET_CURRENT_DESKTOP")?,
            wm_pid: intern(b"_NET_WM_PID")?,
            wm_state: intern(b"_NET_WM_STATE")?,
            hidden: intern(b"_NET_WM_STATE_HIDDEN")?,
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

    pub fn active_window(&self) -> Option<Window> {
        self.prop32(self.root, self.atoms.active, AtomEnum::WINDOW)
            .first()
            .copied()
            .filter(|&w| w != 0 && w != self.root)
    }

    /// Client windows, most recently used (top of stack) first.
    fn clients_mru(&self) -> Vec<Window> {
        let mut list = self.prop32(self.root, self.atoms.stacking, AtomEnum::WINDOW);
        if list.is_empty() {
            list = self.prop32(self.root, self.atoms.client_list, AtomEnum::WINDOW);
        }
        list.reverse();
        list
    }

    pub fn app_key(&self, window: Window) -> AppKey {
        let class = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| {
                let s = String::from_utf8_lossy(&r.value).to_string();
                let mut parts = s.split('\0').filter(|p| !p.is_empty());
                let instance = parts.next().unwrap_or("").to_string();
                parts.next().map(str::to_string).unwrap_or(instance)
            })
            .unwrap_or_default()
            .to_lowercase();
        let exe = self
            .prop32(window, self.atoms.wm_pid, AtomEnum::CARDINAL)
            .first()
            .and_then(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        AppKey { class, exe }
    }

    fn desktop_of(&self, window: Window) -> Option<u32> {
        self.prop32(window, self.atoms.wm_desktop, AtomEnum::CARDINAL)
            .first()
            .copied()
    }

    /// Mirrors upstream's "Alt-Tab eligible" check: normal/dialog windows
    /// that are not skip-taskbar and not transient for another window.
    fn is_eligible(&self, window: Window, include_minimized: bool) -> bool {
        let types = self.prop32(window, self.atoms.wm_type, AtomEnum::ATOM);
        if !types.is_empty()
            && !types
                .iter()
                .any(|t| *t == self.atoms.type_normal || *t == self.atoms.type_dialog)
        {
            return false;
        }
        let state = self.prop32(window, self.atoms.wm_state, AtomEnum::ATOM);
        if state.contains(&self.atoms.skip_taskbar) {
            return false;
        }
        if !include_minimized && state.contains(&self.atoms.hidden) {
            return false;
        }
        let transient = self.prop32(window, AtomEnum::WM_TRANSIENT_FOR.into(), AtomEnum::WINDOW);
        !matches!(transient.first(), Some(&w) if w != 0 && w != self.root)
    }

    /// Windows of the same app as `active`, most recently used first.
    pub fn same_app_windows(&self, active: Window, filter: &Filter) -> Vec<Window> {
        let key = self.app_key(active);
        let current_desktop = self
            .prop32(self.root, self.atoms.current_desktop, AtomEnum::CARDINAL)
            .first()
            .copied();
        let mut out = Vec::new();
        for w in self.clients_mru() {
            if w != active {
                if !self.is_eligible(w, filter.include_minimized) {
                    continue;
                }
                if !filter.all_desktops
                    && let (Some(cur), Some(d)) = (current_desktop, self.desktop_of(w))
                    && d != cur
                    && d != ALL_DESKTOPS
                {
                    continue;
                }
                if !self.app_key(w).same_app(&key) {
                    continue;
                }
            }
            out.push(w);
        }
        if !out.contains(&active) {
            out.insert(0, active);
        }
        out
    }

    /// Ask the WM to activate (and de-iconify) `window`.
    pub fn activate(&self, window: Window, current: Window) -> Result<()> {
        if window == 0 {
            bail!("invalid window");
        }
        // data: source indication 2 (pager/user), timestamp CurrentTime, requestor's active window.
        let ev =
            xproto::ClientMessageEvent::new(32, window, self.atoms.active, [2, 0, current, 0, 0]);
        self.conn.send_event(
            false,
            self.root,
            xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        )?;
        self.conn.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(class: &str, exe: &str) -> AppKey {
        AppKey {
            class: class.into(),
            exe: exe.into(),
        }
    }

    #[test]
    fn same_app_by_class() {
        assert!(key("firefox", "/a").same_app(&key("firefox", "/b")));
        assert!(!key("firefox", "/a").same_app(&key("code", "/a")));
    }

    #[test]
    fn same_app_falls_back_to_exe() {
        assert!(key("", "/usr/bin/x").same_app(&key("foo", "/usr/bin/x")));
        assert!(!key("", "").same_app(&key("", "")));
    }
}
