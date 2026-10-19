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

/// The desktop clipboard, served by a thread of its own: setting an image converts it to PNG
/// for the desktop (arboard does that on Wayland), which took ~0.3 s on a 24 MP photo on the
/// UI thread. Copying hands the image over and returns; reading waits for the answer.
pub struct SystemClipboard {
    tx: Option<std::sync::mpsc::Sender<ClipRequest>>,
}

enum ClipRequest {
    SetImage(ClipImage),
    SetText(String),
    GetImage(std::sync::mpsc::Sender<Option<ClipImage>>),
    GetText(std::sync::mpsc::Sender<Option<String>>),
}

impl SystemClipboard {
    pub fn new() -> Self {
        let (tx, rx) = std::sync::mpsc::channel::<ClipRequest>();
        let started = std::thread::Builder::new()
            .name("clipboard".into())
            .spawn(move || {
                let mut cb = match arboard::Clipboard::new() {
                    Ok(cb) => cb,
                    Err(e) => {
                        log::warn!("clipboard unavailable: {e}");
                        // Answer reads with nothing until the app ends.
                        for req in rx {
                            match req {
                                ClipRequest::GetImage(reply) => drop(reply.send(None)),
                                ClipRequest::GetText(reply) => drop(reply.send(None)),
                                _ => {}
                            }
                        }
                        return;
                    }
                };
                for req in rx {
                    match req {
                        ClipRequest::SetImage(image) => {
                            let r = cb.set_image(arboard::ImageData {
                                width: image.width as usize,
                                height: image.height as usize,
                                bytes: image.rgba.into(),
                            });
                            if let Err(e) = r {
                                log::warn!("clipboard: couldn't copy the image: {e}");
                            }
                        }
                        ClipRequest::SetText(text) => {
                            if let Err(e) = cb.set_text(text) {
                                log::warn!("clipboard: couldn't copy the text: {e}");
                            }
                        }
                        ClipRequest::GetImage(reply) => {
                            let img = cb.get_image().ok().map(|img| ClipImage {
                                width: img.width as u32,
                                height: img.height as u32,
                                rgba: img.bytes.into_owned(),
                            });
                            let _ = reply.send(img);
                        }
                        ClipRequest::GetText(reply) => {
                            let _ = reply.send(cb.get_text().ok());
                        }
                    }
                }
            })
            .is_ok();
        Self {
            tx: started.then_some(tx),
        }
    }

    fn send(&self, req: ClipRequest) -> Result<(), String> {
        self.tx
            .as_ref()
            .ok_or("clipboard unavailable")?
            .send(req)
            .map_err(|_| "clipboard unavailable".to_owned())
    }
}

impl Default for SystemClipboard {
    fn default() -> Self {
        Self::new()
    }
}

impl Clipboard for SystemClipboard {
    fn get_image(&mut self) -> Option<ClipImage> {
        let (reply, answer) = std::sync::mpsc::channel();
        self.send(ClipRequest::GetImage(reply)).ok()?;
        answer.recv().ok().flatten()
    }

    fn set_image(&mut self, image: ClipImage) -> Result<(), String> {
        self.send(ClipRequest::SetImage(image))
    }

    fn get_text(&mut self) -> Option<String> {
        let (reply, answer) = std::sync::mpsc::channel();
        self.send(ClipRequest::GetText(reply)).ok()?;
        answer.recv().ok().flatten()
    }

