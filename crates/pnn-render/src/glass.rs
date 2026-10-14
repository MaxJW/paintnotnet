//! Frosted glass (design A): panels and dialogs show the canvas blurred behind them.
//!
//! Each frame the canvas callback draws the image a second time, at 1/[`DOWNSCALE`] of its
//! on-screen size, into the backdrop texture, then blurs it with separable Gaussian passes.
//! A [`GlassCallback`] paints a rounded rectangle of that blur, slightly more saturated and
//! mixed with the panel's tint, behind a panel's contents. Everything happens on the GPU in
//! the frame's own command encoder, before egui's render pass.

use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};
use egui_wgpu::wgpu;

use crate::CanvasRenderer;
use crate::gpu::create_uniform_buffer;

/// The backdrop is drawn at 1/DOWNSCALE of the canvas area's size.
pub const DOWNSCALE: u32 = 4;
/// Blur iterations (each a horizontal and a vertical pass); the tap step grows each time.
const BLUR_STEPS: [f32; 2] = [1.0, 2.0];

/// Parameters of one glass shape, laid out to match `Params` in glass.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GlassUniforms {
    /// Shape: min x, min y, max x, max y in framebuffer px.
    pub rect: [f32; 4],
    /// Canvas area the backdrop covers: min x, min y, width, height in framebuffer px.
    pub area: [f32; 4],
    /// Tint as sRGB 0–1, and how much of it covers the blur.
    pub tint: [f32; 4],
    /// Corner radius in px.
    pub radius: f32,
    /// 1 = unchanged colours.
    pub saturation: f32,
    /// Filled in by the renderer.
    pub srgb_target: f32,
    /// 0–1, for panels fading in and out.
    pub opacity: f32,
}

struct Pass {
    bind_group: wgpu::BindGroup,
    /// Render into the second texture (else the first).
    into_b: bool,
}

struct Targets {
    size: [u32; 2],
    a: wgpu::TextureView,
    b: wgpu::TextureView,
    passes: Vec<Pass>,
}

struct Shape {
    buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    generation: u64,
}

pub(crate) struct Glass {
    layout: wgpu::BindGroupLayout,
    blur: wgpu::RenderPipeline,
    glass: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    format: wgpu::TextureFormat,
    srgb_target: bool,
    targets: Option<Targets>,
    /// Bumped whenever the targets are recreated (shapes rebuild their bind groups).
    generation: u64,
    /// The backdrop has been drawn (glass paints nothing before). The app only adds glass
    /// shapes in frames where the canvas, and so the backdrop, was drawn.
    ready: bool,
    shapes: HashMap<u64, Shape>,
}

impl Glass {
    pub(crate) fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let shader = device.create_shader_module(wgpu::include_wgsl!("glass.wgsl"));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("glass"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = |label, entry, blend| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_fullscreen"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let blur = pipeline("glass blur", "fs_blur", None);
        let glass = pipeline(
            "glass",
            "fs_glass",
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("glass"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            ..Default::default()
        });
        Self {
            layout,
            blur,
            glass,
            sampler,
            format,
            srgb_target: format.is_srgb(),
            targets: None,
            generation: 0,
            ready: false,
            shapes: HashMap::new(),
        }
    }

