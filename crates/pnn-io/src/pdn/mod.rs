//! paint.net `.pdn` files.
//!
//! The layout comes from paint.net 3.36 (`Data/Document.cs` `FromStream`, `Core/MemoryBlock.cs`
//! deferred serialization; MIT, see NOTICE): `PDN3`, a 24-bit little-endian byte count, that
//! many bytes of UTF-8 XML header, then either `00 01` and the `Document` written by .NET's
//! `BinaryFormatter` ([`nrbf`]) followed by each pixel block's data, or `1f 8b` and the whole
//! serialized document gzipped (no separate pixel data). Files from paint.net 2.0 and earlier
//! are that gzip stream alone. Each pixel block's data: a format byte (0 = gzip chunks, 1 =
//! raw chunks), a big-endian u32 chunk size, then every chunk as big-endian chunk number and
//! byte count followed by the bytes, in any order.
//!
//! Checked against files saved by paint.net 4.21 (pypdn's test files): the object graph
//! (`Document` → `LayerList` → `BitmapLayer` → `Surface` → `MemoryBlock`) and blend ops are
//! as 3.36 wrote them. Whether paint.net 5.x still writes this is not known (no 5.x file has
//! been tried).

pub mod nrbf;
mod write;

pub use write::encode;

use std::io::Read;

use pnn_core::{BlendMode, ColorBgra, Document, Layer, Surface};

use nrbf::{Graph, Object, Value, member};

