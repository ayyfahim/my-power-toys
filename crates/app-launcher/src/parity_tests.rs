//! Parity tests against Microsoft PowerToys Run plugins.
//!
//! Oracles (PowerToys repo):
//! - Calculator: src/modules/launcher/Plugins/Microsoft.PowerToys.Run.Plugin.Calculator.UnitTest
//!   (ExtendedCalculatorParserTests.cs, QueryTests.cs, NumberTranslatorTests.cs)
//! - UnitConverter: Community.PowerToys.Run.Plugin.UnitConverter.UnitTest
//! - ValueGenerator: Community.PowerToys.Run.Plugin.ValueGenerator.UnitTests + InputParser.cs
//! - TimeDate: Microsoft.PowerToys.Run.Plugin.TimeDate.UnitTests
//! - Fuzzy matching: src/modules/launcher/Wox.Test/FuzzyMatcherTest.cs (Wox.Infrastructure/StringMatcher.cs)
//!
//! A failing test here is a parity bug finding; tests are not "fixed" by
//! changing the expectations.

// Expected values are copied verbatim from the PowerToys unit tests.
#![allow(clippy::approx_constant)]

use crate::calculator::evaluate;
use crate::config::AppLauncherConfig;
use crate::desktop::DesktopEntry;
use crate::plugins::{self, QueryCtx};

fn ctx<'a>(search: &'a str, by_keyword: bool, config: &'a AppLauncherConfig) -> QueryCtx<'a> {
    QueryCtx {
        search,
        by_keyword,
        config,
    }
}

fn approx(input: &str, expected: f64) {
    let got = evaluate(input);
    match got {
        Some(v) => assert!(
            (v - expected).abs() < 1e-9 * expected.abs().max(1.0),
            "{input:?}: expected {expected}, got {v}"
        ),
        None => panic!("{input:?}: expected {expected}, got None (not evaluated)"),
    }
}

