//! Parity tests against Microsoft PowerToys ZoomIt.
//!
//! Oracles:
//! - src/modules/ZoomIt/ZoomIt/ZoomItSettings.h (registry defaults)
//! - src/modules/ZoomIt/ZoomIt/ZoomIt.h (PEN_WIDTH, MIN/MAX_PEN_WIDTH, ZOOM_LEVEL_MAX, colors)
//! - src/modules/ZoomIt/ZoomIt/Zoomit.cpp (g_ZoomLevels, WM_MOUSEWHEEL telescope zoom,
//!   break timer text "% 2d:%02d" / "(-% 2d:%02d)", timer positions)
//! - src/settings-ui/Settings.UI.Library/ZoomItProperties.cs (default hotkeys)

use crate::config::{ZOOM_LEVELS, ZoomItConfig};
use crate::geometry::{
    MAX_PEN_WIDTH, MAX_ZOOM, MIN_PEN_WIDTH, Point, clamp_pen_width, format_timer, timer_position,
    zoom_in_step, zoom_out_step, zoom_source_rect,
};

// ---------------------------------------------------------------------------
// Defaults
// ---------------------------------------------------------------------------

#[test]
fn default_hotkeys_match_zoomit_properties() {
    let c = ZoomItConfig::default();
    assert_eq!(c.toggle_key, "Ctrl+1");
    assert_eq!(c.draw_toggle_key, "Ctrl+2");
    assert_eq!(c.break_timer_key, "Ctrl+3");
    assert_eq!(c.live_zoom_toggle_key, "Ctrl+4");
    assert_eq!(c.record_toggle_key, "Ctrl+5");
    assert_eq!(c.snip_toggle_key, "Ctrl+6");
    assert_eq!(c.snip_save_toggle_key, "Ctrl+Shift+6");
}

/// PowerToys also binds DemoType (Ctrl+7), Panorama snip (Ctrl+8 / Ctrl+Shift+8),
/// Mirror (Ctrl+9) and Snip OCR (Ctrl+Alt+6).
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn additional_powertoys_actions_exist() {
    let missing: Vec<&str> = [
        "demotype",
        "panorama",
        "panorama-save",
        "mirror",
        "snip-ocr",
    ]
    .into_iter()
    .filter(|a| !crate::ACTIONS.contains(a))
    .collect();
    assert!(missing.is_empty(), "missing ZoomIt actions: {missing:?}");
}

#[test]
fn default_settings_match_registry_defaults() {
    let c = ZoomItConfig::default();
    assert!(c.show_expired_time); // g_ShowExpiredTime = 1
    assert_eq!(c.zoom_initial_level, 3); // g_SliderZoomLevel = 3
    assert!(c.animate_zoom);
    assert!(c.smooth_image);
    assert_eq!(c.pen_color.to_uppercase(), "#FF0000"); // COLOR_RED
    assert_eq!(c.break_pen_color.to_uppercase(), "#FF0000");
    assert_eq!(c.break_background_color, "#000000");
    assert_eq!(c.pen_width, 5.0); // PEN_WIDTH
    assert_eq!(c.break_timeout, 10);
    assert_eq!(c.break_opacity, 100);
    assert_eq!(c.break_timer_position, 4);
    assert!(!c.break_play_sound_file);
    assert!(c.break_show_desktop);
    assert!(!c.break_show_background_file);
    assert!(!c.break_background_stretch);
    assert_eq!(c.record_frame_rate, 30);
    assert_eq!(c.record_scaling, 100);
    assert!(!c.capture_audio); // g_CaptureAudio = FALSE
    assert!(!c.snip_copy_to_clipboard);
}

/// g_ZoomLevels[] and ZoominSliderLevel 3 -> 2.0x.
#[test]
fn zoom_levels_and_initial_zoom() {
    assert_eq!(ZOOM_LEVELS, [1.25, 1.5, 1.75, 2.0, 3.0, 4.0]);
    assert_eq!(ZoomItConfig::default().initial_zoom(), 2.0);
    for (i, z) in ZOOM_LEVELS.iter().enumerate() {
        let c = ZoomItConfig {
            zoom_initial_level: i as u32,
            ..Default::default()
        };
        assert_eq!(c.initial_zoom(), *z);
    }
}

// ---------------------------------------------------------------------------
// Telescope zoom (WM_MOUSEWHEEL)
// ---------------------------------------------------------------------------

