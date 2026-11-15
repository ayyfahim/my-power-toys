//! Replacement-string tokens, following PowerRename:
//!
//! - Date/time from the file's creation/modification/access time:
//!   `$YYYY $YY $Y $MMMM $MMM $MM $M $DDDD $DDD $DD $D $hh $h $HH $H $TT $tt
//!   $mm $m $ss $s $fff $ff $f`
//! - Photo metadata (EXIF): `$CAMERA_MAKE`, `$DATE_TAKEN_YYYY`, ... (see
//!   [`METADATA_PATTERNS`]). Unavailable values keep the `$NAME` text.
//! - Enumeration groups: `${}`, `${start=N;increment=N;padding=N}`
//! - Randomizer groups: `${rstringalnum=N}`, `${rstringalpha=N}`,
//!   `${rstringdigit=N}`, `${ruuidv4}`
//!
//! `$$` escapes a dollar sign.

use rand::Rng;
use std::collections::HashMap;

/// Broken-down local date/time used for date tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateParts {
    pub year: i32,
    /// 1..=12
    pub month: u32,
    /// 1..=31
    pub day: u32,
    /// 0..=23
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub millis: u32,
    /// 0 = Sunday .. 6 = Saturday
    pub weekday: u32,
}

impl DateParts {
    pub fn from_system_time(t: std::time::SystemTime) -> Self {
        use chrono::{Datelike, Timelike};
        let dt: chrono::DateTime<chrono::Local> = t.into();
        Self {
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            millis: dt.nanosecond() / 1_000_000 % 1000,
            weekday: dt.weekday().num_days_from_sunday(),
        }
    }
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Date/time tokens, longest first within each letter so prefix matching works.
const DATE_TOKENS: &[&str] = &[
    "YYYY", "YY", "Y", "MMMM", "MMM", "MM", "M", "DDDD", "DDD", "DD", "D", "HH", "H", "TT", "tt",
    "hh", "h", "mm", "m", "ss", "s", "fff", "ff", "f",
];

/// EXIF metadata patterns supported by upstream (`MetadataType::EXIF`).
pub const METADATA_PATTERNS: &[&str] = &[
    "CAMERA_MAKE",
    "CAMERA_MODEL",
    "LENS",
    "ISO",
    "APERTURE",
    "SHUTTER",
    "FOCAL",
    "FLASH",
    "WIDTH",
    "HEIGHT",
    "AUTHOR",
    "COPYRIGHT",
    "LATITUDE",
    "LONGITUDE",
    "DATE_TAKEN_YYYY",
    "DATE_TAKEN_YY",
    "DATE_TAKEN_MM",
    "DATE_TAKEN_DD",
    "DATE_TAKEN_HH",
    "DATE_TAKEN_mm",
    "DATE_TAKEN_SS",
    "EXPOSURE_BIAS",
    "ORIENTATION",
    "COLOR_SPACE",
    "ALTITUDE",
];

/// Extensions upstream reads metadata from.
pub const METADATA_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "tif", "tiff", "heic", "heif", "avif"];

fn date_token_value(token: &str, d: &DateParts) -> String {
    let hour12 = match d.hour % 12 {
        0 => 12,
        h => h,
    };
    match token {
        "YYYY" => format!("{:04}", d.year),
        "YY" => format!("{:02}", d.year.rem_euclid(100)),
        "Y" => format!("{}", d.year.rem_euclid(10)),
        "MMMM" => MONTHS[(d.month as usize).clamp(1, 12) - 1].to_string(),
        "MMM" => MONTHS[(d.month as usize).clamp(1, 12) - 1][..3].to_string(),
        "MM" => format!("{:02}", d.month),
        "M" => format!("{}", d.month),
        "DDDD" => WEEKDAYS[(d.weekday as usize) % 7].to_string(),
        "DDD" => WEEKDAYS[(d.weekday as usize) % 7][..3].to_string(),
        "DD" => format!("{:02}", d.day),
        "D" => format!("{}", d.day),
        "HH" => format!("{hour12:02}"),
        "H" => format!("{hour12}"),
        "TT" => (if d.hour < 12 { "AM" } else { "PM" }).to_string(),
        "tt" => (if d.hour < 12 { "am" } else { "pm" }).to_string(),
        "hh" => format!("{:02}", d.hour),
        "h" => format!("{}", d.hour),
        "mm" => format!("{:02}", d.minute),
        "m" => format!("{}", d.minute),
        "ss" => format!("{:02}", d.second),
        "s" => format!("{}", d.second),
        "fff" => format!("{:03}", d.millis),
        "ff" => format!("{:02}", d.millis / 10),
        _ => format!("{}", d.millis / 100),
    }
}

