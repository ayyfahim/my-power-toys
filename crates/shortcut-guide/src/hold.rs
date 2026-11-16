//! Press-and-hold Super activation (upstream `WindowsKeyAction` /
//! `PressTime`). A background thread polls the X11 keymap every
//! [`POLL_INTERVAL`] and opens the overlay once Super has been held on its
//! own for `press_time` ms.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use crate::config::ShortcutGuideConfig;
use crate::x11::KeyState;

pub const POLL_INTERVAL: Duration = Duration::from_millis(30);
const CONFIG_RELOAD_INTERVAL: Duration = Duration::from_secs(2);
const IDLE_INTERVAL: Duration = Duration::from_millis(500);

/// Pure state machine deciding when a Super hold should fire.
#[derive(Debug, Default)]
pub struct HoldDetector {
    pressed_at: Option<Instant>,
    cancelled: bool,
    fired: bool,
}

impl HoldDetector {
    /// Feed one sample. Returns `true` exactly once per qualifying hold.
    pub fn update(
        &mut self,
        super_down: bool,
        other_down: bool,
        now: Instant,
        press_time: Duration,
    ) -> bool {
        if !super_down {
            *self = Self::default();
            return false;
        }
        let pressed_at = *self.pressed_at.get_or_insert(now);
        if other_down {
            // Super used as a modifier (Super+L, Super+drag, ...): not a hold.
            self.cancelled = true;
        }
        if self.cancelled || self.fired {
            return false;
        }
        if now.duration_since(pressed_at) >= press_time {
            self.fired = true;
            return true;
        }
        false
    }
}

/// Spawn the polling thread. It exits when `running` becomes false or when
/// X11 is unavailable.
pub fn spawn(running: Arc<AtomicBool>) -> Option<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("shortcut-guide-hold".into())
        .spawn(move || run(running))
        .map_err(|e| warn!("Shortcut Guide: failed to spawn hold thread: {e}"))
        .ok()
}

fn run(running: Arc<AtomicBool>) {
    let state = match KeyState::connect() {
        Ok(s) => s,
        Err(e) => {
            warn!("Shortcut Guide: press-and-hold Super disabled (X11 unavailable): {e:#}");
            return;
        }
    };
    info!("Shortcut Guide: Super hold watcher running");

    let mut config: ShortcutGuideConfig =
        mpt_common::config::load_module_config("shortcut-guide").unwrap_or_default();
    let mut last_reload = Instant::now();
    let mut detector = HoldDetector::default();

    while running.load(Ordering::Relaxed) {
        if last_reload.elapsed() >= CONFIG_RELOAD_INTERVAL {
            config = mpt_common::config::load_module_config("shortcut-guide").unwrap_or_default();
            last_reload = Instant::now();
        }
        if !config.hold_to_open() {
            detector = HoldDetector::default();
            std::thread::sleep(IDLE_INTERVAL);
            continue;
        }

        match state.snapshot() {
            Ok(snap) => {
                let press_time = Duration::from_millis(u64::from(config.press_time_ms()));
                if detector.update(snap.super_down, snap.other_down, Instant::now(), press_time) {
                    info!("Shortcut Guide: Super held for {press_time:?}, opening overlay");
                    crate::launch_overlay(&config, true);
                }
            }
            Err(e) => {
                warn!("Shortcut Guide: keymap query failed, stopping hold watcher: {e:#}");
                return;
            }
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLD: Duration = Duration::from_millis(900);

    #[test]
    fn fires_after_press_time() {
        let t0 = Instant::now();
        let mut d = HoldDetector::default();
        assert!(!d.update(true, false, t0, HOLD));
        assert!(!d.update(true, false, t0 + Duration::from_millis(500), HOLD));
        assert!(d.update(true, false, t0 + Duration::from_millis(900), HOLD));
        // Only once per hold.
        assert!(!d.update(true, false, t0 + Duration::from_millis(1500), HOLD));
    }

    #[test]
    fn other_key_cancels_hold() {
        let t0 = Instant::now();
        let mut d = HoldDetector::default();
        d.update(true, false, t0, HOLD);
        d.update(true, true, t0 + Duration::from_millis(100), HOLD);
        // Releasing the other key while Super is still held does not re-arm.
        assert!(!d.update(true, false, t0 + Duration::from_millis(2000), HOLD));
    }

    #[test]
    fn release_rearms() {
        let t0 = Instant::now();
        let mut d = HoldDetector::default();
        d.update(true, true, t0, HOLD);
        d.update(false, false, t0 + Duration::from_millis(100), HOLD);
        let t1 = t0 + Duration::from_millis(200);
        assert!(!d.update(true, false, t1, HOLD));
        assert!(d.update(true, false, t1 + HOLD, HOLD));
    }

    #[test]
    fn short_tap_does_not_fire() {
        let t0 = Instant::now();
        let mut d = HoldDetector::default();
        d.update(true, false, t0, HOLD);
        assert!(!d.update(false, false, t0 + Duration::from_millis(200), HOLD));
    }
}
