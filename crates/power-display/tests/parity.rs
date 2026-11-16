//! Parity tests against Microsoft PowerToys Power Display (H-misc tester).
//! Oracle: src/modules/powerdisplay/PowerDisplay.Lib/Utils/{MccsCapabilitiesParser.cs,VcpNames.cs},
//! PowerDisplay.Lib.UnitTests/MccsCapabilitiesParserTests.cs,
//! settings-ui/Settings.UI.Library/PowerDisplayProperties.cs.
//! ddcutil samples are modelled on ddcutil 1.4 / 2.x output.

use mpt_power_display::backlight;
use mpt_power_display::config::{CustomVcpMapping, PowerDisplayConfig};
use mpt_power_display::ddc::{
    VcpReading, parse_capabilities_output, parse_detect, parse_mccs_capabilities, parse_terse,
};
use mpt_power_display::display::{self, ddc_features, step_value};
use mpt_power_display::vcp::{self, Feature};
use mpt_power_display::xrandr;

// ───────────────────────── ddcutil detect ─────────────────────────

/// ddcutil 2.1 `detect` (non-brief) with laptop panel, DP monitor, HDMI
/// monitor with empty serial, and a phantom display.
const DETECT_V2_FULL: &str = "\
Invalid display
   I2C bus:  /dev/i2c-4
      DRM connector:           card1-eDP-1
      EDID synopsis:
         Mfg id:               BOE - BOE
         Model:
         Product code:         2333  (0x091d)
         Serial number:
         Binary serial number: 0 (0x00000000)
         Manufacture year:     2019,  Week: 10
   This is an eDP laptop display. Laptop displays do not support DDC/CI.

Display 1
   I2C bus:  /dev/i2c-6
      DRM connector:           card1-DP-1
      EDID synopsis:
         Mfg id:               DEL - Dell Inc.
         Model:                DELL U2720Q
         Product code:         41332  (0xa174)
         Serial number:        F8KFX13
         Binary serial number: 1111111111 (0x42414c4c)
         Manufacture year:     2021,  Week: 5
   VCP version:         2.1

Display 2
   I2C bus:  /dev/i2c-9
      DRM connector:           card1-HDMI-A-1
      EDID synopsis:
         Mfg id:               GSM - Goldstar Company Ltd
         Model:                LG HDR 4K
         Product code:         30471  (0x7707)
         Serial number:
         Binary serial number: 16843009 (0x01010101)
         Manufacture year:     2020,  Week: 1
   VCP version:         2.1

Phantom display
   I2C bus:  /dev/i2c-7
      DRM connector:           card1-DP-2
      EDID synopsis:
         Mfg id:               DEL - Dell Inc.
         Model:                DELL U2720Q
";

#[test]
fn detect_v2_full_skips_invalid_and_phantom() {
    let d = parse_detect(DETECT_V2_FULL);
    assert_eq!(d.len(), 2, "{d:#?}");
    assert_eq!((d[0].display_number, d[0].bus), (1, Some(6)));
    assert_eq!(d[0].drm_connector.as_deref(), Some("card1-DP-1"));
    assert_eq!(
        (d[0].mfg.as_str(), d[0].model.as_str(), d[0].serial.as_str()),
        ("DEL", "DELL U2720Q", "F8KFX13")
    );
    assert_eq!(d[1].bus, Some(9));
    assert_eq!(d[1].model, "LG HDR 4K");
    assert_eq!(d[1].serial, "");
    assert_eq!(display::ddc_monitor_id(&d[1]), "ddc:GSM:LG HDR 4K:hdmi-1");
}

/// ddcutil 1.x `detect --brief`, plus USB-connected monitor (no I2C bus).
const DETECT_BRIEF_MIXED: &str = "\
Display 1
   I2C bus:             /dev/i2c-3
   Monitor:             AOC:2460G4:GHZG4BA000123

Display 2
   USB bus:device:      001:005
   Monitor:             APP:Apple Pro Display XDR:

No displays found
";

#[test]
fn detect_brief_mixed() {
    let d = parse_detect(DETECT_BRIEF_MIXED);
    assert_eq!(d.len(), 2);
    assert_eq!(d[0].bus, Some(3));
    assert_eq!(d[0].model, "2460G4");
    assert_eq!(d[1].bus, None);
    assert_eq!(d[1].model, "Apple Pro Display XDR");
}

