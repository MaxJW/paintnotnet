//! Flattens a document's layers into composite tiles, recompositing only tiles whose
//! revision changed.
//!
//! Follows paint.net 3.36 `Data/Document.cs` `Render` (MIT, see NOTICE): start from
//! transparent and apply each visible layer with its blend op, pre-scaled by layer opacity.
//! When the bottom layer is visible, opaque-at-255 and Normal, its pixels are taken as-is.
//!
//! Absent (fully transparent) source tiles are skipped. In paint.net's formula a source pixel
//! with alpha 0 leaves the destination unchanged, except that a destination pixel which is
//! also alpha 0 has its colour channels zeroed. That difference is invisible, and keeping
//! the original bytes is what paint.net's own fast path does anyway.

use std::sync::Arc;

use rayon::prelude::*;

use crate::tiles::{Surface, Tile, TileCoord};
use crate::{BlendMode, ColorBgra, Document};

/// Composite one tile. `None` means fully transparent.
pub fn composite_tile(doc: &Document, c: TileCoord) -> Option<Arc<Tile<ColorBgra>>> {
    let mut layers = doc
        .layers()
        .iter()
        .filter(|l| l.visible && l.opacity > 0)
        .peekable();
    let mut acc: Option<Arc<Tile<ColorBgra>>> = None;

    if let Some(first) = layers.peek()
        && first.opacity == 255
        && first.blend_mode == BlendMode::Normal
    {
        acc = first.surface.tile(c).cloned();
        layers.next();
    }

    for layer in layers {
        let Some(src) = layer.surface.tile(c) else {
            continue;
        };
        let dst = acc.get_or_insert_with(|| Arc::new(Tile::filled(ColorBgra::TRANSPARENT)));
        layer
            .blend_mode
            .apply_row(Arc::make_mut(dst).pixels_mut(), src.pixels(), layer.opacity);
    }
    acc
}

/// Cached composite of one document.
#[derive(Default)]
pub struct Compositor {
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles: Vec<Option<Arc<Tile<ColorBgra>>>>,
    seen: Vec<u64>,
}

impl Compositor {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recomposite every tile whose revision changed since the last call (in parallel) and
    /// return those tiles' coordinates. A size change recomposites everything.
    pub fn update(&mut self, doc: &Document) -> Vec<TileCoord> {
        if (self.width, self.height) != (doc.width(), doc.height()) {
            let n = (doc.tiles_x() * doc.tiles_y()) as usize;
            self.width = doc.width();
            self.height = doc.height();
            self.tiles_x = doc.tiles_x();
            self.tiles = vec![None; n];
            self.seen = vec![0; n];
        }
        let tiles_x = self.tiles_x;
        let changed: Vec<TileCoord> = (0..doc.tiles_y())
            .flat_map(|ty| (0..tiles_x).map(move |tx| TileCoord { tx, ty }))
            .filter(|&c| doc.tile_revision(c) != self.seen[(c.ty * tiles_x + c.tx) as usize])
            .collect();
        let results: Vec<_> = changed
            .par_iter()
            .map(|&c| composite_tile(doc, c))
            .collect();
        for (&c, tile) in changed.iter().zip(results) {
            let i = (c.ty * tiles_x + c.tx) as usize;
            self.tiles[i] = tile;
            self.seen[i] = doc.tile_revision(c);
        }
        changed
    }

    pub fn tile(&self, c: TileCoord) -> Option<&Arc<Tile<ColorBgra>>> {
        self.tiles[(c.ty * self.tiles_x + c.tx) as usize].as_ref()
    }

    /// Document tile revision this composite tile was built from. Downstream caches (GPU,
    /// thumbnails) compare against it to find what to refresh.
    pub fn tile_revision(&self, c: TileCoord) -> u64 {
        self.seen[(c.ty * self.tiles_x + c.tx) as usize]
    }

    pub fn tiles_x(&self) -> u32 {
        self.tiles_x
    }

