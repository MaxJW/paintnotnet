//! The GPU compositor: each layer's tiles live on the GPU, and changed tiles are composited
//! there (`composite.wgsl`) with the same integer blend maths as `pnn-core`'s CPU compositor,
//! which stays for saving and flattening. A change uploads only the tiles that changed in
//! the layers that changed; a property change (opacity, blend mode, visibility, order)
//! uploads nothing.
//!
//! Buffers are per chunk (16 × 16 tiles, the display textures' chunks) and hold the chunk's
//! tiles one after another, so a tile is one contiguous write.
//!
//! A layer can also be drawn with the live gradient over it ([`LayerInput::gradient`]): the
//! shader computes the gradient per pixel from the layer's own (pre-gradient) tiles, as
//! `pnn_tools::gradient::Gradient::render` does on the CPU, so dragging a gradient's nubs
//! costs no CPU rendering or uploads. Its f32 maths may differ from the CPU's by a level at
//! some pixels; the CPU renders the result into the document once the nubs stop.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use pnn_core::{BlendMode, ColorBgra, LayerId, Mask, Surface, TILE_SIZE, Tile, TileCoord};
use pnn_tools::gradient::GradientShader;

pub(crate) const CHUNK_TILES: u32 = 16;
const TILE_BYTES: u64 = (TILE_SIZE * TILE_SIZE * 4) as u64;

/// One layer of the stack, bottom first.
pub struct LayerInput<'a> {
    pub id: LayerId,
    /// Visible with opacity > 0.
    pub shown: bool,
    pub mode: BlendMode,
    pub opacity: u8,
    pub surface: &'a Surface,
    /// Draw the live gradient over `surface` (its pre-gradient pixels) instead.
    pub gradient: Option<LayerGradient<'a>>,
}

/// The live gradient on a layer: what to draw and the selection clipping it.
#[derive(Clone, Copy)]
pub struct LayerGradient<'a> {
    pub shader: GradientShader,
    pub selection: Option<&'a Arc<Mask>>,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    op: u32,
    mode: u32,
    opacity: u32,
    chunk_w: u32,
    tx0: u32,
    ty0: u32,
    _pad: [u32; 2],
}

/// `composite.wgsl`'s `Gradient`.
#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
struct GradientUniform {
    kind: u32,
    repeat: u32,
    flags: u32,
    op_mode: u32,
    start: [f32; 2],
    dtdx: f32,
    dtdy: f32,
    inv_len: f32,
    offset: f32,
    start_a: f32,
    end_a: f32,
    start_color: u32,
    end_color: u32,
    solid_color: u32,
    mask_absent: u32,
}

impl GradientUniform {
    fn new(g: &LayerGradient<'_>) -> Self {
        let s = &g.shader;
        let flags = s.solid as u32
            | (s.alpha_only as u32) << 1
            | (s.op.is_some() as u32) << 2
            | (g.selection.is_some() as u32) << 3;
        Self {
            kind: s.kind,
            repeat: s.repeat,
            flags,
            op_mode: s.op.map_or(0, mode_index),
            start: s.start,
            dtdx: s.dtdx,
            dtdy: s.dtdy,
            inv_len: s.inv_len,
            offset: s.offset,
            start_a: s.start_a as f32,
            end_a: s.end_a as f32,
            start_color: s.start_color.to_u32(),
            end_color: s.end_color.to_u32(),
            solid_color: s.solid_color.to_u32(),
            mask_absent: g.selection.map_or(0, |m| m.absent() as u32),
        }
    }
}

fn mode_index(m: BlendMode) -> u32 {
    BlendMode::ALL.iter().position(|&x| x == m).unwrap_or(0) as u32
}

/// `slots` flags for the gradient: the layer's / the selection's tile is absent.
const LAYER_ABSENT: u32 = 1 << 31;
const MASK_ABSENT: u32 = 1 << 30;

const CLEAR: u32 = 0;
const COPY: u32 = 1;
const BLEND: u32 = 2;
/// Added to COPY or BLEND: the layer is drawn with the live gradient.
const GRADIENT: u32 = 4;

pub(crate) struct GpuCompositor {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// Bound as the layer when clearing, and as the selection without one.
    dummy: wgpu::Buffer,
    /// Bound as the gradient when there is none.
    no_gradient: wgpu::Buffer,
}

