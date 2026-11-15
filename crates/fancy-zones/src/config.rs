use crate::layout::Layout;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// FancyZones configuration (`~/.config/my-power-toys/fancy-zones.toml`).
///
/// Every field has a serde default so older config files keep loading.
/// Field names follow the upstream `FZConfigProperties` settings where one exists
/// (e.g. `fancyzones_shiftDrag` -> `shift_drag`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FancyZonesConfig {
    /// Gap between zones in pixels.
    #[serde(default = "default_gap")]
    pub zone_gap: u32,

    /// Available layouts (the template palette followed by custom layouts).
    #[serde(default = "default_layouts")]
    pub layouts: Vec<Layout>,

    /// Per-monitor layout assignment: monitor name (e.g. "DP-1") -> index in `layouts`.
    #[serde(default)]
    pub monitor_layouts: HashMap<String, usize>,

    /// Default layout index used when a monitor has no specific assignment.
    #[serde(default = "default_layout_idx")]
    pub default_layout: usize,

    /// Index of the most recently activated layout (editor click or quick layout
    /// switch). Older versions used this as the single global layout: when a config
    /// has a non-zero `active_layout` but no per-monitor assignments, it is migrated
    /// into `default_layout` by [`FancyZonesConfig::normalize`].
    #[serde(default)]
    pub active_layout: usize,

    // ── Drag-to-snap ────────────────────────────────────────────────────────
    /// Enable the background drag-to-snap / window-tracking thread (X11 only).
    #[serde(default = "default_true")]
    pub drag_snap_enabled: bool,

    /// Hold Shift while dragging a window to show zones (upstream `fancyzones_shiftDrag`).
    /// When false, zones show on every drag and Shift hides them.
    #[serde(default = "default_true")]
    pub shift_drag: bool,

    /// Pressing a non-primary (right) mouse button while dragging toggles zone
    /// activation (upstream `fancyzones_mouseSwitch`).
    #[serde(default)]
    pub mouse_switch: bool,

    /// Hold Ctrl while dragging to span the window across multiple zones.
    #[serde(default = "default_true")]
    pub multi_zone_span: bool,

    /// Middle mouse click during a drag toggles multi-zone spanning
    /// (upstream `fancyzones_mouseMiddleClickSpanningMultipleZones`).
    #[serde(default)]
    pub mouse_middle_click_spanning_multiple_zones: bool,

    /// Which zone wins when the pointer is over overlapping zones
    /// (upstream `fancyzones_overlappingZonesAlgorithm`):
    /// 0 = smallest, 1 = largest, 2 = positional, 3 = closest center.
    #[serde(default)]
    pub overlapping_zones_algorithm: u8,

    /// Make the dragged window semi-transparent while zones are shown
    /// (upstream `fancyzones_makeDraggedWindowTransparent`). Needs a compositor.
    #[serde(default)]
    pub make_dragged_window_transparent: bool,

    /// Show zones on all monitors while dragging, not only the one under the cursor.
    #[serde(default)]
    pub show_on_all_monitors: bool,

    /// Treat all monitors as one large canvas (layout of the primary monitor is
    /// stretched over the bounding box of all monitors).
    #[serde(default)]
    pub span_zones_across_monitors: bool,

    /// Restore the original window size when it is dragged out of a zone
    /// (upstream `fancyzones_restoreSize`).
    #[serde(default)]
    pub restore_size: bool,

    /// Move newly opened windows to the zone their app was last snapped to
    /// (upstream `fancyzones_appLastZone_moveWindows`).
    #[serde(default)]
    pub app_last_zone_move_windows: bool,

    /// Apps excluded from snapping, matched case-insensitively against WM_CLASS
    /// (upstream `fancyzones_excluded_apps`).
    #[serde(default)]
    pub excluded_apps: Vec<String>,

    // ── Keyboard ────────────────────────────────────────────────────────────
    /// Override Super+Arrow to move windows between zones
    /// (upstream `fancyzones_overrideSnapHotkeys`). Read at daemon start.
    #[serde(default)]
    pub override_snap_hotkeys: bool,

    /// Super+Arrow may move a window to the next monitor
    /// (upstream `fancyzones_moveWindowAcrossMonitors`).
    #[serde(default)]
    pub move_windows_across_monitors: bool,

    /// Super+Arrow moves by relative zone position instead of zone index; also
    /// enables Super+Up/Down (upstream `fancyzones_moveWindowsBasedOnPosition`).
    #[serde(default)]
    pub move_windows_based_on_position: bool,

    /// Super+Ctrl+Alt+<1..9> switches the layout of the monitor under the cursor
    /// (upstream `fancyzones_quickLayoutSwitch`). Read at daemon start.
    #[serde(default = "default_true")]
    pub quick_layout_switch: bool,

    /// Flash the zones and layout name after a quick layout switch
    /// (upstream `fancyzones_flashZonesOnQuickSwitch`).
    #[serde(default = "default_true")]
    pub flash_zones_on_quick_switch: bool,

    /// How long the flash is shown, in milliseconds
    /// (upstream `fancyzones_layoutNameLabelDuration`).
    #[serde(default = "default_flash_ms")]
    pub flash_duration_ms: u64,

    // ── Appearance ──────────────────────────────────────────────────────────
    /// Fill color of the zone(s) under the cursor (upstream `fancyzones_zoneHighlightColor`).
    #[serde(default = "default_highlight_color")]
    pub zone_highlight_color: String,

    /// Fill color of the other zones (upstream `fancyzones_zoneColor`).
    #[serde(default = "default_inactive_color")]
    pub zone_inactive_color: String,

    /// Zone border color (upstream `fancyzones_zoneBorderColor`).
    #[serde(default = "default_border_color")]
    pub zone_border_color: String,

    /// Zone number color (upstream `fancyzones_zoneNumberColor`).
    #[serde(default = "default_number_color")]
    pub zone_number_color: String,

    /// Zone fill opacity in percent, 0-100 (upstream `fancyzones_highlight_opacity`).
    #[serde(default = "default_opacity")]
    pub zone_opacity: u8,

    /// Draw zone numbers (upstream `fancyzones_showZoneNumber`).
    #[serde(default = "default_true")]
    pub show_zone_number: bool,
}

