//! Fullscreen measurement overlay drawn over a frozen screenshot.

use crate::clipboard::copy_to_clipboard;
use crate::config::{MeasureStyle, ScreenRulerConfig, UnitsOfMeasure};
use crate::edges::{EdgeRect, RgbaView, detect_edges};
use crate::measurement::{clipboard_text, label_text};
use eframe::egui;
use image::RgbaImage;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Window title; also used to find our window with xdotool on X11.
const WINDOW_TITLE: &str = "mpt-screen-ruler-overlay";
/// Upstream `FEET_HALF_LENGTH` is 2 px; use a slightly larger value in points for visibility.
const FEET_HALF_LENGTH: f32 = 3.0;
/// Upstream `MOUSE_WHEEL_TOLERANCE_STEP`.
const WHEEL_TOLERANCE_STEP: i32 = 15;
const LABEL_OFFSET: f32 = 18.0;
const FONT_SIZE: f32 = 14.0;

const TOOLS: [(MeasureStyle, &str, egui::Key); 4] = [
    (MeasureStyle::Bounds, "Bounds (1)", egui::Key::Num1),
    (MeasureStyle::Spacing, "Spacing (2)", egui::Key::Num2),
    (
        MeasureStyle::HorizontalSpacing,
        "Horizontal (3)",
        egui::Key::Num3,
    ),
    (
        MeasureStyle::VerticalSpacing,
        "Vertical (4)",
        egui::Key::Num4,
    ),
];

/// A spacing measurement kept on screen with Shift+click.
struct SpacingMark {
    cursor: (u32, u32),
    edges: EdgeRect,
    style: MeasureStyle,
}

struct RulerApp {
    screenshot: RgbaImage,
    texture: Option<egui::TextureHandle>,
    config: ScreenRulerConfig,
    tool: MeasureStyle,
    tolerance: u8,
    cross_color: egui::Color32,
    /// Physical size of one pixel in mm; `None` = assume 96 DPI.
    px2mm: Option<f64>,

    // Bounds tool
    drag_start: Option<(u32, u32)>,
    kept_bounds: Vec<EdgeRect>,
    last_bounds: Option<EdgeRect>,

    // Spacing tools
    kept_spacing: Vec<SpacingMark>,
    edge_cache: Option<((u32, u32), u8, EdgeRect)>,

    toast: Option<(String, Instant)>,
    should_close: bool,
}

impl RulerApp {
    fn new(screenshot: RgbaImage, config: ScreenRulerConfig) -> Self {
        let (r, g, b) = config.cross_color_rgb();
        Self {
            screenshot,
            texture: None,
            tool: config.default_measure_style,
            tolerance: config.pixel_tolerance,
            cross_color: egui::Color32::from_rgb(r, g, b),
            px2mm: None,
            config,
            drag_start: None,
            kept_bounds: Vec::new(),
            last_bounds: None,
            kept_spacing: Vec::new(),
            edge_cache: None,
            toast: None,
            should_close: false,
        }
    }

    fn units(&self) -> UnitsOfMeasure {
        self.config.units_of_measure
    }

    fn edges_at(&mut self, px: (u32, u32)) -> Option<EdgeRect> {
        if let Some((p, tol, r)) = self.edge_cache
            && p == px
            && tol == self.tolerance
        {
            return Some(r);
        }
        let view = RgbaView::new(
            self.screenshot.as_raw(),
            self.screenshot.width(),
            self.screenshot.height(),
        )?;
        let r = detect_edges(
            &view,
            (px.0 as i64, px.1 as i64),
            self.config.per_color_channel_edge_detection,
            self.tolerance,
        )?;
        self.edge_cache = Some((px, self.tolerance, r));
        Some(r)
    }

    fn copy(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        match copy_to_clipboard(&text) {
            Ok(()) => {
                info!("Copied measurement: {text:?}");
                self.toast = Some(("Copied to clipboard".into(), Instant::now()));
            }
            Err(e) => {
                warn!("{e:#}");
                self.toast = Some((format!("{e}"), Instant::now()));
            }
        }
    }

    fn bounds_clipboard(&self, current: Option<EdgeRect>) -> String {
        let dims: Vec<(u32, u32)> = self
            .kept_bounds
            .iter()
            .chain(current.as_ref())
            .map(|r| (r.width(), r.height()))
            .collect();
        clipboard_text(&dims, true, true, self.units(), self.px2mm)
    }

