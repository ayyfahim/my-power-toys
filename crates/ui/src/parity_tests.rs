//! Parity tests for the settings UI (mpt-settings) against PowerToys Settings.UI.
//!
//! PowerToys reference:
//! - SettingsXAML/Views/ShellPage.xaml: one navigation entry per utility, including Quick Accent.
//! - SettingsXAML/Views/DashboardPage.xaml: the dashboard lists every utility and shows its
//!   *current* activation shortcut (DashboardViewModel reads it from the module settings).
//! - Settings.UI.Library/*Properties.cs: default activation shortcuts.

use crate::modules::default_module_list;
use mpt_common::hotkey::Hotkey;
use mpt_common::module::PowerModule;

const README_MODULE_IDS: [&str; 28] = [
    "always-on-top",
    "awake",
    "paste-plain",
    "color-picker",
    "hosts-editor",
    "bulk-rename",
    "image-resizer",
    "key-manager",
    "mouse-utils",
    "screen-ruler",
    "text-extractor",
    "shortcut-guide",
    "app-launcher",
    "command-palette",
    "fancy-zones",
    "peek",
    "light-switch",
    "quick-accent",
    "workspaces",
    "crop-and-lock",
    "file-locksmith",
    "environment-variables",
    "new-plus",
    "zoom-it",
    "power-display",
    "grab-and-move",
    "alt-window-cycle",
    "command-not-found",
];

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn dashboard_lists_every_module_registered_by_the_daemon() {
    let ui: Vec<String> = default_module_list().into_iter().map(|m| m.id).collect();
    let missing: Vec<&str> = README_MODULE_IDS
        .iter()
        .copied()
        .filter(|id| !ui.iter().any(|u| u == id))
        .collect();
    assert!(
        missing.is_empty(),
        "modules missing from settings UI: {missing:?} (UI has {})",
        ui.len()
    );
}

#[test]
fn dashboard_module_ids_are_unique_and_known() {
    let ui: Vec<String> = default_module_list().into_iter().map(|m| m.id).collect();
    let mut dedup = ui.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup.len(), ui.len());
    for id in &ui {
        assert!(README_MODULE_IDS.contains(&id.as_str()), "unknown id {id}");
    }
}

