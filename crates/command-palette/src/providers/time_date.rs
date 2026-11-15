//! Current time and date in several formats (upstream Time and Date).
//! Typing a number after the keyword converts a Unix timestamp.

use chrono::{DateTime, Local, TimeZone, Utc};

use super::{PaletteResult, Provider, QueryContext, ResultAction, ResultIcon, strip_keyword};

/// Plain queries that show time/date results when global results are on.
const GLOBAL_TRIGGERS: &[&str] = &["time", "date", "now", "today", "week", "unix", "clock"];

pub struct TimeDateProvider {
    keyword: String,
    global: bool,
}

impl TimeDateProvider {
    pub fn new(keyword: String, global: bool) -> Self {
        Self { keyword, global }
    }

    fn global_trigger(&self, raw: &str) -> bool {
        let q = raw.trim().to_lowercase();
        self.global && GLOBAL_TRIGGERS.contains(&q.as_str())
    }
}

/// `(label, value)` rows for `now`, filtered by `filter` (matched against
/// the label, case-insensitive). A numeric filter is read as a Unix
/// timestamp and converted instead.
pub fn entries<Tz: TimeZone>(now: DateTime<Tz>, filter: &str) -> Vec<(String, String)>
where
    Tz::Offset: std::fmt::Display,
{
    let filter = filter.trim().to_lowercase();

    if let Ok(ts) = filter.parse::<i64>() {
        // Treat 13-digit values as milliseconds.
        let (secs, label) = if filter.len() >= 13 {
            (ts / 1000, "milliseconds")
        } else {
            (ts, "seconds")
        };
        let Some(utc) = Utc.timestamp_opt(secs, 0).single() else {
            return Vec::new();
        };
        let local = utc.with_timezone(&now.timezone());
        return vec![
            (
                format!("Unix {label} {ts} (local)"),
                local.format("%Y-%m-%d %H:%M:%S %:z").to_string(),
            ),
            (
                format!("Unix {label} {ts} (UTC)"),
                utc.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            ),
        ];
    }

    let utc = now.with_timezone(&Utc);
    let rows = vec![
        ("Time".to_string(), now.format("%H:%M:%S").to_string()),
        ("Date".to_string(), now.format("%A, %-d %B %Y").to_string()),
        ("Short date".to_string(), now.format("%Y-%m-%d").to_string()),
        (
            "Date and time".to_string(),
            now.format("%Y-%m-%d %H:%M:%S").to_string(),
        ),
        (
            "ISO 8601".to_string(),
            now.format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
        ),
        (
            "UTC time".to_string(),
            utc.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        ),
        ("Unix timestamp".to_string(), now.timestamp().to_string()),
        ("Week number".to_string(), now.format("%V").to_string()),
        ("Day of year".to_string(), now.format("%j").to_string()),
        ("Time zone".to_string(), now.format("%Z (%:z)").to_string()),
    ];
    if filter.is_empty() || (GLOBAL_TRIGGERS.contains(&filter.as_str()) && filter != "week") {
        return rows;
    }
    rows.into_iter()
        .filter(|(label, _)| label.to_lowercase().contains(&filter))
        .collect()
}

impl Provider for TimeDateProvider {
    fn tag(&self) -> &'static str {
        "time"
    }

    fn matches(&self, raw_query: &str) -> bool {
        self.keyword_match(raw_query) || self.global_trigger(raw_query)
    }

    fn keyword_match(&self, raw_query: &str) -> bool {
        strip_keyword(raw_query, &self.keyword).is_some()
    }

    fn strip_prefix<'a>(&self, raw_query: &'a str) -> &'a str {
        strip_keyword(raw_query, &self.keyword).unwrap_or(raw_query.trim())
    }

    fn search(&self, ctx: &QueryContext) -> Vec<PaletteResult> {
        let by_keyword = self.keyword_match(ctx.raw_query);
        let base = if by_keyword { 250.0 } else { 90.0 };
        entries(Local::now(), ctx.stripped_query)
            .into_iter()
            .take(ctx.max_results)
            .enumerate()
            .map(|(i, (label, value))| PaletteResult {
                id: format!("time:{label}"),
                title: value.clone(),
                subtitle: Some(format!("{label} · Enter to copy")),
                icon: ResultIcon::Emoji('🕒'),
                action: ResultAction::CopyToClipboard(value),
                score: base - i as f64,
                provider_tag: "time",
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::FixedOffset;

    fn fixed_now() -> DateTime<FixedOffset> {
        FixedOffset::east_opt(3600)
            .unwrap()
            .with_ymd_and_hms(2025, 3, 14, 15, 9, 26)
            .unwrap()
    }

    #[test]
    fn all_rows() {
        let rows = entries(fixed_now(), "");
        assert!(rows.contains(&("Time".into(), "15:09:26".into())));
        assert!(rows.contains(&("Short date".into(), "2025-03-14".into())));
        assert!(rows.contains(&("ISO 8601".into(), "2025-03-14T15:09:26+01:00".into())));
        assert!(rows.contains(&("UTC time".into(), "2025-03-14 14:09:26 UTC".into())));
        assert!(rows.contains(&("Week number".into(), "11".into())));
    }

    #[test]
    fn filter_by_label() {
        let rows = entries(fixed_now(), "week");
        assert_eq!(rows, vec![("Week number".to_string(), "11".to_string())]);
    }

    #[test]
    fn unix_conversion() {
        let rows = entries(fixed_now(), "0");
        assert_eq!(rows[1].1, "1970-01-01 00:00:00 UTC");
        assert_eq!(rows[0].1, "1970-01-01 01:00:00 +01:00");
        let ms = entries(fixed_now(), "1700000000000");
        assert!(ms[1].0.contains("milliseconds"));
        assert_eq!(ms[1].1, "2023-11-14 22:13:20 UTC");
    }

    #[test]
    fn global_triggers() {
        let p = TimeDateProvider::new(")".into(), true);
        assert!(p.matches("time"));
        assert!(p.matches(")"));
        assert!(!p.matches("timer"));
        let p = TimeDateProvider::new(")".into(), false);
        assert!(!p.matches("time"));
    }
}
