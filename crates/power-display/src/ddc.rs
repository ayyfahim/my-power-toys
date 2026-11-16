//! DDC/CI backend driven by the `ddcutil` CLI.
//!
//! Parses `ddcutil detect --brief`, `ddcutil getvcp ... --terse` and
//! `ddcutil capabilities` output.

use crate::cmd;
use anyhow::{Result, bail};
use std::collections::BTreeMap;
use std::time::Duration;
use tracing::{debug, warn};

const DETECT_TIMEOUT: Duration = Duration::from_secs(30);
const VCP_TIMEOUT: Duration = Duration::from_secs(10);

/// One valid display reported by `ddcutil detect`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DdcDisplay {
    /// ddcutil display number (`--display N`).
    pub display_number: u32,
    /// I2C bus number (`--bus N`), from `/dev/i2c-N`.
    pub bus: Option<u32>,
    /// DRM connector, e.g. `card0-DP-1`.
    pub drm_connector: Option<String>,
    /// 3-letter EDID manufacturer id, e.g. `DEL`.
    pub mfg: String,
    pub model: String,
    pub serial: String,
}

/// A single feature value from `getvcp --terse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcpReading {
    Continuous { current: u16, max: u16 },
    NonContinuous(u16),
    Unsupported,
}

/// Parsed capabilities: VCP code -> allowed values (empty for continuous
/// features or when the monitor lists no values).
pub type VcpCapabilities = BTreeMap<u8, Vec<u16>>;

// ── Parsing ────────────────────────────────────────────────────────────────

/// Parse `ddcutil detect` (brief or full) output. Only "Display N" sections
/// are returned; "Invalid display" / "Phantom display" sections are skipped.
pub fn parse_detect(output: &str) -> Vec<DdcDisplay> {
    let mut displays = Vec::new();
    let mut current: Option<DdcDisplay> = None;

    for line in output.lines() {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if !indented {
            if let Some(d) = current.take() {
                displays.push(d);
            }
            if let Some(rest) = trimmed.strip_prefix("Display ")
                && let Ok(n) = rest.trim().parse::<u32>()
            {
                current = Some(DdcDisplay {
                    display_number: n,
                    ..Default::default()
                });
            }
            continue;
        }

        let Some(d) = current.as_mut() else { continue };
        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase().replace('_', " ");
        let value = value.trim();

        match key.as_str() {
            "i2c bus" => {
                d.bus = value
                    .rsplit_once("i2c-")
                    .and_then(|(_, n)| n.trim().parse().ok());
            }
            "drm connector" => {
                if !value.is_empty() {
                    d.drm_connector = Some(value.to_string());
                }
            }
            "monitor" => {
                // "MFG:MODEL:SERIAL" (serial may be empty).
                if let Some((mfg, rest)) = value.split_once(':') {
                    let (model, serial) = rest.rsplit_once(':').unwrap_or((rest, ""));
                    d.mfg = mfg.trim().to_string();
                    d.model = model.trim().to_string();
                    d.serial = serial.trim().to_string();
                }
            }
            // Full (non-brief) detect output.
            "mfg id" if d.mfg.is_empty() => {
                d.mfg = value.split_whitespace().next().unwrap_or("").to_string();
            }
            "model" if d.model.is_empty() => d.model = value.to_string(),
            "serial number" if d.serial.is_empty() => d.serial = value.to_string(),
            _ => {}
        }
    }
    if let Some(d) = current.take() {
        displays.push(d);
    }
    displays
}

fn parse_hex_byte(s: &str) -> Option<u16> {
    let s = s.trim();
    let s = s
        .strip_prefix("0x")
        .or_else(|| s.strip_prefix('x'))
        .unwrap_or(s);
    u16::from_str_radix(s, 16).ok()
}

/// Parse `getvcp --terse` output into code -> reading.
///
/// Formats (from the ddcutil docs):
/// - `VCP 10 C 50 100`            continuous: current, max (decimal)
/// - `VCP 60 SNC x0f`             simple non-continuous (hex)
/// - `VCP DF CNC xff xff x02 x00` complex non-continuous: mh ml sh sl
/// - `VCP 73 T x01020304`         table (ignored)
/// - `VCP 84 ERR`                 unsupported / error
pub fn parse_terse(output: &str) -> BTreeMap<u8, VcpReading> {
    let mut map = BTreeMap::new();
    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 || parts[0] != "VCP" {
            continue;
        }
        let Ok(code) = u8::from_str_radix(parts[1], 16) else {
            continue;
        };
        let reading = match parts[2] {
            "C" if parts.len() >= 5 => match (parts[3].parse::<u16>(), parts[4].parse::<u16>()) {
                (Ok(current), Ok(max)) => VcpReading::Continuous { current, max },
                _ => continue,
            },
            "SNC" if parts.len() >= 4 => match parse_hex_byte(parts[3]) {
                Some(v) => VcpReading::NonContinuous(v),
                None => continue,
            },
            "CNC" if parts.len() >= 7 => match parse_hex_byte(parts[6]) {
                // The SL byte holds the value for the features we use.
                Some(sl) => VcpReading::NonContinuous(sl),
                None => continue,
            },
            "ERR" => VcpReading::Unsupported,
            _ => continue,
        };
        map.insert(code, reading);
    }
    map
}

