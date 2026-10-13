//! Edit menu clipboard commands: Cut, Copy, Copy Merged, Paste, Paste into New Layer,
//! Paste into New Image, Copy Selection, Paste Selection.
//!
//! The system clipboard goes through `arboard` (Wayland data-control, falling back to X11);
//! tests swap in an in-memory clipboard so they never touch the desktop's.

use pnn_core::{ColorBgra, Document, Layer, Rect, Selection, Surface, flatten};

use crate::app::PaintNotNetApp;
use crate::commands::Command;
use crate::icons::Icon;
use crate::selection_tools::fill_masked;

/// RGBA8 image (straight alpha).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub trait Clipboard {
    fn get_image(&mut self) -> Option<ClipImage>;
    fn set_image(&mut self, image: ClipImage) -> Result<(), String>;
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: String) -> Result<(), String>;
}

/// The desktop clipboard.
pub struct SystemClipboard(Option<arboard::Clipboard>);

impl SystemClipboard {
    pub fn new() -> Self {
        Self(
            arboard::Clipboard::new()
                .map_err(|e| log::warn!("clipboard unavailable: {e}"))
                .ok(),
        )
    }
}

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for SystemClipboard {
    fn get_image(&mut self) -> Option<ClipImage> {
        let img = self.0.as_mut()?.get_image().ok()?;
        Some(ClipImage {
            width: img.width as u32,
            height: img.height as u32,
            rgba: img.bytes.into_owned(),
        })
    }

    fn set_image(&mut self, image: ClipImage) -> Result<(), String> {
        let cb = self.0.as_mut().ok_or("clipboard unavailable")?;
        cb.set_image(arboard::ImageData {
            width: image.width as usize,
            height: image.height as usize,
            bytes: image.rgba.into(),
        })
        .map_err(|e| e.to_string())
    }

    fn get_text(&mut self) -> Option<String> {
        self.0.as_mut()?.get_text().ok()
    }

    fn set_text(&mut self, text: String) -> Result<(), String> {
        self.0
            .as_mut()
            .ok_or("clipboard unavailable")?
            .set_text(text)
            .map_err(|e| e.to_string())
    }
}

/// In-memory clipboard for tests.
#[derive(Default)]
pub struct MemoryClipboard {
    pub image: Option<ClipImage>,
    pub text: Option<String>,
}

impl Clipboard for MemoryClipboard {
    fn get_image(&mut self) -> Option<ClipImage> {
        self.image.clone()
    }
    fn set_image(&mut self, image: ClipImage) -> Result<(), String> {
        self.image = Some(image);
        Ok(())
    }
    fn get_text(&mut self) -> Option<String> {
        self.text.clone()
    }
    fn set_text(&mut self, text: String) -> Result<(), String> {
        self.text = Some(text);
        Ok(())
    }
}

/// Pixels of `surface` inside the selection (whole surface when nothing is selected), as
/// RGBA with alpha scaled by selection coverage.
fn copy_region(surface: &Surface, selection: &Selection, antialias: bool) -> Option<ClipImage> {
    let (w, h) = (surface.width(), surface.height());
    let (r, mask) = if selection.is_empty() {
        (
            Rect {
                x: 0,
                y: 0,
                width: w,
                height: h,
            },
            None,
        )
    } else {
        (
            selection.pixel_bounds(w, h)?,
            Some(selection.rasterize(w, h, antialias)),
        )
    };
    let mut rgba = Vec::with_capacity((r.width * r.height * 4) as usize);
    for y in r.y..r.y + r.height {
        for x in r.x..r.x + r.width {
            let p = surface.get(x, y);
            let m = mask.as_ref().map_or(255, |m| m.get(x, y) as u32);
            rgba.extend_from_slice(&[p.r, p.g, p.b, (p.a as u32 * m / 255) as u8]);
        }
    }
    Some(ClipImage {
        width: r.width,
        height: r.height,
        rgba,
    })
}

fn to_surface(img: &ClipImage, at: (u32, u32), canvas: (u32, u32)) -> Surface {
    let mut s = Surface::new_transparent(canvas.0, canvas.1);
    for y in 0..img.height {
        for x in 0..img.width {
            let (cx, cy) = (at.0 + x, at.1 + y);
            if cx >= canvas.0 || cy >= canvas.1 {
                continue;
            }
            let i = ((y * img.width + x) * 4) as usize;
            let c = ColorBgra::from_rgba(
                img.rgba[i],
                img.rgba[i + 1],
                img.rgba[i + 2],
                img.rgba[i + 3],
            );
            if c.a > 0 {
                s.set(cx, cy, c);
            }
        }
    }
    s
}

