//! Configuration for the paste-plain / Advanced Paste module.
//!
//! Mirrors upstream `AdvancedPasteProperties` (PowerToys settings) with
//! snake_case field names. Stored in `~/.config/my-power-toys/paste-plain.toml`.

use serde::{Deserialize, Serialize};

/// Module id, also the config file name.
pub const MODULE_ID: &str = "paste-plain";

/// A user-defined AI transformation (upstream `AdvancedPasteCustomAction`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomAction {
    /// Display name shown in the picker.
    #[serde(default)]
    pub name: String,
    /// Instructions sent to the AI model together with the clipboard text.
    #[serde(default)]
    pub prompt: String,
    /// Optional global shortcut (e.g. `Super+Ctrl+Alt+1`) that runs the action directly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    /// Whether the action is listed in the picker (upstream `IsShown`).
    #[serde(default = "default_true")]
    pub is_shown: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PastePlainConfig {
    /// Opens the Advanced Paste picker (upstream `AdvancedPasteUIShortcut`).
    #[serde(default = "default_ui_shortcut")]
    pub advanced_paste_ui_shortcut: String,
    /// Direct "paste as Markdown" (upstream `PasteAsMarkdownShortcut`). Empty = unbound.
    #[serde(default = "default_markdown_shortcut")]
    pub paste_as_markdown_shortcut: String,
    /// Direct "paste as JSON" (upstream `PasteAsJsonShortcut`). Empty = unbound.
    #[serde(default = "default_json_shortcut")]
    pub paste_as_json_shortcut: String,
    /// Direct "paste as .txt file". Empty = unbound.
    #[serde(default)]
    pub paste_as_txt_file_shortcut: String,
    /// Direct "paste as .png file". Empty = unbound.
    #[serde(default)]
    pub paste_as_png_file_shortcut: String,
    /// Direct "paste as .html file". Empty = unbound.
    #[serde(default)]
    pub paste_as_html_file_shortcut: String,

    /// Close the picker when it loses focus (upstream `CloseAfterLosingFocus`).
    #[serde(default)]
    pub close_after_losing_focus: bool,
    /// Show a preview of the clipboard in the picker (upstream `EnableClipboardPreview`).
    #[serde(default = "default_true")]
    pub enable_clipboard_preview: bool,
    /// Show AI results for review before pasting (upstream `ShowCustomPreview`).
    #[serde(default = "default_true")]
    pub show_custom_preview: bool,
    /// List the "paste as file" formats in the picker (upstream `AdditionalActions.PasteAsFile`).
    #[serde(default = "default_true")]
    pub show_paste_as_file: bool,
    /// List "image to text" (OCR via tesseract) in the picker (upstream `AdditionalActions.ImageToText`).
    #[serde(default = "default_true")]
    pub show_image_to_text: bool,
    /// JPEG quality for "paste as .jpg file" (upstream `PasteAsJpgQuality`).
    #[serde(default = "default_jpg_quality")]
    pub paste_as_jpg_quality: u8,

    /// Enable AI-powered actions (upstream `IsAIEnabled`).
    #[serde(default)]
    pub ai_enabled: bool,
    /// OpenAI-compatible base URL, e.g. `https://api.openai.com/v1` or
    /// `http://localhost:11434/v1`. Empty disables AI actions.
    #[serde(default)]
    pub ai_endpoint: String,
    /// Model name sent to the endpoint.
    #[serde(default = "default_ai_model")]
    pub ai_model: String,
    /// Name of the environment variable holding the API key. The key itself is
    /// never stored in the config file.
    #[serde(default = "default_ai_api_key_env")]
    pub ai_api_key_env: String,
    /// Request timeout in seconds.
    #[serde(default = "default_ai_timeout")]
    pub ai_timeout_secs: u64,
    /// Show the free-form "ask AI" prompt box in the picker (upstream `ShowAIPaste`).
    #[serde(default = "default_true")]
    pub show_ai_paste: bool,
    /// Show "fix spelling and grammar" (upstream `AdditionalActions.FixSpellingAndGrammar`).
    #[serde(default = "default_true")]
    pub show_fix_spelling_and_grammar: bool,
    /// User-defined AI actions (upstream `CustomActions`).
    #[serde(default)]
    pub custom_actions: Vec<CustomAction>,
}

