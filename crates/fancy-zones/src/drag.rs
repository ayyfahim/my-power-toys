//! Background thread: drag-to-snap and new-window placement (X11).
//!
//! Drag detection polls `QueryPointer` at ~60 Hz. While button 1 is held, the
//! active window's position is compared with its position at press time; a
//! position change with an unchanged size means the WM is moving it. This works
//! with any WM without grabbing the pointer.

use crate::config::FancyZonesConfig;
use crate::geometry::{self, OverlapAlgorithm, Rect};
use crate::snap::{self, Target};
use crate::state::SnapState;
use crate::x11::{Pointer, Wm};
use crate::zone_window::{ZoneOverlay, ZoneStyle};
use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::Event;

const TICK: Duration = Duration::from_millis(16);
const CONFIG_CHECK: Duration = Duration::from_secs(1);
const NEW_WINDOW_DELAY: Duration = Duration::from_millis(200);
const DRAG_OPACITY: f32 = 0.5;

/// Handle to the running background thread.
pub struct DragThread {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl DragThread {
    /// Connect to X11 and start the thread. Fails if X11 is unavailable.
    pub fn spawn() -> Result<Self> {
        // Connect up front so failures are reported to the caller.
        let wm = Wm::connect()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        let handle = std::thread::Builder::new()
            .name("fancy-zones-drag".into())
            .spawn(move || {
                if let Err(e) = run(wm, &stop2) {
                    warn!("FancyZones drag thread stopped: {e}");
                }
            })?;
        Ok(Self {
            stop,
            handle: Some(handle),
        })
    }

    pub fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for DragThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Drag {
    window: u32,
    excluded: bool,
    /// Client size at drag start (for `restore_size`).
    start_size: (u32, u32),
    targets: Vec<Target>,
    prev: Pointer,
    mouse_toggle: bool,
    span_toggle: bool,
    /// (target, zone) where multi-zone spanning started.
    anchor: Option<(usize, usize)>,
    /// Current selection (target, zones).
    selection: Option<(usize, Vec<usize>)>,
    /// Target(s) currently shown in the overlay (None = overlay hidden).
    shown_for: Option<Option<usize>>,
    transparent: bool,
}

enum State {
    Idle,
    /// Button 1 is down; waiting to see if the active window moves.
    Pressed {
        window: Option<u32>,
        start: Option<Rect>,
    },
    /// Button 1 down but not a window move (e.g. resize or in-app drag).
    Ignored,
    Dragging(Box<Drag>),
}

fn config_mtime() -> Option<SystemTime> {
    let path = mpt_common::config::config_dir()
        .ok()?
        .join("fancy-zones.toml");
    std::fs::metadata(path).ok()?.modified().ok()
}

struct Ctx {
    wm: Wm,
    cfg: FancyZonesConfig,
    overlay: Option<ZoneOverlay>,
    known_clients: HashSet<u32>,
    pending_new: Vec<(u32, Instant)>,
}

fn run(wm: Wm, stop: &AtomicBool) -> Result<()> {
    let cfg = FancyZonesConfig::load();
    wm.watch_root_properties()?;
    let known_clients: HashSet<u32> = wm.client_list().into_iter().collect();
    // Drop stale window entries from previous sessions.
    {
        let live: Vec<u32> = known_clients.iter().copied().collect();
        SnapState::update(|s| s.prune(&live));
    }
    let overlay = match ZoneOverlay::new(&wm, ZoneStyle::from_config(&cfg)) {
        Ok(o) => Some(o),
        Err(e) => {
            warn!("FancyZones: zone overlay unavailable ({e}); drag-snap still works blind");
            None
        }
    };
    let mut ctx = Ctx {
        wm,
        cfg,
        overlay,
        known_clients,
        pending_new: Vec::new(),
    };
    info!("FancyZones drag-to-snap thread running");

    let mut state = State::Idle;
    let mut mtime = config_mtime();
    let mut last_check = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        if last_check.elapsed() >= CONFIG_CHECK {
            last_check = Instant::now();
            let m = config_mtime();
            if m != mtime {
                mtime = m;
                ctx.cfg = FancyZonesConfig::load();
                if let Some(o) = ctx.overlay.as_mut() {
                    o.set_style(ZoneStyle::from_config(&ctx.cfg));
                }
                debug!("FancyZones: config reloaded");
            }
        }

        ctx.drain_events();
        ctx.place_new_windows();

        if let Some(p) = ctx.wm.pointer() {
            state = ctx.step(state, p);
        }
        std::thread::sleep(TICK);
    }

