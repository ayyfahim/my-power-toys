use egui::Color32;
use mpt_common::theme::Theme;

const THEME: Theme = Theme::dark();

// ── Backgrounds (from shared theme) ────────────────────────────────────────
pub const BG_PRIMARY: Color32 = THEME.bg_primary;
pub const BG_SECONDARY: Color32 = THEME.bg_secondary;
pub const BG_CARD: Color32 = THEME.bg_card;
pub const BG_HEADER: Color32 = THEME.bg_header;
pub const BG_HOVER: Color32 = THEME.bg_hover;
pub const BG_BUTTON: Color32 = THEME.bg_button;
pub const BG_BUTTON_HOVER: Color32 = THEME.bg_button_hover;

// ── Text (from shared theme) ───────────────────────────────────────────────
pub const TEXT_PRIMARY: Color32 = THEME.text_primary;
pub const TEXT_SECONDARY: Color32 = THEME.text_secondary;
pub const TEXT_MUTED: Color32 = THEME.text_muted;
pub const TEXT_ACCENT: Color32 = THEME.text_accent;

// ── Module-specific text aliases ───────────────────────────────────────────
pub const TEXT_FORMAT_LABEL: Color32 = TEXT_SECONDARY;
pub const TEXT_FORMAT_VALUE: Color32 = TEXT_PRIMARY;

// ── Borders (from shared theme) ────────────────────────────────────────────
pub const SEPARATOR: Color32 = THEME.separator;
pub const CARD_BORDER: Color32 = THEME.card_border;

// ── Shared dimensions ──────────────────────────────────────────────────────
pub use mpt_common::theme::{CARD_RADIUS, FONT_BODY, FONT_BUTTON, FONT_ICON, FONT_SMALL};

// ── Module-specific dimensions ─────────────────────────────────────────────
pub const WINDOW_WIDTH: f32 = 420.0;
pub const WINDOW_HEIGHT: f32 = 520.0;
pub const INNER_PADDING: f32 = 16.0;
pub const SHADE_BAR_HEIGHT: f32 = 36.0;
pub const CARD_HEIGHT: f32 = 48.0;
pub const CARD_CORNER_RADIUS: f32 = 8.0;
pub const CARD_ACCENT_WIDTH: f32 = 4.0;
pub const HISTORY_SWATCH_DIAMETER: f32 = 28.0;
pub const GRADIENT_STRIP_WIDTH: f32 = 24.0;
pub const FONT_FORMAT_LABEL: f32 = 11.0;
pub const FONT_FORMAT_VALUE: f32 = 13.0;

/// Apply the theme to an egui context.
pub fn setup_visuals(ctx: &egui::Context) {
    THEME.apply(ctx);
}
