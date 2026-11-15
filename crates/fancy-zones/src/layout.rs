use serde::{Deserialize, Serialize};

/// Kind of zone template.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemplateKind {
    NoLayout,
    Focus,
    Columns,
    Rows,
    Grid,
    PriorityGrid,
    Custom,
}

/// A zone layout definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Layout {
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: TemplateKind,
    pub zones: Vec<Zone>,
    /// Quick layout switch key (Super+Ctrl+Alt+<key>, 1-9), like upstream's
    /// per-layout hotkey. When unset, the key maps to the 1-based layout index.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quick_key: Option<u8>,
}

fn default_kind() -> TemplateKind {
    TemplateKind::Custom
}

/// A zone is a rectangular region on screen, defined as percentages (0.0 - 1.0).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Zone {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Zone {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Convert to absolute pixel coordinates for a given screen size.
    pub fn to_pixels(&self, screen_w: u32, screen_h: u32) -> (i32, i32, u32, u32) {
        let x = (self.x * screen_w as f32) as i32;
        let y = (self.y * screen_h as f32) as i32;
        let w = (self.width * screen_w as f32) as u32;
        let h = (self.height * screen_h as f32) as u32;
        (x, y, w, h)
    }

    /// Convert to pixel coordinates with gap applied (half-gap inset on each side).
    pub fn to_pixels_with_gap(
        &self,
        screen_w: u32,
        screen_h: u32,
        gap: u32,
    ) -> (i32, i32, u32, u32) {
        let (x, y, w, h) = self.to_pixels(screen_w, screen_h);
        let half = (gap / 2) as i32;
        (
            x + half,
            y + half,
            (w as i32 - gap as i32).max(100) as u32,
            (h as i32 - gap as i32).max(100) as u32,
        )
    }

    /// Clamp the zone so it lies within the unit square and is at least 1% big.
    pub fn clamped(&self) -> Self {
        let x = self.x.clamp(0.0, 0.99);
        let y = self.y.clamp(0.0, 0.99);
        Self {
            x,
            y,
            width: self.width.clamp(0.01, 1.0 - x),
            height: self.height.clamp(0.01, 1.0 - y),
        }
    }

    /// Check if a point (in percentages) is inside this zone.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width && py >= self.y && py <= self.y + self.height
    }
}

impl Layout {
    /// No layout — empty zone list.
    pub fn no_layout() -> Self {
        Self {
            name: "No layout".to_string(),
            kind: TemplateKind::NoLayout,
            zones: vec![],
            quick_key: None,
        }
    }

    /// Focus layout — one large centered zone (70% x 80%).
    pub fn focus() -> Self {
        Self {
            name: "Focus".to_string(),
            kind: TemplateKind::Focus,
            zones: vec![Zone::new(0.15, 0.1, 0.7, 0.8)],
            quick_key: None,
        }
    }

    /// Create a layout with N equal columns.
    pub fn default_columns(n: usize) -> Self {
        let w = 1.0 / n as f32;
        let zones = (0..n)
            .map(|i| Zone::new(i as f32 * w, 0.0, w, 1.0))
            .collect();
        Self {
            name: format!("{n} Columns"),
            kind: TemplateKind::Columns,
            zones,
            quick_key: None,
        }
    }

    /// Create a layout with N equal rows.
    pub fn default_rows(n: usize) -> Self {
        let h = 1.0 / n as f32;
        let zones = (0..n)
            .map(|i| Zone::new(0.0, i as f32 * h, 1.0, h))
            .collect();
        Self {
            name: format!("{n} Rows"),
            kind: TemplateKind::Rows,
            zones,
            quick_key: None,
        }
    }

    /// Create a grid layout (NxM).
    pub fn grid(cols: usize, rows: usize) -> Self {
        let w = 1.0 / cols as f32;
        let h = 1.0 / rows as f32;
        let zones = (0..rows)
            .flat_map(|r| (0..cols).map(move |c| Zone::new(c as f32 * w, r as f32 * h, w, h)))
            .collect();
        Self {
            name: format!("{cols}x{rows} Grid"),
            kind: TemplateKind::Grid,
            zones,
            quick_key: None,
        }
    }

    /// Priority Grid — large main zone left, two smaller zones stacked right.
    pub fn priority_grid() -> Self {
        Self {
            name: "Priority Grid".to_string(),
            kind: TemplateKind::PriorityGrid,
            zones: vec![
                Zone::new(0.0, 0.0, 0.5, 1.0),   // Main left (50%)
                Zone::new(0.5, 0.0, 0.5, 0.5),   // Top right
                Zone::new(0.5, 0.5, 0.25, 0.5),  // Bottom right left
                Zone::new(0.75, 0.5, 0.25, 0.5), // Bottom right right
            ],
            quick_key: None,
        }
    }

