//! X11 implementation: passive button grabs on the root window plus a
//! background event loop that moves/resizes the client under the pointer.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::properties::WmSizeHints;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    self, Allow, Atom, AtomEnum, ButtonIndex, ConfigureWindowAux, ConnectionExt, Cursor, EventMask,
    GrabMode, ModMask, Window,
};
use x11rb::rust_connection::RustConnection;
use x11rb::{CURRENT_TIME, NONE};

use crate::config::{GrabAndMoveConfig, GrabModifier};
use crate::geometry::{self, Handle, MOVE_CURSOR_GLYPH, Rect};

struct Atoms {
    wm_state: Atom,
    net_supported: Atom,
    net_moveresize: Atom,
    net_active: Atom,
    net_wm_state: Atom,
    fullscreen: Atom,
    max_vert: Atom,
    max_horz: Atom,
    wm_type: Atom,
    type_desktop: Atom,
    type_dock: Atom,
    wm_pid: Atom,
    wm_name: Atom,
    utf8: Atom,
}

impl Atoms {
    fn new(conn: &RustConnection) -> Result<Self> {
        let i = |n: &[u8]| -> Result<Atom> { Ok(conn.intern_atom(false, n)?.reply()?.atom) };
        Ok(Self {
            wm_state: i(b"WM_STATE")?,
            net_supported: i(b"_NET_SUPPORTED")?,
            net_moveresize: i(b"_NET_MOVERESIZE_WINDOW")?,
            net_active: i(b"_NET_ACTIVE_WINDOW")?,
            net_wm_state: i(b"_NET_WM_STATE")?,
            fullscreen: i(b"_NET_WM_STATE_FULLSCREEN")?,
            max_vert: i(b"_NET_WM_STATE_MAXIMIZED_VERT")?,
            max_horz: i(b"_NET_WM_STATE_MAXIMIZED_HORZ")?,
            wm_type: i(b"_NET_WM_WINDOW_TYPE")?,
            type_desktop: i(b"_NET_WM_WINDOW_TYPE_DESKTOP")?,
            type_dock: i(b"_NET_WM_WINDOW_TYPE_DOCK")?,
            wm_pid: i(b"_NET_WM_PID")?,
            wm_name: i(b"_NET_WM_NAME")?,
            utf8: i(b"UTF8_STRING")?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Move,
    Resize,
}

struct Interaction {
    client: Window,
    action: Action,
    button: u8,
    start: (i32, i32),
    last: (i32, i32),
    /// Client rect at press time (move) / current rect (resize), root coords.
    rect: Rect,
    origin: Rect,
    handle: Handle,
    min: (i32, i32),
    active: bool,
    maximized: bool,
}

struct Cursors {
    moving: Cursor,
    handles: [(Handle, Cursor); 8],
}

/// Grab state owned by the background thread.
pub struct Grabber {
    conn: RustConnection,
    root: Window,
    atoms: Atoms,
    cfg: GrabAndMoveConfig,
    modifier: ModMask,
    buttons: Vec<ButtonIndex>,
    use_ewmh: bool,
    cursors: Option<Cursors>,
}

fn event_mask() -> EventMask {
    EventMask::BUTTON_PRESS | EventMask::BUTTON_RELEASE | EventMask::POINTER_MOTION
}

/// Lock-key variants: none, CapsLock, NumLock (Mod2), both.
fn lock_variants() -> [ModMask; 4] {
    [
        ModMask::from(0u16),
        ModMask::LOCK,
        ModMask::M2,
        ModMask::LOCK | ModMask::M2,
    ]
}

impl Grabber {
    /// Connect and install the passive grabs. Errors surface to `start()`.
    pub fn new(cfg: GrabAndMoveConfig) -> Result<Self> {
        let (conn, screen) = RustConnection::connect(None).context("failed to connect to X11")?;
        let root = conn.setup().roots[screen].root;
        let atoms = Atoms::new(&conn)?;
        let modifier = match cfg.modifier_key {
            GrabModifier::Alt => ModMask::M1,
            GrabModifier::Super => ModMask::M4,
        };
        let mut buttons = vec![ButtonIndex::M1];
        if cfg.use_alt_resize {
            buttons.push(ButtonIndex::M3);
        }

        let supported = prop32(&conn, root, atoms.net_supported, AtomEnum::ATOM);
        let use_ewmh = supported.contains(&atoms.net_moveresize);
        let cursors = create_cursors(&conn).ok();

        let mut g = Self {
            conn,
            root,
            atoms,
            cfg,
            modifier,
            buttons,
            use_ewmh,
            cursors,
        };
        g.grab()?;
        info!(
            "Grab and Move: grabbed {:?}+drag (resize: {}, EWMH moveresize: {})",
            g.cfg.modifier_key, g.cfg.use_alt_resize, g.use_ewmh
        );
        Ok(g)
    }

    fn grab(&mut self) -> Result<()> {
        let mut grabbed = 0;
        for &button in &self.buttons {
            for lock in lock_variants() {
                let res = self
                    .conn
                    .grab_button(
                        false,
                        self.root,
                        event_mask(),
                        GrabMode::SYNC,
                        GrabMode::ASYNC,
                        NONE,
                        NONE,
                        button,
                        self.modifier | lock,
                    )?
                    .check();
                match res {
                    Ok(()) => grabbed += 1,
                    Err(e) => warn!("Grab and Move: grab {button:?}+{lock:?} failed: {e}"),
                }
            }
        }
        if grabbed == 0 {
            anyhow::bail!("could not grab modifier+mouse buttons (another program owns them?)");
        }
        Ok(())
    }

    fn ungrab(&self) {
        for &button in &self.buttons {
            for lock in lock_variants() {
                let _ = self
                    .conn
                    .ungrab_button(button, self.root, self.modifier | lock);
            }
        }
        let _ = self.conn.flush();
    }

    /// Event loop; returns when `running` becomes false.
    pub fn run(mut self, running: Arc<AtomicBool>) {
        let mut current: Option<Interaction> = None;
        while running.load(Ordering::Relaxed) {
            let mut pending_motion: Option<(i32, i32)> = None;
            let mut got_event = false;
            loop {
                let ev = match self.conn.poll_for_event() {
                    Ok(Some(ev)) => ev,
                    Ok(None) => break,
                    Err(e) => {
                        warn!("Grab and Move: X11 connection error: {e}");
                        running.store(false, Ordering::Relaxed);
                        return;
                    }
                };
                got_event = true;
                match ev {
                    Event::ButtonPress(e) => {
                        current = self.on_press(&e, current.take());
                    }
                    Event::MotionNotify(e) => {
                        pending_motion = Some((e.root_x as i32, e.root_y as i32));
                    }
                    Event::ButtonRelease(e) => {
                        if let Some(mut it) = current.take() {
                            if let Some(p) = pending_motion.take() {
                                self.on_motion(&mut it, p);
                            }
                            if it.button != e.detail {
                                current = Some(it);
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let (Some(it), Some(p)) = (current.as_mut(), pending_motion) {
                self.on_motion(it, p);
            }
            let _ = self.conn.flush();
            if !got_event {
                std::thread::sleep(Duration::from_millis(8));
            }
        }
        self.ungrab();
        info!("Grab and Move: stopped");
    }

    fn allow(&self, mode: Allow, time: u32) {
        let _ = self.conn.allow_events(mode, time);
        let _ = self.conn.flush();
    }

    fn on_press(
        &mut self,
        e: &xproto::ButtonPressEvent,
        existing: Option<Interaction>,
    ) -> Option<Interaction> {
        if existing.is_some() {
            self.allow(Allow::ASYNC_POINTER, e.time);
            return existing;
        }
        let action = match e.detail {
            1 => Action::Move,
            3 if self.cfg.use_alt_resize => Action::Resize,
            _ => {
                self.allow(Allow::REPLAY_POINTER, e.time);
                return None;
            }
        };
        match self.prepare(e, action) {
            Some(it) => {
                self.allow(Allow::ASYNC_POINTER, e.time);
                self.set_cursor(match action {
                    Action::Move => None,
                    Action::Resize => Some(it.handle),
                });
                Some(it)
            }
            None => {
                // Not ours: deliver the click to the application as usual.
                self.allow(Allow::REPLAY_POINTER, e.time);
                None
            }
        }
    }

    fn prepare(&self, e: &xproto::ButtonPressEvent, action: Action) -> Option<Interaction> {
        if e.child == NONE {
            return None;
        }
        // Passthrough while the active app is excluded for the modifier, or
        // while a fullscreen app (game) is active.
        if let Some(active) = self.active_window() {
            if self.is_app_in(active, &self.cfg.excluded_apps_for_modifier) {
                debug!("Grab and Move: active app excluded for modifier");
                return None;
            }
            if self.cfg.do_not_activate_on_game_mode
                && self.has_state(active, self.atoms.fullscreen)
            {
                debug!("Grab and Move: active window is fullscreen");
                return None;
            }
        }
        let client = self.find_client(e.child, 4)?;
        if !self.is_eligible(client) {
            return None;
        }

        let geo = self.conn.get_geometry(client).ok()?.reply().ok()?;
        let pos = self
            .conn
            .translate_coordinates(client, self.root, 0, 0)
            .ok()?
            .reply()
            .ok()?;
        let rect = Rect::new(
            pos.dst_x as i32,
            pos.dst_y as i32,
            geo.width as i32,
            geo.height as i32,
        );

        let hints = WmSizeHints::get_normal_hints(&self.conn, client)
            .ok()
            .and_then(|c| c.reply().ok().flatten());
        let min_hint = hints.as_ref().and_then(|h| h.min_size);
        let max_hint = hints.as_ref().and_then(|h| h.max_size);
        if action == Action::Resize
            && let (Some(min), Some(max)) = (min_hint, max_hint)
            && min == max
        {
            debug!("Grab and Move: window {client:#x} is not resizable");
            return None;
        }
        let (mw, mh) = min_hint.unwrap_or((0, 0));
        let pt = (e.root_x as i32, e.root_y as i32);
        Some(Interaction {
            client,
            action,
            button: e.detail,
            start: pt,
            last: pt,
            rect,
            origin: rect,
            handle: geometry::closest_handle(pt.0, pt.1, rect),
            min: (
                mw.max(geometry::MIN_WINDOW_WIDTH),
                mh.max(geometry::MIN_WINDOW_HEIGHT),
            ),
            active: false,
            maximized: self.has_state(client, self.atoms.max_vert)
                || self.has_state(client, self.atoms.max_horz),
        })
    }

    fn on_motion(&self, it: &mut Interaction, pt: (i32, i32)) {
        if !it.active {
            if !geometry::past_threshold(
                pt.0 - it.start.0,
                pt.1 - it.start.1,
                self.cfg.drag_threshold,
            ) {
                return;
            }
            it.active = true;
            if it.maximized {
                self.unmaximize(it, pt);
            }
        }
        match it.action {
            Action::Move => {
                let r = geometry::apply_move(it.origin, pt.0 - it.start.0, pt.1 - it.start.1);
                it.rect = r;
                self.move_resize(it.client, r, false);
            }
            Action::Resize => {
                let handle = geometry::closest_handle(pt.0, pt.1, it.rect);
                if handle != it.handle {
                    it.handle = handle;
                    self.set_cursor(Some(handle));
                }
                let r = geometry::apply_resize(
                    it.rect,
                    it.handle,
                    pt.0 - it.last.0,
                    pt.1 - it.last.1,
                    it.min.0,
                    it.min.1,
                );
                it.rect = r;
                self.move_resize(it.client, r, true);
            }
        }
        it.last = pt;
    }

    /// Restore a maximized window, keeping the grab point proportional.
    fn unmaximize(&self, it: &mut Interaction, pt: (i32, i32)) {
        let ev = xproto::ClientMessageEvent::new(
            32,
            it.client,
            self.atoms.net_wm_state,
            [0, self.atoms.max_vert, self.atoms.max_horz, 2, 0],
        );
        self.send_root(ev);
        let _ = self.conn.flush();
        // Give the WM a moment to apply the restored size.
        std::thread::sleep(Duration::from_millis(50));
        let (w, h) = self
            .conn
            .get_geometry(it.client)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|g| (g.width as i32, g.height as i32))
            .unwrap_or((it.rect.w, it.rect.h));
        let (x, y) = geometry::restore_position(it.start, it.rect, w, h);
        it.origin = Rect::new(x + pt.0 - it.start.0, y + pt.1 - it.start.1, w, h);
        it.rect = it.origin;
        it.start = pt;
        it.last = pt;
        it.maximized = false;
    }

    fn move_resize(&self, client: Window, r: Rect, with_size: bool) {
        if self.use_ewmh {
            // gravity Static (10): x/y are the client's own position;
            // flags: x, y (and w, h); source indication 2 (user/pager).
            let mut flags: u32 = 10 | (1 << 8) | (1 << 9) | (2 << 12);
            if with_size {
                flags |= (1 << 10) | (1 << 11);
            }
            let ev = xproto::ClientMessageEvent::new(
                32,
                client,
                self.atoms.net_moveresize,
                [
                    flags,
                    r.x as u32,
                    r.y as u32,
                    r.w.max(1) as u32,
                    r.h.max(1) as u32,
                ],
            );
            self.send_root(ev);
        } else {
            let mut aux = ConfigureWindowAux::new().x(r.x).y(r.y);
            if with_size {
                aux = aux.width(r.w.max(1) as u32).height(r.h.max(1) as u32);
            }
            let _ = self.conn.configure_window(client, &aux);
        }
    }

    fn send_root(&self, ev: xproto::ClientMessageEvent) {
        let _ = self.conn.send_event(
            false,
            self.root,
            EventMask::SUBSTRUCTURE_REDIRECT | EventMask::SUBSTRUCTURE_NOTIFY,
            ev,
        );
    }

    fn set_cursor(&self, handle: Option<Handle>) {
        let Some(c) = &self.cursors else { return };
        let cursor = match handle {
            None => c.moving,
            Some(h) => c
                .handles
                .iter()
                .find(|(k, _)| *k == h)
                .map(|(_, c)| *c)
                .unwrap_or(c.moving),
        };
        let _ = self
            .conn
            .change_active_pointer_grab(cursor, CURRENT_TIME, event_mask());
    }

    fn active_window(&self) -> Option<Window> {
        prop32(
            &self.conn,
            self.root,
            self.atoms.net_active,
            AtomEnum::WINDOW,
        )
        .first()
        .copied()
        .filter(|&w| w != NONE && w != self.root)
    }

    fn has_state(&self, window: Window, state: Atom) -> bool {
        prop32(&self.conn, window, self.atoms.net_wm_state, AtomEnum::ATOM).contains(&state)
    }

    fn has_wm_state(&self, window: Window) -> bool {
        self.conn
            .get_property(false, window, self.atoms.wm_state, AtomEnum::ANY, 0, 0)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some_and(|r| r.type_ != NONE)
    }

    /// Find the managed client window (has WM_STATE) at or below `window`.
    fn find_client(&self, window: Window, depth: u32) -> Option<Window> {
        if self.has_wm_state(window) {
            return Some(window);
        }
        if depth == 0 {
            return None;
        }
        let tree = self.conn.query_tree(window).ok()?.reply().ok()?;
        // Topmost children first.
        tree.children
            .iter()
            .rev()
            .find_map(|&c| self.find_client(c, depth - 1))
    }

    fn is_eligible(&self, client: Window) -> bool {
        let types = prop32(&self.conn, client, self.atoms.wm_type, AtomEnum::ATOM);
        if types
            .iter()
            .any(|t| *t == self.atoms.type_desktop || *t == self.atoms.type_dock)
        {
            return false;
        }
        if self.has_state(client, self.atoms.fullscreen) {
            return false;
        }
        if self.is_app_in(client, &self.cfg.excluded_apps) {
            debug!("Grab and Move: window {client:#x} is excluded");
            return false;
        }
        true
    }

    fn is_app_in(&self, window: Window, list: &[String]) -> bool {
        if list.is_empty() {
            return false;
        }
        let (instance, class) = self
            .conn
            .get_property(false, window, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| {
                let s = String::from_utf8_lossy(&r.value).to_string();
                let mut it = s.split('\0');
                (
                    it.next().unwrap_or("").to_string(),
                    it.next().unwrap_or("").to_string(),
                )
            })
            .unwrap_or_default();
        let exe = prop32(&self.conn, window, self.atoms.wm_pid, AtomEnum::CARDINAL)
            .first()
            .and_then(|pid| std::fs::read_link(format!("/proc/{pid}/exe")).ok())
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .unwrap_or_default();
        let title = self
            .conn
            .get_property(false, window, self.atoms.wm_name, self.atoms.utf8, 0, 1024)
            .ok()
            .and_then(|c| c.reply().ok())
            .map(|r| String::from_utf8_lossy(&r.value).to_string())
            .unwrap_or_default();
        geometry::matches_app(list, &instance, &class, &exe, &title)
    }
}

fn prop32(conn: &RustConnection, window: Window, prop: Atom, ty: impl Into<Atom>) -> Vec<u32> {
    conn.get_property(false, window, prop, ty, 0, 1024)
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|r| r.value32().map(|v| v.collect()))
        .unwrap_or_default()
}

fn create_cursors(conn: &RustConnection) -> Result<Cursors> {
    let font = conn.generate_id()?;
    conn.open_font(font, b"cursor")?.check()?;
    let make = |glyph: u16| -> Result<Cursor> {
        let id = conn.generate_id()?;
        conn.create_glyph_cursor(
            id,
            font,
            font,
            glyph,
            glyph + 1,
            0,
            0,
            0,
            0xffff,
            0xffff,
            0xffff,
        )?;
        Ok(id)
    };
    let all = [
        Handle::TopLeft,
        Handle::Top,
        Handle::TopRight,
        Handle::Right,
        Handle::BottomRight,
        Handle::Bottom,
        Handle::BottomLeft,
        Handle::Left,
    ];
    let mut handles = [(Handle::TopLeft, NONE); 8];
    for (slot, h) in handles.iter_mut().zip(all) {
        *slot = (h, make(h.cursor_glyph())?);
    }
    let moving = make(MOVE_CURSOR_GLYPH)?;
    let _ = conn.close_font(font);
    Ok(Cursors { moving, handles })
}
