use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct NewPlus {
    running: bool,
}

impl NewPlus {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for NewPlus {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for NewPlus {
    fn id(&self) -> &'static str {
        "new-plus"
    }

    fn name(&self) -> &'static str {
        "New+"
    }

    fn description(&self) -> &'static str {
        "Create files and folders from personal templates"
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
