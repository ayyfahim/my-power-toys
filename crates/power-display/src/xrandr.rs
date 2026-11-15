//! Software brightness (gamma scaling) and rotation via `xrandr` on X11.

use crate::cmd;
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(10);

/// Lowest software brightness we allow, so a screen can't be blacked out.
pub const MIN_SOFTWARE_BRIGHTNESS: f32 = 0.10;

#[derive(Debug, Clone, PartialEq)]
pub struct XrandrOutput {
    pub name: String,
    pub primary: bool,
    /// Software brightness multiplier (1.0 = normal).
    pub brightness: f32,
    pub rotation: Rotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Rotation {
    #[default]
    Normal,
    Left,
    Right,
    Inverted,
}

impl Rotation {
    pub const ALL: [Rotation; 4] = [
        Rotation::Normal,
        Rotation::Left,
        Rotation::Right,
        Rotation::Inverted,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Rotation::Normal => "normal",
            Rotation::Left => "left",
            Rotation::Right => "right",
            Rotation::Inverted => "inverted",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Rotation::Normal => "Landscape",
            Rotation::Left => "Portrait (left)",
            Rotation::Right => "Portrait (right)",
            Rotation::Inverted => "Landscape (flipped)",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Rotation::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

pub fn is_x11() -> bool {
    std::env::var_os("DISPLAY").is_some() && std::env::var_os("WAYLAND_DISPLAY").is_none()
}

/// Parse `xrandr --verbose` output, returning connected outputs.
///
/// Output header lines look like
/// `DP-1 connected primary 2560x1440+0+0 (0x48) normal (normal left inverted right x axis y axis) 597mm x 336mm`
/// and each output block contains a `\tBrightness: 1.0` line.
pub fn parse_verbose(output: &str) -> Vec<XrandrOutput> {
    let mut outputs = Vec::new();
    let mut current: Option<XrandrOutput> = None;

    for line in output.lines() {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if !indented {
            if let Some(o) = current.take() {
                outputs.push(o);
            }
            let words: Vec<&str> = line.split_whitespace().collect();
            if words.len() >= 2 && words[1] == "connected" {
                // Rotation is the first rotation keyword before the '(' list.
                let mut rotation = Rotation::Normal;
                for w in &words[2..] {
                    // "(0x47)" is the mode id; "(normal left ...)" lists supported rotations.
                    if w.starts_with('(') && !w.starts_with("(0x") {
                        break;
                    }
                    if let Some(r) = Rotation::parse(w) {
                        rotation = r;
                        break;
                    }
                }
                current = Some(XrandrOutput {
                    name: words[0].to_string(),
                    primary: words.get(2) == Some(&"primary"),
                    brightness: 1.0,
                    rotation,
                });
            }
            continue;
        }
        if let Some(o) = current.as_mut()
            && let Some(v) = line.trim().strip_prefix("Brightness:")
            && let Ok(b) = v.trim().parse::<f32>()
        {
            o.brightness = b;
        }
    }
    if let Some(o) = current.take() {
        outputs.push(o);
    }
    outputs
}

pub fn list_outputs() -> Result<Vec<XrandrOutput>> {
    let out = cmd::run("xrandr", &["--verbose".to_string()], TIMEOUT)?;
    if !out.success {
        bail!("xrandr failed: {}", out.stderr.trim());
    }
    Ok(parse_verbose(&out.stdout))
}

/// Map a 0..=100 slider percentage to an xrandr brightness multiplier.
pub fn percent_to_multiplier(percent: u8) -> f32 {
    let p = percent.min(100) as f32 / 100.0;
    MIN_SOFTWARE_BRIGHTNESS + (1.0 - MIN_SOFTWARE_BRIGHTNESS) * p
}

pub fn multiplier_to_percent(m: f32) -> u8 {
    let p = (m - MIN_SOFTWARE_BRIGHTNESS) / (1.0 - MIN_SOFTWARE_BRIGHTNESS);
    (p.clamp(0.0, 1.0) * 100.0).round() as u8
}

pub fn set_brightness(output: &str, percent: u8) -> Result<()> {
    let m = percent_to_multiplier(percent);
    let args = vec![
        "--output".to_string(),
        output.to_string(),
        "--brightness".to_string(),
        format!("{m:.2}"),
    ];
    let out = cmd::run("xrandr", &args, TIMEOUT)?;
    if !out.success {
        bail!("xrandr --brightness failed: {}", out.stderr.trim());
    }
    Ok(())
}

pub fn set_rotation(output: &str, rotation: Rotation) -> Result<()> {
    let args = vec![
        "--output".to_string(),
        output.to_string(),
        "--rotate".to_string(),
        rotation.as_str().to_string(),
    ];
    let out = cmd::run("xrandr", &args, TIMEOUT)?;
    if !out.success {
        bail!("xrandr --rotate failed: {}", out.stderr.trim());
    }
    Ok(())
}

/// Normalize a DRM connector (`card1-HDMI-A-1`) or xrandr output (`HDMI-1`,
/// `HDMI-A-0`, `DisplayPort-0`) to a comparable key such as `hdmi-1`.
///
/// Drivers name outputs differently (modesetting: `HDMI-1`, amdgpu: `HDMI-A-0`,
/// `DisplayPort-0`), so this is a best-effort match.
pub fn normalize_connector(name: &str) -> String {
    let mut s = name.to_ascii_lowercase();
    // Strip "cardN-" prefix from DRM connector names.
    if let Some(rest) = s.strip_prefix("card")
        && let Some((num, tail)) = rest.split_once('-')
        && num.chars().all(|c| c.is_ascii_digit())
    {
        s = tail.to_string();
    }
    s = s.replace("displayport", "dp");
    s = s.replace("hdmi-a-", "hdmi-");
    s
}

pub fn connectors_match(a: &str, b: &str) -> bool {
    normalize_connector(a) == normalize_connector(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VERBOSE: &str = "\
Screen 0: minimum 8 x 8, current 4480 x 1440, maximum 32767 x 32767
eDP-1 connected 1920x1080+2560+0 (0x47) normal (normal left inverted right x axis y axis) 309mm x 174mm
\tIdentifier: 0x42
\tTimestamp:  12345
\tBrightness: 0.80
\tGamma:      1.0:1.0:1.0
  1920x1080 (0x47) 138.700MHz +HSync -VSync *current +preferred
DP-1 connected primary 1440x2560+0+0 (0x48) left (normal left inverted right x axis y axis) 597mm x 336mm
\tBrightness: 1.0
HDMI-1 disconnected (normal left inverted right x axis y axis)
\tBrightness: 0.5
";

    #[test]
    fn parse_outputs() {
        let o = parse_verbose(VERBOSE);
        assert_eq!(o.len(), 2);
        assert_eq!(o[0].name, "eDP-1");
        assert!(!o[0].primary);
        assert!((o[0].brightness - 0.8).abs() < 1e-6);
        assert_eq!(o[0].rotation, Rotation::Normal);
        assert_eq!(o[1].name, "DP-1");
        assert!(o[1].primary);
        assert_eq!(o[1].rotation, Rotation::Left);
    }

    #[test]
    fn multiplier_round_trip() {
        assert_eq!(multiplier_to_percent(1.0), 100);
        assert_eq!(multiplier_to_percent(MIN_SOFTWARE_BRIGHTNESS), 0);
        assert_eq!(multiplier_to_percent(0.0), 0);
        for p in [0u8, 1, 33, 50, 99, 100] {
            assert_eq!(multiplier_to_percent(percent_to_multiplier(p)), p);
        }
    }

    #[test]
    fn connector_matching() {
        assert!(connectors_match("card1-DP-1", "DP-1"));
        assert!(connectors_match("card0-HDMI-A-1", "HDMI-1"));
        assert!(connectors_match("card0-eDP-1", "eDP-1"));
        assert!(connectors_match("DisplayPort-0", "card0-DP-0"));
        assert!(!connectors_match("card0-DP-1", "DP-2"));
    }
}
