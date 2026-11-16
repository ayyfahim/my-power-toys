pub mod config;
mod inhibitor;

use anyhow::Result;
use chrono::{Local, NaiveDateTime};
use config::{AwakeConfig, AwakeMode};
use inhibitor::{LockWatcher, ScreenSaverInhibitor};
use mpt_common::module::PowerModule;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Worker loop granularity (stop/expiry latency).
const TICK: Duration = Duration::from_secs(1);
/// How often the session lock state is polled.
const LOCK_POLL: Duration = Duration::from_secs(2);

/// When the keep-awake session ends on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Deadline {
    Never,
    /// Monotonic deadline (Timed mode).
    After(Instant),
    /// Wall-clock deadline in local time (Expirable mode).
    At(NaiveDateTime),
}

impl Deadline {
    fn reached(self, now: Instant, now_local: NaiveDateTime) -> bool {
        match self {
            Deadline::Never => false,
            Deadline::After(t) => now >= t,
            Deadline::At(t) => now_local >= t,
        }
    }
}

pub struct Awake {
    running: bool,
    config: AwakeConfig,
    stop_flag: Arc<AtomicBool>,
    /// Set by the worker when the timer/expiration elapsed and it released
    /// the inhibitor by itself.
    expired: Arc<AtomicBool>,
    timer_thread: Option<thread::JoinHandle<()>>,
}

impl Awake {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("awake").unwrap_or_default();
        Self {
            running: false,
            config,
            stop_flag: Arc::new(AtomicBool::new(false)),
            expired: Arc::new(AtomicBool::new(false)),
            timer_thread: None,
        }
    }

    pub fn config(&self) -> &AwakeConfig {
        &self.config
    }

    fn deadline(&self) -> Option<Deadline> {
        match self.config.mode {
            AwakeMode::Off => None,
            AwakeMode::Indefinite => Some(Deadline::Never),
            AwakeMode::Timed => {
                let secs = self.config.timed_secs();
                if secs == 0 {
                    info!("Awake: timed mode with 0 duration, keeping awake indefinitely");
                    Some(Deadline::Never)
                } else {
                    Some(Deadline::After(Instant::now() + Duration::from_secs(secs)))
                }
            }
            AwakeMode::Expirable => match self.config.expire_at_local() {
                Some(dt) => Some(Deadline::At(dt)),
                None => {
                    warn!(
                        "Awake: invalid expire_at format '{}', keeping awake indefinitely",
                        self.config.expire_at
                    );
                    Some(Deadline::Never)
                }
            },
        }
    }

    /// Spawn the worker thread that owns the inhibitor, releases it when the
    /// deadline passes and follows the session lock state.
    fn spawn_worker(&mut self, inhibitor: ScreenSaverInhibitor, deadline: Deadline) {
        let stop = self.stop_flag.clone();
        let expired = self.expired.clone();
        let watch_lock = self.config.keep_screen_on && self.config.allow_display_off_while_locked;
        let keep_screen_on = self.config.keep_screen_on;

        let handle = thread::spawn(move || {
            let mut inhibitor = inhibitor;
            let watcher = watch_lock.then(LockWatcher::new);
            let mut last_lock_check: Option<Instant> = None;

            while !stop.load(Ordering::Relaxed) {
                if deadline.reached(Instant::now(), Local::now().naive_local()) {
                    info!("Awake: timer/expiration elapsed, releasing inhibitor");
                    expired.store(true, Ordering::Relaxed);
                    break;
                }

                if let Some(w) = &watcher
                    && last_lock_check.is_none_or(|t| t.elapsed() >= LOCK_POLL)
                {
                    last_lock_check = Some(Instant::now());
                    if let Some(locked) = w.is_locked() {
                        let want_display = keep_screen_on && !locked;
                        if inhibitor.display_inhibited() != want_display {
                            info!(
                                "Awake: session {}, display may {}turn off",
                                if locked { "locked" } else { "unlocked" },
                                if want_display { "not " } else { "" }
                            );
                            inhibitor.set_display_inhibited(want_display);
                        }
                    }
                }

                thread::sleep(TICK);
            }

            if let Err(e) = inhibitor.release() {
                warn!("Failed to release inhibitor: {e}");
            } else {
                info!("Awake: inhibitor released");
            }
        });

        self.timer_thread = Some(handle);
    }

    /// Release internal resources (worker thread, which releases the inhibitor).
    fn release_resources(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.timer_thread.take() {
            let _ = handle.join();
        }
    }

    /// Whether the timer/expiration elapsed. The worker already released the
    /// inhibitor by itself; this only joins the thread and marks the module
    /// stopped. Returns true if the module auto-stopped.
    pub fn check_expired(&mut self) -> bool {
        if !self.running {
            return false;
        }
        if self.expired.load(Ordering::Relaxed) {
            info!("Awake: auto-stopped (timer/expiration elapsed)");
            self.release_resources();
            self.running = false;
            return true;
        }
        false
    }
}

impl Default for Awake {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for Awake {
    fn id(&self) -> &'static str {
        "awake"
    }

    fn name(&self) -> &'static str {
        "Awake"
    }

    fn description(&self) -> &'static str {
        "Prevent the screen from going to sleep and the system from suspending"
    }

    fn start(&mut self) -> Result<()> {
        // Clean up a session that expired on its own.
        self.release_resources();

        // Reload config from disk (UI may have changed settings)
        self.config = mpt_common::config::load_module_config("awake").unwrap_or_default();
        self.stop_flag = Arc::new(AtomicBool::new(false));
        self.expired = Arc::new(AtomicBool::new(false));

        let Some(deadline) = self.deadline() else {
            info!("Awake: starting in passive (off) mode — no inhibition");
            self.running = true;
            return Ok(());
        };

        if deadline.reached(Instant::now(), Local::now().naive_local()) {
            info!(
                "Awake: expiration {} already passed, not inhibiting",
                self.config.expire_at
            );
            self.expired.store(true, Ordering::Relaxed);
            self.running = true;
            return Ok(());
        }

        info!(
            "Awake: starting in {:?} mode (keep_screen_on={}, allow_display_off_while_locked={}, deadline={deadline:?})",
            self.config.mode,
            self.config.keep_screen_on,
            self.config.allow_display_off_while_locked
        );
        let inhibitor = ScreenSaverInhibitor::inhibit(self.config.keep_screen_on)?;
        self.spawn_worker(inhibitor, deadline);

        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        info!("Awake: stopping");
        self.release_resources();
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running && !self.expired.load(Ordering::Relaxed)
    }

    fn on_hotkey(&mut self) -> Result<()> {
        if self.is_running() {
            self.stop()
        } else {
            self.start()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadline_reached() {
        let now = Instant::now();
        let local = Local::now().naive_local();
        assert!(!Deadline::Never.reached(now, local));
        assert!(!Deadline::After(now + Duration::from_secs(5)).reached(now, local));
        assert!(Deadline::After(now).reached(now, local));
        assert!(Deadline::At(local - chrono::Duration::minutes(1)).reached(now, local));
        assert!(!Deadline::At(local + chrono::Duration::minutes(1)).reached(now, local));
    }

    #[test]
    fn expired_flag_hides_running() {
        let mut a = Awake::new();
        a.running = true;
        assert!(a.is_running());
        a.expired.store(true, Ordering::Relaxed);
        assert!(!a.is_running());
        assert!(a.check_expired());
        assert!(!a.running);
    }
}

#[cfg(test)]
mod parity_tests;
