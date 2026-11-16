//! Pure geometry helpers: zoom rectangle math, shape outlines, arrowheads,
//! timer formatting and layout. No GUI dependencies so it can be unit-tested.

/// Highest zoom factor reachable with the wheel (upstream allows 256x; 32x is
/// plenty for a screen capture and keeps the source rect a few pixels wide).
pub const MAX_ZOOM: f32 = 32.0;
pub const MIN_ZOOM: f32 = 1.0;

pub const MIN_PEN_WIDTH: f32 = 2.0;
pub const MAX_PEN_WIDTH: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Map a point inside this rect into `target`, preserving relative position.
    pub fn map_to(&self, p: Point, target: &Rect) -> Point {
        let fx = if self.w > 0.0 {
            (p.x - self.x) / self.w
        } else {
            0.0
        };
        let fy = if self.h > 0.0 {
            (p.y - self.y) / self.h
        } else {
            0.0
        };
        Point::new(target.x + fx * target.w, target.y + fy * target.h)
    }
}

/// Rectangle spanned by two arbitrary corner points.
pub fn normalize_rect(a: Point, b: Point) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    Rect::new(x, y, (a.x - b.x).abs(), (a.y - b.y).abs())
}

/// The region of a `view_w` x `view_h` screen that is shown magnified at
/// `zoom`, centered on `focus` and clamped so it never leaves the screen.
pub fn zoom_source_rect(view_w: f32, view_h: f32, focus: Point, zoom: f32) -> Rect {
    let zoom = zoom.max(MIN_ZOOM);
    let w = view_w / zoom;
    let h = view_h / zoom;
    let x = (focus.x - w / 2.0).clamp(0.0, (view_w - w).max(0.0));
    let y = (focus.y - h / 2.0).clamp(0.0, (view_h - h).max(0.0));
    Rect::new(x, y, w, h)
}

/// Next zoom target when zooming in (upstream "telescope": jump to 2x, then double).
pub fn zoom_in_step(target: f32) -> f32 {
    if target < 2.0 {
        2.0
    } else {
        (target * 2.0).min(MAX_ZOOM)
    }
}

/// Next zoom target when zooming out (halve; gentle 0.75 steps below 2x).
pub fn zoom_out_step(target: f32) -> f32 {
    if target <= 2.0 {
        (target * 0.75).max(MIN_ZOOM)
    } else {
        target / 2.0
    }
}

/// Exponential approach of `current` toward `target` over `dt` seconds.
/// Snaps to the target once the remaining distance is negligible.
pub fn animate_toward(current: f32, target: f32, dt: f32, rate: f32) -> f32 {
    let t = 1.0 - (-dt.max(0.0) * rate).exp();
    let next = current + (target - current) * t;
    if (target - next).abs() < 0.005 * target.abs().max(1.0) {
        target
    } else {
        next
    }
}

/// Clamp a pen width to the upstream range.
pub fn clamp_pen_width(w: f32) -> f32 {
    w.clamp(MIN_PEN_WIDTH, MAX_PEN_WIDTH)
}

/// Polyline approximating the ellipse inscribed in `r` (closed: first == last).
pub fn ellipse_points(r: &Rect, segments: usize) -> Vec<Point> {
    let segments = segments.max(8);
    let c = r.center();
    let (rx, ry) = (r.w / 2.0, r.h / 2.0);
    (0..=segments)
        .map(|i| {
            let a = i as f32 / segments as f32 * std::f32::consts::TAU;
            Point::new(c.x + rx * a.cos(), c.y + ry * a.sin())
        })
        .collect()
}

/// Closed outline of a rectangle (first == last).
pub fn rect_points(r: &Rect) -> Vec<Point> {
    vec![
        Point::new(r.x, r.y),
        Point::new(r.right(), r.y),
        Point::new(r.right(), r.bottom()),
        Point::new(r.x, r.bottom()),
        Point::new(r.x, r.y),
    ]
}

