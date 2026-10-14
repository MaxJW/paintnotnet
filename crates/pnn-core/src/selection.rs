//! Selections as geometry, like paint.net: an even-odd polygon set (paint.net's Copy
//! Selection exports `{"polygonList": [...]}`). Combine modes are polygon booleans; a coverage
//! mask is rasterised from the geometry (antialiased or pixelated) for pixel operations; the
//! marching ants follow the polygon edges exactly.

use i_overlay::core::fill_rule::FillRule as OverlayFill;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use tiny_skia::{FillRule, PathBuilder, Transform};

use crate::{Mask, Rect, TILE_SIZE, TileCoord};
pub use pnn_sdf::{Feather, Position as FeatherPosition, Profile as FeatherProfile, SignedField};

pub type Contour = Vec<[f64; 2]>;

/// How a new selection combines with the existing one (paint.net's selection modes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CombineMode {
    #[default]
    Replace,
    Union,
    Exclude,
    Intersect,
    Xor,
}

#[derive(Clone, Debug, Default)]
pub struct Selection {
    /// Even-odd polygon set in image pixel coordinates. Empty = nothing selected.
    contours: Vec<Contour>,
    /// Soft edge (paintnotnet's live feather). Kept through moves and combines.
    feather: Feather,
    /// Identity of the geometry (not the feather), so caches of its distance field survive
    /// feather changes.
    geometry_id: u64,
}

impl PartialEq for Selection {
    fn eq(&self, other: &Self) -> bool {
        self.contours == other.contours && self.feather == other.feather
    }
}

fn next_geometry_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// A selection's signed distance field over the region it can affect.
#[derive(Clone, Debug)]
pub struct FeatherField {
    pub region: Rect,
    pub field: SignedField,
    pub geometry_id: u64,
    pub antialias: bool,
    /// Per canvas tile overlapping the region: (min, max) signed distance, so reshaping can
    /// settle tiles that are wholly inside or outside the soft band without visiting pixels.
    tile_ranges: Vec<(TileCoord, f32, f32, bool)>,
}

