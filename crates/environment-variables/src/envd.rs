//! Parsing and serialization of `environment.d(5)` / `/etc/environment` files.
//!
//! The format is `KEY=VALUE`, one assignment per line. Lines starting with
//! `#` or `;` are comments. Values may be unquoted, single-quoted or
//! double-quoted (segments may be concatenated, like systemd's env-file
//! parser). `$VAR`, `${VAR}`, `${VAR:-default}` and `${VAR:+alt}` references
//! are kept verbatim in the logical value; [`expand`] resolves them.
//!
//! [`EnvFile`] keeps every original line, so comments, blank lines and
//! unchanged assignments are written back byte-for-byte.

use std::collections::BTreeMap;

/// One line of an environment file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// Blank line, comment, or a line we could not parse. Kept verbatim.
    Other(String),
    /// A `KEY=VALUE` assignment.
    Var(VarLine),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VarLine {
    pub key: String,
    /// Logical value: quotes removed, `$` references (and `\$`) kept verbatim.
    pub value: String,
    /// Original text of the line, reused when the value is unchanged.
    pub raw: String,
}

/// An environment file that preserves its original layout.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EnvFile {
    pub lines: Vec<Line>,
}

impl EnvFile {
    pub fn parse(content: &str) -> Self {
        let lines = content.lines().map(parse_line).collect();
        Self { lines }
    }

    /// Serialize back to text (always ends with a newline when non-empty).
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            match line {
                Line::Other(s) => out.push_str(s),
                Line::Var(v) => out.push_str(&v.raw),
            }
            out.push('\n');
        }
        out
    }

    /// Effective variables in file order (later duplicates win, as in systemd).
    pub fn vars(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = Vec::new();
        for line in &self.lines {
            if let Line::Var(v) = line {
                if let Some(existing) = out.iter_mut().find(|(k, _)| *k == v.key) {
                    existing.1 = v.value.clone();
                } else {
                    out.push((v.key.clone(), v.value.clone()));
                }
            }
        }
        out
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().rev().find_map(|l| match l {
            Line::Var(v) if v.key == key => Some(v.value.as_str()),
            _ => None,
        })
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Set `key` to `value`. Updates the first assignment in place (keeping
    /// the raw text if the value is unchanged) and drops later duplicates;
    /// appends a new line if the key is absent.
    pub fn set(&mut self, key: &str, value: &str) {
        let mut found = false;
        self.lines.retain_mut(|l| match l {
            Line::Var(v) if v.key == key => {
                if found {
                    return false;
                }
                found = true;
                if v.value != value {
                    *v = VarLine::new(key, value);
                }
                true
            }
            _ => true,
        });
        if !found {
            self.lines.push(Line::Var(VarLine::new(key, value)));
        }
    }

    /// Remove all assignments of `key`. Returns true if something was removed.
    pub fn remove(&mut self, key: &str) -> bool {
        let before = self.lines.len();
        self.lines
            .retain(|l| !matches!(l, Line::Var(v) if v.key == key));
        before != self.lines.len()
    }

    /// Rename `old` to `new` and set its value, keeping the line position.
    pub fn rename(&mut self, old: &str, new: &str, value: &str) {
        if old == new {
            self.set(new, value);
            return;
        }
        // Drop any existing assignment of the new name first.
        self.remove(new);
        let pos = self
            .lines
            .iter()
            .position(|l| matches!(l, Line::Var(v) if v.key == old));
        self.remove(old);
        let line = Line::Var(VarLine::new(new, value));
        match pos {
            Some(p) if p <= self.lines.len() => self.lines.insert(p, line),
            _ => self.lines.push(line),
        }
    }
}

impl VarLine {
    pub fn new(key: &str, value: &str) -> Self {
        Self {
            key: key.to_string(),
            value: value.to_string(),
            raw: format!("{key}={}", quote_value(value)),
        }
    }
}

fn parse_line(raw: &str) -> Line {
    let trimmed = raw.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
        return Line::Other(raw.to_string());
    }
    // /etc/environment files sometimes use shell-style `export KEY=VALUE`.
    let body = trimmed
        .strip_prefix("export ")
        .map(str::trim_start)
        .unwrap_or(trimmed);
    let Some((key, value)) = body.split_once('=') else {
        return Line::Other(raw.to_string());
    };
    let key = key.trim_end();
    if !is_valid_name(key) {
        return Line::Other(raw.to_string());
    }
    match unquote(value) {
        Some(value) => Line::Var(VarLine {
            key: key.to_string(),
            value,
            raw: raw.to_string(),
        }),
        None => Line::Other(raw.to_string()),
    }
}

