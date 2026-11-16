//! Parity tests against Microsoft PowerToys Peek (H-misc tester).
//! Oracle: src/modules/peek/Peek.FilePreviewer/Previewers/*,
//! Peek.Common/Helpers/ReadableStringHelper.cs,
//! src/settings-ui/Settings.UI.Library/PeekProperties.cs.

use crate::PeekConfig;
use crate::preview::{FileKind, detect_kind, format_size, generate_preview, list_archive};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mpt-peek-parity-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---- Defaults (PeekProperties ctor) ----

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn default_always_on_top_is_false() {
    assert!(!PeekConfig::default().always_on_top);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn default_close_after_losing_focus_is_false() {
    assert!(!PeekConfig::default().close_after_losing_focus);
}

#[test]
fn default_confirm_delete_and_space_activation_are_true() {
    let c = PeekConfig::default();
    assert!(c.confirm_file_delete);
    assert!(c.enable_space_to_activate);
}

// ---- Size formatting: port of ReadableStringHelper.BytesToReadableString
// (number + unit part, without the " (N bytes)" suffix) ----

fn pt_readable(bytes: u64) -> String {
    const UNITS: [&str; 7] = ["bytes", "KB", "MB", "GB", "TB", "PB", "EB"];
    let mut index: i32 = 0;
    let mut number = 0.0f64;
    if bytes > 0 {
        index = ((bytes as f64).ln() / 1024f64.ln()).floor() as i32;
        number = bytes as f64 / 1024f64.powi(index);
    }
    if index > 0 && number >= 1000.0 {
        index += 1;
        number = bytes as f64 / 1024f64.powi(index);
    }
    let digits = if (number as i64) == 0 {
        1
    } else {
        (number as i64).to_string().len() as i32
    };
    let precision = if index == 0 { 0 } else { 3 - digits };
    let dp = 10f64.powi(precision);
    number = (number * dp).trunc() / dp;
    let unit = if bytes == 1 {
        "byte"
    } else {
        UNITS[index as usize]
    };
    format!("{:.*} {}", precision as usize, number, unit)
}

#[test]
fn pt_oracle_sanity() {
    assert_eq!(pt_readable(0), "0 bytes");
    assert_eq!(pt_readable(1), "1 byte");
    assert_eq!(pt_readable(500), "500 bytes");
    assert_eq!(pt_readable(1024), "1.00 KB");
    assert_eq!(pt_readable(1_500_000), "1.43 MB");
    assert_eq!(pt_readable(1_048_575), "0.99 MB");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn format_size_matches_pt_readable_string() {
    let mut mismatches = vec![];
    for b in [
        0u64,
        1,
        500,
        1023,
        1024,
        1536,
        10_240,
        1_000_000,
        1_048_575,
        1_500_000,
        2_500_000_000,
        5 << 40,
    ] {
        let (r, p) = (format_size(b), pt_readable(b));
        if r != p {
            mismatches.push(format!("{b}: rust={r:?} pt={p:?}"));
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn format_size_never_shows_1024_of_a_unit() {
    // PT bumps to the next unit when the number has >= 4 digits.
    let s = format_size(1_048_575); // 1 MiB - 1
    assert!(!s.starts_with("1024"), "{s}");
}

// ---- Previewer selection vs PT supported extensions ----

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn pt_archive_extensions_are_archives() {
    // ArchivePreviewer._supportedFileTypes
    let bad: Vec<_> = [
        ".zip", ".rar", ".7z", ".tar", ".nupkg", ".jar", ".gz", ".tar.gz", ".tgz",
    ]
    .iter()
    .filter(|e| detect_kind(Path::new(&format!("a{e}"))) != FileKind::Archive)
    .collect();
    assert!(bad.is_empty(), "not treated as archive: {bad:?}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn pt_audio_extensions_are_audio() {
    let bad: Vec<_> = [
        ".aac", ".ac3", ".amr", ".flac", ".m4a", ".mp3", ".ogg", ".wav", ".wma",
    ]
    .iter()
    .filter(|e| detect_kind(Path::new(&format!("a{e}"))) != FileKind::Audio)
    .collect();
    assert!(bad.is_empty(), "not treated as audio: {bad:?}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn pt_video_extensions_are_video() {
    let bad: Vec<_> = [
        ".mp4", ".3g2", ".3gp", ".3gp2", ".3gpp", ".asf", ".avi", ".m2t", ".m2ts", ".m4v", ".mkv",
        ".mov", ".mp4v", ".mts", ".wm", ".wmv", ".webm",
    ]
    .iter()
    .filter(|e| detect_kind(Path::new(&format!("a{e}"))) != FileKind::Video)
    .collect();
    assert!(bad.is_empty(), "not treated as video: {bad:?}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn common_image_extensions_are_images() {
    // PT: every WIC decoder extension + .qoi (jpe/jfif/heic/avif/dds/jxr ...)
    let bad: Vec<_> = [
        ".png", ".jpg", ".jpeg", ".jpe", ".jfif", ".gif", ".bmp", ".dib", ".tif", ".tiff", ".ico",
        ".webp", ".qoi", ".heic", ".avif",
    ]
    .iter()
    .filter(|e| detect_kind(Path::new(&format!("a{e}"))) != FileKind::Image)
    .collect();
    assert!(bad.is_empty(), "not treated as image: {bad:?}");
}

#[test]
fn uppercase_extensions_detected() {
    assert_eq!(detect_kind(Path::new("A.PNG")), FileKind::Image);
    assert_eq!(detect_kind(Path::new("A.MD")), FileKind::Text);
    assert_eq!(detect_kind(Path::new("A.ZIP")), FileKind::Archive);
}

// ---- Content handling ----

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn non_utf8_text_file_still_previews() {
    // PT (Monaco/TextFileHelper) shows Latin-1/UTF-16 text; it never fails to open.
    let d = tmp("latin1");
    let p = d.join("latin1.txt");
    std::fs::write(&p, b"caf\xe9 cr\xe8me\n").unwrap();
    let r = generate_preview(&p, 50, 20);
    assert!(r.is_ok(), "generate_preview error: {:?}", r.err());
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/H-misc.md"]
fn utf16_text_file_still_previews() {
    let d = tmp("utf16");
    let p = d.join("u16.txt");
    let mut bytes = vec![0xFF, 0xFE];
    for c in "hello\n".encode_utf16() {
        bytes.extend_from_slice(&c.to_le_bytes());
    }
    std::fs::write(&p, bytes).unwrap();
    assert!(generate_preview(&p, 50, 20).is_ok());
}

#[test]
fn text_preview_shows_whole_small_file_and_marks_truncation() {
    let d = tmp("lines");
    let p = d.join("a.txt");
    let body: String = (1..=60).map(|i| format!("line{i}\n")).collect();
    std::fs::write(&p, &body).unwrap();
    let t = generate_preview(&p, 50, 20).unwrap().preview_text.unwrap();
    assert!(t.contains("line50"));
    assert!(!t.contains("line51"));
    assert!(t.ends_with("... (10 more lines)"), "{t}");
    // exactly N lines -> no marker
    let body: String = (1..=50).map(|i| format!("line{i}\n")).collect();
    std::fs::write(&p, &body).unwrap();
    let t = generate_preview(&p, 50, 20).unwrap().preview_text.unwrap();
    assert!(!t.contains("more lines"));
}

#[test]
fn empty_text_file_previews() {
    let d = tmp("empty");
    let p = d.join("e.txt");
    std::fs::write(&p, b"").unwrap();
    let pr = generate_preview(&p, 50, 20).unwrap();
    assert_eq!(pr.size_bytes, 0);
    assert_eq!(pr.preview_text.as_deref(), Some(""));
}

#[test]
fn directory_preview_lists_and_truncates() {
    let d = tmp("dir");
    for i in 0..5 {
        std::fs::write(d.join(format!("f{i}")), b"x").unwrap();
    }
    std::fs::create_dir(d.join("sub")).unwrap();
    let pr = generate_preview(&d, 50, 3).unwrap();
    assert_eq!(pr.kind, FileKind::Directory);
    let t = pr.preview_text.unwrap();
    assert_eq!(t.lines().count(), 4, "{t}");
    assert!(t.ends_with("... (3 more entries)"), "{t}");
}

#[test]
fn corrupt_zip_reports_error_text_not_panic() {
    let d = tmp("badzip");
    let p = d.join("bad.zip");
    std::fs::write(&p, b"PK\x03\x04garbage").unwrap();
    let pr = generate_preview(&p, 50, 20).unwrap();
    assert!(pr.archive.is_none());
    assert!(
        pr.preview_text
            .unwrap()
            .starts_with("Unable to read archive")
    );
}

#[test]
fn zip_listing_truncation_and_totals() {
    use std::io::Write;
    let d = tmp("zip");
    let p = d.join("t.zip");
    {
        let f = std::fs::File::create(&p).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let o = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for i in 0..5 {
            w.start_file(format!("f{i}.txt"), o).unwrap();
            w.write_all(&vec![b'a'; i * 10]).unwrap();
        }
        w.finish().unwrap();
    }
    let l = list_archive(&p, 3).unwrap();
    assert_eq!((l.total, l.entries.len(), l.total_size), (5, 3, 100));
}

#[test]
fn missing_file_is_error() {
    assert!(generate_preview(Path::new("/nonexistent/zzz.txt"), 50, 20).is_err());
}
