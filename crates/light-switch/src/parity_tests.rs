//! Parity tests against Microsoft PowerToys Light Switch (H-misc tester).
//! Oracle: src/modules/LightSwitch/LightSwitchService/ThemeScheduler.cpp
//! (CalculateSunriseSunset), LightSwitchUtils.h (ShouldBeLight),
//! LightSwitchStateManager.cpp (NormalizeMinutes + offsets) and
//! settings-ui/Settings.UI.Library/LightSwitchProperties.cs (defaults).

use super::*;
use chrono::{FixedOffset, NaiveTime, TimeZone, Timelike};
use config::{LightSwitchConfig, ScheduleMode};

// ---------- PT oracle ports ----------

/// Port of CalculateSunriseSunset's inner calcTime: UT hours, or -1 when
/// the sun never rises/sets.
fn pt_calc_ut(lat: f64, lon: f64, year: i32, month: i32, day: i32, sunrise: bool) -> f64 {
    let zenith: f64 = 90.833;
    let n1 = (275.0 * month as f64 / 9.0).floor() as i32;
    let n2 = ((month as f64 + 9.0) / 12.0).floor() as i32;
    let y = year as f64;
    let n3 = (1.0 + ((y - 4.0 * (y / 4.0).floor() + 2.0) / 3.0).floor()).floor() as i32;
    let n = (n1 - (n2 * n3) + day - 30) as f64;
    let lng_hour = lon / 15.0;
    let t = if sunrise {
        n + ((6.0 - lng_hour) / 24.0)
    } else {
        n + ((18.0 - lng_hour) / 24.0)
    };
    let m = (0.9856 * t) - 3.289;
    let mut l =
        m + (1.916 * m.to_radians().sin()) + (0.020 * (2.0 * m.to_radians()).sin()) + 282.634;
    if l < 0.0 {
        l += 360.0;
    }
    if l > 360.0 {
        l -= 360.0;
    }
    let mut ra = (0.91764 * l.to_radians().tan()).atan().to_degrees();
    if ra < 0.0 {
        ra += 360.0;
    }
    if ra > 360.0 {
        ra -= 360.0;
    }
    let lq = (l / 90.0).floor() * 90.0;
    let rq = (ra / 90.0).floor() * 90.0;
    ra = (ra + (lq - rq)) / 15.0;
    let sin_dec = 0.39782 * l.to_radians().sin();
    let cos_dec = sin_dec.asin().cos();
    let cos_h = (zenith.to_radians().cos() - (sin_dec * lat.to_radians().sin()))
        / (cos_dec * lat.to_radians().cos());
    if !(-1.0..=1.0).contains(&cos_h) {
        return -1.0;
    }
    let mut h = if sunrise {
        360.0 - cos_h.acos().to_degrees()
    } else {
        cos_h.acos().to_degrees()
    };
    h /= 15.0;
    let tt = h + ra - (0.06571 * t) - 6.622;
    let mut ut = tt - lng_hour;
    while ut < 0.0 {
        ut += 24.0;
    }
    while ut >= 24.0 {
        ut -= 24.0;
    }
    ut
}

/// PT toLocal with a fixed bias (minutes east of UTC), returning minutes of day.
fn pt_to_local_min(ut: f64, offset_min: i32) -> i32 {
    let mut local = ut + offset_min as f64 / 60.0;
    while local < 0.0 {
        local += 24.0;
    }
    while local >= 24.0 {
        local -= 24.0;
    }
    let hour = local as i32;
    let minute = ((local - hour as f64) * 60.0) as i32;
    hour * 60 + minute
}

fn pt_sun_minutes(lat: f64, lon: f64, y: i32, m: i32, d: i32, tz_min: i32) -> (i32, i32) {
    (
        pt_to_local_min(pt_calc_ut(lat, lon, y, m, d, true), tz_min),
        pt_to_local_min(pt_calc_ut(lat, lon, y, m, d, false), tz_min),
    )
}

/// LightSwitchUtils.h ShouldBeLight
fn pt_should_be_light(now: i32, light: i32, dark: i32) -> bool {
    let l = light.rem_euclid(1440);
    let d = dark.rem_euclid(1440);
    let n = now.rem_euclid(1440);
    if l < d {
        n >= l && n < d
    } else {
        n >= l || n < d
    }
}

fn mins(t: NaiveTime) -> i32 {
    (t.hour() * 60 + t.minute()) as i32
}

fn circ_diff(a: i32, b: i32) -> i32 {
    let d = (a - b).rem_euclid(1440);
    d.min(1440 - d)
}

fn hm(h: u32, m: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(h, m, 0).unwrap()
}

// ---------- Sunrise / sunset vs PT algorithm ----------

