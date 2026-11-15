//! US-QWERTY character → evdev keycode table used for `send_text`.
//!
//! The virtual keyboard emits raw keycodes, and the desktop translates them
//! through the active XKB layout. Typed text is therefore only correct when
//! the session layout is US (or a layout that agrees on the used keys).

use evdev::Key;

/// Map an ASCII character to `(keycode, needs_shift)` on a US layout.
/// Returns `None` for characters that cannot be typed (non-ASCII, control).
pub fn char_to_key(c: char) -> Option<(u16, bool)> {
    let plain = |k: Key| Some((k.code(), false));
    let shifted = |k: Key| Some((k.code(), true));
    if c.is_ascii_lowercase() {
        return letter(c).map(|k| (k.code(), false));
    }
    if c.is_ascii_uppercase() {
        return letter(c.to_ascii_lowercase()).map(|k| (k.code(), true));
    }
    match c {
        '1' => plain(Key::KEY_1),
        '2' => plain(Key::KEY_2),
        '3' => plain(Key::KEY_3),
        '4' => plain(Key::KEY_4),
        '5' => plain(Key::KEY_5),
        '6' => plain(Key::KEY_6),
        '7' => plain(Key::KEY_7),
        '8' => plain(Key::KEY_8),
        '9' => plain(Key::KEY_9),
        '0' => plain(Key::KEY_0),
        '!' => shifted(Key::KEY_1),
        '@' => shifted(Key::KEY_2),
        '#' => shifted(Key::KEY_3),
        '$' => shifted(Key::KEY_4),
        '%' => shifted(Key::KEY_5),
        '^' => shifted(Key::KEY_6),
        '&' => shifted(Key::KEY_7),
        '*' => shifted(Key::KEY_8),
        '(' => shifted(Key::KEY_9),
        ')' => shifted(Key::KEY_0),
        ' ' => plain(Key::KEY_SPACE),
        '\n' => plain(Key::KEY_ENTER),
        '\t' => plain(Key::KEY_TAB),
        '-' => plain(Key::KEY_MINUS),
        '_' => shifted(Key::KEY_MINUS),
        '=' => plain(Key::KEY_EQUAL),
        '+' => shifted(Key::KEY_EQUAL),
        '[' => plain(Key::KEY_LEFTBRACE),
        '{' => shifted(Key::KEY_LEFTBRACE),
        ']' => plain(Key::KEY_RIGHTBRACE),
        '}' => shifted(Key::KEY_RIGHTBRACE),
        '\\' => plain(Key::KEY_BACKSLASH),
        '|' => shifted(Key::KEY_BACKSLASH),
        ';' => plain(Key::KEY_SEMICOLON),
        ':' => shifted(Key::KEY_SEMICOLON),
        '\'' => plain(Key::KEY_APOSTROPHE),
        '"' => shifted(Key::KEY_APOSTROPHE),
        '`' => plain(Key::KEY_GRAVE),
        '~' => shifted(Key::KEY_GRAVE),
        ',' => plain(Key::KEY_COMMA),
        '<' => shifted(Key::KEY_COMMA),
        '.' => plain(Key::KEY_DOT),
        '>' => shifted(Key::KEY_DOT),
        '/' => plain(Key::KEY_SLASH),
        '?' => shifted(Key::KEY_SLASH),
        _ => None,
    }
}

fn letter(c: char) -> Option<Key> {
    const LETTERS: [Key; 26] = [
        Key::KEY_A,
        Key::KEY_B,
        Key::KEY_C,
        Key::KEY_D,
        Key::KEY_E,
        Key::KEY_F,
        Key::KEY_G,
        Key::KEY_H,
        Key::KEY_I,
        Key::KEY_J,
        Key::KEY_K,
        Key::KEY_L,
        Key::KEY_M,
        Key::KEY_N,
        Key::KEY_O,
        Key::KEY_P,
        Key::KEY_Q,
        Key::KEY_R,
        Key::KEY_S,
        Key::KEY_T,
        Key::KEY_U,
        Key::KEY_V,
        Key::KEY_W,
        Key::KEY_X,
        Key::KEY_Y,
        Key::KEY_Z,
    ];
    let idx = (c as u32).checked_sub('a' as u32)? as usize;
    LETTERS.get(idx).copied()
}

/// Convert text into a key sequence. Returns the sequence plus the characters
/// that could not be mapped (they are skipped). `\r` is dropped silently so
/// that CRLF text types a single Enter.
pub fn text_to_keys(text: &str) -> (Vec<(u16, bool)>, Vec<char>) {
    let mut keys = Vec::new();
    let mut skipped = Vec::new();
    for c in text.chars() {
        if c == '\r' {
            continue;
        }
        match char_to_key(c) {
            Some(k) => keys.push(k),
            None => skipped.push(c),
        }
    }
    (keys, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_and_shift() {
        assert_eq!(char_to_key('a'), Some((Key::KEY_A.code(), false)));
        assert_eq!(char_to_key('Z'), Some((Key::KEY_Z.code(), true)));
        assert_eq!(char_to_key('?'), Some((Key::KEY_SLASH.code(), true)));
        assert_eq!(char_to_key('0'), Some((Key::KEY_0.code(), false)));
    }

    #[test]
    fn non_ascii_skipped() {
        let (keys, skipped) = text_to_keys("Hé!\r\n");
        assert_eq!(keys.len(), 3); // H, !, Enter
        assert_eq!(skipped, vec!['é']);
    }

    #[test]
    fn all_printable_ascii_mapped() {
        for b in 0x20u8..0x7f {
            assert!(char_to_key(b as char).is_some(), "unmapped {:?}", b as char);
        }
    }
}
