//! Parity tests: Microsoft PowerToys Image Resizer behaviour is the oracle.
//!
//! Ported from `src/modules/imageresizer/tests/Models/ResizeOperationTests.cs` and from
//! `ui/Models/ResizeOperation.cs`, `ui/Models/ResizeSize.cs`, `ui/Properties/Settings.cs`
//! and `src/settings-ui/Settings.UI.Library/ImageResizerProperties.cs`.
//! Real images are generated, resized on disk and their output inspected.

use crate::{
    FallbackEncoder, ImageResizerConfig, OutputFormat, ResizeFit, ResizeOptions, ResizeUnit,
    SizePreset, default_sizes, resize_image_with,
};
use image::{AnimationDecoder, Rgba, RgbaImage};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

// ── helpers ────────────────────────────────────────────────────────────

struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!(
            "mpt-ir-parity-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        Tmp(p)
    }
    fn names(&self) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 192x96 like upstream Test.png: black left/right quarters, white centre half.
fn test_png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let img = RgbaImage::from_fn(w, h, |x, _| {
        if x < w / 4 || x >= w - w / 4 {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([255, 255, 255, 255])
        }
    });
    let p = dir.join(name);
    img.save(&p).unwrap();
    p
}

/// Upstream test `Settings()`: one size "Test" 96x96 Fit px, ShrinkOnly=false (ctor default).
fn opts(f: impl FnOnce(&mut ResizeOptions)) -> ResizeOptions {
    let mut o = ResizeOptions {
        size: SizePreset::new("Test", ResizeFit::Fit, 96.0, 96.0, ResizeUnit::Pixel),
        output_format: OutputFormat::Original,
        quality: 90,
        shrink_only: false,
        replace: false,
        ignore_orientation: true,
        keep_date_modified: false,
        file_name_template: "%1 (%2)".into(),
        fallback_encoder: FallbackEncoder::Jpeg,
    };
    f(&mut o);
    o
}

fn dims(p: &Path) -> (u32, u32) {
    image::image_dimensions(p).unwrap()
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// Insert a pHYs chunk (pixels per metre) right after IHDR.
fn png_with_dpi(path: &Path, dpi: f64) {
    let mut data = fs::read(path).unwrap();
    let ppm = (dpi / 0.0254).round() as u32;
    let mut body = b"pHYs".to_vec();
    body.extend_from_slice(&ppm.to_be_bytes());
    body.extend_from_slice(&ppm.to_be_bytes());
    body.push(1);
    let mut chunk = 9u32.to_be_bytes().to_vec();
    chunk.extend_from_slice(&body);
    chunk.extend_from_slice(&crc32(&body).to_be_bytes());
    let at = 8 + 25; // signature + IHDR chunk
    data.splice(at..at, chunk);
    fs::write(path, data).unwrap();
}

/// JPEG with an EXIF APP1 (Make = "Canon", Orientation = 6).
fn jpeg_with_exif(path: &Path) {
    let img =
        image::DynamicImage::ImageRgba8(RgbaImage::from_pixel(192, 96, Rgba([200, 10, 10, 255])));
    let mut buf = Vec::new();
    img.to_rgb8()
        .write_with_encoder(image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut buf, 90,
        ))
        .unwrap();
    let mut tiff: Vec<u8> = b"II\x2a\x00\x08\x00\x00\x00".to_vec();
    tiff.extend_from_slice(&2u16.to_le_bytes());
    // Make, ASCII, count 6, offset 38
    tiff.extend_from_slice(&0x010Fu16.to_le_bytes());
    tiff.extend_from_slice(&2u16.to_le_bytes());
    tiff.extend_from_slice(&6u32.to_le_bytes());
    tiff.extend_from_slice(&38u32.to_le_bytes());
    // Orientation, SHORT, 1, value 6
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes());
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&[6, 0, 0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes());
    tiff.extend_from_slice(b"Canon\0");
    let mut payload = b"Exif\0\0".to_vec();
    payload.extend_from_slice(&tiff);
    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    app1.extend_from_slice(&payload);
    buf.splice(2..2, app1);
    fs::write(path, buf).unwrap();
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

