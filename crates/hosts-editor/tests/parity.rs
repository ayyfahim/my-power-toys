//! Parity tests for the Hosts Editor against Microsoft PowerToys
//! (src/modules/Hosts/HostsUILib + Hosts.Tests). PowerToys behaviour is the
//! oracle; a failing test is a parity bug. Only temp files are used; the real
//! /etc/hosts is never touched.

use mpt_hosts_editor::backup::{self, BACKUP_MARKER};
use mpt_hosts_editor::config::{DeleteBackupsMode, HostsEditorConfig};
use mpt_hosts_editor::{HostEntry, HostsFile, HostsLine, WriteOptions, find_duplicates};
use std::fs;
use std::path::PathBuf;

/// Parse a single line; `Some((ip, hosts, comment, active))` when the port
/// considers it a host entry.
fn one(line: &str) -> Option<(String, String, String, bool)> {
    let f = HostsFile::parse_str(line);
    f.entries().first().map(|e| {
        (
            e.ip.clone(),
            e.hostnames.join(" "),
            e.comment.clone().unwrap_or_default(),
            e.enabled,
        )
    })
}

fn check_rows(rows: &[(&str, &str, &str, &str, bool)]) {
    let mut failures = Vec::new();
    for (line, ip, hosts, comment, active) in rows {
        let want = Some((
            ip.to_string(),
            hosts.to_string(),
            comment.to_string(),
            *active,
        ));
        let got = one(line);
        if got != want {
            failures.push(format!("{line:?}: got {got:?}, want {want:?}"));
        }
    }
    assert!(failures.is_empty(), "mismatches:\n{}", failures.join("\n"));
}

// ── Ported from Hosts.Tests/EntryTest.cs ────────────────────────────────────

#[test]
fn entrytest_valid_entry_single_host() {
    check_rows(&[
        ("\t\t10.1.1.1\t\thost\t\t", "10.1.1.1", "host", "", true),
        ("  10.1.1.1  host  ", "10.1.1.1", "host", "", true),
        ("10.1.1.1 host", "10.1.1.1", "host", "", true),
        ("\t\t#\t\t10.1.1.1\thost\t\t", "10.1.1.1", "host", "", false),
        ("  #  10.1.1.1  host  ", "10.1.1.1", "host", "", false),
        ("#10.1.1.1 host", "10.1.1.1", "host", "", false),
        (
            "\t\t10.1.1.1\t\thost\t\t#\t\tcomment\t\t",
            "10.1.1.1",
            "host",
            "comment",
            true,
        ),
        (
            "  10.1.1.1  host  #  comment  ",
            "10.1.1.1",
            "host",
            "comment",
            true,
        ),
        ("10.1.1.1 host#comment", "10.1.1.1", "host", "comment", true),
        (
            "\t\t#\t\t10.1.1.1\thost\t\t#\t\tcomment\t\t",
            "10.1.1.1",
            "host",
            "comment",
            false,
        ),
        (
            "  #  10.1.1.1  host  #  comment  ",
            "10.1.1.1",
            "host",
            "comment",
            false,
        ),
        (
            "#10.1.1.1 host#comment",
            "10.1.1.1",
            "host",
            "comment",
            false,
        ),
    ]);
}

/// Upstream `Line.TrimStart(' ', '#')` strips repeated `#` markers.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn entrytest_valid_entry_repeated_hash_prefix() {
    check_rows(&[
        (
            "# #10.1.1.1 host#comment",
            "10.1.1.1",
            "host",
            "comment",
            false,
        ),
        (
            "# #\t10.1.1.1 host#comment",
            "10.1.1.1",
            "host",
            "comment",
            false,
        ),
        (
            "# # \t10.1.1.1 host#comment",
            "10.1.1.1",
            "host",
            "comment",
            false,
        ),
    ]);
}

#[test]
fn entrytest_valid_entry_multiple_hosts() {
    check_rows(&[
        (
            "\t\t10.1.1.1\t\thost host.local\t\t",
            "10.1.1.1",
            "host host.local",
            "",
            true,
        ),
        (
            "  10.1.1.1  host  host.local  ",
            "10.1.1.1",
            "host host.local",
            "",
            true,
        ),
        (
            "10.1.1.1 host host.local",
            "10.1.1.1",
            "host host.local",
            "",
            true,
        ),
        (
            "#10.1.1.1 host host.local",
            "10.1.1.1",
            "host host.local",
            "",
            false,
        ),
        (
            "  10.1.1.1  host  host.local  #  comment  ",
            "10.1.1.1",
            "host host.local",
            "comment",
            true,
        ),
        (
            "10.1.1.1 host host.local#comment",
            "10.1.1.1",
            "host host.local",
            "comment",
            true,
        ),
        (
            "#10.1.1.1 host host.local#comment",
            "10.1.1.1",
            "host host.local",
            "comment",
            false,
        ),
    ]);
}

