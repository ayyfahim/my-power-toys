use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

pub const DEFAULT_HOTKEY: &str = "Super+Shift+D";

/// When the theme switches automatically (upstream `ScheduleMode`).
///
/// Serialized as the strings this port has always written (`"off"`,
/// `"sunset-sunrise"`, `"fixed"`) so existing config files and the settings
/// UI keep working. Parsing is lenient and also accepts the upstream names
/// (`Off`, `SunsetToSunrise`, `FixedHours`, `FollowNightLight`) in any case
/// and with `-`/`_`/space separators. Unknown values fall back to `Off`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScheduleMode {
    #[default]
    Off,
    /// Dark between sunset and sunrise at `latitude`/`longitude`.
    SunsetSunrise,
    /// Dark between `dark_mode_time` and `light_mode_time`.
    Fixed,
    /// Dark while the desktop's night light is active (GNOME / KDE).
    FollowNightLight,
}

impl ScheduleMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::SunsetSunrise => "sunset-sunrise",
            Self::Fixed => "fixed",
            Self::FollowNightLight => "follow-night-light",
        }
    }

    /// Lenient parse; `None` for unrecognised input.
    pub fn parse(s: &str) -> Option<Self> {
        let norm: String = s
            .trim()
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' '))
            .flat_map(char::to_lowercase)
            .collect();
        match norm.as_str() {
            "" | "off" | "none" | "manual" | "disabled" => Some(Self::Off),
            "sunsetsunrise" | "sunsettosunrise" | "sun" | "solar" => Some(Self::SunsetSunrise),
            "fixed" | "fixedhours" | "custom" | "manualtimes" => Some(Self::Fixed),
            "follownightlight" | "nightlight" => Some(Self::FollowNightLight),
            _ => None,
        }
    }
}

impl fmt::Display for ScheduleMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ScheduleMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or_else(|| format!("unknown schedule mode: {s}"))
    }
}

impl Serialize for ScheduleMode {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for ScheduleMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        Ok(Self::parse(&raw).unwrap_or_else(|| {
            tracing::warn!("Light Switch: unknown schedule_mode {raw:?}, treating as off");
            Self::Off
        }))
    }
}

impl PartialEq<&str> for ScheduleMode {
    fn eq(&self, other: &&str) -> bool {
        Self::parse(other) == Some(*self)
    }
}

/// Which desktop's settings to change. `Auto` uses `XDG_CURRENT_DESKTOP`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DesktopKind {
    #[default]
    Auto,
    Gnome,
    Kde,
    Xfce,
    Cinnamon,
    Mate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightSwitchConfig {
    #[serde(default)]
    pub schedule_mode: ScheduleMode,
    #[serde(default)]
    pub latitude: f64,
    #[serde(default)]
    pub longitude: f64,
    #[serde(default)]
    pub sunrise_offset_min: i32,
    #[serde(default)]
    pub sunset_offset_min: i32,
    #[serde(default = "default_dark_time")]
    pub dark_mode_time: String,
    #[serde(default = "default_light_time")]
    pub light_mode_time: String,
    /// Upstream `ChangeSystem`: desktop/shell theme (GTK theme, KDE colour
    /// scheme, XFCE theme).
    #[serde(default = "default_true")]
    pub apply_system: bool,
    /// Upstream `ChangeApps`: the app colour-scheme preference read by
    /// GTK4/libadwaita and portal-aware apps.
    #[serde(default = "default_true")]
    pub apply_apps: bool,

    /// Toggle hotkey (upstream `ToggleThemeHotkey`).
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Desktop backend to drive.
    #[serde(default)]
    pub desktop: DesktopKind,

    /// Also switch the GTK theme name (GNOME, Cinnamon, MATE). XFCE always
    /// switches its theme name because that is its only mechanism.
    #[serde(default = "default_true")]
    pub switch_gtk_theme: bool,
    #[serde(default = "default_light_gtk")]
    pub light_gtk_theme: String,
    #[serde(default = "default_dark_gtk")]
    pub dark_gtk_theme: String,

    /// KDE colour schemes passed to `plasma-apply-colorscheme`.
    #[serde(default = "default_kde_light")]
    pub kde_light_color_scheme: String,
    #[serde(default = "default_kde_dark")]
    pub kde_dark_color_scheme: String,
    /// Optional KDE global themes for `lookandfeeltool -a`
    /// (e.g. `org.kde.breeze.desktop`). Used instead of the colour scheme
    /// when set.
    #[serde(default)]
    pub kde_light_look_and_feel: Option<String>,
    #[serde(default)]
    pub kde_dark_look_and_feel: Option<String>,

    /// Wallpaper image path to set when switching to each mode.
    #[serde(default)]
    pub light_wallpaper: Option<String>,
    #[serde(default)]
    pub dark_wallpaper: Option<String>,

    /// Power Display profile applied when switching to dark / light mode
    /// (upstream `EnableDarkModeProfile`/`DarkModeProfile` and the light
    /// equivalents). Runs `mpt-power-display --apply-profile <name>`.
    #[serde(default)]
    pub dark_mode_power_display_profile: Option<String>,
    #[serde(default)]
    pub light_mode_power_display_profile: Option<String>,
}

