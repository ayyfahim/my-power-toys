use crate::config::UnitsOfMeasure;
use serde::{Deserialize, Serialize};

/// Fallback DPI when the monitor's physical size is unknown (upstream `DefaultDpi`).
pub const DEFAULT_DPI: f64 = 96.0;

/// Convert a pixel length to `units` (upstream `MeasurementLogic::Convert`).
///
/// `px2mm` is the physical size of one pixel in millimetres (monitor width in
/// mm / width in px). When unknown (`None` or non-positive), 96 DPI is assumed.
pub fn convert_pixels(pixels: f64, units: UnitsOfMeasure, px2mm: Option<f64>) -> f64 {
    if units == UnitsOfMeasure::Pixels {
        return pixels;
    }
    let mm = match px2mm {
        Some(r) if r > 0.0 => pixels * r,
        _ => pixels / DEFAULT_DPI * 25.4,
    };
    match units {
        UnitsOfMeasure::Pixels => pixels,
        UnitsOfMeasure::Inches => mm / 25.4,
        UnitsOfMeasure::Centimeters => mm / 10.0,
        UnitsOfMeasure::Millimeters => mm,
    }
}

/// Format with `sig` significant digits, no exponent, trailing zeros removed
/// (close to C's `%.4g` for the value ranges a screen produces).
pub fn format_significant(v: f64, sig: u32) -> String {
    if v == 0.0 || !v.is_finite() {
        return "0".into();
    }
    let int_digits = v.abs().log10().floor() as i32 + 1;
    let decimals = (sig as i32 - int_digits).max(0) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}

fn format_len(pixels: u32, units: UnitsOfMeasure, px2mm: Option<f64>) -> String {
    if units == UnitsOfMeasure::Pixels {
        pixels.to_string()
    } else {
        format_significant(convert_pixels(pixels as f64, units, px2mm), 4)
    }
}

fn format_dims(
    width: u32,
    height: u32,
    print_width: bool,
    print_height: bool,
    units: UnitsOfMeasure,
    px2mm: Option<f64>,
) -> String {
    let mut s = String::new();
    if print_width {
        s.push_str(&format_len(width, units, px2mm));
        if print_height {
            s.push_str(" \u{00D7} ");
        }
    }
    if print_height {
        s.push_str(&format_len(height, units, px2mm));
    }
    s
}

/// On-screen label text (upstream `Measurement::Print`): always pixels, plus
/// the configured unit in parentheses when it is not pixels.
pub fn label_text(
    width: u32,
    height: u32,
    print_width: bool,
    print_height: bool,
    units: UnitsOfMeasure,
    px2mm: Option<f64>,
) -> String {
    let mut s = format_dims(
        width,
        height,
        print_width,
        print_height,
        UnitsOfMeasure::Pixels,
        None,
    );
    s.push_str(" px");
    if units != UnitsOfMeasure::Pixels {
        s.push_str(if print_width && print_height {
            "\n("
        } else {
            " ("
        });
        s.push_str(&format_dims(
            width,
            height,
            print_width,
            print_height,
            units,
            px2mm,
        ));
        s.push(' ');
        s.push_str(units.abbreviation());
        s.push(')');
    }
    s
}

