//! Borderless, centred search box with a result list (PowerToys Run UI).
//!
//! Keys: Up/Down or Tab/Shift+Tab move the selection, Enter runs the
//! result, Ctrl+Enter or Shift+Enter runs its alternative action, Esc closes.
//! The window also closes when it loses focus.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use egui::text::{CCursor, CCursorRange};
use egui::{Key, Modifiers, RichText, Shadow};

use crate::actions::{self, Action, ExecEnv};
use crate::config::{AppLauncherConfig, PluginId};
use crate::gui::{icons, theme};
use crate::plugins::history::HistoryStore;
use crate::plugins::{Engine, LauncherResult};

struct Request {
    generation: u64,
    query: String,
    include_slow: bool,
}

struct Response {
    generation: u64,
    include_slow: bool,
    results: Vec<LauncherResult>,
}

/// Runs the engine on a background thread so slow plugins (locate, X11)
/// never block rendering.
fn spawn_worker(
    config: AppLauncherConfig,
    ctx: egui::Context,
) -> (Sender<Request>, Receiver<Response>) {
    let (req_tx, req_rx) = mpsc::channel::<Request>();
    let (resp_tx, resp_rx) = mpsc::channel::<Response>();
    std::thread::spawn(move || {
        let mut engine = Engine::new(config);
        engine.load_apps();
        while let Ok(mut req) = req_rx.recv() {
            // Skip superseded requests.
            while let Ok(newer) = req_rx.try_recv() {
                req = newer;
            }
            let results = engine.query(&req.query, req.include_slow);
            if resp_tx
                .send(Response {
                    generation: req.generation,
                    include_slow: req.include_slow,
                    results,
                })
                .is_err()
            {
                break;
            }
            ctx.request_repaint();
        }
    });
    (req_tx, resp_rx)
}

enum CursorUpdate {
    None,
    SelectAll,
    End,
}

struct LauncherApp {
    config: AppLauncherConfig,
    exec_env: ExecEnv,
    query: String,
    results: Vec<LauncherResult>,
    selected: usize,
    tx: Sender<Request>,
    rx: Receiver<Response>,
    generation: u64,
    full_received: bool,
    fast_due: Option<Instant>,
    slow_due: Option<Instant>,
    searching: bool,
    icon_cache: icons::IconCache,
    cursor_update: CursorUpdate,
    origin: Option<(f32, f32)>,
    error: Option<String>,
    should_close: bool,
    closed: bool,
    frame_count: u32,
    had_focus: bool,
}

impl LauncherApp {
    fn new(config: AppLauncherConfig, origin: Option<(f32, f32)>, ctx: &egui::Context) -> Self {
        let (tx, rx) = spawn_worker(config.clone(), ctx.clone());
        let initial = if config.clear_input_on_launch {
            String::new()
        } else {
            HistoryStore::load().last_query
        };
        let exec_env = ExecEnv {
            terminal: config.terminal.clone(),
        };
        let mut app = Self {
            config,
            exec_env,
            query: initial,
            results: Vec::new(),
            selected: 0,
            tx,
            rx,
            generation: 0,
            full_received: false,
            fast_due: None,
            slow_due: None,
            searching: false,
            icon_cache: icons::IconCache::new(),
            cursor_update: CursorUpdate::SelectAll,
            origin,
            error: None,
            should_close: false,
            closed: false,
            frame_count: 0,
            had_focus: false,
        };
        if !app.query.is_empty() {
            app.schedule_search(true);
        }
        app
    }

    fn visible_count(&self) -> usize {
        self.results
            .len()
            .min(self.config.max_results.max(1))
            .min(theme::MAX_VISIBLE_RESULTS)
    }

    /// Queue searches for the current query. `immediate` skips the input delay.
    fn schedule_search(&mut self, immediate: bool) {
        self.generation += 1;
        self.full_received = false;
        self.error = None;
        if self.query.trim().is_empty() {
            self.results.clear();
            self.selected = 0;
            self.fast_due = None;
            self.slow_due = None;
            self.searching = false;
            return;
        }
        self.searching = true;
        let now = Instant::now();
        if immediate || !self.config.search_query_results_with_delay {
            self.fast_due = None;
            self.slow_due = Some(now);
        } else {
            self.fast_due =
                Some(now + Duration::from_millis(self.config.search_input_delay_fast_ms));
            self.slow_due = Some(now + Duration::from_millis(self.config.search_input_delay_ms));
        }
    }

