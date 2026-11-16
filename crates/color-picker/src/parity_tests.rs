//! Parity tests against Microsoft PowerToys Color Picker.
//!
//! Oracles:
//! - src/common/ManagedCommon/ColorFormatHelper.cs (GetDefaultFormat / GetStringRepresentation)
//! - src/modules/colorPicker/ColorPickerUI.UnitTests/Helpers/ColorRepresentationHelperTest.cs
//! - src/modules/colorPicker/ColorPickerUI.UnitTests/Helpers/ColorFormatConversionTest.cs
//! - src/modules/colorPicker/ColorPickerUI.UnitTests/Helpers/ColorConverterTest.cs
//! - src/settings-ui/Settings.UI.Library/ColorPickerProperties.cs (defaults)
//! - src/common/ManagedCommon/ColorNameHelper.cs (color names)

use crate::color::{Color, ColorFormat};
use crate::history::ColorHistory;
use crate::{ClickAction, ColorFormatEntry, ColorPickerConfig};

fn hex(s: &str) -> Color {
    Color::from_hex(s).unwrap()
}

/// PowerToys format name -> Rust format.
fn fmt(name: &str) -> ColorFormat {
    ColorFormat::from_id(name).unwrap_or_else(|| panic!("no Rust format for PowerToys name {name}"))
}

/// .NET `Math.Round(v, digits)` (MidpointRounding.ToEven).
fn net_round(v: f64, digits: i32) -> f64 {
    let f = 10f64.powi(digits);
    let r = (v * f).round_ties_even() / f;
    if r == 0.0 { 0.0 } else { r }
}