impl PaintNotNetApp {
    /// Replace the clipboard backend (tests).
    pub fn set_clipboard(&mut self, clipboard: Box<dyn Clipboard>) {
        self.clipboard = clipboard;
    }

    /// Returns false if `command` isn't a clipboard command.
    pub(crate) fn clipboard_command(&mut self, command: Command) -> bool {
        let antialias = self.antialiased_selection();
        match command {
            Command::Copy | Command::Cut | Command::CopyMerged => {
                let Some(tab) = self.workspace.active_mut() else {
                    return true;
                };
                let selection = tab.doc.selection().clone();
                let image = if command == Command::CopyMerged {
                    copy_region(&flatten(&tab.doc), &selection, antialias)
                } else {
                    copy_region(
                        &tab.doc.layer(tab.doc.active_index()).surface,
                        &selection,
                        antialias,
                    )
                };
                let Some(image) = image else { return true };
                if let Err(e) = self.clipboard.set_image(image) {
                    self.errors.push(("Clipboard".into(), e));
                    return true;
                }
                if command == Command::Cut {
                    let Some(tab) = self.workspace.active_mut() else {
                        return true;
                    };
                    let (w, h) = (tab.doc.width(), tab.doc.height());
                    let mask = if selection.is_empty() {
                        Selection::all(w, h)
                    } else {
                        selection.clone()
                    }
                    .rasterize(w, h, antialias);
                    let a = tab.doc.active_index();
                    let bounds = selection.pixel_bounds(w, h);
                    tab.doc.modify_layer(a, bounds, |l| {
                        fill_masked(&mut l.surface, &mask, ColorBgra::TRANSPARENT)
                    });
                    tab.commit("Cut", Icon::Cut);
                }
            }
            Command::Paste | Command::PasteIntoNewLayer => {
                let Some(image) = self.clipboard.get_image() else {
                    return true;
                };
                let Some(tab) = self.workspace.active() else {
                    // Nothing open: paste into a new image, as paint.net does.
                    self.paste_new_image(image);
                    return true;
                };
                let (w, h) = (tab.doc.width(), tab.doc.height());
                if image.width > w || image.height > h {
                    self.modal = Some(crate::dialogs::Modal::PasteLarger {
                        new_layer: command == Command::PasteIntoNewLayer,
                    });
                    self.pending_paste = Some(image);
                } else {
                    self.paste(image, command == Command::PasteIntoNewLayer, false);
                }
            }
            Command::PasteIntoNewImage => {
                if let Some(image) = self.clipboard.get_image() {
                    self.paste_new_image(image);
                }
            }
            Command::CopySelection => {
                if let Some(tab) = self.workspace.active()
                    && !tab.doc.selection().is_empty()
                {
                    let json = tab.doc.selection().to_json();
                    if let Err(e) = self.clipboard.set_text(json) {
                        self.errors.push(("Clipboard".into(), e));
                    }
                }
            }
            Command::PasteSelection => {
                let Some(text) = self.clipboard.get_text() else {
                    return true;
                };
                let Some(selection) = Selection::from_json(&text) else {
                    self.errors.push((
                        "Paste Selection".into(),
                        "The clipboard doesn't contain a selection.".into(),
                    ));
                    return true;
                };
                if let Some(tab) = self.workspace.active_mut() {
                    tab.doc.set_selection(selection);
                    tab.commit("Paste Selection", Icon::Paste);
                }
            }
            _ => return false,
        }
        true
    }

    fn paste_new_image(&mut self, image: ClipImage) {
        let (w, h) = (image.width.max(1), image.height.max(1));
        let surface = to_surface(&image, (0, 0), (w, h));
        let doc = Document::from_layers(w, h, vec![Layer::new("Background", surface)]);
        self.workspace.add(doc, None, None);
    }

