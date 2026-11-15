//! Module configurations edited by the settings UI.
//!
//! Every config is a type alias to the module crate's own config struct, so a
//! save writes back every field the module knows about (including ones the UI
//! has no control for) instead of rewriting the file with a partial mirror.

use mpt_common::config::{load_module_config, save_module_config};

pub type ColorPickerConf = mpt_color_picker::ColorPickerConfig;
pub type TextExtractorConf = mpt_text_extractor::TextExtractorConfig;
pub type ImageResizerConf = mpt_image_resizer::ImageResizerConfig;
pub type MouseUtilsConf = mpt_mouse_utils::config::MouseUtilsConfig;
pub type AppLauncherConf = mpt_app_launcher::AppLauncherConfig;
pub type FancyZonesConf = mpt_fancy_zones::FancyZonesConfig;
pub type PeekConf = mpt_peek::PeekConfig;
pub type CommandPaletteConf = mpt_command_palette::CommandPaletteConfig;
pub type LightSwitchConf = mpt_light_switch::config::LightSwitchConfig;
pub type AwakeConf = mpt_awake::config::AwakeConfig;
pub type AlwaysOnTopConf = mpt_always_on_top::Config;
pub type KeyManagerConf = mpt_key_manager::KeyManagerConfig;
pub type WorkspacesConf = mpt_workspaces::config::WorkspacesConfig;
pub type HostsEditorConf = mpt_hosts_editor::config::HostsEditorConfig;
pub type BulkRenameConf = mpt_bulk_rename::config::BulkRenameConfig;
pub type PastePlainConf = mpt_paste_plain::PastePlainConfig;
pub type ScreenRulerConf = mpt_screen_ruler::ScreenRulerConfig;
pub type CropAndLockConf = mpt_crop_and_lock::CropAndLockConfig;
pub type FileLocksmithConf = mpt_file_locksmith::FileLocksmithConfig;
pub type EnvironmentVariablesConf = mpt_environment_variables::EnvironmentVariablesConfig;
pub type NewPlusConf = mpt_new_plus::NewPlusConfig;
pub type ZoomItConf = mpt_zoom_it::ZoomItConfig;
pub type PowerDisplayConf = mpt_power_display::PowerDisplayConfig;
pub type GrabAndMoveConf = mpt_grab_and_move::GrabAndMoveConfig;
pub type AltWindowCycleConf = mpt_alt_window_cycle::AltWindowCycleConfig;
pub type CommandNotFoundConf = mpt_command_not_found::CommandNotFoundConfig;

// ── Generic fields ───────────────────────────────────────────────────────────
//
// Simple boolean and text settings of the newer modules go through these two
// enums instead of one `Message` variant per field.

/// A boolean setting edited with a toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolField {
    // Advanced Paste
    PpClipboardPreview,
    PpCloseAfterLosingFocus,
    PpShowPasteAsFile,
    PpAiEnabled,
    // Screen Ruler
    SrDrawFeet,
    SrPerChannel,
    // File Locksmith
    FlContextMenu,
    // Environment Variables
    EvLaunchAdministrator,
    EvUpdateSession,
    // New+
    NpHideExtension,
    NpHideDigits,
    NpReplaceVariables,
    NpContextMenu,
    NpSyncXdg,
    // ZoomIt
    ZiAnimateZoom,
    ZiSmoothImage,
    ZiCaptureAudio,
    ZiSnipCopy,
    // Power Display
    PdShowOsd,
    PdRestoreOnStartup,
    PdEnableDdc,
    PdEnableBacklight,
    PdEnableSoftware,
    PdLinkedLevels,
    // Grab and Move
    GmAltResize,
    GmGameMode,
    // Alt Window Cycle
    AwcIncludeMinimized,
    AwcAllDesktops,
    // Command Not Found
    CnfInstallHooks,
    CnfBash,
    CnfZsh,
    CnfFish,
    CnfFuzzy,
}

