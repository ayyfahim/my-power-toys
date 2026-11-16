use anyhow::{Context, Result, bail};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageFormat};
use serde::{Deserialize, Serialize};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResizePreset {
    Small,  // 640px wide
    Medium, // 1280px wide
    Large,  // 1920px wide
    Phone,  // 1080px wide (portrait)
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Original,
    Png,
    Jpeg,
    Webp,
}

impl ResizePreset {
    pub fn max_width(self) -> u32 {
        match self {
            Self::Small => 640,
            Self::Medium => 1280,
            Self::Large => 1920,
            Self::Phone => 1080,
            Self::Custom => 0,
        }
    }

    /// Display name of the default size preset backing this legacy variant.
    pub fn default_name(self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
            Self::Phone => "Phone",
            Self::Custom => "Custom",
        }
    }
}

/// Upstream `ResizeFit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResizeFit {
    /// Scale down/up to fit inside the box, keeping aspect ratio.
    #[default]
    Fit,
    /// Scale to cover the box, keeping aspect ratio, then center-crop.
    Fill,
    /// Scale each axis independently to exactly the box.
    Stretch,
}

/// Upstream `ResizeUnit`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ResizeUnit {
    #[default]
    Pixel,
    Percent,
    Inch,
    #[serde(alias = "centimeter")]
    Cm,
}

/// Encoder used when the source format cannot be written (upstream
/// `ImageresizerFallbackEncoder`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FallbackEncoder {
    Png,
    #[default]
    Jpeg,
    Bmp,
    Tiff,
    Gif,
    Webp,
}

impl FallbackEncoder {
    pub fn format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Bmp => ImageFormat::Bmp,
            Self::Tiff => ImageFormat::Tiff,
            Self::Gif => ImageFormat::Gif,
            Self::Webp => ImageFormat::WebP,
        }
    }
}

/// A user-editable size preset (upstream `ImageSize`). A width or height of
/// 0 means "auto" (unconstrained for Fit, original size for Fill/Stretch).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SizePreset {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub fit: ResizeFit,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub unit: ResizeUnit,
}

impl SizePreset {
    pub fn new(name: &str, fit: ResizeFit, width: f64, height: f64, unit: ResizeUnit) -> Self {
        Self {
            name: name.into(),
            fit,
            width,
            height,
            unit,
        }
    }

    /// A Fit preset constrained only by width (legacy "max width" behaviour).
    pub fn max_width(name: &str, width: u32) -> Self {
        Self::new(
            name,
            ResizeFit::Fit,
            f64::from(width),
            0.0,
            ResizeUnit::Pixel,
        )
    }

    pub fn has_auto(&self) -> bool {
        self.width == 0.0 || self.height == 0.0
    }

    fn to_pixels(&self, value: f64, original: u32, dpi: f64) -> f64 {
        if value == 0.0 || value.is_nan() {
            return if self.fit == ResizeFit::Fit {
                f64::INFINITY
            } else {
                f64::from(original)
            };
        }
        match self.unit {
            ResizeUnit::Pixel => value,
            ResizeUnit::Percent => value / 100.0 * f64::from(original),
            ResizeUnit::Inch => value * dpi,
            ResizeUnit::Cm => value * dpi / 2.54,
        }
    }

    pub fn pixel_width(&self, original_width: u32, dpi: f64) -> f64 {
        self.to_pixels(self.width, original_width, dpi)
    }

    /// For percent with Fit/Fill, upstream uses the width percentage for both axes.
    pub fn pixel_height(&self, original_height: u32, dpi: f64) -> f64 {
        let v = if self.fit != ResizeFit::Stretch && self.unit == ResizeUnit::Percent {
            self.width
        } else {
            self.height
        };
        self.to_pixels(v, original_height, dpi)
    }

