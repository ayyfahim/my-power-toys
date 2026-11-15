//! Zoom mode: magnified view of the frozen screenshot centered on the mouse.

use super::{Shared, State, Transition, draw, uv_of};
use crate::config::ZoomItConfig;
use crate::geometry::{self, Point, Rect};
use eframe::egui;

/// Animation speed for `animate_zoom` (1/s, exponential approach).
const ZOOM_ANIM_RATE: f32 = 12.0;

pub(crate) struct ZoomState {
    /// Zoom factor currently displayed (animated).
    zoom: f32,
    /// Zoom factor being animated toward.
    target: f32,
    animate: bool,
    /// Focus point in screen coordinates (follows the mouse).
    focus: Option<egui::Pos2>,
    last_frame: Option<std::time::Instant>,
}

impl ZoomState {
    pub fn new(cfg: &ZoomItConfig) -> Self {
        let target = cfg.initial_zoom();
        Self {
            // Upstream animates from 1x up to the initial level on entry.
            zoom: if cfg.animate_zoom { 1.0 } else { target },
            target,
            animate: cfg.animate_zoom,
            focus: None,
            last_frame: None,
        }
    }

    /// The part of the screen (in screen points) shown magnified.
    pub fn view(&self, screen: egui::Rect) -> Rect {
        let focus = self.focus.unwrap_or(screen.center());
        let mut r = geometry::zoom_source_rect(
            screen.width(),
            screen.height(),
            Point::new(focus.x - screen.min.x, focus.y - screen.min.y),
            self.zoom,
        );
        r.x += screen.min.x;
        r.y += screen.min.y;
        r
    }

    fn step(&mut self, steps: i32) {
        for _ in 0..steps.unsigned_abs() {
            self.target = if steps > 0 {
                geometry::zoom_in_step(self.target)
            } else {
                geometry::zoom_out_step(self.target)
            };
        }
        if !self.animate {
            self.zoom = self.target;
        }
    }

    pub fn update(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        screen: egui::Rect,
        shared: &mut Shared,
    ) -> Transition {
        if self.focus.is_none() {
            self.focus = Some(shared.initial_cursor(screen));
        }
        let (escape, right, left, up, down, pointer) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.pointer.secondary_clicked(),
                i.pointer.primary_pressed(),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.pointer.latest_pos(),
            )
        });
        if escape || right {
            return Transition::Exit;
        }
        if let Some(p) = pointer {
            self.focus = Some(p);
        }

        let (wheel, ctrl_wheel) = shared.wheel_steps(ctx);
        let steps = wheel + ctrl_wheel + i32::from(up) - i32::from(down);
        if steps != 0 {
            self.step(steps);
        }

        // Animate toward the target zoom.
        let now = std::time::Instant::now();
        let dt = self
            .last_frame
            .map(|t| (now - t).as_secs_f32())
            .unwrap_or(1.0 / 60.0)
            .min(0.1);
        self.last_frame = Some(now);
        if self.zoom != self.target {
            self.zoom = geometry::animate_toward(self.zoom, self.target, dt, ZOOM_ANIM_RATE);
            ctx.request_repaint();
        }

        let view = self.view(screen);
        if let Some(tex) = shared.texture(ctx) {
            painter.image(tex.id(), screen, uv_of(view, screen), egui::Color32::WHITE);
        }
        shared.hint(
            painter,
            screen,
            &format!(
                "Zoom {:.2}x  ·  wheel / ↑↓ zoom  ·  click to draw  ·  Esc / right-click exit",
                self.zoom
            ),
        );

        if left {
            // Freeze the current view and annotate on top of it.
            self.zoom = self.target;
            let view = self.view(screen);
            let back = std::mem::replace(self, ZoomState::new(&shared.cfg));
            let mut ds = draw::DrawState::new(&shared.cfg, Some(view), Some(back));
            if let Some(p) = pointer {
                ds.begin_at(ctx, p);
            }
            return Transition::To(State::Draw(Box::new(ds)));
        }
        Transition::Stay
    }

    /// Restore after leaving draw mode (keeps focus and zoom).
    pub fn resumed(mut self) -> Self {
        self.last_frame = None;
        self
    }
}
