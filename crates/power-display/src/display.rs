//! Unified monitor model over the DDC/CI, backlight and software backends.

use crate::backlight;
use crate::config::{MonitorStateFile, PowerDisplayConfig, Profile, ProfileMonitorSetting};
use crate::ddc::{DdcDisplay, Ddcutil, VcpCapabilities, VcpReading};
use crate::vcp::{self, Feature};
use crate::xrandr::{self, Rotation, XrandrOutput};
use anyhow::{Context, Result, bail};
use std::collections::BTreeMap;
use tracing::{info, warn};

/// How a monitor is controlled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Backend {
    /// External monitor over DDC/CI (`ddcutil --bus N`).
    Ddc { bus: u32 },
    /// Internal panel via `/sys/class/backlight/<device>`.
    Backlight { device: String, max_raw: u32 },
    /// Software brightness via `xrandr --brightness` (X11 only).
    Software { output: String },
}

impl Backend {
    pub fn label(&self) -> &'static str {
        match self {
            Backend::Ddc { .. } => "DDC/CI",
            Backend::Backlight { .. } => "Backlight",
            Backend::Software { .. } => "Software (gamma)",
        }
    }
}

/// Per-feature limits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureInfo {
    /// Raw maximum for continuous DDC features (values are shown as percent).
    pub max: u16,
    /// Allowed values for non-continuous features.
    pub values: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Monitor {
    /// Stable identifier used in profiles, state and preferences.
    pub id: String,
    pub name: String,
    pub backend: Backend,
    pub mfg: String,
    pub model: String,
    pub serial: String,
    /// DRM connector, e.g. `card0-DP-1`.
    pub drm_connector: Option<String>,
    /// Matching xrandr output name, e.g. `DP-1` (X11 only).
    pub xrandr_output: Option<String>,
    pub features: BTreeMap<Feature, FeatureInfo>,
    /// Current values: percent for continuous features, raw VCP otherwise.
    pub values: BTreeMap<Feature, u16>,
    /// Current rotation, when the output is known to xrandr.
    pub rotation: Option<Rotation>,
}

impl Monitor {
    pub fn supports(&self, f: Feature) -> bool {
        self.features.contains_key(&f)
    }

    pub fn value(&self, f: Feature) -> Option<u16> {
        self.values.get(&f).copied()
    }

    /// Current continuous values (and color preset) as a profile entry.
    pub fn snapshot(&self) -> ProfileMonitorSetting {
        let pct = |f| self.value(f).map(|v| v.min(100) as u8);
        ProfileMonitorSetting {
            monitor_id: self.id.clone(),
            brightness: pct(Feature::Brightness),
            contrast: pct(Feature::Contrast),
            volume: pct(Feature::Volume),
            color_temperature_vcp: self.value(Feature::ColorPreset),
        }
    }
}

pub struct Discovery {
    pub monitors: Vec<Monitor>,
    /// Human-readable problems (missing tools, permission errors...).
    pub warnings: Vec<String>,
}

/// Build the stable id for a DDC monitor.
pub fn ddc_monitor_id(d: &DdcDisplay) -> String {
    if d.serial.is_empty() {
        let port = d
            .drm_connector
            .as_deref()
            .map(xrandr::normalize_connector)
            .or_else(|| d.bus.map(|b| format!("bus{b}")))
            .unwrap_or_else(|| format!("display{}", d.display_number));
        format!("ddc:{}:{}:{}", d.mfg, d.model, port)
    } else {
        format!("ddc:{}:{}:{}", d.mfg, d.model, d.serial)
    }
}

fn is_internal_output(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("edp") || n.starts_with("lvds") || n.starts_with("dsi")
}

pub fn percent_to_raw(percent: u16, max: u16) -> u16 {
    ((percent.min(100) as u32 * max as u32 + 50) / 100) as u16
}

