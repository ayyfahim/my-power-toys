//! Template discovery, naming rules and creation (pure logic + filesystem ops).

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, Local, TimeZone, Timelike};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

use crate::NewPlusConfig;

/// A single template: a file or folder directly inside the templates folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    pub path: PathBuf,
    /// Raw file name in the templates folder (e.g. `01. Note.txt`).
    pub file_name: String,
    pub is_dir: bool,
}

impl Template {
    /// Name shown in menus (upstream `get_menu_title`, without variable preview).
    pub fn display_name(&self, config: &NewPlusConfig) -> String {
        let mut title = self.file_name.clone();
        if config.hide_starting_digits {
            title = strip_starting_digits(&title, self.is_dir);
        }
        if config.hide_file_extension && !self.is_dir {
            title = strip_extension(&title);
        }
        title
    }

    /// Name of the item that will be created (before variable resolution and collision naming).
    pub fn target_name(&self, config: &NewPlusConfig) -> String {
        if config.hide_starting_digits {
            strip_starting_digits(&self.file_name, self.is_dir)
        } else {
            self.file_name.clone()
        }
    }
}

/// List templates: folders first, then files, each sorted by name. Hidden entries are skipped.
pub fn list_templates(dir: &Path) -> Result<Vec<Template>> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let entries = fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?;
    for entry in entries.flatten() {
        let file_name = entry.file_name().to_string_lossy().into_owned();
        if file_name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let is_dir = path.is_dir();
        let t = Template {
            path,
            file_name,
            is_dir,
        };
        if is_dir { dirs.push(t) } else { files.push(t) }
    }
    dirs.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    files.sort_by(|a, b| a.file_name.cmp(&b.file_name));
    dirs.extend(files);
    Ok(dirs)
}

/// Create the templates folder with sample templates if it does not exist yet.
/// Returns true if the folder was created.
pub fn ensure_templates_dir(dir: &Path) -> Result<bool> {
    if dir.exists() {
        return Ok(false);
    }
    fs::create_dir_all(dir).with_context(|| format!("failed to create {}", dir.display()))?;
    fs::write(
        dir.join("Any files or folders placed in the template folder are available via New+.txt"),
        "Place files and folders in this folder to use them as New+ templates.\n",
    )?;
    let example = dir.join("Example folder");
    fs::create_dir_all(&example)?;
    fs::write(example.join("Example txt file.txt"), "Example txt file\n")?;
    fs::write(
        example.join("Another example txt file.txt"),
        "Another example txt file\n",
    )?;
    fs::write(dir.join("01. Text document.txt"), "")?;
    info!("New+: created templates folder {}", dir.display());
    Ok(true)
}

/// Find a template by path, raw file name, display name or target name.
pub fn find_template(dir: &Path, query: &str, config: &NewPlusConfig) -> Result<Template> {
    let as_path = Path::new(query);
    if as_path.is_absolute() && as_path.exists() {
        let file_name = as_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        return Ok(Template {
            path: as_path.to_path_buf(),
            file_name,
            is_dir: as_path.is_dir(),
        });
    }
    let list = list_templates(dir)?;
    list.iter()
        .find(|t| t.file_name == query)
        .or_else(|| list.iter().find(|t| t.display_name(config) == query))
        .or_else(|| list.iter().find(|t| t.target_name(config) == query))
        .cloned()
        .with_context(|| format!("template '{query}' not found in {}", dir.display()))
}

/// Copy `template` into `target_dir` following the config rules; returns the created path.
pub fn create_from_template(
    template: &Path,
    target_dir: &Path,
    config: &NewPlusConfig,
) -> Result<PathBuf> {
    if !target_dir.is_dir() {
        bail!("target directory {} does not exist", target_dir.display());
    }
    let is_dir = template.is_dir();
    if !is_dir && !template.is_file() {
        bail!("template {} does not exist", template.display());
    }
    let file_name = template
        .file_name()
        .context("template has no file name")?
        .to_string_lossy()
        .into_owned();
    let t = Template {
        path: template.to_path_buf(),
        file_name,
        is_dir,
    };
    let now = Local::now();
    let mut name = t.target_name(config);
    if config.replace_variables {
        name = resolve_variables(&name, &dir_name(target_dir), &now);
    }
    if name.is_empty() {
        name = t.file_name.clone();
    }
    let dest = unique_path(target_dir, &name, is_dir, |p| p.symlink_metadata().is_ok());

    if is_dir {
        copy_dir(template, &dest, config.replace_variables, &now)?;
    } else {
        fs::copy(template, &dest)
            .with_context(|| format!("failed to copy to {}", dest.display()))?;
    }
    info!("New+: created {}", dest.display());
    Ok(dest)
}

fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn copy_dir<Tz: TimeZone>(
    src: &Path,
    dest: &Path,
    replace: bool,
    now: &DateTime<Tz>,
) -> Result<()> {
    fs::create_dir(dest).with_context(|| format!("failed to create {}", dest.display()))?;
    let parent = dir_name(dest);
    for entry in fs::read_dir(src)?.flatten() {
        let src_path = entry.path();
        let mut name = entry.file_name().to_string_lossy().into_owned();
        if replace && !name.starts_with('.') {
            let resolved = resolve_variables(&name, &parent, now);
            if !resolved.is_empty() {
                name = resolved;
            }
        }
        let is_dir = src_path.is_dir();
        let child = unique_path(dest, &name, is_dir, |p| p.symlink_metadata().is_ok());
        if is_dir {
            copy_dir(&src_path, &child, replace, now)?;
        } else {
            fs::copy(&src_path, &child)
                .with_context(|| format!("failed to copy to {}", child.display()))?;
        }
    }
    Ok(())
}

/// Strip leading ordering digits like `01. `, `02 `, `03.` (upstream `remove_starting_digits_from_filename`).
pub fn strip_starting_digits(name: &str, is_dir: bool) -> String {
    let (stem, has_ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], true),
        _ => (name, false),
    };
    let stem_only_digits = !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_digit());
    if stem_only_digits && (!is_dir || !has_ext) {
        // Digits ARE the name (e.g. `001231.txt`).
        return name.to_string();
    }
    let bytes = name.as_bytes();
    let mut i = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if i == 0 {
        return name.to_string();
    }
    if i < bytes.len() {
        if bytes[i] == b'.' {
            i += 1;
            if i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
        } else if bytes[i] == b' ' {
            i += 1;
            if i < bytes.len() && bytes[i] == b'.' {
                i += 1;
            }
        }
    }
    name[i..].to_string()
}

fn strip_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

/// Return `dir/name`, or `dir/name (2).ext`, `dir/name (3).ext`, ... if taken.
pub fn unique_path(
    dir: &Path,
    name: &str,
    is_dir: bool,
    exists: impl Fn(&Path) -> bool,
) -> PathBuf {
    let first = dir.join(name);
    if !exists(&first) {
        return first;
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && !is_dir => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    (2..)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|p| !exists(p))
        .expect("unbounded iterator")
}

/// Upstream date/time tokens, longest first so `$YYYY` wins over `$YY`/`$Y`.
const DATE_TOKENS: &[&str] = &[
    "YYYY", "YY", "Y", "MMMM", "MMM", "MM", "M", "DDDD", "DDD", "DD", "D", "HH", "H", "TT", "tt",
    "hh", "h", "mm", "m", "ss", "s", "fff", "ff", "f",
];

const PARENT_FOLDER_NAME: &str = "$PARENT_FOLDER_NAME";

fn date_token_value<Tz: TimeZone>(token: &str, t: &DateTime<Tz>) -> String {
    let t = t.naive_local();
    let hour12 = match t.hour() % 12 {
        0 => 12,
        h => h,
    };
    let ms = (t.nanosecond() / 1_000_000).min(999);
    match token {
        "YYYY" => format!("{:04}", t.year()),
        "YY" => format!("{:02}", t.year() % 100),
        "Y" => format!("{}", t.year() % 10),
        "MMMM" => t.format("%B").to_string(),
        "MMM" => t.format("%b").to_string(),
        "MM" => format!("{:02}", t.month()),
        "M" => format!("{}", t.month()),
        "DDDD" => t.format("%A").to_string(),
        "DDD" => t.format("%a").to_string(),
        "DD" => format!("{:02}", t.day()),
        "D" => format!("{}", t.day()),
        "HH" => format!("{hour12:02}"),
        "H" => format!("{hour12}"),
        "TT" => (if t.hour() < 12 { "AM" } else { "PM" }).to_string(),
        "tt" => (if t.hour() < 12 { "am" } else { "pm" }).to_string(),
        "hh" => format!("{:02}", t.hour()),
        "h" => format!("{}", t.hour()),
        "mm" => format!("{:02}", t.minute()),
        "m" => format!("{}", t.minute()),
        "ss" => format!("{:02}", t.second()),
        "s" => format!("{}", t.second()),
        "fff" => format!("{ms:03}"),
        "ff" => format!("{:02}", ms / 10),
        "f" => format!("{}", ms / 100),
        _ => String::new(),
    }
}

