//! Pure geometry helpers for zone hit-testing, spanning, keyboard navigation
//! and gap application. Everything here is display-independent and unit-tested.

use crate::layout::Layout;
use mpt_common::monitor::Monitor;

/// Integer pixel rectangle in absolute (virtual desktop) coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn from_monitor(m: &Monitor) -> Self {
        Self::new(m.x, m.y, m.width as i32, m.height as i32)
    }

    pub fn right(&self) -> i32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }

    pub fn area(&self) -> i64 {
        self.w.max(0) as i64 * self.h.max(0) as i64
    }

    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }

    /// Half-open containment: left/top edges inclusive, right/bottom exclusive.
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }

    pub fn intersection(&self, o: &Rect) -> Option<Rect> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        (r > x && b > y).then(|| Rect::new(x, y, r - x, b - y))
    }

    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect::new(
            x,
            y,
            self.right().max(o.right()) - x,
            self.bottom().max(o.bottom()) - y,
        )
    }

    /// Translate to coordinates relative to `origin`'s top-left corner.
    pub fn relative_to(&self, origin: &Rect) -> Rect {
        Rect::new(self.x - origin.x, self.y - origin.y, self.w, self.h)
    }
}

/// Pixel rectangles (no gap) of every zone of `layout` placed inside `area`.
/// Edges are computed from rounded absolute positions so adjacent zones share
/// exact borders without 1px holes.
pub fn zone_rects(layout: &Layout, area: Rect) -> Vec<Rect> {
    let fx = |f: f32| area.x + (f * area.w as f32).round() as i32;
    let fy = |f: f32| area.y + (f * area.h as f32).round() as i32;
    layout
        .zones
        .iter()
        .map(|z| {
            let x = fx(z.x);
            let y = fy(z.y);
            Rect::new(x, y, fx(z.x + z.width) - x, fy(z.y + z.height) - y)
        })
        .collect()
}

/// Inset a zone rect by half the gap on every side, so neighbouring zones end up
/// `gap` pixels apart (same convention as `Zone::to_pixels_with_gap`).
pub fn apply_gap(r: Rect, gap: u32) -> Rect {
    let gap = gap as i32;
    if r.w <= gap || r.h <= gap {
        // Too small for the gap: keep the rect as-is rather than inverting it.
        return r;
    }
    let lead = gap / 2;
    Rect::new(r.x + lead, r.y + lead, r.w - gap, r.h - gap)
}

/// How to choose among overlapping zones (upstream `OverlappingZonesAlgorithm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlapAlgorithm {
    Smallest,
    Largest,
    Positional,
    ClosestCenter,
}

impl OverlapAlgorithm {
    pub fn from_index(i: u8) -> Self {
        match i {
            1 => Self::Largest,
            2 => Self::Positional,
            3 => Self::ClosestCenter,
            _ => Self::Smallest,
        }
    }
}

fn dist2(a: (i32, i32), b: (i32, i32)) -> i64 {
    let dx = (a.0 - b.0) as i64;
    let dy = (a.1 - b.1) as i64;
    dx * dx + dy * dy
}

/// Index of the zone under the point, resolving overlaps with `algo`.
pub fn zone_at_point(rects: &[Rect], px: i32, py: i32, algo: OverlapAlgorithm) -> Option<usize> {
    let hits = rects.iter().enumerate().filter(|(_, r)| r.contains(px, py));
    match algo {
        OverlapAlgorithm::Smallest => hits.min_by_key(|(_, r)| r.area()).map(|(i, _)| i),
        OverlapAlgorithm::Largest => hits.max_by_key(|(_, r)| r.area()).map(|(i, _)| i),
        // Positional (upstream splits overlaps by relative position) is
        // approximated by the closest zone center.
        OverlapAlgorithm::Positional | OverlapAlgorithm::ClosestCenter => hits
            .min_by_key(|(_, r)| dist2(r.center(), (px, py)))
            .map(|(i, _)| i),
    }
}

/// Zones selected when spanning from zone `a` to zone `b`: every zone that has at
/// least half its area inside the bounding box of `a` and `b`.
pub fn zones_in_span(rects: &[Rect], a: usize, b: usize) -> Vec<usize> {
    let (Some(ra), Some(rb)) = (rects.get(a), rects.get(b)) else {
        return vec![];
    };
    let bbox = ra.union(rb);
    let mut out: Vec<usize> = rects
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            r.intersection(&bbox)
                .is_some_and(|i| i.area() * 2 >= r.area())
        })
        .map(|(i, _)| i)
        .collect();
    for idx in [a, b] {
        if !out.contains(&idx) {
            out.push(idx);
        }
    }
    out.sort_unstable();
    out
}

