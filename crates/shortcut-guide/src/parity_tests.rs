//! Parity tests against Microsoft PowerToys Shortcut Guide.
//! Oracle: src/settings-ui/Settings.UI.Library/ShortcutGuideProperties.cs and
//! src/common/utils/excluded_apps.h (DisabledApps matching).

use crate::config::*;

#[test]
fn default_press_time_is_900ms_range_100_5000() {
    let c = ShortcutGuideConfig::default();
    assert_eq!(c.press_time, 900);
    assert_eq!((MIN_PRESS_TIME_MS, MAX_PRESS_TIME_MS), (100, 5000));
}

#[test]
fn default_win_key_action_is_taskbar_indicators() {
    assert_eq!(
        ShortcutGuideConfig::default().win_key_action,
        WinKeyAction::TaskbarIndicators
    );
}

#[test]
fn default_close_on_windows_key_release_is_true() {
    assert!(ShortcutGuideConfig::default().close_on_windows_key_release);
}

#[test]
fn default_theme_is_system() {
    assert_eq!(ShortcutGuideConfig::default().theme, GuideTheme::System);
}

#[test]
fn default_hotkey_is_win_shift_slash() {
    // HotkeySettings(win=true, ctrl=false, alt=false, shift=true, 0xBF '/')
    assert_eq!(ShortcutGuideConfig::default().hotkey, "Super+?");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn default_window_position_is_left() {
    // ShortcutGuideProperties: WindowPosition = ShortcutGuideWindowPosition.Left
    assert_eq!(
        ShortcutGuideConfig::default().window_position,
        WindowPosition::Left
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn disabled_apps_match_like_upstream_prefix() {
    // find_app_name_in_path: "STEAM" matches "...\\steamwebhelper.exe".
    let c = ShortcutGuideConfig {
        disabled_apps: vec!["steam".into()],
        ..Default::default()
    };
    assert!(c.is_app_disabled(&["steamwebhelper".into()]));
}

#[test]
fn press_time_clamp() {
    let c = ShortcutGuideConfig {
        press_time: 50,
        ..Default::default()
    };
    assert_eq!(c.press_time_ms(), 100);
}
