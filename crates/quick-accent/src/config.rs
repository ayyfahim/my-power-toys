use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationKey {
    #[default]
    Space,
    LeftRight,
    #[serde(alias = "both")]
    Any,
    /// Holding the letter alone for `hold_duration_ms` opens the toolbar
    /// (upstream `PressAndHold`).
    PressAndHold,
}

/// Toolbar placement. `AboveCursor`/`BelowCursor` are specific to this port;
/// the other nine match upstream's `ToolbarPosition` values (whose display
/// strings are accepted as aliases).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolbarPosition {
    #[default]
    AboveCursor,
    BelowCursor,
    #[serde(alias = "Top center")]
    TopCenter,
    #[serde(alias = "Bottom center")]
    BottomCenter,
    #[serde(alias = "Left")]
    Left,
    #[serde(alias = "Right")]
    Right,
    #[serde(alias = "Top left corner")]
    TopLeft,
    #[serde(alias = "Top right corner")]
    TopRight,
    #[serde(alias = "Bottom left corner")]
    BottomLeft,
    #[serde(alias = "Bottom right corner")]
    BottomRight,
    #[serde(alias = "Center")]
    Center,
}

impl ToolbarPosition {
    pub const ALL: [ToolbarPosition; 11] = [
        Self::AboveCursor,
        Self::BelowCursor,
        Self::TopCenter,
        Self::BottomCenter,
        Self::Left,
        Self::Right,
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
        Self::Center,
    ];

    /// Value passed to the overlay binary via `--position`.
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::AboveCursor => "above-cursor",
            Self::BelowCursor => "below-cursor",
            Self::TopCenter => "top-center",
            Self::BottomCenter => "bottom-center",
            Self::Left => "left",
            Self::Right => "right",
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
            Self::Center => "center",
        }
    }

    pub fn from_arg(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|p| p.as_arg() == s)
            .unwrap_or_default()
    }
}

/// Simple rectangle in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Distance kept between the toolbar and the screen edges / cursor.
pub const EDGE_MARGIN: i32 = 20;

/// Top-left corner of the toolbar for `position`, clamped into `screen`.
pub fn toolbar_origin(
    position: ToolbarPosition,
    cursor: (i32, i32),
    screen: ScreenRect,
    size: (i32, i32),
) -> (i32, i32) {
    let (w, h) = size;
    let (cx, cy) = cursor;
    let left = screen.x + EDGE_MARGIN;
    let right = screen.x + screen.width - w - EDGE_MARGIN;
    let top = screen.y + EDGE_MARGIN;
    let bottom = screen.y + screen.height - h - EDGE_MARGIN;
    let hcenter = screen.x + (screen.width - w) / 2;
    let vcenter = screen.y + (screen.height - h) / 2;

    let (x, y) = match position {
        ToolbarPosition::AboveCursor => (cx - w / 2, cy - h - EDGE_MARGIN),
        ToolbarPosition::BelowCursor => (cx - w / 2, cy + 30),
        ToolbarPosition::TopCenter => (hcenter, top),
        ToolbarPosition::BottomCenter => (hcenter, bottom),
        ToolbarPosition::Left => (left, vcenter),
        ToolbarPosition::Right => (right, vcenter),
        ToolbarPosition::TopLeft => (left, top),
        ToolbarPosition::TopRight => (right, top),
        ToolbarPosition::BottomLeft => (left, bottom),
        ToolbarPosition::BottomRight => (right, bottom),
        ToolbarPosition::Center => (hcenter, vcenter),
    };

    let max_x = (screen.x + screen.width - w).max(screen.x);
    let max_y = (screen.y + screen.height - h).max(screen.y);
    (x.clamp(screen.x, max_x), y.clamp(screen.y, max_y))
}

/// Which key opened the toolbar (for `start_selection_from_the_left`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Space,
    Left,
    Right,
    Hold,
}

impl Trigger {
    pub fn as_arg(self) -> &'static str {
        match self {
            Self::Space => "space",
            Self::Left => "left",
            Self::Right => "right",
            Self::Hold => "hold",
        }
    }

    pub fn from_arg(s: &str) -> Self {
        match s {
            "left" => Self::Left,
            "right" => Self::Right,
            "hold" => Self::Hold,
            _ => Self::Space,
        }
    }
}

/// Initial selected index, mirroring upstream `PowerAccent.ProcessNextChar`:
/// Space selects the first char; arrows start in the middle (Left: just
/// left of centre, Right: centre) unless `start_from_left` is set.
pub fn initial_index(len: usize, trigger: Trigger, start_from_left: bool) -> usize {
    if len == 0 {
        return 0;
    }
    let idx = match trigger {
        Trigger::Space | Trigger::Hold => 0,
        _ if start_from_left => 0,
        Trigger::Left => (len / 2).saturating_sub(1),
        Trigger::Right => len / 2,
    };
    idx.min(len - 1)
}

fn default_input_delay_ms() -> u64 {
    200
}

fn default_hold_duration_ms() -> u64 {
    500
}

fn default_true() -> bool {
    true
}

