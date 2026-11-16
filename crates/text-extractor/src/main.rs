//! `mpt-text-extractor`: region-selection overlay → tesseract → clipboard.

use image::GenericImageView;
use mpt_text_extractor::overlay::{OverlayOptions, run_overlay};
use mpt_text_extractor::{TextExtractorConfig, ocr, screenshot};
use std::fs;
use std::path::PathBuf;
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
                .unwrap_or_else(|_| "mpt_text_extractor=info".into()),
        )
        .init();

    // Prevent multiple instances
    let lock_path = std::env::temp_dir().join("mpt-text-extractor.lock");
    if let Ok(pid_str) = fs::read_to_string(&lock_path)
        && let Ok(pid) = pid_str.trim().parse::<u32>()
        && std::path::Path::new(&format!("/proc/{pid}")).exists()
    {
        info!("Another instance is already running (pid={pid}), exiting");
        return;
    }
    let _ = fs::write(&lock_path, std::process::id().to_string());
    let _guard = LockGuard(lock_path);

    let config: TextExtractorConfig =
        mpt_common::config::load_module_config("text-extractor").unwrap_or_default();

    let shot = match screenshot::capture_fullscreen() {
        Ok(img) => img,
        Err(e) => {
            error!("Failed to capture screenshot: {e:#}");
            std::process::exit(1);
        }
    };

    let options = OverlayOptions {
        languages: ocr::list_languages(),
        language: config.language.clone(),
        mode: config.mode(),
    };
    let Some(sel) = run_overlay(shot.clone(), options) else {
        info!("Text extraction cancelled");
        return;
    };

    let region = shot.view(sel.x, sel.y, sel.width, sel.height).to_image();
    let tmp = std::env::temp_dir().join(format!("mpt-ocr-region-{}.png", std::process::id()));
    if let Err(e) = region.save(&tmp) {
        error!("Failed to save selected region: {e}");
        std::process::exit(1);
    }
    let result = ocr::run_tesseract(&tmp, &sel.language, sel.mode);
    let _ = fs::remove_file(&tmp);

    match result {
        Ok(text) if text.is_empty() => info!("No text detected"),
        Ok(text) => {
            info!(
                "Extracted {} chars (lang={}, {:?})",
                text.len(),
                sel.language,
                sel.mode
            );
            if let Err(e) = ocr::copy_to_clipboard(&text) {
                error!("Failed to copy to clipboard: {e:#}");
                std::process::exit(1);
            }
        }
        Err(e) => {
            error!("OCR failed: {e:#}");
            std::process::exit(1);
        }
    }
}