    if let State::Dragging(mut d) = state {
        ctx.end_visuals(&mut d);
    }
    if let Some(o) = ctx.overlay.as_mut() {
        o.hide(&ctx.wm);
    }
    Ok(())
}

impl Ctx {
    fn drain_events(&mut self) {
        let mut client_list_changed = false;
        while let Ok(Some(ev)) = self.wm.conn.poll_for_event() {
            match ev {
                Event::Expose(e) => {
                    if let Some(o) = &self.overlay {
                        o.handle_expose(&self.wm, e.window);
                    }
                }
                Event::PropertyNotify(e)
                    if e.window == self.wm.root && e.atom == self.wm.atoms.net_client_list =>
                {
                    client_list_changed = true;
                }
                _ => {}
            }
        }
        if client_list_changed {
            let now: HashSet<u32> = self.wm.client_list().into_iter().collect();
            if self.cfg.app_last_zone_move_windows {
                for &w in now.difference(&self.known_clients) {
                    self.pending_new
                        .push((w, Instant::now() + NEW_WINDOW_DELAY));
                }
            }
            let closed: Vec<u32> = self.known_clients.difference(&now).copied().collect();
            if !closed.is_empty() {
                SnapState::update(|s| {
                    for w in &closed {
                        s.windows.remove(w);
                    }
                });
            }
            self.known_clients = now;
        }
    }

    fn place_new_windows(&mut self) {
        if self.pending_new.is_empty() {
            return;
        }
        let now = Instant::now();
        let (due, later): (Vec<_>, Vec<_>) =
            self.pending_new.drain(..).partition(|(_, t)| *t <= now);
        self.pending_new = later;
        if due.is_empty() || !self.cfg.app_last_zone_move_windows {
            return;
        }
        let targets = snap::build_targets(&self.cfg, &snap::monitors());
        for (w, _) in due {
            match snap::apply_app_last_zone(&self.wm, &self.cfg, &targets, w) {
                Ok(true) => info!("FancyZones: restored window {w} to its app's last zone"),
                Ok(false) => {}
                Err(e) => debug!("FancyZones: app-last-zone for {w}: {e}"),
            }
        }
    }

    fn step(&mut self, state: State, p: Pointer) -> State {
        match state {
            State::Idle => {
                if p.button1 && self.cfg.drag_snap_enabled {
                    let window = self.wm.active_window();
                    let start = window.and_then(|w| self.wm.geometry(w));
                    State::Pressed { window, start }
                } else {
                    State::Idle
                }
            }
            State::Ignored => {
                if p.button1 {
                    State::Ignored
                } else {
                    State::Idle
                }
            }
            State::Pressed { window, start } => {
                if !p.button1 {
                    return State::Idle;
                }
                let active = self.wm.active_window();
                if active != window {
                    // Focus changed on click: track the newly active window.
                    let start = active.and_then(|w| self.wm.geometry(w));
                    return State::Pressed {
                        window: active,
                        start,
                    };
                }
                let (Some(w), Some(s)) = (window, start) else {
                    return State::Pressed { window, start };
                };
                let Some(g) = self.wm.geometry(w) else {
                    return State::Ignored;
                };
                if (g.w, g.h) != (s.w, s.h) {
                    return State::Ignored; // resize, not move
                }
                if (g.x, g.y) != (s.x, s.y) {
                    return State::Dragging(Box::new(self.begin_drag(w, s, p)));
                }
                State::Pressed { window, start }
            }
            State::Dragging(mut d) => {
                if p.button1 {
                    self.update_drag(&mut d, p);
                    State::Dragging(d)
                } else {
                    self.finish_drag(&mut d);
                    State::Idle
                }
            }
        }
    }

    fn begin_drag(&mut self, window: u32, start: Rect, p: Pointer) -> Drag {
        let excluded = self
            .wm
            .wm_class(window)
            .map(|c| self.cfg.is_excluded(&c))
            .unwrap_or(false);
        debug!("FancyZones: drag started for window {window} (excluded={excluded})");
        Drag {
            window,
            excluded,
            start_size: (start.w as u32, start.h as u32),
            targets: snap::build_targets(&self.cfg, &snap::monitors()),
            prev: p,
            mouse_toggle: false,
            span_toggle: false,
            anchor: None,
            selection: None,
            shown_for: None,
            transparent: false,
        }
    }