    fn spacing_clipboard(&self, current: Option<EdgeRect>) -> String {
        let (w, h) = self.tool.lines();
        let dims: Vec<(u32, u32)> = self
            .kept_spacing
            .iter()
            .map(|m| &m.edges)
            .chain(current.as_ref())
            .map(|r| (r.width(), r.height()))
            .collect();
        clipboard_text(&dims, w, h, self.units(), self.px2mm)
    }

    fn select_tool(&mut self, tool: MeasureStyle) {
        if self.tool != tool {
            self.tool = tool;
            self.drag_start = None;
            self.last_bounds = None;
        }
    }

    /// Toolbar at the top centre. Returns its screen rect.
    fn toolbar(&mut self, ctx: &egui::Context) -> egui::Rect {
        let theme = mpt_common::theme::Theme::dark();
        egui::Area::new(egui::Id::new("screen_ruler_toolbar"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme.bg_primary)
                    .stroke(egui::Stroke::new(1.0, theme.card_border))
                    .corner_radius(mpt_common::theme::CARD_RADIUS)
                    .inner_margin(egui::Margin::same(6))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (tool, label, _) in TOOLS {
                                let text = egui::RichText::new(label)
                                    .size(mpt_common::theme::FONT_BUTTON)
                                    .color(theme.text_primary);
                                if ui.selectable_label(self.tool == tool, text).clicked() {
                                    self.select_tool(tool);
                                }
                            }
                            ui.separator();
                            ui.label(
                                egui::RichText::new(format!("Tolerance {}", self.tolerance))
                                    .size(mpt_common::theme::FONT_SMALL)
                                    .color(theme.text_secondary),
                            )
                            .on_hover_text("Mouse wheel adjusts edge detection tolerance");
                            if let Some((msg, at)) = &self.toast
                                && at.elapsed() < Duration::from_millis(1500)
                            {
                                ui.label(
                                    egui::RichText::new(msg.as_str())
                                        .size(mpt_common::theme::FONT_SMALL)
                                        .color(theme.text_success),
                                );
                            }
                            ui.separator();
                            let close = egui::RichText::new("Close (Esc)")
                                .size(mpt_common::theme::FONT_BUTTON)
                                .color(theme.text_primary);
                            if ui.button(close).clicked() {
                                self.should_close = true;
                            }
                        });
                    });
            })
            .response
            .rect
    }
}

/// Maps between screen points and screenshot pixels.
#[derive(Clone, Copy)]
struct Mapping {
    origin: egui::Pos2,
    sx: f32,
    sy: f32,
    w: u32,
    h: u32,
}

impl Mapping {
    fn to_px(self, p: egui::Pos2) -> (u32, u32) {
        let x = ((p.x - self.origin.x) * self.sx).floor().max(0.0) as u32;
        let y = ((p.y - self.origin.y) * self.sy).floor().max(0.0) as u32;
        (
            x.min(self.w.saturating_sub(1)),
            y.min(self.h.saturating_sub(1)),
        )
    }

    fn x(self, px: f32) -> f32 {
        self.origin.x + px / self.sx
    }

    fn y(self, px: f32) -> f32 {
        self.origin.y + px / self.sy
    }

    fn rect(self, r: &EdgeRect) -> egui::Rect {
        egui::Rect::from_min_max(
            egui::pos2(self.x(r.left as f32), self.y(r.top as f32)),
            egui::pos2(self.x(r.right as f32 + 1.0), self.y(r.bottom as f32 + 1.0)),
        )
    }
}

fn draw_label(
    painter: &egui::Painter,
    text: String,
    anchor: egui::Pos2,
    align: egui::Align2,
    screen: egui::Rect,
) {
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(FONT_SIZE),
        egui::Color32::WHITE,
    );
    let pad = egui::vec2(8.0, 5.0);
    let size = galley.size() + pad * 2.0;
    let mut rect = align.anchor_size(anchor, size);
    // Keep the label on screen.
    rect = rect.translate(egui::vec2(
        (screen.min.x - rect.min.x).max(0.0) + (screen.max.x - rect.max.x).min(0.0),
        (screen.min.y - rect.min.y).max(0.0) + (screen.max.y - rect.max.y).min(0.0),
    ));
    painter.rect_filled(
        rect.translate(egui::vec2(2.0, 2.0)),
        4.0,
        egui::Color32::from_black_alpha(90),
    );
    painter.rect_filled(
        rect,
        4.0,
        egui::Color32::from_rgba_unmultiplied(30, 30, 36, 235),
    );
    painter.galley(rect.min + pad, galley, egui::Color32::WHITE);
}

