//! Small X11 helpers: Super key state polling (XQueryKeymap) and the WM_CLASS
//! of the active window (for `disabled_apps`).

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;

const XK_SUPER_L: u32 = 0xffeb;
const XK_SUPER_R: u32 = 0xffec;

/// A connection that can answer "is Super held, and is anything else held?".
pub struct KeyState {
    conn: RustConnection,
    root: xproto::Window,
    super_keycodes: Vec<u8>,
}

/// Snapshot of the keyboard/pointer state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeySnapshot {
    pub super_down: bool,
    /// Any non-Super key or mouse button is pressed.
    pub other_down: bool,
}

impl KeyState {
    pub fn connect() -> Result<Self> {
        let (conn, screen_num) =
            RustConnection::connect(None).context("failed to connect to X11")?;
        let root = conn.setup().roots[screen_num].root;
        let super_keycodes = keycodes_for(&conn, &[XK_SUPER_L, XK_SUPER_R])?;
        if super_keycodes.is_empty() {
            anyhow::bail!("no keycode is mapped to Super_L/Super_R");
        }
        Ok(Self {
            conn,
            root,
            super_keycodes,
        })
    }

    pub fn snapshot(&self) -> Result<KeySnapshot> {
        let keys = self
            .conn
            .query_keymap()
            .context("QueryKeymap request failed")?
            .reply()
            .context("QueryKeymap reply failed")?
            .keys;

        let pointer = self
            .conn
            .query_pointer(self.root)
            .context("QueryPointer request failed")?
            .reply()
            .context("QueryPointer reply failed")?;
        let buttons =
            xproto::KeyButMask::BUTTON1 | xproto::KeyButMask::BUTTON2 | xproto::KeyButMask::BUTTON3;
        let button_down = u16::from(pointer.mask) & u16::from(buttons) != 0;

        Ok(classify_keymap(&keys, &self.super_keycodes, button_down))
    }
}

/// Pure classification of a 256-bit XQueryKeymap vector.
pub fn classify_keymap(keys: &[u8; 32], super_keycodes: &[u8], button_down: bool) -> KeySnapshot {
    let mut super_down = false;
    let mut other_down = button_down;
    for (byte_idx, byte) in keys.iter().enumerate() {
        if *byte == 0 {
            continue;
        }
        for bit in 0..8 {
            if byte & (1 << bit) == 0 {
                continue;
            }
            let keycode = (byte_idx * 8 + bit) as u8;
            if super_keycodes.contains(&keycode) {
                super_down = true;
            } else {
                other_down = true;
            }
        }
    }
    KeySnapshot {
        super_down,
        other_down,
    }
}

fn keycodes_for(conn: &RustConnection, keysyms: &[u32]) -> Result<Vec<u8>> {
    let setup = conn.setup();
    let min = setup.min_keycode;
    let max = setup.max_keycode;
    let count = max - min + 1;
    let mapping = conn
        .get_keyboard_mapping(min, count)
        .context("GetKeyboardMapping request failed")?
        .reply()
        .context("GetKeyboardMapping reply failed")?;
    let per = mapping.keysyms_per_keycode as usize;
    if per == 0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (i, syms) in mapping.keysyms.chunks(per).enumerate() {
        if syms.iter().any(|s| keysyms.contains(s)) {
            out.push(min + i as u8);
        }
    }
    Ok(out)
}

/// WM_CLASS (instance and class) of the currently active window.
pub fn active_window_classes() -> Result<Vec<String>> {
    let (conn, screen_num) = RustConnection::connect(None).context("failed to connect to X11")?;
    let root = conn.setup().roots[screen_num].root;
    let active_atom = conn
        .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
        .reply()?
        .atom;
    let reply = conn
        .get_property(false, root, active_atom, xproto::AtomEnum::WINDOW, 0, 1)?
        .reply()?;
    let Some(window) = reply.value32().and_then(|mut v| v.next()) else {
        return Ok(Vec::new());
    };
    if window == 0 {
        return Ok(Vec::new());
    }
    let reply = conn
        .get_property(
            false,
            window,
            xproto::AtomEnum::WM_CLASS,
            xproto::AtomEnum::STRING,
            0,
            256,
        )?
        .reply()?;
    Ok(parse_wm_class(&reply.value))
}

/// WM_CLASS is `instance\0class\0`.
pub fn parse_wm_class(raw: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(raw)
        .split('\0')
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_super_only() {
        let mut keys = [0u8; 32];
        keys[133 / 8] |= 1 << (133 % 8);
        let s = classify_keymap(&keys, &[133, 134], false);
        assert!(s.super_down);
        assert!(!s.other_down);
    }

    #[test]
    fn classify_super_plus_other() {
        let mut keys = [0u8; 32];
        keys[133 / 8] |= 1 << (133 % 8);
        keys[38 / 8] |= 1 << (38 % 8);
        let s = classify_keymap(&keys, &[133], false);
        assert!(s.super_down);
        assert!(s.other_down);
    }

    #[test]
    fn classify_button_counts_as_other() {
        let keys = [0u8; 32];
        let s = classify_keymap(&keys, &[133], true);
        assert!(!s.super_down);
        assert!(s.other_down);
    }

    #[test]
    fn wm_class_parsing() {
        assert_eq!(
            parse_wm_class(b"steam\0Steam\0"),
            vec!["steam".to_string(), "Steam".to_string()]
        );
        assert!(parse_wm_class(b"").is_empty());
    }
}
