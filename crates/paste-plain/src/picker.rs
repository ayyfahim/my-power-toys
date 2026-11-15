//! Advanced Paste picker window (egui).
//!
//! Shows a clipboard preview and the available formats. Keys: 1-9 pick a
//! format, Up/Down + Enter, Esc closes. AI results can be reviewed before
//! pasting when `show_custom_preview` is on.

use crate::actions::{self, ClipboardContent, Output, PasteAction};
use crate::config::PastePlainConfig;
use eframe::egui;
use mpt_common::theme::Theme;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const THEME: Theme = Theme::dark();
const WINDOW_WIDTH: f32 = 440.0;
const WINDOW_HEIGHT: f32 = 520.0;
const PREVIEW_CHARS: usize = 600;

enum State {
    Choosing,
    Running {
        label: String,
        is_ai: bool,
        rx: Receiver<anyhow::Result<Output>>,
    },
    Preview(String),
    Error(String),
}

struct PickerApp {
    cfg: PastePlainConfig,
    content: ClipboardContent,
    actions: Vec<PasteAction>,
    selected: usize,
    prompt: String,
    state: State,
    result: Arc<Mutex<Option<Output>>>,
    image_texture: Option<egui::TextureHandle>,
    image_loaded: bool,
    was_focused: bool,
    frame_count: u32,
    close: bool,
}

impl PickerApp {
    fn new(
        cfg: PastePlainConfig,
        content: ClipboardContent,
        result: Arc<Mutex<Option<Output>>>,
    ) -> Self {
        let actions = actions::picker_actions(&cfg, &content);
        Self {
            cfg,
            content,
            actions,
            selected: 0,
            prompt: String::new(),
            state: State::Choosing,
            result,
            image_texture: None,
            image_loaded: false,
            was_focused: false,
            frame_count: 0,
            close: false,
        }
    }

    fn start(&mut self, action: PasteAction) {
        let (tx, rx) = mpsc::channel();
        let content = self.content.clone();
        let cfg = self.cfg.clone();
        let label = action.label(&cfg);
        let is_ai = action.requires_ai();
        std::thread::spawn(move || {
            let _ = tx.send(actions::transform(&action, &content, &cfg));
        });
        self.state = State::Running { label, is_ai, rx };
    }

    fn finish(&mut self, out: Output) {
        *self.result.lock().unwrap_or_else(|e| e.into_inner()) = Some(out);
        self.close = true;
    }

    fn poll(&mut self) {
        let State::Running { rx, is_ai, .. } = &self.state else {
            return;
        };
        let is_ai = *is_ai;
        match rx.try_recv() {
            Ok(Ok(Output::Text(t))) if is_ai && self.cfg.show_custom_preview => {
                self.state = State::Preview(t);
            }
            Ok(Ok(out)) => self.finish(out),
            Ok(Err(e)) => self.state = State::Error(format!("{e:#}")),
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {
                self.state = State::Error("transformation thread crashed".into());
            }
        }
    }

    fn ensure_image_texture(&mut self, ctx: &egui::Context) {
        if self.image_loaded {
            return;
        }
        self.image_loaded = true;
        let Some(png) = &self.content.png else {
            return;
        };
        if let Ok(img) = image::load_from_memory_with_format(png, image::ImageFormat::Png) {
            let thumb = img.thumbnail(400, 160).to_rgba8();
            let size = [thumb.width() as usize, thumb.height() as usize];
            let ci = egui::ColorImage::from_rgba_unmultiplied(size, thumb.as_raw());
            self.image_texture =
                Some(ctx.load_texture("clipboard-preview", ci, egui::TextureOptions::LINEAR));
        }
    }

    fn draw_preview(&self, ui: &mut egui::Ui) {
        egui::Frame::NONE
            .fill(THEME.bg_secondary)
            .corner_radius(8.0)
            .inner_margin(10.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                if let Some(tex) = &self.image_texture {
                    ui.add(egui::Image::new(tex).max_height(160.0));
                } else if let Some(text) = self.content.plain_text() {
                    let mut shown: String = text.chars().take(PREVIEW_CHARS).collect();
                    if text.chars().count() > PREVIEW_CHARS {
                        shown.push('…');
                    }
                    egui::ScrollArea::vertical()
                        .max_height(110.0)
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(shown)
                                    .monospace()
                                    .color(THEME.text_secondary),
                            );
                        });
                } else {
                    ui.label(egui::RichText::new("Clipboard is empty").color(THEME.text_muted));
                }
                let mut kinds = Vec::new();
                if self.content.text.is_some() {
                    kinds.push("Text");
                }
                if self.content.html.is_some() {
                    kinds.push("HTML");
                }
                if self.content.png.is_some() {
                    kinds.push("Image");
                }
                if !kinds.is_empty() {
                    ui.label(
                        egui::RichText::new(kinds.join(" · "))
                            .small()
                            .color(THEME.text_muted),
                    );
                }
            });
    }

    fn choosing_ui(&mut self, ui: &mut egui::Ui) -> Option<PasteAction> {
        let mut chosen = None;
        let prompt_focused = ui.memory(|m| m.focused().is_some());

        if !prompt_focused {
            let (up, down, enter, digit) = ui.input(|i| {
                let digit = (1..=9).find(|n| i.key_pressed(digit_key(*n)));
                (
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::Enter),
                    digit,
                )
            });
            if !self.actions.is_empty() {
                if up {
                    self.selected = self.selected.saturating_sub(1);
                }
                if down {
                    self.selected = (self.selected + 1).min(self.actions.len() - 1);
                }
                if enter {
                    chosen = self.actions.get(self.selected).cloned();
                }
            }
            if let Some(n) = digit
                && let Some(a) = self.actions.get(n - 1)
            {
                chosen = Some(a.clone());
            }
        }

        if self.cfg.ai_available() && self.cfg.show_ai_paste && self.content.plain_text().is_some()
        {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.prompt)
                    .hint_text("Describe what format you want (AI)…")
                    .desired_width(f32::INFINITY),
            );
            if resp.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !self.prompt.trim().is_empty()
            {
                chosen = Some(PasteAction::Prompt(self.prompt.trim().to_string()));
            }
            ui.add_space(6.0);
        }

        if self.actions.is_empty() {
            ui.label(
                egui::RichText::new("No paste formats available for this clipboard content")
                    .color(THEME.text_muted),
            );
        }

        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, action) in self.actions.iter().enumerate() {
                let selected = i == self.selected;
                let label = action.label(&self.cfg);
                let key = if i < 9 {
                    format!("{}", i + 1)
                } else {
                    String::new()
                };
                let (rect, resp) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 34.0),
                    egui::Sense::click(),
                );
                let fill = if selected {
                    THEME.bg_chip_selected
                } else if resp.hovered() {
                    THEME.bg_hover
                } else {
                    THEME.bg_card
                };
                ui.painter().rect_filled(rect, 6.0, fill);
                let painter = ui.painter();
                painter.text(
                    rect.left_center() + egui::vec2(12.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    label,
                    egui::FontId::proportional(14.0),
                    THEME.text_primary,
                );
                let mut right = key;
                if action.requires_ai() {
                    right = format!("AI   {right}");
                }
                painter.text(
                    rect.right_center() - egui::vec2(12.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    right,
                    egui::FontId::monospace(12.0),
                    THEME.text_muted,
                );
                if resp.hovered() {
                    self.selected = i;
                }
                if resp.clicked() {
                    chosen = Some(action.clone());
                }
                ui.add_space(4.0);
            }
        });
        chosen
    }
}