    /// Paste as floating pixels in Move Selected Pixels mode. `expand` grows the canvas to
    /// fit (anchored top-left) first.
    pub(crate) fn paste(&mut self, image: ClipImage, new_layer: bool, expand: bool) {
        let vp = self.canvas.viewport;
        let Some(tab) = self.workspace.active_mut() else {
            return;
        };
        if expand {
            let (w, h) = (
                tab.doc.width().max(image.width),
                tab.doc.height().max(image.height),
            );
            let layers = crate::doc_dialogs::map_layers(&tab.doc, |l| {
                pnn_core::transform::canvas_resize(
                    &l.surface,
                    w,
                    h,
                    pnn_core::transform::Anchor::TopLeft,
                    ColorBgra::TRANSPARENT,
                )
            });
            tab.doc.replace(w, h, layers);
            tab.commit("Canvas Size", Icon::CanvasSize);
        }
        let (w, h) = (tab.doc.width(), tab.doc.height());
        if new_layer {
            let a = tab.doc.active_index();
            let n = tab.doc.layers().len();
            tab.doc.insert_layer(
                a + 1,
                Layer::new(format!("Layer {}", n + 1), Surface::new_transparent(w, h)),
            );
        }
        // Top-left of the visible part of the canvas (paint.net pastes where you can see it).
        let at = tab.view.map_or((0, 0), |v| {
            let p = v.screen_to_image([0.0, 0.0], vp);
            let x = p[0].max(0.0).min((w.saturating_sub(image.width)) as f64) as u32;
            let y = p[1].max(0.0).min((h.saturating_sub(image.height)) as f64) as u32;
            (x, y)
        });
        let rect = Rect {
            x: at.0,
            y: at.1,
            width: image.width.min(w - at.0),
            height: image.height.min(h - at.1),
        };
        let pixels = to_surface(&image, at, (w, h));
        let selection = Selection::rect(
            rect.x as f64,
            rect.y as f64,
            (rect.x + rect.width) as f64,
            (rect.y + rect.height) as f64,
        );
        let name = if new_layer {
            "Paste into New Layer"
        } else {
            "Paste"
        };
        let key = tab.key;
        self.finish_move();
        self.set_tool(pnn_tools::ToolKind::MoveSelectedPixels);
        self.begin_float(key, pixels, rect, selection);
        if let Some(tab) = self.workspace.tab_mut(key) {
            tab.commit(name, Icon::Paste);
        }
    }
}

/// Shared handle so tests can inspect what the app copied.
impl Clipboard for std::rc::Rc<std::cell::RefCell<MemoryClipboard>> {
    fn get_image(&mut self) -> Option<ClipImage> {
        self.borrow_mut().get_image()
    }
    fn set_image(&mut self, image: ClipImage) -> Result<(), String> {
        self.borrow_mut().set_image(image)
    }
    fn get_text(&mut self) -> Option<String> {
        self.borrow_mut().get_text()
    }
    fn set_text(&mut self, text: String) -> Result<(), String> {
        self.borrow_mut().set_text(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_region_scales_alpha_by_coverage() {
        let mut s = Surface::new_transparent(4, 4);
        s.fill(ColorBgra::from_rgba(10, 20, 30, 255));
        let img = copy_region(&s, &Selection::rect(1.0, 1.0, 3.0, 2.0), false).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[0..4], &[10, 20, 30, 255]);
        let all = copy_region(&s, &Selection::none(), false).unwrap();
        assert_eq!((all.width, all.height), (4, 4));
    }

    /// Opt-in check against the real desktop clipboard (`cargo test -- --ignored`).
    /// Restores the previous clipboard text afterwards.
    #[test]
    #[ignore]
    fn system_clipboard_round_trip() {
        let mut cb = SystemClipboard::new();
        let previous = cb.get_text();
        let img = ClipImage {
            width: 3,
            height: 2,
            rgba: (0..24).map(|i| (i * 10) as u8).collect(),
        };
        cb.set_image(img.clone()).expect("set image");
        let back = cb.get_image().expect("get image");
        assert_eq!((back.width, back.height), (3, 2));
        assert_eq!(back.rgba, img.rgba);
        cb.set_text("paintnotnet clipboard check".into())
            .expect("set text");
        assert_eq!(
            cb.get_text().as_deref(),
            Some("paintnotnet clipboard check")
        );
        if let Some(text) = previous {
            cb.set_text(text).expect("restore");
        }
    }
}
