//! Parity tests against Microsoft PowerToys FancyZones.
//!
//! The oracle functions below are direct ports of
//! `src/modules/fancyzones/FancyZonesLib/LayoutConfigurator.cpp`,
//! `Layout.cpp` (`ZonesFromPoint`, `GetCombinedZoneRange`) and
//! `util.cpp` (`ChooseNextZoneByPosition`), plus the defaults in
//! `FancyZonesData/LayoutDefaults.h` / `LayoutData.h`.
//! A failing test is a parity finding, not a test bug.

// Oracle code mirrors the PowerToys C++ loops line by line.
#![allow(clippy::needless_range_loop, clippy::bool_assert_comparison)]

use crate::config::FancyZonesConfig;
use crate::geometry::{
    self, Direction, IndexMove, OverlapAlgorithm, Rect, adjacent_zone_by_position,
    adjacent_zone_index, apply_gap, snap_rect, zone_at_point, zone_rects, zones_in_span,
};
use crate::layout::{Layout, TemplateKind, Zone};

// ───────────────────────── PowerToys oracle ─────────────────────────

/// PowerToys RECT (left, top, right, bottom), relative to the work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PtRect {
    l: i64,
    t: i64,
    r: i64,
    b: i64,
}

impl PtRect {
    fn to_rect(self) -> Rect {
        Rect::new(
            self.l as i32,
            self.t as i32,
            (self.r - self.l) as i32,
            (self.b - self.t) as i32,
        )
    }
}

const C_MULTIPLIER: i64 = 10000;
const PT_SPACING: i64 = 16; // LayoutDefaults.h DefaultValues::Spacing
const PT_SENSITIVITY: i64 = 20; // LayoutDefaults.h DefaultValues::SensitivityRadius

fn pt_columns(w: i64, h: i64, n: i64, spacing: i64) -> Vec<PtRect> {
    let total_w = w - spacing * (n + 1);
    let total_h = h - spacing * 2;
    let mut left = spacing;
    let top = spacing;
    let mut out = vec![];
    for i in 0..n {
        let right = left + (i + 1) * total_w / n - i * total_w / n;
        let bottom = total_h + spacing;
        out.push(PtRect {
            l: left,
            t: top,
            r: right,
            b: bottom,
        });
        left = right + spacing;
    }
    out
}

fn pt_rows(w: i64, h: i64, n: i64, spacing: i64) -> Vec<PtRect> {
    let total_w = w - spacing * 2;
    let total_h = h - spacing * (n + 1);
    let mut top = spacing;
    let left = spacing;
    let mut out = vec![];
    for i in 0..n {
        let right = total_w + spacing;
        let bottom = top + (i + 1) * total_h / n - i * total_h / n;
        out.push(PtRect {
            l: left,
            t: top,
            r: right,
            b: bottom,
        });
        top = bottom + spacing;
    }
    out
}

struct GridInfo {
    rows_pct: Vec<i64>,
    cols_pct: Vec<i64>,
    map: Vec<Vec<usize>>,
}

fn pt_calc_grid(w: i64, h: i64, g: &GridInfo, spacing: i64) -> Vec<PtRect> {
    let rows = g.rows_pct.len();
    let cols = g.cols_pct.len();
    let mut row_se = vec![(0i64, 0i64); rows];
    let mut col_se = vec![(0i64, 0i64); cols];
    let mut tot = 0;
    for r in 0..rows {
        let s = tot * h / C_MULTIPLIER;
        tot += g.rows_pct[r];
        row_se[r] = (s, tot * h / C_MULTIPLIER);
    }
    tot = 0;
    for c in 0..cols {
        let s = tot * w / C_MULTIPLIER;
        tot += g.cols_pct[c];
        col_se[c] = (s, tot * w / C_MULTIPLIER);
    }
    let n = g.map.iter().flatten().max().map(|m| m + 1).unwrap_or(0);
    let mut out = vec![
        PtRect {
            l: 0,
            t: 0,
            r: 0,
            b: 0
        };
        n
    ];
    for row in 0..rows {
        for col in 0..cols {
            let i = g.map[row][col];
            if (row == 0 || g.map[row - 1][col] != i) && (col == 0 || g.map[row][col - 1] != i) {
                let mut left = col_se[col].0;
                let mut top = row_se[row].0;
                let mut max_row = row;
                while max_row + 1 < rows && g.map[max_row + 1][col] == i {
                    max_row += 1;
                }
                let mut max_col = col;
                while max_col + 1 < cols && g.map[row][max_col + 1] == i {
                    max_col += 1;
                }
                let mut right = col_se[max_col].1;
                let mut bottom = row_se[max_row].1;
                top += if row == 0 { spacing } else { spacing / 2 };
                bottom -= if max_row == rows - 1 {
                    spacing
                } else {
                    spacing / 2
                };
                left += if col == 0 { spacing } else { spacing / 2 };
                right -= if max_col == cols - 1 {
                    spacing
                } else {
                    spacing / 2
                };
                out[i] = PtRect {
                    l: left,
                    t: top,
                    r: right,
                    b: bottom,
                };
            }
        }
    }
    out
}

