//! Syntax highlighting via `syntect` (fancy-regex backend, no C deps).

use std::path::Path;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

/// Don't highlight huge previews (the UI would stall).
const MAX_HIGHLIGHT_BYTES: usize = 256 * 1024;

/// A highlighted run of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HlSpan {
    pub text: String,
    pub color: [u8; 3],
    pub bold: bool,
    pub italic: bool,
}

fn syntax_set() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(|| {
        let mut set = ThemeSet::load_defaults();
        set.themes
            .remove("base16-ocean.dark")
            .or_else(|| set.themes.into_values().next())
            .unwrap_or_default()
    })
}

fn find_syntax<'a>(set: &'a SyntaxSet, path: &Path, text: &str) -> Option<&'a SyntaxReference> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let alias = match ext.to_lowercase().as_str() {
        "tsx" | "ts" | "mts" | "cts" => Some("js"),
        "jsx" | "mjs" | "cjs" => Some("js"),
        "toml" | "ini" | "cfg" | "conf" | "desktop" | "service" => Some("ini"),
        "zsh" | "fish" => Some("sh"),
        "kt" | "kts" => Some("java"),
        "vue" | "svelte" => Some("html"),
        "scss" | "sass" | "less" => Some("css"),
        "jsonc" | "json5" => Some("json"),
        "dockerfile" => Some("sh"),
        _ => None,
    };
    set.find_syntax_by_extension(name)
        .or_else(|| set.find_syntax_by_extension(ext))
        .or_else(|| set.find_syntax_by_extension(&ext.to_lowercase()))
        .or_else(|| alias.and_then(|a| set.find_syntax_by_extension(a)))
        .or_else(|| match name.to_lowercase().as_str() {
            "dockerfile" | "containerfile" | "pkgbuild" => set.find_syntax_by_extension("sh"),
            _ => None,
        })
        .or_else(|| set.find_syntax_by_first_line(text.lines().next().unwrap_or("")))
        .filter(|s| s.name != "Plain Text")
}

/// Highlight `text` according to `path`'s type. Returns None when no syntax
/// matches or the text is too large.
pub fn highlight(path: &Path, text: &str) -> Option<Vec<HlSpan>> {
    if text.len() > MAX_HIGHLIGHT_BYTES {
        return None;
    }
    let set = syntax_set();
    let syntax = find_syntax(set, path, text)?;
    let mut h = HighlightLines::new(syntax, theme());
    let mut spans: Vec<HlSpan> = Vec::new();
    for line in LinesWithEndings::from(text) {
        let ranges = h.highlight_line(line, set).ok()?;
        for (style, piece) in ranges {
            let color = [style.foreground.r, style.foreground.g, style.foreground.b];
            let bold = style.font_style.contains(FontStyle::BOLD);
            let italic = style.font_style.contains(FontStyle::ITALIC);
            // Merge adjacent runs with identical style to keep the job small.
            if let Some(last) = spans.last_mut()
                && last.color == color
                && last.bold == bold
                && last.italic == italic
            {
                last.text.push_str(piece);
            } else {
                spans.push(HlSpan {
                    text: piece.to_string(),
                    color,
                    bold,
                    italic,
                });
            }
        }
    }
    Some(spans)
}

/// Build an egui layout job from highlighted spans.
pub fn to_layout_job(spans: &[HlSpan], font_size: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for s in spans {
        let format = egui::TextFormat {
            font_id: egui::FontId::monospace(font_size),
            color: egui::Color32::from_rgb(s.color[0], s.color[1], s.color[2]),
            italics: s.italic,
            ..Default::default()
        };
        job.append(&s.text, 0.0, format);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlights_rust() {
        let spans = highlight(Path::new("main.rs"), "fn main() {\n    let x = 1;\n}\n").unwrap();
        let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "fn main() {\n    let x = 1;\n}\n");
        let colors: std::collections::HashSet<_> = spans.iter().map(|s| s.color).collect();
        assert!(colors.len() > 1, "expected several colours");
    }

    #[test]
    fn detects_by_name_and_shebang() {
        assert!(highlight(Path::new("Makefile"), "all:\n\techo hi\n").is_some());
        assert!(highlight(Path::new("script"), "#!/bin/bash\necho hi\n").is_some());
        assert!(highlight(Path::new("app.tsx"), "const a = 1;\n").is_some());
        assert!(highlight(Path::new("notes"), "just words\n").is_none());
    }
}
