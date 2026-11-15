//! Coalesces rapid writes (slider drags) so only the latest value per key is
//! sent to the (slow) monitor.

/// Keeps the latest value per key, in first-seen order.
#[derive(Debug, Clone)]
pub struct Coalescer<K, V> {
    pending: Vec<(K, V)>,
}

impl<K, V> Default for Coalescer<K, V> {
    fn default() -> Self {
        Self { pending: vec![] }
    }
}

impl<K: PartialEq, V> Coalescer<K, V> {
    pub fn push(&mut self, key: K, value: V) {
        match self.pending.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.pending.push((key, value)),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    pub fn take(&mut self) -> Vec<(K, V)> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_wins_order_kept() {
        let mut c = Coalescer::default();
        c.push(("a", 1), 10);
        c.push(("b", 1), 20);
        c.push(("a", 1), 30);
        c.push(("a", 2), 40);
        assert_eq!(
            c.take(),
            vec![(("a", 1), 30), (("b", 1), 20), (("a", 2), 40)]
        );
        assert!(c.is_empty());
    }
}