/// Parse the `vcp(...)` section of a raw MCCS capabilities string, e.g.
/// `(prot(monitor)type(lcd)vcp(02 10 12 14(05 08 0B) 60(0F 11) D6(01 04))mccs_ver(2.1))`.
pub fn parse_mccs_capabilities(raw: &str) -> VcpCapabilities {
    let mut caps = VcpCapabilities::new();
    let bytes = raw.as_bytes();
    let lower = raw.to_ascii_lowercase();

    // Find a "vcp(" not preceded by a letter (to skip "vcpname(").
    let mut start = None;
    let mut search = 0;
    while let Some(pos) = lower[search..].find("vcp(") {
        let abs = search + pos;
        let prev_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
        if prev_ok {
            start = Some(abs + 4);
            break;
        }
        search = abs + 4;
    }
    let Some(mut i) = start else { return caps };

    let mut last_code: Option<u8> = None;
    let mut token = String::new();
    let flush = |token: &mut String, caps: &mut VcpCapabilities, last: &mut Option<u8>| {
        if !token.is_empty() {
            if let Ok(code) = u8::from_str_radix(token, 16) {
                caps.entry(code).or_default();
                *last = Some(code);
            }
            token.clear();
        }
    };

    while i < bytes.len() {
        let c = bytes[i] as char;
        match c {
            '(' => {
                flush(&mut token, &mut caps, &mut last_code);
                // Value list for last_code, until matching ')'.
                let end = raw[i + 1..].find(')').map(|e| i + 1 + e);
                let Some(end) = end else { break };
                if let Some(code) = last_code {
                    let values: Vec<u16> = raw[i + 1..end]
                        .split_whitespace()
                        .filter_map(|v| u16::from_str_radix(v, 16).ok())
                        .collect();
                    caps.entry(code).or_default().extend(values);
                }
                i = end + 1;
                continue;
            }
            ')' => {
                flush(&mut token, &mut caps, &mut last_code);
                break;
            }
            c if c.is_ascii_hexdigit() => token.push(c),
            _ => flush(&mut token, &mut caps, &mut last_code),
        }
        i += 1;
    }
    caps
}

/// Parse `ddcutil capabilities` output. Uses the structured "Feature: XX"
/// listing when present, otherwise falls back to a raw capabilities string line.
pub fn parse_capabilities_output(output: &str) -> VcpCapabilities {
    let mut caps = VcpCapabilities::new();
    let mut current: Option<u8> = None;
    let mut in_values = false;

    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Feature:") {
            in_values = false;
            current = rest
                .split_whitespace()
                .next()
                .and_then(|c| u8::from_str_radix(c, 16).ok());
            if let Some(code) = current {
                caps.entry(code).or_default();
            }
            continue;
        }
        if trimmed.starts_with("Values") {
            in_values = true;
            continue;
        }
        if in_values
            && let Some(code) = current
            && let Some((v, _name)) = trimmed.split_once(':')
            && let Some(v) = parse_hex_byte(v)
        {
            caps.entry(code).or_default().push(v);
            continue;
        }
        // Any non-value line ends a value list.
        if in_values && !trimmed.is_empty() {
            in_values = false;
        }
    }

    if caps.is_empty()
        && let Some(raw) = output.lines().find_map(|l| {
            let lower = l.to_ascii_lowercase();
            lower
                .find("capabilities string:")
                .map(|p| l[p + "capabilities string:".len()..].trim().to_string())
        })
    {
        caps = parse_mccs_capabilities(&raw);
    }
    caps
}

// ── Execution ──────────────────────────────────────────────────────────────

/// Thin wrapper around the `ddcutil` binary.
#[derive(Debug, Clone, Default)]
pub struct Ddcutil {
    /// Extra global args (e.g. `--sleep-multiplier 0.5`), passed verbatim.
    pub extra_args: Vec<String>,
}

impl Ddcutil {
    pub fn new(extra_args: Vec<String>) -> Self {
        Self { extra_args }
    }

    pub fn available() -> bool {
        cmd::in_path("ddcutil")
    }

