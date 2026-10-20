//! Thumbnails for File > Open Recent. paint.net 3.36 renders one whenever a file is opened or
//! saved (`DocumentWorkspace.AddToMruList`: the image at most 56 px on its long side, centred in
//! a square with a black border) and keeps it with the list. Here they are small PNGs in the
//! cache folder, one per file, named from a hash of its path; files that leave the list lose
//! theirs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, ColorImage, TextureFilter, TextureOptions};

use crate::workspace::Workspace;

/// Side of a thumbnail in the menu, in points.
pub const SIZE_PT: f32 = 48.0;
/// Side of a stored thumbnail, in pixels (sharp at twice the scale).
const SIZE_PX: usize = 96;

/// Default folder: `$XDG_CACHE_HOME/paintnotnet/recent` or `~/.cache/paintnotnet/recent`.
pub fn default_dir() -> Option<PathBuf> {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))?;
    Some(cache.join("paintnotnet/recent"))
}

/// A file's thumbnail: the texture for the menu and the PNG for the desktop's global menu.
pub struct Thumb {
    pub texture: egui::TextureHandle,
    pub png: Arc<Vec<u8>>,
}

#[derive(Default)]
pub struct RecentThumbs {
    /// Where thumbnails are kept (set by `main`; tests keep them in memory).
    dir: Option<PathBuf>,
    /// Thumbnails made or loaded this session; `None`: looked for, there is none.
    thumbs: HashMap<PathBuf, Option<Thumb>>,
    /// Files whose thumbnail waits for their tab's to catch up: (tab key, path).
    pending: Vec<(u64, PathBuf)>,
}

impl RecentThumbs {
    pub fn set_dir(&mut self, dir: Option<PathBuf>) {
        self.dir = dir;
        self.thumbs.clear();
    }

    /// Make `path`'s thumbnail from tab `key` once that tab's image is composited.
    pub fn request(&mut self, key: u64, path: PathBuf) {
        self.pending.retain(|(_, p)| *p != path);
        self.pending.push((key, path));
    }

    /// Make the thumbnails whose tabs are up to date (after `shell::update_thumbnail`, which
    /// brings each tab's compositor up to date). A closed tab's request is dropped.
    pub fn update(&mut self, ctx: &egui::Context, workspace: &Workspace) {
        let mut done = Vec::new();
        self.pending.retain(|(key, path)| {
            let Some(tab) = workspace.tabs().iter().find(|t| t.key == *key) else {
                return false;
            };
            if tab.thumbnail.is_none() || tab.thumbnail_revision != tab.doc.revision() {
                return true;
            }
            done.push((
                path.clone(),
                framed(crate::shell::thumbnail_pixels(tab, SIZE_PX as f32)),
            ));
            false
        });
        for (path, image) in done {
            let Some(png) = encode(&image) else { continue };
            if let Some(dir) = &self.dir {
                let written = std::fs::create_dir_all(dir)
                    .and_then(|_| std::fs::write(dir.join(file_name(&path)), &png));
                if let Err(e) = written {
                    log::warn!("saving the Open Recent thumbnail: {e}");
                }
            }
            let thumb = Thumb {
                texture: load(ctx, &path, image),
                png: Arc::new(png),
            };
            self.thumbs.insert(path, Some(thumb));
        }
    }

    /// `path`'s thumbnail, loaded from the folder the first time it's asked for.
    pub fn get(&mut self, ctx: &egui::Context, path: &Path) -> Option<&Thumb> {
        let dir = self.dir.as_deref();
        self.thumbs
            .entry(path.to_owned())
            .or_insert_with(|| {
                let png = std::fs::read(dir?.join(file_name(path))).ok()?;
                let image = decode(&png)?;
                Some(Thumb {
                    texture: load(ctx, path, image),
                    png: Arc::new(png),
                })
            })
            .as_ref()
    }

