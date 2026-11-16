//! Time and date plugin (`)`): current time, date, week, Unix timestamp...
//! Enter copies the value. A Unix timestamp (`1700000000`, `u1700000000`,
//! `ums1700000000000`) or a date (`2024-05-01 [13:30[:00]]`) is converted instead
//! of "now". In global results only queries naming a field (`time`, `date`,
//! `week`, `unix`, ...) are answered.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::config::PluginId;
use chrono::{DateTime, Datelike, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone, Utc};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub label: &'static str,
    pub tags: &'static [&'static str],
    pub value: String,
}

/// All values for one point in time.
pub fn entries(dt: DateTime<FixedOffset>) -> Vec<Entry> {
    let e = |label, tags, value: String| Entry { label, tags, value };
    let utc = dt.with_timezone(&Utc);
    let iso = dt.iso_week();
    vec![
        e(
            "Now",
            &["now", "date and time", "datetime"],
            dt.format("%Y-%m-%d %H:%M:%S").to_string(),
        ),
        e(
            "Time",
            &["time", "clock"],
            dt.format("%H:%M:%S").to_string(),
        ),
        e(
            "Date",
            &["date", "today"],
            dt.format("%Y-%m-%d").to_string(),
        ),
        e(
            "Long date",
            &["date", "long date"],
            dt.format("%A, %-d %B %Y").to_string(),
        ),
        e(
            "Day of week",
            &["day", "weekday"],
            dt.format("%A").to_string(),
        ),
        e(
            "Week of year",
            &["week", "calendar week", "kw"],
            format!("{} (ISO {}-W{:02})", iso.week(), iso.year(), iso.week()),
        ),
        e(
            "Day of year",
            &["day of year", "yday"],
            dt.ordinal().to_string(),
        ),
        e("Month", &["month"], dt.format("%B (%m)").to_string()),
        e("Year", &["year"], dt.year().to_string()),
        e(
            "Unix timestamp",
            &["unix", "timestamp", "epoch"],
            dt.timestamp().to_string(),
        ),
        e(
            "Unix timestamp (ms)",
            &["unix", "timestamp", "epoch", "milliseconds"],
            dt.timestamp_millis().to_string(),
        ),
        e("ISO 8601", &["iso", "iso8601", "rfc3339"], dt.to_rfc3339()),
        e(
            "UTC",
            &["utc", "gmt"],
            utc.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        ),
        e(
            "Time zone",
            &["timezone", "time zone", "offset"],
            dt.format("UTC%:z").to_string(),
        ),
    ]
}

/// Parse a timestamp or date from the query, in the given zone.
pub fn parse_time(q: &str, tz: &FixedOffset) -> Option<DateTime<FixedOffset>> {
    let q = q.trim();
    let digits = |s: &str| {
        !s.is_empty()
            && s.trim_start_matches('-')
                .chars()
                .all(|c| c.is_ascii_digit())
    };
    if let Some(ms) = q.strip_prefix("ums").filter(|s| digits(s)) {
        return tz.timestamp_millis_opt(ms.parse().ok()?).single();
    }
    if let Some(s) = q.strip_prefix('u').filter(|s| digits(s)) {
        return tz.timestamp_opt(s.parse().ok()?, 0).single();
    }
    if digits(q) && q.len() >= 5 {
        let n: i64 = q.parse().ok()?;
        // Heuristic like upstream: 13+ digits are milliseconds.
        return if q.trim_start_matches('-').len() >= 13 {
            tz.timestamp_millis_opt(n).single()
        } else {
            tz.timestamp_opt(n, 0).single()
        };
    }
    for fmt in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S"] {
        if let Ok(ndt) = NaiveDateTime::parse_from_str(q, fmt) {
            return tz.from_local_datetime(&ndt).single();
        }
    }
    if let Ok(d) = DateTime::parse_from_rfc3339(q) {
        return Some(d.with_timezone(tz));
    }
    if let Ok(d) = NaiveDate::parse_from_str(q, "%Y-%m-%d") {
        return tz.from_local_datetime(&d.and_hms_opt(0, 0, 0)?).single();
    }
    None
}

/// Select entries for a query at time `now`.
pub fn select(q: &str, by_keyword: bool, now: DateTime<FixedOffset>) -> Vec<Entry> {
    let q = q.trim();
    let ql = q.to_lowercase();
    if by_keyword {
        if q.is_empty() {
            return entries(now);
        }
        if let Some(t) = parse_time(q, now.offset()) {
            return entries(t);
        }
        return entries(now)
            .into_iter()
            .filter(|e| {
                e.label.to_lowercase().contains(&ql) || e.tags.iter().any(|t| t.contains(&ql))
            })
            .collect();
    }
    if ql.len() < 3 {
        return Vec::new();
    }
    entries(now)
        .into_iter()
        .filter(|e| e.tags.iter().any(|t| t.starts_with(&ql)))
        .collect()
}

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let now = Local::now().fixed_offset();
    let score_base = if ctx.by_keyword { 300.0 } else { 45.0 };
    select(ctx.search, ctx.by_keyword, now)
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            LauncherResult::new(PluginId::TimeDate, e.value.clone(), Action::Copy(e.value))
                .subtitle(format!("{} · Enter to copy", e.label))
                .icon(Some("x-office-calendar".into()))
                .score(score_base - i as f64 * 0.1)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(3600)
            .unwrap()
            .with_ymd_and_hms(2024, 1, 15, 13, 30, 5)
            .unwrap()
    }

    fn value(list: &[Entry], label: &str) -> String {
        list.iter()
            .find(|e| e.label == label)
            .unwrap()
            .value
            .clone()
    }

    #[test]
    fn values_for_fixed_time() {
        let e = entries(fixed());
        assert_eq!(value(&e, "Time"), "13:30:05");
        assert_eq!(value(&e, "Date"), "2024-01-15");
        assert_eq!(value(&e, "Day of week"), "Monday");
        assert_eq!(value(&e, "Week of year"), "3 (ISO 2024-W03)");
        assert_eq!(value(&e, "Unix timestamp"), "1705321805");
        assert_eq!(value(&e, "UTC"), "2024-01-15 12:30:05 UTC");
        assert_eq!(value(&e, "Day of year"), "15");
    }

    #[test]
    fn parses_timestamps_and_dates() {
        let tz = FixedOffset::east_opt(0).unwrap();
        assert_eq!(parse_time("0", &tz), None); // too short to be a timestamp
        assert_eq!(
            parse_time("1700000000", &tz).unwrap().timestamp(),
            1_700_000_000
        );
        assert_eq!(
            parse_time("u1700000000", &tz).unwrap().timestamp(),
            1_700_000_000
        );
        assert_eq!(
            parse_time("1700000000123", &tz).unwrap().timestamp_millis(),
            1_700_000_000_123
        );
        assert_eq!(parse_time("1970-01-02", &tz).unwrap().timestamp(), 86400);
        assert_eq!(
            parse_time("1970-01-01 01:00", &tz).unwrap().timestamp(),
            3600
        );
        assert!(parse_time("hello", &tz).is_none());
    }

    #[test]
    fn selection_rules() {
        assert_eq!(select("", true, fixed()).len(), entries(fixed()).len());
        let week = select("week", false, fixed());
        assert!(week.iter().any(|e| e.label == "Week of year"));
        assert!(week.iter().all(|e| e.label != "Time"));
        assert!(select("firefox", false, fixed()).is_empty());
        assert!(select("ti", false, fixed()).is_empty());
        let ts = select("86400", true, fixed());
        assert_eq!(value(&ts, "Date"), "1970-01-02");
    }
}