impl Selection {
    pub fn none() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.contours.is_empty()
    }

    pub fn contours(&self) -> &[Contour] {
        &self.contours
    }

    pub fn from_contours(contours: Vec<Contour>) -> Self {
        let mut s = Self {
            contours: contours.into_iter().filter(|c| c.len() >= 3).collect(),
            feather: Feather::NONE,
            geometry_id: next_geometry_id(),
        };
        if s.area() < 1e-9 {
            s.contours.clear();
        }
        s
    }

    pub fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        let (l, r) = (x0.min(x1), x0.max(x1));
        let (t, b) = (y0.min(y1), y0.max(y1));
        Self::from_contours(vec![vec![[l, t], [r, t], [r, b], [l, b]]])
    }

    /// Ellipse inscribed in the box, flattened finely enough that segments stay ~1 px.
    pub fn ellipse(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (rx, ry) = ((x1 - x0).abs() / 2.0, (y1 - y0).abs() / 2.0);
        let n = ((std::f64::consts::TAU * rx.max(ry)) / 1.5)
            .ceil()
            .clamp(16.0, 4096.0) as usize;
        let pts = (0..n)
            .map(|i| {
                let t = i as f64 / n as f64 * std::f64::consts::TAU;
                [cx + rx * t.cos(), cy + ry * t.sin()]
            })
            .collect();
        Self::from_contours(vec![pts])
    }

    /// Lasso: the polygon through the points (closed automatically).
    pub fn polygon(points: Vec<[f64; 2]>) -> Self {
        Self::from_contours(vec![points])
    }

    /// Everything inside `width` × `height`.
    pub fn all(width: u32, height: u32) -> Self {
        Self::rect(0.0, 0.0, width as f64, height as f64)
    }

    /// Signed-area sum (even-odd regions counted by absolute value of each contour).
    fn area(&self) -> f64 {
        self.contours
            .iter()
            .map(|c| {
                let mut a = 0.0;
                for i in 0..c.len() {
                    let (p, q) = (c[i], c[(i + 1) % c.len()]);
                    a += p[0] * q[1] - q[0] * p[1];
                }
                (a / 2.0).abs()
            })
            .sum()
    }

    pub fn combine(&self, other: &Selection, mode: CombineMode) -> Selection {
        let rule = match mode {
            CombineMode::Replace => return other.clone(),
            CombineMode::Union => OverlayRule::Union,
            CombineMode::Exclude => OverlayRule::Difference,
            CombineMode::Intersect => OverlayRule::Intersect,
            CombineMode::Xor => OverlayRule::Xor,
        };
        if self.is_empty() {
            return match mode {
                CombineMode::Union | CombineMode::Xor => other.clone(),
                _ => Selection::none(),
            };
        }
        if other.is_empty() {
            return match mode {
                CombineMode::Intersect => Selection::none(),
                _ => self.clone(),
            };
        }
        let shapes = self
            .contours
            .overlay(&other.contours, rule, OverlayFill::EvenOdd);
        Selection::from_contours(shapes.into_iter().flatten().collect()).with_feather(other.feather)
    }

    /// paint.net's Invert Selection: xor with the whole canvas.
    pub fn invert(&self, width: u32, height: u32) -> Selection {
        let all = Selection::all(width, height);
        if self.is_empty() {
            return all;
        }
        all.combine(self, CombineMode::Exclude)
    }

    /// Whether point `p` is inside (even-odd), for hit-testing drags.
    pub fn contains(&self, p: [f64; 2]) -> bool {
        let mut inside = false;
        for c in &self.contours {
            let n = c.len();
            for i in 0..n {
                let (a, b) = (c[i], c[(i + 1) % n]);
                if (a[1] > p[1]) != (b[1] > p[1]) {
                    let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
                    if p[0] < x {
                        inside = !inside;
                    }
                }
            }
        }
        inside
    }

    /// Bounding box (min x, min y, max x, max y).
    pub fn bounds(&self) -> Option<[f64; 4]> {
        let mut it = self.contours.iter().flatten();
        let first = it.next()?;
        Some(it.fold([first[0], first[1], first[0], first[1]], |b, p| {
            [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ]
        }))
    }

    /// Bounds rounded out to whole pixels and clipped to the canvas.
    pub fn pixel_bounds(&self, width: u32, height: u32) -> Option<Rect> {
        let [x0, y0, x1, y1] = self.bounds()?;
        // Tolerate float residue from transforms (9.9999999 is 10, not 9).
        const EPS: f64 = 1e-6;
        let x0 = (x0 + EPS).floor().max(0.0) as u32;
        let y0 = (y0 + EPS).floor().max(0.0) as u32;
        let x1 = ((x1 - EPS).ceil().max(0.0) as u32).min(width);
        let y1 = ((y1 - EPS).ceil().max(0.0) as u32).min(height);
        (x1 > x0 && y1 > y0).then_some(Rect {
            x: x0,
            y: y0,
            width: x1 - x0,
            height: y1 - y0,
        })
    }

    /// Apply `p' = [a·x + b·y + c, d·x + e·y + f]`.
    pub fn transform(&self, m: [f64; 6]) -> Selection {
        let contours = self
            .contours
            .iter()
            .map(|c| {
                c.iter()
                    .map(|p| {
                        [
                            m[0] * p[0] + m[1] * p[1] + m[2],
                            m[3] * p[0] + m[4] * p[1] + m[5],
                        ]
                    })
                    .collect()
            })
            .collect();
        Selection {
            contours,
            feather: self.feather,
            geometry_id: next_geometry_id(),
        }
    }

    pub fn feather(&self) -> Feather {
        self.feather
    }

    /// Same geometry with a different soft edge.
    pub fn with_feather(mut self, feather: Feather) -> Selection {
        self.feather = feather;
        self
    }

    pub fn geometry_id(&self) -> u64 {
        self.geometry_id
    }

    pub fn translate(&self, dx: f64, dy: f64) -> Selection {
        self.transform([1.0, 0.0, dx, 0.0, 1.0, dy])
    }

    /// Coverage mask (0..=255) for a `width` × `height` canvas. `antialias` = paint.net's
    /// "Antialiased selection quality"; otherwise pixels are in or out by their centre.
    /// The feather (if any) softens the edge. An empty selection gives an empty mask
    /// (callers treat "no selection" as everything).
    pub fn rasterize(&self, width: u32, height: u32, antialias: bool) -> Mask {
        if self.feather.is_none() {
            return self.rasterize_hard(width, height, antialias);
        }
        match self.distance_field(width, height, antialias) {
            Some(field) => self.shape_field(&field, width, height),
            None => Mask::new(width, height, 0),
        }
    }

    /// The outline's coverage without feathering.
    pub fn rasterize_hard(&self, width: u32, height: u32, antialias: bool) -> Mask {
        let mut mask = Mask::new(width, height, 0);
        let Some(r) = self.pixel_bounds(width, height) else {
            return mask;
        };
        let data = self.raster_region(r, antialias);
        write_region(&mut mask, r, &data);
        mask.compact();
        mask
    }

    /// Hard coverage inside `r` (row-major, r.width × r.height).
    fn raster_region(&self, r: Rect, antialias: bool) -> Vec<u8> {
        let mut pb = PathBuilder::new();
        for c in &self.contours {
            let mut first = true;
            for p in c {
                let (x, y) = ((p[0] - r.x as f64) as f32, (p[1] - r.y as f64) as f32);
                if first {
                    pb.move_to(x, y);
                    first = false;
                } else {
                    pb.line_to(x, y);
                }
            }
            pb.close();
        }
        let size = (r.width * r.height) as usize;
        let Some(path) = pb.finish() else {
            return vec![0; size];
        };
        let Some(mut m) = tiny_skia::Mask::new(r.width, r.height) else {
            return vec![0; size];
        };
        m.fill_path(&path, FillRule::EvenOdd, antialias, Transform::identity());
        m.data().to_vec()
    }

    /// Signed distance field over the region the feathered selection can affect. It depends
    /// on the geometry, quality and how far the feather reaches outward, not otherwise on the
    /// radius, so callers can cache it while the radius changes.
    pub fn distance_field(&self, width: u32, height: u32, antialias: bool) -> Option<FeatherField> {
        let [x0, y0, x1, y1] = self.bounds()?;
        let grow = self.feather.outward_extent().ceil() as f64 + 2.0;
        let rx0 = (x0 - grow).floor().max(0.0) as u32;
        let ry0 = (y0 - grow).floor().max(0.0) as u32;
        let rx1 = ((x1 + grow).ceil().max(0.0) as u32).min(width);
        let ry1 = ((y1 + grow).ceil().max(0.0) as u32).min(height);
        if rx1 <= rx0 || ry1 <= ry0 {
            return None;
        }
        let region = Rect {
            x: rx0,
            y: ry0,
            width: rx1 - rx0,
            height: ry1 - ry0,
        };
        let coverage = self.raster_region(region, antialias);
        // The canvas edge is not an edge of the selection.
        let field = SignedField::from_coverage(
            &coverage,
            region.width as usize,
            region.height as usize,
            false,
        );
        let tile_ranges = tile_ranges(&field, region);
        Some(FeatherField {
            region,
            field,
            geometry_id: self.geometry_id,
            antialias,
            tile_ranges,
        })
    }

    /// Whether a cached field still serves this selection.
    pub fn field_matches(&self, f: &FeatherField, antialias: bool) -> bool {
        if f.geometry_id != self.geometry_id || f.antialias != antialias {
            return false;
        }
        let Some([x0, y0, x1, y1]) = self.bounds() else {
            return false;
        };
        let reach = self.feather.outward_extent() as f64 + 1.0;
        let r = f.region;
        let left = x0 - reach >= r.x as f64 || r.x == 0;
        let top = y0 - reach >= r.y as f64 || r.y == 0;
        let right = x1 + reach <= (r.x + r.width) as f64;
        let bottom = y1 + reach <= (r.y + r.height) as f64;
        left && top && right && bottom
    }

    /// Coverage mask from a (possibly cached) distance field.
    pub fn shape_field(&self, f: &FeatherField, width: u32, height: u32) -> Mask {
        use rayon::prelude::*;
        use std::sync::Arc;
        let mut mask = Mask::new(width, height, 0);
        let r = f.region;
        let feather = self.feather;
        let sd = &f.field.sd;
        let rw = r.width as usize;
        // One shared full tile for all masks, so unchanged interior tiles keep their identity
        // across reshapes (GPU uploads skip them).
        static FULL: std::sync::OnceLock<Arc<crate::Tile<u8>>> = std::sync::OnceLock::new();
        let full = FULL.get_or_init(|| Arc::new(crate::Tile::filled(255u8)));
        // Shape straight into tiles, in parallel; empty tiles stay absent and full ones share
        // a single tile.
        // Distances beyond which coverage is exactly full / zero for this feather.
        let empty_above = feather.outward_extent();
        let full_below = empty_above - feather.radius;
        let tiles: Vec<(TileCoord, Option<Arc<crate::Tile<u8>>>)> = f
            .tile_ranges
            .par_iter()
            .map(|&(c, min, max, covers_tile)| {
                if min >= empty_above {
                    return (c, None);
                }
                if covers_tile && max <= full_below {
                    return (c, Some(full.clone()));
                }
                let mut tile = crate::Tile::filled(0u8);
                let (mut any, mut all) = (false, true);
                for py in 0..TILE_SIZE {
                    let y = c.ty * TILE_SIZE + py;
                    let row = tile.row_mut(py);
                    if y < r.y || y >= r.y + r.height {
                        all = false;
                        continue;
                    }
                    let srow = &sd[(y - r.y) as usize * rw..(y - r.y + 1) as usize * rw];
                    for (px, v) in row.iter_mut().enumerate() {
                        let x = c.tx * TILE_SIZE + px as u32;
                        if x < r.x || x >= r.x + r.width {
                            all = false;
                            continue;
                        }
                        let k = (feather.coverage(srow[(x - r.x) as usize]) * 255.0 + 0.5) as u8;
                        *v = k;
                        any |= k != 0;
                        all &= k == 255;
                    }
                }
                let t = if all {
                    Some(full.clone())
                } else if any {
                    Some(Arc::new(tile))
                } else {
                    None
                };
                (c, t)
            })
            .collect();
        for (c, t) in tiles {
            mask.set_tile(c, t);
        }
        mask
    }

    /// Outlines for the marching ants: the polygon edges, moved to the feather's 50%
    /// contour when feathered.
    pub fn ant_contours(&self) -> Vec<Contour> {
        let offset = self.feather.half_contour_offset() as f64;
        if self.feather.is_none() || offset.abs() < 0.25 {
            return self.contours.clone();
        }
        use i_overlay::mesh::float::outline::offset::OutlineOffset;
        use i_overlay::mesh::float::style::{LineJoin, OutlineStyle};
        // Normalise orientation (outer counter-clockwise, holes clockwise) before offsetting.
        let empty: Vec<Contour> = Vec::new();
        let shapes = self
            .contours
            .overlay(&empty, OverlayRule::Subject, OverlayFill::EvenOdd);
        let style = OutlineStyle::new(offset).line_join(LineJoin::Round(0.25));
        shapes.outline(&style).into_iter().flatten().collect()
    }

    /// Trace a pixel stencil (Magic Wand result) into pixel-edge polygons.
    pub fn from_stencil(stencil: &[bool], width: u32, height: u32) -> Selection {
        Selection::from_contours(trace_stencil(stencil, width as usize, height as usize))
    }

    /// paint.net's Copy Selection format: `{"polygonList": ["x,y,x,y,…", …]}` with each
    /// polygon closed by repeating its first point.
    pub fn to_json(&self) -> String {
        let fmt = |v: f64| {
            let s = format!("{v:.3}");
            s.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        let polys: Vec<String> = self
            .contours
            .iter()
            .map(|c| {
                let pts: Vec<String> = c
                    .iter()
                    .chain(c.first())
                    .map(|p| format!("{},{}", fmt(p[0]), fmt(p[1])))
                    .collect();
                format!("\"{}\"", pts.join(","))
            })
            .collect();
        format!("{{\"polygonList\": [{}]}}", polys.join(", "))
    }

    /// Parse paint.net's selection JSON (Paste Selection). Returns `None` if malformed.
    pub fn from_json(json: &str) -> Option<Selection> {
        let start = json.find('[')?;
        let end = json.rfind(']')?;
        let mut contours = Vec::new();
        for part in json[start + 1..end].split('"').skip(1).step_by(2) {
            let nums: Vec<f64> = part
                .split(',')
                .map(|n| n.trim().parse().ok())
                .collect::<Option<_>>()?;
            if !nums.len().is_multiple_of(2) {
                return None;
            }
            let mut pts: Contour = nums.chunks(2).map(|c| [c[0], c[1]]).collect();
            if pts.len() > 1 && pts.first() == pts.last() {
                pts.pop();
            }
            contours.push(pts);
        }
        Some(Selection::from_contours(contours))
    }
}

