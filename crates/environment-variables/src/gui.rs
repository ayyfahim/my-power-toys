//! egui editor window: User / System / Profiles / Applied tabs.

use crate::envd;
use crate::listvalue;
use crate::manager::{Manager, Target};
use crate::session::SessionReport;
use crate::store::Scope;
use crate::{EnvironmentVariablesConfig, MODULE_ID};
use egui::{Color32, RichText};
use mpt_common::theme::Theme;
use tracing::info;

const THEME: Theme = Theme::dark();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    User,
    System,
    Profiles,
    Applied,
}

/// Variable add/edit dialog.
struct EditDialog {
    target: Target,
    original: Option<String>,
    name: String,
    value: String,
    list_mode: bool,
    entries: Vec<String>,
    selected: Option<usize>,
    error: Option<String>,
}

impl EditDialog {
    fn new(target: Target, original: Option<(String, String)>) -> Self {
        let (name, value) = original.clone().unwrap_or_default();
        let list_mode = listvalue::is_list_variable(&name);
        Self {
            target,
            original: original.map(|(n, _)| n),
            entries: listvalue::split(&value),
            name,
            value,
            list_mode,
            selected: None,
            error: None,
        }
    }

    fn current_value(&self) -> String {
        if self.list_mode {
            listvalue::join(&self.entries)
        } else {
            self.value.clone()
        }
    }
}

enum Confirm {
    DeleteVar(Target, String),
    DeleteProfile(String),
}

enum Action {
    Edit(Target, Option<(String, String)>),
    AskDelete(Confirm),
    CreateProfile(String),
    RenameProfile(String, String),
    ApplyProfile(String),
    UnapplyProfile,
    AddExistingToProfile(String, String, String),
    SetLaunchAdministrator(bool),
    Reload,
}

pub struct EnvApp {
    manager: Option<Manager>,
    load_error: Option<String>,
    tab: Tab,
    edit: Option<EditDialog>,
    confirm: Option<Confirm>,
    status: Vec<(bool, String)>,
    selected_profile: Option<String>,
    new_profile_name: String,
    rename_buffer: String,
    applied_filter: String,
    include_session: bool,
}

