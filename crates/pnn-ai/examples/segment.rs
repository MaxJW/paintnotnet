//! Run a model on an image and save the matte: `segment IN.png OUT.png [best|fast] [MODEL_DIR]`.
//! Prints load, run and edge-control timings. The model runs in a helper process, as in the
//! app (this example started again with `--worker`).

use std::time::Instant;

fn main() {
    env_logger::init();
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--worker") {
        pnn_ai::worker::serve();
        return;
    }
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
    let (w, h) = (img.width(), img.height());
    let exe = std::env::current_exe().expect("exe");
    let mut worker = pnn_ai::Worker::spawn(&exe, "--worker", rt.device).expect("worker");
    let path = model.path_in(&dir);
    for i in 0..3 {
        let t = Instant::now();
        let m = worker
            .matte(model, &path, img.as_raw(), w, h)
            .expect("matte");
        println!(
            "run {i}: {:?} in the model on {}, {:?} in all ({w}x{h})",
            m.took,
            m.device.label(),
            t.elapsed()
        );
        if i == 0 {
            let (w, h) = (w as usize, h as usize);
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
                .apply(&m.coverage, w, h);
                println!(
                    "refine shift {shift} softness {softness}: {:?}",
                    t.elapsed()
                );
                drop(r);
            }
            image::GrayImage::from_raw(w as u32, h as u32, m.coverage)
                .expect("size")
                .save(output)
                .expect("save");
        }
    }
}
