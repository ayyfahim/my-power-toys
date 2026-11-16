use serde::{Deserialize, Serialize};

/// Modifier that must be held while dragging (upstream `modifierKey`: 0 = Alt, 1 = Win).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GrabModifier {
    #[default]
    Alt,
    Super,
}

/// Settings for Grab and Move, stored in
/// `~/.config/my-power-toys/grab-and-move.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrabAndMoveConfig {
    /// Modifier held while dragging (upstream `modifierKey`).
    #[serde(default)]
    pub modifier_key: GrabModifier,
    /// Modifier + right-drag resizes the window (upstream `useAltResize`).
    #[serde(default = "default_true")]
    pub use_alt_resize: bool,
    /// Do nothing while the active window is fullscreen
    /// (Linux stand-in for upstream `doNotActivateOnGameMode`).
    #[serde(default = "default_true")]
    pub do_not_activate_on_game_mode: bool,
    /// Windows that are never moved/resized (upstream `excluded_apps`).
    /// Matched case-insensitively against WM_CLASS instance/class, the
    /// executable name, or as a substring of the window title.
    #[serde(default)]
    pub excluded_apps: Vec<String>,
    /// While one of these apps is the active window, modifier+click is passed
    /// through to it untouched (upstream `excluded_apps_for_modifier`).
    #[serde(default)]
    pub excluded_apps_for_modifier: Vec<String>,
    /// Pixels the pointer must travel before a drag starts.
    #[serde(default = "default_threshold")]
    pub drag_threshold: i32,
}

fn default_true() -> bool {
    true
}

fn default_threshold() -> i32 {
    4
}

impl Default for GrabAndMoveConfig {
    fn default() -> Self {
        Self {
            modifier_key: GrabModifier::Alt,
            use_alt_resize: true,
            do_not_activate_on_game_mode: true,
            excluded_apps: Vec::new(),
            excluded_apps_for_modifier: Vec::new(),
            drag_threshold: default_threshold(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cfg = GrabAndMoveConfig {
            modifier_key: GrabModifier::Super,
            excluded_apps: vec!["gimp".into()],
            ..Default::default()
        };
        let s = toml::to_string(&cfg).unwrap();
        assert!(s.contains("modifier_key = \"super\""));
        let back: GrabAndMoveConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn empty_is_default() {
        let cfg: GrabAndMoveConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, GrabAndMoveConfig::default());
    }
}
