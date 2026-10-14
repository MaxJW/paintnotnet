//! Effects rendered on the GPU (`effects.wgsl`): exact ports of the slowest CPU renderers,
//! so previews of whole-image blurs arrive in milliseconds instead of seconds. An effect
//! offers a [`GpuJob`] (`Renderer::gpu`); [`gpu_renderer`] wraps its CPU renderer so the
//! first tile asked for renders the whole bounds on the GPU and every tile copies from that.
//! Anything the GPU can't do (no adapter, an image over the buffer limits, a failed readback)
//! falls back to the CPU renderer, which gives the same pixels.

use std::sync::mpsc::channel;
use std::sync::{Arc, OnceLock};

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use pnn_core::{ColorBgra, Rect, Surface};
use pnn_effects::framework::{GpuJob, Renderer};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    w: u32,
    h: u32,
    bx: u32,
    by: u32,
    bw: u32,
    bh: u32,
    oy: u32,
    n: i32,
    a: i32,
    b: i32,
    c: i32,
    _pad: u32,
}

/// Samples per submission: long renders go in row bands so no single submission runs long
/// enough to trip the driver's GPU timeout.
const BAND_SAMPLES: u64 = 1 << 28;

pub struct GpuEffects {
    device: wgpu::Device,
    queue: wgpu::Queue,
    layout: wgpu::BindGroupLayout,
    motion: wgpu::ComputePipeline,
    zoom: wgpu::ComputePipeline,
    radial: wgpu::ComputePipeline,
}

impl GpuEffects {
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("effects.wgsl"));
        let buffer = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let storage = |read_only| wgpu::BufferBindingType::Storage { read_only };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu effect"),
            entries: &[
                buffer(0, storage(true)),
                buffer(1, storage(false)),
                buffer(2, wgpu::BufferBindingType::Uniform),
                buffer(3, storage(true)),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu effect"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            device: device.clone(),
            queue: queue.clone(),
            motion: pipeline("motion"),
            zoom: pipeline("zoom"),
            radial: pipeline("radial"),
            layout,
        }
    }

    /// `job` over `bounds` of `src` (row-major, `bounds.width` wide), or `None` if the GPU
    /// can't (the caller renders on the CPU instead).
    pub fn render(&self, job: &GpuJob, src: &Surface, bounds: Rect) -> Option<Vec<ColorBgra>> {
        let (w, h) = (src.width(), src.height());
        if bounds.width == 0
            || bounds.height == 0
            || bounds.x + bounds.width > w
            || bounds.y + bounds.height > h
        {
            return None;
        }
        let limits = self.device.limits();
        let max = limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size);
        let src_bytes = w as u64 * h as u64 * 4;
        let out_bytes = bounds.width as u64 * bounds.height as u64 * 4;
        let workgroups = limits.max_compute_workgroups_per_dimension as u64 * 16;
        if src_bytes > max || out_bytes > max || bounds.width as u64 > workgroups {
            return None;
        }
        let (pipeline, points, n, a, b, c, cost) = match job {
            GpuJob::Motion { points } => (
                &self.motion,
                points.as_slice(),
                points.len() as i32,
                0,
                0,
                0,
                points.len() as u64 * 4,
            ),
            GpuJob::Zoom { amount, fcx, fcy } => (&self.zoom, &[][..], 0, *amount, *fcx, *fcy, 65),
            GpuJob::Radial { n, fsr, fcx, fcy } => (
                &self.radial,
                &[][..],
                *n,
                *fsr,
                *fcx,
                *fcy,
                *n as u64 * 2 + 1,
            ),
        };
        use wgpu::util::DeviceExt;
        let pixels = src.to_pixels();
        let src_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gpu effect source"),
                contents: bytemuck::cast_slice(&pixels),
                usage: wgpu::BufferUsages::STORAGE,
            });
        drop(pixels);
        // A binding can't be empty.
        let points: &[[f32; 2]] = if points.is_empty() {
            &[[0.0; 2]]
        } else {
            points
        };
        let points_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gpu effect points"),
                contents: bytemuck::cast_slice(points),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let dst = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu effect output"),
            size: out_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu effect readback"),
            size: out_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let row_cost = bounds.width as u64 * cost.max(1);
        let band = (BAND_SAMPLES / row_cost).clamp(1, bounds.height as u64) as u32;
        let mut oy = 0;
        while oy < bounds.height {
            let bh = band.min(bounds.height - oy);
            let params = Params {
                w,
                h,
                bx: bounds.x,
                by: bounds.y + oy,
                bw: bounds.width,
                bh,
                oy,
                n,
                a,
                b,
                c,
                _pad: 0,
            };
            let uniform = crate::gpu::create_uniform_buffer(
                &self.device,
                "gpu effect params",
                bytemuck::bytes_of(&params),
            );
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu effect"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: src_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: dst.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: points_buf.as_entire_binding(),
                    },
                ],
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("gpu effect"),
                });
            {
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(bounds.width.div_ceil(16), bh.div_ceil(16), 1);
            }
            oy += bh;
            if oy == bounds.height {
                encoder.copy_buffer_to_buffer(&dst, 0, &readback, 0, out_bytes);
            }
            self.queue.submit([encoder.finish()]);
        }
        let (tx, rx) = channel();
        readback.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;
        let out = {
            let view = readback.get_mapped_range(..).ok()?;
            bytemuck::cast_slice::<u8, ColorBgra>(&view).to_vec()
        };
        readback.unmap();
        Some(out)
    }
}

