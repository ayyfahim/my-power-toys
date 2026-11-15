//! Break timer: fullscreen countdown (upstream `BreakTimerKey`).

use super::{Shared, Transition, color32};
use crate::config::ZoomItConfig;
use crate::geometry;
use eframe::egui::{self, Color32};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use tracing::{info, warn};

const MARGIN: f32 = 50.0;

pub(crate) struct TimerState {
    /// Total break length in seconds (adjustable with the arrow keys).
    total: i64,
    started: Instant,
    sound_played: bool,
    background: Option<egui::TextureHandle>,
    background_loaded: bool,
}

impl TimerState {
    pub fn new(cfg: &ZoomItConfig) -> Self {
        Self {
            total: i64::from(cfg.break_timeout.max(1)) * 60,
            started: Instant::now(),
            sound_played: false,
            background: None,
            background_loaded: false,
        }
    }

    fn remaining(&self) -> i64 {
        self.total - self.started.elapsed().as_secs() as i64
    }

    fn adjust(&mut self, delta_secs: i64) {
        let elapsed = self.started.elapsed().as_secs() as i64;
        // Keep at least one second left so the user sees the change.
        self.total = (self.total + delta_secs).max(elapsed + 1).max(1);
        self.sound_played = false;
    }

    fn load_background(&mut self, ctx: &egui::Context, cfg: &ZoomItConfig) {
        if self.background_loaded {
            return;
        }
        self.background_loaded = true;
        if !cfg.break_show_background_file || cfg.break_background_file.trim().is_empty() {
            return;
        }
        match image::open(cfg.break_background_file.trim()) {
            Ok(img) => {
                let img = img.to_rgba8();
                let color = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                );
                self.background =
                    Some(ctx.load_texture("zoomit-break-bg", color, egui::TextureOptions::LINEAR));
            }
            Err(e) => warn!(
                "Could not load break background {:?}: {e}",
                cfg.break_background_file
            ),
        }
    }

    pub fn update(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        screen: egui::Rect,
        shared: &mut Shared,
    ) -> Transition {
        let (esc, right, up, down) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.pointer.secondary_clicked(),
                i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft),
            )
        });
        if esc || right {
            return Transition::Exit;
        }
        if up {
            self.adjust(60);
        }
        if down {
            self.adjust(-60);
        }
        let (wheel, _) = shared.wheel_steps(ctx);
        if wheel != 0 {
            self.adjust(i64::from(wheel) * 60);
        }

        let cfg = shared.cfg.clone();
        self.load_background(ctx, &cfg);
        let bg_color = color32(&cfg.break_background_color, Color32::BLACK);
        let text_color = color32(&cfg.break_pen_color, Color32::RED);
        let opacity = (cfg.break_opacity.clamp(1, 100) as f32) / 100.0;

        // Live desktop can't show through without a compositor, so the
        // frozen screenshot stands in for it and `break_opacity` blends the
        // break background over it.
        let desktop = shared.texture(ctx);
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        if let Some(tex) = &desktop {
            painter.image(tex.id(), screen, uv, Color32::WHITE);
        } else {
            painter.rect_filled(screen, 0.0, bg_color);
        }
        let layer_alpha = |a: f32| (a * opacity * 255.0).round() as u8;
        if let Some(bg) = &self.background {
            painter.rect_filled(screen, 0.0, bg_color.gamma_multiply(opacity));
            let size = bg.size_vec2();
            let rect = if cfg.break_background_stretch {
                screen
            } else {
                egui::Rect::from_center_size(screen.center(), size)
            };
            painter.image(
                bg.id(),
                rect,
                uv,
                Color32::from_white_alpha(layer_alpha(1.0)),
            );
        } else if cfg.break_show_desktop && desktop.is_some() {
            // Faded desktop.
            let [r, g, b, _] = bg_color.to_array();
            painter.rect_filled(
                screen,
                0.0,
                Color32::from_rgba_unmultiplied(r, g, b, layer_alpha(0.6)),
            );
        } else {
            painter.rect_filled(screen, 0.0, bg_color.gamma_multiply(opacity));
        }

        // Timer text.
        let remaining = self.remaining();
        let main_text = geometry::format_timer(remaining.max(0));
        let font_size = (screen.height() / 5.0).max(32.0);
        let galley =
            painter.layout_no_wrap(main_text, egui::FontId::monospace(font_size), text_color);
        let expired = (remaining < 0 && cfg.show_expired_time).then(|| {
            painter.layout_no_wrap(
                geometry::format_timer(remaining),
                egui::FontId::monospace(font_size / 3.0),
                text_color,
            )
        });
        let block_w = galley
            .size()
            .x
            .max(expired.as_ref().map(|g| g.size().x).unwrap_or(0.0));
        let block_h = galley.size().y + expired.as_ref().map(|g| g.size().y).unwrap_or(0.0);
        let pos = geometry::timer_position(
            cfg.break_timer_position,
            screen.width(),
            screen.height(),
            block_w,
            block_h,
            MARGIN,
        );
        let origin = screen.min + egui::vec2(pos.x, pos.y);
        let main_h = galley.size().y;
        let main_w = galley.size().x;
        painter.galley(
            origin + egui::vec2((block_w - main_w) / 2.0, 0.0),
            galley,
            text_color,
        );
        if let Some(g) = expired {
            let w = g.size().x;
            painter.galley(
                origin + egui::vec2((block_w - w) / 2.0, main_h),
                g,
                text_color,
            );
        }

        if remaining <= 0 && !self.sound_played {
            self.sound_played = true;
            if cfg.break_play_sound_file {
                play_sound(&cfg.break_sound_file);
            }
        }

        shared.hint(
            painter,
            screen,
            "Break timer · ↑/↓ or wheel adjust minutes · Esc exit",
        );
        ctx.request_repaint_after(Duration::from_millis(250));
        Transition::Stay
    }
}

/// Play the configured sound file, or the desktop "complete" sound.
fn play_sound(file: &str) {
    let file = file.trim().to_string();
    std::thread::spawn(move || {
        let attempts: Vec<(&str, Vec<String>)> = if !file.is_empty() {
            vec![
                ("paplay", vec![file.clone()]),
                ("pw-play", vec![file.clone()]),
                ("aplay", vec![file.clone()]),
                ("canberra-gtk-play", vec!["-f".into(), file.clone()]),
            ]
        } else {
            vec![
                ("canberra-gtk-play", vec!["-i".into(), "complete".into()]),
                (
                    "paplay",
                    vec!["/usr/share/sounds/freedesktop/stereo/complete.oga".into()],
                ),
            ]
        };
        for (cmd, args) in attempts {
            let ok = Command::new(cmd)
                .args(&args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                info!("Played break sound via {cmd}");
                return;
            }
        }
        warn!("Could not play break sound (install libcanberra-gtk3 or pulseaudio-utils)");
    });
}
