//! Power Display: control monitor brightness, contrast, volume, color
//! temperature, input source and power state (Linux port of PowerToys
//! Power Display).
//!
//! Backends: DDC/CI via `ddcutil`, laptop panels via `/sys/class/backlight`
//! (written through `brightnessctl` / logind when possible) and software
//! brightness via `xrandr --brightness` on X11.

pub mod backlight;
pub mod cmd;
pub mod config;
pub mod ddc;
pub mod debounce;
pub mod display;
#[cfg(feature = "gui")]
pub mod gui;
pub mod vcp;
pub mod xrandr;

pub use config::{
    CustomVcpMapping, MonitorPreference, MonitorStateFile, PowerDisplayConfig, Profile,
    ProfileMonitorSetting,
};
pub use display::{Backend, Controller, Discovery, Monitor};
pub use vcp::Feature;

use anyhow::Result;
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub const ACTION_BRIGHTNESS_UP: &str = "brightness-up";
pub const ACTION_BRIGHTNESS_DOWN: &str = "brightness-down";
/// Prefix for `on_named_action("profile:<name>")`.
pub const ACTION_PROFILE_PREFIX: &str = "profile:";

/// Re-read hardware values if the cached list is older than this.
const VALUE_REFRESH_AFTER: Duration = Duration::from_secs(3);
/// Re-run discovery if the cached list is older than this.
const REDISCOVER_AFTER: Duration = Duration::from_secs(300);

#[derive(Default)]
struct Cache {
    monitors: Vec<Monitor>,
    discovered_at: Option<Instant>,
    touched_at: Option<Instant>,
}

/// Background state for the brightness hotkeys. Presses are accumulated in
/// `pending_delta` and applied by one worker thread, so a held key does not
/// queue up dozens of slow ddcutil calls.
#[derive(Default)]
struct HotkeyWorker {
    pending_delta: Mutex<i32>,
    busy: AtomicBool,
    cache: Mutex<Cache>,
}

impl HotkeyWorker {
    fn submit(self: &Arc<Self>, delta: i32) {
        *self.pending_delta.lock().unwrap_or_else(|e| e.into_inner()) += delta;
        if self.busy.swap(true, Ordering::AcqRel) {
            return; // running worker will pick it up
        }
        let me = Arc::clone(self);
        std::thread::spawn(move || me.run());
    }

    fn take_delta(&self) -> i32 {
        std::mem::take(&mut *self.pending_delta.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn run(&self) {
        loop {
            let delta = self.take_delta();
            if delta == 0 {
                self.busy.store(false, Ordering::Release);
                // A press may have landed between take and store.
                let more = *self.pending_delta.lock().unwrap_or_else(|e| e.into_inner()) != 0;
                if more && !self.busy.swap(true, Ordering::AcqRel) {
                    continue;
                }
                return;
            }
            self.apply(delta);
        }
    }

    fn apply(&self, delta: i32) {
        let config = PowerDisplayConfig::load();
        let controller = Controller::new(config.clone());
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());

        let stale = cache
            .discovered_at
            .is_none_or(|t| t.elapsed() > REDISCOVER_AFTER)
            || cache.monitors.is_empty();
        if stale {
            let d = controller.discover();
            for w in &d.warnings {
                warn!("Power Display: {w}");
            }
            cache.monitors = d.monitors;
            cache.discovered_at = Some(Instant::now());
        } else if cache
            .touched_at
            .is_none_or(|t| t.elapsed() > VALUE_REFRESH_AFTER)
        {
            for m in cache.monitors.iter_mut() {
                if let Err(e) = controller.refresh_values(m) {
                    warn!("Power Display: refresh {}: {e:#}", m.name);
                }
            }
        }

        let delta = delta.clamp(-100, 100) as i16;
        let (levels, errors) = controller.adjust_brightness_all(&mut cache.monitors, delta);
        cache.touched_at = Some(Instant::now());
        for e in &errors {
            warn!("Power Display: {e}");
        }
        if !errors.is_empty() {
            // Monitor may have been unplugged; rediscover next time.
            cache.discovered_at = None;
        }
        info!("Power Display: brightness {delta:+} -> {levels:?}");
        if config.show_osd_notification {
            notify_levels(&levels);
        }
    }

    fn invalidate(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        *cache = Cache::default();
    }
}

/// Show the new levels with `notify-send` (replacing the previous bubble).
fn notify_levels(levels: &[(String, u16)]) {
    if levels.is_empty() || !cmd::in_path("notify-send") {
        return;
    }
    let body = levels
        .iter()
        .map(|(n, v)| format!("{n}: {v}%"))
        .collect::<Vec<_>>()
        .join("\n");
    let first = levels[0].1;
    let args: Vec<String> = vec![
        "--app-name=My Power Toys".into(),
        "--expire-time=1500".into(),
        "--icon=display-brightness-symbolic".into(),
        format!("--hint=int:value:{first}"),
        "--hint=string:x-canonical-private-synchronous:mpt-power-display".into(),
        "Brightness".into(),
        body,
    ];
    if let Err(e) = cmd::run("notify-send", &args, Duration::from_secs(3)) {
        warn!("Power Display: notify-send: {e:#}");
    }
}

/// Apply the startup profile, or restore the last saved values.
fn apply_startup(config: &PowerDisplayConfig) {
    let profile = config
        .startup_profile
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|name| {
            let p = config.profile(name).cloned();
            if p.is_none() {
                warn!("Power Display: startup profile {name:?} not found");
            }
            p
        });
    let profile = match profile {
        Some(p) => p,
        None if config.restore_settings_on_startup => Profile {
            name: "(restore)".into(),
            monitor_settings: MonitorStateFile::load().monitors,
        },
        None => return,
    };
    if profile.monitor_settings.is_empty() {
        return;
    }
    let controller = Controller::new(config.clone());
    let mut monitors = controller.discover().monitors;
    for e in controller.apply_profile(&mut monitors, &profile) {
        warn!("Power Display: {e}");
    }
}