/// `inner` rendered by the GPU when it has a GPU job for `src`; `inner` itself otherwise.
pub fn gpu_renderer(
    gpu: Option<&Arc<GpuEffects>>,
    inner: Box<dyn Renderer>,
    src: &Surface,
    bounds: Rect,
) -> Box<dyn Renderer> {
    match (gpu, inner.gpu(src)) {
        (Some(gpu), Some(job)) => Box::new(GpuRenderer {
            inner,
            gpu: gpu.clone(),
            job,
            bounds,
            done: OnceLock::new(),
        }),
        _ => inner,
    }
}

struct GpuRenderer {
    inner: Box<dyn Renderer>,
    gpu: Arc<GpuEffects>,
    job: GpuJob,
    bounds: Rect,
    /// The whole bounds, rendered on first use (`None`: the GPU failed; use the CPU).
    done: OnceLock<Option<Vec<ColorBgra>>>,
}

impl Renderer for GpuRenderer {
    fn render_row(&self, src: &Surface, x0: u32, y: u32, out: &mut [ColorBgra]) {
        let rect = Rect {
            x: x0,
            y,
            width: out.len() as u32,
            height: 1,
        };
        self.render_rect(src, rect, out);
    }

    fn render_rect(&self, src: &Surface, rect: Rect, out: &mut [ColorBgra]) {
        let b = self.bounds;
        let inside = rect.x >= b.x
            && rect.y >= b.y
            && rect.x + rect.width <= b.x + b.width
            && rect.y + rect.height <= b.y + b.height;
        let rendered = inside
            .then(|| {
                self.done
                    .get_or_init(|| self.gpu.render(&self.job, src, b))
                    .as_ref()
            })
            .flatten();
        let Some(pixels) = rendered else {
            return self.inner.render_rect(src, rect, out);
        };
        let w = rect.width as usize;
        for (j, row) in out.chunks_mut(w.max(1)).enumerate() {
            let start =
                (rect.y - b.y + j as u32) as usize * b.width as usize + (rect.x - b.x) as usize;
            row.copy_from_slice(&pixels[start..start + w]);
        }
    }