/// (rows, columns) chosen by LayoutConfigurator::Grid for `n` zones.
fn pt_grid_dims(n: i64) -> (i64, i64) {
    let mut rows = 1;
    while n / rows >= rows {
        rows += 1;
    }
    rows -= 1;
    let mut cols = n / rows;
    if n % rows != 0 {
        cols += 1;
    }
    (rows, cols)
}

fn pt_grid(w: i64, h: i64, n: i64, spacing: i64) -> Vec<PtRect> {
    let (rows, cols) = pt_grid_dims(n);
    let rows_pct = (0..rows)
        .map(|r| C_MULTIPLIER * (r + 1) / rows - C_MULTIPLIER * r / rows)
        .collect();
    let cols_pct = (0..cols)
        .map(|c| C_MULTIPLIER * (c + 1) / cols - C_MULTIPLIER * c / cols)
        .collect();
    let mut map = vec![vec![0usize; cols as usize]; rows as usize];
    let mut idx = 0i64;
    for r in 0..rows as usize {
        for c in 0..cols as usize {
            map[r][c] = idx as usize;
            idx += 1;
            if idx == n {
                idx -= 1;
            }
        }
    }
    pt_calc_grid(
        w,
        h,
        &GridInfo {
            rows_pct,
            cols_pct,
            map,
        },
        spacing,
    )
}

fn pt_priority_grid(w: i64, h: i64, n: usize, spacing: i64) -> Vec<PtRect> {
    let g = |rows: Vec<i64>, cols: Vec<i64>, map: Vec<Vec<usize>>| GridInfo {
        rows_pct: rows,
        cols_pct: cols,
        map,
    };
    let c3 = vec![2500, 5000, 2500];
    let c4 = vec![2500, 2500, 2500, 2500];
    let r3 = vec![3333, 3334, 3333];
    let info = match n {
        1 => g(vec![10000], vec![10000], vec![vec![0]]),
        2 => g(vec![10000], vec![6667, 3333], vec![vec![0, 1]]),
        3 => g(vec![10000], c3, vec![vec![0, 1, 2]]),
        4 => g(vec![5000, 5000], c3, vec![vec![0, 1, 2], vec![0, 1, 3]]),
        5 => g(vec![5000, 5000], c3, vec![vec![0, 1, 2], vec![3, 1, 4]]),
        6 => g(r3, c3, vec![vec![0, 1, 2], vec![0, 1, 3], vec![4, 1, 5]]),
        7 => g(r3, c3, vec![vec![0, 1, 2], vec![3, 1, 4], vec![5, 1, 6]]),
        8 => g(
            r3,
            c4,
            vec![vec![0, 1, 2, 3], vec![4, 1, 2, 5], vec![6, 1, 2, 7]],
        ),
        _ => return pt_grid(w, h, n as i64, spacing),
    };
    pt_calc_grid(w, h, &info, spacing)
}

fn pt_focus(w: i64, h: i64, n: i64) -> Vec<PtRect> {
    let mut r = PtRect {
        l: 100,
        t: 100,
        r: 100 + (w as f64 * 0.4) as i64,
        b: 100 + (h as f64 * 0.4) as i64,
    };
    let inc = if n <= 1 { 0 } else { 50 };
    let mut out = vec![];
    for _ in 0..n {
        out.push(r);
        r.l += inc;
        r.r += inc;
        r.t += inc;
        r.b += inc;
    }
    out
}

