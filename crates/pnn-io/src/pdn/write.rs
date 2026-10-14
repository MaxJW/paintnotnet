//! Writing `.pdn` files: the records paint.net 4.21 writes for a document of bitmap layers,
//! in the order .NET's `BinaryFormatter` writes them (each object after the ones before it
//! in the order they were first referenced), followed by each pixel block in gzip chunks as
//! 3.36's `MemoryBlock.FinishSerialization` writes them, and the XML header with a 256 px
//! thumbnail as 3.36's `DocumentWorkspace.DoSave` makes it.
//!
//! The class, member and assembly names, member types and record kinds were copied from a
//! file saved by paint.net 4.21 (see `pdn::tests`). `savedWithVersion` is 4.21's, so
//! paint.net treats the file as it would one of its own from that version. Whether
//! paint.net 4.x/5.x opens these files has not been checked (no copy of paint.net was run).

use std::collections::HashMap;
use std::io::Write;

use pnn_core::{BlendMode, ColorBgra, Document};

const VERSION: [i32; 4] = [4, 21, 6589, 7045];
const DATA_LIB: &str =
    "PaintDotNet.Data, Version=4.21.6589.7045, Culture=neutral, PublicKeyToken=null";
const CORE_LIB: &str =
    "PaintDotNet.Core, Version=4.21.6589.7045, Culture=neutral, PublicKeyToken=null";
const KVP: &str = "System.Collections.Generic.KeyValuePair`2[[System.String, mscorlib, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089],[System.String, mscorlib, Version=4.0.0.0, Culture=neutral, PublicKeyToken=b77a5c561934e089]]";
/// 3.36 `MemoryBlock`'s chunk size (what 4.21 wrote too).
const CHUNK: usize = 256 * 1024;

/// A member's type in a class record (`MemberTypeInfo`).
#[derive(Clone, PartialEq, Eq, Hash)]
enum T {
    /// Primitive: 1 bool, 2 byte, 8 int32, 9 int64.
    P(u8),
    String,
    ObjectArray,
    /// A system class (mscorlib), by name.
    Sys(String),
    /// A class in library `lib` (0 = Data, 1 = Core).
    Cls(String, u8),
}

/// Member values.
enum V {
    Bool(bool),
    Byte(u8),
    I32(i32),
    I64(i64),
    /// A reference to an object written later (or earlier).
    Ref(i32),
    /// A string written inline.
    Str(String),
    /// A value type written inline: class name, library, members.
    Struct(&'static str, u8, Vec<(&'static str, T, V)>),
}

struct W {
    out: Vec<u8>,
    next_id: i32,
    /// Library ids written so far (Data, Core).
    libs: [Option<i32>; 2],
    /// Class metadata already written: (name, member types) → the first object's id.
    classes: HashMap<(String, Vec<T>), i32>,
}

impl W {
    fn id(&mut self) -> i32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    fn i32(&mut self, v: i32) {
        self.out.extend(v.to_le_bytes());
    }

