//! Snapping operations shared by drag-to-snap, keyboard moves, quick layout
//! switching and app-last-zone placement.

use crate::config::FancyZonesConfig;
use crate::geometry::{
    self, Direction, IndexMove, Rect, adjacent_zone_by_position, adjacent_zone_index,
    zones_matching_rect,
};
use crate::layout::Layout;
use crate::state::{SnapState, ZoneRef, app_key};
use crate::x11::Wm;
use crate::zone_window::{SurfaceSpec, ZoneStyle};
use anyhow::{Context, Result};
use mpt_common::monitor::Monitor;
use tracing::{info, warn};

/// Name used for the single target when `span_zones_across_monitors` is on.
pub const SPAN_ALL: &str = "*";

/// A monitor (or the whole desktop) with its active layout resolved to pixels.
#[derive(Debug, Clone)]
pub struct Target {
    pub name: String,
    pub area: Rect,
    pub layout: Layout,
    /// Raw zone rects (no gap) in absolute coordinates.
    pub zones: Vec<Rect>,
}

impl Target {
    pub fn spec(&self, label: Option<String>) -> SurfaceSpec {
        SurfaceSpec {
            area: self.area,
            zones: self.zones.clone(),
            label,
        }
    }

    pub fn snap_rect(&self, zones: &[usize], gap: u32) -> Option<Rect> {
        geometry::snap_rect(&self.zones, zones, gap)
    }
}

/// Detect monitors, falling back to the X screen size.
pub fn monitors() -> Vec<Monitor> {
    match mpt_common::monitor::detect_monitors() {
        Ok(m) if !m.is_empty() => m,
        _ => crate::x11::get_screen_geometry()
            .map(|(w, h)| {
                vec![Monitor {
                    name: "Screen".into(),
                    x: 0,
                    y: 0,
                    width: w,
                    height: h,
                    is_primary: true,
                }]
            })
            .unwrap_or_default(),
    }
}

/// Resolve per-monitor layouts into pixel targets, ordered left-to-right.
pub fn build_targets(cfg: &FancyZonesConfig, monitors: &[Monitor]) -> Vec<Target> {
    let make = |name: &str, layout_of: &str, area: Rect| {
        let layout = cfg
            .layout_for_monitor(layout_of)
            .cloned()
            .unwrap_or_else(Layout::no_layout);
        Target {
            name: name.to_string(),
            area,
            zones: geometry::zone_rects(&layout, area),
            layout,
        }
    };
    if cfg.span_zones_across_monitors && monitors.len() > 1 {
        let rects: Vec<Rect> = monitors.iter().map(Rect::from_monitor).collect();
        let primary = monitors
            .iter()
            .find(|m| m.is_primary)
            .unwrap_or(&monitors[0]);
        if let Some(bb) = geometry::bounding_box(&rects) {
            return vec![make(SPAN_ALL, &primary.name, bb)];
        }
    }
    let mut targets: Vec<Target> = monitors
        .iter()
        .map(|m| make(&m.name, &m.name, Rect::from_monitor(m)))
        .collect();
    targets.sort_by_key(|t| (t.area.x, t.area.y));
    targets
}

/// Index of the target under the point (or nearest by overlap).
pub fn target_at(targets: &[Target], x: i32, y: i32) -> Option<usize> {
    let areas: Vec<Rect> = targets.iter().map(|t| t.area).collect();
    geometry::monitor_at_point(&areas, x, y)
        .or_else(|| geometry::monitor_for_rect(&areas, &Rect::new(x, y, 1, 1)))
}

