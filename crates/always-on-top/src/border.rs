use std::collections::HashMap;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, warn};
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{self, ConnectionExt};
use x11rb::rust_connection::RustConnection;

use crate::config::{AlwaysOnTopConfig, gnome_accent_hex, parse_hex_color};
use x11rb::protocol::shape;

/// Commands sent from the main module to the border thread.
pub enum BorderCmd {
    Add(u32),
    Remove(u32),
    Shutdown,
}

/// Inner corner radius of the frame when rounded corners are enabled
/// (roughly the Windows 11 / GNOME window corner radius).
const CORNER_RADIUS: u32 = 8;

/// One shaped, click-through frame window surrounding a target window.
struct BorderWindows {
    frame: u32,
    /// Last known geometry of the target window.
    last_geom: (i32, i32, u32, u32),
}

impl BorderWindows {
    fn all(&self) -> [u32; 1] {
        [self.frame]
    }
}

/// Resolve the frame color: the desktop accent color when `frame_accent_color`
/// is set and available (GNOME `accent-color`), else `border_color`.
pub fn frame_color(config: &AlwaysOnTopConfig) -> String {
    if config.frame_accent_color
        && let Some(hex) = system_accent_color()
    {
        return hex.to_string();
    }
    config.border_color.clone()
}

fn system_accent_color() -> Option<&'static str> {
    let out = std::process::Command::new("gsettings")
        .args(["get", "org.gnome.desktop.interface", "accent-color"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    gnome_accent_hex(&String::from_utf8_lossy(&out.stdout))
}

/// Rectangles + arcs that together fill a rounded rectangle.
fn rounded_rect(
    x: i16,
    y: i16,
    w: u16,
    h: u16,
    radius: u16,
) -> (Vec<xproto::Rectangle>, Vec<xproto::Arc>) {
    let r = radius.min(w / 2).min(h / 2);
    if r == 0 {
        return (
            vec![xproto::Rectangle {
                x,
                y,
                width: w,
                height: h,
            }],
            vec![],
        );
    }
    let d = r * 2;
    let ri = r as i16;
    let rects = vec![
        xproto::Rectangle {
            x: x + ri,
            y,
            width: w - d,
            height: h,
        },
        xproto::Rectangle {
            x,
            y: y + ri,
            width: w,
            height: h - d,
        },
    ];
    let right = x + (w - d) as i16;
    let bottom = y + (h - d) as i16;
    let arcs = [(x, y), (right, y), (x, bottom), (right, bottom)]
        .into_iter()
        .map(|(ax, ay)| xproto::Arc {
            x: ax,
            y: ay,
            width: d,
            height: d,
            angle1: 0,
            angle2: 360 * 64,
        })
        .collect();
    (rects, arcs)
}

/// Frame window rectangle for a target geometry and border thickness.
fn frame_rect((x, y, w, h): (i32, i32, u32, u32), t: u32) -> (i32, i32, u32, u32) {
    (x - t as i32, y - t as i32, w + 2 * t, h + 2 * t)
}

struct BorderManager {
    conn: RustConnection,
    root: u32,
    screen_depth: u8,
    borders: HashMap<u32, BorderWindows>,
    color_pixel: u32,
    opacity_value: u32,
    thickness: u32,
    corner_radius: u32,
}

impl BorderManager {
    fn new(config: &AlwaysOnTopConfig) -> Result<Self> {
        let (conn, screen_num) =
            RustConnection::connect(None).context("border: failed to connect to X11")?;
        let screen = &conn.setup().roots[screen_num];
        let root = screen.root;
        let screen_depth = screen.root_depth;

        let color = frame_color(config);
        let (r, g, b) = parse_hex_color(&color)
            .or_else(|| parse_hex_color(&config.border_color))
            .unwrap_or((0, 120, 212));
        let color_pixel = (r as u32) << 16 | (g as u32) << 8 | b as u32;

        // _NET_WM_WINDOW_OPACITY is a 32-bit cardinal in range 0..u32::MAX
        let opacity_value = (config.border_opacity.clamp(0.0, 1.0) * u32::MAX as f32) as u32;

        Ok(Self {
            conn,
            root,
            screen_depth,
            borders: HashMap::new(),
            color_pixel,
            opacity_value,
            thickness: config.border_thickness.max(1),
            corner_radius: if config.round_corners_enabled {
                CORNER_RADIUS
            } else {
                0
            },
        })
    }

    fn add(&mut self, target: u32) -> Result<()> {
        if self.borders.contains_key(&target) {
            return Ok(());
        }

        let geom = self.get_geometry(target)?;
        let wins = self.create_border_windows(target, geom)?;
        self.borders.insert(target, wins);
        Ok(())
    }

    fn remove(&mut self, target: u32) -> Result<()> {
        if let Some(bw) = self.borders.remove(&target) {
            for wid in bw.all() {
                let _ = self.conn.destroy_window(wid);
            }
            self.conn.flush().context("flush after destroy")?;
        }
        Ok(())
    }

    fn remove_all(&mut self) {
        let targets: Vec<u32> = self.borders.keys().copied().collect();
        for t in targets {
            let _ = self.remove(t);
        }
    }

    fn create_border_windows(
        &self,
        _target: u32,
        geom: (i32, i32, u32, u32),
    ) -> Result<BorderWindows> {
        let (fx, fy, fw, fh) = frame_rect(geom, self.thickness);
        let wid = self.conn.generate_id().context("generate X11 id")?;

        let values = xproto::CreateWindowAux::new()
            .background_pixel(self.color_pixel)
            .override_redirect(1)
            .event_mask(xproto::EventMask::EXPOSURE);

        self.conn
            .create_window(
                self.screen_depth,
                wid,
                self.root,
                fx as i16,
                fy as i16,
                fw.max(1) as u16,
                fh.max(1) as u16,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                0,
                &values,
            )
            .context("create border window")?;

        // Clicks pass through the frame to whatever is below.
        shape::rectangles(
            &self.conn,
            shape::SO::SET,
            shape::SK::INPUT,
            xproto::ClipOrdering::UNSORTED,
            wid,
            0,
            0,
            &[],
        )
        .context("set frame input shape")?;
        self.apply_frame_shape(wid, geom)?;

        // Raise to top of stacking order
        self.raise_window(wid)?;

        // Set opacity
        self.set_opacity(wid)?;

        // Map the window
        self.conn.map_window(wid).context("map border window")?;
        self.conn.flush().context("flush after create borders")?;

        Ok(BorderWindows {
            frame: wid,
            last_geom: geom,
        })
    }

    /// Shape the frame window as a (rounded) ring: outer rect minus the target area.
    fn apply_frame_shape(&self, wid: u32, geom: (i32, i32, u32, u32)) -> Result<()> {
        let t = self.thickness;
        let (_, _, fw, fh) = frame_rect(geom, t);
        let (fw, fh) = (
            fw.clamp(1, u16::MAX as u32) as u16,
            fh.clamp(1, u16::MAX as u32) as u16,
        );
        let (iw, ih) = (
            geom.2.min(u16::MAX as u32) as u16,
            geom.3.min(u16::MAX as u32) as u16,
        );

        let pixmap = self.conn.generate_id()?;
        self.conn.create_pixmap(1, pixmap, wid, fw, fh)?;
        let gc_off = self.conn.generate_id()?;
        self.conn
            .create_gc(gc_off, pixmap, &xproto::CreateGCAux::new().foreground(0))?;
        let gc_on = self.conn.generate_id()?;
        self.conn
            .create_gc(gc_on, pixmap, &xproto::CreateGCAux::new().foreground(1))?;

        self.conn.poly_fill_rectangle(
            pixmap,
            gc_off,
            &[xproto::Rectangle {
                x: 0,
                y: 0,
                width: fw,
                height: fh,
            }],
        )?;
        let outer_r = if self.corner_radius > 0 {
            (self.corner_radius + t) as u16
        } else {
            0
        };
        let (rects, arcs) = rounded_rect(0, 0, fw, fh, outer_r);
        self.conn.poly_fill_rectangle(pixmap, gc_on, &rects)?;
        self.conn.poly_fill_arc(pixmap, gc_on, &arcs)?;
        let (rects, arcs) = rounded_rect(t as i16, t as i16, iw, ih, self.corner_radius as u16);
        self.conn.poly_fill_rectangle(pixmap, gc_off, &rects)?;
        self.conn.poly_fill_arc(pixmap, gc_off, &arcs)?;

        shape::mask(
            &self.conn,
            shape::SO::SET,
            shape::SK::BOUNDING,
            wid,
            0,
            0,
            pixmap,
        )
        .context("set frame bounding shape")?;
        self.conn.free_gc(gc_off)?;
        self.conn.free_gc(gc_on)?;
        self.conn.free_pixmap(pixmap)?;
        Ok(())
    }

    /// Raise an override-redirect window to the top of the stacking order.
    /// Uses configure_window(stack_mode=ABOVE) which works for OR windows,
    /// unlike _NET_WM_STATE_ABOVE client messages that the WM ignores.
    fn raise_window(&self, wid: u32) -> Result<()> {
        let values = xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE);
        self.conn
            .configure_window(wid, &values)
            .context("raise border window")?;
        Ok(())
    }

    fn set_opacity(&self, wid: u32) -> Result<()> {
        let opacity_atom = self.intern_atom(b"_NET_WM_WINDOW_OPACITY")?;

        self.conn
            .change_property(
                xproto::PropMode::REPLACE,
                wid,
                opacity_atom,
                xproto::AtomEnum::CARDINAL,
                32,
                1,
                &self.opacity_value.to_ne_bytes(),
            )
            .context("set opacity")?;

        Ok(())
    }

    fn get_geometry(&self, window: u32) -> Result<(i32, i32, u32, u32)> {
        let geom = self
            .conn
            .get_geometry(window)
            .context("get_geometry failed")?
            .reply()
            .context("get_geometry reply failed")?;

        let coords = self
            .conn
            .translate_coordinates(window, self.root, 0, 0)
            .context("translate_coordinates failed")?
            .reply()
            .context("translate_coordinates reply failed")?;

        Ok((
            coords.dst_x as i32,
            coords.dst_y as i32,
            geom.width as u32,
            geom.height as u32,
        ))
    }

    fn update_positions(&mut self) {
        let targets: Vec<u32> = self.borders.keys().copied().collect();
        let mut to_remove = Vec::new();

        for target in targets {
            // Check if target window still exists
            let geom = match self.get_geometry(target) {
                Ok(g) => g,
                Err(_) => {
                    debug!("target window {target} gone, removing border");
                    to_remove.push(target);
                    continue;
                }
            };

            let bw = match self.borders.get_mut(&target) {
                Some(bw) => bw,
                None => continue,
            };

            let geom_changed = bw.last_geom != geom;

            if geom_changed {
                let frame = bw.frame;
                bw.last_geom = geom;
                let (fx, fy, fw, fh) = frame_rect(geom, self.thickness);
                let values = xproto::ConfigureWindowAux::new()
                    .x(fx)
                    .y(fy)
                    .width(fw.max(1))
                    .height(fh.max(1))
                    .stack_mode(xproto::StackMode::ABOVE);
                let _ = self.conn.configure_window(frame, &values);
                if let Err(e) = self.apply_frame_shape(frame, geom) {
                    debug!("reshape frame for {target} failed: {e}");
                }
            } else {
                // Even if geometry hasn't changed, re-raise borders so they
                // stay on top after focus/stacking changes.
                for wid in bw.all() {
                    let values =
                        xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE);
                    let _ = self.conn.configure_window(wid, &values);
                }
            }
        }

        for t in to_remove {
            let _ = self.remove(t);
        }

        let _ = self.conn.flush();
    }

    fn intern_atom(&self, name: &[u8]) -> Result<u32> {
        let reply = self
            .conn
            .intern_atom(false, name)
            .context("intern_atom request failed")?
            .reply()
            .context("intern_atom reply failed")?;
        Ok(reply.atom)
    }
}

