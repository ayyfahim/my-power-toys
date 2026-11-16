//! Parity tests for File Locksmith against PowerToys
//! (FileLocksmithLibInterop/FileLocksmith.cpp `find_processes_recursive`,
//! FileLocksmithUI). Uses real child processes holding files open; all files
//! live in a temp directory.

use mpt_file_locksmith::integration::{self, IntegrationPaths};
use mpt_file_locksmith::kill::{self, KillOutcome};
use mpt_file_locksmith::scan::{self, Scanner};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmpdir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("mpt-fl-parity-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::canonicalize(&d).unwrap()
}

struct Kid(Child);
impl Drop for Kid {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn py(script: &str, cwd: &Path) -> Kid {
    let c = Command::new("python3")
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .spawn()
        .expect("python3");
    std::thread::sleep(Duration::from_millis(400));
    Kid(c)
}

fn pids_for(paths: &[PathBuf]) -> Vec<u32> {
    scan::find_processes(paths)
        .processes
        .iter()
        .map(|p| p.pid)
        .collect()
}

fn files_of(paths: &[PathBuf], pid: u32) -> Vec<PathBuf> {
    scan::find_processes(paths)
        .processes
        .into_iter()
        .find(|p| p.pid == pid)
        .map(|p| p.files)
        .unwrap_or_default()
}

#[test]
fn open_fd_on_file_is_found() {
    let d = tmpdir("fd");
    let f = d.join("held.txt");
    fs::write(&f, "x").unwrap();
    let k = py(
        &format!("import time; f=open({:?}); time.sleep(30)", f),
        Path::new("/"),
    );
    let pid = k.0.id();
    assert!(pids_for(std::slice::from_ref(&f)).contains(&pid));
    assert_eq!(files_of(std::slice::from_ref(&f), pid), vec![f.clone()]);
    // A sibling file is not reported.
    let other = d.join("other.txt");
    fs::write(&other, "y").unwrap();
    assert!(!pids_for(&[other]).contains(&pid));
}

#[test]
fn folder_is_searched_recursively() {
    let d = tmpdir("rec");
    let deep = d.join("a/b/c");
    fs::create_dir_all(&deep).unwrap();
    let f = deep.join("deep file.txt");
    fs::write(&f, "x").unwrap();
    let k = py(
        &format!("import time; f=open({:?}); time.sleep(30)", f),
        Path::new("/"),
    );
    let pid = k.0.id();
    assert!(pids_for(std::slice::from_ref(&d)).contains(&pid));
    assert_eq!(files_of(std::slice::from_ref(&d), pid), vec![f]);
}

#[test]
fn cwd_inside_folder_is_found() {
    let d = tmpdir("cwd");
    let sub = d.join("sub");
    fs::create_dir_all(&sub).unwrap();
    let k = py("import time; time.sleep(30)", &sub);
    let pid = k.0.id();
    assert_eq!(files_of(std::slice::from_ref(&d), pid), vec![sub]);
}

#[test]
fn mmap_without_open_fd_is_found() {
    let d = tmpdir("mmap");
    let f = d.join("mapped.bin");
    fs::write(&f, vec![0u8; 8192]).unwrap();
    let k = py(
        &format!(
            "import mmap,os,time\nfd=os.open({:?}, os.O_RDONLY)\nm=mmap.mmap(fd, 0, prot=mmap.PROT_READ)\nos.close(fd)\ntime.sleep(30)",
            f
        ),
        Path::new("/"),
    );
    let pid = k.0.id();
    assert_eq!(files_of(std::slice::from_ref(&f), pid), vec![f]);
}

#[test]
fn deleted_but_open_file_reported_under_folder() {
    let d = tmpdir("del");
    let f = d.join("gone.txt");
    fs::write(&f, "x").unwrap();
    let k = py(
        &format!(
            "import os,time; f=open({:?}); os.unlink({:?}); time.sleep(30)",
            f, f
        ),
        Path::new("/"),
    );
    let pid = k.0.id();
    assert_eq!(files_of(std::slice::from_ref(&d), pid), vec![f]);
}

#[test]
fn own_process_is_excluded() {
    let d = tmpdir("own");
    let f = d.join("mine.txt");
    fs::write(&f, "x").unwrap();
    let _held = fs::File::open(&f).unwrap();
    assert!(!pids_for(std::slice::from_ref(&f)).contains(&std::process::id()));
    // ...but an explicit scanner without exclusions sees us.
    let s = Scanner {
        exclude_pids: vec![],
        ..Scanner::default()
    };
    assert!(
        s.scan(&[f])
            .processes
            .iter()
            .any(|p| p.pid == std::process::id())
    );
}

#[test]
fn trailing_slash_and_relative_dot_paths() {
    let d = tmpdir("norm");
    let f = d.join("x.txt");
    fs::write(&f, "x").unwrap();
    let k = py(
        &format!("import time; f=open({:?}); time.sleep(30)", f),
        Path::new("/"),
    );
    let pid = k.0.id();
    let with_slash = PathBuf::from(format!("{}/", d.display()));
    let with_dot = d.join(".").join("..").join(d.file_name().unwrap());
    assert!(pids_for(&[with_slash]).contains(&pid));
    assert!(pids_for(&[with_dot]).contains(&pid));
}

/// Upstream reports files under the *path the user selected*
/// (`dir_path + kernel_name.substr(...)`). Selecting a folder through a
/// symlink should still find the process and show paths below that folder.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn symlinked_folder_selection() {
    let d = tmpdir("sym");
    let real = d.join("real");
    fs::create_dir_all(&real).unwrap();
    let f = real.join("f.txt");
    fs::write(&f, "x").unwrap();
    let link = d.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let k = py(
        &format!("import time; f=open({:?}); time.sleep(30)", f),
        Path::new("/"),
    );
    let pid = k.0.id();
    assert!(
        pids_for(std::slice::from_ref(&link)).contains(&pid),
        "process not found via symlink"
    );
    assert_eq!(
        files_of(std::slice::from_ref(&link), pid),
        vec![link.join("f.txt")]
    );
}