/// Reference: zoom in: <2 -> 2, else *2 while < ZOOM_LEVEL_MAX(256);
/// zoom out: <=2 -> *0.75 (min 1), else /2.
fn ref_in(t: f32) -> f32 {
    if t < 256.0 {
        if t < 2.0 { 2.0 } else { t * 2.0 }
    } else {
        t
    }
}
fn ref_out(t: f32) -> f32 {
    if t > 1.0 {
        if t <= 2.0 {
            (t * 0.75).max(1.0)
        } else {
            t / 2.0
        }
    } else {
        t
    }
}

#[test]
fn telescope_zoom_matches_reference_up_to_rust_cap() {
    let mut a = 1.25f32;
    let mut b = 1.25f32;
    for _ in 0..4 {
        a = zoom_in_step(a);
        b = ref_in(b);
        assert_eq!(a, b);
    }
    for _ in 0..10 {
        a = zoom_out_step(a);
        b = ref_out(b);
        assert!((a - b).abs() < 1e-6, "{a} vs {b}");
    }
}

/// Upstream ZOOM_LEVEL_MAX is 256; Rust caps at 32 (documented in geometry.rs).
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn max_zoom_is_256() {
    let mut z = 2.0;
    for _ in 0..10 {
        z = zoom_in_step(z);
    }
    assert_eq!(z, 256.0, "MAX_ZOOM = {MAX_ZOOM}");
}

#[test]
fn zoom_source_rect_stays_on_screen() {
    for &z in &[1.0f32, 1.25, 2.0, 4.0, 32.0] {
        for &(fx, fy) in &[(0.0, 0.0), (1599.0, 999.0), (800.0, 500.0), (-50.0, 2000.0)] {
            let r = zoom_source_rect(1600.0, 1000.0, Point::new(fx, fy), z);
            assert!(
                r.x >= 0.0
                    && r.y >= 0.0
                    && r.right() <= 1600.0 + 1e-3
                    && r.bottom() <= 1000.0 + 1e-3
            );
            assert!((r.w - 1600.0 / z).abs() < 1e-3);
        }
    }
}

// ---------------------------------------------------------------------------
// Pen width (MIN_PEN_WIDTH 2 .. MAX_PEN_WIDTH 40)
// ---------------------------------------------------------------------------

#[test]
fn pen_width_range() {
    assert_eq!((MIN_PEN_WIDTH, MAX_PEN_WIDTH), (2.0, 40.0));
    assert_eq!(clamp_pen_width(1.0), 2.0);
    assert_eq!(clamp_pen_width(41.0), 40.0);
    assert_eq!(clamp_pen_width(5.0), 5.0);
}

// ---------------------------------------------------------------------------
// Break timer text
// ---------------------------------------------------------------------------

/// `% 2d:%02d` of total minutes: no hour field, so 90 minutes -> "90:00".
/// (Single-digit minutes get a leading space upstream; trimmed here.)
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn timer_text_uses_minutes_only() {
    assert_eq!(format_timer(600).trim(), "10:00");
    assert_eq!(format_timer(65).trim(), "1:05");
    assert_eq!(format_timer(99 * 60).trim(), "99:00");
    assert_eq!(format_timer(90 * 60).trim(), "90:00");
}

/// Expired time is printed as `(-% 2d:%02d)` below the main "0:00" text.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn expired_timer_text_format() {
    assert_eq!(format_timer(-7).replace(' ', ""), "(-0:07)");
}

/// Positions: rows top=50 / centered / bottom-50, cols left=50 / centered / right-50.
#[test]
fn timer_positions_match_grid() {
    let (w, h, tw, th) = (1600.0, 1000.0, 200.0, 100.0);
    let expect = |p: u32| {
        let x = match p % 3 {
            0 => 50.0,
            1 => (w - tw) / 2.0,
            _ => w - tw - 50.0,
        };
        let y = match p / 3 {
            0 => 50.0,
            1 => (h - th) / 2.0,
            _ => h - th - 50.0,
        };
        Point::new(x, y)
    };
    for p in 0..9 {
        assert_eq!(
            timer_position(p, w, h, tw, th, 50.0),
            expect(p),
            "position {p}"
        );
    }
}

/// Settings UI clamps BreakTimeout to 1..99 minutes; config default round trip.
#[test]
fn break_timeout_round_trip() {
    let c: ZoomItConfig = toml::from_str("break_timeout = 99").unwrap();
    assert_eq!(c.break_timeout, 99);
}