    fn update_drag(&mut self, d: &mut Drag, p: Pointer) {
        let cfg = &self.cfg;
        if cfg.mouse_switch && p.button3 && !d.prev.button3 {
            d.mouse_toggle = !d.mouse_toggle;
        }
        if cfg.mouse_middle_click_spanning_multiple_zones && p.button2 && !d.prev.button2 {
            d.span_toggle = !d.span_toggle;
        }
        d.prev = p;

        let active =
            !d.excluded && geometry::drag_zones_active(cfg.shift_drag, p.shift, d.mouse_toggle);
        if !active {
            d.selection = None;
            d.anchor = None;
            self.end_visuals(d);
            return;
        }

        let t = snap::target_at(&d.targets, p.x, p.y);
        // (Re)show overlay when the monitor under the cursor changes.
        let want = if cfg.show_on_all_monitors { None } else { t };
        if d.shown_for != Some(want)
            && let Some(o) = self.overlay.as_mut()
        {
            let specs = match want {
                Some(i) => vec![d.targets[i].spec(None)],
                None => d.targets.iter().map(|t| t.spec(None)).collect(),
            };
            let _ = o.show(&self.wm, specs);
            d.shown_for = Some(want);
        }
        if cfg.make_dragged_window_transparent && !d.transparent {
            self.wm.set_opacity(d.window, Some(DRAG_OPACITY));
            d.transparent = true;
        }

        let algo = OverlapAlgorithm::from_index(cfg.overlapping_zones_algorithm);
        let zone = t.and_then(|ti| {
            geometry::zone_at_point(&d.targets[ti].zones, p.x, p.y, algo).map(|z| (ti, z))
        });
        let multi = (cfg.multi_zone_span && p.ctrl) || d.span_toggle;
        d.selection = match zone {
            Some((ti, z)) if multi => {
                let anchor = match d.anchor {
                    Some((at, az)) if at == ti => az,
                    _ => z,
                };
                d.anchor = Some((ti, anchor));
                Some((ti, geometry::zones_in_span(&d.targets[ti].zones, anchor, z)))
            }
            Some((ti, z)) => {
                d.anchor = None;
                Some((ti, vec![z]))
            }
            None => {
                if !multi {
                    d.anchor = None;
                }
                None
            }
        };

        if let Some(o) = self.overlay.as_mut() {
            let (surface, zones) = match &d.selection {
                Some((ti, zs)) => {
                    // Surface index: single-monitor overlay has one surface.
                    let s = if cfg.show_on_all_monitors { *ti } else { 0 };
                    (Some(s), zs.clone())
                }
                None => (None, Vec::new()),
            };
            o.set_highlight(&self.wm, surface, &zones);
        }
    }

    fn end_visuals(&mut self, d: &mut Drag) {
        if let Some(o) = self.overlay.as_mut()
            && o.is_shown()
        {
            o.hide(&self.wm);
        }
        d.shown_for = None;
        if d.transparent {
            self.wm.set_opacity(d.window, None);
            d.transparent = false;
        }
    }

    fn finish_drag(&mut self, d: &mut Drag) {
        self.end_visuals(d);
        if let Some((ti, zones)) = &d.selection
            && !zones.is_empty()
        {
            if let Err(e) = snap::snap_window(
                &self.wm,
                &self.cfg,
                d.window,
                &d.targets[*ti],
                zones,
                Some(d.start_size),
            ) {
                warn!("FancyZones: drag snap failed: {e}");
            }
            return;
        }
        // Dropped outside a zone: unsnap, optionally restoring the original size.
        let prev = SnapState::update(|s| s.unsnap(d.window));
        if self.cfg.restore_size
            && let Some(snap) = prev
            && let Some((w, h)) = snap.original_size
        {
            info!("FancyZones: restoring window {} to {w}x{h}", d.window);
            if let Err(e) = self.wm.resize_client(d.window, w, h) {
                warn!("FancyZones: restore size failed: {e}");
            }
        }
    }
}
