use egui::Color32;
use mpt_common::theme::GlassTheme;

const GLASS: GlassTheme = GlassTheme::dark();

// ── Glassmorphism backgrounds (from shared theme) ──────────────────────────
pub const BG_PRIMARY: Color32 = GLASS.bg_primary;
pub const BG_SELECTED: Color32 = GLASS.bg_selected;
pub const BG_HOVER: Color32 = GLASS.bg_hover;
pub const SEPARATOR: Color32 = GLASS.separator;
pub const BORDER: Color32 = GLASS.border;

// ── Text (from shared theme) ───────────────────────────────────────────────
pub const TEXT_PRIMARY: Color32 = GLASS.text_primary;
pub const TEXT_SECONDARY: Color32 = GLASS.text_secondary;
pub const TEXT_HINT: Color32 = GLASS.text_muted;
pub const TEXT_TAG: Color32 = Color32::from_rgb(110, 110, 130);

// ── Accent (from shared theme) ─────────────────────────────────────────────
pub const ACCENT: Color32 = GLASS.accent;

// ── Module-specific dimensions ─────────────────────────────────────────────
pub const WINDOW_WIDTH: f32 = 680.0;
pub const SEARCH_BAR_HEIGHT: f32 = 56.0;
pub const RESULT_ROW_HEIGHT: f32 = 48.0;
pub const MAX_VISIBLE_RESULTS: usize = 12;
pub const FOOTER_HEIGHT: f32 = 26.0;
pub const CORNER_RADIUS: f32 = 12.0;
pub const INNER_PADDING: f32 = 16.0;

// ── Module-specific font sizes ─────────────────────────────────────────────
pub const FONT_SEARCH: f32 = 18.0;
pub const FONT_TITLE: f32 = 14.0;
pub const FONT_SUBTITLE: f32 = 11.0;
pub const FONT_TAG: f32 = 10.0;
pub const FONT_ICON: f32 = 20.0;
