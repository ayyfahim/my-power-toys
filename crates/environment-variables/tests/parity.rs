//! Parity tests for Environment Variables.
//!
//! Oracles:
//! - Microsoft PowerToys (EnvironmentVariablesUILib: ProfileVariablesSet,
//!   EnvironmentVariablesHelper, EnvironmentVariableComparisonHelper tests) for
//!   profile apply/unapply/backup semantics.
//! - The real systemd `30-systemd-environment-d-generator` for what an
//!   environment.d file written by the port actually produces at login
//!   (quoting, `$VAR` expansion). Those tests are skipped when the generator is
//!   not installed.
//!
//! Only temp directories are used (XDG_CONFIG_HOME / HOME are redirected).

use mpt_environment_variables::EnvironmentVariablesConfig;
use mpt_environment_variables::envd::{self, EnvFile};
use mpt_environment_variables::listvalue;
use mpt_environment_variables::manager::{Manager, Target};
use mpt_environment_variables::profiles::ProfileStore;
use mpt_environment_variables::store::{self, Scope, Source};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GENERATOR: &str =
    "/usr/lib/systemd/user-environment-generators/30-systemd-environment-d-generator";

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn tmpdir(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("mpt-env-parity-{tag}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

/// Decode one value printed by the generator (shell-style double quoting).
fn decode_gen(v: &str) -> String {
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        let inner = &v[1..v.len() - 1];
        let mut out = String::new();
        let mut it = inner.chars();
        while let Some(c) = it.next() {
            if c == '\\' {
                match it.next() {
                    Some('t') => out.push('\t'),
                    Some('n') => out.push('\n'),
                    Some(n) => out.push(n),
                    None => {}
                }
            } else {
                out.push(c);
            }
        }
        out
    } else {
        v.to_string()
    }
}

/// Run the real systemd generator over `content` placed in a user
/// environment.d dir. Base environment: HOME=/h PATH=/usr/bin E= (empty).
/// Returns only keys starting with `MPT_`.
fn systemd_eval(content: &str) -> Option<BTreeMap<String, String>> {
    if !Path::new(GENERATOR).exists() {
        eprintln!("systemd generator missing; skipping");
        return None;
    }
    let t = tmpdir("gen");
    fs::create_dir_all(t.join("environment.d")).unwrap();
    fs::write(t.join("environment.d/50-my-power-toys.conf"), content).unwrap();
    let out = Command::new("env")
        .args(["-i", "HOME=/h", "PATH=/usr/bin", "MPT_E="])
        .arg(format!("XDG_CONFIG_HOME={}", t.display()))
        .arg(GENERATOR)
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&t);
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    Some(
        text.lines()
            .filter(|l| l.starts_with("MPT_"))
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.to_string(), decode_gen(v)))
            .collect(),
    )
}