impl EnvApp {
    pub fn new(config: EnvironmentVariablesConfig) -> Self {
        let (manager, load_error) = match Manager::load(config) {
            Ok(m) => (Some(m), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        };
        let selected_profile = manager
            .as_ref()
            .and_then(|m| m.profiles.profiles.first().map(|p| p.name.clone()));
        Self {
            rename_buffer: selected_profile.clone().unwrap_or_default(),
            manager,
            load_error,
            tab: Tab::User,
            edit: None,
            confirm: None,
            status: Vec::new(),
            selected_profile,
            new_profile_name: String::new(),
            applied_filter: String::new(),
            include_session: false,
        }
    }

    fn report(&mut self, r: anyhow::Result<SessionReport>) {
        self.status.clear();
        match r {
            Ok(rep) => {
                self.status
                    .extend(rep.warnings.into_iter().map(|w| (true, w)));
                self.status
                    .extend(rep.messages.into_iter().map(|m| (false, m)));
                if self.status.is_empty() {
                    self.status.push((false, "Saved.".into()));
                }
            }
            Err(e) => self.status.push((true, format!("{e:#}"))),
        }
    }

    fn handle(&mut self, action: Action) {
        let Some(m) = self.manager.as_mut() else {
            return;
        };
        match action {
            Action::Edit(target, original) => self.edit = Some(EditDialog::new(target, original)),
            Action::AskDelete(c) => self.confirm = Some(c),
            Action::CreateProfile(name) => {
                let name = name.trim().to_string();
                let r = m.create_profile(&name).map(|_| SessionReport::default());
                if r.is_ok() {
                    self.selected_profile = Some(name.clone());
                    self.rename_buffer = name;
                    self.new_profile_name.clear();
                }
                self.report(r);
            }
            Action::RenameProfile(old, new) => {
                let r = m
                    .rename_profile(&old, &new)
                    .map(|_| SessionReport::default());
                if r.is_ok() {
                    self.selected_profile = Some(new);
                }
                self.report(r);
            }
            Action::ApplyProfile(name) => {
                let r = m.apply_profile(&name);
                self.report(r);
            }
            Action::UnapplyProfile => {
                let r = m.unapply_profile();
                self.report(r);
            }
            Action::AddExistingToProfile(profile, name, value) => {
                let r = m.set_var(&Target::Profile(profile), None, &name, &value);
                self.report(r);
            }
            Action::SetLaunchAdministrator(v) => {
                m.config.launch_administrator = v;
                let r = mpt_common::config::save_module_config(MODULE_ID, &m.config)
                    .map(|_| SessionReport::default());
                self.report(r);
            }
            Action::Reload => {
                let r = m.reload().map(|_| SessionReport {
                    messages: vec!["Reloaded from disk.".into()],
                    warnings: vec![],
                });
                self.report(r);
            }
        }
    }

    // ── Tabs ────────────────────────────────────────────────────────────

    fn var_table(
        ui: &mut egui::Ui,
        id: &str,
        target: &Target,
        vars: &[(String, String)],
        editable: bool,
        actions: &mut Vec<Action>,
    ) {
        if vars.is_empty() {
            ui.label(RichText::new("No variables.").color(THEME.text_muted));
            return;
        }
        egui::Grid::new(id)
            .striped(true)
            .num_columns(if editable { 3 } else { 2 })
            .min_col_width(80.0)
            .show(ui, |ui| {
                for (k, v) in vars {
                    ui.label(RichText::new(k).strong().color(THEME.text_primary));
                    let shown = if listvalue::is_list_variable(k) {
                        v.replace(':', " : ")
                    } else {
                        v.clone()
                    };
                    ui.add(egui::Label::new(RichText::new(shown).monospace()).truncate())
                        .on_hover_text(v);
                    if editable {
                        ui.horizontal(|ui| {
                            if ui.small_button("Edit").clicked() {
                                actions.push(Action::Edit(
                                    target.clone(),
                                    Some((k.clone(), v.clone())),
                                ));
                            }
                            if ui.small_button("Delete").clicked() {
                                actions.push(Action::AskDelete(Confirm::DeleteVar(
                                    target.clone(),
                                    k.clone(),
                                )));
                            }
                        });
                    }
                    ui.end_row();
                }
            });
    }

    fn user_tab(m: &Manager, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.horizontal(|ui| {
            ui.heading("User variables");
            if ui.button("+ Add variable").clicked() {
                actions.push(Action::Edit(Target::User, None));
            }
        });
        ui.label(
            RichText::new(m.paths.user_file.display().to_string())
                .small()
                .color(THEME.text_secondary),
        );
        ui.add_space(6.0);
        Self::var_table(
            ui,
            "user_vars",
            &Target::User,
            &m.user.vars(),
            true,
            actions,
        );

        if let Some(applied) = &m.profiles.applied
            && !m.profiles.backups.is_empty()
        {
            ui.add_space(10.0);
            ui.label(
                RichText::new(format!(
                    "Overridden by profile '{applied}' (restored when it is unapplied):"
                ))
                .color(THEME.text_accent),
            );
            let backups: Vec<(String, String)> = m
                .profiles
                .backups
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Self::var_table(ui, "backups", &Target::User, &backups, false, actions);
        }

        let others: Vec<_> = m
            .sources
            .iter()
            .filter(|s| s.scope == Scope::User && !s.managed)
            .collect();
        if !others.is_empty() {
            ui.add_space(10.0);
            ui.separator();
            ui.label(RichText::new("Other user files (read-only)").strong());
            for s in others {
                ui.label(
                    RichText::new(s.path.display().to_string())
                        .small()
                        .color(THEME.text_secondary),
                );
                Self::var_table(
                    ui,
                    &format!("user_other_{}", s.path.display()),
                    &Target::User,
                    &s.vars,
                    false,
                    actions,
                );
            }
        }
    }

    fn system_tab(m: &Manager, ui: &mut egui::Ui, actions: &mut Vec<Action>) {
        ui.heading("System variables");
        let mut admin = m.config.launch_administrator;
        if ui
            .checkbox(
                &mut admin,
                "Allow editing system variables (asks for administrator password)",
            )
            .changed()
        {
            actions.push(Action::SetLaunchAdministrator(admin));
        }
        if m.can_edit_system() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Editable (managed by MyPowerToys)").strong());
                if ui.button("+ Add variable").clicked() {
                    actions.push(Action::Edit(Target::System, None));
                }
            });
            ui.label(
                RichText::new(format!(
                    "{} — saved with pkexec",
                    m.paths.system_file.display()
                ))
                .small()
                .color(THEME.text_secondary),
            );
            Self::var_table(
                ui,
                "system_managed",
                &Target::System,
                &m.system_managed.vars(),
                true,
                actions,
            );
        }
        ui.add_space(10.0);
        ui.separator();
        ui.label(RichText::new("Read-only system sources").strong());
        let mut any = false;
        for s in m
            .sources
            .iter()
            .filter(|s| s.scope == Scope::System && s.path != m.paths.system_file)
        {
            any = true;
            ui.add_space(4.0);
            ui.label(
                RichText::new(s.path.display().to_string())
                    .small()
                    .color(THEME.text_secondary),
            );
            Self::var_table(
                ui,
                &format!("sys_{}", s.path.display()),
                &Target::System,
                &s.vars,
                false,
                actions,
            );
        }
        if !any {
            ui.label(
                RichText::new("No variables in /etc/environment or environment.d directories.")
                    .color(THEME.text_muted),
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn profiles_tab(
        m: &Manager,
        ui: &mut egui::Ui,
        selected: &mut Option<String>,
        new_name: &mut String,
        rename_buffer: &mut String,
        actions: &mut Vec<Action>,
    ) {
        ui.heading("Profiles");
        ui.label(
            RichText::new(
                "Only one profile can be applied at a time. Applying writes its variables to \
                 60-my-power-toys-profile.conf and temporarily replaces user variables with the \
                 same name.",
            )
            .small()
            .color(THEME.text_secondary),
        );
        ui.add_space(6.0);
        ui.columns(2, |cols| {
            let left = &mut cols[0];
            left.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(new_name).hint_text("New profile name"));
                if ui.button("Create").clicked() {
                    actions.push(Action::CreateProfile(new_name.clone()));
                }
            });
            left.add_space(4.0);
            for p in &m.profiles.profiles {
                let is_applied = m.profiles.applied.as_deref() == Some(p.name.as_str());
                let label = if is_applied {
                    format!("● {} (applied)", p.name)
                } else {
                    p.name.clone()
                };
                let is_sel = selected.as_deref() == Some(p.name.as_str());
                if left.selectable_label(is_sel, label).clicked() {
                    *selected = Some(p.name.clone());
                    *rename_buffer = p.name.clone();
                }
            }
            if m.profiles.profiles.is_empty() {
                left.label(RichText::new("No profiles yet.").color(THEME.text_muted));
            }

            let right = &mut cols[1];
            let Some(name) = selected.clone() else { return };
            let Some(profile) = m.profiles.get(&name) else {
                *selected = None;
                return;
            };
            let is_applied = m.profiles.applied.as_deref() == Some(name.as_str());
            right.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(rename_buffer).desired_width(160.0));
                if ui.button("Rename").clicked() && *rename_buffer != name {
                    actions.push(Action::RenameProfile(
                        name.clone(),
                        rename_buffer.trim().into(),
                    ));
                }
            });
            right.horizontal(|ui| {
                if is_applied {
                    if ui.button("Unapply").clicked() {
                        actions.push(Action::UnapplyProfile);
                    }
                } else if ui.button("Apply").clicked() {
                    actions.push(Action::ApplyProfile(name.clone()));
                }
                if ui.button("Delete profile").clicked() {
                    actions.push(Action::AskDelete(Confirm::DeleteProfile(name.clone())));
                }
            });
            right.add_space(6.0);
            right.horizontal(|ui| {
                if ui.button("+ Add variable").clicked() {
                    actions.push(Action::Edit(Target::Profile(name.clone()), None));
                }
                egui::ComboBox::from_id_salt("add_existing")
                    .selected_text("Add existing…")
                    .show_ui(ui, |ui| {
                        for row in m.effective(false) {
                            if profile.variables.iter().any(|v| v.name == row.name) {
                                continue;
                            }
                            if ui
                                .selectable_label(
                                    false,
                                    format!("{} ({})", row.name, row.scope.label()),
                                )
                                .clicked()
                            {
                                actions.push(Action::AddExistingToProfile(
                                    name.clone(),
                                    row.name.clone(),
                                    row.value.clone(),
                                ));
                            }
                        }
                    });
            });
            let vars: Vec<(String, String)> = profile
                .variables
                .iter()
                .map(|v| (v.name.clone(), v.value.clone()))
                .collect();
            Self::var_table(
                right,
                "profile_vars",
                &Target::Profile(name.clone()),
                &vars,
                true,
                actions,
            );
        });
    }

    fn applied_tab(
        m: &Manager,
        ui: &mut egui::Ui,
        filter: &mut String,
        include_session: &mut bool,
    ) {
        ui.heading("Applied variables");
        ui.label(
            RichText::new(
                "Effective values for new sessions, in systemd environment.d order. \
                 Later sources override earlier ones.",
            )
            .small()
            .color(THEME.text_secondary),
        );
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(filter).hint_text("Filter by name"));
            ui.checkbox(
                include_session,
                "Include current session (process) variables",
            );
        });
        ui.add_space(6.0);
        let f = filter.to_lowercase();
        egui::Grid::new("applied")
            .striped(true)
            .num_columns(3)
            .show(ui, |ui| {
                for row in m.effective(*include_session) {
                    if !f.is_empty() && !row.name.to_lowercase().contains(&f) {
                        continue;
                    }
                    let color = match row.scope {
                        Scope::System => Color32::from_rgb(230, 170, 90),
                        Scope::User => THEME.text_accent,
                        Scope::Profile => THEME.text_success,
                        Scope::Session => THEME.text_muted,
                    };
                    let mut hover = match &row.path {
                        Some(p) => format!("From {}", p.display()),
                        None => "Inherited from the current session".to_string(),
                    };
                    if row.value != row.expanded {
                        hover.push_str(&format!("\nAs written: {}", row.value));
                    }
                    for (scope, p) in &row.overrides {
                        hover.push_str(&format!("\nOverrides {} ({})", scope.label(), p.display()));
                    }
                    ui.label(RichText::new(row.scope.label()).color(color))
                        .on_hover_text(&hover);
                    ui.label(RichText::new(&row.name).strong());
                    ui.add(egui::Label::new(RichText::new(&row.expanded).monospace()).truncate())
                        .on_hover_text(hover);
                    ui.end_row();
                }
            });
    }

    // ── Dialogs ─────────────────────────────────────────────────────────

    fn edit_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = self.edit.as_mut() else { return };
        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        let title = match (&d.target, &d.original) {
            (Target::Profile(p), None) => format!("Add variable to '{p}'"),
            (Target::Profile(p), Some(_)) => format!("Edit variable in '{p}'"),
            (Target::System, None) => "Add system variable".into(),
            (Target::System, Some(_)) => "Edit system variable".into(),
            (Target::User, None) => "Add user variable".into(),
            (Target::User, Some(_)) => "Edit user variable".into(),
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(true)
            .default_width(520.0)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name");
                    ui.add(egui::TextEdit::singleline(&mut d.name).desired_width(360.0));
                });
                let was_list = d.list_mode;
                ui.checkbox(&mut d.list_mode, "Edit as list (entries joined with ':')");
                if was_list != d.list_mode {
                    if d.list_mode {
                        d.entries = listvalue::split(&d.value);
                    } else {
                        d.value = listvalue::join(&d.entries);
                    }
                    d.selected = None;
                }
                ui.add_space(4.0);
                if d.list_mode {
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            let mut remove = None;
                            for (i, e) in d.entries.iter_mut().enumerate() {
                                ui.horizontal(|ui| {
                                    let sel = d.selected == Some(i);
                                    if ui.selectable_label(sel, format!("{:>2}", i + 1)).clicked() {
                                        d.selected = Some(i);
                                    }
                                    let r = ui.add(
                                        egui::TextEdit::singleline(e)
                                            .code_editor()
                                            .desired_width(360.0),
                                    );
                                    if r.gained_focus() {
                                        d.selected = Some(i);
                                    }
                                    if ui.small_button("✕").on_hover_text("Remove").clicked() {
                                        remove = Some(i);
                                    }
                                });
                            }
                            if let Some(i) = remove {
                                d.entries.remove(i);
                                d.selected = None;
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui.button("Add entry").clicked() {
                            let at = d.selected.map(|i| i + 1).unwrap_or(d.entries.len());
                            d.entries.insert(at, String::new());
                            d.selected = Some(at);
                        }
                        let has_sel = d.selected.is_some();
                        if ui.add_enabled(has_sel, egui::Button::new("Move up")).clicked()
                            && let Some(i) = d.selected
                        {
                            d.selected = Some(listvalue::move_entry(&mut d.entries, i, -1));
                        }
                        if ui.add_enabled(has_sel, egui::Button::new("Move down")).clicked()
                            && let Some(i) = d.selected
                        {
                            d.selected = Some(listvalue::move_entry(&mut d.entries, i, 1));
                        }
                    });
                } else {
                    ui.label("Value");
                    ui.add(
                        egui::TextEdit::multiline(&mut d.value)
                            .code_editor()
                            .desired_rows(3)
                            .desired_width(f32::INFINITY),
                    );
                }
                ui.label(
                    RichText::new(
                        "$VAR and ${VAR:-default} references are kept as written and expanded at login.",
                    )
                    .small()
                    .color(THEME.text_secondary),
                );
                if let Some(err) = &d.error {
                    ui.colored_label(THEME.text_error, err);
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if save {
            let name = d.name.trim().to_string();
            let value = d.current_value();
            if let Err(e) = envd::validate(&name, &value) {
                d.error = Some(e);
                return;
            }
            let target = d.target.clone();
            let original = d.original.clone();
            if let Some(m) = self.manager.as_mut() {
                match m.set_var(&target, original.as_deref(), &name, &value) {
                    Ok(rep) => {
                        self.edit = None;
                        self.report(Ok(rep));
                    }
                    Err(e) => {
                        if let Some(d) = self.edit.as_mut() {
                            d.error = Some(format!("{e:#}"));
                        }
                    }
                }
            }
        } else if cancel || !open {
            self.edit = None;
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(c) = &self.confirm else { return };
        let text = match c {
            Confirm::DeleteVar(_, n) => format!("Delete variable '{n}'?"),
            Confirm::DeleteProfile(n) => format!("Delete profile '{n}'?"),
        };
        let mut yes = false;
        let mut no = false;
        egui::Window::new("Confirm")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(text);
                ui.horizontal(|ui| {
                    yes = ui.button("Delete").clicked();
                    no = ui.button("Cancel").clicked();
                });
            });
        if no {
            self.confirm = None;
        }
        if yes && let Some(c) = self.confirm.take() {
            let Some(m) = self.manager.as_mut() else {
                return;
            };
            let r = match c {
                Confirm::DeleteVar(t, n) => m.delete_var(&t, &n),
                Confirm::DeleteProfile(n) => {
                    if self.selected_profile.as_deref() == Some(n.as_str()) {
                        self.selected_profile = None;
                    }
                    m.delete_profile(&n)
                }
            };
            self.report(r);
        }
    }
}