/// Upstream `Not_Valid_Entry`: lines without a valid IP + host are not
/// entries (they become "additional lines").
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn entrytest_not_valid_entry() {
    let lines = [
        "\t\t10.1.1.1\t\t",
        "  10.1.1.1  ",
        "10.1.1.1",
        "\t\thost\t\t",
        "  host  ",
        "host",
        "\t\t10\t\thost",
        "  10  host  ",
        "10 host",
        "\t\thost\t\t10.1.1.1",
        "  host  10.1.1.1",
        "host 10.1.1.1",
        "# comment 10.1.1.1 host # comment",
    ];
    let bad: Vec<_> = lines
        .iter()
        .filter_map(|l| one(l).map(|e| format!("{l:?} parsed as entry {e:?}")))
        .collect();
    assert!(
        bad.is_empty(),
        "invalid lines treated as entries:\n{}",
        bad.join("\n")
    );
}

/// Upstream `Entry.Validate` ignores the two sample lines of the Windows
/// default hosts file.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn entrytest_sample_lines_are_not_entries() {
    for l in [
        "102.54.94.97 rhino.acme.com # source server",
        "38.25.63.10 x.acme.com # x client host",
        "#      102.54.94.97     rhino.acme.com          # source server",
    ] {
        assert!(one(l).is_none(), "{l:?} parsed as entry {:?}", one(l));
    }
}

/// Upstream requires `IPAddress.TryParse` + IPv4/IPv6 regex. Comment lines
/// whose first word merely contains '.' or ':' must not become disabled
/// entries with a bogus "IP".
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn comment_with_colon_or_dot_is_not_disabled_entry() {
    for l in [
        "# Note: entries below are managed by ansible",
        "# v1.2 of this file",
        "# e.g. see hosts(5)",
    ] {
        assert!(one(l).is_none(), "{l:?} parsed as entry {:?}", one(l));
    }
}

/// Upstream `ValidHosts` uses `Uri.CheckHostName`; garbage hosts make the
/// line invalid (kept verbatim as additional line).
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn invalid_hostname_is_not_entry() {
    assert!(
        one("10.0.0.1 bad!host").is_none(),
        "{:?}",
        one("10.0.0.1 bad!host")
    );
}

/// Upstream `HostsServiceTest.LongHosts_Splitted`: >9 hosts are split into
/// chunks of `Consts.MaxHostsCount` (9) entries.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn hostsservice_long_hosts_splitted() {
    let f = HostsFile::parse_str(
        "10.1.1.1 host01 host02 host03 host04 host05 host06 host07 host08 host09 host10 host11 host12 host13 host14 host15 host16 host17 host18 host19 # comment\n",
    );
    let e = f.entries();
    let hosts: Vec<usize> = e.iter().map(|e| e.hostnames.len()).collect();
    assert_eq!(hosts, vec![9, 9, 1], "entries per chunk");
    assert!(e.iter().all(|e| e.comment.as_deref() == Some("comment")));
}

// ── Round-trip / formatting ────────────────────────────────────────────────

const LINUX_HOSTS: &str = "\
# Static table lookup for hostnames.
# See hosts(5) for details.

127.0.0.1\tlocalhost
127.0.1.1\tmybox.example.org mybox
   # indented comment kept verbatim

# The following lines are desirable for IPv6 capable hosts
::1     localhost ip6-localhost ip6-loopback
ff02::1 ip6-allnodes
ff02::2 ip6-allrouters
#192.168.1.50 nas nas.lan # disabled
";

#[test]
fn roundtrip_preserves_comments_blank_lines_and_entries() {
    let f = HostsFile::parse_str(LINUX_HOSTS);
    let out = String::from_utf8(f.serialize_with(&WriteOptions {
        bom: false,
        no_leading_spaces: true,
    }))
    .unwrap();
    // Comment lines kept verbatim, blank lines kept.
    assert!(out.contains("# Static table lookup for hostnames.\n# See hosts(5) for details.\n\n"));
    assert!(out.contains("\n   # indented comment kept verbatim\n\n"));
    // Re-parsing yields the same entries.
    let again = HostsFile::parse_str(&out);
    let a: Vec<_> = f
        .entries()
        .iter()
        .map(|e| {
            (
                e.ip.clone(),
                e.hostnames.clone(),
                e.enabled,
                e.comment.clone(),
            )
        })
        .collect();
    let b: Vec<_> = again
        .entries()
        .iter()
        .map(|e| {
            (
                e.ip.clone(),
                e.hostnames.clone(),
                e.enabled,
                e.comment.clone(),
            )
        })
        .collect();
    assert_eq!(a, b);
    assert_eq!(a.len(), 6);
    assert!(!a[5].2, "#192.168.1.50 is disabled");
    assert!(out.ends_with('\n'));
}

