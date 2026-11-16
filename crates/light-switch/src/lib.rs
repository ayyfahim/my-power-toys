pub mod config;
mod solar;
mod theme;

use anyhow::Result;
use chrono::{DateTime, Datelike, Local, NaiveTime, Offset, TimeZone, Timelike};
use config::{LightSwitchConfig, ScheduleMode};
use mpt_common::hotkey::Hotkey;
use mpt_common::module::PowerModule;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{info, warn};

pub use config::DesktopKind;
pub use theme::{Desktop, SystemTheme, detect_desktop};

/// How often the scheduler re-evaluates the schedule (and reloads config).
const SCHEDULE_TICK: Duration = Duration::from_secs(30);

pub struct LightSwitch {
    running: bool,
    config: LightSwitchConfig,
    stop_flag: Arc<AtomicBool>,
    scheduler: Option<thread::JoinHandle<()>>,
}

impl LightSwitch {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("light-switch").unwrap_or_default();
        Self {
            running: false,
            config,
            stop_flag: Arc::new(AtomicBool::new(false)),
            scheduler: None,
        }
    }

    fn reload_config(&mut self) {
        match mpt_common::config::load_module_config("light-switch") {
            Ok(c) => self.config = c,
            Err(e) => warn!("Light Switch: failed to load config, keeping previous: {e:#}"),
        }
    }

    fn apply_scheduled_theme(&self) {
        if let Some(dark) = scheduled_dark(&self.config, Local::now()) {
            theme::set_theme(dark, &self.config);
        }
    }

    fn spawn_scheduler(&mut self) {
        let stop = self.stop_flag.clone();
        let initial = self.config.clone();

        let handle = thread::spawn(move || {
            let mut config = initial;
            // Only act on transitions, so a manual toggle via hotkey sticks
            // until the next scheduled change (matches upstream).
            let mut last_dark = scheduled_dark(&config, Local::now());
            let mut last_mode = config.schedule_mode;

            while !stop.load(Ordering::Relaxed) {
                let tick_start = Instant::now();
                while tick_start.elapsed() < SCHEDULE_TICK {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    thread::sleep(Duration::from_millis(500));
                }

                if let Ok(c) =
                    mpt_common::config::load_module_config::<LightSwitchConfig>("light-switch")
                {
                    config = c;
                }
                if config.schedule_mode != last_mode {
                    // Schedule changed in settings: apply right away.
                    last_mode = config.schedule_mode;
                    last_dark = None;
                }

                let dark = scheduled_dark(&config, Local::now());
                if let Some(dark) = dark
                    && last_dark != Some(dark)
                {
                    theme::set_theme(dark, &config);
                }
                last_dark = dark;
            }
        });

        self.scheduler = Some(handle);
    }
}

impl Default for LightSwitch {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether the schedule says it should be dark at `now`. `None` when the
/// schedule is off or its state can't be determined (night light
/// unavailable).
pub fn scheduled_dark<Tz: TimeZone>(config: &LightSwitchConfig, now: DateTime<Tz>) -> Option<bool> {
    match config.schedule_mode {
        ScheduleMode::Off => None,
        ScheduleMode::SunsetSunrise => {
            let (sunrise, sunset) = solar::sunrise_sunset(
                now.year(),
                now.month(),
                now.day(),
                config.latitude,
                config.longitude,
            );

            // Solar times are in UTC; convert to local.
            let tz_offset_min = now.offset().fix().local_minus_utc() / 60;
            let sunrise = offset_time(sunrise, tz_offset_min + config.sunrise_offset_min);
            let sunset = offset_time(sunset, tz_offset_min + config.sunset_offset_min);
            Some(is_dark_between(now.time(), sunset, sunrise))
        }
        ScheduleMode::Fixed => {
            let dark_time = parse_time(&config.dark_mode_time)
                .unwrap_or(NaiveTime::from_hms_opt(20, 0, 0).unwrap());
            let light_time = parse_time(&config.light_mode_time)
                .unwrap_or(NaiveTime::from_hms_opt(6, 0, 0).unwrap());
            Some(is_dark_between(now.time(), dark_time, light_time))
        }
        ScheduleMode::FollowNightLight => {
            static WARNED: AtomicBool = AtomicBool::new(false);
            let active = theme::night_light_active();
            if active.is_none() && !WARNED.swap(true, Ordering::Relaxed) {
                warn!("Light Switch: night light state unavailable (GNOME/KDE only)");
            }
            active
        }
    }
}

/// Dark from `dark_start` until `light_start` (wrapping past midnight).
fn is_dark_between(now: NaiveTime, dark_start: NaiveTime, light_start: NaiveTime) -> bool {
    if light_start < dark_start {
        // Normal case: light during day, dark at night.
        now < light_start || now >= dark_start
    } else {
        // Inverted: dark_start < light_start (e.g. dark at 02:00, light at 10:00).
        now >= dark_start && now < light_start
    }
}

impl PowerModule for LightSwitch {
    fn id(&self) -> &'static str {
        "light-switch"
    }

