//! Parity tests against Microsoft PowerToys Advanced Paste.
//! Oracles:
//! - AdvancedPaste.UnitTests/HelpersTests/HtmlToTextHelperTests.cs (HTML -> plain text)
//! - AdvancedPaste.UnitTests/HelpersTests/MarkdownHelperTests.cs and
//!   AdvancedPaste.Core/MarkdownConverter.cs (HtmlAgilityPack clean-up + ReverseMarkdown defaults)
//! - AdvancedPaste.Core/JsonConverter.cs (JSON passthrough, XML via Newtonsoft
//!   SerializeXmlNode, INI, CSV, line array)

use crate::actions::{
    ClipboardContent, PasteAction, html_to_markdown, html_to_text, picker_actions,
};
use crate::config::PastePlainConfig;
use crate::json;

// ---------------------------------------------------------- HTML -> text

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn html_to_text_table() {
    // HtmlToTextHelperTests.ConvertsHtmlWithoutNativeRendering
    let cases: &[(&str, &str)] = &[
        ("", ""),
        (" \r\n\t ", ""),
        ("<p>Offline text file</p>", "Offline text file"),
        ("<p>first</p><p>second</p>", "first\nsecond"),
        (
            "<div>first<br>second<br><br>fourth</div>",
            "first\nsecond\n\nfourth",
        ),
        ("<H1>Heading</H1><P>Paragraph</P>", "Heading\nParagraph"),
        (
            "<p><b>hel</b><i>lo</i> <a href=\"https://example.test\">world</a></p>",
            "hello world",
        ),
        (
            "<div>\n  one\t <b> two </b>\n three \n</div>",
            "one two three",
        ),
        (
            "<ul><li>one</li><li>two<ul><li>nested</li></ul></li></ul>",
            "one\ntwo\nnested",
        ),
        (
            "<table><tr><th>One</th><th>Two</th></tr><tr><td>A</td><td>B</td></tr></table>",
            "One\tTwo\nA\tB",
        ),
        (
            "<p>&lt;tag&gt; &amp; &quot;quote&quot; &#39; &#x1F680; &nbsp;</p>",
            "<tag> & \"quote\" ' \u{1F680} \u{a0}",
        ),
        ("<p>&amp;lt; is decoded once</p>", "&lt; is decoded once"),
        (
            "<html><head><title>hidden</title></head><body>visible<script>bad()</script><style>bad{}</style><!-- hidden --><template>hidden</template></body></html>",
            "visible",
        ),
        ("<div hidden>hidden</div><p>visible</p>", "visible"),
        ("<div>one<br>two<b>three", "one\ntwothree"),
        ("<pre>  first\n\tsecond  \n</pre>", "  first\n\tsecond  \n"),
    ];
    let mut bad = Vec::new();
    for (html, want) in cases {
        let got = html_to_text(html);
        if got != *want {
            bad.push(format!("{html:?}\n    got  {got:?}\n    want {want:?}"));
        }
    }
    assert!(
        bad.is_empty(),
        "{} of {} cases differ:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

#[test]
fn html_to_text_deep_markup() {
    // TraversesDeepMarkupWithoutRecursiveTextExtraction
    let html = format!("{}content{}", "<span>".repeat(5000), "</span>".repeat(5000));
    assert_eq!(html_to_text(&html), "content");
}

#[test]
fn plain_text_prefers_unmodified_text_flavour() {
    // BothClipboardFallbacksPreferUnmodifiedPlainText
    let original = "  plain\ttext\r\n\u{1F680}  ";
    let c = ClipboardContent {
        text: Some(original.into()),
        html: Some("<p>Do not use the HTML alternative</p>".into()),
        png: None,
    };
    assert_eq!(c.plain_text().as_deref(), Some(original));
}

// ------------------------------------------------------- HTML -> Markdown

#[test]
fn markdown_extracts_fragment_from_cf_html_envelope() {
    // MarkdownHelperTests.ToMarkdownAsync_ExtractsHtmlFragmentFromClipboardEnvelope
    let frag = "<p>Hello <strong>world</strong></p>";
    let header = "Version:0.9\r\nStartHTML:0000000105\r\nEndHTML:0000000199\r\nStartFragment:0000000141\r\nEndFragment:0000000163\r\n";
    let html =
        format!("{header}<html><body><!--StartFragment-->{frag}<!--EndFragment--></body></html>");
    let md = html_to_markdown(&html);
    assert!(md.contains("Hello **world**"), "{md}");
    assert!(!md.contains("StartHTML"), "{md}");
    assert!(!md.contains("StartFragment"), "{md}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn markdown_drops_script_sup_and_office_tags() {
    // MarkdownConverter.CleanNodes removes <script>, <sup> and <o:p>.
    let md = html_to_markdown("<p>Text<sup>1</sup><script>alert(1)</script><o:p>office</o:p></p>");
    assert_eq!(md, "Text");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn markdown_reversemarkdown_defaults() {
    // ReverseMarkdown default config: "-" bullets, "#" headings, "*" emphasis,
    // [text](url) links.
    let mut bad = Vec::new();
    for (html, want) in [
        ("<h1>Title</h1>", "# Title"),
        ("<ul><li>a</li><li>b</li></ul>", "- a\n- b"),
        ("<p><em>x</em></p>", "*x*"),
        (
            "<a href=\"https://e.test\">link</a>",
            "[link](https://e.test)",
        ),
        ("<ol><li>a</li><li>b</li></ol>", "1. a\n2. b"),
    ] {
        let got = html_to_markdown(html);
        if got != want {
            bad.push(format!("{html}: got {got:?}, want {want:?}"));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn markdown_collapses_whitespace_in_text_nodes() {
    // CleanNodes: \s{2,} -> " " first (so "\r\n" becomes " "), then any
    // remaining lone CR/LF is removed.
    let md = html_to_markdown("<p>a   b\r\nc</p>");
    assert_eq!(md, "a b c");
    let md = html_to_markdown("<p>a\nb</p>");
    assert_eq!(md, "ab");
}

// ------------------------------------------------------------- JSON

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn json_valid_json_returned_unchanged() {
    // JsonConverter.Convert: `if (IsJson(text)) return text;`
    let input = r#"{"a":1,"b":[1,2]}"#;
    assert_eq!(json::convert(input), input);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn json_xml_declaration_kept() {
    // Newtonsoft SerializeXmlNode(XmlDocument) emits the declaration as "?xml".
    let out = json::convert(r#"<?xml version="1.0" encoding="utf-8"?><root><a>1</a></root>"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"?xml": {"@version": "1.0", "@encoding": "utf-8"}, "root": {"a": "1"}})
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn json_xml_namespace_declaration_as_attribute() {
    // Newtonsoft serialises xmlns declarations as "@xmlns" / "@xmlns:p".
    let out = json::convert(r#"<root xmlns="urn:x" xmlns:p="urn:p"><p:a>1</p:a></root>"#);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        v,
        serde_json::json!({"root": {"@xmlns": "urn:x", "@xmlns:p": "urn:p", "p:a": "1"}})
    );
}

#[test]
fn json_xml_basic_and_indentation() {
    // Newtonsoft Formatting.Indented uses 2 spaces and `"key": value`.
    let out = json::convert("<r><a>1</a><a>2</a><e/></r>");
    assert_eq!(
        out,
        "{\n  \"r\": {\n    \"a\": [\n      \"1\",\n      \"2\"\n    ],\n    \"e\": null\n  }\n}"
    );
}

#[test]
fn json_csv_ini_lines() {
    let v = |s: &str| serde_json::from_str::<serde_json::Value>(&json::convert(s)).unwrap();
    assert_eq!(v("a,b\n1,2"), serde_json::json!([["a", "b"], ["1", "2"]]));
    assert_eq!(
        v("sep=|\na|b\n1|2"),
        serde_json::json!([["a", "b"], ["1", "2"]])
    );
    assert_eq!(
        v("\"x,\"\"y\"\"\",2\n3,4"),
        serde_json::json!([["x,\"y\"", "2"], ["3", "4"]])
    );
    assert_eq!(
        v("[s]\nk = v\n[t]\nk2= v2 "),
        serde_json::json!({"s": {"k": "v"}, "t": {"k2": "v2 "}})
    );
    // Duplicate INI keys throw upstream -> falls through to CSV / lines.
    assert_eq!(v("[s]\nk=1\nk=2"), serde_json::json!(["[s]", "k=1", "k=2"]));
    assert_eq!(
        v("hello\r\nworld\r\n"),
        serde_json::json!(["hello", "world"])
    );
}

// ------------------------------------------------- picker / formats

#[test]
fn picker_lists_core_formats_for_text() {
    let cfg = PastePlainConfig::default();
    let c = ClipboardContent {
        text: Some("x".into()),
        ..Default::default()
    };
    let a = picker_actions(&cfg, &c);
    assert_eq!(
        &a[..3],
        &[
            PasteAction::PlainText,
            PasteAction::Markdown,
            PasteAction::Json
        ]
    );
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn json_available_for_html_only_clipboard() {
    // Upstream PasteFormats: Json supports Text, and HTML clipboard data
    // always includes a text alternative on Windows. With only text/html on a
    // Linux clipboard, Markdown is offered; JSON should be as well because
    // transform() can derive text from HTML.
    let cfg = PastePlainConfig::default();
    let c = ClipboardContent {
        html: Some("<p>a,b</p>".into()),
        ..Default::default()
    };
    let a = picker_actions(&cfg, &c);
    assert!(a.contains(&PasteAction::Markdown));
    assert!(a.contains(&PasteAction::Json), "{a:?}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/G-launch.md"]
fn default_shortcuts_match_powertoys() {
    // AdvancedPasteProperties: UI = Win+Shift+V, plain text = Win+Ctrl+Alt+V,
    // Markdown and JSON unbound by default.
    let cfg = PastePlainConfig::default();
    assert_eq!(cfg.advanced_paste_ui_shortcut, "Super+Shift+V");
    assert_eq!(cfg.paste_as_markdown_shortcut, "");
    assert_eq!(cfg.paste_as_json_shortcut, "");
}
