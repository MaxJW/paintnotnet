//! Selections as geometry, like paint.net: an even-odd polygon set (paint.net's Copy
//! Selection exports `{"polygonList": [...]}`). Combine modes are polygon booleans; a coverage
//! mask is rasterised from the geometry (antialiased or pixelated) for pixel operations; the
//! marching ants follow the polygon edges exactly.

use i_overlay::core::fill_rule::FillRule as OverlayFill;
use i_overlay::core::overlay_rule::OverlayRule;
use i_overlay::float::single::SingleFloatOverlay;
use tiny_skia::{FillRule, PathBuilder, Transform};

use crate::{Mask, Rect, TILE_SIZE, TileCoord};

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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Selection {
    /// Even-odd polygon set in image pixel coordinates. Empty = nothing selected.
    contours: Vec<Contour>,
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
        Selection::from_contours(shapes.into_iter().flatten().collect())
    }

    /// paint.net's Invert Selection: xor with the whole canvas.
    pub fn invert(&self, width: u32, height: u32) -> Selection {
        let all = Selection::all(width, height);
        if self.is_empty() {
            return all;
        }
        all.combine(self, CombineMode::Exclude)
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
        let x0 = x0.floor().max(0.0) as u32;
        let y0 = y0.floor().max(0.0) as u32;
        let x1 = (x1.ceil().max(0.0) as u32).min(width);
        let y1 = (y1.ceil().max(0.0) as u32).min(height);
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
        Selection { contours }
    }

    pub fn translate(&self, dx: f64, dy: f64) -> Selection {
        self.transform([1.0, 0.0, dx, 0.0, 1.0, dy])
    }

    /// Coverage mask (0..=255) for a `width` × `height` canvas. `antialias` = paint.net's
    /// "Antialiased selection quality"; otherwise pixels are in or out by their centre.
    /// An empty selection gives an empty mask (callers treat "no selection" as everything).
    pub fn rasterize(&self, width: u32, height: u32, antialias: bool) -> Mask {
        let mut mask = Mask::new(width, height, 0);
        let Some(r) = self.pixel_bounds(width, height) else {
            return mask;
        };
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
        let Some(path) = pb.finish() else { return mask };
        let Some(mut m) = tiny_skia::Mask::new(r.width, r.height) else {
            return mask;
        };
        m.fill_path(&path, FillRule::EvenOdd, antialias, Transform::identity());
        let data = m.data();
        // Copy the bounding-box raster into the tiled mask.
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
        mask.compact();
        mask
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

/// Flood fill on `pixels` (row-major) from `seed`: contiguous (4-connected scanline fill) or
/// global. Returns the matching stencil.
pub fn flood_stencil(
    pixels: &[crate::ColorBgra],
    width: u32,
    height: u32,
    seed: (u32, u32),
    tolerance_pct: u8,
    premultiplied: bool,
    contiguous: bool,
) -> Vec<bool> {
    let (w, h) = (width as usize, height as usize);
    let target = pixels[seed.1 as usize * w + seed.0 as usize];
    let matches = |i: usize| colors_match(target, pixels[i], tolerance_pct, premultiplied);
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

    #[test]
    fn contiguous_vs_global_flood() {
        // Two white areas separated by a black column.
        let (w, h) = (5u32, 3u32);
        let mut px = vec![ColorBgra::WHITE; 15];
        for y in 0..3 {
            px[y * 5 + 2] = ColorBgra::BLACK;
        }
        let c = flood_stencil(&px, w, h, (0, 0), 0, true, true);
        assert_eq!(c.iter().filter(|&&b| b).count(), 6);
        let g = flood_stencil(&px, w, h, (0, 0), 0, true, false);
        assert_eq!(g.iter().filter(|&&b| b).count(), 12);
    }
}
