//! Parity tests for Mouse Utilities defaults.
//! Oracle: src/settings-ui/Settings.UI.Library/{FindMyMouse,MouseHighlighter,
//! MousePointerCrosshairs,MouseJump,CursorWrap}Properties.cs and
//! MouseJumpThumbnailSize.cs.

use crate::config::*;

fn d() -> MouseUtilsConfig {
    MouseUtilsConfig::default()
}

/// Effective (rgb, alpha) of a colour/opacity pair.
fn argb(s: &str) -> (u32, f32) {
    parse_argb(s).unwrap()
}

// -- Find My Mouse -----------------------------------------------------------

#[test]
fn fmm_activation_default_left_ctrl_twice() {
    // ActivationMethod = 0 (Press Left Control twice)
    assert_eq!(
        d().find_my_mouse_activation,
        FindMyMouseActivation::LeftCtrlTwice
    );
}

#[test]
fn fmm_spotlight_radius_animation_shake_defaults() {
    let c = d();
    assert_eq!(c.find_my_mouse_spotlight_radius, 100);
    assert_eq!(c.find_my_mouse_animation_ms, 500);
    assert_eq!(c.find_my_mouse_shake_distance, 1000);
    assert_eq!(c.find_my_mouse_shake_interval_ms, 1000);
    assert_eq!(c.find_my_mouse_shake_factor, 400);
    assert!(c.find_my_mouse_game_mode);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn fmm_initial_zoom_default_is_9() {
    // SpotlightInitialZoom = new IntProperty(9)
    assert_eq!(d().find_my_mouse_initial_zoom, 9);
}

#[test]
fn fmm_background_is_black_at_50_percent() {
    // BackgroundColor = "#80000000" (ARGB)
    let c = d();
    let (rgb, a) = argb(&c.find_my_mouse_bg_color);
    let eff = a * c.find_my_mouse_overlay_opacity;
    assert_eq!(rgb, 0x000000);
    assert!((eff - 128.0 / 255.0).abs() < 0.01, "effective alpha {eff}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn fmm_spotlight_color_white_at_50_percent() {
    // SpotlightColor = "#80FFFFFF"
    let (rgb, a) = argb(&d().find_my_mouse_spotlight_color);
    assert_eq!(rgb, 0xFFFFFF);
    assert!((a - 128.0 / 255.0).abs() < 0.01, "alpha {a}");
}

// -- Mouse Highlighter -------------------------------------------------------

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn highlighter_radius_default_30() {
    assert_eq!(d().highlighter_radius, 30);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn highlighter_fade_defaults_400_400() {
    let c = d();
    assert_eq!(
        (c.highlighter_fade_delay_ms, c.highlighter_fade_duration_ms),
        (400, 400)
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn highlighter_left_click_color_default() {
    // DefaultLeftButtonClickColor = "#a6BFFF00"
    let c = d();
    let (rgb, a) = argb(&c.highlighter_primary_color);
    assert_eq!(rgb, 0xBFFF00);
    let eff = a * c.highlighter_opacity;
    assert!(
        (eff - 0xa6 as f32 / 255.0).abs() < 0.01,
        "effective alpha {eff}"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn highlighter_right_click_color_default() {
    // DefaultRightButtonClickColor = "#a600BFFF"
    let (rgb, _) = argb(&d().highlighter_secondary_color);
    assert_eq!(rgb, 0x00BFFF);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn highlighter_auto_activate_default_false() {
    assert!(!d().highlighter_auto_activate);
}

// -- Crosshairs --------------------------------------------------------------

#[test]
fn crosshairs_colour_opacity_radius_thickness() {
    let c = d();
    assert_eq!(c.crosshair_color.to_uppercase(), "#FF0000");
    assert!((c.crosshair_opacity - 0.75).abs() < 1e-6);
    assert_eq!(c.crosshair_center_radius, 20);
    assert_eq!(c.crosshair_thickness, 5);
    assert_eq!(c.crosshair_border_color.to_uppercase(), "#FFFFFF");
    assert_eq!(c.crosshair_orientation, CrosshairOrientation::Both);
    assert!(!c.crosshair_auto_hide);
    assert!(!c.crosshair_fixed_length);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn crosshairs_border_size_default_1() {
    assert_eq!(d().crosshair_border_size, 1);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn crosshairs_fixed_length_default_1() {
    assert_eq!(d().crosshair_fixed_length_px, 1);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn crosshairs_auto_activate_default_false() {
    assert!(!d().crosshair_auto_activate);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn gliding_cursor_speed_defaults_25_5() {
    let c = d();
    assert_eq!(
        (c.gliding_cursor_travel_speed, c.gliding_cursor_delay_speed),
        (25, 5)
    );
}

// -- Mouse Jump / Cursor Wrap --------------------------------------------------

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn mouse_jump_thumbnail_default_1600x1200() {
    let c = d();
    assert_eq!(
        (c.mouse_jump_max_width, c.mouse_jump_max_height),
        (1600, 1200)
    );
}

#[test]
fn cursor_wrap_defaults() {
    let c = d();
    assert_eq!(c.cursor_wrap_mode, CursorWrapMode::Both);
    assert_eq!(c.cursor_wrap_activation_mode, CursorWrapActivation::Always);
    assert!(c.cursor_wrap_disable_during_drag);
    assert!(!c.cursor_wrap_disable_on_single_monitor);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn cursor_wrap_auto_activate_default_false() {
    assert!(!d().cursor_wrap_auto_activate);
}

// -- Shortcuts -----------------------------------------------------------------

#[test]
fn default_shortcuts_match_upstream() {
    let c = d();
    assert_eq!(c.find_my_mouse_shortcut, "Super+Shift+F"); // (win, shift, 0x46)
    assert_eq!(c.highlighter_shortcut, "Super+Shift+H"); // (win, shift, 0x48)
    assert_eq!(c.crosshair_shortcut, "Super+Alt+P");
    assert_eq!(c.gliding_cursor_shortcut, "Super+Alt+.");
    assert_eq!(c.mouse_jump_shortcut, "Super+Shift+D");
}