    fn name(&self) -> &'static str {
        "Light Switch"
    }

    fn description(&self) -> &'static str {
        "Switch between light and dark mode on schedule or shortcut"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        let fallback = || Hotkey::parse(config::DEFAULT_HOTKEY).expect("default hotkey parses");
        let configured = self.config.hotkey.trim();
        if configured.is_empty() {
            return Some(fallback());
        }
        match Hotkey::parse(configured) {
            Ok(hk) => Some(hk),
            Err(e) => {
                warn!(
                    "Invalid light-switch hotkey '{configured}': {e}; using {}",
                    config::DEFAULT_HOTKEY
                );
                Some(fallback())
            }
        }
    }

    fn start(&mut self) -> Result<()> {
        info!("Light Switch: starting");
        self.reload_config();
        self.stop_flag.store(false, Ordering::Relaxed);

        // Apply theme immediately based on schedule
        self.apply_scheduled_theme();

        // The scheduler always runs and re-reads the config, so changing
        // the schedule in settings takes effect without a restart.
        self.spawn_scheduler();

        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        info!("Light Switch: stopping");
        self.stop_flag.store(true, Ordering::Relaxed);

        if let Some(handle) = self.scheduler.take() {
            let _ = handle.join();
        }

        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        info!("Light Switch: toggling theme via hotkey");
        self.reload_config();
        let now = theme::toggle_theme(&self.config);
        info!("Light Switch: now {now:?}");
        Ok(())
    }
}

fn parse_time(s: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(s.trim(), "%H:%M").ok()
}

fn offset_time(time: NaiveTime, offset_minutes: i32) -> NaiveTime {
    let total_min = time.hour() as i32 * 60 + time.minute() as i32 + offset_minutes;
    let total_min = total_min.rem_euclid(1440);
    NaiveTime::from_hms_opt(total_min as u32 / 60, total_min as u32 % 60, 0).unwrap_or(time)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{FixedOffset, Utc};

    fn t(h: u32, m: u32) -> NaiveTime {
        NaiveTime::from_hms_opt(h, m, 0).unwrap()
    }

    #[test]
    fn dark_window_normal_and_inverted() {
        assert!(is_dark_between(t(22, 0), t(20, 0), t(6, 0)));
        assert!(is_dark_between(t(3, 0), t(20, 0), t(6, 0)));
        assert!(!is_dark_between(t(12, 0), t(20, 0), t(6, 0)));
        assert!(is_dark_between(t(5, 0), t(2, 0), t(10, 0)));
        assert!(!is_dark_between(t(11, 0), t(2, 0), t(10, 0)));
    }

    #[test]
    fn fixed_schedule() {
        let cfg = LightSwitchConfig {
            schedule_mode: ScheduleMode::Fixed,
            ..Default::default()
        };
        let tz = FixedOffset::east_opt(0).unwrap();
        let night = tz.with_ymd_and_hms(2025, 1, 1, 23, 0, 0).unwrap();
        let noon = tz.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap();
        assert_eq!(scheduled_dark(&cfg, night), Some(true));
        assert_eq!(scheduled_dark(&cfg, noon), Some(false));
    }

    #[test]
    fn off_schedule_is_none() {
        let cfg = LightSwitchConfig::default();
        assert_eq!(scheduled_dark(&cfg, Utc::now()), None);
    }

    #[test]
    fn sunset_schedule_equator() {
        let cfg = LightSwitchConfig {
            schedule_mode: ScheduleMode::SunsetSunrise,
            ..Default::default()
        };
        // Lat/lon 0,0 in UTC: sunrise ~06:00, sunset ~18:00.
        let midnight = Utc.with_ymd_and_hms(2025, 3, 20, 0, 30, 0).unwrap();
        let noon = Utc.with_ymd_and_hms(2025, 3, 20, 12, 0, 0).unwrap();
        assert_eq!(scheduled_dark(&cfg, midnight), Some(true));
        assert_eq!(scheduled_dark(&cfg, noon), Some(false));
    }

    #[test]
    fn hotkey_from_config() {
        let mut ls = LightSwitch {
            running: false,
            config: LightSwitchConfig::default(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            scheduler: None,
        };
        assert_eq!(ls.default_hotkey().unwrap().to_string(), "Super+Shift+D");
        ls.config.hotkey = "Ctrl+Super+Shift+D".into();
        assert_eq!(
            ls.default_hotkey().unwrap().to_string(),
            "Ctrl+Super+Shift+D"
        );
    }
}

#[cfg(test)]
mod parity_tests;