#[derive(Debug, thiserror::Error)]
pub enum PdnError {
    #[error("not a paint.net image")]
    NotPdn,
    #[error("damaged paint.net image: {0}")]
    Nrbf(#[from] nrbf::NrbfError),
    #[error("damaged paint.net image: {0}")]
    Invalid(String),
    #[error("paint.net image: {0}")]
    Unsupported(&'static str),
}

fn invalid(msg: impl Into<String>) -> PdnError {
    PdnError::Invalid(msg.into())
}

/// The largest side accepted (paint.net's own limit is 65535).
const MAX_SIDE: i64 = 65_535;

/// Read a `.pdn` file.
pub fn decode(bytes: &[u8]) -> Result<Document, PdnError> {
    let (data, deferred) = match bytes.strip_prefix(b"PDN3") {
        Some(rest) => {
            let n = *rest.first().ok_or(PdnError::NotPdn)? as usize
                | (*rest.get(1).ok_or(PdnError::NotPdn)? as usize) << 8
                | (*rest.get(2).ok_or(PdnError::NotPdn)? as usize) << 16;
            let after = rest
                .get(3 + n..)
                .ok_or_else(|| invalid("header cut short"))?;
            if let Some(d) = after.strip_prefix(&[0, 1]) {
                (std::borrow::Cow::Borrowed(d), true)
            } else if after.starts_with(&[0x1f, 0x8b]) {
                (std::borrow::Cow::Owned(gunzip(after, None)?), false)
            } else {
                return Err(PdnError::NotPdn);
            }
        }
        // paint.net 2.0 and earlier: the gzipped document alone.
        None if bytes.starts_with(&[0x1f, 0x8b]) => {
            (std::borrow::Cow::Owned(gunzip(bytes, None)?), false)
        }
        None => return Err(PdnError::NotPdn),
    };
    let (graph, used) = nrbf::read(&data)?;
    let mut blocks = Blocks {
        graph: &graph,
        tail: if deferred { &data[used..] } else { &[] },
    };
    let (_, doc) = graph
        .class(graph.root)
        .ok_or_else(|| invalid("no document"))?;
    let int = |v: Option<&Value>| match v {
        Some(Value::Int(i)) => Some(*i),
        Some(Value::UInt(u)) => i64::try_from(*u).ok(),
        _ => None,
    };
    let (w, h) = (int(member(doc, "width")), int(member(doc, "height")));
    let (Some(w), Some(h)) = (w, h) else {
        return Err(invalid("no size"));
    };
    if !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h) {
        return Err(invalid(format!("size {w} × {h}")));
    }
    let (w, h) = (w as u32, h as u32);
    let list = member(doc, "layers")
        .and_then(|v| graph.get(v))
        .ok_or_else(|| invalid("no layers"))?;
    let Object::Class { members: list, .. } = list else {
        return Err(invalid("layer list"));
    };
    let count = int(member(list, "_size")).unwrap_or(0).max(0) as usize;
    let Some(Object::Array(items)) = member(list, "_items").and_then(|v| graph.get(v)) else {
        return Err(invalid("layer list items"));
    };
    if count == 0 || count > items.len() {
        return Err(invalid("layer count"));
    }
    // Every pixel block's data, in the order the blocks were serialized.
    let pixels = blocks.read_all()?;
    let mut layers = Vec::with_capacity(count);
    for item in &items[..count] {
        let Some(Object::Class { name, members }) = graph.get(item) else {
            return Err(invalid("layer"));
        };
        if !name.ends_with("BitmapLayer") {
            return Err(PdnError::Unsupported("a layer that isn't a bitmap layer"));
        }
        layers.push(layer(&graph, members, w, h, &pixels)?);
    }
    let mut document = Document::from_layers(w, h, layers);
    if let Some(dpi) = resolution(&graph) {
        document.dpi = dpi;
    }
    Ok(document)
}

fn gunzip(bytes: &[u8], expected: Option<usize>) -> Result<Vec<u8>, PdnError> {
    let mut out = Vec::with_capacity(expected.unwrap_or(bytes.len() * 4));
    flate2::read::GzDecoder::new(bytes)
        .read_to_end(&mut out)
        .map_err(|e| invalid(format!("compressed data: {e}")))?;
    Ok(out)
}

fn layer(
    graph: &Graph,
    members: &[(String, Value)],
    w: u32,
    h: u32,
    pixels: &std::collections::HashMap<i32, Vec<u8>>,
) -> Result<Layer, PdnError> {
    // `BitmapLayer` has its own `properties` (the blend op) and `Layer+properties` (name,
    // visibility, opacity).
    let props = |key: &str| -> Option<&[(String, Value)]> {
        let v = members.iter().find(|(n, _)| n == key).map(|(_, v)| v)?;
        match graph.get(v)? {
            Object::Class { members, .. } => Some(members),
            _ => None,
        }
    };
    let common = props("Layer+properties").ok_or_else(|| invalid("layer properties"))?;
    let own = props("properties");
    let name = match member(common, "name").and_then(|v| graph.get(v)) {
        Some(Object::String(s)) => s.clone(),
        _ => String::new(),
    };
    let mut l = Layer::new(name, Surface::new_transparent(w, h));
    l.visible = !matches!(member(common, "visible"), Some(Value::Bool(false)));
    l.opacity = match member(common, "opacity") {
        Some(Value::UInt(o)) => (*o).min(255) as u8,
        Some(Value::Int(o)) => (*o).clamp(0, 255) as u8,
        _ => 255,
    };
    l.blend_mode = own
        .and_then(|p| member(p, "blendOp"))
        .and_then(|v| match graph.get(v) {
            Some(Object::Class { name, .. }) => blend_mode(name),
            _ => None,
        })
        .unwrap_or_default();
    // The pixels: Surface { width, height, stride, scan0: MemoryBlock }.
    let Some(Object::Class {
        members: surface, ..
    }) = members
        .iter()
        .find(|(n, _)| n == "surface")
        .and_then(|(_, v)| graph.get(v))
    else {
        return Err(invalid("layer surface"));
    };
    let stride = match member(surface, "stride") {
        Some(Value::Int(s)) if *s >= w as i64 * 4 => *s as usize,
        _ => return Err(invalid("surface stride")),
    };
    let block = match member(surface, "scan0") {
        Some(Value::Ref(id) | Value::Inline(id)) => pixels.get(id),
        _ => None,
    }
    .ok_or_else(|| invalid("surface pixels"))?;
    if block.len() < stride * (h as usize - 1) + w as usize * 4 {
        return Err(invalid("surface pixels too short"));
    }
    let mut out = Vec::with_capacity((w * h) as usize);
    for y in 0..h as usize {
        let row = &block[y * stride..y * stride + w as usize * 4];
        // paint.net's ColorBgra is B, G, R, A in memory, as ours.
        out.extend(
            row.as_chunks::<4>()
                .0
                .iter()
                .map(|&[b, g, r, a]| ColorBgra::from_bgra(b, g, r, a)),
        );
    }
    l.surface.write_pixels(&out);
    l.surface.compact();
    Ok(l)
}

/// 3.36's `UserBlendOps` class for each mode (also what 4.x writes).
fn blend_mode(class: &str) -> Option<BlendMode> {
    let op = class.rsplit('+').next()?.strip_suffix("BlendOp")?;
    Some(match op {
        "Normal" => BlendMode::Normal,
        "Multiply" => BlendMode::Multiply,
        "Additive" => BlendMode::Additive,
        "ColorBurn" => BlendMode::ColorBurn,
        "ColorDodge" => BlendMode::ColorDodge,
        "Reflect" => BlendMode::Reflect,
        "Glow" => BlendMode::Glow,
        "Overlay" => BlendMode::Overlay,
        "Difference" => BlendMode::Difference,
        "Negation" => BlendMode::Negation,
        "Lighten" => BlendMode::Lighten,
        "Darken" => BlendMode::Darken,
        "Screen" => BlendMode::Screen,
        "Xor" => BlendMode::Xor,
        _ => return None,
    })
}

/// Pixel blocks: `MemoryBlock` objects with their data in the graph (`pointerData`) or after
/// it (`deferred`).
struct Blocks<'a> {
    graph: &'a Graph,
    tail: &'a [u8],
}