#[test]
fn detect_error_texts_yield_nothing() {
    for s in [
        "",
        "No displays found.\n",
        "Unable to open directory /dev/i2c: No such file or directory\nNo /dev/i2c devices exist.\nddcutil requires module i2c-dev.\n",
        "Display 1\n",
    ] {
        let d = parse_detect(s);
        assert!(d.len() <= 1, "{s:?} -> {d:?}");
        if let Some(x) = d.first() {
            assert_eq!(x.bus, None);
        }
    }
}

// ───────────────────────── getvcp --terse ─────────────────────────

#[test]
fn getvcp_terse_realistic_multi_code() {
    // `ddcutil --bus 6 getvcp 10 12 62 14 60 D6 --terse` on a Dell U2720Q
    let out = "\
VCP 10 C 70 100
VCP 12 C 75 100
VCP 62 ERR
VCP 14 SNC x0b
VCP 60 SNC x1b
VCP D6 SNC x01
";
    let m = parse_terse(out);
    assert_eq!(
        m[&0x10],
        VcpReading::Continuous {
            current: 70,
            max: 100
        }
    );
    assert_eq!(m[&0x62], VcpReading::Unsupported);
    assert_eq!(m[&0x14], VcpReading::NonContinuous(0x0B));
    assert_eq!(m[&0x60], VcpReading::NonContinuous(0x1B));
    let (f, v) = ddc_features(None, &m, false);
    assert_eq!(v[&Feature::Brightness], 70);
    assert!(!f.contains_key(&Feature::Volume));
    assert_eq!(v[&Feature::InputSource], 0x1B);
    // current value is always offered even if not in the fallback list
    assert!(f[&Feature::InputSource].values.contains(&0x1B));
}

#[test]
fn getvcp_terse_lowercase_and_odd_max() {
    // Some monitors report max 255 or odd maxima; codes may be lowercase.
    let m = parse_terse("VCP 10 C 128 255\nVCP 62 C 7 31\nVCP d6 SNC x04\n");
    assert_eq!(
        m[&0x10],
        VcpReading::Continuous {
            current: 128,
            max: 255
        }
    );
    assert_eq!(m[&0xD6], VcpReading::NonContinuous(4));
    let (_f, v) = ddc_features(None, &m, false);
    assert_eq!(v[&Feature::Brightness], 50);
    assert_eq!(v[&Feature::Volume], 23); // 7/31 = 22.6% -> 23
}

#[test]
fn getvcp_terse_current_above_max_is_clamped() {
    let m = parse_terse("VCP 10 C 120 100\n");
    let (_f, v) = ddc_features(None, &m, false);
    assert_eq!(v[&Feature::Brightness], 100);
}

#[test]
fn getvcp_error_output_parses_to_nothing() {
    for s in [
        "Display not found\n",
        "DDC communication failed for monitor on bus /dev/i2c-6\n",
        "VCP 10 \n",
        "VCP ZZ C 1 2\n",
        "VCP 10 C x y\n",
    ] {
        assert!(parse_terse(s).is_empty(), "{s:?}");
    }
}

// ───────────────────────── capabilities ─────────────────────────

/// ddcutil 2.x `capabilities` output where an unknown feature lists its
/// values inline ("Values: 00 01 02 (interpretation unavailable)").
const CAPS_V2: &str = "\
Model: U2720Q
MCCS version: 2.1
Commands:
   Op Code: 01 (VCP Request)
   Op Code: 02 (VCP Response)
VCP Features:
   Feature: 02 (New control value)
   Feature: 10 (Brightness)
   Feature: 12 (Contrast)
   Feature: 14 (Select color preset)
      Values:
         01: sRGB
         05: 6500 K
         08: 9300 K
         0b: User 1
   Feature: 60 (Input Source)
      Values:
         0f: DisplayPort-1
         11: HDMI-1
         1b: Unrecognized value
   Feature: 62 (Audio speaker volume)
   Feature: D6 (Power mode)
      Values:
         01: DPM: On,  DPMS: Off
         04: DPM: Off, DPMS: Off
   Feature: E2 (Manufacturer specific feature)
      Values: 00 01 02 04 0e 12 14 (interpretation unavailable)
";

