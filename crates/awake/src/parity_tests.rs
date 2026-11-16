//! Parity tests against Microsoft PowerToys Awake (H-misc tester).
//! Oracle: src/modules/awake/Awake/{Program.cs,Core/Manager.cs,Core/AwakeStateCalculator.cs}
//! and src/settings-ui/Settings.UI.Library/AwakeProperties.cs.

use super::*;
use config::{AwakeConfig, AwakeMode};

fn awake_with(cfg: AwakeConfig) -> Awake {
    Awake {
        running: false,
        config: cfg,
        stop_flag: Arc::new(AtomicBool::new(false)),
        expired: Arc::new(AtomicBool::new(false)),
        timer_thread: None,
    }
}

/// Port of AwakeStateCalculator.ComputeAwakeState: returns "display required".
fn pt_display_required(keep_display_on: bool, locked: bool, allow_off_locked: bool) -> bool {
    keep_display_on && !(locked && allow_off_locked)
}

/// Port of the Rust worker's decision (lib.rs spawn_worker): display is only
/// re-evaluated when watch_lock is true; otherwise it stays at keep_screen_on.
fn rust_display_required(keep: bool, locked: bool, allow: bool) -> bool {
    let watch_lock = keep && allow;
    if watch_lock { keep && !locked } else { keep }
}

#[test]
fn awake_state_matches_pt_calculator_truth_table() {
    for keep in [false, true] {
        for locked in [false, true] {
            for allow in [false, true] {
                assert_eq!(
                    rust_display_required(keep, locked, allow),
                    pt_display_required(keep, locked, allow),
                    "keep={keep} locked={locked} allow={allow}"
                );
            }
        }
    }
}

