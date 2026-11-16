//! Text → JSON conversion, ported from upstream
//! `AdvancedPaste.Core/JsonConverter.cs`.
//!
//! Order of attempts: existing JSON (pretty-printed), XML, INI, CSV, and
//! finally an array of the non-empty lines.

/// Minimal ordered JSON tree (keeps document order, unlike serde_json's default map).
#[derive(Debug, Clone, PartialEq)]
enum J {
    Null,
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn write(&self, out: &mut String, indent: usize) {
        match self {
            J::Null => out.push_str("null"),
            J::Str(s) => out.push_str(&quote(s)),
            J::Arr(items) if items.is_empty() => out.push_str("[]"),
            J::Obj(items) if items.is_empty() => out.push_str("{}"),
            J::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent + 1);
                    item.write(out, indent + 1);
                }
                newline(out, indent);
                out.push(']');
            }
            J::Obj(items) => {
                out.push('{');
                for (i, (k, v)) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, indent + 1);
                    out.push_str(&quote(k));
                    out.push_str(": ");
                    v.write(out, indent + 1);
                }
                newline(out, indent);
                out.push('}');
            }
        }
    }

    fn pretty(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0);
        s
    }
}

fn newline(out: &mut String, indent: usize) {
    out.push('\n');
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| format!("\"{s}\""))
}

/// Convert clipboard text to JSON. Never fails; the last resort is an array of lines.
pub fn convert(text: &str) -> String {
    if let Some(json) = pretty_print_json(text) {
        return json;
    }
    if let Some(j) = try_xml(text) {
        return j.pretty();
    }
    if let Some(j) = try_ini(text) {
        return j.pretty();
    }
    if let Some(j) = try_csv(text) {
        return j.pretty();
    }
    J::Arr(
        split_lines(text)
            .into_iter()
            .map(|l| J::Str(l.into()))
            .collect(),
    )
    .pretty()
}

fn split_lines(text: &str) -> Vec<&str> {
    text.split("\r\n")
        .flat_map(|l| l.split(['\n', '\r']))
        .filter(|l| !l.is_empty())
        .collect()
}

/// Re-indent valid JSON with two spaces while preserving key order and the
/// exact literal text of numbers and strings.
pub fn pretty_print_json(text: &str) -> Option<String> {
    serde_json::from_str::<serde::de::IgnoredAny>(text).ok()?;
    let mut out = String::with_capacity(text.len() * 2);
    let mut indent = 0usize;
    let mut chars = text.trim().chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                out.push('"');
                let mut escaped = false;
                for c in chars.by_ref() {
                    out.push(c);
                    if escaped {
                        escaped = false;
                    } else if c == '\\' {
                        escaped = true;
                    } else if c == '"' {
                        break;
                    }
                }
            }
            '{' | '[' => {
                // Collapse empty containers.
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                let close = if c == '{' { '}' } else { ']' };
                if chars.peek() == Some(&close) {
                    chars.next();
                    out.push(c);
                    out.push(close);
                } else {
                    out.push(c);
                    indent += 1;
                    newline(&mut out, indent);
                }
            }
            '}' | ']' => {
                indent = indent.saturating_sub(1);
                newline(&mut out, indent);
                out.push(c);
            }
            ',' => {
                out.push(',');
                newline(&mut out, indent);
            }
            ':' => out.push_str(": "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    Some(out)
}

// ── XML (Newtonsoft `SerializeXmlNode` conventions) ─────────────────────────

fn try_xml(text: &str) -> Option<J> {
    let trimmed = text.trim();
    if !trimmed.starts_with('<') {
        return None;
    }
    let doc = roxmltree::Document::parse(trimmed).ok()?;
    let root = doc.root_element();
    Some(J::Obj(vec![(qualified_name(root), xml_element(root))]))
}

fn qualified_name(node: roxmltree::Node) -> String {
    let tag = node.tag_name();
    match tag.namespace().and_then(|ns| node.lookup_prefix(ns)) {
        Some(prefix) if !prefix.is_empty() => format!("{prefix}:{}", tag.name()),
        _ => tag.name().to_string(),
    }
}

