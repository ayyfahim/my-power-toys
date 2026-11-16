//! Parity tests against Microsoft PowerToys Text Extractor (PowerOCR).
//!
//! Oracles:
//! - src/modules/PowerOCR/PowerOCR.Core/Formatting/OcrTextFormatter.cs (+ OcrTextFormatterTests.cs)
//! - src/modules/PowerOCR/PowerOCR.Core/Formatting/TableTextFormatter.cs (+ TableTextFormatterTests.cs)
//! - src/settings-ui/Settings.UI.Library/PowerOcrProperties.cs
//!
//! PowerToys formats Windows.Media.Ocr words/lines; the Rust port formats
//! tesseract text / TSV. Tests below feed the tesseract equivalent of the
//! upstream fixtures.

use crate::TextExtractorConfig;
use crate::ocr::{
    OcrMode, join_single_line, parse_language_list, postprocess, run_tesseract, tsv_to_table,
};

const HEADER: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";

/// One TSV word row (level 5). Upstream `Cell(text, x, y)` is 40x20.
fn w(line: u32, word: u32, left: i32, top: i32, text: &str) -> String {
    format!("5\t1\t1\t1\t{line}\t{word}\t{left}\t{top}\t40\t20\t96.0\t{text}\n")
}

const PLAIN: OcrMode = OcrMode {
    single_line: false,
    table: false,
};
const SINGLE: OcrMode = OcrMode {
    single_line: true,
    table: false,
};

// ---------------------------------------------------------------------------
// tesseract missing (not installed in this sandbox)
// ---------------------------------------------------------------------------

#[test]
fn missing_tesseract_is_an_error_not_a_panic() {
    let img = std::env::temp_dir().join("mpt-parity-missing-tesseract.png");
    std::fs::write(&img, b"not really a png").unwrap();
    let r = run_tesseract(&img, "eng", PLAIN);
    let _ = std::fs::remove_file(&img);
    if which("tesseract") {
        return; // only meaningful when tesseract is absent
    }
    let e = r.expect_err("tesseract absent must yield Err");
    assert!(format!("{e:#}").contains("tesseract"), "{e:#}");
}

#[test]
fn missing_tesseract_lists_no_languages() {
    if !which("tesseract") {
        assert!(crate::ocr::list_languages().is_empty());
    }
}

fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).any(|d| d.join(bin).is_file()))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// OcrTextFormatter
// ---------------------------------------------------------------------------

/// FormatDocument_LatinLanguage_UsesOcrLineText
#[test]
fn latin_text_is_kept() {
    assert_eq!(postprocess("Hello world\n", PLAIN), "Hello world");
    assert_eq!(
        postprocess("Hello world\nsecond line\n\n", PLAIN),
        "Hello world\nsecond line"
    );
}

/// CollapseToSingleLine: newlines -> spaces, repeated spaces collapsed, trimmed.
#[test]
fn single_line_collapses_newlines_and_spaces() {
    assert_eq!(
        postprocess("Hello\nworld\r\nagain   twice \n", SINGLE),
        "Hello world again twice"
    );
    assert_eq!(join_single_line("   "), "");
}

/// FormatDocument_ChineseLanguage_JoinsSingleCharacterWords. tesseract
/// (chi_sim/jpn) emits a space between CJK glyphs, e.g. "你 好".
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn cjk_glyphs_are_joined_without_spaces() {
    assert_eq!(postprocess("你 好\n", PLAIN), "你好");
    assert_eq!(postprocess("日 本\n", PLAIN), "日本");
}

/// FormatDocument_CjkLanguage_DoesNotInsertSpaceBeforePunctuation /
/// JoinCjkAwareWords_MultipleCjkCharactersPerWord_DoesNotInsertSpaces
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn cjk_punctuation_is_tight() {
    assert_eq!(postprocess("2026 。\n", PLAIN), "2026。");
    assert_eq!(postprocess("中文 测试 。\n", SINGLE), "中文测试。");
}

