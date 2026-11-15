use crate::config::{ApplyTo, TextFormatting, TimeSource};
use crate::tokens::{self, DateParts};
use anyhow::{Context, Result};
use regex::Regex;
use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::info;

// ── Data types ───────────────────────────────────────────────────────

/// Options that control how a rename preview is generated.
#[derive(Debug, Clone)]
pub struct RenameOptions {
    pub search: String,
    pub replace: String,
    pub use_regex: bool,
    pub match_all: bool,
    pub case_sensitive: bool,
    pub apply_to: ApplyTo,
    pub text_formatting: TextFormatting,
    pub enumerate: bool,
}

/// Extra token options not covered by [`RenameOptions`].
#[derive(Debug, Clone, Copy, Default)]
pub struct TokenOptions {
    /// Expand randomizer groups (`${rstringalnum=N}`, `${ruuidv4}`, ...).
    pub randomize: bool,
    /// Which file time feeds date/time tokens.
    pub time_source: TimeSource,
}

/// A single rename preview entry (before → after).
#[derive(Debug, Clone)]
pub struct RenamePreview {
    pub original: PathBuf,
    pub renamed: PathBuf,
    pub changed: bool,
}

/// A completed rename operation, stored for undo.
#[derive(Debug, Clone)]
pub struct RenameOperation {
    pub renames: Vec<(PathBuf, PathBuf)>,
}

/// Options that control how entries are listed.
#[derive(Debug, Clone, Default)]
pub struct ListOptions {
    pub include_folders: bool,
    pub include_subfolders: bool,
}

// ── Renamer ──────────────────────────────────────────────────────────

pub struct Renamer {
    history: Vec<RenameOperation>,
}

