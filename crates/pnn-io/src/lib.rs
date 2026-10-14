//! Image codecs, OpenRaster (native format) read/write, and the clipboard bridge.
//!
//! Flat formats follow paint.net 3.36 behaviour (`Data/JpegFileType.cs`,
//! `Data/InternalFileType.cs`; MIT, see NOTICE). JPEG and 24-bit formats are composited over
//! white with the Normal blend op, and JPEG quality defaults to 95 (range 0..=100).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use image::codecs::jpeg::JpegEncoder;
use image::{ExtendedColorType, ImageEncoder, ImageFormat, ImageReader};
use pnn_core::{BlendMode, ColorBgra, Document, Layer, Surface, flatten};

pub mod ora;
pub mod pdn;

#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Image {
        path: PathBuf,
        source: image::ImageError,
    },
    #[error("{path}: unsupported file type")]
    UnsupportedFormat { path: PathBuf },
    #[error("{path}: {source}")]
    Pdn {
        path: PathBuf,
        source: pdn::PdnError,
    },
}

/// File types paintnotnet can save, in the order shown in the Save As dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileFormat {
    /// OpenRaster: layers, blend modes and opacity kept (paintnotnet's native format).
    Ora,
    /// paint.net's own format: layers kept (see `pdn`).
    Pdn,
    Png,
    Jpeg,
    Bmp,
    Gif,
    Tiff,
    Tga,
    WebP,
}

impl FileFormat {
    pub const ALL: [FileFormat; 9] = [
        Self::Ora,
        Self::Pdn,
        Self::Png,
        Self::Jpeg,
        Self::Bmp,
        Self::Gif,
        Self::Tiff,
        Self::Tga,
        Self::WebP,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ora => "OpenRaster",
            Self::Pdn => "paint.net image",
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Bmp => "BMP",
            Self::Gif => "GIF",
            Self::Tiff => "TIFF",
            Self::Tga => "TGA",
            Self::WebP => "WebP",
        }
    }

    /// Extensions, the first being the default when saving.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Ora => &["ora"],
            Self::Pdn => &["pdn"],
            Self::Png => &["png"],
            Self::Jpeg => &["jpg", "jpeg", "jpe", "jfif"],
            Self::Bmp => &["bmp", "dib"],
            Self::Gif => &["gif"],
            Self::Tiff => &["tif", "tiff"],
            Self::Tga => &["tga"],
            Self::WebP => &["webp"],
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        Self::ALL
            .into_iter()
            .find(|f| f.extensions().contains(&ext.as_str()))
    }

    /// Whether the format stores alpha. Formats without it are composited over white.
    pub fn has_alpha(self) -> bool {
        !matches!(self, Self::Jpeg | Self::Bmp)
    }

    /// Whether the format keeps layers (no flatten on save).
    pub fn layered(self) -> bool {
        matches!(self, Self::Ora | Self::Pdn)
    }

    /// Whether saving asks for settings first (paint.net's Save Configuration dialog).
    pub fn has_options(self) -> bool {
        matches!(self, Self::Jpeg | Self::WebP)
    }

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png | Self::Ora | Self::Pdn => ImageFormat::Png,
            Self::Jpeg => ImageFormat::Jpeg,
            Self::Bmp => ImageFormat::Bmp,
            Self::Gif => ImageFormat::Gif,
            Self::Tiff => ImageFormat::Tiff,
            Self::Tga => ImageFormat::Tga,
            Self::WebP => ImageFormat::WebP,
        }
    }
}

/// Extensions accepted by Open (everything the decoder understands, not just save formats).
pub const OPEN_EXTENSIONS: &[&str] = &[
    "ora", "pdn", "png", "jpg", "jpeg", "jpe", "jfif", "bmp", "dib", "gif", "tif", "tiff", "tga",
    "webp", "ico", "qoi", "pnm", "pbm", "pgm", "ppm", "dds", "hdr", "exr", "avif",
];

/// libwebp's encoder presets, as pdn-webp offers them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WebPPreset {
    Default,
    Picture,
    Photo,
    Drawing,
    Icon,
    Text,
}

impl WebPPreset {
    pub const ALL: [WebPPreset; 6] = [
        Self::Default,
        Self::Picture,
        Self::Photo,
        Self::Drawing,
        Self::Icon,
        Self::Text,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Picture => "Picture",
            Self::Photo => "Photo",
            Self::Drawing => "Drawing",
            Self::Icon => "Icon",
            Self::Text => "Text",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveOptions {
    /// JPEG quality, 0..=100.
    pub jpeg_quality: u8,
    /// WebP save settings: pdn-webp's (bundled with paint.net 5) names and defaults.
    pub webp_preset: WebPPreset,
    /// 0..=100; ignored when lossless.
    pub webp_quality: u8,
    /// 0..=9 (speed/size trade-off).
    pub webp_effort: u8,
    pub webp_lossless: bool,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: 95,
            webp_preset: WebPPreset::Photo,
            webp_quality: 95,
            webp_effort: 7,
            webp_lossless: false,
        }
    }
}

