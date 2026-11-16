//! Zone overlay drawn directly with X11 override-redirect windows (used while
//! dragging and for the quick-switch flash).
//!
//! With a compositor and a 32-bit ARGB visual, zones are drawn as translucent
//! filled rectangles. Without a compositor, a plain window is shaped (SHAPE
//! extension) down to zone outlines and number badges. Input is always made
//! click-through via an empty input shape.

use crate::config::FancyZonesConfig;
use crate::geometry::{Rect, apply_gap, argb_pixel, parse_hex_color, rgb_pixel};
use crate::x11::Wm;
use anyhow::{Context, Result};
use tracing::{debug, warn};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::shape::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, ConnectionExt as _};

type Rgb = (u8, u8, u8);

/// Colors and options for drawing zones.
#[derive(Debug, Clone)]
pub struct ZoneStyle {
    pub highlight: Rgb,
    pub inactive: Rgb,
    pub border: Rgb,
    pub number: Rgb,
    /// Fill alpha 0-255.
    pub alpha: u8,
    pub show_numbers: bool,
    pub gap: u32,
}

impl ZoneStyle {
    pub fn from_config(c: &FancyZonesConfig) -> Self {
        Self {
            highlight: parse_hex_color(&c.zone_highlight_color).unwrap_or((0, 0x78, 0xD7)),
            inactive: parse_hex_color(&c.zone_inactive_color).unwrap_or((0xF5, 0xFC, 0xFF)),
            border: parse_hex_color(&c.zone_border_color).unwrap_or((255, 255, 255)),
            number: parse_hex_color(&c.zone_number_color).unwrap_or((0, 0, 0)),
            alpha: (c.zone_opacity.min(100) as u32 * 255 / 100) as u8,
            show_numbers: c.show_zone_number,
            gap: c.zone_gap,
        }
    }
}

/// What to show on one overlay window.
#[derive(Debug, Clone)]
pub struct SurfaceSpec {
    /// Absolute area covered by the window (monitor rect).
    pub area: Rect,
    /// Absolute raw zone rects (gap is applied when drawing).
    pub zones: Vec<Rect>,
    /// Optional label drawn at the top center (layout name).
    pub label: Option<String>,
}

struct Surface {
    win: u32,
    gc: u32,
    spec: SurfaceSpec,
    highlighted: Vec<usize>,
}

#[derive(Clone, Copy)]
enum Mode {
    Argb { visual: u32, colormap: u32 },
    Outline,
}

const LABEL_BG: Rgb = (0x26, 0x26, 0x26);
const LABEL_FG: Rgb = (255, 255, 255);
const OUTLINE: i32 = 3;
const OUTLINE_HIGHLIGHT: i32 = 8;

pub struct ZoneOverlay {
    mode: Mode,
    style: ZoneStyle,
    font: Option<(u32, i32, i32)>, // (font, char width, ascent)
    surfaces: Vec<Surface>,
}

fn find_argb_visual(screen: &xproto::Screen) -> Option<u32> {
    screen
        .allowed_depths
        .iter()
        .filter(|d| d.depth == 32)
        .flat_map(|d| d.visuals.iter())
        .find(|v| v.class == xproto::VisualClass::TRUE_COLOR)
        .map(|v| v.visual_id)
}

fn compositor_running(wm: &Wm) -> bool {
    let name = format!("_NET_WM_CM_S{}", wm.screen_num);
    wm.conn
        .intern_atom(false, name.as_bytes())
        .ok()
        .and_then(|c| c.reply().ok())
        .and_then(|a| wm.conn.get_selection_owner(a.atom).ok())
        .and_then(|c| c.reply().ok())
        .is_some_and(|r| r.owner != 0)
}

fn has_shape(wm: &Wm) -> bool {
    wm.conn
        .extension_information(shape::X11_EXTENSION_NAME)
        .ok()
        .flatten()
        .is_some()
}

