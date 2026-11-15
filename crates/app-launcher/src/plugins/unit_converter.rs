//! Unit converter plugin (`%%`): length, mass, temperature, volume and data.
//!
//! Syntax (as upstream): `10 ft in m`, `10ft to cm`, `5 kg lb`, `100 f in c`.
//! Without a target unit, conversions to the common units of the category are listed.

use super::{LauncherResult, QueryCtx};
use crate::actions::Action;
use crate::config::PluginId;
use crate::plugins::calculator::format_number;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Length,
    Mass,
    Temperature,
    Volume,
    Data,
}

#[derive(Debug, Clone, Copy)]
pub struct Unit {
    pub symbol: &'static str,
    pub aliases: &'static [&'static str],
    pub category: Category,
    /// Factor to the base unit (m, kg, K offset handled separately, L, byte).
    pub factor: f64,
    /// Listed when no target unit is given.
    pub common: bool,
}

const fn u(
    symbol: &'static str,
    aliases: &'static [&'static str],
    category: Category,
    factor: f64,
    common: bool,
) -> Unit {
    Unit {
        symbol,
        aliases,
        category,
        factor,
        common,
    }
}

use Category::*;

pub const UNITS: &[Unit] = &[
    // Length (base: metre)
    u(
        "nm",
        &["nanometer", "nanometre", "nanometers"],
        Length,
        1e-9,
        false,
    ),
    u(
        "µm",
        &["um", "micrometer", "micron", "microns"],
        Length,
        1e-6,
        false,
    ),
    u(
        "mm",
        &["millimeter", "millimetre", "millimeters"],
        Length,
        1e-3,
        true,
    ),
    u(
        "cm",
        &["centimeter", "centimetre", "centimeters"],
        Length,
        1e-2,
        true,
    ),
    u(
        "m",
        &["meter", "metre", "meters", "metres"],
        Length,
        1.0,
        true,
    ),
    u(
        "km",
        &["kilometer", "kilometre", "kilometers"],
        Length,
        1e3,
        true,
    ),
    u("in", &["inch", "inches", "\""], Length, 0.0254, true),
    u("ft", &["foot", "feet", "'"], Length, 0.3048, true),
    u("yd", &["yard", "yards"], Length, 0.9144, false),
    u("mi", &["mile", "miles"], Length, 1609.344, true),
    u(
        "nmi",
        &["nauticalmile", "nautical_mile"],
        Length,
        1852.0,
        false,
    ),
    // Mass (base: kilogram)
    u("mg", &["milligram", "milligrams"], Mass, 1e-6, false),
    u("g", &["gram", "grams", "gr"], Mass, 1e-3, true),
    u(
        "kg",
        &["kilogram", "kilograms", "kilo", "kilos"],
        Mass,
        1.0,
        true,
    ),
    u(
        "t",
        &["tonne", "tonnes", "ton", "tons"],
        Mass,
        1000.0,
        false,
    ),
    u("oz", &["ounce", "ounces"], Mass, 0.028349523125, true),
    u("lb", &["lbs", "pound", "pounds"], Mass, 0.45359237, true),
    u("st", &["stone", "stones"], Mass, 6.35029318, false),
    // Temperature (handled specially)
    u(
        "°C",
        &["c", "celsius", "degc", "°c"],
        Temperature,
        1.0,
        true,
    ),
    u(
        "°F",
        &["f", "fahrenheit", "degf", "°f"],
        Temperature,
        1.0,
        true,
    ),
    u("K", &["k", "kelvin"], Temperature, 1.0, true),
    // Volume (base: litre)
    u(
        "ml",
        &["milliliter", "millilitre", "milliliters"],
        Volume,
        1e-3,
        true,
    ),
    u("cm³", &["cm3", "cc"], Volume, 1e-3, false),
    u(
        "l",
        &["liter", "litre", "liters", "litres"],
        Volume,
        1.0,
        true,
    ),
    u("m³", &["m3", "cubicmeter"], Volume, 1000.0, false),
    u(
        "tsp",
        &["teaspoon", "teaspoons"],
        Volume,
        0.00492892159375,
        false,
    ),
    u(
        "tbsp",
        &["tablespoon", "tablespoons"],
        Volume,
        0.01478676478125,
        false,
    ),
    u(
        "fl oz",
        &["floz", "fluidounce"],
        Volume,
        0.0295735295625,
        true,
    ),
    u("cup", &["cups"], Volume, 0.2365882365, true),
    u("pt", &["pint", "pints"], Volume, 0.473176473, false),
    u("qt", &["quart", "quarts"], Volume, 0.946352946, false),
    u("gal", &["gallon", "gallons"], Volume, 3.785411784, true),
    // Data (base: byte)
    u("bit", &["bits"], Data, 0.125, false),
    u("B", &["b", "byte", "bytes"], Data, 1.0, true),
    u("kB", &["kb", "kilobyte", "kilobytes"], Data, 1e3, true),
    u("MB", &["mb", "megabyte", "megabytes"], Data, 1e6, true),
    u("GB", &["gb", "gigabyte", "gigabytes"], Data, 1e9, true),
    u("TB", &["tb", "terabyte", "terabytes"], Data, 1e12, true),
    u("PB", &["pb", "petabyte", "petabytes"], Data, 1e15, false),
    u("KiB", &["kib", "kibibyte"], Data, 1024.0, true),
    u("MiB", &["mib", "mebibyte"], Data, 1048576.0, true),
    u("GiB", &["gib", "gibibyte"], Data, 1073741824.0, true),
    u("TiB", &["tib", "tebibyte"], Data, 1099511627776.0, false),
    u(
        "kbit",
        &["kbits", "kilobit", "kilobits"],
        Data,
        125.0,
        false,
    ),
    u(
        "Mbit",
        &["mbit", "mbits", "megabit", "megabits"],
        Data,
        125_000.0,
        false,
    ),
    u(
        "Gbit",
        &["gbit", "gbits", "gigabit", "gigabits"],
        Data,
        125_000_000.0,
        false,
    ),
];