fn xml_element(node: roxmltree::Node) -> J {
    let attrs: Vec<(String, J)> = node
        .attributes()
        .map(|a| {
            let name = match a.namespace().and_then(|ns| node.lookup_prefix(ns)) {
                Some(p) if !p.is_empty() => format!("@{p}:{}", a.name()),
                _ => format!("@{}", a.name()),
            };
            (name, J::Str(a.value().to_string()))
        })
        .collect();

    let elements: Vec<roxmltree::Node> = node.children().filter(|c| c.is_element()).collect();
    let texts: Vec<String> = node
        .children()
        .filter(|c| c.is_text())
        .filter_map(|c| c.text())
        .filter(|t| !t.trim().is_empty())
        .map(str::to_string)
        .collect();

    if attrs.is_empty() && elements.is_empty() {
        return match texts.len() {
            0 => J::Null,
            1 => J::Str(texts.into_iter().next().unwrap_or_default()),
            _ => J::Str(texts.concat()),
        };
    }

    let mut obj = attrs;
    match texts.len() {
        0 => {}
        1 => obj.push(("#text".into(), J::Str(texts[0].clone()))),
        _ => obj.push((
            "#text".into(),
            J::Arr(texts.into_iter().map(J::Str).collect()),
        )),
    }
    // Group same-named children into arrays at the position of the first occurrence.
    let mut groups: Vec<(String, Vec<J>)> = Vec::new();
    for child in elements {
        let name = qualified_name(child);
        let value = xml_element(child);
        match groups.iter_mut().find(|(n, _)| *n == name) {
            Some((_, v)) => v.push(value),
            None => groups.push((name, vec![value])),
        }
    }
    for (name, mut values) in groups {
        let v = if values.len() == 1 {
            values.remove(0)
        } else {
            J::Arr(values)
        };
        obj.push((name, v));
    }
    J::Obj(obj)
}

// ── INI ─────────────────────────────────────────────────────────────────────

fn ini_section(line: &str) -> Option<&str> {
    // ^\[(.+)\]  (anything may follow the closing bracket)
    let rest = line.strip_prefix('[')?;
    let end = rest.rfind(']')?;
    let name = &rest[..end];
    (!name.is_empty()).then_some(name)
}

fn ini_key_value(line: &str) -> Option<(&str, &str)> {
    // ^([^=]+)\s*=\s*(.*)$
    let (k, v) = line.split_once('=')?;
    if k.is_empty() {
        return None;
    }
    Some((k, v.trim_start()))
}

fn try_ini(text: &str) -> Option<J> {
    let lines: Vec<&str> = split_lines(text)
        .into_iter()
        .filter(|l| !l.starts_with(';'))
        .collect();
    if lines.len() < 2
        || ini_section(lines[0]).is_none()
        || (ini_section(lines[1]).is_none() && ini_key_value(lines[1]).is_none())
    {
        return None;
    }
    let mut sections: Vec<(String, J)> = Vec::new();
    for line in lines {
        if let Some(name) = ini_section(line) {
            let name = name.trim();
            if name.is_empty() || sections.iter().any(|(n, _)| n == name) {
                return None;
            }
            sections.push((name.to_string(), J::Obj(Vec::new())));
        } else {
            let (k, v) = ini_key_value(line)?;
            let k = k.trim();
            if k.is_empty() {
                return None;
            }
            let Some((_, J::Obj(entries))) = sections.last_mut() else {
                return None;
            };
            if entries.iter().any(|(n, _)| n == k) {
                return None;
            }
            entries.push((k.to_string(), J::Str(v.to_string())));
        }
    }
    Some(J::Obj(sections))
}

// ── CSV ─────────────────────────────────────────────────────────────────────

const CSV_DELIMITERS: [char; 3] = [',', ';', '\t'];

fn csv_separator_line(line: &str) -> Option<char> {
    // ^sep=(.)$  (case-insensitive)
    if !line.get(..4)?.eq_ignore_ascii_case("sep=") {
        return None;
    }
    let mut it = line[4..].chars();
    let c = it.next()?;
    it.next().is_none().then_some(c)
}

fn parse_csv_line(line: &str, delimiter: char) -> Option<Vec<String>> {
    let mut values = Vec::new();
    let mut value = String::new();
    let mut inside_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '"' {
            value.push(c);
            if inside_quotes && chars.peek() == Some(&'"') {
                value.push('"');
                chars.next();
            } else {
                inside_quotes = !inside_quotes;
            }
        } else if c == delimiter && !inside_quotes {
            values.push(std::mem::take(&mut value));
        } else {
            value.push(c);
        }
    }
    if inside_quotes {
        return None;
    }
    values.push(value);
    Some(values)
}

fn csv_delimiter(lines: &[&str]) -> Option<(char, usize)> {
    let mut delimiter = '\0';
    let mut count = 0usize;
    if lines.len() > 1
        && let Some(sep) = csv_separator_line(lines[0])
    {
        delimiter = sep;
        count = parse_csv_line(lines[1], sep)?.len() - 1;
    }
    if !lines.is_empty() && count == 0 {
        for candidate in CSV_DELIMITERS {
            let first = parse_csv_line(lines[0], candidate)?.len() - 1;
            let second = if lines.len() >= 2 {
                parse_csv_line(lines[1], candidate)?.len() - 1
            } else {
                0
            };
            if first > count && (second == 0 || second == first) {
                delimiter = candidate;
                count = first;
            }
        }
    }
    (count > 0).then_some((delimiter, count))
}

