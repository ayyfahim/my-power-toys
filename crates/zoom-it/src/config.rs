//! ZoomIt configuration (`~/.config/my-power-toys/zoom-it.toml`).
//!
//! Field names follow upstream `ZoomItProperties` in snake_case.

use serde::{Deserialize, Serialize};

pub const MODULE_ID: &str = "zoom-it";

/// Zoom levels selectable as the initial zoom (upstream `g_ZoomLevels`).
pub const ZOOM_LEVELS: [f32; 6] = [1.25, 1.5, 1.75, 2.0, 3.0, 4.0];

macro_rules! default_fn {
    ($name:ident, $ty:ty, $val:expr) => {
        fn $name() -> $ty {
            $val.into()
        }
    };
}

default_fn!(d_toggle_key, String, "Ctrl+1");
default_fn!(d_draw_toggle_key, String, "Ctrl+2");
default_fn!(d_break_timer_key, String, "Ctrl+3");
default_fn!(d_live_zoom_toggle_key, String, "Ctrl+4");
default_fn!(d_record_toggle_key, String, "Ctrl+5");
default_fn!(d_snip_toggle_key, String, "Ctrl+6");
default_fn!(d_snip_save_toggle_key, String, "Ctrl+Shift+6");
default_fn!(d_true, bool, true);
default_fn!(d_false, bool, false);
default_fn!(d_zoom_initial_level, u32, 3u32);
default_fn!(d_pen_color, String, "#FF0000");
default_fn!(d_pen_width, f32, 5.0f32);
default_fn!(d_font_size, f32, 32.0f32);
default_fn!(d_break_timeout, u32, 10u32);
default_fn!(d_break_opacity, u32, 100u32);
default_fn!(d_break_timer_position, u32, 4u32);
default_fn!(d_empty, String, "");
default_fn!(d_break_background_color, String, "#000000");
default_fn!(d_record_scaling, u32, 100u32);
default_fn!(d_record_frame_rate, u32, 30u32);

/// User configuration for the ZoomIt module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ZoomItConfig {
    // ---- Hotkeys -------------------------------------------------------
    /// Zoom mode (upstream `ToggleKey`).
    #[serde(default = "d_toggle_key")]
    pub toggle_key: String,
    #[serde(default = "d_draw_toggle_key")]
    pub draw_toggle_key: String,
    #[serde(default = "d_break_timer_key")]
    pub break_timer_key: String,
    #[serde(default = "d_live_zoom_toggle_key")]
    pub live_zoom_toggle_key: String,
    #[serde(default = "d_record_toggle_key")]
    pub record_toggle_key: String,
    #[serde(default = "d_snip_toggle_key")]
    pub snip_toggle_key: String,
    #[serde(default = "d_snip_save_toggle_key")]
    pub snip_save_toggle_key: String,

    // ---- Zoom ----------------------------------------------------------
    /// Animate zoom in/out (upstream `AnimnateZoom`).
    #[serde(default = "d_true")]
    pub animate_zoom: bool,
    /// Smooth (linear-filtered) zoomed image (upstream `SmoothImage`).
    #[serde(default = "d_true")]
    pub smooth_image: bool,
    /// Index into [`ZOOM_LEVELS`] for the initial zoom (upstream `ZoominSliderLevel`).
    #[serde(default = "d_zoom_initial_level", alias = "zoomin_slider_level")]
    pub zoom_initial_level: u32,

    // ---- Draw ----------------------------------------------------------
    /// Initial pen color as `#RRGGBB` (upstream `PenColor`).
    #[serde(default = "d_pen_color")]
    pub pen_color: String,
    /// Initial pen width in pixels (upstream `PenWidth`).
    #[serde(default = "d_pen_width")]
    pub pen_width: f32,
    /// Initial font size for typing mode (upstream `Font`).
    #[serde(default = "d_font_size")]
    pub font_size: f32,

    // ---- Break timer ---------------------------------------------------
    /// Break length in minutes (upstream `BreakTimeout`).
    #[serde(default = "d_break_timeout")]
    pub break_timeout: u32,
    /// Background opacity in percent, 1..=100 (upstream `BreakOpacity`).
    #[serde(default = "d_break_opacity")]
    pub break_opacity: u32,
    /// 0..=8 grid position, row-major from top-left; 4 = center (upstream `BreakTimerPosition`).
    #[serde(default = "d_break_timer_position")]
    pub break_timer_position: u32,
    /// Show elapsed time after the timer expires (upstream `ShowExpiredTime`).
    #[serde(default = "d_true")]
    pub show_expired_time: bool,
    /// Play a sound when the timer expires (upstream `BreakPlaySoundFile`).
    #[serde(default = "d_false")]
    pub break_play_sound_file: bool,
    /// Sound file to play; empty = desktop "complete" sound (upstream `BreakSoundFile`).
    #[serde(default = "d_empty")]
    pub break_sound_file: String,
    /// Show the (faded) desktop behind the timer (upstream `BreakShowDesktop`).
    #[serde(default = "d_true")]
    pub break_show_desktop: bool,
    /// Show a background image (upstream `BreakShowBackgroundFile`).
    #[serde(default = "d_false")]
    pub break_show_background_file: bool,
    /// Background image path (upstream `BreakBackgroundFile`).
    #[serde(default = "d_empty")]
    pub break_background_file: String,
    /// Stretch background image instead of centering it (upstream `BreakBackgroundStretch`).
    #[serde(default = "d_false")]
    pub break_background_stretch: bool,
    /// Timer text color (upstream `BreakPenColor`).
    #[serde(default = "d_pen_color")]
    pub break_pen_color: String,
    /// Background color when the desktop is not shown (upstream `BreakBackgroundColor`).
    #[serde(default = "d_break_background_color")]
    pub break_background_color: String,

    // ---- Snip ----------------------------------------------------------
    /// When saving a snip, also copy it to the clipboard (upstream `SnipCopyToClipboard`).
    #[serde(default = "d_false")]
    pub snip_copy_to_clipboard: bool,
    /// Directory for screenshots; empty = `~/Pictures`.
    #[serde(default = "d_empty")]
    pub screenshot_save_location: String,

    // ---- Record --------------------------------------------------------
    /// Output scaling in percent (upstream `RecordScaling`).
    #[serde(default = "d_record_scaling")]
    pub record_scaling: u32,
    /// Frame rate (upstream `RecordFrameRate`).
    #[serde(default = "d_record_frame_rate")]
    pub record_frame_rate: u32,
    /// Record microphone audio (upstream `CaptureAudio`).
    #[serde(default = "d_false")]
    pub capture_audio: bool,
    /// Directory for recordings; empty = `~/Videos`.
    #[serde(default = "d_empty")]
    pub recording_save_location: String,
}

