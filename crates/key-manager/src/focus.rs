//! Focused-application tracking for per-app mappings (X11 only).
//!
//! A background thread polls `_NET_ACTIVE_WINDOW` every ~200 ms and publishes
//! the window's WM_CLASS instance + class and process name (`/proc/<pid>/comm`
//! from `_NET_WM_PID`). On Wayland the focused window is not observable, so
//! app filters are skipped.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt, Window};
use x11rb::rust_connection::RustConnection;

const POLL: Duration = Duration::from_millis(200);

/// Shared focused-app identifiers plus a generation counter so the input
/// loop only re-applies them when they change.
#[derive(Default)]
pub struct FocusState {
    pub ids: Vec<String>,
    pub generation: u64,
}

pub struct FocusTracker {
    pub state: Arc<Mutex<FocusState>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

/// True when the session is Wayland (X11 focus queries would only see
/// XWayland windows).
pub fn is_wayland() -> bool {
    std::env::var("XDG_SESSION_TYPE").is_ok_and(|v| v.eq_ignore_ascii_case("wayland"))
        || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

impl FocusTracker {
    /// Start tracking. Fails if X11 is unavailable.
    pub fn start() -> Result<Self> {
        let (conn, screen) = RustConnection::connect(None).context("cannot connect to X11")?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)?;
        let state = Arc::new(Mutex::new(FocusState::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let (st, sp) = (state.clone(), stop.clone());
        let handle = std::thread::Builder::new()
            .name("key-manager-focus".into())
            .spawn(move || {
                let mut last: Option<Window> = None;
                while !sp.load(Ordering::Relaxed) {
                    let win = active_window(&conn, root, &atoms).ok().flatten();
                    if win != last {
                        last = win;
                        let ids = win
                            .map(|w| window_ids(&conn, w, &atoms))
                            .unwrap_or_default();
                        debug!("Key Manager: focused app {ids:?}");
                        if let Ok(mut s) = st.lock() {
                            s.ids = ids;
                            s.generation += 1;
                        }
                    }
                    std::thread::sleep(POLL);
                }
            })
            .context("spawn focus thread")?;
        Ok(Self {
            state,
            stop,
            handle: Some(handle),
        })
    }
}

impl Drop for FocusTracker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take()
            && h.join().is_err()
        {
            warn!("Key Manager: focus thread panicked");
        }
    }
}

struct Atoms {
    active: Atom,
    pid: Atom,
}

impl Atoms {
    fn new(conn: &RustConnection) -> Result<Self> {
        let intern =
            |name: &[u8]| -> Result<Atom> { Ok(conn.intern_atom(false, name)?.reply()?.atom) };
        Ok(Self {
            active: intern(b"_NET_ACTIVE_WINDOW")?,
            pid: intern(b"_NET_WM_PID")?,
        })
    }
}

fn active_window(conn: &RustConnection, root: Window, atoms: &Atoms) -> Result<Option<Window>> {
    let reply = conn
        .get_property(false, root, atoms.active, AtomEnum::WINDOW, 0, 1)?
        .reply()?;
    Ok(reply
        .value32()
        .and_then(|mut v| v.next())
        .filter(|&w| w != 0))
}

/// WM_CLASS instance and class, plus the process name if known.
fn window_ids(conn: &RustConnection, win: Window, atoms: &Atoms) -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(cookie) = conn.get_property(false, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
        && let Ok(reply) = cookie.reply()
    {
        ids.extend(parse_wm_class(&reply.value));
    }
    if let Ok(cookie) = conn.get_property(false, win, atoms.pid, AtomEnum::CARDINAL, 0, 1)
        && let Ok(reply) = cookie.reply()
        && let Some(pid) = reply.value32().and_then(|mut v| v.next())
        && let Ok(comm) = std::fs::read_to_string(format!("/proc/{pid}/comm"))
    {
        let comm = comm.trim().to_string();
        if !comm.is_empty() && !ids.contains(&comm) {
            ids.push(comm);
        }
    }
    ids
}

/// WM_CLASS is `instance\0class\0`.
pub fn parse_wm_class(raw: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in raw.split(|&b| b == 0) {
        let s = String::from_utf8_lossy(part).trim().to_string();
        if !s.is_empty() && !out.contains(&s) {
            out.push(s);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wm_class_parsing() {
        assert_eq!(
            parse_wm_class(b"Navigator\0firefox\0"),
            vec!["Navigator", "firefox"]
        );
        assert_eq!(parse_wm_class(b"code\0code\0"), vec!["code"]);
        assert!(parse_wm_class(b"").is_empty());
    }
}
