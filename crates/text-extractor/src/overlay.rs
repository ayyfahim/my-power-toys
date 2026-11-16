//! Region-selection overlay: frozen screenshot, drag to select, toolbar with
//! language picker and single-line / table toggles. Esc cancels.

use crate::ocr::OcrMode;
use eframe::egui;
use image::RgbaImage;
use std::sync::{Arc, Mutex};

const WINDOW_TITLE: &str = "Text Extractor";
const MIN_SELECTION: f32 = 4.0;

/// What the user selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Region in screenshot pixels: x, y, width, height.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub language: String,
    pub mode: OcrMode,
}

/// Initial toolbar state.
#[derive(Debug, Clone)]
pub struct OverlayOptions {
    pub languages: Vec<String>,
    pub language: String,
    pub mode: OcrMode,
}

struct OverlayApp {
    screenshot: RgbaImage,
    texture: Option<egui::TextureHandle>,
    languages: Vec<String>,
    language: String,
    mode: OcrMode,
    drag_start: Option<egui::Pos2>,
    result: Arc<Mutex<Option<Selection>>>,
    should_close: bool,
}

/// Convert a rectangle in window points to screenshot pixels, clamped to the image.
pub fn rect_to_pixels(
    min: (f32, f32),
    max: (f32, f32),
    screen: (f32, f32),
    image: (u32, u32),
) -> Option<(u32, u32, u32, u32)> {
    if screen.0 <= 0.0 || screen.1 <= 0.0 || image.0 == 0 || image.1 == 0 {
        return None;
    }
    let sx = image.0 as f32 / screen.0;
    let sy = image.1 as f32 / screen.1;
    let x0 = (min.0.min(max.0) * sx).floor().clamp(0.0, image.0 as f32) as u32;
    let y0 = (min.1.min(max.1) * sy).floor().clamp(0.0, image.1 as f32) as u32;
    let x1 = (min.0.max(max.0) * sx).ceil().clamp(0.0, image.0 as f32) as u32;
    let y1 = (min.1.max(max.1) * sy).ceil().clamp(0.0, image.1 as f32) as u32;
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

impl OverlayApp {
    fn toolbar(&mut self, ctx: &egui::Context, screen: egui::Vec2) {
        egui::Area::new(egui::Id::new("ocr_toolbar"))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(screen.x / 2.0 - 220.0, 16.0))
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .inner_margin(egui::Margin::symmetric(12, 8))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Language");
                            egui::ComboBox::from_id_salt("ocr_lang")
                                .selected_text(self.language.clone())
                                .show_ui(ui, |ui| {
                                    for lang in &self.languages {
                                        ui.selectable_value(
                                            &mut self.language,
                                            lang.clone(),
                                            lang.as_str(),
                                        );
                                    }
                                });
                            ui.separator();
                            ui.checkbox(&mut self.mode.single_line, "Single line");
                            ui.checkbox(&mut self.mode.table, "Table");
                            ui.separator();
                            if ui.button("Cancel (Esc)").clicked() {
                                self.should_close = true;
                            }
                        });
                    });
            });
    }
}

impl eframe::App for OverlayApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.texture.is_none() {
            let size = [
                self.screenshot.width() as usize,
                self.screenshot.height() as usize,
            ];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, self.screenshot.as_raw());
            self.texture =
                Some(ctx.load_texture("ocr_screenshot", image, egui::TextureOptions::LINEAR));
        }

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.should_close = true;
        }

        let mut selection_rect = None;
        let screen = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let available = ui.available_size();
                let full = egui::Rect::from_min_size(egui::Pos2::ZERO, available);
                if let Some(tex) = &self.texture {
                    ui.painter().image(
                        tex.id(),
                        full,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                let resp = ui.interact(full, egui::Id::new("ocr_drag"), egui::Sense::drag());
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);

                if resp.drag_started() {
                    self.drag_start = resp.interact_pointer_pos();
                }
                let current = ctx.input(|i| i.pointer.hover_pos());
                let dim = egui::Color32::from_black_alpha(110);
                match (self.drag_start, current) {
                    (Some(start), Some(cur)) => {
                        let sel = egui::Rect::from_two_pos(start, cur);
                        // Dim everything outside the selection.
                        let p = ui.painter();
                        p.rect_filled(
                            egui::Rect::from_min_max(full.min, egui::pos2(full.max.x, sel.min.y)),
                            0.0,
                            dim,
                        );
                        p.rect_filled(
                            egui::Rect::from_min_max(egui::pos2(full.min.x, sel.max.y), full.max),
                            0.0,
                            dim,
                        );
                        p.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(full.min.x, sel.min.y),
                                egui::pos2(sel.min.x, sel.max.y),
                            ),
                            0.0,
                            dim,
                        );
                        p.rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(sel.max.x, sel.min.y),
                                egui::pos2(full.max.x, sel.max.y),
                            ),
                            0.0,
                            dim,
                        );
                        p.rect_stroke(
                            sel,
                            0.0,
                            egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 120, 215)),
                            egui::StrokeKind::Outside,
                        );
                    }
                    _ => {
                        ui.painter().rect_filled(full, 0.0, dim);
                    }
                }
                if resp.drag_stopped() {
                    if let Some(start) = self.drag_start
                        && let Some(end) = resp.interact_pointer_pos().or(current)
                    {
                        selection_rect = Some(egui::Rect::from_two_pos(start, end));
                    }
                    self.drag_start = None;
                }
                available
            })
            .inner;

        self.toolbar(ctx, screen);

        if let Some(sel) = selection_rect
            && sel.width() >= MIN_SELECTION
            && sel.height() >= MIN_SELECTION
            && let Some((x, y, width, height)) = rect_to_pixels(
                (sel.min.x, sel.min.y),
                (sel.max.x, sel.max.y),
                (screen.x, screen.y),
                (self.screenshot.width(), self.screenshot.height()),
            )
        {
            *self.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(Selection {
                x,
                y,
                width,
                height,
                language: self.language.clone(),
                mode: self.mode,
            });
            self.should_close = true;
        }

        if self.should_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

