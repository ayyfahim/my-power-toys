//! Fullscreen egui overlay hosting all interactive ZoomIt modes.

pub mod draw;
pub mod snip;
pub mod timer;
pub mod zoom;

use crate::capture::{self, MonitorCapture};
use crate::config::ZoomItConfig;
use crate::geometry::{self, Point, Rect};
use anyhow::Result;
use eframe::egui;
use image::RgbaImage;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};

const HINT_DURATION: Duration = Duration::from_secs(4);

/// Mode requested on the command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    Zoom,
    LiveZoom,
    Draw,
    Break,
    Snip,
    SnipSave,
}

impl StartMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "zoom" => Self::Zoom,
            "livezoom" | "live-zoom" => Self::LiveZoom,
            "draw" => Self::Draw,
            "break" | "timer" => Self::Break,
            "snip" => Self::Snip,
            "snip-save" => Self::SnipSave,
            _ => return None,
        })
    }
}

/// Mode-specific state of the running overlay.
pub(crate) enum State {
    Zoom(zoom::ZoomState),
    Draw(Box<draw::DrawState>),
    Break(timer::TimerState),
    Snip(snip::SnipState),
}

/// Requests a mode handler can make of the app after processing a frame.
pub(crate) enum Transition {
    Stay,
    Exit,
    To(State),
}

/// Data shared by every mode.
pub(crate) struct Shared {
    pub cfg: ZoomItConfig,
    pub capture: Option<MonitorCapture>,
    /// Screenshot texture (linear or nearest filtered per `smooth_image`).
    pub texture: Option<egui::TextureHandle>,
    /// Transient message shown at the bottom of the screen.
    pub toast: Option<(String, Instant)>,
    /// Accumulator for pixel-based wheel deltas (touchpads).
    wheel_accum: f32,
    /// When the current mode was entered (hints fade out after a while).
    pub mode_started: Instant,
    /// Set while an export screenshot is pending: hide toasts and hints.
    pub suppress_overlay: bool,
}

impl Shared {
    /// Show a usage hint during the first seconds of a mode.
    pub fn hint(&self, painter: &egui::Painter, screen: egui::Rect, text: &str) {
        if !self.suppress_overlay && self.mode_started.elapsed() < HINT_DURATION {
            draw_hint(painter, screen, text);
            painter.ctx().request_repaint_after(HINT_DURATION);
        }
    }

    pub fn toast(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now()));
    }

    /// Texture of the frozen screenshot, uploading it on first use.
    pub fn texture(&mut self, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        if self.texture.is_none()
            && let Some(cap) = &self.capture
        {
            let img = &cap.image;
            let color = egui::ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            );
            let opts = if self.cfg.smooth_image {
                egui::TextureOptions::LINEAR
            } else {
                egui::TextureOptions::NEAREST
            };
            self.texture = Some(ctx.load_texture("zoomit-screenshot", color, opts));
        }
        self.texture.clone()
    }

    /// Cursor position at capture time, converted to screen points.
    pub fn initial_cursor(&self, screen: egui::Rect) -> egui::Pos2 {
        match &self.capture {
            Some(cap) if cap.cursor.is_some() => {
                let (cx, cy) = cap.cursor.unwrap_or_default();
                let img = Rect::new(
                    0.0,
                    0.0,
                    cap.image.width() as f32,
                    cap.image.height() as f32,
                );
                let p = img.map_to(Point::new(cx, cy), &to_rect(screen));
                egui::pos2(p.x, p.y)
            }
            _ => screen.center(),
        }
    }

    /// Discrete wheel steps this frame, split into (plain, with Ctrl).
    /// Positive = wheel up / away from the user.
    pub fn wheel_steps(&mut self, ctx: &egui::Context) -> (i32, i32) {
        let mut plain = 0;
        let mut ctrl = 0;
        ctx.input(|i| {
            for e in &i.events {
                if let egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                } = e
                {
                    let steps = match unit {
                        egui::MouseWheelUnit::Point => {
                            self.wheel_accum += delta.y;
                            let s = (self.wheel_accum / 50.0).trunc();
                            self.wheel_accum -= s * 50.0;
                            s as i32
                        }
                        _ => {
                            if delta.y > 0.0 {
                                1
                            } else if delta.y < 0.0 {
                                -1
                            } else {
                                0
                            }
                        }
                    };
                    if modifiers.ctrl || modifiers.command {
                        ctrl += steps;
                    } else {
                        plain += steps;
                    }
                }
            }
        });
        (plain, ctrl)
    }
}

pub(crate) fn to_rect(r: egui::Rect) -> Rect {
    Rect::new(r.min.x, r.min.y, r.width(), r.height())
}

pub(crate) fn from_rect(r: Rect) -> egui::Rect {
    egui::Rect::from_min_size(egui::pos2(r.x, r.y), egui::vec2(r.w, r.h))
}

/// UV rectangle of `view` (screen points) within `screen`.
pub(crate) fn uv_of(view: Rect, screen: egui::Rect) -> egui::Rect {
    let (w, h) = (screen.width().max(1.0), screen.height().max(1.0));
    egui::Rect::from_min_max(
        egui::pos2((view.x - screen.min.x) / w, (view.y - screen.min.y) / h),
        egui::pos2(
            (view.right() - screen.min.x) / w,
            (view.bottom() - screen.min.y) / h,
        ),
    )
}

pub(crate) fn color32(hex: &str, fallback: egui::Color32) -> egui::Color32 {
    geometry::parse_hex_color(hex)
        .map(|[r, g, b, a]| egui::Color32::from_rgba_unmultiplied(r, g, b, a))
        .unwrap_or(fallback)
}