fn longest_prefix<'a>(rest: &str, candidates: &[&'a str]) -> Option<&'a str> {
    candidates
        .iter()
        .filter(|c| rest.starts_with(**c))
        .max_by_key(|c| c.len())
        .copied()
}

/// Walk over unescaped `$` positions, calling `f(rest_after_dollar)` which may
/// return `(consumed_len, replacement)`. Escaped `$$` pairs are kept verbatim.
fn scan_dollars(template: &str, mut f: impl FnMut(&str) -> Option<(usize, String)>) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let count = after.bytes().take_while(|&b| b == b'$').count();
        let tail = &after[count..];
        if count % 2 == 0 {
            out.push_str(&after[..count]);
            rest = tail;
            continue;
        }
        // Odd: pairs are escapes, the last `$` may start a token.
        out.push_str(&after[..count - 1]);
        match f(tail) {
            Some((len, value)) => {
                out.push_str(&value);
                rest = &tail[len..];
            }
            None => {
                out.push('$');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Upstream `isFileTimeUsed`: does the template contain a date/time token?
pub fn uses_file_time(template: &str) -> bool {
    let mut used = false;
    scan_dollars(template, |rest| {
        if longest_prefix(rest, METADATA_PATTERNS).is_none()
            && longest_prefix(rest, DATE_TOKENS).is_some()
        {
            used = true;
        }
        None
    });
    used
}

/// Does the template contain a metadata token?
pub fn uses_metadata(template: &str) -> bool {
    let mut used = false;
    scan_dollars(template, |rest| {
        if longest_prefix(rest, METADATA_PATTERNS).is_some() {
            used = true;
        }
        None
    });
    used
}

/// Expand `$`-tokens. Date tokens are expanded only when `time` is given;
/// metadata tokens are expanded only when `metadata` is given (missing values
/// keep the `$NAME` text, like upstream). Other `$` sequences (`$1`, `$$`)
/// are left untouched for the regex engine.
pub fn expand_dollar_tokens(
    template: &str,
    time: Option<&DateParts>,
    metadata: Option<&HashMap<String, String>>,
) -> String {
    scan_dollars(template, |rest| {
        if let Some(pat) = longest_prefix(rest, METADATA_PATTERNS) {
            let meta = metadata?;
            let value = meta
                .get(pat)
                .filter(|v| !v.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("${pat}"));
            return Some((pat.len(), value));
        }
        let d = time?;
        let tok = longest_prefix(rest, DATE_TOKENS)?;
        Some((tok.len(), date_token_value(tok, d)))
    })
}

/// A `${...}` group in the replacement string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Group {
    Enumerate {
        start: i64,
        increment: i64,
        padding: usize,
    },
    Random(RandomKind),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RandomKind {
    /// Characters drawn from this set, `len` times.
    Chars {
        set: String,
        len: usize,
    },
    Uuid,
}

const ALPHA: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
/// Avoid absurd names; upstream is bounded by MAX_PATH.
const MAX_RANDOM_LEN: i64 = 255;

fn find_number(inner: &str, key: &str, allow_negative: bool) -> Option<i64> {
    let idx = inner.find(key)?;
    let tail = &inner[idx + key.len()..];
    let bytes = tail.as_bytes();
    let mut end = 0;
    if allow_negative && bytes.first() == Some(&b'-') {
        end = 1;
    }
    let digits_start = end;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    if end == digits_start {
        return None;
    }
    tail[..end].parse().ok()
}

fn parse_random(inner: &str) -> Option<RandomKind> {
    let mut set = String::new();
    let mut len: Option<i64> = None;
    let mut found = false;
    if let Some(n) = find_number(inner, "rstringalnum=", false) {
        set.push_str(ALPHA);
        set.push_str(DIGITS);
        len = Some(n);
        found = true;
    }
    if let Some(n) = find_number(inner, "rstringalpha=", true) {
        set.push_str(ALPHA);
        len = Some(n);
        found = true;
    }
    if let Some(n) = find_number(inner, "rstringdigit=", false) {
        set.push_str(DIGITS);
        len = Some(n);
        found = true;
    }
    if inner.contains("ruuidv4") {
        return Some(RandomKind::Uuid);
    }
    if !found {
        return None;
    }
    let len = len.unwrap_or(10).clamp(0, MAX_RANDOM_LEN) as usize;
    Some(RandomKind::Chars { set, len })
}

fn parse_enum(inner: &str) -> Group {
    Group::Enumerate {
        start: find_number(inner, "start=", true).unwrap_or(0),
        increment: find_number(inner, "increment=", true).unwrap_or(1),
        padding: find_number(inner, "padding=", false)
            .unwrap_or(0)
            .clamp(0, 64) as usize,
    }
}

/// Classify a group's inner text. Randomizer wins over enumerator when both
/// are enabled (upstream semantics); returns None if neither applies.
pub fn classify_group(inner: &str, enumerate: bool, randomize: bool) -> Option<Group> {
    if randomize && let Some(r) = parse_random(inner) {
        return Some(Group::Random(r));
    }
    enumerate.then(|| parse_enum(inner))
}

/// Does the template contain any `${...}` group that would be expanded?
pub fn has_groups(template: &str, enumerate: bool, randomize: bool) -> bool {
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            return false;
        };
        if classify_group(&after[..end], enumerate, randomize).is_some() {
            return true;
        }
        rest = &after[end + 1..];
    }
    false
}

