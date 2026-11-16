//! Parity tests against Microsoft PowerToys Command Palette built-in extensions.
//! Oracle: src/modules/cmdpal/Tests/Microsoft.CmdPal.Ext.Calc.UnitTests/QueryTests.cs
//! and src/modules/cmdpal/ext/Microsoft.CmdPal.Ext.Calc/Helper/CalculateEngine.cs
//! (RoundingDigits = 10).

use crate::providers::calculator::CalculatorProvider;
use crate::providers::web_search::WebSearchProvider;
use crate::providers::{Provider, QueryContext, ResultAction};

fn calc(q: &str) -> Option<String> {
    let p = CalculatorProvider::new();
    if !p.matches(q) {
        return None;
    }
    let stripped = p.strip_prefix(q);
    let r = p.search(&QueryContext {
        raw_query: q,
        stripped_query: stripped,
        max_results: 8,
    });
    r.into_iter().find_map(|r| match r.action {
        ResultAction::CopyToClipboard(s) => Some(s),
        _ => None,
    })
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn cmdpal_calc_query_tests() {
    // QueryTests.cs DataRows (default trig mode / en-US).
    let cases = [
        ("2+2", "4"),
        ("5*3", "15"),
        ("10/2", "5"),
        ("sqrt(16)", "4"),
        ("2^3", "8"),
        ("max(1,2)", "2"),
        ("min(1,2)", "1"),
        ("pow(2,3)", "8"),
        ("max(1e3,2e3)", "2000"),
        ("max(0b1010,0o12)", "10"),
        ("2^64", "18446744073709551616"),
    ];
    let mut bad = Vec::new();
    for (q, want) in cases {
        let got = calc(q);
        if got.as_deref() != Some(want) {
            bad.push(format!("{q}: got {got:?}, want {want}"));
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
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn cmdpal_calc_rounds_to_10_digits() {
    // CalculateEngine.RoundingDigits = 10.
    assert_eq!(calc("1/3").as_deref(), Some("0.3333333333"));
    assert_eq!(calc("2/7").as_deref(), Some("0.2857142857"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn cmdpal_calc_large_result_not_saturated() {
    // 99999999999 * 99999999999 = 9999999999800000000001. The result must not
    // be clamped to i64::MAX.
    let got = calc("99999999999*99999999999");
    assert_ne!(
        got.as_deref(),
        Some("9223372036854775807"),
        "saturated to i64::MAX"
    );
    let got = calc("1e10*1e10");
    assert_ne!(
        got.as_deref(),
        Some("9223372036854775807"),
        "1e10*1e10 -> {got:?}"
    );
    let got = calc("100000000000*100000000000");
    assert_ne!(
        got.as_deref(),
        Some("9223372036854775807"),
        "1e11*1e11 saturated"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn cmdpal_calc_negative_zero_and_subtraction() {
    assert_eq!(calc("1-1").as_deref(), Some("0"));
    assert_eq!(calc("10-3").as_deref(), Some("7"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn cmdpal_websearch_engines_same_as_launcher() {
    // The launcher supports brave/startpage/ecosia; the same config value in the
    // palette should not silently fall back to Google.
    let p = WebSearchProvider::new("duckduckgo".into(), None);
    let r = p.search(&QueryContext {
        raw_query: "?? rust",
        stripped_query: "rust",
        max_results: 8,
    });
    assert!(
        matches!(&r[0].action, ResultAction::OpenUrl(u) if u.starts_with("https://duckduckgo.com/"))
    );
    let p = WebSearchProvider::new("brave".into(), None);
    let r = p.search(&QueryContext {
        raw_query: "?? rust",
        stripped_query: "rust",
        max_results: 8,
    });
    assert!(
        matches!(&r[0].action, ResultAction::OpenUrl(u) if u.contains("brave")),
        "brave engine -> {:?}",
        r[0].action
    );
}