/// (tile, min sd, max sd, whether the region covers the whole tile) for every canvas tile the
/// field's region overlaps.
fn tile_ranges(field: &SignedField, r: Rect) -> Vec<(TileCoord, f32, f32, bool)> {
    use rayon::prelude::*;
    let coords: Vec<TileCoord> = (r.y / TILE_SIZE..=(r.y + r.height - 1) / TILE_SIZE)
        .flat_map(|ty| {
            (r.x / TILE_SIZE..=(r.x + r.width - 1) / TILE_SIZE).map(move |tx| TileCoord { tx, ty })
        })
        .collect();
    let rw = r.width as usize;
    coords
        .par_iter()
        .map(|&c| {
            let (tx0, ty0) = (c.tx * TILE_SIZE, c.ty * TILE_SIZE);
            let (x0, x1) = (tx0.max(r.x), (tx0 + TILE_SIZE).min(r.x + r.width));
            let (y0, y1) = (ty0.max(r.y), (ty0 + TILE_SIZE).min(r.y + r.height));
            let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
            for y in y0..y1 {
                let row = &field.sd[(y - r.y) as usize * rw..];
                for &v in &row[(x0 - r.x) as usize..(x1 - r.x) as usize] {
                    min = min.min(v);
                    max = max.max(v);
                }
            }
            let covers = x0 == tx0 && y0 == ty0 && x1 == tx0 + TILE_SIZE && y1 == ty0 + TILE_SIZE;
            (c, min, max, covers)
        })
        .collect()
}

