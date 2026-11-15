//! egui window: browse templates and create one into a directory.

use egui::RichText;
use mpt_common::theme::Theme;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::NewPlusConfig;
use crate::templates::{Template, create_from_template, list_templates};

const THEME: Theme = Theme::dark();

struct NewPlusApp {
    config: NewPlusConfig,
    templates: Vec<Template>,
    selected: Option<usize>,
    target_dir: String,
    status: Option<(bool, String)>,
}

impl NewPlusApp {
    fn new(config: NewPlusConfig, start_dir: PathBuf) -> Self {
        let mut app = Self {
            config,
            templates: Vec::new(),
            selected: None,
            target_dir: start_dir.to_string_lossy().into_owned(),
            status: None,
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        self.config = NewPlusConfig::load();
        match list_templates(&self.config.templates_dir()) {
            Ok(list) => self.templates = list,
            Err(e) => {
                self.templates.clear();
                self.status = Some((false, format!("{e:#}")));
            }
        }
        if self.selected.is_some_and(|i| i >= self.templates.len()) {
            self.selected = None;
        }
        if self.selected.is_none() && !self.templates.is_empty() {
            self.selected = Some(0);
        }
    }

    fn create(&mut self) {
        let Some(t) = self.selected.and_then(|i| self.templates.get(i)) else {
            return;
        };
        let target = PathBuf::from(self.target_dir.trim());
        self.status = Some(match create_from_template(&t.path, &target, &self.config) {
            Ok(p) => (true, format!("Created {}", p.display())),
            Err(e) => (false, format!("{e:#}")),
        });
    }

    fn open_templates_folder(&mut self) {
        let dir = self.config.templates_dir();
        if let Err(e) = std::process::Command::new("xdg-open").arg(&dir).spawn() {
            warn!("New+: xdg-open failed: {e}");
            self.status = Some((false, format!("xdg-open failed: {e}")));
        }
    }
}

impl eframe::App for NewPlusApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::TopBottomPanel::bottom("actions").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Create in:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.target_dir)
                        .desired_width(ui.available_width() - 90.0),
                );
                if ui.button("Browse…").clicked()
                    && let Some(dir) = rfd::FileDialog::new()
                        .set_title("Select target folder")
                        .set_directory(self.target_dir.trim())
                        .pick_folder()
                {
                    self.target_dir = dir.to_string_lossy().into_owned();
                }
            });
            ui.horizontal(|ui| {
                let can_create = self.selected.is_some() && !self.target_dir.trim().is_empty();
                if ui
                    .add_enabled(can_create, egui::Button::new("Create"))
                    .clicked()
                    || (can_create && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    self.create();
                }
                if ui.button("Open templates folder").clicked() {
                    self.open_templates_folder();
                }
                if ui.button("Refresh").clicked() {
                    self.reload();
                }
            });
            if let Some((ok, msg)) = &self.status {
                let color = if *ok {
                    THEME.text_success
                } else {
                    THEME.text_error
                };
                ui.label(RichText::new(msg).color(color));
            }
            ui.add_space(6.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("New+");
            ui.label(
                RichText::new(self.config.templates_dir().to_string_lossy())
                    .color(THEME.text_muted)
                    .small(),
            );
            ui.separator();
            if self.templates.is_empty() {
                ui.label("No templates. Add files or folders to the templates folder.");
            }
            let mut create_now = false;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (i, t) in self.templates.iter().enumerate() {
                    let icon = if t.is_dir { "📁" } else { "📄" };
                    let label = format!("{icon}  {}", t.display_name(&self.config));
                    let resp = ui
                        .selectable_label(self.selected == Some(i), label)
                        .on_hover_text(&t.file_name);
                    if resp.clicked() {
                        self.selected = Some(i);
                    }
                    if resp.double_clicked() {
                        self.selected = Some(i);
                        create_now = true;
                    }
                }
            });
            if create_now {
                self.create();
            }
        });
    }
}

pub fn run_window(config: NewPlusConfig, start_dir: PathBuf) {
    info!("Opening New+ window");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 440.0])
            .with_min_inner_size([360.0, 260.0])
            .with_title("New+")
            .with_resizable(true),
        ..Default::default()
    };
    eframe::run_native(
        "New+",
        options,
        Box::new(|cc| {
            THEME.apply(&cc.egui_ctx);
            Ok(Box::new(NewPlusApp::new(config, start_dir)))
        }),
    )
    .ok();
}
