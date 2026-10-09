//! Documents and layers.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::tiles::{Surface, TILE_SIZE, TileCoord, tiles_for};
use crate::{BlendMode, ColorBgra, Selection};

/// Process-wide revision counter. Revisions are never reused, so a document restored from
/// history can't be mistaken for a newer state by caches comparing revisions.
fn next_revision() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Stable identity for a layer, independent of its position in the stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LayerId(u64);

impl LayerId {
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Clone, Debug)]
pub struct Layer {
    id: LayerId,
    pub name: String,
    pub visible: bool,
    /// 0..=255, as in paint.net's Layer Properties dialog.
    pub opacity: u8,
    pub blend_mode: BlendMode,
    pub surface: Surface,
}

impl Layer {
    pub fn new(name: impl Into<String>, surface: Surface) -> Self {
        Self {
            id: LayerId::next(),
            name: name.into(),
            visible: true,
            opacity: 255,
            blend_mode: BlendMode::Normal,
            surface,
        }
    }

    pub fn id(&self) -> LayerId {
        self.id
    }
}

/// Pixel rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// paint.net's default resolution.
pub const DEFAULT_DPI: f64 = 96.0;

/// An image: a stack of equally sized layers, bottom first (paint.net's index order).
///
/// Every layer mutation goes through a method here, so per-tile revisions stay accurate.
/// Consumers (compositor, GPU upload, thumbnails) compare revisions instead of sharing a
/// single dirty list.
#[derive(Clone, Debug)]
pub struct Document {
    width: u32,
    height: u32,
    layers: Vec<Layer>,
    active: usize,
    pub dpi: f64,
    revision: u64,
    tile_revisions: Vec<u64>,
    selection: Selection,
    selection_revision: u64,
}

impl Document {
    /// paint.net's File > New: a white "Background" layer.
    pub fn new_blank(width: u32, height: u32) -> Self {
        let mut surface = Surface::new_transparent(width, height);
        surface.fill(ColorBgra::WHITE);
        Self::from_layers(width, height, vec![Layer::new("Background", surface)])
    }

