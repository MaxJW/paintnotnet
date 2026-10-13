//! Sparse, copy-on-write tiled 2D buffers.
//!
//! Each tile is 256×256 and held in an `Arc`. Cloning a buffer is cheap, and writes copy only
//! the touched tile (this is what makes undo snapshots cheap). Absent tiles read as the
//! buffer's `absent` value: transparent for layers, and 0 (or 255 for "select all") for masks.

use std::sync::Arc;

use crate::ColorBgra;

pub const TILE_SIZE: u32 = 256;
pub const TILE_PIXELS: usize = (TILE_SIZE * TILE_SIZE) as usize;

/// One tile's pixels, row-major, always the full 256×256 even at the image edge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile<T>(Box<[T]>);

impl<T: Copy> Tile<T> {
    pub fn filled(value: T) -> Self {
        Self(vec![value; TILE_PIXELS].into_boxed_slice())
    }

    pub fn pixels(&self) -> &[T] {
        &self.0
    }

    pub fn pixels_mut(&mut self) -> &mut [T] {
        &mut self.0
    }

    pub fn row(&self, y: u32) -> &[T] {
        let start = (y * TILE_SIZE) as usize;
        &self.0[start..start + TILE_SIZE as usize]
    }

    pub fn row_mut(&mut self, y: u32) -> &mut [T] {
        let start = (y * TILE_SIZE) as usize;
        &mut self.0[start..start + TILE_SIZE as usize]
    }

    pub fn get(&self, x: u32, y: u32) -> T {
        self.0[(y * TILE_SIZE + x) as usize]
    }

    pub fn set(&mut self, x: u32, y: u32, value: T) {
        self.0[(y * TILE_SIZE + x) as usize] = value;
    }
}

/// Tile coordinates (in tiles, not pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileCoord {
    pub tx: u32,
    pub ty: u32,
}

/// Number of tiles needed to cover `pixels`.
pub const fn tiles_for(pixels: u32) -> u32 {
    pixels.div_ceil(TILE_SIZE)
}

#[derive(Clone, Debug)]
pub struct TiledBuffer<T> {
    width: u32,
    height: u32,
    tiles_x: u32,
    tiles_y: u32,
    absent: T,
    tiles: Vec<Option<Arc<Tile<T>>>>,
}

