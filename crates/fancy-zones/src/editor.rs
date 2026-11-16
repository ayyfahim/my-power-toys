use crate::config::FancyZonesConfig;
use crate::layout::{Layout, TemplateKind, Zone};
use eframe::egui;
use mpt_common::monitor::Monitor;
use mpt_common::theme::{self, Theme};

const WINDOW_WIDTH: f32 = 900.0;
const WINDOW_HEIGHT: f32 = 600.0;
const MONITOR_BAR_HEIGHT: f32 = 80.0;
const TEMPLATE_CARD_SIZE: f32 = 120.0;
const TEMPLATE_CARD_PADDING: f32 = 12.0;
const ZONE_PREVIEW_GAP: f32 = 2.0;
const DRAFT_PREVIEW_W: f32 = 240.0;
const DRAFT_PREVIEW_H: f32 = 135.0;

/// Kind of custom layout being created in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DraftKind {
    Columns,
    Rows,
    Grid,
    Canvas,
}

/// The custom layout form state.
#[derive(Debug, Clone)]
struct Draft {
    name: String,
    kind: DraftKind,
    /// Columns (Columns/Grid) or rows (Rows).
    count: usize,
    /// Grid rows.
    rows: usize,
    /// Canvas zones in percent (0-100) for easier editing.
    canvas: Vec<[f32; 4]>,
    /// 0 = none, 1-9 = Super+Ctrl+Alt+<key>.
    quick_key: u8,
    /// Index in `config.layouts` being edited, if any.
    editing: Option<usize>,
}

impl Default for Draft {
    fn default() -> Self {
        Self {
            name: "Custom layout".to_string(),
            kind: DraftKind::Columns,
            count: 2,
            rows: 2,
            canvas: vec![[0.0, 0.0, 50.0, 100.0], [50.0, 0.0, 50.0, 100.0]],
            quick_key: 0,
            editing: None,
        }
    }
}

impl Draft {
    /// Build the layout described by the form.
    fn build(&self) -> Layout {
        let name = if self.name.trim().is_empty() {
            "Custom layout"
        } else {
            self.name.trim()
        };
        let mut layout = match self.kind {
            DraftKind::Columns => {
                Layout::custom_from_kind(name, TemplateKind::Columns, self.count, 1)
            }
            DraftKind::Rows => Layout::custom_from_kind(name, TemplateKind::Rows, self.count, 1),
            DraftKind::Grid => {
                Layout::custom_from_kind(name, TemplateKind::Grid, self.count, self.rows)
            }
            DraftKind::Canvas => Layout::canvas(
                name,
                self.canvas
                    .iter()
                    .map(|r| Zone::new(r[0] / 100.0, r[1] / 100.0, r[2] / 100.0, r[3] / 100.0))
                    .collect(),
            ),
        };
        layout.quick_key = (1..=9).contains(&self.quick_key).then_some(self.quick_key);
        layout
    }

    /// Load an existing layout into the form (as a canvas unless it is a
    /// regular grid/columns/rows kind).
    fn from_layout(layout: &Layout, editing: Option<usize>) -> Self {
        let canvas = layout
            .zones
            .iter()
            .map(|z| [z.x * 100.0, z.y * 100.0, z.width * 100.0, z.height * 100.0])
            .collect();
        let n = layout.zones.len().max(1);
        let (kind, count, rows) = match layout.kind {
            TemplateKind::Columns => (DraftKind::Columns, n, 2),
            TemplateKind::Rows => (DraftKind::Rows, n, 2),
            TemplateKind::Grid => {
                // Count distinct column origins to recover cols x rows.
                let mut xs: Vec<i32> = layout
                    .zones
                    .iter()
                    .map(|z| (z.x * 1000.0).round() as i32)
                    .collect();
                xs.sort_unstable();
                xs.dedup();
                let cols = xs.len().max(1);
                (DraftKind::Grid, cols, (n / cols).max(1))
            }
            _ => (DraftKind::Canvas, 2, 2),
        };
        Self {
            name: layout.name.clone(),
            kind,
            count,
            rows,
            canvas,
            quick_key: layout.quick_key.unwrap_or(0),
            editing,
        }
    }
}

