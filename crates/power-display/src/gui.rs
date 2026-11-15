//! Compact egui flyout: per-monitor sliders and pickers, profiles, identify.
//!
//! All hardware access happens on a worker thread. Slider writes are
//! debounced and coalesced there because ddcutil calls take 50-500 ms.

use crate::config::{PowerDisplayConfig, Profile};
use crate::debounce::Coalescer;
use crate::display::{Backend, Controller, Monitor};
use crate::vcp::Feature;
use crate::xrandr::Rotation;
use eframe::egui;
use mpt_common::theme::Theme;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

pub const WINDOW_WIDTH: f32 = 380.0;
pub const WINDOW_HEIGHT: f32 = 520.0;
const IDENTIFY_DURATION: Duration = Duration::from_secs(3);

enum Cmd {
    Refresh,
    Set {
        id: String,
        feature: Feature,
        value: u16,
    },
    Rotate {
        id: String,
        rotation: Rotation,
    },
    ApplyProfile(Profile),
}

enum Event {
    Busy(bool),
    Monitors {
        monitors: Vec<Monitor>,
        warnings: Vec<String>,
    },
    MonitorUpdated(Box<Monitor>),
    Error(String),
    Info(String),
}

// ── Worker ──────────────────────────────────────────────────────────────────

struct Worker {
    controller: Controller,
    monitors: Vec<Monitor>,
    events: Sender<Event>,
    ctx: egui::Context,
    debounce: Duration,
}

impl Worker {
    fn send(&self, e: Event) {
        let _ = self.events.send(e);
        self.ctx.request_repaint();
    }

    fn run(mut self, rx: Receiver<Cmd>) {
        self.refresh();
        let mut pending: Coalescer<(String, Feature), u16> = Coalescer::default();
        loop {
            let cmd = if pending.is_empty() {
                match rx.recv() {
                    Ok(c) => Some(c),
                    Err(_) => return,
                }
            } else {
                match rx.recv_timeout(self.debounce) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => {
                        self.flush(&mut pending);
                        return;
                    }
                }
            };
            match cmd {
                Some(Cmd::Set { id, feature, value }) => pending.push((id, feature), value),
                Some(other) => {
                    self.flush(&mut pending);
                    self.handle(other);
                }
                None => self.flush(&mut pending),
            }
        }
    }

    fn flush(&mut self, pending: &mut Coalescer<(String, Feature), u16>) {
        let writes = pending.take();
        if writes.is_empty() {
            return;
        }
        self.send(Event::Busy(true));
        for ((id, feature), value) in writes {
            let Some(m) = self.monitors.iter_mut().find(|m| m.id == id) else {
                continue;
            };
            if let Err(e) = self.controller.set(m, feature, value) {
                let msg = format!("{e:#}");
                // Re-read so the UI shows the real value.
                let _ = self.controller.refresh_values(m);
                let snapshot = m.clone();
                self.send(Event::Error(msg));
                self.send(Event::MonitorUpdated(Box::new(snapshot)));
            }
        }
        self.send(Event::Busy(false));
    }

    fn handle(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Refresh => self.refresh(),
            Cmd::Rotate { id, rotation } => {
                self.send(Event::Busy(true));
                if let Some(m) = self.monitors.iter_mut().find(|m| m.id == id) {
                    let res = self.controller.set_rotation(m, rotation);
                    let snapshot = m.clone();
                    if let Err(e) = res {
                        self.send(Event::Error(format!("{e:#}")));
                    }
                    self.send(Event::MonitorUpdated(Box::new(snapshot)));
                }
                self.send(Event::Busy(false));
            }
            Cmd::ApplyProfile(p) => {
                self.send(Event::Busy(true));
                let errors = self.controller.apply_profile(&mut self.monitors, &p);
                self.send(Event::Monitors {
                    monitors: self.monitors.clone(),
                    warnings: vec![],
                });
                if errors.is_empty() {
                    self.send(Event::Info(format!("Applied profile \"{}\"", p.name)));
                } else {
                    self.send(Event::Error(errors.join("; ")));
                }
                self.send(Event::Busy(false));
            }
            Cmd::Set { .. } => unreachable!("handled by the debouncer"),
        }
    }

    fn refresh(&mut self) {
        self.send(Event::Busy(true));
        let d = self.controller.discover();
        self.monitors = d.monitors;
        self.send(Event::Monitors {
            monitors: self.monitors.clone(),
            warnings: d.warnings,
        });
        self.send(Event::Busy(false));
    }
}

