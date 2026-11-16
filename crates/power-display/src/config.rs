//! Configuration (`~/.config/my-power-toys/power-display.toml`) and saved
//! monitor state (`power-display-state.toml`).
//!
//! Field names follow upstream `PowerDisplayProperties` / `PowerDisplayProfile`.

use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const MODULE_ID: &str = "power-display";
const STATE_ID: &str = "power-display-state";

fn default_true() -> bool {
    true
}
fn default_brightness_up() -> String {
    "Super+Alt+Up".into()
}
fn default_brightness_down() -> String {
    "Super+Alt+Down".into()
}
fn default_step() -> u8 {
    5
}
fn default_debounce_ms() -> u64 {
    300
}

/// User-defined name for a VCP value (upstream `CustomVcpValueMapping`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomVcpMapping {
    /// VCP code, e.g. 0x14 (color preset) or 0x60 (input source).
    pub vcp_code: u8,
    pub value: u16,
    pub custom_name: String,
    #[serde(default = "default_true")]
    pub apply_to_all: bool,
    #[serde(default)]
    pub target_monitor_id: String,
}

/// Per-monitor preferences (upstream `MonitorInfo` isHidden / enable* flags).
/// `None` means "use the global default".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorPreference {
    pub id: String,
    #[serde(default)]
    pub is_hidden: bool,
    #[serde(default)]
    pub enable_contrast: Option<bool>,
    #[serde(default)]
    pub enable_volume: Option<bool>,
    #[serde(default)]
    pub enable_input_source: Option<bool>,
    #[serde(default)]
    pub enable_color_temperature: Option<bool>,
    #[serde(default)]
    pub enable_power_state: Option<bool>,
    #[serde(default)]
    pub enable_rotation: Option<bool>,
}

/// Effective per-monitor UI flags after applying global defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonitorFlags {
    pub hidden: bool,
    pub contrast: bool,
    pub volume: bool,
    pub input_source: bool,
    pub color_temperature: bool,
    pub power_state: bool,
    pub rotation: bool,
}

/// Values for one monitor inside a profile (upstream `ProfileMonitorSetting`).
/// Continuous values are percentages (0..=100); `color_temperature_vcp` is the
/// raw VCP 0x14 preset value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileMonitorSetting {
    pub monitor_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contrast: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color_temperature_vcp: Option<u16>,
}

impl ProfileMonitorSetting {
    pub fn is_empty(&self) -> bool {
        self.brightness.is_none()
            && self.contrast.is_none()
            && self.volume.is_none()
            && self.color_temperature_vcp.is_none()
    }
}

/// Named set of per-monitor values (upstream `PowerDisplayProfile`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub monitor_settings: Vec<ProfileMonitorSetting>,
}

