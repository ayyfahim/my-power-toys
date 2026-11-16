//! Parity tests against Microsoft PowerToys Screen Ruler (MeasureTool).
//!
//! Oracles:
//! - src/modules/MeasureTool/MeasureToolCore/EdgeDetection.cpp (FindEdge / DetectEdges)
//! - src/modules/MeasureTool/MeasureToolCore/BGRATextureView.h (PixelsClose)
//! - src/modules/MeasureTool/MeasureToolCore/MeasurementLogic.h + Tests/MeasureToolCore.UnitTests
//! - src/modules/MeasureTool/MeasureToolCore/Measurement.cpp (Print / PrintToStream)
//! - src/settings-ui/Settings.UI.Library/MeasureToolProperties.cs (defaults)

// Oracle signature mirrors PowerToys EdgeDetection.cpp.
#![allow(clippy::too_many_arguments)]

use crate::config::{MeasureStyle, ScreenRulerConfig, UnitsOfMeasure};
use crate::edges::{EdgeRect, RgbaView, detect_edges, pixels_close};
use crate::measurement::{clipboard_text, convert_pixels, label_text};

// ---------------------------------------------------------------------------
// Reference transliteration of the PowerToys C++ edge detection
// ---------------------------------------------------------------------------

fn ref_pixels_close(p1: [u8; 4], p2: [u8; 4], tol: u8, per_channel: bool) -> bool {
    let d: Vec<u16> = (0..4).map(|i| p1[i].abs_diff(p2[i]) as u16).collect();
    if per_channel {
        // _mm_cmpgt_epi16(distances16, tol) == 0 over the low 64 bits
        d.iter().all(|&x| x <= tol as u16)
    } else {
        let score = (d.iter().sum::<u16>() as i32) & 0xFFFF;
        score <= tol as i32
    }
}

fn ref_find_edge(
    px: &dyn Fn(i64, i64) -> [u8; 4],
    w: i64,
    h: i64,
    cx: i64,
    cy: i64,
    tol: u8,
    per_channel: bool,
    is_x: bool,
    inc: bool,
) -> i64 {
    let max_dim = if is_x { w } else { h };
    let mut x = cx.clamp(1, w - 2);
    let mut y = cy.clamp(1, h - 2);
    let start = px(x, y);
    loop {
        let (ox, oy) = (x, y);
        if is_x {
            if inc {
                x += 1;
                if x == max_dim {
                    break;
                }
            } else {
                x -= 1;
                if x < 0 {
                    break;
                }
            }
        } else if inc {
            y += 1;
            if y == max_dim {
                break;
            }
        } else {
            y -= 1;
            if y < 0 {
                break;
            }
        }
        if !ref_pixels_close(start, px(x, y), tol, per_channel) {
            return if is_x { ox } else { oy };
        }
    }
    if inc {
        (if is_x { w } else { h }) - 1
    } else {
        0
    }
}

fn ref_detect(data: &[u8], w: u32, h: u32, c: (i64, i64), per_channel: bool, tol: u8) -> EdgeRect {
    let px = |x: i64, y: i64| {
        let i = ((y as usize) * w as usize + x as usize) * 4;
        [data[i], data[i + 1], data[i + 2], data[i + 3]]
    };
    let (w, h) = (w as i64, h as i64);
    let f = |is_x, inc| ref_find_edge(&px, w, h, c.0, c.1, tol, per_channel, is_x, inc) as u32;
    EdgeRect {
        left: f(true, false),
        top: f(false, false),
        right: f(true, true),
        bottom: f(false, true),
    }
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
}

