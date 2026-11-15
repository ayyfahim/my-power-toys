use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct AltWindowCycle {
    running: bool,
}

impl AltWindowCycle {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for AltWindowCycle {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for AltWindowCycle {
    fn id(&self) -> &'static str {
        "alt-window-cycle"
    }

    fn name(&self) -> &'static str {
        "Alt Window Cycle"
    }

    fn description(&self) -> &'static str {
        "Cycle between windows of the same application"
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