#[test]
fn displayed_hotkeys_parse() {
    for m in default_module_list() {
        if let Some(hk) = &m.hotkey {
            assert!(Hotkey::parse(hk).is_ok(), "{}: {hk}", m.id);
        }
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn displayed_hotkeys_match_powertoys_defaults() {
    // (id, PowerToys default from Settings.UI.Library, in Rust notation)
    let expected = [
        ("always-on-top", "Super+Ctrl+T"), // AlwaysOnTopProperties (Win+Ctrl+T)
        ("paste-plain", "Super+Shift+V"),  // AdvancedPasteProperties UI shortcut (Win+Shift+V)
        ("color-picker", "Super+Shift+C"), // ColorPickerProperties
        ("light-switch", "Super+Ctrl+Shift+D"), // LightSwitchProperties
        ("screen-ruler", "Super+Ctrl+Shift+M"), // MeasureToolProperties
        ("text-extractor", "Super+Shift+T"), // PowerOcrProperties
        ("shortcut-guide", "Super+Shift+/"), // ShortcutGuideProperties (0xBF)
        ("fancy-zones", "Super+Shift+`"),  // FZConfigProperties editor (VK_OEM_3)
        ("peek", "Ctrl+Space"),            // PeekProperties
        ("workspaces", "Super+Ctrl+`"),    // WorkspacesProperties (0xC0)
        ("command-palette", "Super+Alt+Space"), // CmdPalProperties
        ("crop-and-lock", "Super+Ctrl+Shift+T"), // CropAndLockProperties thumbnail
        ("power-display", "Super+Ctrl+Shift+P"), // PowerDisplayProperties
        ("alt-window-cycle", "Alt+`"),     // AltWindowCycleProperties
        ("app-launcher", "Alt+Space"),     // PowerLauncherProperties
    ];
    let list = default_module_list();
    let mut diffs = vec![];
    for (id, want) in expected {
        let got = list
            .iter()
            .find(|m| m.id == id)
            .and_then(|m| m.hotkey.clone());
        let ok = got
            .as_deref()
            .and_then(|g| Hotkey::parse(g).ok())
            .map(|g| {
                let w = Hotkey::parse(want).unwrap();
                let mut a = g.modifiers.clone();
                let mut b = w.modifiers.clone();
                a.sort_by_key(|m| *m as u8);
                b.sort_by_key(|m| *m as u8);
                a == b && g.key.eq_ignore_ascii_case(&w.key)
            })
            .unwrap_or(false);
        if !ok {
            diffs.push(format!("{id}: UI shows {got:?}, PowerToys default {want}"));
        }
    }
    assert!(
        diffs.is_empty(),
        "hotkey differences:\n{}",
        diffs.join("\n")
    );
}

fn same(a: &Hotkey, b: &Hotkey) -> bool {
    let mut x = a.modifiers.clone();
    let mut y = b.modifiers.clone();
    x.sort_by_key(|m| *m as u8);
    y.sort_by_key(|m| *m as u8);
    x == y && a.key.eq_ignore_ascii_case(&b.key)
}

#[test]
fn displayed_hotkeys_match_what_the_modules_actually_register() {
    // The dashboard shows a hard-coded string (modules.rs); the daemon registers
    // module.default_hotkey() (or daemon.toml). They must agree.
    let modules: Vec<Box<dyn PowerModule>> = vec![
        Box::new(mpt_always_on_top::AlwaysOnTop::new()),
        Box::new(mpt_color_picker::ColorPicker::new()),
        Box::new(mpt_screen_ruler::ScreenRuler::new()),
        Box::new(mpt_text_extractor::TextExtractor::new()),
        Box::new(mpt_fancy_zones::FancyZones::new()),
        Box::new(mpt_peek::Peek::new()),
        Box::new(mpt_light_switch::LightSwitch::new()),
        Box::new(mpt_command_palette::CommandPalette::new()),
        Box::new(mpt_paste_plain::PastePlain::new()),
        Box::new(mpt_workspaces::Workspaces::new()),
        Box::new(mpt_crop_and_lock::CropAndLock::new()),
        Box::new(mpt_zoom_it::ZoomIt::new()),
        Box::new(mpt_power_display::PowerDisplay::new()),
        Box::new(mpt_alt_window_cycle::AltWindowCycle::new()),
        Box::new(mpt_app_launcher::AppLauncher::new()),
        Box::new(mpt_awake::Awake::new()),
        Box::new(mpt_key_manager::KeyManager::new()),
        Box::new(mpt_mouse_utils::MouseUtils::new()),
    ];
    let list = default_module_list();
    let mut diffs = vec![];
    for m in &modules {
        let ui = list
            .iter()
            .find(|i| i.id == m.id())
            .and_then(|i| i.hotkey.as_deref())
            .and_then(|s| Hotkey::parse(s).ok());
        let real = m.default_hotkey();
        let agree = match (&ui, &real) {
            (None, None) => true,
            (Some(a), Some(b)) => same(a, b),
            _ => false,
        };
        if !agree {
            diffs.push(format!(
                "{}: UI {:?} vs module {:?}",
                m.id(),
                ui.map(|h| h.to_string()),
                real.map(|h| h.to_string())
            ));
        }
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn display_names_match_daemon_names() {
    // mpt-ctl list / tray use PowerModule::name(); the UI uses its own table.
    let modules: Vec<Box<dyn PowerModule>> = vec![
        Box::new(mpt_paste_plain::PastePlain::new()),
        Box::new(mpt_command_palette::CommandPalette::new()),
        Box::new(mpt_app_launcher::AppLauncher::new()),
        Box::new(mpt_mouse_utils::MouseUtils::new()),
    ];
    let list = default_module_list();
    let mut diffs = vec![];
    for m in &modules {
        let ui = list.iter().find(|i| i.id == m.id()).map(|i| i.name.clone());
        if ui.as_deref() != Some(m.name()) {
            diffs.push(format!("{}: UI {:?} vs daemon {:?}", m.id(), ui, m.name()));
        }
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}
