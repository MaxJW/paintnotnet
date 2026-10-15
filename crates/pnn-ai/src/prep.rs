//! Getting pixels in and out of the model: the image scaled to the model's 1024 × 1024 input
//! with ImageNet mean/std normalisation (the models' `preprocessor_config.json`: rescale by
//! 1/255, mean 0.485/0.456/0.406, std 0.229/0.224/0.225, bilinear resampling), and the
//! output (logits) through a sigmoid and back to the image's size.

use rayon::prelude::*;

pub const SIZE: usize = 1024;
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// Weights of a triangle (bilinear) filter from `src` samples to `dst`, widened by the scale
/// when shrinking so every source pixel counts (as PIL's `BILINEAR`, which the models'
/// processor uses): per output sample, the first source index and its weights.
fn weights(src: usize, dst: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = src as f64 / dst as f64;
    let support = scale.max(1.0);
    (0..dst)
        .map(|i| {
            let centre = (i as f64 + 0.5) * scale;
            let lo = ((centre - support).floor().max(0.0)) as usize;
            let hi = ((centre + support).ceil() as usize).min(src);
            let mut w: Vec<f32> = (lo..hi)
                .map(|j| {
                    let d = ((j as f64 + 0.5 - centre) / support).abs();
                    (1.0 - d).max(0.0) as f32
                })
                .collect();
            let sum: f32 = w.iter().sum();
            if sum > 0.0 {
                w.iter_mut().for_each(|v| *v /= sum);
            }
            (lo, w)
        })
        .collect()
}

/// Resample a plane of `sw × sh` samples to `dw × dh`.
pub fn resize(src: &[f32], sw: usize, sh: usize, dw: usize, dh: usize) -> Vec<f32> {
    let wx = weights(sw, dw);
    let wy = weights(sh, dh);
    let mut tmp = vec![0f32; dw * sh];
    tmp.par_chunks_mut(dw).enumerate().for_each(|(y, row)| {
        let s = &src[y * sw..(y + 1) * sw];
        for (o, (lo, w)) in row.iter_mut().zip(&wx) {
            *o = w.iter().zip(&s[*lo..]).map(|(a, b)| a * b).sum();
        }
    });
    let mut out = vec![0f32; dw * dh];
    out.par_chunks_mut(dw).zip(&wy).for_each(|(row, (lo, w))| {
        for (k, wk) in w.iter().enumerate() {
            let s = &tmp[(lo + k) * dw..(lo + k + 1) * dw];
            for (o, v) in row.iter_mut().zip(s) {
                *o += wk * v;
            }
        }
    });
    out
}

/// The model's input: `rgb` (interleaved, `w × h`) as a 1 × 3 × 1024 × 1024 tensor.
pub fn input(rgb: &[u8], w: usize, h: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(3 * SIZE * SIZE);
    for c in 0..3 {
        let plane: Vec<f32> = rgb
            .iter()
            .skip(c)
            .step_by(3)
            .map(|&v| v as f32 / 255.0)
            .collect();
        let scaled = resize(&plane, w, h, SIZE, SIZE);
        out.extend(scaled.into_iter().map(|v| (v - MEAN[c]) / STD[c]));
    }
    out
}

/// The model's output (`SIZE × SIZE` logits) as coverage 0–255 at `w × h`.
pub fn output(logits: &[f32], w: usize, h: usize) -> Vec<u8> {
    let prob: Vec<f32> = logits
        .par_iter()
        .map(|&l| 1.0 / (1.0 + (-l).exp()))
        .collect();
    resize(&prob, SIZE, SIZE, w, h)
        .into_par_iter()
        .map(|p| (p * 255.0).round().clamp(0.0, 255.0) as u8)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resizing_keeps_flat_areas_and_averages_when_shrinking() {
        let flat = vec![0.25f32; 37 * 23];
        for v in resize(&flat, 37, 23, 64, 64) {
            assert!((v - 0.25).abs() < 1e-6);
        }
        // A one-pixel checkerboard shrunk 4× averages to grey rather than aliasing.
        let (w, h) = (64, 64);
        let checker: Vec<f32> = (0..w * h).map(|i| ((i % w + i / w) % 2) as f32).collect();
        for v in resize(&checker, w, h, 16, 16) {
            assert!((v - 0.5).abs() < 0.02, "{v}");
        }
        // Growing: samples between two pixels interpolate.
        let two = [0.0f32, 1.0];
        let up = resize(&two, 2, 1, 4, 1);
        assert_eq!(up[0], 0.0);
        assert!((up[1] - 0.25).abs() < 1e-6 && (up[2] - 0.75).abs() < 1e-6);
        assert_eq!(up[3], 1.0);
    }

    #[test]
    fn input_is_normalised_planes() {
        let rgb = [255u8, 0, 128].repeat(10 * 10);
        let t = input(&rgb, 10, 10);
        assert_eq!(t.len(), 3 * SIZE * SIZE);
        let at = |c: usize| t[c * SIZE * SIZE + 500 * SIZE + 500];
        assert!((at(0) - (1.0 - 0.485) / 0.229).abs() < 1e-4);
        assert!((at(1) - (0.0 - 0.456) / 0.224).abs() < 1e-4);
        assert!((at(2) - (128.0 / 255.0 - 0.406) / 0.225).abs() < 1e-4);
    }

    #[test]
    fn output_is_the_sigmoid_at_the_image_size() {
        let mut logits = vec![-20.0f32; SIZE * SIZE];
        logits[..SIZE * SIZE / 2].fill(20.0);
        let m = output(&logits, 100, 50);
        assert_eq!(m.len(), 100 * 50);
        assert_eq!(m[0], 255);
        assert_eq!(m[100 * 49], 0);
    }
}
