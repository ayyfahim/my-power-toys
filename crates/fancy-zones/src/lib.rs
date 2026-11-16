pub mod config;
pub mod drag;
#[cfg(feature = "gui")]
pub mod editor;
pub mod geometry;
pub mod layout;
#[cfg(feature = "gui")]
pub mod overlay;
pub mod snap;
pub mod state;
pub mod x11;
pub mod zone_window;

pub use config::FancyZonesConfig;

use anyhow::Result;
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use tracing::{info, warn};

/// Quick layout switch actions, Super+Ctrl+Alt+<1..9>.
const LAYOUT_ACTIONS: [&str; 9] = [
    "layout-1", "layout-2", "layout-3", "layout-4", "layout-5", "layout-6", "layout-7", "layout-8",
    "layout-9",
];

pub struct FancyZones {
    running: bool,
    config: config::FancyZonesConfig,
    drag_thread: Option<drag::DragThread>,
}

impl FancyZones {
    pub fn new() -> Self {
        let config = config::FancyZonesConfig::load();
        Self {
            running: false,
            config,
            drag_thread: None,
        }
    }

    pub fn config(&self) -> &config::FancyZonesConfig {
        &self.config
    }

    /// Save current config to disk.
    pub fn save_config(&self) -> Result<()> {
        mpt_common::config::save_module_config("fancy-zones", &self.config)
    }
}

impl Default for FancyZones {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for FancyZones {
    fn id(&self) -> &'static str {
        "fancy-zones"
    }

    fn name(&self) -> &'static str {
        "FancyZones"
    }

    fn description(&self) -> &'static str {
        "Advanced window tiling with custom zone layouts"
    }

    /// Default hotkey opens the editor (Super+Shift+Z).
    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(Hotkey::new(vec![Modifier::Super, Modifier::Shift], "Z"))
    }

    /// Additional hotkeys: Super+Z opens the snap overlay; optionally Super+Arrow
    /// (`override_snap_hotkeys`) and Super+Ctrl+Alt+<1..9> (`quick_layout_switch`).
    /// These are read when the daemon registers hotkeys, so changing the two
    /// flags needs a daemon restart.
    fn additional_hotkeys(&self) -> Vec<(Hotkey, &'static str)> {
        let mut keys = vec![(Hotkey::new(vec![Modifier::Super], "Z"), "snap")];
        if self.config.override_snap_hotkeys {
            keys.push((Hotkey::new(vec![Modifier::Super], "Left"), "move-left"));
            keys.push((Hotkey::new(vec![Modifier::Super], "Right"), "move-right"));
            // Upstream only overrides Win+Up/Down in position-based mode.
            if self.config.move_windows_based_on_position {
                keys.push((Hotkey::new(vec![Modifier::Super], "Up"), "move-up"));
                keys.push((Hotkey::new(vec![Modifier::Super], "Down"), "move-down"));
            }
        }
        if self.config.quick_layout_switch {
            for (i, action) in LAYOUT_ACTIONS.iter().enumerate() {
                keys.push((
                    Hotkey::new(
                        vec![Modifier::Super, Modifier::Ctrl, Modifier::Alt],
                        (i + 1).to_string(),
                    ),
                    action,
                ));
            }
        }
        keys
    }

    fn start(&mut self) -> Result<()> {
        self.config = config::FancyZonesConfig::load();
        self.running = true;
        let on_x11 = std::env::var_os("DISPLAY").is_some();
        if self.config.drag_snap_enabled && on_x11 && self.drag_thread.is_none() {
            match drag::DragThread::spawn() {
                Ok(t) => self.drag_thread = Some(t),
                Err(e) => warn!("FancyZones: drag-to-snap unavailable: {e}"),
            }
        } else if !on_x11 {
            warn!("FancyZones: no X11 display, drag-to-snap disabled");
        }
        info!(
            "FancyZones started with {} layout(s)",
            self.config.layouts.len()
        );
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        if let Some(mut t) = self.drag_thread.take() {
            t.shutdown();
        }
        info!("FancyZones stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    /// Super+Shift+Z -> open editor
    fn on_hotkey(&mut self) -> Result<()> {
        info!("FancyZones: opening editor");

        let bin = find_gui_binary();
        let result = std::process::Command::new(&bin)
            .arg("--editor")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(child) => info!("FancyZones editor spawned (pid={})", child.id()),
            Err(e) => warn!("Failed to spawn FancyZones editor ({bin:?}): {e}"),
        }
        Ok(())
    }

    /// Super+Z -> open snap overlay
    fn on_named_action(&mut self, action: &str) -> Result<()> {
        if action == "snap" {
            info!("FancyZones: opening snap overlay");

            // Capture focused window BEFORE spawning overlay (overlay steals focus)
            let window_id = x11::get_focused_window()?;

            let bin = find_gui_binary();
            let result = std::process::Command::new(&bin)
                .arg("--window-id")
                .arg(window_id.to_string())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();

            match result {
                Ok(child) => info!("FancyZones overlay spawned (pid={})", child.id()),
                Err(e) => warn!("Failed to spawn FancyZones overlay ({bin:?}): {e}"),
            }
        } else if let Some(dir) = geometry::Direction::from_action(action) {
            self.config = config::FancyZonesConfig::load();
            snap::move_active_window(&self.config, dir)?;
        } else if let Some(key) = action
            .strip_prefix("layout-")
            .and_then(|k| k.parse::<u8>().ok())
        {
            snap::quick_switch(key)?;
            self.config = config::FancyZonesConfig::load();
        } else {
            warn!("FancyZones: unknown action '{action}'");
        }
        Ok(())
    }
}

/// Find the mpt-fancy-zones binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-fancy-zones");
        if candidate.exists() {
            return candidate;
        }
    }
    // Fallback: rely on PATH
    "mpt-fancy-zones".into()
}

#[cfg(test)]
mod parity_tests;