/// A text or numeric setting edited with a text input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextField {
    // Advanced Paste
    PpUiShortcut,
    PpMarkdownShortcut,
    PpJsonShortcut,
    PpAiEndpoint,
    PpAiModel,
    PpAiKeyEnv,
    // Screen Ruler
    SrShortcut,
    SrTolerance,
    SrCrossColor,
    // Crop and Lock
    CalThumbnailHotkey,
    CalReparentHotkey,
    CalScreenshotHotkey,
    // File Locksmith
    FlKillTimeout,
    FlRefreshInterval,
    // New+
    NpTemplateLocation,
    // ZoomIt
    ZiToggleKey,
    ZiDrawKey,
    ZiBreakKey,
    ZiLiveZoomKey,
    ZiRecordKey,
    ZiSnipKey,
    ZiSnipSaveKey,
    ZiPenColor,
    ZiBreakTimeout,
    ZiFrameRate,
    ZiScreenshotDir,
    ZiRecordingDir,
    // Power Display
    PdBrightnessUp,
    PdBrightnessDown,
    PdStep,
    // Grab and Move
    GmExcludedApps,
    // Alt Window Cycle
    AwcNext,
    AwcPrevious,
    AwcTimeout,
    // Command Not Found
    CnfTimeout,
}

/// Split a comma-separated list into trimmed, non-empty entries.
pub fn split_list(s: &str) -> Vec<String> {
    s.split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

// ── Aggregate ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ModuleConfigs {
    pub awake: AwakeConf,
    pub always_on_top: AlwaysOnTopConf,
    pub color_picker: ColorPickerConf,
    pub text_extractor: TextExtractorConf,
    pub image_resizer: ImageResizerConf,
    pub mouse_utils: MouseUtilsConf,
    pub app_launcher: AppLauncherConf,
    pub fancy_zones: FancyZonesConf,
    pub peek: PeekConf,
    pub command_palette: CommandPaletteConf,
    pub light_switch: LightSwitchConf,
    pub key_manager: KeyManagerConf,
    pub workspaces: WorkspacesConf,
    pub hosts_editor: HostsEditorConf,
    pub bulk_rename: BulkRenameConf,
    pub paste_plain: PastePlainConf,
    pub screen_ruler: ScreenRulerConf,
    pub crop_and_lock: CropAndLockConf,
    pub file_locksmith: FileLocksmithConf,
    pub environment_variables: EnvironmentVariablesConf,
    pub new_plus: NewPlusConf,
    pub zoom_it: ZoomItConf,
    pub power_display: PowerDisplayConf,
    pub grab_and_move: GrabAndMoveConf,
    pub alt_window_cycle: AltWindowCycleConf,
    pub command_not_found: CommandNotFoundConf,
}

impl ModuleConfigs {
    pub fn load_all() -> Self {
        Self {
            awake: load_module_config("awake").unwrap_or_default(),
            always_on_top: load_module_config("always-on-top").unwrap_or_default(),
            color_picker: load_module_config("color-picker").unwrap_or_default(),
            text_extractor: load_module_config("text-extractor").unwrap_or_default(),
            image_resizer: load_module_config("image-resizer").unwrap_or_default(),
            mouse_utils: load_module_config("mouse-utils").unwrap_or_default(),
            app_launcher: load_module_config("app-launcher").unwrap_or_default(),
            fancy_zones: load_module_config("fancy-zones").unwrap_or_default(),
            peek: load_module_config("peek").unwrap_or_default(),
            command_palette: load_module_config("command-palette").unwrap_or_default(),
            light_switch: load_module_config("light-switch").unwrap_or_default(),
            key_manager: load_module_config("key-manager").unwrap_or_default(),
            workspaces: load_module_config("workspaces").unwrap_or_default(),
            hosts_editor: load_module_config("hosts-editor").unwrap_or_default(),
            bulk_rename: load_module_config("bulk-rename").unwrap_or_default(),
            paste_plain: load_module_config("paste-plain").unwrap_or_default(),
            screen_ruler: load_module_config("screen-ruler").unwrap_or_default(),
            crop_and_lock: load_module_config("crop-and-lock").unwrap_or_default(),
            file_locksmith: load_module_config("file-locksmith").unwrap_or_default(),
            environment_variables: load_module_config("environment-variables").unwrap_or_default(),
            new_plus: load_module_config("new-plus").unwrap_or_default(),
            zoom_it: load_module_config("zoom-it").unwrap_or_default(),
            power_display: load_module_config("power-display").unwrap_or_default(),
            grab_and_move: load_module_config("grab-and-move").unwrap_or_default(),
            alt_window_cycle: load_module_config("alt-window-cycle").unwrap_or_default(),
            command_not_found: load_module_config("command-not-found").unwrap_or_default(),
        }
    }