    /// Drop the thumbnails of files no longer in `keep` (the list), in memory and on disk.
    pub fn retain(&mut self, keep: &[PathBuf]) {
        self.thumbs.retain(|p, _| keep.contains(p));
        self.pending.retain(|(_, p)| keep.contains(p));
        let Some(dir) = &self.dir else { return };
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let names: Vec<String> = keep.iter().map(|p| file_name(p)).collect();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.ends_with(".png") && !names.iter().any(|n| *n == name) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

/// The image centred in a `SIZE_PX` square, with a thin dark border round it (3.36's black
/// border, softened so it suits light and dark menus).
fn framed(image: ColorImage) -> ColorImage {
    let [w, h] = image.size;
    let mut out = ColorImage::filled([SIZE_PX, SIZE_PX], Color32::TRANSPARENT);
    let (x0, y0) = ((SIZE_PX - w) / 2, (SIZE_PX - h) / 2);
    let border = Color32::from_black_alpha(110);
    for y in 0..h {
        for x in 0..w {
            let edge = x < 2 || y < 2 || x + 2 >= w || y + 2 >= h;
            let p = image.pixels[y * w + x];
            out.pixels[(y0 + y) * SIZE_PX + x0 + x] = if edge { p.blend(border) } else { p };
        }
    }
    out
}

/// Stable across runs and Rust versions (FNV-1a), unlike std's hasher.
fn file_name(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in path.as_os_str().as_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}.png")
}

fn load(ctx: &egui::Context, path: &Path, image: ColorImage) -> egui::TextureHandle {
    let options = TextureOptions {
        mipmap_mode: Some(TextureFilter::Linear),
        ..TextureOptions::LINEAR
    };
    ctx.load_texture(format!("recent-{}", path.display()), image, options)
}

fn encode(image: &ColorImage) -> Option<Vec<u8>> {
    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let mut png = Vec::new();
    image::ImageEncoder::write_image(
        image::codecs::png::PngEncoder::new(&mut png),
        &rgba,
        image.size[0] as u32,
        image.size[1] as u32,
        image::ExtendedColorType::Rgba8,
    )
    .ok()?;
    Some(png)
}

fn decode(png: &[u8]) -> Option<ColorImage> {
    let img = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, img.as_raw()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_are_stable_and_distinct() {
        assert_eq!(
            file_name(Path::new("/home/a/picture.png")),
            file_name(Path::new("/home/a/picture.png"))
        );
        assert_ne!(
            file_name(Path::new("/home/a/picture.png")),
            file_name(Path::new("/home/a/picture2.png"))
        );
        // FNV-1a of the empty string is its offset basis.
        assert_eq!(file_name(Path::new("")), "cbf29ce484222325.png");
    }

    #[test]
    fn framed_centres_the_image_with_a_border() {
        let image = ColorImage::filled([40, 20], Color32::from_rgb(200, 30, 30));
        let out = framed(image);
        assert_eq!(out.size, [SIZE_PX, SIZE_PX]);
        let at = |x: usize, y: usize| out.pixels[y * SIZE_PX + x];
        assert_eq!(at(0, 0), Color32::TRANSPARENT, "outside the image");
        let (x0, y0) = ((SIZE_PX - 40) / 2, (SIZE_PX - 20) / 2);
        assert_eq!(at(x0 + 20, y0 + 10), Color32::from_rgb(200, 30, 30));
        assert!(at(x0, y0 + 10).r() < 200, "border darker");
        assert_eq!(at(x0 - 1, y0 + 10), Color32::TRANSPARENT);
    }

    #[test]
    fn png_round_trip() {
        let image = framed(ColorImage::filled([30, 30], Color32::from_rgb(10, 20, 30)));
        let back = decode(&encode(&image).unwrap()).unwrap();
        assert_eq!(back.size, image.size);
        assert_eq!(
            back.pixels[48 * SIZE_PX + 48],
            Color32::from_rgb(10, 20, 30)
        );
    }
}
