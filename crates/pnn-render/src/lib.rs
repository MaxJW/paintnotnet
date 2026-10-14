//! wgpu canvas display: tile textures, zoom/pan, checkerboard, pixel grid,
//! marching ants. Displays what the CPU compositor produced; it never composites.

pub mod glass;
pub mod gpu;
pub mod view;

pub use glass::{GlassCallback, GlassUniforms};
pub use gpu::{CanvasCallback, CanvasRenderer, DisplayTile, ViewUniforms, display_tile};
pub use view::View;