    pub fn from_layers(width: u32, height: u32, layers: Vec<Layer>) -> Self {
        assert!(
            !layers.is_empty(),
            "a document always has at least one layer"
        );
        assert!(
            layers
                .iter()
                .all(|l| l.surface.width() == width && l.surface.height() == height)
        );
        let tiles = (tiles_for(width) * tiles_for(height)) as usize;
        let revision = next_revision();
        Self {
            width,
            height,
            active: layers.len() - 1,
            layers,
            dpi: DEFAULT_DPI,
            revision,
            tile_revisions: vec![revision; tiles],
            selection: Selection::none(),
            selection_revision: revision,
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn tiles_x(&self) -> u32 {
        tiles_for(self.width)
    }

    pub fn tiles_y(&self) -> u32 {
        tiles_for(self.height)
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    pub fn layer(&self, index: usize) -> &Layer {
        &self.layers[index]
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn set_active_index(&mut self, index: usize) {
        assert!(index < self.layers.len());
        self.active = index;
    }

    /// The current selection (empty = nothing selected: tools act on the whole layer).
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    pub fn set_selection(&mut self, selection: Selection) {
        self.selection = selection;
        self.selection_revision = next_revision();
    }

    /// Changes whenever the selection changes (pixel revisions don't).
    pub fn selection_revision(&self) -> u64 {
        self.selection_revision
    }

    /// Increases whenever any pixel of the composite may have changed.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn tile_revision(&self, c: TileCoord) -> u64 {
        self.tile_revisions[(c.ty * self.tiles_x() + c.tx) as usize]
    }

    /// Mutate one layer's pixels. `dirty` is the area that may change (`None` = whole image).
    pub fn modify_layer<R>(
        &mut self,
        index: usize,
        dirty: Option<Rect>,
        f: impl FnOnce(&mut Layer) -> R,
    ) -> R {
        let result = f(&mut self.layers[index]);
        match dirty {
            Some(rect) => self.invalidate_rect(rect),
            None => self.invalidate_all(),
        }
        result
    }

    pub fn invalidate_all(&mut self) {
        self.revision = next_revision();
        let rev = self.revision;
        self.tile_revisions.iter_mut().for_each(|r| *r = rev);
    }

    pub fn invalidate_rect(&mut self, rect: Rect) {
        if rect.width == 0 || rect.height == 0 || rect.x >= self.width || rect.y >= self.height {
            return;
        }
        self.revision = next_revision();
        let rev = self.revision;
        let x1 = (rect.x + rect.width).min(self.width) - 1;
        let y1 = (rect.y + rect.height).min(self.height) - 1;
        let tiles_x = self.tiles_x();
        for ty in rect.y / TILE_SIZE..=y1 / TILE_SIZE {
            for tx in rect.x / TILE_SIZE..=x1 / TILE_SIZE {
                self.tile_revisions[(ty * tiles_x + tx) as usize] = rev;
            }
        }
    }
}

/// Structural edits. Each invalidates the composite so every consumer refreshes.
impl Document {
    /// Insert a layer at `index` (0 = bottom) and make it active.
    pub fn insert_layer(&mut self, index: usize, layer: Layer) {
        assert!(layer.surface.width() == self.width && layer.surface.height() == self.height);
        self.layers.insert(index, layer);
        self.active = index;
        self.invalidate_all();
    }

    /// Remove a layer (never the last one). The layer below becomes active, or the new
    /// bottom layer if the bottom one was removed.
    pub fn remove_layer(&mut self, index: usize) -> Option<Layer> {
        if self.layers.len() <= 1 || index >= self.layers.len() {
            return None;
        }
        let layer = self.layers.remove(index);
        self.active = index.saturating_sub(1).min(self.layers.len() - 1);
        self.invalidate_all();
        Some(layer)
    }

    /// Move a layer to a new position; it stays active.
    pub fn move_layer(&mut self, from: usize, to: usize) {
        if from == to || from >= self.layers.len() || to >= self.layers.len() {
            return;
        }
        let layer = self.layers.remove(from);
        self.layers.insert(to, layer);
        self.active = to;
        self.invalidate_all();
    }

    /// Replace every layer and the size (whole-image transforms: resize, rotate, canvas).
    pub fn replace(&mut self, width: u32, height: u32, layers: Vec<Layer>) {
        assert!(!layers.is_empty());
        assert!(
            layers
                .iter()
                .all(|l| l.surface.width() == width && l.surface.height() == height)
        );
        let active = self.active.min(layers.len() - 1);
        let dpi = self.dpi;
        *self = Self::from_layers(width, height, layers);
        self.active = active;
        self.dpi = dpi;
    }

    /// Bytes of tile memory referenced by this document (shared tiles counted once each).
    pub fn tile_bytes(&self) -> usize {
        self.layers
            .iter()
            .map(|l| l.surface.allocated_tiles())
            .sum::<usize>()
            * crate::tiles::TILE_PIXELS
            * std::mem::size_of::<ColorBgra>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_blank_is_white_background() {
        let d = Document::new_blank(800, 600);
        assert_eq!(d.layers().len(), 1);
        assert_eq!(d.layer(0).name, "Background");
        assert_eq!(d.layer(0).surface.get(799, 599), ColorBgra::WHITE);
        assert_eq!(d.dpi, 96.0);
    }

    #[test]
    fn invalidate_rect_bumps_only_covered_tiles() {
        let mut d = Document::new_blank(600, 600);
        let before = d.revision();
        d.invalidate_rect(Rect {
            x: 250,
            y: 10,
            width: 10,
            height: 5,
        });
        assert!(d.revision() > before);
        let rev = d.revision();
        assert_eq!(d.tile_revision(TileCoord { tx: 0, ty: 0 }), rev);
        assert_eq!(d.tile_revision(TileCoord { tx: 1, ty: 0 }), rev);
        assert!(d.tile_revision(TileCoord { tx: 2, ty: 0 }) < rev);
        assert!(d.tile_revision(TileCoord { tx: 0, ty: 1 }) < rev);
    }

    #[test]
    fn remove_keeps_one_layer_and_activates_below() {
        let mut d = Document::new_blank(4, 4);
        d.insert_layer(1, Layer::new("Layer 2", Surface::new_transparent(4, 4)));
        d.insert_layer(2, Layer::new("Layer 3", Surface::new_transparent(4, 4)));
        assert_eq!(d.active_index(), 2);
        d.remove_layer(2);
        assert_eq!(d.active_index(), 1);
        d.remove_layer(0);
        assert_eq!(d.active_index(), 0);
        assert!(
            d.remove_layer(0).is_none(),
            "the last layer can't be deleted"
        );
    }

    #[test]
    fn revisions_are_never_reused() {
        let a = Document::new_blank(4, 4);
        let mut b = a.clone();
        b.invalidate_all();
        let c = Document::new_blank(4, 4);
        assert!(b.revision() > a.revision());
        assert!(c.revision() > b.revision());
    }

    #[test]
    fn layer_ids_are_unique() {
        let a = Layer::new("a", Surface::new_transparent(1, 1));
        let b = Layer::new("b", Surface::new_transparent(1, 1));
        assert_ne!(a.id(), b.id());
        assert_eq!(a.clone().id(), a.id());
    }
}
