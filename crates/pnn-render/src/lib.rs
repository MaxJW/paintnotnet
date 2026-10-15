//! wgpu canvas display: tile textures, zoom/pan, checkerboard, pixel grid,
//! marching ants. Displays what the CPU compositor produced; it never composites.

mod display;
pub mod effects;
pub mod glass;
pub mod gpu;
mod layers;
pub mod view;

pub use effects::{GpuEffects, gpu_renderer};
pub use glass::{GlassCallback, GlassUniforms};
pub use gpu::{CanvasCallback, CanvasRenderer, ViewUniforms};
pub use layers::{LayerFloating, LayerGradient, LayerInput};
pub use view::View;
