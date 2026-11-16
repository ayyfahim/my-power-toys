use mpt_power_display::gui::{WINDOW_HEIGHT, WINDOW_WIDTH};
use mpt_power_display::{Controller, Feature, PowerDisplayConfig, vcp};
use std::fs;
use std::path::PathBuf;
use tracing::info;

const USAGE: &str = "\
Usage: mpt-power-display [OPTION]

  (no option)            open the flyout
  --list                 list monitors, capabilities and current values
  --apply-profile NAME   apply a saved profile and exit
  --help                 show this help";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_power_display=info".into()),
        )
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        None => {}
        Some("--list") => return list(),
        Some("--apply-profile") => {
            let Some(name) = args.get(1) else {
                eprintln!("--apply-profile needs a profile name\n\n{USAGE}");
                std::process::exit(2);
            };
            return apply_profile(name);
        }
        Some("--help" | "-h") => {
            println!("{USAGE}");
            return;
        }
        Some(other) => {
            eprintln!("unknown option {other:?}\n\n{USAGE}");
            std::process::exit(2);
        }
    }

    // Single instance.
    let lock_path = lock_file_path();
    if let Ok(pid_str) = fs::read_to_string(&lock_path)
        && let Ok(pid) = pid_str.trim().parse::<u32>()
        && pid != std::process::id()
        && std::path::Path::new(&format!("/proc/{pid}")).exists()
    {
        info!("Another instance is already running (pid={pid}), exiting");
        return;
    }
    let _ = fs::write(&lock_path, std::process::id().to_string());
    let _guard = LockGuard(lock_path);

    mpt_power_display::gui::run_flyout(flyout_position());
}

/// Bottom-right corner of the primary monitor, like the upstream tray flyout.
fn flyout_position() -> Option<[f32; 2]> {
    let monitors = mpt_common::monitor::detect_monitors().ok()?;
    let m = monitors
        .iter()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first())?;
    let margin = 16.0;
    let x = m.x as f32 + m.width as f32 - WINDOW_WIDTH - margin;
    let y = m.y as f32 + m.height as f32 - WINDOW_HEIGHT - margin * 3.0;
    Some([x.max(m.x as f32), y.max(m.y as f32)])
}

fn list() {
    let controller = Controller::new(PowerDisplayConfig::load());
    let d = controller.discover();
    for w in &d.warnings {
        eprintln!("warning: {w}");
    }
    for m in &d.monitors {
        println!("{}  [{}]", m.name, m.backend.label());
        println!("  id: {}", m.id);
        if let Some(c) = &m.drm_connector {
            println!("  connector: {c}");
        }
        if let Some(o) = &m.xrandr_output {
            println!("  xrandr output: {o}");
        }
        for (f, info) in &m.features {
            let value = m.value(*f);
            if f.is_continuous() {
                println!(
                    "  {}: {}%",
                    f.label(),
                    value.map_or("?".into(), |v| v.to_string())
                );
            } else {
                let current = value
                    .map(|v| vcp::formatted_value_name(f.vcp_code(), v))
                    .unwrap_or_else(|| "?".into());
                let options: Vec<String> = info
                    .values
                    .iter()
                    .map(|v| vcp::formatted_value_name(f.vcp_code(), *v))
                    .collect();
                println!(
                    "  {}: {current}  (options: {})",
                    f.label(),
                    options.join(", ")
                );
            }
        }
        if !m.supports(Feature::Brightness) {
            println!("  (brightness not controllable)");
        }
    }
}

fn apply_profile(name: &str) {
    let config = PowerDisplayConfig::load();
    let Some(profile) = config.profile(name).cloned() else {
        eprintln!("profile {name:?} not found");
        std::process::exit(1);
    };
    let controller = Controller::new(config);
    let mut monitors = controller.discover().monitors;
    let errors = controller.apply_profile(&mut monitors, &profile);
    for e in &errors {
        eprintln!("error: {e}");
    }
    if !errors.is_empty() {
        std::process::exit(1);
    }
}

fn lock_file_path() -> PathBuf {
    std::env::temp_dir().join("mpt-power-display.lock")
}

struct LockGuard(PathBuf);

impl Drop for LockGuard {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