/// Parse the right-hand side of an assignment. Returns None on an
/// unterminated quote.
pub fn unquote(input: &str) -> Option<String> {
    let s = input.trim();
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => loop {
                match chars.next()? {
                    '\'' => break,
                    // Keep `$` literal inside single quotes.
                    '$' => out.push_str("\\$"),
                    ch => out.push(ch),
                }
            },
            '"' => loop {
                match chars.next()? {
                    '"' => break,
                    '\\' => match chars.next()? {
                        '$' => out.push_str("\\$"),
                        ch @ ('"' | '\\' | '`') => out.push(ch),
                        ch => {
                            out.push('\\');
                            out.push(ch);
                        }
                    },
                    ch => out.push(ch),
                }
            },
            '\\' => match chars.next() {
                Some('$') => out.push_str("\\$"),
                Some(ch) => out.push(ch),
                None => {}
            },
            ch => out.push(ch),
        }
    }
    Some(out)
}

fn is_safe_unquoted(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_-./:,+@%=~${}".contains(c)
}

/// Serialize a logical value. Values made only of safe characters are written
/// unquoted; anything else is double-quoted with `"`, `\` and `` ` `` escaped.
/// `\$` (a literal dollar) is written as `\$`.
pub fn quote_value(value: &str) -> String {
    if !value.is_empty() && value.chars().all(is_safe_unquoted) {
        return value.to_string();
    }
    if value.is_empty() {
        return String::new();
    }
    let mut out = String::from("\"");
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'$') => {
                chars.next();
                out.push_str("\\$");
            }
            '"' | '\\' | '`' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Variable names accepted by environment.d and POSIX shells.
pub fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Validate a name/value pair for writing. Returns an error message.
pub fn validate(name: &str, value: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Name is empty.".into());
    }
    if !is_valid_name(name) {
        return Err(format!(
            "Invalid name '{name}': use letters, digits and '_' and do not start with a digit."
        ));
    }
    if value.contains('\n') || value.contains('\r') || value.contains('\0') {
        return Err("Value cannot contain newlines or NUL characters.".into());
    }
    Ok(())
}

/// Expand `$VAR`, `${VAR}`, `${VAR:-default}` and `${VAR:+alt}` references
/// using `lookup`. Unknown variables expand to the empty string. `\$` yields a
/// literal `$`.
pub fn expand(value: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\\' && chars.get(i + 1) == Some(&'$') {
            out.push('$');
            i += 2;
            continue;
        }
        if c != '$' {
            out.push(c);
            i += 1;
            continue;
        }
        match chars.get(i + 1) {
            Some('{') => {
                // Find the matching closing brace (nesting aware).
                let mut depth = 1;
                let mut j = i + 2;
                while j < chars.len() {
                    match chars[j] {
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
                if j >= chars.len() {
                    // Unterminated: keep verbatim.
                    out.extend(&chars[i..]);
                    break;
                }
                let inner: String = chars[i + 2..j].iter().collect();
                out.push_str(&expand_braced(&inner, lookup));
                i = j + 1;
            }
            Some(&n) if n.is_ascii_alphabetic() || n == '_' => {
                let mut j = i + 1;
                while j < chars.len() && (chars[j].is_ascii_alphanumeric() || chars[j] == '_') {
                    j += 1;
                }
                let name: String = chars[i + 1..j].iter().collect();
                out.push_str(&lookup(&name).unwrap_or_default());
                i = j;
            }
            _ => {
                out.push('$');
                i += 1;
            }
        }
    }
    out
}

fn expand_braced(inner: &str, lookup: &dyn Fn(&str) -> Option<String>) -> String {
    if let Some((name, default)) = inner.split_once(":-") {
        match lookup(name).filter(|v| !v.is_empty()) {
            Some(v) => v,
            None => expand(default, lookup),
        }
    } else if let Some((name, alt)) = inner.split_once(":+") {
        match lookup(name).filter(|v| !v.is_empty()) {
            Some(_) => expand(alt, lookup),
            None => String::new(),
        }
    } else {
        lookup(inner).unwrap_or_default()
    }
}