/// Show the overlay and return the selection, or `None` if cancelled.
pub fn run_overlay(screenshot: RgbaImage, options: OverlayOptions) -> Option<Selection> {
    let result = Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    let img_w = screenshot.width() as f32;
    let img_h = screenshot.height() as f32;
    let on_x11 = std::env::var("WAYLAND_DISPLAY").is_err() && std::env::var("DISPLAY").is_ok();
    if on_x11 {
        let (w, h) = (screenshot.width(), screenshot.height());
        std::thread::spawn(move || x11_configure_overlay(w, h));
    }

    let viewport = if on_x11 {
        egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_decorations(false)
            .with_always_on_top()
            .with_position(egui::pos2(0.0, 0.0))
            .with_inner_size(egui::vec2(img_w, img_h))
            .with_resizable(false)
    } else {
        egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_fullscreen(true)
            .with_decorations(false)
            .with_always_on_top()
    };

    let mut languages = options.languages;
    if !languages.contains(&options.language) {
        languages.insert(0, options.language.clone());
    }

    let _ = eframe::run_native(
        WINDOW_TITLE,
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(move |cc| {
            mpt_common::theme::Theme::dark().apply(&cc.egui_ctx);
            Ok(Box::new(OverlayApp {
                screenshot,
                texture: None,
                languages,
                language: options.language,
                mode: options.mode,
                drag_start: None,
                result: result_clone,
                should_close: false,
            }))
        }),
    );

    result.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// On X11, set override_redirect on our window so it spans all monitors
/// (same approach as the color picker overlay).
fn x11_configure_overlay(width: u32, height: u32) {
    use std::process::Command;
    use std::thread;
    use std::time::Duration;

    for _ in 0..100 {
        thread::sleep(Duration::from_millis(30));
        let Ok(output) = Command::new("xdotool")
            .args(["search", "--name", WINDOW_TITLE])
            .output()
        else {
            return;
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        let Some(wid) = stdout
            .lines()
            .last()
            .map(str::trim)
            .filter(|w| !w.is_empty())
        else {
            continue;
        };
        let _ = Command::new("xdotool")
            .args(["set_window", "--overrideredirect", "1", wid])
            .status();
        let _ = Command::new("xdotool")
            .args(["windowsize", wid, &width.to_string(), &height.to_string()])
            .status();
        let _ = Command::new("xdotool")
            .args(["windowmove", wid, "0", "0"])
            .status();
        let _ = Command::new("xdotool")
            .args(["windowactivate", "--sync", wid])
            .status();
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scales_hidpi_rect() {
        // 1920x1080 points over a 3840x2160 screenshot
        assert_eq!(
            rect_to_pixels((10.0, 20.0), (110.0, 70.0), (1920.0, 1080.0), (3840, 2160)),
            Some((20, 40, 200, 100))
        );
    }

    #[test]
    fn reversed_and_clamped() {
        assert_eq!(
            rect_to_pixels((150.0, 150.0), (50.0, 50.0), (100.0, 100.0), (100, 100)),
            Some((50, 50, 50, 50))
        );
        assert_eq!(
            rect_to_pixels((0.0, 0.0), (0.0, 10.0), (100.0, 100.0), (100, 100)),
            None
        );
    }
}
