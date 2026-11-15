//! Advanced Paste formats (upstream `PasteFormats`) and their transformations.

use crate::clipboard;
use crate::config::PastePlainConfig;
use anyhow::{Context, Result};
use std::path::PathBuf;
use std::process::Command;
use tracing::info;

/// Snapshot of what the clipboard currently offers.
#[derive(Debug, Clone, Default)]
pub struct ClipboardContent {
    pub text: Option<String>,
    pub html: Option<String>,
    pub png: Option<Vec<u8>>,
}

impl ClipboardContent {
    /// Read text, HTML and PNG flavours from the clipboard.
    pub fn read() -> Self {
        let types = clipboard::list_types();
        let offers = |m: &str| types.is_empty() || types.iter().any(|t| t == m);
        let has_text = types.is_empty()
            || types.iter().any(|t| {
                t.starts_with("text/plain")
                    || matches!(t.as_str(), "UTF8_STRING" | "STRING" | "TEXT")
            });
        let text = if has_text {
            clipboard::get_text().ok().filter(|t| !t.is_empty())
        } else {
            None
        };
        let html = if offers("text/html") {
            clipboard::get_html()
        } else {
            None
        };
        let png = if offers("image/png") {
            clipboard::get_png()
        } else {
            None
        };
        Self { text, html, png }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.html.is_none() && self.png.is_none()
    }

    /// Best plain-text view: text flavour, else HTML stripped of tags.
    pub fn plain_text(&self) -> Option<String> {
        self.text
            .clone()
            .or_else(|| self.html.as_deref().map(html_to_text))
            .filter(|t| !t.trim().is_empty())
    }
}

/// A paste format. Order of variants is the default picker order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteAction {
    PlainText,
    Markdown,
    Json,
    ImageToText,
    TxtFile,
    PngFile,
    JpgFile,
    HtmlFile,
    FixSpellingAndGrammar,
    /// Index into `PastePlainConfig::custom_actions`.
    Custom(usize),
    /// Free-form AI instructions typed in the picker.
    Prompt(String),
}

impl PasteAction {
    pub fn label(&self, cfg: &PastePlainConfig) -> String {
        match self {
            Self::PlainText => "Paste as plain text".into(),
            Self::Markdown => "Paste as Markdown".into(),
            Self::Json => "Paste as JSON".into(),
            Self::ImageToText => "Image to text".into(),
            Self::TxtFile => "Paste as .txt file".into(),
            Self::PngFile => "Paste as .png file".into(),
            Self::JpgFile => "Paste as .jpg file".into(),
            Self::HtmlFile => "Paste as .html file".into(),
            Self::FixSpellingAndGrammar => "Fix spelling and grammar".into(),
            Self::Custom(i) => cfg
                .custom_actions
                .get(*i)
                .map(|a| a.name.clone())
                .unwrap_or_else(|| format!("Custom action {}", i + 1)),
            Self::Prompt(p) => format!("AI: {p}"),
        }
    }

    pub fn requires_ai(&self) -> bool {
        matches!(
            self,
            Self::FixSpellingAndGrammar | Self::Custom(_) | Self::Prompt(_)
        )
    }

    /// Whether the action can run on the given clipboard content
    /// (upstream `SupportedClipboardFormats`).
    pub fn supports(&self, c: &ClipboardContent) -> bool {
        let text = c.text.is_some();
        let html = c.html.is_some();
        let image = c.png.is_some();
        match self {
            Self::PlainText | Self::Json => text,
            Self::Markdown | Self::TxtFile => text || html,
            Self::HtmlFile => html,
            Self::ImageToText | Self::PngFile | Self::JpgFile => image,
            Self::FixSpellingAndGrammar | Self::Custom(_) | Self::Prompt(_) => text || html,
        }
    }
}

/// Actions listed in the picker for this config and clipboard content.
pub fn picker_actions(cfg: &PastePlainConfig, content: &ClipboardContent) -> Vec<PasteAction> {
    let mut v = vec![
        PasteAction::PlainText,
        PasteAction::Markdown,
        PasteAction::Json,
    ];
    if cfg.show_image_to_text {
        v.push(PasteAction::ImageToText);
    }
    if cfg.show_paste_as_file {
        v.extend([
            PasteAction::TxtFile,
            PasteAction::PngFile,
            PasteAction::JpgFile,
            PasteAction::HtmlFile,
        ]);
    }
    if cfg.ai_available() {
        if cfg.show_fix_spelling_and_grammar {
            v.push(PasteAction::FixSpellingAndGrammar);
        }
        v.extend(
            cfg.visible_custom_actions()
                .map(|(i, _)| PasteAction::Custom(i)),
        );
    }
    v.retain(|a| a.supports(content));
    v
}