fn default_dark_time() -> String {
    "20:00".into()
}

fn default_light_time() -> String {
    "06:00".into()
}

fn default_true() -> bool {
    true
}

fn default_hotkey() -> String {
    DEFAULT_HOTKEY.into()
}

fn default_light_gtk() -> String {
    "Adwaita".into()
}

fn default_dark_gtk() -> String {
    "Adwaita-dark".into()
}

fn default_kde_light() -> String {
    "BreezeLight".into()
}

fn default_kde_dark() -> String {
    "BreezeDark".into()
}

impl Default for LightSwitchConfig {
    fn default() -> Self {
        Self {
            schedule_mode: ScheduleMode::Off,
            latitude: 0.0,
            longitude: 0.0,
            sunrise_offset_min: 0,
            sunset_offset_min: 0,
            dark_mode_time: default_dark_time(),
            light_mode_time: default_light_time(),
            apply_system: true,
            apply_apps: true,
            hotkey: default_hotkey(),
            desktop: DesktopKind::Auto,
            switch_gtk_theme: true,
            light_gtk_theme: default_light_gtk(),
            dark_gtk_theme: default_dark_gtk(),
            kde_light_color_scheme: default_kde_light(),
            kde_dark_color_scheme: default_kde_dark(),
            kde_light_look_and_feel: None,
            kde_dark_look_and_feel: None,
            light_wallpaper: None,
            dark_wallpaper: None,
            dark_mode_power_display_profile: None,
            light_mode_power_display_profile: None,
        }
    }
}

/// Settings resolved for one mode (light or dark).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModeSettings<'a> {
    pub dark: bool,
    pub gtk_theme: &'a str,
    pub kde_color_scheme: &'a str,
    pub kde_look_and_feel: Option<&'a str>,
    pub wallpaper: Option<&'a str>,
    pub power_display_profile: Option<&'a str>,
}