    fn pump_searches(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        if let Some(t) = self.fast_due
            && now >= t
        {
            self.fast_due = None;
            let _ = self.tx.send(Request {
                generation: self.generation,
                query: self.query.clone(),
                include_slow: false,
            });
        }
        if let Some(t) = self.slow_due
            && now >= t
        {
            self.slow_due = None;
            let _ = self.tx.send(Request {
                generation: self.generation,
                query: self.query.clone(),
                include_slow: true,
            });
        }
        while let Ok(resp) = self.rx.try_recv() {
            if resp.generation != self.generation || (self.full_received && !resp.include_slow) {
                continue;
            }
            if resp.include_slow {
                self.full_received = true;
                self.searching = false;
            }
            self.results = resp.results;
            if self.selected >= self.visible_count() {
                self.selected = 0;
            }
        }
        // Wake up for the next due search.
        if let Some(next) = [self.fast_due, self.slow_due].into_iter().flatten().min() {
            ctx.request_repaint_after(next.saturating_duration_since(now));
        }
    }

    fn activate(&mut self, index: usize, alt: bool) {
        let Some(result) = self.results.get(index).cloned() else {
            return;
        };
        let action = if alt {
            match &result.alt_action {
                Some((_, a)) => a.clone(),
                None => result.action.clone(),
            }
        } else {
            result.action.clone()
        };
        match &action {
            Action::None => return,
            Action::SetQuery(q) => {
                self.query = q.clone();
                self.cursor_update = CursorUpdate::End;
                self.selected = 0;
                self.schedule_search(true);
                return;
            }
            _ => {}
        }

        let mut history = HistoryStore::load();
        history.record(&self.query, &result, self.config.history_size);
        history.last_query = self.query.clone();
        if let Err(e) = history.save() {
            tracing::warn!("App Launcher: failed to save history: {e}");
        }

        match actions::execute(&action, &self.exec_env) {
            Ok(()) => {
                if self.config.clear_input_on_launch {
                    self.query.clear();
                }
                self.should_close = true;
            }
            Err(e) => {
                tracing::warn!("App Launcher: action failed: {e:#}");
                self.error = Some(format!("{e:#}"));
            }
        }
    }

    /// Persist the last query once, right before closing.
    fn finish(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        let mut history = HistoryStore::load();
        history.last_query = if self.config.clear_input_on_launch {
            String::new()
        } else {
            self.query.clone()
        };
        let _ = history.save();
    }