impl Profile {
    /// Upstream `IsValid`: non-blank name and at least one monitor.
    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty() && !self.monitor_settings.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PowerDisplayConfig {
    /// Hotkey that raises brightness on all monitors (no GUI).
    #[serde(default = "default_brightness_up")]
    pub brightness_up_hotkey: String,
    /// Hotkey that lowers brightness on all monitors (no GUI).
    #[serde(default = "default_brightness_down")]
    pub brightness_down_hotkey: String,
    /// Percentage step used by the brightness hotkeys.
    #[serde(default = "default_step")]
    pub hotkey_step: u8,
    /// Percentage step per mouse-wheel notch over a flyout slider.
    #[serde(default = "default_step")]
    pub mouse_wheel_increment: u8,
    /// Show a desktop notification with the new level after a hotkey.
    #[serde(default = "default_true")]
    pub show_osd_notification: bool,

    /// Re-apply the last values written by Power Display when the module starts.
    #[serde(default)]
    pub restore_settings_on_startup: bool,
    /// Profile to apply when the module starts (takes precedence over restore).
    #[serde(default)]
    pub startup_profile: Option<String>,

    #[serde(default = "default_true")]
    pub show_profile_switcher: bool,
    #[serde(default = "default_true")]
    pub show_identify_monitors_button: bool,

    /// Brightness slider changes are applied to all linked monitors.
    #[serde(default)]
    pub linked_levels_active: bool,
    #[serde(default)]
    pub excluded_from_sync_monitor_ids: Vec<String>,

    /// Global defaults for controls shown in the flyout (per-monitor overrides
    /// live in `monitors`).
    #[serde(default = "default_true")]
    pub show_contrast: bool,
    #[serde(default = "default_true")]
    pub show_volume: bool,
    #[serde(default = "default_true")]
    pub show_input_source: bool,
    #[serde(default = "default_true")]
    pub show_color_temperature: bool,
    #[serde(default)]
    pub show_power_state: bool,
    #[serde(default)]
    pub show_rotation: bool,

    /// Ignore DDC/CI capabilities strings and probe every feature directly.
    #[serde(default)]
    pub max_compatibility_mode: bool,

    /// Backends.
    #[serde(default = "default_true")]
    pub enable_ddc: bool,
    #[serde(default = "default_true")]
    pub enable_backlight: bool,
    /// Use `xrandr --brightness` for X11 outputs that have no DDC/backlight control.
    #[serde(default = "default_true")]
    pub enable_software_brightness: bool,
    /// Force a specific `/sys/class/backlight` device.
    #[serde(default)]
    pub backlight_device: Option<String>,
    /// Extra global args passed to every ddcutil call, e.g. `["--sleep-multiplier", "0.5"]`.
    #[serde(default)]
    pub ddcutil_extra_args: Vec<String>,

    /// Delay before slider changes are written to the monitor.
    #[serde(default = "default_debounce_ms")]
    pub slider_debounce_ms: u64,

    #[serde(default)]
    pub custom_vcp_mappings: Vec<CustomVcpMapping>,
    #[serde(default)]
    pub monitors: Vec<MonitorPreference>,
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

impl Default for PowerDisplayConfig {
    fn default() -> Self {
        Self {
            brightness_up_hotkey: default_brightness_up(),
            brightness_down_hotkey: default_brightness_down(),
            hotkey_step: default_step(),
            mouse_wheel_increment: default_step(),
            show_osd_notification: true,
            restore_settings_on_startup: false,
            startup_profile: None,
            show_profile_switcher: true,
            show_identify_monitors_button: true,
            linked_levels_active: false,
            excluded_from_sync_monitor_ids: vec![],
            show_contrast: true,
            show_volume: true,
            show_input_source: true,
            show_color_temperature: true,
            show_power_state: false,
            show_rotation: false,
            max_compatibility_mode: false,
            enable_ddc: true,
            enable_backlight: true,
            enable_software_brightness: true,
            backlight_device: None,
            ddcutil_extra_args: vec![],
            slider_debounce_ms: default_debounce_ms(),
            custom_vcp_mappings: vec![],
            monitors: vec![],
            profiles: vec![],
        }
    }
}

impl PowerDisplayConfig {
    pub fn load() -> Self {
        match mpt_common::config::load_module_config(MODULE_ID) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Power Display: failed to load config, using defaults: {e}");
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        mpt_common::config::save_module_config(MODULE_ID, self)
    }

    pub fn flags_for(&self, monitor_id: &str) -> MonitorFlags {
        let p = self.monitors.iter().find(|m| m.id == monitor_id);
        MonitorFlags {
            hidden: p.is_some_and(|p| p.is_hidden),
            contrast: p
                .and_then(|p| p.enable_contrast)
                .unwrap_or(self.show_contrast),
            volume: p.and_then(|p| p.enable_volume).unwrap_or(self.show_volume),
            input_source: p
                .and_then(|p| p.enable_input_source)
                .unwrap_or(self.show_input_source),
            color_temperature: p
                .and_then(|p| p.enable_color_temperature)
                .unwrap_or(self.show_color_temperature),
            power_state: p
                .and_then(|p| p.enable_power_state)
                .unwrap_or(self.show_power_state),
            rotation: p
                .and_then(|p| p.enable_rotation)
                .unwrap_or(self.show_rotation),
        }
    }

    /// Whether brightness changes on `monitor_id` are linked to other monitors.
    pub fn is_linked(&self, monitor_id: &str) -> bool {
        self.linked_levels_active
            && !self
                .excluded_from_sync_monitor_ids
                .iter()
                .any(|m| m == monitor_id)
    }

    /// Display name for a VCP value, using custom mappings first.
    pub fn value_name(&self, monitor_id: &str, code: u8, value: u16) -> String {
        let custom = self
            .custom_vcp_mappings
            .iter()
            .filter(|m| m.vcp_code == code && m.value == value)
            .find(|m| !m.apply_to_all && m.target_monitor_id == monitor_id)
            .or_else(|| {
                self.custom_vcp_mappings
                    .iter()
                    .find(|m| m.vcp_code == code && m.value == value && m.apply_to_all)
            });
        match custom {
            Some(m) => m.custom_name.clone(),
            None => crate::vcp::formatted_value_name(code, value),
        }
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }

    /// Insert or replace a profile by name.
    pub fn upsert_profile(&mut self, profile: Profile) {
        match self.profiles.iter_mut().find(|p| p.name == profile.name) {
            Some(existing) => *existing = profile,
            None => self.profiles.push(profile),
        }
    }

    pub fn remove_profile(&mut self, name: &str) -> bool {
        let before = self.profiles.len();
        self.profiles.retain(|p| p.name != name);
        self.profiles.len() != before
    }
}

/// Last values written per monitor, used by `restore_settings_on_startup`
/// (upstream `monitor_state.json`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorStateFile {
    #[serde(default)]
    pub monitors: Vec<ProfileMonitorSetting>,
}

impl MonitorStateFile {
    pub fn load() -> Self {
        mpt_common::config::load_module_config(STATE_ID).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        mpt_common::config::save_module_config(STATE_ID, self)
    }