fn random_value(kind: &RandomKind, rng: &mut impl Rng) -> String {
    match kind {
        RandomKind::Uuid => {
            let mut b: [u8; 16] = rng.random();
            b[6] = (b[6] & 0x0f) | 0x40; // version 4
            b[8] = (b[8] & 0x3f) | 0x80; // RFC 4122 variant
            let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
            format!(
                "{}-{}-{}-{}-{}",
                &h[0..8],
                &h[8..12],
                &h[12..16],
                &h[16..20],
                &h[20..32]
            )
        }
        RandomKind::Chars { set, len } => {
            let chars: Vec<char> = set.chars().collect();
            if chars.is_empty() {
                return String::new();
            }
            (0..*len)
                .map(|_| chars[rng.random_range(0..chars.len())])
                .collect()
        }
    }
}

/// Expand `${...}` enumeration/randomizer groups for item number `index`
/// (0-based count of matched items so far).
pub fn expand_groups(
    template: &str,
    enumerate: bool,
    randomize: bool,
    index: u64,
    rng: &mut impl Rng,
) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            break;
        };
        let inner = &after[..end];
        out.push_str(&rest[..start]);
        match classify_group(inner, enumerate, randomize) {
            Some(Group::Enumerate {
                start: s,
                increment,
                padding,
            }) => {
                let value = s.saturating_add(increment.saturating_mul(index as i64));
                out.push_str(&format!("{value:0padding$}"));
            }
            Some(Group::Random(kind)) => out.push_str(&random_value(&kind, rng)),
            None => {
                out.push_str("${");
                out.push_str(inner);
                out.push('}');
            }
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Convert ECMAScript-style `$1`..`$99` group references into the `${1}`
/// form so the Rust regex crate doesn't read `$1_foo` as a group named `1_foo`.
pub fn normalize_group_refs(replacement: &str) -> String {
    scan_dollars(replacement, |rest| {
        let digits = rest
            .bytes()
            .take(2)
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        Some((digits, format!("${{{}}}", &rest[..digits])))
    })
}

// ── EXIF extraction ─────────────────────────────────────────────────────

/// Format a shutter speed in seconds the way upstream does.
pub fn format_shutter(speed: f64) -> String {
    if speed <= 0.0 {
        return "0".into();
    }
    if speed >= 1.0 {
        return format!("{speed:.1}s");
    }
    let recip = (1.0 / speed).round();
    if recip <= 1.0 {
        return format!("{speed:.3}s");
    }
    format!("1/{recip:.0}s")
}

/// Format a GPS coordinate like `47°36.35'N`.
pub fn format_coordinate(coord: f64, is_latitude: bool) -> String {
    let dir = match (is_latitude, coord >= 0.0) {
        (true, true) => 'N',
        (true, false) => 'S',
        (false, true) => 'E',
        (false, false) => 'W',
    };
    let abs = coord.abs();
    let deg = abs.trunc();
    let minutes = (abs - deg) * 60.0;
    format!("{}\u{00B0}{minutes:.2}'{dir}", deg as i64)
}

/// Parse an EXIF `YYYY:MM:DD HH:MM:SS` timestamp into tokens.
pub fn date_taken_patterns(s: &str, out: &mut HashMap<String, String>) {
    let s = s.trim();
    let num = |a: usize, b: usize| -> Option<u32> { s.get(a..b)?.parse().ok() };
    let (Some(y), Some(mo), Some(d), Some(h), Some(mi), Some(se)) = (
        num(0, 4),
        num(5, 7),
        num(8, 10),
        num(11, 13),
        num(14, 16),
        num(17, 19),
    ) else {
        return;
    };
    out.insert("DATE_TAKEN_YYYY".into(), format!("{y:04}"));
    out.insert("DATE_TAKEN_YY".into(), format!("{:02}", y % 100));
    out.insert("DATE_TAKEN_MM".into(), format!("{mo:02}"));
    out.insert("DATE_TAKEN_DD".into(), format!("{d:02}"));
    out.insert("DATE_TAKEN_HH".into(), format!("{h:02}"));
    out.insert("DATE_TAKEN_mm".into(), format!("{mi:02}"));
    out.insert("DATE_TAKEN_SS".into(), format!("{se:02}"));
}

/// Read EXIF metadata patterns from a photo. Returns an empty map when the
/// file has no readable EXIF data.
pub fn read_exif_patterns(path: &std::path::Path) -> HashMap<String, String> {
    use exif::{In, Tag, Value};

    let mut out = HashMap::new();
    let Ok(file) = std::fs::File::open(path) else {
        return out;
    };
    let mut reader = std::io::BufReader::new(file);
    let Ok(data) = exif::Reader::new().read_from_container(&mut reader) else {
        return out;
    };

    let field = |tag: Tag| data.get_field(tag, In::PRIMARY);
    let ascii = |tag: Tag| -> Option<String> {
        match &field(tag)?.value {
            Value::Ascii(v) => {
                let s = v
                    .iter()
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .collect::<Vec<_>>()
                    .join(" ");
                let s = s.trim_matches(|c: char| c == '\0' || c.is_whitespace());
                (!s.is_empty()).then(|| s.to_string())
            }
            _ => None,
        }
    };
    let uint = |tag: Tag| field(tag)?.value.get_uint(0);
    let rational = |tag: Tag| -> Option<f64> {
        match &field(tag)?.value {
            Value::Rational(v) => v.first().map(|r| r.to_f64()),
            Value::SRational(v) => v.first().map(|r| r.to_f64()),
            _ => None,
        }
    };
    let gps = |tag: Tag, ref_tag: Tag, negative: &str| -> Option<f64> {
        let Value::Rational(v) = &field(tag)?.value else {
            return None;
        };
        let parts: Vec<f64> = v.iter().map(|r| r.to_f64()).collect();
        let deg = parts.first().copied().unwrap_or(0.0)
            + parts.get(1).copied().unwrap_or(0.0) / 60.0
            + parts.get(2).copied().unwrap_or(0.0) / 3600.0;
        let neg = ascii(ref_tag).is_some_and(|r| r.eq_ignore_ascii_case(negative));
        Some(if neg { -deg } else { deg })
    };

    let mut put = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            out.insert(k.to_string(), v);
        }
    };
    put("CAMERA_MAKE", ascii(Tag::Make));
    put("CAMERA_MODEL", ascii(Tag::Model));
    put("LENS", ascii(Tag::LensModel));
    put(
        "ISO",
        uint(Tag::PhotographicSensitivity).map(|v| {
            if v == 0 {
                "ISO".to_string()
            } else {
                format!("ISO {v}")
            }
        }),
    );
    put(
        "APERTURE",
        rational(Tag::FNumber).map(|f| format!("f/{f:.1}")),
    );
    put("SHUTTER", rational(Tag::ExposureTime).map(format_shutter));
    put(
        "FOCAL",
        rational(Tag::FocalLength).map(|f| format!("{}mm", f as i64)),
    );
    put(
        "FLASH",
        uint(Tag::Flash).map(|f| if f & 1 == 1 { "Flash On" } else { "Flash Off" }.to_string()),
    );
    put(
        "WIDTH",
        uint(Tag::PixelXDimension)
            .or_else(|| uint(Tag::ImageWidth))
            .map(|v| v.to_string()),
    );
    put(
        "HEIGHT",
        uint(Tag::PixelYDimension)
            .or_else(|| uint(Tag::ImageLength))
            .map(|v| v.to_string()),
    );
    put("AUTHOR", ascii(Tag::Artist));
    put("COPYRIGHT", ascii(Tag::Copyright));
    put(
        "LATITUDE",
        gps(Tag::GPSLatitude, Tag::GPSLatitudeRef, "S").map(|c| format_coordinate(c, true)),
    );
    put(
        "LONGITUDE",
        gps(Tag::GPSLongitude, Tag::GPSLongitudeRef, "W").map(|c| format_coordinate(c, false)),
    );
    put(
        "EXPOSURE_BIAS",
        rational(Tag::ExposureBiasValue).map(|v| format!("{v:.2}")),
    );
    put("ORIENTATION", uint(Tag::Orientation).map(|v| v.to_string()));
    put("COLOR_SPACE", uint(Tag::ColorSpace).map(|v| v.to_string()));
    let below_sea = uint(Tag::GPSAltitudeRef) == Some(1);
    put(
        "ALTITUDE",
        rational(Tag::GPSAltitude).map(|a| format!("{:.2} m", if below_sea { -a } else { a })),
    );
    if let Some(dt) = ascii(Tag::DateTimeOriginal).or_else(|| ascii(Tag::DateTime)) {
        date_taken_patterns(&dt, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn sample_date() -> DateParts {
        // Friday 2024-03-08 14:05:09.123
        DateParts {
            year: 2024,
            month: 3,
            day: 8,
            hour: 14,
            minute: 5,
            second: 9,
            millis: 123,
            weekday: 5,
        }
    }

    #[test]
    fn date_tokens_expand() {
        let d = sample_date();
        let e = |t: &str| expand_dollar_tokens(t, Some(&d), None);
        assert_eq!(e("$YYYY-$MM-$DD"), "2024-03-08");
        assert_eq!(e("$YY $Y $M $D"), "24 4 3 8");
        assert_eq!(e("$MMMM $MMM $DDDD $DDD"), "March Mar Friday Fri");
        assert_eq!(e("$hh:$mm:$ss.$fff"), "14:05:09.123");
        assert_eq!(e("$h $m $s $ff $f"), "14 5 9 12 1");
        assert_eq!(e("$HH $H $TT $tt"), "02 2 PM pm");
    }

    #[test]
    fn date_tokens_midnight_12h() {
        let d = DateParts {
            hour: 0,
            ..sample_date()
        };
        assert_eq!(expand_dollar_tokens("$HH$TT", Some(&d), None), "12AM");
    }

    #[test]
    fn dollar_escape_and_group_refs_untouched() {
        let d = sample_date();
        assert_eq!(expand_dollar_tokens("$$YYYY", Some(&d), None), "$$YYYY");
        assert_eq!(expand_dollar_tokens("$$$YYYY", Some(&d), None), "$$2024");
        assert_eq!(expand_dollar_tokens("$1_$YYYY", Some(&d), None), "$1_2024");
        // No time given → tokens stay.
        assert_eq!(expand_dollar_tokens("$YYYY", None, None), "$YYYY");
    }

    #[test]
    fn file_time_detection() {
        assert!(uses_file_time("photo_$YYYY"));
        assert!(uses_file_time("$hh"));
        assert!(!uses_file_time("$$YYYY"));
        assert!(!uses_file_time("$1"));
        assert!(!uses_file_time("$DATE_TAKEN_YYYY"));
        assert!(!uses_file_time("$HEIGHT"));
        assert!(uses_metadata("$CAMERA_MAKE"));
        assert!(!uses_metadata("$$CAMERA_MAKE"));
    }

    #[test]
    fn metadata_tokens() {
        let mut m = HashMap::new();
        m.insert("CAMERA_MAKE".to_string(), "Canon".to_string());
        date_taken_patterns("2023:12:31 23:59:58", &mut m);
        let d = sample_date();
        assert_eq!(
            expand_dollar_tokens(
                "$CAMERA_MAKE_$DATE_TAKEN_YYYY$DATE_TAKEN_MM$DATE_TAKEN_DD",
                Some(&d),
                Some(&m)
            ),
            "Canon_20231231"
        );
        // Missing value keeps the token name; date tokens still work alongside.
        assert_eq!(
            expand_dollar_tokens("$LENS-$YYYY", Some(&d), Some(&m)),
            "$LENS-2024"
        );
        // $HEIGHT / $DATE_TAKEN_* are not mistaken for $H / $D.
        assert_eq!(
            expand_dollar_tokens("$HEIGHT $DATE_TAKEN_SS", Some(&d), None),
            "$HEIGHT $DATE_TAKEN_SS"
        );
    }

    #[test]
    fn enumeration_groups() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(1);
        let t = "img_${start=10;increment=5;padding=4}";
        assert_eq!(expand_groups(t, true, false, 0, &mut rng), "img_0010");
        assert_eq!(expand_groups(t, true, false, 2, &mut rng), "img_0020");
        assert_eq!(
            expand_groups("${}-${start=1}", true, false, 3, &mut rng),
            "3-4"
        );
        assert_eq!(expand_groups("${start=-2}", true, false, 1, &mut rng), "-1");
        // Disabled → left alone.
        assert_eq!(expand_groups("${}", false, false, 0, &mut rng), "${}");
    }

    #[test]
    fn randomizer_groups() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(42);
        let s = expand_groups("${rstringalnum=8}", false, true, 0, &mut rng);
        assert_eq!(s.len(), 8);
        assert!(s.chars().all(|c| c.is_ascii_alphanumeric()));
        let s = expand_groups("${rstringalpha=5}", false, true, 0, &mut rng);
        assert_eq!(s.len(), 5);
        assert!(s.chars().all(|c| c.is_ascii_alphabetic()));
        let s = expand_groups("x${rstringdigit=6}y", false, true, 0, &mut rng);
        assert_eq!(s.len(), 8);
        assert!(s[1..7].chars().all(|c| c.is_ascii_digit()));
        let u = expand_groups("${ruuidv4}", false, true, 0, &mut rng);
        assert_eq!(u.len(), 36);
        assert_eq!(&u[14..15], "4");
        assert!(matches!(&u[19..20], "8" | "9" | "a" | "b"));
        assert_eq!(u.matches('-').count(), 4);
        // Negative alpha length → empty (upstream loop doesn't run).
        assert_eq!(
            expand_groups("${rstringalpha=-3}", false, true, 0, &mut rng),
            ""
        );
        // Randomizer disabled → not touched (and not an enumerator either).
        assert_eq!(
            expand_groups("${ruuidv4}", false, false, 0, &mut rng),
            "${ruuidv4}"
        );
    }

    #[test]
    fn randomizer_wins_over_enumerator() {
        let mut rng = rand::rngs::StdRng::seed_from_u64(7);
        let s = expand_groups("${}_${rstringdigit=3}", true, true, 5, &mut rng);
        assert!(s.starts_with("5_"));
        assert_eq!(s.len(), 5);
        assert!(has_groups("${}", true, false));
        assert!(!has_groups("${}", false, true));
        assert!(has_groups("a${ruuidv4}", false, true));
    }

    #[test]
    fn group_refs_normalized() {
        assert_eq!(normalize_group_refs("$1_foo"), "${1}_foo");
        assert_eq!(normalize_group_refs("$12x$3"), "${12}x${3}");
        assert_eq!(normalize_group_refs("$$1"), "$$1");
        assert_eq!(normalize_group_refs("${1}"), "${1}");
    }

    #[test]
    fn formatting_helpers() {
        assert_eq!(format_shutter(0.004), "1/250s");
        assert_eq!(format_shutter(2.0), "2.0s");
        assert_eq!(format_coordinate(-33.5, true), "33\u{00B0}30.00'S");
    }
}