pub fn raw_to_percent(raw: u16, max: u16) -> u16 {
    if max == 0 {
        return 0;
    }
    ((raw as u32 * 100 + max as u32 / 2) / max as u32).min(100) as u16
}

/// Build the feature map and values for a DDC monitor from capabilities and
/// `getvcp` readings. Features must answer `getvcp`; when capabilities are
/// available (and `max_compat` is off) they must also be advertised there.
pub fn ddc_features(
    caps: Option<&VcpCapabilities>,
    readings: &BTreeMap<u8, VcpReading>,
    max_compat: bool,
) -> (BTreeMap<Feature, FeatureInfo>, BTreeMap<Feature, u16>) {
    let mut features = BTreeMap::new();
    let mut values = BTreeMap::new();
    let caps = caps.filter(|c| !c.is_empty() && !max_compat);

    for f in Feature::ALL {
        let code = f.vcp_code();
        if let Some(c) = caps
            && !c.contains_key(&code)
        {
            continue;
        }
        match readings.get(&code) {
            Some(VcpReading::Continuous { current, max }) if f.is_continuous() && *max > 0 => {
                features.insert(
                    f,
                    FeatureInfo {
                        max: *max,
                        values: vec![],
                    },
                );
                values.insert(f, raw_to_percent(*current, *max));
            }
            Some(VcpReading::NonContinuous(v))
            | Some(VcpReading::Continuous { current: v, .. })
                if !f.is_continuous() =>
            {
                let mut allowed = caps
                    .and_then(|c| c.get(&code))
                    .filter(|v| !v.is_empty())
                    .cloned()
                    .unwrap_or_else(|| vcp::fallback_values(code));
                if !allowed.contains(v) {
                    allowed.push(*v);
                }
                allowed.sort_unstable();
                allowed.dedup();
                features.insert(
                    f,
                    FeatureInfo {
                        max: 0,
                        values: allowed,
                    },
                );
                values.insert(f, *v);
            }
            _ => {}
        }
    }
    (features, values)
}

const ALL_CODES: [u8; 6] = [
    vcp::VCP_BRIGHTNESS,
    vcp::VCP_CONTRAST,
    vcp::VCP_VOLUME,
    vcp::VCP_COLOR_PRESET,
    vcp::VCP_INPUT_SOURCE,
    vcp::VCP_POWER_MODE,
];

/// Performs discovery and reads/writes values according to the config.
pub struct Controller {
    config: PowerDisplayConfig,
    ddc: Ddcutil,
}

impl Controller {
    pub fn new(config: PowerDisplayConfig) -> Self {
        let ddc = Ddcutil::new(config.ddcutil_extra_args.clone());
        Self { config, ddc }
    }

    pub fn config(&self) -> &PowerDisplayConfig {
        &self.config
    }

