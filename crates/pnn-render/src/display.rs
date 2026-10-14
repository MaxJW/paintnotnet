//! Composited tiles to the screen: compute passes premultiply each tile of a chunk's
//! composite (`layers`) into the chunk's display texture level 0 and build its mip chain
//! (`tiles.wgsl`). The CPU used to build the 9-level premultiplied chain itself and upload
//! every level: about 5 ms of CPU and 44 MB of writes per frame when a 4K image changed
//! everywhere.

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;
use pnn_core::TILE_SIZE;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TileOrigin {
    x: u32,
    y: u32,
    slot: u32,
    _pad: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Level {
    k: u32,
    _pad: [u32; 3],
}

/// One tile to show: where it goes in its chunk (level-0 texels) and its slot in the
/// chunk's composite buffer.
pub(crate) struct TileJob {
    pub x: u32,
    pub y: u32,
    pub slot: u32,
}

pub(crate) struct TileDisplay {
    premultiply: wgpu::ComputePipeline,
    downsample: wgpu::ComputePipeline,
    layout0: wgpu::BindGroupLayout,
    layout_mip: wgpu::BindGroupLayout,
    /// One uniform per mip level (level 1..=8).
    levels: Vec<wgpu::Buffer>,
}

impl TileDisplay {
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
            levels,
        }
    }

    /// Show `jobs` (all in one chunk) from `composite`, the chunk's composite buffer, in
    /// `mip_views` (one single-level view per mip of the chunk's texture).
    pub(crate) fn display(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        mip_views: &[wgpu::TextureView],
        composite: &wgpu::Buffer,
        jobs: &[TileJob],
    ) {
        use wgpu::util::DeviceExt;
        if jobs.is_empty() {
            return;
        }
        let origins: Vec<TileOrigin> = jobs
            .iter()
            .map(|j| TileOrigin {
                x: j.x,
                y: j.y,
                slot: j.slot,
                _pad: 0,
            })
            .collect();
        let origins = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("tile origins"),
            contents: bytemuck::cast_slice(&origins),
            usage: wgpu::BufferUsages::STORAGE,
        });
        {
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("tile premultiply"),
                layout: &self.layout0,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: composite.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: origins.as_entire_binding(),
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
            pass.dispatch_workgroups(TILE_SIZE / 16, TILE_SIZE / 16, jobs.len() as u32);
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
                        resource: origins.as_entire_binding(),
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
            pass.dispatch_workgroups(stride.div_ceil(16), stride.div_ceil(16), jobs.len() as u32);
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
