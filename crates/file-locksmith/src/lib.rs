use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct FileLocksmith {
    running: bool,
}

impl FileLocksmith {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for FileLocksmith {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for FileLocksmith {
    fn id(&self) -> &'static str {
        "file-locksmith"
    }

    fn name(&self) -> &'static str {
        "File Locksmith"
    }

    fn description(&self) -> &'static str {
        "Find and stop processes that are using a file or folder"
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
