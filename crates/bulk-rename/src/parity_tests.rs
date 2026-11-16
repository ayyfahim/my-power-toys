//! Parity tests: Microsoft PowerToys PowerRename behaviour is the oracle.
//!
//! Cases are ported from
//! `src/modules/powerrename/unittests/{CommonRegExTests.h, PowerRenameRegExTests.cpp,
//! HelpersTests.cpp, PowerRenameManagerTests.cpp}` and from reading
//! `lib/{PowerRenameRegEx.cpp, Helpers.cpp, Renaming.cpp, PowerRenameManager.cpp}`.
//! A failing test is a parity bug in the Rust port.

use crate::config::{ApplyTo, TextFormatting, TimeSource};
use crate::tokens::{self, DateParts};
use crate::{ListOptions, RenameOptions, Renamer, TokenOptions};
use rand::SeedableRng;
use std::fs;
use std::path::{Path, PathBuf};

// ── helpers ────────────────────────────────────────────────────────────

fn o(search: &str, replace: &str, regex: bool, match_all: bool, case: bool) -> RenameOptions {
    RenameOptions {
        search: search.into(),
        replace: replace.into(),
        use_regex: regex,
        match_all,
        case_sensitive: case,
        apply_to: ApplyTo::FilenameAndExtension,
        text_formatting: TextFormatting::None,
        enumerate: false,
    }
}

const FAKE_DIR: &str = "/nonexistent-mpt-parity";

/// Preview a batch of (non-existing) names and return the new names.
fn names_with(names: &[&str], opts: &RenameOptions, tok: &TokenOptions) -> Vec<String> {
    let files: Vec<PathBuf> = names.iter().map(|n| Path::new(FAKE_DIR).join(n)).collect();
    Renamer::new()
        .preview_with(&files, opts, tok)
        .expect("preview failed")
        .into_iter()
        .map(|p| {
            p.renamed
                .strip_prefix(FAKE_DIR)
                .map(|r| r.to_string_lossy().into_owned())
                .unwrap_or_else(|_| p.renamed.to_string_lossy().into_owned())
        })
        .collect()
}

fn name1(name: &str, opts: &RenameOptions) -> String {
    names_with(&[name], opts, &TokenOptions::default()).remove(0)
}

fn rnd() -> TokenOptions {
    TokenOptions {
        randomize: true,
        time_source: TimeSource::Created,
    }
}

struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "mpt-br-parity-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn file(&self, rel: &str) -> PathBuf {
        let p = self.0.join(rel);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, rel.as_bytes()).unwrap();
        p
    }
    fn dir(&self, rel: &str) -> PathBuf {
        let p = self.0.join(rel);
        fs::create_dir_all(&p).unwrap();
        p
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn preview_names(paths: &[PathBuf], opts: &RenameOptions) -> Vec<String> {
    Renamer::new()
        .preview(paths, opts)
        .unwrap()
        .into_iter()
        .map(|p| {
            p.renamed
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

// ── CommonRegExTests.h ─────────────────────────────────────────────────

#[test]
fn general_replace_and_no_match() {
    let f = o("foo", "big", false, false, false);
    assert_eq!(name1("foobar", &f), "bigbar");
    assert_eq!(
        name1("foobar", &o("notfound", "big", false, false, false)),
        "foobar"
    );
    // Empty replace term removes the match.
    assert_eq!(name1("foobar", &o("foo", "", false, false, false)), "bar");
}

#[test]
fn case_sensitive_table() {
    for (s, r, t, e) in [
        ("Foo", "Foo", "FooBar", "FooBar"),
        ("Foo", "boo", "FooBar", "booBar"),
        ("Foo", "boo", "foobar", "foobar"),
        ("123", "654", "123456", "654456"),
    ] {
        assert_eq!(name1(t, &o(s, r, false, false, true)), e, "{s}->{r} on {t}");
    }
}

#[test]
fn replace_first_only_and_all_plain() {
    for (s, r, t, e) in [
        ("B", "BB", "ABA", "ABBA"),
        ("B", "A", "ABBBA", "AABBA"),
        ("B", "BBB", "ABABAB", "ABBBABAB"),
    ] {
        assert_eq!(name1(t, &o(s, r, false, false, false)), e);
        assert_eq!(
            name1(t, &o(s, r, true, false, false)),
            e,
            "regex first-only"
        );
    }
    for (s, r, t, e) in [
        ("B", "BB", "ABA", "ABBA"),
        ("B", "A", "ABBBA", "AAAAA"),
        ("B", "BBB", "ABABAB", "ABBBABBBABBB"),
    ] {
        assert_eq!(name1(t, &o(s, r, false, true, false)), e);
        assert_eq!(name1(t, &o(s, r, true, true, false)), e, "regex all");
        assert_eq!(name1(t, &o(s, r, true, true, true)), e, "regex all cs");
    }
    assert_eq!(
        name1("AbABAb", &o("b", "BBB", false, true, true)),
        "ABBBABABBB"
    );
    assert_eq!(
        name1("AbABAb", &o("b", "BBB", true, true, true)),
        "ABBBABABBB"
    );
}

#[test]
fn wildcard_cases() {
    // VerifyReplaceFirstWildCardUseRegex / VerifyMatchAllWildcardUseRegEx (std engine)
    assert_eq!(name1("AAAAAA", &o(".*", "Foo", true, false, false)), "Foo");
    assert_eq!(name1("AAAAAA", &o(".*", "Foo", true, true, false)), "Foo");
    // Plain text ".*" is literal.
    assert_eq!(
        name1("AAAAAA", &o(".*", "Foo", false, true, false)),
        "AAAAAA"
    );
    assert_eq!(name1(".*", &o(".*", "Foo", false, true, false)), "Foo");
    assert_eq!(
        name1(".*Bar.*", &o(".*", "Foo", false, true, false)),
        "FooBarFoo"
    );
}

#[test]
fn counter_no_regex_keeps_dollar_one() {
    let mut f = o("bar", "$1bar_${}", false, false, false);
    f.enumerate = true;
    assert_eq!(name1("foobar", &f), "foo$1bar_0");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn counter_disabled_regex_leaves_group_literal() {
    let f = o("bar", "$1bar_${}", true, false, false);
    assert_eq!(name1("foobar", &f), "foobar_${}");
}

#[test]
fn counters_basic_and_customised() {
    let mut f = o("bar", "bar_${}", true, false, false);
    f.enumerate = true;
    assert_eq!(name1("foobar", &f), "foobar_0");
    f.replace = "_${}_bar_${}".into();
    assert_eq!(name1("foobar", &f), "foo_0_bar_0");
    // increment=10 at index 1 and 2
    f.replace = "bar_${increment=10}".into();
    let r = names_with(
        &["foobar", "foobar", "foobar"],
        &f,
        &TokenOptions::default(),
    );
    assert_eq!(r, ["foobar_0", "foobar_10", "foobar_20"]);
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    assert_eq!(
        tokens::expand_groups("bar_${start=1000}", true, false, 5, &mut rng),
        "bar_1005"
    );
    assert_eq!(
        tokens::expand_groups("bar_${padding=5}", true, false, 204, &mut rng),
        "bar_00204"
    );
    assert_eq!(
        tokens::expand_groups(
            "bar_${increment=7,start=993,padding=5}",
            true,
            false,
            12,
            &mut rng
        ),
        "bar_01077"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn counter_padding_is_modulo_max_path_not_clamped_to_64() {
    // Enumerating.h: padding = options.padding % MAX_PATH (260); padding=100 -> 100 digits.
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    let s = tokens::expand_groups("${padding=100}", true, false, 7, &mut rng);
    assert_eq!(s.len(), 100, "got {} chars", s.len());
}

#[test]
fn counter_negative_with_padding() {
    // swprintf("%04d", -1) == "-001"
    let mut rng = rand::rngs::StdRng::seed_from_u64(0);
    assert_eq!(
        tokens::expand_groups("${start=-1;padding=4}", true, false, 0, &mut rng),
        "-001"
    );
}

#[test]
fn counter_increments_when_result_unchanged() {
    let mut f = o("(.*)", "NewFile-${start=1}", true, false, false);
    f.enumerate = true;
    let r = names_with(
        &["DocA", "DocB", "NewFile-3", "DocC"],
        &f,
        &TokenOptions::default(),
    );
    assert_eq!(r, ["NewFile-1", "NewFile-2", "NewFile-3", "NewFile-4"]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn randomizer_flag_cases() {
    // flags = 0: literal
    assert_eq!(
        name1(
            "foobar",
            &o("bar", "$1bar_${rstringalnum=9}", false, false, false)
        ),
        "foo$1bar_${rstringalnum=9}"
    );
    // RandomizeItems without regex: `${}` is not a randomizer
    assert_eq!(
        names_with(
            &["foobar"],
            &o("bar", "$1bar_${}", false, false, false),
            &rnd()
        )[0],
        "foo$1bar_${}"
    );
    // regex, randomizer disabled
    assert_eq!(
        name1(
            "foobar",
            &o("bar", "$1bar_${rstringalnum=9}", true, false, false)
        ),
        "foobar_${rstringalnum=9}"
    );
    // zero length
    assert_eq!(
        names_with(
            &["foobar"],
            &o("bar", "$1bar_${rstringalnum=0}", true, false, false),
            &rnd()
        )[0],
        "foobar_"
    );
}

#[test]
fn randomizer_shapes() {
    let check = |rep: &str, re: &str| {
        let r = names_with(&["foobar"], &o("bar", rep, true, false, false), &rnd()).remove(0);
        let re = regex::Regex::new(&format!("^{re}$")).unwrap();
        assert!(re.is_match(&r), "{rep} gave {r}");
    };
    check("$1bar_${rstringalnum=9}", r"foobar_[A-Za-z0-9]{9}");
    check("$1bar_${rstringalpha=9}", r"foobar_[A-Za-z]{9}");
    check("$1bar_${rstringdigit=9}", r"foobar_\d{9}");
    let uuid =
        r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-4[0-9a-fA-F]{3}-[89aAbB][0-9a-fA-F]{3}-[0-9a-fA-F]{12}";
    check("$1bar_${ruuidv4}", &format!("foobar_{uuid}"));
    check(
        "$1bar_${rstringalnum=2}${rstringalpha=2}${rstringdigit=2}${ruuidv4}",
        &format!(r"foobar_[A-Za-z0-9]{{2}}[A-Za-z]{{2}}\d{{2}}{uuid}"),
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn unicode_nfd_source_matches_nfc_search() {
    // VerifyUnicodeAndWhitespaceNormalization{SimpleSearch,Regex}
    for regex in [false, true] {
        let f = o("Test\u{0439}", "Match", regex, false, false);
        assert_eq!(name1("Test\u{0438}\u{0306}", &f), "Match", "regex={regex}");
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn nbsp_source_matches_space_search() {
    for regex in [false, true] {
        let f = o("Hello World", "Match", regex, false, false);
        assert_eq!(name1("Hello\u{00A0}World", &f), "Match", "regex={regex}");
    }
}

#[test]
fn regex_anchor_metacharacters() {
    assert_eq!(
        name1("test.txt", &o("$", "_end", true, false, false)),
        "test.txt_end"
    );
    assert_eq!(
        name1("test.txt", &o("^", "start_", true, false, false)),
        "start_test.txt"
    );
}

#[test]
fn sharp_s_to_ss() {
    assert_eq!(
        name1("Ma\u{00DF}", &o("\u{00DF}", "ss", false, false, false)),
        "Mass"
    );
}

// ── PowerRenameRegExTests.cpp (std::wregex engine, the default) ───────

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn capturing_groups_table_std_engine() {
    let cases = [
        (
            "$1_$002_$223_$001021_$00001",
            "foo_$002_bar23_$001021_$00001",
        ),
        ("_$1$2_$123$040", "_foobar_foo23$040"),
        ("$$$1", "$foo"),
        ("$$1", "$1"),
        ("$12", "foo2"),
        ("$10", "foo0"),
        ("$01", "$01"),
        ("$$$11", "$foo1"),
        ("$$$$113a", "$$113a"),
    ];
    let mut failures = Vec::new();
    for (rep, exp) in cases {
        let got = name1("foobar", &o("(foo)(bar)", rep, true, true, true));
        if got != exp {
            failures.push(format!("{rep:?}: expected {exp:?} got {got:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn lookbehind_is_rejected_like_std_engine() {
    for pat in ["(?<=E12).*", "(?<!E12).*"] {
        let files = [PathBuf::from("/x/AAAAAA")];
        assert!(
            Renamer::new()
                .preview(&files, &o(pat, "Foo", true, false, false))
                .is_err()
        );
    }
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn ecmascript_lookahead_supported() {
    // ECMAScript (std::wregex) supports lookahead; the Rust GUI help even lists
    // ".+?(?=bar)" as an example pattern.
    let f = o(".+?(?=bar)", "X", true, false, false);
    let files = [PathBuf::from("/x/foobar")];
    let r = Renamer::new().preview(&files, &f);
    assert!(r.is_ok(), "lookahead rejected: {:?}", r.err());
    assert_eq!(r.unwrap()[0].renamed, PathBuf::from("/x/Xbar"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn ecmascript_backreference_supported() {
    let f = o(r"(o)\1", "0", true, true, false);
    let files = [PathBuf::from("/x/foobar")];
    let r = Renamer::new().preview(&files, &f);
    assert!(r.is_ok(), "backreference rejected: {:?}", r.err());
    assert_eq!(r.unwrap()[0].renamed, PathBuf::from("/x/f0bar"));
}

// ── Date/time tokens (PowerRenameRegExTests.cpp + HelpersTests.cpp) ─────

fn t2020() -> DateParts {
    // SYSTEMTIME{2020, 7, 3(dow), 22, 15, 6, 42, 453}
    DateParts {
        year: 2020,
        month: 7,
        day: 22,
        hour: 15,
        minute: 6,
        second: 42,
        millis: 453,
        weekday: 3,
    }
}
fn t2024() -> DateParts {
    DateParts {
        year: 2024,
        month: 3,
        day: 15,
        hour: 14,
        minute: 30,
        second: 45,
        millis: 123,
        weekday: 5,
    }
}

#[test]
fn file_attributes_padding_and_no_padding() {
    let d = t2020();
    let e = |s: &str| tokens::expand_dollar_tokens(s, Some(&d), None);
    assert_eq!(e("bar$YY-$M-$D-$h-$m-$s-$f"), "bar20-7-22-15-6-42-4");
    assert_eq!(
        e("bar$YYYY-$MM-$DD-$hh-$mm-$ss-$fff"),
        "bar2020-07-22-15-06-42-453"
    );
    let d = DateParts {
        year: 2020,
        month: 1,
        day: 1,
        hour: 15,
        minute: 6,
        second: 42,
        millis: 453,
        weekday: 3,
    };
    assert_eq!(
        tokens::expand_dollar_tokens("bar$MMM-$MMMM-$DDD-$DDDD", Some(&d), None),
        "barJan-January-Wed-Wednesday"
    );
}

#[test]
fn helpers_dated_file_name_table() {
    let d = t2024();
    let cases = [
        ("file_$YYY", "file_24Y"),
        ("file_$DDD", "file_Fri"),
        ("file_$MMM", "file_Mar"),
        ("file_$HHH", "file_02H"),
        ("file_$Y-$Y-$Y", "file_4-4-4"),
        ("file_$D.$D.$D", "file_15.15.15"),
        ("file_$YYYY$Y", "file_20244"),
        ("file_$DDD$D", "file_Fri15"),
        ("file_$DDDD$DD", "file_Friday15"),
        ("file_$MMM$M", "file_Mar3"),
        ("$YYYY$M$D", "2024315"),
        ("file_$Y", "file_4"),
        ("file-$Y.$Y-$M", "file-4.4-3"),
        ("file_$YYYY", "file_2024"),
        ("file_$DDDD", "file_Friday"),
        ("file_$HH", "file_02"),
        ("file_$fff", "file_123"),
        (
            "Photo_$YYYY-$MM-$DD_$hh-$mm-$ss_$fff",
            "Photo_2024-03-15_14-30-45_123",
        ),
        ("$YYYY_$Y-$Y_$MM_$M", "2024_4-4_03_3"),
        ("$D-$DD-$DDD-$DDDD", "15-15-Fri-Friday"),
        ("file_$DDT", "file_15T"),
        ("file_$YYYYA_$MMB_$DDC", "file_2024A_03B_15C"),
        ("file_$DATE_TAKEN_YYYY", "file_$DATE_TAKEN_YYYY"),
        ("file_$HEIGHT", "file_$HEIGHT"),
        ("file_$DEER", "file_15EER"),
    ];
    let mut failures = Vec::new();
    for (src, exp) in cases {
        let got = tokens::expand_dollar_tokens(src, Some(&d), None);
        if got != exp {
            failures.push(format!("{src:?}: expected {exp:?} got {got:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn dated_name_protects_xmp_patterns() {
    // Helpers.cpp: $D(?!(ATE_TAKEN_|ESCRIPTION|OCUMENT_ID)) — XMP $DESCRIPTION / $DOCUMENT_ID
    // are not eaten by $D.
    let d = t2024();
    assert_eq!(
        tokens::expand_dollar_tokens("x_$DESCRIPTION", Some(&d), None),
        "x_$DESCRIPTION"
    );
    assert_eq!(
        tokens::expand_dollar_tokens("x_$DOCUMENT_ID", Some(&d), None),
        "x_$DOCUMENT_ID"
    );
}

#[test]
fn twelve_and_24_hour_formats() {
    let mk = |h, m| DateParts {
        year: 2025,
        month: 4,
        day: 10,
        hour: h,
        minute: m,
        second: 0,
        millis: 0,
        weekday: 4,
    };
    let cases = [
        (mk(0, 0), "[$hh:$mm] [$H:$mm $tt]", "[00:00] [12:00 am]"),
        (mk(12, 0), "[$hh:$mm] [$H:$mm $tt]", "[12:00] [12:00 pm]"),
        (
            mk(1, 5),
            "[$h:$m] [$H:$m $tt] [$hh:$mm] [$HH:$mm $TT]",
            "[1:5] [1:5 am] [01:05] [01:05 AM]",
        ),
        (
            mk(23, 45),
            "[$h:$m] [$H:$m $tt] [$hh:$mm] [$HH:$mm $TT]",
            "[23:45] [11:45 pm] [23:45] [11:45 PM]",
        ),
        (
            mk(14, 30),
            "Date: $YYYY-$MM-$DD Time: $hh:$mm (24h) / $H:$mm $tt (12h)",
            "Date: 2025-04-10 Time: 14:30 (24h) / 2:30 pm (12h)",
        ),
    ];
    for (d, src, exp) in cases {
        assert_eq!(tokens::expand_dollar_tokens(src, Some(&d), None), exp);
    }
}

#[test]
fn date_tokens_full_flow_uses_creation_time_by_default() {
    let t = Tmp::new("date");
    let f = t.file("foo.txt");
    let meta = fs::metadata(&f).unwrap();
    let st = meta.created().or_else(|_| meta.modified()).unwrap();
    let d = DateParts::from_system_time(st);
    let mut opts = o("foo", "$YYYY$MM$DD", true, false, false);
    opts.apply_to = ApplyTo::FilenameOnly;
    let r = preview_names(&[f], &opts);
    assert_eq!(r[0], format!("{:04}{:02}{:02}.txt", d.year, d.month, d.day));
}

// ── HelpersTests.cpp: metadata pattern replacement ───────────────────

#[test]
fn metadata_pattern_helpers() {
    use std::collections::HashMap;
    let mut m: HashMap<String, String> = HashMap::new();
    m.insert("CAMERA_MAKE".into(), "Canon".into());
    m.insert("ISO".into(), "ISO 400".into());
    let e = |s: &str, m: &HashMap<String, String>| tokens::expand_dollar_tokens(s, None, Some(m));
    assert_eq!(e("photo_$CAMERA_MAKE_$ISO", &m), "photo_Canon_ISO 400");
    assert_eq!(
        e("photo_$CAMERA_MAKE_$INVALID_PATTERN", &m),
        "photo_Canon_$INVALID_PATTERN"
    );
    assert_eq!(e("photo_$camera_make", &m), "photo_$camera_make");
    assert_eq!(e("photo_$ISO$", &m), "photo_ISO 400$");
    let empty = HashMap::new();
    assert_eq!(
        e("photo_$ISO_$CAMERA_MAKE", &empty),
        "photo_$ISO_$CAMERA_MAKE"
    );
}

// ── PowerRenameManagerTests.cpp / Renaming.cpp (real files) ───────────

#[test]
fn manager_name_only_and_extension_only() {
    let t = Tmp::new("parts");
    let a = t.file("foo.foo");
    let b = t.file("test.foo");
    let mut f = o("foo", "bar", false, false, false);
    f.apply_to = ApplyTo::FilenameOnly;
    assert_eq!(
        preview_names(&[a.clone(), b.clone()], &f),
        ["bar.foo", "test.foo"]
    );
    f.apply_to = ApplyTo::ExtensionOnly;
    assert_eq!(preview_names(&[a, b], &f), ["foo.bar", "test.bar"]);
}

#[test]
fn manager_uppercase_lowercase_transform() {
    let t = Tmp::new("case");
    let folder = t.dir("foo");
    let file = t.file("foo.test");
    let mut f = o("foo", "bar", false, false, false);
    f.text_formatting = TextFormatting::Uppercase;
    assert_eq!(preview_names(&[folder, file], &f), ["BAR", "BAR.TEST"]);
    let t2 = Tmp::new("lower");
    let d1 = t2.dir("Foo");
    let d2 = t2.dir("Foo.teST");
    f.text_formatting = TextFormatting::Lowercase;
    assert_eq!(preview_names(&[d1, d2], &f), ["bar", "bar.test"]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn manager_titlecase_transform() {
    let t = Tmp::new("title");
    let d = t.dir("foo And The To");
    let fl = t.file("foo And The To.txt");
    let mut f = o("foo", "bar", false, false, false);
    f.text_formatting = TextFormatting::TitleCase;
    assert_eq!(
        preview_names(&[d, fl], &f),
        ["Bar and the To", "Bar and the To.txt"]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn manager_titlecase_with_apostrophes() {
    let t = Tmp::new("title2");
    let d = t.dir("the foo i'll and i've you're dogs' the i'd it's i'm don't to y'all");
    let fl =
        t.file("'the 'foo' 'i'll' and i've you're dogs' the 'i'd' it's i'm don't to y'all.txt");
    let mut f = o("foo", "bar", false, false, false);
    f.text_formatting = TextFormatting::TitleCase;
    assert_eq!(
        preview_names(&[d, fl], &f),
        [
            "The Bar I'll and I've You're Dogs' the I'd It's I'm Don't to Y'all",
            "'The 'Bar' 'I'll' and I've You're Dogs' the 'I'd' It's I'm Don't to Y'all.txt"
        ]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn manager_capitalized_transform() {
    let t = Tmp::new("cap");
    let d = t.dir("foo and the to");
    let d2 = t.dir("foo i'll and i've you're dogs' the i'd it's i'm don't to y'all");
    let fl = t.file("'foo i'll 'and' i've you're dogs' the i'd it's i'm don't to y'all.txt");
    let mut f = o("foo", "bar", false, false, false);
    f.text_formatting = TextFormatting::CapitalizeEachWord;
    assert_eq!(
        preview_names(&[d, d2, fl], &f),
        [
            "Bar And The To",
            "Bar I'll And I've You're Dogs' The I'd It's I'm Don't To Y'all",
            "'Bar I'll 'And' I've You're Dogs' The I'd It's I'm Don't To Y'all.txt"
        ]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn capitalize_lowercases_rest_of_word() {
    // GetTransformedFileName(Capitalized): non-initial letters are lowered.
    let t = Tmp::new("cap2");
    let fl = t.file("hELLO wORLD.txt");
    let mut f = o("", "", false, false, false);
    f.text_formatting = TextFormatting::CapitalizeEachWord;
    assert_eq!(preview_names(&[fl], &f), ["Hello World.txt"]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn manager_name_only_uppercase_transform() {
    // VerifyNameOnlyTransform: file foo.foo -> BAR.foo ; folder foo.txt -> BAR.TXT
    let t = Tmp::new("nameonlyup");
    let fl = t.file("foo.foo");
    let d = t.dir("foo.txt");
    let mut f = o("foo", "bar", false, false, false);
    f.apply_to = ApplyTo::FilenameOnly;
    f.text_formatting = TextFormatting::Uppercase;
    assert_eq!(preview_names(&[fl, d], &f), ["BAR.foo", "BAR.TXT"]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn manager_extension_only_lowercase_transform() {
    // VerifyExtensionOnlyTransform: file foo.FOO -> foo.bar ; folders never renamed in ExtensionOnly
    let t = Tmp::new("extlow");
    let fl = t.file("foo.FOO");
    let d1 = t.dir("bar.FOO");
    let d2 = t.dir("foo.bar");
    let mut f = o("foo", "bar", false, false, false);
    f.apply_to = ApplyTo::ExtensionOnly;
    f.text_formatting = TextFormatting::Lowercase;
    let p = Renamer::new().preview(&[fl, d1, d2], &f).unwrap();
    assert_eq!(p[0].renamed.file_name().unwrap(), "foo.bar");
    assert!(
        !p[1].changed,
        "folder bar.FOO renamed to {:?}",
        p[1].renamed
    );
    assert!(
        !p[2].changed,
        "folder foo.bar renamed to {:?}",
        p[2].renamed
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn folder_name_is_not_split_into_stem_and_extension() {
    // Renaming.cpp: folders always use the whole name as the source.
    let t = Tmp::new("foldersplit");
    let d = t.dir("foo.foo");
    let mut f = o("foo", "bar", false, true, false);
    f.apply_to = ApplyTo::FilenameOnly;
    assert_eq!(preview_names(&[d], &f), ["bar.bar"]);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn dotfile_stem_is_whole_name() {
    // fs::path(".foo").stem() == ".foo", extension() == "" (C++17 filesystem).
    let mut f = o("foo", "bar", false, false, false);
    f.apply_to = ApplyTo::FilenameOnly;
    assert_eq!(name1(".foo", &f), ".bar");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn result_is_trimmed_of_trailing_dots_and_spaces() {
    // GetTrimmedFileName: strip leading whitespace, trailing whitespace and dots.
    assert_eq!(name1("foo.txt", &o("txt", "", false, false, false)), "foo");
    assert_eq!(name1("foo bar", &o("foo", "", false, false, false)), "bar");
    assert_eq!(name1("abc", &o("c", "c  ", false, false, false)), "abc");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn slash_in_new_name_does_not_move_file() {
    // Upstream marks names with \ / : * ? " < > | as ItemNameInvalidChar and does not rename.
    // On Linux '/' is the only path-breaking char: the result must stay in the same folder.
    let f = o("a", "x/y", false, false, false);
    let files = [PathBuf::from("/d/a.txt")];
    let p = Renamer::new().preview(&files, &f).unwrap();
    assert!(
        !p[0].changed || p[0].renamed.parent() == Some(Path::new("/d")),
        "renamed escapes folder: {:?}",
        p[0].renamed
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn case_insensitive_plain_unicode_length_change_no_panic() {
    // 'İ' (2 bytes) lowercases to "i̇" (3 bytes): byte offsets drift.
    let r = std::panic::catch_unwind(|| {
        name1("\u{0130}stanbul_foo", &o("foo", "bar", false, true, false))
    });
    assert!(r.is_ok(), "panicked");
    assert_eq!(r.unwrap(), "\u{0130}stanbul_bar");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn case_insensitive_plain_kelvin_sign() {
    // KELVIN SIGN (3 bytes) lowercases to 'k' (1 byte).
    let r =
        std::panic::catch_unwind(|| name1("\u{212A}x_foo_y", &o("foo", "bar", false, true, false)));
    assert!(r.is_ok(), "panicked");
    assert_eq!(r.unwrap(), "\u{212A}x_bar_y");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn rename_conflict_renames_on_collision() {
    // FOF_RENAMEONCOLLISION: an existing target does not abort the operation.
    let t = Tmp::new("conflict");
    let a = t.file("a.txt");
    let _b = t.file("b.txt");
    let mut r = Renamer::new();
    let p = r
        .preview(std::slice::from_ref(&a), &o("a", "b", false, false, false))
        .unwrap();
    let res = r.execute(&p);
    assert!(res.is_ok(), "execute failed: {:?}", res.err());
    assert!(!a.exists());
    assert_eq!(
        fs::read_to_string(t.0.join("b.txt")).unwrap(),
        "b.txt",
        "existing file clobbered"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn partial_failure_can_still_be_undone() {
    let t = Tmp::new("partial");
    let a1 = t.file("a1.txt");
    let a2 = t.file("a2.txt");
    let _b2 = t.file("b2.txt");
    let mut r = Renamer::new();
    let p = r
        .preview(&[a1.clone(), a2.clone()], &o("a", "b", false, false, false))
        .unwrap();
    let _ = r.execute(&p);
    // Whatever happened, undo must bring a1.txt back.
    let u = r.undo();
    assert!(
        a1.exists(),
        "a1.txt lost after partial failure; undo returned {u:?}"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn nested_folder_and_child_both_renamed() {
    // PowerRenameManager renames depth-first (children before parents).
    let t = Tmp::new("nested");
    t.file("foo/foo.txt");
    let entries = Renamer::list_entries(
        &t.0,
        &ListOptions {
            include_folders: true,
            include_subfolders: true,
        },
    )
    .unwrap();
    let mut r = Renamer::new();
    let p = r
        .preview(&entries, &o("foo", "bar", false, false, false))
        .unwrap();
    let res = r.execute(&p);
    assert!(res.is_ok(), "execute failed: {:?}", res.err());
    assert!(t.0.join("bar/bar.txt").exists());
}

#[test]
fn execute_and_undo_roundtrip() {
    let t = Tmp::new("undo");
    let a = t.file("foo1.txt");
    let b = t.file("foo2.txt");
    let c = t.file("baa.txt");
    let mut r = Renamer::new();
    let p = r
        .preview(
            &[a.clone(), b.clone(), c.clone()],
            &o("foo", "bar", false, false, false),
        )
        .unwrap();
    let op = r.execute(&p).unwrap();
    assert_eq!(op.renames.len(), 2);
    assert!(t.0.join("bar1.txt").exists() && t.0.join("bar2.txt").exists() && c.exists());
    r.undo().unwrap();
    assert!(a.exists() && b.exists());
}

#[test]
fn listing_include_folders_and_subfolders() {
    let t = Tmp::new("list");
    t.file("a.txt");
    t.file("sub/b.txt");
    let base = |o: ListOptions| -> Vec<String> {
        Renamer::list_entries(&t.0, &o)
            .unwrap()
            .into_iter()
            .map(|p| p.strip_prefix(&t.0).unwrap().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(base(ListOptions::default()), ["a.txt"]);
    assert_eq!(
        base(ListOptions {
            include_folders: true,
            include_subfolders: false
        }),
        ["a.txt", "sub"]
    );
    assert_eq!(
        base(ListOptions {
            include_folders: true,
            include_subfolders: true
        }),
        ["a.txt", "sub", "sub/b.txt"]
    );
}

#[test]
fn defaults_match_powerrename() {
    let c = crate::config::BulkRenameConfig::default();
    // PowerRename settings defaults: PersistState true, MRUEnabled true, MaxMRUSize 10.
    assert!(c.persist_state && c.mru_enabled);
    assert_eq!(c.max_mru_size, 10);
    assert_eq!(c.time_source, TimeSource::Created);
    assert_eq!(c.apply_to, ApplyTo::FilenameAndExtension);
}