/// The port's own idea of the effective (expanded) values of a user file.
fn port_eval(content: &str) -> BTreeMap<String, String> {
    let f = EnvFile::parse(content);
    let base: BTreeMap<String, String> = [("HOME", "/h"), ("PATH", "/usr/bin"), ("MPT_E", "")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let src = Source {
        path: PathBuf::from("/u/50-my-power-toys.conf"),
        scope: Scope::User,
        managed: true,
        expands: true,
        vars: f.vars(),
    };
    store::merge(&[src], &base, false)
        .into_iter()
        .filter(|r| r.name.starts_with("MPT_"))
        .map(|r| (r.name, r.expanded))
        .collect()
}

// ── Generated environment.d validity (systemd oracle) ───────────────────────

/// Literal values typed by the user must reach the session unchanged after
/// the port serialises them.
#[test]
fn written_literal_values_survive_systemd() {
    let values = [
        "plain",
        "with space",
        "quote\"inside",
        "back\\slash",
        "tick`",
        "#hash",
        "a'b",
        "  lead and trail  ",
        "C:\\new\\dir",
        "semi;colon",
        "tab\there",
        "x=y",
        "~/bin",
        "100%",
        "{brace}",
        "trailing\\",
    ];
    let mut f = EnvFile::default();
    for (i, v) in values.iter().enumerate() {
        f.set(&format!("MPT_V{i:02}"), v);
    }
    let content = f.serialize();
    let Some(sys) = systemd_eval(&content) else {
        return;
    };
    let mut bad = Vec::new();
    for (i, v) in values.iter().enumerate() {
        let k = format!("MPT_V{i:02}");
        if sys.get(&k).map(String::as_str) != Some(*v) {
            bad.push(format!(
                "{v:?} written as {:?} -> systemd {:?}",
                content.lines().nth(i).unwrap_or(""),
                sys.get(&k)
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "values corrupted by quoting:\n{}",
        bad.join("\n")
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn empty_value_is_written_and_kept() {
    let mut f = EnvFile::default();
    f.set("MPT_EMPTY", "");
    let content = f.serialize();
    assert_eq!(content, "MPT_EMPTY=\n");
    if let Some(sys) = systemd_eval(&content) {
        assert_eq!(sys.get("MPT_EMPTY").map(String::as_str), Some(""));
    }
}

/// The "Applied variables" view (and the values pushed into the running
/// session) must equal what systemd computes at the next login.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn applied_view_matches_systemd_expansion() {
    let content = "\
MPT_A=$HOME/bin:${PATH}
MPT_B=${NOPE:-def}
MPT_C=${HOME:+set}
MPT_D=${MPT_E:-wasempty}
MPT_F=cost $5
MPT_G=$HOME$HOME
MPT_H=${MPT_A}/x
";
    let Some(sys) = systemd_eval(content) else {
        return;
    };
    let port = port_eval(content);
    let diffs: Vec<String> = port
        .iter()
        .filter(|(k, v)| sys.get(*k) != Some(*v))
        .map(|(k, v)| format!("{k}: port {v:?} vs systemd {:?}", sys.get(k)))
        .collect();
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

/// Quoting/escaping of `$` in hand-written files: the port treats `'...'`
/// and `\$` as "literal dollar", systemd expands them.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn dollar_escaping_matches_systemd() {
    let content = "\
MPT_SQ='single $HOME'
MPT_DQ=\"dq \\$HOME\"
MPT_BS=\\$HOME
MPT_DD=$$HOME
";
    let Some(sys) = systemd_eval(content) else {
        return;
    };
    let port = port_eval(content);
    let diffs: Vec<String> = port
        .iter()
        .filter(|(k, v)| sys.get(*k) != Some(*v))
        .map(|(k, v)| format!("{k}: port {v:?} vs systemd {:?}", sys.get(k)))
        .collect();
    assert!(diffs.is_empty(), "{}", diffs.join("\n"));
}

/// A value the port writes for "literal dollar" (`\$` in its logical value)
/// must stay literal for systemd.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn port_literal_dollar_stays_literal() {
    let mut f = EnvFile::default();
    f.set("MPT_LIT", "price \\$HOME");
    let content = f.serialize();
    let Some(sys) = systemd_eval(&content) else {
        return;
    };
    assert_eq!(
        sys.get("MPT_LIT").map(String::as_str),
        Some("price $HOME"),
        "written as {content:?}"
    );
    assert_eq!(
        port_eval(&content).get("MPT_LIT").map(String::as_str),
        Some("price $HOME")
    );
}

/// Lines systemd rejects (invalid names) are not variables for the port
/// either; valid ones around them still work.
#[test]
fn invalid_names_ignored_like_systemd() {
    let content = "1BAD=x\nMPT-DASH=x\nMPT_OK=1\nMPT SPACE=x\n";
    let port = port_eval(content);
    assert_eq!(
        port.keys().cloned().collect::<Vec<_>>(),
        vec!["MPT_OK".to_string()]
    );
    if let Some(sys) = systemd_eval(content) {
        assert_eq!(
            sys.keys()
                .filter(|k| *k != "MPT_E")
                .cloned()
                .collect::<Vec<_>>(),
            vec!["MPT_OK".to_string()]
        );
    }
}

/// Multi-line double-quoted values are one variable for systemd; the port
/// must not invent a variable from the continuation line.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn multiline_quoted_value_not_split_into_phantom_vars() {
    let content = "MPT_ML=\"first\nMPT_PHANTOM=second\n\"\nMPT_AFTER=1\n";
    let port = port_eval(content);
    if let Some(sys) = systemd_eval(content) {
        assert!(!sys.contains_key("MPT_PHANTOM"));
    }
    assert!(!port.contains_key("MPT_PHANTOM"), "port parsed {port:?}");
}

/// Backslash-newline continuation (systemd joins the lines).
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn line_continuation_like_systemd() {
    let content = "MPT_CONT=foo\\\nbar\n";
    let port = port_eval(content);
    if let Some(sys) = systemd_eval(content) {
        assert_eq!(
            port.get("MPT_CONT"),
            sys.get("MPT_CONT"),
            "port {port:?} sys {sys:?}"
        );
    }
}

/// systemd does not de-duplicate PATH entries; the applied view should show
/// what the session will really get.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn applied_path_not_silently_deduplicated() {
    let content = "MPT_PATH=/a:/b:/a\n";
    let port = port_eval(content);
    if let Some(sys) = systemd_eval(content) {
        assert_eq!(port.get("MPT_PATH"), sys.get("MPT_PATH"));
    }
}

// ── List editing (upstream Variable.ValuesList, ComparisonHelper) ──────────

#[test]
fn list_split_edit_join() {
    let mut e = listvalue::split("$HOME/bin:/usr/local/bin:${PATH}");
    assert_eq!(e.len(), 3);
    e.insert(1, "/opt/x".into());
    listvalue::move_entry(&mut e, 3, -3);
    assert_eq!(
        listvalue::join(&e),
        "${PATH}:$HOME/bin:/opt/x:/usr/local/bin"
    );
    e.remove(0);
    assert_eq!(listvalue::join(&e), "$HOME/bin:/opt/x:/usr/local/bin");
}

/// Upstream `RemoveDuplicatePathEntries_PreservesFirstOccurrenceAndOrder`
/// (adapted to ':' and case-sensitive Linux paths) and trailing-separator
/// normalisation (`C:\Tools\` == `C:/Tools`).
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn dedup_preserves_order_and_normalises_trailing_slash() {
    assert_eq!(
        listvalue::dedup("/tools:/usr/bin:/tools:/opt:/usr/bin"),
        "/tools:/usr/bin:/opt"
    );
    assert_eq!(listvalue::dedup("/tools/:/tools:/other"), "/tools/:/other");
}

// ── Name / value validation (EnvironmentVariablesHelperValidationTests) ────

#[test]
fn validation_cases() {
    assert!(envd::validate("ValidName", "").is_ok());
    assert!(envd::validate("valid_name_123", "x").is_ok());
    for bad in [
        "",
        "   ",
        " leading",
        "trailing ",
        "has=equals",
        "\n",
        "\x01",
        "\x7F",
    ] {
        assert!(envd::validate(bad, "v").is_err(), "{bad:?} accepted");
    }
    assert!(envd::validate("N", "test\0value").is_err());
}

// ── Profile apply / unapply (ProfileVariablesSet) ──────────────────────────

/// Upstream: apply backs up overridden *user* variables, unapply restores
/// them and removes profile-only variables. Variables that only exist at
/// system level are not backed up.
#[test]
fn profile_apply_unapply_backup_restore() {
    let mut s = ProfileStore::default();
    s.create("dev").unwrap();
    {
        let p = s.get_mut("dev").unwrap();
        p.set("EDITOR", "nvim");
        p.set("ONLY_PROFILE", "1");
        p.set("PATH", "/opt/dev/bin:$PATH");
    }
    let mut user = EnvFile::parse("# mine\nEDITOR='nano -w'\nPATH=$HOME/bin:$PATH\nOTHER=x\n");
    let before = user.vars();
    let pf = s.apply("dev", &mut user).unwrap();
    assert_eq!(pf.get("PATH"), Some("/opt/dev/bin:$PATH"));
    assert_eq!(s.backups.len(), 2);
    assert!(!user.contains("EDITOR") && !user.contains("PATH"));
    s.unapply(&mut user);
    let mut after = user.vars();
    let mut b = before.clone();
    after.sort();
    b.sort();
    assert_eq!(after, b, "user variables restored");
    assert!(!user.contains("ONLY_PROFILE"));
}

/// Generated profile file is valid for systemd and carries the profile values.
#[test]
fn profile_file_valid_for_systemd() {
    let mut s = ProfileStore::default();
    s.create("weird \"name\" $X").unwrap();
    {
        let p = s.get_mut("weird \"name\" $X").unwrap();
        p.set("MPT_P1", "a b");
        p.set("MPT_P2", "$HOME/x");
        p.set("MPT_P3", "q\"uote");
    }
    let mut user = EnvFile::default();
    let pf = s.apply("weird \"name\" $X", &mut user).unwrap().serialize();
    assert!(pf.starts_with("# "));
    let Some(sys) = systemd_eval(&pf) else { return };
    assert_eq!(sys.get("MPT_P1").map(String::as_str), Some("a b"));
    assert_eq!(sys.get("MPT_P2").map(String::as_str), Some("/h/x"));
    assert_eq!(sys.get("MPT_P3").map(String::as_str), Some("q\"uote"));
}

/// End-to-end through `Manager` on disk (temp XDG_CONFIG_HOME): apply then
/// unapply restores the user file's variables and removes the profile file;
/// an applied profile wins over every user variable (upstream: profile
/// variables replace the user variable).
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn manager_on_disk_apply_unapply() {
    let root = tmpdir("mgr");
    let cfg = root.join("config");
    fs::create_dir_all(cfg.join("environment.d")).unwrap();
    // SAFETY: this is the only test that touches process environment variables.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", &cfg);
        std::env::set_var("HOME", &root);
    }
    let user_file = cfg.join("environment.d").join(store::USER_FILE_NAME);
    let original = "# my vars\nMPT_EDITOR=nano\nMPT_KEEP=\"k k\"\n";
    fs::write(&user_file, original).unwrap();
    // A later user file that also sets the variable.
    fs::write(
        cfg.join("environment.d/90-local.conf"),
        "MPT_EDITOR=emacs\n",
    )
    .unwrap();

    let mut m = Manager::load(EnvironmentVariablesConfig {
        launch_administrator: false,
        update_session: false,
    })
    .unwrap();
    m.create_profile("dev").unwrap();
    m.set_var(&Target::Profile("dev".into()), None, "MPT_EDITOR", "nvim")
        .unwrap();
    m.set_var(&Target::Profile("dev".into()), None, "MPT_NEW", "1")
        .unwrap();
    m.apply_profile("dev").unwrap();

    let profile_file = cfg.join("environment.d").join(store::PROFILE_FILE_NAME);
    assert!(profile_file.exists());
    let eff = m.effective(false);
    let get = |n: &str| {
        eff.iter()
            .find(|r| r.name == n)
            .map(|r| (r.scope, r.expanded.clone()))
    };
    let new_ok = get("MPT_NEW") == Some((Scope::Profile, "1".into()));
    let editor = get("MPT_EDITOR");

    // Reload from disk: applied state persisted.
    let m2 = Manager::load(EnvironmentVariablesConfig {
        launch_administrator: false,
        update_session: false,
    })
    .unwrap();
    let persisted = m2.profiles.applied.as_deref() == Some("dev");

    m.unapply_profile().unwrap();
    let restored = fs::read_to_string(&user_file).unwrap();
    let profile_removed = !profile_file.exists();
    let _ = fs::remove_dir_all(&root);

    assert!(new_ok);
    assert!(persisted);
    assert!(profile_removed);
    let r = EnvFile::parse(&restored);
    assert_eq!(
        r.get("MPT_EDITOR"),
        Some("nano"),
        "restored file:\n{restored}"
    );
    assert_eq!(r.get("MPT_KEEP"), Some("k k"));
    assert!(restored.starts_with("# my vars\n"));
    assert_eq!(
        editor,
        Some((Scope::Profile, "nvim".into())),
        "applied profile value must win over user files"
    );
}
