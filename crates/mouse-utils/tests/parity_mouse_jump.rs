//! Mouse Jump end-to-end parity tests (need a real X server).
//!
//! Run under Xvfb with a fake `xrandr` on PATH that prints the layout in
//! `MPT_FAKE_XRANDR`, e.g.:
//!   MPT_XVFB_TEST=1 withx.sh cargo test -p mpt-mouse-utils --test parity_mouse_jump
//!
//! Oracle: PowerToys MouseJump.Common `MouseHelper.GetJumpLocation`
//! (previewLocation * desktopSize / previewSize + desktopOrigin, see
//! MouseHelperTests.cs) and `LayoutHelper.GetFormLayout`, whose
//! `maximumSize` is the activated screen size (the preview never exceeds the
//! screen it is shown on).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mpt_mouse_utils::config::MouseUtilsConfig;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{ConnectionExt, MapState};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

fn enabled() -> bool {
    std::env::var("MPT_XVFB_TEST").is_ok() && std::env::var("DISPLAY").is_ok()
}

fn keycode(conn: &RustConnection, keysym: u32) -> u8 {
    let s = conn.setup();
    let (min, max) = (s.min_keycode, s.max_keycode);
    let r = conn
        .get_keyboard_mapping(min, max - min + 1)
        .unwrap()
        .reply()
        .unwrap();
    let per = r.keysyms_per_keycode as usize;
    for (i, ks) in r.keysyms.chunks(per).enumerate() {
        if ks.contains(&keysym) {
            return min + i as u8;
        }
    }
    panic!("no keycode for keysym {keysym:#x}");
}

fn fake(conn: &RustConnection, ty: u8, detail: u8, x: i16, y: i16) {
    let root = conn.setup().roots[0].root;
    conn.xtest_fake_input(ty, detail, 0, root, x, y, 0).unwrap();
    conn.flush().unwrap();
    std::thread::sleep(Duration::from_millis(30));
}

/// Find the mapped override-redirect popup (geometry x, y, w, h).
fn find_popup(conn: &RustConnection) -> Option<(i32, i32, i32, i32)> {
    let root = conn.setup().roots[0].root;
    let tree = conn.query_tree(root).ok()?.reply().ok()?;
    for w in tree.children {
        let attr = conn.get_window_attributes(w).ok()?.reply().ok()?;
        if attr.override_redirect && attr.map_state == MapState::VIEWABLE {
            let g = conn.get_geometry(w).ok()?.reply().ok()?;
            return Some((g.x as i32, g.y as i32, g.width as i32, g.height as i32));
        }
    }
    None
}

/// Open Mouse Jump with the cursor at `start`, click preview point `p`
/// (relative to the screen-image area, padding excluded) and return the
/// resulting pointer position plus the popup geometry.
fn jump(
    cfg: MouseUtilsConfig,
    start: (i16, i16),
    preview_w: i32,
    p: (i32, i32),
) -> ((i32, i32), (i32, i32, i32, i32)) {
    let (conn, _) = RustConnection::connect(None).unwrap();
    let root = conn.setup().roots[0].root;
    let stop = Arc::new(AtomicBool::new(false));
    let h = mpt_mouse_utils::mouse_jump::spawn(cfg, stop.clone());
    std::thread::sleep(Duration::from_millis(800));

    conn.warp_pointer(0u32, root, 0, 0, 0, 0, start.0, start.1)
        .unwrap();
    conn.flush().unwrap();
    let sup = keycode(&conn, 0xffeb); // Super_L
    let shift = keycode(&conn, 0xffe1); // Shift_L
    let d = keycode(&conn, 'd' as u32);
    fake(&conn, 2, sup, 0, 0);
    fake(&conn, 2, shift, 0, 0);
    fake(&conn, 2, d, 0, 0);
    fake(&conn, 3, d, 0, 0);
    fake(&conn, 3, shift, 0, 0);
    fake(&conn, 3, sup, 0, 0);

    let mut popup = None;
    for _ in 0..50 {
        std::thread::sleep(Duration::from_millis(100));
        popup = find_popup(&conn);
        if popup.is_some() {
            break;
        }
    }
    let popup = popup.expect("Mouse Jump popup did not appear");
    let pad = (popup.2 - preview_w) / 2;
    let (cx, cy) = (popup.0 + pad + p.0, popup.1 + pad + p.1);
    fake(&conn, 6, 0, cx as i16, cy as i16); // motion
    fake(&conn, 4, 1, 0, 0);
    fake(&conn, 5, 1, 0, 0);
    std::thread::sleep(Duration::from_millis(400));
    let ptr = conn.query_pointer(root).unwrap().reply().unwrap();
    stop.store(true, Ordering::Relaxed);
    let _ = h.join();
    ((ptr.root_x as i32, ptr.root_y as i32), popup)
}

fn cfg(w: u32, h: u32) -> MouseUtilsConfig {
    MouseUtilsConfig {
        mouse_jump: true,
        mouse_jump_max_width: w,
        mouse_jump_max_height: h,
        ..Default::default()
    }
}

/// Single 1600x1000 screen (fake xrandr), max 400x300 -> scale 0.25, preview 400x250.
#[test]
fn mouse_jump_single_monitor_mapping() {
    if !enabled() {
        return;
    }
    let layout = std::env::var("MPT_FAKE_XRANDR").unwrap_or_default();
    if !layout.contains("single") {
        return;
    }
    // Preview point (100, 50) -> desktop (400, 200) (cursor starts at 800,500).
    let (p, popup) = jump(cfg(400, 300), (800, 500), 400, (100, 50));
    eprintln!("popup={popup:?} pointer={p:?}");
    assert!(
        (p.0 - 400).abs() <= 4 && (p.1 - 200).abs() <= 4,
        "got {p:?}"
    );
}

/// Two monitors of different height: 800x1000+0+0 and 800x500+800+0.
#[test]
fn mouse_jump_two_monitor_mapping() {
    if !enabled() {
        return;
    }
    let layout = std::env::var("MPT_FAKE_XRANDR").unwrap_or_default();
    if !layout.contains("dual") {
        return;
    }
    // Point (300, 50) in a 400x250 preview of a 1600x1000 desktop -> (1200, 200),
    // which lies on the second monitor.
    let (p, popup) = jump(cfg(400, 300), (400, 500), 400, (300, 50));
    eprintln!("popup={popup:?} pointer={p:?}");
    assert!(
        (p.0 - 1200).abs() <= 4 && (p.1 - 200).abs() <= 4,
        "got {p:?}"
    );
}

/// PowerToys default thumbnail size is 1600x1200 and the preview is limited
/// to the activated screen; the popup must fit on the 1600x1000 screen.
#[test]
fn mouse_jump_preview_fits_on_screen_with_upstream_default_size() {
    if !enabled() {
        return;
    }
    let layout = std::env::var("MPT_FAKE_XRANDR").unwrap_or_default();
    if !layout.contains("single") {
        return;
    }
    let (_, popup) = jump(cfg(1600, 1200), (800, 500), 1600, (10, 10));
    eprintln!("popup={popup:?}");
    assert!(
        popup.0 >= 0 && popup.1 >= 0 && popup.0 + popup.2 <= 1600 && popup.1 + popup.3 <= 1000,
        "popup {popup:?} does not fit on the 1600x1000 screen"
    );
}
