//! `mpt-paste-plain`: the Advanced Paste picker (opened by Super+Shift+V).

use mpt_paste_plain::actions::ClipboardContent;
use mpt_paste_plain::{PastePlainConfig, clipboard};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;
use tracing::{error, info};

/// Removes the PID lock file on drop.
struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_paste_plain=info".into()),
        )
        .init();

    // Prevent multiple instances
    let lock_path = std::env::temp_dir().join("mpt-paste-plain.lock");
    if let Ok(pid_str) = fs::read_to_string(&lock_path)
        && let Ok(pid) = pid_str.trim().parse::<u32>()
        && std::path::Path::new(&format!("/proc/{pid}")).exists()
    {
        info!("Another instance is already running (pid={pid}), exiting");
        return;
    }
    let _ = fs::write(&lock_path, std::process::id().to_string());
    let guard = LockGuard(lock_path);

    let cfg = PastePlainConfig::load();
    let content = ClipboardContent::read();
    let output = mpt_paste_plain::picker::run_picker(cfg, content);
    drop(guard);

    let Some(output) = output else {
        info!("Advanced Paste cancelled");
        return;
    };
    if let Err(e) = output.store() {
        error!("Failed to write clipboard: {e:#}");
        std::process::exit(1);
    }
    // Give the window manager time to return focus to the previous window.
    std::thread::sleep(Duration::from_millis(250));
    if let Err(e) = clipboard::simulate_paste() {
        error!("Failed to send paste: {e:#}");
        std::process::exit(1);
    }
}
