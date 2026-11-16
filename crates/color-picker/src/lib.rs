pub mod color;
#[cfg(feature = "gui")]
pub mod gui;
pub mod history;
pub mod names;
pub mod picker;
pub mod screenshot;

#[cfg(test)]
mod parity_tests;

use anyhow::Result;
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

pub use color::{Color, ColorFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationBehavior {
    PickAndEdit,
    PickAndClose,
    EditorOnly,
}

fn default_format() -> ColorFormat {
    ColorFormat::Hex
}

fn default_behavior() -> ActivationBehavior {
    ActivationBehavior::PickAndEdit
}

/// What a mouse button does in the picker overlay (upstream `ColorPickerClickAction`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClickAction {
    /// Copy the color and open the editor.
    #[serde(alias = "PickColorThenEditor")]
    PickColorThenEditor,
    /// Copy the color and exit.
    #[serde(alias = "PickColorAndClose")]
    PickColorAndClose,
    /// Exit without copying.
    #[serde(alias = "Close")]
    Close,
}

/// One entry of the editor format list (same shape the settings UI writes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ColorFormatEntry {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Upstream defaults: HEX/RGB/HSL visible, the rest hidden. HSV and CMYK are
/// enabled here because the settings UI and the previous editor showed them.
fn default_formats() -> Vec<ColorFormatEntry> {
    ColorFormat::ALL
        .iter()
        .map(|f| ColorFormatEntry {
            id: f.id().to_string(),
            label: f.label().to_string(),
            enabled: matches!(
                f,
                ColorFormat::Hex
                    | ColorFormat::Rgb
                    | ColorFormat::Hsl
                    | ColorFormat::Hsv
                    | ColorFormat::Cmyk
            ),
        })
        .collect()
}

fn default_history_limit() -> usize {
    history::MAX_HISTORY
}

fn default_middle_click() -> ClickAction {
    ClickAction::PickColorAndClose
}

fn default_secondary_click() -> ClickAction {
    ClickAction::Close
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorPickerConfig {
    /// Format copied to the clipboard (upstream `CopiedColorRepresentation`).
    #[serde(default = "default_format")]
    pub format: ColorFormat,
    #[serde(default = "default_behavior")]
    pub behavior: ActivationBehavior,
    /// Show the nearest color name in the overlay and editor (upstream `ShowColorName`).
    #[serde(default = "default_true")]
    pub show_color_name: bool,
    /// Editor formats: order and visibility (upstream `VisibleColorFormats`).
    #[serde(default = "default_formats")]
    pub formats: Vec<ColorFormatEntry>,
    /// Maximum number of history entries (upstream `ColorHistoryLimit`).
    #[serde(default = "default_history_limit")]
    pub color_history_limit: usize,
    /// Left click. Unset = derived from `behavior` (pick-and-close → close, otherwise editor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_click_action: Option<ClickAction>,
    /// Middle click (upstream `MiddleClickAction`).
    #[serde(default = "default_middle_click")]
    pub middle_click_action: ClickAction,
    /// Right click (upstream `SecondaryClickAction`).
    #[serde(default = "default_secondary_click")]
    pub secondary_click_action: ClickAction,
    /// Show a crosshair cursor while picking (upstream `ChangeCursor`).
    #[serde(default)]
    pub change_cursor: bool,
}

impl Default for ColorPickerConfig {
    fn default() -> Self {
        Self {
            format: default_format(),
            behavior: default_behavior(),
            show_color_name: true,
            formats: default_formats(),
            color_history_limit: default_history_limit(),
            primary_click_action: None,
            middle_click_action: default_middle_click(),
            secondary_click_action: default_secondary_click(),
            change_cursor: false,
        }
    }
}

impl ColorPickerConfig {
    /// Formats in configured order with their enabled flag. Unknown ids are
    /// skipped; formats missing from the list are appended, disabled.
    pub fn format_list(&self) -> Vec<(ColorFormat, bool)> {
        let mut out: Vec<(ColorFormat, bool)> = Vec::new();
        for e in &self.formats {
            if let Some(f) = ColorFormat::from_id(&e.id)
                && !out.iter().any(|(g, _)| *g == f)
            {
                out.push((f, e.enabled));
            }
        }
        for f in ColorFormat::ALL {
            if !out.iter().any(|(g, _)| *g == f) {
                out.push((f, false));
            }
        }
        out
    }

    /// Formats shown in the editor, in order. Never empty: falls back to the
    /// copied format.
    pub fn enabled_formats(&self) -> Vec<ColorFormat> {
        let v: Vec<ColorFormat> = self
            .format_list()
            .into_iter()
            .filter_map(|(f, on)| on.then_some(f))
            .collect();
        if v.is_empty() { vec![self.format] } else { v }
    }

    pub fn primary_click(&self) -> ClickAction {
        self.primary_click_action.unwrap_or(match self.behavior {
            ActivationBehavior::PickAndClose => ClickAction::PickColorAndClose,
            _ => ClickAction::PickColorThenEditor,
        })
    }

    pub fn history_limit(&self) -> usize {
        self.color_history_limit.max(1)
    }
}

pub struct ColorPicker {
    running: bool,
    config: ColorPickerConfig,
}

impl ColorPicker {
    pub fn new() -> Self {
        let config = mpt_common::config::load_module_config("color-picker").unwrap_or_default();
        Self {
            running: false,
            config,
        }
    }
}

impl Default for ColorPicker {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for ColorPicker {
    fn id(&self) -> &'static str {
        "color-picker"
    }

    fn name(&self) -> &'static str {
        "Color Picker"
    }

    fn description(&self) -> &'static str {
        "Pick any color from the screen and copy it to clipboard"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(Hotkey::new(vec![Modifier::Super, Modifier::Shift], "C"))
    }

    fn start(&mut self) -> Result<()> {
        self.running = true;
        info!("Color Picker module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Color Picker module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        self.config = mpt_common::config::load_module_config("color-picker").unwrap_or_default();
        info!("Color Picker: launching GUI");

        let bin = find_gui_binary();
        let result = std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        match result {
            Ok(child) => {
                info!("Color Picker GUI spawned (pid={})", child.id());
            }
            Err(e) => {
                warn!("Failed to spawn Color Picker GUI ({bin:?}): {e}");
                warn!("Falling back to headless mode");
                let color = picker::pick_color()?;
                let formatted = color.format(self.config.format);
                picker::copy_to_clipboard(&formatted)?;
                let mut history = history::ColorHistory::load();
                history.push_with_limit(color, self.config.history_limit());
                if let Err(e) = history.save() {
                    warn!("Failed to save history: {e}");
                }
                info!("Color copied to clipboard: {formatted}");
            }
        }
        Ok(())
    }
}

