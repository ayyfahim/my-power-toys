//! Calculator plugin (`=`), backed by the existing expression evaluator.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::calculator;
use crate::config::PluginId;

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let expr = ctx.search.trim();
    if expr.is_empty() {
        if ctx.by_keyword {
            return vec![
                LauncherResult::new(PluginId::Calculator, "Type a math expression", Action::None)
                    .subtitle("Supports + - * / and parentheses"),
            ];
        }
        return Vec::new();
    }
    let value = calculator::evaluate(expr).or_else(|| {
        // With the keyword, a plain number is a valid (trivial) expression.
        ctx.by_keyword.then(|| expr.parse::<f64>().ok()).flatten()
    });
    let Some(value) = value else {
        return Vec::new();
    };
    if !value.is_finite() {
        return Vec::new();
    }
    let text = format_number(value);
    vec![
        LauncherResult::new(PluginId::Calculator, text.clone(), Action::Copy(text))
            .subtitle(format!("= {expr}  ·  Enter to copy"))
            .score(300.0),
    ]
}

/// Format a result without trailing zeros (max 10 decimals).
pub fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        return format!("{}", v as i64);
    }
    let s = format!("{v:.10}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers() {
        assert_eq!(format_number(5.0), "5");
        assert_eq!(format_number(2.5), "2.5");
        assert_eq!(format_number(1.0 / 3.0), "0.3333333333");
        assert_eq!(format_number(-2.0), "-2");
    }
}
