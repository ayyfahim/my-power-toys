//! Package lookups for a missing command, plus fuzzy "did you mean" matching.
//!
//! Every external tool is invoked with `std::process::Command` and separate
//! arguments (never through a shell). The command name is validated with
//! [`is_valid_command_name`] before any lookup happens.

use std::collections::BTreeSet;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// A package that provides the missing command.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Suggestion {
    /// Command line that installs the package, e.g. `sudo apt install vim`.
    pub install: String,
    /// Which lookup produced this suggestion (e.g. `apt-file`).
    pub source: &'static str,
}

/// Result of looking up a missing command.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LookupResult {
    pub packages: Vec<Suggestion>,
    pub similar: Vec<String>,
}

/// Only `[A-Za-z0-9._+-]`, not starting with `-` (would look like an option
/// to the external tools), not `.`/`..`, at most 255 bytes.
pub fn is_valid_command_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('-')
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
}

/// Escape regex metacharacters that are allowed in a command name.
fn regex_escape(name: &str) -> String {
    let mut out = String::with_capacity(name.len() * 2);
    for c in name.chars() {
        if matches!(c, '.' | '+') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Classic Levenshtein edit distance (by `char`).
pub fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

// ---------------------------------------------------------------------------
// Output parsers (pure)
// ---------------------------------------------------------------------------

/// `apt-file search --regex ...` prints `package: /usr/bin/cmd`.
pub fn parse_apt_file(output: &str) -> Vec<String> {
    dedup(
        output
            .lines()
            .filter_map(|l| l.split_once(": "))
            .map(|(pkg, _)| pkg.trim().to_string())
            .filter(|p| !p.is_empty() && !p.contains(char::is_whitespace)),
    )
}

/// Debian/Ubuntu `/usr/lib/command-not-found` prints lines like
/// `sudo apt install vim  # version ...` or `apt install vim`.
pub fn parse_debian_cnf(output: &str) -> Vec<String> {
    dedup(output.lines().filter_map(|l| {
        let l = l.trim();
        let rest = l
            .strip_prefix("sudo apt install ")
            .or_else(|| l.strip_prefix("apt install "))?;
        rest.split_whitespace().next().map(str::to_string)
    }))
}

/// `dnf provides */bin/cmd` (dnf4 and dnf5). Package header lines look like
/// `vim-enhanced-2:9.0.2081-1.fc39.x86_64 : A version of the VIM editor`.
pub fn parse_dnf_provides(output: &str) -> Vec<String> {
    dedup(output.lines().filter_map(|l| {
        if l.starts_with(char::is_whitespace) {
            return None;
        }
        let (nevra, _) = l.split_once(" : ")?;
        let nevra = nevra.trim();
        if nevra.is_empty() || nevra.contains(char::is_whitespace) {
            return None;
        }
        nevra_to_name(nevra)
    }))
}

/// `name-[epoch:]version-release.arch` -> `name`.
fn nevra_to_name(nevra: &str) -> Option<String> {
    let (nevr, _arch) = nevra.rsplit_once('.')?;
    let (nev, _release) = nevr.rsplit_once('-')?;
    let (name, _version) = nev.rsplit_once('-')?;
    (!name.is_empty()).then(|| name.to_string())
}

/// `pacman -F --machinereadable cmd` prints `repo\0pkg\0version\0path\n`.
/// Only paths in a `bin`/`sbin` directory ending in exactly `cmd` are kept.
pub fn parse_pacman_machinereadable(output: &str, cmd: &str) -> Vec<String> {
    dedup(output.lines().filter_map(|l| {
        let mut f = l.split('\0');
        let _repo = f.next()?;
        let pkg = f.next()?;
        let _ver = f.next()?;
        let path = f.next()?;
        let dir = path.strip_suffix(cmd)?.strip_suffix('/')?;
        (dir.ends_with("bin") && !pkg.is_empty()).then(|| pkg.to_string())
    }))
}

/// `pkgfile -b cmd` prints `repo/pkg`.
pub fn parse_pkgfile(output: &str) -> Vec<String> {
    dedup(output.lines().filter_map(|l| {
        let l = l.trim();
        let pkg = l.rsplit_once('/').map_or(l, |(_, p)| p);
        (!pkg.is_empty() && !pkg.contains(char::is_whitespace)).then(|| pkg.to_string())
    }))
}

/// `zypper se --provides cmd` prints a `|`-separated table:
/// `S | Name | Summary | Type`.
pub fn parse_zypper(output: &str) -> Vec<String> {
    let mut name_col = None;
    let mut type_col = None;
    dedup(output.lines().filter_map(|l| {
        if !l.contains('|') {
            return None;
        }
        let cols: Vec<&str> = l.split('|').map(str::trim).collect();
        if let Some(n) = cols.iter().position(|c| *c == "Name") {
            name_col = Some(n);
            type_col = cols.iter().position(|c| *c == "Type");
            return None;
        }
        let name = cols.get(name_col?)?;
        if let Some(t) = type_col
            && cols.get(t).is_some_and(|t| *t != "package")
        {
            return None;
        }
        (!name.is_empty() && !name.starts_with('-')).then(|| name.to_string())
    }))
}

/// `nix-locate --top-level ...` prints `attr.output  size  type  /nix/store/...`.
pub fn parse_nix_locate(output: &str) -> Vec<String> {
    dedup(output.lines().filter_map(|l| {
        let attr = l.split_whitespace().next()?;
        if attr.starts_with('(') {
            return None;
        }
        let attr = attr.strip_prefix("nixpkgs.").unwrap_or(attr);
        let name = attr
            .rsplit_once('.')
            .filter(|(_, out)| matches!(*out, "out" | "bin"))
            .map_or(attr, |(n, _)| n);
        (!name.is_empty()).then(|| name.to_string())
    }))
}

/// `flatpak search --columns=application,remotes cmd` prints tab-separated
/// `app.id\tremote[,remote...]`. Returns `(remote, app_id)`.
pub fn parse_flatpak(output: &str) -> Vec<(String, String)> {
    let mut seen = BTreeSet::new();
    output
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let app = f.next()?.trim();
            let remote = f.next()?.trim().split(',').next()?.trim();
            (app.contains('.') && !remote.is_empty() && !app.contains(' '))
                .then(|| (remote.to_string(), app.to_string()))
        })
        .filter(|x| seen.insert(x.clone()))
        .collect()
}

