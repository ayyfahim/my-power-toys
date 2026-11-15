//! Edge detection, a port of upstream MeasureTool `EdgeDetection.cpp`.
//!
//! Starting from the cursor pixel, scan left/right/up/down until a pixel is no
//! longer "close" to the start pixel. Each edge is the coordinate of the last
//! similar pixel, so a 5x5 square at (20,100) yields bounds (20,100)-(24,104).

/// Read-only view over a tightly packed RGBA8 buffer.
#[derive(Debug, Clone, Copy)]
pub struct RgbaView<'a> {
    pub data: &'a [u8],
    pub width: u32,
    pub height: u32,
}

impl<'a> RgbaView<'a> {
    /// Returns `None` if `data` is smaller than `width * height * 4`.
    pub fn new(data: &'a [u8], width: u32, height: u32) -> Option<Self> {
        let needed = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        (data.len() >= needed).then_some(Self {
            data,
            width,
            height,
        })
    }

    #[inline]
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * self.width as usize + x as usize) * 4;
        [
            self.data[i],
            self.data[i + 1],
            self.data[i + 2],
            self.data[i + 3],
        ]
    }
}

/// Inclusive pixel bounds of the detected region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EdgeRect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl EdgeRect {
    /// Inclusive rectangle spanned by two corner pixels (upstream Bounds tool
    /// uses `minmax` of drag start and cursor).
    pub fn from_corners(a: (u32, u32), b: (u32, u32)) -> Self {
        Self {
            left: a.0.min(b.0),
            top: a.1.min(b.1),
            right: a.0.max(b.0),
            bottom: a.1.max(b.1),
        }
    }

    /// Width in pixels (inclusive, as upstream: `right - left + 1`).
    pub fn width(&self) -> u32 {
        self.right - self.left + 1
    }

    pub fn height(&self) -> u32 {
        self.bottom - self.top + 1
    }
}

/// Upstream `PixelsClose`: compare all four channels.
/// Per-channel: every channel distance must be `<= tolerance`.
/// Summed: the sum of the channel distances must be `<= tolerance`.
pub fn pixels_close(a: [u8; 4], b: [u8; 4], tolerance: u8, per_channel: bool) -> bool {
    let d = |i: usize| a[i].abs_diff(b[i]) as u16;
    if per_channel {
        (0..4).all(|i| d(i) <= tolerance as u16)
    } else {
        (0..4).map(d).sum::<u16>() <= tolerance as u16
    }
}

#[derive(Clone, Copy)]
enum Dir {
    Left,
    Right,
    Up,
    Down,
}

fn find_edge(
    view: &RgbaView,
    start: (u32, u32),
    tolerance: u8,
    per_channel: bool,
    dir: Dir,
) -> u32 {
    let (mut x, mut y) = start;
    let start_pixel = view.pixel(x, y);
    loop {
        let (old_x, old_y) = (x, y);
        match dir {
            Dir::Left => {
                if x == 0 {
                    return 0;
                }
                x -= 1;
            }
            Dir::Right => {
                if x + 1 >= view.width {
                    return view.width - 1;
                }
                x += 1;
            }
            Dir::Up => {
                if y == 0 {
                    return 0;
                }
                y -= 1;
            }
            Dir::Down => {
                if y + 1 >= view.height {
                    return view.height - 1;
                }
                y += 1;
            }
        }
        if !pixels_close(start_pixel, view.pixel(x, y), tolerance, per_channel) {
            return match dir {
                Dir::Left | Dir::Right => old_x,
                Dir::Up | Dir::Down => old_y,
            };
        }
    }
}

