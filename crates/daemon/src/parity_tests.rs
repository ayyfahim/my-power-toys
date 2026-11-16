//! (Registered as a child of hotkeys.rs so it can reach the private parsers.)
//! Parity tests for mpt-daemon against the PowerToys runner (src/runner).
//!
//! PowerToys reference:
//! - src/runner/main.cpp + powertoy_module.cpp: a disabled module's hotkeys are unregistered
//!   (PowertoyModule::update_hotkeys only registers hotkeys of enabled modules; disabling a
//!   module calls disable() and removes its hotkeys from the CentralizedHotkeys/KeyboardHook).
//! - Settings.UI.Library/HotkeySettings.cs: any virtual key can be used (PageUp, Home, '-', etc.).

use super::{hotkey_key_to_keysyms as key_to_keysyms, parse_hotkey_notation as parse_notation};
use crate::modules::ModuleRegistry;
use mpt_common::config::{DaemonConfig, ModuleEntry};

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

fn registry() -> ModuleRegistry {
    let mut r = ModuleRegistry::new();
    r.register_defaults();
    r
}

#[test]
fn registry_has_all_28_readme_modules() {
    let r = registry();
    let mut ids: Vec<&str> = r.list_modules().iter().map(|(id, _, _)| *id).collect();
    ids.sort();
    let mut want = README_MODULE_IDS.to_vec();
    want.sort();
    assert_eq!(ids, want);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn list_modules_is_in_a_stable_order() {
    // PowerToys lists modules in a fixed (alphabetical) order. HashMap iteration is
    // arbitrary, so `mpt-ctl list` changes order between daemon runs.
    let a: Vec<&str> = registry().list_modules().iter().map(|m| m.0).collect();
    let mut sorted = a.clone();
    sorted.sort();
    let by_name: Vec<&str> = {
        let r = registry();
        let mut v: Vec<(&str, &str)> = r.list_modules().iter().map(|m| (m.0, m.1)).collect();
        v.sort_by(|x, y| x.1.cmp(y.1));
        v.into_iter().map(|m| m.0).collect()
    };
    assert!(
        a == sorted || a == by_name,
        "ListModules order is neither by id nor by name: {a:?}"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn disabled_module_hotkey_is_not_registered() {
    let r = registry();
    let mut cfg = DaemonConfig::default();
    cfg.modules.insert(
        "color-picker".into(),
        ModuleEntry {
            enabled: false,
            hotkey: None,
        },
    );
    let bindings = r.hotkey_bindings(&cfg);
    let cp: Vec<_> = bindings
        .iter()
        .filter(|(id, _)| id == "color-picker" || id.starts_with("color-picker:"))
        .collect();
    assert!(
        cp.is_empty(),
        "disabled color-picker still grabs its hotkey: {cp:?}"
    );
}

#[test]
fn configured_hotkey_overrides_default() {
    let r = registry();
    let mut cfg = DaemonConfig::default();
    cfg.modules.insert(
        "color-picker".into(),
        ModuleEntry {
            enabled: true,
            hotkey: Some("Super+Alt+C".into()),
        },
    );
    let bindings = r.hotkey_bindings(&cfg);
    assert!(bindings.contains(&("color-picker".to_string(), "Super+Alt+C".to_string())));
    assert!(!bindings.contains(&("color-picker".to_string(), "Super+Shift+C".to_string())));
}

#[test]
fn every_default_binding_is_grabbable() {
    // Every default hotkey the daemon produces must map to at least one X keysym,
    // otherwise it is silently skipped at runtime.
    let r = registry();
    let bindings = r.hotkey_bindings(&DaemonConfig::default());
    let mut bad = vec![];
    for (id, raw) in &bindings {
        match parse_notation(raw) {
            Ok(hk) if !key_to_keysyms(&hk.key).is_empty() => {}
            other => bad.push(format!("{id}: {raw} -> {other:?}")),
        }
    }
    assert!(bad.is_empty(), "ungrabbable default bindings: {bad:#?}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn navigation_and_punctuation_keys_are_supported() {
    // All of these are valid PowerToys shortcut keys (VK_PRIOR, VK_NEXT, VK_HOME, VK_END,
    // VK_INSERT, VK_SNAPSHOT, VK_OEM_MINUS, VK_OEM_PLUS, F13-F24).
    let mut missing = vec![];
    for k in [
        "PageUp", "PageDown", "Home", "End", "Insert", "Print", "-", "=", "/", "`", "F13", "F24",
    ] {
        if key_to_keysyms(k).is_empty() {
            missing.push(k);
        }
    }
    assert!(
        missing.is_empty(),
        "keys not supported by daemon: {missing:?}"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn hotkey_notation_parsing() {
    let hk = parse_notation("super+shift+c").unwrap();
    assert_eq!(hk.key, "C");
    let hk = parse_notation("Hold Space").unwrap();
    assert!(hk.modifiers.is_empty());
    assert_eq!(hk.key, "Space");
    let hk = parse_notation("Ctrl+Ctrl").unwrap();
    assert!(hk.modifiers.is_empty());
    assert!(parse_notation("").is_err());
    assert!(parse_notation("Hold ").is_err());
}

#[test]
fn trigger_unknown_module_or_stopped_module_errors() {
    let mut r = registry();
    assert!(r.trigger_hotkey("no-such-module").is_err());
    assert!(r.trigger_hotkey("no-such-module:action").is_err());
    assert!(r.trigger_hotkey("").is_err());
    // Not started in this test => must report "not running".
    assert!(r.trigger_hotkey("peek").is_err());
    assert!(r.trigger_hotkey("peek:anything").is_err());
}

#[test]
fn start_unknown_module_errors_and_stop_unknown_errors() {
    let mut r = registry();
    assert!(r.start_module("bogus").is_err());
    assert!(r.stop_module("bogus").is_err());
    // ids are case-sensitive in PowerToys too (module keys in settings.json)
    assert!(r.start_module("Awake").is_err());
}
