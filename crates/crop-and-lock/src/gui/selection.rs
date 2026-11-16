//! Selection overlay: a borderless window placed exactly over the target
//! window, showing a capture of it. The user drags a rectangle; Esc or a
//! right click cancels (upstream `OverlayWindow`).

use super::{color_image, x11_window_id};
use crate::geometry::{CropRect, display_to_image};
use crate::x11::RootGeometry;
use eframe::egui;
use image::RgbaImage;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::{debug, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    ChangeWindowAttributesAux, ConfigureWindowAux, ConnectionExt as _, InputFocus, StackMode,
};

const TITLE: &str = "Crop And Lock - Select region";
const ACCENT: egui::Color32 = egui::Color32::from_rgb(0, 120, 215);
const MIN_SIZE_PX: u32 = 2;

struct SelectionApp {
    image: RgbaImage,
    texture: Option<egui::TextureHandle>,
    geom: RootGeometry,
    drag_start: Option<egui::Pos2>,
    drag_current: Option<egui::Pos2>,
    result: Arc<Mutex<Option<CropRect>>>,
    configured: bool,
}

impl SelectionApp {
    fn current_rect(&self, screen: egui::Rect) -> Option<CropRect> {
        let (a, b) = (self.drag_start?, self.drag_current?);
        let to_img = |p: egui::Pos2| {
            display_to_image(
                ((p - screen.min).x, (p - screen.min).y),
                screen.width(),
                screen.height(),
                self.image.width(),
                self.image.height(),
            )
        };
        CropRect::from_corners(
            to_img(a),
            to_img(b),
            self.image.width(),
            self.image.height(),
        )
    }
}

impl eframe::App for SelectionApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if !self.configured {
            self.configured = true;
            if let Some(xid) = x11_window_id(frame) {
                let geom = self.geom;
                std::thread::spawn(move || place_over_target(xid, geom));
            }
        }

        let texture = self
            .texture
            .get_or_insert_with(|| {
                ctx.load_texture(
                    "crop-and-lock-target",
                    color_image(&self.image),
                    egui::TextureOptions::LINEAR,
                )
            })
            .clone();

        if ctx.input(|i| i.key_pressed(egui::Key::Escape) || i.pointer.secondary_clicked()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let screen = ui.max_rect();
                let response = ui.allocate_rect(screen, egui::Sense::drag());
                let painter = ui.painter();
                painter.image(
                    texture.id(),
                    screen,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                if response.drag_started() {
                    self.drag_start = response.interact_pointer_pos();
                    self.drag_current = self.drag_start;
                }
                if response.dragged() {
                    self.drag_current = response.interact_pointer_pos().or(self.drag_current);
                }

                let dim = egui::Color32::from_black_alpha(130);
                let selection = match (self.drag_start, self.drag_current) {
                    (Some(a), Some(b)) => Some(egui::Rect::from_two_pos(a, b).intersect(screen)),
                    _ => None,
                };
                match selection {
                    Some(sel) => {
                        // Dim everything except the selection.
                        for r in [
                            egui::Rect::from_min_max(
                                screen.min,
                                egui::pos2(screen.max.x, sel.min.y),
                            ),
                            egui::Rect::from_min_max(
                                egui::pos2(screen.min.x, sel.max.y),
                                screen.max,
                            ),
                            egui::Rect::from_min_max(
                                egui::pos2(screen.min.x, sel.min.y),
                                egui::pos2(sel.min.x, sel.max.y),
                            ),
                            egui::Rect::from_min_max(
                                egui::pos2(sel.max.x, sel.min.y),
                                egui::pos2(screen.max.x, sel.max.y),
                            ),
                        ] {
                            painter.rect_filled(r, 0.0, dim);
                        }
                        painter.rect_stroke(
                            sel,
                            0.0,
                            egui::Stroke::new(2.0, ACCENT),
                            egui::StrokeKind::Outside,
                        );
                        if let Some(r) = self.current_rect(screen) {
                            let label = format!("{} × {}", r.width, r.height);
                            let pos = egui::pos2(sel.min.x, (sel.min.y - 22.0).max(screen.min.y));
                            let galley = painter.layout_no_wrap(
                                label,
                                egui::FontId::proportional(13.0),
                                egui::Color32::WHITE,
                            );
                            let bg = egui::Rect::from_min_size(pos, galley.size())
                                .expand2(egui::vec2(6.0, 2.0));
                            painter.rect_filled(bg, 4.0, egui::Color32::from_black_alpha(200));
                            painter.galley(pos, galley, egui::Color32::WHITE);
                        }
                    }
                    None => {
                        painter.rect_filled(screen, 0.0, dim);
                        painter.text(
                            screen.center(),
                            egui::Align2::CENTER_CENTER,
                            "Drag to select a region  •  Esc to cancel",
                            egui::FontId::proportional(18.0),
                            egui::Color32::WHITE,
                        );
                    }
                }
                painter.rect_stroke(
                    screen,
                    0.0,
                    egui::Stroke::new(2.0, ACCENT),
                    egui::StrokeKind::Inside,
                );

                if response.drag_stopped() {
                    match self.current_rect(screen) {
                        Some(r) if r.width >= MIN_SIZE_PX && r.height >= MIN_SIZE_PX => {
                            *self.result.lock().unwrap() = Some(r);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        _ => {
                            self.drag_start = None;
                            self.drag_current = None;
                        }
                    }
                }
            });
    }
}

/// Make the overlay override-redirect and place it exactly over the target
/// window (in physical pixels), then give it keyboard focus for Esc.
fn place_over_target(xid: u32, geom: RootGeometry) {
    let Ok((conn, _)) = crate::x11::connect() else {
        return;
    };
    let run = || -> anyhow::Result<()> {
        conn.unmap_window(xid)?;
        conn.change_window_attributes(xid, &ChangeWindowAttributesAux::new().override_redirect(1))?;
        conn.configure_window(
            xid,
            &ConfigureWindowAux::new()
                .x(geom.x)
                .y(geom.y)
                .width(geom.width)
                .height(geom.height)
                .stack_mode(StackMode::ABOVE),
        )?;
        conn.map_window(xid)?;
        conn.flush()?;
        for _ in 0..50 {
            std::thread::sleep(Duration::from_millis(20));
            let ok = conn
                .set_input_focus(InputFocus::PARENT, xid, x11rb::CURRENT_TIME)?
                .check()
                .is_ok();
            if ok {
                debug!("Selection overlay focused");
                break;
            }
        }
        conn.flush()?;
        Ok(())
    };
    if let Err(e) = run() {
        warn!("Failed to place selection overlay over target: {e:#}");
    }
}

/// Show the overlay and return the selected region (in target window pixels),
/// or `None` if cancelled.
pub fn run_selection(image: RgbaImage, geom: RootGeometry) -> Option<CropRect> {
    let result = Arc::new(Mutex::new(None));
    let result_clone = result.clone();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_app_id(crate::x11::APP_CLASS)
            .with_decorations(false)
            .with_always_on_top()
            .with_resizable(false)
            .with_position(egui::pos2(geom.x as f32, geom.y as f32))
            .with_inner_size(egui::vec2(geom.width as f32, geom.height as f32)),
        ..Default::default()
    };

    let app = SelectionApp {
        image,
        texture: None,
        geom,
        drag_start: None,
        drag_current: None,
        result: result_clone,
        configured: false,
    };
    if let Err(e) = eframe::run_native(TITLE, options, Box::new(move |_cc| Ok(Box::new(app)))) {
        warn!("Selection overlay failed: {e}");
    }
    result.lock().unwrap().take()
}
