//! Feather Object and the alpha helpers behind Blur Selection Edge.
//!
//! Feather Object (BoltBait's plugin, built in here) softens the edges of an object, i.e.
//! pixels surrounded by transparency, using the distance from each pixel to the object's
//! edge rather than a blur, so colours never smear into the transparent area and the
//! falloff width is even all the way round.

use pnn_core::{Mask, Surface};
use pnn_sdf::{Feather, Position, Profile, SignedField};
use rayon::prelude::*;

/// Multiply the alpha of `surface` by an inside feather of the object (alpha ≥
/// `threshold`). `canvas_edge` makes the canvas border an edge too.
pub fn feather_object(
    surface: &Surface,
    radius: f32,
    profile: Profile,
    threshold: u8,
    canvas_edge: bool,
) -> Surface {
    let (w, h) = (surface.width() as usize, surface.height() as usize);
    let mut px = surface.to_pixels();
    let inside: Vec<u8> = px
        .iter()
        .map(|p| if p.a >= threshold { 255 } else { 0 })
        .collect();
    let field = SignedField::from_coverage(&inside, w, h, canvas_edge);
    let feather = Feather {
        radius: radius.max(0.01),
        profile,
        position: Position::Inside,
    };
    px.par_iter_mut()
        .zip(field.sd.par_iter())
        .for_each(|(p, &sd)| {
            if p.a == 0 {
                return;
            }
            let k = feather.coverage(sd);
            p.a = (p.a as f32 * k + 0.5) as u8;
        });
    let mut out = Surface::new_transparent(w as u32, h as u32);
    out.write_pixels(&px);
    out.compact();
    out
}

/// Blur Selection Edge: inside the hard selection, scale alpha by the soft (feathered)
/// coverage.
pub fn scale_alpha_inside(surface: &mut Surface, hard: &Mask, soft: &Mask) {
    for c in hard.tile_coords().collect::<Vec<_>>() {
        let Some(h) = hard.tile(c) else { continue };
        let s = soft.tile(c);
        let tile = surface.tile_mut(c);
        for (i, p) in tile.pixels_mut().iter_mut().enumerate() {
            if h.pixels()[i] == 0 {
                continue;
            }
            let k = s.map_or(0, |t| t.pixels()[i]) as u32;
            p.a = (p.a as u32 * k / 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pnn_core::ColorBgra;

    fn disc() -> Surface {
        let mut s = Surface::new_transparent(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                let d = ((x as f32 - 32.0).powi(2) + (y as f32 - 32.0).powi(2)).sqrt();
                if d < 20.0 {
                    s.set(x, y, ColorBgra::from_rgba(200, 50, 10, 255));
                }
            }
        }
        s
    }

    #[test]
    fn feathers_alpha_inward_and_keeps_colour() {
        let f = feather_object(&disc(), 8.0, Profile::Linear, 128, false);
        let centre = f.get(32, 32);
        assert_eq!(centre.a, 255);
        let edge = f.get(51, 32); // just inside the rim
        assert!(edge.a > 0 && edge.a < 80, "soft rim: {}", edge.a);
        assert_eq!((edge.r, edge.g, edge.b), (200, 50, 10), "no colour bleed");
        assert_eq!(f.get(60, 32).a, 0, "outside stays clear");
    }

    #[test]
    fn canvas_edge_option() {
        let mut full = Surface::new_transparent(32, 32);
        full.fill(ColorBgra::WHITE);
        let ignored = feather_object(&full, 4.0, Profile::Linear, 128, false);
        assert_eq!(
            ignored.get(0, 16).a,
            255,
            "border is not an edge by default"
        );
        let edged = feather_object(&full, 4.0, Profile::Linear, 128, true);
        assert!(edged.get(0, 16).a < 64, "border feathers when asked");
    }
}
