//! Screen Ruler settings, mirroring upstream `MeasureToolProperties`.

use serde::{Deserialize, Serialize};

/// Module id used for the config file (`~/.config/my-power-toys/screen-ruler.toml`).
pub const MODULE_ID: &str = "screen-ruler";

/// Measuring tool. Mirrors upstream `MeasureToolMeasureStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeasureStyle {
    /// No tool pre-selected; the user picks one from the toolbar (upstream default).
    #[default]
    None,
    Bounds,
    Spacing,
    HorizontalSpacing,
    VerticalSpacing,
}

impl MeasureStyle {
    /// Whether this style draws the horizontal / vertical cross line
    /// (upstream `GetHorizontalVerticalLines`).
    pub fn lines(self) -> (bool, bool) {
        match self {
            MeasureStyle::Spacing => (true, true),
            MeasureStyle::HorizontalSpacing => (true, false),
            MeasureStyle::VerticalSpacing => (false, true),
            MeasureStyle::Bounds | MeasureStyle::None => (true, true),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MeasureStyle::None => "None",
            MeasureStyle::Bounds => "Bounds",
            MeasureStyle::Spacing => "Spacing",
            MeasureStyle::HorizontalSpacing => "Horizontal spacing",
            MeasureStyle::VerticalSpacing => "Vertical spacing",
        }
    }
}

/// Units of measure. Mirrors upstream `UnitsOfMeasure` combo (index 0..3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitsOfMeasure {
    #[default]
    Pixels,
    Inches,
    Centimeters,
    Millimeters,
}

impl UnitsOfMeasure {
    pub fn abbreviation(self) -> &'static str {
        match self {
            UnitsOfMeasure::Pixels => "px",
            UnitsOfMeasure::Inches => "in",
            UnitsOfMeasure::Centimeters => "cm",
            UnitsOfMeasure::Millimeters => "mm",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScreenRulerConfig {
    /// Hotkey that opens the overlay (handled by the daemon).
    #[serde(default = "default_activation_shortcut")]
    pub activation_shortcut: String,

    /// Upstream re-captures the screen every frame. Not supported on Linux: the
    /// overlay cannot be excluded from captures, so the screenshot stays frozen
    /// (a warning is logged when this is enabled).
    #[serde(default)]
    pub continuous_capture: bool,

    /// Draw short perpendicular "feet" at the ends of the measuring cross.
    #[serde(default = "default_true")]
    pub draw_feet_on_cross: bool,

    /// Compare each colour channel against the tolerance instead of the summed difference.
    #[serde(default)]
    pub per_color_channel_edge_detection: bool,

    /// Edge detection tolerance (0-255). Adjustable with the mouse wheel in the overlay.
    #[serde(default = "default_pixel_tolerance")]
    pub pixel_tolerance: u8,

    /// Colour of the measuring lines, e.g. "#FF4500".
    #[serde(default = "default_cross_color")]
    pub measure_cross_color: String,

    /// Tool selected when the overlay opens.
    #[serde(default)]
    pub default_measure_style: MeasureStyle,

    #[serde(default)]
    pub units_of_measure: UnitsOfMeasure,
}

fn default_activation_shortcut() -> String {
    "Super+Shift+R".into()
}

fn default_true() -> bool {
    true
}

fn default_pixel_tolerance() -> u8 {
    30
}

fn default_cross_color() -> String {
    "#FF4500".into()
}

impl Default for ScreenRulerConfig {
    fn default() -> Self {
        Self {
            activation_shortcut: default_activation_shortcut(),
            continuous_capture: false,
            draw_feet_on_cross: true,
            per_color_channel_edge_detection: false,
            pixel_tolerance: default_pixel_tolerance(),
            measure_cross_color: default_cross_color(),
            default_measure_style: MeasureStyle::default(),
            units_of_measure: UnitsOfMeasure::default(),
        }
    }
}

impl ScreenRulerConfig {
    /// Load from `~/.config/my-power-toys/screen-ruler.toml`, falling back to defaults.
    pub fn load() -> Self {
        mpt_common::config::load_module_config(MODULE_ID).unwrap_or_default()
    }

    /// Cross colour as RGB, falling back to the upstream default on parse errors.
    pub fn cross_color_rgb(&self) -> (u8, u8, u8) {
        parse_hex_color(&self.measure_cross_color).unwrap_or((0xFF, 0x45, 0x00))
    }
}

/// Parse a hex colour like "#FF4500" or "FF4500" into (r, g, b).
pub fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim();
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_upstream() {
        let c = ScreenRulerConfig::default();
        assert_eq!(c.activation_shortcut, "Super+Shift+R");
        assert!(!c.continuous_capture);
        assert!(c.draw_feet_on_cross);
        assert!(!c.per_color_channel_edge_detection);
        assert_eq!(c.pixel_tolerance, 30);
        assert_eq!(c.measure_cross_color, "#FF4500");
        assert_eq!(c.default_measure_style, MeasureStyle::None);
        assert_eq!(c.units_of_measure, UnitsOfMeasure::Pixels);
    }

    #[test]
    fn roundtrip_and_empty() {
        let c = ScreenRulerConfig {
            default_measure_style: MeasureStyle::HorizontalSpacing,
            units_of_measure: UnitsOfMeasure::Millimeters,
            ..Default::default()
        };
        let s = toml::to_string_pretty(&c).unwrap();
        assert!(s.contains("default_measure_style = \"horizontal_spacing\""));
        let back: ScreenRulerConfig = toml::from_str(&s).unwrap();
        assert_eq!(back, c);

        let empty: ScreenRulerConfig = toml::from_str("").unwrap();
        assert_eq!(empty, ScreenRulerConfig::default());
    }

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex_color("#FF4500"), Some((255, 69, 0)));
        assert_eq!(parse_hex_color("00ff00"), Some((0, 255, 0)));
        assert_eq!(parse_hex_color("#ZZZZZZ"), None);
        assert_eq!(parse_hex_color("#ééé"), None);
        let c = ScreenRulerConfig {
            measure_cross_color: "bogus".into(),
            ..Default::default()
        };
        assert_eq!(c.cross_color_rgb(), (255, 69, 0));
    }
}