impl eframe::App for EnvApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut actions = Vec::new();

        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::User, "User");
                ui.selectable_value(&mut self.tab, Tab::System, "System");
                ui.selectable_value(&mut self.tab, Tab::Profiles, "Profiles");
                ui.selectable_value(&mut self.tab, Tab::Applied, "Applied variables");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Reload").clicked() {
                        actions.push(Action::Reload);
                    }
                    if let Some(m) = &self.manager
                        && let Some(p) = &m.profiles.applied
                    {
                        ui.label(
                            RichText::new(format!("Applied profile: {p}"))
                                .color(THEME.text_success),
                        );
                    }
                });
            });
            ui.add_space(4.0);
        });

        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            if self.status.is_empty() {
                ui.label(
                    RichText::new(
                        "Changes apply to newly started apps; log out and back in if needed.",
                    )
                    .small()
                    .color(THEME.text_muted),
                );
            }
            for (is_warn, msg) in &self.status {
                let color = if *is_warn {
                    THEME.text_error
                } else {
                    THEME.text_secondary
                };
                ui.label(RichText::new(msg).small().color(color));
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let Some(m) = &self.manager else {
                ui.colored_label(
                    THEME.text_error,
                    format!(
                        "Failed to load environment files: {}",
                        self.load_error.as_deref().unwrap_or("unknown error")
                    ),
                );
                return;
            };
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.tab {
                    Tab::User => Self::user_tab(m, ui, &mut actions),
                    Tab::System => Self::system_tab(m, ui, &mut actions),
                    Tab::Profiles => Self::profiles_tab(
                        m,
                        ui,
                        &mut self.selected_profile,
                        &mut self.new_profile_name,
                        &mut self.rename_buffer,
                        &mut actions,
                    ),
                    Tab::Applied => Self::applied_tab(
                        m,
                        ui,
                        &mut self.applied_filter,
                        &mut self.include_session,
                    ),
                });
        });

        self.edit_dialog(ctx);
        self.confirm_dialog(ctx);

        for a in actions {
            self.handle(a);
        }
    }
}

pub fn run(config: EnvironmentVariablesConfig) {
    info!("Opening Environment Variables editor");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 620.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("Environment Variables")
            .with_app_id("mpt-environment-variables")
            .with_resizable(true),
        ..Default::default()
    };
    if let Err(e) = eframe::run_native(
        "Environment Variables",
        options,
        Box::new(move |cc| {
            THEME.apply(&cc.egui_ctx);
            Ok(Box::new(EnvApp::new(config)))
        }),
    ) {
        tracing::error!("Environment Variables window failed: {e}");
    }
}