/// Open an image file as a single-layer document named "Background", as paint.net does.
pub fn load(path: &Path) -> Result<Document, IoError> {
    let bytes = fs::read(path).map_err(|source| IoError::Io {
        path: path.to_owned(),
        source,
    })?;
    // OpenRaster: a zip starting with an uncompressed "mimetype" entry.
    if bytes.starts_with(b"PK") && bytes.get(30..38) == Some(b"mimetype") {
        return ora::decode(&bytes, path);
    }
    // paint.net: by content, or (for 2.0 and earlier, a bare gzip stream) by extension.
    let pdn_ext = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdn"));
    if bytes.starts_with(b"PDN3") || (pdn_ext && bytes.starts_with(&[0x1f, 0x8b])) {
        return pdn::decode(&bytes).map_err(|source| IoError::Pdn {
            path: path.to_owned(),
            source,
        });
    }
    let image_err = |source| IoError::Image {
        path: path.to_owned(),
        source,
    };
    let mut reader = ImageReader::open(path)
        .map_err(|source| IoError::Io {
            path: path.to_owned(),
            source,
        })?
        .with_guessed_format()
        .map_err(|source| IoError::Io {
            path: path.to_owned(),
            source,
        })?;
    // Large images (e.g. 16-bit 8K) exceed the decoder's default allocation cap.
    reader.no_limits();
    let rgba = reader.decode().map_err(image_err)?.into_rgba8();
    let (width, height) = rgba.dimensions();
    let pixels: Vec<ColorBgra> = rgba
        .pixels()
        .map(|p| ColorBgra::from_rgba(p[0], p[1], p[2], p[3]))
        .collect();
    let mut surface = Surface::new_transparent(width, height);
    surface.write_pixels(&pixels);
    Ok(Document::from_layers(
        width,
        height,
        vec![Layer::new("Background", surface)],
    ))
}

/// Save (flattened, except in the layered formats). The format comes from `format`, not
/// the path's extension.
pub fn save(
    doc: &Document,
    path: &Path,
    format: FileFormat,
    options: SaveOptions,
) -> Result<(), IoError> {
    if format == FileFormat::Ora {
        let bytes = ora::encode(doc).map_err(|msg| IoError::Io {
            path: path.to_owned(),
            source: std::io::Error::other(msg),
        })?;
        return write_atomically(path, &bytes);
    }
    if format == FileFormat::Pdn {
        let bytes = pdn::encode(doc).map_err(|source| IoError::Io {
            path: path.to_owned(),
            source,
        })?;
        return write_atomically(path, &bytes);
    }
    let mut pixels = flatten(doc).to_pixels();
    if !format.has_alpha() {
        for p in &mut pixels {
            *p = BlendMode::Normal.apply(ColorBgra::WHITE, *p, 255);
        }
    }
    let bytes = encode(&pixels, doc.width(), doc.height(), format, options).map_err(|source| {
        IoError::Image {
            path: path.to_owned(),
            source,
        }
    })?;
    write_atomically(path, &bytes)
}

fn encode(
    pixels: &[ColorBgra],
    width: u32,
    height: u32,
    format: FileFormat,
    options: SaveOptions,
) -> Result<Vec<u8>, image::ImageError> {
    let mut out = std::io::Cursor::new(Vec::new());
    let rgb = || {
        pixels
            .iter()
            .flat_map(|p| [p.r, p.g, p.b])
            .collect::<Vec<u8>>()
    };
    match format {
        FileFormat::Jpeg => JpegEncoder::new_with_quality(&mut out, options.jpeg_quality.min(100))
            .write_image(&rgb(), width, height, ExtendedColorType::Rgb8)?,
        FileFormat::WebP => return encode_webp(pixels, width, height, options),
        FileFormat::Bmp => image::RgbImage::from_raw(width, height, rgb())
            .expect("buffer size matches")
            .write_to(&mut out, format.image_format())?,
        _ => {
            let rgba: Vec<u8> = pixels.iter().flat_map(|p| p.to_rgba_array()).collect();
            image::RgbaImage::from_raw(width, height, rgba)
                .expect("buffer size matches")
                .write_to(&mut out, format.image_format())?
        }
    }
    Ok(out.into_inner())
}