    /// Short human-readable description, e.g. `Fit 1280 × auto px`.
    pub fn describe(&self) -> String {
        let fmt = |v: f64| {
            if v == 0.0 {
                "auto".to_string()
            } else {
                format_number(v)
            }
        };
        let fit = match self.fit {
            ResizeFit::Fit => "Fit",
            ResizeFit::Fill => "Fill",
            ResizeFit::Stretch => "Stretch",
        };
        let unit = match self.unit {
            ResizeUnit::Pixel => "px",
            ResizeUnit::Percent => "%",
            ResizeUnit::Inch => "in",
            ResizeUnit::Cm => "cm",
        };
        if self.unit == ResizeUnit::Percent && self.fit != ResizeFit::Stretch {
            format!("{fit} {}{unit}", fmt(self.width))
        } else {
            format!(
                "{fit} {} \u{00D7} {} {unit}",
                fmt(self.width),
                fmt(self.height)
            )
        }
    }
}

/// Default size presets (keeps the historical preset names and widths).
pub fn default_sizes() -> Vec<SizePreset> {
    [
        ResizePreset::Small,
        ResizePreset::Medium,
        ResizePreset::Large,
        ResizePreset::Phone,
    ]
    .into_iter()
    .map(|p| SizePreset::max_width(p.default_name(), p.max_width()))
    .collect()
}

pub fn default_custom_size() -> SizePreset {
    SizePreset::new("Custom", ResizeFit::Fit, 1024.0, 640.0, ResizeUnit::Pixel)
}

/// Result of the dimension computation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizePlan {
    /// Size to scale the whole image to.
    pub scaled_width: u32,
    pub scaled_height: u32,
    /// Centered crop (x, y, w, h) applied after scaling (Fill mode).
    pub crop: Option<(u32, u32, u32, u32)>,
    /// The image can be kept as-is.
    pub no_transform: bool,
}

impl ResizePlan {
    /// Final output dimensions.
    pub fn output_size(&self) -> (u32, u32) {
        match self.crop {
            Some((_, _, w, h)) => (w, h),
            None => (self.scaled_width, self.scaled_height),
        }
    }
}

fn validated_dimension(v: f64) -> Result<u32> {
    if !v.is_finite() || v < 0.0 || v > f64::from(i32::MAX) {
        bail!("resulting dimension is out of range");
    }
    Ok((v.round() as u32).max(1))
}

/// Compute target dimensions (port of upstream `CalculateDimensions`).
pub fn compute_dimensions(
    original_width: u32,
    original_height: u32,
    size: &SizePreset,
    shrink_only: bool,
    ignore_orientation: bool,
    dpi: (f64, f64),
) -> Result<ResizePlan> {
    if original_width == 0 || original_height == 0 {
        bail!("image has zero size");
    }
    let mut width = size.pixel_width(original_width, dpi.0);
    let mut height = size.pixel_height(original_height, dpi.1);

    if ignore_orientation && !size.has_auto() && size.unit != ResizeUnit::Percent {
        let input_landscape = original_width > original_height;
        let input_portrait = original_height > original_width;
        let target_landscape = width > height;
        let target_portrait = height > width;
        if (input_landscape && target_portrait) || (input_portrait && target_landscape) {
            std::mem::swap(&mut width, &mut height);
        }
    }

    let mut scale_x = width / f64::from(original_width);
    let mut scale_y = height / f64::from(original_height);
    match size.fit {
        ResizeFit::Fit => {
            scale_x = scale_x.min(scale_y);
            scale_y = scale_x;
        }
        ResizeFit::Fill => {
            scale_x = scale_x.max(scale_y);
            scale_y = scale_x;
        }
        ResizeFit::Stretch => {}
    }

    let unchanged = ResizePlan {
        scaled_width: original_width,
        scaled_height: original_height,
        crop: None,
        no_transform: true,
    };

    if shrink_only && size.unit != ResizeUnit::Percent {
        if scale_x > 1.0 || scale_y > 1.0 {
            return Ok(unchanged);
        }
        let fill_crop_required = size.fit == ResizeFit::Fill
            && (f64::from(original_width) > width || f64::from(original_height) > height);
        if scale_x == 1.0 && scale_y == 1.0 && !fill_crop_required {
            return Ok(unchanged);
        }
    }

    let scaled_width = validated_dimension(f64::from(original_width) * scale_x)?;
    let scaled_height = validated_dimension(f64::from(original_height) * scale_y)?;

    if size.fit == ResizeFit::Fill {
        let tw = validated_dimension(width)?;
        let th = validated_dimension(height)?;
        if scaled_width > tw || scaled_height > th {
            let tw = tw.min(scaled_width);
            let th = th.min(scaled_height);
            return Ok(ResizePlan {
                scaled_width,
                scaled_height,
                crop: Some(((scaled_width - tw) / 2, (scaled_height - th) / 2, tw, th)),
                no_transform: false,
            });
        }
    }

    Ok(ResizePlan {
        scaled_width,
        scaled_height,
        crop: None,
        no_transform: false,
    })
}

