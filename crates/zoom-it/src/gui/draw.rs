//! Draw mode: annotate a frozen screenshot (or the frozen zoom view).
//!
//! Keys (upstream ZoomIt): R G B O Y P pen color, Shift+color highlighter,
//! Ctrl+wheel pen width, hold Shift = line, Ctrl = rectangle, Tab = ellipse,
//! Ctrl+Shift = arrow, E erase, W whiteboard, K blackboard, Ctrl+Z undo,
//! T type text (wheel = font size), Ctrl+C copy, Ctrl+S save, Esc exit.

use super::zoom::ZoomState;
use super::{Shared, State, Transition, color_image_to_rgba, color32, uv_of};
use crate::config::ZoomItConfig;
use crate::geometry::{self, Point};
use crate::output;
use eframe::egui::{self, Color32, Pos2};

const MAX_UNDO: usize = 32;
const HIGHLIGHT_ALPHA: u8 = 110;
const MIN_FONT: f32 = 8.0;
const MAX_FONT: f32 = 300.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Background {
    Screen,
    White,
    Black,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Free,
    Line,
    Rect,
    Ellipse,
    Arrow,
}

#[derive(Debug, Clone)]
enum Kind {
    Free(Vec<Pos2>),
    /// Two-point shape; `a` is where the drag started.
    Two {
        tool: Tool,
        a: Pos2,
        b: Pos2,
    },
    Text {
        pos: Pos2,
        text: String,
        size: f32,
    },
}

#[derive(Debug, Clone)]
struct Mark {
    kind: Kind,
    color: Color32,
    width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Export {
    Copy,
    Save,
}

struct Typing {
    pos: Pos2,
    text: String,
}

pub(crate) struct DrawState {
    /// Region of the screen shown full-screen (frozen zoom), or `None` for 1:1.
    view: Option<geometry::Rect>,
    /// Zoom state to return to on Esc when entered from zoom mode.
    back: Option<ZoomState>,
    bg: Background,
    marks: Vec<Mark>,
    undo: Vec<(Vec<Mark>, Background)>,
    current: Option<Mark>,
    color: Color32,
    highlight: bool,
    width: f32,
    font_size: f32,
    typing: Option<Typing>,
    pending: Option<Export>,
    /// The stroke started by the click that entered draw mode from zoom;
    /// dropped if it turns out to be a plain click (no stray dot).
    entry_stroke: bool,
}

impl DrawState {
    pub fn new(cfg: &ZoomItConfig, view: Option<geometry::Rect>, back: Option<ZoomState>) -> Self {
        Self {
            view,
            back,
            bg: Background::Screen,
            marks: Vec::new(),
            undo: Vec::new(),
            current: None,
            color: color32(&cfg.pen_color, Color32::RED),
            highlight: false,
            width: geometry::clamp_pen_width(cfg.pen_width),
            font_size: cfg.font_size.clamp(MIN_FONT, MAX_FONT),
            typing: None,
            pending: None,
            entry_stroke: false,
        }
    }

    fn pen_color(&self) -> Color32 {
        if self.highlight {
            let [r, g, b, _] = self.color.to_array();
            Color32::from_rgba_unmultiplied(r, g, b, HIGHLIGHT_ALPHA)
        } else {
            self.color
        }
    }

