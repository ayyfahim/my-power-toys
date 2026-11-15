use anyhow::{Result, anyhow};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Supported file categories for preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Text,
    Image,
    Pdf,
    Audio,
    Video,
    Directory,
    /// zip / tar / tar.gz archives (contents are listed).
    Archive,
    Unknown,
}

impl fmt::Display for FileKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FileKind::Text => write!(f, "Text"),
            FileKind::Image => write!(f, "Image"),
            FileKind::Pdf => write!(f, "PDF"),
            FileKind::Audio => write!(f, "Audio"),
            FileKind::Video => write!(f, "Video"),
            FileKind::Directory => write!(f, "Directory"),
            FileKind::Archive => write!(f, "Archive"),
            FileKind::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Metadata + preview data for a file.
#[derive(Debug, Clone)]
pub struct FilePreview {
    pub path: PathBuf,
    pub kind: FileKind,
    pub size_bytes: u64,
    pub mime_type: Option<String>,
    pub preview_text: Option<String>,
    pub dimensions: Option<(u32, u32)>,
    pub duration: Option<String>,
    /// Archive contents (for [`FileKind::Archive`]).
    pub archive: Option<ArchiveListing>,
}

/// One entry inside an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    pub name: String,
    pub size: u64,
    pub is_dir: bool,
}

/// Archive listing (possibly truncated to `max_entries`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArchiveListing {
    pub entries: Vec<ArchiveEntry>,
    /// Total number of entries in the archive.
    pub total: usize,
    /// Sum of uncompressed sizes of all entries.
    pub total_size: u64,
}

/// How a text file should be rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextFlavor {
    Plain,
    Markdown,
    /// Source code; highlight using the file extension / name.
    Code,
}

/// Decide how to render a text file.
pub fn text_flavor(path: &Path) -> TextFlavor {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "md" | "markdown" | "mdown" | "mkd" => TextFlavor::Markdown,
        "txt" | "log" | "csv" | "" => {
            if is_special_code_name(path) {
                TextFlavor::Code
            } else {
                TextFlavor::Plain
            }
        }
        _ => TextFlavor::Code,
    }
}

/// Extension-less files that are still source code.
fn is_special_code_name(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    matches!(
        name.as_str(),
        "makefile" | "dockerfile" | "containerfile" | "cmakelists.txt" | "pkgbuild" | "justfile"
    )
}

fn archive_type(path: &Path) -> Option<ArchiveType> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_lowercase();
    if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Some(ArchiveType::TarGz)
    } else if name.ends_with(".tar") {
        Some(ArchiveType::Tar)
    } else if [
        ".zip", ".jar", ".apk", ".epub", ".docx", ".xlsx", ".pptx", ".odt", ".ods", ".odp", ".whl",
        ".nupkg", ".cbz", ".xpi", ".war",
    ]
    .iter()
    .any(|e| name.ends_with(e))
    {
        Some(ArchiveType::Zip)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveType {
    Zip,
    Tar,
    TarGz,
}

/// List the contents of a zip / tar / tar.gz archive.
pub fn list_archive(path: &Path, max_entries: usize) -> Result<ArchiveListing> {
    let kind = archive_type(path).ok_or_else(|| anyhow!("not a supported archive"))?;
    let file = std::fs::File::open(path)?;
    let mut listing = ArchiveListing::default();
    let mut push = |name: String, size: u64, is_dir: bool| {
        listing.total += 1;
        listing.total_size = listing.total_size.saturating_add(size);
        if listing.entries.len() < max_entries {
            listing.entries.push(ArchiveEntry { name, size, is_dir });
        }
    };
    match kind {
        ArchiveType::Zip => {
            let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))?;
            for i in 0..zip.len() {
                let entry = zip.by_index_raw(i)?;
                let name = entry
                    .name()
                    .map(|n| n.into_owned())
                    .unwrap_or_else(|_| String::from_utf8_lossy(entry.name_raw()).into_owned());
                push(name, entry.size(), entry.is_dir());
            }
        }
        ArchiveType::Tar | ArchiveType::TarGz => {
            let reader: Box<dyn std::io::Read> = if kind == ArchiveType::TarGz {
                Box::new(flate2::read::GzDecoder::new(std::io::BufReader::new(file)))
            } else {
                Box::new(std::io::BufReader::new(file))
            };
            let mut archive = tar::Archive::new(reader);
            for entry in archive.entries()? {
                let entry = entry?;
                let header = entry.header();
                let is_dir = header.entry_type().is_dir();
                let size = header.size().unwrap_or(0);
                let name = entry
                    .path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| "?".into());
                push(name, size, is_dir);
            }
        }
    }
    Ok(listing)
}