/// Move `window` into `zones` of `target`, record it in the state file
/// (WM_CLASS -> zone, window -> zone + size before snapping).
pub fn snap_window(
    wm: &Wm,
    cfg: &FancyZonesConfig,
    window: u32,
    target: &Target,
    zones: &[usize],
    size_before: Option<(u32, u32)>,
) -> Result<()> {
    let rect = target
        .snap_rect(zones, cfg.zone_gap)
        .context("no zone selected")?;
    wm.move_resize(window, rect)?;
    let app = wm.wm_class(window).ok().and_then(|c| app_key(&c));
    let zone = ZoneRef {
        monitor: target.name.clone(),
        zones: zones.to_vec(),
    };
    SnapState::update(|s| s.record_snap(window, app.as_deref(), zone, size_before));
    info!(
        "FancyZones: snapped window {window} to {} zone(s) {zones:?}",
        target.name
    );
    Ok(())
}

/// Current snap of a window: from the state file, else by matching its geometry.
fn current_snap(
    wm: &Wm,
    cfg: &FancyZonesConfig,
    targets: &[Target],
    window: u32,
) -> Option<(usize, Vec<usize>)> {
    let state = SnapState::load();
    if let Some(ws) = state.windows.get(&window)
        && let Some(t) = targets.iter().position(|t| t.name == ws.zone.monitor)
        && ws.zone.zones.iter().all(|&z| z < targets[t].zones.len())
        && !ws.zone.zones.is_empty()
    {
        return Some((t, ws.zone.zones.clone()));
    }
    let outer = wm.outer_geometry(window)?;
    targets
        .iter()
        .enumerate()
        .find_map(|(i, t)| zones_matching_rect(&t.zones, &outer, cfg.zone_gap, 4).map(|z| (i, z)))
}

/// Super+Arrow: move the active window to the adjacent zone.
pub fn move_active_window(cfg: &FancyZonesConfig, dir: Direction) -> Result<()> {
    let wm = Wm::connect()?;
    let window = wm.active_window().context("no active window")?;
    if let Ok(class) = wm.wm_class(window)
        && cfg.is_excluded(&class)
    {
        info!("FancyZones: window {window} is excluded, ignoring move");
        return Ok(());
    }
    let targets = build_targets(cfg, &monitors());
    if targets.is_empty() {
        anyhow::bail!("no monitors detected");
    }
    let outer = wm.outer_geometry(window).context("window geometry")?;
    let current = current_snap(&wm, cfg, &targets, window);
    let areas: Vec<Rect> = targets.iter().map(|t| t.area).collect();
    let cur_t = current
        .as_ref()
        .map(|(t, _)| *t)
        .or_else(|| geometry::monitor_for_rect(&areas, &outer))
        .unwrap_or(0);

    let dest: Option<(usize, Vec<usize>)> = if cfg.move_windows_based_on_position {
        let from = current
            .as_ref()
            .and_then(|(t, z)| geometry::union_of(&targets[*t].zones, z))
            .unwrap_or(outer);
        let pool: Vec<(usize, usize)> = targets
            .iter()
            .enumerate()
            .filter(|(i, _)| cfg.move_windows_across_monitors || *i == cur_t)
            .flat_map(|(i, t)| (0..t.zones.len()).map(move |z| (i, z)))
            .collect();
        let rects: Vec<Rect> = pool.iter().map(|&(t, z)| targets[t].zones[z]).collect();
        adjacent_zone_by_position(&rects, from, dir).map(|k| (pool[k].0, vec![pool[k].1]))
    } else {
        let len = targets[cur_t].zones.len();
        let forward = matches!(dir, Direction::Right | Direction::Down);
        let cur_zone = current.as_ref().and_then(|(_, z)| {
            if forward {
                z.iter().max().copied()
            } else {
                z.iter().min().copied()
            }
        });
        match adjacent_zone_index(len, cur_zone, dir) {
            IndexMove::Zone(z) => Some((cur_t, vec![z])),
            IndexMove::Overflow => {
                let n = targets.len();
                // Find the next monitor in that direction that has zones.
                let next = if cfg.move_windows_across_monitors && n > 1 {
                    (1..n)
                        .map(|k| {
                            if forward {
                                (cur_t + k) % n
                            } else {
                                (cur_t + n - k) % n
                            }
                        })
                        .find(|&t| !targets[t].zones.is_empty())
                } else {
                    None
                };
                let t = next.unwrap_or(cur_t);
                let len = targets[t].zones.len();
                (len > 0).then(|| (t, vec![if forward { 0 } else { len - 1 }]))
            }
        }
    };

    let Some((t, zones)) = dest else {
        info!("FancyZones: no zone in direction {dir:?}");
        return Ok(());
    };
    let size_before = if current.is_none() {
        wm.geometry(window).map(|g| (g.w as u32, g.h as u32))
    } else {
        None
    };
    snap_window(&wm, cfg, window, &targets[t], &zones, size_before)
}