/// Clipboard text (upstream `SetClipboardToMeasurements` / `PrintToStream`):
/// one line per measurement, unit abbreviation only for non-pixel units.
pub fn clipboard_text(
    dims: &[(u32, u32)],
    print_width: bool,
    print_height: bool,
    units: UnitsOfMeasure,
    px2mm: Option<f64>,
) -> String {
    dims.iter()
        .map(|&(w, h)| {
            let mut s = format_dims(w, h, print_width, print_height, units, px2mm);
            if units != UnitsOfMeasure::Pixels {
                s.push(' ');
                s.push_str(units.abbreviation());
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A measurement between two screen points.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Measurement {
    pub start: Point,
    pub end: Point,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Measurement {
    pub fn new(start: Point, end: Point) -> Self {
        Self { start, end }
    }

    /// Horizontal distance in pixels.
    pub fn width(&self) -> i32 {
        (self.end.x - self.start.x).abs()
    }

    /// Vertical distance in pixels.
    pub fn height(&self) -> i32 {
        (self.end.y - self.start.y).abs()
    }

    /// Diagonal distance in pixels.
    pub fn diagonal(&self) -> f64 {
        let dx = (self.end.x - self.start.x) as f64;
        let dy = (self.end.y - self.start.y) as f64;
        (dx * dx + dy * dy).sqrt()
    }

    /// Format as human-readable string.
    pub fn display_string(&self) -> String {
        format!(
            "{}x{} px (diagonal: {:.1} px)",
            self.width(),
            self.height(),
            self.diagonal()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_measurement() {
        let m = Measurement::new(Point { x: 0, y: 0 }, Point { x: 100, y: 0 });
        assert_eq!(m.width(), 100);
        assert_eq!(m.height(), 0);
        assert!((m.diagonal() - 100.0).abs() < 0.01);
    }

    #[test]
    fn diagonal_measurement() {
        let m = Measurement::new(Point { x: 0, y: 0 }, Point { x: 3, y: 4 });
        assert_eq!(m.width(), 3);
        assert_eq!(m.height(), 4);
        assert!((m.diagonal() - 5.0).abs() < 0.01);
    }

    #[test]
    fn unit_conversion_default_dpi() {
        assert_eq!(convert_pixels(123.0, UnitsOfMeasure::Pixels, None), 123.0);
        assert!((convert_pixels(96.0, UnitsOfMeasure::Inches, None) - 1.0).abs() < 1e-9);
        assert!((convert_pixels(96.0, UnitsOfMeasure::Centimeters, None) - 2.54).abs() < 1e-9);
        assert!((convert_pixels(96.0, UnitsOfMeasure::Millimeters, None) - 25.4).abs() < 1e-9);
        // Non-positive ratio falls back to 96 DPI.
        assert!((convert_pixels(96.0, UnitsOfMeasure::Inches, Some(0.0)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn unit_conversion_physical_size() {
        // 0.25 mm per pixel: 100 px = 25 mm = 2.5 cm.
        let r = Some(0.25);
        assert!((convert_pixels(100.0, UnitsOfMeasure::Millimeters, r) - 25.0).abs() < 1e-9);
        assert!((convert_pixels(100.0, UnitsOfMeasure::Centimeters, r) - 2.5).abs() < 1e-9);
        assert!((convert_pixels(254.0, UnitsOfMeasure::Inches, r) - 2.5).abs() < 1e-9);
    }

    #[test]
    fn significant_formatting() {
        assert_eq!(format_significant(1.0, 4), "1");
        assert_eq!(format_significant(2.54, 4), "2.54");
        assert_eq!(format_significant(26.458333, 4), "26.46");
        assert_eq!(format_significant(0.010416, 4), "0.01042");
        assert_eq!(format_significant(1234.6, 4), "1235");
        assert_eq!(format_significant(0.0, 4), "0");
    }

    #[test]
    fn label_and_clipboard_text() {
        assert_eq!(
            label_text(100, 50, true, true, UnitsOfMeasure::Pixels, None),
            "100 \u{00D7} 50 px"
        );
        assert_eq!(
            label_text(96, 0, true, false, UnitsOfMeasure::Inches, None),
            "96 px (1 in)"
        );
        assert_eq!(
            label_text(96, 192, true, true, UnitsOfMeasure::Centimeters, None),
            "96 \u{00D7} 192 px\n(2.54 \u{00D7} 5.08 cm)"
        );
        assert_eq!(
            clipboard_text(
                &[(10, 20), (5, 6)],
                true,
                true,
                UnitsOfMeasure::Pixels,
                None
            ),
            "10 \u{00D7} 20\n5 \u{00D7} 6"
        );
        assert_eq!(
            clipboard_text(&[(96, 1)], false, true, UnitsOfMeasure::Inches, None),
            "0.01042 in"
        );
        assert_eq!(
            clipboard_text(&[], true, true, UnitsOfMeasure::Pixels, None),
            ""
        );
    }

    #[test]
    fn display_format() {
        let m = Measurement::new(Point { x: 10, y: 20 }, Point { x: 110, y: 80 });
        let s = m.display_string();
        assert!(s.contains("100x60 px"));
    }
}