/// Result of a transformation, ready to be put on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Output {
    Text(String),
    Files(Vec<PathBuf>),
}

impl Output {
    /// Put the output on the clipboard.
    pub fn store(&self) -> Result<()> {
        match self {
            Self::Text(t) => clipboard::set_text(t),
            Self::Files(f) => clipboard::set_uri_list(f),
        }
    }
}

/// Run an action on the clipboard content.
pub fn transform(
    action: &PasteAction,
    content: &ClipboardContent,
    cfg: &PastePlainConfig,
) -> Result<Output> {
    let need_text = || content.plain_text().context("clipboard has no text");
    let out = match action {
        PasteAction::PlainText => Output::Text(need_text()?),
        PasteAction::Markdown => Output::Text(match &content.html {
            Some(html) => html_to_markdown(html),
            None => need_text()?,
        }),
        PasteAction::Json => Output::Text(crate::json::convert(&need_text()?)),
        PasteAction::ImageToText => {
            let png = content.png.as_deref().context("clipboard has no image")?;
            Output::Text(ocr_png(png)?)
        }
        PasteAction::TxtFile => Output::Files(vec![write_temp("txt", need_text()?.as_bytes())?]),
        PasteAction::HtmlFile => {
            let html = content.html.as_deref().context("clipboard has no HTML")?;
            Output::Files(vec![write_temp("html", html.as_bytes())?])
        }
        PasteAction::PngFile => {
            let png = content.png.as_deref().context("clipboard has no image")?;
            Output::Files(vec![write_temp("png", png)?])
        }
        PasteAction::JpgFile => {
            let png = content.png.as_deref().context("clipboard has no image")?;
            let jpg = png_to_jpeg(png, cfg.paste_as_jpg_quality)?;
            Output::Files(vec![write_temp("jpg", &jpg)?])
        }
        PasteAction::FixSpellingAndGrammar => Output::Text(crate::ai::transform(
            cfg,
            crate::ai::FIX_SPELLING_PROMPT,
            &need_text()?,
        )?),
        PasteAction::Custom(i) => {
            let a = cfg
                .custom_actions
                .get(*i)
                .context("custom action no longer exists")?;
            Output::Text(crate::ai::transform(cfg, &a.prompt, &need_text()?)?)
        }
        PasteAction::Prompt(p) => Output::Text(crate::ai::transform(cfg, p, &need_text()?)?),
    };
    Ok(out)
}

/// Read the clipboard, run `action`, store the result and send Ctrl+V.
pub fn run_and_paste(action: &PasteAction, cfg: &PastePlainConfig) -> Result<()> {
    let content = ClipboardContent::read();
    if content.is_empty() {
        info!("Clipboard is empty, nothing to paste");
        return Ok(());
    }
    if !action.supports(&content) {
        anyhow::bail!(
            "'{}' is not available for the current clipboard content",
            action.label(cfg)
        );
    }
    let out = transform(action, &content, cfg)?;
    out.store()?;
    clipboard::simulate_paste()?;
    info!("Advanced Paste: {}", action.label(cfg));
    Ok(())
}

/// Convert HTML to Markdown.
pub fn html_to_markdown(html: &str) -> String {
    html2md::parse_html(strip_cf_html_header(html))
        .trim()
        .to_string()
}

/// Some apps prefix HTML with a `<meta charset>` or Windows CF_HTML header; drop the header.
fn strip_cf_html_header(html: &str) -> &str {
    if html.starts_with("Version:")
        && let Some(i) = html.find('<')
    {
        return &html[i..];
    }
    html
}

/// Crude HTML → text: drop tags, scripts/styles, decode common entities.
pub fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = strip_cf_html_header(html);
    while let Some(lt) = rest.find('<') {
        out.push_str(&rest[..lt]);
        let after = &rest[lt..];
        let Some(gt) = after.find('>') else {
            rest = "";
            break;
        };
        let tag = after[1..gt].trim().to_ascii_lowercase();
        rest = &after[gt + 1..];
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        if (name == "script" || name == "style") && !tag.starts_with('/') {
            let close = format!("</{name}");
            match rest.to_ascii_lowercase().find(&close) {
                Some(i) => rest = &rest[i..],
                None => rest = "",
            }
            continue;
        }
        if matches!(
            name.as_str(),
            "br" | "p" | "div" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
        ) && (tag.starts_with('/') || name == "br")
        {
            out.push('\n');
        }
    }
    out.push_str(rest);
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded.trim().to_string()
}