impl Blocks<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], PdnError> {
        if self.tail.len() < n {
            return Err(invalid("pixel data cut short"));
        }
        let (a, b) = self.tail.split_at(n);
        self.tail = b;
        Ok(a)
    }

    fn u32(&mut self) -> Result<u32, PdnError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("4 bytes"),
        ))
    }

    /// Every block's bytes by object id.
    fn read_all(&mut self) -> Result<std::collections::HashMap<i32, Vec<u8>>, PdnError> {
        let mut out = std::collections::HashMap::new();
        for &id in &self.graph.class_order {
            let Some((name, members)) = self.graph.class(id) else {
                continue;
            };
            if !name.ends_with("MemoryBlock") {
                continue;
            }
            let length = match member(members, "length64").or_else(|| member(members, "length")) {
                Some(Value::Int(n)) if *n >= 0 => *n as usize,
                _ => return Err(invalid("pixel block length")),
            };
            if matches!(member(members, "hasParent"), Some(Value::Bool(true))) {
                return Err(PdnError::Unsupported("a pixel block inside another"));
            }
            // Refuse sizes no image here could have before allocating.
            if length as u64 > (MAX_SIDE * MAX_SIDE * 4) as u64 {
                return Err(invalid("pixel block too large"));
            }
            let data = if matches!(member(members, "deferred"), Some(Value::Bool(true))) {
                self.deferred(length)?
            } else {
                match member(members, "pointerData").and_then(|v| self.graph.get(v)) {
                    Some(Object::Bytes(b)) if b.len() == length => b.clone(),
                    _ => return Err(invalid("pixel block data")),
                }
            };
            out.insert(id, data);
        }
        Ok(out)
    }

    /// 3.36 `MemoryBlock.FinishDeserialization`.
    fn deferred(&mut self, length: usize) -> Result<Vec<u8>, PdnError> {
        let format = self.take(1)?[0];
        if format > 1 {
            return Err(invalid("pixel block format"));
        }
        let chunk = self.u32()? as usize;
        if chunk == 0 {
            return Err(invalid("pixel chunk size"));
        }
        let count = length.div_ceil(chunk);
        let mut found = vec![false; count];
        let mut data = vec![0u8; length];
        for _ in 0..count {
            let number = self.u32()? as usize;
            let size = self.u32()? as usize;
            if number >= count || std::mem::replace(&mut found[number], true) {
                return Err(invalid("pixel chunk number"));
            }
            let offset = number * chunk;
            let this = chunk.min(length - offset);
            let bytes = self.take(size)?;
            let dst = &mut data[offset..offset + this];
            if format == 0 {
                let mut z = flate2::read::GzDecoder::new(bytes);
                z.read_exact(dst)
                    .map_err(|e| invalid(format!("pixel chunk: {e}")))?;
            } else {
                if bytes.len() < this {
                    return Err(invalid("pixel chunk cut short"));
                }
                dst.copy_from_slice(&bytes[..this]);
            }
        }
        Ok(data)
    }
}