fn open_font(wm: &Wm) -> Option<(u32, i32, i32)> {
    const FONTS: &[&str] = &[
        "-*-helvetica-bold-r-normal--34-*-*-*-*-*-iso8859-1",
        "-*-dejavu sans-bold-r-normal--34-*-*-*-*-*-*-*",
        "-misc-fixed-bold-r-normal--18-*-*-*-*-*-iso8859-1",
        "10x20",
        "9x15bold",
        "fixed",
    ];
    for name in FONTS {
        let Ok(id) = wm.conn.generate_id() else {
            return None;
        };
        let ok = wm
            .conn
            .open_font(id, name.as_bytes())
            .ok()
            .is_some_and(|c| c.check().is_ok());
        if !ok {
            continue;
        }
        if let Some(info) = wm.conn.query_font(id).ok().and_then(|c| c.reply().ok()) {
            let w = info.max_bounds.character_width.max(1) as i32;
            return Some((id, w, info.font_ascent as i32));
        }
        let _ = wm.conn.close_font(id);
    }
    None
}

/// Make non-ASCII characters safe for core-font `ImageText8`.
fn ascii_label(s: &str) -> Vec<u8> {
    s.chars()
        .map(|c| {
            if c.is_ascii() && !c.is_control() {
                c as u8
            } else {
                b'?'
            }
        })
        .take(250)
        .collect()
}

fn xrect(r: Rect) -> xproto::Rectangle {
    xproto::Rectangle {
        x: r.x as i16,
        y: r.y as i16,
        width: r.w.max(0) as u16,
        height: r.h.max(0) as u16,
    }
}

/// The four edge strips of a rectangle's outline of thickness `t`.
fn frame_rects(r: Rect, t: i32) -> [Rect; 4] {
    let t = t.min(r.w / 2).min(r.h / 2).max(1);
    [
        Rect::new(r.x, r.y, r.w, t),
        Rect::new(r.x, r.bottom() - t, r.w, t),
        Rect::new(r.x, r.y, t, r.h),
        Rect::new(r.right() - t, r.y, t, r.h),
    ]
}

impl ZoneOverlay {
    pub fn new(wm: &Wm, style: ZoneStyle) -> Result<Self> {
        let screen = wm.screen();
        let mode = match find_argb_visual(screen) {
            Some(visual) if compositor_running(wm) => {
                let colormap = wm.conn.generate_id()?;
                wm.conn.create_colormap(
                    xproto::ColormapAlloc::NONE,
                    colormap,
                    screen.root,
                    visual,
                )?;
                Mode::Argb { visual, colormap }
            }
            _ if has_shape(wm) => Mode::Outline,
            _ => anyhow::bail!("no compositor and no SHAPE extension: cannot draw zones"),
        };
        debug!(
            "FancyZones overlay mode: {}",
            if matches!(mode, Mode::Argb { .. }) {
                "ARGB"
            } else {
                "outline"
            }
        );
        let font = open_font(wm);
        Ok(Self {
            mode,
            style,
            font,
            surfaces: Vec::new(),
        })
    }

    pub fn set_style(&mut self, style: ZoneStyle) {
        self.style = style;
    }

    pub fn is_shown(&self) -> bool {
        !self.surfaces.is_empty()
    }

    /// Replace all overlay windows with new ones for `specs`.
    pub fn show(&mut self, wm: &Wm, specs: Vec<SurfaceSpec>) -> Result<()> {
        self.hide(wm);
        for spec in specs {
            match self.create_surface(wm, spec) {
                Ok(s) => self.surfaces.push(s),
                Err(e) => warn!("FancyZones: failed to create overlay window: {e}"),
            }
        }
        for i in 0..self.surfaces.len() {
            self.redraw(wm, i);
        }
        let _ = wm.conn.flush();
        Ok(())
    }

    fn create_surface(&self, wm: &Wm, spec: SurfaceSpec) -> Result<Surface> {
        let screen = wm.screen();
        let win = wm.conn.generate_id()?;
        let a = spec.area;
        let (depth, visual, aux) = match self.mode {
            Mode::Argb { visual, colormap } => (
                32,
                visual,
                xproto::CreateWindowAux::new()
                    .background_pixel(0)
                    .border_pixel(0)
                    .colormap(colormap),
            ),
            Mode::Outline => (
                screen.root_depth,
                screen.root_visual,
                xproto::CreateWindowAux::new().background_pixel(rgb_pixel(self.style.border)),
            ),
        };
        let aux = aux
            .override_redirect(1)
            .event_mask(xproto::EventMask::EXPOSURE);
        wm.conn
            .create_window(
                depth,
                win,
                screen.root,
                a.x as i16,
                a.y as i16,
                a.w.max(1) as u16,
                a.h.max(1) as u16,
                0,
                xproto::WindowClass::INPUT_OUTPUT,
                visual,
                &aux,
            )?
            .check()
            .context("create_window")?;

        // Click-through: empty input region.
        if has_shape(wm) {
            let _ = wm.conn.shape_rectangles(
                shape::SO::SET,
                shape::SK::INPUT,
                xproto::ClipOrdering::UNSORTED,
                win,
                0,
                0,
                &[],
            );
        }

        let gc = wm.conn.generate_id()?;
        let mut gc_aux = xproto::CreateGCAux::new().graphics_exposures(0);
        if let Some((font, _, _)) = self.font {
            gc_aux = gc_aux.font(font);
        }
        wm.conn.create_gc(gc, win, &gc_aux)?;

        let surface = Surface {
            win,
            gc,
            spec,
            highlighted: Vec::new(),
        };
        if matches!(self.mode, Mode::Outline) {
            self.apply_outline_shape(wm, &surface);
        }
        wm.conn.map_window(win)?;
        wm.conn.configure_window(
            win,
            &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
        )?;
        Ok(surface)
    }