/// Is an external program available on PATH?
pub fn tool_available(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let p = dir.join(name);
            p.is_file()
        })
    })
}

/// Where a thumbnail for `path` is cached.
fn thumbnail_path(path: &Path) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    if let Ok(m) = std::fs::metadata(path) {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    std::env::temp_dir()
        .join("mpt-peek-thumbs")
        .join(format!("{:016x}.png", h.finish()))
}

/// Render a PNG thumbnail for PDFs (first page, via `pdftoppm`) and videos
/// (via `ffmpegthumbnailer` or `ffmpeg`). Returns None when the tool is
/// missing or fails; callers fall back to metadata.
pub fn render_thumbnail(path: &Path, kind: FileKind, max_size: u32) -> Option<PathBuf> {
    let out = thumbnail_path(path);
    if out.exists() {
        return Some(out);
    }
    std::fs::create_dir_all(out.parent()?).ok()?;
    let size = max_size.to_string();
    let ok = match kind {
        FileKind::Pdf => {
            if !tool_available("pdftoppm") {
                tracing::warn!("pdftoppm not found; showing PDF metadata only");
                return None;
            }
            // pdftoppm appends ".png" to the output prefix with -singlefile.
            let prefix = out.with_extension("");
            Command::new("pdftoppm")
                .args(["-png", "-f", "1", "-l", "1", "-singlefile", "-scale-to"])
                .arg(&size)
                .arg(path)
                .arg(&prefix)
                .output()
                .is_ok_and(|o| o.status.success())
        }
        FileKind::Video => {
            if tool_available("ffmpegthumbnailer") {
                Command::new("ffmpegthumbnailer")
                    .arg("-i")
                    .arg(path)
                    .arg("-o")
                    .arg(&out)
                    .args(["-s", &size, "-c", "png"])
                    .output()
                    .is_ok_and(|o| o.status.success())
            } else if tool_available("ffmpeg") {
                let scale = format!("scale='min({size},iw)':-2");
                let run = |seek: bool| {
                    let mut cmd = Command::new("ffmpeg");
                    cmd.args(["-v", "error", "-y"]);
                    if seek {
                        cmd.args(["-ss", "1"]);
                    }
                    cmd.arg("-i")
                        .arg(path)
                        .args(["-frames:v", "1", "-vf", &scale])
                        .arg(&out)
                        .output()
                        .is_ok_and(|o| o.status.success())
                };
                // Very short clips have no frame at 1s; retry from the start.
                (run(true) && out.exists()) || run(false)
            } else {
                tracing::warn!("ffmpegthumbnailer/ffmpeg not found; no video thumbnail");
                return None;
            }
        }
        _ => return None,
    };
    (ok && out.exists()).then_some(out)
}