impl GpuCompositor {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("composite.wgsl"));
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
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
            label: Some("composite"),
            entries: &[
                entry(0, storage(false)),
                entry(1, storage(true)),
                entry(2, storage(true)),
                entry(3, wgpu::BufferBindingType::Uniform),
                entry(4, wgpu::BufferBindingType::Uniform),
                entry(5, storage(true)),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("composite"),
            layout: Some(&pl),
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let dummy = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("composite dummy"),
            size: 16,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let no_gradient = crate::gpu::create_uniform_buffer(
            device,
            "no gradient",
            bytemuck::bytes_of(&GradientUniform::default()),
        );
        Self {
            pipeline,
            layout,
            dummy,
            no_gradient,
        }
    }

    /// Composite `dirty` tiles of `layers` into `store`'s accumulation buffers, uploading
    /// whatever layer tiles changed first. Returns the dirty tiles grouped by chunk, as
    /// (chunk index, [(tile, slot)]), for the display pass to pick up.
    pub(crate) fn composite(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        store: &mut LayerStore,
        layers: &[LayerInput<'_>],
        dirty: &[TileCoord],
    ) -> Vec<(usize, Vec<(TileCoord, u32)>)> {
        store.forget_missing(layers);
        let mut by_chunk: HashMap<usize, Vec<(TileCoord, u32)>> = HashMap::new();
        for &c in dirty {
            if c.tx < store.tiles_x && c.ty < store.tiles_y {
                let (chunk, slot) = store.locate(c);
                by_chunk.entry(chunk).or_default().push((c, slot));
            }
        }
        let shown: Vec<&LayerInput<'_>> = layers.iter().filter(|l| l.shown).collect();
        for l in &shown {
            store.upload(device, queue, l, dirty);
        }
        // 3.36: a visible, opaque, Normal bottom layer is taken as is.
        let copy_first = shown
            .first()
            .is_some_and(|l| l.opacity == 255 && l.mode == BlendMode::Normal);
        let gradient = shown.iter().find_map(|l| l.gradient.as_ref());
        if let Some(g) = gradient {
            store.upload_mask(device, queue, g.selection, dirty);
        }
        let uniform = gradient.map(|g| {
            crate::gpu::create_uniform_buffer(
                device,
                "gradient",
                bytemuck::bytes_of(&GradientUniform::new(g)),
            )
        });
        let mut chunks: Vec<_> = by_chunk.into_iter().collect();
        chunks.sort_by_key(|(i, _)| *i);
        for (chunk, tiles) in &chunks {
            let all: Vec<u32> = tiles.iter().map(|&(_, s)| s).collect();
            let none = Draw {
                chunk: *chunk,
                layer: None,
                gradient: None,
                op: CLEAR,
                mode: 0,
                opacity: 0,
            };
            self.dispatch(device, encoder, store, &none, &all);
            for (k, l) in shown.iter().enumerate() {
                let Some(layer) = store.layers.get(&l.id) else {
                    continue;
                };
                let base = if k == 0 && copy_first { COPY } else { BLEND };
                let draw = Draw {
                    chunk: *chunk,
                    layer: Some(l.id),
                    gradient: None,
                    op: base,
                    mode: mode_index(l.mode),
                    opacity: l.opacity as u32,
                };
                if let (Some(_), Some(uniform)) = (&l.gradient, &uniform) {
                    // The gradient covers absent tiles too.
                    let entries: Vec<u32> = tiles
                        .iter()
                        .map(|&(c, s)| {
                            let mut e = s;
                            if !layer.has(store.tiles_x, c) {
                                e |= LAYER_ABSENT;
                            }
                            if !store.mask_has(c) {
                                e |= MASK_ABSENT;
                            }
                            e
                        })
                        .collect();
                    let draw = Draw {
                        op: base + GRADIENT,
                        gradient: Some(uniform),
                        ..draw
                    };
                    self.dispatch(device, encoder, store, &draw, &entries);
                    continue;
                }
                // Absent tiles are skipped, as on the CPU.
                let present: Vec<u32> = tiles
                    .iter()
                    .filter(|(c, _)| layer.has(store.tiles_x, *c))
                    .map(|&(_, s)| s)
                    .collect();
                if !present.is_empty() {
                    self.dispatch(device, encoder, store, &draw, &present);
                }
            }
        }
        chunks
    }

    fn dispatch(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        store: &LayerStore,
        draw: &Draw<'_>,
        slots: &[u32],
    ) {
        use wgpu::util::DeviceExt;
        let chunk = draw.chunk;
        let layer_buf = draw
            .layer
            .and_then(|id| store.layers.get(&id))
            .and_then(|l| l.chunks[chunk].as_ref())
            .unwrap_or(&self.dummy);
        let mask_buf = store
            .mask
            .as_ref()
            .and_then(|m| m.chunks[chunk].as_ref())
            .unwrap_or(&self.dummy);
        let slots_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("composite slots"),
            contents: bytemuck::cast_slice(slots),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let (chunk_w, _) = store.chunk_tiles(chunk);
        let params = crate::gpu::create_uniform_buffer(
            device,
            "composite params",
            bytemuck::bytes_of(&Params {
                op: draw.op,
                mode: draw.mode,
                opacity: draw.opacity,
                chunk_w,
                tx0: (chunk as u32 % store.chunks_x) * CHUNK_TILES,
                ty0: (chunk as u32 / store.chunks_x) * CHUNK_TILES,
                _pad: [0; 2],
            }),
        );
        let buffers = [
            &store.acc[chunk],
            layer_buf,
            &slots_buf,
            &params,
            draw.gradient.unwrap_or(&self.no_gradient),
            mask_buf,
        ];
        let entries: Vec<wgpu::BindGroupEntry<'_>> = buffers
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite"),
            layout: &self.layout,
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(TILE_SIZE / 16, TILE_SIZE / 16, slots.len() as u32);
    }
}