/// Random "screens" made of a few flat rectangles plus noise, compared with
/// the C++ algorithm for many cursors/tolerances/modes.
#[test]
fn edge_detection_matches_powertoys_reference_on_random_images() {
    let mut rng = Lcg(0x5eed);
    let mut checked = 0;
    for _ in 0..60 {
        let w = 3 + rng.next() % 60;
        let h = 3 + rng.next() % 60;
        let mut data = vec![0u8; (w * h * 4) as usize];
        let base = [rng.next() as u8, rng.next() as u8, rng.next() as u8, 255];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let noise = (rng.next() % 12) as u8;
                data[i] = base[0].wrapping_add(noise);
                data[i + 1] = base[1].wrapping_add(noise / 2);
                data[i + 2] = base[2];
                data[i + 3] = 255;
            }
        }
        for _ in 0..4 {
            let (x0, y0) = (rng.next() % w, rng.next() % h);
            let (x1, y1) = (
                (x0 + rng.next() % 20).min(w - 1),
                (y0 + rng.next() % 20).min(h - 1),
            );
            let col = [rng.next() as u8, rng.next() as u8, rng.next() as u8, 255];
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let i = ((y * w + x) * 4) as usize;
                    data[i..i + 4].copy_from_slice(&col);
                }
            }
        }
        let view = RgbaView::new(&data, w, h).unwrap();
        for _ in 0..20 {
            let c = (
                (rng.next() % (w + 10)) as i64 - 5,
                (rng.next() % (h + 10)) as i64 - 5,
            );
            for per_channel in [false, true] {
                for tol in [0u8, 1, 15, 30, 45, 100, 255] {
                    let got = detect_edges(&view, c, per_channel, tol).unwrap();
                    let exp = ref_detect(&data, w, h, c, per_channel, tol);
                    assert_eq!(
                        got, exp,
                        "w={w} h={h} cursor={c:?} per_channel={per_channel} tol={tol}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 10_000);
}

/// PowerToys comment in ScreenCapturing.cpp: 5x5 green square at 20x100 on blue
/// -> bounds [20,100]-[24,104], dims 5x5.
#[test]
fn edge_detection_upstream_square_example() {
    let (w, h) = (64u32, 128u32);
    let mut data = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let green = (20..25).contains(&x) && (100..105).contains(&y);
            data.extend_from_slice(&if green {
                [0, 255, 0, 255]
            } else {
                [0, 0, 255, 255]
            });
        }
    }
    let view = RgbaView::new(&data, w, h).unwrap();
    for c in [(20, 100), (24, 104), (22, 102)] {
        let r = detect_edges(&view, c, false, 30).unwrap();
        assert_eq!((r.left, r.top, r.right, r.bottom), (20, 100, 24, 104));
        assert_eq!((r.width(), r.height()), (5, 5));
    }
}

#[test]
fn pixels_close_matches_simd_semantics_exhaustive_sample() {
    let mut rng = Lcg(7);
    for _ in 0..200_000 {
        let a = [
            rng.next() as u8,
            rng.next() as u8,
            rng.next() as u8,
            rng.next() as u8,
        ];
        let b = [
            rng.next() as u8,
            rng.next() as u8,
            rng.next() as u8,
            rng.next() as u8,
        ];
        let t = rng.next() as u8;
        for pc in [false, true] {
            assert_eq!(
                pixels_close(a, b, t, pc),
                ref_pixels_close(a, b, t, pc),
                "{a:?} {b:?} {t} {pc}"
            );
        }
    }
}

/// Tolerance boundary: summed difference exactly == tolerance is "close"
/// (`score <= tolerance`), tolerance + 1 is an edge.
#[test]
fn tolerance_boundary_is_inclusive() {
    let a = [100, 100, 100, 255];
    assert!(pixels_close(a, [110, 110, 110, 255], 30, false));
    assert!(!pixels_close(a, [110, 110, 111, 255], 30, false));
    assert!(pixels_close(a, [130, 70, 130, 255], 30, true));
    assert!(!pixels_close(a, [131, 100, 100, 255], 30, true));
}

// ---------------------------------------------------------------------------
// MeasurementLogicTests.cpp
// ---------------------------------------------------------------------------

#[test]
fn pixels_are_unchanged() {
    assert_eq!(convert_pixels(150.0, UnitsOfMeasure::Pixels, None), 150.0);
    assert_eq!(
        convert_pixels(150.0, UnitsOfMeasure::Pixels, Some(0.3)),
        150.0
    );
}

#[test]
fn physical_unit_fallback_uses_96_dpi() {
    assert!((convert_pixels(96.0, UnitsOfMeasure::Inches, None) - 1.0).abs() < 1e-4);
    assert!((convert_pixels(96.0, UnitsOfMeasure::Centimeters, None) - 2.54).abs() < 1e-4);
    assert!((convert_pixels(96.0, UnitsOfMeasure::Millimeters, None) - 25.4).abs() < 1e-4);
    // px2mmRatio = -1 in upstream tests means unknown.
    assert!((convert_pixels(96.0, UnitsOfMeasure::Inches, Some(-1.0)) - 1.0).abs() < 1e-4);
}

#[test]
fn physical_monitor_ratio_remains_preferred() {
    assert!((convert_pixels(100.0, UnitsOfMeasure::Inches, Some(0.254)) - 1.0).abs() < 1e-4);
    assert!((convert_pixels(100.0, UnitsOfMeasure::Centimeters, Some(0.254)) - 2.54).abs() < 1e-4);
    assert!((convert_pixels(100.0, UnitsOfMeasure::Millimeters, Some(0.254)) - 25.4).abs() < 1e-4);
}

/// PowerToys has 5 units (index 4 = DIP, "DIP" abbreviation, pixels * 96 / monitorDpi).
/// The Rust enum only has 4; a settings file with the DIP unit is rejected.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn dip_unit_exists() {
    let r: Result<ScreenRulerConfig, _> = toml::from_str("units_of_measure = \"dip\"");
    assert!(r.is_ok(), "DIP unit missing: {:?}", r.err());
}

