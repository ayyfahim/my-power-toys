//! Parity tests for New+ against PowerToys
//! (NewShellExtensionContextMenu: template_item.cpp, helpers_variables.h,
//! helpers_filesystem.h, new_utilities.h `copy_template`, template_folder.cpp,
//! PowerRename Helpers.cpp `GetDatedFileName`).
//!
//! Everything runs in temp directories; the end-to-end test points HOME,
//! XDG_DATA_HOME and XDG_CONFIG_HOME at a temp dir.

use chrono::{NaiveDate, TimeZone, Utc};
use mpt_new_plus::NewPlusConfig;
use mpt_new_plus::integration;
use mpt_new_plus::templates::{self, Template};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmpdir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("mpt-np-parity-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn cfg(hide_ext: bool, hide_digits: bool, vars: bool) -> NewPlusConfig {
    NewPlusConfig {
        hide_file_extension: hide_ext,
        hide_starting_digits: hide_digits,
        replace_variables: vars,
        ..Default::default()
    }
}

fn tpl(name: &str, is_dir: bool) -> Template {
    Template {
        path: PathBuf::from("/tpl").join(name),
        file_name: name.into(),
        is_dir,
    }
}

fn t0() -> chrono::DateTime<Utc> {
    Utc.from_utc_datetime(
        &NaiveDate::from_ymd_opt(2024, 3, 5)
            .unwrap()
            .and_hms_milli_opt(0, 7, 9, 5)
            .unwrap(),
    )
}

// ── Menu title / target name (template_item.cpp) ───────────────────────────

/// Full table from `remove_starting_digits_from_filename` plus pattern 5
/// ("01Name" -> "Name").
#[test]
fn starting_digits_table() {
    let rows = [
        ("01. First entry.txt", false, "First entry.txt"),
        ("02. Second entry", true, "Second entry"),
        ("03 Third entry", true, "Third entry"),
        ("04 Fourth entry.txt", false, "Fourth entry.txt"),
        ("05.Fifth entry.txt", false, "Fifth entry.txt"),
        ("001231", true, "001231"),
        ("001231.txt", false, "001231.txt"),
        ("13. 0123456789012345.txt", false, "0123456789012345.txt"),
        ("0123.Name", true, "Name"),
        ("01Name.txt", false, "Name.txt"),
        ("06 .Sixth.txt", false, "Sixth.txt"),
    ];
    for (name, dir, want) in rows {
        assert_eq!(templates::strip_starting_digits(name, dir), want, "{name}");
    }
}

#[test]
fn menu_title_options() {
    let t = tpl("01. Report.docx", false);
    assert_eq!(t.display_name(&cfg(true, true, false)), "Report");
    assert_eq!(t.display_name(&cfg(false, true, false)), "Report.docx");
    assert_eq!(t.display_name(&cfg(true, false, false)), "01. Report");
    assert_eq!(t.display_name(&cfg(false, false, false)), "01. Report.docx");
    // Folders never lose a "extension".
    let f = tpl("02. My.Project", true);
    assert_eq!(f.display_name(&cfg(true, true, false)), "My.Project");
    // Target name keeps the extension even when hidden in the menu.
    assert_eq!(t.target_name(&cfg(true, true, false)), "Report.docx");
    assert_eq!(t.target_name(&cfg(true, false, false)), "01. Report.docx");
}

/// Upstream `get_menu_title(..., show_resolved_variables)` previews resolved
/// variables in the menu when "Replace variables" is on.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn menu_title_previews_variables() {
    let t = tpl("$YYYY report.txt", false);
    let title = t.display_name(&cfg(true, true, true));
    assert!(!title.contains("$YYYY"), "menu title not resolved: {title}");
}

// ── Variables (helpers_variables.h / GetDatedFileName) ─────────────────────

#[test]
fn date_time_tokens() {
    let t = t0();
    assert_eq!(
        templates::resolve_date_time("$YYYY-$MM-$DD $hh.$mm.$ss.$fff", &t),
        "2024-03-05 00.07.09.005"
    );
    // 12-hour clock at midnight is 12 AM.
    assert_eq!(
        templates::resolve_date_time("$HH $H $TT $tt", &t),
        "12 12 AM am"
    );
    assert_eq!(templates::resolve_date_time("$ff $f", &t), "00 0");
    assert_eq!(
        templates::resolve_date_time("$$YYYY $$$YYYY", &t),
        "$$YYYY $$2024"
    );
}