fn dedup(it: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    it.filter(|p| seen.insert(p.clone())).collect()
}

// ---------------------------------------------------------------------------
// Running external tools
// ---------------------------------------------------------------------------

/// Search `$PATH` for an executable.
pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// Run a program with a timeout and return stdout (plus stderr if
/// `with_stderr`). Returns `None` if the tool fails to start or times out.
fn run_with_timeout(
    program: &Path,
    args: &[&str],
    timeout: Duration,
    with_stderr: bool,
) -> Option<String> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(if with_stderr {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .spawn()
        .map_err(|e| tracing::warn!("failed to run {program:?}: {e}"))
        .ok()?;

    let readers: Vec<_> = [
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|mut r| {
        thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            buf
        })
    })
    .collect();

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                tracing::warn!("{program:?} timed out after {timeout:?}");
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }

    let mut out = String::new();
    for r in readers {
        if let Ok(buf) = r.join() {
            out.push_str(&String::from_utf8_lossy(&buf));
        }
    }
    Some(out)
}

type LookupFn = Box<dyn FnOnce() -> Vec<Suggestion> + Send>;

fn suggestions(source: &'static str, prefix: &str, pkgs: Vec<String>) -> Vec<Suggestion> {
    pkgs.into_iter()
        .map(|p| Suggestion {
            install: format!("{prefix} {p}"),
            source,
        })
        .collect()
}