    fn gpu(&self, src: &Surface) -> Option<GpuJob> {
        self.inner.gpu(src)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnn_effects::framework::{Env, Value, render_rect};

    #[test]
    fn shader_validates() {
        let module =
            naga::front::wgsl::parse_str(include_str!("effects.wgsl")).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }

    fn gpu() -> Option<Arc<GpuEffects>> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        eprintln!("GPU effects test on {:?}", adapter.get_info().name);
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).ok()?;
        Some(Arc::new(GpuEffects::new(&device, &queue)))
    }

    /// A surface with every kind of pixel: varied colours, opaque, translucent, transparent.
    fn test_image(w: u32, h: u32) -> Surface {
        let mut s = Surface::new_transparent(w, h);
        let mut seed = 0x1234_5678u32;
        for y in 0..h {
            for x in 0..w {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let a = match (x / 16 + y / 16) % 4 {
                    0 => 255,
                    1 => 0,
                    _ => (seed >> 24) as u8,
                };
                let c = ColorBgra::from_bgra((x * 3) as u8, (seed >> 8) as u8, (y * 5) as u8, a);
                s.set(x, y, c);
            }
        }
        s
    }

    fn check(name: &str, values: Vec<Value>, src: &Surface, bounds: Rect, gpu: &Arc<GpuEffects>) {
        let id = pnn_effects::Category::ALL
            .into_iter()
            .flat_map(pnn_effects::effects_in)
            .find(|(_, e)| e.name == name)
            .expect("effect exists")
            .0;
        let def = pnn_effects::effect_def(id).expect("has a definition");
        let env = Env {
            primary: ColorBgra::BLACK,
            secondary: ColorBgra::WHITE,
            bounds,
            selection: None,
            layers: Vec::new(),
            active_layer: 0,
            selection_contours: Arc::new(Vec::new()),
        };
        let mut v = def.defaults_in(&env);
        for (i, x) in values.into_iter().enumerate() {
            v[i] = x;
        }
        let renderer = def.renderer(&v, src, &env);
        let job = renderer.gpu(src).expect("has a GPU job");
        let want = render_rect(renderer.as_ref(), src, bounds);
        let got = gpu.render(&job, src, bounds).expect("GPU renders");
        let diff = want.iter().zip(&got).filter(|(a, b)| a != b).count();
        assert_eq!(
            diff,
            0,
            "{name} {v:?}: {diff} of {} pixels differ",
            want.len()
        );
        // Through the wrapper, tile by tile on many threads, as the app renders.
        let wrapped = gpu_renderer(Some(gpu), def.renderer(&v, src, &env), src, bounds);
        assert!(
            render_rect(wrapped.as_ref(), src, bounds) == want,
            "{name}: wrapper"
        );
    }

    #[test]
    fn gpu_matches_cpu() {
        let Some(gpu) = gpu() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let src = test_image(301, 203);
        let full = Rect {
            x: 0,
            y: 0,
            width: 301,
            height: 203,
        };
        let part = Rect {
            x: 37,
            y: 21,
            width: 150,
            height: 99,
        };
        for bounds in [full, part] {
            for (angle, dist) in [(25.0, 10), (90.0, 1), (313.0, 60)] {
                for centered in [true, false] {
                    let v = vec![Value::Float(angle), Value::Bool(centered), Value::Int(dist)];
                    check("Motion Blur", v, &src, bounds, &gpu);
                }
            }
            for (amount, ox, oy) in [(10, 0.0, 0.0), (100, 0.5, -0.3), (1, -2.0, 2.0)] {
                let v = vec![Value::Int(amount), Value::Float(ox), Value::Float(oy)];
                check("Zoom Blur", v, &src, bounds, &gpu);
            }
            for (angle, ox, oy, q) in [
                (2.0, 0.0, 0.0, 2),
                (45.0, 0.25, -0.5, 4),
                (360.0, -2.0, 2.0, 1),
            ] {
                let v = vec![
                    Value::Float(angle),
                    Value::Float(ox),
                    Value::Float(oy),
                    Value::Int(q),
                ];
                check("Radial Blur", v, &src, bounds, &gpu);
            }
        }
    }
}