// ── defaults ───────────────────────────────────────────────────────────

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn default_size_presets_match_powertoys() {
    // Settings.cs / ImageResizerProperties.cs: Small 854x480, Medium 1366x768,
    // Large 1920x1080, Phone 320x568, all Fit + Pixel.
    let got: Vec<(String, ResizeFit, f64, f64, ResizeUnit)> = default_sizes()
        .into_iter()
        .map(|s| (s.name, s.fit, s.width, s.height, s.unit))
        .collect();
    let exp = vec![
        (
            "Small".to_string(),
            ResizeFit::Fit,
            854.0,
            480.0,
            ResizeUnit::Pixel,
        ),
        (
            "Medium".to_string(),
            ResizeFit::Fit,
            1366.0,
            768.0,
            ResizeUnit::Pixel,
        ),
        (
            "Large".to_string(),
            ResizeFit::Fit,
            1920.0,
            1080.0,
            ResizeUnit::Pixel,
        ),
        (
            "Phone".to_string(),
            ResizeFit::Fit,
            320.0,
            568.0,
            ResizeUnit::Pixel,
        ),
    ];
    assert_eq!(got, exp);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn default_options_match_powertoys() {
    let c = ImageResizerConfig::default();
    let mut bad = Vec::new();
    if c.shrink_only {
        bad.push("shrink_only default true (upstream ImageresizerShrinkOnly=false)");
    }
    if c.quality != 90 {
        bad.push("JPEG quality default != 90");
    }
    if !c.ignore_orientation {
        bad.push("ignore_orientation default false");
    }
    if c.replace || c.keep_date_modified {
        bad.push("replace/keep_date default true");
    }
    if c.file_name_template != "%1 (%2)" {
        bad.push("file name template");
    }
    if c.fallback_encoder != FallbackEncoder::Jpeg {
        bad.push("fallback encoder (upstream JPEG container GUID 19e4a5aa-…)");
    }
    if (
        c.custom_size.fit,
        c.custom_size.width,
        c.custom_size.height,
        c.custom_size.unit,
    ) != (ResizeFit::Fit, 1024.0, 640.0, ResizeUnit::Pixel)
    {
        bad.push("custom size != Fit 1024x640 px");
    }
    if c.active_size().name != "Small" {
        bad.push("initially selected size is not index 0 (Small)");
    }
    assert!(bad.is_empty(), "{bad:#?}");
}

// ── ResizeOperationTests.cs ────────────────────────────────────────────

#[test]
fn fit_96x96_gives_96x48_and_default_name() {
    let t = Tmp::new("fit");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let out = resize_image_with(&src, None, &opts(|_| {})).unwrap();
    assert_eq!(out.file_name().unwrap(), "Test (Test).png");
    assert_eq!(dims(&out), (96, 48));
}

#[test]
fn fill_crops_centre_and_stretch_does_not() {
    let t = Tmp::new("fill");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let out = resize_image_with(&src, None, &opts(|o| o.size.fit = ResizeFit::Fill)).unwrap();
    assert_eq!(dims(&out), (96, 96));
    let px = image::open(&out).unwrap().to_rgba8().get_pixel(0, 0).0;
    assert!(
        px[0] > 200 && px[1] > 200 && px[2] > 200,
        "Fill first pixel should be white, got {px:?}"
    );
    let out = resize_image_with(&src, None, &opts(|o| o.size.fit = ResizeFit::Stretch)).unwrap();
    assert_eq!(dims(&out), (96, 96));
    let px = image::open(&out).unwrap().to_rgba8().get_pixel(0, 0).0;
    assert!(
        px[0] < 50 && px[1] < 50 && px[2] < 50,
        "Stretch first pixel should be black, got {px:?}"
    );
}

#[test]
fn ignore_orientation_cases() {
    let t = Tmp::new("orient");
    let land = test_png(&t.0, "Test.png", 192, 96);
    let port = test_png(&t.0, "TestPortrait.png", 96, 192);
    let o = opts(|o| {
        o.size.width = 96.0;
        o.size.height = 192.0;
    });
    assert_eq!(
        dims(&resize_image_with(&land, None, &o).unwrap()),
        (192, 96)
    );
    let o = opts(|o| {
        o.size.width = 192.0;
        o.size.height = 96.0;
    });
    assert_eq!(
        dims(&resize_image_with(&port, None, &o).unwrap()),
        (96, 192)
    );
    // Auto height: no swap.
    let o = opts(|o| {
        o.size.width = 96.0;
        o.size.height = 0.0;
    });
    assert_eq!(dims(&resize_image_with(&land, None, &o).unwrap()), (96, 48));
    // Percent stretch: no swap.
    let o = opts(|o| {
        o.size = SizePreset::new("Test", ResizeFit::Stretch, 50.0, 200.0, ResizeUnit::Percent);
    });
    assert_eq!(
        dims(&resize_image_with(&land, None, &o).unwrap()),
        (96, 192)
    );
}

#[test]
fn shrink_only_cases() {
    let t = Tmp::new("shrink");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let o = opts(|o| {
        o.shrink_only = true;
        o.size.width = 288.0;
        o.size.height = 288.0;
    });
    assert_eq!(dims(&resize_image_with(&src, None, &o).unwrap()), (192, 96));
    // Percent ignores shrink-only.
    let o = opts(|o| {
        o.shrink_only = true;
        o.size = SizePreset::new("Test", ResizeFit::Fit, 200.0, 0.0, ResizeUnit::Percent);
    });
    assert_eq!(
        dims(&resize_image_with(&src, None, &o).unwrap()),
        (384, 192)
    );
    // Auto height
    let o = opts(|o| {
        o.shrink_only = true;
        o.size.width = 288.0;
        o.size.height = 0.0;
    });
    assert_eq!(dims(&resize_image_with(&src, None, &o).unwrap()).0, 192);
    // Fill + shrink-only, crop required (upstream uses square Test.jpg 96x96)
    let src = test_png(&t.0, "Square.png", 96, 96);
    let o = opts(|o| {
        o.shrink_only = true;
        o.size.fit = ResizeFit::Fill;
        o.size.width = 48.0;
        o.size.height = 96.0;
    });
    assert_eq!(dims(&resize_image_with(&src, None, &o).unwrap()), (48, 96));
}

#[test]
fn out_of_range_dimensions_rejected_before_writing() {
    let t = Tmp::new("range");
    let src = test_png(&t.0, "Test.png", 192, 96);
    for w in [f64::from(i32::MAX) + 1.0, -0.4] {
        let o = opts(|o| {
            o.size.fit = ResizeFit::Stretch;
            o.size.width = w;
            o.size.height = 100.0;
        });
        assert!(
            resize_image_with(&src, None, &o).is_err(),
            "width {w} accepted"
        );
    }
    assert_eq!(t.names(), ["Test.png"]);
}

#[test]
fn rounds_tiny_fill_to_one_pixel() {
    let p = crate::compute_dimensions(
        192,
        96,
        &SizePreset::new("t", ResizeFit::Fill, 0.4, 100.0, ResizeUnit::Pixel),
        false,
        false,
        (96.0, 96.0),
    )
    .unwrap();
    assert_eq!(p.output_size().0, 1);
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn inch_unit_uses_image_dpi() {
    // TransformHonorsUnit: 1in x 1in -> PixelWidth == ceil(decoder.DpiX)
    let t = Tmp::new("dpi");
    let src = test_png(&t.0, "Test.png", 600, 600);
    png_with_dpi(&src, 300.0);
    let o = opts(|o| {
        o.size = SizePreset::new("Test", ResizeFit::Fit, 1.0, 1.0, ResizeUnit::Inch);
    });
    let out = resize_image_with(&src, None, &o).unwrap();
    assert_eq!(dims(&out).0, 300, "1 inch at 300 DPI");
}

#[test]
fn inch_unit_without_dpi_uses_96() {
    let t = Tmp::new("dpi96");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let o = opts(|o| {
        o.size = SizePreset::new("Test", ResizeFit::Stretch, 1.0, 1.0, ResizeUnit::Inch);
    });
    assert_eq!(dims(&resize_image_with(&src, None, &o).unwrap()), (96, 96));
    let o = opts(|o| {
        o.size = SizePreset::new("Test", ResizeFit::Stretch, 2.54, 2.54, ResizeUnit::Cm);
    });
    assert_eq!(dims(&resize_image_with(&src, None, &o).unwrap()), (96, 96));
}

#[test]
fn uniquifies_output_filename() {
    let t = Tmp::new("uniq");
    let src = test_png(&t.0, "Test.png", 192, 96);
    fs::write(t.0.join("Test (Test).png"), b"").unwrap();
    let o = opts(|_| {});
    assert_eq!(
        resize_image_with(&src, None, &o)
            .unwrap()
            .file_name()
            .unwrap(),
        "Test (Test) (1).png"
    );
    assert_eq!(
        resize_image_with(&src, None, &o)
            .unwrap()
            .file_name()
            .unwrap(),
        "Test (Test) (2).png"
    );
}

#[test]
fn file_name_format_all_params() {
    let t = Tmp::new("fmt");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let o = opts(|o| o.file_name_template = "%1_%2_%3_%4_%5_%6".into());
    let out = resize_image_with(&src, None, &o).unwrap();
    assert_eq!(out.file_name().unwrap(), "Test_Test_96_96_96_48.png");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn file_name_format_with_directory() {
    // ExecuteHandlesDirectoriesInFileNameFormat: "Directory\%1 (%2)" creates a sub-folder
    // (Linux separator is '/').
    let t = Tmp::new("fmtdir");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let o = opts(|o| o.file_name_template = "Directory/%1 (%2)".into());
    let out = resize_image_with(&src, None, &o).unwrap();
    assert!(
        t.0.join("Directory/Test (Test).png").exists(),
        "output went to {out:?}"
    );
}

#[test]
fn file_name_is_sanitized() {
    let t = Tmp::new("sanitize");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let o = opts(|o| {
        o.file_name_template = "%1:*?\"<>|(%2)".into();
        o.size.name = "Test\\/".into();
    });
    let out = resize_image_with(&src, None, &o).unwrap();
    assert_eq!(out.file_name().unwrap(), "Test_______(Test__).png");
}

#[test]
fn extension_spelling_kept() {
    let t = Tmp::new("ext");
    let src = test_png(&t.0, "a.png", 192, 96);
    let upper = t.0.join("Test.PNG");
    fs::rename(&src, &upper).unwrap();
    let out = resize_image_with(&upper, None, &opts(|_| {})).unwrap();
    assert_eq!(out.file_name().unwrap(), "Test (Test).PNG");
}

#[test]
fn replace_overwrites_original() {
    let t = Tmp::new("replace");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let out = resize_image_with(&src, None, &opts(|o| o.replace = true)).unwrap();
    assert_eq!(out, src);
    assert_eq!(dims(&src).0, 96);
    assert_eq!(t.names(), ["Test.png"]);
}

#[test]
fn keep_date_modified_and_replace() {
    let t = Tmp::new("date");
    let src = test_png(&t.0, "Test.png", 192, 96);
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
    fs::File::options()
        .write(true)
        .open(&src)
        .unwrap()
        .set_modified(old)
        .unwrap();
    let out = resize_image_with(&src, None, &opts(|o| o.keep_date_modified = true)).unwrap();
    assert_eq!(fs::metadata(&out).unwrap().modified().unwrap(), old);
    let out = resize_image_with(&src, None, &opts(|_| {})).unwrap();
    assert_ne!(fs::metadata(&out).unwrap().modified().unwrap(), old);
    let out = resize_image_with(
        &src,
        None,
        &opts(|o| {
            o.keep_date_modified = true;
            o.replace = true;
        }),
    )
    .unwrap();
    assert_eq!(out, src);
    assert_eq!(fs::metadata(&src).unwrap().modified().unwrap(), old);
    assert_eq!(dims(&src).0, 96);
}

#[test]
fn jpeg_quality_changes_output() {
    let t = Tmp::new("quality");
    let img = RgbaImage::from_fn(400, 300, |x, y| {
        Rgba([
            (x * 7 % 256) as u8,
            (y * 13 % 256) as u8,
            ((x ^ y) % 256) as u8,
            255,
        ])
    });
    let src = t.0.join("Test.jpg");
    image::DynamicImage::ImageRgba8(img)
        .to_rgb8()
        .save(&src)
        .unwrap();
    let size = |q: u8| {
        let o = opts(|o| {
            o.quality = q;
            o.size.width = 200.0;
            o.size.height = 200.0;
        });
        fs::metadata(resize_image_with(&src, None, &o).unwrap())
            .unwrap()
            .len()
    };
    let (lo, hi) = (size(10), size(95));
    assert!(lo < hi, "q10={lo} q95={hi}");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn metadata_kept_by_default() {
    // RemoveMetadata=false (default): EXIF (incl. orientation) is copied to the output.
    let t = Tmp::new("meta");
    let src = t.0.join("Test.jpg");
    jpeg_with_exif(&src);
    assert!(contains(&fs::read(&src).unwrap(), b"Canon"));
    let out = resize_image_with(&src, None, &opts(|_| {})).unwrap();
    let data = fs::read(&out).unwrap();
    assert!(contains(&data, b"Exif\0\0"), "EXIF block dropped");
    assert!(contains(&data, b"Canon"), "camera make dropped");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn every_gif_frame_is_resized() {
    // ExecuteTransformsEachFrame: a 2-frame image keeps 2 frames, each resized.
    let t = Tmp::new("frames");
    let src = t.0.join("Test.gif");
    {
        let f = fs::File::create(&src).unwrap();
        let mut enc = image::codecs::gif::GifEncoder::new(f);
        let frames = [Rgba([255, 0, 0, 255]), Rgba([0, 0, 255, 255])]
            .map(|c| image::Frame::new(RgbaImage::from_pixel(192, 96, c)));
        enc.encode_frames(frames).unwrap();
    }
    let out = resize_image_with(&src, None, &opts(|_| {})).unwrap();
    let dec =
        image::codecs::gif::GifDecoder::new(std::io::BufReader::new(fs::File::open(&out).unwrap()))
            .unwrap();
    let frames = dec.into_frames().collect_frames().unwrap();
    assert_eq!(frames.len(), 2, "frame count");
    assert_eq!(frames[0].buffer().dimensions(), (96, 48));
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn ico_uses_fallback_encoder() {
    // ExecuteUsesFallbackEncoder: ICO has no WIC encoder -> fallback (PNG in the test).
    let t = Tmp::new("ico");
    let src = t.0.join("Test.ico");
    RgbaImage::from_pixel(64, 64, Rgba([1, 2, 3, 255]))
        .save(&src)
        .unwrap();
    let out = resize_image_with(
        &src,
        None,
        &opts(|o| {
            o.fallback_encoder = FallbackEncoder::Png;
            o.size.width = 32.0;
            o.size.height = 32.0;
        }),
    )
    .unwrap();
    assert_eq!(out.file_name().unwrap(), "Test (Test).png");
}

#[test]
#[ignore = "known PowerToys parity gap, see docs/parity/B-files.md"]
fn default_presets_resize_real_photo() {
    // Default presets applied to a 4000x3000 photo (upstream Fit WxH boxes).
    let t = Tmp::new("presets");
    let src = test_png(&t.0, "photo.png", 4000, 3000);
    let exp = [
        ("Small", (640, 480)),
        ("Medium", (1024, 768)),
        ("Large", (1440, 1080)),
        ("Phone", (427, 320)),
    ]; // phone box swapped for landscape input
    let mut bad = Vec::new();
    for (size, (name, want)) in default_sizes().into_iter().zip(exp) {
        let o = opts(|o| o.size = size.clone());
        let out = resize_image_with(&src, None, &o).unwrap();
        let got = dims(&out);
        if got != want || out.file_name().unwrap() != format!("photo ({name}).png").as_str() {
            bad.push(format!(
                "{name}: want {want:?} got {got:?} {:?}",
                out.file_name().unwrap()
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