    fn handle_keys(&mut self, ctx: &egui::Context) {
        let mut down = false;
        let mut up = false;
        let mut enter = false;
        let mut alt_enter = false;
        ctx.input_mut(|i| {
            if i.consume_key(Modifiers::NONE, Key::Escape) {
                self.should_close = true;
            }
            if i.consume_key(Modifiers::SHIFT, Key::Tab) {
                up = true;
            } else if i.consume_key(Modifiers::NONE, Key::Tab) {
                down = true;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                down = true;
            }
            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                up = true;
            }
            if i.consume_key(Modifiers::CTRL, Key::Enter)
                || i.consume_key(Modifiers::SHIFT, Key::Enter)
            {
                alt_enter = true;
            } else if i.consume_key(Modifiers::NONE, Key::Enter) {
                enter = true;
            }
        });
        let count = self.visible_count();
        if count > 0 {
            if down {
                self.selected = (self.selected + 1) % count;
            }
            if up {
                self.selected = (self.selected + count - 1) % count;
            }
        }
        if enter || alt_enter {
            if self.searching && self.results.is_empty() {
                // Results not in yet: run the full search now, then act.
                self.fast_due = None;
                self.slow_due = None;
                let mut engine = Engine::new(self.config.clone());
                engine.load_apps();
                self.results = engine.query(&self.query, true);
                self.searching = false;
            }
            self.activate(self.selected, alt_enter);
        }
    }

    fn draw_search_bar(&mut self, ui: &mut egui::Ui) {
        let frame =
            egui::Frame::NONE.inner_margin(egui::Margin::symmetric(theme::INNER_PADDING as i8, 12));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("\u{1F50D}")
                        .size(theme::FONT_ICON)
                        .color(theme::TEXT_HINT),
                );
                ui.add_space(8.0);
                let mut output = egui::TextEdit::singleline(&mut self.query)
                    .desired_width(ui.available_width())
                    .font(egui::FontId::proportional(theme::FONT_SEARCH))
                    .text_color(theme::TEXT_PRIMARY)
                    .frame(false)
                    .lock_focus(true)
                    .hint_text(RichText::new("Start typing...").color(theme::TEXT_HINT))
                    .show(ui);
                output.response.request_focus();

                let len = self.query.chars().count();
                let range = match self.cursor_update {
                    CursorUpdate::None => None,
                    CursorUpdate::SelectAll => {
                        Some(CCursorRange::two(CCursor::new(0), CCursor::new(len)))
                    }
                    CursorUpdate::End => Some(CCursorRange::one(CCursor::new(len))),
                };
                if let Some(range) = range {
                    output.state.cursor.set_char_range(Some(range));
                    output.state.store(ui.ctx(), output.response.id);
                    self.cursor_update = CursorUpdate::None;
                }

                if output.response.changed() {
                    self.selected = 0;
                    self.schedule_search(false);
                }
            });
        });
    }

    fn draw_results(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let mut clicked: Option<(usize, bool)> = None;
        for i in 0..self.visible_count() {
            let selected = i == self.selected;
            let frame = egui::Frame::NONE
                .fill(if selected {
                    theme::BG_SELECTED
                } else {
                    egui::Color32::TRANSPARENT
                })
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(theme::INNER_PADDING as i8, 6));
            let result = &self.results[i];
            let resp = frame
                .show(ui, |ui| {
                    ui.set_min_height(theme::RESULT_ROW_HEIGHT - 12.0);
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let mut drawn = false;
                        if let Some(name) = &result.icon
                            && let Some(src) = self.icon_cache.get(&ctx, name)
                        {
                            ui.add(egui::Image::new(src).fit_to_exact_size(icons::icon_size()));
                            drawn = true;
                        }
                        if !drawn {
                            ui.add_sized(
                                icons::icon_size(),
                                egui::Label::new(
                                    RichText::new(plugin_glyph(result.plugin))
                                        .size(theme::FONT_ICON)
                                        .color(theme::ACCENT),
                                ),
                            );
                        }
                        ui.add_space(10.0);
                        ui.vertical(|ui| {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&result.title)
                                        .size(theme::FONT_TITLE)
                                        .color(theme::TEXT_PRIMARY),
                                )
                                .truncate(),
                            );
                            if !result.subtitle.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(&result.subtitle)
                                            .size(theme::FONT_SUBTITLE)
                                            .color(theme::TEXT_SECONDARY),
                                    )
                                    .truncate(),
                                );
                            }
                        });
                    });
                })
                .response;
            let resp = resp.interact(egui::Sense::click());
            if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                self.selected = i;
            }
            if resp.clicked() {
                let alt = ui.input(|i| i.modifiers.ctrl || i.modifiers.shift);
                clicked = Some((i, alt));
            }
        }
        if let Some((i, alt)) = clicked {
            self.selected = i;
            self.activate(i, alt);
        }
    }

    fn draw_footer(&self, ui: &mut egui::Ui) {
        let text = if let Some(err) = &self.error {
            RichText::new(format!("\u{26A0} {err}")).color(egui::Color32::from_rgb(240, 120, 110))
        } else if let Some(r) = self.results.get(self.selected) {
            let main = action_label(&r.action);
            let mut s = format!("{}  ·  Enter: {main}", r.plugin.display_name());
            if let Some((label, _)) = &r.alt_action {
                s.push_str(&format!("  ·  Ctrl+Enter: {label}"));
            }
            RichText::new(s).color(theme::TEXT_TAG)
        } else {
            return;
        };
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(theme::INNER_PADDING as i8, 4))
            .show(ui, |ui| {
                ui.add(egui::Label::new(text.size(theme::FONT_TAG)).truncate());
            });
    }

    fn separator(ui: &mut egui::Ui) {
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(theme::INNER_PADDING, ui.cursor().min.y),
                egui::vec2(theme::WINDOW_WIDTH - theme::INNER_PADDING * 2.0, 1.0),
            ),
            0.0,
            theme::SEPARATOR,
        );
        ui.add_space(2.0);
    }
}

fn plugin_glyph(p: PluginId) -> &'static str {
    match p {
        PluginId::Program => "\u{25B6}",
        PluginId::Calculator => "=",
        PluginId::Shell => ">_",
        PluginId::Folder => "\u{1F4C1}",
        PluginId::Uri => "\u{1F517}",
        PluginId::WebSearch => "\u{1F310}",
        PluginId::WindowWalker => "\u{25A3}",
        PluginId::System => "\u{23FB}",
        PluginId::TimeDate => "\u{23F0}",
        PluginId::UnitConverter => "\u{21C4}",
        PluginId::ValueGenerator => "#",
        PluginId::History => "\u{21BA}",
        PluginId::Indexer => "\u{1F4C4}",
    }
}