// ---------------------------------------------------------------------------
// Measurement::Print (overlay label, %.4g) and PrintToStream (clipboard, ostream default precision 6)
// ---------------------------------------------------------------------------

#[test]
fn overlay_label_matches_print() {
    assert_eq!(
        label_text(100, 50, true, true, UnitsOfMeasure::Pixels, None),
        "100 \u{d7} 50 px"
    );
    assert_eq!(
        label_text(100, 50, true, false, UnitsOfMeasure::Pixels, None),
        "100 px"
    );
    assert_eq!(
        label_text(100, 50, false, true, UnitsOfMeasure::Pixels, None),
        "50 px"
    );
    // 96 px = 25.4 mm ; 48 px = 12.7 mm
    assert_eq!(
        label_text(96, 48, true, true, UnitsOfMeasure::Millimeters, None),
        "96 \u{d7} 48 px\n(25.4 \u{d7} 12.7 mm)"
    );
    // %.4g of 100/96 in = 1.042
    assert_eq!(
        label_text(100, 1, true, false, UnitsOfMeasure::Inches, None),
        "100 px (1.042 in)"
    );
}

/// Clipboard text is written with `std::wostream << float` (6 significant
/// digits), not `%.4g`. 1 px = 0.0104167 in, 100 px = 2.64583 cm.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn clipboard_uses_stream_precision_six() {
    assert_eq!(
        clipboard_text(&[(1, 1)], true, false, UnitsOfMeasure::Inches, None),
        "0.0104167 in"
    );
    assert_eq!(
        clipboard_text(&[(100, 1)], true, false, UnitsOfMeasure::Centimeters, None),
        "2.64583 cm"
    );
    assert_eq!(
        clipboard_text(&[(100, 1)], true, false, UnitsOfMeasure::Millimeters, None),
        "26.4583 mm"
    );
}

#[test]
fn clipboard_pixels_have_no_unit_and_one_line_per_measurement() {
    assert_eq!(
        clipboard_text(
            &[(10, 20), (30, 40)],
            true,
            true,
            UnitsOfMeasure::Pixels,
            None
        ),
        "10 \u{d7} 20\n30 \u{d7} 40"
    );
    assert_eq!(
        clipboard_text(&[(10, 20)], false, true, UnitsOfMeasure::Pixels, None),
        "20"
    );
    assert_eq!(
        clipboard_text(&[(96, 96)], true, true, UnitsOfMeasure::Inches, None),
        "1 \u{d7} 1 in"
    );
}

/// Bounds tool: rect = minmax(start, cursor), width = right - left + 1.
#[test]
fn bounds_dims_inclusive() {
    let r = EdgeRect::from_corners((50, 60), (10, 20));
    assert_eq!((r.width(), r.height()), (41, 41));
}

// ---------------------------------------------------------------------------
// Defaults (MeasureToolProperties)
// ---------------------------------------------------------------------------

#[test]
fn defaults_match_measure_tool_properties() {
    let c = ScreenRulerConfig::default();
    assert_eq!(c.pixel_tolerance, 30);
    assert!(!c.continuous_capture);
    assert!(c.draw_feet_on_cross);
    assert!(!c.per_color_channel_edge_detection);
    assert_eq!(c.measure_cross_color, "#FF4500");
    assert_eq!(c.default_measure_style, MeasureStyle::None);
    assert_eq!(c.units_of_measure, UnitsOfMeasure::Pixels);
}

/// GetHorizontalVerticalLines: Bounds/None are not cross tools; Spacing draws both,
/// Horizontal only the horizontal line, Vertical only the vertical one.
#[test]
fn measure_style_lines() {
    assert_eq!(MeasureStyle::Spacing.lines(), (true, true));
    assert_eq!(MeasureStyle::HorizontalSpacing.lines(), (true, false));
    assert_eq!(MeasureStyle::VerticalSpacing.lines(), (false, true));
}
