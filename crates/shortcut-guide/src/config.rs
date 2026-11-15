use crate::shortcuts::ShortcutEntry;
use serde::{Deserialize, Serialize};

/// Default hotkey (upstream: Win+Shift+/ i.e. Win+?).
pub const DEFAULT_HOTKEY: &str = "Super+?";
/// Upstream `ShortcutGuideProperties.DefaultPressTimeMs`.
pub const DEFAULT_PRESS_TIME_MS: u32 = 900;
pub const MIN_PRESS_TIME_MS: u32 = 100;
pub const MAX_PRESS_TIME_MS: u32 = 5000;

/// What holding the Super key on its own does (upstream `WindowsKeyAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WinKeyAction {
    /// Holding Super does nothing.
    Off,
    /// Upstream shows taskbar number badges. Linux has no equivalent, so this
    /// behaves like `Off`.
    #[default]
    #[serde(alias = "taskbar")]
    TaskbarIndicators,
    /// Holding Super alone for `press_time` ms opens the overlay.
    #[serde(alias = "open", alias = "open_guide")]
    OpenShortcutGuide,
}

/// Overlay colour theme (upstream `Theme`: "light" / "dark" / "system").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuideTheme {
    Light,
    Dark,
    #[default]
    System,
}

/// Where the overlay is placed on the monitor (upstream `WindowPosition`
/// has Left/Right; Center keeps the original centred layout of this port).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowPosition {
    #[default]
    Center,
    Left,
    Right,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShortcutGuideConfig {
    /// Show the built-in GNOME shortcut list.
    #[serde(default = "default_true")]
    pub show_gnome_shortcuts: bool,

    /// Categories to hide from the overlay (matched case-insensitively).
    #[serde(default)]
    pub hidden_categories: Vec<String>,

    /// User-defined shortcuts appended to the built-in ones.
    #[serde(default)]
    pub custom_shortcuts: Vec<ShortcutEntry>,

    /// Hotkey that opens the overlay (upstream `OpenShortcutGuide`).
    #[serde(default = "default_hotkey")]
    pub hotkey: String,

    /// Behaviour when Super is held on its own (upstream `WindowsKeyAction`).
    #[serde(default)]
    pub win_key_action: WinKeyAction,

    /// How long Super must be held alone before the overlay opens, in ms
    /// (upstream `PressTime`, 100..=5000).
    #[serde(default = "default_press_time")]
    pub press_time: u32,

    /// Close the overlay when Super is released, if it was opened by holding
    /// Super (upstream `CloseOnWindowsKeyRelease`).
    #[serde(default = "default_true")]
    pub close_on_windows_key_release: bool,

    /// Overlay theme (upstream `Theme`).
    #[serde(default)]
    pub theme: GuideTheme,

    /// WM_CLASS values (instance or class, case-insensitive) of applications
    /// in which the guide must not open (upstream `DisabledApps`).
    #[serde(default)]
    pub disabled_apps: Vec<String>,

    /// Overlay placement (upstream `WindowPosition`).
    #[serde(default)]
    pub window_position: WindowPosition,
}

fn default_true() -> bool {
    true
}

fn default_hotkey() -> String {
    DEFAULT_HOTKEY.to_string()
}

fn default_press_time() -> u32 {
    DEFAULT_PRESS_TIME_MS
}

impl Default for ShortcutGuideConfig {
    fn default() -> Self {
        Self {
            show_gnome_shortcuts: true,
            hidden_categories: Vec::new(),
            custom_shortcuts: Vec::new(),
            hotkey: default_hotkey(),
            win_key_action: WinKeyAction::default(),
            press_time: DEFAULT_PRESS_TIME_MS,
            close_on_windows_key_release: true,
            theme: GuideTheme::default(),
            disabled_apps: Vec::new(),
            window_position: WindowPosition::default(),
        }
    }
}

