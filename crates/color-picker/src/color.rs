use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Color representations, matching upstream `ColorFormatHelper.GetDefaultFormat`.
///
/// Serialized ids are lowercase (`hex`, `rgb`, ..., `hexint`); the upstream
/// display names (`HEX Int`, `CIELAB`, `NCol`, ...) are also accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorFormat {
    #[serde(alias = "HEX")]
    Hex,
    #[serde(alias = "RGB")]
    Rgb,
    #[serde(alias = "HSL")]
    Hsl,
    #[serde(alias = "HSV")]
    Hsv,
    #[serde(alias = "CMYK")]
    Cmyk,
    #[serde(alias = "HSB")]
    Hsb,
    #[serde(alias = "HSI")]
    Hsi,
    #[serde(alias = "HWB")]
    Hwb,
    #[serde(alias = "NCol")]
    Ncol,
    #[serde(alias = "CIELAB")]
    Cielab,
    #[serde(alias = "CIEXYZ")]
    Ciexyz,
    #[serde(alias = "Oklab")]
    Oklab,
    #[serde(alias = "Oklch")]
    Oklch,
    #[serde(alias = "VEC4")]
    Vec4,
    #[serde(alias = "Decimal")]
    Decimal,
    #[serde(alias = "HEX Int", alias = "hex-int", alias = "hex_int")]
    Hexint,
}

impl ColorFormat {
    /// All formats in upstream order.
    pub const ALL: [ColorFormat; 16] = [
        ColorFormat::Hex,
        ColorFormat::Rgb,
        ColorFormat::Hsl,
        ColorFormat::Hsv,
        ColorFormat::Cmyk,
        ColorFormat::Hsb,
        ColorFormat::Hsi,
        ColorFormat::Hwb,
        ColorFormat::Ncol,
        ColorFormat::Cielab,
        ColorFormat::Ciexyz,
        ColorFormat::Oklab,
        ColorFormat::Oklch,
        ColorFormat::Vec4,
        ColorFormat::Decimal,
        ColorFormat::Hexint,
    ];

    /// Stable lowercase id (used in config files).
    pub fn id(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::Rgb => "rgb",
            Self::Hsl => "hsl",
            Self::Hsv => "hsv",
            Self::Cmyk => "cmyk",
            Self::Hsb => "hsb",
            Self::Hsi => "hsi",
            Self::Hwb => "hwb",
            Self::Ncol => "ncol",
            Self::Cielab => "cielab",
            Self::Ciexyz => "ciexyz",
            Self::Oklab => "oklab",
            Self::Oklch => "oklch",
            Self::Vec4 => "vec4",
            Self::Decimal => "decimal",
            Self::Hexint => "hexint",
        }
    }

    /// Upstream display name.
    pub fn label(self) -> &'static str {
        match self {
            Self::Hex => "HEX",
            Self::Rgb => "RGB",
            Self::Hsl => "HSL",
            Self::Hsv => "HSV",
            Self::Cmyk => "CMYK",
            Self::Hsb => "HSB",
            Self::Hsi => "HSI",
            Self::Hwb => "HWB",
            Self::Ncol => "NCol",
            Self::Cielab => "CIELAB",
            Self::Ciexyz => "CIEXYZ",
            Self::Oklab => "Oklab",
            Self::Oklch => "Oklch",
            Self::Vec4 => "VEC4",
            Self::Decimal => "Decimal",
            Self::Hexint => "HEX Int",
        }
    }

    /// Parse an id or upstream display name (case-insensitive).
    pub fn from_id(s: &str) -> Option<Self> {
        let norm: String = s
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        Self::ALL.into_iter().find(|f| f.id() == norm)
    }
}

impl Color {
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parse a hex color string like "#FF8000" or "FF8000".
    pub fn from_hex(hex: &str) -> Result<Self> {
        let hex = hex.trim().trim_start_matches('#');
        if hex.len() != 6 {
            anyhow::bail!("invalid hex color: #{hex}");
        }
        let r = u8::from_str_radix(&hex[0..2], 16).context("invalid red component")?;
        let g = u8::from_str_radix(&hex[2..4], 16).context("invalid green component")?;
        let b = u8::from_str_radix(&hex[4..6], 16).context("invalid blue component")?;
        Ok(Self::new(r, g, b))
    }

