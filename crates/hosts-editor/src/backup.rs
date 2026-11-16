//! Hosts file backups and automatic cleanup (upstream `BackupManager`).

use crate::config::{DeleteBackupsMode, HostsEditorConfig};
use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};
use tracing::{info, warn};

/// Marker contained in every backup file name, e.g. `hosts.mpt-backup-20250101120000`.
pub const BACKUP_MARKER: &str = ".mpt-backup-";

/// A backup file found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupFile {
    pub path: PathBuf,
    pub created: SystemTime,
}

/// Decide which backups to delete (pure; mirrors upstream semantics).
///
/// - `Never`: nothing.
/// - `Count`: keep the newest `count` backups (`count < 1` = keep all).
/// - `Age`: delete backups older than `days` (`days < 1` = keep all); when
///   `count > 0`, the newest `count` backups are always kept.
pub fn backups_to_delete(
    backups: &[BackupFile],
    mode: DeleteBackupsMode,
    days: u32,
    count: u32,
    now: SystemTime,
) -> Vec<PathBuf> {
    let mut sorted: Vec<&BackupFile> = backups.iter().collect();
    // Newest first.
    sorted.sort_by(|a, b| b.created.cmp(&a.created));

    match mode {
        DeleteBackupsMode::Never => Vec::new(),
        DeleteBackupsMode::Count => {
            if count < 1 {
                return Vec::new();
            }
            sorted
                .into_iter()
                .skip(count as usize)
                .map(|b| b.path.clone())
                .collect()
        }
        DeleteBackupsMode::Age => {
            if days < 1 {
                return Vec::new();
            }
            let cutoff = now
                .checked_sub(Duration::from_secs(u64::from(days) * 86_400))
                .unwrap_or(SystemTime::UNIX_EPOCH);
            sorted
                .into_iter()
                .skip(count as usize)
                .filter(|b| b.created < cutoff)
                .map(|b| b.path.clone())
                .collect()
        }
    }
}

/// List backup files in `dir`.
pub fn list_backups(dir: &Path) -> Vec<BackupFile> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    read.flatten()
        .filter(|e| e.file_name().to_string_lossy().contains(BACKUP_MARKER))
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            let created = meta.created().or_else(|_| meta.modified()).ok()?;
            Some(BackupFile {
                path: e.path(),
                created,
            })
        })
        .collect()
}

/// Copy `hosts_path` into the configured backup directory. Returns the backup path.
pub fn create_backup(hosts_path: &Path, config: &HostsEditorConfig) -> Result<PathBuf> {
    let dir = config.resolved_backup_dir();
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = hosts_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "hosts".into());
    let target = dir.join(format!("{name}{BACKUP_MARKER}{}", timestamp_now()));
    fs::copy(hosts_path, &target)
        .with_context(|| format!("copying {} to {}", hosts_path.display(), target.display()))?;
    info!("Hosts backup saved to {}", target.display());
    Ok(target)
}

/// Apply the configured cleanup policy to the backup directory.
pub fn cleanup_backups(config: &HostsEditorConfig) -> usize {
    let dir = config.resolved_backup_dir();
    let to_delete = backups_to_delete(
        &list_backups(&dir),
        config.delete_backups_mode,
        config.delete_backups_days,
        config.delete_backups_count,
        SystemTime::now(),
    );
    let mut deleted = 0;
    for path in to_delete {
        match fs::remove_file(&path) {
            Ok(()) => {
                deleted += 1;
                info!("Deleted old hosts backup {}", path.display());
            }
            Err(e) => warn!("Failed to delete backup {}: {e}", path.display()),
        }
    }
    deleted
}

/// Current UTC time as `YYYYMMDDhhmmss`.
pub fn timestamp_now() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_timestamp(secs)
}

fn format_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}{m:02}{d:02}{:02}{:02}{:02}",
        rem / 3600,
        (rem / 60) % 60,
        rem % 60
    )
}

/// Days since 1970-01-01 → (year, month, day). Howard Hinnant's algorithm.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(name: &str, days_ago: u64, now: SystemTime) -> BackupFile {
        BackupFile {
            path: PathBuf::from(name),
            created: now - Duration::from_secs(days_ago * 86_400),
        }
    }

    fn names(v: Vec<PathBuf>) -> Vec<String> {
        let mut v: Vec<String> = v
            .into_iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn never_deletes_nothing() {
        let now = SystemTime::now();
        let all = vec![b("a", 100, now), b("b", 200, now)];
        assert!(backups_to_delete(&all, DeleteBackupsMode::Never, 1, 1, now).is_empty());
    }

    #[test]
    fn count_keeps_newest() {
        let now = SystemTime::now();
        let all = vec![
            b("a", 3, now),
            b("b", 1, now),
            b("c", 2, now),
            b("d", 4, now),
        ];
        let del = backups_to_delete(&all, DeleteBackupsMode::Count, 0, 2, now);
        assert_eq!(names(del), vec!["a", "d"]);
        assert!(backups_to_delete(&all, DeleteBackupsMode::Count, 0, 0, now).is_empty());
    }

    #[test]
    fn age_respects_days_and_minimum_count() {
        let now = SystemTime::now();
        let all = vec![
            b("new", 1, now),
            b("mid", 10, now),
            b("old1", 20, now),
            b("old2", 30, now),
        ];
        // No minimum: everything older than 15 days goes.
        let del = backups_to_delete(&all, DeleteBackupsMode::Age, 15, 0, now);
        assert_eq!(names(del), vec!["old1", "old2"]);
        // Keep at least 3 newest: only old2 goes.
        let del = backups_to_delete(&all, DeleteBackupsMode::Age, 15, 3, now);
        assert_eq!(names(del), vec!["old2"]);
        // days < 1 disables age cleanup.
        assert!(backups_to_delete(&all, DeleteBackupsMode::Age, 0, 0, now).is_empty());
    }

    #[test]
    fn timestamp_format() {
        assert_eq!(format_timestamp(0), "19700101000000");
        // 2024-02-29 12:34:56 UTC
        assert_eq!(format_timestamp(1_709_210_096), "20240229123456");
    }

    #[test]
    fn create_and_list_on_disk() {
        let tmp = std::env::temp_dir().join(format!("mpt-hosts-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let hosts = tmp.join("hosts");
        fs::write(&hosts, "127.0.0.1 localhost\n").unwrap();
        let cfg = HostsEditorConfig {
            backup_path: tmp.join("bk").to_string_lossy().into_owned(),
            delete_backups_mode: DeleteBackupsMode::Count,
            delete_backups_count: 1,
            ..Default::default()
        };
        let p = create_backup(&hosts, &cfg).unwrap();
        assert!(p.exists());
        fs::write(tmp.join("bk").join("hosts.mpt-backup-19990101000000"), "x").unwrap();
        assert_eq!(list_backups(&tmp.join("bk")).len(), 2);
        let _ = fs::remove_dir_all(&tmp);
    }
}
