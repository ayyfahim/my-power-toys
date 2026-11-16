//! Pure geometry for move/resize, mirroring upstream GrabAndMove `main.cpp`
//! (`GetClosestHandle`, `HandleDragMove`, `HandleDragResize`).

/// Upstream minimum window size during resize.
pub const MIN_WINDOW_WIDTH: i32 = 150;
pub const MIN_WINDOW_HEIGHT: i32 = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    fn right(&self) -> i32 {
        self.x + self.w
    }
    fn bottom(&self) -> i32 {
        self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Handle {
    fn moves_left(self) -> bool {
        matches!(self, Handle::TopLeft | Handle::BottomLeft | Handle::Left)
    }
    fn moves_right(self) -> bool {
        matches!(self, Handle::TopRight | Handle::BottomRight | Handle::Right)
    }
    fn moves_top(self) -> bool {
        matches!(self, Handle::TopLeft | Handle::Top | Handle::TopRight)
    }
    fn moves_bottom(self) -> bool {
        matches!(
            self,
            Handle::BottomLeft | Handle::Bottom | Handle::BottomRight
        )
    }

    /// Glyph index in the X "cursor" font for this handle.
    pub fn cursor_glyph(self) -> u16 {
        match self {
            Handle::TopLeft => 134,    // top_left_corner
            Handle::Top => 138,        // top_side
            Handle::TopRight => 136,   // top_right_corner
            Handle::Right => 96,       // right_side
            Handle::BottomRight => 14, // bottom_right_corner
            Handle::Bottom => 16,      // bottom_side
            Handle::BottomLeft => 12,  // bottom_left_corner
            Handle::Left => 70,        // left_side
        }
    }
}

/// Glyph index of the "fleur" (move) cursor.
pub const MOVE_CURSOR_GLYPH: u16 = 52;

/// Pick the corner/edge-midpoint handle nearest to the pointer
/// (ties resolve in upstream order; default bottom-right).
pub fn closest_handle(px: i32, py: i32, r: Rect) -> Handle {
    let cx = r.x + r.w / 2;
    let cy = r.y + r.h / 2;
    let candidates = [
        (r.x, r.y, Handle::TopLeft),
        (cx, r.y, Handle::Top),
        (r.right(), r.y, Handle::TopRight),
        (r.right(), cy, Handle::Right),
        (r.right(), r.bottom(), Handle::BottomRight),
        (cx, r.bottom(), Handle::Bottom),
        (r.x, r.bottom(), Handle::BottomLeft),
        (r.x, cy, Handle::Left),
    ];
    let mut best = Handle::BottomRight;
    let mut best_dist = i64::MAX;
    for (x, y, h) in candidates {
        let dx = (px - x) as i64;
        let dy = (py - y) as i64;
        let d = dx * dx + dy * dy;
        if d < best_dist {
            best_dist = d;
            best = h;
        }
    }
    best
}

/// Translate a rect by the pointer delta.
pub fn apply_move(r: Rect, dx: i32, dy: i32) -> Rect {
    Rect::new(r.x + dx, r.y + dy, r.w, r.h)
}

/// Move the edges selected by `handle` by the delta, then enforce the
/// minimum size while keeping the opposite (anchored) edge fixed.
pub fn apply_resize(r: Rect, handle: Handle, dx: i32, dy: i32, min_w: i32, min_h: i32) -> Rect {
    let (mut left, mut top, mut right, mut bottom) = (r.x, r.y, r.right(), r.bottom());
    if handle.moves_left() {
        left += dx;
    }
    if handle.moves_right() {
        right += dx;
    }
    if handle.moves_top() {
        top += dy;
    }
    if handle.moves_bottom() {
        bottom += dy;
    }
    let min_w = min_w.max(1);
    let min_h = min_h.max(1);
    if right - left < min_w {
        if handle.moves_left() {
            left = right - min_w;
        } else {
            right = left + min_w;
        }
    }
    if bottom - top < min_h {
        if handle.moves_top() {
            top = bottom - min_h;
        } else {
            bottom = top + min_h;
        }
    }
    Rect::new(left, top, right - left, bottom - top)
}

/// After un-maximizing, place the restored window so the grab point keeps the
/// same proportional position inside it (upstream behaviour).
pub fn restore_position(
    grab: (i32, i32),
    maximized: Rect,
    restored_w: i32,
    restored_h: i32,
) -> (i32, i32) {
    let ratio_x = if maximized.w > 0 {
        (grab.0 - maximized.x) as f32 / maximized.w as f32
    } else {
        0.5
    };
    let ratio_y = if maximized.h > 0 {
        (grab.1 - maximized.y) as f32 / maximized.h as f32
    } else {
        0.5
    };
    (
        grab.0 - (restored_w as f32 * ratio_x) as i32,
        grab.1 - (restored_h as f32 * ratio_y) as i32,
    )
}

/// Whether the pointer has moved far enough from the press point to start a drag.
pub fn past_threshold(dx: i32, dy: i32, threshold: i32) -> bool {
    dx.abs() >= threshold || dy.abs() >= threshold
}

/// Case-insensitive excluded-app match against WM_CLASS parts, the
/// executable basename, or a substring of the window title.
pub fn matches_app(list: &[String], instance: &str, class: &str, exe: &str, title: &str) -> bool {
    let title = title.to_lowercase();
    list.iter()
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .any(|e| {
            e.eq_ignore_ascii_case(instance)
                || e.eq_ignore_ascii_case(class)
                || e.eq_ignore_ascii_case(exe)
                || title.contains(&e)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const R: Rect = Rect {
        x: 100,
        y: 100,
        w: 400,
        h: 300,
    };

    #[test]
    fn closest_handle_corners_and_edges() {
        assert_eq!(closest_handle(110, 110, R), Handle::TopLeft);
        assert_eq!(closest_handle(490, 390, R), Handle::BottomRight);
        assert_eq!(closest_handle(490, 110, R), Handle::TopRight);
        assert_eq!(closest_handle(110, 390, R), Handle::BottomLeft);
        assert_eq!(closest_handle(300, 105, R), Handle::Top);
        assert_eq!(closest_handle(300, 395, R), Handle::Bottom);
        assert_eq!(closest_handle(105, 250, R), Handle::Left);
        assert_eq!(closest_handle(495, 250, R), Handle::Right);
    }

    #[test]
    fn move_translates() {
        assert_eq!(apply_move(R, -10, 25), Rect::new(90, 125, 400, 300));
    }

    #[test]
    fn resize_bottom_right_grows() {
        let r = apply_resize(
            R,
            Handle::BottomRight,
            20,
            30,
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!(r, Rect::new(100, 100, 420, 330));
    }

    #[test]
    fn resize_top_left_keeps_bottom_right_anchor() {
        let r = apply_resize(
            R,
            Handle::TopLeft,
            50,
            40,
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!(r, Rect::new(150, 140, 350, 260));
        assert_eq!(r.right(), R.right());
        assert_eq!(r.bottom(), R.bottom());
    }

    #[test]
    fn resize_edges_affect_one_axis() {
        let r = apply_resize(
            R,
            Handle::Left,
            -30,
            99,
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!(r, Rect::new(70, 100, 430, 300));
        let r = apply_resize(
            R,
            Handle::Bottom,
            99,
            -20,
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!(r, Rect::new(100, 100, 400, 280));
    }

    #[test]
    fn resize_clamps_to_min_with_anchor() {
        // Shrinking from the left past the minimum keeps the right edge.
        let r = apply_resize(
            R,
            Handle::Left,
            1000,
            0,
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!(r.w, MIN_WINDOW_WIDTH);
        assert_eq!(r.x + r.w, R.right());
        // Shrinking from the bottom keeps the top edge.
        let r = apply_resize(
            R,
            Handle::Bottom,
            0,
            -1000,
            MIN_WINDOW_WIDTH,
            MIN_WINDOW_HEIGHT,
        );
        assert_eq!(r.h, MIN_WINDOW_HEIGHT);
        assert_eq!(r.y, R.y);
    }

    #[test]
    fn restore_keeps_proportional_grab() {
        let max = Rect::new(0, 0, 1000, 800);
        let (x, y) = restore_position((500, 400), max, 200, 100);
        assert_eq!((x, y), (400, 350));
        let (x, y) = restore_position((0, 0), max, 200, 100);
        assert_eq!((x, y), (0, 0));
    }

    #[test]
    fn threshold() {
        assert!(!past_threshold(3, -3, 4));
        assert!(past_threshold(-4, 0, 4));
    }

    #[test]
    fn excluded_matching() {
        let list = vec![
            "Gimp".to_string(),
            "  ".to_string(),
            "secret doc".to_string(),
        ];
        assert!(matches_app(&list, "gimp-2.10", "Gimp", "gimp", ""));
        assert!(matches_app(&list, "x", "y", "z", "My Secret Doc - Editor"));
        assert!(!matches_app(
            &list, "firefox", "Firefox", "firefox", "Mozilla"
        ));
        assert!(!matches_app(&[], "a", "b", "c", "d"));
    }
}
