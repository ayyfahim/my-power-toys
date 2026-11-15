use std::fs;
use std::path::PathBuf;
use tracing::info;

/// Removes the PID lock file when dropped.
struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn lock_file_path() -> PathBuf {
    std::env::temp_dir().join("mpt-environment-variables.lock")
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_environment_variables=info".into()),
        )
        .init();

    // Prevent multiple instances.
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
    let _ = fs::write(&lock_path, std::process::id().to_string());
    let _guard = LockGuard(lock_path);

    let config = mpt_environment_variables::load_config();
    mpt_environment_variables::gui::run(config);
}