/// Bounding rectangle of the selected zones.
pub fn union_of(rects: &[Rect], idxs: &[usize]) -> Option<Rect> {
    idxs.iter()
        .filter_map(|&i| rects.get(i))
        .copied()
        .reduce(|a, b| a.union(&b))
}

/// Final window rect for a set of zones: union of the raw zones, then the gap.
pub fn snap_rect(rects: &[Rect], idxs: &[usize], gap: u32) -> Option<Rect> {
    union_of(rects, idxs).map(|r| apply_gap(r, gap))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn from_action(action: &str) -> Option<Self> {
        match action {
            "move-left" => Some(Self::Left),
            "move-right" => Some(Self::Right),
            "move-up" => Some(Self::Up),
            "move-down" => Some(Self::Down),
            _ => None,
        }
    }
}

/// Result of index-based keyboard navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMove {
    Zone(usize),
    /// Moved past the last (Right) or first (Left) zone.
    Overflow,
}

/// Upstream index navigation: Right goes to the next zone index, Left to the
/// previous one. An unsnapped window goes to the first (Right) or last (Left) zone.
pub fn adjacent_zone_index(len: usize, current: Option<usize>, dir: Direction) -> IndexMove {
    if len == 0 {
        return IndexMove::Overflow;
    }
    let forward = matches!(dir, Direction::Right | Direction::Down);
    match (current, forward) {
        (None, true) => IndexMove::Zone(0),
        (None, false) => IndexMove::Zone(len - 1),
        (Some(i), true) if i + 1 < len => IndexMove::Zone(i + 1),
        (Some(i), false) if i > 0 && i <= len => IndexMove::Zone(i - 1),
        _ => IndexMove::Overflow,
    }
}

/// Position-based navigation: among `candidates`, pick the one whose center lies
/// in direction `dir` from `from`'s center, preferring small distance along the
/// axis and penalising off-axis offset. Candidates whose center equals `from`'s
/// center (the current zone) are skipped.
pub fn adjacent_zone_by_position(candidates: &[Rect], from: Rect, dir: Direction) -> Option<usize> {
    let (fx, fy) = from.center();
    candidates
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let (cx, cy) = r.center();
            let (along, across) = match dir {
                Direction::Left => (fx - cx, cy - fy),
                Direction::Right => (cx - fx, cy - fy),
                Direction::Up => (fy - cy, cx - fx),
                Direction::Down => (cy - fy, cx - fx),
            };
            (along > 0).then_some((i, along as i64 + 2 * across.unsigned_abs() as i64))
        })
        .min_by_key(|&(_, score)| score)
        .map(|(i, _)| i)
}

/// Monitor containing the point, if any.
pub fn monitor_at_point(monitors: &[Rect], px: i32, py: i32) -> Option<usize> {
    monitors.iter().position(|m| m.contains(px, py))
}

/// Monitor with the largest overlap with `r` (falls back to closest center).
pub fn monitor_for_rect(monitors: &[Rect], r: &Rect) -> Option<usize> {
    let best = monitors
        .iter()
        .enumerate()
        .filter_map(|(i, m)| m.intersection(r).map(|x| (i, x.area())))
        .max_by_key(|&(_, a)| a)
        .map(|(i, _)| i);
    best.or_else(|| {
        monitors
            .iter()
            .enumerate()
            .min_by_key(|(_, m)| dist2(m.center(), r.center()))
            .map(|(i, _)| i)
    })
}

/// Bounding box of all monitors (for `span_zones_across_monitors`).
pub fn bounding_box(rects: &[Rect]) -> Option<Rect> {
    rects.iter().copied().reduce(|a, b| a.union(&b))
}

/// Find the zones a window currently occupies, if its rect matches the gap-applied
/// snap rect of a single zone or a contiguous span (tolerance in pixels).
pub fn zones_matching_rect(
    rects: &[Rect],
    window: &Rect,
    gap: u32,
    tol: i32,
) -> Option<Vec<usize>> {
    let close = |a: &Rect, b: &Rect| {
        (a.x - b.x).abs() <= tol
            && (a.y - b.y).abs() <= tol
            && (a.w - b.w).abs() <= tol
            && (a.h - b.h).abs() <= tol
    };
    for (i, r) in rects.iter().enumerate() {
        if close(&apply_gap(*r, gap), window) {
            return Some(vec![i]);
        }
    }
    None
}

