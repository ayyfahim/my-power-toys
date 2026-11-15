//! Internal laptop panels via `/sys/class/backlight/*`.
//!
//! Writing sysfs normally needs root, so writes go through (in order):
//! `brightnessctl`, systemd-logind `SetBrightness` (via `busctl`), and finally
//! a direct sysfs write.

use crate::cmd;
use anyhow::{Context, Result, bail};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::debug;

const SYSFS_BACKLIGHT: &str = "/sys/class/backlight";
const TOOL_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklightDevice {
    /// Device name, e.g. `intel_backlight`.
    pub name: String,
    pub max_brightness: u32,
    /// DRM connector of the panel if sysfs links it (e.g. `card0-eDP-1`).
    pub drm_connector: Option<String>,
    /// `raw`, `platform` or `firmware`.
    pub kind: String,
}

fn read_u32(path: &Path) -> Result<u32> {
    let s = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    s.trim()
        .parse()
        .with_context(|| format!("parse {}", path.display()))
}

/// List backlight devices. When several exist for one panel (e.g.
/// `acpi_video0` + `intel_backlight`) the preferred one is kept, using the
/// kernel's own preference order: firmware > platform > raw.
pub fn list_devices() -> Vec<BacklightDevice> {
    list_devices_in(Path::new(SYSFS_BACKLIGHT))
}

pub fn list_devices_in(root: &Path) -> Vec<BacklightDevice> {
    let mut devices = scan(root);
    devices.sort_by_key(|d| (kind_rank(&d.kind), d.name.clone()));
    // Laptops usually have one panel; keep only the best device unless the
    // devices are clearly tied to different connectors.
    let mut seen_connectors: Vec<Option<String>> = Vec::new();
    devices.retain(|d| {
        let key = d.drm_connector.clone();
        if seen_connectors
            .iter()
            .any(|c| c.is_none() || *c == key || key.is_none())
        {
            false
        } else {
            seen_connectors.push(key);
            true
        }
    });
    devices
}

/// Look up one device by name, without de-duplication.
pub fn find_device(name: &str) -> Option<BacklightDevice> {
    scan(Path::new(SYSFS_BACKLIGHT))
        .into_iter()
        .find(|d| d.name == name)
}

/// All devices under `root`, unsorted and not de-duplicated.
pub fn scan(root: &Path) -> Vec<BacklightDevice> {
    let Ok(entries) = fs::read_dir(root) else {
        return vec![];
    };
    let mut devices = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let Ok(max) = read_u32(&path.join("max_brightness")) else {
            continue;
        };
        if max == 0 {
            continue;
        }
        let kind = fs::read_to_string(path.join("type"))
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "raw".into());
        // /sys/class/backlight/X/device -> .../drm/card0/card0-eDP-1 for many drivers.
        let drm_connector = fs::read_link(path.join("device"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .filter(|n| n.starts_with("card") && n.contains('-'));
        devices.push(BacklightDevice {
            name,
            max_brightness: max,
            drm_connector,
            kind,
        });
    }
    devices
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        "firmware" => 0,
        "platform" => 1,
        _ => 2,
    }
}

pub fn device_path(name: &str) -> PathBuf {
    Path::new(SYSFS_BACKLIGHT).join(name)
}

pub fn read_brightness(name: &str) -> Result<u32> {
    let p = device_path(name);
    // actual_brightness reflects the hardware; fall back to brightness.
    read_u32(&p.join("actual_brightness")).or_else(|_| read_u32(&p.join("brightness")))
}

/// Convert a raw value to a 0..=100 percentage.
pub fn raw_to_percent(raw: u32, max: u32) -> u8 {
    if max == 0 {
        return 0;
    }
    ((raw as u64 * 100 + max as u64 / 2) / max as u64).min(100) as u8
}

/// Convert a 0..=100 percentage to a raw value.
pub fn percent_to_raw(percent: u8, max: u32) -> u32 {
    ((percent.min(100) as u64 * max as u64 + 50) / 100) as u32
}

pub fn write_brightness(name: &str, raw: u32) -> Result<()> {
    // Device names come from sysfs directory entries; still reject anything odd
    // before handing them to other tools.
    if name.is_empty() || name.contains('/') || name.starts_with('-') {
        bail!("invalid backlight device name {name:?}");
    }
    let value = raw.to_string();

    if cmd::in_path("brightnessctl") {
        let args = vec![
            "--device".to_string(),
            name.to_string(),
            "set".to_string(),
            value.clone(),
        ];
        match cmd::run("brightnessctl", &args, TOOL_TIMEOUT) {
            Ok(o) if o.success => return Ok(()),
            Ok(o) => debug!("brightnessctl failed: {}", o.stderr.trim()),
            Err(e) => debug!("brightnessctl: {e}"),
        }
    }

    // systemd-logind lets the active session set backlight without root.
    if cmd::in_path("busctl") {
        let args: Vec<String> = [
            "call",
            "org.freedesktop.login1",
            "/org/freedesktop/login1/session/auto",
            "org.freedesktop.login1.Session",
            "SetBrightness",
            "ssu",
            "backlight",
            name,
            &value,
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        match cmd::run("busctl", &args, TOOL_TIMEOUT) {
            Ok(o) if o.success => return Ok(()),
            Ok(o) => debug!("logind SetBrightness failed: {}", o.stderr.trim()),
            Err(e) => debug!("busctl: {e}"),
        }
    }

    let path = device_path(name).join("brightness");
    match fs::write(&path, &value) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::PermissionDenied => bail!(
            "permission denied writing {}: install brightnessctl, or add a udev rule \
             granting the video group write access",
            path.display()
        ),
        Err(e) => Err(e).with_context(|| format!("write {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_conversion() {
        assert_eq!(raw_to_percent(0, 255), 0);
        assert_eq!(raw_to_percent(255, 255), 100);
        assert_eq!(raw_to_percent(128, 255), 50);
        assert_eq!(raw_to_percent(10, 0), 0);
        assert_eq!(percent_to_raw(100, 96000), 96000);
        assert_eq!(percent_to_raw(50, 255), 128);
        assert_eq!(percent_to_raw(200, 10), 10);
        for p in 0..=100u8 {
            assert_eq!(raw_to_percent(percent_to_raw(p, 1000), 1000), p);
        }
    }

    #[test]
    fn list_from_fake_sysfs() {
        let dir = std::env::temp_dir().join(format!("mpt-pd-bl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        for (name, max, kind) in [
            ("acpi_video0", "15", "firmware"),
            ("intel_backlight", "96000", "raw"),
        ] {
            let d = dir.join(name);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("max_brightness"), max).unwrap();
            fs::write(d.join("type"), kind).unwrap();
        }
        let devs = list_devices_in(&dir);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(devs.len(), 1);
        assert_eq!(devs[0].name, "acpi_video0");
        assert_eq!(devs[0].max_brightness, 15);
    }
}
