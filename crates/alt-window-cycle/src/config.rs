use serde::{Deserialize, Serialize};

/// Settings for Alt Window Cycle, stored in
/// `~/.config/my-power-toys/alt-window-cycle.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AltWindowCycleConfig {
    /// Hotkey that activates the next window of the focused app
    /// (upstream `next_window_shortcut`, default Alt+`).
    #[serde(default = "default_next")]
    pub next_window_shortcut: String,
    /// Hotkey that activates the previous window of the focused app
    /// (upstream `previous_window_shortcut`, default Alt+Shift+`).
    #[serde(default = "default_previous")]
    pub previous_window_shortcut: String,
    /// Include minimized (hidden) windows in the cycle.
    #[serde(default = "default_true")]
    pub include_minimized: bool,
    /// Include windows on every virtual desktop, not just the current one.
    #[serde(default)]
    pub all_desktops: bool,
    /// Repeated presses within this many milliseconds continue the same cycle
    /// (like holding Alt in upstream's switcher). After the timeout the next
    /// press starts a new cycle from the most recently used order.
    #[serde(default = "default_timeout")]
    pub continue_timeout_ms: u64,
}

fn default_next() -> String {
    "Alt+`".to_string()
}

fn default_previous() -> String {
    "Alt+Shift+`".to_string()
}

fn default_true() -> bool {
    true
}

fn default_timeout() -> u64 {
    1500
}

impl Default for AltWindowCycleConfig {
    fn default() -> Self {
        Self {
            next_window_shortcut: default_next(),
            previous_window_shortcut: default_previous(),
            include_minimized: true,
            all_desktops: false,
            continue_timeout_ms: default_timeout(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let cfg = AltWindowCycleConfig::default();
        let s = toml::to_string(&cfg).unwrap();
        let back: AltWindowCycleConfig = toml::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn partial_uses_defaults() {
        let cfg: AltWindowCycleConfig = toml::from_str("all_desktops = true").unwrap();
        assert!(cfg.all_desktops);
        assert_eq!(cfg.next_window_shortcut, "Alt+`");
        assert!(cfg.include_minimized);
    }
}