    /// Update highlighted zones for each surface (by index); redraws on change.
    pub fn set_highlight(&mut self, wm: &Wm, surface: Option<usize>, zones: &[usize]) {
        for i in 0..self.surfaces.len() {
            let want: Vec<usize> = if Some(i) == surface {
                zones.to_vec()
            } else {
                Vec::new()
            };
            if self.surfaces[i].highlighted != want {
                self.surfaces[i].highlighted = want;
                if matches!(self.mode, Mode::Outline) {
                    self.apply_outline_shape(wm, &self.surfaces[i]);
                }
                self.redraw(wm, i);
                let _ = wm.conn.configure_window(
                    self.surfaces[i].win,
                    &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE),
                );
            }
        }
        let _ = wm.conn.flush();
    }

    /// Redraw after an Expose event. Returns true if the window was ours.
    pub fn handle_expose(&self, wm: &Wm, window: u32) -> bool {
        if let Some(i) = self.surfaces.iter().position(|s| s.win == window) {
            self.redraw(wm, i);
            let _ = wm.conn.flush();
            true
        } else {
            false
        }
    }

    pub fn hide(&mut self, wm: &Wm) {
        for s in self.surfaces.drain(..) {
            let _ = wm.conn.free_gc(s.gc);
            let _ = wm.conn.destroy_window(s.win);
        }
        let _ = wm.conn.flush();
    }

    fn pixel(&self, rgb: Rgb, alpha: u8) -> u32 {
        match self.mode {
            Mode::Argb { .. } => argb_pixel(rgb, alpha),
            Mode::Outline => rgb_pixel(rgb),
        }
    }

    fn label_box(&self, s: &Surface, text: &[u8]) -> Option<(Rect, i32, i32)> {
        let (_, cw, ascent) = self.font?;
        let tw = cw * text.len() as i32;
        let pad = 12;
        let h = ascent + 2 * pad;
        let w = tw + 2 * pad;
        let x = (s.spec.area.w - w) / 2;
        let y = 24;
        Some((Rect::new(x, y, w, h), x + pad, y + pad + ascent))
    }

    fn number_box(&self, zone: Rect, n: usize) -> Option<(Rect, i32, i32, Vec<u8>)> {
        let (_, cw, ascent) = self.font?;
        let text = (n + 1).to_string().into_bytes();
        let tw = cw * text.len() as i32;
        let (cx, cy) = zone.center();
        let side = (tw.max(ascent) + 16).max(32);
        let r = Rect::new(cx - side / 2, cy - side / 2, side, side);
        Some((r, cx - tw / 2, cy + ascent / 2, text))
    }

    fn rel_zones(&self, s: &Surface) -> Vec<Rect> {
        s.spec
            .zones
            .iter()
            .map(|z| apply_gap(z.relative_to(&s.spec.area), self.style.gap))
            .collect()
    }

    fn apply_outline_shape(&self, wm: &Wm, s: &Surface) {
        let mut rects = Vec::new();
        for (i, z) in self.rel_zones(s).into_iter().enumerate() {
            let t = if s.highlighted.contains(&i) {
                OUTLINE_HIGHLIGHT
            } else {
                OUTLINE
            };
            rects.extend(frame_rects(z, t).map(xrect));
            if self.style.show_numbers
                && let Some((b, _, _, _)) = self.number_box(z, i)
            {
                rects.push(xrect(b));
            }
        }
        if let Some(label) = &s.spec.label
            && let Some((b, _, _)) = self.label_box(s, &ascii_label(label))
        {
            rects.push(xrect(b));
        }
        let _ = wm.conn.shape_rectangles(
            shape::SO::SET,
            shape::SK::BOUNDING,
            xproto::ClipOrdering::UNSORTED,
            s.win,
            0,
            0,
            &rects,
        );
    }

    fn fill(&self, wm: &Wm, s: &Surface, pixel: u32, rects: &[Rect]) {
        let _ = wm.conn.change_gc(
            s.gc,
            &xproto::ChangeGCAux::new().foreground(pixel).line_width(0),
        );
        let rs: Vec<_> = rects.iter().copied().map(xrect).collect();
        let _ = wm.conn.poly_fill_rectangle(s.win, s.gc, &rs);
    }

    fn text(&self, wm: &Wm, s: &Surface, (fg, bg): (u32, u32), (x, y): (i32, i32), text: &[u8]) {
        let _ = wm.conn.change_gc(
            s.gc,
            &xproto::ChangeGCAux::new().foreground(fg).background(bg),
        );
        let _ = wm.conn.image_text8(s.win, s.gc, x as i16, y as i16, text);
    }

    fn redraw(&self, wm: &Wm, idx: usize) {
        let Some(s) = self.surfaces.get(idx) else {
            return;
        };
        let st = &self.style;
        let argb = matches!(self.mode, Mode::Argb { .. });
        let full = Rect::new(0, 0, s.spec.area.w, s.spec.area.h);
        if argb {
            self.fill(wm, s, 0, &[full]);
        }
        let border = self.pixel(st.border, 255);
        let number = self.pixel(st.number, 255);
        for (i, z) in self.rel_zones(s).into_iter().enumerate() {
            let hl = s.highlighted.contains(&i);
            let color = if hl { st.highlight } else { st.inactive };
            let t = if hl { OUTLINE_HIGHLIGHT } else { OUTLINE };
            let fill = self.pixel(color, st.alpha);
            if argb {
                self.fill(wm, s, fill, &[z]);
                self.fill(wm, s, border, &frame_rects(z, if hl { 3 } else { 2 }));
            } else {
                let edge = if hl {
                    self.pixel(st.highlight, 255)
                } else {
                    border
                };
                self.fill(wm, s, edge, &frame_rects(z, t));
            }
            if st.show_numbers
                && let Some((b, tx, ty, text)) = self.number_box(z, i)
            {
                let bg = if argb { fill } else { self.pixel(color, 255) };
                if !argb {
                    self.fill(wm, s, bg, &[b]);
                }
                self.text(wm, s, (number, bg), (tx, ty), &text);
            }
        }
        if let Some(label) = &s.spec.label {
            let text = ascii_label(label);
            if let Some((b, tx, ty)) = self.label_box(s, &text) {
                let bg = self.pixel(LABEL_BG, 230);
                self.fill(wm, s, bg, &[b]);
                self.text(wm, s, (self.pixel(LABEL_FG, 255), bg), (tx, ty), &text);
            }
        }
    }
}

