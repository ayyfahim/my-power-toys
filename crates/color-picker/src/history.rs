use crate::color::Color;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Default history size (upstream `ColorHistoryLimit` default).
pub const MAX_HISTORY: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub color: Color,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ColorHistory {
    pub entries: Vec<HistoryEntry>,
}

impl ColorHistory {
    /// Load from ~/.config/my-power-toys/color-picker-history.json
    pub fn load() -> Self {
        let path = history_path();
        match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Add a color to history (most recent first, max 20).
    pub fn push(&mut self, color: Color) {
        self.push_with_limit(color, MAX_HISTORY);
    }

    /// Add a color to history, keeping at most `limit` entries (minimum 1).
    /// Re-picking a color already in history moves it to the front.
    pub fn push_with_limit(&mut self, color: Color, limit: usize) {
        self.entries
            .retain(|e| (e.color.r, e.color.g, e.color.b) != (color.r, color.g, color.b));
        let entry = HistoryEntry {
            color,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };
        self.entries.insert(0, entry);
        self.entries.truncate(limit.max(1));
    }

    /// Remove color at index.
    pub fn remove(&mut self, index: usize) {
        if index < self.entries.len() {
            self.entries.remove(index);
        }
    }

    /// Save to disk.
    pub fn save(&self) -> Result<()> {
        let path = history_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

fn history_path() -> std::path::PathBuf {
    mpt_common::config::config_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("color-picker-history.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_truncate() {
        let mut h = ColorHistory::default();
        for i in 0..25 {
            h.push(Color::new(i, i, i));
        }
        assert_eq!(h.entries.len(), MAX_HISTORY);
        // Most recent should be first
        assert_eq!(h.entries[0].color.r, 24);
    }

    #[test]
    fn custom_limit_and_dedupe() {
        let mut h = ColorHistory::default();
        for i in 0..10 {
            h.push_with_limit(Color::new(i, i, i), 3);
        }
        assert_eq!(h.entries.len(), 3);
        h.push_with_limit(Color::new(8, 8, 8), 3);
        assert_eq!(h.entries.len(), 3);
        assert_eq!(h.entries[0].color.r, 8);
        assert_eq!(h.entries[1].color.r, 9);
    }

    #[test]
    fn remove_entry() {
        let mut h = ColorHistory::default();
        h.push(Color::new(1, 1, 1));
        h.push(Color::new(2, 2, 2));
        h.remove(0);
        assert_eq!(h.entries.len(), 1);
        assert_eq!(h.entries[0].color.r, 1);
    }
}