// --- Defaults (AwakeProperties ctor) ---

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn default_mode_is_passive() {
    // PT: Mode = AwakeMode.PASSIVE
    assert_eq!(AwakeConfig::default().mode, AwakeMode::Off);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn default_keep_display_on_is_false() {
    // PT: KeepDisplayOn = false
    assert!(!AwakeConfig::default().keep_screen_on);
}

#[test]
fn default_allow_display_off_while_locked_is_true() {
    assert!(AwakeConfig::default().allow_display_off_while_locked);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn default_interval_is_0h_1m() {
    // PT: IntervalHours = 0, IntervalMinutes = 1
    let c = AwakeConfig::default();
    assert_eq!((c.timed_hours, c.timed_minutes), (0, 1));
}

// --- Timed logic ---

#[test]
fn timed_seconds_formula() {
    // PT: computedTime = IntervalHours*3600 + IntervalMinutes*60
    for (h, m, s) in [
        (0, 0, 0),
        (0, 1, 60),
        (1, 30, 5400),
        (2, 0, 7200),
        (25, 59, 93540),
    ] {
        let c = AwakeConfig {
            timed_hours: h,
            timed_minutes: m,
            ..Default::default()
        };
        assert_eq!(c.timed_secs(), s);
    }
}

#[test]
fn timed_deadline_is_now_plus_duration() {
    let a = awake_with(AwakeConfig {
        mode: AwakeMode::Timed,
        timed_hours: 1,
        timed_minutes: 30,
        ..Default::default()
    });
    let before = Instant::now();
    let Some(Deadline::After(t)) = a.deadline() else {
        panic!("expected After deadline")
    };
    let d = t.duration_since(before);
    assert!(
        d <= Duration::from_secs(5401) && d >= Duration::from_secs(5400),
        "{d:?}"
    );
    // not reached just before, reached exactly at deadline
    let local = Local::now().naive_local();
    assert!(!Deadline::After(t).reached(t - Duration::from_millis(1), local));
    assert!(Deadline::After(t).reached(t, local));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn timed_zero_duration_expires_immediately_like_pt() {
    // PT SetTimedKeepAwake(0): Observable.Interval(...).TakeWhile(remaining > 0)
    // completes on the first tick -> HandleTimerCompletion -> Passive.
    // It never keeps the machine awake indefinitely.
    let a = awake_with(AwakeConfig {
        mode: AwakeMode::Timed,
        timed_hours: 0,
        timed_minutes: 0,
        ..Default::default()
    });
    let d = a.deadline().unwrap();
    assert_ne!(
        d,
        Deadline::Never,
        "timed 0h0m keeps awake forever (PT expires at once)"
    );
}

// --- Expirable logic ---

#[test]
fn expirable_future_deadline_not_reached() {
    let future = (Local::now() + chrono::Duration::hours(2)).naive_local();
    let a = awake_with(AwakeConfig {
        mode: AwakeMode::Expirable,
        expire_at: future.format("%Y-%m-%dT%H:%M").to_string(),
        ..Default::default()
    });
    let d = a.deadline().unwrap();
    assert!(matches!(d, Deadline::At(_)));
    assert!(!d.reached(Instant::now(), Local::now().naive_local()));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn expirable_in_past_at_load_is_bumped_by_5_minutes_like_pt() {
    // PT Program.ProcessSettings: if ExpirationDateTime <= Now ->
    // ExpirationDateTime = Now + 5 min, save, then keep awake until then.
    let past = (Local::now() - chrono::Duration::hours(1)).naive_local();
    let a = awake_with(AwakeConfig {
        mode: AwakeMode::Expirable,
        expire_at: past.format("%Y-%m-%dT%H:%M").to_string(),
        ..Default::default()
    });
    let d = a.deadline().unwrap();
    assert!(
        !d.reached(Instant::now(), Local::now().naive_local()),
        "past expiration is treated as already expired (no keep-awake); PT re-arms for now+5min"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn expirable_invalid_date_does_not_keep_awake_forever() {
    // PT never runs expirable mode without a valid DateTimeOffset (CLI
    // validator rejects unparsable --expire-at). Rust falls back to Never.
    let a = awake_with(AwakeConfig {
        mode: AwakeMode::Expirable,
        expire_at: "not a date".into(),
        ..Default::default()
    });
    assert_ne!(a.deadline().unwrap(), Deadline::Never);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn expirable_accepts_pt_settings_iso8601_with_offset() {
    // PT persists expirationDateTime as a DateTimeOffset, e.g.
    // "2026-03-01T18:00:00+01:00" (System.Text.Json round-trip format).
    for s in [
        "2026-03-01T18:00:00+01:00",
        "2026-03-01T18:00:00.1234567+01:00",
    ] {
        let c = AwakeConfig {
            expire_at: s.into(),
            ..Default::default()
        };
        assert!(c.expire_at_local().is_some(), "{s} rejected");
    }
}

#[test]
fn expirable_formats_documented_are_parsed() {
    for s in [
        "2026-03-01T18:00",
        "2026-03-01T18:00:59",
        "2026-03-01 18:00",
        " 2026-03-01T18:00 ",
    ] {
        let c = AwakeConfig {
            expire_at: s.into(),
            ..Default::default()
        };
        assert!(c.expire_at_local().is_some(), "{s}");
    }
    for s in [
        "",
        "2026-13-01T18:00",
        "2026-02-30T10:00",
        "2026-03-01T24:00",
    ] {
        let c = AwakeConfig {
            expire_at: s.into(),
            ..Default::default()
        };
        assert!(c.expire_at_local().is_none(), "{s}");
    }
}

#[test]
fn passive_mode_has_no_deadline() {
    let a = awake_with(AwakeConfig {
        mode: AwakeMode::Off,
        ..Default::default()
    });
    assert!(a.deadline().is_none());
    let a = awake_with(AwakeConfig {
        mode: AwakeMode::Indefinite,
        ..Default::default()
    });
    assert_eq!(a.deadline(), Some(Deadline::Never));
}

#[test]
fn mode_serialization_names() {
    #[derive(serde::Serialize)]
    struct W {
        mode: AwakeMode,
    }
    for (m, s) in [
        (AwakeMode::Off, "off"),
        (AwakeMode::Indefinite, "indefinite"),
        (AwakeMode::Timed, "timed"),
        (AwakeMode::Expirable, "expirable"),
    ] {
        assert_eq!(
            toml::to_string(&W { mode: m }).unwrap().trim(),
            format!("mode = \"{s}\"")
        );
    }
}

/// PT: when the timer/expiration completes under PowerToys config,
/// HandleTimerCompletion -> SetPassiveKeepAwake() writes Mode=PASSIVE to the
/// settings file, so a restart does not re-arm the timer.
/// Requires XDG_CONFIG_HOME to point at a scratch dir (set by the runner).
#[test]
fn expiry_reverts_persisted_mode_to_passive() {
    if std::env::var("MPT_PARITY_SCRATCH").is_err() {
        eprintln!("skipped: set MPT_PARITY_SCRATCH=1 and XDG_CONFIG_HOME to a temp dir");
        return;
    }
    let past = (Local::now() - chrono::Duration::minutes(1)).naive_local();
    let cfg = AwakeConfig {
        mode: AwakeMode::Expirable,
        expire_at: past.format("%Y-%m-%dT%H:%M").to_string(),
        ..Default::default()
    };
    mpt_common::config::save_module_config("awake", &cfg).unwrap();
    let mut a = Awake::new();
    a.start().unwrap();
    assert!(!a.is_running());
    a.check_expired();
    let saved: AwakeConfig = mpt_common::config::load_module_config("awake").unwrap();
    assert_eq!(
        saved.mode,
        AwakeMode::Off,
        "mode stays Expirable after expiry"
    );
}
