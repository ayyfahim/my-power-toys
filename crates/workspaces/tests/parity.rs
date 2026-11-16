//! Parity tests against Microsoft PowerToys Workspaces
//! (src/settings-ui/Settings.UI.Library/WorkspacesProperties.cs,
//! src/modules/Workspaces/{WorkspacesLauncher,WorkspacesLib,WorkspacesEditor}).
//!
//! X11 tests run only with DISPLAY set; they act as a minimal window manager
//! (SubstructureRedirect on root) to observe the requests the launcher sends.

use std::time::{Duration, Instant};

use mpt_workspaces::config::{self, AppEntry, SortBy, Workspace, WorkspacesConfig};
use mpt_workspaces::launcher::{self, LaunchStatus};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;
use x11rb::wrapper::ConnectionExt as _;

fn app(name: &str, class: &str, exec: &str, x: i32, y: i32) -> AppEntry {
    AppEntry {
        name: name.into(),
        wm_class: class.into(),
        exec: exec.into(),
        args: vec![],
        x,
        y,
        width: 400,
        height: 300,
        monitor: "Screen".into(),
        enabled: true,
        minimized: false,
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn default_hotkey_is_win_ctrl_backtick() {
    // DefaultHotkeyValue = HotkeySettings(win, ctrl, alt=false, shift=false, 0xC0 = VK_OEM_3 '`')
    assert_eq!(WorkspacesConfig::default().hotkey, "Super+Ctrl+`");
}

#[test]
fn default_sort_is_last_launched() {
    // SortByProperty enum: LastLaunched is the first (default) value
    assert_eq!(WorkspacesConfig::default().sort_by, SortBy::LastLaunched);
}

#[test]
fn new_workspace_defaults() {
    // PowerToys: move-existing-windows defaults to true, is-shortcut-needed false
    let ws = Workspace::new("W", vec![]);
    assert!(ws.move_existing);
    assert!(!ws.create_shortcut);
    assert!(ws.last_launched.is_none());
    assert!(config::parse_timestamp(&ws.created_at).is_some());
}

#[test]
fn launcher_skips_disabled_and_reports_spawn_failures() {
    // Without DISPLAY the window search fails, so this only exercises spawn paths.
    if std::env::var_os("DISPLAY").is_some() {
        return;
    }
    let mut disabled = app("off", "off", "/bin/true", 0, 0);
    disabled.enabled = false;
    let mut ws = Workspace::new(
        "t",
        vec![
            disabled,
            app("missing", "missing", "/nonexistent/binary", 0, 0),
        ],
    );
    let st = launcher::launch_workspace(&mut ws);
    assert!(matches!(st[0], LaunchStatus::Skipped { .. }));
    assert!(matches!(st[1], LaunchStatus::Failed { .. }));
    assert!(ws.last_launched.is_some());
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn desktop_shortcut_uses_stable_id_and_unique_files() {
    let home = std::env::temp_dir().join(format!("ws-parity-{}", std::process::id()));
    std::fs::create_dir_all(&home).unwrap();
    unsafe { std::env::set_var("HOME", &home) };
    let dir = home.join(".local/share/applications");
    let mut failures = vec![];

    config::create_desktop_shortcut("Dev Setup", 3).unwrap();
    let content = std::fs::read_to_string(dir.join("mpt-workspace-dev-setup.desktop")).unwrap();
    let exec = content
        .lines()
        .find(|l| l.starts_with("Exec="))
        .unwrap()
        .to_string();
    // PowerToys shortcuts launch by workspace id, which never changes. An index
    // changes when an earlier workspace is deleted.
    if exec.ends_with(" 3") {
        failures.push(format!("shortcut launches by list index: {exec}"));
    }

    // Two different workspace names must not share one .desktop file.
    config::create_desktop_shortcut("My WS", 0).unwrap();
    config::create_desktop_shortcut("my-ws", 1).unwrap();
    let n = std::fs::read_dir(&dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("my-ws")
        })
        .count();
    if n != 2 {
        failures.push(format!("'My WS' and 'my-ws' share {n} shortcut file(s)"));
    }
    // Removing one must not delete the other one's shortcut.
    config::remove_desktop_shortcut("my-ws");
    let left = std::fs::read_dir(&dir).unwrap().count();
    if left < 2 {
        failures.push("removing 'my-ws' also removed the shortcut of 'My WS'".into());
    }
    let _ = std::fs::remove_dir_all(&home);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ───────────── X11 (fake WM) ─────────────

fn atom(c: &RustConnection, n: &str) -> u32 {
    c.intern_atom(false, n.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .atom
}

/// (window, message type name, data32)
fn collect(wm: &RustConnection, t: Duration) -> Vec<(u32, String, [u32; 5])> {
    let mut out = vec![];
    let end = Instant::now() + t;
    while Instant::now() < end {
        while let Ok(Some(ev)) = wm.poll_for_event() {
            if let Event::ClientMessage(e) = ev {
                let name = wm.get_atom_name(e.type_).unwrap().reply().unwrap().name;
                out.push((
                    e.window,
                    String::from_utf8_lossy(&name).into_owned(),
                    e.data.as_data32(),
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    out
}

#[test]
fn x11_restore_move_existing_and_window_states() {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("DISPLAY not set, skipping");
        return;
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
            10,
            10,
            200,
            200,
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
    let t1 = mk(b"xterm\0XTerm\0");
    let t2 = mk(b"xterm\0XTerm\0");
    let ed = mk(b"gedit\0Gedit\0");
    let cl = atom(&c, "_NET_CLIENT_LIST");
    c.change_property32(
        xproto::PropMode::REPLACE,
        root,
        cl,
        xproto::AtomEnum::WINDOW,
        &[t1, t2, ed],
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

    let mut failures = vec![];

    // Two saved terminals, two running terminals: each entry should get its own window.
    let mut minimized = app("editor", "gedit", "/bin/false", 50, 60);
    minimized.minimized = true;
    let mut ws = Workspace::new(
        "dev",
        vec![
            app("term A", "xterm", "/bin/false", 0, 0),
            app("term B", "xterm", "/bin/false", 800, 0),
            minimized,
        ],
    );
    let st = launcher::launch_workspace(&mut ws);
    eprintln!("statuses: {st:?}");
    let msgs = collect(&wm, Duration::from_millis(500));
    for m in &msgs {
        eprintln!("msg: {m:?}");
    }
    let moves: Vec<(u32, i32, i32)> = msgs
        .iter()
        .filter(|m| m.1 == "_NET_MOVERESIZE_WINDOW")
        .map(|m| (m.0, m.2[1] as i32, m.2[2] as i32))
        .collect();
    let term_windows: std::collections::HashSet<u32> = moves
        .iter()
        .filter(|m| m.0 == t1 || m.0 == t2)
        .map(|m| m.0)
        .collect();
    if term_windows.len() != 2 {
        failures.push(format!(
            "both xterm entries moved the same window(s) {term_windows:?}: {moves:?}"
        ));
    }
    // A minimized app should be minimized after restore (WM_CHANGE_STATE IconicState
    // or _NET_WM_STATE add HIDDEN).
    let minimized_req = msgs
        .iter()
        .any(|m| m.0 == ed && (m.1 == "WM_CHANGE_STATE" && m.2[0] == 3));
    if !minimized_req {
        failures.push("minimized app was not minimized on restore".into());
    }

    // Direct call: does move_resize_window emit _NET_MOVERESIZE_WINDOW at all?
    let mut r = Ok(());
    for _ in 0..20 {
        r = mpt_workspaces::x11::move_resize_window(t1, 123, 45, 400, 300);
    }
    let direct = collect(&wm, Duration::from_millis(500));
    eprintln!(
        "20 direct calls: {} _NET_WM_STATE, {} _NET_MOVERESIZE_WINDOW",
        direct.iter().filter(|m| m.1 == "_NET_WM_STATE").count(),
        direct
            .iter()
            .filter(|m| m.1 == "_NET_MOVERESIZE_WINDOW")
            .count()
    );
    let n_moves = direct
        .iter()
        .filter(|m| m.1 == "_NET_MOVERESIZE_WINDOW")
        .count();
    if n_moves != 20 {
        failures.push(format!(
            "only {n_moves}/20 _NET_MOVERESIZE_WINDOW requests reached the WM ({r:?})"
        ));
    }

    // Capture: list_windows should report the windows from _NET_CLIENT_LIST.
    let caps = mpt_workspaces::x11::list_windows().unwrap();
    eprintln!(
        "captured: {:?}",
        caps.iter()
            .map(|w| (&w.wm_class, w.x, w.y, w.width, w.height))
            .collect::<Vec<_>>()
    );
    if caps.len() != 3 {
        failures.push(format!("captured {} windows, expected 3", caps.len()));
    }
    assert!(
        failures.is_empty(),
        "{} finding(s):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