fn default_gap() -> u32 {
    8
}

fn default_layouts() -> Vec<Layout> {
    Layout::all_templates()
}

fn default_layout_idx() -> usize {
    2 // "3 Columns"
}

fn default_true() -> bool {
    true
}

fn default_flash_ms() -> u64 {
    1500
}

fn default_highlight_color() -> String {
    "#0078D7".to_string()
}

fn default_inactive_color() -> String {
    "#F5FCFF".to_string()
}

fn default_border_color() -> String {
    "#FFFFFF".to_string()
}

fn default_number_color() -> String {
    "#000000".to_string()
}

fn default_opacity() -> u8 {
    50
}

impl FancyZonesConfig {
    /// Load from disk, falling back to defaults, and apply [`Self::normalize`].
    pub fn load() -> Self {
        let mut cfg: Self =
            mpt_common::config::load_module_config("fancy-zones").unwrap_or_default();
        cfg.normalize();
        cfg
    }

    /// Fix up values from older or hand-edited configs.
    pub fn normalize(&mut self) {
        if self.layouts.is_empty() {
            self.layouts = default_layouts();
        }
        // Legacy single-layout configs: honour `active_layout` as the global layout.
        if self.active_layout != 0
            && self.active_layout < self.layouts.len()
            && self.monitor_layouts.is_empty()
        {
            self.default_layout = self.active_layout;
        }
        if self.default_layout >= self.layouts.len() {
            self.default_layout = 0;
        }
        self.zone_opacity = self.zone_opacity.min(100);
    }

    /// Index of the layout assigned to a monitor, falling back to `default_layout`.
    pub fn layout_index_for_monitor(&self, monitor_name: &str) -> usize {
        self.monitor_layouts
            .get(monitor_name)
            .copied()
            .unwrap_or(self.default_layout)
    }

    /// Get the layout assigned to a specific monitor, falling back to default.
    pub fn layout_for_monitor(&self, monitor_name: &str) -> Option<&Layout> {
        self.layouts
            .get(self.layout_index_for_monitor(monitor_name))
    }

    /// Assign a layout to a monitor and record it as the active layout.
    pub fn set_monitor_layout(&mut self, monitor_name: &str, idx: usize) {
        self.monitor_layouts.insert(monitor_name.to_string(), idx);
        self.active_layout = idx;
    }

    /// Resolve a quick-switch key (1-9): a layout whose `quick_key` matches wins,
    /// otherwise the key is used as a 1-based index into `layouts`.
    pub fn layout_for_quick_key(&self, key: u8) -> Option<usize> {
        if let Some(i) = self.layouts.iter().position(|l| l.quick_key == Some(key)) {
            return Some(i);
        }
        let idx = (key as usize).checked_sub(1)?;
        (idx < self.layouts.len()).then_some(idx)
    }

    /// Remove a layout and shift every stored index that pointed past it.
    /// Monitors that used the removed layout fall back to `default_layout`.
    pub fn remove_layout(&mut self, idx: usize) {
        if idx >= self.layouts.len() || self.layouts.len() == 1 {
            return;
        }
        self.layouts.remove(idx);
        let fix = |v: usize| if v > idx { v - 1 } else { v };
        self.monitor_layouts.retain(|_, v| *v != idx);
        for v in self.monitor_layouts.values_mut() {
            *v = fix(*v);
        }
        self.default_layout = if self.default_layout == idx {
            0
        } else {
            fix(self.default_layout)
        };
        self.active_layout = if self.active_layout == idx {
            self.default_layout
        } else {
            fix(self.active_layout)
        };
    }

    /// Whether a window with the given WM_CLASS strings is excluded from snapping.
    pub fn is_excluded(&self, wm_class: &[String]) -> bool {
        crate::geometry::is_excluded(wm_class, &self.excluded_apps)
    }
}