/// Show `specs` for `duration` then remove them (quick-switch flash). Blocking;
/// call from a background thread.
pub fn flash(
    style: ZoneStyle,
    specs: Vec<SurfaceSpec>,
    duration: std::time::Duration,
) -> Result<()> {
    let wm = Wm::connect()?;
    let mut overlay = ZoneOverlay::new(&wm, style)?;
    overlay.show(&wm, specs)?;
    let deadline = std::time::Instant::now() + duration;
    while std::time::Instant::now() < deadline {
        while let Ok(Some(ev)) = wm.conn.poll_for_event() {
            if let x11rb::protocol::Event::Expose(e) = ev {
                overlay.handle_expose(&wm, e.window);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    overlay.hide(&wm);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rects_cover_edges() {
        let f = frame_rects(Rect::new(10, 10, 100, 50), 4);
        assert_eq!(f[0], Rect::new(10, 10, 100, 4));
        assert_eq!(f[1], Rect::new(10, 56, 100, 4));
        assert_eq!(f[3], Rect::new(106, 10, 4, 50));
        // Thickness never exceeds half the rect.
        let f = frame_rects(Rect::new(0, 0, 4, 4), 10);
        assert_eq!(f[0].h, 2);
    }

    #[test]
    fn labels_are_ascii() {
        assert_eq!(ascii_label("Grid ü"), b"Grid ?".to_vec());
    }
}
