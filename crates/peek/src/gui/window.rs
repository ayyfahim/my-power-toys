use egui::{self, RichText, ScrollArea};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use crate::PeekConfig;
use crate::gui::highlight::{self, HlSpan};
use crate::gui::markdown::{self, Block};
use crate::gui::theme;
use crate::preview::{self, FileKind, FilePreview, TextFlavor};

/// Cached rendering of a text preview.
enum TextRender {
    Plain,
    Highlighted(Vec<HlSpan>),
    Markdown(Vec<Block>),
}

/// Background thumbnail rendering (PDF first page / video frame).
enum Thumb {
    NotStarted,
    Pending(mpsc::Receiver<Option<PathBuf>>),
    Ready(egui::TextureHandle),
    Unavailable,
}

pub struct PeekApp {
    preview: FilePreview,
    config: PeekConfig,
    sibling_files: Vec<PathBuf>,
    current_index: usize,
    should_close: bool,
    had_focus: bool,
    image_texture: Option<egui::TextureHandle>,
    image_load_attempted: bool,
    text_render: Option<TextRender>,
    thumb: Thumb,
    confirm_delete: bool,
    status: Option<String>,
}

impl PeekApp {
    pub fn new(preview: FilePreview, config: PeekConfig) -> Self {
        let sibling_files = list_sibling_files(&preview.path);
        let current_index = sibling_files
            .iter()
            .position(|p| p == &preview.path)
            .unwrap_or(0);

        Self {
            preview,
            config,
            sibling_files,
            current_index,
            should_close: false,
            had_focus: false,
            image_texture: None,
            image_load_attempted: false,
            text_render: None,
            thumb: Thumb::NotStarted,
            confirm_delete: false,
            status: None,
        }
    }

    fn reset_render_state(&mut self) {
        self.image_texture = None;
        self.image_load_attempted = false;
        self.text_render = None;
        self.thumb = Thumb::NotStarted;
        self.confirm_delete = false;
    }

    /// Delete key: move the current file to trash (upstream `ConfirmFileDelete`).
    fn delete_current(&mut self) {
        let path = self.preview.path.clone();
        match preview::move_to_trash(&path) {
            Ok(()) => {
                tracing::info!("Moved {} to trash", path.display());
                self.status = None;
                if let Some(pos) = self.sibling_files.iter().position(|p| p == &path) {
                    self.sibling_files.remove(pos);
                }
                if self.sibling_files.is_empty() {
                    self.should_close = true;
                    return;
                }
                if self.current_index >= self.sibling_files.len() {
                    self.current_index = 0;
                }
                self.navigate(0);
            }
            Err(e) => {
                tracing::warn!("Delete failed: {e:#}");
                self.status = Some(format!("Delete failed: {e}"));
            }
        }
        self.confirm_delete = false;
    }

    fn load_texture_from(
        ctx: &egui::Context,
        path: &Path,
        name: &str,
    ) -> Option<egui::TextureHandle> {
        let img = image::open(path)
            .map_err(|e| tracing::warn!("Failed to load image {}: {e}", path.display()))
            .ok()?;
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        Some(ctx.load_texture(name, color_image, egui::TextureOptions::LINEAR))
    }