struct EditorApp {
    monitors: Vec<Monitor>,
    selected_monitor: usize,
    config: FancyZonesConfig,
    templates: Vec<Layout>,
    draft: Draft,
    theme: Theme,
    theme_applied: bool,
}

impl EditorApp {
    fn new(monitors: Vec<Monitor>, mut config: FancyZonesConfig) -> Self {
        let templates = Layout::all_templates();
        // Custom layouts live after the built-in templates in `config.layouts`.
        if config.layouts.len() < templates.len() {
            config.layouts = templates.clone();
        }
        Self {
            monitors,
            selected_monitor: 0,
            config,
            templates,
            draft: Draft::default(),
            theme: Theme::dark(),
            theme_applied: false,
        }
    }

    fn selected_monitor_name(&self) -> &str {
        self.monitors
            .get(self.selected_monitor)
            .map(|m| m.name.as_str())
            .unwrap_or("unknown")
    }

    fn selected_template_idx(&self) -> usize {
        let name = self.selected_monitor_name();
        self.config
            .monitor_layouts
            .get(name)
            .copied()
            .unwrap_or(self.config.default_layout)
    }

    fn save_config(&self) {
        if let Err(e) = mpt_common::config::save_module_config("fancy-zones", &self.config) {
            tracing::warn!("Failed to save FancyZones config: {e}");
        }
    }
}

impl eframe::App for EditorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.theme_applied {
            self.theme.apply(ctx);
            self.theme_applied = true;
        }

        // Copy theme (it's Copy) to avoid borrow issues with the closure
        let t = self.theme;

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);

                // Title
                ui.label(
                    egui::RichText::new("FancyZones Editor")
                        .size(theme::FONT_TITLE)
                        .strong()
                        .color(t.text_primary),
                );
                ui.add_space(4.0);

                // Monitor bar
                ui.label(
                    egui::RichText::new("Select a monitor to configure its layout:")
                        .size(theme::FONT_BODY)
                        .color(t.text_secondary),
                );
                ui.add_space(4.0);
                self.draw_monitor_bar(ui);

                ui.add_space(theme::SECTION_SPACING);
                ui.separator();
                ui.add_space(8.0);

                // Templates section
                ui.label(
                    egui::RichText::new("Templates")
                        .size(theme::FONT_TITLE)
                        .strong()
                        .color(t.text_primary),
                );
                ui.add_space(4.0);
                self.draw_templates(ui);

                ui.add_space(theme::SECTION_SPACING);
                ui.separator();
                ui.add_space(8.0);

                // Custom section
                ui.label(
                    egui::RichText::new("Custom")
                        .size(theme::FONT_TITLE)
                        .strong()
                        .color(t.text_primary),
                );
                ui.add_space(4.0);
                self.draw_custom_layouts(ui);
                ui.add_space(8.0);
                self.draw_custom_form(ui);
            });
        });
    }
}

impl EditorApp {
    fn draw_monitor_bar(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;

        ui.horizontal(|ui| {
            for (i, monitor) in self.monitors.iter().enumerate() {
                let is_selected = i == self.selected_monitor;

                let desired = egui::vec2(140.0, MONITOR_BAR_HEIGHT);
                let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

                if response.clicked() {
                    self.selected_monitor = i;
                }

                let fill = if is_selected {
                    t.bg_card
                } else {
                    t.bg_secondary
                };
                let stroke = if is_selected {
                    egui::Stroke::new(2.0, t.text_accent)
                } else {
                    egui::Stroke::new(1.0, t.card_border)
                };

                ui.painter().rect(
                    rect,
                    theme::CARD_RADIUS,
                    fill,
                    stroke,
                    egui::StrokeKind::Outside,
                );

                // Monitor number badge
                let badge_text = format!("{}", i + 1);
                let badge_galley = ui.painter().layout_no_wrap(
                    badge_text,
                    egui::FontId::proportional(20.0),
                    t.text_primary,
                );
                ui.painter().galley(
                    rect.center_top() + egui::vec2(-badge_galley.size().x / 2.0, 8.0),
                    badge_galley,
                    t.text_primary,
                );

                // Resolution text
                let res_text = format!("{}x{}", monitor.width, monitor.height);
                let res_galley = ui.painter().layout_no_wrap(
                    res_text,
                    egui::FontId::proportional(theme::FONT_SMALL),
                    t.text_secondary,
                );
                ui.painter().galley(
                    rect.center_bottom() + egui::vec2(-res_galley.size().x / 2.0, -20.0),
                    res_galley,
                    t.text_secondary,
                );
            }
        });
    }

