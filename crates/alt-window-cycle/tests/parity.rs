//! Parity tests against Microsoft PowerToys Alt Window Cycle
//! (src/modules/AltWindowCycle/AltWindowCycleLogic.h, AltWindowCycle.cpp,
//! src/settings-ui/Settings.UI.Library/AltWindowCycleProperties.cs).

use std::time::{Duration, Instant};

use mpt_alt_window_cycle::AltWindowCycleConfig;
use mpt_alt_window_cycle::cycle::{Direction, pick, plan_cycle, wrap_index};
use mpt_common::module::PowerModule;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

fn pt_wrap(index: i32, count: i32) -> i32 {
    if count <= 0 {
        return 0;
    }
    ((index % count) + count) % count
}

/// AltWindowCycleLogic::BeginCycle -> selected index (None == Ignore)
fn pt_begin(current: i32, count: i32, forward: bool) -> Option<i32> {
    if count < 2 {
        return None;
    }
    let c = current.max(0);
    Some(pt_wrap(if forward { c + 1 } else { c - 1 }, count))
}

#[test]
fn default_shortcuts_match() {
    let c = AltWindowCycleConfig::default();
    assert_eq!(c.next_window_shortcut, "Alt+`");
    assert_eq!(c.previous_window_shortcut, "Alt+Shift+`");
}

#[test]
fn wrap_index_matches() {
    for count in 0..6 {
        for i in -13..13 {
            assert_eq!(
                wrap_index(i as isize, count as usize) as i32,
                pt_wrap(i, count),
                "i={i} n={count}"
            );
        }
    }
}

#[test]
fn begin_cycle_matches() {
    for n in 0..6usize {
        let order: Vec<u32> = (100..100 + n as u32).collect();
        for cur in -1..n as i32 {
            let current = if cur < 0 { 999 } else { order[cur as usize] };
            for fwd in [true, false] {
                let dir = if fwd {
                    Direction::Forward
                } else {
                    Direction::Backward
                };
                let got = pick(&order, current, dir).map(|i| i as i32);
                assert_eq!(
                    got,
                    pt_begin(cur, n as i32, fwd),
                    "n={n} cur={cur} fwd={fwd}"
                );
            }
        }
    }
}

#[test]
fn fresh_cycle_goes_to_second_most_recent() {
    // PowerToys lists the app's windows in z-order; first press selects index 1.
    let (order, idx) = plan_cycle(None, &[7, 8, 9], 7, Direction::Forward).unwrap();
    assert_eq!(order[idx], 8);
    let (order, idx) = plan_cycle(None, &[7, 8, 9], 7, Direction::Backward).unwrap();
    assert_eq!(order[idx], 9);
}

// ───────────── X11 with a fake WM ─────────────

fn atom(c: &RustConnection, n: &str) -> u32 {
    c.intern_atom(false, n.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .atom
}

#[test]
fn x11_cycles_within_same_app_in_mru_order() {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("DISPLAY not set, skipping");
        return;
    }
    let cfg_home = std::env::temp_dir().join(format!("awc-parity-{}", std::process::id()));
    std::fs::create_dir_all(cfg_home.join("my-power-toys")).unwrap();
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &cfg_home);
        std::env::remove_var("WAYLAND_DISPLAY");
    }
    let (c, sn) = RustConnection::connect(None).unwrap();
    let screen = c.setup().roots[sn].clone();
    let root = screen.root;
    let mk = |class: &[u8]| {
        let w = c.generate_id().unwrap();
        c.create_window(
            screen.root_depth,
            w,
            root,
            0,
            0,
            100,
            100,
            0,
            xproto::WindowClass::INPUT_OUTPUT,
            0,
            &xproto::CreateWindowAux::new(),
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
        w
    };
    let a1 = mk(b"xterm\0XTerm\0");
    let other = mk(b"gedit\0Gedit\0");
    let a2 = mk(b"xterm\0XTerm\0");
    let a3 = mk(b"xterm\0XTerm\0");
    // Stacking bottom -> top: a3, a2, other, a1  => MRU: a1, other, a2, a3
    let stacking = atom(&c, "_NET_CLIENT_LIST_STACKING");
    c.change_property32(
        xproto::PropMode::REPLACE,
        root,
        stacking,
        xproto::AtomEnum::WINDOW,
        &[a3, a2, other, a1],
    )
    .unwrap();
    let cl = atom(&c, "_NET_CLIENT_LIST");
    c.change_property32(
        xproto::PropMode::REPLACE,
        root,
        cl,
        xproto::AtomEnum::WINDOW,
        &[a1, other, a2, a3],
    )
    .unwrap();
    let active = atom(&c, "_NET_ACTIVE_WINDOW");
    c.change_property32(
        xproto::PropMode::REPLACE,
        root,
        active,
        xproto::AtomEnum::WINDOW,
        &[a1],
    )
    .unwrap();
    c.flush().unwrap();

    let (wm, _) = RustConnection::connect(None).unwrap();
    wm.change_window_attributes(
        root,
        &xproto::ChangeWindowAttributesAux::new().event_mask(
            xproto::EventMask::SUBSTRUCTURE_REDIRECT | xproto::EventMask::SUBSTRUCTURE_NOTIFY,
        ),
    )
    .unwrap();
    wm.flush().unwrap();

    let activations = |wm: &RustConnection| {
        let mut out = vec![];
        let end = Instant::now() + Duration::from_millis(400);
        while Instant::now() < end {
            while let Ok(Some(ev)) = wm.poll_for_event() {
                if let Event::ClientMessage(e) = ev
                    && e.type_ == active
                {
                    out.push(e.window);
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        out
    };

    let mut m = mpt_alt_window_cycle::AltWindowCycle::new();
    m.start().unwrap();
    m.on_hotkey().unwrap();
    let first = activations(&wm);
    eprintln!("first press activated {first:?} (a1={a1} a2={a2} a3={a3} other={other})");
    m.on_named_action("reverse").unwrap();
    let back = activations(&wm);
    eprintln!("reverse press activated {back:?}");
    let _ = std::fs::remove_dir_all(&cfg_home);
    assert_eq!(
        first,
        vec![a2],
        "Alt+` from a1 should activate the next xterm window a2"
    );
    assert!(!first.contains(&other));
}
