use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct CropAndLock {
    running: bool,
}

impl CropAndLock {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for CropAndLock {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for CropAndLock {
    fn id(&self) -> &'static str {
        "crop-and-lock"
    }

    fn name(&self) -> &'static str {
        "Crop and Lock"
    }

    fn description(&self) -> &'static str {
        "Crop a window region into a live thumbnail window"
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
