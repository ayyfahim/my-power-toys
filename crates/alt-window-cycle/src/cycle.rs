//! Pure ordering logic for cycling between windows of one application.
//! Mirrors upstream `AltWindowCycleLogic::WrapIndex` / `BeginCycle`.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// Wrap `index` into `0..count`. Returns 0 when `count` is 0.
pub fn wrap_index(index: isize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    index.rem_euclid(count as isize) as usize
}

/// Given the cycle order and the current window, pick the next/previous one.
/// If `current` is not in `order`, it is treated as index 0 (like upstream).
/// Returns `None` when there is nothing to switch to.
pub fn pick(order: &[u32], current: u32, dir: Direction) -> Option<usize> {
    if order.len() < 2 {
        return None;
    }
    let cur = order.iter().position(|&w| w == current).unwrap_or(0) as isize;
    let step = match dir {
        Direction::Forward => 1,
        Direction::Backward => -1,
    };
    let idx = wrap_index(cur + step, order.len());
    (order[idx] != current).then_some(idx)
}

/// Merge a previous cycle snapshot with the live window list: windows that
/// still exist keep their snapshot order, new windows are appended in live
/// order, and closed windows are dropped.
pub fn merge_snapshot(snapshot: &[u32], live: &[u32]) -> Vec<u32> {
    let mut out: Vec<u32> = snapshot
        .iter()
        .copied()
        .filter(|w| live.contains(w))
        .collect();
    for &w in live {
        if !out.contains(&w) {
            out.push(w);
        }
    }
    out
}

/// Decide the cycle order and target index.
///
/// `live_mru` is the app's windows, most recently used first.
/// `previous` is the order of a still-fresh earlier cycle (if any); it is
/// reused only when `active` belongs to it, so repeated presses walk through
/// all windows instead of toggling between the two most recent ones.
pub fn plan_cycle(
    previous: Option<&[u32]>,
    live_mru: &[u32],
    active: u32,
    dir: Direction,
) -> Option<(Vec<u32>, usize)> {
    let order = match previous {
        Some(prev) if prev.contains(&active) => merge_snapshot(prev, live_mru),
        _ => live_mru.to_vec(),
    };
    let idx = pick(&order, active, dir)?;
    Some((order, idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap() {
        assert_eq!(wrap_index(-1, 3), 2);
        assert_eq!(wrap_index(3, 3), 0);
        assert_eq!(wrap_index(5, 0), 0);
    }

    #[test]
    fn pick_forward_and_back() {
        let order = [10, 20, 30];
        assert_eq!(pick(&order, 10, Direction::Forward), Some(1));
        assert_eq!(pick(&order, 30, Direction::Forward), Some(0));
        assert_eq!(pick(&order, 10, Direction::Backward), Some(2));
        assert_eq!(pick(&order, 20, Direction::Backward), Some(0));
    }

    #[test]
    fn pick_needs_two_windows() {
        assert_eq!(pick(&[], 1, Direction::Forward), None);
        assert_eq!(pick(&[1], 1, Direction::Forward), None);
    }

    #[test]
    fn pick_unknown_current_starts_at_zero() {
        assert_eq!(pick(&[1, 2, 3], 99, Direction::Forward), Some(1));
    }

    #[test]
    fn merge_keeps_order_drops_closed_appends_new() {
        assert_eq!(merge_snapshot(&[1, 2, 3], &[3, 4, 1]), vec![1, 3, 4]);
    }

    #[test]
    fn fresh_cycle_uses_mru() {
        let (order, idx) = plan_cycle(None, &[5, 6, 7], 5, Direction::Forward).unwrap();
        assert_eq!(order[idx], 6);
    }

    #[test]
    fn continued_cycle_walks_all_windows() {
        // First press: A is active, MRU = A,B,C -> B.
        let (order, idx) = plan_cycle(None, &[1, 2, 3], 1, Direction::Forward).unwrap();
        assert_eq!(order[idx], 2);
        // B is now top of stacking: MRU = B,A,C. Continuing the cycle goes to C.
        let (order2, idx2) = plan_cycle(Some(&order), &[2, 1, 3], 2, Direction::Forward).unwrap();
        assert_eq!(order2[idx2], 3);
        // And then wraps to A.
        let (order3, idx3) = plan_cycle(Some(&order2), &[3, 2, 1], 3, Direction::Forward).unwrap();
        assert_eq!(order3[idx3], 1);
    }

    #[test]
    fn stale_snapshot_ignored_when_active_not_in_it() {
        let (order, idx) = plan_cycle(Some(&[8, 9]), &[1, 2], 1, Direction::Backward).unwrap();
        assert_eq!(order, vec![1, 2]);
        assert_eq!(order[idx], 2);
    }
}