/// Upstream `ReplaceQuotationMarksInCsvData`: strip enclosing quotes and
/// collapse doubled quotes.
fn unquote_csv(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let n = chars.len();
    // Pass 1: ^"(?!")  |  (?<!")"$  |  ^""$
    let mut v: Vec<char> = if n == 2 && chars[0] == '"' && chars[1] == '"' {
        Vec::new()
    } else {
        let drop_first = n >= 1 && chars[0] == '"' && (n == 1 || chars[1] != '"');
        let drop_last = n >= 2 && chars[n - 1] == '"' && chars[n - 2] != '"';
        let start = usize::from(drop_first);
        let end = if drop_last { n - 1 } else { n };
        chars[start..end.max(start)].to_vec()
    };
    // Pass 2: ^"(?=("")+)  |  (?<=("")+)"$
    let n = v.len();
    let drop_first = n >= 3 && v[0] == '"' && v[1] == '"' && v[2] == '"';
    let drop_last = n >= 3 && v[n - 1] == '"' && v[n - 2] == '"' && v[n - 3] == '"';
    if drop_last {
        v.pop();
    }
    if drop_first && !v.is_empty() {
        v.remove(0);
    }
    // Pass 3: "" → "
    v.into_iter().collect::<String>().replace("\"\"", "\"")
}

fn try_csv(text: &str) -> Option<J> {
    let lines = split_lines(text);
    let (delimiter, count) = csv_delimiter(&lines)?;
    let mut rows = Vec::new();
    for line in lines {
        if csv_separator_line(line).is_some() {
            continue;
        }
        let values = parse_csv_line(line, delimiter)?;
        if values.len() - 1 != count {
            return None;
        }
        rows.push(J::Arr(
            values.iter().map(|v| J::Str(unquote_csv(v))).collect(),
        ));
    }
    Some(J::Arr(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> serde_json::Value {
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn json_is_pretty_printed_in_order() {
        let out = convert(r#"{"b":1,"a":[1,2],"e":{},"s":"x,{y}"}"#);
        assert_eq!(
            out,
            "{\n  \"b\": 1,\n  \"a\": [\n    1,\n    2\n  ],\n  \"e\": {},\n  \"s\": \"x,{y}\"\n}"
        );
    }

    #[test]
    fn csv_to_array_of_arrays() {
        let out = convert("name,age\n\"Doe, John\",42\n");
        assert_eq!(
            parse(&out),
            serde_json::json!([["name", "age"], ["Doe, John", "42"]])
        );
    }

    #[test]
    fn csv_semicolon_and_sep_line() {
        let out = convert("sep=;\na;b\n1;2");
        assert_eq!(parse(&out), serde_json::json!([["a", "b"], ["1", "2"]]));
        let out = convert("a\tb\n1\t2");
        assert_eq!(parse(&out), serde_json::json!([["a", "b"], ["1", "2"]]));
    }

    #[test]
    fn csv_quotes() {
        assert_eq!(unquote_csv("\"He said \"\"hi\"\"\""), "He said \"hi\"");
        assert_eq!(unquote_csv("\"\""), "");
        assert_eq!(unquote_csv("plain"), "plain");
    }

    #[test]
    fn csv_mismatched_columns_falls_back_to_lines() {
        let out = convert("a,b\n1,2,3");
        assert_eq!(parse(&out), serde_json::json!(["a,b", "1,2,3"]));
    }

    #[test]
    fn xml_to_json() {
        let xml = r#"<root id="1"><item>a</item><item>b</item><empty/><t x="y">txt</t></root>"#;
        assert_eq!(
            parse(&convert(xml)),
            serde_json::json!({
                "root": {
                    "@id": "1",
                    "item": ["a", "b"],
                    "empty": null,
                    "t": {"@x": "y", "#text": "txt"}
                }
            })
        );
    }

    #[test]
    fn ini_to_json() {
        let ini = "; comment\n[main]\nkey = value\nother=2\n[second]\nx = y";
        assert_eq!(
            parse(&convert(ini)),
            serde_json::json!({"main": {"key": "value", "other": "2"}, "second": {"x": "y"}})
        );
    }

    #[test]
    fn plain_text_is_line_array() {
        assert_eq!(
            parse(&convert("hello world\n\nsecond")),
            serde_json::json!(["hello world", "second"])
        );
    }
}
