//! The cropped windows: a live thumbnail (upstream `ThumbnailCropAndLockWindow`)
//! and a static snapshot (upstream `ScreenshotCropAndLockWindow`).

use super::{color_image, paint_fitted};
use crate::capture::WindowCapturer;
use crate::geometry::CropRect;
use eframe::egui;
use image::RgbaImage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};
use x11rb::connection::Connection;

/// Live refresh interval (~30 fps).
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

struct Shared {
    latest: Mutex<Option<RgbaImage>>,
    source_gone: AtomicBool,
    stop: AtomicBool,
}

enum Content {
    Live { shared: Arc<Shared> },
    Static { image: Option<RgbaImage> },
}

struct CroppedApp {
    content: Content,
    texture: Option<egui::TextureHandle>,
    source: u32,
}

impl CroppedApp {
    fn take_new_frame(&mut self) -> Option<RgbaImage> {
        match &mut self.content {
            Content::Live { shared } => shared.latest.lock().unwrap().take(),
            Content::Static { image } => image.take(),
        }
    }
}

impl eframe::App for CroppedApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Content::Live { shared } = &self.content
            && shared.source_gone.load(Ordering::Relaxed)
        {
            info!("Source window closed; closing thumbnail");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if let Some(img) = self.take_new_frame() {
            let ci = color_image(&img);
            match &mut self.texture {
                Some(t) if t.size() == [img.width() as usize, img.height() as usize] => {
                    t.set(ci, egui::TextureOptions::LINEAR);
                }
                _ => {
                    self.texture =
                        Some(ctx.load_texture("crop-and-lock", ci, egui::TextureOptions::LINEAR));
                }
            }
        }

        let live = matches!(self.content, Content::Live { .. });
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(egui::Color32::BLACK))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let response = ui.allocate_rect(rect, egui::Sense::click());
                if let Some(tex) = &self.texture {
                    paint_fitted(ui.painter(), rect, tex);
                }
                // Clicking a live thumbnail brings the source window forward.
                if live && response.clicked() {
                    activate_source(self.source);
                }
            });

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn activate_source(window: u32) {
    let res = crate::x11::connect().and_then(|(conn, screen)| {
        let root = conn.setup().roots[screen].root;
        crate::x11::activate_window(&conn, root, window)
    });
    if let Err(e) = res {
        warn!("Failed to activate source window: {e:#}");
    }
}

fn viewport(title: &str, rect: CropRect) -> egui::ViewportBuilder {
    egui::ViewportBuilder::default()
        .with_title(title)
        .with_app_id(crate::x11::APP_CLASS)
        .with_inner_size(egui::vec2(rect.width as f32, rect.height as f32))
        .with_min_inner_size(egui::vec2(16.0, 16.0))
        .with_resizable(true)
}

/// Live thumbnail of `rect` in the source window. Returns when the window is
/// closed or the source window is destroyed.
pub fn run_thumbnail(capturer: WindowCapturer, rect: CropRect, title: &str) {
    let source = capturer.window();
    let first = capturer.capture(Some(rect)).ok();
    let shared = Arc::new(Shared {
        latest: Mutex::new(first),
        source_gone: AtomicBool::new(false),
        stop: AtomicBool::new(false),
    });

    let options = eframe::NativeOptions {
        viewport: viewport(title, rect),
        ..Default::default()
    };
    let shared_app = shared.clone();
    let result = eframe::run_native(
        title,
        options,
        Box::new(move |cc| {
            let ctx = cc.egui_ctx.clone();
            let shared_thread = shared_app.clone();
            std::thread::spawn(move || capture_loop(capturer, rect, shared_thread, ctx));
            Ok(Box::new(CroppedApp {
                content: Content::Live { shared: shared_app },
                texture: None,
                source,
            }))
        }),
    );
    shared.stop.store(true, Ordering::Relaxed);
    if let Err(e) = result {
        warn!("Thumbnail window failed: {e}");
    }
}

fn capture_loop(capturer: WindowCapturer, rect: CropRect, shared: Arc<Shared>, ctx: egui::Context) {
    let mut failures = 0u32;
    while !shared.stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        match capturer.capture(Some(rect)) {
            Ok(img) => {
                failures = 0;
                *shared.latest.lock().unwrap() = Some(img);
                ctx.request_repaint();
            }
            Err(e) => {
                if !capturer.is_alive() {
                    shared.source_gone.store(true, Ordering::Relaxed);
                    ctx.request_repaint();
                    break;
                }
                // Unmapped/minimized windows cannot be read; keep the last frame.
                failures += 1;
                if failures == 1 {
                    debug!("Capture failed (keeping last frame): {e:#}");
                }
            }
        }
        let _ = capturer.connection().flush();
        if let Some(rest) = FRAME_INTERVAL.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
    }
    // Dropping the capturer un-redirects the source window.
}

/// Static snapshot window (upstream screenshot mode).
pub fn run_snapshot(image: RgbaImage, rect: CropRect, title: &str, source: u32) {
    let options = eframe::NativeOptions {
        viewport: viewport(title, rect),
        ..Default::default()
    };
    let app = CroppedApp {
        content: Content::Static { image: Some(image) },
        texture: None,
        source,
    };
    if let Err(e) = eframe::run_native(title, options, Box::new(move |_cc| Ok(Box::new(app)))) {
        warn!("Screenshot window failed: {e}");
    }
}
