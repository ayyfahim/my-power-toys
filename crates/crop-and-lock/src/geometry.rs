//! Pure geometry and pixel helpers (no X11 / GUI dependencies), unit tested.

use anyhow::{Result, bail};

/// A crop rectangle in pixel coordinates relative to the target window's
/// top-left corner (inside its border).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl CropRect {
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Build a rectangle from two drag corners (any order), clamped to a
    /// `bounds_w` x `bounds_h` area. Returns `None` if the result is empty.
    pub fn from_corners(
        a: (f32, f32),
        b: (f32, f32),
        bounds_w: u32,
        bounds_h: u32,
    ) -> Option<Self> {
        let clamp = |v: f32, max: u32| -> u32 { v.round().clamp(0.0, max as f32) as u32 };
        let x0 = clamp(a.0.min(b.0), bounds_w);
        let x1 = clamp(a.0.max(b.0), bounds_w);
        let y0 = clamp(a.1.min(b.1), bounds_h);
        let y1 = clamp(a.1.max(b.1), bounds_h);
        let rect = Self::new(x0, y0, x1 - x0, y1 - y0);
        (!rect.is_empty()).then_some(rect)
    }

    pub const fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Intersect with a `w` x `h` area starting at the origin.
    /// Returns `None` if nothing remains (e.g. the source window shrank).
    pub fn clamp_to(&self, w: u32, h: u32) -> Option<Self> {
        if self.x >= w || self.y >= h {
            return None;
        }
        let width = self.width.min(w - self.x);
        let height = self.height.min(h - self.y);
        let rect = Self::new(self.x, self.y, width, height);
        (!rect.is_empty()).then_some(rect)
    }
}

/// Map a point from display space (e.g. egui points of a widget showing the
/// image stretched over `display_w` x `display_h`) to image pixel space.
pub fn display_to_image(
    pos: (f32, f32),
    display_w: f32,
    display_h: f32,
    image_w: u32,
    image_h: u32,
) -> (f32, f32) {
    if display_w <= 0.0 || display_h <= 0.0 {
        return (0.0, 0.0);
    }
    (
        pos.0 / display_w * image_w as f32,
        pos.1 / display_h * image_h as f32,
    )
}

/// Largest rectangle with the source aspect ratio that fits in the
/// destination, centered. Returns `(offset_x, offset_y, width, height)`.
/// Mirrors the letterboxing done by upstream's cropped windows.
pub fn fit_centered(src_w: f32, src_h: f32, dst_w: f32, dst_h: f32) -> (f32, f32, f32, f32) {
    if src_w <= 0.0 || src_h <= 0.0 || dst_w <= 0.0 || dst_h <= 0.0 {
        return (0.0, 0.0, dst_w.max(0.0), dst_h.max(0.0));
    }
    let src_aspect = src_w / src_h;
    let dst_aspect = dst_w / dst_h;
    let (w, h) = if dst_aspect > src_aspect {
        (dst_h * src_aspect, dst_h)
    } else {
        (dst_w, dst_w / src_aspect)
    };
    ((dst_w - w) / 2.0, (dst_h - h) / 2.0, w, h)
}

/// Convert X11 ZPixmap image data (as returned by GetImage) to tightly packed
/// RGBA8. Supports the 32 bits-per-pixel layout used by depth 24/32 visuals.
/// Alpha is forced to 255 so ARGB windows do not render see-through.
pub fn zpixmap_to_rgba(
    data: &[u8],
    width: u32,
    height: u32,
    bits_per_pixel: u8,
    scanline_pad: u8,
    lsb_first: bool,
) -> Result<Vec<u8>> {
    if bits_per_pixel != 32 {
        bail!("unsupported X11 pixel format: {bits_per_pixel} bits per pixel");
    }
    let pad = u32::from(scanline_pad.max(8));
    let row_bits = width * 32;
    let stride = (row_bits.div_ceil(pad) * pad / 8) as usize;
    let needed = stride * height as usize;
    if data.len() < needed {
        bail!(
            "X11 image data too short: got {} bytes, need {needed}",
            data.len()
        );
    }

    let mut out = Vec::with_capacity(width as usize * height as usize * 4);
    for row in data.chunks_exact(stride).take(height as usize) {
        for px in row[..width as usize * 4].chunks_exact(4) {
            let (r, g, b) = if lsb_first {
                // Little-endian 0xXXRRGGBB -> bytes B, G, R, X
                (px[2], px[1], px[0])
            } else {
                // Big-endian -> bytes X, R, G, B
                (px[1], px[2], px[3])
            };
            out.extend_from_slice(&[r, g, b, 255]);
        }
    }
    Ok(out)
}