#[test]
fn capabilities_v2_structured() {
    let c = parse_capabilities_output(CAPS_V2);
    assert_eq!(c[&0x14], vec![0x01, 0x05, 0x08, 0x0B]);
    assert_eq!(c[&0x60], vec![0x0F, 0x11, 0x1B]);
    assert_eq!(c[&0xD6], vec![0x01, 0x04]);
    assert!(c[&0x62].is_empty());
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn capabilities_inline_values_are_parsed() {
    let c = parse_capabilities_output(CAPS_V2);
    assert_eq!(
        c.get(&0xE2).cloned().unwrap_or_default(),
        vec![0x00, 0x01, 0x02, 0x04, 0x0E, 0x12, 0x14]
    );
}

const DELL_U3011: &str = "(prot(monitor)type(lcd)model(U3011)cmds(01 02 03 07 0C E3 F3)vcp(02 04 05 06 08 10 12 14(01 05 08 0B 0C) 16 18 1A 52 60(01 03 04 0C 0F 11 12) AC AE B2 B6 C6 C8 C9 D6(01 04 05) DC(00 02 03 04 05) DF FD)mccs_ver(2.1)mswhql(1))";
const DELL_P2416D: &str = "(prot(monitor)type(LCD)model(P2416D)cmds(01 02 03 07 0C E3 F3) vcp(02 04 05 08 10 12 14(05 08 0B 0C) 16 18 1A 52 60(01 11 0F) AA(01 02) AC AE B2 B6 C6 C8 C9 D6(01 04 05) DC(00 02 03 05) DF E0 E1 E2(00 01 02 04 0E 12 14 19) F0(00 08) F1(01 02) F2 FD) mswhql(1)asset_eep(40)mccs_ver(2.1))";

#[test]
fn mccs_pt_dell_samples() {
    let c = parse_mccs_capabilities(DELL_U3011);
    assert_eq!(c[&0x14], vec![0x01, 0x05, 0x08, 0x0B, 0x0C]);
    assert_eq!(c[&0x60], vec![0x01, 0x03, 0x04, 0x0C, 0x0F, 0x11, 0x12]);
    assert!(c.contains_key(&0x10) && c.contains_key(&0x12) && !c.contains_key(&0x62));
    let c = parse_mccs_capabilities(DELL_P2416D);
    assert_eq!(c[&0x60], vec![0x01, 0x11, 0x0F]);
    assert_eq!(c[&0xE2].len(), 8);
    assert!(c.contains_key(&0xFD));
}

#[test]
fn mccs_pt_no_outer_parens_and_whitespace() {
    let c =
        parse_mccs_capabilities("prot(monitor)type(lcd)model(TestMonitor)vcp(10 12)mccs_ver(2.0)");
    assert!(c.contains_key(&0x10) && c.contains_key(&0x12));
    let c = parse_mccs_capabilities(
        "(  prot( monitor )  type( lcd )  vcp( 10   12   14( 01  05 ) )  )",
    );
    assert_eq!(c.len(), 3);
    assert_eq!(c[&0x14], vec![0x01, 0x05]);
    let c = parse_mccs_capabilities("(vcp(Aa Bb cC Dd))");
    assert_eq!(
        c.keys().copied().collect::<Vec<_>>(),
        vec![0xAA, 0xBB, 0xCC, 0xDD]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn mccs_pt_concatenated_hex() {
    // PT Parse_ConcatenatedHexFormat_ParsesCorrectly: "vcp(101214)" -> 10, 12, 14
    let c = parse_mccs_capabilities("(prot(monitor)cmds(01020307)vcp(101214)mccs_ver(2.1))");
    assert!(
        c.contains_key(&0x10) && c.contains_key(&0x12) && c.contains_key(&0x14),
        "{c:?}"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn mccs_pt_malformed_unterminated_keeps_last_code() {
    // PT Parse_MalformedInput_ReturnsPartialResults: 10 and 12
    let c = parse_mccs_capabilities("(prot(monitor)vcp(10 12");
    assert!(c.contains_key(&0x10));
    assert!(c.contains_key(&0x12), "last code lost: {c:?}");
}

#[test]
fn mccs_pt_invalid_hex_skipped() {
    let c = parse_mccs_capabilities("(vcp(10 GG 12 14))");
    assert_eq!(
        c.keys().copied().collect::<Vec<_>>(),
        vec![0x10, 0x12, 0x14]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn mccs_pt_single_char_hex_skipped() {
    // PT Parse_SingleCharacterHex_Skipped: only 10 and 12
    let c = parse_mccs_capabilities("(vcp(10 A 12))");
    assert_eq!(c.keys().copied().collect::<Vec<_>>(), vec![0x10, 0x12]);
}

#[test]
fn mccs_pt_window_segment_after_vcp_does_not_leak() {
    let c = parse_mccs_capabilities(
        "(vcp(10)window1(type(PIP) area(25 25 1895 1175) max(640 480) min(10 10) window(10)))",
    );
    assert_eq!(c.keys().copied().collect::<Vec<_>>(), vec![0x10]);
}

#[test]
fn mccs_empty_and_missing_vcp() {
    assert!(parse_mccs_capabilities("").is_empty());
    assert!(parse_mccs_capabilities("(prot(monitor)type(lcd))").is_empty());
}

// ───────────────────────── VCP names (VcpNames.cs) ─────────────────────────

#[test]
fn vcp_value_names_match_pt_tables() {
    let pt: &[(u8, &[(u16, &str)])] = &[
        (
            0x14,
            &[
                (0x01, "sRGB"),
                (0x02, "Display Native"),
                (0x03, "4000K"),
                (0x04, "5000K"),
                (0x05, "6500K"),
                (0x06, "7500K"),
                (0x07, "8200K"),
                (0x08, "9300K"),
                (0x09, "10000K"),
                (0x0A, "11500K"),
                (0x0B, "User 1"),
                (0x0C, "User 2"),
                (0x0D, "User 3"),
            ],
        ),
        (
            0x60,
            &[
                (0x01, "VGA-1"),
                (0x02, "VGA-2"),
                (0x03, "DVI-1"),
                (0x04, "DVI-2"),
                (0x05, "Composite Video 1"),
                (0x06, "Composite Video 2"),
                (0x07, "S-Video-1"),
                (0x08, "S-Video-2"),
                (0x09, "Tuner-1"),
                (0x0A, "Tuner-2"),
                (0x0B, "Tuner-3"),
                (0x0C, "Component Video 1"),
                (0x0D, "Component Video 2"),
                (0x0E, "Component Video 3"),
                (0x0F, "DisplayPort-1"),
                (0x10, "DisplayPort-2"),
                (0x11, "HDMI-1"),
                (0x12, "HDMI-2"),
                (0x1B, "USB-C"),
            ],
        ),
        (
            0xD6,
            &[
                (0x01, "On"),
                (0x02, "Standby"),
                (0x03, "Suspend"),
                (0x04, "Off (DPM)"),
                (0x05, "Off (Hard)"),
            ],
        ),
    ];
    for (code, entries) in pt {
        for (v, name) in *entries {
            assert_eq!(vcp::value_name(*code, *v), Some(*name), "{code:#x}:{v:#x}");
            assert_eq!(
                vcp::formatted_value_name(*code, *v),
                format!("{name} (0x{v:02X})")
            );
        }
    }
    assert_eq!(vcp::formatted_value_name(0x60, 0x7F), "0x7F");
}

#[test]
fn vcp_codes_match_spec() {
    assert_eq!(Feature::Brightness.vcp_code(), 0x10);
    assert_eq!(Feature::Contrast.vcp_code(), 0x12);
    assert_eq!(Feature::ColorPreset.vcp_code(), 0x14);
    assert_eq!(Feature::InputSource.vcp_code(), 0x60);
    assert_eq!(Feature::Volume.vcp_code(), 0x62);
    assert_eq!(Feature::PowerMode.vcp_code(), 0xD6);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn custom_mapping_with_empty_name_falls_back_to_builtin() {
    // PT GetValueName: `custom != null && !string.IsNullOrEmpty(custom.CustomName)`
    let mut c = PowerDisplayConfig::default();
    c.custom_vcp_mappings.push(CustomVcpMapping {
        vcp_code: 0x60,
        value: 0x11,
        custom_name: String::new(),
        apply_to_all: true,
        target_monitor_id: String::new(),
    });
    assert_eq!(c.value_name("m", 0x60, 0x11), "HDMI-1 (0x11)");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn custom_mapping_first_match_wins_like_pt() {
    // PT: FirstOrDefault(m => code/value match && (ApplyToAll || target == id))
    let mut c = PowerDisplayConfig::default();
    c.custom_vcp_mappings.push(CustomVcpMapping {
        vcp_code: 0x60,
        value: 0x11,
        custom_name: "Console".into(),
        apply_to_all: true,
        target_monitor_id: String::new(),
    });
    c.custom_vcp_mappings.push(CustomVcpMapping {
        vcp_code: 0x60,
        value: 0x11,
        custom_name: "Dock".into(),
        apply_to_all: false,
        target_monitor_id: "m2".into(),
    });
    assert_eq!(c.value_name("m2", 0x60, 0x11), "Console");
}

// ───────────────────────── scaling / steps ─────────────────────────

#[test]
fn ddc_percent_round_trip_for_common_maxima() {
    for max in [100u16, 255, 50, 31, 1000, 65535] {
        for p in 0..=100u16 {
            let raw = display::percent_to_raw(p, max);
            assert!(raw <= max);
            if max >= 100 {
                assert_eq!(display::raw_to_percent(raw, max), p, "max {max} p {p}");
            }
        }
    }
}

#[test]
fn backlight_scaling_small_and_large_max() {
    // acpi_video0 often has max 7..15; intel_backlight 96000+, amdgpu 255.
    assert_eq!(backlight::percent_to_raw(0, 15), 0);
    assert_eq!(backlight::percent_to_raw(100, 15), 15);
    assert_eq!(backlight::raw_to_percent(15, 15), 100);
    assert_eq!(backlight::raw_to_percent(1, 15), 7);
    assert_eq!(backlight::percent_to_raw(100, u32::MAX), u32::MAX);
    assert_eq!(backlight::raw_to_percent(u32::MAX, u32::MAX), 100);
    // monotonic
    let mut last = 0;
    for p in 0..=100u8 {
        let r = backlight::percent_to_raw(p, 937);
        assert!(r >= last);
        last = r;
    }
}

#[test]
fn backlight_fake_sysfs_prefers_firmware_and_skips_zero_max() {
    let dir = std::env::temp_dir().join(format!("mpt-pd-parity-bl-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, max, kind) in [
        ("acpi_video0", "15", "firmware"),
        ("intel_backlight", "96000", "raw"),
        ("dead", "0", "raw"),
        ("nvidia_0", "100", "platform"),
    ] {
        let d = dir.join(name);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("max_brightness"), max).unwrap();
        std::fs::write(d.join("type"), kind).unwrap();
    }
    std::fs::create_dir_all(dir.join("broken")).unwrap(); // no max_brightness
    let all = backlight::scan(&dir);
    assert_eq!(all.len(), 3);
    let devs = backlight::list_devices_in(&dir);
    assert_eq!(devs.len(), 1);
    assert_eq!(devs[0].name, "acpi_video0");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn backlight_missing_root_is_empty() {
    assert!(backlight::list_devices_in(std::path::Path::new("/nonexistent/backlight")).is_empty());
}

#[test]
fn hotkey_step_defaults_match_pt_wheel_increment() {
    // PT MouseWheelIncrement = 5
    let c = PowerDisplayConfig::default();
    assert_eq!(c.mouse_wheel_increment, 5);
    assert_eq!(c.hotkey_step, 5);
    assert_eq!(step_value(97, 5), 100);
    assert_eq!(step_value(2, -5), 0);
    assert_eq!(step_value(50, i16::MAX), 100);
    assert_eq!(step_value(50, i16::MIN), 0);
}

#[test]
fn defaults_match_pt_properties() {
    let c = PowerDisplayConfig::default();
    assert!(!c.restore_settings_on_startup);
    assert!(c.show_profile_switcher);
    assert!(c.show_identify_monitors_button);
    assert!(!c.max_compatibility_mode);
    assert!(!c.linked_levels_active);
}

// ───────────────────────── xrandr ─────────────────────────

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn xrandr_connected_without_mode_is_not_a_target() {
    // A monitor that is plugged in but switched off in xrandr has no CRTC;
    // `xrandr --output X --brightness` then fails. It should not be listed.
    let out = "\
Screen 0: minimum 320 x 200, current 1920 x 1080, maximum 16384 x 16384
DP-1 connected primary 1920x1080+0+0 (0x48) normal (normal left inverted right x axis y axis) 527mm x 296mm
\tBrightness: 1.0
HDMI-1 connected (normal left inverted right x axis y axis)
\tBrightness: 1.0
";
    let o = xrandr::parse_verbose(out);
    assert_eq!(
        o.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
        vec!["DP-1"]
    );
}

#[test]
fn xrandr_rotation_with_reflection() {
    let out = "DP-2 connected 1080x1920+1920+0 (0x4a) right X axis (normal left inverted right x axis y axis) 527mm x 296mm\n\tBrightness: 0.55\n";
    let o = xrandr::parse_verbose(out);
    assert_eq!(o[0].rotation, xrandr::Rotation::Right);
    assert_eq!(xrandr::multiplier_to_percent(o[0].brightness), 50);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn amdgpu_zero_based_xrandr_names_map_to_one_based_drm() {
    // xf86-video-amdgpu names outputs DisplayPort-0, HDMI-A-0 ... (0-based)
    // while DRM connectors are card0-DP-1, card0-HDMI-A-1 (1-based).
    assert!(xrandr::connectors_match("card0-DP-1", "DisplayPort-0"));
    assert!(!xrandr::connectors_match("card0-DP-1", "DisplayPort-1"));
}
