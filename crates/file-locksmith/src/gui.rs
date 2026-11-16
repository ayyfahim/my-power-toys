//! egui window: list of processes using the selected paths.

use crate::kill::{self, KillOutcome};
use crate::scan::{self, ProcessInfo, ScanResult};
use crate::{FileLocksmithConfig, spawn_elevated};
use eframe::egui;
use egui::{Color32, RichText};
use mpt_common::theme::Theme;
use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Child;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};
use tracing::{info, warn};

const THEME: Theme = Theme::dark();

type KillResults = Vec<(u32, Result<KillOutcome, String>)>;

struct App {
    config: FileLocksmithConfig,
    is_root: bool,
    /// Paths being inspected. Empty = show the picker.
    paths: Vec<PathBuf>,
    /// Picker state.
    pending: Vec<PathBuf>,
    path_input: String,
    result: Option<ScanResult>,
    scan_rx: Option<Receiver<ScanResult>>,
    last_scan: Instant,
    kill_rx: Option<Receiver<KillResults>>,
    busy: HashSet<u32>,
    status: Option<(String, bool)>,
    elevated_child: Option<Child>,
}

impl App {
    fn new(paths: Vec<PathBuf>) -> Self {
        let mut app = Self {
            config: FileLocksmithConfig::load(),
            is_root: scan::current_user_is_root(),
            paths: Vec::new(),
            pending: Vec::new(),
            path_input: String::new(),
            result: None,
            scan_rx: None,
            last_scan: Instant::now(),
            kill_rx: None,
            busy: HashSet::new(),
            status: None,
            elevated_child: None,
        };
        if !paths.is_empty() {
            app.set_paths(paths);
        }
        app
    }

    fn set_paths(&mut self, paths: Vec<PathBuf>) {
        self.paths = scan::normalize_targets(&paths);
        self.result = None;
        self.status = None;
        self.start_scan();
    }

    fn start_scan(&mut self) {
        if self.scan_rx.is_some() || self.paths.is_empty() {
            return;
        }
        let (tx, rx) = channel();
        let paths = self.paths.clone();
        std::thread::spawn(move || {
            let _ = tx.send(scan::find_processes(&paths));
        });
        self.scan_rx = Some(rx);
        self.last_scan = Instant::now();
    }