fn non_empty(v: &Option<String>) -> Option<&str> {
    v.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

impl LightSwitchConfig {
    pub fn mode(&self, dark: bool) -> ModeSettings<'_> {
        if dark {
            ModeSettings {
                dark,
                gtk_theme: self.dark_gtk_theme.trim(),
                kde_color_scheme: self.kde_dark_color_scheme.trim(),
                kde_look_and_feel: non_empty(&self.kde_dark_look_and_feel),
                wallpaper: non_empty(&self.dark_wallpaper),
                power_display_profile: non_empty(&self.dark_mode_power_display_profile),
            }
        } else {
            ModeSettings {
                dark,
                gtk_theme: self.light_gtk_theme.trim(),
                kde_color_scheme: self.kde_light_color_scheme.trim(),
                kde_look_and_feel: non_empty(&self.kde_light_look_and_feel),
                wallpaper: non_empty(&self.light_wallpaper),
                power_display_profile: non_empty(&self.light_mode_power_display_profile),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Serialize)]
    struct Wrap {
        schedule_mode: ScheduleMode,
    }

    fn parse(v: &str) -> ScheduleMode {
        toml::from_str::<Wrap>(&format!("schedule_mode = {v:?}"))
            .unwrap()
            .schedule_mode
    }

    #[test]
    fn parses_existing_values() {
        assert_eq!(parse("off"), ScheduleMode::Off);
        assert_eq!(parse("sunset-sunrise"), ScheduleMode::SunsetSunrise);
        assert_eq!(parse("fixed"), ScheduleMode::Fixed);
    }

    #[test]
    fn parses_upstream_values() {
        assert_eq!(parse("Off"), ScheduleMode::Off);
        assert_eq!(parse("SunsetToSunrise"), ScheduleMode::SunsetSunrise);
        assert_eq!(parse("sunset-to-sunrise"), ScheduleMode::SunsetSunrise);
        assert_eq!(parse("FixedHours"), ScheduleMode::Fixed);
        assert_eq!(parse("fixed_hours"), ScheduleMode::Fixed);
        assert_eq!(parse("FollowNightLight"), ScheduleMode::FollowNightLight);
        assert_eq!(parse("follow-night-light"), ScheduleMode::FollowNightLight);
    }

    #[test]
    fn unknown_falls_back_to_off() {
        assert_eq!(parse("whenever"), ScheduleMode::Off);
        assert_eq!(parse(""), ScheduleMode::Off);
        assert!(ScheduleMode::parse("whenever").is_none());
        assert!("whenever".parse::<ScheduleMode>().is_err());
    }

    #[test]
    fn serializes_as_legacy_strings() {
        for (mode, s) in [
            (ScheduleMode::Off, "off"),
            (ScheduleMode::SunsetSunrise, "sunset-sunrise"),
            (ScheduleMode::Fixed, "fixed"),
            (ScheduleMode::FollowNightLight, "follow-night-light"),
        ] {
            let out = toml::to_string(&Wrap {
                schedule_mode: mode,
            })
            .unwrap();
            assert_eq!(out.trim(), format!("schedule_mode = \"{s}\""));
            assert_eq!(parse(s), mode);
        }
    }

    #[test]
    fn str_comparison() {
        assert!(ScheduleMode::Fixed == "fixed");
        assert!(ScheduleMode::SunsetSunrise == "SunsetToSunrise");
        assert!(ScheduleMode::Off != "fixed");
    }

    #[test]
    fn legacy_file_parses_with_defaults() {
        let cfg: LightSwitchConfig = toml::from_str(
            "schedule_mode = \"fixed\"\ndark_mode_time = \"21:00\"\napply_apps = false\n",
        )
        .unwrap();
        assert_eq!(cfg.schedule_mode, ScheduleMode::Fixed);
        assert_eq!(cfg.dark_mode_time, "21:00");
        assert!(!cfg.apply_apps);
        assert_eq!(cfg.hotkey, DEFAULT_HOTKEY);
        assert_eq!(cfg.dark_gtk_theme, "Adwaita-dark");
        assert!(cfg.dark_mode_power_display_profile.is_none());
    }

    #[test]
    fn mode_settings_skip_blank_options() {
        let cfg = LightSwitchConfig {
            dark_wallpaper: Some("  ".into()),
            dark_mode_power_display_profile: Some("Night".into()),
            ..Default::default()
        };
        let dark = cfg.mode(true);
        assert!(dark.wallpaper.is_none());
        assert_eq!(dark.power_display_profile, Some("Night"));
        assert_eq!(dark.gtk_theme, "Adwaita-dark");
        let light = cfg.mode(false);
        assert_eq!(light.kde_color_scheme, "BreezeLight");
        assert!(light.power_display_profile.is_none());
    }
}