/// Whether drag zones should be shown (upstream `DraggingState`):
/// with `shift_drag`, zones appear while Shift is held; otherwise they appear by
/// default and Shift hides them. The mouse toggle flips the result.
pub fn drag_zones_active(shift_drag: bool, shift_held: bool, mouse_toggled: bool) -> bool {
    let on = shift_held ^ mouse_toggled;
    if shift_drag { on } else { !on }
}

/// Case-insensitive match of WM_CLASS (instance and class) against excluded apps.
/// An entry matches if it equals or is contained in either WM_CLASS string.
pub fn is_excluded(wm_class: &[String], excluded: &[String]) -> bool {
    excluded
        .iter()
        .map(|e| e.trim().to_lowercase())
        .filter(|e| !e.is_empty())
        .any(|e| wm_class.iter().any(|c| c.to_lowercase().contains(&e)))
}

/// Parse `#RRGGBB` / `RRGGBB` (also `#RGB`).
pub fn parse_hex_color(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.trim().trim_start_matches('#');
    let hex = |h: &str| u8::from_str_radix(h, 16).ok();
    match s.len() {
        6 => Some((hex(&s[0..2])?, hex(&s[2..4])?, hex(&s[4..6])?)),
        3 => {
            let d = |i: usize| hex(&s[i..i + 1]).map(|v| v * 17);
            Some((d(0)?, d(1)?, d(2)?))
        }
        _ => None,
    }
}

/// Premultiplied ARGB32 pixel value for 32-bit visuals.
pub fn argb_pixel(rgb: (u8, u8, u8), alpha: u8) -> u32 {
    let a = alpha as u32;
    let pm = |c: u8| c as u32 * a / 255;
    (a << 24) | (pm(rgb.0) << 16) | (pm(rgb.1) << 8) | pm(rgb.2)
}