/// Copy a row-major `r`-sized buffer into the tiled mask.
fn write_region(mask: &mut Mask, r: Rect, data: &[u8]) {
    for ty in r.y / TILE_SIZE..=(r.y + r.height - 1) / TILE_SIZE {
        for tx in r.x / TILE_SIZE..=(r.x + r.width - 1) / TILE_SIZE {
            let tile = mask.tile_mut(TileCoord { tx, ty });
            for py in 0..TILE_SIZE {
                let y = ty * TILE_SIZE + py;
                if y < r.y || y >= r.y + r.height {
                    continue;
                }
                let row = tile.row_mut(py);
                for (px, v) in row.iter_mut().enumerate() {
                    let x = tx * TILE_SIZE + px as u32;
                    if x >= r.x && x < r.x + r.width {
                        *v = data[((y - r.y) * r.width + (x - r.x)) as usize];
                    }
                }
            }
        }
    }
}

/// Pixel-edge boundary tracing: each inside pixel contributes the edges it shares with an
/// outside neighbour, oriented clockwise (y down) so they chain into closed loops. Where two
/// loops touch diagonally the walk turns right, keeping loops simple. Collinear runs are
/// merged.
fn trace_stencil(stencil: &[bool], w: usize, h: usize) -> Vec<Contour> {
    let inside = |x: i64, y: i64| {
        x >= 0
            && y >= 0
            && (x as usize) < w
            && (y as usize) < h
            && stencil[y as usize * w + x as usize]
    };
    let vid = |x: usize, y: usize| y * (w + 1) + x;
    // Up to two outgoing edges per vertex (diagonal touches).
    let mut out: Vec<[u32; 2]> = vec![[u32::MAX; 2]; (w + 1) * (h + 1)];
    let mut used: Vec<[bool; 2]> = vec![[false; 2]; (w + 1) * (h + 1)];
    let mut add = |from: usize, to: usize| {
        let slot = &mut out[from];
        if slot[0] == u32::MAX {
            slot[0] = to as u32;
        } else {
            slot[1] = to as u32;
        }
    };
    for y in 0..h {
        for x in 0..w {
            if !stencil[y * w + x] {
                continue;
            }
            let (xi, yi) = (x as i64, y as i64);
            if !inside(xi, yi - 1) {
                add(vid(x, y), vid(x + 1, y));
            }
            if !inside(xi + 1, yi) {
                add(vid(x + 1, y), vid(x + 1, y + 1));
            }
            if !inside(xi, yi + 1) {
                add(vid(x + 1, y + 1), vid(x, y + 1));
            }
            if !inside(xi - 1, yi) {
                add(vid(x, y + 1), vid(x, y));
            }
        }
    }
    let pos = |v: usize| ((v % (w + 1)) as i64, (v / (w + 1)) as i64);
    let mut contours = Vec::new();
    for start in 0..out.len() {
        for k in 0..2 {
            if out[start][k] == u32::MAX || used[start][k] {
                continue;
            }
            let mut loop_pts: Vec<[f64; 2]> = Vec::new();
            let (mut v, mut slot) = (start, k);
            let mut prev_dir = (0i64, 0i64);
            loop {
                used[v][slot] = true;
                let next = out[v][slot] as usize;
                let (a, b) = (pos(v), pos(next));
                let dir = (b.0 - a.0, b.1 - a.1);
                if dir != prev_dir {
                    loop_pts.push([a.0 as f64, a.1 as f64]);
                }
                prev_dir = dir;
                v = next;
                if v == start
                    && out[v]
                        .iter()
                        .enumerate()
                        .all(|(i, &e)| e == u32::MAX || used[v][i])
                {
                    break;
                }
                // Pick the unused outgoing edge, preferring a right turn at diagonal touches.
                let choices: Vec<usize> = (0..2)
                    .filter(|&i| out[v][i] != u32::MAX && !used[v][i])
                    .collect();
                slot = match choices.as_slice() {
                    [] => break,
                    [only] => *only,
                    _ => {
                        let right = (-dir.1, dir.0);
                        *choices
                            .iter()
                            .find(|&&i| {
                                let n = pos(out[v][i] as usize);
                                (n.0 - pos(v).0, n.1 - pos(v).1) == right
                            })
                            .unwrap_or(&choices[0])
                    }
                };
            }
            if loop_pts.len() >= 3 {
                contours.push(loop_pts);
            }
        }
    }
    contours
}