/// Move a file to the trash (`gio trash`, falling back to `trash-put`).
pub fn move_to_trash(path: &Path) -> Result<()> {
    let attempts: [(&str, &[&str]); 2] = [("gio", &["trash", "--"]), ("trash-put", &["--"])];
    let mut last_err = None;
    for (tool, args) in attempts {
        if !tool_available(tool) {
            continue;
        }
        match Command::new(tool).args(args).arg(path).output() {
            Ok(o) if o.status.success() => return Ok(()),
            Ok(o) => {
                last_err = Some(anyhow!(
                    "{tool} failed: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                ))
            }
            Err(e) => last_err = Some(anyhow!("{tool} failed: {e}")),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("no trash tool found (install gio or trash-cli)")))
}

/// Detect what kind of file we're dealing with based on extension.
pub fn detect_kind(path: &Path) -> FileKind {
    if path.is_dir() {
        return FileKind::Directory;
    }

    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if archive_type(path).is_some() {
        return FileKind::Archive;
    }
    if is_special_code_name(path) {
        return FileKind::Text;
    }

    match ext.as_str() {
        // Text
        "txt" | "md" | "rs" | "py" | "js" | "ts" | "json" | "toml" | "yaml" | "yml" | "xml"
        | "html" | "css" | "sh" | "bash" | "zsh" | "c" | "cpp" | "h" | "hpp" | "java" | "go"
        | "rb" | "lua" | "sql" | "csv" | "log" | "conf" | "cfg" | "ini" | "env" | "dockerfile"
        | "makefile" | "markdown" | "jsx" | "tsx" | "mjs" | "cjs" | "kt" | "kts" | "swift"
        | "cs" | "php" | "pl" | "pm" | "r" | "scala" | "hs" | "ex" | "exs" | "erl" | "clj"
        | "vue" | "svelte" | "scss" | "sass" | "less" | "diff" | "patch" | "rst" | "tex"
        | "cmake" | "gradle" | "properties" | "nix" | "zig" | "dart" | "ps1" | "bat" | "fish"
        | "cc" | "cxx" | "hh" | "m" | "mm" | "proto" | "graphql" | "tf" | "vim" | "el" | "lisp"
        | "ml" | "fs" | "jl" | "nim" | "v" | "sv" | "vhd" | "asm" | "s" | "d" | "groovy"
        | "csproj" | "sln" | "desktop" | "service" | "srt" | "tsv" | "jsonc" | "json5" | "lock"
        | "gitignore" | "editorconfig" => FileKind::Text,
        // Images
        "png" | "jpg" | "jpeg" | "gif" | "bmp" | "svg" | "webp" | "ico" | "tiff" | "tif" => {
            FileKind::Image
        }
        // PDF
        "pdf" => FileKind::Pdf,
        // Audio
        "mp3" | "wav" | "flac" | "ogg" | "m4a" | "aac" | "wma" | "opus" => FileKind::Audio,
        // Video
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" | "m4v" => FileKind::Video,
        _ => FileKind::Unknown,
    }
}

/// Generate a preview for a file.
pub fn generate_preview(
    path: &Path,
    max_preview_lines: usize,
    max_dir_entries: usize,
) -> Result<FilePreview> {
    if !path.exists() {
        return Err(anyhow!("file not found: {}", path.display()));
    }

    let kind = detect_kind(path);
    let metadata = std::fs::metadata(path)?;
    let size_bytes = metadata.len();
    let mime_type = detect_mime(path);

    let mut preview = FilePreview {
        path: path.to_path_buf(),
        kind,
        size_bytes,
        mime_type,
        preview_text: None,
        dimensions: None,
        duration: None,
        archive: None,
    };

    match kind {
        FileKind::Text => {
            preview.preview_text = Some(read_text_preview(path, max_preview_lines)?);
        }
        FileKind::Image => {
            preview.dimensions = get_image_dimensions(path);
        }
        FileKind::Pdf => {
            preview.preview_text = get_pdf_info(path);
        }
        FileKind::Audio | FileKind::Video => {
            preview.duration = get_media_duration(path);
        }
        FileKind::Directory => {
            preview.preview_text = Some(list_directory(path, max_dir_entries)?);
        }
        FileKind::Archive => match list_archive(path, MAX_ARCHIVE_ENTRIES) {
            Ok(listing) => preview.archive = Some(listing),
            Err(e) => preview.preview_text = Some(format!("Unable to read archive: {e}")),
        },
        FileKind::Unknown => {}
    }

    Ok(preview)
}

/// Maximum number of archive entries listed.
pub const MAX_ARCHIVE_ENTRIES: usize = 1000;

/// Read the first N lines of a text file.
fn read_text_preview(path: &Path, max_lines: usize) -> Result<String> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().take(max_lines).collect();
    let preview = lines.join("\n");
    if content.lines().count() > max_lines {
        Ok(format!(
            "{preview}\n... ({} more lines)",
            content.lines().count() - max_lines
        ))
    } else {
        Ok(preview)
    }
}

/// List directory contents (first N entries).
fn list_directory(path: &Path, max_entries: usize) -> Result<String> {
    let mut entries: Vec<String> = Vec::new();
    let mut total = 0;

    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        total += 1;
        if entries.len() < max_entries {
            let name = entry.file_name().to_string_lossy().to_string();
            let kind = if entry.file_type()?.is_dir() {
                "dir"
            } else {
                "file"
            };
            entries.push(format!("[{kind}] {name}"));
        }
    }

    let mut result = entries.join("\n");
    if total > max_entries {
        result.push_str(&format!("\n... ({} more entries)", total - max_entries));
    }
    Ok(result)
}