impl RulerApp {
    fn draw_spacing(
        &self,
        painter: &egui::Painter,
        map: Mapping,
        cursor: (u32, u32),
        edges: &EdgeRect,
        style: MeasureStyle,
    ) {
        let (horiz, vert) = style.lines();
        let stroke = egui::Stroke::new(1.0, self.cross_color);
        let feet = self.config.draw_feet_on_cross;
        if horiz {
            let y = map.y(cursor.1 as f32 + 0.5);
            let x0 = map.x(edges.left as f32);
            let x1 = map.x(edges.right as f32 + 1.0);
            painter.line_segment([egui::pos2(x0, y), egui::pos2(x1, y)], stroke);
            if feet {
                for x in [x0 + 0.5, x1 - 0.5] {
                    painter.line_segment(
                        [
                            egui::pos2(x, y - FEET_HALF_LENGTH),
                            egui::pos2(x, y + FEET_HALF_LENGTH),
                        ],
                        stroke,
                    );
                }
            }
        }
        if vert {
            let x = map.x(cursor.0 as f32 + 0.5);
            let y0 = map.y(edges.top as f32);
            let y1 = map.y(edges.bottom as f32 + 1.0);
            painter.line_segment([egui::pos2(x, y0), egui::pos2(x, y1)], stroke);
            if feet {
                for y in [y0 + 0.5, y1 - 0.5] {
                    painter.line_segment(
                        [
                            egui::pos2(x - FEET_HALF_LENGTH, y),
                            egui::pos2(x + FEET_HALF_LENGTH, y),
                        ],
                        stroke,
                    );
                }
            }
        }
    }

    fn draw_bounds(&self, painter: &egui::Painter, map: Mapping, r: &EdgeRect, screen: egui::Rect) {
        let rect = map.rect(r);
        let c = self.cross_color;
        painter.rect_filled(
            rect,
            0.0,
            egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 40),
        );
        painter.rect_stroke(
            rect,
            0.0,
            egui::Stroke::new(1.0, c),
            egui::StrokeKind::Inside,
        );
        let text = label_text(r.width(), r.height(), true, true, self.units(), self.px2mm);
        draw_label(
            painter,
            text,
            rect.center(),
            egui::Align2::CENTER_CENTER,
            screen,
        );
    }
}