    fn run(&self, bus: Option<u32>, args: &[&str], timeout: Duration) -> Result<cmd::CmdOutput> {
        let mut full: Vec<String> = self.extra_args.clone();
        if let Some(b) = bus {
            full.push("--bus".into());
            full.push(b.to_string());
        }
        full.extend(args.iter().map(|s| s.to_string()));
        debug!("ddcutil {}", full.join(" "));
        cmd::run("ddcutil", &full, timeout)
    }

    pub fn detect(&self) -> Result<Vec<DdcDisplay>> {
        let out = self.run(None, &["detect", "--brief"], DETECT_TIMEOUT)?;
        let displays = parse_detect(&out.stdout);
        if displays.is_empty() && !out.success {
            bail!(
                "ddcutil detect failed: {}",
                first_line(&out.stderr, &out.stdout)
            );
        }
        Ok(displays)
    }

    pub fn capabilities(&self, bus: u32) -> Result<VcpCapabilities> {
        let out = self.run(Some(bus), &["capabilities"], VCP_TIMEOUT)?;
        if !out.success {
            bail!(
                "ddcutil capabilities failed: {}",
                first_line(&out.stderr, &out.stdout)
            );
        }
        Ok(parse_capabilities_output(&out.stdout))
    }

    /// Read several VCP codes. Uses one multi-code call (ddcutil >= 1.3) and
    /// falls back to one call per code for older versions.
    pub fn getvcp(&self, bus: u32, codes: &[u8]) -> Result<BTreeMap<u8, VcpReading>> {
        let hex: Vec<String> = codes.iter().map(|c| format!("{c:02X}")).collect();
        let mut args: Vec<&str> = vec!["getvcp"];
        args.extend(hex.iter().map(String::as_str));
        args.push("--terse");
        let out = self.run(Some(bus), &args, VCP_TIMEOUT)?;
        let mut map = parse_terse(&out.stdout);
        if map.is_empty() && codes.len() > 1 {
            for h in &hex {
                match self.run(Some(bus), &["getvcp", h, "--terse"], VCP_TIMEOUT) {
                    Ok(o) => map.extend(parse_terse(&o.stdout)),
                    Err(e) => warn!("ddcutil getvcp {h} on bus {bus}: {e}"),
                }
            }
        }
        if map.is_empty() && !out.success {
            bail!(
                "ddcutil getvcp failed: {}",
                first_line(&out.stderr, &out.stdout)
            );
        }
        Ok(map)
    }

    pub fn setvcp(&self, bus: u32, code: u8, value: u16) -> Result<()> {
        let code = format!("{code:02X}");
        let value = value.to_string();
        let out = self.run(
            Some(bus),
            &["--noverify", "setvcp", &code, &value],
            VCP_TIMEOUT,
        )?;
        if !out.success {
            bail!(
                "ddcutil setvcp {code} {value} failed: {}",
                first_line(&out.stderr, &out.stdout)
            );
        }
        Ok(())
    }
}

