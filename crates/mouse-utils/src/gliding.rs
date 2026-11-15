//! Gliding Cursor (upstream: MousePointerCrosshairs "gliding cursor").
//!
//! Each press of the shortcut advances a state machine (same as upstream):
//! 1. start: a vertical line sweeps horizontally at travel speed,
//! 2. slow the horizontal sweep down to delay speed,
//! 3. lock X, a horizontal line sweeps vertically at travel speed,
//! 4. slow the vertical sweep down,
//! 5. left-click at the intersection and finish.
//!
//! Escape cancels at any point. Lines wrap around to the start of the screen
//! and return to travel speed when they reach the far edge.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, ConnectionExt, GrabMode, ModMask};
use x11rb::protocol::xtest::ConnectionExt as XtestConnectionExt;
use x11rb::rust_connection::RustConnection;

use crate::config::{CrosshairOrientation, MouseUtilsConfig};
use crate::crosshairs::{CrosshairLines, LineStyle};

const TICK: Duration = Duration::from_millis(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlidePhase {
    Idle,
    FastHorizontal,
    SlowHorizontal,
    FastVertical,
    SlowVertical,
}

/// Pure state of the gliding cursor (positions in pixels, speeds in px/s).
#[derive(Debug, Clone)]
pub struct GlideState {
    pub phase: GlidePhase,
    pub x: f64,
    pub y: f64,
    pub fast: f64,
    pub slow: f64,
    pub speed: f64,
    pub width: f64,
    pub height: f64,
}

/// What the caller must do after a shortcut press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlideAction {
    Started,
    Continue,
    /// Click at the given point and finish.
    Click(i32, i32),
}

impl GlideState {
    pub fn new(fast: u32, slow: u32, width: u16, height: u16) -> Self {
        Self {
            phase: GlidePhase::Idle,
            x: 0.0,
            y: 0.0,
            fast: fast.max(1) as f64,
            slow: slow.max(1) as f64,
            speed: 0.0,
            width: width.max(1) as f64,
            height: height.max(1) as f64,
        }
    }

    pub fn press(&mut self) -> GlideAction {
        match self.phase {
            GlidePhase::Idle => {
                self.x = 0.0;
                self.y = (self.height / 2.0).floor();
                self.speed = self.fast;
                self.phase = GlidePhase::FastHorizontal;
                GlideAction::Started
            }
            GlidePhase::FastHorizontal => {
                self.speed = self.slow;
                self.phase = GlidePhase::SlowHorizontal;
                GlideAction::Continue
            }
            GlidePhase::SlowHorizontal => {
                self.y = 0.0;
                self.speed = self.fast;
                self.phase = GlidePhase::FastVertical;
                GlideAction::Continue
            }
            GlidePhase::FastVertical => {
                self.speed = self.slow;
                self.phase = GlidePhase::SlowVertical;
                GlideAction::Continue
            }
            GlidePhase::SlowVertical => {
                self.phase = GlidePhase::Idle;
                GlideAction::Click(self.x as i32, self.y as i32)
            }
        }
    }

    pub fn cancel(&mut self) {
        self.phase = GlidePhase::Idle;
    }

    /// Advance the moving line by `dt`.
    pub fn tick(&mut self, dt: Duration) {
        let step = self.speed * dt.as_secs_f64();
        match self.phase {
            GlidePhase::FastHorizontal | GlidePhase::SlowHorizontal => {
                self.x += step;
                if self.x >= self.width {
                    self.x = 0.0;
                    self.speed = self.fast;
                    self.phase = GlidePhase::FastHorizontal;
                }
            }
            GlidePhase::FastVertical | GlidePhase::SlowVertical => {
                self.y += step;
                if self.y >= self.height {
                    self.y = 0.0;
                    self.speed = self.fast;
                    self.phase = GlidePhase::FastVertical;
                }
            }
            GlidePhase::Idle => {}
        }
    }
}

/// Spawn the gliding cursor thread. Setting `trigger` to true acts as one
/// press of the gliding cursor shortcut.
pub fn spawn(
    config: MouseUtilsConfig,
    trigger: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("gliding-cursor".into())
        .spawn(move || {
            if let Err(e) = run(config, trigger, stop) {
                warn!("Gliding Cursor stopped: {e}");
            }
        })
        .expect("failed to spawn gliding-cursor thread")
}

fn escape_keycodes(conn: &RustConnection) -> Result<Vec<u8>> {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let reply = conn
        .get_keyboard_mapping(min, max.saturating_sub(min).saturating_add(1))?
        .reply()
        .context("get_keyboard_mapping failed")?;
    let per = reply.keysyms_per_keycode.max(1) as usize;
    Ok(reply
        .keysyms
        .chunks(per)
        .enumerate()
        .filter(|(_, ks)| ks.contains(&xkeysym::key::Escape))
        .map(|(i, _)| min.saturating_add(i as u8))
        .collect())
}

