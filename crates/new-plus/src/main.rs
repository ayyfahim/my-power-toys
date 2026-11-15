use mpt_new_plus::NewPlusConfig;
use mpt_new_plus::templates::{
    create_from_template, ensure_templates_dir, file_uri_to_path, find_template, list_templates,
};
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use tracing::{info, warn};

const USAGE: &str = "Usage:
  mpt-new-plus                          open the New+ window
  mpt-new-plus list                     list templates
  mpt-new-plus create <template> [dir]  create <template> in [dir]
                                        (default: $NAUTILUS_SCRIPT_CURRENT_URI or cwd)";

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_new_plus=info".into()),
        )
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = NewPlusConfig::load();
    let dir = config.templates_dir();
    if let Err(e) = ensure_templates_dir(&dir) {
        warn!("New+: {e:#}");
    }

    match args.first().map(String::as_str) {
        None => {
            run_gui(config);
            ExitCode::SUCCESS
        }
        Some("list") => match list_templates(&dir) {
            Ok(list) => {
                for t in list {
                    let kind = if t.is_dir { "folder" } else { "file" };
                    println!("{}\t{kind}\t{}", t.display_name(&config), t.file_name);
                }
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("error: {e:#}");
                ExitCode::FAILURE
            }
        },
        Some("create") if args.len() == 2 || args.len() == 3 => {
            let target = match args.get(2) {
                Some(d) => PathBuf::from(d),
                None => default_target_dir(),
            };
            let result = find_template(&dir, &args[1], &config)
                .and_then(|t| create_from_template(&t.path, &target, &config));
            match result {
                Ok(path) => {
                    println!("{}", path.display());
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e:#}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Nautilus passes the current folder as `NAUTILUS_SCRIPT_CURRENT_URI`; otherwise use cwd.
fn default_target_dir() -> PathBuf {
    if let Ok(uri) = std::env::var("NAUTILUS_SCRIPT_CURRENT_URI") {
        match file_uri_to_path(&uri) {
            Some(p) => return p,
            None => warn!("New+: non-local folder {uri}, using current directory"),
        }
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn run_gui(config: NewPlusConfig) {
    let lock_path = lock_file_path();
    if lock_path.exists() {
        if let Ok(pid_str) = fs::read_to_string(&lock_path)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
            && std::path::Path::new(&format!("/proc/{pid}")).exists()
        {
            info!("Another instance is already running (pid={pid}), exiting");
            return;
        }
        let _ = fs::remove_file(&lock_path);
    }
    let _ = fs::write(&lock_path, std::process::id().to_string());
    let _guard = LockGuard(lock_path);

    let start_dir = std::env::current_dir().unwrap_or_default();
    mpt_new_plus::gui::run_window(config, start_dir);
}

struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn lock_file_path() -> PathBuf {
    let dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(dir).join("mpt-new-plus.lock")
}
