//! Document model: tiled surfaces, layers, colour, the 14 paint.net blend ops,
//! history, and selections with their coverage masks.
//!
//! No GUI or GPU dependencies; everything here is unit-testable.

pub mod blend;
pub mod color;
pub mod compositor;
pub mod document;
pub mod tiles;

pub use blend::BlendMode;
pub use color::ColorBgra;
pub use compositor::{Compositor, flatten};
pub use document::{DEFAULT_DPI, Document, Layer, LayerId, Rect};
pub use tiles::{Mask, Surface, TILE_SIZE, Tile, TileCoord, TiledBuffer};