/// Detect MIME type using `file --mime-type`.
fn detect_mime(path: &Path) -> Option<String> {
    let output = Command::new("file")
        .args(["--mime-type", "-b"])
        .arg(path)
        .output()
        .ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get image dimensions using `identify` (ImageMagick) or `file`.
fn get_image_dimensions(path: &Path) -> Option<(u32, u32)> {
    // Try identify (ImageMagick)
    if let Ok(output) = Command::new("identify")
        .args(["-format", "%w %h"])
        .arg(path)
        .output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        let parts: Vec<&str> = text.split_whitespace().collect();
        if parts.len() >= 2
            && let (Ok(w), Ok(h)) = (parts[0].parse(), parts[1].parse())
        {
            return Some((w, h));
        }
    }

    // Fallback: parse `file` output for common formats
    if let Ok(output) = Command::new("file").arg(path).output()
        && output.status.success()
    {
        let text = String::from_utf8_lossy(&output.stdout);
        // Try to find "WxH" pattern like "1920 x 1080"
        for part in text.split(", ") {
            let cleaned = part.replace(" x ", "x");
            if let Some((w_str, h_str)) = cleaned.split_once('x')
                && let (Ok(w), Ok(h)) = (w_str.trim().parse::<u32>(), h_str.trim().parse::<u32>())
                && w > 0
                && h > 0
                && w < 100000
                && h < 100000
            {
                return Some((w, h));
            }
        }
    }

    None
}

/// Get PDF info using `pdfinfo`.
fn get_pdf_info(path: &Path) -> Option<String> {
    let output = Command::new("pdfinfo").arg(path).output().ok()?;

    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

/// Get media duration using `ffprobe`.
fn get_media_duration(path: &Path) -> Option<String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "quiet",
            "-show_entries",
            "format=duration",
            "-of",
            "csv=p=0",
        ])
        .arg(path)
        .output()
        .ok()?;

    if output.status.success() {
        let secs: f64 = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse()
            .ok()?;
        let mins = (secs / 60.0).floor() as u64;
        let remaining = (secs % 60.0).floor() as u64;
        Some(format!("{mins}:{remaining:02}"))
    } else {
        None
    }
}

