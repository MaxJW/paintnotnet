//! 32-bit BGRA pixel with straight (non-premultiplied) alpha.
//!
//! Ported from paint.net 3.36 `Core/ColorBgra.cs` (MIT, see NOTICE). The memory layout is
//! B, G, R, A, and `to_u32` packs it as `b | g << 8 | r << 16 | a << 24`, as paint.net does.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub struct ColorBgra {
    pub b: u8,
    pub g: u8,
    pub r: u8,
    pub a: u8,
}

impl ColorBgra {
    pub const TRANSPARENT: Self = Self::from_bgra(0, 0, 0, 0);
    pub const BLACK: Self = Self::from_bgra(0, 0, 0, 255);
    pub const WHITE: Self = Self::from_bgra(255, 255, 255, 255);

    pub const fn from_bgra(b: u8, g: u8, r: u8, a: u8) -> Self {
        Self { b, g, r, a }
    }

    pub const fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { b, g, r, a }
    }

    pub const fn from_u32(bgra: u32) -> Self {
        Self {
            b: bgra as u8,
            g: (bgra >> 8) as u8,
            r: (bgra >> 16) as u8,
            a: (bgra >> 24) as u8,
        }
    }

    pub const fn to_u32(self) -> u32 {
        self.b as u32 | (self.g as u32) << 8 | (self.r as u32) << 16 | (self.a as u32) << 24
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }

    pub const fn to_rgba_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_like_paint_net() {
        let c = ColorBgra::from_rgba(0x11, 0x22, 0x33, 0x44);
        assert_eq!(c.to_u32(), 0x4411_2233);
        assert_eq!(ColorBgra::from_u32(0x4411_2233), c);
        assert_eq!(bytemuck::bytes_of(&c), &[0x33, 0x22, 0x11, 0x44]);
    }
}