/// Spawn the border management thread. Returns a sender to communicate with it.
pub fn spawn_border_thread(config: &AlwaysOnTopConfig) -> Result<mpsc::Sender<BorderCmd>> {
    let (tx, rx) = mpsc::channel();

    let mut manager = BorderManager::new(config)?;

    std::thread::Builder::new()
        .name("aot-borders".into())
        .spawn(move || {
            loop {
                // Process all pending commands (non-blocking)
                loop {
                    match rx.try_recv() {
                        Ok(BorderCmd::Add(w)) => {
                            if let Err(e) = manager.add(w) {
                                warn!("border add failed for {w}: {e}");
                            }
                        }
                        Ok(BorderCmd::Remove(w)) => {
                            if let Err(e) = manager.remove(w) {
                                warn!("border remove failed for {w}: {e}");
                            }
                        }
                        Ok(BorderCmd::Shutdown) => {
                            manager.remove_all();
                            return;
                        }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => {
                            manager.remove_all();
                            return;
                        }
                    }
                }

                manager.update_positions();
                std::thread::sleep(Duration::from_millis(100));
            }
        })
        .context("failed to spawn border thread")?;

    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rect_surrounds_target() {
        assert_eq!(frame_rect((100, 50, 800, 600), 4), (96, 46, 808, 608));
    }

    #[test]
    fn rounded_rect_parts() {
        let (rects, arcs) = rounded_rect(0, 0, 100, 50, 0);
        assert_eq!(rects.len(), 1);
        assert!(arcs.is_empty());

        let (rects, arcs) = rounded_rect(2, 3, 100, 50, 10);
        assert_eq!(rects.len(), 2);
        assert_eq!((rects[0].x, rects[0].width), (12, 80));
        assert_eq!((rects[1].y, rects[1].height), (13, 30));
        assert_eq!(arcs.len(), 4);
        assert_eq!((arcs[3].x, arcs[3].y, arcs[3].width), (82, 33, 20));

        // Radius is clamped to half the smaller side.
        let (_, arcs) = rounded_rect(0, 0, 10, 6, 50);
        assert_eq!(arcs[0].width, 6);
    }
}
