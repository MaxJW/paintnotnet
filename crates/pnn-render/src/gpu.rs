//! GPU display of composite tiles.
//!
//! Each document gets textures in chunks of up to 4096×4096 px (16×16 tiles), so large images
//! need only a handful of draws. Composite tiles are converted on the CPU to premultiplied
//! BGRA with a 256→1 box-filtered mip chain (correct trilinear filtering when zoomed out),
//! then written into their chunk. The GPU only displays; it never composites.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use pnn_core::{ColorBgra, TILE_SIZE, Tile, TileCoord};

/// Chunk edge in tiles.
const CHUNK_TILES: u32 = 16;
const CHUNK_PX: u32 = CHUNK_TILES * TILE_SIZE;
/// Mip levels kept per tile: 256, 128, …, 1.
const TILE_MIPS: u32 = 9;

/// A composite tile prepared for upload: premultiplied BGRA bytes for each mip level.
pub struct DisplayTile {
    levels: Vec<Vec<u8>>,
}

/// Convert a composite tile for display. `None` (fully transparent) gives zeros.
pub fn display_tile(tile: Option<&Tile<ColorBgra>>) -> DisplayTile {
    let Some(tile) = tile else {
        return DisplayTile {
            levels: (0..TILE_MIPS)
                .map(|k| vec![0u8; ((TILE_SIZE >> k).pow(2) * 4) as usize])
                .collect(),
        };
    };
    let mut level0 = Vec::with_capacity((TILE_SIZE * TILE_SIZE * 4) as usize);
    for p in tile.pixels() {
        let a = p.a as u32;
        let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
        level0.extend_from_slice(&[pm(p.b), pm(p.g), pm(p.r), p.a]);
    }
    let mut levels = vec![level0];
    for k in 1..TILE_MIPS {
        let src_size = (TILE_SIZE >> (k - 1)) as usize;
        let size = src_size / 2;
        let src = &levels[k as usize - 1];
        let mut dst = vec![0u8; size * size * 4];
        for y in 0..size {
            for x in 0..size {
                for ch in 0..4 {
                    let at = |sx: usize, sy: usize| src[(sy * src_size + sx) * 4 + ch] as u32;
                    let sum = at(2 * x, 2 * y)
                        + at(2 * x + 1, 2 * y)
                        + at(2 * x, 2 * y + 1)
                        + at(2 * x + 1, 2 * y + 1);
                    dst[(y * size + x) * 4 + ch] = ((sum + 2) / 4) as u8;
                }
            }
        }
        levels.push(dst);
    }
    DisplayTile { levels }
}

/// Per-frame view parameters, laid out to match `View` in canvas.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ViewUniforms {
    pub origin: [f32; 2],
    pub zoom: f32,
    pub ppp: f32,
    pub viewport: [f32; 2],
    pub image_size: [f32; 2],
    pub grid: f32,
    pub srgb_target: f32,
    /// 1.0 = tint pixels outside the selection mask (feather preview).
    pub overlay: f32,
    pub _pad: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ChunkUniforms {
    origin: [f32; 2],
    size: [f32; 2],
}

struct Chunk {
    texture: wgpu::Texture,
    /// Selection coverage for the feather preview overlay (R8, one level).
    overlay: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
    mips: u32,
}

struct GpuCanvas {
    width: u32,
    height: u32,
    chunks_x: u32,
    chunks: Vec<Chunk>,
    view_buffer: wgpu::Buffer,
    view_bind_group: wgpu::BindGroup,
    /// View parameters for drawing into the glass backdrop.
    backdrop_buffer: wgpu::Buffer,
    backdrop_bind_group: wgpu::BindGroup,
}

/// Lives in egui-wgpu's `callback_resources`. One per window.
pub struct CanvasRenderer {
    pipeline: wgpu::RenderPipeline,
    view_layout: wgpu::BindGroupLayout,
    chunk_layout: wgpu::BindGroupLayout,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    srgb_target: bool,
    canvases: HashMap<u64, GpuCanvas>,
    pub(crate) glass: crate::glass::Glass,
}

impl CanvasRenderer {
    /// Create the renderer and register it with egui-wgpu.
    pub fn install(render_state: &egui_wgpu::RenderState) {
        let renderer = Self::new(&render_state.device, render_state.target_format);
        render_state
            .renderer
            .write()
            .callback_resources
            .insert(renderer);
    }