    pub fn format(self, fmt: ColorFormat) -> String {
        match fmt {
            ColorFormat::Hex => format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b),
            ColorFormat::Rgb => format!("rgb({}, {}, {})", self.r, self.g, self.b),
            ColorFormat::Hsl => {
                let (h, s, l) = self.hsl_f64();
                format!("hsl({}, {}%, {}%)", round0(h), pct(s), pct(l))
            }
            ColorFormat::Hsv => {
                let (h, s, v) = self.hsv_f64();
                format!("hsv({}, {}%, {}%)", round0(h), pct(s), pct(v))
            }
            ColorFormat::Cmyk => {
                let (c, m, y, k) = self.to_cmyk();
                format!("cmyk({c}%, {m}%, {y}%, {k}%)")
            }
            ColorFormat::Hsb => {
                let (h, s, b) = self.hsv_f64();
                format!("hsb({}, {}%, {}%)", round0(h), pct(s), pct(b))
            }
            ColorFormat::Hsi => {
                let (h, s, i) = self.hsi_f64();
                format!("hsi({}, {}%, {}%)", round0(h), pct(s), pct(i))
            }
            ColorFormat::Hwb => {
                let (h, w, b) = self.hwb_f64();
                format!("hwb({}, {}%, {}%)", round0(h), pct(w), pct(b))
            }
            ColorFormat::Ncol => {
                let (_, w, b) = self.hwb_f64();
                format!("{}, {}%, {}%", natural_hue(self.hue()), pct(w), pct(b))
            }
            ColorFormat::Cielab => {
                let (l, a, b) = self.to_cielab();
                format!(
                    "CIELab({}, {}, {})",
                    round_n(l, 2),
                    round_n(a, 2),
                    round_n(b, 2)
                )
            }
            ColorFormat::Ciexyz => {
                let (x, y, z) = self.to_ciexyz();
                format!(
                    "XYZ({}, {}, {})",
                    round_n(x * 100.0, 4),
                    round_n(y * 100.0, 4),
                    round_n(z * 100.0, 4)
                )
            }
            ColorFormat::Oklab => {
                let (l, a, b) = self.to_oklab();
                format!(
                    "oklab({}, {}, {})",
                    round_n(l, 2),
                    round_n(a, 2),
                    round_n(b, 2)
                )
            }
            ColorFormat::Oklch => {
                let (l, c, h) = self.to_oklch();
                format!(
                    "oklch({}, {}, {})",
                    round_n(l, 2),
                    round_n(c, 2),
                    round_n(h, 2)
                )
            }
            ColorFormat::Vec4 => format!(
                "({}f, {}f, {}f, 1f)",
                unit_float(self.r),
                unit_float(self.g),
                unit_float(self.b)
            ),
            ColorFormat::Decimal => {
                // Upstream "%Dv": BGR order (R + G*256 + B*65536).
                let v = self.r as u32 + (self.g as u32) * 256 + (self.b as u32) * 65536;
                v.to_string()
            }
            ColorFormat::Hexint => {
                format!("0xFF{:02X}{:02X}{:02X}", self.r, self.g, self.b)
            }
        }
    }

    /// Hue in degrees [0, 360), like .NET `Color.GetHue()`.
    pub fn hue(self) -> f64 {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;
        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;
        if d == 0.0 {
            return 0.0;
        }
        let h = if max == r {
            (g - b) / d
        } else if max == g {
            2.0 + (b - r) / d
        } else {
            4.0 + (r - g) / d
        };
        let h = h * 60.0;
        if h < 0.0 { h + 360.0 } else { h }
    }

    fn min_max(self) -> (f64, f64) {
        let min = self.r.min(self.g).min(self.b) as f64 / 255.0;
        let max = self.r.max(self.g).max(self.b) as f64 / 255.0;
        (min, max)
    }

    /// HSL with hue [0..360], saturation and lightness [0..1].
    pub fn hsl_f64(self) -> (f64, f64, f64) {
        let (min, max) = self.min_max();
        let l = (max + min) / 2.0;
        let s = if l == 0.0 || min == max {
            0.0
        } else if l <= 0.5 {
            (max - min) / (max + min)
        } else {
            (max - min) / (2.0 - (max + min))
        };
        (self.hue(), s, l)
    }

    /// HSV/HSB with hue [0..360], saturation and value [0..1].
    pub fn hsv_f64(self) -> (f64, f64, f64) {
        let (min, max) = self.min_max();
        let s = if max == 0.0 { 0.0 } else { (max - min) / max };
        (self.hue(), s, max)
    }

    /// HSI with hue [0..360], saturation and intensity [0..1].
    pub fn hsi_f64(self) -> (f64, f64, f64) {
        if self.r == 0 && self.g == 0 && self.b == 0 {
            return (0.0, 0.0, 0.0);
        }
        let i = (self.r as f64 + self.g as f64 + self.b as f64) / 255.0 / 3.0;
        let (min, _) = self.min_max();
        (self.hue(), 1.0 - min / i, i)
    }

    /// HWB with hue [0..360], whiteness and blackness [0..1].
    pub fn hwb_f64(self) -> (f64, f64, f64) {
        let (min, max) = self.min_max();
        (self.hue(), min, 1.0 - max)
    }

    /// Linear-light RGB components.
    pub fn to_linear_rgb(self) -> (f64, f64, f64) {
        (
            srgb_to_linear(self.r as f64 / 255.0),
            srgb_to_linear(self.g as f64 / 255.0),
            srgb_to_linear(self.b as f64 / 255.0),
        )
    }

    /// CIE XYZ (D65), each roughly [0..1].
    pub fn to_ciexyz(self) -> (f64, f64, f64) {
        let (r, g, b) = self.to_linear_rgb();
        (
            r * 0.412_390_799_265_959_5 + g * 0.357_584_339_383_878 + b * 0.180_480_788_401_834_3,
            r * 0.212_639_005_871_510_36 + g * 0.715_168_678_767_756 + b * 0.072_192_315_360_733_72,
            r * 0.019_330_818_715_591_85 + g * 0.119_194_779_794_626 + b * 0.950_532_152_249_660_6,
        )
    }

    /// CIE L*a*b* relative to the sRGB D65 white point.
    pub fn to_cielab(self) -> (f64, f64, f64) {
        let (x, y, z) = self.to_ciexyz();
        let (xn, yn, zn) = (0.950_455_927_051_671_7, 1.0, 1.089_057_750_759_878_4);
        let delta: f64 = 6.0 / 29.0;
        let m = (1.0 / 3.0) * delta.powi(-2);
        let t = delta.powi(3);
        let f = |v: f64| {
            if v > t {
                v.cbrt()
            } else {
                v * m + 16.0 / 116.0
            }
        };
        let (fx, fy, fz) = (f(x / xn), f(y / yn), f(z / zn));
        (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
    }

    /// Oklab (L [0..1], a/b roughly [-0.5..0.5]).
    pub fn to_oklab(self) -> (f64, f64, f64) {
        let (r, g, b) = self.to_linear_rgb();
        let l = 0.412_221_470_799_999_93 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
        let m = 0.211_903_498_199_999_9 * r + 0.680_699_545_099_999_9 * g + 0.107_396_956_6 * b;
        let s = 0.088_302_461_899_999_98 * r + 0.281_718_837_6 * g + 0.629_978_700_500_000_2 * b;
        let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
        (
            0.210_454_255_3 * l_ + 0.793_617_785 * m_ - 0.004_072_046_8 * s_,
            1.977_998_495_1 * l_ - 2.428_592_205 * m_ + 0.450_593_709_9 * s_,
            0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766 * s_,
        )
    }

    /// Oklch (L [0..1], chroma, hue [0..360)).
    pub fn to_oklch(self) -> (f64, f64, f64) {
        let (l, a, b) = self.to_oklab();
        let c = (a * a + b * b).sqrt();
        let h = if round_n(c, 3) == 0.0 {
            0.0
        } else {
            (b.atan2(a).to_degrees() + 360.0) % 360.0
        };
        (l, c, h)
    }

    pub fn to_hsl(self) -> (u16, u8, u8) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let l = (max + min) / 2.0;

        if (max - min).abs() < f64::EPSILON {
            return (0, 0, (l * 100.0) as u8);
        }

        let d = max - min;
        let s = if l > 0.5 {
            d / (2.0 - max - min)
        } else {
            d / (max + min)
        };

        let h = if (max - r).abs() < f64::EPSILON {
            let mut h = (g - b) / d;
            if g < b {
                h += 6.0;
            }
            h
        } else if (max - g).abs() < f64::EPSILON {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };

        ((h * 60.0) as u16, (s * 100.0) as u8, (l * 100.0) as u8)
    }

    pub fn to_hsv(self) -> (u16, u8, u8) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let d = max - min;

        let v = max;

        if max.abs() < f64::EPSILON {
            return (0, 0, (v * 100.0) as u8);
        }

        let s = d / max;

        if d.abs() < f64::EPSILON {
            return (0, 0, (v * 100.0) as u8);
        }

        let h = if (max - r).abs() < f64::EPSILON {
            let mut h = (g - b) / d;
            if g < b {
                h += 6.0;
            }
            h
        } else if (max - g).abs() < f64::EPSILON {
            (b - r) / d + 2.0
        } else {
            (r - g) / d + 4.0
        };

        ((h * 60.0) as u16, (s * 100.0) as u8, (v * 100.0) as u8)
    }

    pub fn to_cmyk(self) -> (u8, u8, u8, u8) {
        let r = self.r as f64 / 255.0;
        let g = self.g as f64 / 255.0;
        let b = self.b as f64 / 255.0;

        let k = 1.0 - r.max(g).max(b);
        if (1.0 - k).abs() < f64::EPSILON {
            return (0, 0, 0, 100);
        }
        let c = (1.0 - r - k) / (1.0 - k);
        let m = (1.0 - g - k) / (1.0 - k);
        let y = (1.0 - b - k) / (1.0 - k);

        (
            (c * 100.0).round() as u8,
            (m * 100.0).round() as u8,
            (y * 100.0).round() as u8,
            (k * 100.0).round() as u8,
        )
    }

    /// Generate `count` shade variations from light to dark.
    pub fn shades(self, count: usize) -> Vec<Color> {
        if count == 0 {
            return vec![];
        }
        if count == 1 {
            return vec![self];
        }

        let (h, s, _l) = self.to_hsl();
        let h_f = h as f64;
        let s_f = s as f64 / 100.0;

        (0..count)
            .map(|i| {
                let t = i as f64 / (count - 1) as f64;
                let l_f = 0.9 - t * 0.8; // 90% → 10%
                Self::from_hsl(h_f, s_f, l_f)
            })
            .collect()
    }

    pub fn from_hsl(h: f64, s: f64, l: f64) -> Self {
        if s.abs() < f64::EPSILON {
            let v = (l * 255.0).round() as u8;
            return Self::new(v, v, v);
        }

        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;

        let r = hue_to_rgb(p, q, h / 360.0 + 1.0 / 3.0);
        let g = hue_to_rgb(p, q, h / 360.0);
        let b = hue_to_rgb(p, q, h / 360.0 - 1.0 / 3.0);

        Self::new(
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        )
    }

    #[cfg(feature = "gui")]
    pub fn to_egui_color32(self) -> egui::Color32 {
        egui::Color32::from_rgb(self.r, self.g, self.b)
    }
}