    pub fn discover(&self) -> Discovery {
        let mut monitors = Vec::new();
        let mut warnings = Vec::new();

        // ── DDC/CI ────────────────────────────────────────────────────────
        let mut ddc_without_connector = false;
        if self.config.enable_ddc {
            if Ddcutil::available() {
                match self.ddc.detect() {
                    Ok(found) => {
                        ddc_without_connector = found.iter().any(|d| d.drm_connector.is_none());
                        let probed: Vec<Option<Monitor>> = std::thread::scope(|s| {
                            let handles: Vec<_> = found
                                .iter()
                                .map(|d| s.spawn(move || self.probe_ddc(d)))
                                .collect();
                            handles
                                .into_iter()
                                .map(|h| h.join().ok().flatten())
                                .collect()
                        });
                        monitors.extend(probed.into_iter().flatten());
                    }
                    Err(e) => warnings.push(format!("{e:#}")),
                }
            } else {
                warnings.push(
                    "ddcutil not found: install it to control external monitors over DDC/CI".into(),
                );
            }
        }

        // ── Backlight ─────────────────────────────────────────────────────
        let mut have_backlight = false;
        if self.config.enable_backlight {
            let devices = match &self.config.backlight_device {
                Some(forced) => match backlight::find_device(forced) {
                    Some(d) => vec![d],
                    None => {
                        warnings.push(format!("backlight device {forced:?} not found"));
                        vec![]
                    }
                },
                None => backlight::list_devices(),
            };
            for d in devices {
                have_backlight = true;
                let mut values = BTreeMap::new();
                match backlight::read_brightness(&d.name) {
                    Ok(raw) => {
                        values.insert(
                            Feature::Brightness,
                            backlight::raw_to_percent(raw, d.max_brightness) as u16,
                        );
                    }
                    Err(e) => warn!("backlight {}: {e:#}", d.name),
                }
                monitors.push(Monitor {
                    id: format!("backlight:{}", d.name),
                    name: "Built-in display".into(),
                    backend: Backend::Backlight {
                        device: d.name.clone(),
                        max_raw: d.max_brightness,
                    },
                    mfg: String::new(),
                    model: d.name.clone(),
                    serial: String::new(),
                    drm_connector: d.drm_connector.clone(),
                    xrandr_output: None,
                    features: BTreeMap::from([(Feature::Brightness, FeatureInfo::default())]),
                    values,
                    rotation: None,
                });
            }
        }

        // ── xrandr: match outputs, add software fallback ──────────────────
        let outputs: Vec<XrandrOutput> = if xrandr::is_x11() && crate::cmd::in_path("xrandr") {
            xrandr::list_outputs().unwrap_or_else(|e| {
                warn!("xrandr: {e:#}");
                vec![]
            })
        } else {
            vec![]
        };
        attach_xrandr_outputs(&mut monitors, &outputs);

        if self.config.enable_software_brightness {
            for o in &outputs {
                let covered = monitors
                    .iter()
                    .any(|m| m.xrandr_output.as_deref() == Some(o.name.as_str()));
                let skip_internal = have_backlight && is_internal_output(&o.name);
                let skip_external = ddc_without_connector && !is_internal_output(&o.name);
                if covered || skip_internal || skip_external {
                    continue;
                }
                monitors.push(Monitor {
                    id: format!("xrandr:{}", o.name),
                    name: o.name.clone(),
                    backend: Backend::Software {
                        output: o.name.clone(),
                    },
                    mfg: String::new(),
                    model: String::new(),
                    serial: String::new(),
                    drm_connector: None,
                    xrandr_output: Some(o.name.clone()),
                    features: BTreeMap::from([(Feature::Brightness, FeatureInfo::default())]),
                    values: BTreeMap::from([(
                        Feature::Brightness,
                        xrandr::multiplier_to_percent(o.brightness) as u16,
                    )]),
                    rotation: Some(o.rotation),
                });
            }
        }

        if monitors.is_empty() && warnings.is_empty() {
            warnings.push("no controllable monitors found".into());
        }
        info!("Power Display: discovered {} monitor(s)", monitors.len());
        Discovery { monitors, warnings }
    }

