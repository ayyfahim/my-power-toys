use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Placement {
    #[default]
    Bottom,
    Top,
}

/// Encoding used when writing the hosts file (upstream `HostsEncoding`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum HostsEncoding {
    /// UTF-8 without byte order mark.
    #[default]
    Utf8,
    /// UTF-8 with a byte order mark (EF BB BF) at the start of the file.
    Utf8Bom,
}

/// Automatic backup cleanup policy (upstream `HostsDeleteBackupMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum DeleteBackupsMode {
    /// Keep every backup.
    Never,
    /// Keep only the newest `delete_backups_count` backups.
    Count,
    /// Delete backups older than `delete_backups_days`, always keeping the
    /// newest `delete_backups_count` (0 = no minimum).
    #[default]
    Age,
}

fn default_true() -> bool {
    true
}

fn default_delete_backups_days() -> u32 {
    15
}

fn default_delete_backups_count() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostsEditorConfig {
    #[serde(default = "default_true")]
    pub show_disabled: bool,
    /// Upstream `BackupHosts`.
    #[serde(default = "default_true")]
    pub backup_before_save: bool,
    /// Upstream `AdditionalLinesPosition`-like placement for new entries.
    #[serde(default)]
    pub new_entry_placement: Placement,
    /// Upstream `LoopbackDuplicates`: also flag duplicates among loopback /
    /// unspecified addresses (127.0.0.1, ::1, 0.0.0.0, ...).
    #[serde(default)]
    pub loopback_duplicates: bool,
    /// Upstream `Encoding`.
    #[serde(default)]
    pub encoding: HostsEncoding,
    /// Upstream `NoLeadingSpaces`: don't indent active entries with two
    /// spaces when the file also contains disabled (`# `) entries.
    #[serde(default)]
    pub no_leading_spaces: bool,
    /// Upstream `ShowStartupWarning`: show a warning before editing.
    #[serde(default = "default_true")]
    pub show_startup_warning: bool,
    /// Upstream `BackupPath`. Empty = default
    /// (`~/.local/share/my-power-toys/hosts-backups`).
    #[serde(default)]
    pub backup_path: String,
    /// Upstream `DeleteBackupsMode`.
    #[serde(default)]
    pub delete_backups_mode: DeleteBackupsMode,
    /// Upstream `DeleteBackupsDays`.
    #[serde(default = "default_delete_backups_days")]
    pub delete_backups_days: u32,
    /// Upstream `DeleteBackupsCount`.
    #[serde(default = "default_delete_backups_count")]
    pub delete_backups_count: u32,
}

impl Default for HostsEditorConfig {
    fn default() -> Self {
        Self {
            show_disabled: true,
            backup_before_save: true,
            new_entry_placement: Placement::default(),
            loopback_duplicates: false,
            encoding: HostsEncoding::default(),
            no_leading_spaces: false,
            show_startup_warning: true,
            backup_path: String::new(),
            delete_backups_mode: DeleteBackupsMode::default(),
            delete_backups_days: default_delete_backups_days(),
            delete_backups_count: default_delete_backups_count(),
        }
    }
}

impl HostsEditorConfig {
    /// Directory where backups are written (resolves the empty default).
    pub fn resolved_backup_dir(&self) -> PathBuf {
        let trimmed = self.backup_path.trim();
        if !trimmed.is_empty() {
            if let Some(rest) = trimmed.strip_prefix("~/")
                && let Some(home) = std::env::var_os("HOME")
            {
                return PathBuf::from(home).join(rest);
            }
            return PathBuf::from(trimmed);
        }
        default_backup_dir()
    }
}

pub fn default_backup_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("my-power-toys").join("hosts-backups")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_config_loads_with_defaults() {
        let cfg: HostsEditorConfig = toml::from_str("show_disabled = false\n").unwrap();
        assert!(!cfg.show_disabled);
        assert!(cfg.backup_before_save);
        assert!(cfg.show_startup_warning);
        assert_eq!(cfg.delete_backups_mode, DeleteBackupsMode::Age);
        assert_eq!(cfg.delete_backups_days, 15);
        assert_eq!(cfg.delete_backups_count, 5);
        assert_eq!(cfg.encoding, HostsEncoding::Utf8);
    }

    #[test]
    fn config_roundtrip() {
        let cfg = HostsEditorConfig {
            encoding: HostsEncoding::Utf8Bom,
            delete_backups_mode: DeleteBackupsMode::Count,
            backup_path: "/var/backups".into(),
            ..Default::default()
        };
        let s = toml::to_string(&cfg).unwrap();
        let back: HostsEditorConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.encoding, HostsEncoding::Utf8Bom);
        assert_eq!(back.delete_backups_mode, DeleteBackupsMode::Count);
        assert_eq!(back.resolved_backup_dir(), PathBuf::from("/var/backups"));
    }
}