    fn push_undo(&mut self) {
        self.undo.push((self.marks.clone(), self.bg));
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    /// Start a stroke immediately (used when entering from zoom via click).
    pub fn begin_at(&mut self, ctx: &egui::Context, p: Pos2) {
        let tool = ctx.input(tool_from_input);
        self.start_mark(tool, p);
        self.entry_stroke = true;
    }

    fn start_mark(&mut self, tool: Tool, p: Pos2) {
        let kind = match tool {
            Tool::Free => Kind::Free(vec![p]),
            t => Kind::Two {
                tool: t,
                a: p,
                b: p,
            },
        };
        self.current = Some(Mark {
            kind,
            color: self.pen_color(),
            width: self.width,
        });
    }

    fn commit_current(&mut self) {
        let entry = std::mem::take(&mut self.entry_stroke);
        if let Some(m) = self.current.take() {
            let is_dot = match &m.kind {
                Kind::Free(pts) => pts.len() <= 1,
                Kind::Two { a, b, .. } => a.distance(*b) < 1.0,
                Kind::Text { .. } => false,
            };
            if entry && is_dot {
                return;
            }
            self.push_undo();
            self.marks.push(m);
        }
    }

    fn commit_typing(&mut self) {
        if let Some(t) = self.typing.take()
            && !t.text.is_empty()
        {
            self.push_undo();
            self.marks.push(Mark {
                kind: Kind::Text {
                    pos: t.pos,
                    text: t.text,
                    size: self.font_size,
                },
                color: self.color,
                width: self.width,
            });
        }
    }

    fn set_color(&mut self, c: Color32, highlight: bool) {
        self.color = c;
        self.highlight = highlight;
    }

    fn toggle_bg(&mut self, bg: Background) {
        self.push_undo();
        self.bg = if self.bg == bg {
            Background::Screen
        } else {
            bg
        };
    }

    fn leave(&mut self) -> Transition {
        match self.back.take() {
            Some(z) => Transition::To(State::Zoom(z.resumed())),
            None => Transition::Exit,
        }
    }

    pub fn update(
        &mut self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        screen: egui::Rect,
        shared: &mut Shared,
    ) -> Transition {
        // ---- keyboard / clipboard events --------------------------------
        let events = ctx.input(|i| i.events.clone());
        let mut transition = Transition::Stay;
        // Only feed text typed after typing mode was entered (not the "t" itself).
        let typing_before = self.typing.is_some();
        for ev in &events {
            match ev {
                egui::Event::Screenshot { image, .. } => {
                    if let Some(kind) = self.pending.take() {
                        self.finish_export(kind, image, shared);
                    }
                }
                egui::Event::Text(t) if typing_before => {
                    if let Some(typing) = self.typing.as_mut() {
                        typing.text.push_str(t);
                    }
                }
                egui::Event::Copy if self.typing.is_none() => {
                    self.request_export(ctx, Export::Copy)
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(t) = self.handle_key(ctx, *key, *modifiers) {
                        transition = t;
                    }
                }
                _ => {}
            }
        }
        if !matches!(transition, Transition::Stay) {
            return transition;
        }

        // ---- wheel: pen width / font size --------------------------------
        let (wheel, ctrl_wheel) = shared.wheel_steps(ctx);
        if self.typing.is_some() && wheel != 0 {
            self.font_size = (self.font_size + wheel as f32 * 4.0).clamp(MIN_FONT, MAX_FONT);
        }
        if ctrl_wheel != 0 {
            self.width = geometry::clamp_pen_width(self.width + ctrl_wheel as f32);
            if let Some(m) = self.current.as_mut() {
                m.width = self.width;
            }
        }

        // ---- pointer ------------------------------------------------------
        let (pos, pressed, down, released, secondary, tool) = ctx.input(|i| {
            (
                i.pointer.latest_pos(),
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.secondary_clicked(),
                tool_from_input(i),
            )
        });
        if secondary && self.current.is_none() {
            if self.typing.is_some() {
                self.commit_typing();
            } else {
                return self.leave();
            }
        }
        if let Some(p) = pos {
            if pressed {
                if self.typing.is_some() {
                    // Clicking while typing moves the text cursor.
                    self.commit_typing();
                    self.typing = Some(Typing {
                        pos: p,
                        text: String::new(),
                    });
                } else if self.current.is_none() {
                    self.start_mark(tool, p);
                }
            } else if down && let Some(m) = self.current.as_mut() {
                match &mut m.kind {
                    Kind::Free(pts) => {
                        if pts.last().is_none_or(|l| l.distance(p) >= 1.0) {
                            pts.push(p);
                        }
                    }
                    Kind::Two { tool: t, b, .. } => {
                        if tool != Tool::Free {
                            *t = tool;
                        }
                        *b = p;
                    }
                    Kind::Text { .. } => {}
                }
            }
        }
        if released || (!down && self.current.is_some()) {
            self.commit_current();
        }

        // ---- paint ----------------------------------------------------------
        self.paint(ctx, painter, screen, shared);
        shared.suppress_overlay = self.pending.is_some();
        if self.pending.is_some() {
            ctx.request_repaint();
        }
        Transition::Stay
    }

    fn handle_key(
        &mut self,
        ctx: &egui::Context,
        key: egui::Key,
        mods: egui::Modifiers,
    ) -> Option<Transition> {
        use egui::Key;
        if self.typing.is_some() {
            match key {
                Key::Escape => self.commit_typing(),
                Key::Enter => {
                    if let Some(t) = self.typing.as_mut() {
                        t.text.push('\n');
                    }
                }
                Key::Backspace => {
                    if let Some(t) = self.typing.as_mut() {
                        t.text.pop();
                    }
                }
                _ => {}
            }
            return None;
        }
        let ctrl = mods.ctrl || mods.command;
        match key {
            Key::Escape => return Some(self.leave()),
            Key::Z if ctrl => {
                if let Some((marks, bg)) = self.undo.pop() {
                    self.marks = marks;
                    self.bg = bg;
                }
            }
            Key::S if ctrl => self.request_export(ctx, Export::Save),
            Key::C if ctrl => self.request_export(ctx, Export::Copy),
            _ if ctrl => {}
            Key::R => self.set_color(Color32::from_rgb(255, 0, 0), mods.shift),
            Key::G => self.set_color(Color32::from_rgb(0, 255, 0), mods.shift),
            Key::B => self.set_color(Color32::from_rgb(0, 0, 255), mods.shift),
            Key::O => self.set_color(Color32::from_rgb(255, 128, 0), mods.shift),
            Key::Y => self.set_color(Color32::from_rgb(255, 255, 0), mods.shift),
            Key::P => self.set_color(Color32::from_rgb(255, 128, 255), mods.shift),
            Key::E => {
                if !self.marks.is_empty() {
                    self.push_undo();
                    self.marks.clear();
                }
            }
            Key::W => self.toggle_bg(Background::White),
            Key::K => self.toggle_bg(Background::Black),
            Key::T => {
                let pos = ctx
                    .input(|i| i.pointer.latest_pos())
                    .unwrap_or(ctx.screen_rect().center());
                self.current = None;
                self.typing = Some(Typing {
                    pos,
                    text: String::new(),
                });
            }
            _ => {}
        }
        None
    }

    fn request_export(&mut self, ctx: &egui::Context, kind: Export) {
        if self.pending.is_none() {
            self.pending = Some(kind);
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
    }

    fn finish_export(&mut self, kind: Export, image: &egui::ColorImage, shared: &mut Shared) {
        let Some(img) = color_image_to_rgba(image) else {
            shared.toast("Failed to read the rendered image");
            return;
        };
        let msg = match kind {
            Export::Copy => match output::copy_png_to_clipboard(&img) {
                Ok(()) => "Copied to clipboard".to_string(),
                Err(e) => format!("Copy failed: {e:#}"),
            },
            Export::Save => match output::save_png(&img, &shared.cfg.screenshot_dir()) {
                Ok(p) => format!("Saved {}", p.display()),
                Err(e) => format!("Save failed: {e:#}"),
            },
        };
        shared.toast(msg);
    }

    fn paint(
        &self,
        ctx: &egui::Context,
        painter: &egui::Painter,
        screen: egui::Rect,
        shared: &mut Shared,
    ) {
        match self.bg {
            Background::White => {
                painter.rect_filled(screen, 0.0, Color32::WHITE);
            }
            Background::Black => {
                painter.rect_filled(screen, 0.0, Color32::BLACK);
            }
            Background::Screen => {
                if let Some(tex) = shared.texture(ctx) {
                    let view = self.view.unwrap_or(super::to_rect(screen));
                    painter.image(tex.id(), screen, uv_of(view, screen), Color32::WHITE);
                }
            }
        }
        for m in self.marks.iter().chain(self.current.as_ref()) {
            paint_mark(painter, m);
        }

        let show_overlay = self.pending.is_none();
        if let Some(t) = &self.typing {
            let font = egui::FontId::proportional(self.font_size);
            let galley = painter.layout_no_wrap(t.text.clone(), font, self.color);
            let caret_x = t.pos.x + galley.rows.last().map(|r| r.rect.width()).unwrap_or(0.0);
            let caret_y = t.pos.y + galley.size().y.max(self.font_size) - self.font_size;
            painter.galley(t.pos, galley, self.color);
            if show_overlay {
                painter.line_segment(
                    [
                        egui::pos2(caret_x + 2.0, caret_y),
                        egui::pos2(caret_x + 2.0, caret_y + self.font_size),
                    ],
                    egui::Stroke::new(2.0, self.color),
                );
            }
        }

        if show_overlay {
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            if self.typing.is_none()
                && let Some(p) = ctx.input(|i| i.pointer.latest_pos())
            {
                painter.circle_stroke(
                    p,
                    self.width / 2.0,
                    egui::Stroke::new(1.0, self.pen_color()),
                );
            }
            let help = if self.typing.is_some() {
                "Typing · wheel font size · click to move · Esc done"
            } else {
                "Draw · R G B O Y P color (Shift = highlighter) · Ctrl+wheel width · Shift line · \
                 Ctrl rect · Tab ellipse · Ctrl+Shift arrow · T text · E erase · W/K board · \
                 Ctrl+Z undo · Ctrl+C copy · Ctrl+S save · Esc exit"
            };
            shared.hint(painter, screen, help);
        }
    }
}

fn tool_from_input(i: &egui::InputState) -> Tool {
    let m = i.modifiers;
    let ctrl = m.ctrl || m.command;
    if ctrl && m.shift {
        Tool::Arrow
    } else if ctrl {
        Tool::Rect
    } else if m.shift {
        Tool::Line
    } else if i.key_down(egui::Key::Tab) {
        Tool::Ellipse
    } else {
        Tool::Free
    }
}

fn to_point(p: Pos2) -> Point {
    Point::new(p.x, p.y)
}

fn to_pos(p: Point) -> Pos2 {
    egui::pos2(p.x, p.y)
}

fn paint_mark(painter: &egui::Painter, m: &Mark) {
    let stroke = egui::Stroke::new(m.width, m.color);
    let opaque = m.color.a() == 255;
    let round_joints = |pts: &[Pos2]| {
        if opaque && m.width > 2.0 {
            for p in pts {
                painter.circle_filled(*p, m.width / 2.0, m.color);
            }
        }
    };
    match &m.kind {
        Kind::Free(pts) => {
            if pts.len() == 1 {
                painter.circle_filled(pts[0], m.width / 2.0, m.color);
            } else {
                painter.add(egui::Shape::line(pts.clone(), stroke));
                round_joints(pts);
            }
        }
        Kind::Two { tool, a, b } => {
            let r = geometry::normalize_rect(to_point(*a), to_point(*b));
            match tool {
                Tool::Line | Tool::Free => {
                    painter.line_segment([*a, *b], stroke);
                    round_joints(&[*a, *b]);
                }
                Tool::Rect => {
                    let pts: Vec<Pos2> =
                        geometry::rect_points(&r).into_iter().map(to_pos).collect();
                    painter.add(egui::Shape::line(pts.clone(), stroke));
                    round_joints(&pts);
                }
                Tool::Ellipse => {
                    let pts = geometry::ellipse_points(&r, 96)
                        .into_iter()
                        .map(to_pos)
                        .collect();
                    painter.add(egui::Shape::line(pts, stroke));
                }
                Tool::Arrow => {
                    // Upstream draws the head at the drag start point.
                    let (tail, tip) = (*b, *a);
                    let (l, rw) = geometry::arrowhead(to_point(tail), to_point(tip), m.width);
                    painter.line_segment([tail, tip], stroke);
                    painter.add(egui::Shape::convex_polygon(
                        vec![tip, to_pos(l), to_pos(rw)],
                        m.color,
                        egui::Stroke::NONE,
                    ));
                    painter.add(egui::Shape::line(vec![to_pos(l), tip, to_pos(rw)], stroke));
                    round_joints(&[tail, tip, to_pos(l), to_pos(rw)]);
                }
            }
        }
        Kind::Text { pos, text, size } => {
            painter.text(
                *pos,
                egui::Align2::LEFT_TOP,
                text,
                egui::FontId::proportional(*size),
                m.color,
            );
        }
    }
}
