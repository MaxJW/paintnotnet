//! Reshaping a matte after the model has run, live while the dialog's sliders move: move the
//! edge in or out, soften it, invert it.

use rayon::prelude::*;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Refine {
    /// Pixels to move the edge out (positive) or in (negative): grey-level dilation or
    /// erosion by a disc, so soft detail such as hair keeps its shape.
    pub shift: i32,
    /// Gaussian blur of the matte, standard deviation in pixels.
    pub softness: f32,
    pub invert: bool,
}

impl Refine {
    pub fn apply(&self, matte: &[u8], w: usize, h: usize) -> Vec<u8> {
        let mut m = match self.shift {
            0 => matte.to_vec(),
            s => disc_extreme(matte, w, h, s.unsigned_abs() as usize, s > 0),
        };
        if self.softness > 0.0 {
            m = gaussian(&m, w, h, self.softness);
        }
        if self.invert {
            m.par_iter_mut().for_each(|v| *v = 255 - *v);
        }
        m
    }
}

/// Max (`grow`) or min of each pixel's disc of radius `r`, over the part inside the image.
/// Per output row: the disc's rows, each a running max/min over its half-width (van Herk /
/// Gil-Werman, three comparisons per pixel whatever the width). Mattes are mostly flat (all
/// subject or all background), so runs of pixels whose whole neighbourhood is one value are
/// filled with it and only the stretches near an edge are computed.
fn disc_extreme(m: &[u8], w: usize, h: usize, r: usize, grow: bool) -> Vec<u8> {
    const B: usize = 32;
    let neutral = if grow { 0u8 } else { 255 };
    let pick = move |a: u8, b: u8| if grow { a.max(b) } else { a.min(b) };
    let spans: Vec<(isize, usize)> = (-(r as isize)..=r as isize)
        .map(|dy| {
            let hw = ((r * r) as f64 - (dy * dy) as f64).sqrt().floor() as usize;
            (dy, hw)
        })
        .collect();
    // Each 32 × 32 block's value if it is uniform.
    let (bw, bh) = (w.div_ceil(B), h.div_ceil(B));
    let flat: Vec<Option<u8>> = (0..bw * bh)
        .into_par_iter()
        .map(|k| {
            let (bx, by) = (k % bw, k / bw);
            let v = m[by * B * w + bx * B];
            (by * B..((by + 1) * B).min(h))
                .all(|y| {
                    m[y * w + bx * B..y * w + ((bx + 1) * B).min(w)]
                        .iter()
                        .all(|&p| p == v)
                })
                .then_some(v)
        })
        .collect();
    let mut out = vec![neutral; w * h];
    out.par_chunks_mut(w).enumerate().for_each_init(
        || (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
        |(padded, fwd, back, runs), (y, row)| {
            // Blocks of this row whose neighbourhood (the disc around every pixel) is one value.
            let (by0, by1) = (y.saturating_sub(r) / B, (y + r).min(h - 1) / B);
            runs.clear();
            for bx in 0..bw {
                let (x0, x1) = (bx * B, ((bx + 1) * B).min(w));
                let (bx0, bx1) = (x0.saturating_sub(r) / B, (x1 - 1 + r).min(w - 1) / B);
                let first = flat[by0 * bw + bx0];
                let uniform = first.is_some()
                    && (by0..=by1).all(|j| (bx0..=bx1).all(|i| flat[j * bw + i] == first));
                match first.filter(|_| uniform) {
                    Some(v) => row[x0..x1].fill(v),
                    None => match runs.last_mut() {
                        Some((_, end)) if *end == x0 => *end = x1,
                        _ => runs.push((x0, x1)),
                    },
                }
            }
            for &(xs, xe) in runs.iter() {
                let out = &mut row[xs..xe];
                for &(dy, hw) in &spans {
                    let sy = y as isize + dy;
                    if sy < 0 || sy >= h as isize {
                        continue;
                    }
                    let src = &m[sy as usize * w..(sy as usize + 1) * w];
                    // Source columns xs - hw .. xe + hw, padded with the neutral value outside
                    // the image; then blocks of the window's width: running extremes forwards
                    // and backwards within each block.
                    let k = 2 * hw + 1;
                    let n = (xe - xs) + 2 * hw;
                    let len = n.div_ceil(k) * k;
                    padded.clear();
                    padded.resize(len, neutral);
                    let lo = xs as isize - hw as isize;
                    let (a, b) = (lo.max(0) as usize, (xe + hw).min(w));
                    let at = (a as isize - lo) as usize;
                    padded[at..at + (b - a)].copy_from_slice(&src[a..b]);
                    if hw == 0 {
                        for (o, &v) in out.iter_mut().zip(padded.iter()) {
                            *o = pick(*o, v);
                        }
                        continue;
                    }
                    fwd.resize(len, neutral);
                    back.resize(len, neutral);
                    for b in (0..len).step_by(k) {
                        fwd[b] = padded[b];
                        for i in b + 1..b + k {
                            fwd[i] = pick(fwd[i - 1], padded[i]);
                        }
                        back[b + k - 1] = padded[b + k - 1];
                        for i in (b..b + k - 1).rev() {
                            back[i] = pick(back[i + 1], padded[i]);
                        }
                    }
                    // Window [x, x + k) in padded coordinates is centred on column xs + x.
                    for (x, o) in out.iter_mut().enumerate() {
                        *o = pick(*o, pick(back[x], fwd[x + k - 1]));
                    }
                }
            }
        },
    );
    out
}

/// Three box blurs approximating a Gaussian of standard deviation `sigma` (Kovesi's box
/// sizes), edges extended.
fn gaussian(m: &[u8], w: usize, h: usize, sigma: f32) -> Vec<u8> {
    let n = 3.0f64;
    let s = sigma as f64;
    let ideal = (12.0 * s * s / n + 1.0).sqrt();
    let mut wl = ideal.floor() as usize;
    if wl.is_multiple_of(2) {
        wl -= 1;
    }
    let wu = wl + 2;
    let mm = ((12.0 * s * s - n * (wl * wl) as f64 - 4.0 * n * wl as f64 - 3.0 * n)
        / (-4.0 * wl as f64 - 4.0))
        .round() as usize;
    let radii: Vec<usize> = (0..3)
        .map(|i| if i < mm { wl / 2 } else { wu / 2 })
        .collect();
    let mut a: Vec<u16> = m.iter().map(|&v| v as u16 * 64).collect();
    for &r in &radii {
        if r == 0 {
            continue;
        }
        a = box_rows(&a, w, h, r);
        a = box_cols(&a, w, h, r);
    }
    a.into_iter()
        .map(|v| ((v + 32) / 64).min(255) as u8)
        .collect()
}

/// Box blur of radius `r` along each row, edges extended.
fn box_rows(a: &[u16], w: usize, h: usize, r: usize) -> Vec<u16> {
    let mut out = vec![0u16; w * h];
    let d = (2 * r + 1) as u32;
    out.par_chunks_mut(w)
        .zip(a.par_chunks(w))
        .take(h)
        .for_each(|(o, row)| {
            let at = |i: isize| row[i.clamp(0, w as isize - 1) as usize] as u32;
            let mut sum: u32 = (-(r as isize)..=r as isize).map(at).sum();
            for (x, v) in o.iter_mut().enumerate() {
                *v = ((sum + d / 2) / d) as u16;
                let x = x as isize;
                sum = sum + at(x + r as isize + 1) - at(x - r as isize);
            }
        });
    out
}

/// Box blur of radius `r` down each column, edges extended: strips of columns in parallel,
/// each walking down the rows with a running sum per column.
fn box_cols(a: &[u16], w: usize, h: usize, r: usize) -> Vec<u16> {
    const STRIP: usize = 256;
    let d = (2 * r + 1) as u32;
    let strips: Vec<(usize, Vec<u16>)> = (0..w.div_ceil(STRIP))
        .into_par_iter()
        .map(|k| {
            let (x0, x1) = (k * STRIP, ((k + 1) * STRIP).min(w));
            let sw = x1 - x0;
            let row = |y: isize| &a[y.clamp(0, h as isize - 1) as usize * w + x0..][..sw];
            let mut sum = vec![0u32; sw];
            for y in -(r as isize)..=r as isize {
                for (s, &v) in sum.iter_mut().zip(row(y)) {
                    *s += v as u32;
                }
            }
            let mut out = vec![0u16; sw * h];
            for y in 0..h as isize {
                let o = &mut out[y as usize * sw..(y as usize + 1) * sw];
                for (o, &s) in o.iter_mut().zip(&sum) {
                    *o = ((s + d / 2) / d) as u16;
                }
                let (add, sub) = (row(y + r as isize + 1), row(y - r as isize));
                for ((s, &p), &q) in sum.iter_mut().zip(add).zip(sub) {
                    *s = *s + p as u32 - q as u32;
                }
            }
            (x0, out)
        })
        .collect();
    let mut out = vec![0u16; w * h];
    for (x0, strip) in strips {
        let sw = strip.len() / h.max(1);
        for (y, src) in strip.chunks(sw.max(1)).enumerate() {
            out[y * w + x0..y * w + x0 + sw].copy_from_slice(src);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(w: usize, h: usize, cx: f64, cy: f64, r: f64) -> Vec<u8> {
        (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) as f64, (i / w) as f64);
                if (x - cx).hypot(y - cy) <= r { 255 } else { 0 }
            })
            .collect()
    }

    fn area(m: &[u8]) -> usize {
        m.iter().filter(|&&v| v >= 128).count()
    }

    /// Brute force for the disc dilation/erosion.
    fn slow(m: &[u8], w: usize, h: usize, r: usize, grow: bool) -> Vec<u8> {
        let mut out = vec![0u8; w * h];
        let r2 = (r * r) as isize;
        for y in 0..h as isize {
            for x in 0..w as isize {
                let mut acc = if grow { 0u8 } else { 255 };
                for dy in -(r as isize)..=r as isize {
                    for dx in -(r as isize)..=r as isize {
                        let (sx, sy) = (x + dx, y + dy);
                        if dx * dx + dy * dy > r2
                            || sx < 0
                            || sy < 0
                            || sx >= w as isize
                            || sy >= h as isize
                        {
                            continue;
                        }
                        let v = m[sy as usize * w + sx as usize];
                        acc = if grow { acc.max(v) } else { acc.min(v) };
                    }
                }
                out[y as usize * w + x as usize] = acc;
            }
        }
        out
    }

    #[test]
    fn shifting_is_a_disc_dilation_or_erosion() {
        // Noise, and a matte-like image: flat areas (skipped) with soft blobs and specks
        // near block corners and the image's edges (computed).
        let (w, h) = (61, 47);
        let noise: Vec<u8> = (0..w * h)
            .map(|i| ((i * 2654435761usize) >> 13) as u8)
            .collect();
        let (mw, mh) = (150, 110);
        let matte: Vec<u8> = (0..mw * mh)
            .map(|i| {
                let (x, y) = ((i % mw) as f64, (i / mw) as f64);
                let d = (x - 70.0).hypot(y - 50.0) - 30.0;
                let speck = (i % mw == 31 && i / mw == 95) || (i % mw == 149 && i / mw == 0);
                if speck {
                    200
                } else {
                    ((0.5 - d / 3.0).clamp(0.0, 1.0) * 255.0) as u8
                }
            })
            .collect();
        for (m, w, h) in [(&noise, w, h), (&matte, mw, mh)] {
            for r in [1, 2, 5, 9, 40] {
                for grow in [true, false] {
                    assert_eq!(
                        disc_extreme(m, w, h, r, grow),
                        slow(m, w, h, r, grow),
                        "{w}x{h} r={r} grow={grow}"
                    );
                }
            }
        }
    }

    #[test]
    fn moving_the_edge_grows_and_shrinks_a_disc_by_that_much() {
        let (w, h) = (200, 200);
        let m = disc(w, h, 100.0, 100.0, 50.0);
        let pi = std::f64::consts::PI;
        for shift in [-10, 10] {
            let out = Refine {
                shift,
                ..Default::default()
            }
            .apply(&m, w, h);
            let r = 50.0 + shift as f64;
            let expected = pi * r * r;
            let got = area(&out) as f64;
            assert!(
                (got - expected).abs() / expected < 0.03,
                "shift {shift}: {got} vs {expected}"
            );
        }
    }

    #[test]
    fn softening_blurs_and_inverting_flips() {
        let (w, h) = (64, 32);
        let m: Vec<u8> = (0..w * h)
            .map(|i| if i % w < 32 { 255 } else { 0 })
            .collect();
        let soft = Refine {
            softness: 4.0,
            ..Default::default()
        }
        .apply(&m, w, h);
        // Far from the edge unchanged; at the edge half; monotone across it.
        assert_eq!(soft[5], 255);
        assert_eq!(soft[60], 0);
        let edge = (soft[31] as i32 + soft[32] as i32) / 2;
        assert!((edge - 128).abs() <= 3, "{edge}");
        assert!(soft[..64].windows(2).all(|p| p[0] >= p[1]));
        // About as wide as a Gaussian of σ 4: 16 % → 84 % over ~2σ.
        let width = soft[..64].iter().filter(|&&v| v > 40 && v < 215).count();
        assert!((6..=10).contains(&width), "{width}");
        let inv = Refine {
            invert: true,
            ..Default::default()
        }
        .apply(&m, w, h);
        assert_eq!(inv[0], 0);
        assert_eq!(inv[63], 255);
    }
}