/// Arrowhead wings for an arrow whose tip is at `tip`, drawn from `tail`.
/// Mirrors upstream `DrawArrow`: head length `2.5 * pen`, half-width `1.5 * pen`.
/// Returns `(left_wing, right_wing)`.
pub fn arrowhead(tail: Point, tip: Point, pen_width: f32) -> (Point, Point) {
    let length = pen_width * 2.5;
    let width = pen_width * 1.5;
    let (mut dx, mut dy) = (tip.x - tail.x, tip.y - tail.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len > f32::EPSILON {
        dx /= len;
        dy /= len;
    } else {
        dx = 1.0;
        dy = 0.0;
    }
    let mx = tip.x - length * dx;
    let my = tip.y - length * dy;
    (
        Point::new(mx - dy * width, my + dx * width),
        Point::new(mx + dy * width, my - dx * width),
    )
}

/// Format a break-timer value. Positive = remaining, negative = elapsed since
/// expiry (shown with a leading minus like upstream).
pub fn format_timer(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "" };
    let s = seconds.unsigned_abs();
    let (h, m, sec) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 {
        format!("{sign}{h}:{m:02}:{sec:02}")
    } else {
        format!("{sign}{m}:{sec:02}")
    }
}

/// Top-left position of a `text_w` x `text_h` timer in a `area_w` x `area_h`
/// window for upstream `BreakTimerPosition` (0..=8, row-major grid; 4 = center).
pub fn timer_position(
    position: u32,
    area_w: f32,
    area_h: f32,
    text_w: f32,
    text_h: f32,
    margin: f32,
) -> Point {
    let position = position.min(8);
    let x = match position % 3 {
        0 => margin,
        1 => (area_w - text_w) / 2.0,
        _ => area_w - text_w - margin,
    };
    let y = match position / 3 {
        0 => margin,
        1 => (area_h - text_h) / 2.0,
        _ => area_h - text_h - margin,
    };
    Point::new(x, y)
}

/// Parse `#RRGGBB` or `#RRGGBBAA` (leading `#` optional).
pub fn parse_hex_color(s: &str) -> Option<[u8; 4]> {
    let s = s.trim().trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
    match s.len() {
        6 => Some([byte(0)?, byte(2)?, byte(4)?, 255]),
        8 => Some([byte(0)?, byte(2)?, byte(4)?, byte(6)?]),
        _ => None,
    }
}