fn default_true() -> bool {
    true
}
fn default_ui_shortcut() -> String {
    "Super+Shift+V".into()
}
fn default_markdown_shortcut() -> String {
    "Super+Ctrl+Alt+M".into()
}
fn default_json_shortcut() -> String {
    "Super+Ctrl+Alt+J".into()
}
fn default_jpg_quality() -> u8 {
    90
}
fn default_ai_model() -> String {
    "gpt-4o-mini".into()
}
fn default_ai_api_key_env() -> String {
    "OPENAI_API_KEY".into()
}
fn default_ai_timeout() -> u64 {
    60
}

impl Default for PastePlainConfig {
    fn default() -> Self {
        Self {
            advanced_paste_ui_shortcut: default_ui_shortcut(),
            paste_as_markdown_shortcut: default_markdown_shortcut(),
            paste_as_json_shortcut: default_json_shortcut(),
            paste_as_txt_file_shortcut: String::new(),
            paste_as_png_file_shortcut: String::new(),
            paste_as_html_file_shortcut: String::new(),
            close_after_losing_focus: false,
            enable_clipboard_preview: true,
            show_custom_preview: true,
            show_paste_as_file: true,
            show_image_to_text: true,
            paste_as_jpg_quality: default_jpg_quality(),
            ai_enabled: false,
            ai_endpoint: String::new(),
            ai_model: default_ai_model(),
            ai_api_key_env: default_ai_api_key_env(),
            ai_timeout_secs: default_ai_timeout(),
            show_ai_paste: true,
            show_fix_spelling_and_grammar: true,
            custom_actions: Vec::new(),
        }
    }
}

impl PastePlainConfig {
    /// Load from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        mpt_common::config::load_module_config(MODULE_ID).unwrap_or_default()
    }

    /// AI actions are usable only when enabled and an endpoint is configured.
    pub fn ai_available(&self) -> bool {
        self.ai_enabled && !self.ai_endpoint.trim().is_empty() && !self.ai_model.trim().is_empty()
    }

    /// Custom actions that are listed in the picker, with their index in `custom_actions`.
    pub fn visible_custom_actions(&self) -> impl Iterator<Item = (usize, &CustomAction)> {
        self.custom_actions
            .iter()
            .enumerate()
            .filter(|(_, a)| a.is_shown && !a.name.trim().is_empty() && !a.prompt.trim().is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_gives_defaults() {
        let cfg: PastePlainConfig = toml::from_str("").unwrap();
        assert_eq!(cfg, PastePlainConfig::default());
        assert_eq!(cfg.ai_api_key_env, "OPENAI_API_KEY");
        assert!(!cfg.ai_available());
    }

    #[test]
    fn round_trip_with_custom_actions() {
        let mut cfg = PastePlainConfig {
            ai_enabled: true,
            ai_endpoint: "http://localhost:11434/v1".into(),
            ..Default::default()
        };
        cfg.custom_actions.push(CustomAction {
            name: "Translate".into(),
            prompt: "Translate to French".into(),
            shortcut: Some("Super+Ctrl+Alt+1".into()),
            is_shown: true,
        });
        let s = toml::to_string_pretty(&cfg).unwrap();
        assert!(!s.contains("api_key ="));
        let back: PastePlainConfig = toml::from_str(&s).unwrap();
        assert_eq!(back, cfg);
        assert!(back.ai_available());
        assert_eq!(back.visible_custom_actions().count(), 1);
    }

    #[test]
    fn custom_action_defaults() {
        let cfg: PastePlainConfig =
            toml::from_str("[[custom_actions]]\nname = \"A\"\nprompt = \"p\"\n").unwrap();
        assert!(cfg.custom_actions[0].is_shown);
        assert!(cfg.custom_actions[0].shortcut.is_none());
    }
}