/// DPI from the EXIF resolution paint.net keeps in its metadata strings
/// (`<exif id="282" … value="base64">`: XResolution, a rational; 296: ResolutionUnit, 2 =
/// inches, 3 = centimetres).
fn resolution(graph: &Graph) -> Option<f64> {
    let tag = |id: &str| -> Option<Vec<u8>> {
        graph.objects.values().find_map(|o| match o {
            Object::String(s) if s.starts_with("<exif") && s.contains(&format!("id=\"{id}\"")) => {
                let v = s.split("value=\"").nth(1)?.split('"').next()?;
                base64(v)
            }
            _ => None,
        })
    };
    let x = tag("282")?;
    let (num, den) = (
        u32::from_le_bytes(x.get(0..4)?.try_into().ok()?),
        u32::from_le_bytes(x.get(4..8)?.try_into().ok()?),
    );
    if den == 0 || num == 0 {
        return None;
    }
    let v = num as f64 / den as f64;
    let unit = tag("296").and_then(|u| u.first().copied()).unwrap_or(2);
    Some(if unit == 3 { v * 2.54 } else { v })
}

fn base64(s: &str) -> Option<Vec<u8>> {
    let val = |c: u8| -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    };
    let s = s.trim_end_matches('=').as_bytes();
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    for chunk in s.chunks(4) {
        let mut n = 0u32;
        for (i, &c) in chunk.iter().enumerate() {
            n |= val(c)? << (18 - 6 * i);
        }
        out.extend(&n.to_be_bytes()[1..chunk.len()]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes() {
        assert_eq!(base64("YAAAAAEAAAA=").unwrap(), [96, 0, 0, 0, 1, 0, 0, 0]);
        assert_eq!(base64("AgA=").unwrap(), [2, 0]);
        assert_eq!(base64("cGFpbnQ=").unwrap(), b"paint");
    }

    #[test]
    fn rejects_other_files() {
        assert!(matches!(decode(b"PNG\x00"), Err(PdnError::NotPdn)));
        assert!(decode(b"PDN3\x05\x00\x00<a/>").is_err());
    }

    /// paint.net 4.21 files from pypdn's tests (MIT, github.com/addisonElliott/pypdn
    /// `tests/data`), when `PNN_PDN_SAMPLES` names their folder: they load with the right
    /// layers, and each blend mode's layer over the background flattens exactly to pypdn's
    /// expected PNG for it (whether paint.net or pypdn made those PNGs isn't stated; they
    /// matched all 13 modes pixel for pixel, 2026-10-14).
    #[test]
    fn reads_paint_net_4_files() {
        let Ok(dir) = std::env::var("PNN_PDN_SAMPLES") else {
            eprintln!("PNN_PDN_SAMPLES not set: skipped");
            return;
        };
        let dir = std::path::Path::new(&dir);
        let load = |f: &str| decode(&std::fs::read(dir.join(f)).expect("reads")).expect(f);
        let d = load("Untitled3.pdn");
        assert_eq!((d.width(), d.height(), d.layers().len()), (800, 600, 2));
        assert_eq!(d.layer(0).name, "Background");
        assert_eq!(d.layer(1).name, "Layer 2");
        assert_eq!(d.layer(1).opacity, 161);
        assert_eq!(d.layer(1).blend_mode, BlendMode::Additive);
        assert_eq!(d.dpi, 96.0);
        for f in ["Untitled.pdn", "Untitled2.pdn"] {
            load(f);
        }
        let blend = load("FlattenBlendTest.pdn");
        assert_eq!(blend.layers().len(), 14);
        let mut report = Vec::new();
        for i in 1..blend.layers().len() {
            let mode = blend.layer(i).blend_mode;
            let name = &blend.layer(i).name;
            let png = ["Flatten", "flatten"]
                .iter()
                .map(|p| dir.join(format!("{p}{name}Test.png")))
                .find(|p| p.exists());
            let Some(png) = png else {
                report.push(format!("{name}: no PNG"));
                continue;
            };
            let mut d = blend.clone();
            for k in 1..d.layers().len() {
                d.modify_layer(k, None, |l| l.visible = k == i);
            }
            let flat = pnn_core::flatten(&d);
            let want = image::open(&png).expect("png").into_rgba8();
            let mut worst = 0u8;
            let mut off = 0usize;
            for (x, y, p) in want.enumerate_pixels() {
                let c = flat.get(x, y);
                let dmax = [
                    c.r.abs_diff(p[0]),
                    c.g.abs_diff(p[1]),
                    c.b.abs_diff(p[2]),
                    c.a.abs_diff(p[3]),
                ]
                .into_iter()
                .max()
                .unwrap_or(0);
                worst = worst.max(dmax);
                off += (dmax > 0) as usize;
            }
            report.push(format!(
                "{name} ({mode:?}): {off} pixels differ, worst {worst}"
            ));
            assert_eq!(
                off, 0,
                "{name} ({mode:?}) flattens differently (worst {worst})"
            );
        }
        eprintln!("{}", report.join("\n"));
        assert_eq!(report.len(), 13, "every mode has a PNG");
    }

    /// Our own files read back exactly: layers, names (non-ASCII too), visibility, opacity,
    /// every blend mode, DPI; across several chunks; and the header's thumbnail is a PNG.
    #[test]
    fn round_trip() {
        let (w, h) = (700u32, 500u32);
        let mut layers = Vec::new();
        for (i, &mode) in BlendMode::ALL.iter().enumerate() {
            let mut s = Surface::new_transparent(w, h);
            for y in (i as u32 * 7)..h {
                for x in 0..w {
                    if !(x + y + i as u32).is_multiple_of(3) {
                        s.set(
                            x,
                            y,
                            ColorBgra::from_bgra(x as u8, y as u8, (i * 17) as u8, (x ^ y) as u8),
                        );
                    }
                }
            }
            let mut l = Layer::new(format!("Layer {i} – ünïcode"), s);
            l.blend_mode = mode;
            l.opacity = 255 - i as u8 * 9;
            l.visible = i % 4 != 3;
            layers.push(l);
        }
        let mut doc = Document::from_layers(w, h, layers);
        doc.dpi = 300.0;
        let bytes = encode(&doc).expect("encodes");
        let back = decode(&bytes).expect("decodes");
        assert_eq!((back.width(), back.height()), (w, h));
        assert_eq!(back.dpi, 300.0);
        assert_eq!(back.layers().len(), doc.layers().len());
        for (a, b) in doc.layers().iter().zip(back.layers()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.blend_mode, b.blend_mode);
            assert_eq!(a.opacity, b.opacity);
            assert_eq!(a.visible, b.visible);
            assert!(
                a.surface.to_pixels() == b.surface.to_pixels(),
                "{} pixels",
                a.name
            );
        }
        // The header: PDN3, its length, XML with the size and an embedded PNG thumbnail.
        let n = bytes[4] as usize | (bytes[5] as usize) << 8 | (bytes[6] as usize) << 16;
        let header = std::str::from_utf8(&bytes[7..7 + n]).expect("utf-8");
        assert!(header.starts_with("<pdnImage width=\"700\" height=\"500\" layers=\"14\""));
        let png = header
            .split("png=\"")
            .nth(1)
            .and_then(|s| s.split('"').next())
            .expect("thumb");
        let thumb = image::load_from_memory(&base64(png).expect("base64")).expect("a PNG");
        assert_eq!((thumb.width(), thumb.height()), (256, 182));
    }
}
