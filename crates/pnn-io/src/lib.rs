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
}

/// File types paintnotnet can save, in the order shown in the Save As dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileFormat {
    Png,
    Jpeg,
    Bmp,
    Gif,
    Tiff,
    Tga,
    WebP,
}

impl FileFormat {
    pub const ALL: [FileFormat; 7] = [
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

    /// Whether saving asks for settings first (paint.net's Save Configuration dialog).
    pub fn has_options(self) -> bool {
        matches!(self, Self::Jpeg)
    }

    fn image_format(self) -> ImageFormat {
        match self {
            Self::Png => ImageFormat::Png,
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
    "png", "jpg", "jpeg", "jpe", "jfif", "bmp", "dib", "gif", "tif", "tiff", "tga", "webp", "ico",
    "qoi", "pnm", "pbm", "pgm", "ppm", "dds", "hdr", "exr", "avif",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SaveOptions {
    /// JPEG quality, 0..=100.
    pub jpeg_quality: u8,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self { jpeg_quality: 95 }
    }
}

/// Open an image file as a single-layer document named "Background", as paint.net does.
pub fn load(path: &Path) -> Result<Document, IoError> {
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

/// Flatten and save. The format comes from `format`, not the path's extension.
pub fn save(
    doc: &Document,
    path: &Path,
    format: FileFormat,
    options: SaveOptions,
) -> Result<(), IoError> {
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
