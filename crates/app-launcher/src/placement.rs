//! Window placement (upstream `Position`: cursor / primary / focused monitor).

use crate::config::LauncherPosition;
use mpt_common::monitor::Monitor;

fn contains(m: &Monitor, (x, y): (i32, i32)) -> bool {
    x >= m.x && y >= m.y && x < m.x + m.width as i32 && y < m.y + m.height as i32
}

/// Choose the monitor for the launcher. `cursor` and `focused` are points in
/// root coordinates (when known). Falls back to the primary, then the first monitor.
pub fn pick_monitor(
    monitors: &[Monitor],
    mode: LauncherPosition,
    cursor: Option<(i32, i32)>,
    focused: Option<(i32, i32)>,
) -> Option<&Monitor> {
    let point = match mode {
        LauncherPosition::Cursor => cursor,
        LauncherPosition::Focused => focused.or(cursor),
        LauncherPosition::Primary => None,
    };
    point
        .and_then(|p| monitors.iter().find(|m| contains(m, p)))
        .or_else(|| monitors.iter().find(|m| m.is_primary))
        .or_else(|| monitors.first())
}

/// Top-left position for a window of `width` centred horizontally on the
/// monitor, at 25% of its height (like upstream).
pub fn window_origin(m: &Monitor, width: f32) -> (f32, f32) {
    let x = m.x as f32 + (m.width as f32 - width).max(0.0) / 2.0;
    let y = m.y as f32 + m.height as f32 * 0.25;
    (x, y)
}

/// Resolve the window origin for the current session (X11 only; `None`
/// lets the GUI centre itself on whatever monitor the WM picks).
pub fn resolve_origin(mode: LauncherPosition, width: f32) -> Option<(f32, f32)> {
    let monitors = mpt_common::monitor::detect_monitors().ok()?;
    let (cursor, focused) = match crate::x11::X11::connect() {
        Ok(x) => (x.cursor_position(), x.focused_window_center()),
        Err(_) => (None, None),
    };
    pick_monitor(&monitors, mode, cursor, focused).map(|m| window_origin(m, width))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mon(name: &str, x: i32, primary: bool) -> Monitor {
        Monitor {
            name: name.into(),
            x,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: primary,
        }
    }

    #[test]
    fn picks_by_mode() {
        let ms = vec![mon("A", 0, true), mon("B", 1920, false)];
        let cur = Some((2000, 100));
        let foc = Some((100, 100));
        assert_eq!(
            pick_monitor(&ms, LauncherPosition::Cursor, cur, foc)
                .unwrap()
                .name,
            "B"
        );
        assert_eq!(
            pick_monitor(&ms, LauncherPosition::Focused, cur, foc)
                .unwrap()
                .name,
            "A"
        );
        assert_eq!(
            pick_monitor(&ms, LauncherPosition::Primary, cur, foc)
                .unwrap()
                .name,
            "A"
        );
        assert_eq!(
            pick_monitor(&ms, LauncherPosition::Focused, cur, None)
                .unwrap()
                .name,
            "B"
        );
        assert_eq!(
            pick_monitor(&ms, LauncherPosition::Cursor, Some((-5, -5)), None)
                .unwrap()
                .name,
            "A"
        );
        assert!(pick_monitor(&[], LauncherPosition::Cursor, cur, foc).is_none());
    }

    #[test]
    fn centres_window() {
        let m = mon("B", 1920, false);
        assert_eq!(window_origin(&m, 680.0), (1920.0 + 620.0, 270.0));
    }
}
