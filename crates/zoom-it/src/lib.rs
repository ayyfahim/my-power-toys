//! ZoomIt: screen zoom, annotation, break timer, snipping and recording.

#[cfg(feature = "gui")]
pub mod capture;
pub mod config;
pub mod geometry;
#[cfg(feature = "gui")]
pub mod gui;
#[cfg(feature = "gui")]
pub mod output;
pub mod record;
pub mod screen;

#[cfg(test)]
mod parity_tests;

use anyhow::Result;
use mpt_common::hotkey::Hotkey;
use mpt_common::module::PowerModule;
use std::process::{Command, Stdio};
use tracing::{info, warn};

pub use config::ZoomItConfig;
use record::{Recorder, ToggleResult};

/// Named actions exposed through `additional_hotkeys`.
pub const ACTIONS: [&str; 6] = ["draw", "break", "livezoom", "record", "snip", "snip-save"];

pub struct ZoomIt {
    running: bool,
    config: ZoomItConfig,
    recorder: Recorder,
}

impl ZoomIt {
    pub fn new() -> Self {
        Self {
            running: false,
            config: ZoomItConfig::load(),
            recorder: Recorder::default(),
        }
    }

    fn reload_config(&mut self) {
        self.config = ZoomItConfig::load();
    }

    fn launch(&self, mode: &str) -> Result<()> {
        let bin = find_gui_binary();
        let child = Command::new(&bin)
            .args(["--mode", mode])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {bin:?}: {e}"))?;
        info!("ZoomIt: launched {mode} (pid={})", child.id());
        // Reap the child in the background so it doesn't linger as a zombie.
        std::thread::spawn(move || {
            let mut child = child;
            let _ = child.wait();
        });
        Ok(())
    }

    fn toggle_record(&mut self) -> Result<()> {
        match self.recorder.toggle(&self.config)? {
            ToggleResult::Started(p) => info!("ZoomIt: recording to {}", p.display()),
            ToggleResult::Stopped(p) => info!("ZoomIt: recording saved to {}", p.display()),
        }
        Ok(())
    }
}

impl Default for ZoomIt {
    fn default() -> Self {
        Self::new()
    }
}

fn parse_hotkey(s: &str, what: &str) -> Option<Hotkey> {
    if s.trim().is_empty() {
        return None;
    }
    match Hotkey::parse(s) {
        Ok(h) => Some(h),
        Err(e) => {
            warn!("ZoomIt: invalid {what} hotkey {s:?}: {e}");
            None
        }
    }
}

impl PowerModule for ZoomIt {
    fn id(&self) -> &'static str {
        "zoom-it"
    }

    fn name(&self) -> &'static str {
        "ZoomIt"
    }

    fn description(&self) -> &'static str {
        "Screen zoom, annotation and presentation timer"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        parse_hotkey(&self.config.toggle_key, "zoom")
    }

    fn start(&mut self) -> Result<()> {
        self.reload_config();
        self.running = true;
        info!("ZoomIt module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if self.recorder.is_recording() {
            let _ = self.recorder.stop();
        }
        self.running = false;
        info!("ZoomIt module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        self.launch("zoom")
    }

    fn additional_hotkeys(&self) -> Vec<(Hotkey, &'static str)> {
        let c = &self.config;
        [
            (&c.draw_toggle_key, "draw"),
            (&c.break_timer_key, "break"),
            (&c.live_zoom_toggle_key, "livezoom"),
            (&c.record_toggle_key, "record"),
            (&c.snip_toggle_key, "snip"),
            (&c.snip_save_toggle_key, "snip-save"),
        ]
        .into_iter()
        .filter_map(|(s, action)| parse_hotkey(s, action).map(|h| (h, action)))
        .collect()
    }

    fn on_named_action(&mut self, action: &str) -> Result<()> {
        self.reload_config();
        match action {
            "zoom" | "draw" | "break" | "livezoom" | "snip" | "snip-save" => self.launch(action),
            "record" => self.toggle_record(),
            other => {
                warn!("ZoomIt: unknown action {other:?}");
                Ok(())
            }
        }
    }
}

/// Find the mpt-zoom-it binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-zoom-it");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-zoom-it".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys() {
        let m = ZoomIt {
            running: false,
            config: ZoomItConfig::default(),
            recorder: Recorder::default(),
        };
        assert_eq!(m.default_hotkey().unwrap().to_string(), "Ctrl+1");
        let extra = m.additional_hotkeys();
        let names: Vec<_> = extra.iter().map(|(_, a)| *a).collect();
        assert_eq!(names, ACTIONS);
        assert_eq!(extra[5].0.to_string(), "Ctrl+Shift+6");
    }
}
