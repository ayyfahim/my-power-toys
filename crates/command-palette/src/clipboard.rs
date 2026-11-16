//! Clipboard history: a small JSON store written by a watcher thread that
//! runs inside the daemon (module `start()`), read by the palette GUI.
//!
//! The watcher only records anything while
//! `providers.clipboard_history = true` (off by default). It polls the text
//! clipboard every `clipboard_history.poll_interval_ms` with `wl-paste`
//! (Wayland) or `xclip`/`xsel` (X11), skips entries marked as secret by
//! password managers (`x-kde-passwordManagerHint`), and stores the file
//! with mode 0600.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::CommandPaletteConfig;

const CONFIG_RELOAD: Duration = Duration::from_secs(5);
const TOOL_TIMEOUT: Duration = Duration::from_millis(700);
const SECRET_HINT: &str = "x-kde-passwordManagerHint";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipEntry {
    pub text: String,
    /// Unix seconds when the text was (last) copied.
    pub copied_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClipboardStore {
    /// Newest first.
    #[serde(default)]
    pub entries: Vec<ClipEntry>,
}

pub fn store_path() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::config_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("my-power-toys")
        .join("clipboard-history.json")
}

impl ClipboardStore {
    pub fn load() -> Self {
        std::fs::read_to_string(store_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Write atomically with owner-only permissions.
    pub fn save(&self) -> Result<()> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        let path = store_path();
        let dir = path.parent().context("store path has no parent")?;
        std::fs::create_dir_all(dir)?;
        let tmp = path.with_extension("json.tmp");
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .with_context(|| format!("failed to open {}", tmp.display()))?;
        f.write_all(serde_json::to_string(self)?.as_bytes())?;
        f.sync_all()?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Remove the history file.
    pub fn clear_file() -> Result<()> {
        let path = store_path();
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    /// Add `text` as the newest entry (moving an existing duplicate to the
    /// front). Returns true if the store changed.
    pub fn push(&mut self, text: &str, now: u64, max_entries: usize) -> bool {
        if self.entries.first().is_some_and(|e| e.text == text) {
            return false;
        }
        self.entries.retain(|e| e.text != text);
        self.entries.insert(
            0,
            ClipEntry {
                text: text.to_string(),
                copied_at: now,
            },
        );
        self.entries.truncate(max_entries.max(1));
        true
    }
}

/// Whether a clipboard text should be recorded.
pub fn should_store(text: &str, max_bytes: usize) -> bool {
    !text.trim().is_empty() && text.len() <= max_bytes
}

/// Whether the offered clipboard targets mark the content as secret.
pub fn is_sensitive(targets: &str) -> bool {
    targets.lines().any(|t| t.trim() == SECRET_HINT)
}

/// Whether the offered targets include plain text.
pub fn has_text_target(targets: &str) -> bool {
    targets.lines().map(str::trim).any(|t| {
        matches!(t, "UTF8_STRING" | "STRING" | "TEXT" | "text/plain")
            || t.starts_with("text/plain;")
    })
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Run a command, capturing stdout, killing it after `timeout`
/// (clipboard owners can fail to answer).
fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<Vec<u8>> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let out = reader.join().ok()?;
                return status.success().then_some(out);
            }
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return None;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    WlPaste,
    Xclip,
    Xsel,
}

fn tool_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

fn detect_backend() -> Option<Backend> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    if wayland && tool_exists("wl-paste") {
        return Some(Backend::WlPaste);
    }
    if std::env::var_os("DISPLAY").is_some() {
        if tool_exists("xclip") {
            return Some(Backend::Xclip);
        }
        if tool_exists("xsel") {
            return Some(Backend::Xsel);
        }
    }
    None
}

/// Current clipboard text, or `None` if empty, non-text or secret.
fn read_clipboard(backend: Backend) -> Option<String> {
    let targets = match backend {
        Backend::WlPaste => run_with_timeout("wl-paste", &["--list-types"], TOOL_TIMEOUT),
        Backend::Xclip => run_with_timeout(
            "xclip",
            &["-selection", "clipboard", "-t", "TARGETS", "-o"],
            TOOL_TIMEOUT,
        ),
        // xsel can't list targets.
        Backend::Xsel => None,
    }
    .map(|b| String::from_utf8_lossy(&b).to_string());

    if let Some(targets) = &targets {
        if is_sensitive(targets) || !has_text_target(targets) {
            return None;
        }
    } else if backend != Backend::Xsel {
        // Empty clipboard or tool failure.
        return None;
    }

    let bytes = match backend {
        Backend::WlPaste => run_with_timeout(
            "wl-paste",
            &["--no-newline", "--type", "text/plain;charset=utf-8"],
            TOOL_TIMEOUT,
        )
        .or_else(|| {
            run_with_timeout(
                "wl-paste",
                &["--no-newline", "--type", "text"],
                TOOL_TIMEOUT,
            )
        }),
        Backend::Xclip => run_with_timeout(
            "xclip",
            &["-selection", "clipboard", "-t", "UTF8_STRING", "-o"],
            TOOL_TIMEOUT,
        ),
        Backend::Xsel => run_with_timeout("xsel", &["--clipboard", "--output"], TOOL_TIMEOUT),
    }?;
    String::from_utf8(bytes).ok()
}

/// Spawn the watcher thread (called from the module's `start`).
pub fn spawn_watcher(stop: Arc<AtomicBool>) -> Option<JoinHandle<()>> {
    std::thread::Builder::new()
        .name("cmdpal-clipboard".into())
        .spawn(move || watch(stop))
        .map_err(|e| warn!("Command Palette: failed to start clipboard watcher: {e}"))
        .ok()
}

fn load_config() -> CommandPaletteConfig {
    mpt_common::config::load_module_config("command-palette").unwrap_or_default()
}

fn watch(stop: Arc<AtomicBool>) {
    let mut config = load_config();
    let mut last_reload = Instant::now();
    let mut backend: Option<Backend> = None;
    let mut warned_missing = false;
    let mut last_seen: Option<String> = None;
    let mut store: Option<ClipboardStore> = None;

    while !stop.load(Ordering::Relaxed) {
        if last_reload.elapsed() >= CONFIG_RELOAD {
            config = load_config();
            last_reload = Instant::now();
        }

        let interval = Duration::from_millis(config.clipboard_history.poll_interval_ms.max(200));

        if !config.providers.clipboard_history {
            store = None;
            last_seen = None;
            sleep_interruptible(&stop, Duration::from_secs(1));
            continue;
        }

        if backend.is_none() {
            backend = detect_backend();
            if backend.is_none() {
                if !warned_missing {
                    warn!(
                        "Command Palette: clipboard history needs wl-paste (Wayland) or xclip/xsel (X11)"
                    );
                    warned_missing = true;
                }
                sleep_interruptible(&stop, Duration::from_secs(5));
                continue;
            }
            info!("Command Palette: clipboard history watcher using {backend:?}");
        }

        if let Some(b) = backend
            && let Some(text) = read_clipboard(b)
            && last_seen.as_deref() != Some(text.as_str())
        {
            if should_store(&text, config.clipboard_history.max_entry_bytes) {
                let st = store.get_or_insert_with(ClipboardStore::load);
                if st.push(&text, now_secs(), config.clipboard_history.max_entries)
                    && let Err(e) = st.save()
                {
                    warn!("Command Palette: failed to save clipboard history: {e:#}");
                }
            }
            last_seen = Some(text);
        }

        sleep_interruptible(&stop, interval);
    }
}

fn sleep_interruptible(stop: &AtomicBool, total: Duration) {
    let start = Instant::now();
    while start.elapsed() < total && !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100).min(total));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_dedupes_and_truncates() {
        let mut s = ClipboardStore::default();
        assert!(s.push("a", 1, 3));
        assert!(s.push("b", 2, 3));
        assert!(!s.push("b", 3, 3));
        assert!(s.push("a", 4, 3));
        assert_eq!(
            s.entries
                .iter()
                .map(|e| e.text.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        s.push("c", 5, 3);
        s.push("d", 6, 3);
        assert_eq!(s.entries.len(), 3);
        assert_eq!(s.entries[0].text, "d");
    }

    #[test]
    fn store_filters() {
        assert!(!should_store("   ", 100));
        assert!(!should_store(&"x".repeat(101), 100));
        assert!(should_store("hello", 100));
    }

    #[test]
    fn target_detection() {
        assert!(is_sensitive("UTF8_STRING\nx-kde-passwordManagerHint\n"));
        assert!(!is_sensitive("UTF8_STRING\nTARGETS\n"));
        assert!(has_text_target("TARGETS\nUTF8_STRING\n"));
        assert!(has_text_target("text/plain;charset=utf-8\n"));
        assert!(!has_text_target("image/png\n"));
    }

    #[test]
    fn json_round_trip() {
        let mut s = ClipboardStore::default();
        s.push("hi", 10, 5);
        let back: ClipboardStore =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.entries, s.entries);
    }
}