    pub fn save(&self, module_id: &str) {
        let result = match module_id {
            "awake" => save_module_config(module_id, &self.awake),
            "always-on-top" => save_module_config(module_id, &self.always_on_top),
            "color-picker" => save_module_config(module_id, &self.color_picker),
            "text-extractor" => save_module_config(module_id, &self.text_extractor),
            "image-resizer" => save_module_config(module_id, &self.image_resizer),
            "mouse-utils" => save_module_config(module_id, &self.mouse_utils),
            "app-launcher" => save_module_config(module_id, &self.app_launcher),
            "fancy-zones" => save_module_config(module_id, &self.fancy_zones),
            "peek" => save_module_config(module_id, &self.peek),
            "command-palette" => save_module_config(module_id, &self.command_palette),
            "light-switch" => save_module_config(module_id, &self.light_switch),
            "key-manager" => save_module_config(module_id, &self.key_manager),
            "workspaces" => save_module_config(module_id, &self.workspaces),
            "hosts-editor" => save_module_config(module_id, &self.hosts_editor),
            "bulk-rename" => save_module_config(module_id, &self.bulk_rename),
            "paste-plain" => save_module_config(module_id, &self.paste_plain),
            "screen-ruler" => save_module_config(module_id, &self.screen_ruler),
            "crop-and-lock" => save_module_config(module_id, &self.crop_and_lock),
            "file-locksmith" => save_module_config(module_id, &self.file_locksmith),
            "environment-variables" => save_module_config(module_id, &self.environment_variables),
            "new-plus" => save_module_config(module_id, &self.new_plus),
            "zoom-it" => save_module_config(module_id, &self.zoom_it),
            "power-display" => save_module_config(module_id, &self.power_display),
            "grab-and-move" => save_module_config(module_id, &self.grab_and_move),
            "alt-window-cycle" => save_module_config(module_id, &self.alt_window_cycle),
            "command-not-found" => save_module_config(module_id, &self.command_not_found),
            _ => Ok(()),
        };
        if let Err(e) = result {
            tracing::warn!("Failed to save {module_id} settings: {e:#}");
        }
    }

    /// Current value of a boolean field.
    pub fn get_bool(&self, f: BoolField) -> bool {
        use BoolField::*;
        match f {
            PpClipboardPreview => self.paste_plain.enable_clipboard_preview,
            PpCloseAfterLosingFocus => self.paste_plain.close_after_losing_focus,
            PpShowPasteAsFile => self.paste_plain.show_paste_as_file,
            PpAiEnabled => self.paste_plain.ai_enabled,
            SrDrawFeet => self.screen_ruler.draw_feet_on_cross,
            SrPerChannel => self.screen_ruler.per_color_channel_edge_detection,
            FlContextMenu => self.file_locksmith.context_menu,
            EvLaunchAdministrator => self.environment_variables.launch_administrator,
            EvUpdateSession => self.environment_variables.update_session,
            NpHideExtension => self.new_plus.hide_file_extension,
            NpHideDigits => self.new_plus.hide_starting_digits,
            NpReplaceVariables => self.new_plus.replace_variables,
            NpContextMenu => self.new_plus.context_menu,
            NpSyncXdg => self.new_plus.sync_xdg_templates,
            ZiAnimateZoom => self.zoom_it.animate_zoom,
            ZiSmoothImage => self.zoom_it.smooth_image,
            ZiCaptureAudio => self.zoom_it.capture_audio,
            ZiSnipCopy => self.zoom_it.snip_copy_to_clipboard,
            PdShowOsd => self.power_display.show_osd_notification,
            PdRestoreOnStartup => self.power_display.restore_settings_on_startup,
            PdEnableDdc => self.power_display.enable_ddc,
            PdEnableBacklight => self.power_display.enable_backlight,
            PdEnableSoftware => self.power_display.enable_software_brightness,
            PdLinkedLevels => self.power_display.linked_levels_active,
            GmAltResize => self.grab_and_move.use_alt_resize,
            GmGameMode => self.grab_and_move.do_not_activate_on_game_mode,
            AwcIncludeMinimized => self.alt_window_cycle.include_minimized,
            AwcAllDesktops => self.alt_window_cycle.all_desktops,
            CnfInstallHooks => self.command_not_found.install_shell_hooks,
            CnfBash => self.command_not_found.bash,
            CnfZsh => self.command_not_found.zsh,
            CnfFish => self.command_not_found.fish,
            CnfFuzzy => self.command_not_found.fuzzy_suggestions,
        }
    }