fn digit_key(n: usize) -> egui::Key {
    match n {
        1 => egui::Key::Num1,
        2 => egui::Key::Num2,
        3 => egui::Key::Num3,
        4 => egui::Key::Num4,
        5 => egui::Key::Num5,
        6 => egui::Key::Num6,
        7 => egui::Key::Num7,
        8 => egui::Key::Num8,
        _ => egui::Key::Num9,
    }
}

impl eframe::App for PickerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_count = self.frame_count.saturating_add(1);
        if self.frame_count <= 5
            && let Some(monitor) = ctx.input(|i| i.viewport().monitor_size)
        {
            let x = (monitor.x - WINDOW_WIDTH) / 2.0;
            let y = (monitor.y - WINDOW_HEIGHT) / 3.0;
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([x, y].into()));
        }

        // Close when focus is lost (upstream CloseAfterLosingFocus).
        let focused = ctx.input(|i| i.viewport().focused);
        if focused == Some(true) {
            self.was_focused = true;
        } else if focused == Some(false) && self.was_focused && self.cfg.close_after_losing_focus {
            self.close = true;
        }

        self.ensure_image_texture(ctx);
        self.poll();

        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut next_action = None;

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(THEME.bg_primary).inner_margin(16.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("Advanced Paste")
                        .size(18.0)
                        .strong()
                        .color(THEME.text_primary),
                );
                ui.add_space(8.0);
                if self.cfg.enable_clipboard_preview {
                    self.draw_preview(ui);
                    ui.add_space(10.0);
                }

                match &mut self.state {
                    State::Choosing => {
                        if escape {
                            self.close = true;
                        }
                        next_action = self.choosing_ui(ui);
                    }
                    State::Running { label, .. } => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("{label}…"));
                        });
                        if escape {
                            self.state = State::Choosing;
                        }
                    }
                    State::Preview(text) => {
                        ui.label(egui::RichText::new("Result preview").color(THEME.text_secondary));
                        egui::ScrollArea::vertical()
                            .max_height(220.0)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::TextEdit::multiline(text)
                                        .desired_width(f32::INFINITY)
                                        .code_editor(),
                                );
                            });
                        ui.add_space(8.0);
                        let mut paste = false;
                        let mut back = escape;
                        ui.horizontal(|ui| {
                            paste = ui.button("Paste (Ctrl+Enter)").clicked();
                            back |= ui.button("Back (Esc)").clicked();
                        });
                        paste |= ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::Enter));
                        if paste {
                            let t = text.clone();
                            self.finish(Output::Text(t));
                        } else if back {
                            self.state = State::Choosing;
                        }
                    }
                    State::Error(msg) => {
                        ui.colored_label(THEME.text_error, msg.as_str());
                        ui.add_space(8.0);
                        if ui.button("Back").clicked()
                            || escape
                            || ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            self.state = State::Choosing;
                        }
                    }
                }
            });

        if let Some(action) = next_action {
            self.start(action);
        }

        if self.close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if matches!(self.state, State::Running { .. }) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

/// Show the picker. Returns the output chosen by the user, if any.
pub fn run_picker(cfg: PastePlainConfig, content: ClipboardContent) -> Option<Output> {
    let result = Arc::new(Mutex::new(None));
    let result_clone = result.clone();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Advanced Paste")
            .with_app_id("mpt-paste-plain")
            .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
            .with_always_on_top()
            .with_resizable(false),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "Advanced Paste",
        options,
        Box::new(move |cc| {
            THEME.apply(&cc.egui_ctx);
            Ok(Box::new(PickerApp::new(cfg, content, result_clone)))
        }),
    );
    result.lock().unwrap_or_else(|e| e.into_inner()).take()
}
