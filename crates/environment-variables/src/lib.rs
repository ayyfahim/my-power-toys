use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct EnvironmentVariables {
    running: bool,
}

impl EnvironmentVariables {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for EnvironmentVariables {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for EnvironmentVariables {
    fn id(&self) -> &'static str {
        "environment-variables"
    }

    fn name(&self) -> &'static str {
        "Environment Variables"
    }

    fn description(&self) -> &'static str {
        "Manage user environment variables and profiles"
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
