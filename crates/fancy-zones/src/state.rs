//! Small persisted runtime state shared by the daemon thread, the hotkey handlers
//! and the `mpt-fancy-zones` overlay binary:
//! - which zone(s) each live window is snapped to and its size before snapping
//!   (for keyboard navigation and `restore_size`),
//! - the last zone of each app by WM_CLASS (for `app_last_zone_move_windows`).
//!
//! Stored as JSON in `~/.config/my-power-toys/fancy-zones-state.json`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::warn;

/// Zone(s) a window or app is snapped to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZoneRef {
    /// Monitor name (or `"*"` when spanning across all monitors).
    pub monitor: String,
    /// Zone indices in the monitor's layout.
    pub zones: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSnap {
    #[serde(flatten)]
    pub zone: ZoneRef,
    /// Client size before the window was first snapped.
    #[serde(default)]
    pub original_size: Option<(u32, u32)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapState {
    /// Lowercased WM_CLASS class -> last zone.
    #[serde(default)]
    pub app_zones: HashMap<String, ZoneRef>,
    /// X11 window id -> current snap.
    #[serde(default)]
    pub windows: HashMap<u32, WindowSnap>,
}

static LOCK: Mutex<()> = Mutex::new(());

fn state_path() -> Option<PathBuf> {
    mpt_common::config::config_dir()
        .ok()
        .map(|d| d.join("fancy-zones-state.json"))
}

/// Key used for `app_zones`: the WM_CLASS class (second string), else the instance.
pub fn app_key(wm_class: &[String]) -> Option<String> {
    wm_class
        .get(1)
        .or_else(|| wm_class.first())
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
}

impl SnapState {
    pub fn load() -> Self {
        let Some(path) = state_path() else {
            return Self::default();
        };
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = state_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("json.tmp");
        let res = serde_json::to_string(self)
            .map_err(anyhow::Error::from)
            .and_then(|s| std::fs::write(&tmp, s).map_err(Into::into))
            .and_then(|_| std::fs::rename(&tmp, &path).map_err(Into::into));
        if let Err(e) = res {
            warn!("FancyZones: failed to save state {}: {e}", path.display());
        }
    }

    /// Load, modify and save under a process-wide lock.
    pub fn update<R>(f: impl FnOnce(&mut SnapState) -> R) -> R {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut s = Self::load();
        let r = f(&mut s);
        s.save();
        r
    }

    /// Record a snap. The original size is kept from the first snap so that
    /// moving between zones does not overwrite it.
    pub fn record_snap(
        &mut self,
        window: u32,
        app: Option<&str>,
        zone: ZoneRef,
        size_before: Option<(u32, u32)>,
    ) {
        let original_size = self
            .windows
            .get(&window)
            .and_then(|w| w.original_size)
            .or(size_before);
        if let Some(app) = app {
            self.app_zones.insert(app.to_string(), zone.clone());
        }
        self.windows.insert(
            window,
            WindowSnap {
                zone,
                original_size,
            },
        );
    }

    /// Forget a window's snap, returning it.
    pub fn unsnap(&mut self, window: u32) -> Option<WindowSnap> {
        self.windows.remove(&window)
    }

    /// Drop entries for windows that no longer exist.
    pub fn prune(&mut self, live: &[u32]) {
        self.windows.retain(|w, _| live.contains(w));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zr(m: &str, z: &[usize]) -> ZoneRef {
        ZoneRef {
            monitor: m.into(),
            zones: z.to_vec(),
        }
    }

    #[test]
    fn original_size_is_kept_across_snaps() {
        let mut s = SnapState::default();
        s.record_snap(1, Some("firefox"), zr("DP-1", &[0]), Some((800, 600)));
        s.record_snap(1, Some("firefox"), zr("DP-1", &[1, 2]), Some((640, 1080)));
        let w = &s.windows[&1];
        assert_eq!(w.original_size, Some((800, 600)));
        assert_eq!(w.zone.zones, vec![1, 2]);
        assert_eq!(s.app_zones["firefox"], zr("DP-1", &[1, 2]));
        assert!(s.unsnap(1).is_some());
        assert!(s.unsnap(1).is_none());
    }

    #[test]
    fn roundtrip_and_prune() {
        let mut s = SnapState::default();
        s.record_snap(7, None, zr("A", &[3]), None);
        s.record_snap(9, None, zr("A", &[1]), None);
        let json = serde_json::to_string(&s).unwrap();
        let mut back: SnapState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.windows.len(), 2);
        back.prune(&[9]);
        assert_eq!(back.windows.keys().copied().collect::<Vec<_>>(), vec![9]);
        let empty: SnapState = serde_json::from_str("{}").unwrap();
        assert!(empty.windows.is_empty());
    }

    #[test]
    fn app_key_prefers_class() {
        assert_eq!(
            app_key(&["navigator".into(), "Firefox".into()]),
            Some("firefox".into())
        );
        assert_eq!(app_key(&["xterm".into()]), Some("xterm".into()));
        assert_eq!(app_key(&[]), None);
    }
}
