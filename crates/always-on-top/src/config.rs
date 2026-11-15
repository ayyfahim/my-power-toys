use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlwaysOnTopConfig {
    #[serde(default = "default_true")]
    pub show_border: bool,

    #[serde(default = "default_border_color")]
    pub border_color: String,

    #[serde(default = "default_border_opacity")]
    pub border_opacity: f32,

    #[serde(default = "default_border_thickness")]
    pub border_thickness: u32,

    #[serde(default = "default_true")]
    pub play_sound: bool,

    #[serde(default)]
    pub excluded_apps: Vec<String>,

    /// Pin/unpin shortcut (upstream `hotkey`, Win+Ctrl+T).
    #[serde(default = "default_hotkey")]
    pub hotkey: String,

    /// Raise the pinned window's opacity (upstream `increase-opacity-hotkey`).
    #[serde(default = "default_increase_opacity_hotkey")]
    pub increase_opacity_hotkey: String,

    /// Lower the pinned window's opacity (upstream `decrease-opacity-hotkey`).
    #[serde(default = "default_decrease_opacity_hotkey")]
    pub decrease_opacity_hotkey: String,

    /// Use the desktop accent color for the frame instead of `border_color`.
    #[serde(default = "default_true")]
    pub frame_accent_color: bool,

    /// Draw the frame with rounded corners.
    #[serde(default = "default_true")]
    pub round_corners_enabled: bool,

    /// Do not pin while the focused window is fullscreen (Linux stand-in for Game Mode).
    #[serde(default = "default_true")]
    pub do_not_activate_on_game_mode: bool,
}

/// Opacity change per increase/decrease hotkey press.
pub const OPACITY_STEP: f32 = 0.1;
/// Lowest opacity the decrease hotkey goes to (upstream stops at 20%).
pub const MIN_OPACITY: f32 = 0.2;

/// Next opacity after an increase (`up = true`) or decrease press, kept in `MIN_OPACITY..=1.0`.
pub fn step_opacity(current: f32, up: bool) -> f32 {
    let next = if up {
        current + OPACITY_STEP
    } else {
        current - OPACITY_STEP
    };
    // Round to whole percent to avoid float drift.
    ((next * 100.0).round() / 100.0).clamp(MIN_OPACITY, 1.0)
}

/// Map a GNOME `accent-color` value (as printed by `gsettings get`, e.g. `'blue'`)
/// to the hex color libadwaita uses.
pub fn gnome_accent_hex(value: &str) -> Option<&'static str> {
    Some(
        match value.trim().trim_matches('\'').to_lowercase().as_str() {
            "blue" => "#3584E4",
            "teal" => "#2190A4",
            "green" => "#3A944A",
            "yellow" => "#C88800",
            "orange" => "#ED5B00",
            "red" => "#E62D42",
            "pink" => "#D56199",
            "purple" => "#9141AC",
            "slate" => "#6F8396",
            _ => return None,
        },
    )
}

fn default_hotkey() -> String {
    "Super+T".into()
}

fn default_increase_opacity_hotkey() -> String {
    "Super+Ctrl+Shift+=".into()
}

fn default_decrease_opacity_hotkey() -> String {
    "Super+Ctrl+Shift+-".into()
}

fn default_true() -> bool {
    true
}

fn default_border_color() -> String {
    "#0078D4".into()
}

fn default_border_opacity() -> f32 {
    0.8
}

fn default_border_thickness() -> u32 {
    3
}

impl Default for AlwaysOnTopConfig {
    fn default() -> Self {
        Self {
            show_border: true,
            border_color: default_border_color(),
            border_opacity: default_border_opacity(),
            border_thickness: default_border_thickness(),
            play_sound: true,
            excluded_apps: Vec::new(),
            hotkey: default_hotkey(),
            increase_opacity_hotkey: default_increase_opacity_hotkey(),
            decrease_opacity_hotkey: default_decrease_opacity_hotkey(),
            frame_accent_color: true,
            round_corners_enabled: true,
            do_not_activate_on_game_mode: true,
        }
    }
}

/// Parse a hex color string like "#0078D4" or "#FF0000" into (r, g, b).
pub fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.strip_prefix('#').unwrap_or(hex);
    if hex.len() != 6 {
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
    fn parse_colors() {
        assert_eq!(parse_hex_color("#0078D4"), Some((0, 120, 212)));
        assert_eq!(parse_hex_color("#FF0000"), Some((255, 0, 0)));
        assert_eq!(parse_hex_color("00FF00"), Some((0, 255, 0)));
        assert_eq!(parse_hex_color("#ZZZ"), None);
    }

    #[test]
    fn default_config_roundtrips() {
        let cfg = AlwaysOnTopConfig::default();
        let s = toml::to_string_pretty(&cfg).unwrap();
        let _: AlwaysOnTopConfig = toml::from_str(&s).unwrap();
    }

    #[test]
    fn old_config_gets_new_defaults() {
        let cfg: AlwaysOnTopConfig = toml::from_str("show_border = false").unwrap();
        assert!(!cfg.show_border);
        assert_eq!(cfg.hotkey, "Super+T");
        assert_eq!(cfg.increase_opacity_hotkey, "Super+Ctrl+Shift+=");
        assert!(cfg.round_corners_enabled);
        assert!(cfg.do_not_activate_on_game_mode);
        for hk in [
            &cfg.hotkey,
            &cfg.increase_opacity_hotkey,
            &cfg.decrease_opacity_hotkey,
        ] {
            mpt_common::hotkey::Hotkey::parse(hk).unwrap();
        }
    }

    #[test]
    fn opacity_steps_are_clamped() {
        assert_eq!(step_opacity(1.0, true), 1.0);
        assert_eq!(step_opacity(1.0, false), 0.9);
        assert_eq!(step_opacity(0.3, false), 0.2);
        assert_eq!(step_opacity(0.2, false), 0.2);
        assert_eq!(step_opacity(0.2, true), 0.3);
    }

    #[test]
    fn gnome_accent_names() {
        assert_eq!(gnome_accent_hex("'blue'\n"), Some("#3584E4"));
        assert_eq!(gnome_accent_hex("purple"), Some("#9141AC"));
        assert_eq!(gnome_accent_hex("'unknown'"), None);
    }
}