fn action_label(a: &Action) -> &'static str {
    match a {
        Action::None => "-",
        Action::Launch { .. } => "Launch",
        Action::RunCommand { .. } => "Run",
        Action::Open(_) => "Open",
        Action::OpenContainingFolder(_) => "Open containing folder",
        Action::OpenTerminalAt(_) => "Open in terminal",
        Action::Copy(_) => "Copy",
        Action::ActivateWindow(_) => "Switch to",
        Action::CloseWindow(_) => "Close",
        Action::System(_) => "Run",
        Action::SetQuery(_) => "Use",
    }
}

impl eframe::App for LauncherApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frame_count = self.frame_count.saturating_add(1);

        // Position during the first frames (some WMs ignore the initial position).
        if self.frame_count <= 5 {
            let ppp = ctx
                .input(|i| i.viewport().native_pixels_per_point)
                .unwrap_or(1.0)
                .max(0.1);
            if let Some((x, y)) = self.origin {
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                    [x / ppp, y / ppp].into(),
                ));
            } else if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
                let x = (monitor.x - theme::WINDOW_WIDTH) / 2.0;
                let y = monitor.y * 0.25;
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([x, y].into()));
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        self.pump_searches(ctx);
        self.handle_keys(ctx);

        // Close on focus loss (only after the window had focus once).
        let focused = ctx.input(|i| i.focused);
        if focused {
            self.had_focus = true;
        } else if self.had_focus {
            self.should_close = true;
        }

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_PRIMARY)
                    .corner_radius(theme::CORNER_RADIUS)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                self.draw_search_bar(ui);
                if self.visible_count() > 0 {
                    Self::separator(ui);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(4, 0))
                        .show(ui, |ui| self.draw_results(ui));
                    self.draw_footer(ui);
                } else if !self.query.trim().is_empty() {
                    Self::separator(ui);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(theme::INNER_PADDING as i8, 12))
                        .show(ui, |ui| {
                            let msg = if self.searching {
                                "Searching..."
                            } else {
                                "No results"
                            };
                            ui.label(
                                RichText::new(msg)
                                    .size(theme::FONT_TITLE)
                                    .color(theme::TEXT_HINT),
                            );
                        });
                    if self.error.is_some() {
                        self.draw_footer(ui);
                    }
                }
            });

        // Resize to content.
        let rows = self.visible_count();
        let has_query = !self.query.trim().is_empty();
        let footer = if rows > 0 || self.error.is_some() {
            theme::FOOTER_HEIGHT
        } else {
            0.0
        };
        let height = theme::SEARCH_BAR_HEIGHT
            + if rows > 0 {
                4.0 + rows as f32 * theme::RESULT_ROW_HEIGHT + 8.0
            } else if has_query {
                4.0 + theme::RESULT_ROW_HEIGHT
            } else {
                0.0
            }
            + footer;
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            theme::WINDOW_WIDTH,
            height,
        )));

        if self.should_close {
            self.finish();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl Drop for LauncherApp {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Open the launcher window. `origin` is the top-left corner in physical
/// root-window pixels (see [`crate::placement::resolve_origin`]).
pub fn run_launcher(config: AppLauncherConfig, origin: Option<(f32, f32)>) {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("App Launcher")
        .with_app_id("mpt-app-launcher")
        .with_inner_size([theme::WINDOW_WIDTH, theme::SEARCH_BAR_HEIGHT])
        .with_min_inner_size([400.0, theme::SEARCH_BAR_HEIGHT])
        .with_decorations(false)
        .with_always_on_top()
        .with_transparent(true)
        .with_resizable(false)
        .with_taskbar(false);
    if let Some((x, y)) = origin {
        viewport = viewport.with_position([x, y]);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "App Launcher",
        options,
        Box::new(move |cc| {
            setup_visuals(&cc.egui_ctx);
            Ok(Box::new(LauncherApp::new(config, origin, &cc.egui_ctx)))
        }),
    ) {
        tracing::error!("App Launcher: failed to open window: {e}");
    }
}

fn setup_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = egui::Color32::TRANSPARENT;
    visuals.panel_fill = egui::Color32::TRANSPARENT;
    visuals.window_shadow = Shadow::NONE;
    visuals.window_stroke = egui::Stroke::NONE;
    visuals.widgets.noninteractive.bg_fill = egui::Color32::TRANSPARENT;
    ctx.set_visuals(visuals);
}