    /// Start / poll background thumbnail generation for PDFs and videos.
    fn poll_thumbnail(&mut self, ctx: &egui::Context) {
        match &self.thumb {
            Thumb::NotStarted => {
                if !matches!(self.preview.kind, FileKind::Pdf | FileKind::Video) {
                    self.thumb = Thumb::Unavailable;
                    return;
                }
                let (tx, rx) = mpsc::channel();
                let path = self.preview.path.clone();
                let kind = self.preview.kind;
                let ctx2 = ctx.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(preview::render_thumbnail(&path, kind, 1024));
                    ctx2.request_repaint();
                });
                self.thumb = Thumb::Pending(rx);
            }
            Thumb::Pending(rx) => match rx.try_recv() {
                Ok(Some(png)) => {
                    self.thumb = match Self::load_texture_from(ctx, &png, "peek-thumb") {
                        Some(t) => Thumb::Ready(t),
                        None => Thumb::Unavailable,
                    };
                }
                Ok(None) | Err(mpsc::TryRecvError::Disconnected) => self.thumb = Thumb::Unavailable,
                Err(mpsc::TryRecvError::Empty) => {}
            },
            Thumb::Ready(_) | Thumb::Unavailable => {}
        }
    }

    fn ensure_text_render(&mut self) {
        if self.text_render.is_some() {
            return;
        }
        let text = self.preview.preview_text.as_deref().unwrap_or("");
        let render = match preview::text_flavor(&self.preview.path) {
            TextFlavor::Markdown if self.config.render_markdown => {
                TextRender::Markdown(markdown::parse(text))
            }
            TextFlavor::Plain => TextRender::Plain,
            _ if self.config.syntax_highlighting => {
                match highlight::highlight(&self.preview.path, text) {
                    Some(spans) => TextRender::Highlighted(spans),
                    None => TextRender::Plain,
                }
            }
            _ => TextRender::Plain,
        };
        self.text_render = Some(render);
    }

    fn navigate(&mut self, delta: i32) {
        if self.sibling_files.is_empty() {
            return;
        }
        let len = self.sibling_files.len() as i32;
        let new_index = ((self.current_index as i32 + delta) % len + len) % len;
        self.current_index = new_index as usize;

        let new_path = &self.sibling_files[self.current_index];
        match preview::generate_preview(
            new_path,
            self.config.max_preview_lines,
            self.config.max_dir_entries,
        ) {
            Ok(new_preview) => {
                self.preview = new_preview;
                self.reset_render_state();
            }
            Err(e) => {
                tracing::warn!("Failed to generate preview for {}: {e}", new_path.display());
            }
        }
    }

    fn open_with_default(&self) {
        let _ = std::process::Command::new("xdg-open")
            .arg(&self.preview.path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    }

    fn try_load_image(&mut self, ctx: &egui::Context) {
        if self.image_load_attempted {
            return;
        }
        self.image_load_attempted = true;

        if self.preview.kind != FileKind::Image {
            return;
        }

        let path = &self.preview.path;
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        // SVG: skip raster loading
        if ext == "svg" {
            return;
        }

        self.image_texture = Self::load_texture_from(ctx, path, "peek-image");
    }

    fn draw_header(&self, ui: &mut egui::Ui) {
        let frame = egui::Frame::NONE
            .fill(theme::BG_HEADER)
            .inner_margin(egui::Margin::symmetric(theme::INNER_PADDING as i8, 10));

        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                // File icon based on kind
                let icon = match self.preview.kind {
                    FileKind::Text => "\u{1F4C4}",      // document
                    FileKind::Image => "\u{1F5BC}",     // framed picture
                    FileKind::Pdf => "\u{1F4D1}",       // bookmark tabs
                    FileKind::Audio => "\u{1F3B5}",     // musical note
                    FileKind::Video => "\u{1F3AC}",     // clapper board
                    FileKind::Directory => "\u{1F4C1}", // folder
                    FileKind::Archive => "\u{1F4E6}",   // package
                    FileKind::Unknown => "\u{1F4CE}",   // paperclip
                };
                ui.label(RichText::new(icon).size(20.0));
                ui.add_space(6.0);

                ui.vertical(|ui| {
                    // Filename
                    let filename = self
                        .preview
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy();
                    ui.label(
                        RichText::new(filename.as_ref())
                            .size(theme::FONT_FILENAME)
                            .color(theme::TEXT_PRIMARY)
                            .strong(),
                    );

                    // Metadata line
                    let mut meta_parts = vec![
                        preview::format_size(self.preview.size_bytes),
                        self.preview.kind.to_string(),
                    ];
                    if let Some(ref mime) = self.preview.mime_type {
                        meta_parts.push(mime.clone());
                    }
                    if let Some((w, h)) = self.preview.dimensions {
                        meta_parts.push(format!("{w}\u{00D7}{h}"));
                    }
                    if let Some(ref dur) = self.preview.duration {
                        meta_parts.push(dur.clone());
                    }

                    ui.label(
                        RichText::new(meta_parts.join("  \u{2022}  "))
                            .size(theme::FONT_META)
                            .color(theme::TEXT_SECONDARY),
                    );
                });

                // Navigation info (right-aligned)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.sibling_files.is_empty() {
                        ui.label(
                            RichText::new(format!(
                                "{}/{}",
                                self.current_index + 1,
                                self.sibling_files.len()
                            ))
                            .size(theme::FONT_META)
                            .color(theme::TEXT_HINT),
                        );
                    }
                });
            });
        });
    }

    fn draw_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let available = ui.available_size();
        let content_height = available.y - theme::FOOTER_HEIGHT;

        let frame = egui::Frame::NONE
            .fill(theme::BG_PRIMARY)
            .inner_margin(egui::Margin::symmetric(theme::INNER_PADDING as i8, 8));

        frame.show(ui, |ui| {
            ui.set_min_height(content_height);

            match self.preview.kind {
                FileKind::Text => self.draw_text_content(ui),
                FileKind::Image => self.draw_image_content(ui, ctx),
                FileKind::Directory => self.draw_directory_content(ui),
                FileKind::Archive => self.draw_archive_content(ui),
                FileKind::Pdf | FileKind::Video => {
                    self.poll_thumbnail(ctx);
                    if let Thumb::Ready(tex) = &self.thumb {
                        Self::draw_texture_fit(ui, tex);
                    } else {
                        self.draw_metadata_content(ui);
                    }
                }
                FileKind::Audio => self.draw_metadata_content(ui),
                FileKind::Unknown => self.draw_unknown_content(ui),
            }
        });
    }

    fn draw_text_content(&mut self, ui: &mut egui::Ui) {
        if self.preview.preview_text.is_some() {
            self.ensure_text_render();
        }
        if let Some(ref text) = self.preview.preview_text {
            ScrollArea::both()
                .max_height(ui.available_height())
                .auto_shrink([false, false])
                .show(ui, |ui| match &self.text_render {
                    Some(TextRender::Highlighted(spans)) => {
                        ui.label(highlight::to_layout_job(spans, theme::FONT_CODE));
                    }
                    Some(TextRender::Markdown(blocks)) => {
                        ui.set_max_width(ui.available_width());
                        markdown::render(
                            ui,
                            blocks,
                            theme::FONT_BODY + 1.0,
                            theme::TEXT_PRIMARY,
                            theme::ACCENT,
                        );
                    }
                    _ => {
                        ui.label(
                            RichText::new(text)
                                .size(theme::FONT_CODE)
                                .color(theme::TEXT_PRIMARY)
                                .family(egui::FontFamily::Monospace),
                        );
                    }
                });
        } else {
            ui.label(
                RichText::new("Unable to read file content")
                    .size(theme::FONT_BODY)
                    .color(theme::TEXT_HINT),
            );
        }
    }

    fn draw_image_content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.try_load_image(ctx);

        if let Some(ref texture) = self.image_texture {
            Self::draw_texture_fit(ui, texture);
        } else {
            // SVG or failed to load
            let dims_text = match self.preview.dimensions {
                Some((w, h)) => format!("{w} \u{00D7} {h} pixels"),
                None => "dimensions unknown".to_string(),
            };
            ui.centered_and_justified(|ui| {
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("\u{1F5BC}")
                            .size(48.0)
                            .color(theme::TEXT_HINT),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(dims_text)
                            .size(theme::FONT_BODY)
                            .color(theme::TEXT_SECONDARY),
                    );
                });
            });
        }
    }

    /// Draw a texture scaled to fit the available space (never upscaled).
    fn draw_texture_fit(ui: &mut egui::Ui, texture: &egui::TextureHandle) {
        let available = ui.available_size();
        let tex_size = texture.size_vec2();
        let scale_x = available.x / tex_size.x;
        let scale_y = available.y / tex_size.y;
        let scale = scale_x.min(scale_y).min(1.0);
        let display_size = egui::vec2(tex_size.x * scale, tex_size.y * scale);
        ui.centered_and_justified(|ui| {
            ui.image(egui::load::SizedTexture::new(texture.id(), display_size));
        });
    }

    fn draw_archive_content(&self, ui: &mut egui::Ui) {
        let Some(listing) = &self.preview.archive else {
            if let Some(text) = &self.preview.preview_text {
                ui.label(
                    RichText::new(text)
                        .size(theme::FONT_BODY)
                        .color(theme::TEXT_SECONDARY),
                );
            }
            return;
        };
        ui.label(
            RichText::new(format!(
                "{} entries  \u{2022}  {} uncompressed",
                listing.total,
                preview::format_size(listing.total_size)
            ))
            .size(theme::FONT_META)
            .color(theme::TEXT_SECONDARY),
        );
        ui.add_space(4.0);
        ScrollArea::vertical()
            .max_height(ui.available_height())
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Grid::new("archive_entries")
                    .striped(true)
                    .num_columns(2)
                    .spacing([16.0, 2.0])
                    .show(ui, |ui| {
                        for e in &listing.entries {
                            let icon = if e.is_dir { "\u{1F4C1}" } else { "\u{1F4C4}" };
                            ui.label(
                                RichText::new(format!("{icon} {}", e.name))
                                    .size(theme::FONT_BODY)
                                    .color(theme::TEXT_PRIMARY),
                            );
                            ui.label(
                                RichText::new(if e.is_dir {
                                    String::new()
                                } else {
                                    preview::format_size(e.size)
                                })
                                .size(theme::FONT_META)
                                .color(theme::TEXT_HINT),
                            );
                            ui.end_row();
                        }
                    });
                if listing.total > listing.entries.len() {
                    ui.label(
                        RichText::new(format!(
                            "... ({} more entries)",
                            listing.total - listing.entries.len()
                        ))
                        .size(theme::FONT_META)
                        .color(theme::TEXT_HINT),
                    );
                }
            });
    }

    fn draw_directory_content(&self, ui: &mut egui::Ui) {
        if let Some(ref text) = self.preview.preview_text {
            ScrollArea::vertical()
                .max_height(ui.available_height())
                .show(ui, |ui| {
                    for line in text.lines() {
                        let (icon, name) = if let Some(rest) = line.strip_prefix("[dir] ") {
                            ("\u{1F4C1} ", rest)
                        } else if let Some(rest) = line.strip_prefix("[file] ") {
                            ("\u{1F4C4} ", rest)
                        } else {
                            ("", line)
                        };

                        ui.horizontal(|ui| {
                            if !icon.is_empty() {
                                ui.label(RichText::new(icon).size(theme::FONT_BODY));
                            }
                            ui.label(
                                RichText::new(name)
                                    .size(theme::FONT_BODY)
                                    .color(theme::TEXT_PRIMARY),
                            );
                        });
                    }
                });
        }
    }

    fn draw_metadata_content(&self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                let icon = match self.preview.kind {
                    FileKind::Pdf => "\u{1F4D1}",
                    FileKind::Audio => "\u{1F3B5}",
                    FileKind::Video => "\u{1F3AC}",
                    _ => "\u{1F4CE}",
                };
                ui.label(RichText::new(icon).size(48.0).color(theme::TEXT_HINT));
                ui.add_space(12.0);

                if let Some(ref text) = self.preview.preview_text {
                    // PDF info
                    ScrollArea::vertical()
                        .max_height(ui.available_height() - 80.0)
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(text)
                                    .size(theme::FONT_CODE)
                                    .color(theme::TEXT_PRIMARY)
                                    .family(egui::FontFamily::Monospace),
                            );
                        });
                } else if let Some(ref dur) = self.preview.duration {
                    ui.label(
                        RichText::new(format!("Duration: {dur}"))
                            .size(theme::FONT_BODY)
                            .color(theme::TEXT_SECONDARY),
                    );
                }
            });
        });
    }

    fn draw_unknown_content(&self, ui: &mut egui::Ui) {
        ui.centered_and_justified(|ui| {
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("\u{1F4CE}")
                        .size(48.0)
                        .color(theme::TEXT_HINT),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new("No preview available for this file type")
                        .size(theme::FONT_BODY)
                        .color(theme::TEXT_SECONDARY),
                );
                if let Some(ref mime) = self.preview.mime_type {
                    ui.label(
                        RichText::new(mime.as_str())
                            .size(theme::FONT_META)
                            .color(theme::TEXT_HINT),
                    );
                }
            });
        });
    }

    fn draw_footer(&mut self, ui: &mut egui::Ui) {
        // Separator
        ui.painter().rect_filled(
            egui::Rect::from_min_size(
                egui::pos2(theme::INNER_PADDING, ui.cursor().min.y),
                egui::vec2(theme::WINDOW_WIDTH - theme::INNER_PADDING * 2.0, 1.0),
            ),
            0.0,
            theme::SEPARATOR,
        );

        let frame = egui::Frame::NONE
            .fill(theme::BG_FOOTER)
            .inner_margin(egui::Margin::symmetric(theme::INNER_PADDING as i8, 8));

        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                // Open with button
                let btn = ui.add(
                    egui::Button::new(
                        RichText::new("Open with default app")
                            .size(theme::FONT_BUTTON)
                            .color(theme::ACCENT),
                    )
                    .fill(egui::Color32::TRANSPARENT)
                    .stroke(egui::Stroke::new(1.0, theme::ACCENT))
                    .corner_radius(6.0),
                );
                if btn.clicked() {
                    self.open_with_default();
                    self.should_close = true;
                }

                if self.confirm_delete {
                    ui.label(
                        RichText::new("Move to trash?")
                            .size(theme::FONT_BUTTON)
                            .color(theme::TEXT_PRIMARY),
                    );
                    if ui.button("Delete").clicked() {
                        self.delete_current();
                    }
                    if ui.button("Cancel").clicked() {
                        self.confirm_delete = false;
                    }
                } else if let Some(msg) = &self.status {
                    ui.label(
                        RichText::new(msg)
                            .size(theme::FONT_META)
                            .color(egui::Color32::from_rgb(0xE5, 0x6B, 0x6B)),
                    );
                }

                // Shortcuts hint (right-aligned)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let space = if self.config.enable_space_to_activate {
                        "  Space/Esc close"
                    } else {
                        "  Esc close"
                    };
                    ui.label(
                        RichText::new(format!(
                            "\u{2190}\u{2192} navigate  \u{23CE} open  Del trash{space}"
                        ))
                        .size(theme::FONT_META)
                        .color(theme::TEXT_HINT),
                    );
                });
            });
        });
    }
}

