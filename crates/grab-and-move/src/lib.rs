use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct GrabAndMove {
    running: bool,
}

impl GrabAndMove {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for GrabAndMove {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for GrabAndMove {
    fn id(&self) -> &'static str {
        "grab-and-move"
    }

    fn name(&self) -> &'static str {
        "Grab and Move"
    }

    fn description(&self) -> &'static str {
        "Move and resize windows by dragging anywhere with a modifier key"
    }

    fn start(&mut self) -> Result<()> {
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }
}
