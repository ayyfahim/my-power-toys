pub mod cropped;
pub mod selection;

use eframe::egui;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// X11 window id of an eframe window, if running on X11.
pub fn x11_window_id(handle: &impl HasWindowHandle) -> Option<u32> {
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => u32::try_from(h.window).ok(),
        RawWindowHandle::Xcb(h) => Some(h.window.get()),
        _ => None,
    }
}

/// Draw `texture` letterboxed (aspect preserved, centered) inside `rect`.
pub fn paint_fitted(painter: &egui::Painter, rect: egui::Rect, texture: &egui::TextureHandle) {
    let [tw, th] = texture.size();
    let (x, y, w, h) =
        crate::geometry::fit_centered(tw as f32, th as f32, rect.width(), rect.height());
    let dst = egui::Rect::from_min_size(rect.min + egui::vec2(x, y), egui::vec2(w, h));
    painter.image(
        texture.id(),
        dst,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

pub fn color_image(img: &image::RgbaImage) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied(
        [img.width() as usize, img.height() as usize],
        img.as_raw(),
    )
}