impl eframe::App for PeekApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle keyboard shortcuts
        let (esc, left, right, enter, space, delete) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::Escape),
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Delete),
            )
        });
        if esc {
            if self.confirm_delete {
                self.confirm_delete = false;
            } else {
                self.should_close = true;
            }
        }
        if left {
            self.navigate(-1);
        }
        if right {
            self.navigate(1);
        }
        if enter {
            if self.confirm_delete {
                self.delete_current();
            } else {
                self.open_with_default();
                self.should_close = true;
            }
        }
        // Space toggles the preview like the activation shortcut (upstream
        // EnableSpaceToActivate).
        if space && self.config.enable_space_to_activate {
            self.should_close = true;
        }
        if delete && self.preview.kind != FileKind::Directory {
            if self.config.confirm_file_delete {
                self.confirm_delete = true;
            } else {
                self.delete_current();
            }
        }

        // Close on focus loss (upstream CloseAfterLosingFocus)
        let has_focus = ctx.input(|i| i.focused);
        if has_focus {
            self.had_focus = true;
        }
        if self.config.close_after_losing_focus && !has_focus && self.had_focus {
            self.should_close = true;
        }

        // Main panel
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG_PRIMARY)
                    .corner_radius(theme::CORNER_RADIUS)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER)),
            )
            .show(ctx, |ui| {
                self.draw_header(ui);

                // Separator
                ui.painter().rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(theme::INNER_PADDING, ui.cursor().min.y),
                        egui::vec2(theme::WINDOW_WIDTH - theme::INNER_PADDING * 2.0, 1.0),
                    ),
                    0.0,
                    theme::SEPARATOR,
                );

                self.draw_content(ui, ctx);
                self.draw_footer(ui);
            });

        if self.should_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        if matches!(self.thumb, Thumb::Pending(_)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

/// Run the peek preview window.
pub fn run_peek(preview: FilePreview, config: PeekConfig) {
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([theme::WINDOW_WIDTH, theme::WINDOW_HEIGHT])
        .with_min_inner_size([400.0, 300.0])
        .with_decorations(false)
        .with_transparent(true)
        .with_resizable(true);
    if config.always_on_top {
        viewport = viewport.with_always_on_top();
    }
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let _ = eframe::run_native(
        "Peek",
        native_options,
        Box::new(move |cc| {
            setup_visuals(&cc.egui_ctx);
            Ok(Box::new(PeekApp::new(preview, config)))
        }),
    );
}

fn setup_visuals(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = egui::Color32::TRANSPARENT;
    visuals.panel_fill = egui::Color32::TRANSPARENT;
    visuals.window_shadow = egui::Shadow::NONE;
    ctx.set_visuals(visuals);
}

/// List files in the same directory as the given path (non-recursive).
fn list_sibling_files(path: &Path) -> Vec<PathBuf> {
    let parent = match path.parent() {
        Some(p) => p,
        None => return vec![path.to_path_buf()],
    };

    let mut files: Vec<PathBuf> = match std::fs::read_dir(parent) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| !p.is_dir())
            .collect(),
        Err(_) => return vec![path.to_path_buf()],
    };

    files.sort();
    files
}
