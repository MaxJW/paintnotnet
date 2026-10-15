//! Edge colour clean-up: the colours of the subject (and of the background) where the matte is
//! soft, so a cut-out's hair or fur doesn't keep a fringe of the old background.
//!
//! A soft pixel is a mix `I = αF + (1 − α)B` of the foreground colour F and background colour
//! B; this solves for both with "Fast Multi-Level Foreground Estimation" (Germer, Uelwer,
//! Conrad, Harmeling, ICPR 2020, arXiv:2006.14970), ported from PyMatting's
//! `pymatting/foreground/estimate_foreground_ml.py` (MIT; commit 6d5c4a6, see NOTICE): from a
//! 1 × 1 image up to full size, each level starts from the one below (nearest neighbour) and
//! refines every pixel's F and B by solving the 2 × 2 least-squares system made of the
//! compositing equation and smoothness towards its four neighbours, weighted by how much
//! the matte changes between them. Same constants as PyMatting's defaults.
//!
//! Differences: PyMatting sweeps the pixels in order (Gauss–Seidel); here even rows are
//! updated, then odd rows, each row left to right, so rows can run in parallel (a "zebra"
//! Gauss–Seidel; same fixed point, a different path to it). F and B are kept in 16 bits.

use rayon::prelude::*;

const REGULARIZATION: f32 = 1e-5;
const SMALL_ITERATIONS: usize = 10;
const BIG_ITERATIONS: usize = 2;
const SMALL_SIZE: usize = 32;
const GRADIENT_WEIGHT: f32 = 1.0;

/// Estimated colours, interleaved RGB per pixel.
pub struct EdgeColours {
    pub foreground: Vec<u8>,
    pub background: Vec<u8>,
}

/// F and B of one pixel in 16-bit fixed point (0 = 0.0, 65535 = 1.0).
type Fb = [u16; 6];

fn to16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}

fn from16(v: u16) -> f32 {
    v as f32 * (1.0 / 65535.0)
}

/// Estimate the foreground and background colours of `rgb` (interleaved, `w × h`) given
/// the matte `alpha` (0–255).
pub fn estimate(rgb: &[u8], alpha: &[u8], w: usize, h: usize) -> EdgeColours {
    assert_eq!(rgb.len(), w * h * 3);
    assert_eq!(alpha.len(), w * h);
    // Mean colours of the clear subject (α > 0.9) and clear background (α < 0.1): the
    // 1 × 1 starting point.
    let (fsum, fn_, bsum, bn) = rgb
        .par_chunks(3)
        .zip(alpha.par_iter())
        .fold(
            || ([0f64; 3], 0u64, [0f64; 3], 0u64),
            |(mut fs, mut fc, mut bs, mut bc), (p, &a)| {
                if a as f32 > 0.9 * 255.0 {
                    (0..3).for_each(|c| fs[c] += p[c] as f64);
                    fc += 1;
                }
                if (a as f32) < 0.1 * 255.0 {
                    (0..3).for_each(|c| bs[c] += p[c] as f64);
                    bc += 1;
                }
                (fs, fc, bs, bc)
            },
        )
        .reduce(
            || ([0f64; 3], 0, [0f64; 3], 0),
            |a, b| {
                (
                    [a.0[0] + b.0[0], a.0[1] + b.0[1], a.0[2] + b.0[2]],
                    a.1 + b.1,
                    [a.2[0] + b.2[0], a.2[1] + b.2[1], a.2[2] + b.2[2]],
                    a.3 + b.3,
                )
            },
        );
    let mean = |s: [f64; 3], n: u64| s.map(|v| (v / 255.0 / (n as f64 + 1e-5)) as f32);
    let (fm, bm) = (mean(fsum, fn_), mean(bsum, bn));
    let mut prev: Vec<Fb> = vec![[
        to16(fm[0]),
        to16(fm[1]),
        to16(fm[2]),
        to16(bm[0]),
        to16(bm[1]),
        to16(bm[2]),
    ]];
    let (mut pw, mut ph) = (1usize, 1usize);
    let levels = (w.max(h) as f64).log2().ceil().max(1.0) as u32;
    for level in 0..=levels {
        let t = level as f64 / levels as f64;
        let lw = ((w as f64).powf(t).round() as usize).max(1);
        let lh = ((h as f64).powf(t).round() as usize).max(1);
        // Nearest neighbour, as PyMatting: source index = destination × source / destination.
        let xs: Vec<usize> = (0..lw).map(|x| x * w / lw).collect();
        let ys: Vec<usize> = (0..lh).map(|y| y * h / lh).collect();
        let full = lw == w && lh == h;
        let img: Vec<[f32; 3]> = (0..lw * lh)
            .into_par_iter()
            .map(|i| {
                let k = if full { i } else { ys[i / lw] * w + xs[i % lw] };
                [
                    rgb[3 * k] as f32 / 255.0,
                    rgb[3 * k + 1] as f32 / 255.0,
                    rgb[3 * k + 2] as f32 / 255.0,
                ]
            })
            .collect();
        let a: Vec<f32> = (0..lw * lh)
            .into_par_iter()
            .map(|i| {
                let k = if full { i } else { ys[i / lw] * w + xs[i % lw] };
                alpha[k] as f32 / 255.0
            })
            .collect();
        let mut fb: Vec<Fb> = (0..lw * lh)
            .into_par_iter()
            .map(|i| {
                let (x, y) = (i % lw, i / lw);
                prev[(y * ph / lh).min(ph - 1) * pw + (x * pw / lw).min(pw - 1)]
            })
            .collect();
        let iterations = if lw <= SMALL_SIZE && lh <= SMALL_SIZE {
            SMALL_ITERATIONS
        } else {
            BIG_ITERATIONS
        };
        for _ in 0..iterations {
            for parity in 0..2 {
                sweep(&mut fb, &img, &a, lw, lh, parity);
            }
        }
        prev = fb;
        (pw, ph) = (lw, lh);
    }
    let out = |off: usize| -> Vec<u8> {
        prev.par_iter()
            .flat_map_iter(|p| (0..3).map(move |c| (from16(p[off + c]) * 255.0 + 0.5) as u8))
            .collect()
    };
    EdgeColours {
        foreground: out(0),
        background: out(3),
    }
}