/// WebP through libwebp, configured as pdn-webp 2.x does (`src/WebP/WebPEncoder.cpp`, MIT,
/// see NOTICE): the preset at the chosen quality; lossless uses libwebp's lossless preset at
/// the effort level, keeps transparent pixels' colour (`exact`) and hints the content type;
/// lossy maps effort 0–6 to `method`, and 7–9 add sharp YUV, auto-filter and best alpha
/// filtering.
fn encode_webp(
    pixels: &[ColorBgra],
    width: u32,
    height: u32,
    o: SaveOptions,
) -> Result<Vec<u8>, image::ImageError> {
    use libwebp_sys as w;
    let fail = |msg: String| {
        image::ImageError::Encoding(image::error::EncodingError::new(
            image::error::ImageFormatHint::Exact(ImageFormat::WebP),
            msg,
        ))
    };
    let preset = match o.webp_preset {
        WebPPreset::Default => w::WebPPreset::WEBP_PRESET_DEFAULT,
        WebPPreset::Picture => w::WebPPreset::WEBP_PRESET_PICTURE,
        WebPPreset::Photo => w::WebPPreset::WEBP_PRESET_PHOTO,
        WebPPreset::Drawing => w::WebPPreset::WEBP_PRESET_DRAWING,
        WebPPreset::Icon => w::WebPPreset::WEBP_PRESET_ICON,
        WebPPreset::Text => w::WebPPreset::WEBP_PRESET_TEXT,
    };
    let mut config = w::WebPConfig::new_with_preset(preset, o.webp_quality.min(100) as f32)
        .map_err(|_| fail("libwebp version mismatch".into()))?;
    config.thread_level = 1;
    let effort = o.webp_effort.min(9) as i32;
    if o.webp_lossless {
        // libwebp's `WebPConfigLosslessPreset` (src/enc/config_enc.c), done here so no
        // `unsafe` FFI call is needed: (method, quality) per level.
        const LOSSLESS: [(i32, f32); 10] = [
            (0, 0.0),
            (1, 20.0),
            (2, 25.0),
            (3, 30.0),
            (3, 50.0),
            (4, 50.0),
            (4, 75.0),
            (4, 90.0),
            (5, 90.0),
            (6, 100.0),
        ];
        let (method, quality) = LOSSLESS[effort as usize];
        config.lossless = 1;
        config.method = method;
        config.quality = quality;
        config.exact = 1;
        config.image_hint = match o.webp_preset {
            WebPPreset::Photo => w::WebPImageHint::WEBP_HINT_PHOTO,
            WebPPreset::Picture => w::WebPImageHint::WEBP_HINT_PICTURE,
            WebPPreset::Drawing => w::WebPImageHint::WEBP_HINT_GRAPH,
            _ => config.image_hint,
        };
    } else {
        config.method = effort.min(6);
        if effort >= 7 {
            config.use_sharp_yuv = 1;
        }
        if effort >= 8 {
            config.autofilter = 1;
        }
        if effort >= 9 {
            config.alpha_filtering = 2;
        }
    }
    let opaque = pixels.iter().all(|p| p.a == 255);
    let mem = if opaque {
        // pdn-webp imports opaque images without alpha.
        let rgb: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
        webp::Encoder::from_rgb(&rgb, width, height).encode_advanced(&config)
    } else {
        let rgba: Vec<u8> = pixels.iter().flat_map(|p| p.to_rgba_array()).collect();
        webp::Encoder::from_rgba(&rgba, width, height).encode_advanced(&config)
    }
    .map_err(|e| fail(format!("{e:?}")))?;
    Ok(mem.to_vec())
}