#[test]
fn crlf_line_endings_parse_cleanly() {
    let f = HostsFile::parse_str("# c\r\n10.0.0.1 a b # x\r\n\r\n::1 lh\r\n");
    let e = f.entries();
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].hostnames, vec!["a", "b"]);
    assert_eq!(e[0].comment.as_deref(), Some("x"));
    assert_eq!(e[1].ip, "::1");
    let out = String::from_utf8(f.serialize_with(&WriteOptions::default())).unwrap();
    assert!(!out.contains('\r'), "{out:?}");
}

/// Comments may themselves contain '#': upstream joins the remainder.
#[test]
fn comment_containing_hash() {
    let e = one("10.0.0.1 a # c1 # c2").unwrap();
    assert_eq!(e.2, "c1 # c2");
}

/// Upstream writes `# comment` only when the comment is non-empty
/// (`if (e.Comment != string.Empty)`), and trims trailing whitespace.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn empty_inline_comment_not_written() {
    let f = HostsFile::parse_str("10.0.0.1 a #\n");
    let out = String::from_utf8(f.serialize_with(&WriteOptions::default())).unwrap();
    assert!(!out.contains('#'), "empty comment re-emitted: {out:?}");
    assert!(
        !out.lines().any(|l| l.ends_with(' ')),
        "trailing space: {out:?}"
    );
}

/// Upstream `HostsServiceTest.Host_Added`: columns padded to longest
/// address + 1 and longest hosts + 1, active lines indented by two spaces
/// when any entry is disabled.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn hostsservice_host_added_exact_format() {
    let mut f = HostsFile::parse_str(
        "10.1.1.1 host host.local # comment\n10.1.1.2 host2 host2.local # another comment\n",
    );
    f.add_entry(
        "10.1.1.30".into(),
        vec!["host30".into(), "host30.local".into()],
        Some("new entry".into()),
    );
    let last = f.entries().len() - 1;
    f.toggle_entry(last).unwrap();
    let out = String::from_utf8(f.serialize_with(&WriteOptions::default())).unwrap();
    assert_eq!(
        out,
        "  10.1.1.1  host host.local     # comment\n  10.1.1.2  host2 host2.local   # another comment\n# 10.1.1.30 host30 host30.local # new entry\n"
    );
}

/// Upstream `NoLeadingSpaces_Disabled_RemovesIndent` semantic part:
/// with NoLeadingSpaces no active line starts with whitespace.
#[test]
fn hostsservice_no_leading_spaces() {
    let mut f = HostsFile::parse_str("10.1.1.1 host\n10.1.1.2 host2\n");
    f.add_entry("10.1.1.30".into(), vec!["host30".into()], None);
    f.toggle_entry(2).unwrap();
    let opts = WriteOptions {
        bom: false,
        no_leading_spaces: true,
    };
    let out = String::from_utf8(f.serialize_with(&opts)).unwrap();
    assert!(out.lines().all(|l| !l.starts_with(' ')), "{out:?}");
    let out = String::from_utf8(f.serialize_with(&WriteOptions::default())).unwrap();
    assert!(out.starts_with("  10.1.1.1"), "{out:?}");
}

#[test]
fn hostsservice_host_deleted_and_updated_semantics() {
    let src = "10.1.1.1 host host.local # comment\n10.1.1.2 host2 host2.local # another comment\n";
    let mut f = HostsFile::parse_str(src);
    f.remove_entry(0).unwrap();
    let again = HostsFile::parse_str(
        &String::from_utf8(f.serialize_with(&WriteOptions::default())).unwrap(),
    );
    assert_eq!(again.entries().len(), 1);
    assert_eq!(again.entries()[0].ip, "10.1.1.2");

    let mut f = HostsFile::parse_str(src);
    f.update_entry(
        0,
        "10.1.1.10".into(),
        vec!["host".into(), "host.local".into(), "host1.local".into()],
        Some("updated comment".into()),
    )
    .unwrap();
    f.toggle_entry(0).unwrap();
    let out = String::from_utf8(f.serialize_with(&WriteOptions::default())).unwrap();
    let again = HostsFile::parse_str(&out);
    let e = again.entries();
    assert!(!e[0].enabled);
    assert_eq!(e[0].ip, "10.1.1.10");
    assert_eq!(e[0].hostnames.len(), 3);
    assert_eq!(e[0].comment.as_deref(), Some("updated comment"));
    assert!(e[1].enabled);
}

#[test]
fn hostsservice_empty_hosts() {
    let f = HostsFile::parse_str("");
    assert!(f.serialize_with(&WriteOptions::default()).is_empty());
}

