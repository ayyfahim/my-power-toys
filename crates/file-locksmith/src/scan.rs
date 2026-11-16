//! Find processes that are using a set of files or directories by walking
//! `/proc/<pid>/{fd,cwd,exe,maps}`.
//!
//! The proc root and passwd file are parameters so the matching logic can be
//! tested against a fake directory tree.

use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

/// A process that uses at least one of the requested paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessInfo {
    pub pid: u32,
    /// Short name from `/proc/<pid>/comm`.
    pub name: String,
    /// Full command line (NUL separators replaced by spaces).
    pub cmdline: String,
    /// Real user id of the process, if known.
    pub uid: Option<u32>,
    /// User name resolved from the passwd file (falls back to the numeric uid).
    pub user: String,
    /// Executable path from `/proc/<pid>/exe`, if readable.
    pub exe: Option<PathBuf>,
    /// The matched files, sorted and de-duplicated.
    pub files: Vec<PathBuf>,
}

/// Result of a scan.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanResult {
    /// Matching processes, sorted by pid.
    pub processes: Vec<ProcessInfo>,
    /// Number of processes whose open files could not be read (usually
    /// processes owned by other users when not running as root).
    pub inaccessible: usize,
}

/// Scanner configuration.
#[derive(Debug, Clone)]
pub struct Scanner {
    pub proc_root: PathBuf,
    pub passwd_path: PathBuf,
    /// Pids to skip (by default the scanning process itself).
    pub exclude_pids: Vec<u32>,
}

impl Default for Scanner {
    fn default() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
            passwd_path: PathBuf::from("/etc/passwd"),
            exclude_pids: vec![std::process::id()],
        }
    }
}

/// Scan the real `/proc` for processes using `paths`.
pub fn find_processes(paths: &[PathBuf]) -> ScanResult {
    Scanner::default().scan(paths)
}

impl Scanner {
    /// Scan for processes using any of `paths`. A path matches a file used by
    /// a process if it is equal to it or a parent directory of it.
    pub fn scan(&self, paths: &[PathBuf]) -> ScanResult {
        let targets = normalize_targets(paths);
        let mut result = ScanResult::default();
        if targets.is_empty() {
            return result;
        }
        let users = fs::read_to_string(&self.passwd_path)
            .map(|s| parse_passwd(&s))
            .unwrap_or_default();

        let Ok(entries) = fs::read_dir(&self.proc_root) else {
            return result;
        };
        for entry in entries.flatten() {
            let Some(pid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            if self.exclude_pids.contains(&pid) {
                continue;
            }
            match self.inspect(pid, &targets, &users) {
                Inspect::Match(info) => result.processes.push(info),
                Inspect::NoMatch => {}
                Inspect::Denied => result.inaccessible += 1,
            }
        }
        result.processes.sort_by_key(|p| p.pid);
        result
    }

    fn inspect(&self, pid: u32, targets: &[PathBuf], users: &HashMap<u32, String>) -> Inspect {
        let dir = self.proc_root.join(pid.to_string());
        let mut used = BTreeSet::new();
        let mut denied = false;

        match fs::read_dir(dir.join("fd")) {
            Ok(fds) => {
                for fd in fds.flatten() {
                    if let Ok(link) = fs::read_link(fd.path()) {
                        used.insert(link);
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                // The process exited, or a kernel thread / fake entry.
                if !dir.exists() {
                    return Inspect::NoMatch;
                }
            }
            Err(_) => denied = true,
        }

        let exe = fs::read_link(dir.join("exe")).ok().map(strip_deleted);
        for name in ["cwd", "exe"] {
            if let Ok(link) = fs::read_link(dir.join(name)) {
                used.insert(link);
            }
        }
        if let Ok(maps) = fs::read_to_string(dir.join("maps")) {
            used.extend(parse_maps(&maps));
        }

        let files: BTreeSet<PathBuf> = used
            .into_iter()
            .map(strip_deleted)
            .filter(|p| p.is_absolute() && path_matches(p, targets))
            .collect();

        if files.is_empty() {
            return if denied {
                Inspect::Denied
            } else {
                Inspect::NoMatch
            };
        }

        let name = fs::read_to_string(dir.join("comm"))
            .map(|s| s.trim_end().to_string())
            .unwrap_or_default();
        let cmdline = fs::read(dir.join("cmdline"))
            .map(|b| parse_cmdline(&b))
            .unwrap_or_default();
        let uid = fs::read_to_string(dir.join("status"))
            .ok()
            .and_then(|s| parse_status_uid(&s))
            .or_else(|| {
                use std::os::unix::fs::MetadataExt;
                fs::metadata(&dir).ok().map(|m| m.uid())
            });
        let user = match uid {
            Some(uid) => users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()),
            None => String::new(),
        };

        Inspect::Match(ProcessInfo {
            pid,
            name,
            cmdline,
            uid,
            user,
            exe,
            files: files.into_iter().collect(),
        })
    }
}

enum Inspect {
    Match(ProcessInfo),
    NoMatch,
    Denied,
}

/// Make targets absolute and canonical where possible, dropping duplicates.
pub fn normalize_targets(paths: &[PathBuf]) -> Vec<PathBuf> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let mut out: Vec<PathBuf> = Vec::new();
    for p in paths {
        if p.as_os_str().is_empty() {
            continue;
        }
        let abs = if p.is_absolute() {
            p.clone()
        } else {
            cwd.join(p)
        };
        let norm = fs::canonicalize(&abs).unwrap_or_else(|_| lexical_normalize(&abs));
        if !out.contains(&norm) {
            out.push(norm);
        }
    }
    out
}

/// Resolve `.` and `..` without touching the filesystem.
fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// True if `file` equals one of `targets` or lies below one of them.
/// `Path::starts_with` compares whole components, so `/a/bc` does not match `/a/b`.
pub fn path_matches(file: &Path, targets: &[PathBuf]) -> bool {
    targets.iter().any(|t| file.starts_with(t))
}

/// Remove the ` (deleted)` suffix the kernel appends to unlinked files.
fn strip_deleted(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_suffix(" (deleted)")) {
        Some(s) => PathBuf::from(s),
        None => p,
    }
}