fn temp_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join("mpt-advanced-paste");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    Ok(dir)
}

/// Write data to a fresh temp file named `clipboard-<timestamp>.<ext>`.
fn write_temp(ext: &str, data: &[u8]) -> Result<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = temp_dir()?.join(format!("clipboard-{ts}.{ext}"));
    std::fs::write(&path, data).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn png_to_jpeg(png: &[u8], quality: u8) -> Result<Vec<u8>> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .context("clipboard image is not a valid PNG")?;
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
    rgb.write_with_encoder(enc)
        .context("JPEG encoding failed")?;
    Ok(out)
}

/// OCR a PNG with tesseract, using the Text Extractor language if configured.
fn ocr_png(png: &[u8]) -> Result<String> {
    #[derive(serde::Deserialize, Default)]
    struct OcrConf {
        #[serde(default)]
        language: String,
    }
    let lang = mpt_common::config::load_module_config::<OcrConf>("text-extractor")
        .map(|c| c.language)
        .ok()
        .filter(|l| !l.trim().is_empty())
        .unwrap_or_else(|| "eng".into());
    let path = write_temp("png", png)?;
    let out = Command::new("tesseract")
        .arg(&path)
        .arg("stdout")
        .args(["-l", &lang])
        .output()
        .context("failed to run tesseract — is tesseract-ocr installed?");
    let _ = std::fs::remove_file(&path);
    let out = out?;
    if !out.status.success() {
        anyhow::bail!(
            "tesseract failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CustomAction;

    fn text(s: &str) -> ClipboardContent {
        ClipboardContent {
            text: Some(s.into()),
            ..Default::default()
        }
    }

    #[test]
    fn markdown_from_html() {
        let md = html_to_markdown("<h1>Title</h1><p>Some <b>bold</b> text</p>");
        assert!(md.contains("Title"), "{md}");
        assert!(md.contains("**bold**"), "{md}");
    }

    #[test]
    fn html_text_stripping() {
        assert_eq!(
            html_to_text("<style>x{}</style><p>a &amp; b</p><p>c</p>"),
            "a & b\nc"
        );
    }

    #[test]
    fn picker_filters_by_content_and_ai() {
        let mut cfg = PastePlainConfig::default();
        cfg.custom_actions.push(CustomAction {
            name: "Translate".into(),
            prompt: "to French".into(),
            shortcut: None,
            is_shown: true,
        });
        let actions = picker_actions(&cfg, &text("hi"));
        assert!(actions.contains(&PasteAction::Json));
        assert!(actions.contains(&PasteAction::TxtFile));
        assert!(!actions.contains(&PasteAction::PngFile));
        assert!(!actions.iter().any(PasteAction::requires_ai));

        cfg.ai_enabled = true;
        cfg.ai_endpoint = "http://localhost/v1".into();
        let actions = picker_actions(&cfg, &text("hi"));
        assert!(actions.contains(&PasteAction::Custom(0)));
        assert!(actions.contains(&PasteAction::FixSpellingAndGrammar));

        let image = ClipboardContent {
            png: Some(vec![1]),
            ..Default::default()
        };
        let actions = picker_actions(&cfg, &image);
        assert!(actions.contains(&PasteAction::PngFile));
        assert!(!actions.contains(&PasteAction::PlainText));
    }

    #[test]
    fn json_transform_and_txt_file() {
        let cfg = PastePlainConfig::default();
        let out = transform(&PasteAction::Json, &text("a,b\n1,2"), &cfg).unwrap();
        assert!(matches!(out, Output::Text(ref t) if t.starts_with('[')));
        let out = transform(&PasteAction::TxtFile, &text("hello"), &cfg).unwrap();
        let Output::Files(files) = out else {
            panic!("expected files")
        };
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), "hello");
        let _ = std::fs::remove_file(&files[0]);
    }

    #[test]
    fn jpeg_conversion() {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let jpg = png_to_jpeg(&png, 90).unwrap();
        assert_eq!(&jpg[..2], &[0xFF, 0xD8]);
    }
}
