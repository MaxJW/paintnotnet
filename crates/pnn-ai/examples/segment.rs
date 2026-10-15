//! Run a model on an image and save the matte: `segment IN.png OUT.png [best|fast] [MODEL_DIR]`.
//! Prints load and run times.

use std::time::Instant;

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    let (input, output) = (&args[1], &args[2]);
    let quality = match args.get(3).map(String::as_str) {
        Some("best") => pnn_ai::Quality::Best,
        _ => pnn_ai::Quality::Fast,
    };
    let dir = args
        .get(4)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(pnn_ai::models_dir);
    let rt = pnn_ai::runtime().expect("runtime");
    let model = pnn_ai::Model::find(quality, rt.device.precision());
    println!("{}; {}", rt.device.label(), model.file_name());
    let img = image::open(input).expect("open").to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let t = Instant::now();
    let mut seg = pnn_ai::Segmenter::load(model, &model.path_in(&dir)).expect("load");
    println!("load {:?} on {}", t.elapsed(), seg.device.label());
    for i in 0..3 {
        let t = Instant::now();
        let m = seg.matte(img.as_raw(), w, h).expect("matte");
        println!("run {i}: {:?} ({w}x{h})", t.elapsed());
        if i == 0 {
            for (shift, softness) in [
                (5, 0.0),
                (30, 0.0),
                (-30, 0.0),
                (0, 4.0),
                (0, 20.0),
                (30, 20.0),
            ] {
                let t = Instant::now();
                let r = pnn_ai::Refine {
                    shift,
                    softness,
                    invert: false,
                }
                .apply(&m, w, h);
                println!(
                    "refine shift {shift} softness {softness}: {:?}",
                    t.elapsed()
                );
                drop(r);
            }
            image::GrayImage::from_raw(w as u32, h as u32, m)
                .expect("size")
                .save(output)
                .expect("save");
        }
    }
}