impl<T: Copy + PartialEq> TiledBuffer<T> {
    pub fn new(width: u32, height: u32, absent: T) -> Self {
        assert!(width > 0 && height > 0, "buffer must be at least 1×1");
        let (tiles_x, tiles_y) = (tiles_for(width), tiles_for(height));
        Self {
            width,
            height,
            tiles_x,
            tiles_y,
            absent,
            tiles: vec![None; (tiles_x * tiles_y) as usize],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn tiles_x(&self) -> u32 {
        self.tiles_x
    }

    pub fn tiles_y(&self) -> u32 {
        self.tiles_y
    }

    pub fn absent(&self) -> T {
        self.absent
    }

    pub fn tile_coords(&self) -> impl Iterator<Item = TileCoord> + use<T> {
        let (tiles_x, tiles_y) = (self.tiles_x, self.tiles_y);
        (0..tiles_y).flat_map(move |ty| (0..tiles_x).map(move |tx| TileCoord { tx, ty }))
    }

    fn index(&self, c: TileCoord) -> usize {
        debug_assert!(c.tx < self.tiles_x && c.ty < self.tiles_y);
        (c.ty * self.tiles_x + c.tx) as usize
    }

    /// The tile, or `None` if it is absent (every pixel equals `absent()`).
    pub fn tile(&self, c: TileCoord) -> Option<&Arc<Tile<T>>> {
        self.tiles[self.index(c)].as_ref()
    }

    /// Writable tile, allocating it (filled with `absent`) or un-sharing it as needed.
    pub fn tile_mut(&mut self, c: TileCoord) -> &mut Tile<T> {
        let absent = self.absent;
        let i = self.index(c);
        let slot = self.tiles[i].get_or_insert_with(|| Arc::new(Tile::filled(absent)));
        Arc::make_mut(slot)
    }

    /// Writable tiles overlapping `r` (allocated or un-shared as needed), for working on
    /// several at once (e.g. in parallel).
    pub fn tiles_mut_in(&mut self, r: crate::Rect) -> Vec<(TileCoord, &mut Tile<T>)> {
        if r.width == 0 || r.height == 0 {
            return Vec::new();
        }
        let (tx0, tx1) = (r.x / TILE_SIZE, (r.x + r.width - 1) / TILE_SIZE);
        let (ty0, ty1) = (r.y / TILE_SIZE, (r.y + r.height - 1) / TILE_SIZE);
        let (tiles_x, absent) = (self.tiles_x, self.absent);
        self.tiles
            .iter_mut()
            .enumerate()
            .filter_map(|(i, slot)| {
                let c = TileCoord {
                    tx: i as u32 % tiles_x,
                    ty: i as u32 / tiles_x,
                };
                (c.tx >= tx0 && c.tx <= tx1 && c.ty >= ty0 && c.ty <= ty1).then(|| {
                    let tile = slot.get_or_insert_with(|| Arc::new(Tile::filled(absent)));
                    (c, Arc::make_mut(tile))
                })
            })
            .collect()
    }

    pub fn set_tile(&mut self, c: TileCoord, tile: Option<Arc<Tile<T>>>) {
        let i = self.index(c);
        self.tiles[i] = tile;
    }

    pub fn get(&self, x: u32, y: u32) -> T {
        debug_assert!(x < self.width && y < self.height);
        let c = TileCoord {
            tx: x / TILE_SIZE,
            ty: y / TILE_SIZE,
        };
        match self.tile(c) {
            Some(t) => t.get(x % TILE_SIZE, y % TILE_SIZE),
            None => self.absent,
        }
    }

    pub fn set(&mut self, x: u32, y: u32, value: T) {
        debug_assert!(x < self.width && y < self.height);
        let c = TileCoord {
            tx: x / TILE_SIZE,
            ty: y / TILE_SIZE,
        };
        self.tile_mut(c).set(x % TILE_SIZE, y % TILE_SIZE, value);
    }

    /// Set every pixel to `value`. Filling with `absent` frees all tiles.
    pub fn fill(&mut self, value: T) {
        if value == self.absent {
            self.tiles.iter_mut().for_each(|t| *t = None);
        } else {
            let tile = Arc::new(Tile::filled(value));
            self.tiles.iter_mut().for_each(|t| *t = Some(tile.clone()));
        }
    }

    /// Free tiles whose every pixel equals the absent value (restores sparsity after
    /// whole-surface rewrites).
    pub fn compact(&mut self) {
        let absent = self.absent;
        for slot in &mut self.tiles {
            if slot
                .as_ref()
                .is_some_and(|t| t.pixels().iter().all(|&p| p == absent))
            {
                *slot = None;
            }
        }
    }

    /// Number of allocated tiles (memory diagnostics, tests).
    pub fn allocated_tiles(&self) -> usize {
        self.tiles.iter().filter(|t| t.is_some()).count()
    }

    /// The pixel rectangle (x, y, w, h) a tile covers inside the buffer.
    pub fn tile_bounds(&self, c: TileCoord) -> (u32, u32, u32, u32) {
        let (x, y) = (c.tx * TILE_SIZE, c.ty * TILE_SIZE);
        (
            x,
            y,
            TILE_SIZE.min(self.width - x),
            TILE_SIZE.min(self.height - y),
        )
    }

    /// Copy row-major pixels (exactly `width * height`) into the buffer.
    pub fn write_pixels(&mut self, pixels: &[T]) {
        assert_eq!(pixels.len(), (self.width as usize) * (self.height as usize));
        let width = self.width as usize;
        for c in self.tile_coords().collect::<Vec<_>>() {
            let (x0, y0, w, h) = self.tile_bounds(c);
            let tile = self.tile_mut(c);
            for row in 0..h {
                let src_start = (y0 + row) as usize * width + x0 as usize;
                tile.row_mut(row)[..w as usize]
                    .copy_from_slice(&pixels[src_start..src_start + w as usize]);
            }
        }
    }

    /// Row-major copy of all pixels.
    pub fn to_pixels(&self) -> Vec<T> {
        let width = self.width as usize;
        let mut out = vec![self.absent; width * self.height as usize];
        for c in self.tile_coords() {
            let Some(tile) = self.tile(c) else { continue };
            let (x0, y0, w, h) = self.tile_bounds(c);
            for row in 0..h {
                let dst_start = (y0 + row) as usize * width + x0 as usize;
                out[dst_start..dst_start + w as usize]
                    .copy_from_slice(&tile.row(row)[..w as usize]);
            }
        }
        out
    }
}

/// A layer's pixels.
pub type Surface = TiledBuffer<ColorBgra>;

impl Surface {
    pub fn new_transparent(width: u32, height: u32) -> Self {
        Self::new(width, height, ColorBgra::TRANSPARENT)
    }
}

/// Per-pixel selection coverage, 0 = unselected, 255 = fully selected. Built from selection
/// geometry plus feathering. Every selection-aware operation consumes this.
pub type Mask = TiledBuffer<u8>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_tiles_read_as_absent_value() {
        let s = Surface::new_transparent(300, 300);
        assert_eq!(s.tiles_x(), 2);
        assert_eq!(s.get(299, 299), ColorBgra::TRANSPARENT);
        assert_eq!(s.allocated_tiles(), 0);
        let m = Mask::new(10, 10, 255);
        assert_eq!(m.get(3, 3), 255);
    }

    #[test]
    fn writes_allocate_only_touched_tile() {
        let mut s = Surface::new_transparent(600, 600);
        s.set(257, 3, ColorBgra::WHITE);
        assert_eq!(s.allocated_tiles(), 1);
        assert_eq!(s.get(257, 3), ColorBgra::WHITE);
        assert_eq!(s.get(256, 3), ColorBgra::TRANSPARENT);
    }

    #[test]
    fn clones_share_tiles_until_written() {
        let mut a = Surface::new_transparent(256, 256);
        a.set(0, 0, ColorBgra::BLACK);
        let mut b = a.clone();
        let c = TileCoord { tx: 0, ty: 0 };
        assert!(Arc::ptr_eq(a.tile(c).unwrap(), b.tile(c).unwrap()));
        b.set(1, 1, ColorBgra::WHITE);
        assert!(!Arc::ptr_eq(a.tile(c).unwrap(), b.tile(c).unwrap()));
        assert_eq!(a.get(1, 1), ColorBgra::TRANSPARENT);
        assert_eq!(b.get(0, 0), ColorBgra::BLACK);
    }

    #[test]
    fn pixel_round_trip_across_tile_edges() {
        let (w, h) = (300u32, 270u32);
        let pixels: Vec<ColorBgra> = (0..w * h)
            .map(|i| ColorBgra::from_u32(i.wrapping_mul(2_654_435_761)))
            .collect();
        let mut s = Surface::new_transparent(w, h);
        s.write_pixels(&pixels);
        assert_eq!(s.to_pixels(), pixels);
        assert_eq!(s.get(299, 269), pixels[(269 * w + 299) as usize]);
        assert_eq!(
            s.tile_bounds(TileCoord { tx: 1, ty: 1 }),
            (256, 256, 44, 14)
        );
    }

    #[test]
    fn fill_with_absent_frees_tiles() {
        let mut s = Surface::new_transparent(512, 512);
        s.fill(ColorBgra::WHITE);
        assert_eq!(s.allocated_tiles(), 4);
        assert_eq!(s.get(511, 511), ColorBgra::WHITE);
        s.fill(ColorBgra::TRANSPARENT);
        assert_eq!(s.allocated_tiles(), 0);
    }
}