impl Default for FancyZonesConfig {
    fn default() -> Self {
        Self {
            zone_gap: default_gap(),
            layouts: default_layouts(),
            monitor_layouts: HashMap::new(),
            default_layout: default_layout_idx(),
            active_layout: 0,
            drag_snap_enabled: true,
            shift_drag: true,
            mouse_switch: false,
            multi_zone_span: true,
            mouse_middle_click_spanning_multiple_zones: false,
            overlapping_zones_algorithm: 0,
            make_dragged_window_transparent: false,
            show_on_all_monitors: false,
            span_zones_across_monitors: false,
            restore_size: false,
            app_last_zone_move_windows: false,
            excluded_apps: Vec::new(),
            override_snap_hotkeys: false,
            move_windows_across_monitors: false,
            move_windows_based_on_position: false,
            quick_layout_switch: true,
            flash_zones_on_quick_switch: true,
            flash_duration_ms: default_flash_ms(),
            zone_highlight_color: default_highlight_color(),
            zone_inactive_color: default_inactive_color(),
            zone_border_color: default_border_color(),
            zone_number_color: default_number_color(),
            zone_opacity: default_opacity(),
            show_zone_number: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_roundtrip() {
        let config = FancyZonesConfig::default();
        let s = toml::to_string_pretty(&config).unwrap();
        let parsed: FancyZonesConfig = toml::from_str(&s).unwrap();
        assert_eq!(parsed.zone_gap, 8);
        assert_eq!(parsed.layouts.len(), 6);
        assert!(parsed.shift_drag);
        assert!(!parsed.override_snap_hotkeys);
        assert_eq!(parsed.zone_highlight_color, "#0078D7");
    }

    #[test]
    fn empty_toml_gives_defaults() {
        let parsed: FancyZonesConfig = toml::from_str("").unwrap();
        assert_eq!(parsed.zone_gap, 8);
        assert_eq!(parsed.layouts.len(), 6);
        assert_eq!(parsed.default_layout, 2);
        assert!(parsed.quick_layout_switch);
        assert_eq!(parsed.zone_opacity, 50);
    }

    #[test]
    fn old_config_still_loads() {
        let old = "zone_gap = 4\ndefault_layout = 1\n\n[monitor_layouts]\nDP-1 = 4\n";
        let parsed: FancyZonesConfig = toml::from_str(old).unwrap();
        assert_eq!(parsed.zone_gap, 4);
        assert_eq!(parsed.default_layout, 1);
        assert_eq!(parsed.monitor_layouts.get("DP-1"), Some(&4));
        assert!(parsed.shift_drag);
    }

    #[test]
    fn legacy_active_layout_migrates() {
        let mut parsed: FancyZonesConfig = toml::from_str("active_layout = 4").unwrap();
        parsed.normalize();
        assert_eq!(parsed.default_layout, 4);
        assert_eq!(parsed.layout_for_monitor("X").unwrap().name, "3x2 Grid");

        // Explicit per-monitor assignments win over the legacy field.
        let mut parsed: FancyZonesConfig =
            toml::from_str("active_layout = 4\n[monitor_layouts]\nDP-1 = 1\n").unwrap();
        parsed.normalize();
        assert_eq!(parsed.default_layout, 2);
    }

    #[test]
    fn per_monitor_layout() {
        let mut config = FancyZonesConfig::default();
        config.monitor_layouts.insert("DP-1".to_string(), 4); // Grid
        config.monitor_layouts.insert("HDMI-1".to_string(), 1); // Focus

        let dp1 = config.layout_for_monitor("DP-1").unwrap();
        assert_eq!(dp1.name, "3x2 Grid");

        let hdmi = config.layout_for_monitor("HDMI-1").unwrap();
        assert_eq!(hdmi.name, "Focus");

        // Unknown monitor falls back to default_layout
        let unknown = config.layout_for_monitor("VGA-1").unwrap();
        assert_eq!(unknown.name, "3 Columns");
    }

    #[test]
    fn quick_key_lookup() {
        let mut config = FancyZonesConfig::default();
        assert_eq!(config.layout_for_quick_key(2), Some(1));
        assert_eq!(config.layout_for_quick_key(9), None);
        let mut custom = Layout::grid(2, 2);
        custom.quick_key = Some(2);
        config.layouts.push(custom);
        assert_eq!(config.layout_for_quick_key(2), Some(6));
    }

    #[test]
    fn remove_layout_fixes_indices() {
        let mut config = FancyZonesConfig::default();
        config.layouts.push(Layout::grid(2, 2)); // 6
        config.layouts.push(Layout::grid(4, 4)); // 7
        config.monitor_layouts.insert("A".into(), 6);
        config.monitor_layouts.insert("B".into(), 7);
        config.default_layout = 7;
        config.active_layout = 7;
        config.remove_layout(6);
        assert_eq!(config.layouts.len(), 7);
        assert!(!config.monitor_layouts.contains_key("A"));
        assert_eq!(config.monitor_layouts.get("B"), Some(&6));
        assert_eq!(config.default_layout, 6);
        assert_eq!(config.active_layout, 6);
    }
}