fn first_line(a: &str, b: &str) -> String {
    a.lines()
        .chain(b.lines())
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("unknown error")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const DETECT_BRIEF_V2: &str = "\
Display 1
   I2C bus:  /dev/i2c-4
   DRM connector:           card1-DP-1
   Monitor:                 DEL:DELL U2715H:GH85D6AB1A2L

Display 2
   I2C bus:  /dev/i2c-7
   DRM_connector:           card1-HDMI-A-1
   Monitor:                 GSM:LG ULTRAWIDE:

Invalid display
   I2C bus:  /dev/i2c-5
   DRM connector:           card1-eDP-1
   Monitor:                 BOE:0x0a1d:
";

    const DETECT_BRIEF_V1: &str = "\
Display 1
   I2C bus:             /dev/i2c-6
   Monitor:             ACI:VE247:K3LMQS052443
";

    const DETECT_FULL: &str = "\
Display 1
   I2C bus:  /dev/i2c-3
      DRM connector:                card0-DP-2
      driver:                       i915
   EDID synopsis:
      Mfg id:               DEL - Dell Inc.
      Model:                DELL P2419H
      Product code:         41150  (0xa0be)
      Serial number:        8W2PYV2
   VCP version:         2.1
";

    #[test]
    fn detect_brief_v2() {
        let d = parse_detect(DETECT_BRIEF_V2);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].display_number, 1);
        assert_eq!(d[0].bus, Some(4));
        assert_eq!(d[0].drm_connector.as_deref(), Some("card1-DP-1"));
        assert_eq!(d[0].mfg, "DEL");
        assert_eq!(d[0].model, "DELL U2715H");
        assert_eq!(d[0].serial, "GH85D6AB1A2L");
        assert_eq!(d[1].bus, Some(7));
        assert_eq!(d[1].drm_connector.as_deref(), Some("card1-HDMI-A-1"));
        assert_eq!(d[1].model, "LG ULTRAWIDE");
        assert_eq!(d[1].serial, "");
    }

    #[test]
    fn detect_brief_v1() {
        let d = parse_detect(DETECT_BRIEF_V1);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].bus, Some(6));
        assert_eq!(d[0].drm_connector, None);
        assert_eq!(d[0].model, "VE247");
        assert_eq!(d[0].serial, "K3LMQS052443");
    }

    #[test]
    fn detect_full() {
        let d = parse_detect(DETECT_FULL);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].bus, Some(3));
        assert_eq!(d[0].drm_connector.as_deref(), Some("card0-DP-2"));
        assert_eq!(d[0].mfg, "DEL");
        assert_eq!(d[0].model, "DELL P2419H");
        assert_eq!(d[0].serial, "8W2PYV2");
    }

    #[test]
    fn detect_none() {
        assert!(parse_detect("No displays found.\n").is_empty());
    }

    #[test]
    fn terse_values() {
        let out = "\
VCP 10 C 50 100
VCP 12 C 75 100
VCP 62 ERR
VCP 14 SNC x05
VCP 60 SNC x0f
VCP D6 CNC x00 x00 x00 x01
VCP 73 T x01020304
garbage line
";
        let m = parse_terse(out);
        assert_eq!(
            m[&0x10],
            VcpReading::Continuous {
                current: 50,
                max: 100
            }
        );
        assert_eq!(
            m[&0x12],
            VcpReading::Continuous {
                current: 75,
                max: 100
            }
        );
        assert_eq!(m[&0x62], VcpReading::Unsupported);
        assert_eq!(m[&0x14], VcpReading::NonContinuous(5));
        assert_eq!(m[&0x60], VcpReading::NonContinuous(0x0f));
        assert_eq!(m[&0xD6], VcpReading::NonContinuous(1));
        assert!(!m.contains_key(&0x73));
        assert_eq!(m.len(), 6);
    }

    #[test]
    fn mccs_raw_caps() {
        let raw = "(prot(monitor)type(lcd)model(U2715H)cmds(01 02 03 07 0C E3 F3)\
vcp(02 04 05 08 10 12 14(05 08 0B) 16 18 1A 52 60(0F 10 11 12) AA(01 02) AC AE B2 B6 C6 C8 C9 D6(01 04 05) DC(00 02 03 05) DF E0 E1 E2(00 01 02 04 0E 12 14 19) F0(00 08) F1 F2 FD)\
mswhql(1)asset_eep(40)mccs_ver(2.1))";
        let caps = parse_mccs_capabilities(raw);
        assert!(caps.contains_key(&0x10));
        assert!(caps.contains_key(&0x12));
        assert!(!caps.contains_key(&0x62));
        assert_eq!(caps[&0x14], vec![0x05, 0x08, 0x0B]);
        assert_eq!(caps[&0x60], vec![0x0F, 0x10, 0x11, 0x12]);
        assert_eq!(caps[&0xD6], vec![0x01, 0x04, 0x05]);
        assert!(caps[&0x10].is_empty());
        assert!(caps.contains_key(&0xFD));
    }

    #[test]
    fn mccs_skips_vcpname() {
        let raw = "(vcpname(10(Bright))vcp(10 62))";
        let caps = parse_mccs_capabilities(raw);
        assert_eq!(caps.keys().copied().collect::<Vec<_>>(), vec![0x10, 0x62]);
    }

    #[test]
    fn structured_caps() {
        let out = "\
Model: U2715H
MCCS version: 2.1
Commands:
   Op Code: 01 (VCP Request)
VCP Features:
   Feature: 02 (New control value)
   Feature: 10 (Brightness)
   Feature: 12 (Contrast)
   Feature: 14 (Select color preset)
      Values:
         05: 6500 K
         08: 9300 K
         0b: User 1
   Feature: 60 (Input Source)
      Values:
         0f: DisplayPort-1
         11: HDMI-1
   Feature: 62 (Audio speaker volume)
";
        let caps = parse_capabilities_output(out);
        assert_eq!(caps[&0x14], vec![0x05, 0x08, 0x0B]);
        assert_eq!(caps[&0x60], vec![0x0F, 0x11]);
        assert!(caps[&0x62].is_empty());
        assert!(caps.contains_key(&0x10));
    }

    #[test]
    fn raw_caps_line_fallback() {
        let out = "Unparsed capabilities string: (prot(monitor)vcp(10 12 60(11 12)))\n";
        let caps = parse_capabilities_output(out);
        assert_eq!(caps[&0x60], vec![0x11, 0x12]);
        assert!(caps.contains_key(&0x12));
    }
}