/// Collect all failures in a table test so one run lists every mismatch.
fn table(cases: &[(&str, f64)]) {
    let mut failures = Vec::new();
    for (input, expected) in cases {
        let r = std::panic::catch_unwind(|| approx(input, *expected));
        if let Err(e) = r {
            let msg = e
                .downcast_ref::<String>()
                .cloned()
                .unwrap_or_else(|| format!("{input:?} panicked"));
            failures.push(msg);
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from PowerToys:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

// ---------------------------------------------------------------- Calculator

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_basic_precedence_and_decimals() {
    // ExtendedCalculatorParserTests.Interpret_NoErrors_WhenCalledWithRounding_Data (subset without functions)
    table(&[
        ("2 * 2", 4.0),
        ("5.6/2", 2.8),
        ("123 * 4.56", 560.88),
        ("1 - 9.0 / 10", 0.1),
        ("0.5 * ((2*-395.2)+198.2)", -296.1),
        ("2+2.11", 4.11),
        ("8.43 + 4.43 - 12.8", 0.06),
        ("1-1", 0.0),
        ("0+(1*2)/(0+1)", 2.0),
        ("0+(1*2)/0.5", 4.0),
        ("4.5/3", 1.5),
    ]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_power_operator() {
    // Mages: unary minus binds tighter than ^ ("-2 ^ 2" = 4).
    table(&[("-2 ^ 2", 4.0), ("-(2 ^ 2)", -4.0), ("2^10", 1024.0)]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_functions() {
    table(&[
        ("round(2 * pi)", 6.0),
        ("pi * ( sin ( cos ( 2)))", -1.26995475603563),
        ("exp(5)", 148.413159102577),
        ("ln(3)", 1.0986122886681),
        ("log(3)", 0.47712125471966),
        ("log2(3)", 1.58496250072116),
        ("log10(3)", 0.47712125471966),
        ("ln (3)", 1.0986122886681),
        ("cosh(0)", 1.0),
        ("sin(0)", 0.0),
        ("factorial(5)", 120.0),
        ("sign(-2)", -1.0),
        ("abs(-2)", 2.0),
        ("sqrt( 36)", 6.0),
        ("ceil(2 * (pi ^ 2))", 20.0),
    ]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_constants() {
    table(&[
        ("2 * pi", 6.28318530717959),
        ("e^5", 148.413159102577),
        ("e*2", 5.43656365691809),
        ("ln(e)", 1.0),
    ]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_hex_octal_binary_literals() {
    // QueryTests.RightAnswerForLargeHexadecimalNumbers + ExtendedCalculatorParserTests
    table(&[
        ("0x1234+0x1234", 9320.0),
        ("0x1234-0x1234", 0.0),
        ("0x12345678+0x12345678", 610839792.0),
        ("0xf4572220-0xf4572410", -496.0),
        ("0+(1*2)/0o004", 0.5),
        ("0X78AD+0o123", 30893.0 + 83.0),
        ("0b101+1", 6.0),
    ]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_hex_is_not_silently_misread_as_zero() {
    // Even without hex support the evaluator must not return a wrong number.
    let r = evaluate("0x1234+0x1234");
    assert!(
        r.is_none() || r == Some(9320.0),
        "0x1234+0x1234 evaluated to {r:?} (PowerToys: 9320)"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_implicit_multiplication() {
    // QueryTests.RightAnswerForHumanMultiplicationExpressions
    table(&[
        ("2(1+1)", 4.0),
        ("pi(1+1)", 6.2831853072),
        ("pilog(100)", 6.2831853072),
        ("3log(100)", 6.0),
        ("2e", 5.4365636569),
        ("(1+1)(3+2)", 10.0),
        ("(1+1)cos(pi)", -2.0),
        ("log(100)cos(pi)", -2.0),
    ]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_implicit_multiplication_not_truncated() {
    // "2(3+4)" is 14 in PowerToys; returning 2 (dropping the rest) is wrong.
    let r = evaluate("2(3+4)");
    assert!(r.is_none() || r == Some(14.0), "2(3+4) -> {r:?}");
    let r = evaluate("(1+1)(3+2)");
    assert!(r.is_none() || r == Some(10.0), "(1+1)(3+2) -> {r:?}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_scientific_notation() {
    table(&[
        ("2E2", 200.0),
        ("2E-2", 0.02),
        ("1.2E2", 120.0),
        ("1.2E-1", 0.12),
        ("5/5E3", 0.001),
        ("-5/5E3", -0.001),
        ("1+1.1e3", 1101.0),
    ]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_rejects_invalid_or_unbalanced_input() {
    // ExtendedCalculatorParserTests.InputValid_TestValid_WhenCalled (false rows)
    let mut wrong = Vec::new();
    for input in [
        "((1 * 2)",
        "(1 * 2)))",
        "1 + 2 +",
        "1+2*",
        "1+2/",
        "1+2%",
        "max 4",
        "9+",
        "9-",
        "9*",
        "9/",
        "5 + 3 apples",
        "2 + 3 )",
    ] {
        if let Some(v) = evaluate(input) {
            wrong.push(format!("{input:?} -> {v}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "accepted invalid input (PowerToys rejects): {wrong:?}"
    );
}

#[test]
fn calc_division_by_zero_global_gives_no_result() {
    // QueryTests.NoResultOnInvalidGlobalQuery
    let cfg = AppLauncherConfig::default();
    for q in ["5/0", "5 / 0", "10+(8*9)/0+7", "10+(8*9)/0*7"] {
        assert!(
            plugins::calculator::search(&ctx(q, false, &cfg)).is_empty(),
            "{q}"
        );
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_division_by_zero_keyword_shows_error() {
    // QueryTests.ErrorResultOnInvalidKeywordQuery: "=5/0" -> "Expression contains division by zero"
    let cfg = AppLauncherConfig::default();
    for q in [
        "5/0",
        "5 / 0",
        "10+(8*9)/0+7",
        "10+(8*9)/0x00",
        "10+(8*9)/0b0",
    ] {
        let r = plugins::calculator::search(&ctx(q, true, &cfg));
        assert!(
            r.iter()
                .any(|r| r.title.to_lowercase().contains("division by zero")),
            "={q}: expected a 'division by zero' error row, got {:?}",
            r.iter().map(|r| &r.title).collect::<Vec<_>>()
        );
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_keyword_invalid_expression_shows_error() {
    // QueryTests.ErrorResultOnInvalidKeywordQuery: "=pi(9+)" etc. -> error row.
    let cfg = AppLauncherConfig::default();
    for q in ["pi(9+)", "log()", "1+(2"] {
        let r = plugins::calculator::search(&ctx(q, true, &cfg));
        assert!(
            !r.is_empty(),
            "={q}: PowerToys shows 'Expression wrong or incomplete', got no row"
        );
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn calc_decimal_separator_comma() {
    // QueryTests.NoErrorForDivisionByNumberWithDecimalDigits: "10+(8*9)/1,5"
    // is valid. With ',' as decimal separator (de-DE) it is 58, with ','
    // as group separator (en-US, NumberTranslator removes it) it is 14.8.
    let r = evaluate("10+(8*9)/1,5");
    assert!(
        matches!(r, Some(v) if (v - 58.0).abs() < 1e-9 || (v - 14.8).abs() < 1e-9),
        "10+(8*9)/1,5 -> {r:?} (PowerToys: 58 in de-DE, 14.8 in en-US)"
    );
}

#[test]
fn calc_result_rounding_to_10_decimals() {
    // CalculateEngine.Round: 10 decimals, trailing zeros removed.
    use crate::plugins::calculator::format_number;
    assert_eq!(format_number(1.0 / 3.0), "0.3333333333");
    assert_eq!(format_number(2.0 * std::f64::consts::PI), "6.2831853072");
    assert_eq!(format_number(0.1 + 0.2), "0.3");
}

// ------------------------------------------------------------ Unit converter

use crate::plugins::unit_converter as uc;

fn conv(q: &str) -> Option<f64> {
    let r = uc::parse(q)?;
    uc::convert(r.value, r.from, r.to?)
}

#[test]
fn units_basic_from_unit_handler_tests() {
    assert!((conv("1 c in f").unwrap() - 33.8).abs() < 1e-9);
    assert_eq!(conv("1 meter in centimeter"), Some(100.0));
    assert_eq!(conv("1 meters in centimeters"), Some(100.0));
    let v = conv("1 nanometer in kilometer").unwrap();
    assert!((v - 1e-12).abs() < 1e-24, "{v}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn units_kb_is_kilobit_kb_is_kilobyte() {
    // UnitHandlerTests.HandlesByteCapitals: 1 kB in kb = 8 (UnitsNet: kb = kilobit).
    assert_eq!(conv("1 kB in kb"), Some(8.0));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn units_tiny_result_not_displayed_as_zero() {
    // PowerToys shows "1E-12 kilometer"; the result title must not be "0 km".
    let cfg = AppLauncherConfig::default();
    let r = uc::search(&ctx("1 nm in km", true, &cfg));
    assert_eq!(r.len(), 1);
    assert_ne!(r[0].title, "0 km", "1 nm in km displayed as zero");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn units_shorthand_feet_inches() {
    // InputInterpreterTests.HandlesShorthandFeetInchNotation
    let v = conv("1' in cm");
    assert!(matches!(v, Some(x) if (x - 30.48).abs() < 1e-9), "{v:?}");
    let v = conv("1\" in cm");
    assert!(matches!(v, Some(x) if (x - 2.54).abs() < 1e-9), "{v:?}");
    let v = conv("1'6\" in cm");
    assert!(
        matches!(v, Some(x) if (x - 45.72).abs() < 1e-9),
        "1'6\" -> {v:?}"
    );
    let v = conv("1'6 in cm");
    assert!(
        matches!(v, Some(x) if (x - 45.72).abs() < 1e-9),
        "1'6 -> {v:?}"
    );
    let v = conv("-1'6\" in cm");
    assert!(
        matches!(v, Some(x) if (x + 45.72).abs() < 1e-9),
        "-1'6\" -> {v:?}"
    );
}

#[test]
fn units_case_insensitive_degree_names_and_metre() {
    assert!((conv("5 CeLsIuS in faHrenheiT").unwrap() - 41.0).abs() < 1e-9);
    assert_eq!(conv("1 centimetre in kilometre"), Some(1e-5));
}

#[test]
fn units_invalid_queries() {
    // InputInterpreterTests.ParseInvalidQueries
    for q in ["a f in c", "12 f in", "1-2 f in c", "12- f in c"] {
        assert!(uc::parse(q).is_none(), "{q} should be rejected");
    }
    // ParseValidQueries
    assert_eq!(uc::parse("12 f in c").unwrap().value, 12.0);
    assert_eq!(uc::parse("10m to cm").unwrap().value, 10.0);
    assert_eq!(uc::parse("-12 f in c").unwrap().value, -12.0);
    assert_eq!(uc::parse("-10m to cm").unwrap().value, -10.0);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn units_other_unitsnet_categories() {
    // PowerToys supports every UnitsNet quantity (area, speed, energy, pressure, time, ...).
    let mut missing = Vec::new();
    for q in [
        "7 sqft in sqcm",
        "100 km/h in m/s",
        "1 kWh in J",
        "1 bar in psi",
        "1 h in min",
        "1 parsec in nanometer",
    ] {
        if conv(q).is_none() {
            missing.push(q);
        }
    }
    assert!(missing.is_empty(), "not convertible: {missing:?}");
}

// ----------------------------------------------------------- Value generator

use crate::plugins::value_generator::generate;

fn gen_ok(q: &str) -> String {
    generate(q).unwrap_or_else(|e| panic!("{q}: {e}")).value
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn vg_hashes_exact_uppercase_hex() {
    // HashRequest.ResultToString uses "X2" (upper-case hex).
    assert_eq!(gen_ok("md5 abc"), "900150983CD24FB0D6963F7D28E17F72");
    assert_eq!(
        gen_ok("sha1 abc"),
        "A9993E364706816ABA3E25717850C26C9CD0D89D"
    );
    assert_eq!(
        gen_ok("sha256 abc"),
        "BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD"
    );
}

#[test]
fn vg_hash_values_case_insensitive() {
    assert_eq!(
        gen_ok("md5 abc").to_lowercase(),
        "900150983cd24fb0d6963f7d28e17f72"
    );
    assert_eq!(
        gen_ok("sha384 abc").to_lowercase(),
        "cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7"
    );
    assert!(
        gen_ok("sha512 abc")
            .to_lowercase()
            .starts_with("ddaf35a193617aba")
    );
    assert!(generate("sha1111 abc").is_err());
    assert!(generate("md5").is_err());
}

fn guid_version(s: &str) -> char {
    s.chars().nth(14).unwrap()
}

#[test]
fn vg_guid_v4_v7() {
    assert_eq!(guid_version(&gen_ok("uuid")), '4');
    assert_eq!(guid_version(&gen_ok("guidv4")), '4');
    assert_eq!(guid_version(&gen_ok("guidv7")), '7');
    assert_eq!(guid_version(&gen_ok("GUIDv7")), '7');
    // GUIDv7GeneratorTimeOrdered
    let mut prev = gen_ok("uuidv7");
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(2));
        let next = gen_ok("uuidv7");
        assert!(next > prev, "uuidv7 not time ordered: {prev} !< {next}");
        prev = next;
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn vg_guid_v1() {
    assert_eq!(guid_version(&gen_ok("guidv1")), '1');
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn vg_guid_v3_v5_exact() {
    // GUIDGeneratorTests.GUIDv3Andv5
    let cases = [
        ("guidv3 ns:DNS abc", "5bd670ce-29c8-3369-a8a1-10ce44c7259e"),
        ("guidv3 ns:URL abc", "874a8cb4-4e91-3055-a476-3d3e2ffe375f"),
        ("guidv3 ns:OID abc", "5557cd36-6b67-38ac-83fe-825f5905fc15"),
        ("guidv3 ns:X500 abc", "589392cb-93e1-392c-a846-367c45ed1ecc"),
        (
            "guidv3 589392cb-93e1-392c-a846-367c45ed1ecc abc",
            "f55f77d2-feed-378e-aa47-2f716b07aaad",
        ),
        ("guidv5 ns:DNS abc", "6cb8e707-0fc5-5f55-88d4-d4fed43e64a8"),
        ("uUiD5 ns:URL abc", "68661508-f3c4-55b4-945d-ae2b4dfe5db4"),
        ("guidv5 ns:OID abc", "7697a46f-b283-5da3-8e7c-62c11c03dd9e"),
        ("guidv5 ns:X500 abc", "53e882a6-63b1-578b-8bf1-8f0878cfa6b7"),
        (
            "guidv5 589392cb-93e1-392c-a846-367c45ed1ecc abc",
            "396466f5-96f4-57e2-aea1-78e1bb1bacc6",
        ),
    ];
    let mut bad = Vec::new();
    for (q, want) in cases {
        match generate(q) {
            Ok(g) if g.value == want => {}
            Ok(g) => bad.push(format!("{q} -> {} (want {want})", g.value)),
            Err(e) => bad.push(format!("{q} -> error {e}")),
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn vg_guid_invalid() {
    // ParserTest: guidv2 / Guidvv are errors; v3 with bad namespace is an error.
    assert!(generate("guidv2 ns:DNS abc").is_err());
    assert!(generate("Guidvv ns:DNS abc").is_err());
    assert!(generate("guidv3 abc abc").is_err());
}

#[test]
fn vg_base64() {
    assert_eq!(gen_ok("base64 abc"), "YWJj");
    assert_eq!(gen_ok("base64d YWJj"), "abc");
    assert!(generate("base99 abc").is_err());
    assert!(generate("base64s abc").is_err());
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn vg_url_encode_matches_httputility() {
    // UrlEncodeRequest uses HttpUtility.UrlEncode: space -> '+', lower-case hex.
    assert_eq!(
        gen_ok("url http://googl.de/u oii d"),
        "http%3a%2f%2fgoogl.de%2fu+oii+d"
    );
    assert_eq!(gen_ok("url a b&c=d/é"), "a+b%26c%3dd%2f%c3%a9");
}

#[test]
fn vg_url_decode() {
    assert_eq!(
        gen_ok("urld http://googl.de/u oii d=oo"),
        "http://googl.de/u oii d=oo"
    );
    assert_eq!(gen_ok("urld a+b%26c"), "a b&c");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn vg_data_and_hex_escaping() {
    // esc:data -> Uri.EscapeDataString, uesc:data -> Uri.UnescapeDataString,
    // esc:hex -> Uri.HexEscape (single char), uesc:hex -> Uri.HexUnescape.
    let mut bad = Vec::new();
    for (q, want) in [
        ("esc:data a b&c=d", "a%20b%26c%3Dd"),
        ("uesc:data a%20b%26c", "a b&c"),
        ("esc:hex d", "%64"),
        ("esc:hex 4", "%34"),
        ("uesc:hex %21", "!"),
    ] {
        match generate(q) {
            Ok(g) if g.value == want => {}
            Ok(g) => bad.push(format!("{q} -> {} (want {want})", g.value)),
            Err(e) => bad.push(format!("{q} -> error {e}")),
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
    // esc:hex with 0 or >1 characters throws.
    assert!(generate("esc:hex ").is_err());
    assert!(generate("esc:hex z44").is_err());
}

// ---------------------------------------------------------------- Time/date

use crate::plugins::time_date;
use chrono::{FixedOffset, TimeZone};

fn utc() -> FixedOffset {
    FixedOffset::east_opt(0).unwrap()
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn td_unix_prefixes_with_sign() {
    // StringParserTests: u10456, u-10456, u+10456, ums10456, ums-10456, ums+10456 are valid.
    let tz = utc();
    let mut bad = Vec::new();
    for q in [
        "u10456",
        "u-10456",
        "u+10456",
        "ums10456",
        "ums-10456",
        "ums+10456",
    ] {
        if time_date::parse_time(q, &tz).is_none() {
            bad.push(q);
        }
    }
    assert!(bad.is_empty(), "not parsed: {bad:?}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn td_plain_number_is_not_a_timestamp() {
    // StringParserTests: "10456" -> false (a prefix u/ums/ft/oa/exc/exf is required).
    assert!(time_date::parse_time("10456", &utc()).is_none());
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn td_windows_file_time_ole_and_excel_dates() {
    let tz = utc();
    let mut bad = Vec::new();
    for (q, want) in [
        ("exc61", Some("1900-03-01")),
        ("exc1.99998843", Some("1900-01-01")),
        ("exf0.99998843", Some("1904-01-01")),
        ("oa2958465.99999999", Some("9999-12-31")),
        ("exc60", None),
        ("exc0", None),
    ] {
        let got = time_date::parse_time(q, &tz).map(|d| d.format("%Y-%m-%d").to_string());
        if got.as_deref() != want {
            bad.push(format!("{q}: got {got:?}, want {want:?}"));
        }
    }
    assert!(
        time_date::parse_time("ft10456", &tz).is_some(),
        "ft (Windows file time) prefix not supported; also: {bad:?}"
    );
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn td_global_results_only_date_time_now() {
    // QueryTests: setting "Only Date, Time, Now on global results" is on by default:
    // "year" -> 0 results, "time" -> 2, "date" -> 2, "now" -> 3.
    let now = FixedOffset::east_opt(3600)
        .unwrap()
        .with_ymd_and_hms(2024, 1, 15, 13, 30, 5)
        .unwrap();
    let n = |q: &str| time_date::select(q, false, now).len();
    let got = [
        ("year", n("year")),
        ("time", n("time")),
        ("date", n("date")),
        ("now", n("now")),
    ];
    assert_eq!(
        got,
        [("year", 0), ("time", 2), ("date", 2), ("now", 3)],
        "global result counts differ"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn td_week_of_month_present() {
    // TimeDateResultTests: "(week of month" -> "Week of the month".
    let now = FixedOffset::east_opt(0)
        .unwrap()
        .with_ymd_and_hms(2025, 3, 3, 0, 0, 0)
        .unwrap();
    let e = time_date::entries(now);
    assert!(
        e.iter().any(|e| e.label.to_lowercase().contains("week of")
            && e.label.to_lowercase().contains("month")),
        "no 'Week of the month' entry: {:?}",
        e.iter().map(|e| e.label).collect::<Vec<_>>()
    );
}

// ------------------------------------------------------------ Fuzzy matching

fn app(name: &str) -> DesktopEntry {
    DesktopEntry {
        name: name.into(),
        generic_name: None,
        comment: None,
        exec: "true".into(),
        icon: None,
        categories: vec![],
        keywords: vec![],
        terminal: false,
        no_display: false,
        path: "/x.desktop".into(),
    }
}

fn program_titles(query: &str, names: &[&str]) -> Vec<String> {
    let cfg = AppLauncherConfig::default();
    let apps: Vec<_> = names.iter().map(|n| app(n)).collect();
    plugins::program::search(&ctx(query, false, &cfg), &apps)
        .into_iter()
        .map(|r| r.title)
        .collect()
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn fuzzy_match_test_inst_order() {
    // FuzzyMatcherTest.MatchTest
    let got = program_titles(
        "inst",
        &[
            "file open in browser-test",
            "Install Package",
            "add new bsd",
            "Inste",
            "aac",
        ],
    );
    assert_eq!(
        got,
        vec!["Inste", "Install Package", "file open in browser-test"]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn fuzzy_regular_precision_table() {
    // FuzzyMatcherTest.WhenGivenDesiredPrecision... / WhenGivenQueryShouldReturnResultsContainingAllQuerySubstrings
    // at the default precision (Regular).
    const MS: &str = "Microsoft SQL Server Management Studio";
    let cases: &[(&str, &str, bool)] = &[
        ("goo", "Google Chrome", true),
        ("chr", "Chrome", true),
        ("chr", "Help cure hope raise on mind entity Chrome", true),
        ("chr", "Candy Crush Saga from King", false),
        ("cand", "Candy Crush Saga from King", true),
        ("cand", "Help cure hope raise on mind entity Chrome", false),
        ("exce", "OverLeaf-Latex: An online LaTeX editor", false),
        ("term", "Windows Terminal (Preview)", true),
        ("sql s managa", MS, false),
        ("sql' s manag", MS, false),
        ("sql s manag", MS, true),
        ("sql manag", MS, true),
        ("sql", MS, true),
        ("sql serv", MS, true),
        ("sql serv man", MS, true),
        ("sql studio", MS, true),
        ("mic", MS, true),
        ("chr", "Shutdown", false),
        ("mssms", MS, false),
        (
            "chr",
            "Change settings for text-to-speech and for speech recognition (if installed).",
            true,
        ),
        (
            "ch r",
            "Change settings for text-to-speech and for speech recognition (if installed).",
            true,
        ),
        ("a test", "This is a test", true),
        ("test", "This is a test", true),
    ];
    let mut bad = Vec::new();
    for (q, s, want) in cases {
        let got = !program_titles(q, &[s]).is_empty();
        if got != *want {
            bad.push(format!("{q:?} vs {s:?}: matched={got}, PowerToys={want}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} mismatches:\n{}",
        bad.len(),
        bad.join("\n")
    );
}

#[test]
fn fuzzy_exact_match_ranks_first() {
    // WhenMultipleResultsExactMatchingResultShouldHaveGreatestScore
    assert_eq!(program_titles("vim", &["Vim Diff", "Vim"])[0], "Vim");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn fuzzy_space_preceded_match_scores_higher() {
    // FuzzyMatchingScoreShouldBeHigherWhenPreceedingCharacterIsSpace
    let s = |h: &str| plugins::text_score(h, "term").unwrap_or(0.0);
    assert!(
        s("Windows Terminal") > s("Windows_Terminal"),
        "{} vs {}",
        s("Windows Terminal"),
        s("Windows_Terminal")
    );
    assert!(s("Windows Terminal") > s("WindowsTerminal"));
}

#[test]
fn fuzzy_not_all_chars_found_is_zero() {
    // WhenGivenNotAllCharactersFoundInSearchStringThenShouldReturnZeroScore
    assert!(program_titles("Chrome", &["Can have rum only in my glass"]).is_empty());
}

// --------------------------------------------------------------- Routing

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn route_defaults_match_plugin_json() {
    // ActionKeyword / IsGlobal from each upstream plugin.json.
    use crate::config::PluginId::*;
    let cfg = AppLauncherConfig::default();
    let expect = [
        (Program, ".", true),
        (Calculator, "=", true),
        (Shell, ">", false),
        (Folder, "", true),
        (Uri, "//", true),
        (WebSearch, "??", true),
        (WindowWalker, "<", true),
        (System, "", true),
        (TimeDate, ")", true),
        (UnitConverter, "%%", false),
        (ValueGenerator, "#", false),
        (History, "!!", false),
        (Indexer, "?", true),
    ];
    let mut bad = Vec::new();
    for (id, kw, global) in expect {
        let p = cfg.plugin(id);
        if p.action_keyword != kw || p.include_in_global_result != global {
            bad.push(format!(
                "{id:?}: kw={:?} global={} (PowerToys kw={kw:?} global={global})",
                p.action_keyword, p.include_in_global_result
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