    pub fn tiles_y(&self) -> u32 {
        (self.tiles.len() as u32)
            .checked_div(self.tiles_x)
            .unwrap_or(0)
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// The composite as a surface (shares tiles; no pixel copies). Call `update` first.
    pub fn to_surface(&self) -> Surface {
        let mut s = Surface::new_transparent(self.width, self.height);
        for c in s.tile_coords().collect::<Vec<_>>() {
            s.set_tile(c, self.tile(c).cloned());
        }
        s
    }
}

/// Flatten a document in one go (saving, copy merged, flatten).
pub fn flatten(doc: &Document) -> Surface {
    let mut c = Compositor::new();
    c.update(doc);
    c.to_surface()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layer, Rect};

    fn doc_with_top(top: ColorBgra, opacity: u8, mode: BlendMode) -> Document {
        let bottom = Document::new_blank(300, 300).layer(0).clone();
        let mut surface = Surface::new_transparent(300, 300);
        surface.set(5, 5, top);
        surface.set(299, 299, top);
        let mut layer = Layer::new("Layer 2", surface);
        layer.opacity = opacity;
        layer.blend_mode = mode;
        Document::from_layers(300, 300, vec![bottom, layer])
    }

    #[test]
    fn composites_with_blend_mode_and_opacity() {
        let top = ColorBgra::from_rgba(200, 100, 50, 255);
        for mode in BlendMode::ALL {
            for opacity in [255, 128] {
                let d = doc_with_top(top, opacity, mode);
                let flat = flatten(&d);
                let expected = mode.apply(ColorBgra::WHITE, top, opacity);
                assert_eq!(flat.get(5, 5), expected, "{mode:?} @ {opacity}");
                assert_eq!(
                    flat.get(299, 299),
                    expected,
                    "{mode:?} @ {opacity} (edge tile)"
                );
                assert_eq!(flat.get(6, 5), ColorBgra::WHITE, "untouched pixel");
            }
        }
    }

    #[test]
    fn hidden_and_zero_opacity_layers_are_skipped() {
        let mut d = doc_with_top(ColorBgra::BLACK, 255, BlendMode::Normal);
        d.modify_layer(1, None, |l| l.visible = false);
        assert_eq!(flatten(&d).get(5, 5), ColorBgra::WHITE);
        d.modify_layer(1, None, |l| {
            l.visible = true;
            l.opacity = 0;
        });
        assert_eq!(flatten(&d).get(5, 5), ColorBgra::WHITE);
    }

    #[test]
    fn bottom_layer_fast_path_shares_tiles() {
        let d = Document::new_blank(256, 256);
        let flat = flatten(&d);
        let c = TileCoord { tx: 0, ty: 0 };
        assert!(Arc::ptr_eq(
            flat.tile(c).unwrap(),
            d.layer(0).surface.tile(c).unwrap()
        ));
    }

    #[test]
    fn non_normal_bottom_layer_blends_over_transparent() {
        let mut d = Document::new_blank(10, 10);
        d.modify_layer(0, None, |l| l.opacity = 128);
        let expected = BlendMode::Normal.apply(ColorBgra::TRANSPARENT, ColorBgra::WHITE, 128);
        assert_eq!(flatten(&d).get(0, 0), expected);
        assert_eq!(expected.a, 128);
    }

    #[test]
    fn update_recomposites_only_changed_tiles() {
        let mut d = Document::new_blank(600, 600);
        let mut comp = Compositor::new();
        assert_eq!(comp.update(&d).len(), 9);
        assert!(comp.update(&d).is_empty());
        d.modify_layer(
            0,
            Some(Rect {
                x: 300,
                y: 300,
                width: 1,
                height: 1,
            }),
            |l| l.surface.set(300, 300, ColorBgra::BLACK),
        );
        assert_eq!(comp.update(&d), vec![TileCoord { tx: 1, ty: 1 }]);
        assert_eq!(
            comp.tile(TileCoord { tx: 1, ty: 1 }).unwrap().get(44, 44),
            ColorBgra::BLACK
        );
    }
}