/// Extract file paths from the contents of `/proc/<pid>/maps`.
///
/// Line format: `address perms offset dev inode [pathname]`; the pathname may
/// contain spaces. Pseudo entries like `[heap]` are skipped.
pub fn parse_maps(contents: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for line in contents.lines() {
        let mut rest = line;
        let mut ok = true;
        for _ in 0..5 {
            rest = rest.trim_start();
            match rest.find(char::is_whitespace) {
                Some(i) => rest = &rest[i..],
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        let path = rest.trim();
        if path.starts_with('/') {
            out.push(PathBuf::from(path));
        }
    }
    out
}

/// Turn a NUL-separated cmdline into a space-separated string.
pub fn parse_cmdline(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Read the real uid from the `Uid:` line of `/proc/<pid>/status`.
pub fn parse_status_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .and_then(|rest| rest.split_whitespace().next())
        .and_then(|s| s.parse().ok())
}

/// Parse `/etc/passwd` into a uid -> user name map.
pub fn parse_passwd(contents: &str) -> HashMap<u32, String> {
    let mut map = HashMap::new();
    for line in contents.lines() {
        if line.starts_with('#') {
            continue;
        }
        let mut fields = line.split(':');
        let (Some(name), Some(_), Some(uid)) = (fields.next(), fields.next(), fields.next()) else {
            continue;
        };
        if let Ok(uid) = uid.parse::<u32>() {
            map.entry(uid).or_insert_with(|| name.to_string());
        }
    }
    map
}

/// Whether this process runs with root privileges.
pub fn current_user_is_root() -> bool {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct FakeProc {
        root: PathBuf,
    }

    impl FakeProc {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "mpt-file-locksmith-test-{tag}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).unwrap();
            fs::write(
                root.join("passwd"),
                "root:x:0:0:root:/root:/bin/sh\nalice:x:1000:1000::/home/alice:/bin/sh\n",
            )
            .unwrap();
            Self { root }
        }

        fn proc_root(&self) -> PathBuf {
            self.root.join("proc")
        }

        fn add(&self, pid: u32, comm: &str, uid: u32, fds: &[&str], maps: &str) {
            let dir = self.proc_root().join(pid.to_string());
            fs::create_dir_all(dir.join("fd")).unwrap();
            fs::write(dir.join("comm"), format!("{comm}\n")).unwrap();
            fs::write(dir.join("cmdline"), format!("/usr/bin/{comm}\0--flag\0")).unwrap();
            fs::write(
                dir.join("status"),
                format!("Name:\t{comm}\nUid:\t{uid}\t{uid}\t{uid}\t{uid}\n"),
            )
            .unwrap();
            fs::write(dir.join("maps"), maps).unwrap();
            symlink(format!("/usr/bin/{comm}"), dir.join("exe")).unwrap();
            symlink("/home/alice", dir.join("cwd")).unwrap();
            for (i, target) in fds.iter().enumerate() {
                symlink(target, dir.join("fd").join(i.to_string())).unwrap();
            }
        }

        fn scanner(&self) -> Scanner {
            Scanner {
                proc_root: self.proc_root(),
                passwd_path: self.root.join("passwd"),
                exclude_pids: vec![],
            }
        }
    }

    impl Drop for FakeProc {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    // Use paths that don't exist on the test machine so canonicalize falls back
    // to lexical normalization.
    const DIR: &str = "/nonexistent-mpt/work";

    #[test]
    fn matches_fd_cwd_exe_and_maps() {
        let fake = FakeProc::new("match");
        fake.add(
            100,
            "editor",
            1000,
            &[
                "/dev/null",
                "socket:[1234]",
                "/nonexistent-mpt/work/a.txt",
                "/nonexistent-mpt/work/sub/b.txt (deleted)",
            ],
            "",
        );
        fake.add(
            200,
            "viewer",
            0,
            &["/nonexistent-mpt/other.txt"],
            "7f00-7f10 r--p 00000000 08:01 42   /nonexistent-mpt/work/lib with space.so\n\
             7f10-7f20 rw-p 00000000 00:00 0    [heap]\n",
        );
        fake.add(300, "idle", 1000, &["/nonexistent-mpt/workshop/c.txt"], "");

        let res = fake.scanner().scan(&[PathBuf::from(DIR)]);
        assert_eq!(res.inaccessible, 0);
        let pids: Vec<u32> = res.processes.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![100, 200]);

        let editor = &res.processes[0];
        assert_eq!(editor.name, "editor");
        assert_eq!(editor.user, "alice");
        assert_eq!(editor.uid, Some(1000));
        assert_eq!(editor.cmdline, "/usr/bin/editor --flag");
        assert_eq!(editor.exe, Some(PathBuf::from("/usr/bin/editor")));
        assert_eq!(
            editor.files,
            vec![
                PathBuf::from("/nonexistent-mpt/work/a.txt"),
                PathBuf::from("/nonexistent-mpt/work/sub/b.txt"),
            ]
        );

        let viewer = &res.processes[1];
        assert_eq!(viewer.user, "root");
        assert_eq!(
            viewer.files,
            vec![PathBuf::from("/nonexistent-mpt/work/lib with space.so")]
        );
    }

    #[test]
    fn matches_single_file_and_cwd() {
        let fake = FakeProc::new("file");
        fake.add(10, "a", 1000, &["/nonexistent-mpt/work/a.txt"], "");
        fake.add(11, "b", 1000, &["/nonexistent-mpt/work/a.txt.bak"], "");

        let res = fake
            .scanner()
            .scan(&[PathBuf::from("/nonexistent-mpt/work/./a.txt")]);
        let pids: Vec<u32> = res.processes.iter().map(|p| p.pid).collect();
        assert_eq!(pids, vec![10]);

        // Every fake process has cwd /home/alice.
        let res = fake.scanner().scan(&[PathBuf::from("/home/alice")]);
        assert_eq!(res.processes.len(), 2);
        assert_eq!(res.processes[0].files, vec![PathBuf::from("/home/alice")]);
    }

    #[test]
    fn excluded_and_unknown_uid() {
        let fake = FakeProc::new("exclude");
        fake.add(5, "x", 4242, &["/nonexistent-mpt/work/a"], "");
        fake.add(6, "y", 1000, &["/nonexistent-mpt/work/a"], "");
        let mut scanner = fake.scanner();
        scanner.exclude_pids = vec![6];
        let res = scanner.scan(&[PathBuf::from(DIR)]);
        assert_eq!(res.processes.len(), 1);
        assert_eq!(res.processes[0].user, "4242");
    }

    #[test]
    fn empty_targets_return_nothing() {
        let fake = FakeProc::new("empty");
        fake.add(1, "x", 0, &["/"], "");
        assert!(fake.scanner().scan(&[]).processes.is_empty());
    }

    #[test]
    fn parse_maps_lines() {
        let maps = "55d0-55d1 r--p 00000000 fd:01 1 /usr/bin/cat\n\
                    7ffd-7ffe rw-p 00000000 00:00 0                          [stack]\n\
                    7f00-7f01 r--p 00000000 fd:01 2 /tmp/x (deleted)\n\
                    garbage\n";
        assert_eq!(
            parse_maps(maps),
            vec![
                PathBuf::from("/usr/bin/cat"),
                PathBuf::from("/tmp/x (deleted)")
            ]
        );
    }

    #[test]
    fn parse_helpers() {
        assert_eq!(
            parse_status_uid("Name:\tx\nUid:\t1000\t0\t0\t0\n"),
            Some(1000)
        );
        assert_eq!(parse_status_uid("Name:\tx\n"), None);
        assert_eq!(parse_cmdline(b"a\0b c\0"), "a b c");
        let users = parse_passwd("# c\nroot:x:0:0::/:/bin/sh\nbad\nbob:x:1001:1001::/:/bin/sh\n");
        assert_eq!(users.get(&0).map(String::as_str), Some("root"));
        assert_eq!(users.get(&1001).map(String::as_str), Some("bob"));
        assert_eq!(users.len(), 2);
    }

    #[test]
    fn path_matching_is_component_wise() {
        let t = vec![PathBuf::from("/a/b")];
        assert!(path_matches(Path::new("/a/b"), &t));
        assert!(path_matches(Path::new("/a/b/c"), &t));
        assert!(!path_matches(Path::new("/a/bc"), &t));
        assert!(!path_matches(Path::new("/a"), &t));
    }
}
