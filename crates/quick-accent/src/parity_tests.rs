//! Parity tests against Microsoft PowerToys Quick Accent (PowerAccent).
//!
//! Oracles:
//! - src/modules/poweraccent/PowerAccent.Common/CharacterMappings.cs (tables, merge order)
//! - src/modules/poweraccent/PowerAccent.Core/PowerAccent.cs (ToUpper, descriptions)
//! - src/modules/poweraccent/PowerAccent.Core/Tools/Calculation.cs (toolbar placement)
//! - src/settings-ui/Settings.UI.Library/PowerAccentProperties.cs / PowerAccentSettings.cs (defaults)
//! - src/common/utils/excluded_apps.h (excluded apps matching)

use crate::characters::{LANGUAGES, accents_for_letter, unicode_description};
use crate::config::{
    ActivationKey, QuickAccentConfig, ScreenRect, ToolbarPosition, toolbar_origin,
};

fn langs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// Upstream letter tables (letters a-z only; the Rust port has no
/// comma/digit/slash keys, those are covered by a separate test).
/// (rust code, upstream enum, [(letter, chars)])
type Table = (
    &'static str,
    &'static str,
    &'static [(char, &'static [&'static str])],
);

const UPSTREAM: &[Table] = &[
    (
        "fr",
        "FR",
        &[
            ('a', &["à", "â", "á", "ä", "ã", "æ"]),
            ('c', &["ç"]),
            ('e', &["é", "è", "ê", "ë", "€"]),
            ('i', &["î", "ï", "í", "ì"]),
            ('o', &["ô", "ö", "ó", "ò", "õ", "œ"]),
            ('u', &["û", "ù", "ü", "ú"]),
            ('y', &["ÿ", "ý"]),
        ],
    ),
    (
        "de",
        "DE",
        &[
            ('a', &["ä"]),
            ('e', &["€"]),
            ('o', &["ö"]),
            ('s', &["ß"]),
            ('u', &["ü"]),
        ],
    ),
    (
        "es",
        "SP",
        &[
            ('a', &["á"]),
            ('e', &["é", "€"]),
            ('h', &["ḥ"]),
            ('i', &["í"]),
            ('l', &["ḷ"]),
            ('n', &["ñ"]),
            ('o', &["ó"]),
            ('u', &["ú", "ü"]),
        ],
    ),
    (
        "pt",
        "PT",
        &[
            ('a', &["á", "à", "â", "ã", "ª"]),
            ('c', &["ç"]),
            ('e', &["é", "ê", "€"]),
            ('i', &["í"]),
            ('o', &["ô", "ó", "õ", "º"]),
            ('s', &["$"]),
            ('u', &["ú"]),
        ],
    ),
    (
        "pl",
        "PL",
        &[
            ('a', &["ą"]),
            ('c', &["ć"]),
            ('e', &["ę", "€"]),
            ('l', &["ł"]),
            ('n', &["ń"]),
            ('o', &["ó"]),
            ('s', &["ś"]),
            ('z', &["ż", "ź"]),
        ],
    ),
    (
        "it",
        "IT",
        &[
            ('a', &["à"]),
            ('e', &["è", "é", "ə", "€"]),
            ('i', &["ì", "í"]),
            ('o', &["ò", "ó"]),
            ('u', &["ù", "ú"]),
        ],
    ),
    (
        "nl",
        "NL",
        &[
            ('a', &["á", "à", "ä"]),
            ('c', &["ç"]),
            ('e', &["é", "è", "ë", "ê", "€"]),
            ('i', &["í", "ï", "î"]),
            ('n', &["ñ"]),
            ('o', &["ó", "ö", "ô"]),
            ('u', &["ú", "ü", "û"]),
        ],
    ),
    (
        "sv",
        "SV",
        &[('a', &["å", "ä"]), ('e', &["é"]), ('o', &["ö"])],
    ),
    (
        "no",
        "NO",
        &[
            ('a', &["å", "æ"]),
            ('e', &["€", "é"]),
            ('o', &["ø"]),
            ('s', &["$"]),
        ],
    ),
    (
        "da",
        "DK",
        &[('a', &["å", "æ"]), ('e', &["€"]), ('o', &["ø"])],
    ),
    (
        "cs",
        "CZ",
        &[
            ('a', &["á"]),
            ('c', &["č"]),
            ('d', &["ď"]),
            ('e', &["ě", "é"]),
            ('i', &["í"]),
            ('n', &["ň"]),
            ('o', &["ó"]),
            ('r', &["ř"]),
            ('s', &["š"]),
            ('t', &["ť"]),
            ('u', &["ů", "ú"]),
            ('y', &["ý"]),
            ('z', &["ž"]),
        ],
    ),
    (
        "hu",
        "HU",
        &[
            ('a', &["á"]),
            ('e', &["é"]),
            ('i', &["í"]),
            ('o', &["ó", "ő", "ö"]),
            ('u', &["ú", "ű", "ü"]),
            ('y', &["ÿ", "ý"]),
        ],
    ),
    (
        "ro",
        "RO",
        &[
            ('a', &["ă", "â"]),
            ('i', &["î"]),
            ('s', &["ș"]),
            ('t', &["ț"]),
        ],
    ),
    (
        "tr",
        "TK",
        &[
            ('a', &["â"]),
            ('c', &["ç"]),
            ('e', &["ë", "€"]),
            ('g', &["ğ"]),
            ('i', &["ı", "İ", "î"]),
            ('o', &["ö", "ô"]),
            ('s', &["ş"]),
            ('t', &["₺"]),
            ('u', &["ü", "û"]),
        ],
    ),
];

fn rust_table(code: &str, letter: char) -> Vec<String> {
    accents_for_letter(letter, &langs(&[code]))
        .into_iter()
        .map(|c| c.to_string())
        .collect()
}

/// Compare one language letter by letter; returns mismatch descriptions.
fn diff_language(code: &str) -> Vec<String> {
    let (_, upstream, table) = UPSTREAM.iter().find(|(c, _, _)| *c == code).unwrap();
    let mut out = Vec::new();
    for letter in 'a'..='z' {
        let want: Vec<String> = table
            .iter()
            .find(|(l, _)| *l == letter)
            .map(|(_, v)| v.iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();
        let got = rust_table(code, letter);
        if want != got {
            out.push(format!(
                "{upstream} '{letter}': upstream {want:?}, rust {got:?}"
            ));
        }
    }
    out
}

macro_rules! table_test {
    ($name:ident, $code:literal, ignore) => {
        #[test]
        #[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
        fn $name() {
            let d = diff_language($code);
            assert!(d.is_empty(), "{} mismatches:\n{}", d.len(), d.join("\n"));
        }
    };
    ($name:ident, $code:literal) => {
        #[test]
        fn $name() {
            let d = diff_language($code);
            assert!(d.is_empty(), "{} mismatches:\n{}", d.len(), d.join("\n"));
        }
    };
}

table_test!(table_fr_matches_upstream, "fr", ignore);
table_test!(table_de_matches_upstream, "de", ignore);
table_test!(table_es_matches_upstream, "es", ignore);
table_test!(table_pt_matches_upstream, "pt", ignore);
table_test!(table_pl_matches_upstream, "pl", ignore);
table_test!(table_it_matches_upstream, "it", ignore);
table_test!(table_nl_matches_upstream, "nl", ignore);
table_test!(table_sv_matches_upstream, "sv", ignore);
table_test!(table_no_matches_upstream, "no", ignore);
table_test!(table_da_matches_upstream, "da", ignore);
table_test!(table_cs_matches_upstream, "cs", ignore);
table_test!(table_hu_matches_upstream, "hu", ignore);
table_test!(table_ro_matches_upstream, "ro");
table_test!(table_tr_matches_upstream, "tr", ignore);

/// Upstream `Language` enum (47 entries incl. SPECIAL, CUR, IPA, ...).
const UPSTREAM_LANGUAGES: &[&str] = &[
    "SPECIAL", "BE", "BE_CYRL", "BG", "CA", "CRH", "CUR", "CY", "CZ", "DK", "GA", "GD", "DE", "EL",
    "EST", "EPO", "FI", "FR", "GRC", "HR", "HE", "HU", "IS", "IPA", "IT", "KU", "LT", "MK", "MT",
    "MI", "NL", "NO", "PI", "PIE", "PL", "PT", "PJT", "RO", "ROM", "SK", "SL", "SP", "SR",
    "SR_CYRL", "SV", "TK", "VI",
];

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn language_count_matches_upstream() {
    assert_eq!(
        LANGUAGES.len(),
        UPSTREAM_LANGUAGES.len(),
        "rust has {:?}",
        LANGUAGES
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn non_letter_keys_have_accents() {
    // Upstream maps VK_COMMA (quotes), digits (SPECIAL: sub/superscripts,
    // fractions) and VK_SLASH_ (¿). The port only handles a-z.
    let all: Vec<String> = LANGUAGES.iter().map(|s| s.to_string()).collect();
    assert!(
        !accents_for_letter(',', &all).is_empty(),
        "no quotes for ','"
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn uppercase_sharp_s_is_capital_sharp_s() {
    // PowerAccent.ToUpper maps "ß" to "ẞ" (U+1E9E).
    assert_eq!(accents_for_letter('S', &langs(&["de"])), vec!['ẞ']);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn uppercase_dotless_i_is_dotted_capital_i() {
    // PowerAccent.ToUpper maps "ı" to "İ"; upstream TK 'i' = ı, İ, î -> İ, İ(dup), Î.
    // Upstream does not deduplicate after ToUpper, but the first entry must be İ.
    let got = accents_for_letter('I', &langs(&["tr"]));
    assert_eq!(got.first(), Some(&'İ'));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn multi_language_merge_uses_display_order_not_selection_order() {
    // CharacterMappings.Collect orders by DisplayOrder (alphabetical by enum
    // name): DE comes before SP regardless of the order the user picked.
    // DE 'u' = ü ; SP 'u' = ú, ü  -> ü, ú
    assert_eq!(
        accents_for_letter('u', &langs(&["es", "de"])),
        vec!['ü', 'ú']
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn unicode_description_format() {
    // PowerAccent.GetCharacterDescription: "(U+{0:X4}) {1}"
    assert_eq!(
        unicode_description('é'),
        "(U+00E9) LATIN SMALL LETTER E WITH ACUTE"
    );
}

// -- defaults (PowerAccentProperties / PowerAccentSettings) -----------------

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn default_input_delay_is_300ms() {
    assert_eq!(QuickAccentConfig::default().input_delay_ms, 300);
}

#[test]
fn default_hold_duration_is_500ms() {
    assert_eq!(QuickAccentConfig::default().hold_duration_ms, 500);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn default_activation_key_is_both() {
    assert_eq!(
        QuickAccentConfig::default().activation_key,
        ActivationKey::Any
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn default_toolbar_position_is_top_center() {
    assert_eq!(
        QuickAccentConfig::default().toolbar_position,
        ToolbarPosition::TopCenter
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn default_languages_is_all() {
    let d = QuickAccentConfig::default().languages;
    assert_eq!(d.len(), LANGUAGES.len(), "default languages: {d:?}");
}

#[test]
fn other_defaults_match() {
    let d = QuickAccentConfig::default();
    assert!(d.do_not_activate_on_game_mode);
    assert!(!d.show_unicode_description);
    assert!(!d.sort_by_usage_frequency);
    assert!(!d.start_selection_from_the_left);
    assert!(d.excluded_apps.is_empty());
}

// -- toolbar placement (Calculation.GetRawCoordinatesFromPosition) ----------

const SCREEN: ScreenRect = ScreenRect {
    x: 0,
    y: 0,
    width: 1920,
    height: 1080,
};

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn toolbar_edge_offset_is_24() {
    // Calculation.GetRawCoordinatesFromPosition: int offset = 24 (dpi = 1).
    let size = (200, 80);
    assert_eq!(
        toolbar_origin(ToolbarPosition::TopCenter, (0, 0), SCREEN, size),
        (860, 24)
    );
    assert_eq!(
        toolbar_origin(ToolbarPosition::BottomRight, (0, 0), SCREEN, size),
        (1920 - 200 - 24, 1080 - 80 - 24)
    );
    assert_eq!(
        toolbar_origin(ToolbarPosition::Left, (0, 0), SCREEN, size),
        (24, 500)
    );
}

#[test]
fn toolbar_center_matches_upstream() {
    assert_eq!(
        toolbar_origin(ToolbarPosition::Center, (0, 0), SCREEN, (200, 80)),
        (860, 500)
    );
}

#[test]
fn toolbar_on_second_monitor() {
    let right = ScreenRect {
        x: 1920,
        y: 0,
        width: 1280,
        height: 1024,
    };
    let (x, _) = toolbar_origin(ToolbarPosition::TopCenter, (2000, 10), right, (200, 80));
    assert_eq!(x, 1920 + (1280 - 200) / 2);
}

// -- excluded apps (common/utils/excluded_apps.h) ----------------------------

#[test]
fn excluded_app_matches_case_insensitive_name() {
    let cfg = QuickAccentConfig {
        excluded_apps: vec!["FIREFOX".into()],
        ..Default::default()
    };
    assert!(cfg.is_excluded(&["Navigator".into(), "firefox".into()]));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/F-input.md"]
fn excluded_app_matches_name_prefix_like_upstream() {
    // find_app_name_in_path: the excluded row only has to occur in the file
    // name part starting at its first character (e.g. "fire" excludes
    // firefox.exe).
    let cfg = QuickAccentConfig {
        excluded_apps: vec!["fire".into()],
        ..Default::default()
    };
    assert!(cfg.is_excluded(&["firefox".into()]));
}