/// Apply a sequence of assignments on top of `env`, expanding each value
/// against the environment built so far (systemd's environment.d semantics).
pub fn apply_assignments(env: &mut BTreeMap<String, String>, vars: &[(String, String)]) {
    for (k, v) in vars {
        let expanded = {
            let snapshot = &*env;
            expand(v, &|name| snapshot.get(name).cloned())
        };
        env.insert(k.clone(), expanded);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_quoted_values() {
        let f = EnvFile::parse(
            "# comment\n\nA=1\nB=\"hello world\"\nC='single $HOME'\nD=  spaced  \nexport E=x\n",
        );
        assert_eq!(f.get("A"), Some("1"));
        assert_eq!(f.get("B"), Some("hello world"));
        assert_eq!(f.get("C"), Some("single \\$HOME"));
        assert_eq!(f.get("D"), Some("spaced"));
        assert_eq!(f.get("E"), Some("x"));
    }

    #[test]
    fn keeps_var_references_verbatim() {
        let f = EnvFile::parse("PATH=$HOME/bin:${PATH}\nX=\"${FOO:-bar}\"\n");
        assert_eq!(f.get("PATH"), Some("$HOME/bin:${PATH}"));
        assert_eq!(f.get("X"), Some("${FOO:-bar}"));
    }

    #[test]
    fn roundtrip_preserves_comments_and_layout() {
        let src = "# header\nA=1\n  # indented comment\nB=\"x y\"\ngarbage line\n\nC=$HOME\n";
        let f = EnvFile::parse(src);
        assert_eq!(f.serialize(), src);
    }

    #[test]
    fn set_updates_in_place_and_keeps_raw_when_unchanged() {
        let mut f = EnvFile::parse("# c\nA='1'\nB=2\nA=3\n");
        f.set("A", "3");
        // Value of first A ('1') differs, so it is rewritten; duplicate dropped.
        assert_eq!(f.serialize(), "# c\nA=3\nB=2\n");
        f.set("B", "2");
        assert_eq!(f.serialize(), "# c\nA=3\nB=2\n");
        f.set("NEW", "a b");
        assert_eq!(f.serialize(), "# c\nA=3\nB=2\nNEW=\"a b\"\n");
    }

    #[test]
    fn unchanged_quoted_line_is_not_rewritten() {
        let mut f = EnvFile::parse("A=\"x\"\n");
        f.set("A", "x");
        assert_eq!(f.serialize(), "A=\"x\"\n");
    }

    #[test]
    fn remove_and_rename() {
        let mut f = EnvFile::parse("A=1\nB=2\nC=3\n");
        f.rename("B", "BB", "22");
        assert_eq!(f.serialize(), "A=1\nBB=22\nC=3\n");
        assert!(f.remove("A"));
        assert!(!f.remove("A"));
        assert_eq!(f.serialize(), "BB=22\nC=3\n");
    }

    #[test]
    fn later_duplicates_win() {
        let f = EnvFile::parse("A=1\nA=2\n");
        assert_eq!(f.get("A"), Some("2"));
        assert_eq!(f.vars(), vec![("A".into(), "2".into())]);
    }

    #[test]
    fn quote_roundtrip() {
        for v in [
            "simple",
            "with space",
            "quote\"inside",
            "back\\slash",
            "$HOME/x:${PATH}",
            "literal \\$ dollar",
            "tick`",
            "",
            "#hash",
        ] {
            let line = format!("K={}", quote_value(v));
            let f = EnvFile::parse(&line);
            assert_eq!(f.get("K"), Some(v), "roundtrip of {v:?} via {line:?}");
        }
    }

    #[test]
    fn invalid_lines_are_kept() {
        let f = EnvFile::parse("1BAD=x\nnoequals\nA=\"unterminated\n");
        assert!(f.vars().is_empty());
        assert_eq!(f.lines.len(), 3);
    }

    #[test]
    fn expansion() {
        let env: BTreeMap<String, String> = [("HOME", "/home/u"), ("PATH", "/usr/bin"), ("E", "")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let look = |n: &str| env.get(n).cloned();
        assert_eq!(expand("$HOME/bin:${PATH}", &look), "/home/u/bin:/usr/bin");
        assert_eq!(expand("${NOPE:-def}", &look), "def");
        assert_eq!(expand("${E:-def}", &look), "def");
        assert_eq!(expand("${HOME:+set}", &look), "set");
        assert_eq!(expand("${NOPE:+set}", &look), "");
        assert_eq!(expand("\\$HOME", &look), "$HOME");
        assert_eq!(expand("cost $5", &look), "cost $5");
        assert_eq!(expand("${UNTERMINATED", &look), "${UNTERMINATED");
    }

    #[test]
    fn apply_assignments_is_sequential() {
        let mut env = BTreeMap::new();
        env.insert("PATH".to_string(), "/usr/bin".to_string());
        apply_assignments(
            &mut env,
            &[
                ("A".into(), "x".into()),
                ("PATH".into(), "/opt/$A:$PATH".into()),
            ],
        );
        assert_eq!(env["PATH"], "/opt/x:/usr/bin");
    }

    #[test]
    fn name_validation() {
        assert!(is_valid_name("PATH"));
        assert!(is_valid_name("_x1"));
        assert!(!is_valid_name("1x"));
        assert!(!is_valid_name("A-B"));
        assert!(!is_valid_name(""));
        assert!(validate("A", "line\nbreak").is_err());
    }
}
