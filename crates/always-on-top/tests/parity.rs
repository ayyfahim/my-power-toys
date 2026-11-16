//! Parity tests against Microsoft PowerToys Always on Top
//! (src/settings-ui/Settings.UI.Library/AlwaysOnTopProperties.cs,
//! src/modules/alwaysontop/AlwaysOnTop/{AlwaysOnTop.cpp,Settings.h}).
//!
//! The X11 test runs only when DISPLAY is set (e.g. under Xvfb). It acts as a
//! minimal window manager (SubstructureRedirect on root) to observe the
//! `_NET_WM_STATE` client messages the module sends.

use std::time::{Duration, Instant};

use mpt_always_on_top::AlwaysOnTop;
use mpt_always_on_top::config::{AlwaysOnTopConfig, step_opacity};
use mpt_common::module::PowerModule;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

// ───────────── Defaults (AlwaysOnTopProperties.cs) ─────────────

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn default_hotkey_is_win_ctrl_t() {
    // DefaultHotkeyValue = HotkeySettings(win, ctrl, alt=false, shift=false, 'T')
    assert_eq!(AlwaysOnTopConfig::default().hotkey, "Super+Ctrl+T");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn default_opacity_hotkeys_are_win_ctrl_plus_minus() {
    // 0xBB = VK_OEM_PLUS, 0xBD = VK_OEM_MINUS, with Win+Ctrl only (no Shift)
    let c = AlwaysOnTopConfig::default();
    assert_eq!(c.increase_opacity_hotkey, "Super+Ctrl+=");
    assert_eq!(c.decrease_opacity_hotkey, "Super+Ctrl+-");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn default_frame_thickness_is_4() {
    assert_eq!(AlwaysOnTopConfig::default().border_thickness, 4);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn default_frame_color_is_0099cc() {
    assert_eq!(
        AlwaysOnTopConfig::default().border_color.to_lowercase(),
        "#0099cc"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn default_frame_opacity_is_100_percent() {
    assert_eq!(AlwaysOnTopConfig::default().border_opacity, 1.0);
}

#[test]
fn default_flags_match() {
    let c = AlwaysOnTopConfig::default();
    assert!(c.show_border, "frame-enabled");
    assert!(c.frame_accent_color, "frame-accent-color");
    assert!(c.play_sound, "sound-enabled");
    assert!(
        c.do_not_activate_on_game_mode,
        "do-not-activate-on-game-mode"
    );
    assert!(c.round_corners_enabled, "round-corners-enabled");
    assert!(c.excluded_apps.is_empty(), "excluded-apps");
}

#[test]
fn opacity_steps_match_settings_h() {
    // minTransparencyPercentage 20, max 100, step 10
    let mut v = 1.0f32;
    let mut seen = vec![];
    for _ in 0..12 {
        v = step_opacity(v, false);
        seen.push((v * 100.0).round() as i32);
    }
    assert_eq!(seen, vec![90, 80, 70, 60, 50, 40, 30, 20, 20, 20, 20, 20]);
    for _ in 0..12 {
        v = step_opacity(v, true);
    }
    assert_eq!(v, 1.0);
}

// ───────────── X11 behaviour ─────────────

fn atom(c: &RustConnection, n: &str) -> u32 {
    c.intern_atom(false, n.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .atom
}

/// Collect `_NET_WM_STATE` client messages for `timeout`: (window, action, prop1).
fn collect_state_msgs(wm: &RustConnection, timeout: Duration) -> Vec<(u32, u32, u32)> {
    let net_wm_state = atom(wm, "_NET_WM_STATE");
    let mut out = vec![];
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        while let Ok(Some(ev)) = wm.poll_for_event() {
            if let Event::ClientMessage(e) = ev
                && e.type_ == net_wm_state
            {
                let d = e.data.as_data32();
                out.push((e.window, d[0], d[1]));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    out
}

fn override_redirect_children(
    c: &RustConnection,
    root: u32,
    except: &[u32],
) -> Vec<(u32, i16, i16, u16, u16)> {
    let tree = c.query_tree(root).unwrap().reply().unwrap();
    tree.children
        .into_iter()
        .filter(|w| !except.contains(w))
        .filter_map(|w| {
            let a = c.get_window_attributes(w).ok()?.reply().ok()?;
            if !a.override_redirect || a.map_state != xproto::MapState::VIEWABLE {
                return None;
            }
            let g = c.get_geometry(w).ok()?.reply().ok()?;
            Some((w, g.x, g.y, g.width, g.height))
        })
        .collect()
}

fn create_client(c: &RustConnection, root: u32, depth: u8, x: i16, y: i16, class: &[u8]) -> u32 {
    let w = c.generate_id().unwrap();
    c.create_window(
        depth,
        w,
        root,
        x,
        y,
        400,
        300,
        0,
        xproto::WindowClass::INPUT_OUTPUT,
        0,
        &xproto::CreateWindowAux::new().background_pixel(0xffffff),
    )
    .unwrap();
    c.change_property8(
        xproto::PropMode::REPLACE,
        w,
        xproto::AtomEnum::WM_CLASS,
        xproto::AtomEnum::STRING,
        class,
    )
    .unwrap();
    c.map_window(w).unwrap();
    c.flush().unwrap();
    w
}

fn set_active(c: &RustConnection, root: u32, w: u32) {
    let a = atom(c, "_NET_ACTIVE_WINDOW");
    c.change_property32(
        xproto::PropMode::REPLACE,
        root,
        a,
        xproto::AtomEnum::WINDOW,
        &[w],
    )
    .unwrap();
    c.flush().unwrap();
}

/// Everything X11-related runs in one test so the environment is not shared
/// between parallel tests.
#[test]
fn x11_pin_border_exclusion_and_cleanup() {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("DISPLAY not set, skipping");
        return;
    }
    let cfg_home = std::env::temp_dir().join(format!("aot-parity-{}", std::process::id()));
    let cfg_dir = cfg_home.join("my-power-toys");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    // Exclude "Firefox": it matches the WM_CLASS *class*, not the instance.
    std::fs::write(
        cfg_dir.join("always-on-top.toml"),
        "excluded_apps = [\"Firefox\"]\nplay_sound = false\n",
    )
    .unwrap();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &cfg_home);
        std::env::remove_var("WAYLAND_DISPLAY");
    }

    let (c, sn) = RustConnection::connect(None).unwrap();
    let screen = c.setup().roots[sn].clone();
    let root = screen.root;
    let win = create_client(&c, root, screen.root_depth, 100, 100, b"testapp\0TestApp\0");
    let ff = create_client(
        &c,
        root,
        screen.root_depth,
        600,
        100,
        b"Navigator\0Firefox\0",
    );

    // Become the "window manager" to receive _NET_WM_STATE requests.
    let (wm, _) = RustConnection::connect(None).unwrap();
    wm.change_window_attributes(
        root,
        &xproto::ChangeWindowAttributesAux::new().event_mask(
            xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        ),
    )
    .unwrap();
    wm.flush().unwrap();
    let above = atom(&wm, "_NET_WM_STATE_ABOVE");

    let mut m = AlwaysOnTop::new();
    m.start().unwrap();
    let mut failures: Vec<String> = vec![];

    // 1. Pin the active window.
    set_active(&c, root, win);
    m.on_hotkey().unwrap();
    let msgs = collect_state_msgs(&wm, Duration::from_millis(600));
    eprintln!("pin messages: {msgs:?}");
    if !msgs.contains(&(win, 1, above)) {
        failures.push(format!(
            "pin: expected _NET_WM_STATE ADD ABOVE for {win}, got {msgs:?}"
        ));
    }
    let borders = override_redirect_children(&c, root, &[win, ff]);
    eprintln!("border windows: {borders:?}");
    match borders.first() {
        None => failures.push("pin: no border window created".into()),
        Some(&(_, x, y, w, h)) => {
            // PowerToys default frame thickness is 4 px.
            if (x, y, w, h) != (96, 96, 408, 308) {
                failures.push(format!(
                    "border geometry {:?} != PT thickness-4 frame (96,96,408,308)",
                    (x, y, w, h)
                ));
            }
        }
    }

    // 2. Excluded by WM_CLASS class name ("Firefox").
    set_active(&c, root, ff);
    m.on_hotkey().unwrap();
    let msgs = collect_state_msgs(&wm, Duration::from_millis(400));
    eprintln!("excluded-app messages: {msgs:?}");
    if msgs.iter().any(|&(w, _, _)| w == ff) {
        failures.push(format!(
            "excluded app 'Firefox' (WM_CLASS class) was pinned: {msgs:?}"
        ));
        // undo so cleanup check below only concerns `win`
        m.on_hotkey().unwrap();
        let _ = collect_state_msgs(&wm, Duration::from_millis(300));
    }

    // 3. Disabling the module unpins every pinned window (PowerToys CleanUp -> UnpinAll).
    m.stop().unwrap();
    let msgs = collect_state_msgs(&wm, Duration::from_millis(600));
    eprintln!("stop messages: {msgs:?}");
    if !msgs.contains(&(win, 0, above)) {
        failures.push(format!(
            "stop: expected REMOVE ABOVE for pinned {win}, got {msgs:?}"
        ));
    }
    std::thread::sleep(Duration::from_millis(300));
    let left = override_redirect_children(&c, root, &[win, ff]);
    if !left.is_empty() {
        failures.push(format!("stop: border windows left behind: {left:?}"));
    }

    // 4. No active window and focus = PointerRoot: nothing should be targeted.
    let net_active = atom(&c, "_NET_ACTIVE_WINDOW");
    c.delete_property(root, net_active).unwrap();
    c.set_input_focus(xproto::InputFocus::POINTER_ROOT, 1u32, x11rb::CURRENT_TIME)
        .unwrap();
    c.flush().unwrap();
    let mut m2 = AlwaysOnTop::new();
    let r = m2.on_hotkey();
    let msgs = collect_state_msgs(&wm, Duration::from_millis(400));
    eprintln!("pointer-root result: {r:?}, messages: {msgs:?}");
    if r.is_ok() && !msgs.is_empty() {
        failures.push(format!(
            "PointerRoot focus treated as window id 1: {msgs:?}"
        ));
    }

    let _ = std::fs::remove_dir_all(&cfg_home);
    assert!(
        failures.is_empty(),
        "{} finding(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