// ── App ─────────────────────────────────────────────────────────────────────

struct Identify {
    until: Instant,
    /// (number, x, y, w, h)
    targets: Vec<(usize, i32, i32, u32, u32)>,
}

struct App {
    theme: Theme,
    config: PowerDisplayConfig,
    tx: Sender<Cmd>,
    rx: Receiver<Event>,
    monitors: Vec<Monitor>,
    warnings: Vec<String>,
    status: Option<(String, bool, Instant)>,
    busy: bool,
    loaded: bool,
    new_profile_name: String,
    selected_profile: Option<String>,
    identify: Option<Identify>,
    was_focused: bool,
}

impl App {
    fn new(ctx: &egui::Context) -> Self {
        let theme = Theme::dark();
        theme.apply(ctx);
        let config = PowerDisplayConfig::load();
        let (tx, cmd_rx) = mpsc::channel();
        let (ev_tx, rx) = mpsc::channel();
        let worker = Worker {
            controller: Controller::new(config.clone()),
            monitors: vec![],
            events: ev_tx,
            ctx: ctx.clone(),
            debounce: Duration::from_millis(config.slider_debounce_ms.clamp(50, 2000)),
        };
        std::thread::spawn(move || worker.run(cmd_rx));
        Self {
            theme,
            config,
            tx,
            rx,
            monitors: vec![],
            warnings: vec![],
            status: None,
            busy: true,
            loaded: false,
            new_profile_name: String::new(),
            selected_profile: None,
            identify: None,
            was_focused: false,
        }
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send(cmd);
    }

    fn set_status(&mut self, msg: String, error: bool) {
        self.status = Some((msg, error, Instant::now()));
    }

    fn save_config(&mut self) {
        if let Err(e) = self.config.save() {
            self.set_status(format!("Failed to save config: {e:#}"), true);
        }
    }