/// Format a number the way upstream displays sizes (no trailing `.0`).
pub fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Expand the output file name template (upstream `ImageresizerFileName`):
/// `%1` original name, `%2` size name, `%3` selected width, `%4` selected
/// height, `%5` actual width, `%6` actual height. Characters that are invalid
/// in file names are replaced with `_`. The result has no extension.
pub fn format_file_name(
    template: &str,
    original_name: &str,
    size_name: &str,
    selected: (f64, f64),
    actual: (u32, u32),
) -> String {
    let size_name = size_name.replace(['\\', '/'], "_");
    let mut out = String::with_capacity(template.len() + original_name.len());
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%'
            && let Some(&d) = chars.peek()
            && ('1'..='6').contains(&d)
        {
            chars.next();
            match d {
                '1' => out.push_str(original_name),
                '2' => out.push_str(&size_name),
                '3' => out.push_str(&format_number(selected.0)),
                '4' => out.push_str(&format_number(selected.1)),
                '5' => out.push_str(&actual.0.to_string()),
                _ => out.push_str(&actual.1.to_string()),
            }
            continue;
        }
        out.push(c);
    }
    let out: String = out
        .chars()
        .map(|c| match c {
            ':' | '*' | '?' | '"' | '<' | '>' | '|' | '/' | '\\' | '\0' => '_',
            c => c,
        })
        .collect();
    let trimmed = out.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        format!("{original_name}_")
    } else {
        trimmed.to_string()
    }
}

/// All options for a resize operation.
#[derive(Debug, Clone)]
pub struct ResizeOptions {
    pub size: SizePreset,
    pub output_format: OutputFormat,
    pub quality: u8,
    pub shrink_only: bool,
    pub replace: bool,
    pub ignore_orientation: bool,
    pub keep_date_modified: bool,
    pub file_name_template: String,
    pub fallback_encoder: FallbackEncoder,
}

impl Default for ResizeOptions {
    fn default() -> Self {
        Self {
            size: SizePreset::max_width("Medium", 1280),
            output_format: OutputFormat::Original,
            quality: 85,
            shrink_only: true,
            replace: false,
            ignore_orientation: true,
            keep_date_modified: false,
            file_name_template: "%1 (%2)".into(),
            fallback_encoder: FallbackEncoder::default(),
        }
    }
}

/// Pick the encoder for an input file.
fn target_format(input: &Path, opts: &ResizeOptions) -> ImageFormat {
    match opts.output_format {
        OutputFormat::Png => ImageFormat::Png,
        OutputFormat::Jpeg => ImageFormat::Jpeg,
        OutputFormat::Webp => ImageFormat::WebP,
        OutputFormat::Original => match ImageFormat::from_path(input) {
            Ok(f) if f.writing_enabled() => f,
            _ => opts.fallback_encoder.format(),
        },
    }
}

fn format_extension(format: ImageFormat, input: &Path) -> String {
    // Keep the original extension spelling when it matches the format.
    if let Some(ext) = input.extension().and_then(|e| e.to_str())
        && format
            .extensions_str()
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext))
    {
        return ext.to_string();
    }
    format
        .extensions_str()
        .first()
        .copied()
        .unwrap_or("png")
        .to_string()
}