/// One compositing dispatch.
#[derive(Clone, Copy)]
struct Draw<'a> {
    chunk: usize,
    layer: Option<LayerId>,
    gradient: Option<&'a wgpu::Buffer>,
    op: u32,
    mode: u32,
    opacity: u32,
}

/// The live gradient's selection on the GPU (one byte per pixel, laid out as the layers).
struct GpuMask {
    mask: Arc<Mask>,
    chunks: Vec<Option<wgpu::Buffer>>,
    /// Per tile: uploaded (or absent, nothing to upload).
    done: Vec<bool>,
}

/// One layer's tiles on the GPU.
struct GpuLayer {
    /// Per chunk, created when the layer first has a tile there.
    chunks: Vec<Option<wgpu::Buffer>>,
    /// Per tile, what was uploaded (held so a freed tile's address can't be mistaken for a
    /// new one); `None` = absent (transparent).
    uploaded: Vec<Option<Arc<Tile<ColorBgra>>>>,
}

impl GpuLayer {
    fn has(&self, tiles_x: u32, c: TileCoord) -> bool {
        self.uploaded[(c.ty * tiles_x + c.tx) as usize].is_some()
    }
}

/// A document's layers and composite on the GPU.
pub(crate) struct LayerStore {
    tiles_x: u32,
    tiles_y: u32,
    chunks_x: u32,
    /// Per chunk: the composite, unpremultiplied (what the display pass reads).
    pub(crate) acc: Vec<wgpu::Buffer>,
    layers: HashMap<LayerId, GpuLayer>,
    mask: Option<GpuMask>,
}