impl Renamer {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
        }
    }

    // ── Preview ──────────────────────────────────────────────────────

    /// Generate a preview of the rename operation without modifying anything.
    ///
    /// Date/time and metadata tokens are always active; randomizer groups
    /// need [`TokenOptions::randomize`] (see [`Renamer::preview_with`]).
    pub fn preview(&self, files: &[PathBuf], opts: &RenameOptions) -> Result<Vec<RenamePreview>> {
        self.preview_with(files, opts, &TokenOptions::default())
    }

    /// Like [`Renamer::preview`], with extra token options.
    pub fn preview_with(
        &self,
        files: &[PathBuf],
        opts: &RenameOptions,
        tokens: &TokenOptions,
    ) -> Result<Vec<RenamePreview>> {
        // Validate/compile the regex once so we fail even on empty input.
        let regex = if opts.use_regex && !opts.search.is_empty() {
            let pattern = if opts.case_sensitive {
                opts.search.clone()
            } else {
                format!("(?i){}", opts.search)
            };
            Some(Regex::new(&pattern).context("invalid regex pattern")?)
        } else {
            None
        };

        let use_time = tokens::uses_file_time(&opts.replace);
        let use_meta = tokens::uses_metadata(&opts.replace);
        let use_groups = tokens::has_groups(&opts.replace, opts.enumerate, tokens.randomize);
        let mut rng = rand::rng();
        let mut enum_index: u64 = 0;

        let mut previews = Vec::with_capacity(files.len());

        for file in files {
            let filename = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();

            // 1. Split into (target, rest) depending on ApplyTo.
            let (target, rest) = split_name(filename, opts.apply_to);

            // 2. Build this item's replacement string (date/time, metadata,
            //    enumeration and randomizer tokens).
            let mut replace = opts.replace.clone();
            if use_time || use_meta {
                let time = if use_time {
                    file_time(file, tokens.time_source).map(DateParts::from_system_time)
                } else {
                    None
                };
                let meta = if use_meta && supports_metadata(file) {
                    Some(tokens::read_exif_patterns(file))
                } else {
                    None
                };
                replace = tokens::expand_dollar_tokens(&replace, time.as_ref(), meta.as_ref());
            }
            let matched = self.is_match(&target, opts, regex.as_ref());
            if use_groups {
                replace = tokens::expand_groups(
                    &replace,
                    opts.enumerate,
                    tokens.randomize,
                    enum_index,
                    &mut rng,
                );
            }
            if matched {
                enum_index += 1;
            }

            // 3. Search & replace on the target part.
            let replaced = self.search_replace(&target, opts, &replace, regex.as_ref());

            // 4. Reassemble.
            let new_name = join_name(&replaced, &rest, opts.apply_to);

            // 5. Text formatting.
            let new_name = apply_formatting(&new_name, opts.text_formatting);

            let changed = new_name != filename;
            let renamed = file.with_file_name(&*new_name);

            previews.push(RenamePreview {
                original: file.clone(),
                renamed,
                changed,
            });
        }

        Ok(previews)
    }

    /// Backward-compatible helper that builds `RenameOptions` from bare
    /// regex pattern + replacement (match-all, case-sensitive, full name).
    pub fn preview_regex(
        &self,
        files: &[PathBuf],
        pattern: &str,
        replacement: &str,
    ) -> Result<Vec<RenamePreview>> {
        let opts = RenameOptions {
            search: pattern.into(),
            replace: replacement.into(),
            use_regex: true,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        self.preview(files, &opts)
    }

    // ── Execute / Undo ───────────────────────────────────────────────

    /// Execute the renames. Only renames entries that actually changed.
    pub fn execute(&mut self, previews: &[RenamePreview]) -> Result<RenameOperation> {
        let mut renames = Vec::new();

        for preview in previews {
            if !preview.changed {
                continue;
            }

            if preview.renamed.exists() {
                anyhow::bail!("target already exists: {}", preview.renamed.display());
            }

            fs::rename(&preview.original, &preview.renamed).with_context(|| {
                format!(
                    "failed to rename {} -> {}",
                    preview.original.display(),
                    preview.renamed.display()
                )
            })?;

            info!(
                "Renamed: {} -> {}",
                preview.original.display(),
                preview.renamed.display()
            );
            renames.push((preview.original.clone(), preview.renamed.clone()));
        }

        let op = RenameOperation { renames };
        self.history.push(op.clone());
        Ok(op)
    }

    /// Undo the last rename operation.
    pub fn undo(&mut self) -> Result<()> {
        let op = self
            .history
            .pop()
            .ok_or_else(|| anyhow::anyhow!("nothing to undo"))?;

        for (original, renamed) in op.renames.iter().rev() {
            fs::rename(renamed, original).with_context(|| {
                format!(
                    "failed to undo rename {} -> {}",
                    renamed.display(),
                    original.display()
                )
            })?;
            info!("Undone: {} -> {}", renamed.display(), original.display());
        }

        Ok(())
    }

    // ── Listing ──────────────────────────────────────────────────────

    /// List entries in a directory with the given options.
    pub fn list_entries(dir: &Path, opts: &ListOptions) -> Result<Vec<PathBuf>> {
        let mut entries = Vec::new();
        Self::collect_entries(dir, opts, opts.include_subfolders, &mut entries)?;
        entries.sort();
        Ok(entries)
    }

    /// Convenience wrapper: list only files (legacy behaviour).
    pub fn list_files(dir: &Path) -> Result<Vec<PathBuf>> {
        Self::list_entries(dir, &ListOptions::default())
    }

    fn collect_entries(
        dir: &Path,
        opts: &ListOptions,
        recurse: bool,
        out: &mut Vec<PathBuf>,
    ) -> Result<()> {
        let rd = fs::read_dir(dir)
            .with_context(|| format!("failed to read directory {}", dir.display()))?;

        for entry in rd.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if opts.include_folders {
                    out.push(path.clone());
                }
                if recurse {
                    Self::collect_entries(&path, opts, true, out)?;
                }
            } else {
                out.push(path);
            }
        }

        Ok(())
    }

    // ── Internal helpers ─────────────────────────────────────────────

    /// Does the search term occur in `input`?
    fn is_match(&self, input: &str, opts: &RenameOptions, regex: Option<&Regex>) -> bool {
        if opts.search.is_empty() {
            return false;
        }
        match regex {
            Some(re) => re.is_match(input),
            None if opts.case_sensitive => input.contains(&opts.search),
            None => input.to_lowercase().contains(&opts.search.to_lowercase()),
        }
    }

    /// Perform the search/replace on a single string slice.
    fn search_replace(
        &self,
        input: &str,
        opts: &RenameOptions,
        replace: &str,
        regex: Option<&Regex>,
    ) -> String {
        if opts.search.is_empty() {
            return input.to_string();
        }

        if let Some(re) = regex {
            let replace = tokens::normalize_group_refs(replace);
            let result = if opts.match_all {
                re.replace_all(input, replace.as_str())
            } else {
                re.replace(input, replace.as_str())
            };
            result.into_owned()
        } else if opts.case_sensitive {
            if opts.match_all {
                input.replace(&opts.search, replace)
            } else {
                input.replacen(&opts.search, replace, 1)
            }
        } else {
            plain_replace_case_insensitive(input, &opts.search, replace, opts.match_all)
        }
    }
}

