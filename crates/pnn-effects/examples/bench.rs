//! Times every adjustment and effect at its default settings on a 4K photo-like image, the
//! way the app applies one (renderer built once, rows rendered in parallel).
//!
//! `cargo run --release -p pnn-effects --example bench [filter]` prints a Markdown table,
//! slowest first. Phase 15 uses it to rank what to speed up. `BENCH_AT=0.75` sets every
//! number property to 75% of its range instead of its default (the slow end of sliders).

use std::sync::Arc;
use std::time::{Duration, Instant};

use pnn_core::{ColorBgra, Mask, Rect, Surface};
use pnn_effects::framework::{EffectDef, Env, PropKind, Value, render_rect};
use pnn_effects::{Adjustment, Category, adjustments, effect_def, effects_in};

const W: u32 = 3840;
const H: u32 = 2160;

/// Smooth gradients, a few hard-edged shapes and fine noise: something like a photo.
fn test_image() -> Surface {
    let mut s = Surface::new_transparent(W, H);
    let mut seed = 0x2545_f491_u32;
    let mut noise = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 17) as i32 - 8
    };
    let mut pixels = Vec::with_capacity((W * H) as usize);
    for y in 0..H {
        for x in 0..W {
            let (fx, fy) = (x as f32 / W as f32, y as f32 / H as f32);
            let mut r = 40.0 + 180.0 * fx;
            let mut g = 60.0 + 150.0 * fy;
            let mut b = 200.0 - 120.0 * fx * fy;
            let (dx, dy) = (fx - 0.6, fy - 0.45);
            if dx * dx + dy * dy < 0.04 {
                (r, g, b) = (230.0, 90.0, 40.0);
            }
            if (0.1..0.3).contains(&fx) && (0.6..0.9).contains(&fy) {
                (r, g, b) = (20.0, 30.0, 35.0);
            }
            let n = noise() as f32;
            let byte = |v: f32| (v + n).clamp(0.0, 255.0) as u8;
            pixels.push(ColorBgra::from_rgba(byte(r), byte(g), byte(b), 255));
        }
    }
    s.write_pixels(&pixels);
    s
}

/// An ellipse in the middle third, for the Selection effects.
fn selection() -> (Mask, Vec<Vec<[f64; 2]>>) {
    let mut m = Mask::new(W, H, 0);
    let (cx, cy, rx, ry) = (
        W as f64 / 2.0,
        H as f64 / 2.0,
        W as f64 / 6.0,
        H as f64 / 6.0,
    );
    for y in (cy - ry) as u32..(cy + ry) as u32 {
        for x in (cx - rx) as u32..(cx + rx) as u32 {
            let (dx, dy) = ((x as f64 - cx) / rx, (y as f64 - cy) / ry);
            if dx * dx + dy * dy <= 1.0 {
                m.set(x, y, 255);
            }
        }
    }
    let outline = (0..256)
        .map(|i| {
            let t = i as f64 / 256.0 * std::f64::consts::TAU;
            [cx + rx * t.cos(), cy + ry * t.sin()]
        })
        .collect();
    (m, vec![outline])
}

/// The defaults, or with `BENCH_AT` set every number property at that fraction of its range.
fn values(def: &EffectDef, env: &Env) -> Vec<Value> {
    let mut v = def.defaults_in(env);
    if let Some(f) = std::env::var("BENCH_AT")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
    {
        for (value, p) in v.iter_mut().zip(&def.props) {
            match p.kind {
                PropKind::Int { min, max } => {
                    *value = Value::Int(min + ((max - min) as f64 * f).round() as i32)
                }
                PropKind::Float { min, max, .. } => *value = Value::Float(min + (max - min) * f),
                _ => {}
            }
        }
    }
    v
}

fn time(def: &EffectDef, src: &Surface, env: &Env) -> Duration {
    let all = Rect {
        x: 0,
        y: 0,
        width: W,
        height: H,
    };
    let mut best = Duration::MAX;
    for _ in 0..2 {
        let t = Instant::now();
        let values = values(def, env);
        let r = def.renderer(&values, src, env);
        let out = render_rect(r.as_ref(), src, all);
        std::hint::black_box(&out);
        best = best.min(t.elapsed());
    }
    best
}

fn main() {
    let filter = std::env::args().nth(1).unwrap_or_default().to_lowercase();
    let src = test_image();
    let (mask, outline) = selection();
    let all = Rect {
        x: 0,
        y: 0,
        width: W,
        height: H,
    };
    let plain = Env {
        primary: ColorBgra::BLACK,
        secondary: ColorBgra::WHITE,
        bounds: all,
        selection: None,
        layers: vec![("Background".into(), src.clone())],
        active_layer: 0,
        selection_contours: Default::default(),
    };
    let selected = Env {
        selection: Some(Arc::new(mask)),
        selection_contours: Arc::new(outline),
        ..plain.clone()
    };
    let mut rows: Vec<(String, String, Duration)> = Vec::new();
    for a in Adjustment::ALL {
        if !a.name().to_lowercase().contains(&filter) {
            continue;
        }
        if let Some(def) = adjustments::def(a) {
            rows.push((
                "Adjustment".into(),
                a.name().into(),
                time(&def, &src, &plain),
            ));
        }
    }
    for c in Category::ALL {
        for (id, info) in effects_in(c) {
            if !info.name.to_lowercase().contains(&filter) {
                continue;
            }
            let Some(def) = effect_def(id) else {
                continue;
            };
            let env = if c == Category::Selection {
                &selected
            } else {
                &plain
            };
            rows.push((c.name().into(), info.name.into(), time(&def, &src, env)));
            eprintln!(
                "{:>8.1} ms  {}",
                rows.last().unwrap().2.as_secs_f64() * 1e3,
                info.name
            );
        }
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.2));
    println!("| Effect | Menu | ms at {W}×{H} |\n|---|---|---:|");
    for (cat, name, d) in rows {
        println!("| {name} | {cat} | {:.0} |", d.as_secs_f64() * 1e3);
    }
}