/// Super+Ctrl+Alt+<key>: switch the layout of the monitor under the cursor.
pub fn quick_switch(key: u8) -> Result<()> {
    let mut cfg = FancyZonesConfig::load();
    let idx = cfg
        .layout_for_quick_key(key)
        .with_context(|| format!("no layout for quick key {key}"))?;
    let wm = Wm::connect()?;
    let mons = monitors();
    let (px, py) = wm.pointer().map(|p| (p.x, p.y)).unwrap_or((0, 0));
    let rects: Vec<Rect> = mons.iter().map(Rect::from_monitor).collect();
    let mon = if cfg.span_zones_across_monitors {
        mons.iter().find(|m| m.is_primary).or(mons.first())
    } else {
        geometry::monitor_at_point(&rects, px, py)
            .map(|i| &mons[i])
            .or_else(|| mons.iter().find(|m| m.is_primary))
            .or(mons.first())
    }
    .context("no monitors detected")?
    .clone();
    cfg.set_monitor_layout(&mon.name, idx);
    mpt_common::config::save_module_config("fancy-zones", &cfg)?;
    let name = cfg.layouts[idx].name.clone();
    info!("FancyZones: layout '{name}' applied to {}", mon.name);

    if cfg.flash_zones_on_quick_switch {
        let targets = build_targets(&cfg, &mons);
        let target_name = if cfg.span_zones_across_monitors && mons.len() > 1 {
            SPAN_ALL.to_string()
        } else {
            mon.name.clone()
        };
        if let Some(t) = targets.into_iter().find(|t| t.name == target_name) {
            let style = ZoneStyle::from_config(&cfg);
            let duration =
                std::time::Duration::from_millis(cfg.flash_duration_ms.clamp(100, 10_000));
            std::thread::spawn(move || {
                if let Err(e) = crate::zone_window::flash(style, vec![t.spec(Some(name))], duration)
                {
                    warn!("FancyZones: flash failed: {e}");
                }
            });
        }
    }
    Ok(())
}

/// Place a newly opened window into the zone its app last used.
pub fn apply_app_last_zone(
    wm: &Wm,
    cfg: &FancyZonesConfig,
    targets: &[Target],
    window: u32,
) -> Result<bool> {
    if !wm.is_normal_window(window) {
        return Ok(false);
    }
    let class = wm.wm_class(window)?;
    if cfg.is_excluded(&class) {
        return Ok(false);
    }
    let Some(key) = app_key(&class) else {
        return Ok(false);
    };
    let state = SnapState::load();
    let Some(zone) = state.app_zones.get(&key) else {
        return Ok(false);
    };
    // Same monitor if it still exists, else the monitor the window opened on.
    let t = targets
        .iter()
        .position(|t| t.name == zone.monitor)
        .or_else(|| {
            let outer = wm.outer_geometry(window)?;
            let areas: Vec<Rect> = targets.iter().map(|t| t.area).collect();
            geometry::monitor_for_rect(&areas, &outer)
        });
    let Some(t) = t else { return Ok(false) };
    let target = &targets[t];
    if zone.zones.is_empty() || zone.zones.iter().any(|&z| z >= target.zones.len()) {
        return Ok(false);
    }
    let size_before = wm.geometry(window).map(|g| (g.w as u32, g.h as u32));
    snap_window(wm, cfg, window, target, &zone.zones.clone(), size_before)?;
    Ok(true)
}