/// paint.net's flood-fill colour test (3.36 `FloodToolBase.CheckColor`): squared channel
/// differences, RGB weighted by the seed's alpha so fully transparent pixels match each
/// other ("premultiplied" tolerance alpha mode). `tolerance` is 0..=100 %.
pub fn colors_match(
    seed: crate::ColorBgra,
    c: crate::ColorBgra,
    tolerance_pct: u8,
    premultiplied: bool,
) -> bool {
    let t = tolerance_pct as f64 / 100.0;
    let tol = (t * t * 256.0) as i64;
    let w = if premultiplied { seed.a as i64 } else { 256 };
    let ch = |a: u8, b: u8| {
        let d = a as i64 - b as i64;
        (1 + d * d) * w / 256
    };
    let da = seed.a as i64 - c.a as i64;
    let sum = ch(seed.r, c.r) + ch(seed.g, c.g) + ch(seed.b, c.b) + da * da;
    sum <= tol * tol * 4
}

/// How a flood fill decides which colours match the seed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tolerance {
    /// 0..=100 %.
    pub pct: u8,
    /// Weight colour differences by the seed's alpha (transparent pixels match each other).
    pub premultiplied: bool,
    /// Compare in OKLab (as the eye sees differences) instead of paint.net's RGB.
    pub perceptual: bool,
}

/// paintnotnet's perceptual colour test: OKLab distance (colour weighted by the seed's alpha
/// in premultiplied mode, plus the alpha difference), within 0.96·t². Calibrated so 50%
/// reaches about as far through greys as paint.net's 50% (128 matches 202, not 210); 100%
/// matches everything. Unlike RGB it treats a dark blue step as large and a light yellow
/// step of the same RGB size as small, as they look.
pub fn colors_match_perceptual(
    seed_lab: [f32; 3],
    seed: crate::ColorBgra,
    c: crate::ColorBgra,
    tolerance_pct: u8,
    premultiplied: bool,
) -> bool {
    if tolerance_pct >= 100 {
        return true;
    }
    let t = tolerance_pct as f32 / 100.0;
    let limit = 0.96 * t * t;
    let lab = crate::color::oklab(c);
    let w = if premultiplied {
        seed.a as f32 / 255.0
    } else {
        1.0
    };
    let d2 = (0..3).map(|k| (lab[k] - seed_lab[k]).powi(2)).sum::<f32>() * w * w;
    let da = (seed.a as f32 - c.a as f32) / 255.0;
    d2 + da * da <= limit * limit
}