/// Resize one image with full options. `output_dir` = None → same folder as
/// the input. Returns the written path.
pub fn resize_image_with(
    input: &Path,
    output_dir: Option<&Path>,
    opts: &ResizeOptions,
) -> Result<PathBuf> {
    let modified = std::fs::metadata(input).and_then(|m| m.modified()).ok();
    let img = image::open(input).with_context(|| format!("failed to open {}", input.display()))?;
    let plan = compute_dimensions(
        img.width(),
        img.height(),
        &opts.size,
        opts.shrink_only,
        opts.ignore_orientation,
        (96.0, 96.0),
    )?;

    let resized = if plan.no_transform {
        img
    } else {
        let scaled = img.resize_exact(plan.scaled_width, plan.scaled_height, FilterType::Lanczos3);
        match plan.crop {
            Some((x, y, w, h)) => scaled.crop_imm(x, y, w, h),
            None => scaled,
        }
    };

    let format = target_format(input, opts);
    let ext = format_extension(format, input);
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let dir = output_dir
        .map(Path::to_path_buf)
        .or_else(|| input.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));

    let output_path = if opts.replace {
        let target = input.with_extension(&ext);
        // Write to a temporary file first so a failed encode never destroys the original.
        let tmp = dir.join(format!(".{stem}.mpt-resize-tmp.{ext}"));
        save_image(&resized, &tmp, format, opts.quality)?;
        std::fs::rename(&tmp, &target)
            .with_context(|| format!("failed to replace {}", target.display()))?;
        if target != input {
            std::fs::remove_file(input).ok();
        }
        target
    } else {
        let (sel_w, sel_h) = (opts.size.width, opts.size.height);
        let name = format_file_name(
            &opts.file_name_template,
            stem,
            &opts.size.name,
            (sel_w, sel_h),
            (resized.width(), resized.height()),
        );
        let path = unique_path(&dir, &name, &ext);
        save_image(&resized, &path, format, opts.quality)?;
        path
    };

    if opts.keep_date_modified
        && let Some(t) = modified
    {
        let res = std::fs::File::options()
            .write(true)
            .open(&output_path)
            .and_then(|f| f.set_modified(t));
        if let Err(e) = res {
            tracing::warn!(
                "Failed to keep modified date on {}: {e}",
                output_path.display()
            );
        }
    }

    info!(
        "Resized {} -> {} ({}x{})",
        input.display(),
        output_path.display(),
        resized.width(),
        resized.height()
    );
    Ok(output_path)
}