/// Write to a temporary file next to `path`, then rename over it, so a failed save never
/// leaves a truncated file behind.
fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let io_err = |source| IoError::Io {
        path: path.to_owned(),
        source,
    };
    let dir = path
        .parent()
        .filter(|d| !d.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let file_name = path
        .file_name()
        .ok_or_else(|| io_err(std::io::ErrorKind::InvalidInput.into()))?;
    let tmp = dir.join(format!(".{}.paintnotnet-tmp", file_name.to_string_lossy()));
    let result = (|| {
        let mut file = fs::File::create(&tmp).map_err(io_err)?;
        file.write_all(bytes).map_err(io_err)?;
        file.sync_all().map_err(io_err)?;
        fs::rename(&tmp, path).map_err(io_err)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_doc() -> Document {
        let mut s = Surface::new_transparent(300, 200);
        s.set(0, 0, ColorBgra::from_rgba(255, 0, 0, 255));
        s.set(299, 199, ColorBgra::from_rgba(0, 0, 255, 128));
        Document::from_layers(300, 200, vec![Layer::new("Background", s)])
    }

    #[test]
    fn webp_lossy_and_lossless_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        // A smooth gradient with a transparent corner.
        let mut s = Surface::new_transparent(64, 48);
        for y in 0..48 {
            for x in 0..64 {
                let a = if x < 8 && y < 8 { 0 } else { 255 };
                s.set(
                    x,
                    y,
                    ColorBgra::from_rgba((x * 4) as u8, (y * 5) as u8, 128, a),
                );
            }
        }
        let doc = Document::from_layers(64, 48, vec![Layer::new("Background", s)]);
        let lossless = dir.path().join("l.webp");
        let opts = SaveOptions {
            webp_lossless: true,
            ..SaveOptions::default()
        };
        save(&doc, &lossless, FileFormat::WebP, opts).unwrap();
        let back = load(&lossless).unwrap();
        let (a, b) = (&doc.layer(0).surface, &back.layer(0).surface);
        assert_eq!(a.to_pixels(), b.to_pixels(), "lossless is exact");
        let lossy = dir.path().join("q.webp");
        let small = SaveOptions {
            webp_quality: 50,
            ..SaveOptions::default()
        };
        save(&doc, &lossy, FileFormat::WebP, small).unwrap();
        let back = load(&lossy).unwrap();
        let p = back.layer(0).surface.get(40, 30);
        let q = doc.layer(0).surface.get(40, 30);
        assert!((p.r as i32 - q.r as i32).abs() < 12, "{p:?} vs {q:?}");
        assert_eq!(back.layer(0).surface.get(2, 2).a, 0, "alpha kept");
        // On photo-like noise, lossy is much smaller than lossless.
        let mut n = Surface::new_transparent(128, 128);
        let mut seed = 12345u32;
        for y in 0..128 {
            for x in 0..128 {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let v = (seed >> 16) as u8;
                n.set(x, y, ColorBgra::from_rgba(v, v / 2, 255 - v, 255));
            }
        }
        let noisy = Document::from_layers(128, 128, vec![Layer::new("Background", n)]);
        let (nl, nq) = (dir.path().join("nl.webp"), dir.path().join("nq.webp"));
        save(&noisy, &nl, FileFormat::WebP, opts).unwrap();
        save(&noisy, &nq, FileFormat::WebP, small).unwrap();
        let size = |p: &std::path::Path| std::fs::metadata(p).unwrap().len();
        assert!(
            size(&nq) < size(&nl),
            "lossy {} vs lossless {}",
            size(&nq),
            size(&nl)
        );
    }

    #[test]
    fn png_round_trip_keeps_alpha() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.png");
        save(
            &sample_doc(),
            &path,
            FileFormat::Png,
            SaveOptions::default(),
        )
        .unwrap();
        let back = load(&path).unwrap();
        assert_eq!((back.width(), back.height()), (300, 200));
        assert_eq!(back.layer(0).name, "Background");
        let s = &back.layer(0).surface;
        assert_eq!(s.get(0, 0), ColorBgra::from_rgba(255, 0, 0, 255));
        assert_eq!(s.get(299, 199), ColorBgra::from_rgba(0, 0, 255, 128));
        assert_eq!(s.get(1, 1), ColorBgra::TRANSPARENT);
    }

    #[test]
    fn bmp_composites_over_white() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bmp");
        save(
            &sample_doc(),
            &path,
            FileFormat::Bmp,
            SaveOptions::default(),
        )
        .unwrap();
        let s = load(&path).unwrap().layer(0).surface.clone();
        assert_eq!(s.get(1, 1), ColorBgra::WHITE);
        let expected =
            BlendMode::Normal.apply(ColorBgra::WHITE, ColorBgra::from_rgba(0, 0, 255, 128), 255);
        assert_eq!(s.get(299, 199), expected);
    }

    #[test]
    fn every_save_format_round_trips_dimensions() {
        let dir = tempfile::tempdir().unwrap();
        for format in FileFormat::ALL {
            let path = dir.path().join(format!("a.{}", format.extensions()[0]));
            save(&sample_doc(), &path, format, SaveOptions::default())
                .unwrap_or_else(|e| panic!("{format:?}: {e}"));
            let back = load(&path).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert_eq!((back.width(), back.height()), (300, 200), "{format:?}");
        }
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains("paintnotnet-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temporary files left behind");
    }

    #[test]
    fn format_from_extension() {
        assert_eq!(
            FileFormat::from_path(Path::new("x/photo.JPG")),
            Some(FileFormat::Jpeg)
        );
        assert_eq!(
            FileFormat::from_path(Path::new("x/a.tiff")),
            Some(FileFormat::Tiff)
        );
        assert_eq!(FileFormat::from_path(Path::new("x/a.xyz")), None);
    }

    #[test]
    fn failed_save_leaves_no_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing-dir").join("a.png");
        assert!(
            save(
                &sample_doc(),
                &path,
                FileFormat::Png,
                SaveOptions::default()
            )
            .is_err()
        );
    }
}