fn srgb_to_linear(c: f64) -> f64 {
    if c > 0.04045 {
        ((c + 0.055) / 1.055).powf(2.4)
    } else {
        c / 12.92
    }
}

/// Round to `n` decimals with banker's rounding (like .NET `Math.Round`), normalising -0.
fn round_n(v: f64, n: i32) -> f64 {
    let f = 10f64.powi(n);
    let r = (v * f).round_ties_even() / f;
    if r == 0.0 { 0.0 } else { r }
}

fn round0(v: f64) -> f64 {
    round_n(v, 0)
}

/// Fraction [0..1] → rounded percentage.
fn pct(v: f64) -> f64 {
    round0(v * 100.0)
}

/// Byte → `0.##` float (upstream VEC4 `ff` formatter).
fn unit_float(b: u8) -> f64 {
    ((b as f64 / 255.0) * 100.0).round() / 100.0
}

/// Upstream `GetNaturalColorFromHue` (NCol hue notation).
fn natural_hue(hue: f64) -> String {
    let (letter, base) = match hue {
        h if h < 60.0 => ('R', 0.0),
        h if h < 120.0 => ('Y', 60.0),
        h if h < 180.0 => ('G', 120.0),
        h if h < 240.0 => ('C', 180.0),
        h if h < 300.0 => ('B', 240.0),
        _ => ('M', 300.0),
    };
    format!("{letter}{}", round0((hue - base) / 0.6))
}