impl eframe::App for RulerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.texture.is_none() {
            let size = [
                self.screenshot.width() as usize,
                self.screenshot.height() as usize,
            ];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, self.screenshot.as_raw());
            self.texture =
                Some(ctx.load_texture("screenshot", image, egui::TextureOptions::NEAREST));
        }

        // Keyboard / wheel input
        let (escape, copy, shift, wheel, tool_key) = ctx.input(|i| {
            let copy = i.events.iter().any(|e| matches!(e, egui::Event::Copy))
                || (i.modifiers.command && i.key_pressed(egui::Key::C));
            let tool_key = TOOLS
                .iter()
                .find(|(_, _, k)| i.key_pressed(*k))
                .map(|(t, _, _)| *t);
            (
                i.key_pressed(egui::Key::Escape),
                copy,
                i.modifiers.shift,
                i.raw_scroll_delta.y,
                tool_key,
            )
        });
        if escape {
            self.should_close = true;
        }
        if let Some(t) = tool_key {
            self.select_tool(t);
        }
        if wheel != 0.0 {
            let step = if wheel > 0.0 {
                WHEEL_TOLERANCE_STEP
            } else {
                -WHEEL_TOLERANCE_STEP
            };
            self.tolerance = (self.tolerance as i32 + step).clamp(0, 255) as u8;
        }

        let toolbar_rect = self.toolbar(ctx);

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let screen = ui.max_rect();
                let response = ui.allocate_rect(screen, egui::Sense::click_and_drag());
                let painter = ui.painter().clone();
                if let Some(tex) = &self.texture {
                    painter.image(
                        tex.id(),
                        screen,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                let map = Mapping {
                    origin: screen.min,
                    sx: self.screenshot.width() as f32 / screen.width().max(1.0),
                    sy: self.screenshot.height() as f32 / screen.height().max(1.0),
                    w: self.screenshot.width(),
                    h: self.screenshot.height(),
                };
                let hover = ctx
                    .input(|i| i.pointer.hover_pos())
                    .filter(|p| !toolbar_rect.contains(*p));

                match self.tool {
                    MeasureStyle::None => {}
                    MeasureStyle::Bounds => {
                        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                        if response.drag_started()
                            && let Some(origin) = ctx.input(|i| i.pointer.press_origin())
                        {
                            self.drag_start = Some(map.to_px(origin));
                            self.last_bounds = None;
                        }
                        let current = match (self.drag_start, hover) {
                            (Some(start), Some(p)) => {
                                Some(EdgeRect::from_corners(start, map.to_px(p)))
                            }
                            _ => None,
                        };
                        if response.drag_stopped() {
                            let text = self.bounds_clipboard(current);
                            self.copy(text);
                            if let Some(r) = current {
                                if shift {
                                    self.kept_bounds.push(r);
                                } else {
                                    self.last_bounds = Some(r);
                                }
                            }
                            self.drag_start = None;
                        } else if copy {
                            let text = self.bounds_clipboard(current.or(self.last_bounds));
                            self.copy(text);
                        }

                        for r in &self.kept_bounds {
                            self.draw_bounds(&painter, map, r, screen);
                        }
                        let shown = if self.drag_start.is_some() {
                            current
                        } else {
                            self.last_bounds
                        };
                        if let Some(r) = shown {
                            self.draw_bounds(&painter, map, &r, screen);
                        }
                    }
                    style @ (MeasureStyle::Spacing
                    | MeasureStyle::HorizontalSpacing
                    | MeasureStyle::VerticalSpacing) => {
                        ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                        for m in &self.kept_spacing {
                            self.draw_spacing(&painter, map, m.cursor, &m.edges, m.style);
                        }
                        let current = hover.and_then(|p| {
                            let px = map.to_px(p);
                            self.edges_at(px).map(|e| (px, e, p))
                        });
                        if let Some((px, edges, pos)) = current {
                            self.draw_spacing(&painter, map, px, &edges, style);
                            let (w, h) = style.lines();
                            let text = label_text(
                                edges.width(),
                                edges.height(),
                                w,
                                h,
                                self.units(),
                                self.px2mm,
                            );
                            // Like upstream, put the label on the side of the
                            // cursor facing the larger half of the screen.
                            let right = pos.x < screen.center().x;
                            let down = pos.y < screen.center().y;
                            let align = egui::Align2([
                                if right {
                                    egui::Align::Min
                                } else {
                                    egui::Align::Max
                                },
                                if down {
                                    egui::Align::Min
                                } else {
                                    egui::Align::Max
                                },
                            ]);
                            let anchor = pos
                                + egui::vec2(
                                    if right { LABEL_OFFSET } else { -LABEL_OFFSET },
                                    if down { LABEL_OFFSET } else { -LABEL_OFFSET },
                                );
                            draw_label(&painter, text, anchor, align, screen);
                        }
                        let edges = current.map(|(_, e, _)| e);
                        if response.clicked() {
                            let text = self.spacing_clipboard(edges);
                            self.copy(text);
                            if shift {
                                if let Some((px, e, _)) = current {
                                    self.kept_spacing.push(SpacingMark {
                                        cursor: px,
                                        edges: e,
                                        style,
                                    });
                                }
                            } else {
                                // Upstream closes the overlay after a plain click.
                                self.should_close = true;
                            }
                        } else if copy {
                            let text = self.spacing_clipboard(edges);
                            self.copy(text);
                        }
                    }
                }
            });

        if self.should_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint();
    }
}

/// Show the overlay over `screenshot` until the user closes it.
pub fn run_overlay(screenshot: RgbaImage, config: ScreenRulerConfig) {
    if config.continuous_capture {
        warn!(
            "continuous_capture is not supported on Linux: the overlay cannot be \
             excluded from screen captures, so the screenshot stays frozen"
        );
    }

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

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        WINDOW_TITLE,
        native_options,
        Box::new(move |cc| {
            mpt_common::theme::Theme::dark().apply(&cc.egui_ctx);
            Ok(Box::new(RulerApp::new(screenshot, config)))
        }),
    ) {
        warn!("Screen Ruler overlay failed: {e}");
    }
}

/// On X11, set override_redirect on our window so it spans all monitors and
/// is not managed by the WM (same approach as the Color Picker overlay).
fn x11_configure_overlay(width: u32, height: u32) {
    use std::process::Command;

    for _ in 0..100 {
        std::thread::sleep(Duration::from_millis(30));

        let Ok(output) = Command::new("xdotool")
            .args(["search", "--name", WINDOW_TITLE])
            .output()
        else {
            warn!("xdotool not found; overlay may not cover all monitors");
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