    fn draw_templates(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        let current_idx = self.selected_template_idx();

        ui.horizontal_wrapped(|ui| {
            for (i, template) in self.templates.iter().enumerate() {
                let is_selected = i == current_idx;
                let desired = egui::vec2(TEMPLATE_CARD_SIZE, TEMPLATE_CARD_SIZE + 24.0);
                let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

                if response.clicked() {
                    let mon_name = self.selected_monitor_name().to_string();
                    self.config.set_monitor_layout(&mon_name, i);
                    self.save_config();
                }

                let fill = if is_selected {
                    t.bg_card
                } else {
                    t.bg_secondary
                };
                let stroke = if is_selected {
                    egui::Stroke::new(2.0, t.text_accent)
                } else {
                    egui::Stroke::new(1.0, t.card_border)
                };

                ui.painter().rect(
                    rect,
                    theme::CARD_RADIUS,
                    fill,
                    stroke,
                    egui::StrokeKind::Outside,
                );

                // Zone preview area (top portion of card)
                let preview_rect = egui::Rect::from_min_size(
                    rect.min + egui::vec2(TEMPLATE_CARD_PADDING, TEMPLATE_CARD_PADDING),
                    egui::vec2(
                        TEMPLATE_CARD_SIZE - 2.0 * TEMPLATE_CARD_PADDING,
                        TEMPLATE_CARD_SIZE - 2.0 * TEMPLATE_CARD_PADDING - 10.0,
                    ),
                );

                draw_zone_preview(ui, preview_rect, template, &t);

                // Template name
                let name_galley = ui.painter().layout_no_wrap(
                    template.name.clone(),
                    egui::FontId::proportional(theme::FONT_SMALL),
                    t.text_secondary,
                );
                let name_pos = egui::pos2(
                    rect.center().x - name_galley.size().x / 2.0,
                    rect.max.y - 18.0,
                );
                ui.painter().galley(name_pos, name_galley, t.text_secondary);
            }
        });
    }
}