/// Build the list of lookups available on this system.
fn available_lookups(cmd: &str, timeout: Duration) -> Vec<LookupFn> {
    let mut v: Vec<LookupFn> = Vec::new();
    let cmd = cmd.to_string();

    let debian_cnf = Path::new("/usr/lib/command-not-found");
    if is_executable(debian_cnf) {
        let c = cmd.clone();
        v.push(Box::new(move || {
            let out = run_with_timeout(debian_cnf, &["--", &c], timeout, true).unwrap_or_default();
            suggestions(
                "command-not-found",
                "sudo apt install",
                parse_debian_cnf(&out),
            )
        }));
    }
    if let Some(bin) = find_in_path("apt-file") {
        let re = format!("/(usr/)?s?bin/{}$", regex_escape(&cmd));
        v.push(Box::new(move || {
            let out = run_with_timeout(&bin, &["search", "--regex", &re], timeout, false)
                .unwrap_or_default();
            suggestions("apt-file", "sudo apt install", parse_apt_file(&out))
        }));
    }
    if let Some(bin) = find_in_path("dnf") {
        let pattern = format!("*/bin/{cmd}");
        v.push(Box::new(move || {
            let out = run_with_timeout(&bin, &["-q", "provides", &pattern], timeout, false)
                .unwrap_or_default();
            suggestions("dnf", "sudo dnf install", parse_dnf_provides(&out))
        }));
    }
    if let Some(bin) = find_in_path("pkgfile") {
        let c = cmd.clone();
        v.push(Box::new(move || {
            let out = run_with_timeout(&bin, &["-b", &c], timeout, false).unwrap_or_default();
            suggestions("pkgfile", "sudo pacman -S", parse_pkgfile(&out))
        }));
    } else if let Some(bin) = find_in_path("pacman") {
        let c = cmd.clone();
        v.push(Box::new(move || {
            let out = run_with_timeout(&bin, &["-F", "--machinereadable", &c], timeout, false)
                .unwrap_or_default();
            suggestions(
                "pacman",
                "sudo pacman -S",
                parse_pacman_machinereadable(&out, &c),
            )
        }));
    }
    if let Some(bin) = find_in_path("zypper") {
        let c = cmd.clone();
        v.push(Box::new(move || {
            let out = run_with_timeout(
                &bin,
                &["--non-interactive", "se", "--provides", &c],
                timeout,
                false,
            )
            .unwrap_or_default();
            suggestions("zypper", "sudo zypper install", parse_zypper(&out))
        }));
    }
    if let Some(bin) = find_in_path("nix-locate") {
        let path = format!("/bin/{cmd}");
        v.push(Box::new(move || {
            let out = run_with_timeout(
                &bin,
                &["--top-level", "--whole-name", "--at-root", &path],
                timeout,
                false,
            )
            .unwrap_or_default();
            suggestions("nix-locate", "nix-shell -p", parse_nix_locate(&out))
        }));
    }
    if let Some(bin) = find_in_path("flatpak") {
        let c = cmd.clone();
        v.push(Box::new(move || {
            let out = run_with_timeout(
                &bin,
                &["search", "--columns=application,remotes", &c],
                timeout,
                false,
            )
            .unwrap_or_default();
            parse_flatpak(&out)
                .into_iter()
                .take(3)
                .map(|(remote, app)| Suggestion {
                    install: format!("flatpak install {remote} {app}"),
                    source: "flatpak",
                })
                .collect()
        }));
    }
    v
}

/// Size of the multiset symmetric difference of the characters of `a` and `b`.
fn unshared_chars(a: &str, b: &str) -> usize {
    let mut rest: Vec<char> = b.chars().collect();
    let mut missing = 0;
    for c in a.chars() {
        match rest.iter().position(|&r| r == c) {
            Some(i) => {
                rest.swap_remove(i);
            }
            None => missing += 1,
        }
    }
    missing + rest.len()
}

/// Executables on `$PATH` within Levenshtein distance 2 of `cmd`
/// (closest first, at most `limit`).
pub fn similar_commands(cmd: &str, limit: usize) -> Vec<String> {
    let Some(path) = std::env::var_os("PATH") else {
        return vec![];
    };
    let len = cmd.chars().count();
    // Sorted by (edit distance, characters not shared, name) so that
    // transpositions like `gti` -> `git` rank above unrelated distance-2 hits.
    let mut found: BTreeSet<(usize, usize, String)> = BTreeSet::new();
    for dir in std::env::split_paths(&path) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Some(name) = e.file_name().to_str().map(str::to_string) else {
                continue;
            };
            if name == cmd || name.chars().count().abs_diff(len) > 2 {
                continue;
            }
            let d = levenshtein(cmd, &name);
            if d <= 2 && is_executable(&e.path()) {
                found.insert((d, unshared_chars(cmd, &name), name));
            }
        }
    }
    let mut seen = BTreeSet::new();
    found
        .into_iter()
        .filter(|(_, _, n)| seen.insert(n.clone()))
        .map(|(_, _, n)| n)
        .take(limit)
        .collect()
}

