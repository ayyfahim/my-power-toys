use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct PowerDisplay {
    running: bool,
}

impl PowerDisplay {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for PowerDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for PowerDisplay {
    fn id(&self) -> &'static str {
        "power-display"
    }

    fn name(&self) -> &'static str {
        "Power Display"
    }

    fn description(&self) -> &'static str {
        "Control monitor brightness, contrast and volume"
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
