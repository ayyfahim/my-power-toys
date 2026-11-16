use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xinput::{self, ConnectionExt as XiConnectionExt};
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;

use crate::config::{FindMyMouseActivation, MouseUtilsConfig, is_app_excluded, parse_app_list};
use crate::x11util;

/// Spawn the Find My Mouse monitor thread.
/// Watches for the configured activation (double Ctrl, shake, etc.) and shows a spotlight overlay.
pub fn spawn(config: MouseUtilsConfig, stop: Arc<AtomicBool>) -> thread::JoinHandle<()> {
    spawn_with_trigger(config, Arc::new(AtomicBool::new(false)), stop)
}

/// Like [`spawn`], but `trigger` can be set from outside (custom shortcut
/// activation) to toggle the spotlight.
pub fn spawn_with_trigger(
    config: MouseUtilsConfig,
    trigger: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) -> thread::JoinHandle<()> {
    thread::Builder::new()
        .name("find-my-mouse".into())
        .spawn(move || {
            if let Err(e) = run(config, trigger, stop) {
                warn!("Find My Mouse stopped: {e}");
            }
        })
        .expect("failed to spawn find-my-mouse thread")
}

// ── Shake detection (port of upstream SuperSonar::DetectShake) ─────────

#[derive(Debug, Clone, Copy)]
struct Movement {
    dx: i64,
    dy: i64,
    tick_ms: u64,
}

/// Detects a mouse "shake": consecutive same-direction movements are merged;
/// whenever the direction changes, the movements within the last
/// `interval_ms` are examined. A shake is reported when the total distance
/// travelled is at least `min_distance` and exceeds the diagonal of the
/// bounding box of the movement by `factor_percent`%.
#[derive(Debug, Clone)]
pub struct ShakeDetector {
    history: Vec<Movement>,
    min_distance: f64,
    interval_ms: u64,
    factor_percent: f64,
}

fn sign(v: i64) -> i8 {
    v.signum() as i8
}

impl ShakeDetector {
    pub fn new(min_distance: u32, interval_ms: u32, factor_percent: u32) -> Self {
        Self {
            history: Vec::new(),
            min_distance: min_distance as f64,
            interval_ms: interval_ms.max(1) as u64,
            factor_percent: factor_percent.max(1) as f64,
        }
    }

    pub fn from_config(config: &MouseUtilsConfig) -> Self {
        Self::new(
            config.find_my_mouse_shake_distance,
            config.find_my_mouse_shake_interval_ms,
            config.find_my_mouse_shake_factor,
        )
    }

    /// Feed a relative pointer movement. Returns `true` when a shake is detected.
    pub fn push(&mut self, dx: i64, dy: i64, now_ms: u64) -> bool {
        if dx == 0 && dy == 0 {
            return false;
        }
        match self.history.last_mut() {
            Some(last) if sign(last.dx) == sign(dx) && sign(last.dy) == sign(dy) => {
                last.dx += dx;
                last.dy += dy;
                false
            }
            Some(_) => {
                self.history.push(Movement {
                    dx,
                    dy,
                    tick_ms: now_ms,
                });
                // Direction changed: check for a shake.
                self.detect(now_ms)
            }
            None => {
                self.history.push(Movement {
                    dx,
                    dy,
                    tick_ms: now_ms,
                });
                false
            }
        }
    }

    fn detect(&mut self, now_ms: u64) -> bool {
        let start = now_ms.saturating_sub(self.interval_ms);
        self.history.retain(|m| m.tick_ms >= start);

        let (mut x, mut y) = (0i64, 0i64);
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (0i64, 0i64, 0i64, 0i64);
        let mut travelled = 0.0f64;
        for m in &self.history {
            x += m.dx;
            y += m.dy;
            travelled += ((m.dx * m.dx + m.dy * m.dy) as f64).sqrt();
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            min_y = min_y.min(y);
            max_y = max_y.max(y);
        }
        if travelled < self.min_distance {
            return false;
        }
        let w = (max_x - min_x) as f64;
        let h = (max_y - min_y) as f64;
        let diagonal = (w * w + h * h).sqrt();
        if diagonal > 0.0 && travelled / diagonal > self.factor_percent / 100.0 {
            self.history.clear();
            return true;
        }
        false
    }
}

// ── Main loop ──────────────────────────────────────────────────────────