/// Build an ICCCM WM_NORMAL_HINTS property value that fixes the window size
/// and requests the user-specified position.
pub fn fixed_size_hints(width: u32, height: u32) -> [u32; 18] {
    const US_POSITION: u32 = 1;
    const P_MIN_SIZE: u32 = 1 << 4;
    const P_MAX_SIZE: u32 = 1 << 5;
    let mut hints = [0u32; 18];
    hints[0] = US_POSITION | P_MIN_SIZE | P_MAX_SIZE;
    hints[5] = width;
    hints[6] = height;
    hints[7] = width;
    hints[8] = height;
    hints
}

/// Parse an X11 window id given as decimal or `0x`-prefixed hexadecimal.
pub fn parse_window_id(s: &str) -> Result<u32> {
    let s = s.trim();
    let parsed = if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16)
    } else {
        s.parse::<u32>()
    };
    match parsed {
        Ok(0) => bail!("window id must not be 0"),
        Ok(id) => Ok(id),
        Err(e) => bail!("invalid window id '{s}': {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_any_order_and_clamped() {
        let r = CropRect::from_corners((50.0, 40.0), (10.0, 5.0), 100, 100).unwrap();
        assert_eq!(r, CropRect::new(10, 5, 40, 35));

        let r = CropRect::from_corners((-20.0, -20.0), (500.0, 30.0), 100, 80).unwrap();
        assert_eq!(r, CropRect::new(0, 0, 100, 30));
    }

    #[test]
    fn corners_empty_is_none() {
        assert!(CropRect::from_corners((10.0, 10.0), (10.0, 50.0), 100, 100).is_none());
        assert!(CropRect::from_corners((200.0, 10.0), (300.0, 50.0), 100, 100).is_none());
    }

    #[test]
    fn clamp_to_shrunk_window() {
        let r = CropRect::new(10, 10, 100, 100);
        assert_eq!(r.clamp_to(50, 200), Some(CropRect::new(10, 10, 40, 100)));
        assert_eq!(r.clamp_to(10, 200), None);
        assert_eq!(r.clamp_to(500, 500), Some(r));
    }

    #[test]
    fn display_mapping_scales() {
        let p = display_to_image((50.0, 25.0), 100.0, 50.0, 200, 100);
        assert_eq!(p, (100.0, 50.0));
        assert_eq!(display_to_image((1.0, 1.0), 0.0, 10.0, 10, 10), (0.0, 0.0));
    }

    #[test]
    fn fit_letterboxes() {
        // Wide source in square destination -> bars top/bottom.
        let (x, y, w, h) = fit_centered(200.0, 100.0, 100.0, 100.0);
        assert_eq!((x, y, w, h), (0.0, 25.0, 100.0, 50.0));
        // Tall source in square destination -> bars left/right.
        let (x, y, w, h) = fit_centered(100.0, 200.0, 100.0, 100.0);
        assert_eq!((x, y, w, h), (25.0, 0.0, 50.0, 100.0));
    }

    #[test]
    fn zpixmap_lsb_conversion() {
        // 2x1 pixels: blue-ish and red-ish in BGRX order.
        let data = [10, 20, 30, 0, 1, 2, 3, 0];
        let rgba = zpixmap_to_rgba(&data, 2, 1, 32, 32, true).unwrap();
        assert_eq!(rgba, vec![30, 20, 10, 255, 3, 2, 1, 255]);
    }

    #[test]
    fn zpixmap_msb_conversion() {
        let data = [0, 30, 20, 10];
        let rgba = zpixmap_to_rgba(&data, 1, 1, 32, 32, false).unwrap();
        assert_eq!(rgba, vec![30, 20, 10, 255]);
    }

    #[test]
    fn zpixmap_rejects_short_or_unsupported() {
        assert!(zpixmap_to_rgba(&[0; 4], 2, 1, 32, 32, true).is_err());
        assert!(zpixmap_to_rgba(&[0; 8], 2, 1, 16, 32, true).is_err());
    }

    #[test]
    fn size_hints_fixed() {
        let h = fixed_size_hints(320, 240);
        assert_eq!(h[0], 1 | 16 | 32);
        assert_eq!(&h[5..9], &[320, 240, 320, 240]);
    }

    #[test]
    fn window_id_parsing() {
        assert_eq!(parse_window_id("0x3a00007").unwrap(), 0x3a00007);
        assert_eq!(parse_window_id("12345").unwrap(), 12345);
        assert!(parse_window_id("0").is_err());
        assert!(parse_window_id("abc").is_err());
    }
}