    fn probe_ddc(&self, d: &DdcDisplay) -> Option<Monitor> {
        let bus = d.bus?;
        let caps = if self.config.max_compatibility_mode {
            None
        } else {
            match self.ddc.capabilities(bus) {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!("ddcutil capabilities on bus {bus}: {e:#}");
                    None
                }
            }
        };
        let readings = match self.ddc.getvcp(bus, &ALL_CODES) {
            Ok(r) => r,
            Err(e) => {
                warn!("ddcutil getvcp on bus {bus}: {e:#}");
                BTreeMap::new()
            }
        };
        let (features, values) =
            ddc_features(caps.as_ref(), &readings, self.config.max_compatibility_mode);
        let name = if d.model.is_empty() {
            format!("Display {}", d.display_number)
        } else {
            d.model.clone()
        };
        Some(Monitor {
            id: ddc_monitor_id(d),
            name,
            backend: Backend::Ddc { bus },
            mfg: d.mfg.clone(),
            model: d.model.clone(),
            serial: d.serial.clone(),
            drm_connector: d.drm_connector.clone(),
            xrandr_output: None,
            features,
            values,
            rotation: None,
        })
    }

    /// Re-read the current values of one monitor.
    pub fn refresh_values(&self, m: &mut Monitor) -> Result<()> {
        match &m.backend {
            Backend::Ddc { bus } => {
                let codes: Vec<u8> = m.features.keys().map(|f| f.vcp_code()).collect();
                let readings = self.ddc.getvcp(*bus, &codes)?;
                for (f, info) in &m.features {
                    match readings.get(&f.vcp_code()) {
                        Some(VcpReading::Continuous { current, max }) if f.is_continuous() => {
                            let max = if *max > 0 { *max } else { info.max };
                            m.values.insert(*f, raw_to_percent(*current, max));
                        }
                        Some(VcpReading::NonContinuous(v)) => {
                            m.values.insert(*f, *v);
                        }
                        _ => {}
                    }
                }
            }
            Backend::Backlight { device, max_raw } => {
                let raw = backlight::read_brightness(device)?;
                m.values.insert(
                    Feature::Brightness,
                    backlight::raw_to_percent(raw, *max_raw) as u16,
                );
            }
            Backend::Software { output } => {
                if let Some(o) = xrandr::list_outputs()?
                    .into_iter()
                    .find(|o| &o.name == output)
                {
                    m.values.insert(
                        Feature::Brightness,
                        xrandr::multiplier_to_percent(o.brightness) as u16,
                    );
                }
            }
        }
        Ok(())
    }

    /// Write a value. Continuous features take a percentage (0..=100),
    /// others the raw VCP value. Updates `m.values` and the saved state.
    pub fn set(&self, m: &mut Monitor, feature: Feature, value: u16) -> Result<()> {
        let Some(info) = m.features.get(&feature) else {
            bail!("{} does not support {}", m.name, feature.label());
        };
        let value = if feature.is_continuous() {
            value.min(100)
        } else {
            value
        };
        match &m.backend {
            Backend::Ddc { bus } => {
                let raw = if feature.is_continuous() {
                    percent_to_raw(value, info.max)
                } else {
                    value
                };
                self.ddc
                    .setvcp(*bus, feature.vcp_code(), raw)
                    .with_context(|| format!("{}: set {}", m.name, feature.label()))?;
            }
            Backend::Backlight { device, max_raw } => {
                let mut raw = backlight::percent_to_raw(value as u8, *max_raw);
                // Never switch the panel fully off from a slider.
                if raw == 0 && *max_raw > 0 {
                    raw = 1;
                }
                backlight::write_brightness(device, raw)?;
            }
            Backend::Software { output } => {
                xrandr::set_brightness(output, value as u8)?;
            }
        }
        m.values.insert(feature, value);

        if matches!(
            feature,
            Feature::Brightness | Feature::Contrast | Feature::Volume | Feature::ColorPreset
        ) {
            let mut update = ProfileMonitorSetting {
                monitor_id: m.id.clone(),
                ..Default::default()
            };
            match feature {
                Feature::Brightness => update.brightness = Some(value as u8),
                Feature::Contrast => update.contrast = Some(value as u8),
                Feature::Volume => update.volume = Some(value as u8),
                _ => update.color_temperature_vcp = Some(value),
            }
            MonitorStateFile::record_and_save(&update);
        }
        Ok(())
    }

    pub fn set_rotation(&self, m: &mut Monitor, rotation: Rotation) -> Result<()> {
        let Some(output) = m.xrandr_output.clone() else {
            bail!("{}: rotation needs an X11 output", m.name);
        };
        xrandr::set_rotation(&output, rotation)?;
        m.rotation = Some(rotation);
        Ok(())
    }

    /// Apply one profile entry to the matching monitor. Returns errors as text.
    pub fn apply_setting(
        &self,
        monitors: &mut [Monitor],
        setting: &ProfileMonitorSetting,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        let Some(m) = monitors.iter_mut().find(|m| m.id == setting.monitor_id) else {
            return errors;
        };
        let wanted = [
            (Feature::Brightness, setting.brightness.map(u16::from)),
            (Feature::Contrast, setting.contrast.map(u16::from)),
            (Feature::Volume, setting.volume.map(u16::from)),
            (Feature::ColorPreset, setting.color_temperature_vcp),
        ];
        for (f, v) in wanted {
            if let Some(v) = v
                && m.supports(f)
                && m.value(f) != Some(v)
                && let Err(e) = self.set(m, f, v)
            {
                errors.push(format!("{e:#}"));
            }
        }
        errors
    }

    pub fn apply_profile(&self, monitors: &mut [Monitor], profile: &Profile) -> Vec<String> {
        info!("Power Display: applying profile {:?}", profile.name);
        profile
            .monitor_settings
            .iter()
            .flat_map(|s| self.apply_setting(monitors, s))
            .collect()
    }

    /// Capture the current values of all visible monitors as a profile.
    pub fn profile_from(&self, name: &str, monitors: &[Monitor]) -> Profile {
        Profile {
            name: name.to_string(),
            monitor_settings: monitors
                .iter()
                .filter(|m| !self.config.flags_for(&m.id).hidden)
                .map(Monitor::snapshot)
                .filter(|s| !s.is_empty())
                .collect(),
        }
    }

    /// Change brightness of every visible monitor by `delta` percent.
    /// Returns the resulting levels (monitor name, percent).
    pub fn adjust_brightness_all(
        &self,
        monitors: &mut [Monitor],
        delta: i16,
    ) -> (Vec<(String, u16)>, Vec<String>) {
        let mut levels = Vec::new();
        let mut errors = Vec::new();
        for m in monitors.iter_mut() {
            if self.config.flags_for(&m.id).hidden || !m.supports(Feature::Brightness) {
                continue;
            }
            let current = m.value(Feature::Brightness).unwrap_or(50);
            let target = step_value(current, delta);
            if target == current {
                levels.push((m.name.clone(), current));
                continue;
            }
            match self.set(m, Feature::Brightness, target) {
                Ok(()) => levels.push((m.name.clone(), target)),
                Err(e) => errors.push(format!("{e:#}")),
            }
        }
        (levels, errors)
    }
}