fn hue_to_rgb(p: f64, q: f64, mut t: f64) -> f64 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_format() {
        let c = Color::new(255, 128, 0);
        assert_eq!(c.format(ColorFormat::Hex), "#FF8000");
    }

    #[test]
    fn rgb_format() {
        let c = Color::new(255, 128, 0);
        assert_eq!(c.format(ColorFormat::Rgb), "rgb(255, 128, 0)");
    }

    #[test]
    fn hsl_format() {
        let c = Color::new(255, 0, 0);
        assert_eq!(c.format(ColorFormat::Hsl), "hsl(0, 100%, 50%)");
    }

    #[test]
    fn hsv_format() {
        let c = Color::new(255, 0, 0);
        assert_eq!(c.format(ColorFormat::Hsv), "hsv(0, 100%, 100%)");
    }

    #[test]
    fn black_hsl() {
        let c = Color::new(0, 0, 0);
        assert_eq!(c.format(ColorFormat::Hsl), "hsl(0, 0%, 0%)");
    }

    #[test]
    fn white_hsl() {
        let c = Color::new(255, 255, 255);
        assert_eq!(c.format(ColorFormat::Hsl), "hsl(0, 0%, 100%)");
    }

    #[test]
    fn black_hsv() {
        let c = Color::new(0, 0, 0);
        assert_eq!(c.format(ColorFormat::Hsv), "hsv(0, 0%, 0%)");
    }

    #[test]
    fn white_hsv() {
        let c = Color::new(255, 255, 255);
        assert_eq!(c.format(ColorFormat::Hsv), "hsv(0, 0%, 100%)");
    }

    #[test]
    fn from_hex_valid() {
        let c = Color::from_hex("#FF8000").unwrap();
        assert_eq!(c.r, 255);
        assert_eq!(c.g, 128);
        assert_eq!(c.b, 0);
    }

    #[test]
    fn from_hex_no_hash() {
        let c = Color::from_hex("FF8000").unwrap();
        assert_eq!(c.r, 255);
    }

    #[test]
    fn from_hex_invalid() {
        assert!(Color::from_hex("GG0000").is_err());
        assert!(Color::from_hex("#FFF").is_err());
    }

    #[test]
    fn shades_count() {
        let c = Color::new(255, 0, 0);
        let shades = c.shades(12);
        assert_eq!(shades.len(), 12);
        // First shade should be light, last should be dark
        let (_, _, l_first) = shades[0].to_hsl();
        let (_, _, l_last) = shades[11].to_hsl();
        assert!(l_first > l_last);
    }

    #[test]
    fn shades_zero() {
        let c = Color::new(128, 128, 128);
        assert!(c.shades(0).is_empty());
    }

    #[test]
    fn shades_one() {
        let c = Color::new(128, 128, 128);
        assert_eq!(c.shades(1).len(), 1);
    }

    #[test]
    fn cmyk_format() {
        let c = Color::new(255, 0, 0);
        assert_eq!(c.format(ColorFormat::Cmyk), "cmyk(0%, 100%, 100%, 0%)");
    }

    #[test]
    fn cmyk_black() {
        let c = Color::new(0, 0, 0);
        assert_eq!(c.format(ColorFormat::Cmyk), "cmyk(0%, 0%, 0%, 100%)");
    }

    #[test]
    fn cmyk_white() {
        let c = Color::new(255, 255, 255);
        assert_eq!(c.format(ColorFormat::Cmyk), "cmyk(0%, 0%, 0%, 0%)");
    }
}

