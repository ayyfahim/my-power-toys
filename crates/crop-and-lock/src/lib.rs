//! Crop And Lock: crop a region of a window into its own window.
//!
//! Linux (X11) port of PowerToys Crop And Lock. Three modes, as upstream:
//! - **Thumbnail**: a new window that shows the live content of the region.
//! - **Reparent**: the real window is moved (X11 reparented) into a container
//!   window sized to the region, so it stays interactive.
//! - **Screenshot**: a static snapshot of the region (also copied to the
//!   clipboard as PNG).
//!
//! The daemon grabs the hotkeys; on activation this module spawns the
//! `mpt-crop-and-lock` GUI binary for the currently active window.

#[cfg(feature = "gui")]
pub mod capture;
#[cfg(feature = "gui")]
pub mod clipboard;
pub mod geometry;
#[cfg(feature = "gui")]
pub mod gui;
#[cfg(feature = "gui")]
pub mod reparent;
pub mod x11;

use anyhow::{Context, Result, bail};
use mpt_common::hotkey::{Hotkey, Modifier};
use mpt_common::module::PowerModule;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use x11rb::connection::Connection;

pub const MODULE_ID: &str = "crop-and-lock";

pub const DEFAULT_THUMBNAIL_HOTKEY: &str = "Super+Ctrl+Shift+T";
pub const DEFAULT_REPARENT_HOTKEY: &str = "Super+Ctrl+Shift+R";
pub const DEFAULT_SCREENSHOT_HOTKEY: &str = "Super+Ctrl+Shift+S";

fn default_thumbnail_hotkey() -> String {
    DEFAULT_THUMBNAIL_HOTKEY.to_string()
}
fn default_reparent_hotkey() -> String {
    DEFAULT_REPARENT_HOTKEY.to_string()
}
fn default_screenshot_hotkey() -> String {
    DEFAULT_SCREENSHOT_HOTKEY.to_string()
}

/// Settings, mirroring upstream `CropAndLockProperties`
/// (`thumbnail-hotkey`, `reparent-hotkey`, `screenshot-hotkey`).
/// Stored in `~/.config/my-power-toys/crop-and-lock.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CropAndLockConfig {
    #[serde(default = "default_thumbnail_hotkey")]
    pub thumbnail_hotkey: String,
    #[serde(default = "default_reparent_hotkey")]
    pub reparent_hotkey: String,
    #[serde(default = "default_screenshot_hotkey")]
    pub screenshot_hotkey: String,
}

impl Default for CropAndLockConfig {
    fn default() -> Self {
        Self {
            thumbnail_hotkey: default_thumbnail_hotkey(),
            reparent_hotkey: default_reparent_hotkey(),
            screenshot_hotkey: default_screenshot_hotkey(),
        }
    }
}

impl CropAndLockConfig {
    pub fn load() -> Self {
        mpt_common::config::load_module_config(MODULE_ID).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        mpt_common::config::save_module_config(MODULE_ID, self)
    }

    /// Parsed hotkey for a mode, falling back to the upstream default when the
    /// configured string is invalid.
    pub fn hotkey(&self, mode: CropMode) -> Hotkey {
        let (configured, default) = match mode {
            CropMode::Thumbnail => (&self.thumbnail_hotkey, DEFAULT_THUMBNAIL_HOTKEY),
            CropMode::Reparent => (&self.reparent_hotkey, DEFAULT_REPARENT_HOTKEY),
            CropMode::Screenshot => (&self.screenshot_hotkey, DEFAULT_SCREENSHOT_HOTKEY),
        };
        Hotkey::parse(configured).unwrap_or_else(|e| {
            warn!(
                "Crop And Lock: invalid {} hotkey '{configured}' ({e}), using {default}",
                mode.as_str()
            );
            Hotkey::parse(default).unwrap_or_else(|_| {
                Hotkey::new(vec![Modifier::Super, Modifier::Ctrl, Modifier::Shift], "T")
            })
        })
    }
}

/// Crop mode (upstream `CropAndLockType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropMode {
    Thumbnail,
    Reparent,
    Screenshot,
}

impl CropMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            CropMode::Thumbnail => "thumbnail",
            CropMode::Reparent => "reparent",
            CropMode::Screenshot => "screenshot",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "thumbnail" => Some(CropMode::Thumbnail),
            "reparent" => Some(CropMode::Reparent),
            "screenshot" => Some(CropMode::Screenshot),
            _ => None,
        }
    }
}

/// Command-line arguments of the GUI binary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliArgs {
    pub mode: CropMode,
    pub window: u32,
}

/// Parse `--mode <thumbnail|reparent|screenshot> --window <xid>`.
/// `--window` accepts decimal or `0x` hex ids.
pub fn parse_cli_args<I, S>(args: I) -> Result<CliArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut mode = None;
    let mut window = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let arg = arg.as_ref();
        let (key, inline) = match arg.split_once('=') {
            Some((k, v)) => (k, Some(v.to_string())),
            None => (arg, None),
        };
        let mut value = || -> Result<String> {
            match &inline {
                Some(v) => Ok(v.clone()),
                None => iter
                    .next()
                    .map(|v| v.as_ref().to_string())
                    .with_context(|| format!("missing value for {key}")),
            }
        };
        match key {
            "--mode" | "-m" => {
                let v = value()?;
                mode = Some(CropMode::parse(&v).with_context(|| format!("unknown mode '{v}'"))?);
            }
            "--window" | "-w" => window = Some(geometry::parse_window_id(&value()?)?),
            other => bail!("unknown argument '{other}'"),
        }
    }
    Ok(CliArgs {
        mode: mode.unwrap_or(CropMode::Thumbnail),
        window: window.context("--window <xid> is required")?,
    })
}

