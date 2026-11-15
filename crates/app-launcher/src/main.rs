use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use tracing::info;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_app_launcher=info".into()),
        )
        .init();

    // Single instance: PID lock file.
    let lock_path = lock_file_path();
    if lock_path.exists() {
        if let Ok(pid_str) = fs::read_to_string(&lock_path)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
            && std::path::Path::new(&format!("/proc/{pid}")).exists()
        {
            info!("Another instance is already running (pid={pid}), exiting");
            std::process::exit(0);
        }
        let _ = fs::remove_file(&lock_path);
    }
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(mut file) => {
            let _ = file.write_all(std::process::id().to_string().as_bytes());
        }
        Err(_) => {
            info!("Lock file contention, another instance likely starting, exiting");
            std::process::exit(0);
        }
    }
    let lock_cleanup = lock_path.clone();
    let _guard = scopeguard::guard((), move |_| {
        let _ = fs::remove_file(&lock_cleanup);
    });

    let config: mpt_app_launcher::AppLauncherConfig =
        mpt_common::config::load_module_config("app-launcher").unwrap_or_default();

    // Resolve placement before our window exists, so "focused" still refers
    // to the previously active window.
    let origin = mpt_app_launcher::placement::resolve_origin(
        config.position,
        mpt_app_launcher::gui::theme::WINDOW_WIDTH,
    );

    info!("App Launcher GUI starting");
    mpt_app_launcher::gui::overlay::run_launcher(config, origin);
}

fn lock_file_path() -> PathBuf {
    std::env::temp_dir().join("mpt-app-launcher.lock")
}