/// Upstream negative lookaheads keep metadata-like tokens literal:
/// `$D(?!ATE_TAKEN_|ESCRIPTION|OCUMENT_ID)`, `$H(?!EIGHT)`.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn date_tokens_do_not_eat_metadata_names() {
    let t = t0();
    assert_eq!(
        templates::resolve_date_time("$DESCRIPTION", &t),
        "$DESCRIPTION"
    );
    assert_eq!(templates::resolve_date_time("$HEIGHT", &t), "$HEIGHT");
    assert_eq!(
        templates::resolve_date_time("$DATE_TAKEN_YYYY", &t),
        "$DATE_TAKEN_YYYY"
    );
}

#[test]
fn env_variables() {
    let look = |v: &str| match v {
        "USERNAME" => Some("alice".to_string()),
        "EMPTY" => Some(String::new()),
        _ => None,
    };
    assert_eq!(
        templates::resolve_env("%USERNAME%-%NOPE%-%EMPTY%.txt", look),
        "alice-%NOPE%-%EMPTY%.txt"
    );
    assert_eq!(templates::resolve_env("50%%USERNAME%", look), "50%alice");
    assert_eq!(templates::resolve_env("%%", look), "%%");
}

#[test]
fn parent_folder_and_invalid_chars() {
    let t = t0();
    assert_eq!(
        templates::resolve_variables("$PARENT_FOLDER_NAME notes.md", "proj", &t),
        "proj notes.md"
    );
    // Case-sensitive like upstream.
    assert_eq!(
        templates::resolve_variables("$parent_folder_name.md", "proj", &t),
        "$parent_folder_name.md"
    );
    // A '/' coming from a variable must not create a sub-path.
    assert_eq!(templates::resolve_variables("a/b", "", &t), "a b");
}

// ── Name conflicts (helpers_filesystem.h make_unique_path_name) ────────────

/// Upstream starts counting at 1: `Name (1).txt`, `Name (2).txt`, ...
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn unique_name_counter_starts_at_one() {
    let d = tmpdir("uniq");
    fs::write(d.join("Note.txt"), "").unwrap();
    let p = templates::unique_path(&d, "Note.txt", false, |p| p.exists());
    assert_eq!(p, d.join("Note (1).txt"));
}

/// Upstream uses `stem()`/`extension()` for folders too.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn unique_folder_name_with_dot() {
    let d = tmpdir("uniqdir");
    fs::create_dir(d.join("my.project")).unwrap();
    let p = templates::unique_path(&d, "my.project", true, |p| p.exists());
    assert_eq!(p, d.join("my (1).project"));
}

// ── Creation ───────────────────────────────────────────────────────────────

#[test]
fn create_file_and_folder_with_variables() {
    let d = tmpdir("create");
    let tpl_dir = d.join("tpl");
    let target = d.join("proj");
    fs::create_dir_all(&target).unwrap();
    fs::create_dir_all(tpl_dir.join("01. $PARENT_FOLDER_NAME kit/$YYYY sub")).unwrap();
    fs::write(
        tpl_dir.join("01. $PARENT_FOLDER_NAME kit/$YYYY sub/%NOPE% $PARENT_FOLDER_NAME.txt"),
        "deep",
    )
    .unwrap();
    fs::write(
        tpl_dir.join("01. $PARENT_FOLDER_NAME kit/.hidden $YYYY"),
        "h",
    )
    .unwrap();
    fs::write(tpl_dir.join("02. $YYYY-$MM-$DD log.txt"), "log").unwrap();
    let c = cfg(true, true, true);
    let year = chrono::Local::now().format("%Y").to_string();

    let f =
        templates::create_from_template(&tpl_dir.join("02. $YYYY-$MM-$DD log.txt"), &target, &c)
            .unwrap();
    let fname = f.file_name().unwrap().to_string_lossy().to_string();
    assert!(
        fname.starts_with(&year) && fname.ends_with(" log.txt"),
        "{fname}"
    );
    assert_eq!(fs::read_to_string(&f).unwrap(), "log");

    let k =
        templates::create_from_template(&tpl_dir.join("01. $PARENT_FOLDER_NAME kit"), &target, &c)
            .unwrap();
    assert_eq!(k, target.join("proj kit"));
    assert!(k.join(format!("{year} sub")).is_dir());
    assert!(
        k.join(".hidden $YYYY").is_file(),
        "hidden entries are not renamed"
    );
    let leaf = fs::read_dir(k.join(format!("{year} sub")))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .file_name();
    assert!(leaf.to_string_lossy().starts_with("%NOPE% "), "{leaf:?}");

    // Variables off: names kept verbatim (digits still stripped).
    let raw = templates::create_from_template(
        &tpl_dir.join("02. $YYYY-$MM-$DD log.txt"),
        &target,
        &cfg(true, true, false),
    )
    .unwrap();
    assert_eq!(raw, target.join("$YYYY-$MM-$DD log.txt"));
}