#[test]
fn process_info_fields() {
    let d = tmpdir("info");
    let f = d.join("i.txt");
    fs::write(&f, "x").unwrap();
    let k = py(
        &format!("import time; f=open({:?}); time.sleep(30)", f),
        Path::new("/"),
    );
    let pid = k.0.id();
    let res = scan::find_processes(&[f]);
    let p = res.processes.iter().find(|p| p.pid == pid).unwrap();
    assert!(p.name.starts_with("python"), "{}", p.name);
    assert!(p.cmdline.contains("time.sleep(30)"));
    assert_eq!(p.uid, Some(unsafe { libc_getuid() }));
    assert!(!p.user.is_empty());
}

unsafe extern "C" {
    #[link_name = "getuid"]
    fn libc_getuid() -> u32;
}

// ── Kill behaviour ─────────────────────────────────────────────────────────

#[test]
fn end_task_terminates_cooperative_process() {
    let d = tmpdir("term");
    let mut k = py("import time; time.sleep(30)", &d);
    let pid = k.0.id();
    assert_eq!(
        kill::end_process(pid, Duration::from_secs(3)).unwrap(),
        KillOutcome::Terminated
    );
    let st = k.0.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(st.signal(), Some(15));
}

#[test]
fn end_task_kills_process_ignoring_sigterm() {
    let d = tmpdir("kill");
    let mut k = py(
        "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(30)",
        &d,
    );
    let pid = k.0.id();
    let t = std::time::Instant::now();
    assert_eq!(
        kill::end_process(pid, Duration::from_millis(300)).unwrap(),
        KillOutcome::Killed
    );
    assert!(t.elapsed() < Duration::from_secs(2));
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(k.0.wait().unwrap().signal(), Some(9));
    // After End task the process disappears from the scan.
    assert!(!pids_for(std::slice::from_ref(&d)).contains(&pid));
}