pub struct CropAndLock {
    running: bool,
    config: CropAndLockConfig,
}

impl CropAndLock {
    pub fn new() -> Self {
        Self {
            running: false,
            config: CropAndLockConfig::load(),
        }
    }

    fn launch(&mut self, mode: CropMode) -> Result<()> {
        let (conn, screen_num) = x11::connect()?;
        let root = conn.setup().roots[screen_num].root;
        let window = x11::active_window(&conn, root)?;

        // Upstream ignores the hotkey when a cropped window is in the foreground.
        if x11::is_own_window(&conn, window) {
            info!("Crop And Lock: active window is already a cropped window, ignoring");
            return Ok(());
        }
        drop(conn);

        let bin = find_gui_binary();
        info!(
            "Crop And Lock: launching {bin:?} (mode={}, window=0x{window:x})",
            mode.as_str()
        );
        let child = std::process::Command::new(&bin)
            .arg("--mode")
            .arg(mode.as_str())
            .arg("--window")
            .arg(window.to_string())
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("failed to spawn Crop And Lock GUI ({bin:?})"))?;
        info!("Crop And Lock GUI spawned (pid={})", child.id());
        Ok(())
    }
}

impl Default for CropAndLock {
    fn default() -> Self {
        Self::new()
    }
}

impl PowerModule for CropAndLock {
    fn id(&self) -> &'static str {
        MODULE_ID
    }

    fn name(&self) -> &'static str {
        "Crop and Lock"
    }

    fn description(&self) -> &'static str {
        "Crop a window region into a live thumbnail window"
    }

    fn default_hotkey(&self) -> Option<Hotkey> {
        Some(self.config.hotkey(CropMode::Thumbnail))
    }

    fn additional_hotkeys(&self) -> Vec<(Hotkey, &'static str)> {
        vec![
            (
                self.config.hotkey(CropMode::Reparent),
                CropMode::Reparent.as_str(),
            ),
            (
                self.config.hotkey(CropMode::Screenshot),
                CropMode::Screenshot.as_str(),
            ),
        ]
    }

    fn start(&mut self) -> Result<()> {
        self.config = CropAndLockConfig::load();
        self.running = true;
        info!("Crop And Lock module started");
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        self.running = false;
        info!("Crop And Lock module stopped");
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn on_hotkey(&mut self) -> Result<()> {
        self.launch(CropMode::Thumbnail)
    }

    fn on_named_action(&mut self, action: &str) -> Result<()> {
        match CropMode::parse(action) {
            Some(mode) => self.launch(mode),
            None => {
                warn!("Crop And Lock: unknown action '{action}'");
                Ok(())
            }
        }
    }
}

/// Find the mpt-crop-and-lock binary, looking next to the current executable first.
fn find_gui_binary() -> std::path::PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("mpt-crop-and-lock");
        if candidate.exists() {
            return candidate;
        }
    }
    "mpt-crop-and-lock".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys_match_upstream() {
        let cfg = CropAndLockConfig::default();
        let all = [Modifier::Super, Modifier::Ctrl, Modifier::Shift];
        for (mode, key) in [
            (CropMode::Thumbnail, "T"),
            (CropMode::Reparent, "R"),
            (CropMode::Screenshot, "S"),
        ] {
            let hk = cfg.hotkey(mode);
            assert_eq!(hk.key, key);
            assert_eq!(hk.modifiers, all);
        }
    }

    #[test]
    fn invalid_hotkey_falls_back() {
        let cfg = CropAndLockConfig {
            reparent_hotkey: "Hyper+R".into(),
            ..Default::default()
        };
        assert_eq!(
            cfg.hotkey(CropMode::Reparent),
            Hotkey::parse(DEFAULT_REPARENT_HOTKEY).unwrap()
        );
    }

    #[test]
    fn config_roundtrip_and_partial() {
        let cfg = CropAndLockConfig {
            thumbnail_hotkey: "Super+Alt+T".into(),
            ..Default::default()
        };
        let text = toml::to_string(&cfg).unwrap();
        assert!(text.contains("thumbnail-hotkey"));
        let back: CropAndLockConfig = toml::from_str(&text).unwrap();
        assert_eq!(back, cfg);

        let partial: CropAndLockConfig =
            toml::from_str("screenshot-hotkey = \"Ctrl+Shift+S\"").unwrap();
        assert_eq!(partial.screenshot_hotkey, "Ctrl+Shift+S");
        assert_eq!(partial.thumbnail_hotkey, DEFAULT_THUMBNAIL_HOTKEY);
    }

    #[test]
    fn mode_parse_roundtrip() {
        for mode in [
            CropMode::Thumbnail,
            CropMode::Reparent,
            CropMode::Screenshot,
        ] {
            assert_eq!(CropMode::parse(mode.as_str()), Some(mode));
        }
        assert_eq!(CropMode::parse("Reparent"), Some(CropMode::Reparent));
        assert_eq!(CropMode::parse("other"), None);
    }

    #[test]
    fn cli_parsing() {
        let a = parse_cli_args(["--mode", "reparent", "--window", "0x400001"]).unwrap();
        assert_eq!(
            a,
            CliArgs {
                mode: CropMode::Reparent,
                window: 0x400001
            }
        );
        let a = parse_cli_args(["--window=42"]).unwrap();
        assert_eq!(a.mode, CropMode::Thumbnail);
        assert_eq!(a.window, 42);
        assert!(parse_cli_args(["--mode", "screenshot"]).is_err());
        assert!(parse_cli_args(["--mode", "bogus", "--window", "1"]).is_err());
        assert!(parse_cli_args(["--window"]).is_err());
        assert!(parse_cli_args(["--frobnicate"]).is_err());
    }
}
