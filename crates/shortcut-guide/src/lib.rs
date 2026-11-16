pub mod config;
#[cfg(feature = "gui")]
pub mod gui;
pub mod hold;
pub mod shortcuts;
pub mod x11;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use anyhow::Result;
use config::ShortcutGuideConfig;
use mpt_common::hotkey::Hotkey;
use mpt_common::module::PowerModule;
use tracing::{info, warn};

pub use config::{GuideTheme, WinKeyAction, WindowPosition};

/// CLI flag passed to the GUI when it was opened by holding Super and must
/// close when Super is released.
pub const ARG_CLOSE_ON_SUPER_RELEASE: &str = "--close-on-super-release";

pub struct ShortcutGuide {
    running: bool,
    config: ShortcutGuideConfig,
    hold_running: Arc<AtomicBool>,
    hold_thread: Option<JoinHandle<()>>,
}

impl ShortcutGuide {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("shortcut-guide").unwrap_or_default();
        Self {
            running: false,
            config,
            hold_running: Arc::new(AtomicBool::new(false)),
            hold_thread: None,
        }
    }

    fn reload_config(&mut self) {
        match mpt_common::config::load_module_config("shortcut-guide") {
            Ok(cfg) => self.config = cfg,
            Err(e) => warn!("Shortcut Guide: failed to load config, keeping previous: {e:#}"),
        }
    }
}

impl Default for ShortcutGuide {
    fn default() -> Self {
        Self::new()
    }
}

fn fallback_hotkey() -> Hotkey {
    Hotkey::parse(config::DEFAULT_HOTKEY).expect("default hotkey parses")
}

/// Open the overlay unless the active window is in `disabled_apps`.
/// `from_hold` is true when triggered by holding Super.
pub fn launch_overlay(config: &ShortcutGuideConfig, from_hold: bool) {
    if !config.disabled_apps.is_empty() {
        match x11::active_window_classes() {
            Ok(classes) if config.is_app_disabled(&classes) => {
                info!("Shortcut Guide: disabled for active app {classes:?}");
                return;
            }
            Ok(_) => {}
            Err(e) => warn!("Shortcut Guide: could not read active window class: {e:#}"),
        }
    }

    let bin = find_gui_binary();
    let mut cmd = std::process::Command::new(&bin);
    if from_hold && config.close_on_windows_key_release {
        cmd.arg(ARG_CLOSE_ON_SUPER_RELEASE);
    }
    let result = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match result {
        Ok(child) => info!("Shortcut Guide GUI spawned (pid={})", child.id()),
        Err(e) => warn!("Failed to spawn Shortcut Guide GUI ({bin:?}): {e}"),
    }
}

impl PowerModule for ShortcutGuide {
    fn id(&self) -> &'static str {
        "shortcut-guide"
    }

    fn name(&self) -> &'static str {
        "Shortcut Guide"
    }

    fn description(&self) -> &'static str {
        "Overlay showing available keyboard shortcuts"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        let configured = self.config.hotkey.trim();
        if configured.is_empty() {
            return Some(fallback_hotkey());
        }
        match Hotkey::parse(configured) {
            Ok(hk) => Some(hk),
            Err(e) => {
                warn!(
                    "Invalid shortcut-guide hotkey '{configured}': {e}; using {}",
                    config::DEFAULT_HOTKEY
                );
                Some(fallback_hotkey())
            }
        }
    }

    fn start(&mut self) -> Result<()> {
        self.reload_config();
        // The hold watcher always runs while the module is enabled and
        // re-reads `win_key_action` itself, so toggling it needs no restart.
        self.hold_running.store(true, Ordering::Relaxed);
        self.hold_thread = hold::spawn(Arc::clone(&self.hold_running));
        self.running = true;
        info!("Shortcut Guide module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.hold_running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.hold_thread.take() {
            let _ = handle.join();
        }
        self.running = false;
        info!("Shortcut Guide module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        info!("Shortcut Guide: launching GUI");
        self.reload_config();
        launch_overlay(&self.config, false);
        Ok(())
    }
}

fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-shortcut-guide");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-shortcut-guide".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkey_is_super_question() {
        let m = ShortcutGuide {
            running: false,
            config: ShortcutGuideConfig::default(),
            hold_running: Arc::new(AtomicBool::new(false)),
            hold_thread: None,
        };
        assert_eq!(m.default_hotkey().unwrap().to_string(), "Super+?");
    }

    #[test]
    fn configured_hotkey_used() {
        let m = ShortcutGuide {
            running: false,
            config: ShortcutGuideConfig {
                hotkey: "Super+Shift+F1".into(),
                ..Default::default()
            },
            hold_running: Arc::new(AtomicBool::new(false)),
            hold_thread: None,
        };
        assert_eq!(m.default_hotkey().unwrap().to_string(), "Super+Shift+F1");
    }
}

#[cfg(test)]
mod parity_tests;
