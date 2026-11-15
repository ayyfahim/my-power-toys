pub mod clipboard;
pub mod config;
pub mod edges;
#[cfg(feature = "gui")]
pub mod gui;
pub mod measurement;
#[cfg(feature = "gui")]
pub mod screenshot;

use anyhow::Result;
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use tracing::{info, warn};

pub use config::{MeasureStyle, ScreenRulerConfig, UnitsOfMeasure};

/// Name of the overlay binary spawned by `on_hotkey`.
pub const GUI_BINARY: &str = "mpt-screen-ruler";

pub struct ScreenRuler {
    running: bool,
}

impl ScreenRuler {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for ScreenRuler {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for ScreenRuler {
    fn id(&self) -> &'static str {
        "screen-ruler"
    }

    fn name(&self) -> &'static str {
        "Screen Ruler"
    }

    fn description(&self) -> &'static str {
        "Measure pixel distances on screen with an overlay"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(Hotkey::new(vec![Modifier::Super, Modifier::Shift], "R"))
    }

    fn start(&mut self) -> Result<()> {
        self.running = true;
        info!("Screen Ruler module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Screen Ruler module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        info!("Screen Ruler: launching overlay");
        // The binary reads the config itself, so edits apply on the next activation.
        let bin = find_gui_binary();
        match std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                info!("Screen Ruler overlay spawned (pid={})", child.id());
                // Reap the child when it exits so it doesn't linger as a zombie.
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => {
                warn!("Failed to spawn Screen Ruler overlay ({bin:?}): {e}");
                return Err(e.into());
            }
        }
        Ok(())
    }
}

/// Find the `mpt-screen-ruler` binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join(GUI_BINARY);
        if candidate.exists() {
            return candidate;
        }
    }
    // Fallback: rely on PATH
    GUI_BINARY.into()
}