    fn poll_events(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Busy(b) => self.busy = b,
                Event::Monitors { monitors, warnings } => {
                    self.monitors = monitors;
                    if !warnings.is_empty() || !self.loaded {
                        self.warnings = warnings;
                    }
                    self.loaded = true;
                }
                Event::MonitorUpdated(m) => {
                    if let Some(slot) = self.monitors.iter_mut().find(|x| x.id == m.id) {
                        *slot = *m;
                    }
                }
                Event::Error(e) => self.set_status(e, true),
                Event::Info(i) => self.set_status(i, false),
            }
        }
    }

    fn visible_indices(&self) -> Vec<usize> {
        (0..self.monitors.len())
            .filter(|&i| !self.config.flags_for(&self.monitors[i].id).hidden)
            .collect()
    }

    /// Update the local value and queue the write; propagates brightness to
    /// linked monitors.
    fn change(&mut self, idx: usize, feature: Feature, value: u16) {
        let id = self.monitors[idx].id.clone();
        let mut targets = vec![idx];
        if feature == Feature::Brightness && self.config.is_linked(&id) {
            targets = self
                .visible_indices()
                .into_iter()
                .filter(|&i| {
                    let m = &self.monitors[i];
                    m.supports(Feature::Brightness) && self.config.is_linked(&m.id)
                })
                .collect();
        }
        for i in targets {
            let m = &mut self.monitors[i];
            m.values.insert(feature, value);
            let id = m.id.clone();
            self.send(Cmd::Set { id, feature, value });
        }
    }

    fn start_identify(&mut self) {
        let geoms = mpt_common::monitor::detect_monitors().unwrap_or_default();
        let mut targets = Vec::new();
        for (n, &i) in self.visible_indices().iter().enumerate() {
            if let Some(out) = &self.monitors[i].xrandr_output
                && let Some(g) = geoms.iter().find(|g| &g.name == out)
            {
                targets.push((n + 1, g.x, g.y, g.width, g.height));
            }
        }
        if targets.is_empty() {
            self.set_status(
                "Identify needs X11 outputs matched to monitors".into(),
                true,
            );
            return;
        }
        self.identify = Some(Identify {
            until: Instant::now() + IDENTIFY_DURATION,
            targets,
        });
    }

    fn show_identify(&mut self, ctx: &egui::Context) {
        let Some(ident) = &self.identify else { return };
        if Instant::now() >= ident.until {
            self.identify = None;
            return;
        }
        let accent = self.theme.text_accent;
        for &(n, x, y, w, h) in &ident.targets {
            let size = 220.0;
            let pos = egui::pos2(
                x as f32 + (w as f32 - size) / 2.0,
                y as f32 + (h as f32 - size) / 2.0,
            );
            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(("mpt-power-display-identify", n)),
                egui::ViewportBuilder::default()
                    .with_title(format!("Monitor {n}"))
                    .with_position(pos)
                    .with_inner_size([size, size])
                    .with_decorations(false)
                    .with_resizable(false)
                    .with_always_on_top(),
                move |ctx, _| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                egui::RichText::new(n.to_string())
                                    .size(140.0)
                                    .strong()
                                    .color(accent),
                            );
                        });
                    });
                },
            );
        }
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Power Display")
                    .size(16.0)
                    .strong()
                    .color(self.theme.text_primary),
            );
            if self.busy {
                ui.spinner();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("✕").on_hover_text("Close (Esc)").clicked() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("⟳"))
                    .on_hover_text("Refresh monitors")
                    .clicked()
                {
                    self.send(Cmd::Refresh);
                }
                if self.config.show_identify_monitors_button
                    && self.visible_indices().len() > 1
                    && ui.button("🖵").on_hover_text("Identify monitors").clicked()
                {
                    self.start_identify();
                }
                let linkable = self
                    .visible_indices()
                    .iter()
                    .filter(|&&i| self.monitors[i].supports(Feature::Brightness))
                    .count()
                    > 1;
                if linkable {
                    let mut linked = self.config.linked_levels_active;
                    if ui
                        .toggle_value(&mut linked, "🔗")
                        .on_hover_text("Link brightness across monitors")
                        .changed()
                    {
                        self.config.linked_levels_active = linked;
                        self.save_config();
                    }
                }
            });
        });
    }

    fn profiles_bar(&mut self, ui: &mut egui::Ui) {
        if !self.config.show_profile_switcher {
            return;
        }
        let card = egui::Frame::NONE
            .fill(self.theme.bg_card)
            .stroke(egui::Stroke::new(1.0, self.theme.card_border))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(10));
        card.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label("Profile");
                let names: Vec<String> = self
                    .config
                    .profiles
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                let text = self
                    .selected_profile
                    .clone()
                    .unwrap_or_else(|| "Choose…".into());
                let mut chosen = None;
                egui::ComboBox::from_id_salt("profile-picker")
                    .selected_text(text)
                    .width(150.0)
                    .show_ui(ui, |ui| {
                        if names.is_empty() {
                            ui.label("No profiles yet");
                        }
                        for n in &names {
                            if ui
                                .selectable_label(self.selected_profile.as_ref() == Some(n), n)
                                .clicked()
                            {
                                chosen = Some(n.clone());
                            }
                        }
                    });
                if let Some(n) = chosen {
                    self.selected_profile = Some(n.clone());
                    if let Some(p) = self.config.profile(&n).cloned() {
                        self.send(Cmd::ApplyProfile(p));
                    }
                }
                if let Some(sel) = self.selected_profile.clone()
                    && ui
                        .small_button("🗑")
                        .on_hover_text("Delete profile")
                        .clicked()
                {
                    self.config.remove_profile(&sel);
                    if self.config.startup_profile.as_deref() == Some(sel.as_str()) {
                        self.config.startup_profile = None;
                    }
                    self.selected_profile = None;
                    self.save_config();
                }
            });
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_profile_name)
                        .hint_text("New profile name")
                        .desired_width(180.0),
                );
                let name = self.new_profile_name.trim().to_string();
                if ui
                    .add_enabled(!name.is_empty(), egui::Button::new("Save current"))
                    .clicked()
                {
                    let profile =
                        Controller::new(self.config.clone()).profile_from(&name, &self.monitors);
                    if profile.is_valid() {
                        self.config.upsert_profile(profile);
                        self.save_config();
                        self.selected_profile = Some(name.clone());
                        self.new_profile_name.clear();
                        self.set_status(format!("Saved profile \"{name}\""), false);
                    } else {
                        self.set_status("Nothing to save: no monitor values".into(), true);
                    }
                }
            });
            if let Some(sel) = self.selected_profile.clone() {
                let mut on_start = self.config.startup_profile.as_deref() == Some(sel.as_str());
                if ui
                    .checkbox(&mut on_start, "Apply this profile on startup")
                    .changed()
                {
                    self.config.startup_profile = on_start.then_some(sel);
                    self.save_config();
                }
            }
        });
    }

    /// Slider with mouse-wheel support. Returns the new value if changed.
    fn slider(&self, ui: &mut egui::Ui, label: &str, value: u16) -> Option<u16> {
        let mut v = value.min(100);
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.add_sized(
                [78.0, 18.0],
                egui::Label::new(egui::RichText::new(label).color(self.theme.text_secondary)),
            );
            ui.spacing_mut().slider_width = ui.available_width() - 56.0;
            let resp = ui.add(egui::Slider::new(&mut v, 0..=100).suffix("%"));
            changed |= resp.changed();
            if resp.hovered() {
                let dy = ui.input(|i| i.raw_scroll_delta.y);
                if dy != 0.0 {
                    let step = self.config.mouse_wheel_increment.clamp(1, 50) as i16;
                    let delta = if dy > 0.0 { step } else { -step };
                    v = crate::display::step_value(v, delta);
                    changed = true;
                    // Don't also scroll the surrounding area.
                    ui.input_mut(|i| {
                        i.raw_scroll_delta = egui::Vec2::ZERO;
                        i.smooth_scroll_delta = egui::Vec2::ZERO;
                    });
                }
            }
        });
        (changed && v != value).then_some(v)
    }

    /// Dropdown for a non-continuous feature. Returns the chosen value.
    fn picker(&self, ui: &mut egui::Ui, m: &Monitor, feature: Feature) -> Option<u16> {
        let info = m.features.get(&feature)?;
        let code = feature.vcp_code();
        let current = m.value(feature);
        let text = current
            .map(|v| self.config.value_name(&m.id, code, v))
            .unwrap_or_else(|| "Unknown".into());
        let mut chosen = None;
        ui.horizontal(|ui| {
            ui.add_sized(
                [78.0, 18.0],
                egui::Label::new(
                    egui::RichText::new(feature.label()).color(self.theme.text_secondary),
                ),
            );
            egui::ComboBox::from_id_salt((&m.id, code))
                .selected_text(text)
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    for &v in &info.values {
                        let name = self.config.value_name(&m.id, code, v);
                        if ui.selectable_label(current == Some(v), name).clicked() {
                            chosen = Some(v);
                        }
                    }
                });
        });
        chosen.filter(|v| Some(*v) != current)
    }

    fn monitor_card(&mut self, ui: &mut egui::Ui, idx: usize, number: usize) {
        let m = self.monitors[idx].clone();
        let flags = self.config.flags_for(&m.id);
        let card = egui::Frame::NONE
            .fill(self.theme.bg_card)
            .stroke(egui::Stroke::new(1.0, self.theme.card_border))
            .corner_radius(8.0)
            .inner_margin(egui::Margin::same(10));
        let mut changes: Vec<(Feature, u16)> = Vec::new();
        let mut rotate = None;
        card.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("{number}"))
                        .strong()
                        .color(self.theme.text_accent),
                );
                ui.label(
                    egui::RichText::new(&m.name)
                        .strong()
                        .color(self.theme.text_primary),
                );
                let mut detail = m.backend.label().to_string();
                if let Some(o) = &m.xrandr_output {
                    detail = format!("{o} · {detail}");
                }
                ui.label(
                    egui::RichText::new(detail)
                        .small()
                        .color(self.theme.text_muted),
                );
                if self.config.is_linked(&m.id) && m.supports(Feature::Brightness) {
                    ui.label(egui::RichText::new("🔗").small())
                        .on_hover_text("Brightness linked");
                }
            });
            ui.add_space(4.0);

            if m.features.is_empty() {
                ui.label(
                    egui::RichText::new("No controls available (DDC/CI disabled or unsupported?)")
                        .color(self.theme.text_muted),
                );
            }

            let continuous = [
                (Feature::Brightness, true),
                (Feature::Contrast, flags.contrast),
                (Feature::Volume, flags.volume),
            ];
            for (f, enabled) in continuous {
                if enabled
                    && m.supports(f)
                    && let Some(v) = self.slider(ui, f.label(), m.value(f).unwrap_or(0))
                {
                    changes.push((f, v));
                }
            }
            let pickers = [
                (Feature::ColorPreset, flags.color_temperature),
                (Feature::InputSource, flags.input_source),
                (Feature::PowerMode, flags.power_state),
            ];
            for (f, enabled) in pickers {
                if enabled
                    && m.supports(f)
                    && let Some(v) = self.picker(ui, &m, f)
                {
                    changes.push((f, v));
                }
            }
            if flags.rotation
                && m.xrandr_output.is_some()
                && let Some(current) = m.rotation
            {
                ui.horizontal(|ui| {
                    ui.add_sized(
                        [78.0, 18.0],
                        egui::Label::new(
                            egui::RichText::new("Orientation").color(self.theme.text_secondary),
                        ),
                    );
                    egui::ComboBox::from_id_salt((&m.id, "rotation"))
                        .selected_text(current.label())
                        .show_ui(ui, |ui| {
                            for r in Rotation::ALL {
                                if ui.selectable_label(r == current, r.label()).clicked()
                                    && r != current
                                {
                                    rotate = Some(r);
                                }
                            }
                        });
                });
            }
            if matches!(m.backend, Backend::Software { .. }) {
                ui.label(
                    egui::RichText::new("Software dimming (no hardware control found)")
                        .small()
                        .color(self.theme.text_muted),
                );
            }
        });

        for (f, v) in changes {
            self.change(idx, f, v);
        }
        if let Some(r) = rotate {
            self.monitors[idx].rotation = Some(r);
            self.send(Cmd::Rotate {
                id: m.id.clone(),
                rotation: r,
            });
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_events();

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Flyout behaviour: close when focus moves elsewhere (after first focus).
        match ctx.input(|i| i.viewport().focused) {
            Some(true) => self.was_focused = true,
            Some(false) if self.was_focused && self.identify.is_none() => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(self.theme.bg_primary)
                    .inner_margin(egui::Margin::same(12)),
            )
            .show(ctx, |ui| {
                self.header(ui);
                ui.add_space(6.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let visible = self.visible_indices();
                        if self.loaded && visible.is_empty() {
                            ui.label(
                                egui::RichText::new("No controllable monitors found.")
                                    .color(self.theme.text_secondary),
                            );
                        } else if !self.loaded {
                            ui.label(
                                egui::RichText::new("Detecting monitors…")
                                    .color(self.theme.text_secondary),
                            );
                        }
                        for (n, idx) in visible.into_iter().enumerate() {
                            self.monitor_card(ui, idx, n + 1);
                            ui.add_space(8.0);
                        }
                        if self.loaded {
                            self.profiles_bar(ui);
                        }
                        for w in &self.warnings {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(format!("⚠ {w}"))
                                    .small()
                                    .color(self.theme.text_muted),
                            );
                        }
                    });
            });

        if let Some((msg, error, at)) = &self.status {
            if at.elapsed() > Duration::from_secs(6) {
                self.status = None;
            } else {
                let color = if *error {
                    self.theme.text_error
                } else {
                    self.theme.text_success
                };
                let msg = msg.clone();
                egui::TopBottomPanel::bottom("status")
                    .frame(
                        egui::Frame::NONE
                            .fill(self.theme.bg_header)
                            .inner_margin(egui::Margin::symmetric(12, 6)),
                    )
                    .show(ctx, |ui| {
                        ui.label(egui::RichText::new(msg).small().color(color));
                    });
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }

        self.show_identify(ctx);
    }
}

/// Open the flyout. `position` is the top-left corner in screen coordinates.
pub fn run_flyout(position: Option<[f32; 2]>) {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Power Display")
        .with_app_id("mpt-power-display")
        .with_inner_size([WINDOW_WIDTH, WINDOW_HEIGHT])
        .with_min_inner_size([320.0, 240.0])
        .with_decorations(false)
        .with_always_on_top()
        .with_resizable(true);
    if let Some(p) = position {
        viewport = viewport.with_position(p);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "Power Display",
        options,
        Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx)))),
    ) {
        tracing::error!("Power Display flyout failed: {e}");
    }
}