#[cfg(test)]
mod upstream_format_tests {
    use super::*;

    const RED: Color = Color { r: 255, g: 0, b: 0 };
    // Upstream example color (Color.Moccasin).
    const MOCCASIN: Color = Color {
        r: 255,
        g: 228,
        b: 181,
    };

    #[test]
    fn hsb_hsi_hwb() {
        assert_eq!(RED.format(ColorFormat::Hsb), "hsb(0, 100%, 100%)");
        assert_eq!(RED.format(ColorFormat::Hsi), "hsi(0, 100%, 33%)");
        assert_eq!(RED.format(ColorFormat::Hwb), "hwb(0, 0%, 0%)");
        assert_eq!(MOCCASIN.format(ColorFormat::Hsl), "hsl(38, 100%, 85%)");
        assert_eq!(MOCCASIN.format(ColorFormat::Hsv), "hsv(38, 29%, 100%)");
        assert_eq!(MOCCASIN.format(ColorFormat::Hwb), "hwb(38, 71%, 0%)");
        assert_eq!(
            Color::new(0, 0, 0).format(ColorFormat::Hsi),
            "hsi(0, 0%, 0%)"
        );
    }

    #[test]
    fn ncol() {
        assert_eq!(RED.format(ColorFormat::Ncol), "R0, 0%, 0%");
        assert_eq!(
            Color::new(0, 255, 0).format(ColorFormat::Ncol),
            "G0, 0%, 0%"
        );
        assert_eq!(MOCCASIN.format(ColorFormat::Ncol), "R64, 71%, 0%");
    }