/// Monitor-relative clamp of a pixel crop: returns `(x, y, w, h)` in whole
/// pixels fully inside a `img_w` x `img_h` image, or `None` if empty.
pub fn pixel_crop(r: &Rect, img_w: u32, img_h: u32) -> Option<(u32, u32, u32, u32)> {
    let x0 = r.x.floor().clamp(0.0, img_w as f32) as u32;
    let y0 = r.y.floor().clamp(0.0, img_h as f32) as u32;
    let x1 = r.right().ceil().clamp(0.0, img_w as f32) as u32;
    let y1 = r.bottom().ceil().clamp(0.0, img_h as f32) as u32;
    (x1 > x0 && y1 > y0).then(|| (x0, y0, x1 - x0, y1 - y0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn zoom_rect_centered_and_clamped() {
        let r = zoom_source_rect(1000.0, 800.0, Point::new(500.0, 400.0), 2.0);
        assert_eq!(r, Rect::new(250.0, 200.0, 500.0, 400.0));

        let r = zoom_source_rect(1000.0, 800.0, Point::new(0.0, 0.0), 4.0);
        assert_eq!(r, Rect::new(0.0, 0.0, 250.0, 200.0));

        let r = zoom_source_rect(1000.0, 800.0, Point::new(1000.0, 800.0), 4.0);
        assert_eq!(r, Rect::new(750.0, 600.0, 250.0, 200.0));

        // zoom < 1 is treated as 1 (whole screen)
        let r = zoom_source_rect(1000.0, 800.0, Point::new(10.0, 10.0), 0.5);
        assert_eq!(r, Rect::new(0.0, 0.0, 1000.0, 800.0));
    }

    #[test]
    fn telescope_steps() {
        assert!(approx(zoom_in_step(1.25), 2.0));
        assert!(approx(zoom_in_step(2.0), 4.0));
        assert!(approx(zoom_in_step(MAX_ZOOM), MAX_ZOOM));
        assert!(approx(zoom_out_step(8.0), 4.0));
        assert!(approx(zoom_out_step(2.0), 1.5));
        assert!(approx(zoom_out_step(1.1), 1.0));
    }

    #[test]
    fn animation_converges() {
        let mut z = 1.0;
        for _ in 0..200 {
            z = animate_toward(z, 4.0, 1.0 / 60.0, 12.0);
        }
        assert_eq!(z, 4.0);
        let half = animate_toward(1.0, 3.0, 0.01, 12.0);
        assert!(half > 1.0 && half < 3.0);
    }

    #[test]
    fn rects_and_mapping() {
        let r = normalize_rect(Point::new(10.0, 50.0), Point::new(2.0, 20.0));
        assert_eq!(r, Rect::new(2.0, 20.0, 8.0, 30.0));
        let src = Rect::new(0.0, 0.0, 100.0, 100.0);
        let dst = Rect::new(50.0, 50.0, 50.0, 50.0);
        assert_eq!(
            src.map_to(Point::new(50.0, 100.0), &dst),
            Point::new(75.0, 100.0)
        );
        assert_eq!(rect_points(&r).len(), 5);
    }

    #[test]
    fn ellipse_on_boundary() {
        let r = Rect::new(0.0, 0.0, 200.0, 100.0);
        let pts = ellipse_points(&r, 32);
        assert_eq!(pts.len(), 33);
        for p in &pts {
            let nx = (p.x - 100.0) / 100.0;
            let ny = (p.y - 50.0) / 50.0;
            assert!(approx(nx * nx + ny * ny, 1.0));
        }
    }

    #[test]
    fn arrowhead_wings() {
        // Horizontal arrow pointing right, pen 4 -> length 10, half width 6.
        let (l, r) = arrowhead(Point::new(0.0, 0.0), Point::new(100.0, 0.0), 4.0);
        assert!(approx(l.x, 90.0) && approx(l.y, 6.0));
        assert!(approx(r.x, 90.0) && approx(r.y, -6.0));
        // Degenerate arrow does not produce NaN.
        let (l, _) = arrowhead(Point::new(5.0, 5.0), Point::new(5.0, 5.0), 2.0);
        assert!(l.x.is_finite() && l.y.is_finite());
    }

    #[test]
    fn timer_format() {
        assert_eq!(format_timer(600), "10:00");
        assert_eq!(format_timer(65), "1:05");
        assert_eq!(format_timer(0), "0:00");
        assert_eq!(format_timer(-7), "-0:07");
        assert_eq!(format_timer(3725), "1:02:05");
    }

    #[test]
    fn timer_grid_positions() {
        let p = timer_position(0, 1000.0, 800.0, 100.0, 50.0, 50.0);
        assert_eq!(p, Point::new(50.0, 50.0));
        let p = timer_position(4, 1000.0, 800.0, 100.0, 50.0, 50.0);
        assert_eq!(p, Point::new(450.0, 375.0));
        let p = timer_position(8, 1000.0, 800.0, 100.0, 50.0, 50.0);
        assert_eq!(p, Point::new(850.0, 700.0));
        // out of range clamps to bottom-right
        assert_eq!(timer_position(42, 1000.0, 800.0, 100.0, 50.0, 50.0), p);
    }

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex_color("#FF8000"), Some([255, 128, 0, 255]));
        assert_eq!(parse_hex_color("00ff0080"), Some([0, 255, 0, 128]));
        assert_eq!(parse_hex_color("#xyz"), None);
        assert_eq!(parse_hex_color("#12345"), None);
    }

    #[test]
    fn pen_width_clamped() {
        assert_eq!(clamp_pen_width(0.5), MIN_PEN_WIDTH);
        assert_eq!(clamp_pen_width(100.0), MAX_PEN_WIDTH);
    }

    #[test]
    fn crop_clamped() {
        let r = Rect::new(-5.0, 10.2, 20.0, 1000.0);
        assert_eq!(pixel_crop(&r, 100, 100), Some((0, 10, 15, 90)));
        assert_eq!(pixel_crop(&Rect::new(5.0, 5.0, 0.0, 3.0), 100, 100), None);
    }
}