    fn string(&mut self, s: &str) {
        let mut n = s.len();
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                self.out.push(b);
                break;
            }
            self.out.push(b | 0x80);
        }
        self.out.extend(s.as_bytes());
    }

    fn lib(&mut self, which: u8) -> i32 {
        if let Some(id) = self.libs[which as usize] {
            return id;
        }
        let id = self.id();
        self.out.push(12);
        self.i32(id);
        self.string(if which == 0 { DATA_LIB } else { CORE_LIB });
        self.libs[which as usize] = Some(id);
        id
    }

    /// A class record (or a reference to its metadata if written before) and its values.
    /// `lib`: `None` for system classes.
    fn class(&mut self, id: i32, name: &str, lib: Option<u8>, members: Vec<(&str, T, V)>) {
        // Libraries a member type names must come first.
        for (_, t, _) in &members {
            if let T::Cls(_, l) = t {
                self.lib(*l);
            }
        }
        let lib_id = lib.map(|l| self.lib(l));
        let types: Vec<T> = members.iter().map(|(_, t, _)| t.clone()).collect();
        let key = (name.to_owned(), types.clone());
        if let Some(&meta) = self.classes.get(&key) {
            self.out.push(1);
            self.i32(id);
            self.i32(meta);
        } else {
            self.classes.insert(key, id);
            self.out.push(if lib.is_some() { 5 } else { 4 });
            self.i32(id);
            self.string(name);
            self.i32(members.len() as i32);
            for (n, _, _) in &members {
                self.string(n);
            }
            for t in &types {
                self.out.push(match t {
                    T::P(_) => 0,
                    T::String => 1,
                    T::Sys(_) => 3,
                    T::Cls(..) => 4,
                    T::ObjectArray => 5,
                });
            }
            for t in &types {
                match t {
                    T::P(p) => self.out.push(*p),
                    T::Sys(s) => self.string(s),
                    T::Cls(s, l) => {
                        let l = self.lib(*l);
                        self.string(s);
                        self.i32(l);
                    }
                    T::String | T::ObjectArray => {}
                }
            }
            if let Some(l) = lib_id {
                self.i32(l);
            }
        }
        for (_, _, v) in members {
            self.value(v);
        }
    }

    fn value(&mut self, v: V) {
        match v {
            V::Bool(b) => self.out.push(b as u8),
            V::Byte(b) => self.out.push(b),
            V::I32(i) => self.i32(i),
            V::I64(i) => self.out.extend(i.to_le_bytes()),
            V::Ref(id) => {
                self.out.push(9);
                self.i32(id);
            }
            V::Str(s) => {
                let id = self.id();
                self.out.push(6);
                self.i32(id);
                self.string(&s);
            }
            V::Struct(name, lib, members) => {
                // Value types get negative ids.
                let id = -self.id();
                self.class(id, name, Some(lib), members);
            }
        }
    }

    /// `KeyValuePair<string, string>[]` (paint.net's metadata).
    fn pairs(&mut self, id: i32, pairs: &[(String, String)]) {
        self.out.push(7);
        self.i32(id);
        self.out.push(0); // single
        self.i32(1);
        self.i32(pairs.len() as i32);
        self.out.push(3);
        self.string(KVP);
        for (k, v) in pairs {
            let pid = -self.id();
            self.class(
                pid,
                KVP,
                None,
                vec![
                    ("key", T::String, V::Str(k.clone())),
                    ("value", T::String, V::Str(v.clone())),
                ],
            );
        }
    }
}

fn op_class(mode: BlendMode) -> String {
    let name = match mode {
        BlendMode::Normal => "Normal",
        BlendMode::Multiply => "Multiply",
        BlendMode::Additive => "Additive",
        BlendMode::ColorBurn => "ColorBurn",
        BlendMode::ColorDodge => "ColorDodge",
        BlendMode::Reflect => "Reflect",
        BlendMode::Glow => "Glow",
        BlendMode::Overlay => "Overlay",
        BlendMode::Difference => "Difference",
        BlendMode::Negation => "Negation",
        BlendMode::Lighten => "Lighten",
        BlendMode::Darken => "Darken",
        BlendMode::Screen => "Screen",
        BlendMode::Xor => "Xor",
    };
    format!("PaintDotNet.UserBlendOps+{name}BlendOp")
}

/// paint.net's EXIF metadata string for a tag.
fn exif(id: u16, kind: u16, bytes: &[u8]) -> String {
    format!(
        "<exif id=\"{id}\" len=\"{}\" type=\"{kind}\" value=\"{}\" />",
        bytes.len(),
        base64(bytes)
    )
}