/// Layout::ZonesFromPoint (non-overlapping branch is enough for these tests;
/// overlapping zones fall back to "smallest" like the default algorithm).
fn pt_zones_from_point(zones: &[PtRect], x: i64, y: i64, radius: i64) -> Vec<usize> {
    let mut captured = vec![];
    let mut strict = vec![];
    for (i, z) in zones.iter().enumerate() {
        if z.l - radius <= x && x <= z.r + radius && z.t - radius <= y && y <= z.b + radius {
            captured.push(i);
        }
        if z.l <= x && x < z.r && z.t <= y && y < z.b {
            strict.push(i);
        }
    }
    if captured.len() == 1 && strict.is_empty() {
        return vec![];
    }
    let mut overlap = false;
    for i in 0..captured.len() {
        for j in i + 1..captured.len() {
            let (a, b) = (zones[captured[i]], zones[captured[j]]);
            if a.t.max(b.t) + radius < a.b.min(b.b) && a.l.max(b.l) + radius < a.r.min(b.r) {
                overlap = true;
            }
        }
    }
    if overlap {
        let best = captured
            .iter()
            .copied()
            .min_by_key(|&i| (zones[i].r - zones[i].l) * (zones[i].b - zones[i].t))
            .unwrap();
        return vec![best];
    }
    captured
}

/// Layout::GetCombinedZoneRange: every zone fully inside the bounding box.
fn pt_combined_zone_range(zones: &[PtRect], a: usize, b: usize) -> Vec<usize> {
    let bb = PtRect {
        l: zones[a].l.min(zones[b].l),
        t: zones[a].t.min(zones[b].t),
        r: zones[a].r.max(zones[b].r),
        b: zones[a].b.max(zones[b].b),
    };
    (0..zones.len())
        .filter(|&i| {
            let z = zones[i];
            bb.l <= z.l && z.r <= bb.r && bb.t <= z.t && z.b <= bb.b
        })
        .collect()
}

/// FancyZonesUtils::ChooseNextZoneByPosition.
fn pt_choose_next_zone_by_position(
    dir: Direction,
    window: PtRect,
    zones: &[PtRect],
) -> Option<usize> {
    let ecc = 2.0f64;
    let center = |r: PtRect| {
        (
            0.5 * r.l as f64 + 0.5 * r.r as f64,
            0.5 * r.t as f64 + 0.5 * r.b as f64,
        )
    };
    let (dx, dy) = match dir {
        Direction::Up => (0.0, -1.0),
        Direction::Down => (0.0, 1.0),
        Direction::Left => (-1.0, 0.0),
        Direction::Right => (1.0, 0.0),
    };
    let wc = center(window);
    let mut best = None;
    let mut best_d = 1e100;
    for (i, z) in zones.iter().enumerate() {
        let c = center(*z);
        // `center += 0.001 * (i + 1)` on a std::complex only shifts the real part.
        let (zx, zy) = (c.0 + 0.001 * (i + 1) as f64 - wc.0, c.1 - wc.1);
        // (arrow * conj(zone)).real() == dx*zx + dy*zy
        let sp = dx * zx + dy * zy;
        if sp <= 0.0 {
            continue;
        }
        let cos = sp / (zx * zx + zy * zy).sqrt();
        let tan = cos.clamp(-1.0, 1.0).acos().tan().abs();
        if tan > 10.0 {
            continue;
        }
        let iy = 2.0 * ecc / (1.0 + ecc * ecc * tan * tan);
        let d = sp / iy;
        if d.is_finite() && d < best_d {
            best_d = d;
            best = Some(i);
        }
    }
    best
}

fn port_rects_with_gap(layout: &Layout, w: i32, h: i32, gap: u32) -> Vec<Rect> {
    let raw = zone_rects(layout, Rect::new(0, 0, w, h));
    (0..raw.len())
        .map(|i| snap_rect(&raw, &[i], gap).unwrap())
        .collect()
}

fn pt_to_rects(v: &[PtRect]) -> Vec<Rect> {
    v.iter().map(|r| r.to_rect()).collect()
}

const DIVISIBLE: [(i32, i32); 1] = [(1920, 1080)];
const ODD_SIZES: [(i32, i32); 4] = [(1366, 768), (2560, 1440), (1000, 700), (1280, 1024)];