    /// Set a boolean field. Returns the id of the module whose config changed.
    pub fn set_bool(&mut self, f: BoolField, v: bool) -> &'static str {
        use BoolField::*;
        let (slot, module) = match f {
            PpClipboardPreview => (
                &mut self.paste_plain.enable_clipboard_preview,
                "paste-plain",
            ),
            PpCloseAfterLosingFocus => (
                &mut self.paste_plain.close_after_losing_focus,
                "paste-plain",
            ),
            PpShowPasteAsFile => (&mut self.paste_plain.show_paste_as_file, "paste-plain"),
            PpAiEnabled => (&mut self.paste_plain.ai_enabled, "paste-plain"),
            SrDrawFeet => (&mut self.screen_ruler.draw_feet_on_cross, "screen-ruler"),
            SrPerChannel => (
                &mut self.screen_ruler.per_color_channel_edge_detection,
                "screen-ruler",
            ),
            FlContextMenu => (&mut self.file_locksmith.context_menu, "file-locksmith"),
            EvLaunchAdministrator => (
                &mut self.environment_variables.launch_administrator,
                "environment-variables",
            ),
            EvUpdateSession => (
                &mut self.environment_variables.update_session,
                "environment-variables",
            ),
            NpHideExtension => (&mut self.new_plus.hide_file_extension, "new-plus"),
            NpHideDigits => (&mut self.new_plus.hide_starting_digits, "new-plus"),
            NpReplaceVariables => (&mut self.new_plus.replace_variables, "new-plus"),
            NpContextMenu => (&mut self.new_plus.context_menu, "new-plus"),
            NpSyncXdg => (&mut self.new_plus.sync_xdg_templates, "new-plus"),
            ZiAnimateZoom => (&mut self.zoom_it.animate_zoom, "zoom-it"),
            ZiSmoothImage => (&mut self.zoom_it.smooth_image, "zoom-it"),
            ZiCaptureAudio => (&mut self.zoom_it.capture_audio, "zoom-it"),
            ZiSnipCopy => (&mut self.zoom_it.snip_copy_to_clipboard, "zoom-it"),
            PdShowOsd => (
                &mut self.power_display.show_osd_notification,
                "power-display",
            ),
            PdRestoreOnStartup => (
                &mut self.power_display.restore_settings_on_startup,
                "power-display",
            ),
            PdEnableDdc => (&mut self.power_display.enable_ddc, "power-display"),
            PdEnableBacklight => (&mut self.power_display.enable_backlight, "power-display"),
            PdEnableSoftware => (
                &mut self.power_display.enable_software_brightness,
                "power-display",
            ),
            PdLinkedLevels => (
                &mut self.power_display.linked_levels_active,
                "power-display",
            ),
            GmAltResize => (&mut self.grab_and_move.use_alt_resize, "grab-and-move"),
            GmGameMode => (
                &mut self.grab_and_move.do_not_activate_on_game_mode,
                "grab-and-move",
            ),
            AwcIncludeMinimized => (
                &mut self.alt_window_cycle.include_minimized,
                "alt-window-cycle",
            ),
            AwcAllDesktops => (&mut self.alt_window_cycle.all_desktops, "alt-window-cycle"),
            CnfInstallHooks => (
                &mut self.command_not_found.install_shell_hooks,
                "command-not-found",
            ),
            CnfBash => (&mut self.command_not_found.bash, "command-not-found"),
            CnfZsh => (&mut self.command_not_found.zsh, "command-not-found"),
            CnfFish => (&mut self.command_not_found.fish, "command-not-found"),
            CnfFuzzy => (
                &mut self.command_not_found.fuzzy_suggestions,
                "command-not-found",
            ),
        };
        *slot = v;
        module
    }

    /// Current value of a text field, formatted for a text input.
    pub fn get_text(&self, f: TextField) -> String {
        use TextField::*;
        match f {
            PpUiShortcut => self.paste_plain.advanced_paste_ui_shortcut.clone(),
            PpMarkdownShortcut => self.paste_plain.paste_as_markdown_shortcut.clone(),
            PpJsonShortcut => self.paste_plain.paste_as_json_shortcut.clone(),
            PpAiEndpoint => self.paste_plain.ai_endpoint.clone(),
            PpAiModel => self.paste_plain.ai_model.clone(),
            PpAiKeyEnv => self.paste_plain.ai_api_key_env.clone(),
            SrShortcut => self.screen_ruler.activation_shortcut.clone(),
            SrTolerance => self.screen_ruler.pixel_tolerance.to_string(),
            SrCrossColor => self.screen_ruler.measure_cross_color.clone(),
            CalThumbnailHotkey => self.crop_and_lock.thumbnail_hotkey.clone(),
            CalReparentHotkey => self.crop_and_lock.reparent_hotkey.clone(),
            CalScreenshotHotkey => self.crop_and_lock.screenshot_hotkey.clone(),
            FlKillTimeout => self.file_locksmith.kill_timeout_ms.to_string(),
            FlRefreshInterval => self.file_locksmith.refresh_interval_ms.to_string(),
            NpTemplateLocation => self.new_plus.template_location.clone(),
            ZiToggleKey => self.zoom_it.toggle_key.clone(),
            ZiDrawKey => self.zoom_it.draw_toggle_key.clone(),
            ZiBreakKey => self.zoom_it.break_timer_key.clone(),
            ZiLiveZoomKey => self.zoom_it.live_zoom_toggle_key.clone(),
            ZiRecordKey => self.zoom_it.record_toggle_key.clone(),
            ZiSnipKey => self.zoom_it.snip_toggle_key.clone(),
            ZiSnipSaveKey => self.zoom_it.snip_save_toggle_key.clone(),
            ZiPenColor => self.zoom_it.pen_color.clone(),
            ZiBreakTimeout => self.zoom_it.break_timeout.to_string(),
            ZiFrameRate => self.zoom_it.record_frame_rate.to_string(),
            ZiScreenshotDir => self.zoom_it.screenshot_save_location.clone(),
            ZiRecordingDir => self.zoom_it.recording_save_location.clone(),
            PdBrightnessUp => self.power_display.brightness_up_hotkey.clone(),
            PdBrightnessDown => self.power_display.brightness_down_hotkey.clone(),
            PdStep => self.power_display.hotkey_step.to_string(),
            GmExcludedApps => self.grab_and_move.excluded_apps.join(", "),
            AwcNext => self.alt_window_cycle.next_window_shortcut.clone(),
            AwcPrevious => self.alt_window_cycle.previous_window_shortcut.clone(),
            AwcTimeout => self.alt_window_cycle.continue_timeout_ms.to_string(),
            CnfTimeout => self.command_not_found.lookup_timeout_ms.to_string(),
        }
    }

    /// Set a text field. Numeric fields that fail to parse are left unchanged
    /// and `None` is returned; otherwise the id of the changed module.
    pub fn set_text(&mut self, f: TextField, v: String) -> Option<&'static str> {
        use TextField::*;
        let module = match f {
            PpUiShortcut => {
                self.paste_plain.advanced_paste_ui_shortcut = v;
                "paste-plain"
            }
            PpMarkdownShortcut => {
                self.paste_plain.paste_as_markdown_shortcut = v;
                "paste-plain"
            }
            PpJsonShortcut => {
                self.paste_plain.paste_as_json_shortcut = v;
                "paste-plain"
            }
            PpAiEndpoint => {
                self.paste_plain.ai_endpoint = v;
                "paste-plain"
            }
            PpAiModel => {
                self.paste_plain.ai_model = v;
                "paste-plain"
            }
            PpAiKeyEnv => {
                self.paste_plain.ai_api_key_env = v;
                "paste-plain"
            }
            SrShortcut => {
                self.screen_ruler.activation_shortcut = v;
                "screen-ruler"
            }
            SrTolerance => {
                self.screen_ruler.pixel_tolerance = v.trim().parse().ok()?;
                "screen-ruler"
            }
            SrCrossColor => {
                self.screen_ruler.measure_cross_color = v;
                "screen-ruler"
            }
            CalThumbnailHotkey => {
                self.crop_and_lock.thumbnail_hotkey = v;
                "crop-and-lock"
            }
            CalReparentHotkey => {
                self.crop_and_lock.reparent_hotkey = v;
                "crop-and-lock"
            }
            CalScreenshotHotkey => {
                self.crop_and_lock.screenshot_hotkey = v;
                "crop-and-lock"
            }
            FlKillTimeout => {
                self.file_locksmith.kill_timeout_ms = v.trim().parse().ok()?;
                "file-locksmith"
            }
            FlRefreshInterval => {
                self.file_locksmith.refresh_interval_ms = v.trim().parse().ok()?;
                "file-locksmith"
            }
            NpTemplateLocation => {
                self.new_plus.template_location = v;
                "new-plus"
            }
            ZiToggleKey => {
                self.zoom_it.toggle_key = v;
                "zoom-it"
            }
            ZiDrawKey => {
                self.zoom_it.draw_toggle_key = v;
                "zoom-it"
            }
            ZiBreakKey => {
                self.zoom_it.break_timer_key = v;
                "zoom-it"
            }
            ZiLiveZoomKey => {
                self.zoom_it.live_zoom_toggle_key = v;
                "zoom-it"
            }
            ZiRecordKey => {
                self.zoom_it.record_toggle_key = v;
                "zoom-it"
            }
            ZiSnipKey => {
                self.zoom_it.snip_toggle_key = v;
                "zoom-it"
            }
            ZiSnipSaveKey => {
                self.zoom_it.snip_save_toggle_key = v;
                "zoom-it"
            }
            ZiPenColor => {
                self.zoom_it.pen_color = v;
                "zoom-it"
            }
            ZiBreakTimeout => {
                self.zoom_it.break_timeout = v.trim().parse::<u32>().ok()?.clamp(1, 99);
                "zoom-it"
            }
            ZiFrameRate => {
                self.zoom_it.record_frame_rate = v.trim().parse::<u32>().ok()?.clamp(1, 120);
                "zoom-it"
            }
            ZiScreenshotDir => {
                self.zoom_it.screenshot_save_location = v;
                "zoom-it"
            }
            ZiRecordingDir => {
                self.zoom_it.recording_save_location = v;
                "zoom-it"
            }
            PdBrightnessUp => {
                self.power_display.brightness_up_hotkey = v;
                "power-display"
            }
            PdBrightnessDown => {
                self.power_display.brightness_down_hotkey = v;
                "power-display"
            }
            PdStep => {
                self.power_display.hotkey_step = v.trim().parse::<u8>().ok()?.clamp(1, 100);
                "power-display"
            }
            GmExcludedApps => {
                self.grab_and_move.excluded_apps = split_list(&v);
                "grab-and-move"
            }
            AwcNext => {
                self.alt_window_cycle.next_window_shortcut = v;
                "alt-window-cycle"
            }
            AwcPrevious => {
                self.alt_window_cycle.previous_window_shortcut = v;
                "alt-window-cycle"
            }
            AwcTimeout => {
                self.alt_window_cycle.continue_timeout_ms = v.trim().parse().ok()?;
                "alt-window-cycle"
            }
            CnfTimeout => {
                self.command_not_found.lookup_timeout_ms = v.trim().parse().ok()?;
                "command-not-found"
            }
        };
        Some(module)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_list_trims_and_drops_empty() {
        assert_eq!(split_list(" gimp, ,Steam ,"), vec!["gimp", "Steam"]);
        assert!(split_list("").is_empty());
    }

    #[test]
    fn color_picker_round_trip_keeps_unknown_ui_fields() {
        // A field the UI has no control for must survive load + save.
        let src = "format = \"oklch\"\ncolor_history_limit = 7\nchange_cursor = true\n";
        let cfg: ColorPickerConf = toml::from_str(src).unwrap();
        let out = toml::to_string(&cfg).unwrap();
        let back: ColorPickerConf = toml::from_str(&out).unwrap();
        assert_eq!(back.color_history_limit, 7);
        assert!(back.change_cursor);
        assert_eq!(back.format, mpt_color_picker::ColorFormat::Oklch);
    }
}
