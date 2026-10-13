//! OpenRaster (`.ora`), paintnotnet's layered format.
//!
//! Follows the OpenRaster 0.0.6 specification (<https://www.openraster.org/>): a zip whose
//! first entry is an uncompressed `mimetype` (`image/openraster`), a `stack.xml` listing the
//! layers top-first, one PNG per layer under `data/`, a `mergedimage.png` and a
//! `Thumbnails/thumbnail.png` (at most 256 px).
//!
//! Blend modes: paint.net modes with an OpenRaster/SVG equivalent use it (`svg:multiply`, …);
//! Reflect, Glow, Negation and paint.net's Xor have none and are written as `pdn:reflect`
//! etc. (other programs treat unknown ops as normal). Layers in files from other programs may
//! be offset or smaller than the image; they're placed at their x/y.

use std::io::{Cursor, Read, Write};
use std::path::Path;

use pnn_core::{BlendMode, ColorBgra, Document, Layer, Surface, flatten};
use quick_xml::events::Event;
use zip::write::SimpleFileOptions;

use crate::IoError;

const MIME: &str = "image/openraster";

/// paint.net blend mode → OpenRaster `composite-op`.
pub fn composite_op(m: BlendMode) -> &'static str {
    match m {
        BlendMode::Normal => "svg:src-over",
        BlendMode::Multiply => "svg:multiply",
        BlendMode::Additive => "svg:plus",
        BlendMode::ColorBurn => "svg:color-burn",
        BlendMode::ColorDodge => "svg:color-dodge",
        BlendMode::Reflect => "pdn:reflect",
        BlendMode::Glow => "pdn:glow",
        BlendMode::Overlay => "svg:overlay",
        BlendMode::Difference => "svg:difference",
        BlendMode::Negation => "pdn:negation",
        BlendMode::Lighten => "svg:lighten",
        BlendMode::Darken => "svg:darken",
        BlendMode::Screen => "svg:screen",
        BlendMode::Xor => "pdn:xor",
    }
}

fn blend_from(op: &str) -> BlendMode {
    BlendMode::ALL
        .into_iter()
        .find(|&m| composite_op(m) == op)
        .unwrap_or(BlendMode::Normal)
}

