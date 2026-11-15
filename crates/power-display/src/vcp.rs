//! MCCS VCP feature codes and human-readable value names.
//!
//! Mirrors upstream `PowerDisplay.Lib/Utils/VcpNames.cs` for the codes this
//! module controls.

use serde::{Deserialize, Serialize};

pub const VCP_BRIGHTNESS: u8 = 0x10;
pub const VCP_CONTRAST: u8 = 0x12;
pub const VCP_COLOR_PRESET: u8 = 0x14;
pub const VCP_INPUT_SOURCE: u8 = 0x60;
pub const VCP_VOLUME: u8 = 0x62;
pub const VCP_POWER_MODE: u8 = 0xD6;

/// A monitor feature this module can read and write.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Feature {
    Brightness,
    Contrast,
    Volume,
    /// "Select Color Preset" (VCP 0x14), shown as color temperature.
    ColorPreset,
    InputSource,
    PowerMode,
}

impl Feature {
    pub const ALL: [Feature; 6] = [
        Feature::Brightness,
        Feature::Contrast,
        Feature::Volume,
        Feature::ColorPreset,
        Feature::InputSource,
        Feature::PowerMode,
    ];

    pub fn vcp_code(self) -> u8 {
        match self {
            Feature::Brightness => VCP_BRIGHTNESS,
            Feature::Contrast => VCP_CONTRAST,
            Feature::Volume => VCP_VOLUME,
            Feature::ColorPreset => VCP_COLOR_PRESET,
            Feature::InputSource => VCP_INPUT_SOURCE,
            Feature::PowerMode => VCP_POWER_MODE,
        }
    }

    pub fn from_vcp_code(code: u8) -> Option<Self> {
        Feature::ALL.into_iter().find(|f| f.vcp_code() == code)
    }

    /// Continuous features are exposed as a 0..=100 percentage.
    /// Non-continuous features use the raw VCP value (e.g. 0x11 = HDMI-1).
    pub fn is_continuous(self) -> bool {
        matches!(
            self,
            Feature::Brightness | Feature::Contrast | Feature::Volume
        )
    }

    pub fn label(self) -> &'static str {
        match self {
            Feature::Brightness => "Brightness",
            Feature::Contrast => "Contrast",
            Feature::Volume => "Volume",
            Feature::ColorPreset => "Color temperature",
            Feature::InputSource => "Input source",
            Feature::PowerMode => "Power state",
        }
    }
}

/// Built-in name for a non-continuous VCP value, if known.
pub fn value_name(code: u8, value: u16) -> Option<&'static str> {
    let v = value;
    let name = match code {
        VCP_COLOR_PRESET => match v {
            0x01 => "sRGB",
            0x02 => "Display Native",
            0x03 => "4000K",
            0x04 => "5000K",
            0x05 => "6500K",
            0x06 => "7500K",
            0x07 => "8200K",
            0x08 => "9300K",
            0x09 => "10000K",
            0x0A => "11500K",
            0x0B => "User 1",
            0x0C => "User 2",
            0x0D => "User 3",
            _ => return None,
        },
        VCP_INPUT_SOURCE => match v {
            0x01 => "VGA-1",
            0x02 => "VGA-2",
            0x03 => "DVI-1",
            0x04 => "DVI-2",
            0x05 => "Composite Video 1",
            0x06 => "Composite Video 2",
            0x07 => "S-Video-1",
            0x08 => "S-Video-2",
            0x09 => "Tuner-1",
            0x0A => "Tuner-2",
            0x0B => "Tuner-3",
            0x0C => "Component Video 1",
            0x0D => "Component Video 2",
            0x0E => "Component Video 3",
            0x0F => "DisplayPort-1",
            0x10 => "DisplayPort-2",
            0x11 => "HDMI-1",
            0x12 => "HDMI-2",
            0x1B => "USB-C",
            _ => return None,
        },
        VCP_POWER_MODE => match v {
            0x01 => "On",
            0x02 => "Standby",
            0x03 => "Suspend",
            0x04 => "Off (DPM)",
            0x05 => "Off (Hard)",
            _ => return None,
        },
        _ => return None,
    };
    Some(name)
}

/// Formatted name like "HDMI-1 (0x11)", or "0x11" if unknown.
pub fn formatted_value_name(code: u8, value: u16) -> String {
    match value_name(code, value) {
        Some(n) => format!("{n} (0x{value:02X})"),
        None => format!("0x{value:02X}"),
    }
}

/// Values offered when a monitor does not advertise the allowed values for a
/// non-continuous feature in its capabilities string.
pub fn fallback_values(code: u8) -> Vec<u16> {
    match code {
        VCP_COLOR_PRESET => (0x01..=0x0B).collect(),
        VCP_INPUT_SOURCE => vec![0x01, 0x03, 0x0F, 0x10, 0x11, 0x12],
        VCP_POWER_MODE => vec![0x01, 0x04, 0x05],
        _ => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(value_name(0x14, 0x05), Some("6500K"));
        assert_eq!(value_name(0x60, 0x11), Some("HDMI-1"));
        assert_eq!(value_name(0xD6, 0x04), Some("Off (DPM)"));
        assert_eq!(value_name(0x60, 0x77), None);
        assert_eq!(formatted_value_name(0x60, 0x0F), "DisplayPort-1 (0x0F)");
        assert_eq!(formatted_value_name(0x60, 0x77), "0x77");
    }

    #[test]
    fn feature_codes_round_trip() {
        for f in Feature::ALL {
            assert_eq!(Feature::from_vcp_code(f.vcp_code()), Some(f));
        }
        assert!(Feature::Brightness.is_continuous());
        assert!(!Feature::InputSource.is_continuous());
    }
}