/// Upstream `update_last_write_time`: created items get "now" as mtime.
#[test]
fn created_files_get_current_mtime() {
    let d = tmpdir("mtime");
    let tpl_dir = d.join("tpl");
    fs::create_dir_all(tpl_dir.join("Folder")).unwrap();
    fs::write(tpl_dir.join("Folder/inner.txt"), "x").unwrap();
    fs::write(tpl_dir.join("old.txt"), "x").unwrap();
    for p in ["old.txt", "Folder/inner.txt", "Folder"] {
        assert!(
            Command::new("touch")
                .args(["-d", "2001-01-01"])
                .arg(tpl_dir.join(p))
                .status()
                .unwrap()
                .success()
        );
    }
    let target = d.join("t");
    fs::create_dir_all(&target).unwrap();
    let c = cfg(true, true, false);
    let a = templates::create_from_template(&tpl_dir.join("old.txt"), &target, &c).unwrap();
    let b = templates::create_from_template(&tpl_dir.join("Folder"), &target, &c).unwrap();
    let recent = |p: &Path| {
        fs::metadata(p)
            .unwrap()
            .modified()
            .unwrap()
            .elapsed()
            .unwrap()
            .as_secs()
            < 60
    };
    assert!(recent(&a), "file mtime copied from template");
    assert!(
        recent(&b.join("inner.txt")),
        "inner file mtime copied from template"
    );
}

#[test]
fn template_listing_order_and_hidden() {
    let d = tmpdir("list");
    fs::create_dir_all(d.join("b folder")).unwrap();
    fs::create_dir_all(d.join("a folder")).unwrap();
    fs::write(d.join("z.txt"), "").unwrap();
    fs::write(d.join("01. a.txt"), "").unwrap();
    fs::write(d.join(".hidden"), "").unwrap();
    let names: Vec<String> = templates::list_templates(&d)
        .unwrap()
        .into_iter()
        .map(|t| t.file_name)
        .collect();
    assert_eq!(names, ["a folder", "b folder", "01. a.txt", "z.txt"]);
}

#[test]
fn default_settings_match_upstream() {
    let c = NewPlusConfig::default();
    assert!(c.hide_file_extension);
    assert!(c.hide_starting_digits);
    assert!(!c.replace_variables);
}

#[test]
fn desktop_exec_quoting() {
    assert_eq!(
        integration::desktop_exec_arg("/tpl/50% off $x.txt"),
        "\"/tpl/50%% off \\\\$x.txt\""
    );
}

// ── End to end: integration files + generated scripts + CLI ────────────────

