//! Parity tests for mpt-common (hotkeys + config) using Microsoft PowerToys as the oracle.
//!
//! PowerToys reference:
//! - src/settings-ui/Settings.UI.Library/HotkeySettings.cs: a hotkey is stored as
//!   booleans (Win, Ctrl, Alt, Shift) + a key code, so modifier order never matters,
//!   modifiers cannot be duplicated, and ToString() always renders Win, Ctrl, Alt, Shift, key.
//!   A hotkey without a key (Code == 0) is not a valid activation shortcut.
//! - src/settings-ui/Settings.UI.Library/SettingsUtils.cs GetSettingsOrDefault(): a missing or
//!   unparsable settings file falls back to the default settings instead of failing.

use mpt_common::config::{
    DaemonConfig, ModuleEntry, load_daemon_config, load_module_config, save_daemon_config,
    set_module_enabled,
};
use mpt_common::hotkey::{Hotkey, Modifier};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

// ── Hotkey parsing / formatting ─────────────────────────────────────────────

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn hotkey_format_uses_canonical_modifier_order() {
    // PowerToys HotkeySettings.ToString(): Win, Ctrl, Alt, Shift, key.
    let hk = Hotkey::parse("Shift+Ctrl+A").unwrap();
    assert_eq!(hk.to_string(), "Ctrl+Shift+A");
    let hk = Hotkey::parse("Shift+Alt+Ctrl+Super+K").unwrap();
    assert_eq!(hk.to_string(), "Super+Ctrl+Alt+Shift+K");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn hotkey_equality_ignores_modifier_order() {
    // Same booleans in PowerToys => same hotkey (used for conflict detection).
    assert_eq!(
        Hotkey::parse("Ctrl+Shift+A").unwrap(),
        Hotkey::parse("Shift+Ctrl+A").unwrap()
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn hotkey_duplicate_modifier_is_rejected_or_collapsed() {
    // PowerToys cannot represent Ctrl twice; it is either one Ctrl or invalid.
    match Hotkey::parse("Ctrl+Ctrl+A") {
        Err(_) => {}
        Ok(hk) => assert_eq!(hk.modifiers, vec![Modifier::Ctrl], "got {hk:?}"),
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn hotkey_trailing_plus_without_key_is_rejected() {
    // "Ctrl+" has no key: PowerToys treats a shortcut without key code as invalid.
    assert!(
        Hotkey::parse("Ctrl+").is_err(),
        "{:?}",
        Hotkey::parse("Ctrl+")
    );
    assert!(Hotkey::parse("Super+Shift+ ").is_err());
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn hotkey_plus_key_is_representable() {
    // PowerToys supports Ctrl + '+' (VK_OEM_PLUS), e.g. ZoomIt / Always on Top opacity
    // (AlwaysOnTopProperties DefaultIncreaseOpacityHotkeyValue = Win+Ctrl+0xBB '=+').
    let hk = Hotkey::parse("Ctrl++");
    assert!(hk.is_ok(), "Ctrl++ should parse as Ctrl + '+': {hk:?}");
    assert_eq!(hk.unwrap().key, "+");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn hotkey_modifier_only_is_not_a_valid_shortcut() {
    // A modifier on its own has no key code in PowerToys.
    let hk = Hotkey::parse("Ctrl");
    assert!(hk.is_err(), "modifier-only hotkey accepted as {hk:?}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn hotkey_single_letter_key_is_uppercased_in_display() {
    // PowerToys shows the key name from the virtual key code: always "T", never "t".
    assert_eq!(Hotkey::parse("super+t").unwrap().to_string(), "Super+T");
}

#[test]
fn hotkey_whitespace_and_aliases() {
    let hk = Hotkey::parse("  ctrl + alt + Delete ").unwrap();
    assert_eq!(hk.to_string(), "Ctrl+Alt+Delete");
    assert_eq!(Hotkey::parse("Win+T").unwrap().to_string(), "Super+T");
    assert_eq!(Hotkey::parse("Meta+T").unwrap().to_string(), "Super+T");
    assert_eq!(Hotkey::parse("Control+X").unwrap().to_string(), "Ctrl+X");
}

#[test]
fn hotkey_unknown_modifier_and_empty_rejected() {
    assert!(Hotkey::parse("Hyper+A").is_err());
    assert!(Hotkey::parse("").is_err());
    assert!(Hotkey::parse("   ").is_err());
    assert!(Hotkey::parse("+").is_err());
}

#[test]
fn hotkey_roundtrip_for_powertoys_defaults() {
    // Default shortcuts from Settings.UI.Library *Properties.cs, written in Rust notation.
    for s in [
        "Super+Ctrl+T",       // Always on Top
        "Super+Shift+C",      // Color Picker
        "Super+Shift+T",      // Text Extractor
        "Super+Ctrl+Shift+M", // Screen Ruler
        "Super+Shift+/",      // Shortcut Guide
        "Super+Shift+`",      // FancyZones editor
        "Super+Ctrl+`",       // Workspaces
        "Ctrl+Space",         // Peek
        "Super+Alt+Space",    // Command Palette
        "Alt+`",              // Alt Window Cycle
        "Super+Ctrl+Shift+D", // Light Switch
        "F1",
    ] {
        let hk = Hotkey::parse(s).unwrap();
        assert_eq!(hk.to_string(), s);
        assert_eq!(Hotkey::parse(&hk.to_string()).unwrap(), hk);
    }
}

#[test]
fn hotkey_serde_roundtrip() {
    let hk = Hotkey::new(vec![Modifier::Super, Modifier::Shift], "C");
    let s = toml::to_string(&hk).unwrap();
    let back: Hotkey = toml::from_str(&s).unwrap();
    assert_eq!(back, hk);
}

// ── Config load / save ──────────────────────────────────────────────────────

/// Config tests change XDG_CONFIG_HOME; serialize them.
static ENV_LOCK: Mutex<()> = Mutex::new(());

fn with_config_home<F: FnOnce(&std::path::Path)>(name: &str, f: F) {
    let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let base = std::env::temp_dir().join(format!("mpt-parity-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    // SAFETY: guarded by ENV_LOCK; only these tests touch the environment.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &base) };
    let dir = base.join("my-power-toys");
    f(&dir);
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn config_missing_file_gives_defaults() {
    with_config_home("missing", |dir| {
        assert!(!dir.exists());
        let cfg = load_daemon_config().unwrap();
        assert!(cfg.modules.is_empty());
        assert_eq!(cfg.general.theme, "system"); // PowerToys GeneralSettings.Theme = "system"
    });
}

#[test]
fn config_empty_file_gives_defaults() {
    with_config_home("empty", |dir| {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("daemon.toml"), "").unwrap();
        let cfg = load_daemon_config().unwrap();
        assert!(cfg.modules.is_empty());
        assert_eq!(cfg.general.theme, "system");
    });
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn config_corrupt_daemon_toml_falls_back_to_defaults() {
    // PowerToys SettingsUtils.GetSettingsOrDefault: unparsable file => defaults, runner keeps going.
    // mpt-daemon calls `load_daemon_config()?` in main, so an Err here = daemon refuses to start.
    with_config_home("corrupt", |dir| {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("daemon.toml"),
            "[modules.awake]\nenabled = \"yes\"\n",
        )
        .unwrap();
        let r = load_daemon_config();
        assert!(
            r.is_ok(),
            "corrupt daemon.toml is fatal: {:#}",
            r.unwrap_err()
        );
    });
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn config_garbage_daemon_toml_falls_back_to_defaults() {
    with_config_home("garbage", |dir| {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("daemon.toml"), "\u{0}\u{1}not toml [[[").unwrap();
        assert!(load_daemon_config().is_ok());
    });
}

#[test]
fn config_entry_without_enabled_defaults_to_true() {
    with_config_home("noenabled", |dir| {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("daemon.toml"),
            "[modules.peek]\nhotkey = \"Ctrl+Space\"\n",
        )
        .unwrap();
        let cfg = load_daemon_config().unwrap();
        assert!(cfg.modules["peek"].enabled);
        assert_eq!(cfg.modules["peek"].hotkey.as_deref(), Some("Ctrl+Space"));
    });
}

#[test]
fn config_save_load_roundtrip_creates_dir() {
    with_config_home("roundtrip", |dir| {
        let mut cfg = DaemonConfig::default();
        cfg.general.theme = "dark".into();
        cfg.modules.insert(
            "always-on-top".into(),
            ModuleEntry {
                enabled: false,
                hotkey: Some("Super+Ctrl+T".into()),
            },
        );
        save_daemon_config(&cfg).unwrap();
        assert!(dir.join("daemon.toml").exists());
        let back = load_daemon_config().unwrap();
        assert_eq!(back.general.theme, "dark");
        assert!(!back.modules["always-on-top"].enabled);
        assert_eq!(
            back.modules["always-on-top"].hotkey.as_deref(),
            Some("Super+Ctrl+T")
        );
    });
}

#[test]
fn set_module_enabled_preserves_hotkey_and_other_modules() {
    with_config_home("setenabled", |dir| {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join("daemon.toml"),
            "[general]\ntheme = \"dark\"\n[modules.peek]\nenabled = true\nhotkey = \"Ctrl+Space\"\n[modules.awake]\nenabled = false\n",
        )
        .unwrap();
        set_module_enabled("peek", false).unwrap();
        set_module_enabled("zoom-it", true).unwrap();
        let cfg = load_daemon_config().unwrap();
        assert!(!cfg.modules["peek"].enabled);
        assert_eq!(cfg.modules["peek"].hotkey.as_deref(), Some("Ctrl+Space"));
        assert!(!cfg.modules["awake"].enabled);
        assert!(cfg.modules["zoom-it"].enabled);
        assert_eq!(cfg.general.theme, "dark");
    });
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn set_module_enabled_on_corrupt_file_does_not_silently_discard_other_settings() {
    // A single bad value must not make the next toggle wipe every other module's
    // settings (enabled flags and custom hotkeys).
    with_config_home("corruptset", |dir| {
        std::fs::create_dir_all(dir).unwrap();
        let original = "[modules.peek]\nenabled = false\nhotkey = \"Ctrl+Alt+P\"\n[modules.awake]\nenabled = \"no\"\n";
        std::fs::write(dir.join("daemon.toml"), original).unwrap();
        let r = set_module_enabled("zoom-it", false);
        let after = std::fs::read_to_string(dir.join("daemon.toml")).unwrap();
        assert!(
            r.is_err() || after.contains("Ctrl+Alt+P"),
            "set_module_enabled returned Ok and rewrote the file without peek's hotkey:\n{after}"
        );
    });
}

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
struct DemoModuleCfg {
    #[serde(default)]
    size: u32,
}

#[test]
fn module_config_missing_gives_default() {
    with_config_home("modmissing", |_| {
        let c: DemoModuleCfg = load_module_config("demo").unwrap();
        assert_eq!(c, DemoModuleCfg::default());
    });
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/A-settings.md"]
fn module_config_corrupt_falls_back_to_default() {
    // PowerToys GetSettingsOrDefault<T> => default T for an unreadable module settings.json.
    with_config_home("modcorrupt", |dir| {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("demo.toml"), "size = \"big\"\n").unwrap();
        let r: anyhow::Result<DemoModuleCfg> = load_module_config("demo");
        assert!(
            r.is_ok(),
            "corrupt module config is an error: {:#}",
            r.unwrap_err()
        );
    });
}
