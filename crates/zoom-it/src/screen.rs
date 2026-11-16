//! Session detection, cursor position and "monitor under the cursor" lookup.
//! GUI-free so it can be used by the recorder in the daemon process.

use anyhow::{Context, Result};
use std::process::Command;
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionType {
    Wayland,
    X11,
    Unknown,
}

pub fn detect_session() -> SessionType {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        SessionType::Wayland
    } else if std::env::var_os("DISPLAY").is_some() {
        SessionType::X11
    } else {
        SessionType::Unknown
    }
}

/// Geometry of one monitor in virtual-desktop coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorGeom {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl MonitorGeom {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x
            && py >= self.y
            && px < self.x + self.width as i32
            && py < self.y + self.height as i32
    }
}

/// Pick the monitor containing `cursor`, else the primary/first one.
pub fn pick_monitor(
    monitors: &[MonitorGeom],
    primary: Option<usize>,
    cursor: Option<(i32, i32)>,
) -> Option<MonitorGeom> {
    cursor
        .and_then(|(cx, cy)| monitors.iter().find(|m| m.contains(cx, cy)))
        .or_else(|| primary.and_then(|i| monitors.get(i)))
        .or(monitors.first())
        .cloned()
}

/// Global cursor position in virtual-desktop coordinates.
pub fn cursor_position() -> Option<(i32, i32)> {
    match detect_session() {
        SessionType::Wayland => cursor_via_hyprctl()
            .or_else(|_| cursor_via_sway())
            .or_else(|_| cursor_via_x11())
            .ok(),
        _ => cursor_via_x11().ok(),
    }
}

/// Monitor currently under the mouse cursor and the cursor position.
pub fn monitor_under_cursor() -> Result<(MonitorGeom, Option<(i32, i32)>)> {
    let cursor = cursor_position();
    let mons = mpt_common::monitor::detect_monitors().unwrap_or_default();
    let primary = mons.iter().position(|m| m.is_primary);
    let geoms: Vec<MonitorGeom> = mons
        .into_iter()
        .map(|m| MonitorGeom {
            name: m.name,
            x: m.x,
            y: m.y,
            width: m.width,
            height: m.height,
        })
        .collect();
    if let Some(m) = pick_monitor(&geoms, primary, cursor) {
        debug!("monitor under cursor: {m:?}");
        return Ok((m, cursor));
    }
    // No monitor tool available: fall back to the X11 root window size.
    let (w, h) = x11_root_size().context("could not determine monitor geometry")?;
    Ok((
        MonitorGeom {
            name: "screen".into(),
            x: 0,
            y: 0,
            width: w,
            height: h,
        },
        cursor,
    ))
}

fn cursor_via_x11() -> Result<(i32, i32)> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt;
    let (conn, screen) = x11rb::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let reply = conn.query_pointer(root)?.reply()?;
    Ok((reply.root_x as i32, reply.root_y as i32))
}

fn x11_root_size() -> Result<(u32, u32)> {
    use x11rb::connection::Connection;
    let (conn, screen) = x11rb::connect(None)?;
    let s = &conn.setup().roots[screen];
    Ok((s.width_in_pixels as u32, s.height_in_pixels as u32))
}

fn cursor_via_hyprctl() -> Result<(i32, i32)> {
    let out = Command::new("hyprctl").arg("cursorpos").output()?;
    anyhow::ensure!(out.status.success(), "hyprctl failed");
    parse_xy(&String::from_utf8_lossy(&out.stdout)).context("bad hyprctl output")
}

fn cursor_via_sway() -> Result<(i32, i32)> {
    let out = Command::new("swaymsg")
        .args(["-t", "get_seats", "--raw"])
        .output()?;
    anyhow::ensure!(out.status.success(), "swaymsg failed");
    let s = String::from_utf8_lossy(&out.stdout);
    // Minimal extraction of "cursor": {"x": .., "y": ..} without a JSON dep.
    let idx = s.find("\"cursor\"").context("no cursor")?;
    let rest = &s[idx..];
    let num_after = |key: &str| -> Option<f64> {
        let i = rest.find(key)? + key.len();
        let tail = rest[i..].trim_start_matches([':', ' ']);
        let end = tail
            .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-'))
            .unwrap_or(tail.len());
        tail[..end].parse().ok()
    };
    let x = num_after("\"x\"").context("no x")?;
    let y = num_after("\"y\"").context("no y")?;
    Ok((x.round() as i32, y.round() as i32))
}

/// Parse `"X, Y"` style output.
pub fn parse_xy(s: &str) -> Option<(i32, i32)> {
    let mut it = s.trim().split(',');
    let x = it.next()?.trim().parse::<f64>().ok()?;
    let y = it.next()?.trim().parse::<f64>().ok()?;
    Some((x.round() as i32, y.round() as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(name: &str, x: i32, w: u32) -> MonitorGeom {
        MonitorGeom {
            name: name.into(),
            x,
            y: 0,
            width: w,
            height: 1080,
        }
    }

    #[test]
    fn picks_monitor_under_cursor() {
        let mons = vec![m("A", 0, 1920), m("B", 1920, 2560)];
        assert_eq!(
            pick_monitor(&mons, Some(0), Some((2000, 10))).unwrap().name,
            "B"
        );
        assert_eq!(
            pick_monitor(&mons, Some(1), Some((-5, 10))).unwrap().name,
            "B"
        );
        assert_eq!(pick_monitor(&mons, None, None).unwrap().name, "A");
        assert!(pick_monitor(&[], None, Some((0, 0))).is_none());
    }

    #[test]
    fn parses_xy() {
        assert_eq!(parse_xy("12, 34\n"), Some((12, 34)));
        assert_eq!(parse_xy("1.6,2.4"), Some((2, 2)));
        assert_eq!(parse_xy("nope"), None);
    }
}
