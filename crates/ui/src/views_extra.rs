//! Settings pages for Advanced Paste, Screen Ruler and the modules added for
//! PowerToys parity. Simple fields go through `Message::SetCfgBool` /
//! `Message::SetCfgText` (see `module_settings::{BoolField, TextField}`).

use iced::widget::{Space, column, container, row, text, text_input, toggler};
use iced::{Alignment, Element, Length};

use crate::Settings;
use crate::message::Message;
use crate::module_settings::{BoolField, TextField};
use crate::theme;
use crate::translations::Tr;
use crate::types::{Ui, bold};
use crate::widgets::seg_button;

impl Settings {
    /// Dispatch for the pages defined in this file. `None` = not handled here.
    pub(crate) fn settings_extra<'a>(
        &self,
        id: &str,
        tr: &'a Tr,
        ui: Ui,
    ) -> Option<Element<'a, Message>> {
        let page = match id {
            "paste-plain" => self.settings_paste_plain(tr, ui),
            "screen-ruler" => self.settings_screen_ruler(tr, ui),
            "crop-and-lock" => self.settings_crop_and_lock(tr, ui),
            "file-locksmith" => self.settings_file_locksmith(tr, ui),
            "environment-variables" => self.settings_environment_variables(tr, ui),
            "new-plus" => self.settings_new_plus(tr, ui),
            "zoom-it" => self.settings_zoom_it(tr, ui),
            "power-display" => self.settings_power_display(tr, ui),
            "grab-and-move" => self.settings_grab_and_move(tr, ui),
            "alt-window-cycle" => self.settings_alt_window_cycle(tr, ui),
            "command-not-found" => self.settings_command_not_found(tr, ui),
            _ => return None,
        };
        Some(page)
    }

    // ── Building blocks ─────────────────────────────────────────────────────

    fn x_title<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        text(tr.module_settings)
            .size(ui.sz(16.0))
            .font(bold())
            .color(ui.heading())
            .into()
    }

    fn x_heading<'a>(&self, label: &'a str, ui: Ui) -> Element<'a, Message> {
        text(label)
            .size(ui.sz(14.0))
            .font(ui.font())
            .color(ui.heading())
            .into()
    }

    fn x_hint<'a>(&self, label: &'a str, ui: Ui) -> Element<'a, Message> {
        text(label)
            .size(ui.sz(12.0))
            .font(ui.font())
            .color(theme::subtext0(ui.dark))
            .into()
    }

    /// Toggle row bound to a [`BoolField`].
    fn x_toggle<'a>(
        &self,
        label: &'a str,
        desc: &'a str,
        field: BoolField,
        ui: Ui,
    ) -> Element<'a, Message> {
        let mut labels = column![text(label).size(ui.sz(14.0)).font(ui.font())].spacing(2);
        if !desc.is_empty() {
            labels = labels.push(
                text(desc)
                    .size(ui.sz(12.0))
                    .font(ui.font())
                    .color(theme::subtext0(ui.dark)),
            );
        }
        row![
            labels.width(Length::Fill),
            toggler(self.module_configs.get_bool(field))
                .on_toggle(move |v| Message::SetCfgBool(field, v))
                .size(ui.sz(22.0)),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into()
    }

    /// Labelled text input bound to a [`TextField`].
    fn x_input<'a>(
        &self,
        label: &'a str,
        placeholder: &'a str,
        field: TextField,
        ui: Ui,
    ) -> Element<'a, Message> {
        let value = self.module_configs.get_text(field);
        column![
            text(label)
                .size(ui.sz(12.0))
                .font(ui.font())
                .color(theme::subtext0(ui.dark)),
            text_input(placeholder, &value)
                .on_input(move |s| Message::SetCfgText(field, s))
                .size(ui.sz(13.0))
                .padding(8),
        ]
        .spacing(4)
        .width(Length::Fill)
        .into()
    }

    /// Two inputs side by side.
    fn x_pair<'a>(&self, a: Element<'a, Message>, b: Element<'a, Message>) -> Element<'a, Message> {
        row![a, b].spacing(12).into()
    }

    // ── Advanced Paste ──────────────────────────────────────────────────────

    fn settings_paste_plain<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        let mut col = column![
            self.x_title(tr, ui),
            self.x_heading(tr.ms_shortcuts, ui),
            self.x_hint(tr.ms_shortcut_hint, ui),
            self.x_input(
                tr.ms_pp_ui_shortcut,
                "Super+Shift+V",
                TextField::PpUiShortcut,
                ui
            ),
            self.x_pair(
                self.x_input(
                    tr.ms_pp_markdown_shortcut,
                    "Super+Ctrl+Alt+M",
                    TextField::PpMarkdownShortcut,
                    ui
                ),
                self.x_input(
                    tr.ms_pp_json_shortcut,
                    "Super+Ctrl+Alt+J",
                    TextField::PpJsonShortcut,
                    ui
                ),
            ),
            Space::with_height(8),
            self.x_heading(tr.ms_general, ui),
            self.x_toggle(
                tr.ms_pp_preview,
                tr.ms_pp_preview_desc,
                BoolField::PpClipboardPreview,
                ui
            ),
            self.x_toggle(
                tr.ms_pp_close_focus,
                tr.ms_pp_close_focus_desc,
                BoolField::PpCloseAfterLosingFocus,
                ui
            ),
            self.x_toggle(
                tr.ms_pp_as_file,
                tr.ms_pp_as_file_desc,
                BoolField::PpShowPasteAsFile,
                ui
            ),
            Space::with_height(8),
            self.x_toggle(tr.ms_pp_ai, tr.ms_pp_ai_desc, BoolField::PpAiEnabled, ui),
        ]
        .spacing(8);

        if self.module_configs.paste_plain.ai_enabled {
            col = col
                .push(self.x_input(
                    tr.ms_pp_ai_endpoint,
                    "https://api.openai.com/v1",
                    TextField::PpAiEndpoint,
                    ui,
                ))
                .push(self.x_pair(
                    self.x_input(tr.ms_pp_ai_model, "gpt-4o-mini", TextField::PpAiModel, ui),
                    self.x_input(
                        tr.ms_pp_ai_key_env,
                        "OPENAI_API_KEY",
                        TextField::PpAiKeyEnv,
                        ui,
                    ),
                ));
        }
        col.into()
    }

    // ── Screen Ruler ────────────────────────────────────────────────────────

    fn settings_screen_ruler<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        use mpt_screen_ruler::UnitsOfMeasure;

        let cfg = &self.module_configs.screen_ruler;
        let units = [
            (UnitsOfMeasure::Pixels, "px"),
            (UnitsOfMeasure::Inches, "in"),
            (UnitsOfMeasure::Centimeters, "cm"),
            (UnitsOfMeasure::Millimeters, "mm"),
        ];
        let mut units_row = row![].spacing(2);
        for (u, label) in units {
            units_row = units_row.push(seg_button(
                label,
                cfg.units_of_measure == u,
                Message::SetScreenRulerUnits(u),
                ui,
            ));
        }

        column![
            self.x_title(tr, ui),
            self.x_input(
                tr.ms_sr_shortcut,
                "Super+Shift+R",
                TextField::SrShortcut,
                ui
            ),
            self.x_hint(tr.ms_shortcut_hint, ui),
            Space::with_height(8),
            self.x_toggle(tr.ms_sr_feet, tr.ms_sr_feet_desc, BoolField::SrDrawFeet, ui),
            self.x_toggle(
                tr.ms_sr_per_channel,
                tr.ms_sr_per_channel_desc,
                BoolField::SrPerChannel,
                ui
            ),
            self.x_pair(
                self.x_input(tr.ms_sr_tolerance, "30", TextField::SrTolerance, ui),
                self.x_input(tr.ms_sr_color, "#FF4500", TextField::SrCrossColor, ui),
            ),
            self.x_hint(tr.ms_sr_units, ui),
            container(units_row).style(theme::segmented_control),
        ]
        .spacing(8)
        .into()
    }

    // ── Crop and Lock ───────────────────────────────────────────────────────

    fn settings_crop_and_lock<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        column![
            self.x_title(tr, ui),
            self.x_heading(tr.ms_shortcuts, ui),
            self.x_hint(tr.ms_shortcut_hint, ui),
            self.x_input(
                tr.ms_cal_thumbnail,
                mpt_crop_and_lock::DEFAULT_THUMBNAIL_HOTKEY,
                TextField::CalThumbnailHotkey,
                ui
            ),
            self.x_input(
                tr.ms_cal_reparent,
                mpt_crop_and_lock::DEFAULT_REPARENT_HOTKEY,
                TextField::CalReparentHotkey,
                ui
            ),
            self.x_input(
                tr.ms_cal_screenshot,
                mpt_crop_and_lock::DEFAULT_SCREENSHOT_HOTKEY,
                TextField::CalScreenshotHotkey,
                ui
            ),
        ]
        .spacing(8)
        .into()
    }

    // ── File Locksmith ──────────────────────────────────────────────────────

    fn settings_file_locksmith<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        column![
            self.x_title(tr, ui),
            self.x_toggle(
                tr.ms_fl_context_menu,
                tr.ms_fl_context_menu_desc,
                BoolField::FlContextMenu,
                ui
            ),
            self.x_pair(
                self.x_input(tr.ms_fl_kill_timeout, "3000", TextField::FlKillTimeout, ui),
                self.x_input(tr.ms_fl_refresh, "2000", TextField::FlRefreshInterval, ui),
            ),
        ]
        .spacing(8)
        .into()
    }

    // ── Environment Variables ───────────────────────────────────────────────

    fn settings_environment_variables<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        column![
            self.x_title(tr, ui),
            self.x_toggle(
                tr.ms_ev_admin,
                tr.ms_ev_admin_desc,
                BoolField::EvLaunchAdministrator,
                ui
            ),
            self.x_toggle(
                tr.ms_ev_update_session,
                tr.ms_ev_update_session_desc,
                BoolField::EvUpdateSession,
                ui
            ),
        ]
        .spacing(8)
        .into()
    }

    // ── New+ ────────────────────────────────────────────────────────────────

    fn settings_new_plus<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        column![
            self.x_title(tr, ui),
            self.x_input(
                tr.ms_np_location,
                "~/.local/share/my-power-toys/new-plus-templates",
                TextField::NpTemplateLocation,
                ui
            ),
            Space::with_height(4),
            self.x_toggle(
                tr.ms_np_hide_ext,
                tr.ms_np_hide_ext_desc,
                BoolField::NpHideExtension,
                ui
            ),
            self.x_toggle(
                tr.ms_np_hide_digits,
                tr.ms_np_hide_digits_desc,
                BoolField::NpHideDigits,
                ui
            ),
            self.x_toggle(
                tr.ms_np_replace_vars,
                tr.ms_np_replace_vars_desc,
                BoolField::NpReplaceVariables,
                ui
            ),
            self.x_toggle(
                tr.ms_np_context_menu,
                tr.ms_np_context_menu_desc,
                BoolField::NpContextMenu,
                ui
            ),
            self.x_toggle(
                tr.ms_np_sync_xdg,
                tr.ms_np_sync_xdg_desc,
                BoolField::NpSyncXdg,
                ui
            ),
        ]
        .spacing(8)
        .into()
    }

    // ── ZoomIt ──────────────────────────────────────────────────────────────

    fn settings_zoom_it<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        column![
            self.x_title(tr, ui),
            self.x_heading(tr.ms_shortcuts, ui),
            self.x_hint(tr.ms_shortcut_hint, ui),
            self.x_pair(
                self.x_input(tr.ms_zi_zoom, "Ctrl+1", TextField::ZiToggleKey, ui),
                self.x_input(tr.ms_zi_draw, "Ctrl+2", TextField::ZiDrawKey, ui),
            ),
            self.x_pair(
                self.x_input(tr.ms_zi_break, "Ctrl+3", TextField::ZiBreakKey, ui),
                self.x_input(tr.ms_zi_live_zoom, "Ctrl+4", TextField::ZiLiveZoomKey, ui),
            ),
            self.x_pair(
                self.x_input(tr.ms_zi_record, "Ctrl+5", TextField::ZiRecordKey, ui),
                self.x_input(tr.ms_zi_snip, "Ctrl+6", TextField::ZiSnipKey, ui),
            ),
            self.x_input(
                tr.ms_zi_snip_save,
                "Ctrl+Shift+6",
                TextField::ZiSnipSaveKey,
                ui
            ),
            Space::with_height(8),
            self.x_heading(tr.ms_zi_zoom, ui),
            self.x_toggle(
                tr.ms_zi_animate,
                tr.ms_zi_animate_desc,
                BoolField::ZiAnimateZoom,
                ui
            ),
            self.x_toggle(
                tr.ms_zi_smooth,
                tr.ms_zi_smooth_desc,
                BoolField::ZiSmoothImage,
                ui
            ),
            Space::with_height(8),
            self.x_heading(tr.ms_general, ui),
            self.x_pair(
                self.x_input(tr.ms_zi_pen_color, "#FF0000", TextField::ZiPenColor, ui),
                self.x_input(tr.ms_zi_break_timeout, "10", TextField::ZiBreakTimeout, ui),
            ),
            self.x_toggle(
                tr.ms_zi_snip_copy,
                tr.ms_zi_snip_copy_desc,
                BoolField::ZiSnipCopy,
                ui
            ),
            self.x_input(
                tr.ms_zi_screenshot_dir,
                "~/Pictures",
                TextField::ZiScreenshotDir,
                ui
            ),
            Space::with_height(8),
            self.x_heading(tr.ms_zi_record, ui),
            self.x_input(tr.ms_zi_frame_rate, "30", TextField::ZiFrameRate, ui),
            self.x_toggle(
                tr.ms_zi_audio,
                tr.ms_zi_audio_desc,
                BoolField::ZiCaptureAudio,
                ui
            ),
            self.x_input(
                tr.ms_zi_recording_dir,
                "~/Videos",
                TextField::ZiRecordingDir,
                ui
            ),
        ]
        .spacing(8)
        .into()
    }

    // ── Power Display ───────────────────────────────────────────────────────

    fn settings_power_display<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        column![
            self.x_title(tr, ui),
            self.x_heading(tr.ms_shortcuts, ui),
            self.x_hint(tr.ms_shortcut_hint, ui),
            self.x_pair(
                self.x_input(
                    tr.ms_pd_brightness_up,
                    "Super+Alt+Up",
                    TextField::PdBrightnessUp,
                    ui
                ),
                self.x_input(
                    tr.ms_pd_brightness_down,
                    "Super+Alt+Down",
                    TextField::PdBrightnessDown,
                    ui
                ),
            ),
            self.x_input(tr.ms_pd_step, "5", TextField::PdStep, ui),
            self.x_toggle(tr.ms_pd_osd, tr.ms_pd_osd_desc, BoolField::PdShowOsd, ui),
            Space::with_height(8),
            self.x_heading(tr.ms_general, ui),
            self.x_toggle(
                tr.ms_pd_restore,
                tr.ms_pd_restore_desc,
                BoolField::PdRestoreOnStartup,
                ui
            ),
            self.x_toggle(
                tr.ms_pd_linked,
                tr.ms_pd_linked_desc,
                BoolField::PdLinkedLevels,
                ui
            ),
            Space::with_height(8),
            self.x_heading(tr.ms_pd_backends, ui),
            self.x_toggle(tr.ms_pd_ddc, tr.ms_pd_ddc_desc, BoolField::PdEnableDdc, ui),
            self.x_toggle(
                tr.ms_pd_backlight,
                tr.ms_pd_backlight_desc,
                BoolField::PdEnableBacklight,
                ui
            ),
            self.x_toggle(
                tr.ms_pd_software,
                tr.ms_pd_software_desc,
                BoolField::PdEnableSoftware,
                ui
            ),
        ]
        .spacing(8)
        .into()
    }

    // ── Grab and Move ───────────────────────────────────────────────────────

    fn settings_grab_and_move<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        use mpt_grab_and_move::GrabModifier;

        let cfg = &self.module_configs.grab_and_move;
        column![
            self.x_title(tr, ui),
            self.x_heading(tr.ms_gm_modifier, ui),
            self.x_hint(tr.ms_gm_modifier_desc, ui),
            container(
                row![
                    seg_button(
                        "Alt",
                        cfg.modifier_key == GrabModifier::Alt,
                        Message::SetGrabModifier(GrabModifier::Alt),
                        ui
                    ),
                    seg_button(
                        "Super",
                        cfg.modifier_key == GrabModifier::Super,
                        Message::SetGrabModifier(GrabModifier::Super),
                        ui
                    ),
                ]
                .spacing(2),
            )
            .style(theme::segmented_control),
            Space::with_height(8),
            self.x_toggle(
                tr.ms_gm_resize,
                tr.ms_gm_resize_desc,
                BoolField::GmAltResize,
                ui
            ),
            self.x_toggle(
                tr.ms_gm_game_mode,
                tr.ms_gm_game_mode_desc,
                BoolField::GmGameMode,
                ui
            ),
            self.x_input(
                tr.ms_excluded_apps_list,
                "gimp, blender",
                TextField::GmExcludedApps,
                ui
            ),
        ]
        .spacing(8)
        .into()
    }

    // ── Alt Window Cycle ────────────────────────────────────────────────────

    fn settings_alt_window_cycle<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        column![
            self.x_title(tr, ui),
            self.x_heading(tr.ms_shortcuts, ui),
            self.x_hint(tr.ms_shortcut_hint, ui),
            self.x_pair(
                self.x_input(tr.ms_awc_next, "Alt+`", TextField::AwcNext, ui),
                self.x_input(tr.ms_awc_prev, "Alt+Shift+`", TextField::AwcPrevious, ui),
            ),
            Space::with_height(8),
            self.x_heading(tr.ms_general, ui),
            self.x_toggle(
                tr.ms_awc_minimized,
                tr.ms_awc_minimized_desc,
                BoolField::AwcIncludeMinimized,
                ui
            ),
            self.x_toggle(
                tr.ms_awc_all_desktops,
                tr.ms_awc_all_desktops_desc,
                BoolField::AwcAllDesktops,
                ui
            ),
            self.x_input(tr.ms_awc_timeout, "1500", TextField::AwcTimeout, ui),
        ]
        .spacing(8)
        .into()
    }

    // ── Command Not Found ───────────────────────────────────────────────────

    fn settings_command_not_found<'a>(&self, tr: &'a Tr, ui: Ui) -> Element<'a, Message> {
        let mut col = column![
            self.x_title(tr, ui),
            self.x_toggle(
                tr.ms_cnf_install,
                tr.ms_cnf_install_desc,
                BoolField::CnfInstallHooks,
                ui
            ),
        ]
        .spacing(8);

        if self.module_configs.command_not_found.install_shell_hooks {
            col = col
                .push(self.x_heading(tr.ms_cnf_shells, ui))
                .push(self.x_toggle("Bash", "~/.bashrc", BoolField::CnfBash, ui))
                .push(self.x_toggle("Zsh", "~/.zshrc", BoolField::CnfZsh, ui))
                .push(self.x_toggle("Fish", "~/.config/fish/functions", BoolField::CnfFish, ui));
        }

        col.push(Space::with_height(8))
            .push(self.x_toggle(
                tr.ms_cnf_fuzzy,
                tr.ms_cnf_fuzzy_desc,
                BoolField::CnfFuzzy,
                ui,
            ))
            .push(self.x_input(tr.ms_cnf_timeout, "5000", TextField::CnfTimeout, ui))
            .into()
    }
}