    fn new(device: &wgpu::Device, target_format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("canvas.wgsl"));
        let uniform_entry = |binding, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let sampler_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
            count: None,
        };
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas view"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT),
                sampler_entry(1),
                sampler_entry(2),
            ],
        });
        let chunk_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("canvas chunk"),
            entries: &[
                uniform_entry(0, wgpu::ShaderStages::VERTEX),
                texture_entry(1),
                texture_entry(2),
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("canvas"),
            bind_group_layouts: &[Some(&view_layout), Some(&chunk_layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("canvas"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: target_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = |label, filter: wgpu::FilterMode, mip: wgpu::MipmapFilterMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some(label),
                mag_filter: filter,
                min_filter: filter,
                mipmap_filter: mip,
                ..Default::default()
            })
        };
        Self {
            pipeline,
            nearest: sampler(
                "canvas nearest",
                wgpu::FilterMode::Nearest,
                wgpu::MipmapFilterMode::Nearest,
            ),
            linear: sampler(
                "canvas linear",
                wgpu::FilterMode::Linear,
                wgpu::MipmapFilterMode::Linear,
            ),
            view_layout,
            chunk_layout,
            srgb_target: target_format.is_srgb(),
            canvases: HashMap::new(),
            glass: crate::glass::Glass::new(device, target_format),
        }
    }

    pub fn srgb_target(&self) -> bool {
        self.srgb_target
    }

    /// Make sure textures exist for document `key` at this size. Returns true if they were
    /// (re)created, in which case every tile must be uploaded.
    pub fn ensure_canvas(
        &mut self,
        device: &wgpu::Device,
        key: u64,
        width: u32,
        height: u32,
    ) -> bool {
        if let Some(c) = self.canvases.get(&key)
            && (c.width, c.height) == (width, height)
        {
            return false;
        }
        let chunks_x = width.div_ceil(CHUNK_PX);
        let chunks_y = height.div_ceil(CHUNK_PX);
        let mut chunks = Vec::with_capacity((chunks_x * chunks_y) as usize);
        for cy in 0..chunks_y {
            for cx in 0..chunks_x {
                let (x0, y0) = (cx * CHUNK_PX, cy * CHUNK_PX);
                let (w, h) = (CHUNK_PX.min(width - x0), CHUNK_PX.min(height - y0));
                let mips = TILE_MIPS.min(32 - w.max(h).leading_zeros());
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("canvas chunk"),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: mips,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Bgra8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let uniforms = ChunkUniforms {
                    origin: [x0 as f32, y0 as f32],
                    size: [w as f32, h as f32],
                };
                let buffer =
                    create_uniform_buffer(device, "canvas chunk", bytemuck::bytes_of(&uniforms));
                let view = texture.create_view(&Default::default());
                let overlay = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("canvas overlay"),
                    size: wgpu::Extent3d {
                        width: w,
                        height: h,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::R8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let overlay_view = overlay.create_view(&Default::default());
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("canvas chunk"),
                    layout: &self.chunk_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&overlay_view),
                        },
                    ],
                });
                chunks.push(Chunk {
                    texture,
                    overlay,
                    bind_group,
                    width: w,
                    height: h,
                    mips,
                });
            }
        }
        let view_group = |label| {
            let buffer =
                create_uniform_buffer(device, label, bytemuck::bytes_of(&ViewUniforms::default()));
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.view_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.nearest),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Sampler(&self.linear),
                    },
                ],
            });
            (buffer, group)
        };
        let (view_buffer, view_bind_group) = view_group("canvas view");
        let (backdrop_buffer, backdrop_bind_group) = view_group("canvas backdrop view");
        self.canvases.insert(
            key,
            GpuCanvas {
                width,
                height,
                chunks_x,
                chunks,
                view_buffer,
                view_bind_group,
                backdrop_buffer,
                backdrop_bind_group,
            },
        );
        true
    }

    /// Write one composite tile (all its mip levels) into the document's textures.
    pub fn upload(&self, queue: &wgpu::Queue, key: u64, c: TileCoord, tile: &DisplayTile) {
        let Some(canvas) = self.canvases.get(&key) else {
            return;
        };
        let chunk =
            &canvas.chunks[((c.ty / CHUNK_TILES) * canvas.chunks_x + c.tx / CHUNK_TILES) as usize];
        let (lx, ly) = (
            (c.tx % CHUNK_TILES) * TILE_SIZE,
            (c.ty % CHUNK_TILES) * TILE_SIZE,
        );
        for k in 0..chunk.mips {
            let (level_w, level_h) = ((chunk.width >> k).max(1), (chunk.height >> k).max(1));
            let (x, y) = (lx >> k, ly >> k);
            if x >= level_w || y >= level_h {
                continue;
            }
            let stride = TILE_SIZE >> k;
            let (w, h) = (stride.min(level_w - x), stride.min(level_h - y));
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &chunk.texture,
                    mip_level: k,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &tile.levels[k as usize],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// Write one tile of selection coverage for the feather preview (`None` = unselected).
    pub fn upload_overlay(
        &self,
        queue: &wgpu::Queue,
        key: u64,
        c: TileCoord,
        coverage: Option<&[u8]>,
    ) {
        static ZERO: [u8; (TILE_SIZE * TILE_SIZE) as usize] = [0; (TILE_SIZE * TILE_SIZE) as usize];
        let Some(canvas) = self.canvases.get(&key) else {
            return;
        };
        let chunk =
            &canvas.chunks[((c.ty / CHUNK_TILES) * canvas.chunks_x + c.tx / CHUNK_TILES) as usize];
        let (x, y) = (
            (c.tx % CHUNK_TILES) * TILE_SIZE,
            (c.ty % CHUNK_TILES) * TILE_SIZE,
        );
        if x >= chunk.width || y >= chunk.height {
            return;
        }
        let (w, h) = (
            TILE_SIZE.min(chunk.width - x),
            TILE_SIZE.min(chunk.height - y),
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &chunk.overlay,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            coverage.unwrap_or(&ZERO),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TILE_SIZE),
                rows_per_image: Some(h),
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Free a closed document's textures.
    pub fn release(&mut self, key: u64) {
        self.canvases.remove(&key);
    }

    /// Draw document `key` at a quarter of its on-screen size over `surround` (sRGB 0–1) into
    /// the glass backdrop, and blur it.
    fn render_backdrop(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        key: u64,
        mut uniforms: ViewUniforms,
        surround: [f32; 4],
    ) {
        let Some(canvas) = self.canvases.get(&key) else {
            return;
        };
        let size = crate::glass::Glass::backdrop_size(uniforms.viewport);
        let s = 1.0 / crate::glass::DOWNSCALE as f32;
        uniforms.origin = uniforms.origin.map(|v| v * s);
        uniforms.zoom *= s;
        uniforms.ppp *= s;
        uniforms.viewport = size.map(|v| v as f32);
        uniforms.grid = 0.0;
        uniforms.overlay = 0.0;
        queue.write_buffer(&canvas.backdrop_buffer, 0, bytemuck::bytes_of(&uniforms));
        let lin = |c: f32| {
            if !self.srgb_target {
                c
            } else if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let clear = wgpu::Color {
            r: lin(surround[0]) as f64,
            g: lin(surround[1]) as f64,
            b: lin(surround[2]) as f64,
            a: 1.0,
        };
        let view = self.glass.ensure(device, size);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass backdrop"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(clear),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &canvas.backdrop_bind_group, &[]);
            for chunk in &canvas.chunks {
                pass.set_bind_group(1, &chunk.bind_group, &[]);
                pass.draw(0..6, 0..1);
            }
        }
        self.glass.blur(encoder);
    }
}

pub(crate) fn create_uniform_buffer(
    device: &wgpu::Device,
    label: &str,
    contents: &[u8],
) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}

/// egui paint callback that draws document `key` with `uniforms`.
pub struct CanvasCallback {
    pub key: u64,
    pub uniforms: ViewUniforms,
    /// Also draw the glass backdrop, over this surround colour (sRGB 0–1).
    pub backdrop: Option<[f32; 4]>,
}

impl egui_wgpu::CallbackTrait for CanvasCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<CanvasRenderer>()
            && let Some(canvas) = renderer.canvases.get(&self.key)
        {
            let mut uniforms = self.uniforms;
            uniforms.srgb_target = if renderer.srgb_target { 1.0 } else { 0.0 };
            queue.write_buffer(&canvas.view_buffer, 0, bytemuck::bytes_of(&uniforms));
            if let Some(surround) = self.backdrop {
                renderer.render_backdrop(device, queue, encoder, self.key, uniforms, surround);
            }
        }
        Vec::new()
    }

    fn paint(
        &self,
        _info: egui::PaintCallbackInfo,
        pass: &mut wgpu::RenderPass<'static>,
        resources: &egui_wgpu::CallbackResources,
    ) {
        let Some(renderer) = resources.get::<CanvasRenderer>() else {
            return;
        };
        let Some(canvas) = renderer.canvases.get(&self.key) else {
            return;
        };
        pass.set_pipeline(&renderer.pipeline);
        pass.set_bind_group(0, &canvas.view_bind_group, &[]);
        for chunk in &canvas.chunks {
            pass.set_bind_group(1, &chunk.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_validates() {
        let module =
            naga::front::wgsl::parse_str(include_str!("canvas.wgsl")).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }

    #[test]
    fn uniforms_match_wgsl_layout() {
        assert_eq!(std::mem::size_of::<ViewUniforms>(), 48);
        assert_eq!(std::mem::size_of::<ChunkUniforms>(), 16);
    }

    #[test]
    fn display_tile_premultiplies_and_mips() {
        let mut tile = Tile::filled(ColorBgra::from_rgba(255, 128, 0, 128));
        tile.set(0, 0, ColorBgra::TRANSPARENT);
        let d = display_tile(Some(&tile));
        assert_eq!(d.levels.len(), TILE_MIPS as usize);
        assert_eq!(d.levels[8].len(), 4);
        // pixel (1,0): b=0, g=128*128/255, r=128, a=128
        assert_eq!(&d.levels[0][4..8], &[0, 64, 128, 128]);
        assert_eq!(&d.levels[0][0..4], &[0, 0, 0, 0]);
        // 1×1 level averages the whole tile (one transparent pixel barely moves it)
        assert_eq!(d.levels[8][3], 128);
    }

    #[test]
    fn transparent_tile_is_all_zero() {
        let d = display_tile(None);
        assert!(d.levels.iter().all(|l| l.iter().all(|&b| b == 0)));
    }
}
