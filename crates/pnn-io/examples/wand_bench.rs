//! Time the Magic Wand's steps on an image: `wand_bench IMAGE [x y]...` (default: a few seeds).

use std::time::Instant;

use pnn_core::selection::{CombineMode, Selection, Tolerance, flood_stencil};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t = Instant::now();
    let doc = pnn_io::load(std::path::Path::new(&args[1])).expect("load");
    let (w, h) = (doc.width(), doc.height());
    println!("load {:?} {w}x{h}", t.elapsed());
    let t = Instant::now();
    let pixels = doc.layer(0).surface.to_pixels();
    println!("to_pixels {:?}", t.elapsed());
    {
        use rayon::prelude::*;
        let t = Instant::now();
        let n = pixels
            .par_iter()
            .filter(|&&c| pnn_core::color::oklab(c)[0] > 0.5)
            .count();
        println!("oklab of every pixel (parallel) {:?} ({n})", t.elapsed());
    }
    let seeds: Vec<(u32, u32)> = if args.len() > 3 {
        args[2..]
            .chunks(2)
            .map(|c| (c[0].parse().unwrap(), c[1].parse().unwrap()))
            .collect()
    } else {
        vec![(w / 2, h / 2), (10, 10), (w - 10, h / 3), (w / 4, h - 10)]
    };
    let tol = Tolerance {
        pct: 50,
        premultiplied: true,
        perceptual: true,
    };
    for seed in seeds {
        let t = Instant::now();
        let st = flood_stencil(&pixels, w, h, seed, tol, true);
        let t_flood = t.elapsed();
        let n = st.iter().filter(|&&b| b).count();
        let t = Instant::now();
        let shape = Selection::from_stencil(&st, w, h);
        let t_trace = t.elapsed();
        let pts: usize = shape.contours().iter().map(|c| c.len()).sum();
        let t = Instant::now();
        let combined = Selection::none().combine(&shape, CombineMode::Replace);
        let t_comb = t.elapsed();
        let t = Instant::now();
        let m = combined.rasterize(w, h, true);
        let t_rast = t.elapsed();
        let t = Instant::now();
        let ants = combined.ant_contours();
        let t_ants = t.elapsed();
        let t = Instant::now();
        let _ = combined.pixel_bounds(w, h);
        let _ = combined.bounds();
        let t_bounds = t.elapsed();
        drop(m);
        println!(
            "seed {seed:?}: {n} px selected; flood {t_flood:?}, trace {t_trace:?} ({} contours, {pts} points), combine {t_comb:?}, rasterize {t_rast:?}, ants {t_ants:?} ({} contours), bounds {t_bounds:?}",
            shape.contours().len(),
            ants.len()
        );
    }
}