    /// Create a "main + side" layout (like i3/sway master-stack).
    pub fn main_plus_stack() -> Self {
        Self {
            name: "Main + Stack".to_string(),
            kind: TemplateKind::Custom,
            zones: vec![
                Zone::new(0.0, 0.0, 0.6, 1.0), // Main (60%)
                Zone::new(0.6, 0.0, 0.4, 0.5), // Stack top
                Zone::new(0.6, 0.5, 0.4, 0.5), // Stack bottom
            ],
            quick_key: None,
        }
    }

    /// All built-in template layouts.
    pub fn all_templates() -> Vec<Self> {
        vec![
            Self::no_layout(),
            Self::focus(),
            Self::default_columns(3),
            Self::default_rows(3),
            Self::grid(3, 2),
            Self::priority_grid(),
        ]
    }

    /// Build a custom layout of the given kind with `a` x `b` zones
    /// (`a` = columns for Columns/Grid, rows for Rows; `b` = grid rows).
    pub fn custom_from_kind(name: &str, kind: TemplateKind, a: usize, b: usize) -> Self {
        let a = a.clamp(1, 16);
        let b = b.clamp(1, 16);
        let mut layout = match kind {
            TemplateKind::Rows => Self::default_rows(a),
            TemplateKind::Grid => Self::grid(a, b),
            TemplateKind::Focus => Self::focus(),
            TemplateKind::PriorityGrid => Self::priority_grid(),
            TemplateKind::NoLayout => Self::no_layout(),
            TemplateKind::Columns | TemplateKind::Custom => Self::default_columns(a),
        };
        layout.name = name.to_string();
        layout.kind = kind;
        layout
    }

    /// Custom canvas layout from free-form rectangles (clamped to the screen).
    pub fn canvas(name: &str, zones: Vec<Zone>) -> Self {
        Self {
            name: name.to_string(),
            kind: TemplateKind::Custom,
            zones: zones.into_iter().map(|z| z.clamped()).collect(),
            quick_key: None,
        }
    }

    /// Find which zone a point falls into.
    pub fn zone_at(&self, px: f32, py: f32) -> Option<usize> {
        self.zones.iter().position(|z| z.contains(px, py))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_columns() {
        let layout = Layout::default_columns(3);
        assert_eq!(layout.zones.len(), 3);
        let z = &layout.zones[0];
        assert!((z.x - 0.0).abs() < 0.001);
        assert!((z.width - 1.0 / 3.0).abs() < 0.001);
    }

    #[test]
    fn zone_pixel_conversion() {
        let z = Zone::new(0.5, 0.0, 0.5, 1.0);
        let (x, y, w, h) = z.to_pixels(1920, 1080);
        assert_eq!(x, 960);
        assert_eq!(y, 0);
        assert_eq!(w, 960);
        assert_eq!(h, 1080);
    }

    #[test]
    fn zone_contains_point() {
        let z = Zone::new(0.0, 0.0, 0.5, 1.0);
        assert!(z.contains(0.25, 0.5));
        assert!(!z.contains(0.75, 0.5));
    }

    #[test]
    fn zone_at_lookup() {
        let layout = Layout::default_columns(2);
        assert_eq!(layout.zone_at(0.25, 0.5), Some(0));
        assert_eq!(layout.zone_at(0.75, 0.5), Some(1));
    }

    #[test]
    fn main_plus_stack_layout() {
        let layout = Layout::main_plus_stack();
        assert_eq!(layout.zones.len(), 3);
        assert!(layout.zone_at(0.3, 0.5) == Some(0)); // main area
    }

    #[test]
    fn grid_layout() {
        let layout = Layout::grid(2, 2);
        assert_eq!(layout.zones.len(), 4);
        assert_eq!(layout.zone_at(0.25, 0.25), Some(0));
        assert_eq!(layout.zone_at(0.75, 0.75), Some(3));
    }

    #[test]
    fn custom_layouts_and_clamp() {
        let l = Layout::custom_from_kind("My grid", TemplateKind::Grid, 2, 3);
        assert_eq!(l.zones.len(), 6);
        assert_eq!(l.name, "My grid");
        let l = Layout::custom_from_kind("R", TemplateKind::Rows, 4, 1);
        assert_eq!(l.zones.len(), 4);
        let c = Layout::canvas("C", vec![Zone::new(0.8, -0.2, 0.5, 2.0)]);
        let z = c.zones[0];
        assert!((z.y - 0.0).abs() < 1e-6);
        assert!((z.x + z.width - 1.0).abs() < 1e-6);
        assert!((z.height - 1.0).abs() < 1e-6);
    }

    #[test]
    fn layout_without_quick_key_parses() {
        let json = r#"{"name":"x","kind":"Grid","zones":[]}"#;
        let parsed: Layout = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.quick_key, None);
    }

    #[test]
    fn layout_serializable() {
        let layout = Layout::main_plus_stack();
        let json = serde_json::to_string(&layout).unwrap();
        let parsed: Layout = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.zones.len(), 3);
    }
}
