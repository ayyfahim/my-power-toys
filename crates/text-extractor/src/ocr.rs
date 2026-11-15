use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;
use tracing::info;

/// How tesseract output is post-processed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OcrMode {
    /// Join all lines into a single line separated by spaces.
    pub single_line: bool,
    /// Table mode: `--psm 6` + TSV output, rebuilt as tab-separated columns.
    /// Takes precedence over `single_line`.
    pub table: bool,
}

/// Capture a screen region and run OCR on it.
///
/// Flow:
/// 1. Let user select a region via screenshot tool (gnome-screenshot -a, grim -g "$(slurp)", etc.)
/// 2. Run tesseract on the captured image
/// 3. Return the extracted text
pub fn capture_and_extract(lang: &str) -> Result<String> {
    capture_and_extract_with(lang, OcrMode::default())
}

/// Headless capture + OCR with a post-processing mode.
pub fn capture_and_extract_with(lang: &str, mode: OcrMode) -> Result<String> {
    let tmp_img = std::env::temp_dir().join("mpt-ocr-capture.png");
    let tmp_img_str = tmp_img.to_string_lossy();

    // Step 1: Capture a region
    let captured = capture_region(&tmp_img_str)?;
    if !captured {
        anyhow::bail!("screen capture cancelled or failed");
    }

    // Step 2: Run tesseract OCR
    let text = run_tesseract(&tmp_img, lang, mode);

    // Cleanup
    let _ = std::fs::remove_file(&tmp_img);

    let text = text?;
    info!("OCR extracted: {} characters", text.len());
    Ok(text)
}

/// Run tesseract on an image file and post-process the result according to `mode`.
pub fn run_tesseract(image: &Path, lang: &str, mode: OcrMode) -> Result<String> {
    let mut cmd = Command::new("tesseract");
    cmd.arg(image).arg("stdout").args(["-l", lang]);
    if mode.table {
        cmd.args(["--psm", "6", "tsv"]);
    }
    let output = cmd
        .output()
        .context("failed to run tesseract — is tesseract-ocr installed?")?;

    if !output.status.success() {
        anyhow::bail!(
            "tesseract failed with exit code {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    Ok(postprocess(&raw, mode))
}

/// Apply the OCR mode to raw tesseract output (plain text, or TSV in table mode).
pub fn postprocess(raw: &str, mode: OcrMode) -> String {
    if mode.table {
        tsv_to_table(raw)
    } else if mode.single_line {
        join_single_line(raw)
    } else {
        raw.trim().to_string()
    }
}

/// Collapse all whitespace (including newlines) into single spaces.
pub fn join_single_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Debug, Clone)]
struct Word {
    line: (u32, u32, u32),
    left: i32,
    right: i32,
    height: i32,
    text: String,
}

fn parse_tsv_words(tsv: &str) -> Vec<Word> {
    let mut words = Vec::new();
    for line in tsv.lines().skip(1) {
        let cols: Vec<&str> = line.splitn(12, '\t').collect();
        if cols.len() < 12 || cols[0] != "5" {
            continue;
        }
        let num = |i: usize| cols[i].trim().parse::<i64>().unwrap_or(0);
        let text = cols[11].trim();
        if text.is_empty() || cols[10].trim().starts_with('-') {
            continue;
        }
        words.push(Word {
            line: (num(2) as u32, num(3) as u32, num(4) as u32),
            left: num(6) as i32,
            right: (num(6) + num(8)) as i32,
            height: num(9) as i32,
            text: text.to_string(),
        });
    }
    words
}

/// Rebuild a table from tesseract TSV output.
///
/// Column boundaries are found globally: word x-ranges from all rows are
/// merged when closer than ~one text height, and each resulting span becomes
/// a column. Cells are separated by tabs, rows by newlines.
pub fn tsv_to_table(tsv: &str) -> String {
    let words = parse_tsv_words(tsv);
    if words.is_empty() {
        return String::new();
    }

    let mut heights: Vec<i32> = words.iter().map(|w| w.height.max(1)).collect();
    heights.sort_unstable();
    let gap = heights[heights.len() / 2].max(4);

    // Merge x-ranges into column spans.
    let mut ranges: Vec<(i32, i32)> = words.iter().map(|w| (w.left, w.right)).collect();
    ranges.sort_unstable();
    let mut spans: Vec<(i32, i32)> = Vec::new();
    for (l, r) in ranges {
        match spans.last_mut() {
            Some(last) if l - last.1 < gap => last.1 = last.1.max(r),
            _ => spans.push((l, r)),
        }
    }
    let column_of = |w: &Word| {
        spans
            .iter()
            .position(|(l, r)| w.left >= *l && w.left <= *r)
            .unwrap_or(0)
    };

    // Rows in tesseract order.
    let mut rows: Vec<((u32, u32, u32), Vec<String>)> = Vec::new();
    for w in &words {
        let col = column_of(w);
        if rows.last().is_none_or(|(k, _)| *k != w.line) {
            rows.push((w.line, vec![String::new(); spans.len()]));
        }
        if let Some((_, cells)) = rows.last_mut() {
            let cell = &mut cells[col];
            if !cell.is_empty() {
                cell.push(' ');
            }
            cell.push_str(&w.text);
        }
    }

    rows.into_iter()
        .map(|(_, cells)| cells.join("\t").trim_end_matches('\t').to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Installed tesseract languages (`tesseract --list-langs`), without `osd`.
pub fn list_languages() -> Vec<String> {
    match Command::new("tesseract").arg("--list-langs").output() {
        // Older versions print the list on stderr.
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).into_owned();
            s.push('\n');
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            parse_language_list(&s)
        }
        Err(_) => Vec::new(),
    }
}