const PLACES: &[(&str, f64, f64)] = &[
    ("Paris", 48.8566, 2.3522),
    ("NewYork", 40.7128, -74.0060),
    ("Sydney", -33.8688, 151.2093),
    ("Tokyo", 35.6762, 139.6503),
    ("Quito", -0.1807, -78.4678),
    ("Reykjavik", 64.1466, -21.9426),
    ("Anchorage", 61.2181, -149.9003),
    ("CapeTown", -33.9249, 18.4241),
    ("Honolulu", 21.3069, -157.8583),
    ("Auckland", -36.8485, 174.7633),
];
const DATES: &[(i32, u32, u32)] = &[
    (2025, 1, 15),
    (2025, 3, 20),
    (2025, 6, 21),
    (2025, 9, 23),
    (2025, 12, 21),
    (2024, 2, 29),
];

#[test]
fn sun_times_match_pt_algorithm_within_5_minutes() {
    let mut bad = vec![];
    let mut max = 0;
    for &(name, lat, lon) in PLACES {
        for &(y, m, d) in DATES {
            let (r, s) = solar::sunrise_sunset(y, m, d, lat, lon);
            let (pr, ps) = pt_sun_minutes(lat, lon, y, m as i32, d as i32, 0);
            let (dr, ds) = (circ_diff(mins(r), pr), circ_diff(mins(s), ps));
            max = max.max(dr).max(ds);
            if dr > 5 || ds > 5 {
                // NOAA vs Almanac formulas differ up to ~4 min at high latitude
                bad.push(format!(
                    "{name} {y}-{m}-{d}: rust {r}/{s} pt {:02}:{:02}/{:02}:{:02}",
                    pr / 60,
                    pr % 60,
                    ps / 60,
                    ps % 60
                ));
            }
        }
    }
    eprintln!("max deviation vs PT: {max} min");
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Independent references (UTC), from published almanac tables (±3 min).
#[test]
fn sun_times_match_published_references() {
    // (name, lat, lon, date, sunrise UTC, sunset UTC)
    let refs = [
        (
            "Paris summer",
            48.8566,
            2.3522,
            (2025, 6, 21),
            hm(3, 47),
            hm(19, 58),
        ),
        (
            "NY winter",
            40.7128,
            -74.0060,
            (2025, 12, 21),
            hm(12, 16),
            hm(21, 32),
        ),
        (
            "Sydney summer",
            -33.8688,
            151.2093,
            (2025, 12, 21),
            hm(18, 41),
            hm(9, 5),
        ),
        (
            "Tokyo equinox",
            35.6762,
            139.6503,
            (2025, 3, 20),
            hm(20, 45),
            hm(8, 53),
        ),
    ];
    let mut bad = vec![];
    for (n, lat, lon, (y, m, d), er, es) in refs {
        let (r, s) = solar::sunrise_sunset(y, m, d, lat, lon);
        if circ_diff(mins(r), mins(er)) > 3 || circ_diff(mins(s), mins(es)) > 3 {
            bad.push(format!("{n}: got {r}/{s}, expected {er}/{es}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn local_time_conversion_matches_pt_for_timezones() {
    // scheduled sunrise/sunset in local time for Paris (UTC+2 in summer) and
    // Sydney (UTC+11 in Dec), offsets 0
    let cases = [
        (48.8566, 2.3522, 120, (2025, 6, 21)),
        (-33.8688, 151.2093, 660, (2025, 12, 21)),
        (40.7128, -74.0060, -300, (2025, 12, 21)),
    ];
    for (lat, lon, tz, (y, m, d)) in cases {
        let (pr, ps) = pt_sun_minutes(lat, lon, y, m, d, tz);
        let cfg = LightSwitchConfig {
            schedule_mode: ScheduleMode::SunsetSunrise,
            latitude: lat,
            longitude: lon,
            ..Default::default()
        };
        let off = FixedOffset::east_opt(tz * 60).unwrap();
        // Probe 5 minutes after PT's sunrise and sunset; skip the boundary
        // band where algorithms may disagree by a minute or two.
        for (probe, expect_dark) in [
            (pr + 5, false),
            (ps + 5, true),
            (pr - 5, true),
            (ps - 5, false),
        ] {
            let p = probe.rem_euclid(1440);
            let now = off
                .with_ymd_and_hms(y, m as u32, d as u32, (p / 60) as u32, (p % 60) as u32, 0)
                .unwrap();
            assert_eq!(
                scheduled_dark(&cfg, now),
                Some(expect_dark),
                "lat {lat} tz {tz} probe {p}"
            );
        }
    }
}

// ---------- Polar day / night ----------

/// PT: calcTime returns -1, toLocal turns it into 23:00 (UTC) for both
/// sunrise and sunset, so light == dark and ShouldBeLight is always true.
/// i.e. PT stays in LIGHT mode all day during polar day and polar night.
#[test]
fn polar_day_and_night_follow_pt_behaviour() {
    let utc = FixedOffset::east_opt(0).unwrap();
    for &(lat, lon) in &[(69.6492, 18.9553), (78.2232, 15.6267)] {
        for &(m, d) in &[(6u32, 21u32), (12, 21)] {
            let (pr, ps) = pt_sun_minutes(lat, lon, 2025, m as i32, d as i32, 0);
            let cfg = LightSwitchConfig {
                schedule_mode: ScheduleMode::SunsetSunrise,
                latitude: lat,
                longitude: lon,
                ..Default::default()
            };
            for h in [0u32, 6, 12, 18, 23] {
                let now = utc.with_ymd_and_hms(2025, m, d, h, 30, 0).unwrap();
                let pt_light = pt_should_be_light((h * 60 + 30) as i32, pr, ps);
                assert_eq!(
                    scheduled_dark(&cfg, now),
                    Some(!pt_light),
                    "lat {lat} {m}-{d} {h}:30 (pt rise {pr} set {ps})"
                );
            }
        }
    }
}

#[test]
fn polar_does_not_panic_for_extreme_inputs() {
    for lat in [-90.0, -89.99, 89.99, 90.0, 66.56, -66.56] {
        for (m, d) in [(3, 20), (6, 21), (12, 21)] {
            let _ = solar::sunrise_sunset(2025, m, d, lat, 0.0);
        }
    }
    let _ = solar::sunrise_sunset(2025, 2, 30, 45.0, 0.0); // invalid date
    let _ = solar::sunrise_sunset(2025, 6, 21, f64::NAN, 0.0);
}

// ---------- Offsets ----------

#[test]
fn offsets_shift_boundaries_like_pt() {
    // Paris summer in UTC+2; sunset offset +30 → still light 20 min after
    // sunset; sunrise offset -45 → already light 30 min before sunrise.
    let (lat, lon, tz) = (48.8566, 2.3522, 120);
    let (pr, ps) = pt_sun_minutes(lat, lon, 2025, 6, 21, tz);
    let cfg = LightSwitchConfig {
        schedule_mode: ScheduleMode::SunsetSunrise,
        latitude: lat,
        longitude: lon,
        sunrise_offset_min: -45,
        sunset_offset_min: 30,
        ..Default::default()
    };
    let off = FixedOffset::east_opt(tz * 60).unwrap();
    let at = |mm: i32| {
        let p = mm.rem_euclid(1440);
        off.with_ymd_and_hms(2025, 6, 21, (p / 60) as u32, (p % 60) as u32, 0)
            .unwrap()
    };
    for (probe, pt_light) in [
        (ps + 20, true),
        (ps + 35, false),
        (pr - 30, true),
        (pr - 50, false),
    ] {
        assert_eq!(pt_should_be_light(probe, pr - 45, ps + 30), pt_light);
        assert_eq!(
            scheduled_dark(&cfg, at(probe)),
            Some(!pt_light),
            "probe {probe}"
        );
    }
}

#[test]
fn offset_wraps_across_midnight() {
    // Sunset offset large enough to push dark start past midnight.
    let cfg = LightSwitchConfig {
        schedule_mode: ScheduleMode::SunsetSunrise,
        latitude: 0.0,
        longitude: 0.0,
        sunset_offset_min: 7 * 60, // ~18:07 + 7h = ~01:07
        ..Default::default()
    };
    let utc = FixedOffset::east_opt(0).unwrap();
    let at = |h, m| utc.with_ymd_and_hms(2025, 3, 20, h, m, 0).unwrap();
    assert_eq!(scheduled_dark(&cfg, at(23, 30)), Some(false));
    assert_eq!(scheduled_dark(&cfg, at(0, 30)), Some(false));
    assert_eq!(scheduled_dark(&cfg, at(2, 0)), Some(true));
    assert_eq!(scheduled_dark(&cfg, at(7, 0)), Some(false));
}

// ---------- Fixed hours / transition logic ----------

#[test]
fn is_dark_between_equals_not_should_be_light_exhaustive() {
    // Every minute for a grid of light/dark times, including equal times.
    let times = [0, 1, 359, 360, 480, 719, 720, 1080, 1200, 1380, 1439];
    for &l in &times {
        for &d in &times {
            for now in 0..1440 {
                let r = is_dark_between(
                    hm((now / 60) as u32, (now % 60) as u32),
                    hm((d / 60) as u32, (d % 60) as u32),
                    hm((l / 60) as u32, (l % 60) as u32),
                );
                assert_eq!(
                    r,
                    !pt_should_be_light(now, l, d),
                    "now {now} light {l} dark {d}"
                );
            }
        }
    }
}

#[test]
fn fixed_schedule_across_midnight() {
    let cfg = LightSwitchConfig {
        schedule_mode: ScheduleMode::Fixed,
        dark_mode_time: "22:00".into(),
        light_mode_time: "07:00".into(),
        ..Default::default()
    };
    let utc = FixedOffset::east_opt(0).unwrap();
    let at = |d, h, m| utc.with_ymd_and_hms(2025, 12, d, h, m, 0).unwrap();
    assert_eq!(scheduled_dark(&cfg, at(31, 21, 59)), Some(false));
    assert_eq!(scheduled_dark(&cfg, at(31, 22, 0)), Some(true));
    assert_eq!(scheduled_dark(&cfg, at(31, 23, 59)), Some(true));
    let ny = utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
    assert_eq!(scheduled_dark(&cfg, ny), Some(true));
    assert_eq!(scheduled_dark(&cfg, at(1, 6, 59)), Some(true));
    assert_eq!(scheduled_dark(&cfg, at(1, 7, 0)), Some(false));
}

#[test]
fn fixed_schedule_inverted_and_equal() {
    let utc = FixedOffset::east_opt(0).unwrap();
    let at = |h, m| utc.with_ymd_and_hms(2025, 5, 5, h, m, 0).unwrap();
    // Light starts 20:00, dark starts 06:00 (night-shift worker).
    let cfg = LightSwitchConfig {
        schedule_mode: ScheduleMode::Fixed,
        dark_mode_time: "06:00".into(),
        light_mode_time: "20:00".into(),
        ..Default::default()
    };
    assert_eq!(scheduled_dark(&cfg, at(12, 0)), Some(true));
    assert_eq!(scheduled_dark(&cfg, at(23, 0)), Some(false));
    assert_eq!(scheduled_dark(&cfg, at(3, 0)), Some(false));
    // Equal times: PT ShouldBeLight → always light.
    let cfg = LightSwitchConfig {
        schedule_mode: ScheduleMode::Fixed,
        dark_mode_time: "09:00".into(),
        light_mode_time: "09:00".into(),
        ..Default::default()
    };
    for h in [0, 8, 9, 10, 23] {
        assert_eq!(scheduled_dark(&cfg, at(h, 0)), Some(false), "{h}");
    }
}

#[test]
fn transitions_over_a_day_match_pt() {
    // Simulate the 30 s scheduler over 48 h for fixed 20:00/08:00 and
    // count flips: PT flips exactly twice per day.
    let cfg = LightSwitchConfig {
        schedule_mode: ScheduleMode::Fixed,
        dark_mode_time: "20:00".into(),
        light_mode_time: "08:00".into(),
        ..Default::default()
    };
    let utc = FixedOffset::east_opt(0).unwrap();
    let start = utc.with_ymd_and_hms(2025, 3, 29, 12, 0, 0).unwrap();
    let mut last = scheduled_dark(&cfg, start);
    let mut flips = vec![];
    for i in 1..(48 * 120) {
        let now = start + chrono::Duration::seconds(30 * i);
        let d = scheduled_dark(&cfg, now);
        if d != last {
            flips.push(now.format("%d %H:%M").to_string());
        }
        last = d;
    }
    assert_eq!(flips, vec!["29 20:00", "30 08:00", "30 20:00", "31 08:00"]);
}

// ---------- Defaults / config ----------

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn default_light_time_is_0800() {
    // PT DefaultLightTime = 480 (08:00), DefaultDarkTime = 1200 (20:00)
    let c = LightSwitchConfig::default();
    assert_eq!(c.dark_mode_time, "20:00");
    assert_eq!(c.light_mode_time, "08:00");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn default_hotkey_is_ctrl_super_shift_d() {
    // PT DefaultToggleThemeHotkey = Ctrl+Win+Shift+D
    let h = mpt_common::hotkey::Hotkey::parse(config::DEFAULT_HOTKEY).unwrap();
    let pt = mpt_common::hotkey::Hotkey::parse("Ctrl+Super+Shift+D").unwrap();
    assert_eq!(h.to_string(), pt.to_string());
}

#[test]
fn default_other_values_match_pt() {
    let c = LightSwitchConfig::default();
    assert_eq!(c.schedule_mode, ScheduleMode::Off);
    assert!(c.apply_system && c.apply_apps);
    assert_eq!((c.sunrise_offset_min, c.sunset_offset_min), (0, 0));
    assert_eq!((c.latitude, c.longitude), (0.0, 0.0));
}

#[test]
fn invalid_fixed_time_is_not_silently_replaced() {
    // The settings page accepts free text. "8:00 PM" or "25:00" silently
    // fall back to 20:00/06:00 instead of being rejected. PT uses a
    // TimePicker so invalid values cannot occur; parse_time must at least
    // accept single-digit hours, which TimePicker-style input produces.
    assert_eq!(parse_time("8:00"), Some(hm(8, 0)));
    assert_eq!(parse_time(" 07:30 "), Some(hm(7, 30)));
    assert_eq!(parse_time("25:00"), None);
}