impl ShortcutGuideConfig {
    /// Resolve the final shortcut list: built-in (optional) + custom, minus hidden categories.
    pub fn resolve(&self) -> Vec<ShortcutEntry> {
        let mut out = Vec::new();
        if self.show_gnome_shortcuts {
            out.extend(crate::shortcuts::gnome_shortcuts());
        }
        out.extend(self.custom_shortcuts.iter().cloned());
        out.retain(|s| {
            !self
                .hidden_categories
                .iter()
                .any(|c| c.trim().eq_ignore_ascii_case(s.category.trim()))
        });
        out
    }

    /// `press_time` clamped to the upstream range.
    pub fn press_time_ms(&self) -> u32 {
        self.press_time.clamp(MIN_PRESS_TIME_MS, MAX_PRESS_TIME_MS)
    }

    /// Whether holding Super alone should open the overlay.
    pub fn hold_to_open(&self) -> bool {
        self.win_key_action == WinKeyAction::OpenShortcutGuide
    }

    /// Whether the given window class (instance and/or class name) is listed
    /// in `disabled_apps`.
    pub fn is_app_disabled(&self, wm_classes: &[String]) -> bool {
        self.disabled_apps.iter().any(|app| {
            let app = app.trim();
            !app.is_empty() && wm_classes.iter().any(|c| c.eq_ignore_ascii_case(app))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_gnome_shortcuts() {
        let cfg = ShortcutGuideConfig::default();
        assert!(!cfg.resolve().is_empty());
    }

    #[test]
    fn hidden_category_filters_out() {
        let mut cfg = ShortcutGuideConfig::default();
        cfg.hidden_categories.push("Windows".into());
        let list = cfg.resolve();
        assert!(list.iter().all(|s| s.category != "Windows"));
    }

    #[test]
    fn custom_shortcut_appears() {
        let mut cfg = ShortcutGuideConfig {
            show_gnome_shortcuts: false,
            ..Default::default()
        };
        cfg.custom_shortcuts.push(ShortcutEntry {
            keys: "Ctrl+Alt+K".into(),
            description: "Custom action".into(),
            category: "Custom".into(),
        });
        let list = cfg.resolve();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].keys, "Ctrl+Alt+K");
    }

    #[test]
    fn old_config_file_still_parses() {
        let cfg: ShortcutGuideConfig = toml::from_str("show_gnome_shortcuts = false\n").unwrap();
        assert!(!cfg.show_gnome_shortcuts);
        assert_eq!(cfg.hotkey, DEFAULT_HOTKEY);
        assert_eq!(cfg.press_time, DEFAULT_PRESS_TIME_MS);
        assert_eq!(cfg.win_key_action, WinKeyAction::TaskbarIndicators);
        assert!(cfg.close_on_windows_key_release);
    }

    #[test]
    fn enums_parse() {
        let cfg: ShortcutGuideConfig = toml::from_str(
            "win_key_action = \"open_shortcut_guide\"\ntheme = \"light\"\nwindow_position = \"right\"\n",
        )
        .unwrap();
        assert!(cfg.hold_to_open());
        assert_eq!(cfg.theme, GuideTheme::Light);
        assert_eq!(cfg.window_position, WindowPosition::Right);
    }

    #[test]
    fn press_time_clamped() {
        let mut cfg = ShortcutGuideConfig {
            press_time: 10,
            ..Default::default()
        };
        assert_eq!(cfg.press_time_ms(), MIN_PRESS_TIME_MS);
        cfg.press_time = 99_999;
        assert_eq!(cfg.press_time_ms(), MAX_PRESS_TIME_MS);
    }

    #[test]
    fn disabled_apps_match_case_insensitive() {
        let cfg = ShortcutGuideConfig {
            disabled_apps: vec!["Steam".into(), " ".into()],
            ..Default::default()
        };
        assert!(cfg.is_app_disabled(&["steam".into(), "Steam".into()]));
        assert!(!cfg.is_app_disabled(&["firefox".into()]));
        assert!(!cfg.is_app_disabled(&[]));
    }

    #[test]
    fn round_trip() {
        let cfg = ShortcutGuideConfig {
            win_key_action: WinKeyAction::OpenShortcutGuide,
            ..Default::default()
        };
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: ShortcutGuideConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.win_key_action, WinKeyAction::OpenShortcutGuide);
    }
}
