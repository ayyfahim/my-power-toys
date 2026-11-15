use egui::Color32;
use mpt_common::theme::{self, GlassTheme, Theme};

use crate::config::GuideTheme;

/// Colours used by the overlay. Built from the shared glass theme (dark) or a
/// light variant derived from the shared light theme.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    pub bg_primary: Color32,
    pub bg_header: Color32,
    pub bg_card: Color32,
    pub separator: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub key_bg: Color32,
    pub key_border: Color32,
    pub key_text: Color32,
}

impl Palette {
    pub const fn dark() -> Self {
        let glass = GlassTheme::dark();
        Self {
            dark: true,
            bg_primary: glass.bg_primary,
            bg_header: glass.bg_header,
            bg_card: glass.bg_hover,
            separator: glass.separator,
            border: glass.border,
            accent: glass.accent,
            text_primary: glass.text_primary,
            text_secondary: glass.text_secondary,
            text_muted: glass.text_muted,
            key_bg: glass.bg_hover,
            key_border: glass.border,
            key_text: glass.text_primary,
        }
    }

    pub const fn light() -> Self {
        let light = Theme::light();
        Self {
            dark: false,
            bg_primary: Color32::from_rgba_premultiplied(245, 245, 250, 235),
            bg_header: Color32::from_rgba_premultiplied(232, 232, 240, 245),
            bg_card: Color32::from_rgba_premultiplied(225, 225, 235, 200),
            separator: Color32::from_rgba_premultiplied(150, 150, 170, 90),
            border: Color32::from_rgba_premultiplied(160, 160, 185, 140),
            accent: light.text_accent,
            text_primary: light.text_primary,
            text_secondary: light.text_secondary,
            text_muted: light.text_muted,
            key_bg: Color32::from_rgba_premultiplied(255, 255, 255, 240),
            key_border: Color32::from_rgba_premultiplied(150, 150, 175, 160),
            key_text: light.text_primary,
        }
    }

    pub fn for_theme(theme: GuideTheme) -> Self {
        let dark = match theme {
            GuideTheme::Dark => true,
            GuideTheme::Light => false,
            GuideTheme::System => system_prefers_dark(),
        };
        if dark { Self::dark() } else { Self::light() }
    }
}

/// Best-effort desktop colour-scheme detection. Defaults to dark when no
/// preference can be read.
pub fn system_prefers_dark() -> bool {
    if let Some(v) = gsettings("org.gnome.desktop.interface", "color-scheme") {
        if v.contains("prefer-dark") {
            return true;
        }
        if v.contains("prefer-light") {
            return false;
        }
    }
    if let Ok(theme) = std::env::var("GTK_THEME") {
        return theme.to_lowercase().contains("dark");
    }
    if let Some(v) = gsettings("org.gnome.desktop.interface", "gtk-theme") {
        return v.to_lowercase().contains("dark");
    }
    true
}

fn gsettings(schema: &str, key: &str) -> Option<String> {
    let out = std::process::Command::new("gsettings")
        .args(["get", schema, key])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

// ── Dimensions (aligned with command-palette overlay) ──────────────────────
pub const WINDOW_MAX_WIDTH: f32 = 780.0;
pub const WINDOW_MAX_HEIGHT: f32 = 640.0;
pub const CORNER_RADIUS: f32 = 12.0;
pub const INNER_PADDING: f32 = 16.0;
pub const CATEGORY_SPACING: f32 = 14.0;
pub const ROW_HEIGHT: f32 = 26.0;
/// Gap between the overlay and the monitor edge for Left/Right placement.
pub const EDGE_MARGIN: f32 = 24.0;

// ── Fonts (from shared theme) ──────────────────────────────────────────────
pub const FONT_TITLE: f32 = theme::FONT_TITLE;
pub const FONT_SUBTITLE: f32 = theme::FONT_SMALL;
pub const FONT_CATEGORY: f32 = theme::FONT_SECTION;
pub const FONT_DESC: f32 = theme::FONT_BODY;
pub const FONT_KEY: f32 = theme::FONT_CHIP;
pub const FONT_HINT: f32 = theme::FONT_SMALL;
