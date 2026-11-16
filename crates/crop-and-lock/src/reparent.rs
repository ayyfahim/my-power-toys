//! Reparent mode: move the real window into a container window sized to the
//! crop region (child offset by -x,-y), and put it back when the container
//! closes.
//!
//! Upstream uses `SetParent`. On X11 the equivalent is `ReparentWindow`. The
//! target is added to our save-set, so if this process dies the X server
//! reparents it back to the root window and maps it again; the window
//! manager then manages it as a fresh top-level.

use crate::geometry::{CropRect, fixed_size_hints};
use crate::x11::{APP_CLASS, intern_atom, root_geometry};
use anyhow::{Context, Result, bail};
use std::time::Duration;
use tracing::{info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, InputFocus, PropMode, SetMode,
    Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::{COPY_DEPTH_FROM_PARENT, COPY_FROM_PARENT, CURRENT_TIME};

/// Why the reparent session ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReparentEnd {
    /// The user closed the container; the window was restored.
    Closed,
    /// The source window was destroyed by its application.
    SourceDestroyed,
    /// Another client (usually the WM) took the window back.
    SourceTaken,
}

/// Run a reparent session. Returns `Err` only if reparenting could not be
/// set up (the caller then falls back to thumbnail mode); in that case the
/// target window has been left at (or returned to) its original place.
pub fn run(target: Window, rect: CropRect, title: &str) -> Result<ReparentEnd> {
    let (conn, screen_num) = crate::x11::connect()?;
    let screen = conn.setup().roots[screen_num].clone();
    let root = screen.root;

    let geom = root_geometry(&conn, target)?;
    let rect = rect
        .clamp_to(geom.width, geom.height)
        .context("crop region is outside the window")?;
    let attrs = conn.get_window_attributes(target)?.reply()?;
    if attrs.override_redirect {
        bail!("window 0x{target:x} is override-redirect (menu/popup); not reparenting");
    }
    let orig_parent = conn.query_tree(target)?.reply()?.parent;
    let atoms = Atoms::new(&conn)?;

    let bw = geom.border_width as i32;
    let container_x = geom.x + rect.x as i32;
    let container_y = geom.y + rect.y as i32;

    let container = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        container,
        root,
        container_x as i16,
        container_y as i16,
        rect.width as u16,
        rect.height as u16,
        0,
        WindowClass::INPUT_OUTPUT,
        COPY_FROM_PARENT,
        &CreateWindowAux::new()
            .background_pixel(screen.black_pixel)
            .event_mask(
                EventMask::STRUCTURE_NOTIFY
                    | EventMask::SUBSTRUCTURE_NOTIFY
                    | EventMask::FOCUS_CHANGE,
            ),
    )?
    .check()
    .context("failed to create container window")?;

    set_container_properties(&conn, container, &atoms, title, rect)?;

    // From here on the container exists; clean it up on any setup failure.
    let session = Session {
        conn: &conn,
        root,
        target,
        container,
        restore_x: geom.x - bw,
        restore_y: geom.y - bw,
    };

    if let Err(e) = session.attach(rect, bw) {
        session.restore();
        return Err(e);
    }

    // Verify the window manager did not immediately grab the window back.
    std::thread::sleep(Duration::from_millis(150));
    let parent_now = conn.query_tree(target)?.reply().map(|t| t.parent).ok();
    if parent_now != Some(container) {
        session.restore();
        bail!(
            "window 0x{target:x} did not stay in the container (parent now {parent_now:?}, \
             originally 0x{orig_parent:x})"
        );
    }
    info!(
        "Reparented window 0x{target:x} into container 0x{container:x} ({}x{} at {},{})",
        rect.width, rect.height, rect.x, rect.y
    );

    let end = session.event_loop(&atoms);
    match end {
        ReparentEnd::Closed => session.restore(),
        ReparentEnd::SourceDestroyed | ReparentEnd::SourceTaken => session.destroy_container(),
    }
    Ok(end)
}

struct Atoms {
    wm_protocols: u32,
    wm_delete_window: u32,
    net_wm_name: u32,
    net_wm_pid: u32,
    utf8_string: u32,
}

impl Atoms {
    fn new(conn: &RustConnection) -> Result<Self> {
        Ok(Self {
            wm_protocols: intern_atom(conn, b"WM_PROTOCOLS")?,
            wm_delete_window: intern_atom(conn, b"WM_DELETE_WINDOW")?,
            net_wm_name: intern_atom(conn, b"_NET_WM_NAME")?,
            net_wm_pid: intern_atom(conn, b"_NET_WM_PID")?,
            utf8_string: intern_atom(conn, b"UTF8_STRING")?,
        })
    }
}

