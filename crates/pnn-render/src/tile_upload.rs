//! Changed composite tiles go to the GPU as they are; compute passes premultiply them into a
//! chunk's level 0 and build its mip chain (`tiles.wgsl`). The CPU used to build the 9-level
//! premultiplied chain itself (`display_tile`) and upload every level: about 5 ms of CPU and
//! 44 MB of writes per frame when a 4K image changes everywhere (a live gradient or effect
//! preview); now it uploads 33 MB of raw tiles and does no per-pixel work.

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use pnn_core::{ColorBgra, TILE_SIZE, Tile};

/// Tiles per dispatch (the raw buffer holds this many: 16 MB).
const BATCH: usize = 64;
const TILE_TEXELS: u64 = (TILE_SIZE * TILE_SIZE) as u64;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TileOrigin {
    x: u32,
    y: u32,
    empty: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Level {
    k: u32,
    _pad: [u32; 3],
}

pub(crate) struct TileUploader {
    premultiply: wgpu::ComputePipeline,
    downsample: wgpu::ComputePipeline,
    layout0: wgpu::BindGroupLayout,
    layout_mip: wgpu::BindGroupLayout,
    raw: wgpu::Buffer,
    origins: wgpu::Buffer,
    /// One uniform per mip level (level 1..=8).
    levels: Vec<wgpu::Buffer>,
}

/// One tile to show: where it goes in its chunk (level-0 texels) and its pixels (`None`:
/// fully transparent).
pub(crate) struct TileJob<'a> {
    pub x: u32,
    pub y: u32,
    pub tile: Option<&'a Tile<ColorBgra>>,
}

impl TileUploader {
    pub(crate) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("tiles.wgsl"));
        let storage = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let target = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::StorageTexture {
                access: wgpu::StorageTextureAccess::WriteOnly,
                format: wgpu::TextureFormat::Rgba8Unorm,
                view_dimension: wgpu::TextureViewDimension::D2,
            },
            count: None,
        };
        let layout0 = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tile premultiply"),
            entries: &[storage(0), storage(1), target(2)],
        });
        let layout_mip = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tile downsample"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                storage(4),
                target(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline = |label, layout: &wgpu::BindGroupLayout, entry| {
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                ..Default::default()
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pl),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let premultiply = pipeline("tile premultiply", &layout0, "premultiply");
        let downsample = pipeline("tile downsample", &layout_mip, "downsample");
        let raw = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("raw tiles"),
            size: BATCH as u64 * TILE_TEXELS * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let origins = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tile origins"),
            size: (BATCH * std::mem::size_of::<TileOrigin>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let levels = (1..9u32)
            .map(|k| {
                crate::gpu::create_uniform_buffer(
                    device,
                    "tile mip level",
                    bytemuck::bytes_of(&Level { k, _pad: [0; 3] }),
                )
            })
            .collect();
        Self {
            premultiply,
            downsample,
            layout0,
            layout_mip,
            raw,
            origins,
            levels,
        }
    }

    /// Upload `jobs` (all in one chunk) and build their levels in `mip_views` (one
    /// single-level view per mip of the chunk's texture).
    pub(crate) fn upload(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        mip_views: &[wgpu::TextureView],
        jobs: &[TileJob<'_>],
    ) {
        for batch in jobs.chunks(BATCH) {
            let origins: Vec<TileOrigin> = batch
                .iter()
                .map(|j| TileOrigin {
                    x: j.x,
                    y: j.y,
                    empty: j.tile.is_none() as u32,
                    _pad: 0,
                })
                .collect();
            queue.write_buffer(&self.origins, 0, bytemuck::cast_slice(&origins));
            for (i, j) in batch.iter().enumerate() {
                if let Some(t) = j.tile {
                    queue.write_buffer(
                        &self.raw,
                        i as u64 * TILE_TEXELS * 4,
                        bytemuck::cast_slice(t.pixels()),
                    );
                }
            }
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("tile upload"),
            });
            {
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("tile premultiply"),
                    layout: &self.layout0,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: self.raw.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: self.origins.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&mip_views[0]),
                        },
                    ],
                });
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.premultiply);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(TILE_SIZE / 16, TILE_SIZE / 16, batch.len() as u32);
            }
            for k in 1..mip_views.len() {
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("tile downsample"),
                    layout: &self.layout_mip,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&mip_views[k - 1]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: self.origins.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 5,
                            resource: wgpu::BindingResource::TextureView(&mip_views[k]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 6,
                            resource: self.levels[k - 1].as_entire_binding(),
                        },
                    ],
                });
                let stride = (TILE_SIZE >> k).max(1);
                let mut pass = encoder.begin_compute_pass(&Default::default());
                pass.set_pipeline(&self.downsample);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(
                    stride.div_ceil(16),
                    stride.div_ceil(16),
                    batch.len() as u32,
                );
            }
            queue.submit([encoder.finish()]);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("tiles.wgsl")).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }
}