fn png(pixels: &[ColorBgra], w: u32, h: u32) -> Result<Vec<u8>, image::ImageError> {
    let rgba: Vec<u8> = pixels.iter().flat_map(|p| p.to_rgba_array()).collect();
    let mut out = Cursor::new(Vec::new());
    image::RgbaImage::from_raw(w, h, rgba)
        .expect("buffer size matches")
        .write_to(&mut out, image::ImageFormat::Png)?;
    Ok(out.into_inner())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Encode `doc` as an OpenRaster file.
pub fn encode(doc: &Document) -> Result<Vec<u8>, String> {
    let (w, h) = (doc.width(), doc.height());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let deflated =
        SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let e = |e: zip::result::ZipError| e.to_string();
    let io = |e: std::io::Error| e.to_string();
    zip.start_file("mimetype", stored).map_err(e)?;
    zip.write_all(MIME.as_bytes()).map_err(io)?;

    let mut stack = String::new();
    stack.push_str(&format!(
        "<?xml version='1.0' encoding='UTF-8'?>\n<image version=\"0.0.6\" w=\"{w}\" h=\"{h}\" \
         xres=\"{0}\" yres=\"{0}\">\n  <stack>\n",
        doc.dpi.round() as i64
    ));
    // Top layer first.
    let n = doc.layers().len();
    for (i, layer) in doc.layers().iter().enumerate().rev() {
        let file = format!("data/layer{i}.png");
        let bytes = png(&layer.surface.to_pixels(), w, h).map_err(|e| e.to_string())?;
        zip.start_file(&file, stored).map_err(e)?;
        zip.write_all(&bytes).map_err(io)?;
        let selected = if i == doc.active_index() {
            " selected=\"true\""
        } else {
            ""
        };
        stack.push_str(&format!(
            "    <layer name=\"{}\" src=\"{file}\" x=\"0\" y=\"0\" opacity=\"{:.4}\" \
             visibility=\"{}\" composite-op=\"{}\"{selected}/>\n",
            xml_escape(&layer.name),
            layer.opacity as f64 / 255.0,
            if layer.visible { "visible" } else { "hidden" },
            composite_op(layer.blend_mode),
        ));
    }
    let _ = n;
    stack.push_str("  </stack>\n</image>\n");
    zip.start_file("stack.xml", deflated).map_err(e)?;
    zip.write_all(stack.as_bytes()).map_err(io)?;

    let merged = flatten(doc);
    zip.start_file("mergedimage.png", stored).map_err(e)?;
    zip.write_all(&png(&merged.to_pixels(), w, h).map_err(|e| e.to_string())?)
        .map_err(io)?;
    // Thumbnail: at most 256 px on the long side.
    let scale = (256.0 / w.max(h) as f64).min(1.0);
    let (tw, th) = (
        ((w as f64 * scale).round() as u32).max(1),
        ((h as f64 * scale).round() as u32).max(1),
    );
    let thumb = pnn_core::transform::resample(
        &merged,
        tw,
        th,
        pnn_core::transform::Resampling::Bilinear,
        false,
    );
    zip.start_file("Thumbnails/thumbnail.png", stored)
        .map_err(e)?;
    zip.write_all(&png(&thumb.to_pixels(), tw, th).map_err(|e| e.to_string())?)
        .map_err(io)?;
    Ok(zip.finish().map_err(e)?.into_inner())
}

struct LayerEntry {
    name: String,
    src: String,
    x: i64,
    y: i64,
    opacity: f64,
    visible: bool,
    op: String,
    selected: bool,
}

/// Decode an OpenRaster file.
pub fn decode(bytes: &[u8], path: &Path) -> Result<Document, IoError> {
    let bad = |msg: String| IoError::Io {
        path: path.to_owned(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, msg),
    };
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| bad(e.to_string()))?;
    let mut read = |name: &str| -> Result<Vec<u8>, IoError> {
        let mut f = zip.by_name(name).map_err(|e| bad(format!("{name}: {e}")))?;
        let mut v = Vec::new();
        f.read_to_end(&mut v).map_err(|e| bad(e.to_string()))?;
        Ok(v)
    };
    let mime = read("mimetype").unwrap_or_default();
    if mime.trim_ascii() != MIME.as_bytes() {
        return Err(bad("not an OpenRaster file".into()));
    }
    let stack = String::from_utf8(read("stack.xml")?).map_err(|e| bad(e.to_string()))?;
    let mut reader = quick_xml::Reader::from_str(&stack);
    let (mut w, mut h, mut dpi) = (0u32, 0u32, pnn_core::DEFAULT_DPI);
    let mut layers: Vec<LayerEntry> = Vec::new();
    // Nested stacks are flattened into one list, keeping their order.
    loop {
        match reader.read_event().map_err(|e| bad(e.to_string()))? {
            Event::Eof => break,
            Event::Start(t) | Event::Empty(t) => {
                let attrs: Vec<(String, String)> = t
                    .attributes()
                    .flatten()
                    .map(|a| {
                        (
                            AsRef::<str>::as_ref(&a.key).to_owned(),
                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                                .map(|v| v.into_owned())
                                .unwrap_or_default(),
                        )
                    })
                    .collect();
                let get = |k: &str| attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
                match AsRef::<str>::as_ref(&t.name()) {
                    "image" => {
                        w = get("w").and_then(|v| v.parse().ok()).unwrap_or(0);
                        h = get("h").and_then(|v| v.parse().ok()).unwrap_or(0);
                        if let Some(x) = get("xres").and_then(|v| v.parse::<f64>().ok()) {
                            dpi = x;
                        }
                    }
                    "layer" => layers.push(LayerEntry {
                        name: get("name").unwrap_or_else(|| "Layer".into()),
                        src: get("src").unwrap_or_default(),
                        x: get("x").and_then(|v| v.parse().ok()).unwrap_or(0),
                        y: get("y").and_then(|v| v.parse().ok()).unwrap_or(0),
                        opacity: get("opacity").and_then(|v| v.parse().ok()).unwrap_or(1.0),
                        visible: get("visibility").as_deref() != Some("hidden"),
                        op: get("composite-op").unwrap_or_else(|| "svg:src-over".into()),
                        selected: get("selected").as_deref() == Some("true"),
                    }),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    if w == 0 || h == 0 {
        return Err(bad("stack.xml has no image size".into()));
    }
    // stack.xml lists top first; documents store bottom first.
    let mut out = Vec::new();
    let mut active = 0usize;
    for (i, e) in layers.iter().rev().enumerate() {
        let img = image::load_from_memory(&read(&e.src)?)
            .map_err(|source| IoError::Image {
                path: path.to_owned(),
                source,
            })?
            .to_rgba8();
        let mut s = Surface::new_transparent(w, h);
        for (px, py, p) in img.enumerate_pixels() {
            let (x, y) = (px as i64 + e.x, py as i64 + e.y);
            if x >= 0 && y >= 0 && (x as u32) < w && (y as u32) < h {
                s.set(
                    x as u32,
                    y as u32,
                    ColorBgra::from_rgba(p[0], p[1], p[2], p[3]),
                );
            }
        }
        let mut l = Layer::new(e.name.clone(), s);
        l.visible = e.visible;
        l.opacity = (e.opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        l.blend_mode = blend_from(&e.op);
        if e.selected {
            active = i;
        }
        out.push(l);
    }
    if out.is_empty() {
        out.push(Layer::new("Background", Surface::new_transparent(w, h)));
    }
    let mut doc = Document::from_layers(w, h, out);
    doc.dpi = dpi;
    doc.set_active_index(active.min(doc.layers().len() - 1));
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_layers_and_properties() {
        let mut a = Surface::new_transparent(30, 20);
        a.set(1, 1, ColorBgra::from_rgba(255, 0, 0, 255));
        let mut b = Surface::new_transparent(30, 20);
        b.set(5, 5, ColorBgra::from_rgba(0, 0, 255, 100));
        let mut top = Layer::new("Top & <special> \"name\"", b);
        top.opacity = 128;
        top.visible = false;
        top.blend_mode = BlendMode::Glow;
        let mut doc = Document::from_layers(30, 20, vec![Layer::new("Background", a), top]);
        doc.dpi = 300.0;
        doc.set_active_index(1);
        let bytes = encode(&doc).unwrap();
        // The mimetype comes first, uncompressed.
        assert_eq!(&bytes[30..38], b"mimetype");
        let back = decode(&bytes, Path::new("t.ora")).unwrap();
        assert_eq!((back.width(), back.height(), back.dpi), (30, 20, 300.0));
        assert_eq!(back.layers().len(), 2);
        let (l0, l1) = (back.layer(0), back.layer(1));
        assert_eq!(l0.name, "Background");
        assert_eq!(l1.name, "Top & <special> \"name\"");
        assert_eq!(
            (l1.opacity, l1.visible, l1.blend_mode),
            (128, false, BlendMode::Glow)
        );
        assert_eq!(l0.surface.to_pixels(), doc.layer(0).surface.to_pixels());
        assert_eq!(l1.surface.get(5, 5), ColorBgra::from_rgba(0, 0, 255, 100));
        assert_eq!(back.active_index(), 1);
    }

    #[test]
    fn every_blend_mode_round_trips_and_unknown_ops_are_normal() {
        for m in BlendMode::ALL {
            assert_eq!(blend_from(composite_op(m)), m);
        }
        assert_eq!(blend_from("svg:hue"), BlendMode::Normal);
    }

    /// Writes a sample file for checking against other programs:
    /// `PNN_ORA_OUT=/path/sample.ora cargo test -p pnn-io ora_sample -- --ignored`.
    #[test]
    #[ignore = "writes a file for manual interoperability checks"]
    fn ora_sample() {
        let Ok(out) = std::env::var("PNN_ORA_OUT") else {
            return;
        };
        let mut bg = Surface::new_transparent(64, 48);
        let mut fg = Surface::new_transparent(64, 48);
        for y in 0..48 {
            for x in 0..64 {
                bg.set(x, y, ColorBgra::from_rgba(200, 200, 60, 255));
                if (16..48).contains(&x) && (12..36).contains(&y) {
                    fg.set(x, y, ColorBgra::from_rgba(40, 90, 220, 255));
                }
            }
        }
        let mut top = Layer::new("Blue box", fg);
        top.blend_mode = BlendMode::Multiply;
        top.opacity = 200;
        let doc = Document::from_layers(64, 48, vec![Layer::new("Background", bg), top]);
        std::fs::write(out, encode(&doc).unwrap()).unwrap();
    }

    /// Reads a file written by another program:
    /// `PNN_ORA_IN=/path/file.ora cargo test -p pnn-io ora_read -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads a file for manual interoperability checks"]
    fn ora_read() {
        let Ok(p) = std::env::var("PNN_ORA_IN") else {
            return;
        };
        let d = decode(&std::fs::read(&p).unwrap(), Path::new(&p)).unwrap();
        println!("{}x{} dpi {}", d.width(), d.height(), d.dpi);
        for l in d.layers() {
            println!(
                "{:?} opacity {} visible {} {:?} px(30,20)={:?}",
                l.name,
                l.opacity,
                l.visible,
                l.blend_mode,
                l.surface.get(30, 20)
            );
        }
    }
}
