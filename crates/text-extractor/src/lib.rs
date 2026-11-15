pub mod ocr;
#[cfg(feature = "gui")]
pub mod overlay;
#[cfg(feature = "gui")]
pub mod screenshot;

use anyhow::Result;
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub use ocr::OcrMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextExtractorConfig {
    /// Tesseract language (e.g. "eng", "fra", "eng+fra").
    /// Upstream `PreferredLanguage`; preselected in the capture toolbar.
    #[serde(default = "default_lang")]
    pub language: String,
    /// Initial state of the "single line" toggle (join lines with spaces).
    #[serde(default)]
    pub single_line: bool,
    /// Initial state of the "table" toggle (tab-separated columns).
    #[serde(default)]
    pub table_mode: bool,
}

fn default_lang() -> String {
    "eng".to_string()
}

impl Default for TextExtractorConfig {
    fn default() -> Self {
        Self {
            language: default_lang(),
            single_line: false,
            table_mode: false,
        }
    }
}

impl TextExtractorConfig {
    pub fn mode(&self) -> OcrMode {
        OcrMode {
            single_line: self.single_line,
            table: self.table_mode,
        }
    }
}

pub struct TextExtractor {
    running: bool,
    config: TextExtractorConfig,
}

impl TextExtractor {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("text-extractor").unwrap_or_default();
        Self {
            running: false,
            config,
        }
    }

    fn run_headless(&self) -> Result<()> {
        let text = ocr::capture_and_extract_with(&self.config.language, self.config.mode())?;
        if text.is_empty() {
            info!("No text detected");
        } else {
            info!("Extracted {} chars, copying to clipboard", text.len());
            ocr::copy_to_clipboard(&text)?;
        }
        Ok(())
    }
}

impl Default for TextExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for TextExtractor {
    fn id(&self) -> &'static str {
        "text-extractor"
    }

    fn name(&self) -> &'static str {
        "Text Extractor"
    }

    fn description(&self) -> &'static str {
        "Extract text from any screen region using OCR (requires tesseract)"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(Hotkey::new(vec![Modifier::Super, Modifier::Shift], "T"))
    }

    fn start(&mut self) -> Result<()> {
        self.running = true;
        info!("Text Extractor started (lang={})", self.config.language);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Text Extractor stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        self.config = mpt_common::config::load_module_config("text-extractor").unwrap_or_default();

        if let Some(bin) = find_gui_binary() {
            match std::process::Command::new(&bin)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
            {
                Ok(child) => {
                    info!("Text Extractor overlay spawned (pid={})", child.id());
                    return Ok(());
                }
                Err(e) => warn!("Failed to spawn Text Extractor overlay ({bin:?}): {e}"),
            }
        }

        info!("Text Extractor: capturing screen region for OCR (headless)");
        self.run_headless()
    }
}

/// Find the mpt-text-extractor binary next to the current executable, else on PATH.
fn find_gui_binary() -> Option<std::path::PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-text-extractor");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join("mpt-text-extractor"))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_written_config_still_parses() {
        let cfg: TextExtractorConfig = toml::from_str("language = \"fra\"\n").unwrap();
        assert_eq!(cfg.language, "fra");
        assert_eq!(cfg.mode(), OcrMode::default());
    }

    #[test]
    fn round_trip() {
        let cfg = TextExtractorConfig {
            language: "eng+deu".into(),
            single_line: true,
            table_mode: true,
        };
        let back: TextExtractorConfig = toml::from_str(&toml::to_string(&cfg).unwrap()).unwrap();
        assert_eq!(back.language, "eng+deu");
        assert!(back.mode().single_line && back.mode().table);
    }
}