    fn set_text(&mut self, text: String) -> Result<(), String> {
        self.send(ClipRequest::SetText(text))
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
/// RGBA with alpha scaled by selection coverage (`mask`, the selection's cached mask). Rows in
/// parallel.
fn copy_region(
    surface: &Surface,
    selection: &Selection,
    mask: Option<&pnn_core::Mask>,
) -> Option<ClipImage> {
    use rayon::prelude::*;
    let (w, h) = (surface.width(), surface.height());
    let r = if selection.is_empty() {
        Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        }
    } else {
        selection.pixel_bounds(w, h)?
    };
    let mask = mask.filter(|_| !selection.is_empty());
    let mut rgba = vec![0u8; (r.width * r.height * 4) as usize];
    rgba.par_chunks_mut((r.width * 4) as usize)
        .enumerate()
        .for_each(|(j, row)| {
            let y = r.y + j as u32;
            for (i, px) in row.chunks_mut(4).enumerate() {
                let x = r.x + i as u32;
                let p = surface.get(x, y);
                let m = mask.map_or(255, |m| m.get(x, y) as u32);
                px.copy_from_slice(&[p.r, p.g, p.b, (p.a as u32 * m / 255) as u8]);
            }
        });
    Some(ClipImage {
        width: r.width,
        height: r.height,
        rgba,
    })
}

fn to_surface(img: &ClipImage, at: (u32, u32), canvas: (u32, u32)) -> Surface {
    use rayon::prelude::*;
    let mut s = Surface::new_transparent(canvas.0, canvas.1);
    let r = Rect {
        x: at.0.min(canvas.0),
        y: at.1.min(canvas.1),
        width: img.width.min(canvas.0.saturating_sub(at.0)),
        height: img.height.min(canvas.1.saturating_sub(at.1)),
    };
    if r.width == 0 || r.height == 0 {
        return s;
    }
    // Tiles in parallel (a pixel at a time took ~0.15 s for a 24 MP paste).
    let t = pnn_core::TILE_SIZE;
    s.slots_in(r).into_par_iter().for_each(|(c, slot)| {
        let mut tile = pnn_core::Tile::filled(ColorBgra::TRANSPARENT);
        let mut any = false;
        let (tx0, ty0) = (c.tx * t, c.ty * t);
        for cy in ty0.max(r.y)..(ty0 + t).min(r.y + r.height) {
            for cx in tx0.max(r.x)..(tx0 + t).min(r.x + r.width) {
                let i = (((cy - at.1) * img.width + (cx - at.0)) * 4) as usize;
                let c = ColorBgra::from_rgba(
                    img.rgba[i],
                    img.rgba[i + 1],
                    img.rgba[i + 2],
                    img.rgba[i + 3],
                );
                if c.a > 0 {
                    tile.set(cx - tx0, cy - ty0, c);
                    any = true;
                }
            }
        }
        if any {
            *slot = Some(std::sync::Arc::new(tile));
        }
    });
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
                let (w, h) = (tab.doc.width(), tab.doc.height());
                let selection = tab.doc.selection().clone();
                let bounds = selection.pixel_bounds(w, h);
                let mask = tab.selection_mask(antialias);
                let (doc, key, a) = (tab.doc.clone(), tab.key, tab.doc.active_index());
                let layer = doc.layer(a).id();
                let (cut, merged) = (command == Command::Cut, command == Command::CopyMerged);
                // Building the copy, and clearing what a Cut takes, cost ~40 ms each on a
                // 24 MP photo: on the effect threads for a large image.
                self.run_click_job(
                    w as u64 * h as u64,
                    move || {
                        let image = if merged {
                            copy_region(&flatten(&doc), &selection, mask.as_deref())
                        } else {
                            copy_region(&doc.layer(a).surface, &selection, mask.as_deref())
                        };
                        let cleared = (cut && image.is_some()).then(|| {
                            let mask = mask.unwrap_or_else(|| {
                                std::sync::Arc::new(Selection::all(w, h).rasterize(w, h, antialias))
                            });
                            let mut s = doc.layer(a).surface.clone();
                            fill_masked(&mut s, &mask, ColorBgra::TRANSPARENT);
                            s
                        });
                        (image, cleared)
                    },
                    move |app, (image, cleared)| {
                        let Some(image) = image else { return };
                        if let Err(e) = app.clipboard.set_image(image) {
                            app.errors.push(("Clipboard".into(), e));
                            return;
                        }
                        if let Some(surface) = cleared
                            && let Some(tab) = app.workspace.tab_mut(key)
                            && let Some(i) = tab.doc.layers().iter().position(|l| l.id() == layer)
                        {
                            tab.doc.modify_layer(i, bounds, |l| l.surface = surface);
                            tab.commit("Cut", Icon::Cut);
                        }
                    },
                );
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
        let sel = Selection::rect(1.0, 1.0, 3.0, 2.0);
        let mask = sel.rasterize(4, 4, false);
        let img = copy_region(&s, &sel, Some(&mask)).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(&img.rgba[0..4], &[10, 20, 30, 255]);
        let all = copy_region(&s, &Selection::none(), None).unwrap();
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
