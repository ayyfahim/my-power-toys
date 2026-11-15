//! Minimal Markdown rendering: pulldown-cmark events → simple blocks → egui.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Inline style flags.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub strike: bool,
    pub code: bool,
    pub link: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading(u8, Vec<Span>),
    Paragraph(Vec<Span>),
    /// `marker` is "•" or "N." ; `depth` starts at 0.
    ListItem {
        depth: usize,
        marker: String,
        spans: Vec<Span>,
    },
    Quote(Vec<Span>),
    Code {
        lang: String,
        text: String,
    },
    /// Rows of cells; first row is the header.
    Table(Vec<Vec<String>>),
    Rule,
}

/// Parse Markdown into blocks.
pub fn parse(md: &str) -> Vec<Block> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);

    let mut blocks = Vec::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut style = Style::default();
    // Stack of list counters: None = bullet, Some(n) = next ordered number.
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut item_marker: Option<String> = None;
    let mut quote_depth = 0usize;
    let mut heading: Option<u8> = None;
    let mut code: Option<(String, String)> = None;
    let mut table: Option<Vec<Vec<String>>> = None;
    let mut cell = String::new();

    let push_text = |spans: &mut Vec<Span>, text: &str, style: Style| {
        if let Some(last) = spans.last_mut()
            && last.style == style
        {
            last.text.push_str(text);
        } else {
            spans.push(Span {
                text: text.to_string(),
                style,
            });
        }
    };

    // Flush accumulated inline spans into a block.
    macro_rules! flush {
        () => {
            if !spans.is_empty() {
                let taken = std::mem::take(&mut spans);
                if let Some(level) = heading {
                    blocks.push(Block::Heading(level, taken));
                } else if let Some(marker) = item_marker.take() {
                    blocks.push(Block::ListItem {
                        depth: lists.len().saturating_sub(1),
                        marker,
                        spans: taken,
                    });
                } else if quote_depth > 0 {
                    blocks.push(Block::Quote(taken));
                } else {
                    blocks.push(Block::Paragraph(taken));
                }
            }
        };
    }

    for event in Parser::new_ext(md, opts) {
        if let Some((_, text)) = code.as_mut() {
            match event {
                Event::Text(t) => {
                    text.push_str(&t);
                    continue;
                }
                Event::End(TagEnd::CodeBlock) => {
                    let (lang, text) = code.take().unwrap_or_default();
                    blocks.push(Block::Code {
                        lang,
                        text: text.trim_end_matches('\n').to_string(),
                    });
                    continue;
                }
                _ => continue,
            }
        }
        if let Some(rows) = table.as_mut() {
            match event {
                Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => rows.push(Vec::new()),
                Event::End(TagEnd::TableCell) => {
                    if let Some(row) = rows.last_mut() {
                        row.push(std::mem::take(&mut cell).trim().to_string());
                    }
                }
                Event::Text(t) | Event::Code(t) => cell.push_str(&t),
                Event::End(TagEnd::Table) => {
                    blocks.push(Block::Table(table.take().unwrap_or_default()));
                }
                _ => {}
            }
            continue;
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush!();
                heading = Some(match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                });
            }
            Event::End(TagEnd::Heading(_)) => {
                flush!();
                heading = None;
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => flush!(),
            Event::Start(Tag::BlockQuote(_)) => {
                flush!();
                quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush!();
                quote_depth = quote_depth.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                flush!();
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                flush!();
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                flush!();
                let marker = match lists.last_mut() {
                    Some(Some(n)) => {
                        let m = format!("{n}.");
                        *n += 1;
                        m
                    }
                    _ => "\u{2022}".to_string(),
                };
                item_marker = Some(marker);
            }
            Event::End(TagEnd::Item) => flush!(),
            Event::TaskListMarker(done) => {
                item_marker = Some(if done { "\u{2611}" } else { "\u{2610}" }.to_string());
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush!();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                code = Some((lang, String::new()));
            }
            Event::Start(Tag::Table(_)) => {
                flush!();
                table = Some(Vec::new());
            }
            Event::Start(Tag::Emphasis) => style.italic = true,
            Event::End(TagEnd::Emphasis) => style.italic = false,
            Event::Start(Tag::Strong) => style.bold = true,
            Event::End(TagEnd::Strong) => style.bold = false,
            Event::Start(Tag::Strikethrough) => style.strike = true,
            Event::End(TagEnd::Strikethrough) => style.strike = false,
            Event::Start(Tag::Link { .. }) => style.link = true,
            Event::End(TagEnd::Link) => style.link = false,
            Event::Start(Tag::Image { dest_url, .. }) => {
                push_text(&mut spans, &format!("[image: {dest_url}] "), style);
            }
            Event::Text(t) => push_text(&mut spans, &t, style),
            Event::Code(t) => push_text(
                &mut spans,
                &t,
                Style {
                    code: true,
                    ..style
                },
            ),
            Event::SoftBreak => push_text(&mut spans, " ", style),
            Event::HardBreak => push_text(&mut spans, "\n", style),
            Event::Rule => {
                flush!();
                blocks.push(Block::Rule);
            }
            Event::Html(h) | Event::InlineHtml(h) => {
                let t = h.trim();
                if !t.is_empty() && !t.starts_with("<!--") {
                    push_text(&mut spans, t, style);
                }
            }
            _ => {}
        }
    }
    flush!();
    blocks
}