pub struct PowerDisplay {
    running: bool,
    config: PowerDisplayConfig,
    worker: Arc<HotkeyWorker>,
}

impl PowerDisplay {
    pub fn new() -> Self {
        Self {
            running: false,
            config: PowerDisplayConfig::load(),
            worker: Arc::new(HotkeyWorker::default()),
        }
    }

    fn parse_hotkey(s: &str, fallback: &str) -> Hotkey {
        Hotkey::parse(s).unwrap_or_else(|e| {
            warn!("Power Display: invalid hotkey {s:?} ({e}), using {fallback}");
            Hotkey::parse(fallback).expect("valid fallback hotkey")
        })
    }
}

impl Default for PowerDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for PowerDisplay {
    fn id(&self) -> &'static str {
        "power-display"
    }

    fn name(&self) -> &'static str {
        "Power Display"
    }

    fn description(&self) -> &'static str {
        "Control monitor brightness, contrast and volume"
    }

    /// Upstream default is Win+Ctrl+Shift+P.
    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(Hotkey::new(
            vec![Modifier::Super, Modifier::Ctrl, Modifier::Shift],
            "P",
        ))
    }

    fn start(&mut self) -> Result<()> {
        self.config = PowerDisplayConfig::load();
        self.worker.invalidate();
        self.running = true;
        if self.config.startup_profile.is_some() || self.config.restore_settings_on_startup {
            let config = self.config.clone();
            std::thread::spawn(move || apply_startup(&config));
        }
        info!("Power Display module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Power Display module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        let bin = find_gui_binary();
        info!("Power Display: launching flyout ({bin:?})");
        match std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => info!("Power Display flyout spawned (pid={})", child.id()),
            Err(e) => warn!("Failed to spawn Power Display flyout ({bin:?}): {e}"),
        }
        // The flyout may change values behind our back.
        self.worker.invalidate();
        Ok(())
    }

    fn additional_hotkeys(&self) -> Vec<(Hotkey, &'static str)> {
        vec![
            (
                Self::parse_hotkey(&self.config.brightness_up_hotkey, "Super+Alt+Up"),
                ACTION_BRIGHTNESS_UP,
            ),
            (
                Self::parse_hotkey(&self.config.brightness_down_hotkey, "Super+Alt+Down"),
                ACTION_BRIGHTNESS_DOWN,
            ),
        ]
    }

    fn on_named_action(&mut self, action: &str) -> Result<()> {
        let step = PowerDisplayConfig::load().hotkey_step.clamp(1, 100) as i32;
        match action {
            ACTION_BRIGHTNESS_UP => self.worker.submit(step),
            ACTION_BRIGHTNESS_DOWN => self.worker.submit(-step),
            other => {
                if let Some(name) = other.strip_prefix(ACTION_PROFILE_PREFIX) {
                    let config = PowerDisplayConfig::load();
                    let Some(profile) = config.profile(name).cloned() else {
                        anyhow::bail!("profile {name:?} not found");
                    };
                    self.worker.invalidate();
                    std::thread::spawn(move || {
                        let c = Controller::new(config);
                        let mut monitors = c.discover().monitors;
                        for e in c.apply_profile(&mut monitors, &profile) {
                            warn!("Power Display: {e}");
                        }
                    });
                } else {
                    warn!("Power Display: unknown action {other:?}");
                }
            }
        }
        Ok(())
    }
}

/// Find the mpt-power-display binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-power-display");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-power-display".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys() {
        let m = PowerDisplay {
            running: false,
            config: PowerDisplayConfig::default(),
            worker: Arc::new(HotkeyWorker::default()),
        };
        assert_eq!(
            m.default_hotkey().unwrap().to_string(),
            "Super+Ctrl+Shift+P"
        );
        let extra = m.additional_hotkeys();
        assert_eq!(extra[0].0.to_string(), "Super+Alt+Up");
        assert_eq!(extra[0].1, ACTION_BRIGHTNESS_UP);
        assert_eq!(extra[1].0.to_string(), "Super+Alt+Down");
    }

    #[test]
    fn invalid_hotkey_falls_back() {
        let h = PowerDisplay::parse_hotkey("Hyper+X", "Super+Alt+Up");
        assert_eq!(h.to_string(), "Super+Alt+Up");
    }
}
