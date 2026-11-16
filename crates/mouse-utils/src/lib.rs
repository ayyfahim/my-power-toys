pub mod config;
pub mod crosshairs;
pub mod cursor_wrap;
pub mod find_my_mouse;
pub mod gliding;
pub mod highlighter;
pub mod mouse_jump;
mod x11util;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use anyhow::Result;
use mpt_common::hotkey::Hotkey;
use mpt_common::module::PowerModule;
use tracing::{info, warn};

use config::FindMyMouseActivation;

/// Named actions exposed through [`PowerModule::additional_hotkeys`].
pub const ACTION_FIND_MY_MOUSE: &str = "find-my-mouse";
pub const ACTION_HIGHLIGHTER: &str = "highlighter";
pub const ACTION_CROSSHAIRS: &str = "crosshairs";
pub const ACTION_GLIDING_CURSOR: &str = "gliding-cursor";
pub const ACTION_CURSOR_WRAP: &str = "cursor-wrap";

/// Flags shared with the worker threads so hotkeys can toggle/trigger features.
#[derive(Default)]
struct Controls {
    find_my_mouse_trigger: Arc<AtomicBool>,
    highlighter_active: Arc<AtomicBool>,
    crosshairs_active: Arc<AtomicBool>,
    gliding_trigger: Arc<AtomicBool>,
    cursor_wrap_active: Arc<AtomicBool>,
}

fn toggle(flag: &AtomicBool) -> bool {
    !flag.fetch_xor(true, Ordering::Relaxed)
}

pub struct MouseUtils {
    running: bool,
    config: config::MouseUtilsConfig,
    stop_flags: Vec<Arc<AtomicBool>>,
    threads: Vec<thread::JoinHandle<()>>,
    controls: Controls,
}

impl MouseUtils {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("mouse-utils").unwrap_or_default();
        Self {
            running: false,
            config,
            stop_flags: Vec::new(),
            threads: Vec::new(),
            controls: Controls::default(),
        }
    }

    /// Shortcut bindings (hotkey string, action) for the enabled features.
    fn shortcut_bindings(&self) -> Vec<(&str, &'static str)> {
        let c = &self.config;
        let mut out = Vec::new();
        if c.find_my_mouse && c.find_my_mouse_activation == FindMyMouseActivation::CustomShortcut {
            out.push((c.find_my_mouse_shortcut.as_str(), ACTION_FIND_MY_MOUSE));
        }
        if c.click_highlighter {
            out.push((c.highlighter_shortcut.as_str(), ACTION_HIGHLIGHTER));
        }
        if c.crosshair {
            out.push((c.crosshair_shortcut.as_str(), ACTION_CROSSHAIRS));
        }
        if c.gliding_cursor {
            out.push((c.gliding_cursor_shortcut.as_str(), ACTION_GLIDING_CURSOR));
        }
        if c.cursor_wrap {
            out.push((c.cursor_wrap_shortcut.as_str(), ACTION_CURSOR_WRAP));
        }
        out.retain(|(hk, _)| !hk.trim().is_empty());
        out
    }

    pub fn config(&self) -> &config::MouseUtilsConfig {
        &self.config
    }
}

impl Default for MouseUtils {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for MouseUtils {
    fn id(&self) -> &'static str {
        "mouse-utils"
    }

