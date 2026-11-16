pub mod border;
pub mod config;
mod x11;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc;

use anyhow::Result;
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use mpt_common::platform::{self, DisplayServer};
use tracing::{info, warn};

use border::BorderCmd;
use config::AlwaysOnTopConfig;

pub use config::AlwaysOnTopConfig as Config;

/// Named actions exposed through [`PowerModule::additional_hotkeys`].
pub const ACTION_INCREASE_OPACITY: &str = "increase-opacity";
pub const ACTION_DECREASE_OPACITY: &str = "decrease-opacity";

/// Shown when the module is used on Wayland.
const WAYLAND_UNSUPPORTED: &str = "Always on Top works on X11 only: Wayland does not let other \
apps change a window's stacking. Use your compositor's own action instead: GNOME: \
`gsettings set org.gnome.desktop.wm.keybindings toggle-above \"['<Super>t']\"` (or \
Alt+Space -> Always on Top); KDE Plasma: System Settings -> Shortcuts -> KWin -> \
\"Keep Window Above Others\"; Hyprland: the `pin` dispatcher on a floating window. \
Or log in to an \"on Xorg\" session to use this module.";

pub struct AlwaysOnTop {
    running: bool,
    config: AlwaysOnTopConfig,
    pinned: HashSet<u32>,
    /// Opacity set through the opacity hotkeys, per pinned window.
    opacity: HashMap<u32, f32>,
    border_tx: Option<mpsc::Sender<BorderCmd>>,
}

impl AlwaysOnTop {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("always-on-top").unwrap_or_default();
        Self {
            running: false,
            config,
            pinned: HashSet::new(),
            opacity: HashMap::new(),
            border_tx: None,
        }
    }

    fn toggle_active_window(&mut self) -> Result<()> {
        match platform::detect_display_server() {
            DisplayServer::X11 => self.toggle_x11(),
            DisplayServer::Wayland => {
                warn!("{WAYLAND_UNSUPPORTED}");
                anyhow::bail!(WAYLAND_UNSUPPORTED)
            }
            DisplayServer::Unknown => {
                anyhow::bail!("unknown display server, cannot toggle always-on-top")
            }
        }
    }

    fn toggle_x11(&mut self) -> Result<()> {
        let focused = x11::get_focused_window()?;

        // Check excluded apps
        if self.is_excluded(&focused.wm_class) {
            info!(
                "Window '{}' is excluded from always-on-top",
                focused.wm_class
            );
            return Ok(());
        }

        // Determine if we're pinning or unpinning based on our own state tracking.
        // We use self.pinned instead of the X11 _NET_WM_STATE_ABOVE property
        // because the WM may not have processed a prior toggle yet (race condition).
        let will_pin = !self.pinned.contains(&focused.id);

        // Upstream "Do not activate when Game Mode is on": on Linux, skip
        // pinning fullscreen windows (games, video players).
        if will_pin
            && self.config.do_not_activate_on_game_mode
            && x11::is_fullscreen(focused.id, focused.root).unwrap_or(false)
        {
            info!(
                "Window '{}' is fullscreen (game mode), not pinning",
                focused.wm_class
            );
            return Ok(());
        }

        // Explicitly add or remove the X11 state
        x11::set_always_on_top(focused.id, focused.root, will_pin)?;

        // Update tracking
        if will_pin {
            self.pinned.insert(focused.id);
            info!("Pinned window {} ({})", focused.id, focused.wm_class);
        } else {
            self.pinned.remove(&focused.id);
            // Upstream restores full opacity when a window is unpinned.
            if self.opacity.remove(&focused.id).is_some()
                && let Err(e) = x11::set_window_opacity(focused.id, focused.root, 1.0)
            {
                warn!("Failed to reset opacity of {}: {e}", focused.id);
            }
            info!("Unpinned window {} ({})", focused.id, focused.wm_class);
        }

        // Show/hide border
        if self.config.show_border
            && let Some(tx) = &self.border_tx
        {
            let cmd = if will_pin {
                BorderCmd::Add(focused.id)
            } else {
                BorderCmd::Remove(focused.id)
            };
            let _ = tx.send(cmd);
        }

        // Play sound
        if self.config.play_sound {
            play_notification_sound();
        }

        Ok(())
    }

    /// Step the opacity of the focused window, if it is pinned (upstream only
    /// changes the opacity of always-on-top windows).
    fn change_opacity(&mut self, up: bool) -> Result<()> {
        if !matches!(platform::detect_display_server(), DisplayServer::X11) {
            anyhow::bail!(WAYLAND_UNSUPPORTED);
        }
        let focused = x11::get_focused_window()?;
        if !self.pinned.contains(&focused.id) {
            info!(
                "Opacity hotkey ignored: window {} is not pinned",
                focused.id
            );
            return Ok(());
        }
        let current = self.opacity.get(&focused.id).copied().unwrap_or(1.0);
        let next = config::step_opacity(current, up);
        x11::set_window_opacity(focused.id, focused.root, next)?;
        self.opacity.insert(focused.id, next);
        info!(
            "Window {} opacity -> {}%",
            focused.id,
            (next * 100.0).round()
        );
        Ok(())
    }

    fn is_excluded(&self, wm_class: &str) -> bool {
        if wm_class.is_empty() {
            return false;
        }
        let lower = wm_class.to_lowercase();
        self.config
            .excluded_apps
            .iter()
            .any(|app| lower.contains(&app.to_lowercase()))
    }
}