fn run_bin(home: &Path, args: &[&str], envs: &[(&str, &str)]) -> std::process::Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_mpt-new-plus"));
    c.args(args)
        .env("HOME", home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env("XDG_RUNTIME_DIR", home);
    for (k, v) in envs {
        c.env(k, v);
    }
    c.output().unwrap()
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn end_to_end_integration_files() {
    let home = tmpdir("home");
    // SAFETY: the only test in this binary that changes process env vars.
    unsafe {
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_DATA_HOME", home.join(".local/share"));
        std::env::set_var("XDG_CONFIG_HOME", home.join(".config"));
    }
    let bin = PathBuf::from(env!("CARGO_BIN_EXE_mpt-new-plus"));
    let config = NewPlusConfig::default();
    let tdir = config.templates_dir();
    assert!(tdir.starts_with(&home));
    templates::ensure_templates_dir(&tdir).unwrap();
    fs::write(tdir.join("02. Meeting notes.md"), "# notes\n").unwrap();

    let ndir = home.join(".local/share/nautilus/scripts/New+");
    fs::create_dir_all(&ndir).unwrap();
    fs::write(ndir.join("My own script"), "#!/bin/sh\necho mine\n").unwrap();

    integration::apply(&config, &bin).unwrap();
    let mut scripts: Vec<String> = fs::read_dir(&ndir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    scripts.sort();
    assert!(
        scripts.contains(&"Meeting notes".to_string()),
        "{scripts:?}"
    );
    assert!(scripts.contains(&"Example folder".to_string()));
    assert!(scripts.contains(&"Text document".to_string()));
    assert!(scripts.contains(&"Open templates folder".to_string()));

    // Run the generated Nautilus script like Nautilus would.
    let target = home.join("My Docs");
    fs::create_dir_all(&target).unwrap();
    let st = Command::new(ndir.join("Meeting notes"))
        .env("HOME", &home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .env("XDG_CONFIG_HOME", home.join(".config"))
        .env(
            "NAUTILUS_SCRIPT_CURRENT_URI",
            format!("file://{}", target.display()).replace(' ', "%20"),
        )
        .output()
        .unwrap();
    assert!(
        st.status.success(),
        "{}",
        String::from_utf8_lossy(&st.stderr)
    );
    assert_eq!(
        fs::read_to_string(target.join("Meeting notes.md")).unwrap(),
        "# notes\n"
    );

    // Dolphin service menu.
    let dolphin = home.join(".local/share/kio/servicemenus/mpt-new-plus.desktop");
    let desk = fs::read_to_string(&dolphin).unwrap();
    assert!(desk.contains("X-KDE-Submenu=New+"));
    assert!(desk.contains("Name=Meeting notes"));
    assert!(desk.contains("MimeType=inode/directory;"));
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&dolphin).unwrap().permissions().mode() & 0o111,
        0o111
    );

    // Template added later appears after re-apply; removed one disappears.
    fs::remove_file(tdir.join("02. Meeting notes.md")).unwrap();
    integration::apply(&config, &bin).unwrap();
    assert!(!ndir.join("Meeting notes").exists());

    // Disabled context menu removes the integration.
    integration::apply(
        &NewPlusConfig {
            context_menu: false,
            ..config.clone()
        },
        &bin,
    )
    .unwrap();
    assert!(!dolphin.exists());
    assert!(!ndir.join("Example folder").exists());

    // Stop removes everything generated but keeps the user's own script.
    integration::apply(&config, &bin).unwrap();
    integration::remove_all();
    assert!(!dolphin.exists());
    assert_eq!(fs::read_dir(&ndir).unwrap().count(), 1);
    assert!(ndir.join("My own script").exists());

    // XDG Templates sync creates and removes symlinks.
    let xdg_cfg = NewPlusConfig {
        sync_xdg_templates: true,
        ..config.clone()
    };
    integration::apply(&xdg_cfg, &bin).unwrap();
    let xdg = integration::xdg_templates_dir().unwrap();
    assert!(
        xdg.join("Text document.txt")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    integration::remove_all();
    assert!(xdg.join("Text document.txt").symlink_metadata().is_err());

    // CLI: create twice -> upstream conflict naming "Text document (1).txt".
    let out = run_bin(
        &home,
        &["create", "Text document", &target.to_string_lossy()],
        &[],
    );
    assert!(out.status.success());
    let out = run_bin(
        &home,
        &["create", "Text document", &target.to_string_lossy()],
        &[],
    );
    let second = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let _ = fs::remove_dir_all(&home);
    assert_eq!(
        second,
        target.join("Text document (1).txt").to_string_lossy()
    );
}

/// Thunar is one of the file managers listed for New+; upstream integrates
/// with the file manager's "New" menu. Thunar has no service-menu mechanism
/// besides custom actions (uca.xml) and ~/Templates.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn thunar_integration_exists() {
    let src = include_str!("../src/integration.rs");
    assert!(
        src.contains("Thunar") || src.contains("uca.xml"),
        "no Thunar integration (only via optional XDG Templates sync)"
    );
}