/// Update the rows of one parity in place. Their neighbours above and below are rows of
/// the other parity, which this pass doesn't change: a copy of those is read instead, so the
/// rows being written can be handed out in parallel.
fn sweep(fb: &mut [Fb], img: &[[f32; 3]], a: &[f32], w: usize, h: usize, parity: usize) {
    let other: Vec<Vec<Fb>> = (0..h)
        .filter(|y| y % 2 != parity)
        .map(|y| fb[y * w..(y + 1) * w].to_vec())
        .collect();
    // Row y of the other parity is `other[y / 2]` (both parities count from their first row).
    fb.par_chunks_mut(w)
        .enumerate()
        .filter(|(y, _)| y % 2 == parity)
        .for_each(|(y, row)| {
            let up = (y > 0).then(|| &other[(y - 1) / 2]);
            let down = (y + 1 < h).then(|| &other[y.div_ceil(2)]);
            for x in 0..w {
                let i = y * w + x;
                let a0 = a[i];
                let a1 = 1.0 - a0;
                let (mut a00, a01, mut a11) = (a0 * a0, a0 * a1, a1 * a1);
                let mut b0 = img[i].map(|v| a0 * v);
                let mut b1 = img[i].map(|v| a1 * v);
                // Left, right, up, down; past the edge the neighbour is the pixel itself.
                let neighbours: [(Fb, f32); 4] = [
                    if x > 0 {
                        (row[x - 1], a[i - 1])
                    } else {
                        (row[x], a0)
                    },
                    if x + 1 < w {
                        (row[x + 1], a[i + 1])
                    } else {
                        (row[x], a0)
                    },
                    match up {
                        Some(r) => (r[x], a[i - w]),
                        None => (row[x], a0),
                    },
                    match down {
                        Some(r) => (r[x], a[i + w]),
                        None => (row[x], a0),
                    },
                ];
                for (n, an) in neighbours {
                    let da = REGULARIZATION + GRADIENT_WEIGHT * (a0 - an).abs();
                    a00 += da;
                    a11 += da;
                    for c in 0..3 {
                        b0[c] += da * from16(n[c]);
                        b1[c] += da * from16(n[3 + c]);
                    }
                }
                let inv = 1.0 / (a00 * a11 - a01 * a01);
                let (m00, m01, m11) = (inv * a11, -inv * a01, inv * a00);
                let mut out = [0u16; 6];
                for c in 0..3 {
                    out[c] = to16(m00 * b0[c] + m01 * b1[c]);
                    out[3 + c] = to16(m01 * b0[c] + m11 * b1[c]);
                }
                row[x] = out;
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A red subject over a blue background, mixed in a soft band (as hair against the sky):
    /// the estimate gives back red across the band, and blue behind it.
    #[test]
    fn recovers_the_colours_mixed_in_a_soft_edge() {
        let (w, h) = (120usize, 90usize);
        let (fg, bg) = ([200.0f32, 40.0, 30.0], [40.0f32, 90.0, 220.0]);
        let mut alpha = vec![0u8; w * h];
        let mut rgb = vec![0u8; w * h * 3];
        for y in 0..h {
            for x in 0..w {
                // Opaque on the left, a 30 px ramp, clear on the right; plus wiggles.
                let edge = 45.0 + 8.0 * (y as f32 * 0.3).sin();
                let t = ((x as f32 - edge) / 30.0).clamp(0.0, 1.0);
                let al = 1.0 - t;
                alpha[y * w + x] = (al * 255.0).round() as u8;
                let al = alpha[y * w + x] as f32 / 255.0;
                for c in 0..3 {
                    rgb[3 * (y * w + x) + c] = (al * fg[c] + (1.0 - al) * bg[c]).round() as u8;
                }
            }
        }
        let est = estimate(&rgb, &alpha, w, h);
        let mut worst_f = 0.0f32;
        let mut worst_b = 0.0f32;
        for (i, &al) in alpha.iter().enumerate() {
            for c in 0..3 {
                if al >= 26 {
                    worst_f = worst_f.max((est.foreground[3 * i + c] as f32 - fg[c]).abs());
                }
                if al <= 229 {
                    worst_b = worst_b.max((est.background[3 * i + c] as f32 - bg[c]).abs());
                }
            }
        }
        // Where the subject shows at least 10 %, its colour is back within a few levels
        // (the naive cut-out would keep up to 90 % of the blue there).
        assert!(worst_f <= 2.0, "foreground off by {worst_f}");
        assert!(worst_b <= 2.0, "background off by {worst_b}");
    }

    #[test]
    fn opaque_and_clear_pixels_keep_their_colour() {
        let (w, h) = (40usize, 30usize);
        let rgb: Vec<u8> = (0..w * h * 3).map(|i| (i * 37 % 251) as u8).collect();
        let alpha = vec![255u8; w * h];
        let est = estimate(&rgb, &alpha, w, h);
        for (a, b) in est.foreground.iter().zip(&rgb) {
            assert!((*a as i32 - *b as i32).abs() <= 1);
        }
        let est = estimate(&rgb, &vec![0u8; w * h], w, h);
        for (a, b) in est.background.iter().zip(&rgb) {
            assert!((*a as i32 - *b as i32).abs() <= 1);
        }
    }
}
