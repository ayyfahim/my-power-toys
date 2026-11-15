use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct CommandNotFound {
    running: bool,
}

impl CommandNotFound {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for CommandNotFound {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for CommandNotFound {
    fn id(&self) -> &'static str {
        "command-not-found"
    }

    fn name(&self) -> &'static str {
        "Command Not Found"
    }

    fn description(&self) -> &'static str {
        "Suggest packages that provide a missing shell command"
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