#[test]
fn ipv6_entries_parse() {
    let e = one("fe80::1%eth0 router.lan").unwrap();
    assert_eq!(e.0, "fe80::1%eth0");
    let e = one("::ffff:192.168.0.1 mapped").unwrap();
    assert_eq!(e.0, "::ffff:192.168.0.1");
    let e = one("#2001:db8::1 v6host # doc").unwrap();
    assert_eq!((e.0.as_str(), e.3), ("2001:db8::1", false));
}

// ── DuplicateService ───────────────────────────────────────────────────────

fn ent(ip: &str, hosts: &[&str]) -> HostEntry {
    HostEntry {
        ip: ip.into(),
        hostnames: hosts.iter().map(|s| s.to_string()).collect(),
        comment: None,
        enabled: true,
    }
}

#[test]
fn duplicates_include_disabled_entries() {
    let a = ent("10.0.0.1", &["x"]);
    let mut b = ent("10.0.0.2", &["X"]);
    b.enabled = false;
    assert_eq!(find_duplicates(&[&a, &b], false), vec![true, true]);
}

/// Upstream classifies `fe80::1%eth0` as IPv6 (regex has a zone-index
/// branch), so it shares the address family with `fe80::2`.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn duplicates_ipv6_zone_index_same_family() {
    let a = ent("fe80::1%eth0", &["router"]);
    let b = ent("fe80::2", &["router"]);
    assert_eq!(find_duplicates(&[&a, &b], false), vec![true, true]);
}

/// Upstream IPv4 regex accepts leading zeros (`[01]?[0-9][0-9]?`).
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/C-system.md"]
fn duplicates_ipv4_leading_zero_same_family() {
    let a = ent("010.0.0.1", &["h"]);
    let b = ent("10.0.0.2", &["h"]);
    assert_eq!(find_duplicates(&[&a, &b], false), vec![true, true]);
}

#[test]
fn duplicates_loopback_variants() {
    let a = ent("::1", &["localhost"]);
    let b = ent("0:0:0:0:0:0:0:1", &["localhost"]);
    assert_eq!(find_duplicates(&[&a, &b], false), vec![false, false]);
    assert_eq!(find_duplicates(&[&a, &b], true), vec![true, true]);
}

// ── BackupManager ──────────────────────────────────────────────────────────

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpt-hosts-parity-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn backup_copies_exact_content() {
    let d = tmpdir("bk");
    let hosts = d.join("hosts");
    let content = "10.1.1.1 host host.local # comment\r\n10.1.1.2 host2\n";
    fs::write(&hosts, content).unwrap();
    let cfg = HostsEditorConfig {
        backup_path: d.join("b").to_string_lossy().into(),
        ..Default::default()
    };
    let p = backup::create_backup(&hosts, &cfg).unwrap();
    assert_eq!(fs::read_to_string(&p).unwrap(), content);
    assert!(
        p.file_name()
            .unwrap()
            .to_string_lossy()
            .contains(BACKUP_MARKER)
    );
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn backup_cleanup_by_count_on_disk() {
    let d = tmpdir("cnt");
    let b = d.join("b");
    fs::create_dir_all(&b).unwrap();
    for i in 0..4 {
        fs::write(b.join(format!("hosts{BACKUP_MARKER}2020010100000{i}")), "x").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    fs::write(b.join("unrelated.txt"), "keep").unwrap();
    let cfg = HostsEditorConfig {
        backup_path: b.to_string_lossy().into(),
        delete_backups_mode: DeleteBackupsMode::Count,
        delete_backups_count: 2,
        ..Default::default()
    };
    assert_eq!(backup::cleanup_backups(&cfg), 2);
    assert!(b.join("unrelated.txt").exists());
    assert_eq!(backup::list_backups(&b).len(), 2);
    let _ = fs::remove_dir_all(&d);
}

/// Upstream default settings (HostsProperties).
#[test]
fn default_settings_match_upstream() {
    let c = HostsEditorConfig::default();
    assert!(c.show_startup_warning);
    assert!(!c.loopback_duplicates);
    assert!(!c.no_leading_spaces);
    assert!(c.backup_before_save);
    assert_eq!(c.delete_backups_mode, DeleteBackupsMode::Age);
    assert_eq!(c.delete_backups_days, 15);
    assert_eq!(c.delete_backups_count, 5);
}

#[test]
fn hostsline_variants_exposed() {
    let f = HostsFile::parse_str("# c\n\n1.2.3.4 a\n");
    assert!(matches!(f.lines[0], HostsLine::Comment(_)));
    assert!(matches!(f.lines[1], HostsLine::Blank));
    assert!(matches!(f.lines[2], HostsLine::Entry(_)));
}