fn spans_job(
    spans: &[Span],
    size: f32,
    base: egui::Color32,
    accent: egui::Color32,
    force_bold: bool,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    for s in spans {
        let font = if s.style.code {
            egui::FontId::monospace(size * 0.92)
        } else {
            egui::FontId::proportional(size)
        };
        let mut color = if s.style.link { accent } else { base };
        if (s.style.bold || force_bold) && !s.style.link {
            color = egui::Color32::WHITE;
        }
        let format = egui::TextFormat {
            font_id: font,
            color,
            italics: s.style.italic,
            strikethrough: if s.style.strike {
                egui::Stroke::new(1.0, base)
            } else {
                egui::Stroke::NONE
            },
            underline: if s.style.link {
                egui::Stroke::new(1.0, accent)
            } else {
                egui::Stroke::NONE
            },
            background: if s.style.code {
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18)
            } else {
                egui::Color32::TRANSPARENT
            },
            ..Default::default()
        };
        job.append(&s.text, 0.0, format);
    }
    job.wrap.max_width = f32::INFINITY;
    job
}

/// Render parsed blocks.
pub fn render(
    ui: &mut egui::Ui,
    blocks: &[Block],
    body: f32,
    text: egui::Color32,
    accent: egui::Color32,
) {
    ui.spacing_mut().item_spacing.y = 6.0;
    for block in blocks {
        match block {
            Block::Heading(level, spans) => {
                let size = match level {
                    1 => body * 1.8,
                    2 => body * 1.5,
                    3 => body * 1.25,
                    _ => body * 1.1,
                };
                ui.add_space(4.0);
                ui.add(egui::Label::new(spans_job(spans, size, text, accent, true)).wrap());
                if *level <= 2 {
                    ui.separator();
                }
            }
            Block::Paragraph(spans) => {
                ui.add(egui::Label::new(spans_job(spans, body, text, accent, false)).wrap());
            }
            Block::ListItem {
                depth,
                marker,
                spans,
            } => {
                ui.horizontal_wrapped(|ui| {
                    ui.add_space(12.0 + *depth as f32 * 16.0);
                    ui.label(egui::RichText::new(marker).size(body).color(accent));
                    ui.add(egui::Label::new(spans_job(spans, body, text, accent, false)).wrap());
                });
            }
            Block::Quote(spans) => {
                egui::Frame::NONE
                    .inner_margin(egui::Margin {
                        left: 10,
                        right: 4,
                        top: 2,
                        bottom: 2,
                    })
                    .show(ui, |ui| {
                        let r = ui.max_rect();
                        ui.painter().line_segment(
                            [
                                r.left_top() - egui::vec2(8.0, 0.0),
                                r.left_bottom() - egui::vec2(8.0, 0.0),
                            ],
                            egui::Stroke::new(3.0, accent.gamma_multiply(0.6)),
                        );
                        ui.add(
                            egui::Label::new(spans_job(
                                spans,
                                body,
                                text.gamma_multiply(0.85),
                                accent,
                                false,
                            ))
                            .wrap(),
                        );
                    });
            }
            Block::Code { text: code, .. } => {
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 90))
                    .corner_radius(6.0)
                    .inner_margin(egui::Margin::same(8))
                    .show(ui, |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt(code.as_ptr() as usize)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(code)
                                        .family(egui::FontFamily::Monospace)
                                        .size(body * 0.92)
                                        .color(text),
                                );
                            });
                    });
            }
            Block::Table(rows) => {
                egui::Grid::new(rows.as_ptr() as usize)
                    .striped(true)
                    .spacing([16.0, 4.0])
                    .show(ui, |ui| {
                        for (i, row) in rows.iter().enumerate() {
                            for cell in row {
                                let t = egui::RichText::new(cell).size(body).color(text);
                                ui.label(if i == 0 { t.strong() } else { t });
                            }
                            ui.end_row();
                        }
                    });
            }
            Block::Rule => {
                ui.separator();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(spans: &[Span]) -> String {
        spans.iter().map(|s| s.text.as_str()).collect()
    }

    #[test]
    fn parses_common_blocks() {
        let md = "# Title\n\nSome **bold** and `code`.\n\n- one\n- two\n\n1. first\n2. second\n\n> quoted\n\n```rust\nfn x() {}\n```\n\n---\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        let b = parse(md);
        assert!(matches!(&b[0], Block::Heading(1, s) if plain(s) == "Title"));
        match &b[1] {
            Block::Paragraph(s) => {
                assert_eq!(plain(s), "Some bold and code.");
                assert!(s.iter().any(|x| x.style.bold && x.text == "bold"));
                assert!(s.iter().any(|x| x.style.code && x.text == "code"));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert!(matches!(&b[2], Block::ListItem { marker, .. } if marker == "\u{2022}"));
        assert!(matches!(&b[4], Block::ListItem { marker, .. } if marker == "1."));
        assert!(matches!(&b[5], Block::ListItem { marker, .. } if marker == "2."));
        assert!(matches!(&b[6], Block::Quote(s) if plain(s) == "quoted"));
        assert!(
            matches!(&b[7], Block::Code { lang, text } if lang == "rust" && text == "fn x() {}")
        );
        assert_eq!(b[8], Block::Rule);
        assert_eq!(
            b[9],
            Block::Table(vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["1".to_string(), "2".to_string()]
            ])
        );
    }

    #[test]
    fn nested_lists_have_depth() {
        let b = parse("- a\n  - b\n");
        assert!(matches!(&b[0], Block::ListItem { depth: 0, .. }));
        assert!(matches!(&b[1], Block::ListItem { depth: 1, .. }));
    }

    #[test]
    fn task_list_markers() {
        let b = parse("- [x] done\n- [ ] todo\n");
        assert!(matches!(&b[0], Block::ListItem { marker, .. } if marker == "\u{2611}"));
        assert!(matches!(&b[1], Block::ListItem { marker, .. } if marker == "\u{2610}"));
    }
}
