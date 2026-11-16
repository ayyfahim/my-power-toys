//! Parity tests against Microsoft PowerToys Crop And Lock
//! (src/settings-ui/Settings.UI.Library/CropAndLockProperties.cs and
//! src/modules/CropAndLock/CropAndLock/OverlayWindow.cpp).

use mpt_common::hotkey::Hotkey;
use mpt_crop_and_lock::geometry::{CropRect, fit_centered};
use mpt_crop_and_lock::{CropAndLockConfig, CropMode, parse_cli_args};

/// OverlayWindow::OnLeftButtonDown/OnLeftButtonUp: start point is inside the
/// window bounds, end point is clamped to [0, w] x [0, h], rectangle is
/// ordered, and only an exactly-empty rectangle is rejected.
fn pt_crop(start: (i32, i32), end: (i32, i32), w: i32, h: i32) -> Option<CropRect> {
    if start.0 < 0 || start.0 >= w || start.1 < 0 || start.1 >= h {
        return None; // click outside the window closes the overlay
    }
    let x = end.0.clamp(0, w);
    let y = end.1.clamp(0, h);
    let (l, r) = if x < start.0 {
        (x, start.0)
    } else {
        (start.0, x)
    };
    let (t, b) = if y < start.1 {
        (y, start.1)
    } else {
        (start.1, y)
    };
    if r - l == 0 || b - t == 0 {
        return None;
    }
    Some(CropRect::new(
        l as u32,
        t as u32,
        (r - l) as u32,
        (b - t) as u32,
    ))
}

#[test]
fn default_hotkeys_match_powertoys() {
    let c = CropAndLockConfig::default();
    assert_eq!(
        c.hotkey(CropMode::Reparent),
        Hotkey::parse("Super+Ctrl+Shift+R").unwrap()
    );
    assert_eq!(
        c.hotkey(CropMode::Thumbnail),
        Hotkey::parse("Super+Ctrl+Shift+T").unwrap()
    );
    assert_eq!(
        c.hotkey(CropMode::Screenshot),
        Hotkey::parse("Super+Ctrl+Shift+S").unwrap()
    );
}

#[test]
fn crop_rect_matches_overlay_algorithm() {
    let (w, h) = (640, 480);
    let pts = [-50, 0, 1, 2, 100, 320, 479, 480, 639, 640, 700];
    let mut diffs = vec![];
    for &sx in &[0, 1, 100, 639] {
        for &sy in &[0, 5, 479] {
            for &ex in &pts {
                for &ey in &pts {
                    let want = pt_crop((sx, sy), (ex, ey), w, h);
                    let got = CropRect::from_corners(
                        (sx as f32, sy as f32),
                        (ex as f32, ey as f32),
                        w as u32,
                        h as u32,
                    );
                    if want != got {
                        diffs.push(format!(
                            "({sx},{sy})->({ex},{ey}): port={got:?} pt={want:?}"
                        ));
                    }
                }
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches, e.g. {:?}",
        diffs.len(),
        &diffs[..diffs.len().min(5)]
    );
}

#[test]
fn one_pixel_selection_is_accepted() {
    // PowerToys only rejects a zero-width/height rect.
    assert_eq!(
        CropRect::from_corners((10.0, 10.0), (11.0, 30.0), 100, 100),
        Some(CropRect::new(10, 10, 1, 20))
    );
}

#[test]
fn shrunk_source_window_clamps_crop() {
    let r = CropRect::new(100, 100, 200, 200);
    assert_eq!(
        r.clamp_to(250, 400),
        Some(CropRect::new(100, 100, 150, 200))
    );
    assert_eq!(r.clamp_to(100, 400), None);
}

#[test]
fn letterbox_keeps_aspect() {
    // Thumbnail window resized: content keeps the crop aspect ratio, centered.
    let (x, y, w, h) = fit_centered(300.0, 100.0, 600.0, 600.0);
    assert_eq!((x, y, w, h), (0.0, 200.0, 600.0, 200.0));
    let (x, y, w, h) = fit_centered(300.0, 100.0, 300.0, 100.0);
    assert_eq!((x, y, w, h), (0.0, 0.0, 300.0, 100.0));
}

#[test]
fn cli_accepts_all_three_modes() {
    for m in ["thumbnail", "reparent", "screenshot"] {
        let a = parse_cli_args(["--mode", m, "--window", "0x10"]).unwrap();
        assert_eq!(a.mode.as_str(), m);
        assert_eq!(a.window, 16);
    }
}