// ───────────────────────── Defaults ─────────────────────────

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn default_spacing_is_16_like_powertoys() {
    // LayoutDefaults.h: Spacing = 16, ShowSpacing = true
    assert_eq!(FancyZonesConfig::default().zone_gap, 16);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn default_layout_is_priority_grid_with_3_zones() {
    // LayoutData.h: type = PriorityGrid, zoneCount = 3
    let cfg = FancyZonesConfig::default();
    let l = &cfg.layouts[cfg.default_layout];
    assert_eq!(
        (l.kind, l.zones.len()),
        (TemplateKind::PriorityGrid, 3),
        "got {}",
        l.name
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn templates_default_to_3_zones() {
    // LayoutDefaultSettings.DefaultZoneCount = 3 for every template
    for l in Layout::all_templates() {
        if matches!(l.kind, TemplateKind::NoLayout) {
            continue;
        }
        assert_eq!(
            l.zones.len(),
            3,
            "template {} has {} zones",
            l.name,
            l.zones.len()
        );
    }
}

#[test]
fn powertoys_color_defaults_match() {
    // ConfigDefaults.cs
    let c = FancyZonesConfig::default();
    assert_eq!(c.zone_highlight_color, "#0078D7");
    assert_eq!(c.zone_inactive_color, "#F5FCFF");
    assert_eq!(c.zone_border_color, "#FFFFFF");
    assert_eq!(c.zone_number_color, "#000000");
    assert_eq!(c.zone_opacity, 50);
    assert!(c.show_zone_number);
    assert_eq!(c.flash_duration_ms, 1500);
}

#[test]
fn powertoys_flag_defaults_match() {
    // FZConfigProperties.cs constructor
    let c = FancyZonesConfig::default();
    assert!(c.shift_drag);
    assert!(!c.mouse_switch);
    assert!(!c.mouse_middle_click_spanning_multiple_zones);
    assert!(!c.override_snap_hotkeys);
    assert!(!c.move_windows_across_monitors);
    assert!(!c.move_windows_based_on_position);
    assert_eq!(c.overlapping_zones_algorithm, 0);
    assert!(!c.app_last_zone_move_windows);
    assert!(!c.restore_size);
    assert!(c.quick_layout_switch);
    assert!(c.flash_zones_on_quick_switch);
    assert!(!c.show_on_all_monitors);
    assert!(!c.span_zones_across_monitors);
    assert!(!c.make_dragged_window_transparent);
    assert!(c.excluded_apps.is_empty());
}

// ───────────────────────── Columns / Rows ─────────────────────────

#[test]
fn columns_1_to_5_no_spacing_divisible_resolution() {
    for (w, h) in DIVISIBLE {
        for n in 1..=5 {
            let port = port_rects_with_gap(&Layout::default_columns(n), w, h, 0);
            let pt = pt_to_rects(&pt_columns(w as i64, h as i64, n as i64, 0));
            assert_eq!(port, pt, "{w}x{h} columns={n}");
        }
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn columns_1_to_5_no_spacing_odd_resolutions() {
    let mut diffs = vec![];
    for (w, h) in ODD_SIZES {
        for n in 1..=5 {
            let port = port_rects_with_gap(&Layout::default_columns(n), w, h, 0);
            let pt = pt_to_rects(&pt_columns(w as i64, h as i64, n as i64, 0));
            if port != pt {
                diffs.push(format!("{w}x{h} n={n}: port={port:?} pt={pt:?}"));
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn columns_1_to_5_spacing_16() {
    let mut diffs = vec![];
    for (w, h) in DIVISIBLE.iter().chain(ODD_SIZES.iter()) {
        for n in 1..=5 {
            let port = port_rects_with_gap(&Layout::default_columns(n), *w, *h, 16);
            let pt = pt_to_rects(&pt_columns(*w as i64, *h as i64, n as i64, PT_SPACING));
            if port != pt {
                diffs.push(format!(
                    "{w}x{h} n={n}: port[0]={:?} pt[0]={:?}",
                    port[0], pt[0]
                ));
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
fn rows_1_to_5_no_spacing_divisible_resolution() {
    for (w, h) in DIVISIBLE {
        for n in 1..=5 {
            let port = port_rects_with_gap(&Layout::default_rows(n), w, h, 0);
            let pt = pt_to_rects(&pt_rows(w as i64, h as i64, n as i64, 0));
            assert_eq!(port, pt, "{w}x{h} rows={n}");
        }
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn rows_1_to_5_no_spacing_odd_resolutions() {
    let mut diffs = vec![];
    for (w, h) in ODD_SIZES {
        for n in 1..=5 {
            let port = port_rects_with_gap(&Layout::default_rows(n), w, h, 0);
            let pt = pt_to_rects(&pt_rows(w as i64, h as i64, n as i64, 0));
            if port != pt {
                diffs.push(format!("{w}x{h} n={n}: port={port:?} pt={pt:?}"));
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn rows_1_to_5_spacing_16() {
    let mut diffs = vec![];
    for (w, h) in DIVISIBLE.iter().chain(ODD_SIZES.iter()) {
        for n in 1..=5 {
            let port = port_rects_with_gap(&Layout::default_rows(n), *w, *h, 16);
            let pt = pt_to_rects(&pt_rows(*w as i64, *h as i64, n as i64, PT_SPACING));
            if port != pt {
                diffs.push(format!(
                    "{w}x{h} n={n}: port[0]={:?} pt[0]={:?}",
                    port[0], pt[0]
                ));
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn outer_margin_equals_spacing_like_powertoys() {
    // PowerToys: the outer margin equals the full spacing (16), port insets gap/2.
    let port = port_rects_with_gap(&Layout::default_columns(2), 1920, 1080, 16);
    assert_eq!(port[0].x, 16, "left margin");
    assert_eq!(port[0].y, 16, "top margin");
    // Interior gap between zone 0 and zone 1 is `spacing` in both.
    assert_eq!(port[1].x - port[0].right(), 16, "interior gap");
}

// ───────────────────────── Grid ─────────────────────────

#[test]
fn grid_dims_oracle_sanity() {
    let dims: Vec<_> = (1..=8).map(pt_grid_dims).collect();
    assert_eq!(
        dims,
        vec![
            (1, 1),
            (1, 2),
            (1, 3),
            (2, 2),
            (2, 3),
            (2, 3),
            (2, 4),
            (2, 4)
        ]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn grid_1_to_8_zones_without_merges_spacing_0_and_16() {
    let mut diffs = vec![];
    for n in [1i64, 2, 3, 4, 6, 8] {
        let (rows, cols) = pt_grid_dims(n);
        let layout = Layout::grid(cols as usize, rows as usize);
        assert_eq!(layout.zones.len() as i64, n);
        for (w, h) in DIVISIBLE.iter().chain(ODD_SIZES.iter()) {
            for sp in [0i64, PT_SPACING] {
                let port = port_rects_with_gap(&layout, *w, *h, sp as u32);
                let pt = pt_to_rects(&pt_grid(*w as i64, *h as i64, n, sp));
                if port != pt {
                    diffs.push(format!(
                        "n={n} {w}x{h} spacing={sp}: port={port:?}\n   pt={pt:?}"
                    ));
                }
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn grid_1_to_8_zones_divisible_no_spacing() {
    for n in [1i64, 2, 3, 4, 6, 8] {
        let (rows, cols) = pt_grid_dims(n);
        let layout = Layout::grid(cols as usize, rows as usize);
        let port = port_rects_with_gap(&layout, 1920, 1080, 0);
        let pt = pt_to_rects(&pt_grid(1920, 1080, n, 0));
        assert_eq!(port, pt, "n={n}");
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn grid_with_5_or_7_zones_supported() {
    // PowerToys Grid(5) is 2x3 with the last two cells merged, Grid(7) is 2x4
    // with the last two cells merged. The port can only build full cols x rows
    // grids, so the zone count never matches.
    for n in [5usize, 7] {
        let (rows, cols) = pt_grid_dims(n as i64);
        let l = Layout::custom_from_kind("g", TemplateKind::Grid, cols as usize, rows as usize);
        assert_eq!(l.zones.len(), n, "Grid with {n} zones");
    }
}

// ───────────────────────── Priority Grid / Focus ─────────────────────────

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn priority_grid_matches_powertoys_table() {
    let port_layout = Layout::priority_grid();
    let n = port_layout.zones.len();
    let port = port_rects_with_gap(&port_layout, 1920, 1080, 0);
    let pt = pt_to_rects(&pt_priority_grid(1920, 1080, n, 0));
    assert_eq!(port, pt, "priority grid with {n} zones");
}

#[test]
fn priority_grid_oracle_3_zones_is_25_50_25() {
    let pt = pt_to_rects(&pt_priority_grid(1920, 1080, 3, 0));
    assert_eq!(
        pt,
        vec![
            Rect::new(0, 0, 480, 1080),
            Rect::new(480, 0, 960, 1080),
            Rect::new(1440, 0, 480, 1080)
        ]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn focus_layout_matches_powertoys() {
    let port_layout = Layout::focus();
    let n = port_layout.zones.len() as i64;
    let port = port_rects_with_gap(&port_layout, 1920, 1080, 0);
    let pt = pt_to_rects(&pt_focus(1920, 1080, n));
    assert_eq!(port, pt, "focus with {n} zone(s)");
}

// ───────────────────────── Hit-testing ─────────────────────────

#[test]
fn hit_test_inside_zone_centers_matches() {
    for layout in [
        Layout::default_columns(3),
        Layout::default_rows(4),
        Layout::grid(3, 2),
    ] {
        let raw = zone_rects(&layout, Rect::new(0, 0, 1920, 1080));
        for (i, r) in raw.iter().enumerate() {
            let (cx, cy) = r.center();
            let pt_rects: Vec<PtRect> = raw
                .iter()
                .map(|r| PtRect {
                    l: r.x as i64,
                    t: r.y as i64,
                    r: r.right() as i64,
                    b: r.bottom() as i64,
                })
                .collect();
            assert_eq!(
                pt_zones_from_point(&pt_rects, cx as i64, cy as i64, PT_SENSITIVITY),
                vec![i]
            );
            assert_eq!(
                zone_at_point(&raw, cx, cy, OverlapAlgorithm::Smallest),
                Some(i)
            );
        }
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn hit_test_near_border_selects_both_zones_sensitivity_radius_20() {
    // PowerToys: with sensitivityRadius 20, a point within 20 px of the border
    // between two adjacent zones captures BOTH zones (window spans them).
    let pt = pt_columns(1920, 1080, 2, PT_SPACING);
    let got_pt = pt_zones_from_point(&pt, 960, 500, PT_SENSITIVITY);
    assert_eq!(got_pt, vec![0, 1]);
    let raw = zone_rects(&Layout::default_columns(2), Rect::new(0, 0, 1920, 1080));
    let got_port = zone_at_point(&raw, 960, 500, OverlapAlgorithm::Smallest);
    assert_eq!(
        got_port.map(|z| vec![z]),
        Some(got_pt),
        "port selects a single zone"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn hit_test_in_outer_margin_selects_nothing() {
    // PowerToys: a point in the outer spacing margin is only captured via the
    // sensitivity radius by one zone, which is discarded -> no zone.
    let pt = pt_columns(1920, 1080, 3, PT_SPACING);
    assert!(pt_zones_from_point(&pt, 5, 500, PT_SENSITIVITY).is_empty());
    let raw = zone_rects(&Layout::default_columns(3), Rect::new(0, 0, 1920, 1080));
    assert_eq!(
        zone_at_point(&raw, 5, 500, OverlapAlgorithm::Smallest),
        None
    );
}

#[test]
fn hit_test_overlapping_canvas_smallest_and_largest() {
    let layout = Layout::canvas(
        "o",
        vec![
            Zone::new(0.0, 0.0, 1.0, 1.0),
            Zone::new(0.25, 0.25, 0.5, 0.5),
        ],
    );
    let raw = zone_rects(&layout, Rect::new(0, 0, 1920, 1080));
    let pt: Vec<PtRect> = raw
        .iter()
        .map(|r| PtRect {
            l: r.x as i64,
            t: r.y as i64,
            r: r.right() as i64,
            b: r.bottom() as i64,
        })
        .collect();
    assert_eq!(pt_zones_from_point(&pt, 960, 540, PT_SENSITIVITY), vec![1]);
    assert_eq!(
        zone_at_point(&raw, 960, 540, OverlapAlgorithm::Smallest),
        Some(1)
    );
    assert_eq!(
        zone_at_point(&raw, 960, 540, OverlapAlgorithm::Largest),
        Some(0)
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn hit_test_layout_zone_at_boundary_is_half_open() {
    // geometry::zone_at_point is half-open (PowerToys strict capture uses
    // left <= x < right). Layout::zone_at (used by the Super+Z overlay) is not.
    let l = Layout::default_columns(2);
    let raw = zone_rects(&l, Rect::new(0, 0, 1920, 1080));
    assert_eq!(
        zone_at_point(&raw, 960, 540, OverlapAlgorithm::Smallest),
        Some(1)
    );
    assert_eq!(
        l.zone_at(0.5, 0.5),
        Some(1),
        "Layout::zone_at picks the left zone"
    );
}

// ───────────────────────── Multi-zone selection ─────────────────────────

#[test]
fn span_on_grid_matches_powertoys() {
    let layout = Layout::grid(3, 2);
    let raw = zone_rects(&layout, Rect::new(0, 0, 1920, 1080));
    let pt: Vec<PtRect> = raw
        .iter()
        .map(|r| PtRect {
            l: r.x as i64,
            t: r.y as i64,
            r: r.right() as i64,
            b: r.bottom() as i64,
        })
        .collect();
    for a in 0..6 {
        for b in 0..6 {
            assert_eq!(
                zones_in_span(&raw, a, b),
                pt_combined_zone_range(&pt, a, b),
                "{a}->{b}"
            );
        }
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn span_on_canvas_only_includes_fully_contained_zones() {
    // Zone 2 has 75% of its area inside the bounding box of zones 0 and 1 but is
    // not fully contained: PowerToys excludes it, the port's 50% rule includes it.
    let layout = Layout::canvas(
        "c",
        vec![
            Zone::new(0.0, 0.0, 0.5, 0.5),
            Zone::new(0.5, 0.0, 0.5, 0.5),
            Zone::new(0.25, 0.2, 0.5, 0.4),
        ],
    );
    let raw = zone_rects(&layout, Rect::new(0, 0, 1920, 1080));
    let pt: Vec<PtRect> = raw
        .iter()
        .map(|r| PtRect {
            l: r.x as i64,
            t: r.y as i64,
            r: r.right() as i64,
            b: r.bottom() as i64,
        })
        .collect();
    assert_eq!(pt_combined_zone_range(&pt, 0, 1), vec![0, 1]);
    assert_eq!(zones_in_span(&raw, 0, 1), vec![0, 1]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn spanned_snap_rect_with_spacing_matches_powertoys() {
    // PowerToys GetCombinedZonesRect = union of the (spacing-adjusted) zone rects.
    let pt = pt_columns(1920, 1080, 3, PT_SPACING);
    let pt_union = PtRect {
        l: pt[0].l,
        t: pt[0].t,
        r: pt[1].r,
        b: pt[1].b,
    }
    .to_rect();
    let raw = zone_rects(&Layout::default_columns(3), Rect::new(0, 0, 1920, 1080));
    assert_eq!(snap_rect(&raw, &[0, 1], 16), Some(pt_union));
}

// ───────────────────────── Keyboard (Win+Arrow) ─────────────────────────

#[test]
fn index_move_matches_powertoys_move_by_direction_and_index() {
    use Direction::*;
    // Unsnapped: Left -> last zone, Right -> zone 0
    assert_eq!(adjacent_zone_index(4, None, Left), IndexMove::Zone(3));
    assert_eq!(adjacent_zone_index(4, None, Right), IndexMove::Zone(0));
    // Middle
    assert_eq!(adjacent_zone_index(4, Some(1), Left), IndexMove::Zone(0));
    assert_eq!(adjacent_zone_index(4, Some(1), Right), IndexMove::Zone(2));
    // Edges -> caller cycles (single monitor) or moves to the next monitor
    assert_eq!(adjacent_zone_index(4, Some(0), Left), IndexMove::Overflow);
    assert_eq!(adjacent_zone_index(4, Some(3), Right), IndexMove::Overflow);
}

/// Compare the port's position-based neighbour choice with PowerToys'
/// ChooseNextZoneByPosition (candidates exclude the window's own zone).
fn position_mismatches(rects: &[Rect]) -> Vec<String> {
    let pt: Vec<PtRect> = rects
        .iter()
        .map(|r| PtRect {
            l: r.x as i64,
            t: r.y as i64,
            r: r.right() as i64,
            b: r.bottom() as i64,
        })
        .collect();
    let mut out = vec![];
    for from in 0..rects.len() {
        for dir in [
            Direction::Left,
            Direction::Right,
            Direction::Up,
            Direction::Down,
        ] {
            let free: Vec<usize> = (0..rects.len()).filter(|&i| i != from).collect();
            let free_rects: Vec<PtRect> = free.iter().map(|&i| pt[i]).collect();
            let want = pt_choose_next_zone_by_position(dir, pt[from], &free_rects).map(|k| free[k]);
            let got = adjacent_zone_by_position(rects, rects[from], dir);
            if want != got {
                out.push(format!("from {from} {dir:?}: port={got:?} pt={want:?}"));
            }
        }
    }
    out
}

#[test]
fn position_move_on_regular_grids_matches_powertoys() {
    let area = Rect::new(0, 0, 1920, 1080);
    let mut diffs = vec![];
    for l in [
        Layout::grid(2, 2),
        Layout::grid(3, 3),
        Layout::default_columns(4),
        Layout::default_rows(3),
    ] {
        for d in position_mismatches(&zone_rects(&l, area)) {
            diffs.push(format!("{}: {d}", l.name));
        }
    }
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn position_move_on_priority_grid_and_canvas_matches_powertoys() {
    let mut diffs = vec![];
    for n in 3..=8 {
        let rects = pt_to_rects(&pt_priority_grid(1920, 1080, n, 0));
        for d in position_mismatches(&rects) {
            diffs.push(format!("priority{n}: {d}"));
        }
    }
    let canvas = Layout::canvas(
        "c",
        vec![
            Zone::new(0.0, 0.0, 0.6, 0.3),
            Zone::new(0.65, 0.0, 0.35, 0.9),
            Zone::new(0.0, 0.35, 0.3, 0.65),
            Zone::new(0.32, 0.4, 0.3, 0.3),
        ],
    );
    for d in position_mismatches(&zone_rects(&canvas, Rect::new(0, 0, 1920, 1080))) {
        diffs.push(format!("canvas: {d}"));
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches:\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

// ───────────────────────── Overlay vs snap geometry ─────────────────────────

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn overlay_zone_pixels_equal_snap_rect() {
    // The Super+Z overlay draws zones with Zone::to_pixels_with_gap, the window is
    // placed with geometry::snap_rect. They should be identical.
    let mut diffs = vec![];
    for (w, h) in DIVISIBLE.iter().chain(ODD_SIZES.iter()) {
        for l in [
            Layout::default_columns(3),
            Layout::default_rows(5),
            Layout::grid(4, 4),
        ] {
            let raw = zone_rects(&l, Rect::new(0, 0, *w, *h));
            for (i, z) in l.zones.iter().enumerate() {
                let (x, y, zw, zh) = z.to_pixels_with_gap(*w as u32, *h as u32, 8);
                let a = Rect::new(x, y, zw as i32, zh as i32);
                let b = apply_gap(raw[i], 8);
                if a != b {
                    diffs.push(format!(
                        "{} {w}x{h} zone {i}: overlay={a:?} snap={b:?}",
                        l.name
                    ));
                }
            }
        }
    }
    assert!(
        diffs.is_empty(),
        "{} mismatches, e.g.\n{}",
        diffs.len(),
        diffs.iter().take(5).cloned().collect::<Vec<_>>().join("\n")
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/E-windows.md"]
fn small_zone_gap_never_grows_zone() {
    // to_pixels_with_gap clamps width/height to >= 100 px, making small zones
    // larger than the zone itself.
    let z = Zone::new(0.0, 0.0, 0.05, 0.05);
    let (_, _, w, h) = z.to_pixels_with_gap(1000, 1000, 8);
    assert!(w <= 50 && h <= 50, "zone 50x50 drawn as {w}x{h}");
}

// ───────────────────────── Persistence / multi-monitor ─────────────────────────

#[test]
fn custom_layout_persists_through_toml() {
    let mut cfg = FancyZonesConfig::default();
    let mut l = Layout::canvas("Mine", vec![Zone::new(0.1, 0.1, 0.3, 0.4)]);
    l.quick_key = Some(7);
    cfg.layouts.push(l);
    cfg.set_monitor_layout("DP-1", 6);
    let s = toml::to_string_pretty(&cfg).unwrap();
    let mut back: FancyZonesConfig = toml::from_str(&s).unwrap();
    back.normalize();
    assert_eq!(back.layouts.len(), 7);
    assert_eq!(back.layouts[6].name, "Mine");
    assert_eq!(back.layouts[6].quick_key, Some(7));
    assert!((back.layouts[6].zones[0].height - 0.4).abs() < 1e-6);
    assert_eq!(back.layout_index_for_monitor("DP-1"), 6);
    assert_eq!(back.layout_for_quick_key(7), Some(6));
}

#[test]
fn multi_monitor_targets_per_monitor_and_spanned() {
    use mpt_common::monitor::Monitor;
    let mons = vec![
        Monitor {
            name: "B".into(),
            x: 1920,
            y: 0,
            width: 1280,
            height: 1024,
            is_primary: false,
        },
        Monitor {
            name: "A".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: true,
        },
    ];
    let mut cfg = FancyZonesConfig::default();
    cfg.monitor_layouts.insert("B".into(), 1); // Focus
    let t = crate::snap::build_targets(&cfg, &mons);
    assert_eq!(
        t.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        vec!["A", "B"]
    );
    assert_eq!(t[1].zones[0].x >= 1920, true);
    cfg.span_zones_across_monitors = true;
    let t = crate::snap::build_targets(&cfg, &mons);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].area, Rect::new(0, 0, 3200, 1080));
    assert_eq!(
        geometry::bounding_box(&[Rect::new(0, 0, 1, 1)]),
        Some(Rect::new(0, 0, 1, 1))
    );
}