impl Default for Renamer {
    fn default() -> Self {
        Self::new()
    }
}

// ── Free helpers ─────────────────────────────────────────────────────

/// Split a filename into (target, rest) depending on the `ApplyTo` mode.
///
/// - `FilenameOnly`         → target = stem,      rest = ".ext"
/// - `ExtensionOnly`        → target = ext,       rest = stem + "."
/// - `FilenameAndExtension` → target = full name,  rest = ""
fn split_name(filename: &str, apply_to: ApplyTo) -> (String, String) {
    match apply_to {
        ApplyTo::FilenameAndExtension => (filename.to_string(), String::new()),
        ApplyTo::FilenameOnly => {
            if let Some(dot) = filename.rfind('.') {
                (filename[..dot].to_string(), filename[dot..].to_string())
            } else {
                (filename.to_string(), String::new())
            }
        }
        ApplyTo::ExtensionOnly => {
            if let Some(dot) = filename.rfind('.') {
                (
                    filename[dot + 1..].to_string(),
                    filename[..=dot].to_string(),
                )
            } else {
                // No extension – nothing to operate on.
                (String::new(), filename.to_string())
            }
        }
    }
}

/// Reassemble the filename from the modified part and the untouched part.
fn join_name(modified: &str, rest: &str, apply_to: ApplyTo) -> String {
    match apply_to {
        ApplyTo::FilenameAndExtension => modified.to_string(),
        ApplyTo::FilenameOnly => format!("{modified}{rest}"),
        ApplyTo::ExtensionOnly => format!("{rest}{modified}"),
    }
}

/// Apply text formatting to the whole filename (including extension).
fn apply_formatting(name: &str, fmt: TextFormatting) -> Cow<'_, str> {
    match fmt {
        TextFormatting::None => Cow::Borrowed(name),
        TextFormatting::Lowercase => Cow::Owned(name.to_lowercase()),
        TextFormatting::Uppercase => Cow::Owned(name.to_uppercase()),
        TextFormatting::TitleCase => {
            let mut chars = name.chars();
            let result = match chars.next() {
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    format!("{upper}{}", chars.as_str())
                }
                None => String::new(),
            };
            Cow::Owned(result)
        }
        TextFormatting::CapitalizeEachWord => {
            let mut result = String::with_capacity(name.len());
            let mut capitalize_next = true;
            for c in name.chars() {
                if c == ' ' || c == '_' || c == '-' || c == '.' {
                    capitalize_next = true;
                    result.push(c);
                } else if capitalize_next {
                    for u in c.to_uppercase() {
                        result.push(u);
                    }
                    capitalize_next = false;
                } else {
                    result.push(c);
                }
            }
            Cow::Owned(result)
        }
    }
}

/// Case-insensitive plain-text replacement.
fn plain_replace_case_insensitive(
    input: &str,
    search: &str,
    replace: &str,
    match_all: bool,
) -> String {
    if search.is_empty() {
        return input.to_string();
    }

    let lower_input = input.to_lowercase();
    let lower_search = search.to_lowercase();
    let mut result = String::with_capacity(input.len());
    let mut start = 0;

    while let Some(pos) = lower_input[start..].find(&lower_search) {
        let abs_pos = start + pos;
        result.push_str(&input[start..abs_pos]);
        result.push_str(replace);
        start = abs_pos + search.len();

        if !match_all {
            break;
        }
    }

    result.push_str(&input[start..]);
    result
}

/// Read the requested timestamp of a file.
fn file_time(path: &Path, source: TimeSource) -> Option<std::time::SystemTime> {
    let meta = fs::metadata(path).ok()?;
    match source {
        // Birth time isn't available on every filesystem; fall back to mtime.
        TimeSource::Created => meta.created().or_else(|_| meta.modified()).ok(),
        TimeSource::Modified => meta.modified().ok(),
        TimeSource::Accessed => meta.accessed().ok(),
    }
}