/// Small hint line at the top of the screen.
pub(crate) fn draw_hint(painter: &egui::Painter, screen: egui::Rect, text: &str) {
    let font = egui::FontId::proportional(14.0);
    let galley = painter.layout_no_wrap(text.to_owned(), font, egui::Color32::WHITE);
    let pos = egui::pos2(
        screen.center().x - galley.size().x / 2.0,
        screen.min.y + 8.0,
    );
    let bg = egui::Rect::from_min_size(pos, galley.size()).expand(6.0);
    painter.rect_filled(bg, 6.0, egui::Color32::from_black_alpha(170));
    painter.galley(pos, galley, egui::Color32::WHITE);
}

struct ZoomItApp {
    shared: Shared,
    state: State,
    frame: u64,
}

impl eframe::App for ZoomItApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame += 1;
        if self.frame < 5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        let screen = ctx.screen_rect();
        self.shared.suppress_overlay = false;
        let transition = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let painter = ui.painter().clone();
                match &mut self.state {
                    State::Zoom(s) => s.update(ctx, &painter, screen, &mut self.shared),
                    State::Draw(s) => s.update(ctx, &painter, screen, &mut self.shared),
                    State::Break(s) => s.update(ctx, &painter, screen, &mut self.shared),
                    State::Snip(s) => s.update(ctx, &painter, screen, &mut self.shared),
                }
            })
            .inner;

        // Toast
        if let Some((msg, at)) = &self.shared.toast
            && !self.shared.suppress_overlay
        {
            if at.elapsed() < Duration::from_millis(1800) {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("zoomit-toast"),
                ));
                let font = egui::FontId::proportional(18.0);
                let galley = painter.layout_no_wrap(msg.clone(), font, egui::Color32::WHITE);
                let pos = egui::pos2(
                    screen.center().x - galley.size().x / 2.0,
                    screen.max.y - 60.0,
                );
                let bg = egui::Rect::from_min_size(pos, galley.size()).expand(10.0);
                painter.rect_filled(bg, 8.0, egui::Color32::from_black_alpha(200));
                painter.galley(pos, galley, egui::Color32::WHITE);
                ctx.request_repaint_after(Duration::from_millis(100));
            } else {
                self.shared.toast = None;
            }
        }

        match transition {
            Transition::Stay => {}
            Transition::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Transition::To(next) => {
                self.state = next;
                self.shared.mode_started = Instant::now();
                ctx.request_repaint();
            }
        }
    }
}

/// Capture the screen (if needed) and run the overlay until the user exits.
pub fn run(mode: StartMode) -> Result<()> {
    let cfg = ZoomItConfig::load();

    let needs_capture = match mode {
        StartMode::Break => cfg.break_show_desktop || cfg.break_opacity < 100,
        _ => true,
    };
    let capture = if needs_capture {
        match capture::capture_monitor_under_cursor() {
            Ok(c) => Some(c),
            Err(e) if mode == StartMode::Break => {
                warn!("Screen capture failed ({e}); break timer without desktop");
                None
            }
            Err(e) => {
                error!("Screen capture failed: {e}");
                return Err(e);
            }
        }
    } else {
        None
    };

    let (mon_pos, mon_size) = match &capture {
        Some(c) => (
            egui::pos2(c.monitor.x as f32, c.monitor.y as f32),
            egui::vec2(c.monitor.width as f32, c.monitor.height as f32),
        ),
        None => match crate::screen::monitor_under_cursor() {
            Ok((m, _)) => (
                egui::pos2(m.x as f32, m.y as f32),
                egui::vec2(m.width as f32, m.height as f32),
            ),
            Err(_) => (egui::pos2(0.0, 0.0), egui::vec2(1920.0, 1080.0)),
        },
    };

    let state = match mode {
        StartMode::Zoom | StartMode::LiveZoom => State::Zoom(zoom::ZoomState::new(&cfg)),
        StartMode::Draw => State::Draw(Box::new(draw::DrawState::new(&cfg, None, None))),
        StartMode::Break => State::Break(timer::TimerState::new(&cfg)),
        StartMode::Snip => State::Snip(snip::SnipState::new(false)),
        StartMode::SnipSave => State::Snip(snip::SnipState::new(true)),
    };

    info!("ZoomIt overlay starting in {mode:?} mode");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ZoomIt")
            .with_app_id("mpt-zoom-it")
            .with_decorations(false)
            .with_always_on_top()
            .with_position(mon_pos)
            .with_inner_size(mon_size)
            .with_resizable(false)
            .with_fullscreen(true)
            .with_taskbar(false),
        ..Default::default()
    };

    let shared = Shared {
        cfg,
        capture,
        texture: None,
        toast: None,
        wheel_accum: 0.0,
        mode_started: Instant::now(),
        suppress_overlay: false,
    };
    eframe::run_native(
        "ZoomIt",
        options,
        Box::new(move |cc| {
            mpt_common::theme::Theme::dark().apply(&cc.egui_ctx);
            Ok(Box::new(ZoomItApp {
                shared,
                state,
                frame: 0,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))
}

/// Convert an egui screenshot into an image crate buffer.
pub(crate) fn color_image_to_rgba(img: &egui::ColorImage) -> Option<RgbaImage> {
    let [w, h] = img.size;
    let mut raw = Vec::with_capacity(w * h * 4);
    for c in &img.pixels {
        raw.extend_from_slice(&c.to_srgba_unmultiplied());
    }
    RgbaImage::from_raw(w as u32, h as u32, raw)
}
