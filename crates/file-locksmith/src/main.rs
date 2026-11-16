use std::fs;
use std::path::PathBuf;
use tracing::info;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_file_locksmith=info".into()),
        )
        .init();

    let mut elevated = false;
    let mut paths = Vec::new();
    let mut only_paths = false;
    for arg in std::env::args_os().skip(1) {
        if !only_paths {
            match arg.to_str() {
                Some("--") => {
                    only_paths = true;
                    continue;
                }
                Some("--elevated") => {
                    elevated = true;
                    continue;
                }
                Some("-h" | "--help") => {
                    println!("Usage: mpt-file-locksmith [--elevated] [--] [PATH...]");
                    println!("Shows which processes are using the given files or folders.");
                    return;
                }
                _ => {}
            }
        }
        paths.push(PathBuf::from(arg));
    }

    // Launched from the hotkey (no paths): allow a single empty window only.
    // Context-menu launches carry paths and may open several windows, like upstream.
    let _guard = if paths.is_empty() && !elevated {
        let lock_path = std::env::temp_dir().join("mpt-file-locksmith.lock");
        if let Ok(pid_str) = fs::read_to_string(&lock_path)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
            && pid != std::process::id()
            && std::path::Path::new(&format!("/proc/{pid}")).exists()
        {
            info!("Another instance is already running (pid={pid}), exiting");
            return;
        }
        let _ = fs::write(&lock_path, std::process::id().to_string());
        Some(scopeguard::guard(lock_path, |p| {
            let _ = fs::remove_file(p);
        }))
    } else {
        None
    };

    let paths = mpt_file_locksmith::scan::normalize_targets(&paths);
    mpt_file_locksmith::gui::run(paths);
}