/// Find the mpt-color-picker binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-color-picker");
        if candidate.exists() {
            return candidate;
        }
    }
    // Fallback: rely on PATH
    "mpt-color-picker".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The exact shape the settings UI (`ColorPickerConf`) writes.
    const UI_TOML: &str = r#"
format = "rgb"
behavior = "pick-and-close"
show_color_name = false

[[formats]]
id = "hsl"
label = "HSL"
enabled = true

[[formats]]
id = "hex"
label = "HEX"
enabled = false

[[formats]]
id = "rgb"
label = "RGB"
enabled = true
"#;

    #[test]
    fn reads_ui_written_config() {
        let cfg: ColorPickerConfig = toml::from_str(UI_TOML).unwrap();
        assert_eq!(cfg.format, ColorFormat::Rgb);
        assert!(!cfg.show_color_name);
        assert_eq!(
            cfg.enabled_formats(),
            vec![ColorFormat::Hsl, ColorFormat::Rgb]
        );
        assert_eq!(cfg.format_list().len(), ColorFormat::ALL.len());
        assert_eq!(cfg.primary_click(), ClickAction::PickColorAndClose);
        assert_eq!(cfg.color_history_limit, history::MAX_HISTORY);
    }

    #[test]
    fn defaults_and_round_trip() {
        let cfg: ColorPickerConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.enabled_formats().len(), 5);
        assert_eq!(cfg.primary_click(), ClickAction::PickColorThenEditor);
        assert_eq!(cfg.middle_click_action, ClickAction::PickColorAndClose);
        assert_eq!(cfg.secondary_click_action, ClickAction::Close);
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: ColorPickerConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.formats, cfg.formats);
    }

    #[test]
    fn all_disabled_falls_back_to_copied_format() {
        let cfg = ColorPickerConfig {
            format: ColorFormat::Oklch,
            formats: vec![ColorFormatEntry {
                id: "hex".into(),
                label: "HEX".into(),
                enabled: false,
            }],
            ..Default::default()
        };
        assert_eq!(cfg.enabled_formats(), vec![ColorFormat::Oklch]);
    }

    #[test]
    fn click_action_names() {
        #[derive(Deserialize)]
        struct W {
            a: ClickAction,
        }
        let w: W = toml::from_str("a = \"PickColorAndClose\"").unwrap();
        assert_eq!(w.a, ClickAction::PickColorAndClose);
        let w: W = toml::from_str("a = \"close\"").unwrap();
        assert_eq!(w.a, ClickAction::Close);
    }
}
