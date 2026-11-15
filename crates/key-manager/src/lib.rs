pub mod actions;
pub mod engine;
pub mod focus;
pub mod keymap;
pub mod mapping;
pub mod worker;

use anyhow::{Context, Result};
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub use mapping::{KeyAction, KeyCombo, KeyMapping, KeyTrigger};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KeyManagerConfig {
    #[serde(default)]
    pub mappings: Vec<KeyMapping>,
}

pub struct KeyManager {
    running: bool,
    config: KeyManagerConfig,
    worker: Option<worker::Worker>,
}

impl KeyManager {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("key-manager").unwrap_or_default();
        Self {
            running: false,
            config,
            worker: None,
        }
    }

    pub fn config(&self) -> &KeyManagerConfig {
        &self.config
    }

    pub fn add_mapping(&mut self, mapping: KeyMapping) {
        self.config.mappings.push(mapping);
    }

    pub fn remove_mapping(&mut self, index: usize) -> Result<()> {
        if index >= self.config.mappings.len() {
            anyhow::bail!("mapping index {index} out of range");
        }
        self.config.mappings.remove(index);
        Ok(())
    }

    pub fn save_config(&self) -> Result<()> {
        mpt_common::config::save_module_config("key-manager", &self.config)
    }

    fn reload_config(&mut self) {
        match mpt_common::config::load_module_config::<KeyManagerConfig>("key-manager") {
            Ok(c) => self.config = c,
            Err(e) => warn!("Key Manager: cannot load config ({e:#}); keeping previous mappings"),
        }
    }

    /// (Re)start the input pipeline for the current config.
    fn start_worker(&mut self) -> Result<()> {
        self.stop_worker();
        let (rules, warnings) = engine::Rules::compile(&self.config.mappings);
        for w in &warnings {
            warn!("Key Manager: {w}");
        }
        if rules.is_empty() {
            info!("Key Manager: no enabled mappings, standing by");
            return Ok(());
        }
        let w = worker::start(rules)?;
        self.worker = Some(w);
        info!(
            "Key Manager: input pipeline running with {} enabled mapping(s)",
            self.config.mappings.iter().filter(|m| m.enabled).count()
        );
        Ok(())
    }

    fn stop_worker(&mut self) {
        if let Some(w) = self.worker.take() {
            w.stop();
        }
    }

    /// Whether the evdev/uinput worker thread is active.
    pub fn is_intercepting(&self) -> bool {
        self.worker.as_ref().is_some_and(worker::Worker::is_alive)
    }
}

impl Default for KeyManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Settings app binary (`mpt-settings`), next to the current exe or on PATH.
fn find_settings_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-settings");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-settings".into()
}

impl PowerModule for KeyManager {
    fn id(&self) -> &'static str {
        "key-manager"
    }

    fn name(&self) -> &'static str {
        "Key Manager"
    }

    fn description(&self) -> &'static str {
        "Remap keys and shortcuts, send text, launch apps, per-app rules"
    }

    fn start(&mut self) -> Result<()> {
        self.reload_config();
        self.start_worker()?;
        self.running = true;
        info!("Key Manager module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.stop_worker();
        self.running = false;
        info!("Key Manager module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    /// Opens the settings app, which hosts the key mapping editor.
    /// `mpt-settings` has no CLI argument to open a specific page.
    fn on_hotkey(&mut self) -> Result<()> {
        let bin = find_settings_binary();
        let mut child = std::process::Command::new(&bin)
            .spawn()
            .with_context(|| format!("failed to launch {}", bin.display()))?;
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    /// `reload`: re-read the config and restart the input pipeline.
    fn on_named_action(&mut self, action: &str) -> Result<()> {
        match action {
            "reload" => {
                self.reload_config();
                if self.running {
                    self.start_worker()?;
                }
                info!("Key Manager: configuration reloaded");
                Ok(())
            }
            other => anyhow::bail!("unknown key-manager action: {other}"),
        }
    }
}

impl Drop for KeyManager {
    fn drop(&mut self) {
        self.stop_worker();
    }
}
