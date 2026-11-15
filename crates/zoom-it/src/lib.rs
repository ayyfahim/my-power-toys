use anyhow::Result;
use mpt_common::module::PowerModule;

pub struct ZoomIt {
    running: bool,
}

impl ZoomIt {
    pub fn new() -> Self {
        Self { running: false }
    }
}

impl Default for ZoomIt {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for ZoomIt {
    fn id(&self) -> &'static str {
        "zoom-it"
    }

    fn name(&self) -> &'static str {
        "ZoomIt"
    }

    fn description(&self) -> &'static str {
        "Screen zoom, annotation and presentation timer"
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