/// Add `delta` to a percentage, clamped to 0..=100.
pub fn step_value(current: u16, delta: i16) -> u16 {
    (current as i32 + delta as i32).clamp(0, 100) as u16
}

/// Fill in `xrandr_output` / `rotation` for monitors whose DRM connector
/// matches an xrandr output. Internal panels without a connector link are
/// matched to the first internal (eDP/LVDS/DSI) output.
pub fn attach_xrandr_outputs(monitors: &mut [Monitor], outputs: &[XrandrOutput]) {
    for m in monitors.iter_mut() {
        let found = m
            .drm_connector
            .as_deref()
            .and_then(|c| {
                outputs
                    .iter()
                    .find(|o| xrandr::connectors_match(c, &o.name))
            })
            .or_else(|| {
                matches!(m.backend, Backend::Backlight { .. })
                    .then(|| outputs.iter().find(|o| is_internal_output(&o.name)))
                    .flatten()
            });
        if let Some(o) = found {
            m.xrandr_output = Some(o.name.clone());
            m.rotation = Some(o.rotation);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ddc::{parse_capabilities_output, parse_terse};

    #[test]
    fn ids() {
        let d = DdcDisplay {
            display_number: 1,
            bus: Some(4),
            drm_connector: Some("card1-DP-1".into()),
            mfg: "DEL".into(),
            model: "DELL U2715H".into(),
            serial: "GH85".into(),
        };
        assert_eq!(ddc_monitor_id(&d), "ddc:DEL:DELL U2715H:GH85");
        let mut d2 = d.clone();
        d2.serial.clear();
        assert_eq!(ddc_monitor_id(&d2), "ddc:DEL:DELL U2715H:dp-1");
        d2.drm_connector = None;
        assert_eq!(ddc_monitor_id(&d2), "ddc:DEL:DELL U2715H:bus4");
    }

    #[test]
    fn percent_math() {
        assert_eq!(percent_to_raw(50, 100), 50);
        assert_eq!(percent_to_raw(50, 255), 128);
        assert_eq!(raw_to_percent(128, 255), 50);
        assert_eq!(raw_to_percent(5, 0), 0);
        assert_eq!(step_value(98, 5), 100);
        assert_eq!(step_value(3, -5), 0);
        assert_eq!(step_value(50, -5), 45);
    }

    #[test]
    fn features_from_caps_and_readings() {
        let readings = parse_terse(
            "VCP 10 C 128 255\nVCP 12 C 75 100\nVCP 62 ERR\nVCP 14 SNC x05\nVCP 60 SNC x0f\nVCP D6 SNC x01\n",
        );
        let caps = parse_capabilities_output(
            "Unparsed capabilities string: (vcp(10 12 14(05 08) 60(0F 11) 62))\n",
        );
        let (f, v) = ddc_features(Some(&caps), &readings, false);
        assert_eq!(f[&Feature::Brightness].max, 255);
        assert_eq!(v[&Feature::Brightness], 50);
        assert_eq!(v[&Feature::Contrast], 75);
        // Volume advertised but ERR on read -> unsupported.
        assert!(!f.contains_key(&Feature::Volume));
        assert_eq!(f[&Feature::ColorPreset].values, vec![0x05, 0x08]);
        assert_eq!(f[&Feature::InputSource].values, vec![0x0F, 0x11]);
        // D6 readable but not advertised -> hidden unless max compatibility.
        assert!(!f.contains_key(&Feature::PowerMode));

        let (f2, _) = ddc_features(Some(&caps), &readings, true);
        assert!(f2.contains_key(&Feature::PowerMode));
        assert_eq!(
            f2[&Feature::InputSource].values,
            vcp::fallback_values(vcp::VCP_INPUT_SOURCE)
        );

        // No caps at all -> probe-based.
        let (f3, _) = ddc_features(None, &readings, false);
        assert!(f3.contains_key(&Feature::PowerMode));
    }

    #[test]
    fn xrandr_matching() {
        let mk = |id: &str, backend: Backend, conn: Option<&str>| Monitor {
            id: id.into(),
            name: id.into(),
            backend,
            mfg: String::new(),
            model: String::new(),
            serial: String::new(),
            drm_connector: conn.map(String::from),
            xrandr_output: None,
            features: BTreeMap::new(),
            values: BTreeMap::new(),
            rotation: None,
        };
        let mut ms = vec![
            mk("a", Backend::Ddc { bus: 4 }, Some("card1-HDMI-A-1")),
            mk(
                "b",
                Backend::Backlight {
                    device: "intel_backlight".into(),
                    max_raw: 100,
                },
                None,
            ),
            mk("c", Backend::Ddc { bus: 5 }, Some("card1-DP-3")),
        ];
        let outs = xrandr::parse_verbose(
            "eDP-1 connected 1920x1080+0+0 (0x1) normal (normal left) 1mm x 1mm\nHDMI-1 connected 1920x1080+1920+0 (0x2) left (normal left) 1mm x 1mm\n",
        );
        attach_xrandr_outputs(&mut ms, &outs);
        assert_eq!(ms[0].xrandr_output.as_deref(), Some("HDMI-1"));
        assert_eq!(ms[0].rotation, Some(Rotation::Left));
        assert_eq!(ms[1].xrandr_output.as_deref(), Some("eDP-1"));
        assert_eq!(ms[2].xrandr_output, None);
    }
}