impl EditorApp {
    fn draw_custom_layouts(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        let first = self.templates.len();
        if self.config.layouts.len() <= first {
            ui.label(
                egui::RichText::new("No custom layouts yet. Create one below.")
                    .size(theme::FONT_BODY)
                    .color(t.text_muted),
            );
            return;
        }
        let current_idx = self.selected_template_idx();
        let mut assign = None;
        let mut edit = None;
        let mut delete = None;
        ui.horizontal_wrapped(|ui| {
            for i in first..self.config.layouts.len() {
                let layout = &self.config.layouts[i];
                ui.vertical(|ui| {
                    let desired = egui::vec2(TEMPLATE_CARD_SIZE, TEMPLATE_CARD_SIZE + 24.0);
                    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());
                    if response.clicked() {
                        assign = Some(i);
                    }
                    let selected = i == current_idx;
                    ui.painter().rect(
                        rect,
                        theme::CARD_RADIUS,
                        if selected { t.bg_card } else { t.bg_secondary },
                        if selected {
                            egui::Stroke::new(2.0, t.text_accent)
                        } else {
                            egui::Stroke::new(1.0, t.card_border)
                        },
                        egui::StrokeKind::Outside,
                    );
                    let preview_rect = egui::Rect::from_min_size(
                        rect.min + egui::vec2(TEMPLATE_CARD_PADDING, TEMPLATE_CARD_PADDING),
                        egui::vec2(
                            TEMPLATE_CARD_SIZE - 2.0 * TEMPLATE_CARD_PADDING,
                            TEMPLATE_CARD_SIZE - 2.0 * TEMPLATE_CARD_PADDING - 10.0,
                        ),
                    );
                    draw_zone_preview(ui, preview_rect, layout, &t);
                    let label = match layout.quick_key {
                        Some(k) => format!("{} [{k}]", layout.name),
                        None => layout.name.clone(),
                    };
                    let name_galley = ui.painter().layout_no_wrap(
                        label,
                        egui::FontId::proportional(theme::FONT_SMALL),
                        t.text_secondary,
                    );
                    ui.painter().galley(
                        egui::pos2(
                            rect.center().x - name_galley.size().x / 2.0,
                            rect.max.y - 18.0,
                        ),
                        name_galley,
                        t.text_secondary,
                    );
                    ui.horizontal(|ui| {
                        if ui.small_button("Edit").clicked() {
                            edit = Some(i);
                        }
                        if ui.small_button("Delete").clicked() {
                            delete = Some(i);
                        }
                    });
                });
            }
        });
        if let Some(i) = assign {
            let mon = self.selected_monitor_name().to_string();
            self.config.set_monitor_layout(&mon, i);
            self.save_config();
        }
        if let Some(i) = edit {
            self.draft = Draft::from_layout(&self.config.layouts[i], Some(i));
        }
        if let Some(i) = delete {
            self.config.remove_layout(i);
            if self.draft.editing == Some(i) {
                self.draft.editing = None;
            }
            self.save_config();
        }
    }

    fn draw_custom_form(&mut self, ui: &mut egui::Ui) {
        let t = self.theme;
        let heading = if self.draft.editing.is_some() {
            "Edit layout"
        } else {
            "New layout"
        };
        ui.label(
            egui::RichText::new(heading)
                .size(theme::FONT_BODY)
                .strong()
                .color(t.text_primary),
        );
        ui.horizontal(|ui| {
            ui.label("Name:");
            ui.text_edit_singleline(&mut self.draft.name);
            ui.label("Quick key:");
            egui::ComboBox::from_id_salt("fz_quick_key")
                .selected_text(match self.draft.quick_key {
                    0 => "None".to_string(),
                    k => format!("Super+Ctrl+Alt+{k}"),
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.draft.quick_key, 0, "None");
                    for k in 1..=9u8 {
                        ui.selectable_value(&mut self.draft.quick_key, k, format!("{k}"));
                    }
                });
        });
        ui.horizontal(|ui| {
            for (kind, label) in [
                (DraftKind::Columns, "Columns"),
                (DraftKind::Rows, "Rows"),
                (DraftKind::Grid, "Grid"),
                (DraftKind::Canvas, "Canvas"),
            ] {
                ui.radio_value(&mut self.draft.kind, kind, label);
            }
        });
        match self.draft.kind {
            DraftKind::Columns | DraftKind::Rows => {
                ui.horizontal(|ui| {
                    ui.label("Zones:");
                    ui.add(egui::DragValue::new(&mut self.draft.count).range(1..=16));
                });
            }
            DraftKind::Grid => {
                ui.horizontal(|ui| {
                    ui.label("Columns:");
                    ui.add(egui::DragValue::new(&mut self.draft.count).range(1..=16));
                    ui.label("Rows:");
                    ui.add(egui::DragValue::new(&mut self.draft.rows).range(1..=16));
                });
            }
            DraftKind::Canvas => self.draw_canvas_table(ui),
        }

        ui.add_space(4.0);
        let layout = self.draft.build();
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(DRAFT_PREVIEW_W, DRAFT_PREVIEW_H),
            egui::Sense::hover(),
        );
        ui.painter().rect(
            rect,
            theme::CARD_RADIUS,
            t.bg_secondary,
            egui::Stroke::new(1.0, t.card_border),
            egui::StrokeKind::Outside,
        );
        draw_zone_preview(ui, rect.shrink(4.0), &layout, &t);

        ui.horizontal(|ui| {
            let mon = self.selected_monitor_name().to_string();
            if let Some(i) = self.draft.editing {
                if ui.button("Update layout").clicked() && i < self.config.layouts.len() {
                    self.clear_quick_key_conflicts(layout.quick_key, Some(i));
                    self.config.layouts[i] = layout.clone();
                    self.save_config();
                }
                if ui.button("Cancel edit").clicked() {
                    self.draft.editing = None;
                }
            } else if ui.button("Create and apply to monitor").clicked() {
                self.clear_quick_key_conflicts(layout.quick_key, None);
                self.config.layouts.push(layout.clone());
                let idx = self.config.layouts.len() - 1;
                self.config.set_monitor_layout(&mon, idx);
                self.draft.editing = Some(idx);
                self.save_config();
            }
            if ui.button("Copy current monitor layout").clicked() {
                let idx = self.selected_template_idx();
                if let Some(l) = self.config.layouts.get(idx) {
                    let mut d = Draft::from_layout(l, None);
                    d.name = format!("{} (copy)", l.name);
                    d.kind = DraftKind::Canvas;
                    d.quick_key = 0;
                    self.draft = d;
                }
            }
        });
    }

    fn draw_canvas_table(&mut self, ui: &mut egui::Ui) {
        ui.label("Zones (percent of the monitor): x, y, width, height");
        let mut remove = None;
        egui::Grid::new("fz_canvas_zones")
            .num_columns(6)
            .striped(true)
            .show(ui, |ui| {
                for (i, r) in self.draft.canvas.iter_mut().enumerate() {
                    ui.label(format!("{}", i + 1));
                    for v in r.iter_mut() {
                        ui.add(
                            egui::DragValue::new(v)
                                .range(0.0..=100.0)
                                .speed(0.5)
                                .suffix("%"),
                        );
                    }
                    if ui.small_button("Remove").clicked() {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
        if let Some(i) = remove {
            self.draft.canvas.remove(i);
        }
        if ui.button("Add zone").clicked() {
            self.draft.canvas.push([25.0, 25.0, 50.0, 50.0]);
        }
    }

    /// Quick keys are unique: clear the key from any other layout using it.
    fn clear_quick_key_conflicts(&mut self, key: Option<u8>, except: Option<usize>) {
        let Some(key) = key else { return };
        for (i, l) in self.config.layouts.iter_mut().enumerate() {
            if Some(i) != except && l.quick_key == Some(key) {
                l.quick_key = None;
            }
        }
    }
}

fn draw_zone_preview(ui: &mut egui::Ui, preview_rect: egui::Rect, layout: &Layout, t: &Theme) {
    if layout.zones.is_empty() {
        let painter = ui.painter();
        painter.rect_stroke(
            preview_rect,
            2.0,
            egui::Stroke::new(1.0, t.separator),
            egui::StrokeKind::Inside,
        );
        painter.line_segment(
            [preview_rect.left_top(), preview_rect.right_bottom()],
            egui::Stroke::new(1.0, t.separator),
        );
        return;
    }

    let pw = preview_rect.width();
    let ph = preview_rect.height();
    let painter = ui.painter();

    for zone in &layout.zones {
        let zr = egui::Rect::from_min_size(
            preview_rect.min
                + egui::vec2(
                    zone.x * pw + ZONE_PREVIEW_GAP,
                    zone.y * ph + ZONE_PREVIEW_GAP,
                ),
            egui::vec2(
                zone.width * pw - 2.0 * ZONE_PREVIEW_GAP,
                zone.height * ph - 2.0 * ZONE_PREVIEW_GAP,
            ),
        );
        painter.rect_filled(zr, 2.0, t.bg_hover);
    }
}

/// Run the FancyZones editor GUI.
pub fn run_editor(monitors: Vec<Monitor>, config: FancyZonesConfig) {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(egui::vec2(WINDOW_WIDTH, WINDOW_HEIGHT))
            .with_min_inner_size(egui::vec2(600.0, 400.0))
            .with_title("FancyZones Editor"),
        ..Default::default()
    };

    let _ = eframe::run_native(
        "FancyZones Editor",
        native_options,
        Box::new(move |_cc| Ok(Box::new(EditorApp::new(monitors, config)))),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_builds_layouts() {
        let mut d = Draft {
            kind: DraftKind::Grid,
            count: 3,
            rows: 2,
            quick_key: 4,
            ..Draft::default()
        };
        let l = d.build();
        assert_eq!(l.zones.len(), 6);
        assert_eq!(l.quick_key, Some(4));
        d.kind = DraftKind::Canvas;
        let l = d.build();
        assert_eq!(l.zones.len(), 2);
        assert!((l.zones[1].x - 0.5).abs() < 1e-6);
    }

    #[test]
    fn draft_roundtrips_grid() {
        let d = Draft::from_layout(&Layout::grid(3, 2), Some(7));
        assert_eq!(d.kind, DraftKind::Grid);
        assert_eq!((d.count, d.rows), (3, 2));
        assert_eq!(d.editing, Some(7));
    }
}
