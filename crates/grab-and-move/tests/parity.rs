//! Parity tests against Microsoft PowerToys Grab And Move
//! (src/modules/GrabAndMove/GrabAndMove/main.cpp, GrabAndMoveProperties.cs,
//! src/common/utils/excluded_apps.h).

use mpt_grab_and_move::geometry::{
    self, Handle, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH, Rect, apply_resize, closest_handle,
    restore_position,
};
use mpt_grab_and_move::{GrabAndMoveConfig, GrabModifier};

/// GetClosestHandle with C++ integer semantics (cx = (left+right)/2, truncating).
fn pt_closest(px: i32, py: i32, l: i32, t: i32, r: i32, b: i32) -> Handle {
    let cx = (l + r) / 2;
    let cy = (t + b) / 2;
    let hs = [
        (l, t, Handle::TopLeft),
        (cx, t, Handle::Top),
        (r, t, Handle::TopRight),
        (r, cy, Handle::Right),
        (r, b, Handle::BottomRight),
        (cx, b, Handle::Bottom),
        (l, b, Handle::BottomLeft),
        (l, cy, Handle::Left),
    ];
    let mut best = Handle::BottomRight;
    let mut min = i64::MAX;
    for (x, y, h) in hs {
        let d = ((px - x) as i64).pow(2) + ((py - y) as i64).pow(2);
        if d < min {
            min = d;
            best = h;
        }
    }
    best
}

#[test]
fn defaults_match_powertoys() {
    let c = GrabAndMoveConfig::default();
    assert_eq!(c.modifier_key, GrabModifier::Alt); // modifierKey 0
    assert!(c.use_alt_resize); // useAltResize true
    assert!(c.do_not_activate_on_game_mode); // true
    assert!(c.excluded_apps.is_empty());
    assert!(c.excluded_apps_for_modifier.is_empty());
    assert_eq!(c.drag_threshold, 4); // SM_CXDRAG default
    assert_eq!((MIN_WINDOW_WIDTH, MIN_WINDOW_HEIGHT), (150, 50));
}

#[test]
fn closest_handle_matches_on_positive_coordinates() {
    let rects = [(100, 100, 501, 401), (0, 0, 1920, 1080), (10, 20, 161, 71)];
    for (l, t, r, b) in rects {
        let rect = Rect::new(l, t, r - l, b - t);
        for px in (l - 30..r + 30).step_by(7) {
            for py in (t - 30..b + 30).step_by(7) {
                assert_eq!(
                    closest_handle(px, py, rect),
                    pt_closest(px, py, l, t, r, b),
                    "pt=({px},{py}) rect={rect:?}"
                );
            }
        }
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn closest_handle_matches_on_negative_coordinates() {
    // Windows on a monitor left of the primary have negative x.
    let mut diffs = vec![];
    for (l, t, r, b) in [(-1001, -501, -300, 200), (-301, 0, 100, 301)] {
        let rect = Rect::new(l, t, r - l, b - t);
        for px in (l - 10..r + 10).step_by(3) {
            for py in (t - 10..b + 10).step_by(3) {
                let (a, e) = (closest_handle(px, py, rect), pt_closest(px, py, l, t, r, b));
                if a != e {
                    diffs.push(format!("({px},{py}) in {rect:?}: port={a:?} pt={e:?}"));
                }
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches, e.g. {:?}",
        diffs.len(),
        &diffs[..diffs.len().min(3)]
    );
}

/// HandleDragResize applied over a pointer path, re-evaluating the handle at
/// each step (as upstream does).
#[test]
fn resize_path_matches_powertoys() {
    let start = Rect::new(200, 200, 600, 400);
    let path: Vec<(i32, i32)> = (0..40).map(|i| (790 - i * 25, 590 - i * 18)).collect();
    // PowerToys
    let (mut l, mut t, mut r, mut b) = (200, 200, 800, 600);
    let mut last = path[0];
    for &p in &path[1..] {
        let h = pt_closest(p.0, p.1, l, t, r, b);
        let (dx, dy) = (p.0 - last.0, p.1 - last.1);
        let left = matches!(h, Handle::TopLeft | Handle::BottomLeft | Handle::Left);
        let right = matches!(h, Handle::TopRight | Handle::BottomRight | Handle::Right);
        let top = matches!(h, Handle::TopLeft | Handle::Top | Handle::TopRight);
        let bottom = matches!(h, Handle::BottomLeft | Handle::Bottom | Handle::BottomRight);
        if left {
            l += dx
        }
        if right {
            r += dx
        }
        if top {
            t += dy
        }
        if bottom {
            b += dy
        }
        if r - l < 150 {
            if left { l = r - 150 } else { r = l + 150 }
        }
        if b - t < 50 {
            if top { t = b - 50 } else { b = t + 50 }
        }
        last = p;
    }
    // Port
    let mut rect = start;
    let mut last = path[0];
    for &p in &path[1..] {
        let h = closest_handle(p.0, p.1, rect);
        rect = apply_resize(
            rect,
            h,
            p.0 - last.0,
            p.1 - last.1,
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        last = p;
    }
    assert_eq!(rect, Rect::new(l, t, r - l, b - t));
}

#[test]
fn restore_from_maximized_matches() {
    let max = Rect::new(0, 0, 1920, 1080);
    for &(gx, gy) in &[(0, 0), (960, 540), (1919, 1079), (37, 801)] {
        let (w, h) = (800, 600);
        let rl = (gx - max.x) as f32 / max.w as f32;
        let rt = (gy - max.y) as f32 / max.h as f32;
        let want = (gx - (rl * w as f32) as i32, gy - (rt * h as f32) as i32);
        assert_eq!(restore_position((gx, gy), max, w, h), want);
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn excluded_app_matches_executable_name_prefix_like_powertoys() {
    // PowerToys find_app_name_in_path: "CODE" matches "...\CODE-OSS.EXE" because
    // the occurrence starts at the file name. The port only accepts exact names.
    let list = vec!["code".to_string()];
    assert!(geometry::matches_app(
        &list, "code-oss", "Code-oss", "code-oss", "Untitled"
    ));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn excluded_app_matches_full_path_like_powertoys() {
    // An entry with a folder ("bin/gimp") matches the end of the process path in PowerToys.
    let list = vec!["bin/gimp".to_string()];
    assert!(geometry::matches_app(
        &list,
        "gimp",
        "Gimp",
        "gimp",
        "GNU Image Manipulation Program"
    ));
}

#[test]
fn excluded_app_title_substring() {
    let list = vec!["Secret".to_string()];
    assert!(geometry::matches_app(&list, "x", "y", "z", "my secret doc"));
}