    /// Merge `update` into the stored entry for its monitor.
    pub fn record(&mut self, update: &ProfileMonitorSetting) {
        let entry = match self
            .monitors
            .iter_mut()
            .position(|m| m.monitor_id == update.monitor_id)
        {
            Some(i) => &mut self.monitors[i],
            None => {
                self.monitors.push(ProfileMonitorSetting {
                    monitor_id: update.monitor_id.clone(),
                    ..Default::default()
                });
                self.monitors.last_mut().expect("just pushed")
            }
        };
        if update.brightness.is_some() {
            entry.brightness = update.brightness;
        }
        if update.contrast.is_some() {
            entry.contrast = update.contrast;
        }
        if update.volume.is_some() {
            entry.volume = update.volume;
        }
        if update.color_temperature_vcp.is_some() {
            entry.color_temperature_vcp = update.color_temperature_vcp;
        }
    }

    /// Load, merge and save in one step; errors are logged.
    pub fn record_and_save(update: &ProfileMonitorSetting) {
        let mut state = Self::load();
        state.record(update);
        if let Err(e) = state.save() {
            tracing::warn!("Power Display: failed to save monitor state: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_round_trip() {
        let c = PowerDisplayConfig::default();
        let s = toml::to_string_pretty(&c).unwrap();
        let back: PowerDisplayConfig = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn empty_file_gives_defaults() {
        let c: PowerDisplayConfig = toml::from_str("").unwrap();
        assert_eq!(c, PowerDisplayConfig::default());
        assert_eq!(c.brightness_up_hotkey, "Super+Alt+Up");
        assert_eq!(c.hotkey_step, 5);
    }

    #[test]
    fn profiles_round_trip() {
        let mut c = PowerDisplayConfig::default();
        c.upsert_profile(Profile {
            name: "Night".into(),
            monitor_settings: vec![ProfileMonitorSetting {
                monitor_id: "ddc:DEL:DELL U2715H:GH85".into(),
                brightness: Some(20),
                color_temperature_vcp: Some(0x04),
                ..Default::default()
            }],
        });
        c.upsert_profile(Profile {
            name: "Night".into(),
            monitor_settings: vec![ProfileMonitorSetting {
                monitor_id: "backlight:intel_backlight".into(),
                brightness: Some(10),
                ..Default::default()
            }],
        });
        c.startup_profile = Some("Night".into());
        assert_eq!(c.profiles.len(), 1);
        let s = toml::to_string_pretty(&c).unwrap();
        let back: PowerDisplayConfig = toml::from_str(&s).unwrap();
        assert_eq!(back, c);
        assert!(back.profile("Night").unwrap().is_valid());
        assert!(c.remove_profile("Night"));
        assert!(c.profiles.is_empty());
    }

    #[test]
    fn monitor_flags_and_linking() {
        let mut c = PowerDisplayConfig::default();
        c.monitors.push(MonitorPreference {
            id: "a".into(),
            is_hidden: true,
            enable_volume: Some(false),
            enable_power_state: Some(true),
            ..Default::default()
        });
        let f = c.flags_for("a");
        assert!(f.hidden && !f.volume && f.power_state && f.contrast);
        let g = c.flags_for("b");
        assert!(!g.hidden && g.volume && !g.power_state);

        assert!(!c.is_linked("a"));
        c.linked_levels_active = true;
        c.excluded_from_sync_monitor_ids.push("a".into());
        assert!(!c.is_linked("a"));
        assert!(c.is_linked("b"));
    }

    #[test]
    fn custom_value_names() {
        let mut c = PowerDisplayConfig::default();
        c.custom_vcp_mappings.push(CustomVcpMapping {
            vcp_code: 0x60,
            value: 0x11,
            custom_name: "Console".into(),
            apply_to_all: true,
            target_monitor_id: String::new(),
        });
        c.custom_vcp_mappings.push(CustomVcpMapping {
            vcp_code: 0x60,
            value: 0x11,
            custom_name: "Laptop dock".into(),
            apply_to_all: false,
            target_monitor_id: "m2".into(),
        });
        assert_eq!(c.value_name("m1", 0x60, 0x11), "Console");
        assert_eq!(c.value_name("m2", 0x60, 0x11), "Laptop dock");
        assert_eq!(c.value_name("m1", 0x60, 0x0F), "DisplayPort-1 (0x0F)");
    }

    #[test]
    fn state_record_merges() {
        let mut s = MonitorStateFile::default();
        s.record(&ProfileMonitorSetting {
            monitor_id: "m".into(),
            brightness: Some(40),
            ..Default::default()
        });
        s.record(&ProfileMonitorSetting {
            monitor_id: "m".into(),
            contrast: Some(70),
            ..Default::default()
        });
        assert_eq!(s.monitors.len(), 1);
        assert_eq!(s.monitors[0].brightness, Some(40));
        assert_eq!(s.monitors[0].contrast, Some(70));
    }
}