    fn end(&mut self, pids: Vec<u32>) {
        if pids.is_empty() || self.kill_rx.is_some() {
            return;
        }
        let timeout = FileLocksmithConfig::load().kill_timeout();
        self.busy.extend(pids.iter().copied());
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let results = if pids.len() == 1 {
                vec![(pids[0], kill::end_process(pids[0], timeout))]
            } else {
                kill::end_all(&pids, timeout)
            };
            let results = results
                .into_iter()
                .map(|(pid, r)| (pid, r.map_err(|e| format!("{e:#}"))))
                .collect();
            let _ = tx.send(results);
        });
        self.kill_rx = Some(rx);
    }

    fn run_as_admin(&mut self) {
        match spawn_elevated(&self.paths) {
            Ok(child) => {
                info!("Relaunching elevated via pkexec (pid={})", child.id());
                self.elevated_child = Some(child);
                self.status = Some(("Waiting for administrator authorization…".into(), false));
            }
            Err(e) => self.status = Some((format!("{e:#}"), true)),
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.scan_rx
            && let Ok(res) = rx.try_recv()
        {
            self.result = Some(res);
            self.scan_rx = None;
        }

        if let Some(rx) = &self.kill_rx
            && let Ok(results) = rx.try_recv()
        {
            self.kill_rx = None;
            self.busy.clear();
            let errors: Vec<String> = results
                .iter()
                .filter_map(|(_, r)| r.as_ref().err().cloned())
                .collect();
            self.status = if errors.is_empty() {
                let killed = results
                    .iter()
                    .filter(|(_, r)| matches!(r, Ok(KillOutcome::Killed)))
                    .count();
                let msg = if killed > 0 {
                    format!("Ended {} process(es), {killed} forcibly", results.len())
                } else {
                    format!("Ended {} process(es)", results.len())
                };
                Some((msg, false))
            } else {
                Some((errors.join("\n"), true))
            };
            self.start_scan();
        }

        if let Some(child) = &mut self.elevated_child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    self.elevated_child = None;
                    match status.code() {
                        // pkexec: 126 = not authorized / dismissed, 127 = auth failed.
                        Some(126 | 127) => {
                            self.status =
                                Some(("Administrator authorization was cancelled".into(), true));
                        }
                        Some(0) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        _ => {
                            self.status =
                                Some((format!("Administrator instance exited ({status})"), true));
                        }
                    }
                }
                Ok(None) => {
                    if self.status.as_ref().is_some_and(|(_, e)| !e) {
                        self.status =
                            Some(("Running as administrator in another window".into(), false));
                    }
                }
                Err(e) => {
                    warn!("pkexec wait failed: {e}");
                    self.elevated_child = None;
                }
            }
        }

        let interval = self.config.refresh_interval_ms;
        if interval > 0 && !self.paths.is_empty() {
            let interval = Duration::from_millis(interval);
            if self.last_scan.elapsed() >= interval && self.kill_rx.is_none() {
                self.start_scan();
            }
            ctx.request_repaint_after(interval);
        }
        if self.scan_rx.is_some() || self.kill_rx.is_some() || self.elevated_child.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        if self.paths.is_empty() {
            for p in dropped {
                if !self.pending.contains(&p) {
                    self.pending.push(p);
                }
            }
        } else {
            self.set_paths(dropped);
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading(if self.is_root {
                "File Locksmith (Administrator)"
            } else {
                "File Locksmith"
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !self.is_root
                    && ui
                        .add_enabled(
                            self.elevated_child.is_none(),
                            egui::Button::new("Run as administrator"),
                        )
                        .on_hover_text("Relaunch with pkexec to see and end every process")
                        .clicked()
                {
                    self.run_as_admin();
                }
                if self.paths.is_empty() {
                    return;
                }
                let n = self.result.as_ref().map_or(0, |r| r.processes.len());
                if ui
                    .add_enabled(
                        n > 0 && self.kill_rx.is_none(),
                        egui::Button::new("End all"),
                    )
                    .clicked()
                    && let Some(r) = &self.result
                {
                    let pids = r.processes.iter().map(|p| p.pid).collect();
                    self.end(pids);
                }
                if ui
                    .add_enabled(self.scan_rx.is_none(), egui::Button::new("⟳ Reload"))
                    .clicked()
                {
                    self.start_scan();
                }
                if ui.button("Change selection…").clicked() {
                    self.pending = std::mem::take(&mut self.paths);
                    self.result = None;
                    self.status = None;
                }
            });
        });

        if !self.paths.is_empty() {
            let label = match self.paths.as_slice() {
                [one] => one.display().to_string(),
                many => format!("{} items selected", many.len()),
            };
            let tooltip = self
                .paths
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("\n");
            ui.label(RichText::new(label).color(THEME.text_secondary))
                .on_hover_text(tooltip);
        }

        if let Some((msg, is_err)) = &self.status {
            let color = if *is_err {
                THEME.text_error
            } else {
                THEME.text_success
            };
            ui.label(RichText::new(msg).color(color));
        }
        ui.separator();
    }

    fn picker(&mut self, ui: &mut egui::Ui) {
        ui.label("Select the files or folders to check. You can also drop them on this window.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.path_input)
                    .hint_text("/path/to/file or folder")
                    .desired_width(ui.available_width() - 260.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Add").clicked() || enter) && !self.path_input.trim().is_empty() {
                let p = PathBuf::from(expand_tilde(self.path_input.trim()));
                if !self.pending.contains(&p) {
                    self.pending.push(p);
                }
                self.path_input.clear();
            }
            if ui.button("Browse files…").clicked()
                && let Some(files) = rfd::FileDialog::new()
                    .set_title("Select files")
                    .pick_files()
            {
                for f in files {
                    if !self.pending.contains(&f) {
                        self.pending.push(f);
                    }
                }
            }
            if ui.button("Browse folder…").clicked()
                && let Some(dir) = rfd::FileDialog::new()
                    .set_title("Select folder")
                    .pick_folder()
                && !self.pending.contains(&dir)
            {
                self.pending.push(dir);
            }
        });
        ui.add_space(8.0);

        if self.pending.is_empty() {
            ui.label(RichText::new("No files selected").color(THEME.text_muted));
        } else {
            let mut remove = None;
            egui::ScrollArea::vertical()
                .max_height(ui.available_height() - 40.0)
                .show(ui, |ui| {
                    for (i, p) in self.pending.iter().enumerate() {
                        ui.horizontal(|ui| {
                            if ui.small_button("✕").clicked() {
                                remove = Some(i);
                            }
                            ui.label(p.display().to_string());
                        });
                    }
                });
            if let Some(i) = remove {
                self.pending.remove(i);
            }
        }
        ui.add_space(8.0);
        if ui
            .add_enabled(
                !self.pending.is_empty(),
                egui::Button::new("Find processes"),
            )
            .clicked()
        {
            let pending = std::mem::take(&mut self.pending);
            self.set_paths(pending);
        }
    }

    fn list(&mut self, ui: &mut egui::Ui) {
        let Some(result) = self.result.clone() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Scanning processes…");
            });
            return;
        };

        if result.inaccessible > 0 && !self.is_root {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "⚠ {} process(es) could not be inspected. Run as administrator to see them.",
                        result.inaccessible
                    ))
                    .color(THEME.text_muted),
                );
            });
            ui.add_space(4.0);
        }

        if result.processes.is_empty() {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No processes are using the selected files")
                        .size(16.0)
                        .color(THEME.text_secondary),
                );
            });
            return;
        }

        let mut to_end = None;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for p in &result.processes {
                    if self.process_row(ui, p) {
                        to_end = Some(p.pid);
                    }
                    ui.add_space(4.0);
                }
            });
        if let Some(pid) = to_end {
            self.end(vec![pid]);
        }
    }

    /// Draw one process. Returns true if "End task" was clicked.
    fn process_row(&self, ui: &mut egui::Ui, p: &ProcessInfo) -> bool {
        let mut clicked = false;
        let busy = self.busy.contains(&p.pid);
        egui::Frame::new()
            .fill(THEME.bg_card)
            .corner_radius(6.0)
            .inner_margin(8.0)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let id = ui.make_persistent_id(("proc", p.pid));
                egui::collapsing_header::CollapsingState::load_with_default_open(
                    ui.ctx(),
                    id,
                    false,
                )
                .show_header(ui, |ui| {
                    let name = if p.name.is_empty() { "?" } else { &p.name };
                    let hover = p
                        .exe
                        .as_ref()
                        .map(|e| e.display().to_string())
                        .unwrap_or_else(|| p.cmdline.clone());
                    ui.label(RichText::new(name).strong().color(THEME.text_primary))
                        .on_hover_text(hover);
                    ui.label(RichText::new(format!("PID {}", p.pid)).color(THEME.text_muted));
                    ui.label(RichText::new(format!("User {}", p.user)).color(THEME.text_muted));
                    ui.label(
                        RichText::new(format!("{} file(s)", p.files.len()))
                            .color(THEME.text_muted),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if busy {
                            ui.spinner();
                        } else if ui
                            .add_enabled(self.kill_rx.is_none(), egui::Button::new("End task"))
                            .clicked()
                        {
                            clicked = true;
                        }
                    });
                })
                .body(|ui| {
                    if p.uid == Some(0) && !self.is_root {
                        ui.label(
                            RichText::new(
                                "This process runs as root; ending it requires administrator rights.",
                            )
                            .color(Color32::from_rgb(230, 180, 80)),
                        );
                    }
                    if let Some(exe) = &p.exe {
                        ui.label(RichText::new(format!("Executable: {}", exe.display())).small());
                    }
                    if !p.cmdline.is_empty() {
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("Command: {}", p.cmdline)).small(),
                            )
                            .truncate(),
                        )
                        .on_hover_text(&p.cmdline);
                    }
                    ui.add_space(4.0);
                    ui.label(RichText::new("Files").strong());
                    for f in &p.files {
                        let full = f.display().to_string();
                        let short = f
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_else(|| full.clone());
                        ui.add(egui::Label::new(short).truncate())
                            .on_hover_text(full);
                    }
                });
            });
        clicked
    }
}

fn expand_tilde(s: &str) -> String {
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest).display().to_string();
    }
    s.to_string()
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll(ctx);
        self.handle_drops(ctx);
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.add_space(6.0);
            self.header(ui);
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.paths.is_empty() {
                self.picker(ui);
            } else {
                self.list(ui);
            }
        });
    }
}

/// Open the File Locksmith window. Blocks until it is closed.
pub fn run(paths: Vec<PathBuf>) {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([760.0, 520.0])
            .with_min_inner_size([480.0, 320.0])
            .with_title("File Locksmith")
            .with_app_id("mpt-file-locksmith")
            .with_drag_and_drop(true)
            .with_resizable(true),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "File Locksmith",
        options,
        Box::new(|cc| {
            THEME.apply(&cc.egui_ctx);
            Ok(Box::new(App::new(paths)))
        }),
    ) {
        tracing::error!("File Locksmith window failed: {e}");
    }
}