pub fn find_unit(name: &str) -> Option<&'static Unit> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    // Exact symbol first (case-sensitive: "MB" vs "Mbit"), then aliases case-insensitively.
    UNITS.iter().find(|u| u.symbol == n).or_else(|| {
        let l = n.to_lowercase();
        UNITS
            .iter()
            .find(|u| u.symbol.to_lowercase() == l || u.aliases.contains(&l.as_str()))
    })
}

fn to_kelvin(v: f64, unit: &Unit) -> f64 {
    match unit.symbol {
        "°C" => v + 273.15,
        "°F" => (v - 32.0) * 5.0 / 9.0 + 273.15,
        _ => v,
    }
}

fn from_kelvin(k: f64, unit: &Unit) -> f64 {
    match unit.symbol {
        "°C" => k - 273.15,
        "°F" => (k - 273.15) * 9.0 / 5.0 + 32.0,
        _ => k,
    }
}

/// Convert `value` from one unit to another of the same category.
pub fn convert(value: f64, from: &Unit, to: &Unit) -> Option<f64> {
    if from.category != to.category {
        return None;
    }
    if from.category == Temperature {
        return Some(from_kelvin(to_kelvin(value, from), to));
    }
    Some(value * from.factor / to.factor)
}

/// Parsed request.
#[derive(Debug)]
pub struct Request {
    pub value: f64,
    pub from: &'static Unit,
    pub to: Option<&'static Unit>,
}

fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    // Accept a decimal comma when there is no dot.
    let s = if !s.contains('.') {
        s.replace(',', ".")
    } else {
        s.replace(',', "")
    };
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

pub fn parse(query: &str) -> Option<Request> {
    let q = query.trim();
    // Split leading number from the rest: "10ft in m" -> "10", "ft in m"
    let num_end = q
        .char_indices()
        .find(|(i, c)| {
            !(c.is_ascii_digit() || *c == '.' || *c == ',' || (*i == 0 && (*c == '-' || *c == '+')))
        })
        .map(|(i, _)| i)
        .unwrap_or(q.len());
    let value = parse_number(&q[..num_end])?;
    let rest = q[num_end..].trim();

    // Normalise "fl oz" -> "floz" so it is one token.
    let rest = rest.replace("fl oz", "floz").replace("fl. oz", "floz");
    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let (from_tok, tail) = tokens.split_first()?;
    let from = find_unit(from_tok)?;
    let target_tok = match tail {
        [] => None,
        [t] => Some(*t),
        [conn, t] if ["in", "to", "as", "=", "into"].contains(&conn.to_lowercase().as_str()) => {
            Some(*t)
        }
        _ => return None,
    };
    let to = match target_tok {
        Some(t) => Some(find_unit(t).filter(|u| u.category == from.category)?),
        None => None,
    };
    Some(Request { value, from, to })
}

