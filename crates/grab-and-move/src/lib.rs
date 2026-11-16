pub mod config;
pub mod geometry;
mod x11;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use anyhow::Result;
use mpt_common::module::PowerModule;
use mpt_common::platform::{self, DisplayServer};
use tracing::info;

pub use config::{GrabAndMoveConfig, GrabModifier};

const MODULE_ID: &str = "grab-and-move";

pub struct GrabAndMove {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl GrabAndMove {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            thread: None,
        }
    }
}

impl Default for GrabAndMove {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for GrabAndMove {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn name(&self) -> &'static str {
        "Grab and Move"
    }

    fn description(&self) -> &'static str {
        "Move and resize windows by dragging anywhere with a modifier key"
    }

    fn start(&mut self) -> Result<()> {
        if self.is_running() {
            return Ok(());
        }
        // Clean up a thread that exited on its own (e.g. X connection lost).
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        match platform::detect_display_server() {
            DisplayServer::X11 => {}
            DisplayServer::Wayland => anyhow::bail!(
                "Grab and Move is not supported on Wayland: clients cannot grab global mouse input or move other windows; use your compositor's built-in modifier+drag instead"
            ),
            DisplayServer::Unknown => anyhow::bail!("no display server detected"),
        }
        let cfg: GrabAndMoveConfig =
            mpt_common::config::load_module_config(MODULE_ID).unwrap_or_default();
        let grabber = x11::Grabber::new(cfg)?;
        self.running.store(true, Ordering::Relaxed);
        let running = self.running.clone();
        self.thread = Some(
            std::thread::Builder::new()
                .name("grab-and-move".into())
                .spawn(move || grabber.run(running))?,
        );
        info!("Grab and Move started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

impl Drop for GrabAndMove {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
