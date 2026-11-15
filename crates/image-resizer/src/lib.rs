mod resizer;

#[cfg(feature = "gui")]
pub mod gui;

use anyhow::Result;
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub use resizer::{
    FallbackEncoder, OutputFormat, ResizeFit, ResizeOptions, ResizePlan, ResizePreset, ResizeUnit,
    SizePreset, compute_dimensions, default_custom_size, default_sizes, format_file_name,
    resize_batch, resize_image, resize_image_with,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResizerConfig {
    /// Legacy preset selector (still written by the settings UI). Used when
    /// `selected_size` is empty or doesn't match any entry in `sizes`.
    #[serde(default = "default_preset")]
    pub preset: ResizePreset,
    #[serde(default = "default_format")]
    pub output_format: OutputFormat,
    /// JPEG quality (upstream `ImageresizerJpegQualityLevel`).
    #[serde(default = "default_quality")]
    pub quality: u8,
    /// User-editable size presets (upstream `ImageresizerSizes`).
    #[serde(default = "default_sizes")]
    pub sizes: Vec<SizePreset>,
    /// Name of the selected entry in `sizes`, or `"Custom"` for `custom_size`.
    #[serde(default)]
    pub selected_size: String,
    /// Upstream `ImageresizerCustomSize`.
    #[serde(default = "default_custom_size")]
    pub custom_size: SizePreset,
    /// Upstream `ImageresizerShrinkOnly`: never enlarge images.
    #[serde(default = "default_true")]
    pub shrink_only: bool,
    /// Upstream `ImageresizerReplace`: overwrite the original files.
    #[serde(default)]
    pub replace: bool,
    /// Upstream `ImageresizerIgnoreOrientation`: swap the target box for
    /// portrait/landscape mismatches.
    #[serde(default = "default_true")]
    pub ignore_orientation: bool,
    /// Upstream `ImageresizerKeepDateModified`.
    #[serde(default)]
    pub keep_date_modified: bool,
    /// Upstream `ImageresizerFileName` (`%1`..`%6`).
    #[serde(default = "default_file_name")]
    pub file_name_template: String,
    /// Upstream `ImageresizerFallbackEncoder`.
    #[serde(default)]
    pub fallback_encoder: FallbackEncoder,
}

fn default_true() -> bool {
    true
}

fn default_file_name() -> String {
    "%1 (%2)".into()
}

fn default_preset() -> ResizePreset {
    ResizePreset::Medium
}

fn default_format() -> OutputFormat {
    OutputFormat::Original
}

fn default_quality() -> u8 {
    85
}

impl Default for ImageResizerConfig {
    fn default() -> Self {
        Self {
            preset: default_preset(),
            output_format: default_format(),
            quality: default_quality(),
            sizes: default_sizes(),
            selected_size: String::new(),
            custom_size: default_custom_size(),
            shrink_only: true,
            replace: false,
            ignore_orientation: true,
            keep_date_modified: false,
            file_name_template: default_file_name(),
            fallback_encoder: FallbackEncoder::default(),
        }
    }
}

impl ImageResizerConfig {
    /// The currently selected size (resolving `selected_size` / `preset`).
    pub fn active_size(&self) -> SizePreset {
        let pick = |name: &str| -> Option<SizePreset> {
            if name.eq_ignore_ascii_case("custom") {
                let mut c = self.custom_size.clone();
                if c.name.is_empty() {
                    c.name = "Custom".into();
                }
                return Some(c);
            }
            self.sizes
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(name))
                .cloned()
        };
        if !self.selected_size.is_empty()
            && let Some(s) = pick(&self.selected_size)
        {
            return s;
        }
        pick(self.preset.default_name())
            .or_else(|| self.sizes.first().cloned())
            .unwrap_or_else(|| SizePreset::max_width(self.preset.default_name(), 1280))
    }

    /// Build resize options from the config.
    pub fn resize_options(&self) -> ResizeOptions {
        ResizeOptions {
            size: self.active_size(),
            output_format: self.output_format,
            quality: self.quality,
            shrink_only: self.shrink_only,
            replace: self.replace,
            ignore_orientation: self.ignore_orientation,
            keep_date_modified: self.keep_date_modified,
            file_name_template: self.file_name_template.clone(),
            fallback_encoder: self.fallback_encoder,
        }
    }
}

pub struct ImageResizer {
    running: bool,
    config: ImageResizerConfig,
}

impl ImageResizer {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("image-resizer").unwrap_or_default();
        Self {
            running: false,
            config,
        }
    }

    pub fn config(&self) -> &ImageResizerConfig {
        &self.config
    }
}

impl Default for ImageResizer {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for ImageResizer {
    fn id(&self) -> &'static str {
        "image-resizer"
    }

    fn name(&self) -> &'static str {
        "Image Resizer"
    }

    fn description(&self) -> &'static str {
        "Batch resize images with presets and custom dimensions"
    }

    fn start(&mut self) -> Result<()> {
        self.running = true;
        info!("Image Resizer module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Image Resizer module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        info!("Image Resizer: launching GUI");

        let bin = find_gui_binary();
        let result = std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(child) => {
                info!("Image Resizer GUI spawned (pid={})", child.id());
            }
            Err(e) => {
                warn!("Failed to spawn Image Resizer GUI ({bin:?}): {e}");
            }
        }
        Ok(())
    }
}

/// Find the mpt-image-resizer binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-image-resizer");
        if candidate.exists() {
            return candidate;
        }
    }
    // Fallback: rely on PATH
    "mpt-image-resizer".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_loads() {
        let cfg: ImageResizerConfig =
            toml::from_str("preset = \"large\"\noutput_format = \"jpeg\"\nquality = 70\n").unwrap();
        assert_eq!(cfg.sizes.len(), 4);
        assert_eq!(cfg.active_size().name, "Large");
        assert_eq!(cfg.active_size().width, 1920.0);
        assert_eq!(cfg.file_name_template, "%1 (%2)");
        assert!(cfg.shrink_only);
    }

    #[test]
    fn selected_size_overrides_preset() {
        let mut cfg = ImageResizerConfig::default();
        cfg.sizes.push(SizePreset::new(
            "Thumb",
            ResizeFit::Fill,
            100.0,
            100.0,
            ResizeUnit::Pixel,
        ));
        cfg.selected_size = "thumb".into();
        assert_eq!(cfg.active_size().name, "Thumb");
        cfg.selected_size = "custom".into();
        assert_eq!(cfg.active_size().width, 1024.0);
        cfg.selected_size = "missing".into();
        assert_eq!(cfg.active_size().name, "Medium");
        let s = toml::to_string(&cfg).unwrap();
        let back: ImageResizerConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.sizes.len(), 5);
    }
}