/// FormatCjkText_PunctuationPrefixedWord_PreservesWordSpacing (Latin words keep spaces).
#[test]
fn latin_words_inside_cjk_text_keep_spaces() {
    for s in ["echo %PATH%", "open .gitignore", "Use .NET", "value .5"] {
        assert_eq!(postprocess(s, PLAIN), s);
        assert_eq!(postprocess(s, SINGLE), s);
    }
}

/// FormatSingleLine_ChineseLanguage_PreservesSpaceBetweenLatinWordsAcrossLines
#[test]
fn single_line_keeps_space_between_latin_lines() {
    assert_eq!(postprocess("Power\nToys\n", SINGLE), "Power Toys");
}

// ---------------------------------------------------------------------------
// TableTextFormatter
// ---------------------------------------------------------------------------

/// Format_TwoByTwoGrid_UsesTabsAndNewLines
#[test]
fn table_two_by_two() {
    let tsv = format!(
        "{HEADER}{}{}{}{}",
        w(1, 1, 0, 0, "A1"),
        w(1, 2, 100, 0, "B1"),
        w(2, 1, 0, 40, "A2"),
        w(2, 2, 100, 40, "B2")
    );
    assert_eq!(tsv_to_table(&tsv), "A1\tB1\nA2\tB2");
}

/// Format_SparseSecondRow_PreservesEmptyColumn
#[test]
fn table_sparse_row_keeps_empty_cell() {
    let tsv = format!(
        "{HEADER}{}{}{}",
        w(1, 1, 0, 0, "A1"),
        w(1, 2, 100, 0, "B1"),
        w(2, 1, 100, 40, "B2")
    );
    assert_eq!(tsv_to_table(&tsv), "A1\tB1\n\tB2");
}

/// Format_ColumnsSeparatedByOnePixelGap_RemainSeparate. Upstream cells are OCR
/// *lines*; the Rust port works on tesseract *words* and merges words closer
/// than one text height, so two 1-px-apart cells become one cell.
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn table_columns_one_pixel_apart_stay_separate() {
    let tsv = format!("{HEADER}{}{}", w(1, 1, 0, 0, "A1"), w(1, 2, 41, 0, "B1"));
    assert_eq!(tsv_to_table(&tsv), "A1\tB1");
}

/// Format_CjkFragmentsInSameCell_DoesNotInsertSpace
#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/D-screen.md"]
fn table_cjk_fragments_in_same_cell_have_no_space() {
    let tsv = format!("{HEADER}{}{}", w(1, 1, 0, 0, "你"), w(1, 2, 10, 0, "好"));
    assert_eq!(tsv_to_table(&tsv), "你好");
}

/// Table mode takes precedence over single line.
#[test]
fn table_wins_over_single_line() {
    let tsv = format!("{HEADER}{}{}", w(1, 1, 0, 0, "A1"), w(1, 2, 100, 0, "B1"));
    assert_eq!(
        postprocess(
            &tsv,
            OcrMode {
                single_line: true,
                table: true
            }
        ),
        "A1\tB1"
    );
}

// ---------------------------------------------------------------------------
// Languages / settings
// ---------------------------------------------------------------------------

#[test]
fn language_list_parsing_variants() {
    // tesseract 4/5 format, with a script/ dir entry and osd.
    let out = "List of available languages in \"/usr/share/tesseract/tessdata/\" (5):\nchi_sim\neng\nosd\nscript/Latin\njpn\n";
    let langs = parse_language_list(out);
    assert!(langs.contains(&"chi_sim".to_string()) && langs.contains(&"jpn".to_string()));
    assert!(!langs.contains(&"osd".to_string()));
}

/// PowerToys: PreferredLanguage default empty = use the OS/UI language.
/// Rust: hard-coded "eng" (documented difference, asserted as-is).
#[test]
fn default_language() {
    assert_eq!(TextExtractorConfig::default().language, "eng");
}