fn run(config: MouseUtilsConfig, trigger: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> Result<()> {
    let (conn, screen_num) =
        RustConnection::connect(None).context("Gliding Cursor: failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let (root, sw, sh) = (screen.root, screen.width_in_pixels, screen.height_in_pixels);
    let escapes = escape_keycodes(&conn)?;

    let mut style = LineStyle::from_config(&config);
    // Upstream forces both lines, full length, while gliding.
    style.orientation = CrosshairOrientation::Both;
    style.fixed_length = None;

    let mut state = GlideState::new(
        config.gliding_cursor_travel_speed,
        config.gliding_cursor_delay_speed,
        sw,
        sh,
    );
    let mut lines: Option<CrosshairLines> = None;
    let mut last_tick = Instant::now();

    info!(
        "Gliding Cursor ready (travel={}px/s, delay={}px/s)",
        config.gliding_cursor_travel_speed, config.gliding_cursor_delay_speed
    );

    let finish = |conn: &RustConnection, lines: &mut Option<CrosshairLines>| {
        if let Some(l) = lines.take() {
            l.destroy(conn);
        }
        for &kc in &escapes {
            let _ = conn.ungrab_key(kc, root, ModMask::ANY);
        }
        let _ = conn.flush();
    };

    while !stop.load(Ordering::Relaxed) {
        let mut cancel = false;
        while let Some(event) = conn.poll_for_event()? {
            if let Event::KeyPress(k) = event
                && escapes.contains(&k.detail)
            {
                cancel = true;
            }
        }

        if cancel && state.phase != GlidePhase::Idle {
            debug!("Gliding Cursor: cancelled with Escape");
            state.cancel();
            finish(&conn, &mut lines);
        }

        if trigger.swap(false, Ordering::Relaxed) {
            match state.press() {
                GlideAction::Started => {
                    debug!("Gliding Cursor: started");
                    lines = Some(CrosshairLines::create(&conn, screen_num, style.clone())?);
                    for &kc in &escapes {
                        let _ = conn.grab_key(
                            false,
                            root,
                            ModMask::ANY,
                            kc,
                            GrabMode::ASYNC,
                            GrabMode::ASYNC,
                        );
                    }
                    last_tick = Instant::now();
                }
                GlideAction::Continue => debug!("Gliding Cursor: phase {:?}", state.phase),
                GlideAction::Click(x, y) => {
                    finish(&conn, &mut lines);
                    conn.warp_pointer(0u32, root, 0, 0, 0, 0, x as i16, y as i16)?;
                    conn.xtest_fake_input(xproto::BUTTON_PRESS_EVENT, 1, 0, root, 0, 0, 0)?;
                    conn.xtest_fake_input(xproto::BUTTON_RELEASE_EVENT, 1, 0, root, 0, 0, 0)?;
                    conn.flush()?;
                    info!("Gliding Cursor: clicked at ({x}, {y})");
                }
            }
        }

        if state.phase != GlidePhase::Idle {
            let now = Instant::now();
            state.tick(now - last_tick);
            last_tick = now;
            let (x, y) = (state.x as i16, state.y as i16);
            conn.warp_pointer(0u32, root, 0, 0, 0, 0, x, y)?;
            if let Some(l) = lines.as_mut() {
                l.update(&conn, x, y)?;
            }
            conn.flush()?;
            thread::sleep(TICK);
        } else {
            thread::sleep(Duration::from_millis(30));
        }
    }

    finish(&conn, &mut lines);
    info!("Gliding Cursor stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_sequence_clicks_at_intersection() {
        let mut s = GlideState::new(1000, 100, 1920, 1080);
        assert_eq!(s.press(), GlideAction::Started);
        assert_eq!((s.x, s.y), (0.0, 540.0));
        s.tick(Duration::from_millis(500)); // fast: +500
        assert_eq!(s.x, 500.0);
        assert_eq!(s.press(), GlideAction::Continue); // slow horizontal
        s.tick(Duration::from_millis(500)); // slow: +50
        assert_eq!(s.x, 550.0);
        assert_eq!(s.press(), GlideAction::Continue); // vertical fast from top
        assert_eq!(s.y, 0.0);
        s.tick(Duration::from_millis(300));
        assert_eq!(s.x, 550.0);
        assert_eq!(s.y, 300.0);
        assert_eq!(s.press(), GlideAction::Continue); // slow vertical
        s.tick(Duration::from_millis(200));
        assert_eq!(s.y, 320.0);
        assert_eq!(s.press(), GlideAction::Click(550, 320));
        assert_eq!(s.phase, GlidePhase::Idle);
    }

    #[test]
    fn wraps_and_resets_to_fast() {
        let mut s = GlideState::new(1000, 100, 800, 600);
        s.press();
        s.press(); // slow
        s.x = 799.0;
        s.tick(Duration::from_millis(100));
        assert_eq!(s.x, 0.0);
        assert_eq!(s.phase, GlidePhase::FastHorizontal);
        assert_eq!(s.speed, 1000.0);
    }

    #[test]
    fn cancel_returns_to_idle() {
        let mut s = GlideState::new(1000, 100, 800, 600);
        s.press();
        s.cancel();
        assert_eq!(s.phase, GlidePhase::Idle);
        s.tick(Duration::from_secs(1));
        assert_eq!(s.x, 0.0);
    }
}
