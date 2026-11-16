//! `mpt-zoom-it --mode zoom|livezoom|draw|break|snip|snip-save`

use mpt_zoom_it::gui::{self, StartMode};
use std::fs;
use std::path::PathBuf;
use tracing::{error, info};

const USAGE: &str = "usage: mpt-zoom-it [--mode zoom|livezoom|draw|break|snip|snip-save]";

fn parse_args() -> Result<StartMode, String> {
    let mut args = std::env::args().skip(1);
    let mut mode = StartMode::Zoom;
    while let Some(arg) = args.next() {
        let value = match arg.as_str() {
            "--mode" | "-m" => args.next().ok_or("--mode needs a value")?,
            "-h" | "--help" => return Err(String::new()),
            other => match other.strip_prefix("--mode=") {
                Some(v) => v.to_string(),
                // Also accept `--draw`, `--break`, ... and bare mode names.
                None => other.trim_start_matches("--").to_string(),
            },
        };
        mode = StartMode::parse(&value).ok_or_else(|| format!("unknown mode {value:?}"))?;
    }
    Ok(mode)
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_zoom_it=info".into()),
        )
        .init();

    let mode = match parse_args() {
        Ok(m) => m,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("{e}");
            }
            eprintln!("{USAGE}");
            std::process::exit(2);
        }
    };

    // Single instance: a second hotkey press while an overlay is open is ignored.
    let lock_path = lock_file_path();
    if let Ok(pid_str) = fs::read_to_string(&lock_path)
        && let Ok(pid) = pid_str.trim().parse::<u32>()
        && std::path::Path::new(&format!("/proc/{pid}")).exists()
        && pid != std::process::id()
    {
        info!("Another ZoomIt overlay is already running (pid={pid}), exiting");
        return;
    }
    let _ = fs::write(&lock_path, std::process::id().to_string());
    let guard = LockGuard(lock_path);
    let result = gui::run(mode);
    drop(guard);

    if let Err(e) = result {
        error!("ZoomIt failed: {e:#}");
        std::process::exit(1);
    }
}

struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn lock_file_path() -> PathBuf {
    std::env::temp_dir().join("mpt-zoom-it.lock")
}