/// Format file size in human-readable form.
pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_text_files() {
        assert_eq!(detect_kind(Path::new("file.rs")), FileKind::Text);
        assert_eq!(detect_kind(Path::new("README.md")), FileKind::Text);
        assert_eq!(detect_kind(Path::new("config.toml")), FileKind::Text);
        assert_eq!(detect_kind(Path::new("script.py")), FileKind::Text);
    }

    #[test]
    fn detect_image_files() {
        assert_eq!(detect_kind(Path::new("photo.png")), FileKind::Image);
        assert_eq!(detect_kind(Path::new("photo.jpg")), FileKind::Image);
        assert_eq!(detect_kind(Path::new("icon.svg")), FileKind::Image);
    }

    #[test]
    fn detect_media_files() {
        assert_eq!(detect_kind(Path::new("song.mp3")), FileKind::Audio);
        assert_eq!(detect_kind(Path::new("video.mp4")), FileKind::Video);
        assert_eq!(detect_kind(Path::new("movie.mkv")), FileKind::Video);
    }

    #[test]
    fn detect_pdf() {
        assert_eq!(detect_kind(Path::new("doc.pdf")), FileKind::Pdf);
    }

    #[test]
    fn detect_unknown() {
        assert_eq!(detect_kind(Path::new("data.xyz")), FileKind::Unknown);
        assert_eq!(detect_kind(Path::new("binary.bin")), FileKind::Unknown);
    }

    #[test]
    fn format_size_bytes() {
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1_500_000), "1.4 MB");
        assert_eq!(format_size(2_500_000_000), "2.3 GB");
    }

    #[test]
    fn detect_archives_and_flavors() {
        assert_eq!(detect_kind(Path::new("a.zip")), FileKind::Archive);
        assert_eq!(detect_kind(Path::new("a.tar.gz")), FileKind::Archive);
        assert_eq!(detect_kind(Path::new("a.TGZ")), FileKind::Archive);
        assert_eq!(detect_kind(Path::new("a.tar")), FileKind::Archive);
        assert_eq!(detect_kind(Path::new("Makefile")), FileKind::Text);
        assert_eq!(text_flavor(Path::new("README.md")), TextFlavor::Markdown);
        assert_eq!(text_flavor(Path::new("main.rs")), TextFlavor::Code);
        assert_eq!(text_flavor(Path::new("notes.txt")), TextFlavor::Plain);
        assert_eq!(text_flavor(Path::new("Dockerfile")), TextFlavor::Code);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mpt-peek-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn list_zip_archive() {
        use std::io::Write;
        let dir = temp_dir("zip");
        let path = dir.join("t.zip");
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.add_directory("docs/", opts).unwrap();
            w.start_file("docs/a.txt", opts).unwrap();
            w.write_all(b"hello").unwrap();
            w.start_file("b.bin", opts).unwrap();
            w.write_all(&[0u8; 10]).unwrap();
            w.finish().unwrap();
        }
        let l = list_archive(&path, 2).unwrap();
        assert_eq!(l.total, 3);
        assert_eq!(l.total_size, 15);
        assert_eq!(l.entries.len(), 2);
        assert!(l.entries[0].is_dir);
        assert_eq!(l.entries[1].name, "docs/a.txt");
        assert_eq!(l.entries[1].size, 5);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_tar_gz_archive() {
        let dir = temp_dir("tgz");
        let path = dir.join("t.tar.gz");
        {
            let f = std::fs::File::create(&path).unwrap();
            let gz = flate2::write::GzEncoder::new(f, flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            h.set_size(4);
            h.set_mode(0o644);
            h.set_cksum();
            b.append_data(&mut h, "x/file.txt", &b"abcd"[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let p = generate_preview(&path, 10, 10).unwrap();
        assert_eq!(p.kind, FileKind::Archive);
        let l = p.archive.unwrap();
        assert_eq!(l.total, 1);
        assert_eq!(l.entries[0].name, "x/file.txt");
        assert_eq!(l.entries[0].size, 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn file_kind_display() {
        assert_eq!(format!("{}", FileKind::Text), "Text");
        assert_eq!(format!("{}", FileKind::Image), "Image");
        assert_eq!(format!("{}", FileKind::Pdf), "PDF");
    }
}
