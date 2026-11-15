//! Usage counts for `sort_by_usage_frequency`, persisted in
//! `~/.config/my-power-toys/quick-accent-usage.toml`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

const USAGE_ID: &str = "quick-accent-usage";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct UsageCounts {
    /// Character (as a one-char string) -> times selected.
    #[serde(default)]
    pub counts: HashMap<String, u64>,
}

impl UsageCounts {
    pub fn load() -> Self {
        mpt_common::config::load_module_config(USAGE_ID).unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<()> {
        mpt_common::config::save_module_config(USAGE_ID, self)
    }

    pub fn count(&self, ch: char) -> u64 {
        self.counts.get(&ch.to_string()).copied().unwrap_or(0)
    }

    pub fn record(&mut self, ch: char) {
        *self.counts.entry(ch.to_string()).or_insert(0) += 1;
    }

    /// Load, increment and save in one go.
    pub fn record_and_save(ch: char) {
        let mut usage = Self::load();
        usage.record(ch);
        if let Err(e) = usage.save() {
            tracing::warn!("Quick Accent: failed to save usage counts: {e:#}");
        }
    }

    /// Stable sort, most used first; unused characters keep their order.
    pub fn sort(&self, chars: &mut [char]) {
        chars.sort_by_key(|c| std::cmp::Reverse(self.count(*c)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_by_count_stably() {
        let mut usage = UsageCounts::default();
        usage.record('ê');
        usage.record('ê');
        usage.record('ë');
        let mut chars = vec!['é', 'è', 'ê', 'ë'];
        usage.sort(&mut chars);
        assert_eq!(chars, vec!['ê', 'ë', 'é', 'è']);
    }

    #[test]
    fn round_trip() {
        let mut usage = UsageCounts::default();
        usage.record('ß');
        let s = toml::to_string(&usage).unwrap();
        let back: UsageCounts = toml::from_str(&s).unwrap();
        assert_eq!(back.count('ß'), 1);
        assert_eq!(back.count('x'), 0);
    }
}