    fn name(&self) -> &'static str {
        "Mouse Utilities"
    }

    fn description(&self) -> &'static str {
        "Find My Mouse, click highlighter, crosshairs, mouse jump, cursor wrap, gliding cursor"
    }

    fn start(&mut self) -> Result<()> {
        // Reload config in case settings changed
        self.config = mpt_common::config::load_module_config("mouse-utils").unwrap_or_default();

        info!(
            "Mouse Utilities starting (find_mouse={}, highlighter={}, crosshair={}, jump={}, wrap={}, gliding={})",
            self.config.find_my_mouse,
            self.config.click_highlighter,
            self.config.crosshair,
            self.config.mouse_jump,
            self.config.cursor_wrap,
            self.config.gliding_cursor,
        );

        // Find My Mouse
        if self.config.find_my_mouse {
            let stop = Arc::new(AtomicBool::new(false));
            let handle = find_my_mouse::spawn_with_trigger(
                self.config.clone(),
                self.controls.find_my_mouse_trigger.clone(),
                stop.clone(),
            );
            self.stop_flags.push(stop);
            self.threads.push(handle);
        }

        // Mouse Highlighter
        if self.config.click_highlighter {
            let stop = Arc::new(AtomicBool::new(false));
            self.controls
                .highlighter_active
                .store(self.config.highlighter_auto_activate, Ordering::Relaxed);
            let handle = highlighter::spawn_with_toggle(
                self.config.clone(),
                self.controls.highlighter_active.clone(),
                stop.clone(),
            );
            self.stop_flags.push(stop);
            self.threads.push(handle);
        }

        // Crosshairs
        if self.config.crosshair {
            let stop = Arc::new(AtomicBool::new(false));
            self.controls
                .crosshairs_active
                .store(self.config.crosshair_auto_activate, Ordering::Relaxed);
            let handle = crosshairs::spawn_with_toggle(
                self.config.clone(),
                self.controls.crosshairs_active.clone(),
                stop.clone(),
            );
            self.stop_flags.push(stop);
            self.threads.push(handle);
        }

        // Cursor Wrap
        if self.config.cursor_wrap {
            let stop = Arc::new(AtomicBool::new(false));
            self.controls
                .cursor_wrap_active
                .store(self.config.cursor_wrap_auto_activate, Ordering::Relaxed);
            let handle = cursor_wrap::spawn(
                self.config.clone(),
                self.controls.cursor_wrap_active.clone(),
                stop.clone(),
            );
            self.stop_flags.push(stop);
            self.threads.push(handle);
        }

        // Mouse Jump grabs its own shortcut inside its thread.
        if self.config.mouse_jump {
            let stop = Arc::new(AtomicBool::new(false));
            let handle = mouse_jump::spawn(self.config.clone(), stop.clone());
            self.stop_flags.push(stop);
            self.threads.push(handle);
        }
        if self.config.gliding_cursor {
            let stop = Arc::new(AtomicBool::new(false));
            self.controls
                .gliding_trigger
                .store(false, Ordering::Relaxed);
            let handle = gliding::spawn(
                self.config.clone(),
                self.controls.gliding_trigger.clone(),
                stop.clone(),
            );
            self.stop_flags.push(stop);
            self.threads.push(handle);
        }

        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        info!("Mouse Utilities stopping...");

        // Signal all threads to stop
        for flag in &self.stop_flags {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        // Join all threads
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
        self.stop_flags.clear();

        self.running = false;
        info!("Mouse Utilities stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        info!("Mouse Utilities: hotkey triggered");
        Ok(())
    }

    fn additional_hotkeys(&self) -> Vec<(Hotkey, &'static str)> {
        self.shortcut_bindings()
            .into_iter()
            .filter_map(|(hk, action)| match Hotkey::parse(hk) {
                Ok(h) => Some((h, action)),
                Err(e) => {
                    warn!("Mouse Utilities: invalid shortcut '{hk}' for {action}: {e}");
                    None
                }
            })
            .collect()
    }

    fn on_named_action(&mut self, action: &str) -> Result<()> {
        let c = &self.controls;
        match action {
            ACTION_FIND_MY_MOUSE => c.find_my_mouse_trigger.store(true, Ordering::Relaxed),
            ACTION_GLIDING_CURSOR => c.gliding_trigger.store(true, Ordering::Relaxed),
            ACTION_HIGHLIGHTER => {
                let on = toggle(&c.highlighter_active);
                info!("Mouse Highlighter toggled: {on}");
            }
            ACTION_CROSSHAIRS => {
                let on = toggle(&c.crosshairs_active);
                info!("Crosshairs toggled: {on}");
            }
            ACTION_CURSOR_WRAP => {
                let on = toggle(&c.cursor_wrap_active);
                info!("Cursor Wrap toggled: {on}");
            }
            other => warn!("Mouse Utilities: unknown action '{other}'"),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(config: config::MouseUtilsConfig) -> MouseUtils {
        MouseUtils {
            running: false,
            config,
            stop_flags: Vec::new(),
            threads: Vec::new(),
            controls: Controls::default(),
        }
    }

    #[test]
    fn hotkeys_follow_enabled_features() {
        let m = module(config::MouseUtilsConfig::default());
        // Defaults: only Find My Mouse is enabled, with double-Ctrl activation.
        assert!(m.additional_hotkeys().is_empty());

        let m = module(config::MouseUtilsConfig {
            find_my_mouse_activation: FindMyMouseActivation::CustomShortcut,
            crosshair: true,
            gliding_cursor: true,
            cursor_wrap: true,
            click_highlighter: true,
            ..Default::default()
        });
        let actions: Vec<_> = m
            .additional_hotkeys()
            .into_iter()
            .map(|(h, a)| (h.to_string(), a))
            .collect();
        assert!(actions.contains(&("Super+Shift+F".into(), ACTION_FIND_MY_MOUSE)));
        assert!(actions.contains(&("Super+Shift+H".into(), ACTION_HIGHLIGHTER)));
        assert!(actions.contains(&("Super+Alt+P".into(), ACTION_CROSSHAIRS)));
        assert!(actions.contains(&("Super+Alt+.".into(), ACTION_GLIDING_CURSOR)));
        assert!(actions.contains(&("Super+Alt+U".into(), ACTION_CURSOR_WRAP)));
    }

    #[test]
    fn named_actions_toggle_flags() {
        let mut m = module(config::MouseUtilsConfig::default());
        m.on_named_action(ACTION_CROSSHAIRS).unwrap();
        assert!(m.controls.crosshairs_active.load(Ordering::Relaxed));
        m.on_named_action(ACTION_CROSSHAIRS).unwrap();
        assert!(!m.controls.crosshairs_active.load(Ordering::Relaxed));
        m.on_named_action(ACTION_FIND_MY_MOUSE).unwrap();
        assert!(m.controls.find_my_mouse_trigger.load(Ordering::Relaxed));
    }
}