/// Whether Find My Mouse may activate now (game mode / excluded apps).
fn activation_allowed(config: &MouseUtilsConfig, conn: &RustConnection, root: u32) -> bool {
    let Some(active) = x11util::active_window(conn, root) else {
        return true;
    };
    if config.find_my_mouse_game_mode && x11util::is_fullscreen(conn, root, active) {
        debug!("Find My Mouse: active window is fullscreen (game mode), not activating");
        return false;
    }
    let excluded = parse_app_list(&config.find_my_mouse_excluded_apps);
    if !excluded.is_empty() {
        let class = x11util::wm_class(conn, active);
        if is_app_excluded(&excluded, &class) {
            debug!("Find My Mouse: focused app {class:?} is excluded");
            return false;
        }
    }
    true
}

fn run(config: MouseUtilsConfig, trigger: Arc<AtomicBool>, stop: Arc<AtomicBool>) -> Result<()> {
    let (conn, screen_num) =
        RustConnection::connect(None).context("Find My Mouse: failed to connect to X11")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;

    // Select XI2 raw key events on root window for double-Ctrl detection
    // XI_RawKeyPress = bit 13, XI_RawKeyRelease = bit 14
    // XI_RawButtonPress = bit 15 (for dismiss on click)
    let mask: u32 = (1 << 13) | (1 << 14) | (1 << 15);
    let event_mask = xinput::EventMask {
        deviceid: 3, // XIAllMasterDevices
        mask: vec![mask.into()],
    };
    conn.xinput_xi_select_events(root, &[event_mask])?
        .check()
        .context("Find My Mouse: failed to select XInput2 events")?;
    conn.flush()?;

    // Build keycode -> keysym map
    let keymap = build_keymap(&conn)?;

    let activation = config.find_my_mouse_activation;
    info!("Find My Mouse monitor active (activation={activation:?})");

    let epoch = Instant::now();
    let mut shake = ShakeDetector::from_config(&config);
    let mut last_pos: Option<(i16, i16)> = None;
    let mut last_ctrl_time: Option<Instant> = None;
    let mut overlay: Option<SpotlightOverlay> = None;

    while !stop.load(Ordering::Relaxed) {
        let mut want_activate = false;
        let mut want_dismiss = false;

        // Custom shortcut (delivered by the daemon as a named action).
        if trigger.swap(false, Ordering::Relaxed) {
            if overlay.is_some() {
                want_dismiss = true;
            } else if activation == FindMyMouseActivation::CustomShortcut {
                want_activate = true;
            }
        }

        while let Some(event) = conn.poll_for_event()? {
            match &event {
                x11rb::protocol::Event::XinputRawKeyPress(e) => {
                    if overlay.is_some() {
                        // Any key dismisses the spotlight
                        want_dismiss = true;
                        continue;
                    }
                    let keysym = keymap.get(&(e.detail as u8)).copied().unwrap_or(0);
                    let target = match activation {
                        FindMyMouseActivation::LeftCtrlTwice => Some(xkeysym::key::Control_L),
                        FindMyMouseActivation::RightCtrlTwice => Some(xkeysym::key::Control_R),
                        _ => None,
                    };
                    if target.is_some_and(|t| t == keysym) {
                        let now = Instant::now();
                        if let Some(last) = last_ctrl_time
                            && now.duration_since(last) < Duration::from_millis(400)
                        {
                            debug!("Find My Mouse: double Ctrl detected");
                            last_ctrl_time = None;
                            want_activate = true;
                        } else {
                            last_ctrl_time = Some(now);
                        }
                    } else {
                        // Another key in between breaks the double-press sequence.
                        last_ctrl_time = None;
                    }
                }
                x11rb::protocol::Event::XinputRawButtonPress(_) if overlay.is_some() => {
                    want_dismiss = true;
                }
                _ => {}
            }
        }

        // Shake detection via pointer polling.
        if activation == FindMyMouseActivation::ShakeMouse && overlay.is_none() {
            let pos = get_cursor_position(&conn, root)?;
            if let Some((lx, ly)) = last_pos {
                let now_ms = epoch.elapsed().as_millis() as u64;
                if shake.push((pos.0 - lx) as i64, (pos.1 - ly) as i64, now_ms) {
                    debug!("Find My Mouse: shake detected");
                    want_activate = true;
                }
            }
            last_pos = Some(pos);
        } else {
            last_pos = None;
        }

        if want_dismiss {
            if let Some(ov) = overlay.take() {
                debug!("Find My Mouse: dismissed");
                ov.destroy()?;
            }
        } else if want_activate && overlay.is_none() && activation_allowed(&config, &conn, root) {
            overlay = Some(SpotlightOverlay::create(&config)?);
        }

        if let Some(ref mut ov) = overlay {
            ov.update_cursor()?;
        }
        thread::sleep(Duration::from_millis(16));
    }

    if let Some(ov) = overlay.take() {
        let _ = ov.destroy();
    }

    info!("Find My Mouse monitor stopped");
    Ok(())
}

