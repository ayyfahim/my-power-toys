use mpt_color_picker::color::Color;
use mpt_color_picker::gui::editor::{EditorExit, EditorOptions, run_editor_with, spawn_picker};
use mpt_color_picker::gui::overlay::{OverlayOptions, run_overlay_with};
use mpt_color_picker::history::ColorHistory;
use mpt_color_picker::picker::copy_to_clipboard;
use mpt_color_picker::screenshot::capture_fullscreen;
use mpt_color_picker::{ActivationBehavior, ClickAction, ColorPickerConfig};
use std::fs;
use std::path::PathBuf;
use tracing::{error, info, warn};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "mpt_color_picker=info".into()),
        )
        .init();

    // `--pick` forces pick mode (used when the editor's "Pick color" relaunches us).
    let force_pick = std::env::args().any(|a| a == "--pick");

    // Prevent multiple instances
    let lock_path = lock_file_path();
    if force_pick {
        // The editor that relaunched us may still be shutting down.
        for _ in 0..40 {
            if !lock_held_by_live_process(&lock_path) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
    if lock_path.exists() {
        // Check if the process is still alive
        if let Ok(pid_str) = fs::read_to_string(&lock_path)
            && let Ok(pid) = pid_str.trim().parse::<u32>()
        {
            let proc_path = format!("/proc/{pid}");
            if std::path::Path::new(&proc_path).exists() {
                info!("Another instance is already running (pid={pid}), exiting");
                std::process::exit(0);
            }
        }
        // Stale lock file, remove it
        let _ = fs::remove_file(&lock_path);
    }

    // Write our PID
    let _ = fs::write(&lock_path, std::process::id().to_string());

    // Ensure lock file is cleaned up on exit
    let lock_path_cleanup = lock_path.clone();
    let _guard = scopeguard::guard((), move |_| {
        let _ = fs::remove_file(&lock_path_cleanup);
    });

    // Load config
    let config: ColorPickerConfig =
        mpt_common::config::load_module_config("color-picker").unwrap_or_default();

    info!("Color Picker GUI starting (behavior={:?})", config.behavior);

    let editor_options = EditorOptions {
        formats: config.enabled_formats(),
        show_color_name: config.show_color_name,
    };
    let behavior = if force_pick {
        ActivationBehavior::PickAndEdit
    } else {
        config.behavior
    };

    let exit = match behavior {
        ActivationBehavior::EditorOnly => {
            let history = ColorHistory::load();
            let color = history
                .entries
                .first()
                .map(|e| e.color)
                .unwrap_or(Color::new(128, 128, 128));
            run_editor_with(color, history, editor_options)
        }
        ActivationBehavior::PickAndClose | ActivationBehavior::PickAndEdit => {
            // Capture screenshot
            let screenshot = match capture_fullscreen() {
                Ok(img) => img,
                Err(e) => {
                    error!("Failed to capture screenshot: {e}");
                    std::process::exit(1);
                }
            };

            // Show overlay, get picked color
            let overlay_options = OverlayOptions {
                tooltip_format: config.format,
                show_color_name: config.show_color_name,
                change_cursor: config.change_cursor,
                // In forced pick mode (from the editor) a left click returns to the editor.
                primary_click: if force_pick {
                    ClickAction::PickColorThenEditor
                } else {
                    config.primary_click()
                },
                middle_click: config.middle_click_action,
                secondary_click: config.secondary_click_action,
            };
            let picked = run_overlay_with(screenshot, overlay_options);

            match picked {
                Some((color, action)) => {
                    info!("Picked color: {color} ({action:?})");

                    // Copy to clipboard
                    let formatted = color.format(config.format);
                    if let Err(e) = copy_to_clipboard(&formatted) {
                        error!("Failed to copy to clipboard: {e}");
                    } else {
                        info!("Copied to clipboard: {formatted}");
                    }

                    // Save to history
                    let mut history = ColorHistory::load();
                    history.push_with_limit(color, config.history_limit());
                    if let Err(e) = history.save() {
                        warn!("Failed to save history: {e}");
                    }

                    // Open editor if the click action asks for it
                    if action == ClickAction::PickColorThenEditor {
                        run_editor_with(color, history, editor_options)
                    } else {
                        EditorExit::Closed
                    }
                }
                None => {
                    info!("Color picking cancelled");
                    EditorExit::Closed
                }
            }
        }
    };

    if exit == EditorExit::PickAgain {
        // Release the lock before relaunching so the new instance can start.
        drop(_guard);
        spawn_picker();
    }
}

fn lock_held_by_live_process(lock_path: &std::path::Path) -> bool {
    fs::read_to_string(lock_path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .is_some_and(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists())
}

fn lock_file_path() -> PathBuf {
    std::env::temp_dir().join("mpt-color-picker.lock")
}