/// Upstream only reads metadata from these image types (and never folders).
fn supports_metadata(path: &Path) -> bool {
    !path.is_dir()
        && path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            tokens::METADATA_EXTENSIONS
                .iter()
                .any(|x| x.eq_ignore_ascii_case(e))
        })
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // -- Legacy API ---------------------------------------------------

    #[test]
    fn preview_regex_replace() {
        let renamer = Renamer::new();
        let files = vec![
            PathBuf::from("/tmp/photo_001.jpg"),
            PathBuf::from("/tmp/photo_002.jpg"),
            PathBuf::from("/tmp/document.txt"),
        ];

        let previews = renamer
            .preview_regex(&files, r"photo_(\d+)", "img_$1")
            .unwrap();

        assert!(previews[0].changed);
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/img_001.jpg"));
        assert!(previews[1].changed);
        assert_eq!(previews[1].renamed, PathBuf::from("/tmp/img_002.jpg"));
        assert!(!previews[2].changed);
    }

    #[test]
    fn preview_no_match() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/test.txt")];
        let previews = renamer.preview_regex(&files, "xyz", "abc").unwrap();
        assert!(!previews[0].changed);
    }

    #[test]
    fn invalid_regex_errors() {
        let renamer = Renamer::new();
        let result = renamer.preview_regex(&[], "[invalid", "");
        assert!(result.is_err());
    }

    // -- Plain text search --------------------------------------------

    #[test]
    fn plain_text_replace_all() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/foo-bar-foo.txt")];
        let opts = RenameOptions {
            search: "foo".into(),
            replace: "baz".into(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/baz-bar-baz.txt"));
    }

    #[test]
    fn plain_text_replace_first() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/foo-bar-foo.txt")];
        let opts = RenameOptions {
            search: "foo".into(),
            replace: "baz".into(),
            use_regex: false,
            match_all: false,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/baz-bar-foo.txt"));
    }

    // -- Case insensitive ---------------------------------------------

    #[test]
    fn case_insensitive_plain() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/FooBar.txt")];
        let opts = RenameOptions {
            search: "foobar".into(),
            replace: "replaced".into(),
            use_regex: false,
            match_all: true,
            case_sensitive: false,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/replaced.txt"));
    }

    #[test]
    fn case_insensitive_regex() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/FooBar.txt")];
        let opts = RenameOptions {
            search: "foobar".into(),
            replace: "replaced".into(),
            use_regex: true,
            match_all: true,
            case_sensitive: false,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/replaced.txt"));
    }

    // -- Apply to -----------------------------------------------------

    #[test]
    fn apply_to_filename_only() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/foo.foo")];
        let opts = RenameOptions {
            search: "foo".into(),
            replace: "bar".into(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameOnly,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        // Only stem changes, extension stays .foo
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/bar.foo"));
    }

    #[test]
    fn apply_to_extension_only() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/foo.foo")];
        let opts = RenameOptions {
            search: "foo".into(),
            replace: "bar".into(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::ExtensionOnly,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        // Only extension changes, stem stays foo
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/foo.bar"));
    }

    // -- Text formatting ----------------------------------------------

    #[test]
    fn formatting_lowercase() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/HELLO_WORLD.TXT")];
        let opts = RenameOptions {
            search: String::new(),
            replace: String::new(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::Lowercase,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/hello_world.txt"));
    }

    #[test]
    fn formatting_uppercase() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/hello.txt")];
        let opts = RenameOptions {
            search: String::new(),
            replace: String::new(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::Uppercase,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/HELLO.TXT"));
    }

    #[test]
    fn formatting_title_case() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/hello world.txt")];
        let opts = RenameOptions {
            search: String::new(),
            replace: String::new(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::TitleCase,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/Hello world.txt"));
    }

    #[test]
    fn formatting_capitalize_each_word() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/hello world-foo.txt")];
        let opts = RenameOptions {
            search: String::new(),
            replace: String::new(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::CapitalizeEachWord,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(
            previews[0].renamed,
            PathBuf::from("/tmp/Hello World-Foo.Txt")
        );
    }

    // -- Enumeration --------------------------------------------------

    #[test]
    fn enumerate_simple() {
        let renamer = Renamer::new();
        let files = vec![
            PathBuf::from("/tmp/a.txt"),
            PathBuf::from("/tmp/b.txt"),
            PathBuf::from("/tmp/c.txt"),
        ];
        let opts = RenameOptions {
            search: r"(.+)\.txt".into(),
            replace: "${}_$1.txt".into(),
            use_regex: true,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::None,
            enumerate: true,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/0_a.txt"));
        assert_eq!(previews[1].renamed, PathBuf::from("/tmp/1_b.txt"));
        assert_eq!(previews[2].renamed, PathBuf::from("/tmp/2_c.txt"));
    }

    #[test]
    fn enumerate_with_options() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/a.jpg"), PathBuf::from("/tmp/b.jpg")];
        let opts = RenameOptions {
            search: r"(.+)\.jpg".into(),
            replace: "img_${start=10;increment=5;padding=4}.jpg".into(),
            use_regex: true,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::None,
            enumerate: true,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/img_0010.jpg"));
        assert_eq!(previews[1].renamed, PathBuf::from("/tmp/img_0015.jpg"));
    }

    // -- Edge cases ---------------------------------------------------

    #[test]
    fn empty_search_returns_unchanged() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/test.txt")];
        let opts = RenameOptions {
            search: String::new(),
            replace: "anything".into(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameAndExtension,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        assert!(!previews[0].changed);
    }

    #[test]
    fn extension_only_no_extension() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/Makefile")];
        let opts = RenameOptions {
            search: "Make".into(),
            replace: "Cake".into(),
            use_regex: false,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::ExtensionOnly,
            text_formatting: TextFormatting::None,
            enumerate: false,
        };
        let previews = renamer.preview(&files, &opts).unwrap();
        // No extension → nothing to operate on → unchanged.
        assert!(!previews[0].changed);
    }

    // -- Tokens -------------------------------------------------------

    fn opts(search: &str, replace: &str, use_regex: bool, enumerate: bool) -> RenameOptions {
        RenameOptions {
            search: search.into(),
            replace: replace.into(),
            use_regex,
            match_all: true,
            case_sensitive: true,
            apply_to: ApplyTo::FilenameOnly,
            text_formatting: TextFormatting::None,
            enumerate,
        }
    }

    #[test]
    fn enumerate_without_regex() {
        let renamer = Renamer::new();
        let files = vec![
            PathBuf::from("/tmp/photo.jpg"),
            PathBuf::from("/tmp/other.jpg"),
            PathBuf::from("/tmp/photo2.jpg"),
        ];
        let previews = renamer
            .preview(&files, &opts("photo", "pic_${padding=2}", false, true))
            .unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/pic_00.jpg"));
        // Non-matching item doesn't consume a number.
        assert!(!previews[1].changed);
        assert_eq!(previews[2].renamed, PathBuf::from("/tmp/pic_012.jpg"));
    }

    #[test]
    fn regex_group_followed_by_underscore() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/a.png")];
        let previews = renamer
            .preview(&files, &opts("(.*)", "$1_foo", true, false))
            .unwrap();
        assert_eq!(previews[0].renamed, PathBuf::from("/tmp/a_foo.png"));
    }

    #[test]
    fn randomizer_requires_flag() {
        let renamer = Renamer::new();
        let files = vec![PathBuf::from("/tmp/a.txt")];
        let o = opts("a", "${rstringdigit=4}", false, false);
        let plain = renamer.preview(&files, &o).unwrap();
        assert_eq!(
            plain[0].renamed,
            PathBuf::from("/tmp/${rstringdigit=4}.txt")
        );
        let tok = TokenOptions {
            randomize: true,
            ..Default::default()
        };
        let rnd = renamer.preview_with(&files, &o, &tok).unwrap();
        let name = rnd[0]
            .renamed
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(name.len(), 4);
        assert!(name.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn date_tokens_from_file_time() {
        let dir = std::env::temp_dir().join(format!("mpt-br-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("doc.txt");
        fs::write(&file, "x").unwrap();
        let mtime = fs::metadata(&file).unwrap().modified().unwrap();
        let d = DateParts::from_system_time(mtime);
        let tok = TokenOptions {
            randomize: false,
            time_source: TimeSource::Modified,
        };
        let previews = Renamer::new()
            .preview_with(
                std::slice::from_ref(&file),
                &opts("doc", "$YYYY-$MM-$DD_doc", false, false),
                &tok,
            )
            .unwrap();
        let expected = format!("{:04}-{:02}-{:02}_doc.txt", d.year, d.month, d.day);
        assert_eq!(previews[0].renamed, dir.join(expected));
        let _ = fs::remove_dir_all(&dir);
    }
}