    fn bind_group(
        &self,
        device: &wgpu::Device,
        buffer: &wgpu::Buffer,
        source: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(source),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        })
    }

    /// Backdrop texture size for a canvas area of `area_px` framebuffer pixels.
    pub(crate) fn backdrop_size(area_px: [f32; 2]) -> [u32; 2] {
        area_px.map(|v| (v.max(1.0) as u32).div_ceil(DOWNSCALE).max(1))
    }

    /// Make sure the backdrop textures are `size`; returns the view to draw the canvas into.
    pub(crate) fn ensure(&mut self, device: &wgpu::Device, size: [u32; 2]) -> &wgpu::TextureView {
        if self.targets.as_ref().is_none_or(|t| t.size != size) {
            let texture = |label| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d {
                            width: size[0],
                            height: size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: self.format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
            };
            let (a, b) = (texture("glass backdrop a"), texture("glass backdrop b"));
            let mut passes = Vec::new();
            for step in BLUR_STEPS {
                for (dir, into_b) in [([step, 0.0], true), ([0.0, step], false)] {
                    let uniforms = GlassUniforms {
                        rect: [dir[0], dir[1], 0.0, 0.0],
                        ..Default::default()
                    };
                    let buffer =
                        create_uniform_buffer(device, "glass blur", bytemuck::bytes_of(&uniforms));
                    let source = if into_b { &a } else { &b };
                    passes.push(Pass {
                        bind_group: self.bind_group(device, &buffer, source),
                        into_b,
                    });
                }
            }
            self.targets = Some(Targets { size, a, b, passes });
            self.generation += 1;
        }
        &self.targets.as_ref().expect("targets").a
    }

    /// Blur the backdrop (drawn into the first texture); the result ends up there again.
    pub(crate) fn blur(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(t) = &self.targets else { return };
        for pass in &t.passes {
            let view = if pass.into_b { &t.b } else { &t.a };
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("glass blur"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            rp.set_pipeline(&self.blur);
            rp.set_bind_group(0, &pass.bind_group, &[]);
            rp.draw(0..3, 0..1);
        }
        self.ready = true;
    }

    fn prepare_shape(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
        mut uniforms: GlassUniforms,
    ) {
        let Some(t) = &self.targets else { return };
        uniforms.srgb_target = if self.srgb_target { 1.0 } else { 0.0 };
        // The backdrop covers whole texels: map onto exactly what it drew.
        let size = Self::backdrop_size([uniforms.area[2], uniforms.area[3]]);
        uniforms.area[2] = (size[0] * DOWNSCALE) as f32;
        uniforms.area[3] = (size[1] * DOWNSCALE) as f32;
        let stale = self
            .shapes
            .get(&id)
            .is_none_or(|s| s.generation != self.generation);
        if stale {
            let buffer = create_uniform_buffer(device, "glass", bytemuck::bytes_of(&uniforms));
            let bind_group = self.bind_group(device, &buffer, &t.a);
            self.shapes.insert(
                id,
                Shape {
                    buffer,
                    bind_group,
                    generation: self.generation,
                },
            );
        } else if let Some(s) = self.shapes.get(&id) {
            queue.write_buffer(&s.buffer, 0, bytemuck::bytes_of(&uniforms));
        }
    }
}

/// egui paint callback for one glass shape. `id` must be stable for the shape (it keys the
/// shape's GPU buffer) and unique among the shapes drawn in a frame.
pub struct GlassCallback {
    pub id: u64,
    pub uniforms: GlassUniforms,
}

impl egui_wgpu::CallbackTrait for GlassCallback {
    fn prepare(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _screen: &egui_wgpu::ScreenDescriptor,
        _encoder: &mut wgpu::CommandEncoder,
        resources: &mut egui_wgpu::CallbackResources,
    ) -> Vec<wgpu::CommandBuffer> {
        if let Some(renderer) = resources.get_mut::<CanvasRenderer>() {
            renderer
                .glass
                .prepare_shape(device, queue, self.id, self.uniforms);
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
        let glass = &renderer.glass;
        let Some(shape) = glass.shapes.get(&self.id) else {
            return;
        };
        if !glass.ready || shape.generation != glass.generation {
            return;
        }
        pass.set_pipeline(&glass.glass);
        pass.set_bind_group(0, &shape.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_validates() {
        let module = naga::front::wgsl::parse_str(include_str!("glass.wgsl")).expect("WGSL parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("WGSL validates");
    }

    #[test]
    fn uniforms_match_wgsl_layout() {
        assert_eq!(std::mem::size_of::<GlassUniforms>(), 64);
    }

    #[test]
    fn backdrop_is_a_quarter_size_rounded_up() {
        assert_eq!(Glass::backdrop_size([1000.0, 801.0]), [250, 201]);
        assert_eq!(Glass::backdrop_size([0.0, 3.0]), [1, 1]);
    }
}