/// Flood fill on `pixels` (row-major) from `seed`: contiguous (4-connected scanline fill) or
/// global. Returns the matching stencil.
pub fn flood_stencil(
    pixels: &[crate::ColorBgra],
    width: u32,
    height: u32,
    seed: (u32, u32),
    tolerance: Tolerance,
    contiguous: bool,
) -> Vec<bool> {
    let (w, h) = (width as usize, height as usize);
    let target = pixels[seed.1 as usize * w + seed.0 as usize];
    let target_lab = crate::color::oklab(target);
    let Tolerance {
        pct,
        premultiplied,
        perceptual,
    } = tolerance;
    let matches = |i: usize| {
        if perceptual {
            colors_match_perceptual(target_lab, target, pixels[i], pct, premultiplied)
        } else {
            colors_match(target, pixels[i], pct, premultiplied)
        }
    };
    let mut stencil = vec![false; w * h];
    if !contiguous {
        use rayon::prelude::*;
        stencil
            .par_iter_mut()
            .enumerate()
            .for_each(|(i, s)| *s = matches(i));
        return stencil;
    }
    let mut stack = vec![(seed.0 as usize, seed.1 as usize)];
    while let Some((x, y)) = stack.pop() {
        if stencil[y * w + x] || !matches(y * w + x) {
            continue;
        }
        // Extend the span left and right.
        let mut l = x;
        while l > 0 && !stencil[y * w + l - 1] && matches(y * w + l - 1) {
            l -= 1;
        }
        let mut r = x;
        while r + 1 < w && !stencil[y * w + r + 1] && matches(y * w + r + 1) {
            r += 1;
        }
        for i in l..=r {
            stencil[y * w + i] = true;
        }
        for ny in [y.wrapping_sub(1), y + 1] {
            if ny >= h {
                continue;
            }
            let mut i = l;
            while i <= r {
                if !stencil[ny * w + i] && matches(ny * w + i) {
                    stack.push((i, ny));
                    // Skip the rest of this run; it's reached from the pushed seed.
                    while i <= r && matches(ny * w + i) {
                        i += 1;
                    }
                } else {
                    i += 1;
                }
            }
        }
    }
    stencil
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ColorBgra;

    fn coverage(s: &Selection, w: u32, h: u32, aa: bool) -> Vec<u8> {
        s.rasterize(w, h, aa).to_pixels()
    }

    #[test]
    fn rect_rasterizes_exactly() {
        let s = Selection::rect(2.0, 3.0, 6.0, 5.0);
        let m = coverage(&s, 10, 10, true);
        let count = m.iter().filter(|&&v| v == 255).count();
        assert_eq!(count, 4 * 2);
        assert_eq!(m[3 * 10 + 2], 255);
        assert_eq!(m[3 * 10 + 6], 0);
        assert_eq!(
            s.pixel_bounds(10, 10),
            Some(Rect {
                x: 2,
                y: 3,
                width: 4,
                height: 2
            })
        );
    }

    #[test]
    fn combine_modes() {
        let a = Selection::rect(0.0, 0.0, 10.0, 10.0);
        let b = Selection::rect(5.0, 0.0, 15.0, 10.0);
        let area = |s: &Selection| {
            coverage(s, 20, 20, false)
                .iter()
                .filter(|&&v| v > 127)
                .count()
        };
        assert_eq!(area(&a.combine(&b, CombineMode::Union)), 150);
        assert_eq!(area(&a.combine(&b, CombineMode::Intersect)), 50);
        assert_eq!(area(&a.combine(&b, CombineMode::Exclude)), 50);
        assert_eq!(area(&a.combine(&b, CombineMode::Xor)), 100);
        assert_eq!(area(&a.combine(&b, CombineMode::Replace)), 100);
        assert!(
            a.combine(
                &Selection::rect(50.0, 50.0, 60.0, 60.0),
                CombineMode::Intersect
            )
            .is_empty()
        );
    }

    #[test]
    fn feather_zero_is_pixel_identical() {
        let s = Selection::ellipse(3.0, 4.0, 41.0, 30.0);
        let f = s.clone().with_feather(Feather {
            radius: 0.0,
            ..Feather::NONE
        });
        assert_eq!(
            s.rasterize(50, 40, true).to_pixels(),
            f.rasterize(50, 40, true).to_pixels()
        );
    }

    #[test]
    fn tile_skipping_matches_per_pixel_shaping() {
        // Large enough that whole tiles fall inside, outside and across the soft band.
        let (w, h) = (1100, 800);
        let base = Selection::ellipse(40.0, 30.0, 1000.0, 720.0);
        let field = base.distance_field(w, h, true).unwrap();
        for profile in [
            FeatherProfile::Linear,
            FeatherProfile::Smooth,
            FeatherProfile::Gaussian,
        ] {
            for position in [
                FeatherPosition::Inside,
                FeatherPosition::Centered,
                FeatherPosition::Outside,
            ] {
                for radius in [1.0, 7.5, 64.0] {
                    let feather = Feather {
                        radius,
                        profile,
                        position,
                    };
                    let s = base.clone().with_feather(feather);
                    let mask = s.shape_field(&field, w, h);
                    let r = field.region;
                    for y in 0..h {
                        for x in 0..w {
                            let inside =
                                x >= r.x && y >= r.y && x < r.x + r.width && y < r.y + r.height;
                            let expect = if inside {
                                let sd = field.field.sd[((y - r.y) * r.width + (x - r.x)) as usize];
                                (feather.coverage(sd) * 255.0 + 0.5) as u8
                            } else {
                                0
                            };
                            assert_eq!(mask.get(x, y), expect, "{feather:?} at ({x},{y})");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn feathered_rect_ramps_inward() {
        let feather = Feather {
            radius: 8.0,
            profile: FeatherProfile::Linear,
            position: FeatherPosition::Inside,
        };
        let s = Selection::rect(10.0, 10.0, 60.0, 60.0).with_feather(feather);
        let m = s.rasterize(70, 70, true);
        assert_eq!(m.get(35, 9), 0);
        assert_eq!(m.get(35, 10), ((0.5 / 8.0) * 255.0 + 0.5) as u8);
        assert_eq!(m.get(35, 13), ((3.5 / 8.0) * 255.0 + 0.5) as u8);
        assert_eq!(m.get(35, 35), 255);
        // A feather change keeps the geometry, so a cached field stays valid.
        let f = s.distance_field(70, 70, true).unwrap();
        let wider = s.clone().with_feather(Feather {
            radius: 16.0,
            ..feather
        });
        assert!(wider.field_matches(&f, true));
        assert_eq!(
            wider.shape_field(&f, 70, 70).to_pixels(),
            wider.rasterize(70, 70, true).to_pixels()
        );
    }

    #[test]
    fn ants_move_to_the_half_contour() {
        let feather = Feather {
            radius: 8.0,
            profile: FeatherProfile::Smooth,
            position: FeatherPosition::Inside,
        };
        let s = Selection::rect(0.0, 0.0, 40.0, 40.0).with_feather(feather);
        let ants = Selection::from_contours(s.ant_contours());
        let [x0, y0, x1, y1] = ants.bounds().unwrap();
        assert!(
            (x0 - 4.0).abs() < 0.1
                && (y0 - 4.0).abs() < 0.1
                && (x1 - 36.0).abs() < 0.1
                && (y1 - 36.0).abs() < 0.1,
            "{x0} {y0} {x1} {y1}"
        );
        let out = s.clone().with_feather(Feather {
            position: FeatherPosition::Outside,
            ..feather
        });
        let b = Selection::from_contours(out.ant_contours())
            .bounds()
            .unwrap();
        assert!((b[0] + 4.0).abs() < 0.2, "outset by r/2: {b:?}");
    }

    #[test]
    fn contains_respects_holes() {
        let ring = Selection::rect(0.0, 0.0, 10.0, 10.0)
            .combine(&Selection::rect(3.0, 3.0, 7.0, 7.0), CombineMode::Exclude);
        assert!(ring.contains([1.0, 1.0]));
        assert!(!ring.contains([5.0, 5.0]));
        assert!(!ring.contains([11.0, 5.0]));
    }

    #[test]
    fn invert_and_select_all() {
        let s = Selection::rect(0.0, 0.0, 5.0, 10.0);
        let inv = s.invert(10, 10);
        let m = coverage(&inv, 10, 10, false);
        assert_eq!(m.iter().filter(|&&v| v > 127).count(), 50);
        assert_eq!(m[0], 0);
        assert_eq!(m[9], 255);
        assert_eq!(Selection::none().invert(10, 10), Selection::all(10, 10));
    }

    #[test]
    fn antialiased_ellipse_has_soft_edge() {
        let s = Selection::ellipse(0.0, 0.0, 40.0, 40.0);
        let m = coverage(&s, 40, 40, true);
        assert_eq!(m[20 * 40 + 20], 255);
        assert_eq!(m[0], 0);
        assert!(
            m.iter().any(|&v| v > 0 && v < 255),
            "partial coverage at the edge"
        );
        let p = coverage(&s, 40, 40, false);
        assert!(p.iter().all(|&v| v == 0 || v == 255), "pixelated is binary");
    }

    #[test]
    fn stencil_tracing_matches_pixels() {
        // A ring: 5x5 block with a hole, plus a diagonal neighbour pixel.
        let (w, h) = (8, 8);
        let mut st = vec![false; w * h];
        for y in 1..6 {
            for x in 1..6 {
                st[y * w + x] = !(x == 3 && y == 3);
            }
        }
        st[6 * w + 6] = true; // touches the block diagonally at (6,6)
        let s = Selection::from_stencil(&st, w as u32, h as u32);
        let m = coverage(&s, w as u32, h as u32, false);
        for i in 0..w * h {
            assert_eq!(m[i] > 127, st[i], "pixel {i}");
        }
    }

    #[test]
    fn json_round_trip_like_paint_net() {
        let s = Selection::rect(3.0, 4.0, 9.0, 19.0);
        let json = s.to_json();
        assert_eq!(json, r#"{"polygonList": ["3,4,9,4,9,19,3,19,3,4"]}"#);
        assert_eq!(Selection::from_json(&json), Some(s));
        assert!(Selection::from_json("nonsense").is_none());
    }

    #[test]
    fn paint_net_tolerance() {
        let red = ColorBgra::from_rgba(255, 0, 0, 255);
        let near = ColorBgra::from_rgba(240, 10, 0, 255);
        assert!(colors_match(red, red, 0, true));
        assert!(!colors_match(red, near, 0, true));
        assert!(colors_match(red, near, 50, true));
        // Premultiplied: transparent pixels of any colour match each other.
        let t1 = ColorBgra::from_rgba(255, 0, 0, 0);
        let t2 = ColorBgra::from_rgba(0, 255, 0, 0);
        assert!(colors_match(t1, t2, 0, true));
        assert!(!colors_match(t1, t2, 0, false));
    }

    /// OKLab tolerance: 50% reaches through greys about as far as paint.net's RGB 50%, and
    /// tells apart what RGB can't: a dark blue step (clearly visible) and a light yellow step
    /// (barely visible) of the same RGB size.
    #[test]
    fn perceptual_tolerance() {
        let m = |a: ColorBgra, b: ColorBgra, pct: u8| {
            colors_match_perceptual(crate::color::oklab(a), a, b, pct, true)
        };
        let grey = |v: u8| ColorBgra::from_rgba(v, v, v, 255);
        assert!(m(grey(128), grey(200), 50) && !m(grey(128), grey(212), 50));
        assert!(colors_match(grey(128), grey(200), 50, true));
        let (blue1, blue2) = (
            ColorBgra::from_rgba(0, 0, 40, 255),
            ColorBgra::from_rgba(0, 0, 90, 255),
        );
        let (yellow1, yellow2) = (
            ColorBgra::from_rgba(235, 235, 40, 255),
            ColorBgra::from_rgba(235, 235, 90, 255),
        );
        // RGB sees the same step in both pairs.
        assert_eq!(
            colors_match(blue1, blue2, 30, true),
            colors_match(yellow1, yellow2, 30, true)
        );
        // OKLab: the yellows match, the blues don't.
        assert!(m(yellow1, yellow2, 30), "light yellows look alike");
        assert!(!m(blue1, blue2, 30), "dark blues look different");
        assert!(m(blue1, ColorBgra::WHITE, 100), "100% matches everything");
    }

    #[test]
    fn contiguous_vs_global_flood() {
        // Two white areas separated by a black column.
        let (w, h) = (5u32, 3u32);
        let mut px = vec![ColorBgra::WHITE; 15];
        for y in 0..3 {
            px[y * 5 + 2] = ColorBgra::BLACK;
        }
        let c = flood_stencil(
            &px,
            w,
            h,
            (0, 0),
            Tolerance {
                pct: 0,
                premultiplied: true,
                perceptual: false,
            },
            true,
        );
        assert_eq!(c.iter().filter(|&&b| b).count(), 6);
        let g = flood_stencil(
            &px,
            w,
            h,
            (0, 0),
            Tolerance {
                pct: 0,
                premultiplied: true,
                perceptual: false,
            },
            false,
        );
        assert_eq!(g.iter().filter(|&&b| b).count(), 12);
    }
}

#[cfg(test)]
mod perf {
    use super::*;

    /// Feather slider cost on a 4K canvas: reshaping a cached distance field.
    /// Run with `cargo test --release -p pnn-core feather_slider_4k -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn feather_slider_4k() {
        let (w, h) = (3840, 2160);
        let s = Selection::ellipse(200.0, 150.0, 3600.0, 2000.0).with_feather(Feather {
            radius: 16.0,
            ..Feather::NONE
        });
        let t0 = std::time::Instant::now();
        let field = s.distance_field(w, h, true).unwrap();
        let field_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let mut times = Vec::new();
        for r in [8.0f32, 12.0, 20.0, 32.0, 48.0] {
            let s2 = s.clone().with_feather(Feather {
                radius: r,
                ..Feather::NONE
            });
            let t = std::time::Instant::now();
            let m = s2.shape_field(&field, w, h);
            times.push(t.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(m);
        }
        println!(
            "distance field (once per shape): {field_ms:.1} ms; reshape per slider step: {times:.1?} ms"
        );
        let t = std::time::Instant::now();
        let ants = s.ant_contours();
        println!(
            "ant contour offset: {:.1} ms ({} points)",
            t.elapsed().as_secs_f64() * 1000.0,
            ants.iter().map(|c| c.len()).sum::<usize>()
        );
        let t = std::time::Instant::now();
        let s3 = s.clone().with_feather(Feather {
            radius: 30.0,
            ..Feather::NONE
        });
        assert!(
            s3.field_matches(&field, true),
            "field cache must survive radius changes"
        );
        println!(
            "field_matches: {:.3} ms",
            t.elapsed().as_secs_f64() * 1000.0
        );
    }
}