/// Parse `tesseract --list-langs` output.
pub fn parse_language_list(output: &str) -> Vec<String> {
    let mut langs: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains(' ') && !l.contains(':') && *l != "osd")
        .map(str::to_string)
        .collect();
    langs.sort();
    langs.dedup();
    langs
}

/// Capture a screen region to a file.
fn capture_region(output_path: &str) -> Result<bool> {
    // Try Wayland first: grim + slurp
    if let Ok(slurp_output) = Command::new("slurp").output()
        && slurp_output.status.success()
    {
        let geometry = String::from_utf8_lossy(&slurp_output.stdout);
        let geometry = geometry.trim();
        if let Ok(s) = Command::new("grim")
            .args(["-g", geometry, output_path])
            .status()
            && s.success()
        {
            return Ok(true);
        }
    }

    // Try gnome-screenshot with area selection
    if let Ok(s) = Command::new("gnome-screenshot")
        .args(["-a", "-f", output_path])
        .status()
        && s.success()
    {
        return Ok(true);
    }

    // Try scrot with select
    if let Ok(s) = Command::new("scrot").args(["-s", output_path]).status()
        && s.success()
    {
        return Ok(true);
    }

    anyhow::bail!(
        "no screenshot tool available — install grim+slurp (Wayland) or gnome-screenshot/scrot (X11)"
    )
}

/// Copy text to clipboard.
pub fn copy_to_clipboard(text: &str) -> Result<()> {
    use std::io::Write;

    // Try wl-copy first
    if let Ok(mut child) = Command::new("wl-copy")
        .stdin(std::process::Stdio::piped())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).ok();
        }
        if child.wait().map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
    }

    // Fallback to xclip
    let mut child = Command::new("xclip")
        .args(["-selection", "clipboard", "-i"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .context("failed to run xclip")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n";

    fn word(line: u32, left: i32, width: i32, text: &str) -> String {
        format!(
            "5\t1\t1\t1\t{line}\t1\t{left}\t{}\t{width}\t20\t95.0\t{text}\n",
            line * 30
        )
    }

    #[test]
    fn single_line_joins() {
        assert_eq!(
            join_single_line("Hello\nworld  \n\n again "),
            "Hello world again"
        );
    }

    #[test]
    fn table_from_tsv() {
        let mut tsv = HEADER.to_string();
        // Row 1: "Name" | "Unit price"
        tsv += &word(1, 10, 60, "Name");
        tsv += &word(1, 200, 40, "Unit");
        tsv += &word(1, 245, 50, "price");
        // Row 2: "Apple" | "1.50"
        tsv += &word(2, 10, 70, "Apple");
        tsv += &word(2, 200, 40, "1.50");
        // Row 3: empty first column, only second
        tsv += &word(3, 210, 40, "2.00");
        // Non-word levels and empty words are ignored
        tsv += "4\t1\t1\t1\t1\t0\t10\t30\t300\t20\t-1\t\n";
        tsv += "5\t1\t1\t1\t3\t2\t400\t90\t5\t20\t-1\t \n";
        assert_eq!(tsv_to_table(&tsv), "Name\tUnit price\nApple\t1.50\n\t2.00");
    }

    #[test]
    fn empty_tsv() {
        assert_eq!(tsv_to_table(HEADER), "");
        assert_eq!(postprocess("  x \n", OcrMode::default()), "x");
    }

    #[test]
    fn language_list() {
        let out =
            "List of available languages in \"/usr/share/tessdata/\" (4):\nfra\neng\nosd\ndeu\n";
        assert_eq!(parse_language_list(out), vec!["deu", "eng", "fra"]);
    }
}
