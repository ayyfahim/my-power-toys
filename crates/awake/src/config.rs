use serde::{Deserialize, Serialize};

/// Awake operating mode, mirroring Microsoft PowerToys Awake modes.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AwakeMode {
    /// Module enabled but not keeping awake (passive).
    Off,
    /// Keep awake forever until manually stopped.
    #[default]
    Indefinite,
    /// Keep awake for a fixed duration (hours + minutes).
    Timed,
    /// Keep awake until a specific date/time.
    Expirable,
}

/// Persisted configuration for the Awake module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwakeConfig {
    #[serde(default)]
    pub mode: AwakeMode,

    /// When true, also prevent the display from turning off.
    /// When false, only prevent system suspend.
    /// (Upstream `KeepDisplayOn`; `keep_display_on` is accepted as an alias.)
    #[serde(default = "default_true", alias = "keep_display_on")]
    pub keep_screen_on: bool,

    /// When the session is locked, stop keeping the display on (system
    /// sleep stays blocked). Upstream `AllowDisplayOffWhileLocked`.
    #[serde(default = "default_true")]
    pub allow_display_off_while_locked: bool,

    /// Hours component for `Timed` mode.
    #[serde(default)]
    pub timed_hours: u32,

    /// Minutes component for `Timed` mode.
    #[serde(default = "default_30")]
    pub timed_minutes: u32,

    /// Expiration date-time string for `Expirable` mode.
    /// Format: `%Y-%m-%dT%H:%M` (e.g. `2026-03-01T18:00`).
    #[serde(default)]
    pub expire_at: String,
}

fn default_true() -> bool {
    true
}

fn default_30() -> u32 {
    30
}

impl Default for AwakeConfig {
    fn default() -> Self {
        Self {
            mode: AwakeMode::default(),
            keep_screen_on: true,
            allow_display_off_while_locked: true,
            timed_hours: 0,
            timed_minutes: default_30(),
            expire_at: String::new(),
        }
    }
}

impl AwakeConfig {
    /// Total duration of `Timed` mode in seconds.
    pub fn timed_secs(&self) -> u64 {
        u64::from(self.timed_hours) * 3600 + u64::from(self.timed_minutes) * 60
    }

    /// Parsed `expire_at` (local time), if valid.
    pub fn expire_at_local(&self) -> Option<chrono::NaiveDateTime> {
        let raw = self.expire_at.trim();
        ["%Y-%m-%dT%H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%d %H:%M"]
            .iter()
            .find_map(|fmt| chrono::NaiveDateTime::parse_from_str(raw, fmt).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_parses() {
        let cfg: AwakeConfig =
            toml::from_str("mode = \"timed\"\nkeep_screen_on = false\n").unwrap();
        assert_eq!(cfg.mode, AwakeMode::Timed);
        assert!(!cfg.keep_screen_on);
        assert!(cfg.allow_display_off_while_locked);
        assert_eq!(cfg.timed_secs(), 30 * 60);
    }

    #[test]
    fn keep_display_on_alias() {
        let cfg: AwakeConfig = toml::from_str("keep_display_on = false\n").unwrap();
        assert!(!cfg.keep_screen_on);
    }

    #[test]
    fn expire_at_formats() {
        let mut cfg = AwakeConfig {
            expire_at: "2026-03-01T18:00".into(),
            ..Default::default()
        };
        assert!(cfg.expire_at_local().is_some());
        cfg.expire_at = "2026-03-01 18:00".into();
        assert!(cfg.expire_at_local().is_some());
        cfg.expire_at = "tomorrow".into();
        assert!(cfg.expire_at_local().is_none());
    }
}
