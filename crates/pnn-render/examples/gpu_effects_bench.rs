//! Times the GPU effects (`effects.wgsl`) on a 4K image at 75% of their sliders (and the
//! fractals at their defaults too), against the CPU renderer they port. `cargo run --release -p pnn-render --example gpu_effects_bench`.

use std::sync::Arc;
use std::time::Instant;

use pnn_core::{ColorBgra, Rect, Surface};
use pnn_effects::framework::{Env, Value, render_rect};
use pnn_render::GpuEffects;

const W: u32 = 3840;
const H: u32 = 2160;

fn main() {
    let instance = wgpu::Instance::default();
    let adapter =
        pollster::block_on(instance.request_adapter(&Default::default())).expect("a GPU adapter");
    println!("{}", adapter.get_info().name);
    let (device, queue) =
        pollster::block_on(adapter.request_device(&Default::default())).expect("a device");
    let gpu = GpuEffects::new(&device, &queue);
    let mut src = Surface::new_transparent(W, H);
    let pixels: Vec<ColorBgra> = (0..W * H)
        .map(|i| {
            let (x, y) = (i % W, i / W);
            ColorBgra::from_rgba((x / 15) as u8, (y / 9) as u8, (x ^ y) as u8, 255)
        })
        .collect();
    src.write_pixels(&pixels);
    let bounds = Rect {
        x: 0,
        y: 0,
        width: W,
        height: H,
    };
    let env = Env {
        primary: ColorBgra::BLACK,
        secondary: ColorBgra::WHITE,
        bounds,
        selection: None,
        layers: Vec::new(),
        active_layer: 0,
        selection_contours: Arc::new(Vec::new()),
    };
    let cases: [(&str, Vec<Value>); 7] = [
        (
            "Motion Blur",
            vec![Value::Float(90.0), Value::Bool(true), Value::Int(150)],
        ),
        (
            "Zoom Blur",
            vec![Value::Int(75), Value::Float(0.0), Value::Float(0.0)],
        ),
        (
            "Radial Blur",
            vec![
                Value::Float(270.0),
                Value::Float(0.0),
                Value::Float(0.0),
                Value::Int(4),
            ],
        ),
        ("Julia Fractal", vec![]),
        (
            "Julia Fractal",
            vec![
                Value::Float(7.75),
                Value::Float(37.5),
                Value::Float(90.0),
                Value::Int(4),
            ],
        ),
        ("Mandelbrot Fractal", vec![]),
        (
            "Mandelbrot Fractal",
            vec![
                Value::Int(7),
                Value::Float(75.0),
                Value::Float(90.0),
                Value::Int(4),
            ],
        ),
    ];
    println!("| Effect | GPU | CPU |\n|---|---|---|");
    for (name, values) in cases {
        let id = pnn_effects::Category::ALL
            .into_iter()
            .flat_map(pnn_effects::effects_in)
            .find(|(_, e)| e.name == name)
            .expect("effect exists")
            .0;
        let def = pnn_effects::effect_def(id).expect("has a definition");
        let mut v = def.defaults_in(&env);
        v[..values.len()].copy_from_slice(&values);
        let renderer = def.renderer(&v, &src, &env);
        let job = renderer.gpu(&src).expect("a GPU job");
        // The first run compiles pipelines for the driver; time the second.
        gpu.render(&job, &src, bounds);
        let t = Instant::now();
        let got = gpu.render(&job, &src, bounds).expect("GPU renders");
        let gpu_time = t.elapsed();
        let t = Instant::now();
        let want = render_rect(renderer.as_ref(), &src, bounds);
        let cpu_time = t.elapsed();
        // The fractals are f32 on the GPU (agreed to differ at the sets' edges).
        let exact = !matches!(
            job,
            pnn_effects::framework::GpuJob::Julia { .. }
                | pnn_effects::framework::GpuJob::Mandelbrot { .. }
        );
        assert!(!exact || got == want, "{name}: GPU and CPU differ");
        println!("| {name} | {gpu_time:.2?} | {cpu_time:.2?} |");
    }
}