/// Detect the edges around `cursor` (pixel coordinates in `view`).
///
/// Like upstream, the start point is clamped to `[1, dim - 2]` so the cursor
/// pixel is never on the outermost row/column. Returns `None` for empty images.
pub fn detect_edges(
    view: &RgbaView,
    cursor: (i64, i64),
    per_channel: bool,
    tolerance: u8,
) -> Option<EdgeRect> {
    if view.width == 0 || view.height == 0 {
        return None;
    }
    let clamp = |v: i64, dim: u32| -> u32 {
        let lo = 1i64.min(dim as i64 - 1);
        let hi = (dim as i64 - 2).max(lo);
        v.clamp(lo, hi) as u32
    };
    let start = (clamp(cursor.0, view.width), clamp(cursor.1, view.height));
    Some(EdgeRect {
        left: find_edge(view, start, tolerance, per_channel, Dir::Left),
        top: find_edge(view, start, tolerance, per_channel, Dir::Up),
        right: find_edge(view, start, tolerance, per_channel, Dir::Right),
        bottom: find_edge(view, start, tolerance, per_channel, Dir::Down),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];

    fn image(w: u32, h: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                v.extend_from_slice(&f(x, y));
            }
        }
        v
    }

    #[test]
    fn square_on_background_matches_upstream_example() {
        // 5x5 green square at (20,100) on blue: bounds [20,100]-[24,104].
        let data = image(64, 128, |x, y| {
            if (20..25).contains(&x) && (100..105).contains(&y) {
                GREEN
            } else {
                BLUE
            }
        });
        let view = RgbaView::new(&data, 64, 128).unwrap();
        let r = detect_edges(&view, (22, 102), false, 30).unwrap();
        assert_eq!(
            r,
            EdgeRect {
                left: 20,
                top: 100,
                right: 24,
                bottom: 104
            }
        );
        assert_eq!((r.width(), r.height()), (5, 5));

        // From the background, the scan stops just before the square.
        let r = detect_edges(&view, (10, 102), false, 30).unwrap();
        assert_eq!((r.left, r.right), (0, 19));
    }

    #[test]
    fn uniform_image_reaches_borders() {
        let data = image(10, 8, |_, _| BLUE);
        let view = RgbaView::new(&data, 10, 8).unwrap();
        let r = detect_edges(&view, (5, 4), true, 0).unwrap();
        assert_eq!(
            r,
            EdgeRect {
                left: 0,
                top: 0,
                right: 9,
                bottom: 7
            }
        );
    }

    #[test]
    fn cursor_is_clamped_away_from_border() {
        // Column 0 differs; cursor at x=0 is clamped to x=1, so left edge = 1.
        let data = image(10, 10, |x, _| if x == 0 { GREEN } else { BLUE });
        let view = RgbaView::new(&data, 10, 10).unwrap();
        let r = detect_edges(&view, (-5, 50), false, 30).unwrap();
        assert_eq!(r.left, 1);
        assert_eq!(r.right, 9);
        assert_eq!(r.bottom, 9);
    }

    #[test]
    fn summed_vs_per_channel_tolerance() {
        let a = [100, 100, 100, 255];
        let b = [120, 120, 120, 255]; // each channel 20, sum 60
        assert!(pixels_close(a, b, 30, true));
        assert!(!pixels_close(a, b, 30, false));
        assert!(pixels_close(a, b, 60, false));
        assert!(!pixels_close(a, [131, 100, 100, 255], 30, true));

        // Gradient of +20 per channel at x>=5: per-channel (tol 30) sees no edge.
        let data = image(10, 3, |x, _| if x >= 5 { b } else { a });
        let view = RgbaView::new(&data, 10, 3).unwrap();
        let per = detect_edges(&view, (2, 1), true, 30).unwrap();
        let sum = detect_edges(&view, (2, 1), false, 30).unwrap();
        assert_eq!(per.right, 9);
        assert_eq!(sum.right, 4);
    }

    #[test]
    fn rect_from_drag_corners() {
        let r = EdgeRect::from_corners((30, 5), (10, 25));
        assert_eq!((r.left, r.top, r.right, r.bottom), (10, 5, 30, 25));
        assert_eq!((r.width(), r.height()), (21, 21));
        let p = EdgeRect::from_corners((7, 7), (7, 7));
        assert_eq!((p.width(), p.height()), (1, 1));
    }

    #[test]
    fn tiny_and_invalid_images() {
        let data = image(1, 1, |_, _| BLUE);
        let view = RgbaView::new(&data, 1, 1).unwrap();
        let r = detect_edges(&view, (0, 0), false, 30).unwrap();
        assert_eq!((r.width(), r.height()), (1, 1));

        assert!(RgbaView::new(&[0u8; 3], 1, 1).is_none());
        let empty = RgbaView::new(&[], 0, 0).unwrap();
        assert!(detect_edges(&empty, (0, 0), false, 30).is_none());
    }
}