impl Default for ZoomItConfig {
    fn default() -> Self {
        Self {
            toggle_key: d_toggle_key(),
            draw_toggle_key: d_draw_toggle_key(),
            break_timer_key: d_break_timer_key(),
            live_zoom_toggle_key: d_live_zoom_toggle_key(),
            record_toggle_key: d_record_toggle_key(),
            snip_toggle_key: d_snip_toggle_key(),
            snip_save_toggle_key: d_snip_save_toggle_key(),
            animate_zoom: true,
            smooth_image: true,
            zoom_initial_level: d_zoom_initial_level(),
            pen_color: d_pen_color(),
            pen_width: d_pen_width(),
            font_size: d_font_size(),
            break_timeout: d_break_timeout(),
            break_opacity: d_break_opacity(),
            break_timer_position: d_break_timer_position(),
            show_expired_time: true,
            break_play_sound_file: false,
            break_sound_file: String::new(),
            break_show_desktop: true,
            break_show_background_file: false,
            break_background_file: String::new(),
            break_background_stretch: false,
            break_pen_color: d_pen_color(),
            break_background_color: d_break_background_color(),
            snip_copy_to_clipboard: false,
            screenshot_save_location: String::new(),
            record_scaling: d_record_scaling(),
            record_frame_rate: d_record_frame_rate(),
            capture_audio: false,
            recording_save_location: String::new(),
        }
    }
}

impl ZoomItConfig {
    /// Load from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        mpt_common::config::load_module_config(MODULE_ID).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        mpt_common::config::save_module_config(MODULE_ID, self)
    }

    /// Initial zoom factor resolved from `zoom_initial_level`.
    pub fn initial_zoom(&self) -> f32 {
        let idx = (self.zoom_initial_level as usize).min(ZOOM_LEVELS.len() - 1);
        ZOOM_LEVELS[idx]
    }

    /// Directory where screenshots are saved.
    pub fn screenshot_dir(&self) -> std::path::PathBuf {
        resolve_dir(
            &self.screenshot_save_location,
            dirs::picture_dir,
            "Pictures",
        )
    }

    /// Directory where recordings are saved.
    pub fn recording_dir(&self) -> std::path::PathBuf {
        resolve_dir(&self.recording_save_location, dirs::video_dir, "Videos")
    }
}

fn resolve_dir(
    configured: &str,
    xdg: fn() -> Option<std::path::PathBuf>,
    fallback: &str,
) -> std::path::PathBuf {
    let configured = configured.trim();
    if !configured.is_empty() {
        if let Some(rest) = configured.strip_prefix("~/")
            && let Some(home) = dirs::home_dir()
        {
            return home.join(rest);
        }
        return configured.into();
    }
    xdg()
        .or_else(|| dirs::home_dir().map(|h| h.join(fallback)))
        .unwrap_or_else(std::env::temp_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_upstream() {
        let c = ZoomItConfig::default();
        assert_eq!(c.toggle_key, "Ctrl+1");
        assert_eq!(c.snip_save_toggle_key, "Ctrl+Shift+6");
        assert_eq!(c.break_timeout, 10);
        assert_eq!(c.break_timer_position, 4);
        assert!((c.initial_zoom() - 2.0).abs() < f32::EPSILON);
    }

    #[test]
    fn empty_toml_gives_defaults() {
        let c: ZoomItConfig = toml::from_str("").unwrap();
        assert_eq!(c, ZoomItConfig::default());
    }

    #[test]
    fn roundtrip_and_alias() {
        let c = ZoomItConfig {
            break_timeout: 3,
            ..Default::default()
        };
        let s = toml::to_string_pretty(&c).unwrap();
        let back: ZoomItConfig = toml::from_str(&s).unwrap();
        assert_eq!(back, c);

        let aliased: ZoomItConfig = toml::from_str("zoomin_slider_level = 99").unwrap();
        assert_eq!(aliased.zoom_initial_level, 99);
        assert!((aliased.initial_zoom() - 4.0).abs() < f32::EPSILON);
    }
}
