use serde::{Deserialize, Serialize};

/// How to apply the search/replace operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ApplyTo {
    /// Only modify the filename (stem), keep extension untouched.
    FilenameOnly,
    /// Only modify the extension, keep filename untouched.
    ExtensionOnly,
    /// Modify both filename and extension together.
    #[default]
    FilenameAndExtension,
}

/// Post-processing text formatting to apply after replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TextFormatting {
    #[default]
    None,
    /// all lowercase
    Lowercase,
    /// ALL UPPERCASE
    Uppercase,
    /// Title case (first character of the name capitalised)
    TitleCase,
    /// Capitalize Each Word
    CapitalizeEachWord,
}

/// Which file timestamp feeds the `$YYYY`, `$MM`, ... tokens
/// (upstream CreationTime / ModificationTime / AccessTime flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TimeSource {
    #[default]
    Created,
    Modified,
    Accessed,
}

fn default_true() -> bool {
    true
}

fn default_max_mru() -> usize {
    10
}

/// Persisted configuration for the Bulk Rename module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkRenameConfig {
    /// Interpret the search field as a regular expression.
    #[serde(default)]
    pub use_regex: bool,

    /// Replace every occurrence (true) or only the first one (false).
    #[serde(default = "default_true")]
    pub match_all_occurrences: bool,

    /// Case-sensitive matching.
    #[serde(default)]
    pub case_sensitive: bool,

    /// Which part of the filename the operation targets.
    #[serde(default)]
    pub apply_to: ApplyTo,

    /// Include folders (not just files) in the listing.
    #[serde(default)]
    pub include_folders: bool,

    /// Recurse into subfolders when listing entries.
    #[serde(default)]
    pub include_subfolders: bool,

    /// Post-replacement text formatting.
    #[serde(default)]
    pub text_formatting: TextFormatting,

    /// Enable enumeration patterns (`${}`) in the replacement string.
    #[serde(default)]
    pub enumerate_items: bool,

    /// Enable randomizer patterns (`${rstringalnum=N}`, `${rstringalpha=N}`,
    /// `${rstringdigit=N}`, `${ruuidv4}`) in the replacement string.
    #[serde(default)]
    pub randomize_items: bool,

    /// File time used for date/time tokens.
    #[serde(default)]
    pub time_source: TimeSource,

    /// Upstream `PersistState`: remember the last search/replace text and
    /// options between sessions.
    #[serde(default = "default_true")]
    pub persist_state: bool,

    /// Upstream `MRUEnabled`: keep a history of search/replace terms.
    #[serde(default = "default_true")]
    pub mru_enabled: bool,

    /// Upstream `MaxMRUSize`.
    #[serde(default = "default_max_mru")]
    pub max_mru_size: usize,
}

/// Session state persisted separately from the settings
/// (`~/.config/my-power-toys/bulk-rename-state.toml`), so the settings UI
/// never overwrites the history.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BulkRenameState {
    #[serde(default)]
    pub search: String,
    #[serde(default)]
    pub replace: String,
    #[serde(default)]
    pub search_mru: Vec<String>,
    #[serde(default)]
    pub replace_mru: Vec<String>,
}

/// Module id used for the state file.
pub const STATE_ID: &str = "bulk-rename-state";

impl BulkRenameState {
    pub fn load() -> Self {
        mpt_common::config::load_module_config(STATE_ID).unwrap_or_default()
    }

    pub fn save(&self) {
        if let Err(e) = mpt_common::config::save_module_config(STATE_ID, self) {
            tracing::warn!("Failed to save bulk-rename state: {e}");
        }
    }
}

/// Insert `item` at the front of an MRU list (deduplicated, capped at `max`).
pub fn push_mru(list: &mut Vec<String>, item: &str, max: usize) {
    if item.is_empty() {
        return;
    }
    list.retain(|s| s != item);
    list.insert(0, item.to_string());
    list.truncate(max);
}

impl Default for BulkRenameConfig {
    fn default() -> Self {
        Self {
            use_regex: false,
            match_all_occurrences: true,
            case_sensitive: false,
            apply_to: ApplyTo::default(),
            include_folders: false,
            include_subfolders: false,
            text_formatting: TextFormatting::default(),
            enumerate_items: false,
            randomize_items: false,
            time_source: TimeSource::default(),
            persist_state: true,
            mru_enabled: true,
            max_mru_size: default_max_mru(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mru_dedup_and_cap() {
        let mut l = Vec::new();
        push_mru(&mut l, "a", 3);
        push_mru(&mut l, "b", 3);
        push_mru(&mut l, "c", 3);
        push_mru(&mut l, "a", 3);
        assert_eq!(l, ["a", "c", "b"]);
        push_mru(&mut l, "d", 3);
        assert_eq!(l, ["d", "a", "c"]);
        push_mru(&mut l, "", 3);
        assert_eq!(l.len(), 3);
        push_mru(&mut l, "e", 0);
        assert!(l.is_empty());
    }

    #[test]
    fn old_config_gets_defaults() {
        let cfg: BulkRenameConfig = toml::from_str("use_regex = true\n").unwrap();
        assert!(cfg.use_regex);
        assert!(cfg.persist_state);
        assert!(cfg.mru_enabled);
        assert_eq!(cfg.max_mru_size, 10);
        assert_eq!(cfg.time_source, TimeSource::Created);
    }
}
