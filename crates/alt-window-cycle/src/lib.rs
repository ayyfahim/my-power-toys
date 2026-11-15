pub mod config;
pub mod cycle;
mod x11;

use std::time::{Duration, Instant};

use anyhow::Result;
use mpt_common::hotkey::Hotkey;
use mpt_common::module::PowerModule;
use mpt_common::platform::{self, DisplayServer};
use tracing::{info, warn};

pub use config::AltWindowCycleConfig;
use cycle::Direction;

const MODULE_ID: &str = "alt-window-cycle";

/// An in-progress cycle: the order used and the window we activated last.
struct Session {
    order: Vec<u32>,
    target: u32,
    at: Instant,
}

pub struct AltWindowCycle {
    running: bool,
    config: AltWindowCycleConfig,
    session: Option<Session>,
}

impl AltWindowCycle {
    pub fn new() -> Self {
        Self {
            running: false,
            config: load_config(),
            session: None,
        }
    }

    fn cycle(&mut self, dir: Direction) -> Result<()> {
        if !self.running {
            return Ok(());
        }
        match platform::detect_display_server() {
            DisplayServer::X11 => {}
            DisplayServer::Wayland => {
                anyhow::bail!(
                    "Alt Window Cycle is not supported on Wayland (no portable API to list and activate windows)"
                )
            }
            DisplayServer::Unknown => anyhow::bail!("no display server detected"),
        }
        self.config = load_config();

        let x = x11::X11::connect()?;
        let Some(active) = x.active_window() else {
            info!("Alt Window Cycle: no active window");
            return Ok(());
        };
        let filter = x11::Filter {
            include_minimized: self.config.include_minimized,
            all_desktops: self.config.all_desktops,
        };
        let live = x.same_app_windows(active, &filter);

        let timeout = Duration::from_millis(self.config.continue_timeout_ms);
        let previous = self
            .session
            .as_ref()
            .filter(|s| s.target == active && s.at.elapsed() <= timeout)
            .map(|s| s.order.as_slice());

        let Some((order, idx)) = cycle::plan_cycle(previous, &live, active, dir) else {
            info!("Alt Window Cycle: no other windows of this app");
            self.session = None;
            return Ok(());
        };
        let target = order[idx];
        x.activate(target, active)?;
        info!("Alt Window Cycle: activated window {target:#x} ({dir:?})");
        self.session = Some(Session {
            order,
            target,
            at: Instant::now(),
        });
        Ok(())
    }
}

fn load_config() -> AltWindowCycleConfig {
    mpt_common::config::load_module_config(MODULE_ID).unwrap_or_default()
}

fn parse_or(s: &str, fallback: &str) -> Hotkey {
    Hotkey::parse(s).unwrap_or_else(|e| {
        warn!("invalid Alt Window Cycle hotkey '{s}': {e}; using {fallback}");
        Hotkey::parse(fallback).expect("valid fallback hotkey")
    })
}

impl Default for AltWindowCycle {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for AltWindowCycle {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn name(&self) -> &'static str {
        "Alt Window Cycle"
    }

    fn description(&self) -> &'static str {
        "Cycle between windows of the same application"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(parse_or(&self.config.next_window_shortcut, "Alt+`"))
    }

    fn additional_hotkeys(&self) -> Vec<(Hotkey, &'static str)> {
        vec![(
            parse_or(&self.config.previous_window_shortcut, "Alt+Shift+`"),
            "reverse",
        )]
    }

    fn start(&mut self) -> Result<()> {
        self.config = load_config();
        self.session = None;
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        self.session = None;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        self.cycle(Direction::Forward)
    }

    fn on_named_action(&mut self, action: &str) -> Result<()> {
        match action {
            "reverse" => self.cycle(Direction::Backward),
            other => {
                warn!("Alt Window Cycle: unknown action '{other}'");
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpt_common::hotkey::Modifier;

    #[test]
    fn default_hotkeys_parse() {
        let m = AltWindowCycle {
            running: false,
            config: AltWindowCycleConfig::default(),
            session: None,
        };
        let hk = m.default_hotkey().unwrap();
        assert_eq!(hk.modifiers, vec![Modifier::Alt]);
        assert_eq!(hk.key, "`");
        let extra = m.additional_hotkeys();
        assert_eq!(extra[0].1, "reverse");
        assert_eq!(extra[0].0.modifiers, vec![Modifier::Alt, Modifier::Shift]);
    }
}
