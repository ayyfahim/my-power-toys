use std::fs;
use std::path::PathBuf;
use tracing::{error, info};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_fancy_zones=info".into()),
        )
        .init();

    // Prevent multiple instances
    let lock_path = lock_file_path();
    if lock_path.exists() {
        if let Ok(pid_str) = fs::read_to_string(&lock_path)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
        {
            let proc_path = format!("/proc/{pid}");
            if std::path::Path::new(&proc_path).exists() {
                info!("Another instance is already running (pid={pid}), exiting");
                std::process::exit(0);
            }
        }
        let _ = fs::remove_file(&lock_path);
    }

    let _ = fs::write(&lock_path, std::process::id().to_string());
    let lock_path_cleanup = lock_path.clone();
    let _guard = scopeguard::guard((), move |_| {
        let _ = fs::remove_file(&lock_path_cleanup);
    });

    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--editor") {
        run_editor_mode();
    } else if let Some(window_id) = parse_window_id(&args) {
        run_snap_mode(window_id);
    } else {
        error!("Usage: mpt-fancy-zones --editor");
        error!("       mpt-fancy-zones --window-id <WINDOW_ID>");
        std::process::exit(1);
    }
}

fn run_editor_mode() {
    info!("FancyZones editor starting");

    let config = mpt_fancy_zones::config::FancyZonesConfig::load();

    let monitors = match mpt_common::monitor::detect_monitors() {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to detect monitors: {e}");
            // Provide a fallback single monitor from X11 screen geometry
            match mpt_fancy_zones::x11::get_screen_geometry() {
                Ok((w, h)) => vec![mpt_common::monitor::Monitor {
                    name: "Screen".to_string(),
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                    is_primary: true,
                }],
                Err(e2) => {
                    error!("Cannot determine screen size: {e2}");
                    std::process::exit(1);
                }
            }
        }
    };

    info!("Detected {} monitor(s)", monitors.len());
    mpt_fancy_zones::editor::run_editor(monitors, config);
}

fn run_snap_mode(window_id: u32) {
    info!("FancyZones overlay starting for window {window_id}");

    let config = mpt_fancy_zones::config::FancyZonesConfig::load();

    if let Ok(class) = mpt_fancy_zones::x11::get_wm_class(window_id)
        && config.is_excluded(&class)
    {
        info!("Window {window_id} ({class:?}) is excluded from FancyZones");
        return;
    }

    // Detect which monitor the window is on
    let monitor = match mpt_fancy_zones::x11::find_monitor_for_window(window_id) {
        Ok(m) => m,
        Err(e) => {
            info!("Could not detect monitor for window ({e}), using full screen");
            match mpt_fancy_zones::x11::get_screen_geometry() {
                Ok((w, h)) => mpt_common::monitor::Monitor {
                    name: "Screen".to_string(),
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                    is_primary: true,
                },
                Err(e2) => {
                    error!("Cannot determine screen size: {e2}");
                    std::process::exit(1);
                }
            }
        }
    };

    info!(
        "Window is on monitor '{}' ({}x{} at {},{})",
        monitor.name, monitor.width, monitor.height, monitor.x, monitor.y
    );

    // Get layout for this monitor
    let layout = config
        .layout_for_monitor(&monitor.name)
        .cloned()
        .unwrap_or_else(|| mpt_fancy_zones::layout::Layout::default_columns(2));

    if layout.zones.is_empty() {
        info!(
            "No layout configured for monitor '{}', nothing to do",
            monitor.name
        );
        return;
    }

    let style = mpt_fancy_zones::zone_window::ZoneStyle::from_config(&config);

    // Show overlay on the target monitor
    let selected = mpt_fancy_zones::overlay::run_overlay_styled(layout.clone(), style, &monitor);

    match selected {
        Some(zone_idx) => {
            info!("Zone {zone_idx} selected, snapping window {window_id}");

            if zone_idx < layout.zones.len() {
                let target = mpt_fancy_zones::snap::Target {
                    name: monitor.name.clone(),
                    area: mpt_fancy_zones::geometry::Rect::from_monitor(&monitor),
                    zones: mpt_fancy_zones::geometry::zone_rects(
                        &layout,
                        mpt_fancy_zones::geometry::Rect::from_monitor(&monitor),
                    ),
                    layout: layout.clone(),
                };
                let result = mpt_fancy_zones::x11::Wm::connect().and_then(|wm| {
                    let size_before = wm.geometry(window_id).map(|g| (g.w as u32, g.h as u32));
                    mpt_fancy_zones::snap::snap_window(
                        &wm,
                        &config,
                        window_id,
                        &target,
                        &[zone_idx],
                        size_before,
                    )
                });
                if let Err(e) = result {
                    error!("Failed to snap window: {e}");
                }
            }
        }
        None => {
            info!("Zone selection cancelled");
        }
    }
}

fn parse_window_id(args: &[String]) -> Option<u32> {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--window-id" {
            return args.get(i + 1)?.parse().ok();
        }
    }
    None
}

fn lock_file_path() -> PathBuf {
    std::env::temp_dir().join("mpt-fancy-zones.lock")
}
