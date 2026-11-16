pub mod actions;
pub mod ai;
pub mod clipboard;
pub mod config;
pub mod json;
#[cfg(test)]
mod parity_tests;
#[cfg(feature = "gui")]
pub mod picker;

use anyhow::Result;
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use tracing::{info, warn};

pub use actions::{ClipboardContent, Output, PasteAction};
pub use config::{CustomAction, PastePlainConfig};

/// Named action that opens the Advanced Paste picker.
pub const ACTION_ADVANCED_PASTE: &str = "advanced-paste";

/// Static action names for custom AI actions with shortcuts (index = position in config).
const CUSTOM_ACTION_IDS: [&str; 20] = [
    "custom-1",
    "custom-2",
    "custom-3",
    "custom-4",
    "custom-5",
    "custom-6",
    "custom-7",
    "custom-8",
    "custom-9",
    "custom-10",
    "custom-11",
    "custom-12",
    "custom-13",
    "custom-14",
    "custom-15",
    "custom-16",
    "custom-17",
    "custom-18",
    "custom-19",
    "custom-20",
];

/// Direct (non-picker) named actions.
fn direct_action(name: &str) -> Option<PasteAction> {
    Some(match name {
        "markdown" => PasteAction::Markdown,
        "json" => PasteAction::Json,
        "txt-file" => PasteAction::TxtFile,
        "png-file" => PasteAction::PngFile,
        "html-file" => PasteAction::HtmlFile,
        other => {
            let i = CUSTOM_ACTION_IDS.iter().position(|id| *id == other)?;
            PasteAction::Custom(i)
        }
    })
}

/// Hotkey bindings derived from the config (`(hotkey string, action name)`).
pub fn configured_bindings(cfg: &PastePlainConfig) -> Vec<(String, &'static str)> {
    let mut v: Vec<(String, &'static str)> = vec![
        (
            cfg.advanced_paste_ui_shortcut.clone(),
            ACTION_ADVANCED_PASTE,
        ),
        (cfg.paste_as_markdown_shortcut.clone(), "markdown"),
        (cfg.paste_as_json_shortcut.clone(), "json"),
        (cfg.paste_as_txt_file_shortcut.clone(), "txt-file"),
        (cfg.paste_as_png_file_shortcut.clone(), "png-file"),
        (cfg.paste_as_html_file_shortcut.clone(), "html-file"),
    ];
    if cfg.ai_available() {
        for (i, a) in cfg
            .custom_actions
            .iter()
            .enumerate()
            .take(CUSTOM_ACTION_IDS.len())
        {
            if let Some(s) = &a.shortcut {
                v.push((s.clone(), CUSTOM_ACTION_IDS[i]));
            }
        }
    }
    v.retain(|(s, _)| !s.trim().is_empty());
    v
}

pub struct PastePlain {
    running: bool,
    config: PastePlainConfig,
}

impl PastePlain {
    pub fn new() -> Self {
        Self {
            running: false,
            config: PastePlainConfig::load(),
        }
    }

    /// Read clipboard, strip formatting, write back as plain text, then simulate Ctrl+V.
    pub fn paste_as_plain(&self) -> Result<()> {
        let text = clipboard::get_text()?;
        if text.is_empty() {
            info!("Clipboard is empty, nothing to paste");
            return Ok(());
        }
        clipboard::set_text(&text)?;
        clipboard::simulate_paste()?;
        info!("Pasted {} chars as plain text", text.len());
        Ok(())
    }

    fn open_picker(&self) -> Result<()> {
        let bin = find_gui_binary();
        match std::process::Command::new(&bin)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => {
                info!("Advanced Paste picker spawned (pid={})", child.id());
                Ok(())
            }
            Err(e) => Err(anyhow::anyhow!(
                "failed to launch Advanced Paste picker ({bin:?}): {e}"
            )),
        }
    }
}

impl Default for PastePlain {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for PastePlain {
    fn id(&self) -> &'static str {
        "paste-plain"
    }

    fn name(&self) -> &'static str {
        "Paste as Plain Text"
    }

    fn description(&self) -> &'static str {
        "Paste clipboard contents as plain text, stripping all formatting"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(Hotkey::new(vec![Modifier::Super, Modifier::Ctrl], "V"))
    }

    fn start(&mut self) -> Result<()> {
        self.config = PastePlainConfig::load();
        self.running = true;
        info!("Paste as Plain Text module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Paste as Plain Text module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        self.paste_as_plain()
    }

    fn additional_hotkeys(&self) -> Vec<(Hotkey, &'static str)> {
        configured_bindings(&self.config)
            .into_iter()
            .filter_map(|(s, action)| match Hotkey::parse(&s) {
                Ok(hk) => Some((hk, action)),
                Err(e) => {
                    warn!("paste-plain: invalid shortcut {s:?} for {action}: {e}");
                    None
                }
            })
            .collect()
    }

    fn on_named_action(&mut self, action: &str) -> Result<()> {
        self.config = PastePlainConfig::load();
        if action == ACTION_ADVANCED_PASTE {
            return self.open_picker();
        }
        let Some(paste_action) = direct_action(action) else {
            anyhow::bail!("unknown paste-plain action: {action}");
        };
        if paste_action.requires_ai() && !self.config.ai_available() {
            anyhow::bail!("AI paste is disabled");
        }
        // Run off the hotkey thread: AI requests can take several seconds.
        let cfg = self.config.clone();
        std::thread::spawn(move || {
            if let Err(e) = actions::run_and_paste(&paste_action, &cfg) {
                warn!("Advanced Paste failed: {e:#}");
            }
        });
        Ok(())
    }
}

/// Find the mpt-paste-plain binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-paste-plain");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-paste-plain".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings() {
        let cfg = PastePlainConfig::default();
        let b = configured_bindings(&cfg);
        assert!(b.contains(&("Super+Shift+V".into(), ACTION_ADVANCED_PASTE)));
        assert!(b.iter().any(|(_, a)| *a == "markdown"));
        assert!(!b.iter().any(|(_, a)| *a == "txt-file"));
    }

    #[test]
    fn custom_shortcuts_only_with_ai() {
        let mut cfg = PastePlainConfig::default();
        cfg.custom_actions.push(CustomAction {
            name: "x".into(),
            prompt: "y".into(),
            shortcut: Some("Super+Ctrl+Alt+1".into()),
            is_shown: true,
        });
        assert!(
            !configured_bindings(&cfg)
                .iter()
                .any(|(_, a)| *a == "custom-1")
        );
        cfg.ai_enabled = true;
        cfg.ai_endpoint = "http://x/v1".into();
        assert!(
            configured_bindings(&cfg)
                .iter()
                .any(|(_, a)| *a == "custom-1")
        );
        assert_eq!(direct_action("custom-1"), Some(PasteAction::Custom(0)));
        assert_eq!(direct_action("json"), Some(PasteAction::Json));
        assert_eq!(direct_action("nope"), None);
    }
}