pub fn search(ctx: &QueryCtx) -> Vec<LauncherResult> {
    let q = ctx.search.trim();
    if q.is_empty() {
        return vec![
            LauncherResult::new(
                PluginId::UnitConverter,
                "Convert units, e.g. 10 ft in m",
                Action::None,
            )
            .subtitle("Length, mass, temperature, volume and data"),
        ];
    }
    let Some(req) = parse(q) else {
        return Vec::new();
    };
    let targets: Vec<&Unit> = match req.to {
        Some(t) => vec![t],
        None => UNITS
            .iter()
            .filter(|u| u.category == req.from.category && u.common && u.symbol != req.from.symbol)
            .collect(),
    };
    targets
        .into_iter()
        .enumerate()
        .filter_map(|(i, to)| {
            let v = convert(req.value, req.from, to)?;
            let text = format_number(round_sig(v));
            Some(
                LauncherResult::new(
                    PluginId::UnitConverter,
                    format!("{text} {}", to.symbol),
                    Action::Copy(text.clone()),
                )
                .subtitle(format!(
                    "{} {} = {text} {} · Enter to copy",
                    format_number(req.value),
                    req.from.symbol,
                    to.symbol
                ))
                .score(300.0 - i as f64),
            )
        })
        .collect()
}

/// Round to 10 significant digits to hide floating point noise.
fn round_sig(v: f64) -> f64 {
    if v == 0.0 {
        return 0.0;
    }
    let digits = 10 - v.abs().log10().ceil() as i32;
    let p = 10f64.powi(digits.clamp(-300, 300));
    (v * p).round() / p
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(q: &str) -> f64 {
        let r = parse(q).unwrap_or_else(|| panic!("parse {q}"));
        round_sig(convert(r.value, r.from, r.to.unwrap()).unwrap())
    }

    #[test]
    fn length() {
        assert_eq!(conv("1 ft in cm"), 30.48);
        assert_eq!(conv("10ft to m"), 3.048);
        assert_eq!(conv("1 mi km"), 1.609344);
        assert_eq!(conv("12 in in ft"), 1.0);
        assert_eq!(conv("1 in cm"), 2.54);
    }

    #[test]
    fn mass() {
        assert_eq!(conv("1 kg in lb"), 2.204622622);
        assert_eq!(conv("16 oz lb"), 1.0);
    }

    #[test]
    fn temperature() {
        assert_eq!(conv("100 c in f"), 212.0);
        assert_eq!(conv("32 °F to °C"), 0.0);
        assert_eq!(conv("0 k in c"), -273.15);
        assert_eq!(conv("-40 f c"), -40.0);
    }

    #[test]
    fn volume() {
        assert_eq!(conv("1 gal in l"), 3.785411784);
        assert_eq!(conv("1000 ml to l"), 1.0);
        assert_eq!(conv("1 fl oz in ml"), 29.57352956);
    }

    #[test]
    fn data() {
        assert_eq!(conv("1 GiB in MiB"), 1024.0);
        assert_eq!(conv("1 GB in MB"), 1000.0);
        assert_eq!(conv("8 bit in B"), 1.0);
        assert_eq!(conv("1 Gbit in MB"), 125.0);
    }

    #[test]
    fn decimal_comma_and_negative() {
        assert_eq!(conv("2,5 m in cm"), 250.0);
        assert_eq!(conv("-1 km m"), -1000.0);
    }

    #[test]
    fn rejects_mismatched_or_unknown() {
        assert!(parse("1 kg in m").is_none());
        assert!(parse("1 foo in m").is_none());
        assert!(parse("kg in m").is_none());
        assert!(parse("1 m in cm extra").is_none());
    }

    #[test]
    fn no_target_lists_common_units() {
        let cfg = crate::config::AppLauncherConfig::default();
        let r = search(&QueryCtx {
            search: "1 m",
            by_keyword: true,
            config: &cfg,
        });
        assert!(r.len() >= 4);
        assert!(r.iter().any(|r| r.title == "100 cm"));
    }
}