impl Default for AlwaysOnTop {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for AlwaysOnTop {
    fn id(&self) -> &'static str {
        "always-on-top"
    }

    fn name(&self) -> &'static str {
        "Always on Top"
    }

    fn description(&self) -> &'static str {
        "Pin the focused window to stay always on top with a keyboard shortcut"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        let configured = self.config.hotkey.trim();
        if configured.is_empty() {
            return Some(Hotkey::new(vec![Modifier::Super], "T"));
        }
        match Hotkey::parse(configured) {
            Ok(hk) => Some(hk),
            Err(e) => {
                warn!("Invalid always-on-top hotkey '{configured}': {e}; using Super+T");
                Some(Hotkey::new(vec![Modifier::Super], "T"))
            }
        }
    }

    fn additional_hotkeys(&self) -> Vec<(Hotkey, &'static str)> {
        [
            (
                &self.config.increase_opacity_hotkey,
                ACTION_INCREASE_OPACITY,
            ),
            (
                &self.config.decrease_opacity_hotkey,
                ACTION_DECREASE_OPACITY,
            ),
        ]
        .into_iter()
        .filter(|(hk, _)| !hk.trim().is_empty())
        .filter_map(|(hk, action)| match Hotkey::parse(hk) {
            Ok(h) => Some((h, action)),
            Err(e) => {
                warn!("Invalid always-on-top {action} hotkey '{hk}': {e}");
                None
            }
        })
        .collect()
    }

    fn on_named_action(&mut self, action: &str) -> Result<()> {
        match action {
            ACTION_INCREASE_OPACITY => self.change_opacity(true),
            ACTION_DECREASE_OPACITY => self.change_opacity(false),
            other => {
                warn!("Always on Top: unknown action '{other}'");
                Ok(())
            }
        }
    }

    fn start(&mut self) -> Result<()> {
        // Reload config so settings edits apply on restart of the module.
        self.config = mpt_common::config::load_module_config("always-on-top").unwrap_or_default();
        info!("Always on Top module started");
        self.running = true;

        // Start border thread if borders are enabled and we're on X11
        if self.config.show_border
            && matches!(platform::detect_display_server(), DisplayServer::X11)
        {
            match border::spawn_border_thread(&self.config) {
                Ok(tx) => self.border_tx = Some(tx),
                Err(e) => warn!("Failed to start border thread: {e}"),
            }
        }

        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        info!("Always on Top module stopped");
        self.running = false;

        // Shutdown border thread
        if let Some(tx) = self.border_tx.take() {
            let _ = tx.send(BorderCmd::Shutdown);
        }

        // Restore opacity of windows we changed.
        if !self.opacity.is_empty()
            && let Ok(focused) = x11::get_focused_window()
        {
            for (&win, _) in self.opacity.iter() {
                let _ = x11::set_window_opacity(win, focused.root, 1.0);
            }
        }
        self.opacity.clear();
        self.pinned.clear();
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        info!("Always on Top hotkey triggered");
        self.toggle_active_window()
    }
}

/// Play a short notification sound using the system's canberra-gtk-play.
fn play_notification_sound() {
    std::thread::spawn(|| {
        let result = std::process::Command::new("canberra-gtk-play")
            .args(["-i", "bell", "-d", "Always On Top"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        if let Err(e) = result {
            // canberra-gtk-play not installed — silently ignore
            tracing::debug!("canberra-gtk-play not available: {e}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module(config: AlwaysOnTopConfig) -> AlwaysOnTop {
        AlwaysOnTop {
            running: false,
            config,
            pinned: HashSet::new(),
            opacity: HashMap::new(),
            border_tx: None,
        }
    }

    #[test]
    fn hotkeys_come_from_config() {
        let m = module(AlwaysOnTopConfig::default());
        assert_eq!(m.default_hotkey().unwrap().to_string(), "Super+T");
        let extra: Vec<_> = m
            .additional_hotkeys()
            .into_iter()
            .map(|(h, a)| (h.to_string(), a))
            .collect();
        assert_eq!(
            extra,
            vec![
                ("Super+Ctrl+Shift+=".to_string(), ACTION_INCREASE_OPACITY),
                ("Super+Ctrl+Shift+-".to_string(), ACTION_DECREASE_OPACITY),
            ]
        );

        let m = module(AlwaysOnTopConfig {
            hotkey: "Super+Ctrl+T".into(),
            increase_opacity_hotkey: String::new(),
            ..Default::default()
        });
        assert_eq!(m.default_hotkey().unwrap().to_string(), "Super+Ctrl+T");
        assert_eq!(m.additional_hotkeys().len(), 1);
    }
}