/// Replace date/time `$` tokens. A token is only replaced when preceded by an odd run of `$`
/// (so `$$YYYY` stays literal), matching upstream's regex semantics.
pub fn resolve_date_time<Tz: TimeZone>(s: &str, t: &DateTime<Tz>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(pos) = rest.find('$') {
        out.push_str(&rest[..pos]);
        let after = &rest[pos..];
        let run = after.bytes().take_while(|&b| b == b'$').count();
        let tail = &after[run..];
        if run % 2 == 1
            && let Some(tok) = DATE_TOKENS.iter().find(|tok| tail.starts_with(**tok))
        {
            out.push_str(&after[..run - 1]);
            out.push_str(&date_token_value(tok, t));
            rest = &tail[tok.len()..];
        } else {
            out.push_str(&after[..run]);
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

/// Replace `%VAR%` environment variables. Unknown variables are left as-is.
pub fn resolve_env(s: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('%') else { break };
        let var = &after[..end];
        match (!var.is_empty()).then(|| lookup(var)).flatten() {
            Some(val) if !val.is_empty() => {
                out.push_str(&rest[..start]);
                out.push_str(&val);
                rest = &after[end + 1..];
            }
            _ => {
                // Keep the first `%` and continue scanning from the closing one.
                out.push_str(&rest[..start + 1 + end]);
                rest = &after[end..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Environment lookup: exact name, then upper-case; `USERNAME` falls back to `USER`.
pub fn env_lookup(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| std::env::var(name.to_uppercase()).ok())
        .or_else(|| {
            name.eq_ignore_ascii_case("USERNAME")
                .then(|| std::env::var("USER").ok())
                .flatten()
        })
}

/// Resolve all upstream variables in a file name (`resolve_variables_in_filename`).
pub fn resolve_variables<Tz: TimeZone>(name: &str, parent: &str, t: &DateTime<Tz>) -> String {
    resolve_variables_with(name, parent, t, env_lookup)
}

fn resolve_variables_with<Tz: TimeZone>(
    name: &str,
    parent: &str,
    t: &DateTime<Tz>,
    lookup: impl Fn(&str) -> Option<String>,
) -> String {
    let mut s = resolve_date_time(name, t);
    s = resolve_env(&s, lookup);
    if !parent.is_empty() {
        s = s.replace(PARENT_FOLDER_NAME, parent);
    }
    make_valid_filename(&s)
}

fn make_valid_filename(s: &str) -> String {
    s.chars()
        .map(|c| if c == '/' || c == '\0' { ' ' } else { c })
        .collect()
}

/// Decode a `file://` URI into a local path (percent-decoding). Returns None for other schemes.
pub fn file_uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // Skip optional host part (`file://localhost/...`).
    let path = &rest[rest.find('/')?..];
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Some(v) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    use std::os::unix::ffi::OsStringExt;
    Some(PathBuf::from(std::ffi::OsString::from_vec(out)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, Utc};
    use std::collections::HashSet;

    fn time() -> DateTime<Utc> {
        let naive = NaiveDate::from_ymd_opt(2024, 3, 5)
            .unwrap()
            .and_hms_milli_opt(14, 7, 9, 123)
            .unwrap();
        Utc.from_utc_datetime(&naive)
    }

    #[test]
    fn date_variables() {
        let t = time();
        assert_eq!(resolve_date_time("$YYYY-$MM-$DD", &t), "2024-03-05");
        assert_eq!(resolve_date_time("$hh.$mm.$ss.$fff", &t), "14.07.09.123");
        assert_eq!(resolve_date_time("$HH $TT $tt", &t), "02 PM pm");
        assert_eq!(
            resolve_date_time("$YY $Y $M $D $h $m $s", &t),
            "24 4 3 5 14 7 9"
        );
        assert_eq!(
            resolve_date_time("$MMMM $MMM $DDDD $DDD", &t),
            "March Mar Tuesday Tue"
        );
        assert_eq!(resolve_date_time("$$YYYY", &t), "$$YYYY");
        assert_eq!(resolve_date_time("$$$YYYY", &t), "$$2024");
        assert_eq!(resolve_date_time("cost $5 $x", &t), "cost $5 $x");
    }

    #[test]
    fn env_and_parent_variables() {
        let t = time();
        let lookup = |v: &str| (v == "USERNAME").then(|| "alice".to_string());
        assert_eq!(
            resolve_variables_with("%USERNAME% notes %NOPE%.txt", "", &t, lookup),
            "alice notes %NOPE%.txt"
        );
        assert_eq!(
            resolve_variables_with("$PARENT_FOLDER_NAME $YYYY.md", "proj", &t, lookup),
            "proj 2024.md"
        );
        assert_eq!(
            resolve_variables_with("$PARENT_FOLDER_NAME", "", &t, lookup),
            "$PARENT_FOLDER_NAME"
        );
        assert_eq!(
            resolve_env("100% sure %USERNAME%", lookup),
            "100% sure alice"
        );
        assert_eq!(resolve_variables_with("a/b", "", &t, lookup), "a b");
    }

    #[test]
    fn starting_digits() {
        assert_eq!(
            strip_starting_digits("01. First entry.txt", false),
            "First entry.txt"
        );
        assert_eq!(
            strip_starting_digits("02. Second entry", true),
            "Second entry"
        );
        assert_eq!(strip_starting_digits("03 Third entry", true), "Third entry");
        assert_eq!(
            strip_starting_digits("04 Fourth entry.txt", false),
            "Fourth entry.txt"
        );
        assert_eq!(
            strip_starting_digits("05.Fifth entry.txt", false),
            "Fifth entry.txt"
        );
        assert_eq!(strip_starting_digits("06 .Sixth", true), "Sixth");
        assert_eq!(strip_starting_digits("001231", true), "001231");
        assert_eq!(strip_starting_digits("001231.txt", false), "001231.txt");
        assert_eq!(strip_starting_digits("0123.Name", true), "Name");
        assert_eq!(
            strip_starting_digits("13. 0123456789012345.txt", false),
            "0123456789012345.txt"
        );
        assert_eq!(strip_starting_digits("Plain.txt", false), "Plain.txt");
    }

    #[test]
    fn collision_naming() {
        let dir = Path::new("/t");
        let taken: HashSet<PathBuf> = ["/t/a.txt", "/t/a (2).txt", "/t/Folder", "/t/x.y"]
            .iter()
            .map(PathBuf::from)
            .collect();
        let exists = |p: &Path| taken.contains(p);
        assert_eq!(unique_path(dir, "b.txt", false, exists), dir.join("b.txt"));
        assert_eq!(
            unique_path(dir, "a.txt", false, exists),
            dir.join("a (3).txt")
        );
        assert_eq!(
            unique_path(dir, "Folder", true, exists),
            dir.join("Folder (2)")
        );
        assert_eq!(unique_path(dir, "x.y", true, exists), dir.join("x.y (2)"));
    }

    #[test]
    fn display_names() {
        let cfg = NewPlusConfig::default();
        let t = Template {
            path: PathBuf::from("/x/01. Note.md"),
            file_name: "01. Note.md".into(),
            is_dir: false,
        };
        assert_eq!(t.display_name(&cfg), "Note");
        assert_eq!(t.target_name(&cfg), "Note.md");
    }

    #[test]
    fn uri_decoding() {
        assert_eq!(
            file_uri_to_path("file:///home/a/My%20Docs"),
            Some(PathBuf::from("/home/a/My Docs"))
        );
        assert_eq!(
            file_uri_to_path("file://localhost/tmp/%C3%A9"),
            Some(PathBuf::from("/tmp/é"))
        );
        assert_eq!(file_uri_to_path("sftp://host/tmp"), None);
    }

    #[test]
    fn create_and_collide() {
        let base = std::env::temp_dir().join(format!("mpt-new-plus-test-{}", std::process::id()));
        let tpl = base.join("tpl");
        let target = base.join("target");
        fs::create_dir_all(&target).unwrap();
        ensure_templates_dir(&tpl).unwrap();
        let list = list_templates(&tpl).unwrap();
        assert!(list[0].is_dir);
        let cfg = NewPlusConfig::default();
        let folder = find_template(&tpl, "Example folder", &cfg).unwrap();
        let a = create_from_template(&folder.path, &target, &cfg).unwrap();
        let b = create_from_template(&folder.path, &target, &cfg).unwrap();
        assert_eq!(a, target.join("Example folder"));
        assert_eq!(b, target.join("Example folder (2)"));
        assert!(b.join("Example txt file.txt").is_file());
        let doc = find_template(&tpl, "Text document", &cfg).unwrap();
        let c = create_from_template(&doc.path, &target, &cfg).unwrap();
        assert_eq!(c, target.join("Text document.txt"));
        fs::remove_dir_all(&base).unwrap();
    }
}