/// Look up packages (in parallel, each bounded by `timeout`) and,
/// optionally, similar commands. `cmd` must already be validated.
pub fn lookup(cmd: &str, timeout: Duration, fuzzy: bool) -> LookupResult {
    debug_assert!(is_valid_command_name(cmd));
    let handles: Vec<_> = available_lookups(cmd, timeout)
        .into_iter()
        .map(thread::spawn)
        .collect();
    let similar = if fuzzy {
        similar_commands(cmd, 5)
    } else {
        vec![]
    };
    let mut seen = BTreeSet::new();
    let packages = handles
        .into_iter()
        .filter_map(|h| h.join().ok())
        .flatten()
        .filter(|s| seen.insert(s.install.clone()))
        .collect();
    LookupResult { packages, similar }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        for ok in [
            "vim",
            "g++",
            "python3.12",
            "x86_64-linux-gnu-gcc",
            "a_b",
            "7z",
        ] {
            assert!(is_valid_command_name(ok), "{ok}");
        }
        for bad in [
            "", "-rf", "--help", ".", "..", "a b", "a;b", "$(x)", "a/b", "`x`", "é", "a\nb", "*",
        ] {
            assert!(!is_valid_command_name(bad), "{bad:?}");
        }
        assert!(!is_valid_command_name(&"a".repeat(256)));
    }

    #[test]
    fn regex_escaping() {
        assert_eq!(regex_escape("g++"), "g\\+\\+");
        assert_eq!(regex_escape("python3.12"), "python3\\.12");
    }

    #[test]
    fn levenshtein_distance() {
        assert_eq!(levenshtein("", ""), 0);
        assert_eq!(levenshtein("abc", ""), 3);
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
        assert_eq!(levenshtein("gti", "git"), 2);
        assert_eq!(levenshtein("sl", "ls"), 2);
        assert_eq!(levenshtein("pyhton", "python"), 2);
        assert_eq!(levenshtein("vim", "vim"), 0);
        assert_eq!(levenshtein("vi", "vim"), 1);
        assert_eq!(unshared_chars("gti", "git"), 0);
        assert_eq!(unshared_chars("gti", "ctr"), 4);
    }

    #[test]
    fn apt_file() {
        let out = "vim: /usr/bin/vim\nvim-tiny: /usr/bin/vim.tiny\nvim: /bin/vim\n";
        assert_eq!(parse_apt_file(out), ["vim", "vim-tiny"]);
        assert!(parse_apt_file("").is_empty());
    }

    #[test]
    fn debian_cnf() {
        let out = "Command 'htop' not found, but can be installed with:\n\n\
                   sudo apt install htop   # version 3.0.5-7build2, or\n\
                   sudo apt install btop\n\nSee 'snap info htop' for additional versions.\n";
        assert_eq!(parse_debian_cnf(out), ["htop", "btop"]);
        assert_eq!(parse_debian_cnf("apt install cowsay\n"), ["cowsay"]);
    }

    #[test]
    fn dnf_provides() {
        let dnf4 = "Last metadata expiration check: 0:12:01 ago on Mon 01 Jan 2024.\n\
vim-enhanced-2:9.0.2081-1.fc39.x86_64 : A version of the VIM editor which includes recent enhancements\n\
Repo        : fedora\n\
Matched from:\n\
Filename    : /usr/bin/vim\n\
\n\
vim-enhanced-2:9.1.158-1.fc39.x86_64 : A version of the VIM editor which includes recent enhancements\n\
Repo        : updates\n\
Matched from:\n\
Filename    : /usr/bin/vim\n";
        assert_eq!(parse_dnf_provides(dnf4), ["vim-enhanced"]);

        let dnf5 = "python3-pip-23.2.1-1.fc39.noarch : A tool for installing Python packages\n\
Repo         : fedora\n\
Matched From : \n\
Filename     : /usr/bin/pip\n";
        assert_eq!(parse_dnf_provides(dnf5), ["python3-pip"]);
        assert!(parse_dnf_provides("Error: No matches found.\n").is_empty());
    }

    #[test]
    fn pacman_machinereadable() {
        let out = "extra\0vim\09.1.0-1\0usr/bin/vim\n\
                   extra\0vim\09.1.0-1\0usr/share/vim/vim91/vim\n\
                   extra\0gvim\09.1.0-1\0usr/bin/vim\n\
                   extra\0foo\x001-1\0usr/bin/xvim\n";
        assert_eq!(parse_pacman_machinereadable(out, "vim"), ["vim", "gvim"]);
    }

    #[test]
    fn pkgfile() {
        assert_eq!(parse_pkgfile("extra/vim\nextra/gvim\n"), ["vim", "gvim"]);
    }

    #[test]
    fn zypper() {
        let out = "Loading repository data...\nReading installed packages...\n\n\
S | Name         | Summary                 | Type\n\
--+--------------+-------------------------+--------\n\
  | vim          | Vi IMproved             | package\n\
  | vim          | Vi IMproved             | srcpackage\n\
i | vim-small    | Vi IMproved small       | package\n";
        assert_eq!(parse_zypper(out), ["vim", "vim-small"]);
        assert!(parse_zypper("No matching items found.\n").is_empty());
    }

    #[test]
    fn nix_locate() {
        let out = "vim.out     4,104,344 x /nix/store/abc-vim-9.1/bin/vim\n\
                   neovim.out  1,234 s /nix/store/def-neovim/bin/vim\n\
                   (vimHugeX.out) 1 x /nix/store/x/bin/vim\n";
        assert_eq!(parse_nix_locate(out), ["vim", "neovim"]);
    }

    #[test]
    fn flatpak() {
        let out = "org.vim.Vim\tflathub\nio.neovim.nvim\tflathub,other\nNo matches found\n";
        assert_eq!(
            parse_flatpak(out),
            [
                ("flathub".to_string(), "org.vim.Vim".to_string()),
                ("flathub".to_string(), "io.neovim.nvim".to_string())
            ]
        );
    }
}