fn default_languages() -> Vec<String> {
    vec![
        "fr".into(),
        "es".into(),
        "de".into(),
        "pt".into(),
        "it".into(),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickAccentConfig {
    #[serde(default)]
    pub activation_key: ActivationKey,

    /// Delay after the activation key before the toolbar opens
    /// (upstream `InputTime`).
    #[serde(default = "default_input_delay_ms", alias = "input_time")]
    pub input_delay_ms: u64,

    #[serde(default = "default_languages")]
    pub languages: Vec<String>,

    #[serde(default)]
    pub toolbar_position: ToolbarPosition,

    /// How long a letter must be held alone in `press-and-hold` activation
    /// (upstream `HoldDuration`).
    #[serde(default = "default_hold_duration_ms")]
    pub hold_duration_ms: u64,

    /// Don't open while a fullscreen window is focused or Feral GameMode is
    /// active (upstream `DoNotActivateOnGameMode`).
    #[serde(default = "default_true")]
    pub do_not_activate_on_game_mode: bool,

    /// WM_CLASS values (instance or class, case-insensitive) where Quick
    /// Accent is disabled (upstream `ExcludedApps`).
    #[serde(default)]
    pub excluded_apps: Vec<String>,

    /// Show the Unicode name of the selected character
    /// (upstream `ShowUnicodeDescription`).
    #[serde(default)]
    pub show_unicode_description: bool,

    /// Order characters by how often they were picked
    /// (upstream `SortByUsageFrequency`).
    #[serde(default)]
    pub sort_by_usage_frequency: bool,

    /// Arrow activation selects the first character instead of the middle
    /// one (upstream `StartSelectionFromTheLeft`).
    #[serde(default)]
    pub start_selection_from_the_left: bool,
}

impl Default for QuickAccentConfig {
    fn default() -> Self {
        Self {
            activation_key: ActivationKey::default(),
            input_delay_ms: default_input_delay_ms(),
            languages: default_languages(),
            toolbar_position: ToolbarPosition::default(),
            hold_duration_ms: default_hold_duration_ms(),
            do_not_activate_on_game_mode: true,
            excluded_apps: Vec::new(),
            show_unicode_description: false,
            sort_by_usage_frequency: false,
            start_selection_from_the_left: false,
        }
    }
}

impl QuickAccentConfig {
    pub fn is_excluded(&self, wm_classes: &[String]) -> bool {
        self.excluded_apps.iter().any(|app| {
            let app = app.trim();
            !app.is_empty() && wm_classes.iter().any(|c| c.eq_ignore_ascii_case(app))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: ScreenRect = ScreenRect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn legacy_config_parses() {
        let cfg: QuickAccentConfig =
            toml::from_str("activation_key = \"left-right\"\ntoolbar_position = \"top-center\"\n")
                .unwrap();
        assert_eq!(cfg.activation_key, ActivationKey::LeftRight);
        assert_eq!(cfg.toolbar_position, ToolbarPosition::TopCenter);
        assert_eq!(cfg.hold_duration_ms, 500);
        assert!(cfg.do_not_activate_on_game_mode);
    }

    #[test]
    fn upstream_position_names() {
        let cfg: QuickAccentConfig =
            toml::from_str("toolbar_position = \"Bottom right corner\"\n").unwrap();
        assert_eq!(cfg.toolbar_position, ToolbarPosition::BottomRight);
        let cfg: QuickAccentConfig = toml::from_str("toolbar_position = \"Center\"\n").unwrap();
        assert_eq!(cfg.toolbar_position, ToolbarPosition::Center);
    }

    #[test]
    fn position_arg_round_trip() {
        for p in ToolbarPosition::ALL {
            assert_eq!(ToolbarPosition::from_arg(p.as_arg()), p);
        }
    }

    #[test]
    fn toolbar_origins() {
        let size = (200, 80);
        assert_eq!(
            toolbar_origin(ToolbarPosition::TopCenter, (0, 0), SCREEN, size),
            (860, EDGE_MARGIN)
        );
        assert_eq!(
            toolbar_origin(ToolbarPosition::BottomRight, (0, 0), SCREEN, size),
            (1920 - 200 - EDGE_MARGIN, 1080 - 80 - EDGE_MARGIN)
        );
        assert_eq!(
            toolbar_origin(ToolbarPosition::Center, (0, 0), SCREEN, size),
            (860, 500)
        );
        // Above cursor near the top edge gets clamped on screen.
        assert_eq!(
            toolbar_origin(ToolbarPosition::AboveCursor, (5, 5), SCREEN, size),
            (0, 0)
        );
    }

    #[test]
    fn initial_selection() {
        assert_eq!(initial_index(5, Trigger::Space, false), 0);
        assert_eq!(initial_index(5, Trigger::Left, false), 1);
        assert_eq!(initial_index(5, Trigger::Right, false), 2);
        assert_eq!(initial_index(5, Trigger::Right, true), 0);
        assert_eq!(initial_index(1, Trigger::Left, false), 0);
        assert_eq!(initial_index(0, Trigger::Right, false), 0);
    }

    #[test]
    fn excluded_apps() {
        let cfg = QuickAccentConfig {
            excluded_apps: vec!["Code".into()],
            ..Default::default()
        };
        assert!(cfg.is_excluded(&["code".into(), "Code".into()]));
        assert!(!cfg.is_excluded(&["firefox".into()]));
    }
}
