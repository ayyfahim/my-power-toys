//! Snip: select a region of the frozen screenshot and copy (or save) it.

use super::{Shared, Transition, from_rect, to_rect};
use crate::geometry::{self, Point, Rect};
use crate::output;
use eframe::egui::{self, Color32};
use tracing::{error, info};

pub(crate) struct SnipState {
    save: bool,
    start: Option<egui::Pos2>,
    end: Option<egui::Pos2>,
}

impl SnipState {
    pub fn new(save: bool) -> Self {
        Self {
            save,
            start: None,
            end: None,
        }
    }

    pub fn update(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        screen: egui::Rect,
        shared: &mut Shared,
    ) -> Transition {
        let (esc, right, pos, pressed, down, released) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.pointer.secondary_clicked(),
                i.pointer.latest_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
            )
        });
        if esc || right {
            return Transition::Exit;
        }
        if let Some(p) = pos {
            if pressed {
                self.start = Some(p);
            }
            if down && self.start.is_some() {
                self.end = Some(p);
            }
        }

        // Paint: dimmed screenshot with the selection cut out.
        if let Some(tex) = shared.texture(ctx) {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(tex.id(), screen, uv, Color32::WHITE);
        }
        let dim = Color32::from_black_alpha(110);
        let sel = self.selection();
        match sel {
            Some(r) => {
                let r = from_rect(r);
                for band in [
                    egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, r.min.y)),
                    egui::Rect::from_min_max(egui::pos2(screen.min.x, r.max.y), screen.max),
                    egui::Rect::from_min_max(
                        egui::pos2(screen.min.x, r.min.y),
                        egui::pos2(r.min.x, r.max.y),
                    ),
                    egui::Rect::from_min_max(
                        egui::pos2(r.max.x, r.min.y),
                        egui::pos2(screen.max.x, r.max.y),
                    ),
                ] {
                    painter.rect_filled(band, 0.0, dim);
                }
                painter.rect_stroke(
                    r,
                    0.0,
                    egui::Stroke::new(1.5, Color32::WHITE),
                    egui::StrokeKind::Outside,
                );
                let label = format!("{:.0} × {:.0}", r.width(), r.height());
                painter.text(
                    r.left_top() - egui::vec2(0.0, 4.0),
                    egui::Align2::LEFT_BOTTOM,
                    label,
                    egui::FontId::proportional(13.0),
                    Color32::WHITE,
                );
            }
            None => {
                painter.rect_filled(screen, 0.0, dim);
            }
        }
        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
        shared.hint(
            painter,
            screen,
            if self.save {
                "Snip to file · drag to select · Esc cancel"
            } else {
                "Snip to clipboard · drag to select · Esc cancel"
            },
        );

        if released && let Some(r) = sel {
            self.finish(r, screen, shared);
            return Transition::Exit;
        }
        if released {
            self.start = None;
            self.end = None;
        }
        Transition::Stay
    }

    fn selection(&self) -> Option<Rect> {
        let (a, b) = (self.start?, self.end?);
        let r = geometry::normalize_rect(Point::new(a.x, a.y), Point::new(b.x, b.y));
        (r.w >= 2.0 && r.h >= 2.0).then_some(r)
    }

    fn finish(&self, sel: Rect, screen: egui::Rect, shared: &Shared) {
        let Some(cap) = &shared.capture else { return };
        let img = &cap.image;
        let px = Rect::new(0.0, 0.0, img.width() as f32, img.height() as f32);
        let screen_r = to_rect(screen);
        let a = screen_r.map_to(Point::new(sel.x, sel.y), &px);
        let b = screen_r.map_to(Point::new(sel.right(), sel.bottom()), &px);
        let Some((x, y, w, h)) =
            geometry::pixel_crop(&geometry::normalize_rect(a, b), img.width(), img.height())
        else {
            return;
        };
        let cropped = image::imageops::crop_imm(img, x, y, w, h).to_image();
        if self.save {
            match output::save_png(&cropped, &shared.cfg.screenshot_dir()) {
                Ok(p) => info!("Snip saved to {}", p.display()),
                Err(e) => error!("Snip save failed: {e:#}"),
            }
        }
        if (!self.save || shared.cfg.snip_copy_to_clipboard)
            && let Err(e) = output::copy_png_to_clipboard(&cropped)
        {
            error!("Snip copy failed: {e:#}");
        }
    }
}