    #[test]
    fn cie_xyz_and_lab() {
        assert_eq!(
            RED.format(ColorFormat::Ciexyz),
            "XYZ(41.2391, 21.2639, 1.9331)"
        );
        assert_eq!(
            RED.format(ColorFormat::Cielab),
            "CIELab(53.24, 80.09, 67.2)"
        );
        assert_eq!(
            Color::new(255, 255, 255).format(ColorFormat::Cielab),
            "CIELab(100, 0, 0)"
        );
        assert_eq!(
            Color::new(0, 0, 0).format(ColorFormat::Ciexyz),
            "XYZ(0, 0, 0)"
        );
    }

    #[test]
    fn oklab_oklch() {
        assert_eq!(RED.format(ColorFormat::Oklab), "oklab(0.63, 0.22, 0.13)");
        assert_eq!(RED.format(ColorFormat::Oklch), "oklch(0.63, 0.26, 29.23)");
        assert_eq!(
            Color::new(255, 255, 255).format(ColorFormat::Oklch),
            "oklch(1, 0, 0)"
        );
    }

    #[test]
    fn vec4_decimal_hexint() {
        assert_eq!(
            Color::new(255, 128, 0).format(ColorFormat::Vec4),
            "(1f, 0.5f, 0f, 1f)"
        );
        assert_eq!(RED.format(ColorFormat::Decimal), "255");
        assert_eq!(
            Color::new(0, 0, 255).format(ColorFormat::Decimal),
            "16711680"
        );
        assert_eq!(
            Color::new(255, 128, 0).format(ColorFormat::Hexint),
            "0xFFFF8000"
        );
    }

    #[test]
    fn ids_round_trip() {
        for f in ColorFormat::ALL {
            assert_eq!(ColorFormat::from_id(f.id()), Some(f));
            assert_eq!(ColorFormat::from_id(f.label()), Some(f));
        }
        assert_eq!(ColorFormat::from_id("HEX Int"), Some(ColorFormat::Hexint));
        assert_eq!(ColorFormat::from_id("bogus"), None);
    }

    #[test]
    fn serde_accepts_ids_and_upstream_names() {
        #[derive(Deserialize)]
        struct W {
            f: ColorFormat,
        }
        let w: W = toml::from_str("f = \"hexint\"").unwrap();
        assert_eq!(w.f, ColorFormat::Hexint);
        let w: W = toml::from_str("f = \"CIELAB\"").unwrap();
        assert_eq!(w.f, ColorFormat::Cielab);
    }
}