/// Opaque RGB pixel value for 24-bit TrueColor visuals.
pub fn rgb_pixel(rgb: (u8, u8, u8)) -> u32 {
    ((rgb.0 as u32) << 16) | ((rgb.1 as u32) << 8) | rgb.2 as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::Zone;

    const SCREEN: Rect = Rect::new(0, 0, 1920, 1080);

    #[test]
    fn zone_rects_share_edges() {
        let rects = zone_rects(&Layout::default_columns(3), SCREEN);
        assert_eq!(rects[0], Rect::new(0, 0, 640, 1080));
        assert_eq!(rects[0].right(), rects[1].x);
        assert_eq!(rects[1].right(), rects[2].x);
        assert_eq!(rects[2].right(), 1920);
        // Offset monitor
        let rects = zone_rects(&Layout::default_columns(2), Rect::new(1920, 100, 1000, 500));
        assert_eq!(rects[1], Rect::new(2420, 100, 500, 500));
    }

    #[test]
    fn gap_application() {
        assert_eq!(
            apply_gap(Rect::new(0, 0, 100, 100), 8),
            Rect::new(4, 4, 92, 92)
        );
        assert_eq!(
            apply_gap(Rect::new(10, 10, 100, 50), 0),
            Rect::new(10, 10, 100, 50)
        );
        assert_eq!(
            apply_gap(Rect::new(0, 0, 100, 100), 7),
            Rect::new(3, 3, 93, 93)
        );
        // Gap bigger than the rect never inverts it.
        let r = apply_gap(Rect::new(0, 0, 4, 4), 10);
        assert!(r.w >= 1 && r.h >= 1);
    }

    #[test]
    fn zone_under_point() {
        let rects = zone_rects(&Layout::grid(2, 2), SCREEN);
        let algo = OverlapAlgorithm::Smallest;
        assert_eq!(zone_at_point(&rects, 10, 10, algo), Some(0));
        assert_eq!(zone_at_point(&rects, 1900, 1000, algo), Some(3));
        assert_eq!(zone_at_point(&rects, 960, 540, algo), Some(3)); // shared corner -> right/bottom
        assert_eq!(zone_at_point(&rects, -5, 10, algo), None);
    }

    #[test]
    fn overlapping_zones() {
        let layout = Layout::canvas(
            "o",
            vec![Zone::new(0.0, 0.0, 1.0, 1.0), Zone::new(0.4, 0.4, 0.2, 0.2)],
        );
        let rects = zone_rects(&layout, SCREEN);
        assert_eq!(
            zone_at_point(&rects, 960, 540, OverlapAlgorithm::Smallest),
            Some(1)
        );
        assert_eq!(
            zone_at_point(&rects, 960, 540, OverlapAlgorithm::Largest),
            Some(0)
        );
        assert_eq!(
            zone_at_point(&rects, 10, 10, OverlapAlgorithm::Smallest),
            Some(0)
        );
    }

    #[test]
    fn span_union() {
        let rects = zone_rects(&Layout::grid(3, 2), SCREEN);
        // Zone 0 (top-left) to zone 4 (bottom-middle) covers 0,1,3,4
        assert_eq!(zones_in_span(&rects, 0, 4), vec![0, 1, 3, 4]);
        assert_eq!(zones_in_span(&rects, 2, 2), vec![2]);
        let u = union_of(&rects, &[0, 1, 3, 4]).unwrap();
        assert_eq!(u, Rect::new(0, 0, 1280, 1080));
        assert_eq!(
            snap_rect(&rects, &[0, 1], 8).unwrap(),
            Rect::new(4, 4, 1272, 532)
        );
        assert_eq!(union_of(&rects, &[]), None);
    }

    #[test]
    fn index_navigation() {
        use Direction::*;
        assert_eq!(adjacent_zone_index(3, None, Right), IndexMove::Zone(0));
        assert_eq!(adjacent_zone_index(3, None, Left), IndexMove::Zone(2));
        assert_eq!(adjacent_zone_index(3, Some(0), Right), IndexMove::Zone(1));
        assert_eq!(adjacent_zone_index(3, Some(2), Right), IndexMove::Overflow);
        assert_eq!(adjacent_zone_index(3, Some(0), Left), IndexMove::Overflow);
        assert_eq!(adjacent_zone_index(0, None, Left), IndexMove::Overflow);
    }

    #[test]
    fn position_navigation() {
        use Direction::*;
        let rects = zone_rects(&Layout::grid(2, 2), SCREEN);
        assert_eq!(adjacent_zone_by_position(&rects, rects[0], Right), Some(1));
        assert_eq!(adjacent_zone_by_position(&rects, rects[0], Down), Some(2));
        assert_eq!(adjacent_zone_by_position(&rects, rects[3], Up), Some(1));
        assert_eq!(adjacent_zone_by_position(&rects, rects[3], Left), Some(2));
        assert_eq!(adjacent_zone_by_position(&rects, rects[0], Left), None);
        assert_eq!(adjacent_zone_by_position(&rects, rects[0], Up), None);
        // Across two monitors: right of the right-most zone on monitor A is on B.
        let mut all = zone_rects(&Layout::default_columns(2), SCREEN);
        all.extend(zone_rects(
            &Layout::default_columns(2),
            Rect::new(1920, 0, 1920, 1080),
        ));
        assert_eq!(adjacent_zone_by_position(&all, all[1], Right), Some(2));
    }

    #[test]
    fn monitors() {
        let mons = [SCREEN, Rect::new(1920, 0, 1280, 1024)];
        assert_eq!(monitor_at_point(&mons, 2000, 10), Some(1));
        assert_eq!(monitor_at_point(&mons, 5000, 10), None);
        assert_eq!(
            monitor_for_rect(&mons, &Rect::new(1800, 0, 400, 300)),
            Some(1)
        );
        assert_eq!(bounding_box(&mons), Some(Rect::new(0, 0, 3200, 1080)));
    }

    #[test]
    fn match_window_to_zone() {
        let rects = zone_rects(&Layout::default_columns(2), SCREEN);
        let w = apply_gap(rects[1], 8);
        assert_eq!(zones_matching_rect(&rects, &w, 8, 2), Some(vec![1]));
        assert_eq!(
            zones_matching_rect(&rects, &Rect::new(5, 5, 300, 300), 8, 2),
            None
        );
    }

    #[test]
    fn drag_activation() {
        assert!(drag_zones_active(true, true, false));
        assert!(!drag_zones_active(true, false, false));
        assert!(drag_zones_active(true, false, true));
        assert!(drag_zones_active(false, false, false));
        assert!(!drag_zones_active(false, true, false));
    }

    #[test]
    fn exclusion_and_colors() {
        let class = vec!["navigator".to_string(), "Firefox".to_string()];
        assert!(is_excluded(&class, &["firefox".into()]));
        assert!(!is_excluded(&class, &["chrome".into(), " ".into()]));
        assert_eq!(parse_hex_color("#0078D7"), Some((0, 0x78, 0xD7)));
        assert_eq!(parse_hex_color("fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex_color("#12"), None);
        assert_eq!(argb_pixel((255, 255, 255), 128), 0x80808080);
        assert_eq!(argb_pixel((255, 0, 0), 255), 0xFFFF0000);
        assert_eq!(rgb_pixel((1, 2, 3)), 0x010203);
    }
}