fn set_container_properties(
    conn: &RustConnection,
    container: Window,
    atoms: &Atoms,
    title: &str,
    rect: CropRect,
) -> Result<()> {
    conn.change_property8(
        PropMode::REPLACE,
        container,
        AtomEnum::WM_NAME,
        AtomEnum::STRING,
        title.as_bytes(),
    )?;
    conn.change_property8(
        PropMode::REPLACE,
        container,
        atoms.net_wm_name,
        atoms.utf8_string,
        title.as_bytes(),
    )?;
    let class = format!("{APP_CLASS}\0{APP_CLASS}\0");
    conn.change_property8(
        PropMode::REPLACE,
        container,
        AtomEnum::WM_CLASS,
        AtomEnum::STRING,
        class.as_bytes(),
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        container,
        atoms.wm_protocols,
        AtomEnum::ATOM,
        &[atoms.wm_delete_window],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        container,
        atoms.net_wm_pid,
        AtomEnum::CARDINAL,
        &[std::process::id()],
    )?;
    conn.change_property32(
        PropMode::REPLACE,
        container,
        AtomEnum::WM_NORMAL_HINTS,
        AtomEnum::WM_SIZE_HINTS,
        &fixed_size_hints(rect.width, rect.height),
    )?;
    Ok(())
}

struct Session<'a> {
    conn: &'a RustConnection,
    root: Window,
    target: Window,
    container: Window,
    /// Original absolute position of the target's outer (border) corner.
    restore_x: i32,
    restore_y: i32,
}

impl Session<'_> {
    fn attach(&self, rect: CropRect, bw: i32) -> Result<()> {
        let conn = self.conn;
        // Safety net: on our disconnect the server returns the window to root.
        conn.change_save_set(SetMode::INSERT, self.target)?
            .check()
            .context("ChangeSaveSet failed")?;
        conn.reparent_window(
            self.target,
            self.container,
            (-(rect.x as i32) - bw) as i16,
            (-(rect.y as i32) - bw) as i16,
        )?
        .check()
        .context("ReparentWindow failed")?;
        conn.map_window(self.target)?;
        conn.map_window(self.container)?;
        conn.flush()?;
        Ok(())
    }

    fn event_loop(&self, atoms: &Atoms) -> ReparentEnd {
        loop {
            let event = match self.conn.wait_for_event() {
                Ok(ev) => ev,
                Err(e) => {
                    warn!("X11 connection error in reparent loop: {e}");
                    return ReparentEnd::SourceTaken;
                }
            };
            match event {
                Event::ClientMessage(ev)
                    if ev.window == self.container
                        && ev.type_ == atoms.wm_protocols
                        && ev.data.as_data32()[0] == atoms.wm_delete_window =>
                {
                    info!("Reparent container closed by user");
                    return ReparentEnd::Closed;
                }
                Event::DestroyNotify(ev) if ev.window == self.target => {
                    info!("Source window 0x{:x} was destroyed", self.target);
                    return ReparentEnd::SourceDestroyed;
                }
                Event::DestroyNotify(ev) if ev.window == self.container => {
                    // Should not happen (only we destroy it), but the target
                    // would be gone with it.
                    return ReparentEnd::SourceDestroyed;
                }
                Event::ReparentNotify(ev)
                    if ev.window == self.target && ev.parent != self.container =>
                {
                    info!("Source window was reparented away by another client");
                    return ReparentEnd::SourceTaken;
                }
                Event::FocusIn(ev) if ev.event == self.container => {
                    // Forward keyboard focus to the embedded window.
                    let _ =
                        self.conn
                            .set_input_focus(InputFocus::PARENT, self.target, CURRENT_TIME);
                    let _ = self.conn.flush();
                }
                _ => {}
            }
        }
    }

    /// Put the target back at its original absolute position (as a child of
    /// the root window, so the window manager re-manages it) and destroy the
    /// container.
    fn restore(&self) {
        let conn = self.conn;
        if conn
            .get_geometry(self.target)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some()
        {
            let res = conn
                .reparent_window(
                    self.target,
                    self.root,
                    self.restore_x as i16,
                    self.restore_y as i16,
                )
                .ok()
                .and_then(|c| c.check().ok());
            if res.is_none() {
                warn!(
                    "Failed to restore window 0x{:x} to the root window",
                    self.target
                );
            }
            let _ = conn.map_window(self.target);
            let _ = conn.change_save_set(SetMode::DELETE, self.target);
            info!(
                "Restored window 0x{:x} at {},{}",
                self.target, self.restore_x, self.restore_y
            );
        }
        self.destroy_container();
    }

    fn destroy_container(&self) {
        // Only safe once the target is no longer a child of the container.
        let still_child = self
            .conn
            .query_tree(self.target)
            .ok()
            .and_then(|c| c.reply().ok())
            .is_some_and(|t| t.parent == self.container);
        if still_child {
            warn!("Not destroying container: source window is still inside it");
            // Exiting drops the connection; the save-set returns the window.
            return;
        }
        let _ = self.conn.destroy_window(self.container);
        let _ = self.conn.flush();
    }
}
