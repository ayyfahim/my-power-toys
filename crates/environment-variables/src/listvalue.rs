//! List-type variables such as `PATH` (entries separated by `:`).

/// Well-known colon-separated list variables. Upstream PowerToys hard-codes a
/// similar list (PATH, PATHEXT, PSMODULEPATH, ...) for its list editor.
const LIST_VARIABLES: &[&str] = &[
    "PATH",
    "MANPATH",
    "INFOPATH",
    "CDPATH",
    "LD_LIBRARY_PATH",
    "LIBRARY_PATH",
    "PKG_CONFIG_PATH",
    "PYTHONPATH",
    "PERL5LIB",
    "CLASSPATH",
    "GOPATH",
    "XDG_DATA_DIRS",
    "XDG_CONFIG_DIRS",
    "GSETTINGS_SCHEMA_DIR",
    "GI_TYPELIB_PATH",
    "QT_PLUGIN_PATH",
    "CMAKE_PREFIX_PATH",
    "ACLOCAL_PATH",
];

/// Whether a variable should be shown with the list editor by default.
pub fn is_list_variable(name: &str) -> bool {
    LIST_VARIABLES.contains(&name) || name.ends_with("_PATH") || name.ends_with("_DIRS")
}

/// Split a value into list entries. An empty value is an empty list.
pub fn split(value: &str) -> Vec<String> {
    if value.is_empty() {
        return Vec::new();
    }
    value.split(':').map(str::to_string).collect()
}

/// Join list entries with `:`, dropping empty entries (an empty PATH entry
/// means "current directory", which is almost never intended).
pub fn join(entries: &[String]) -> String {
    entries
        .iter()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
        .collect::<Vec<_>>()
        .join(":")
}

/// Remove duplicate entries, keeping the first occurrence.
pub fn dedup(value: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    value
        .split(':')
        .filter(|e| !e.is_empty() && seen.insert(*e))
        .collect::<Vec<_>>()
        .join(":")
}

/// Move entry `index` by `delta` positions (clamped). Returns the new index.
pub fn move_entry(entries: &mut [String], index: usize, delta: isize) -> usize {
    if entries.is_empty() || index >= entries.len() {
        return index;
    }
    let target = (index as isize + delta).clamp(0, entries.len() as isize - 1) as usize;
    if target < index {
        entries[target..=index].rotate_right(1);
    } else if target > index {
        entries[index..=target].rotate_left(1);
    }
    target
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection() {
        assert!(is_list_variable("PATH"));
        assert!(is_list_variable("XDG_DATA_DIRS"));
        assert!(is_list_variable("FOO_PATH"));
        assert!(!is_list_variable("HOME"));
    }

    #[test]
    fn split_join_roundtrip() {
        let v = "$HOME/bin:/usr/bin:${PATH}";
        assert_eq!(join(&split(v)), v);
        assert!(split("").is_empty());
        assert_eq!(join(&["a".into(), "".into(), " b ".into()]), "a:b");
    }

    #[test]
    fn dedup_keeps_first() {
        assert_eq!(dedup("/a:/b:/a::/c:/b"), "/a:/b:/c");
    }

    #[test]
    fn move_entries() {
        let mut e: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(move_entry(&mut e, 2, -1), 1);
        assert_eq!(e, ["a", "c", "b"]);
        assert_eq!(move_entry(&mut e, 0, -1), 0);
        assert_eq!(move_entry(&mut e, 0, 5), 2);
        assert_eq!(e, ["c", "b", "a"]);
    }
}