/// Ease-out cubic interpolation progress for the spotlight animation.
fn spotlight_progress(elapsed_ms: u64, duration_ms: u32) -> f32 {
    if duration_ms == 0 {
        return 1.0;
    }
    let t = (elapsed_ms as f32 / duration_ms as f32).clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Spotlight radius at a given animation progress: starts at
/// `radius * initial_zoom` and shrinks to `radius` (upstream behaviour).
fn spotlight_radius(radius: u32, initial_zoom: u32, progress: f32) -> u32 {
    let start = radius as f32 * initial_zoom.max(1) as f32;
    (start + (radius as f32 - start) * progress).round() as u32
}

/// The spotlight overlay: a fullscreen dark window with a circular "hole" via SHAPE extension.
struct SpotlightOverlay {
    conn: RustConnection,
    window: u32,
    root: u32,
    screen_width: u16,
    screen_height: u16,
    radius: u32,
    initial_zoom: u32,
    animation_ms: u32,
    opacity: f32,
    opacity_atom: u32,
    started: Instant,
    last_drawn: Option<(i16, i16, u32)>,
    animating: bool,
}

impl SpotlightOverlay {
    fn create(config: &MouseUtilsConfig) -> Result<Self> {
        let (conn, screen_num) =
            RustConnection::connect(None).context("spotlight: connect to X11")?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let screen_width = screen.width_in_pixels;
        let screen_height = screen.height_in_pixels;
        let depth = screen.root_depth;

        let bg_color = parse_hex_color(&config.find_my_mouse_bg_color);
        let radius = config.find_my_mouse_spotlight_radius.max(1);
        let opacity = config.find_my_mouse_overlay_opacity.clamp(0.0, 1.0);
        let animation_ms = config.find_my_mouse_animation_ms;

        // Create fullscreen override-redirect window
        let window = conn.generate_id()?;
        let values = xproto::CreateWindowAux::new()
            .background_pixel(bg_color)
            .override_redirect(1)
            .event_mask(
                xproto::EventMask::EXPOSURE
                    | xproto::EventMask::KEY_PRESS
                    | xproto::EventMask::BUTTON_PRESS,
            );

        conn.create_window(
            depth,
            window,
            root,
            0,
            0,
            screen_width,
            screen_height,
            0,
            xproto::WindowClass::INPUT_OUTPUT,
            0,
            &values,
        )?;

        // Fade in from transparent when animating.
        let opacity_atom = x11util::intern_atom(&conn, b"_NET_WM_WINDOW_OPACITY")?;
        let initial_opacity = if animation_ms == 0 { opacity } else { 0.0 };
        x11util::set_opacity(&conn, opacity_atom, window, initial_opacity)?;

        // Raise above everything
        x11util::raise(&conn, window)?;

        let mut overlay = Self {
            conn,
            window,
            root,
            screen_width,
            screen_height,
            radius,
            initial_zoom: config.find_my_mouse_initial_zoom,
            animation_ms,
            opacity,
            opacity_atom,
            started: Instant::now(),
            last_drawn: None,
            animating: true,
        };
        overlay.update_cursor()?;
        overlay.conn.map_window(window)?;
        overlay.conn.flush()?;

        info!(
            "Find My Mouse: spotlight shown (zoom={}x, animation={}ms)",
            overlay.initial_zoom, animation_ms
        );
        Ok(overlay)
    }

    fn update_cursor(&mut self) -> Result<()> {
        let (cx, cy) = get_cursor_position(&self.conn, self.root)?;
        let progress =
            spotlight_progress(self.started.elapsed().as_millis() as u64, self.animation_ms);
        let r = spotlight_radius(self.radius, self.initial_zoom, progress);
        if self.animating {
            x11util::set_opacity(
                &self.conn,
                self.opacity_atom,
                self.window,
                self.opacity * progress,
            )?;
            self.animating = progress < 1.0;
        }
        if self.last_drawn != Some((cx, cy, r)) {
            apply_spotlight_shape(
                &self.conn,
                self.window,
                self.screen_width,
                self.screen_height,
                cx,
                cy,
                r,
            )?;
            self.last_drawn = Some((cx, cy, r));
        }
        self.conn.flush()?;
        Ok(())
    }

    fn destroy(self) -> Result<()> {
        self.conn.destroy_window(self.window)?;
        self.conn.flush()?;
        debug!("Find My Mouse: spotlight overlay destroyed");
        Ok(())
    }
}

/// Apply a SHAPE bounding region that is the entire window minus a circle.
/// This creates the "spotlight" effect: dark everywhere except around the cursor.
fn apply_spotlight_shape(
    conn: &RustConnection,
    window: u32,
    sw: u16,
    sh: u16,
    cx: i16,
    cy: i16,
    radius: u32,
) -> Result<()> {
    x11util::shape_circle(conn, window, sw, sh, cx, cy, radius, true)
}

fn get_cursor_position(conn: &RustConnection, root: u32) -> Result<(i16, i16)> {
    let reply = conn
        .query_pointer(root)?
        .reply()
        .context("query_pointer failed")?;
    Ok((reply.root_x, reply.root_y))
}

fn parse_hex_color(hex: &str) -> u32 {
    let hex = hex.trim_start_matches('#');
    u32::from_str_radix(hex, 16).unwrap_or(0x000000)
}

fn build_keymap(conn: &RustConnection) -> Result<std::collections::HashMap<u8, u32>> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max.saturating_sub(min).saturating_add(1);

    let reply = conn
        .get_keyboard_mapping(min, count)?
        .reply()
        .context("get_keyboard_mapping failed")?;

    if reply.keysyms_per_keycode == 0 {
        return Ok(std::collections::HashMap::new());
    }

    let per_kc = reply.keysyms_per_keycode as usize;
    let mut map = std::collections::HashMap::new();

    for (offset, keysyms) in reply.keysyms.chunks(per_kc).enumerate() {
        let keycode = min.saturating_add(offset as u8);
        if let Some(&ks) = keysyms.first()
            && ks != 0
        {
            map.insert(keycode, ks);
        }
    }

    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shake_back_and_forth_detected() {
        let mut d = ShakeDetector::new(1000, 1000, 400);
        let mut t = 0;
        let mut detected = false;
        for i in 0..12 {
            let dx = if i % 2 == 0 { 200 } else { -200 };
            // a few samples per stroke, same direction -> merged
            for _ in 0..4 {
                t += 10;
                detected |= d.push(dx / 4, 0, t);
            }
        }
        assert!(detected);
    }

    #[test]
    fn straight_line_is_not_a_shake() {
        let mut d = ShakeDetector::new(1000, 1000, 400);
        let mut detected = false;
        for i in 0..100 {
            detected |= d.push(30, if i % 2 == 0 { 1 } else { 0 }, i * 5);
        }
        assert!(!detected);
    }

    #[test]
    fn short_or_slow_shake_ignored() {
        // Too little distance.
        let mut d = ShakeDetector::new(1000, 1000, 400);
        let mut detected = false;
        for i in 0..6u64 {
            detected |= d.push(if i % 2 == 0 { 50 } else { -50 }, 0, i * 20);
        }
        assert!(!detected);

        // Enough distance, but spread over far more than the interval.
        let mut d = ShakeDetector::new(1000, 1000, 400);
        for i in 0..12u64 {
            detected |= d.push(if i % 2 == 0 { 200 } else { -200 }, 0, i * 600);
        }
        assert!(!detected);
    }

    #[test]
    fn spotlight_animation_math() {
        assert_eq!(spotlight_progress(0, 0), 1.0);
        assert_eq!(spotlight_progress(0, 500), 0.0);
        assert_eq!(spotlight_progress(500, 500), 1.0);
        assert_eq!(spotlight_progress(10_000, 500), 1.0);
        assert_eq!(spotlight_radius(100, 4, 0.0), 400);
        assert_eq!(spotlight_radius(100, 4, 1.0), 100);
        assert_eq!(spotlight_radius(100, 0, 0.0), 100);
    }
}