impl LayerStore {
    pub(crate) fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let tiles_x = width.div_ceil(TILE_SIZE);
        let tiles_y = height.div_ceil(TILE_SIZE);
        let chunks_x = tiles_x.div_ceil(CHUNK_TILES);
        let chunks_y = tiles_y.div_ceil(CHUNK_TILES);
        let mut store = Self {
            tiles_x,
            tiles_y,
            chunks_x,
            acc: Vec::new(),
            layers: HashMap::new(),
            mask: None,
        };
        store.acc = (0..chunks_x * chunks_y)
            .map(|i| store.chunk_buffer(device, i as usize, "composite"))
            .collect();
        store
    }

    /// Tiles across and down chunk `i`.
    fn chunk_tiles(&self, i: usize) -> (u32, u32) {
        let (cx, cy) = (i as u32 % self.chunks_x, i as u32 / self.chunks_x);
        (
            CHUNK_TILES.min(self.tiles_x - cx * CHUNK_TILES),
            CHUNK_TILES.min(self.tiles_y - cy * CHUNK_TILES),
        )
    }

    fn chunk_buffer(&self, device: &wgpu::Device, i: usize, label: &str) -> wgpu::Buffer {
        self.sized_chunk_buffer(device, i, label, TILE_BYTES)
    }

    fn sized_chunk_buffer(
        &self,
        device: &wgpu::Device,
        i: usize,
        label: &str,
        tile_bytes: u64,
    ) -> wgpu::Buffer {
        let (w, h) = self.chunk_tiles(i);
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: (w * h) as u64 * tile_bytes,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    }

    /// (chunk index, slot within the chunk) of tile `c`.
    pub(crate) fn locate(&self, c: TileCoord) -> (usize, u32) {
        let (cx, cy) = (c.tx / CHUNK_TILES, c.ty / CHUNK_TILES);
        let chunk = (cy * self.chunks_x + cx) as usize;
        let (w, _) = self.chunk_tiles(chunk);
        (chunk, (c.ty % CHUNK_TILES) * w + c.tx % CHUNK_TILES)
    }

    /// Whether the live gradient's selection has a tile at `c`.
    fn mask_has(&self, c: TileCoord) -> bool {
        self.mask.as_ref().is_some_and(|m| m.mask.tile(c).is_some())
    }

    /// Upload `selection`'s tiles among `tiles` (once per selection).
    fn upload_mask(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        selection: Option<&Arc<Mask>>,
        tiles: &[TileCoord],
    ) {
        let Some(sel) = selection else {
            self.mask = None;
            return;
        };
        if !self
            .mask
            .as_ref()
            .is_some_and(|m| Arc::ptr_eq(&m.mask, sel))
        {
            self.mask = Some(GpuMask {
                mask: sel.clone(),
                chunks: (0..self.acc.len()).map(|_| None).collect(),
                done: vec![false; (self.tiles_x * self.tiles_y) as usize],
            });
        }
        let mask_bytes = TILE_BYTES / 4;
        for &c in tiles {
            if c.tx >= self.tiles_x || c.ty >= self.tiles_y {
                continue;
            }
            let i = (c.ty * self.tiles_x + c.tx) as usize;
            let (chunk, slot) = self.locate(c);
            let m = self.mask.as_ref().expect("set above");
            if m.done[i] {
                continue;
            }
            let tile = m.mask.tile(c).cloned();
            let new_buffer = (tile.is_some() && m.chunks[chunk].is_none())
                .then(|| self.sized_chunk_buffer(device, chunk, "gradient selection", mask_bytes));
            let m = self.mask.as_mut().expect("set above");
            if new_buffer.is_some() {
                m.chunks[chunk] = new_buffer;
            }
            if let (Some(t), Some(buffer)) = (tile, &m.chunks[chunk]) {
                queue.write_buffer(buffer, slot as u64 * mask_bytes, t.pixels());
            }
            m.done[i] = true;
        }
    }

    /// Drop layers no longer in the document.
    fn forget_missing(&mut self, layers: &[LayerInput<'_>]) {
        self.layers
            .retain(|id, _| layers.iter().any(|l| l.id == *id));
    }

    /// Upload `layer`'s tiles among `tiles` that differ from what the GPU has.
    fn upload(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layer: &LayerInput<'_>,
        tiles: &[TileCoord],
    ) {
        let n = (self.tiles_x * self.tiles_y) as usize;
        let chunks = self.acc.len();
        self.layers.entry(layer.id).or_insert_with(|| GpuLayer {
            chunks: (0..chunks).map(|_| None).collect(),
            uploaded: vec![None; n],
        });
        for &c in tiles {
            if c.tx >= self.tiles_x || c.ty >= self.tiles_y {
                continue;
            }
            let i = (c.ty * self.tiles_x + c.tx) as usize;
            let tile = layer.surface.tile(c);
            let gpu = &self.layers[&layer.id];
            let same = match (tile, &gpu.uploaded[i]) {
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            };
            if same {
                continue;
            }
            let (chunk, slot) = self.locate(c);
            let new_buffer = (tile.is_some() && gpu.chunks[chunk].is_none())
                .then(|| self.chunk_buffer(device, chunk, "layer tiles"));
            let gpu = self.layers.get_mut(&layer.id).expect("inserted");
            if new_buffer.is_some() {
                gpu.chunks[chunk] = new_buffer;
            }
            if let (Some(t), Some(buffer)) = (tile, &gpu.chunks[chunk]) {
                queue.write_buffer(
                    buffer,
                    slot as u64 * TILE_BYTES,
                    bytemuck::cast_slice(t.pixels()),
                );
            }
            gpu.uploaded[i] = tile.cloned();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnn_core::{Document, Layer, Rect};

    #[test]
    fn shader_validates() {
        let module =
            naga::front::wgsl::parse_str(include_str!("composite.wgsl")).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }

    fn device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).ok()?;
        pollster::block_on(adapter.request_device(&Default::default())).ok()
    }

    /// The composite of every tile, read back.
    fn read_back(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        store: &LayerStore,
    ) -> HashMap<(u32, u32), Vec<ColorBgra>> {
        let mut out = HashMap::new();
        for (chunk, acc) in store.acc.iter().enumerate() {
            let size = acc.size();
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.copy_buffer_to_buffer(acc, 0, &readback, 0, size);
            queue.submit([encoder.finish()]);
            readback.map_async(wgpu::MapMode::Read, .., |r| r.expect("maps"));
            device
                .poll(wgpu::PollType::wait_indefinitely())
                .expect("polls");
            let data: Vec<ColorBgra> =
                bytemuck::cast_slice(&readback.get_mapped_range(..).expect("mapped")).to_vec();
            for ty in 0..store.tiles_y {
                for tx in 0..store.tiles_x {
                    let c = TileCoord { tx, ty };
                    let (k, slot) = store.locate(c);
                    if k == chunk {
                        let start = slot as usize * (TILE_SIZE * TILE_SIZE) as usize;
                        out.insert(
                            (tx, ty),
                            data[start..start + (TILE_SIZE * TILE_SIZE) as usize].to_vec(),
                        );
                    }
                }
            }
        }
        out
    }

    fn noise_layer(w: u32, h: u32, seed: u32, holes: bool) -> Surface {
        let mut s = Surface::new_transparent(w, h);
        let mut v = seed | 1;
        for y in 0..h {
            for x in 0..w {
                // Leave some tiles absent.
                if holes && ((x / TILE_SIZE) + (y / TILE_SIZE) + seed).is_multiple_of(3) {
                    continue;
                }
                v ^= v << 13;
                v ^= v >> 17;
                v ^= v << 5;
                let a = match (x / 7 + y / 5 + seed) % 4 {
                    0 => 255,
                    1 => 0,
                    _ => (v >> 24) as u8,
                };
                s.set(
                    x,
                    y,
                    ColorBgra::from_u32((v & 0x00ff_ffff) | ((a as u32) << 24)),
                );
            }
        }
        s
    }

    fn inputs<'a>(
        doc: &'a Document,
        gradient: Option<(usize, LayerGradient<'a>)>,
    ) -> Vec<LayerInput<'a>> {
        doc.layers()
            .iter()
            .enumerate()
            .map(|(i, l)| LayerInput {
                id: l.id(),
                shown: l.visible && l.opacity > 0,
                mode: l.blend_mode,
                opacity: l.opacity,
                surface: &l.surface,
                gradient: gradient.filter(|(k, _)| *k == i).map(|(_, g)| g),
            })
            .collect()
    }

    fn check(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        comp: &GpuCompositor,
        store: &mut LayerStore,
        doc: &Document,
        dirty: &[TileCoord],
        what: &str,
    ) {
        let mut encoder = device.create_command_encoder(&Default::default());
        comp.composite(
            device,
            queue,
            &mut encoder,
            store,
            &inputs(doc, None),
            dirty,
        );
        queue.submit([encoder.finish()]);
        let got = read_back(device, queue, store);
        let want = pnn_core::compositor::flatten(doc);
        for ty in 0..store.tiles_y {
            for tx in 0..store.tiles_x {
                let c = TileCoord { tx, ty };
                let w: Vec<ColorBgra> = want.tile(c).map_or_else(
                    || vec![ColorBgra::TRANSPARENT; (TILE_SIZE * TILE_SIZE) as usize],
                    |t| t.pixels().to_vec(),
                );
                let g = &got[&(tx, ty)];
                let diff = w.iter().zip(g).filter(|(a, b)| a != b).count();
                assert_eq!(diff, 0, "{what}: tile {tx},{ty}: {diff} pixels differ");
            }
        }
    }

    #[test]
    fn gpu_composite_matches_cpu() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let comp = GpuCompositor::new(&device);
        // 4400 px wide: two chunks across, edge tiles part-filled.
        let (w, h): (u32, u32) = (4400, 300);
        let all: Vec<TileCoord> = (0..h.div_ceil(TILE_SIZE))
            .flat_map(|ty| (0..w.div_ceil(TILE_SIZE)).map(move |tx| TileCoord { tx, ty }))
            .collect();
        for (i, &mode) in BlendMode::ALL.iter().enumerate() {
            for (bottom_opacity, opacity) in [(255, 255), (255, 97), (180, 255)] {
                let mut bottom = Layer::new("Background", noise_layer(w, h, 11, false));
                bottom.opacity = bottom_opacity;
                let mut top = Layer::new("Top", noise_layer(w, h, 23 + i as u32, true));
                top.blend_mode = mode;
                top.opacity = opacity;
                let mut hidden = Layer::new("Hidden", noise_layer(w, h, 5, false));
                hidden.visible = false;
                let doc = Document::from_layers(w, h, vec![bottom, hidden, top]);
                let mut store = LayerStore::new(&device, w, h);
                let what = format!("{mode:?} bottom {bottom_opacity} top {opacity}");
                check(&device, &queue, &comp, &mut store, &doc, &all, &what);
            }
        }
    }

    #[test]
    fn gpu_composite_updates_changed_tiles() {
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let comp = GpuCompositor::new(&device);
        let (w, h) = (700, 600);
        let mut doc = Document::from_layers(
            w,
            h,
            vec![
                Layer::new("Background", noise_layer(w, h, 3, false)),
                Layer::new("Top", noise_layer(w, h, 8, true)),
            ],
        );
        let tiles = |d: &Document| -> Vec<TileCoord> {
            (0..d.tiles_y())
                .flat_map(|ty| (0..d.tiles_x()).map(move |tx| TileCoord { tx, ty }))
                .collect()
        };
        let mut store = LayerStore::new(&device, w, h);
        let mut seen: Vec<u64> = vec![0; (doc.tiles_x() * doc.tiles_y()) as usize];
        let mut step = |doc: &Document, store: &mut LayerStore, what: &str| {
            let dirty: Vec<TileCoord> = tiles(doc)
                .into_iter()
                .filter(|&c| doc.tile_revision(c) != seen[(c.ty * doc.tiles_x() + c.tx) as usize])
                .collect();
            for &c in &dirty {
                seen[(c.ty * doc.tiles_x() + c.tx) as usize] = doc.tile_revision(c);
            }
            check(&device, &queue, &comp, store, doc, &dirty, what);
        };
        step(&doc, &mut store, "first");
        let r = Rect {
            x: 300,
            y: 260,
            width: 20,
            height: 20,
        };
        doc.modify_layer(1, Some(r), |l| {
            for y in 260..280 {
                for x in 300..320 {
                    l.surface.set(x, y, ColorBgra::from_rgba(9, 200, 30, 140));
                }
            }
        });
        step(&doc, &mut store, "painted");
        doc.modify_layer(1, None, |l| l.blend_mode = BlendMode::Difference);
        step(&doc, &mut store, "blend mode");
        doc.move_layer(1, 0);
        step(&doc, &mut store, "reordered");
        doc.modify_layer(1, None, |l| l.visible = false);
        step(&doc, &mut store, "hidden");
        doc.remove_layer(1);
        step(&doc, &mut store, "removed");
    }

    /// The live gradient drawn by the shader against `Gradient::render` on the CPU, for
    /// every type, repeat mode and way of landing on the layer, with and without a soft
    /// selection, over a layer with absent tiles.
    #[test]
    fn gpu_gradient_matches_cpu() {
        use pnn_tools::gradient::{Gradient, GradientOp};
        use pnn_tools::{GradientRepeat, GradientType};
        let Some((device, queue)) = device() else {
            eprintln!("no GPU adapter: skipped");
            return;
        };
        let comp = GpuCompositor::new(&device);
        let (w, h): (u32, u32) = (600, 520);
        let doc = Document::from_layers(
            w,
            h,
            vec![
                Layer::new("Background", noise_layer(w, h, 3, false)),
                Layer::new("Top", noise_layer(w, h, 8, true)),
            ],
        );
        // A soft ellipse; tiles far outside it are absent.
        let mut mask: Mask = pnn_core::TiledBuffer::new(w, h, 0);
        for y in 0..h {
            for x in 0..w {
                let (dx, dy) = ((x as f32 - 260.0) / 200.0, (y as f32 - 230.0) / 150.0);
                let d = (dx * dx + dy * dy).sqrt();
                mask.set(x, y, ((1.2 - d) * 5.0 * 255.0).clamp(0.0, 255.0) as u8);
            }
        }
        mask.compact();
        let mask = Arc::new(mask);
        let all: Vec<TileCoord> = (0..doc.tiles_y())
            .flat_map(|ty| (0..doc.tiles_x()).map(move |tx| TileCoord { tx, ty }))
            .collect();
        let full = Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        let (mut worst, mut worst_case) = (0.0f64, String::new());
        for kind in GradientType::ALL {
            for repeat in [
                GradientRepeat::NoRepeat,
                GradientRepeat::Wrapped,
                GradientRepeat::Reflected,
            ] {
                for op in [
                    GradientOp::Overwrite,
                    GradientOp::Blend(BlendMode::Normal),
                    GradientOp::Blend(BlendMode::Multiply),
                ] {
                    for alpha_only in [false, true] {
                        for selection in [None, Some(&mask)] {
                            let g = Gradient {
                                kind,
                                repeat,
                                start: [130.5, 90.25],
                                end: [430.0, 333.0],
                                start_color: ColorBgra::from_rgba(250, 30, 10, 255),
                                end_color: ColorBgra::from_rgba(20, 90, 240, 120),
                                alpha_only,
                            };
                            let mut cpu = doc.clone();
                            let before = doc.layer(1).surface.clone();
                            cpu.modify_layer(1, None, |l| {
                                g.render(&mut l.surface, &before, selection.map(|m| &**m), full, op)
                            });
                            let want = pnn_core::compositor::flatten(&cpu);
                            let mut store = LayerStore::new(&device, w, h);
                            let lg = LayerGradient {
                                shader: g.shader(op),
                                selection,
                            };
                            let mut encoder = device.create_command_encoder(&Default::default());
                            comp.composite(
                                &device,
                                &queue,
                                &mut encoder,
                                &mut store,
                                &inputs(&doc, Some((1, lg))),
                                &all,
                            );
                            queue.submit([encoder.finish()]);
                            let got = read_back(&device, &queue, &store);
                            let (mut off, mut far) = (0usize, 0usize);
                            for y in 0..h {
                                for x in 0..w {
                                    let c = TileCoord {
                                        tx: x / TILE_SIZE,
                                        ty: y / TILE_SIZE,
                                    };
                                    let a = want.get(x, y);
                                    let b = got[&(c.tx, c.ty)]
                                        [((y % TILE_SIZE) * TILE_SIZE + x % TILE_SIZE) as usize];
                                    let d = [
                                        a.b.abs_diff(b.b),
                                        a.g.abs_diff(b.g),
                                        a.r.abs_diff(b.r),
                                        a.a.abs_diff(b.a),
                                    ];
                                    let m = *d.iter().max().expect("4 channels");
                                    off += (m > 0) as usize;
                                    far += (m > 2) as usize;
                                }
                            }
                            let n = (w * h) as f64;
                            let what = format!(
                                "{kind:?} {repeat:?} {op:?} alpha_only={alpha_only} selection={}",
                                selection.is_some()
                            );
                            // Allowed: a level or two where f32 rounding moves the dither or a
                            // table step (RTX 3070: at most 51 of 312 000 pixels, none by more
                            // than 2), and a few pixels on a repeat's seam (where the position
                            // wraps from 1 to 0, a hair's difference flips the colour).
                            assert!(off as f64 / n < 0.005, "{what}: {off} pixels differ");
                            assert!(
                                far as f64 / n < 0.0005,
                                "{what}: {far} pixels differ by > 2"
                            );
                            if off as f64 / n > worst {
                                (worst, worst_case) =
                                    (off as f64 / n, format!("{what}: {off} off, {far} by > 2"));
                            }
                        }
                    }
                }
            }
        }
        eprintln!("worst case: {worst_case}");
    }
}