/// `dir/name.ext`, or `dir/name (N).ext` if it already exists.
fn unique_path(dir: &Path, name: &str, ext: &str) -> PathBuf {
    let mut path = dir.join(format!("{name}.{ext}"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{name} ({n}).{ext}"));
        n += 1;
    }
    path
}

/// Resize a single image (legacy API: Fit to `max_width`, shrink only,
/// `<name>_resized.<ext>` output name).
pub fn resize_image(
    input: &Path,
    output_dir: &Path,
    max_width: u32,
    format: OutputFormat,
    quality: u8,
) -> Result<PathBuf> {
    let opts = ResizeOptions {
        size: SizePreset::max_width("resized", max_width),
        output_format: format,
        quality,
        file_name_template: "%1_%2".into(),
        ..Default::default()
    };
    resize_image_with(input, Some(output_dir), &opts)
}

/// Resize multiple images.
pub fn resize_batch(
    inputs: &[PathBuf],
    output_dir: &Path,
    max_width: u32,
    format: OutputFormat,
    quality: u8,
) -> Vec<Result<PathBuf>> {
    std::fs::create_dir_all(output_dir).ok();
    inputs
        .iter()
        .map(|input| resize_image(input, output_dir, max_width, format, quality))
        .collect()
}

fn save_image(img: &DynamicImage, path: &Path, format: ImageFormat, quality: u8) -> Result<()> {
    match format {
        ImageFormat::Jpeg => {
            let file = std::fs::File::create(path)
                .with_context(|| format!("failed to create {}", path.display()))?;
            let writer = BufWriter::new(file);
            let encoder = JpegEncoder::new_with_quality(writer, quality.clamp(1, 100));
            img.to_rgb8()
                .write_with_encoder(encoder)
                .with_context(|| format!("failed to encode JPEG {}", path.display()))?;
        }
        ImageFormat::WebP | ImageFormat::Gif => {
            img.to_rgba8()
                .save_with_format(path, format)
                .with_context(|| format!("failed to save {}", path.display()))?;
        }
        _ => {
            img.save_with_format(path, format)
                .or_else(|_| img.to_rgba8().save_with_format(path, format))
                .with_context(|| format!("failed to save {}", path.display()))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DPI: (f64, f64) = (96.0, 96.0);

    fn px(fit: ResizeFit, w: impl Into<f64>, h: impl Into<f64>) -> SizePreset {
        SizePreset::new("t", fit, w.into(), h.into(), ResizeUnit::Pixel)
    }

    #[test]
    fn preset_widths() {
        assert_eq!(ResizePreset::Small.max_width(), 640);
        assert_eq!(ResizePreset::Medium.max_width(), 1280);
        assert_eq!(ResizePreset::Large.max_width(), 1920);
        assert_eq!(ResizePreset::Phone.max_width(), 1080);
    }

    #[test]
    fn default_sizes_keep_names() {
        let names: Vec<_> = default_sizes().into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["Small", "Medium", "Large", "Phone"]);
    }

    #[test]
    fn fit_keeps_aspect() {
        let p = compute_dimensions(
            4000,
            3000,
            &px(ResizeFit::Fit, 1920, 1080),
            false,
            false,
            DPI,
        )
        .unwrap();
        assert_eq!(p.output_size(), (1440, 1080));
        assert!(p.crop.is_none());
    }

    #[test]
    fn fit_width_only() {
        let p = compute_dimensions(
            4000,
            3000,
            &px(ResizeFit::Fit, 1280.0, 0.0),
            false,
            true,
            DPI,
        )
        .unwrap();
        assert_eq!(p.output_size(), (1280, 960));
    }

    #[test]
    fn fit_height_only() {
        let p = compute_dimensions(
            4000,
            3000,
            &px(ResizeFit::Fit, 0.0, 600.0),
            false,
            true,
            DPI,
        )
        .unwrap();
        assert_eq!(p.output_size(), (800, 600));
    }

    #[test]
    fn fill_crops_center() {
        let p = compute_dimensions(
            4000,
            3000,
            &px(ResizeFit::Fill, 1000, 1000),
            false,
            false,
            DPI,
        )
        .unwrap();
        assert_eq!((p.scaled_width, p.scaled_height), (1333, 1000));
        assert_eq!(p.crop, Some((166, 0, 1000, 1000)));
        assert_eq!(p.output_size(), (1000, 1000));
    }

    #[test]
    fn stretch_ignores_aspect() {
        let p = compute_dimensions(
            4000,
            3000,
            &px(ResizeFit::Stretch, 500, 500),
            false,
            false,
            DPI,
        )
        .unwrap();
        assert_eq!(p.output_size(), (500, 500));
    }

    #[test]
    fn shrink_only_skips_enlarging() {
        let p = compute_dimensions(800, 600, &px(ResizeFit::Fit, 1920, 1080), true, false, DPI)
            .unwrap();
        assert!(p.no_transform);
        assert_eq!(p.output_size(), (800, 600));
        let p = compute_dimensions(800, 600, &px(ResizeFit::Fit, 1920, 1080), false, false, DPI)
            .unwrap();
        assert_eq!(p.output_size(), (1440, 1080));
    }

    #[test]
    fn percent_scales_both_axes() {
        let s = SizePreset::new("half", ResizeFit::Fit, 50.0, 0.0, ResizeUnit::Percent);
        let p = compute_dimensions(4000, 3000, &s, true, true, DPI).unwrap();
        assert_eq!(p.output_size(), (2000, 1500));
        // Shrink-only is ignored for percent (upstream behaviour).
        let s = SizePreset::new("double", ResizeFit::Fit, 200.0, 0.0, ResizeUnit::Percent);
        let p = compute_dimensions(100, 50, &s, true, true, DPI).unwrap();
        assert_eq!(p.output_size(), (200, 100));
        // Stretch uses separate percentages.
        let s = SizePreset::new("s", ResizeFit::Stretch, 50.0, 100.0, ResizeUnit::Percent);
        let p = compute_dimensions(400, 300, &s, false, false, DPI).unwrap();
        assert_eq!(p.output_size(), (200, 300));
    }

    #[test]
    fn orientation_swap() {
        // Portrait input, landscape target box.
        let s = px(ResizeFit::Fit, 1920, 1080);
        let p = compute_dimensions(3000, 4000, &s, false, true, DPI).unwrap();
        assert_eq!(p.output_size(), (1080, 1440));
        let p = compute_dimensions(3000, 4000, &s, false, false, DPI).unwrap();
        assert_eq!(p.output_size(), (810, 1080));
    }

    #[test]
    fn inch_and_cm_units() {
        let s = SizePreset::new("in", ResizeFit::Stretch, 2.0, 1.0, ResizeUnit::Inch);
        let p = compute_dimensions(1000, 1000, &s, false, false, DPI).unwrap();
        assert_eq!(p.output_size(), (192, 96));
        let s = SizePreset::new("cm", ResizeFit::Stretch, 2.54, 2.54, ResizeUnit::Cm);
        let p = compute_dimensions(1000, 1000, &s, false, false, DPI).unwrap();
        assert_eq!(p.output_size(), (96, 96));
    }

    #[test]
    fn file_name_template() {
        assert_eq!(
            format_file_name("%1 (%2)", "photo", "Medium", (1280.0, 0.0), (1280, 960)),
            "photo (Medium)"
        );
        assert_eq!(
            format_file_name("%1_%3x%4_%5x%6", "a", "x", (1920.0, 1080.5), (1440, 1080)),
            "a_1920x1080.5_1440x1080"
        );
        // Invalid characters are replaced, unknown % codes left alone.
        assert_eq!(
            format_file_name("%1 %7 %2", "img", "a/b:c", (1.0, 1.0), (1, 1)),
            "img %7 a_b_c"
        );
        assert_eq!(format_file_name("", "img", "x", (1.0, 1.0), (1, 1)), "img_");
    }

    #[test]
    fn size_preset_serde() {
        let s = SizePreset::new("Thumb", ResizeFit::Fill, 128.0, 128.0, ResizeUnit::Cm);
        let t = toml::to_string(&s).unwrap();
        assert!(t.contains("fit = \"fill\""));
        assert!(t.contains("unit = \"cm\""));
        let back: SizePreset = toml::from_str(&t).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn end_to_end_resize_and_template() {
        let dir = std::env::temp_dir().join(format!("mpt-ir-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("pic.png");
        image::RgbaImage::new(400, 200).save(&input).unwrap();
        let opts = ResizeOptions {
            size: SizePreset::new("Half", ResizeFit::Fit, 50.0, 0.0, ResizeUnit::Percent),
            ..Default::default()
        };
        let out = resize_image_with(&input, None, &opts).unwrap();
        assert_eq!(out, dir.join("pic (Half).png"));
        assert_eq!(image::image_dimensions(&out).unwrap(), (200, 100));
        // Second run gets a uniquified name.
        let out2 = resize_image_with(&input, None, &opts).unwrap();
        assert_eq!(out2, dir.join("pic (Half) (1).png"));
        // Replace overwrites the original in place.
        let opts = ResizeOptions {
            replace: true,
            ..opts
        };
        let out3 = resize_image_with(&input, None, &opts).unwrap();
        assert_eq!(out3, input);
        assert_eq!(image::image_dimensions(&input).unwrap(), (200, 100));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
