//! Visual smoke test: shows Mouse Utilities overlays under Xvfb so they can be
//! screenshotted from outside. Only runs with MPT_XVFB_SHOT=<feature>.
//! Features: crosshairs | findmymouse | highlighter | jump

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mpt_mouse_utils::config::MouseUtilsConfig;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

fn fake(conn: &RustConnection, ty: u8, detail: u8) {
    let root = conn.setup().roots[0].root;
    conn.xtest_fake_input(ty, detail, 0, root, 0, 0, 0).unwrap();
    conn.flush().unwrap();
    std::thread::sleep(Duration::from_millis(40));
}

fn keycode(conn: &RustConnection, keysym: u32) -> u8 {
    let s = conn.setup();
    let r = conn
        .get_keyboard_mapping(s.min_keycode, s.max_keycode - s.min_keycode + 1)
        .unwrap()
        .reply()
        .unwrap();
    let per = r.keysyms_per_keycode as usize;
    r.keysyms
        .chunks(per)
        .position(|k| k.contains(&keysym))
        .map(|i| s.min_keycode + i as u8)
        .unwrap()
}

#[test]
fn show_overlay_for_screenshot() {
    let Ok(feature) = std::env::var("MPT_XVFB_SHOT") else {
        return;
    };
    let (conn, _) = RustConnection::connect(None).unwrap();
    let root = conn.setup().roots[0].root;
    conn.warp_pointer(0u32, root, 0, 0, 0, 0, 800, 500).unwrap();
    conn.flush().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let cfg = MouseUtilsConfig::default();
    let h = match feature.as_str() {
        "crosshairs" => mpt_mouse_utils::crosshairs::spawn(cfg, stop.clone()),
        "highlighter" => mpt_mouse_utils::highlighter::spawn(cfg, stop.clone()),
        "findmymouse" => mpt_mouse_utils::find_my_mouse::spawn(cfg, stop.clone()),
        "jump" => mpt_mouse_utils::mouse_jump::spawn(cfg, stop.clone()),
        _ => panic!("unknown feature"),
    };
    std::thread::sleep(Duration::from_millis(1000));
    match feature.as_str() {
        "findmymouse" => {
            let ctrl = keycode(&conn, 0xffe3);
            for _ in 0..2 {
                fake(&conn, 2, ctrl);
                fake(&conn, 3, ctrl);
            }
        }
        "highlighter" => {
            fake(&conn, 4, 1); // left button down (held)
        }
        "jump" => {
            let (s, sh, d) = (
                keycode(&conn, 0xffeb),
                keycode(&conn, 0xffe1),
                keycode(&conn, 'd' as u32),
            );
            fake(&conn, 2, s);
            fake(&conn, 2, sh);
            fake(&conn, 2, d);
            fake(&conn, 3, d);
            fake(&conn, 3, sh);
            fake(&conn, 3, s);
        }
        _ => {}
    }
    // Screenshot is taken from outside during this window.
    std::thread::sleep(Duration::from_secs(6));
    if feature == "highlighter" {
        fake(&conn, 5, 1);
    }
    if feature == "jump" {
        let esc = keycode(&conn, 0xff1b);
        fake(&conn, 2, esc);
        fake(&conn, 3, esc);
    }
    stop.store(true, Ordering::Relaxed);
    let _ = h.join();
}