fn check_strings(color: Color, rows: &[(&str, &str)]) {
    let mut failures = Vec::new();
    for (name, expected) in rows {
        let got = color.format(fmt(name));
        if got != *expected {
            failures.push(format!("{name}: expected {expected:?}, got {got:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "format mismatches for {color}:\n{}",
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// ColorRepresentationHelperTest.GetStringRepresentationTest (Color.Black)
// The HEX row is split into its own test so the other rows are checked too.
// ---------------------------------------------------------------------------

#[test]
fn string_repr_black_all_formats_except_hex() {
    check_strings(
        Color::new(0, 0, 0),
        &[
            ("CMYK", "cmyk(0%, 0%, 0%, 100%)"),
            ("NCol", "R0, 0%, 100%"),
            ("HSB", "hsb(0, 0%, 0%)"),
            ("HSI", "hsi(0, 0%, 0%)"),
            ("HSL", "hsl(0, 0%, 0%)"),
            ("HSV", "hsv(0, 0%, 0%)"),
            ("HWB", "hwb(0, 0%, 100%)"),
            ("RGB", "rgb(0, 0, 0)"),
            ("CIEXYZ", "XYZ(0, 0, 0)"),
            ("CIELAB", "CIELab(0, 0, 0)"),
            ("Oklab", "oklab(0, 0, 0)"),
            ("Oklch", "oklch(0, 0, 0)"),
            ("VEC4", "(0f, 0f, 0f, 1f)"),
            ("Decimal", "0"),
            ("HEX Int", "0xFF000000"),
        ],
    );
}

/// PowerToys HEX default format is `%Rex%Grx%Blx`: lowercase, no `#`.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn string_repr_hex_is_lowercase_without_hash() {
    assert_eq!(Color::new(0, 0, 0).format(ColorFormat::Hex), "000000");
    assert_eq!(Color::new(255, 0, 0).format(ColorFormat::Hex), "ff0000");
    assert_eq!(Color::new(255, 255, 255).format(ColorFormat::Hex), "ffffff");
    assert_eq!(Color::new(0, 255, 0).format(ColorFormat::Hex), "00ff00");
    assert_eq!(Color::new(0, 0, 255).format(ColorFormat::Hex), "0000ff");
}

// ---------------------------------------------------------------------------
// ColorFormatConversionTest.GetStringRepresentation_{Red,White,Green,Blue}
// ---------------------------------------------------------------------------

#[test]
fn string_repr_red() {
    check_strings(
        Color::new(255, 0, 0),
        &[
            ("CMYK", "cmyk(0%, 100%, 100%, 0%)"),
            ("RGB", "rgb(255, 0, 0)"),
            ("HSL", "hsl(0, 100%, 50%)"),
            ("HSV", "hsv(0, 100%, 100%)"),
            ("HSB", "hsb(0, 100%, 100%)"),
            ("HSI", "hsi(0, 100%, 33%)"),
            ("HWB", "hwb(0, 0%, 0%)"),
            ("Decimal", "255"),
            ("HEX Int", "0xFFFF0000"),
            ("VEC4", "(1f, 0f, 0f, 1f)"),
        ],
    );
}

#[test]
fn string_repr_white() {
    check_strings(
        Color::new(255, 255, 255),
        &[
            ("CMYK", "cmyk(0%, 0%, 0%, 0%)"),
            ("RGB", "rgb(255, 255, 255)"),
            ("HSL", "hsl(0, 0%, 100%)"),
            ("HSV", "hsv(0, 0%, 100%)"),
            ("Decimal", "16777215"),
            ("HEX Int", "0xFFFFFFFF"),
            ("VEC4", "(1f, 1f, 1f, 1f)"),
        ],
    );
}

#[test]
fn string_repr_green() {
    check_strings(
        Color::new(0, 255, 0),
        &[
            ("CMYK", "cmyk(100%, 0%, 100%, 0%)"),
            ("RGB", "rgb(0, 255, 0)"),
            ("HSL", "hsl(120, 100%, 50%)"),
            ("HSV", "hsv(120, 100%, 100%)"),
            ("Decimal", "65280"),
            ("HEX Int", "0xFF00FF00"),
        ],
    );
}

#[test]
fn string_repr_blue() {
    check_strings(
        Color::new(0, 0, 255),
        &[
            ("CMYK", "cmyk(100%, 100%, 0%, 0%)"),
            ("RGB", "rgb(0, 0, 255)"),
            ("HSL", "hsl(240, 100%, 50%)"),
            ("HSV", "hsv(240, 100%, 100%)"),
            ("Decimal", "16711680"),
            ("HEX Int", "0xFF0000FF"),
        ],
    );
}

/// Every PowerToys default format name must be resolvable.
#[test]
fn every_powertoys_format_name_is_known() {
    for n in [
        "HEX", "RGB", "HSL", "HSV", "CMYK", "HSB", "HSI", "HWB", "NCol", "CIEXYZ", "CIELAB",
        "Oklab", "Oklch", "VEC4", "Decimal", "HEX Int",
    ] {
        assert!(ColorFormat::from_id(n).is_some(), "{n}");
    }
}

/// Moccasin is PowerToys' example color (ColorFormatHelper.GetStringRepresentation(null, ..)).
/// Values computed with the PowerToys formulas (hue via Color.GetHue, banker's rounding).
#[test]
fn string_repr_moccasin_example_color() {
    check_strings(
        Color::new(255, 228, 181),
        &[
            ("RGB", "rgb(255, 228, 181)"),
            ("HSL", "hsl(38, 100%, 85%)"),
            ("HSV", "hsv(38, 29%, 100%)"),
            ("HSB", "hsb(38, 29%, 100%)"),
            ("HWB", "hwb(38, 71%, 0%)"),
            ("NCol", "R64, 71%, 0%"),
            ("CMYK", "cmyk(0%, 11%, 29%, 0%)"),
            ("VEC4", "(1f, 0.89f, 0.71f, 1f)"),
            ("Decimal", "11920639"),
            ("HEX Int", "0xFFFFE4B5"),
        ],
    );
}

// ---------------------------------------------------------------------------
// ColorConverterTest numeric conversions
// ---------------------------------------------------------------------------

/// (hue, sat%, light%, r%, g%, b%)
const HSL_ROWS: &[(f64, f64, f64, f64, f64, f64)] = &[
    (0., 0., 0., 0., 0., 0.),
    (0., 0., 100., 100., 100., 100.),
    (0., 100., 50., 100., 0., 0.),
    (15., 100., 50., 100., 25., 0.),
    (20., 60., 22.5, 36., 18., 9.),
    (30., 100., 50., 100., 50., 0.),
    (45., 100., 50., 100., 75., 0.),
    (60., 100., 50., 100., 100., 0.),
    (75., 100., 50., 75., 100., 0.),
    (90., 100., 50., 50., 100., 0.),
    (105., 100., 50., 25., 100., 0.),
    (120., 100., 25., 0., 50., 0.),
    (120., 100., 50., 0., 100., 0.),
    (135., 100., 50., 0., 100., 25.),
    (150., 100., 50., 0., 100., 50.),
    (165., 100., 50., 0., 100., 75.),
    (180., 100., 50., 0., 100., 100.),
    (195., 100., 50., 0., 75., 100.),
    (210., 100., 50., 0., 50., 100.),
    (225., 100., 50., 0., 25., 100.),
    (240., 100., 50., 0., 0., 100.),
    (255., 100., 50., 25., 0., 100.),
    (270., 100., 50., 50., 0., 100.),
    (285., 100., 50., 75., 0., 100.),
    (300., 100., 50., 100., 0., 100.),
    (315., 100., 50., 100., 0., 75.),
    (330., 100., 50., 100., 0., 50.),
    (345., 100., 50., 100., 0., 25.),
];

const HSV_ROWS: &[(f64, f64, f64, f64, f64, f64)] = &[
    (0., 0., 0., 0., 0., 0.),
    (0., 0., 100., 100., 100., 100.),
    (0., 100., 100., 100., 0., 0.),
    (15., 100., 100., 100., 25., 0.),
    (20., 75., 36., 36., 18., 9.),
    (30., 100., 100., 100., 50., 0.),
    (45., 100., 100., 100., 75., 0.),
    (60., 100., 100., 100., 100., 0.),
    (75., 100., 100., 75., 100., 0.),
    (90., 100., 100., 50., 100., 0.),
    (105., 100., 100., 25., 100., 0.),
    (120., 100., 50., 0., 50., 0.),
    (120., 100., 100., 0., 100., 0.),
    (135., 100., 100., 0., 100., 25.),
    (150., 100., 100., 0., 100., 50.),
    (165., 100., 100., 0., 100., 75.),
    (180., 100., 100., 0., 100., 100.),
    (195., 100., 100., 0., 75., 100.),
    (210., 100., 100., 0., 50., 100.),
    (225., 100., 100., 0., 25., 100.),
    (240., 100., 100., 0., 0., 100.),
    (255., 100., 100., 25., 0., 100.),
    (270., 100., 100., 50., 0., 100.),
    (285., 100., 100., 75., 0., 100.),
    (300., 100., 100., 100., 0., 100.),
    (315., 100., 100., 100., 0., 75.),
    (330., 100., 100., 100., 0., 50.),
    (345., 100., 100., 100., 0., 25.),
];

fn pct_to_byte(p: f64) -> u8 {
    (255.0 / 100.0 * p).round_ties_even() as u8
}

#[test]
fn rgb_to_hsl_powertoys_rows() {
    for &(h, s, l, r, g, b) in HSL_ROWS {
        let c = Color::new(pct_to_byte(r), pct_to_byte(g), pct_to_byte(b));
        let (rh, rs, rl) = c.hsl_f64();
        assert!((rh - h).abs() <= 0.2, "{c} hue {rh} vs {h}");
        assert!((rs * 100.0 - s).abs() <= 0.2, "{c} sat {rs} vs {s}");
        assert!((rl * 100.0 - l).abs() <= 0.2, "{c} light {rl} vs {l}");
    }
}

#[test]
fn rgb_to_hsv_hsb_powertoys_rows() {
    for &(h, s, v, r, g, b) in HSV_ROWS {
        let c = Color::new(pct_to_byte(r), pct_to_byte(g), pct_to_byte(b));
        let (rh, rs, rv) = c.hsv_f64();
        assert!((rh - h).abs() <= 0.2, "{c} hue {rh} vs {h}");
        assert!((rs * 100.0 - s).abs() <= 0.2, "{c} sat {rs} vs {s}");
        assert!((rv * 100.0 - v).abs() <= 0.2, "{c} value {rv} vs {v}");
    }
}

#[test]
fn rgb_to_cmyk_powertoys_rows() {
    // (c, m, y, k, r, g, b)
    let rows: &[(u8, u8, u8, u8, u8, u8, u8)] = &[
        (0, 0, 0, 100, 0, 0, 0),
        (0, 0, 0, 0, 255, 255, 255),
        (0, 100, 100, 0, 255, 0, 0),
        (0, 75, 100, 0, 255, 64, 0),
        (0, 50, 75, 64, 92, 46, 23),
        (0, 50, 100, 0, 255, 128, 0),
        (0, 25, 100, 0, 255, 192, 0),
        (0, 0, 100, 0, 255, 255, 0),
        (25, 0, 100, 0, 192, 255, 0),
        (50, 0, 100, 0, 128, 255, 0),
        (75, 0, 100, 0, 64, 255, 0),
        (100, 0, 100, 50, 0, 128, 0),
        (100, 0, 100, 0, 0, 255, 0),
        (100, 0, 75, 0, 0, 255, 64),
        (100, 0, 50, 0, 0, 255, 128),
        (100, 0, 25, 0, 0, 255, 192),
        (100, 0, 0, 0, 0, 255, 255),
        (100, 25, 0, 0, 0, 192, 255),
        (100, 50, 0, 0, 0, 128, 255),
        (100, 75, 0, 0, 0, 64, 255),
        (100, 100, 0, 0, 0, 0, 255),
        (75, 100, 0, 0, 64, 0, 255),
        (50, 100, 0, 0, 128, 0, 255),
        (25, 100, 0, 0, 192, 0, 255),
        (0, 100, 0, 0, 255, 0, 255),
        (0, 100, 25, 0, 255, 0, 192),
        (0, 100, 50, 0, 255, 0, 128),
        (0, 100, 75, 0, 255, 0, 64),
    ];
    for &(c, m, y, k, r, g, b) in rows {
        let col = Color::new(r, g, b);
        let (rc, rm, ry, rk) = col.to_cmyk();
        for (got, exp, n) in [(rc, c, "c"), (rm, m, "m"), (ry, y, "y"), (rk, k, "k")] {
            // PowerToys tolerance 0.5 on [0..100]; Rust already rounds to integer.
            assert!(
                (got as i32 - exp as i32).abs() <= 1,
                "{col} {n}: {got} vs {exp}"
            );
        }
    }
}

const HSI_ROWS: &[(&str, f64, f64, f64)] = &[
    ("FFFFFF", 0.0, 0.0, 100.0),
    ("808080", 0.0, 0.0, 50.0),
    ("000000", 0.0, 0.0, 0.0),
    ("FF0000", 0.0, 100.0, 33.3),
    ("BFBF00", 60.0, 100.0, 50.0),
    ("008000", 120.0, 100.0, 16.7),
    ("80FFFF", 180.0, 40.0, 83.3),
    ("8080FF", 240.0, 25.0, 66.7),
    ("BF40BF", 300.0, 57.1, 58.3),
    ("A0A424", 61.8, 69.9, 47.1),
    ("411BEA", 251.1, 75.6, 42.6),
    ("1EAC41", 134.9, 66.7, 34.9),
    ("F0C80E", 49.5, 91.1, 59.3),
    ("B430E5", 283.7, 68.6, 59.6),
    ("ED7651", 14.3, 44.6, 57.0),
    ("FEF888", 56.9, 36.3, 83.5),
    ("19CB97", 162.4, 80.0, 49.5),
    ("362698", 248.3, 53.3, 31.9),
    ("7E7EB8", 240.5, 13.5, 57.0),
];

#[test]
fn rgb_to_hsi_powertoys_rows() {
    for &(h, hue, s, i) in HSI_ROWS {
        let c = hex(h);
        let (rh, rs, ri) = c.hsi_f64();
        assert!((rh - hue).abs() <= 0.5, "{h} hue {rh} vs {hue}");
        assert!((rs * 100.0 - s).abs() <= 0.5, "{h} sat {rs} vs {s}");
        assert!((ri * 100.0 - i).abs() <= 0.5, "{h} int {ri} vs {i}");
    }
}

const HWB_NCOL_ROWS: &[(&str, f64, &str, f64, f64)] = &[
    ("FFFFFF", 0., "R0", 100., 0.),
    ("808080", 0., "R0", 50., 50.),
    ("000000", 0., "R0", 0., 100.),
    ("FF0000", 0., "R0", 0., 0.),
    ("BFBF00", 60., "Y0", 0., 25.),
    ("008000", 120., "G0", 0., 50.),
    ("80FFFF", 180., "C0", 50., 0.),
    ("8080FF", 240., "B0", 50., 0.),
    ("BF40BF", 300., "M0", 25., 25.),
    ("A0A424", 62., "Y3", 14., 36.),
    ("411BEA", 251., "B18", 11., 8.),
    ("1EAC41", 135., "G25", 12., 33.),
    ("F0C80E", 49., "R82", 5., 6.),
    ("B430E5", 284., "B73", 19., 10.),
    ("ED7651", 14., "R24", 32., 7.),
    ("FEF888", 57., "R95", 53., 0.),
    ("19CB97", 162., "G71", 10., 20.),
    ("362698", 248., "B14", 15., 40.),
    ("7E7EB8", 240., "B0", 49., 28.),
];

#[test]
fn rgb_to_hwb_powertoys_rows() {
    for &(h, hue, _, w, b) in HWB_NCOL_ROWS {
        let c = hex(h);
        let (rh, rw, rb) = c.hwb_f64();
        assert!((rh - hue).abs() <= 0.5, "{h} hue {rh} vs {hue}");
        assert!((rw * 100.0 - w).abs() <= 0.5, "{h} white {rw} vs {w}");
        assert!((rb * 100.0 - b).abs() <= 0.5, "{h} black {rb} vs {b}");
    }
}

/// NCol formatted string: `%Hn, %Wh%, %Bn%`, built from the PowerToys rows
/// (whiteness/blackness rounded to integer percent as GetStringRepresentation does).
#[test]
fn ncol_strings_powertoys_rows() {
    let mut failures = Vec::new();
    for &(h, _, hn, _, _) in HWB_NCOL_ROWS {
        let c = hex(h);
        let (_, w, b) = c.hwb_f64();
        let expected = format!(
            "{hn}, {}%, {}%",
            net_round(w * 100.0, 0),
            net_round(b * 100.0, 0)
        );
        let got = c.format(ColorFormat::Ncol);
        if got != expected {
            failures.push(format!("{h}: expected {expected}, got {got}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const LAB_ROWS: &[(&str, f64, f64, f64)] = &[
    ("FFFFFF", 100.00, 0.00, 0.00),
    ("808080", 53.59, 0.00, 0.00),
    ("000000", 0.00, 0.00, 0.00),
    ("FF0000", 53.24, 80.09, 67.20),
    ("008000", 46.23, -51.70, 49.90),
    ("80FFFF", 93.16, -35.23, -10.87),
    ("8080FF", 59.20, 33.10, -63.46),
    ("BF40BF", 50.10, 65.50, -41.48),
    ("BFBF00", 75.04, -17.35, 76.03),
    ("0048BA", 34.35, 27.94, -64.80),
    ("B0BF1A", 73.91, -23.39, 71.15),
    ("D0FF14", 93.87, -40.20, 88.97),
    ("1B4D3E", 29.13, -20.96, 3.95),
    ("FFEF00", 93.01, -13.86, 91.48),
    ("FFA600", 75.16, 23.41, 79.10),
    ("1A2421", 13.18, -5.23, 0.56),
    ("003399", 25.76, 28.89, -59.09),
    ("D70A53", 46.03, 71.90, 18.03),
    ("80FFD5", 92.09, -45.08, 9.29),
    ("EFDFBB", 89.26, -0.13, 19.65),
    ("5218FA", 36.65, 75.63, -97.70),
    ("FF496C", 59.08, 69.89, 21.80),
    ("545AA7", 41.20, 19.32, -42.34),
    ("E6A8D7", 75.91, 30.13, -14.79),
    ("ADDFAD", 84.32, -25.67, 19.37),
    ("E3F988", 94.25, -23.70, 51.58),
];

#[test]
fn rgb_to_cielab_powertoys_rows() {
    for &(h, l, a, b) in LAB_ROWS {
        let (rl, ra, rb) = hex(h).to_cielab();
        assert_eq!(net_round(rl, 2), l, "{h} L");
        assert_eq!(net_round(ra, 2), a, "{h} a");
        assert_eq!(net_round(rb, 2), b, "{h} b");
    }
}

/// CIELAB string `CIELab(%Lc, %Ca, %Cb)` uses 2 decimals (ColorPercentFormatted default).
#[test]
fn cielab_strings_powertoys_rows() {
    let mut failures = Vec::new();
    for &(h, l, a, b) in LAB_ROWS {
        let expected = format!("CIELab({}, {}, {})", l, a, b)
            .replace("-0,", "0,")
            .replace("-0)", "0)");
        let got = hex(h).format(ColorFormat::Cielab);
        if got != expected {
            failures.push(format!("{h}: expected {expected}, got {got}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const XYZ_ROWS: &[(&str, f64, f64, f64)] = &[
    ("FFFFFF", 95.0456, 100.0000, 108.9058),
    ("808080", 20.5166, 21.5861, 23.5085),
    ("000000", 0.0000, 0.0000, 0.0000),
    ("FF0000", 41.2391, 21.2639, 1.9331),
    ("008000", 7.7188, 15.4377, 2.5729),
    ("80FFFF", 62.7084, 83.3261, 107.3900),
    ("8080FF", 34.6688, 27.2469, 98.0434),
    ("BF40BF", 32.7217, 18.5062, 51.1405),
    ("BFBF00", 40.1154, 48.3384, 7.2171),
    ("0048BA", 11.1792, 8.1793, 47.4455),
    ("B0BF1A", 36.7205, 46.5663, 8.0311),
    ("D0FF14", 61.8965, 84.9797, 13.8037),
    ("1B4D3E", 3.9752, 5.8883, 5.4847),
    ("FFEF00", 72.1042, 82.9942, 12.2215),
    ("FFA600", 54.8747, 48.5351, 6.4783),
    ("1A2421", 1.3313, 1.5911, 1.6759),
    ("003399", 6.9329, 4.6672, 30.6735),
    ("D70A53", 29.6934, 15.2913, 9.5719),
    ("80FFD5", 56.6693, 80.9105, 75.5840),
    ("EFDFBB", 70.9510, 74.7146, 57.6991),
    ("5218FA", 21.0597, 9.3488, 91.1403),
    ("FF496C", 46.3280, 27.1114, 16.9814),
    ("545AA7", 14.2864, 11.9869, 38.1214),
    ("E6A8D7", 58.8989, 49.7359, 70.7897),
    ("ADDFAD", 51.1617, 64.6757, 49.3246),
    ("E3F988", 69.9955, 85.8597, 36.1785),
];

#[test]
fn rgb_to_ciexyz_powertoys_rows() {
    for &(h, x, y, z) in XYZ_ROWS {
        let (rx, ry, rz) = hex(h).to_ciexyz();
        assert_eq!(net_round(rx * 100.0, 4), x, "{h} X");
        assert_eq!(net_round(ry * 100.0, 4), y, "{h} Y");
        assert_eq!(net_round(rz * 100.0, 4), z, "{h} Z");
    }
}

/// `XYZ(%Xv, %Yv, %Zv)` with 4 decimals.
#[test]
fn ciexyz_strings_powertoys_rows() {
    let mut failures = Vec::new();
    for &(h, x, y, z) in XYZ_ROWS {
        let expected = format!("XYZ({x}, {y}, {z})");
        let got = hex(h).format(ColorFormat::Ciexyz);
        if got != expected {
            failures.push(format!("{h}: expected {expected}, got {got}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

const OKLAB_ROWS: &[(&str, f64, f64, f64)] = &[
    ("FFFFFF", 1.00, 0.00, 0.00),
    ("808080", 0.6, 0.00, 0.00),
    ("000000", 0.00, 0.00, 0.00),
    ("FF0000", 0.628, 0.22, 0.13),
    ("008000", 0.52, -0.14, 0.11),
    ("80FFFF", 0.928, -0.11, -0.03),
    ("8080FF", 0.661, 0.03, -0.18),
    ("BF40BF", 0.598, 0.18, -0.11),
    ("BFBF00", 0.779, -0.06, 0.16),
    ("0048BA", 0.444, -0.03, -0.19),
    ("B0BF1A", 0.767, -0.07, 0.15),
    ("D0FF14", 0.934, -0.12, 0.19),
    ("1B4D3E", 0.382, -0.06, 0.01),
    ("FFEF00", 0.935, -0.05, 0.19),
    ("FFA600", 0.794, 0.06, 0.16),
    ("1A2421", 0.25, -0.02, 0.0),
    ("003399", 0.371, -0.02, -0.17),
    ("D70A53", 0.563, 0.22, 0.04),
    ("80FFD5", 0.916, -0.13, 0.02),
    ("EFDFBB", 0.907, 0.0, 0.05),
    ("5218FA", 0.489, 0.05, -0.28),
    ("FF496C", 0.675, 0.21, 0.05),
    ("545AA7", 0.5, 0.02, -0.12),
    ("E6A8D7", 0.804, 0.09, -0.04),
    ("ADDFAD", 0.856, -0.07, 0.05),
    ("E3F988", 0.942, -0.07, 0.12),
];

#[test]
fn rgb_to_oklab_powertoys_rows() {
    for &(h, l, a, b) in OKLAB_ROWS {
        let (rl, ra, rb) = hex(h).to_oklab();
        assert_eq!(net_round(rl, 3), l, "{h} L");
        assert_eq!(net_round(ra, 2), a, "{h} a");
        assert_eq!(net_round(rb, 2), b, "{h} b");
    }
}

const OKLCH_ROWS: &[(&str, f64, f64, f64)] = &[
    ("FFFFFF", 1.00, 0.00, 0.00),
    ("808080", 0.6, 0.00, 0.00),
    ("000000", 0.00, 0.00, 0.00),
    ("FF0000", 0.628, 0.258, 29.23),
    ("008000", 0.52, 0.177, 142.5),
    ("80FFFF", 0.928, 0.113, 195.38),
    ("8080FF", 0.661, 0.184, 280.13),
    ("BF40BF", 0.598, 0.216, 327.86),
    ("BFBF00", 0.779, 0.17, 109.77),
    ("0048BA", 0.444, 0.19, 260.86),
    ("B0BF1A", 0.767, 0.169, 115.4),
    ("D0FF14", 0.934, 0.224, 122.28),
    ("1B4D3E", 0.382, 0.06, 170.28),
    ("FFEF00", 0.935, 0.198, 104.67),
    ("FFA600", 0.794, 0.171, 71.19),
    ("1A2421", 0.25, 0.015, 174.74),
    ("003399", 0.371, 0.173, 262.12),
    ("D70A53", 0.563, 0.222, 11.5),
    ("80FFD5", 0.916, 0.129, 169.38),
    ("EFDFBB", 0.907, 0.05, 86.89),
    ("5218FA", 0.489, 0.286, 279.13),
    ("FF496C", 0.675, 0.217, 14.37),
    ("545AA7", 0.5, 0.121, 277.7),
    ("E6A8D7", 0.804, 0.095, 335.4),
    ("ADDFAD", 0.856, 0.086, 144.78),
    ("E3F988", 0.942, 0.141, 118.24),
];

#[test]
fn rgb_to_oklch_powertoys_rows() {
    for &(h, l, c, hue) in OKLCH_ROWS {
        let (rl, rc, rh) = hex(h).to_oklch();
        assert_eq!(net_round(rl, 3), l, "{h} L");
        assert_eq!(net_round(rc, 3), c, "{h} C");
        assert_eq!(net_round(rh, 2), hue, "{h} h");
    }
}

/// Oklab/Oklch strings use 2 decimals for every component (`%Lo`, `%Oa`, ... -> Math.Round(v, 2)).
#[test]
fn oklab_oklch_strings_two_decimals() {
    for &(h, ..) in OKLCH_ROWS {
        let c = hex(h);
        let (l, a, b) = c.to_oklab();
        let (_, ch, hue) = c.to_oklch();
        assert_eq!(
            c.format(ColorFormat::Oklab),
            format!(
                "oklab({}, {}, {})",
                net_round(l, 2),
                net_round(a, 2),
                net_round(b, 2)
            ),
            "{h}"
        );
        assert_eq!(
            c.format(ColorFormat::Oklch),
            format!(
                "oklch({}, {}, {})",
                net_round(l, 2),
                net_round(ch, 2),
                net_round(hue, 2)
            ),
            "{h}"
        );
    }
}

/// PowerToys ColorRGBtoCMYKZeroDivTest: no panic for any color; also every format is total.
#[test]
fn all_formats_total_over_sampled_colors() {
    for r in (0..=255u16).step_by(15) {
        for g in (0..=255u16).step_by(15) {
            for b in (0..=255u16).step_by(15) {
                let c = Color::new(r as u8, g as u8, b as u8);
                for f in ColorFormat::ALL {
                    let s = c.format(f);
                    assert!(
                        !s.contains("NaN") && !s.contains("inf") && !s.is_empty(),
                        "{c} {f:?}: {s}"
                    );
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Settings defaults (ColorPickerProperties constructor)
// ---------------------------------------------------------------------------

/// PowerToys: HEX, RGB and HSL visible; HSV/CMYK/... hidden.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn default_visible_formats_are_hex_rgb_hsl() {
    let cfg = ColorPickerConfig::default();
    assert_eq!(
        cfg.enabled_formats(),
        vec![ColorFormat::Hex, ColorFormat::Rgb, ColorFormat::Hsl]
    );
}

/// PowerToys VisibleColorFormats insertion order.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn default_format_order_matches_powertoys() {
    let cfg = ColorPickerConfig::default();
    let got: Vec<&str> = cfg.format_list().iter().map(|(f, _)| f.label()).collect();
    assert_eq!(
        got,
        vec![
            "HEX", "RGB", "HSL", "HSV", "CMYK", "HSB", "HSI", "HWB", "NCol", "CIEXYZ", "CIELAB",
            "Oklab", "Oklch", "VEC4", "Decimal", "HEX Int"
        ]
    );
}

/// PowerToys ShowColorName = false.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn default_show_color_name_is_false() {
    assert!(!ColorPickerConfig::default().show_color_name);
    let cfg: ColorPickerConfig = toml::from_str("").unwrap();
    assert!(!cfg.show_color_name);
}

#[test]
fn default_click_actions_history_and_copied_format() {
    let cfg = ColorPickerConfig::default();
    assert_eq!(cfg.format, ColorFormat::Hex); // CopiedColorRepresentation = "HEX"
    assert_eq!(cfg.primary_click(), ClickAction::PickColorThenEditor);
    assert_eq!(cfg.middle_click_action, ClickAction::PickColorAndClose);
    assert_eq!(cfg.secondary_click_action, ClickAction::Close);
    assert_eq!(cfg.color_history_limit, 20);
    assert!(!cfg.change_cursor);
}

/// PowerToys lets users add custom formats (name + format string, e.g.
/// `"My" -> "%Re-%Gr-%Bl"`) and shows them in the editor. A custom entry
/// in the Rust config is silently dropped.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn custom_format_entry_is_shown_in_editor() {
    let cfg = ColorPickerConfig {
        formats: vec![
            ColorFormatEntry {
                id: "hex".into(),
                label: "HEX".into(),
                enabled: true,
            },
            ColorFormatEntry {
                id: "My format".into(),
                label: "My format".into(),
                enabled: true,
            },
        ],
        ..Default::default()
    };
    // Expect two visible formats (HEX + the custom one).
    assert_eq!(cfg.enabled_formats().len(), 2, "custom format was dropped");
}

// ---------------------------------------------------------------------------
// History (MainViewModel.UpdateColorHistory)
// ---------------------------------------------------------------------------

#[test]
fn history_move_to_front_and_limit() {
    let mut h = ColorHistory::default();
    for i in 0..25u8 {
        h.push_with_limit(Color::new(i, 0, 0), 20);
    }
    assert_eq!(h.entries.len(), 20);
    assert_eq!(h.entries[0].color.r, 24);
    assert_eq!(h.entries[19].color.r, 5);
    h.push_with_limit(Color::new(10, 0, 0), 20);
    assert_eq!(h.entries.len(), 20);
    assert_eq!(h.entries[0].color.r, 10);
    assert_eq!(h.entries[1].color.r, 24);
}

/// ColorHistoryLimit is not exposed in the PowerToys settings page; Rust clamps it to >= 1.
#[test]
fn history_limit_lower_bound_is_one() {
    let cfg = ColorPickerConfig {
        color_history_limit: 0,
        ..Default::default()
    };
    assert_eq!(cfg.history_limit(), 1);
}

// ---------------------------------------------------------------------------
// Color names (ColorNameHelper.GetColorNameIdentifier, en-us resources)
// ---------------------------------------------------------------------------

/// Achromatic buckets: lum > 240 -> White, lum < 20 -> Black, sat <= 20 ->
/// Light gray (lum > 170) / Gray (lum > 100) / Dark gray.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn color_names_achromatic_buckets() {
    use crate::names::nearest_color_name as name;
    let mut failures = Vec::new();
    for (c, exp) in [
        (Color::new(250, 250, 240), "White"),
        (Color::new(15, 10, 25), "Black"),
        (Color::new(200, 200, 200), "Light gray"),
        (Color::new(128, 128, 128), "Gray"),
        (Color::new(80, 80, 80), "Dark gray"),
    ] {
        let got = name(c);
        if got != exp {
            failures.push(format!("{c}: expected {exp}, got {got}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