#[test]
fn end_all_mixed() {
    let d = tmpdir("all");
    let a = py("import time; time.sleep(30)", &d);
    let b = py(
        "import signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); time.sleep(30)",
        &d,
    );
    let found = pids_for(std::slice::from_ref(&d));
    assert!(found.contains(&a.0.id()) && found.contains(&b.0.id()));
    let mut res = kill::end_all(
        &[a.0.id(), b.0.id(), 999_999_999],
        Duration::from_millis(500),
    );
    res.sort_by_key(|(p, _)| *p);
    let outcomes: Vec<_> = res.into_iter().map(|(p, r)| (p, r.unwrap())).collect();
    let mut want = vec![
        (a.0.id(), KillOutcome::Terminated),
        (b.0.id(), KillOutcome::Killed),
        (999_999_999, KillOutcome::NotRunning),
    ];
    want.sort_by_key(|(p, _)| *p);
    assert_eq!(outcomes, want);
}

#[test]
fn already_exited_process_is_not_running() {
    let mut c = Command::new("true").spawn().unwrap();
    let pid = c.id();
    c.wait().unwrap();
    assert_eq!(
        kill::end_process(pid, Duration::ZERO).unwrap(),
        KillOutcome::NotRunning
    );
}

// ── File manager integration ───────────────────────────────────────────────

#[test]
fn nautilus_script_passes_paths_verbatim() {
    let d = tmpdir("naut");
    let bin = d.join("fake bin");
    let out = d.join("args.txt");
    fs::write(
        &bin,
        format!(
            "#!/bin/sh\nfor a in \"$@\"; do printf '%s|' \"$a\"; done > {:?}\n",
            out
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    let paths = IntegrationPaths::new(&d.join("data"), &d.join("config"));
    integration::install_to(&paths, &bin.to_string_lossy()).unwrap();
    let st = Command::new(&paths.nautilus_script)
        .env(
            "NAUTILUS_SCRIPT_SELECTED_FILE_PATHS",
            "/tmp/a b.txt\n/tmp/*\n/tmp/it's $HOME\n",
        )
        .current_dir(&d)
        .status()
        .unwrap();
    assert!(st.success());
    assert_eq!(
        fs::read_to_string(&out).unwrap(),
        "/tmp/a b.txt|/tmp/*|/tmp/it's $HOME|"
    );
    // Fallback: arguments.
    Command::new(&paths.nautilus_script)
        .args(["/x y", "-z"])
        .env_remove("NAUTILUS_SCRIPT_SELECTED_FILE_PATHS")
        .status()
        .unwrap();
    assert_eq!(fs::read_to_string(&out).unwrap(), "/x y|-z|");
}

#[test]
fn dolphin_menu_is_valid_desktop_file() {
    let s = integration::dolphin_service_menu("/opt/my app/mpt-file-locksmith");
    assert!(s.contains("[Desktop Entry]"));
    assert!(s.contains("[Desktop Action fileLocksmith]"));
    assert!(s.contains("Name=Unlock with File Locksmith"));
    assert!(s.contains("Exec=\"/opt/my app/mpt-file-locksmith\" %F"));
    // Folders must be included (upstream shows the entry on folders too).
    assert!(s.contains("MimeType=all/all;") || s.contains("inode/directory"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn thunar_uca_without_trailing_newline_and_self_closing() {
    let user = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions>\n<action><name>Mine</name><unique-id>1</unique-id></action>\n</actions>";
    let with = integration::thunar_insert(Some(user), "/b/mpt").unwrap();
    assert_eq!(integration::thunar_remove(&with), user);
    // Thunar writes `<actions/>` when all actions were deleted.
    let empty = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<actions/>\n";
    assert!(
        integration::thunar_insert(Some(empty), "/b/mpt").is_some(),
        "empty <actions/> document is skipped, so the entry is never installed"
    );
}

#[test]
fn uninstall_keeps_foreign_files() {
    let d = tmpdir("foreign");
    let paths = IntegrationPaths::new(&d.join("data"), &d.join("config"));
    fs::create_dir_all(paths.nautilus_script.parent().unwrap()).unwrap();
    fs::write(&paths.nautilus_script, "#!/bin/sh\necho user script\n").unwrap();
    integration::uninstall_from(&paths).unwrap();
    assert!(paths.nautilus_script.exists());
    integration::install_to(&paths, "/b/mpt").unwrap();
    integration::uninstall_from(&paths).unwrap();
    assert!(!paths.nautilus_script.exists() && !paths.dolphin_service_menu.exists());
}
