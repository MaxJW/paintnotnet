# Phase 15 audit: maths, accuracy and speed (2026-10-14)

What paintnotnet does today, where it falls short, and what to do about it, in order of
visible benefit. Speed figures are from `cargo run --release -p pnn-effects --example bench`
(every adjustment and effect, rendered in 256 px tiles in parallel as the app does, on a
3840 × 2160 photo-like image, best of two runs; AMD Ryzen 7 5700X3D, 8 cores / 16 threads). `BENCH_AT=0.75`
puts every number property at 75% of its range: the slow end of a slider while it's dragged.

The benchmark first rendered row by row, which made separable blurs redo their vertical pass
for every row (Gaussian Blur at radius 150 measured 2.9 s). The app renders tiles, so
`framework::render_rect` now does too; Gaussian Blur, Drop Shadow and Square Blur are all
under 125 ms at 75%.

## 1. Speed: effects too slow to preview live at 4K

| Effect | defaults | at 75% | What makes it slow | Fix |
|---|---:|---:|---|---|
| Mandelbrot Fractal | 4.9 s | 5.6 s | 3.36's per-pixel iteration with supersampling, scalar f64 | lower per-sample cost (bail-out tests, cardioid/bulb check, periodicity), GPU later |
| Motion Blur | 0.36 s | 4.9 s | one bilinear sample per pixel of distance (3.36) | running sums along the motion direction: cost independent of distance |
| Radial Blur | 1.3 s | 4.0 s | many fixed-point rotated samples per pixel (3.36) | fewer, better-weighted samples (angle-adaptive), or polar-space running sums |
| Outline | 0.20 s | 2.6 s | local histogram updated per pixel, O(radius) | Perreault–Hébert constant-time histograms (column histograms) |
| Median | 0.20 s | 2.5 s | same | same |
| Reduce Noise | 0.19 s | 2.5 s | same | same |
| Dents | 0.28 s | 2.6 s | Perlin noise per supersample | cheaper noise per sample, fewer redundant octaves |
| Julia Fractal | 0.53 s | 2.4 s | as Mandelbrot | as Mandelbrot |
| Surface Blur | 0.37 s | 1.4 s | local histogram, O(radius) | constant-time histograms |
| Bokeh Blur | 0.16 s | 1.3 s | per-row prefix sums over each disc chord, O(radius) | acceptable; GPU later |
| Morphology | — | 1.1 s | O(radius) min/max | van Herk/Gil–Werman: O(1) per pixel for square/line shapes |

Everything else is under 0.9 s at 75% and under 0.5 s at defaults; most adjustments take
2–16 ms. The effect runner already shows tiles as they finish, so these are "live" in the
sense of progressive, not instant.

Target from the roadmap: every slider live at 4K. The algorithmic fixes above remove the
O(radius) and O(distance) costs on the CPU; GPU compute (wgpu) is the next step for whatever
still misses ~100 ms.

## 2. Accuracy: where better maths visibly helps

1. **Gaussian Blur is not a Gaussian.** 3.36 uses a tent kernel (weights r+1−|d|), which is
   a triangle, not a bell: highlights blur into soft diamonds/squares rather than discs, and
   edges get a visible kink. Fix: a true Gaussian with the same variance (σ = r/√6, so the
   amount of blur at a given radius stays familiar), via a box-cascade or recursive filter
   (cost independent of radius). Option: blur in linear light, so bright highlights spread
   the way light does instead of being dimmed by sRGB averaging.
2. **Gradients band.** The gradient is looked up in a 256-entry table, so a gradient across a
   4K canvas has 256 flat bands about 15 px wide, very visible in dark or subtle gradients.
   Fix: interpolate per pixel in higher precision and dither to 8 bits (blue-noise or
   interleaved-gradient noise), which removes banding without visible noise.
3. **Scaling a selection down aliases.** Move Selected Pixels samples one point per output
   pixel with a fixed 4×4 bicubic kernel, so shrinking below about half size turns fine
   detail into moiré and shimmering edges. Fix: widen the filter by the shrink factor
   (area-correct footprint), as Image › Resize already does.
4. **Magic Wand and Paint Bucket tolerance is in RGB.** 3.36 compares squared R, G, B, A
   differences, which is uneven to the eye (a small change in a dark blue counts the same as
   a big one in a light yellow). Fix: an option to compare in OKLab (perceptual distance),
   tolerance scaled so 0–100% still spans "identical" to "everything".
5. **Blending is in 8-bit sRGB.** The compositor blends layers on 8-bit gamma-encoded values,
   as paint.net and most editors (and file formats such as OpenRaster) do. Linear-light
   blending looks more physical but changes every image's look and breaks agreement with
   other programs, so it isn't recommended as a default; at most a per-document option later.

Already fine (checked, no change needed): Image › Resize (standard filters including
Lanczos 3, premultiplied alpha, optional linear light); feather (exact signed distance
fields); shape and text antialiasing (tiny-skia coverage); brush stamping (8-bit coverage
per dab with max-accumulation, no build-up within a stroke).

## 3. Plan

- **15a** (this document): benchmark, tile-correct `render_rect`, audit.
- **15b** speed, CPU: constant-time histograms (Median, Reduce Noise, Outline, Surface Blur),
  distance-independent Motion Blur, Morphology (van Herk), faster fractals, Radial Blur, Dents.
- **15c** accuracy: true Gaussian (+ linear light option), dithered gradients, filtered
  downscaling in the move tools, OKLab tolerance option. Each keeps a test showing the
  difference from the old method.
- **15d** GPU compute for whatever still misses the live target, and the 165 fps frame budget
  while painting, panning and previewing (measured with the smoke test at 4K).