fn base64(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                s.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

/// The serialized document (records only) and each layer's pixel bytes.
fn records(doc: &Document) -> Vec<u8> {
    let (w, h) = (doc.width() as i32, doc.height() as i32);
    let n = doc.layers().len();
    let mut wr = W {
        out: Vec::new(),
        next_id: 1,
        libs: [None, None],
        classes: HashMap::new(),
    };
    // Ids in the order .NET would give them: the document, then what it refers to, …
    let doc_id = wr.id();
    let list_id = wr.id();
    let version_id = wr.id();
    let meta_id = wr.id();
    let items_id = wr.id();
    let layer_ids: Vec<i32> = (0..n).map(|_| wr.id()).collect();
    // Per layer: bitmap properties, surface, layer properties.
    let parts: Vec<[i32; 3]> = (0..n).map(|_| [wr.id(), wr.id(), wr.id()]).collect();
    // Per layer: blend op, pixel block (and the shared empty metadata after the first).
    let mut tail_ids = Vec::new();
    let mut empty_id = 0;
    for i in 0..n {
        let op = wr.id();
        let block = wr.id();
        tail_ids.push((op, block));
        if i == 0 {
            empty_id = wr.id();
        }
    }
    // Header record: root, header id -1, version 1.0.
    wr.out.push(0);
    wr.i32(doc_id);
    wr.i32(-1);
    wr.i32(1);
    wr.i32(0);
    wr.class(
        doc_id,
        "PaintDotNet.Document",
        Some(0),
        vec![
            ("isDisposed", T::P(1), V::Bool(false)),
            (
                "layers",
                T::Cls("PaintDotNet.LayerList".into(), 0),
                V::Ref(list_id),
            ),
            ("width", T::P(8), V::I32(w)),
            ("height", T::P(8), V::I32(h)),
            (
                "savedWith",
                T::Sys("System.Version".into()),
                V::Ref(version_id),
            ),
            (
                "userMetadataItems",
                T::Sys(format!("{KVP}[]")),
                V::Ref(meta_id),
            ),
        ],
    );
    wr.class(
        list_id,
        "PaintDotNet.LayerList",
        Some(0),
        vec![
            (
                "parent",
                T::Cls("PaintDotNet.Document".into(), 0),
                V::Ref(doc_id),
            ),
            ("ArrayList+_items", T::ObjectArray, V::Ref(items_id)),
            ("ArrayList+_size", T::P(8), V::I32(n as i32)),
            ("ArrayList+_version", T::P(8), V::I32(n as i32)),
        ],
    );
    wr.class(
        version_id,
        "System.Version",
        None,
        ["_Major", "_Minor", "_Build", "_Revision"]
            .into_iter()
            .zip(VERSION)
            .map(|(m, v)| (m, T::P(8), V::I32(v)))
            .collect(),
    );
    // Resolution as EXIF (what 4.21 keeps): unit inches, X and Y as rationals.
    let dpi = if doc.dpi.is_finite() && doc.dpi > 0.0 {
        doc.dpi
    } else {
        96.0
    };
    let (num, den) = if dpi.fract() == 0.0 {
        (dpi as u32, 1u32)
    } else {
        ((dpi * 100.0).round() as u32, 100)
    };
    let rational: Vec<u8> = num
        .to_le_bytes()
        .into_iter()
        .chain(den.to_le_bytes())
        .collect();
    let metadata = vec![
        ("$exif.tag0".to_owned(), exif(296, 3, &2u16.to_le_bytes())),
        ("$exif.tag1".to_owned(), exif(282, 5, &rational)),
        ("$exif.tag2".to_owned(), exif(283, 5, &rational)),
    ];
    wr.pairs(meta_id, &metadata);
    // ArrayList's items (object[]).
    wr.out.push(16);
    wr.i32(items_id);
    wr.i32(n as i32);
    for &id in &layer_ids {
        wr.value(V::Ref(id));
    }
    for (i, &id) in layer_ids.iter().enumerate() {
        let [bp, surface, lp] = parts[i];
        wr.class(
            id,
            "PaintDotNet.BitmapLayer",
            Some(0),
            vec![
                (
                    "properties",
                    T::Cls("PaintDotNet.BitmapLayer+BitmapLayerProperties".into(), 0),
                    V::Ref(bp),
                ),
                (
                    "surface",
                    T::Cls("PaintDotNet.Surface".into(), 1),
                    V::Ref(surface),
                ),
                ("Layer+isDisposed", T::P(1), V::Bool(false)),
                ("Layer+width", T::P(8), V::I32(w)),
                ("Layer+height", T::P(8), V::I32(h)),
                (
                    "Layer+properties",
                    T::Cls("PaintDotNet.Layer+LayerProperties".into(), 0),
                    V::Ref(lp),
                ),
            ],
        );
    }
    for (i, layer) in doc.layers().iter().enumerate() {
        let [bp, surface, lp] = parts[i];
        let (op, block) = tail_ids[i];
        let op_name = op_class(layer.blend_mode);
        wr.class(
            bp,
            "PaintDotNet.BitmapLayer+BitmapLayerProperties",
            Some(0),
            vec![("blendOp", T::Cls(op_name, 0), V::Ref(op))],
        );
        wr.class(
            surface,
            "PaintDotNet.Surface",
            Some(1),
            vec![
                ("width", T::P(8), V::I32(w)),
                ("height", T::P(8), V::I32(h)),
                ("stride", T::P(8), V::I32(w * 4)),
                (
                    "scan0",
                    T::Cls("PaintDotNet.MemoryBlock".into(), 1),
                    V::Ref(block),
                ),
            ],
        );
        let mode = BlendMode::ALL
            .iter()
            .position(|&m| m == layer.blend_mode)
            .unwrap_or(0) as i32;
        wr.class(
            lp,
            "PaintDotNet.Layer+LayerProperties",
            Some(0),
            vec![
                ("name", T::String, V::Str(layer.name.clone())),
                (
                    "userMetadataItems",
                    T::Sys(format!("{KVP}[]")),
                    V::Ref(empty_id),
                ),
                ("visible", T::P(1), V::Bool(layer.visible)),
                ("isBackground", T::P(1), V::Bool(i == 0)),
                ("opacity", T::P(2), V::Byte(layer.opacity)),
                (
                    "blendMode",
                    T::Cls("PaintDotNet.LayerBlendMode".into(), 0),
                    V::Struct(
                        "PaintDotNet.LayerBlendMode",
                        0,
                        vec![("value__", T::P(8), V::I32(mode))],
                    ),
                ),
            ],
        );
    }
    for (i, layer) in doc.layers().iter().enumerate() {
        let (op, block) = tail_ids[i];
        wr.class(op, &op_class(layer.blend_mode), Some(0), Vec::new());
        wr.class(
            block,
            "PaintDotNet.MemoryBlock",
            Some(1),
            vec![
                ("length64", T::P(9), V::I64(w as i64 * h as i64 * 4)),
                ("hasParent", T::P(1), V::Bool(false)),
                ("deferred", T::P(1), V::Bool(true)),
            ],
        );
        if i == 0 {
            wr.pairs(empty_id, &[]);
        }
    }
    wr.out.push(11);
    wr.out
}

/// One pixel block as 3.36's `MemoryBlock.FinishSerialization` writes it: format 0 (gzip),
/// the chunk size, then each chunk's number, compressed size and bytes.
fn pixel_block(pixels: &[ColorBgra], out: &mut Vec<u8>) -> std::io::Result<()> {
    use rayon::prelude::*;
    let bytes: &[u8] = bytemuck::cast_slice(pixels);
    out.push(0);
    out.extend((CHUNK as u32).to_be_bytes());
    // Chunks compress independently: in parallel, written in order.
    let chunks: Vec<Vec<u8>> = bytes
        .par_chunks(CHUNK)
        .map(|chunk| {
            let mut z = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            z.write_all(chunk)?;
            z.finish()
        })
        .collect::<std::io::Result<_>>()?;
    for (i, data) in chunks.into_iter().enumerate() {
        out.extend((i as u32).to_be_bytes());
        out.extend((data.len() as u32).to_be_bytes());
        out.extend(data);
    }
    Ok(())
}

/// The embedded thumbnail: the flattened image at most 256 px on its long side, as PNG.
fn thumbnail(doc: &Document) -> Option<Vec<u8>> {
    let flat = pnn_core::flatten(doc);
    let (w, h) = (doc.width(), doc.height());
    let rgba: Vec<u8> = flat
        .to_pixels()
        .iter()
        .flat_map(|c| [c.r, c.g, c.b, c.a])
        .collect();
    let img = image::RgbaImage::from_raw(w, h, rgba)?;
    // 3.36's thumbnail size (integer division), then an area average.
    let (tw, th) = if w > 256 || h > 256 {
        if w > h {
            (256, (h * 256 / w).max(1))
        } else {
            ((w * 256 / h).max(1), 256)
        }
    } else {
        (w, h)
    };
    let thumb = image::imageops::thumbnail(&img, tw, th);
    let mut png = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

/// The whole `.pdn` file.
pub fn encode(doc: &Document) -> std::io::Result<Vec<u8>> {
    let custom = thumbnail(doc)
        .map(|png| format!("<custom><thumb png=\"{}\" /></custom>", base64(&png)))
        .unwrap_or_default();
    let header = format!(
        "<pdnImage width=\"{}\" height=\"{}\" layers=\"{}\" savedWithVersion=\"{}\">{custom}</pdnImage>",
        doc.width(),
        doc.height(),
        doc.layers().len(),
        VERSION.map(|v| v.to_string()).join(".")
    );
    if header.len() >= 1 << 24 {
        return Err(std::io::Error::other("header too large"));
    }
    let mut out = b"PDN3".to_vec();
    out.extend(&(header.len() as u32).to_le_bytes()[..3]);
    out.extend(header.as_bytes());
    out.extend([0, 1]);
    out.extend(records(doc));
    for layer in doc.layers() {
        pixel_block(&layer.surface.to_pixels(), &mut out)?;
    }
    Ok(out)
}
