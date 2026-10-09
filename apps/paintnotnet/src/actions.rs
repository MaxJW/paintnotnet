//! Document commands (Edit, Image, Layers menus), the Pencil tool, and dialog plumbing.
//! Each change is recorded as a history step with paint.net 3.36's step names.

use std::collections::HashSet;

use eframe::egui;
use pnn_core::transform;
use pnn_core::{ColorBgra, Document, Layer, Rect, Surface};
use pnn_tools::ToolBlend;

use crate::app::PaintNotNetApp;
use crate::canvas::{ToolButton, ToolEvent};
use crate::commands::Command;
use crate::doc_dialogs::{self, DocDialog, Outcome, map_layers};
use crate::icons::Icon;
use crate::workspace::DocumentTab;

/// A Pencil stroke in progress.
pub struct PencilStroke {
    key: u64,
    last: (i64, i64),
    color: ColorBgra,
    blend: ToolBlend,
    /// Pixels already drawn this stroke: a semi-transparent colour isn't blended twice
    /// where the stroke's segments meet or cross.
    visited: HashSet<(i64, i64)>,
}

/// A layer copy with a fresh identity (Duplicate Layer).
fn duplicate(layer: &Layer) -> Layer {
    let mut l = Layer::new(layer.name.clone(), layer.surface.clone());
    l.visible = layer.visible;
    l.opacity = layer.opacity;
    l.blend_mode = layer.blend_mode;
    l
}

fn transform_active(
    tab: &mut DocumentTab,
    f: impl Fn(&Surface) -> Surface,
    name: &str,
    icon: Icon,
) {
    let i = tab.doc.active_index();
    tab.doc.modify_layer(i, None, |l| l.surface = f(&l.surface));
    tab.commit(name, icon);
}

fn transform_all(
    tab: &mut DocumentTab,
    f: impl Fn(&Surface) -> Surface + Sync,
    name: &str,
    icon: Icon,
) {
    let layers = map_layers(&tab.doc, |l| f(&l.surface));
    let (w, h) = (layers[0].surface.width(), layers[0].surface.height());
    tab.doc.replace(w, h, layers);
    tab.commit(name, icon);
}

impl PaintNotNetApp {
    /// Whether a document command can run on the active image right now.
    pub(crate) fn document_command_available(&self, command: Command) -> Option<bool> {
        let tab = self.workspace.active();
        let n = tab.map_or(0, |t| t.doc.layers().len());
        let a = tab.map_or(0, |t| t.doc.active_index());
        let has = tab.is_some();
        Some(match command {
            Command::Undo => tab.is_some_and(|t| t.history.can_undo()),
            Command::Redo => tab.is_some_and(|t| t.history.can_redo()),
            Command::AddNewLayer
            | Command::DuplicateLayer
            | Command::ToggleLayerVisibility
            | Command::ImportFromFile => has,
            Command::DeleteLayer => n > 1,
            Command::MergeLayerDown
            | Command::MoveLayerDown
            | Command::MoveLayerToBottom
            | Command::GoToLayerBelow
            | Command::GoToBottomLayer => has && a > 0,
            Command::MoveLayerUp
            | Command::MoveLayerToTop
            | Command::GoToLayerAbove
            | Command::GoToTopLayer => has && a + 1 < n,
            Command::FlipLayerHorizontal
            | Command::FlipLayerVertical
            | Command::RotateLayer180
            | Command::RotateZoom
            | Command::LayerProperties => has,
            Command::FlipImageHorizontal
            | Command::FlipImageVertical
            | Command::RotateImage90Clockwise
            | Command::RotateImage90CounterClockwise
            | Command::RotateImage180
            | Command::Resize
            | Command::CanvasSize => has,
            Command::Flatten => n > 1,
            _ => return None,
        })
    }

    /// Run a document command. Returns false if `command` isn't one.
    pub(crate) fn document_command(&mut self, command: Command) -> bool {
        if !matches!(self.document_command_available(command), Some(true)) {
            return self.document_command_available(command).is_some();
        }
        if let Command::ImportFromFile = command {
            let key = self.workspace.active().map(|t| t.key);
            if let Some(key) = key {
                self.files.pick_import(key, self.last_dir.clone());
            }
            return true;
        }
        let Some(tab) = self.workspace.active_mut() else {
            return true;
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        let n = tab.doc.layers().len();
        let a = tab.doc.active_index();
        match command {
            Command::Undo => {
                tab.undo();
            }
            Command::Redo => {
                tab.redo();
            }
            Command::AddNewLayer => {
                tab.doc.insert_layer(
                    a + 1,
                    Layer::new(format!("Layer {}", n + 1), Surface::new_transparent(w, h)),
                );
                tab.commit("New Layer", Icon::AddLayer);
            }
            Command::DeleteLayer => {
                tab.doc.remove_layer(a);
                tab.commit("Delete Layer", Icon::DeleteLayer);
            }
            Command::DuplicateLayer => {
                let copy = duplicate(tab.doc.layer(a));
                tab.doc.insert_layer(a + 1, copy);
                tab.commit("Duplicate Layer", Icon::DuplicateLayer);
            }
            Command::MergeLayerDown => {
                let top = tab.doc.layer(a).clone();
                tab.doc
                    .modify_layer(a - 1, None, |bottom| transform::merge_down(&top, bottom));
                tab.doc.remove_layer(a);
                tab.doc.set_active_index(a - 1);
                tab.commit("Merge Layer Down", Icon::MergeLayerDown);
            }
            Command::ToggleLayerVisibility => {
                tab.doc.modify_layer(a, None, |l| l.visible = !l.visible);
                tab.commit("Layer Visibility", Icon::LayerProperties);
            }
            Command::FlipLayerHorizontal => transform_active(
                tab,
                transform::flip_horizontal,
                "Flip Horizontal",
                Icon::FlipLayerHorizontal,
            ),
            Command::FlipLayerVertical => transform_active(
                tab,
                transform::flip_vertical,
                "Flip Vertical",
                Icon::FlipLayerVertical,
            ),
            Command::RotateLayer180 => transform_active(
                tab,
                |s| transform::rotate_quarter(s, 2),
                "Rotate 180°",
                Icon::Rotate180,
            ),
            Command::GoToTopLayer => tab.doc.set_active_index(n - 1),
            Command::GoToLayerAbove => tab.doc.set_active_index(a + 1),
            Command::GoToLayerBelow => tab.doc.set_active_index(a - 1),
            Command::GoToBottomLayer => tab.doc.set_active_index(0),
            Command::MoveLayerUp => {
                tab.doc.move_layer(a, a + 1);
                tab.commit("Move Layer Up", Icon::MoveLayerUp);
            }
            Command::MoveLayerDown => {
                tab.doc.move_layer(a, a - 1);
                tab.commit("Move Layer Down", Icon::MoveLayerDown);
            }
            Command::MoveLayerToTop => {
                tab.doc.move_layer(a, n - 1);
                tab.commit("Move Layer Up", Icon::MoveLayerUp);
            }
            Command::MoveLayerToBottom => {
                tab.doc.move_layer(a, 0);
                tab.commit("Move Layer Down", Icon::MoveLayerDown);
            }
            Command::FlipImageHorizontal => transform_all(
                tab,
                transform::flip_horizontal,
                "Flip Horizontal (all)",
                Icon::FlipImageHorizontal,
            ),
            Command::FlipImageVertical => transform_all(
                tab,
                transform::flip_vertical,
                "Flip Vertical (all)",
                Icon::FlipImageVertical,
            ),
            Command::RotateImage90Clockwise => transform_all(
                tab,
                |s| transform::rotate_quarter(s, 1),
                "Rotate 90° Clockwise",
                Icon::Rotate90Cw,
            ),
            Command::RotateImage90CounterClockwise => transform_all(
                tab,
                |s| transform::rotate_quarter(s, 3),
                "Rotate 90° Counter-Clockwise",
                Icon::Rotate90Ccw,
            ),
            Command::RotateImage180 => transform_all(
                tab,
                |s| transform::rotate_quarter(s, 2),
                "Rotate 180°",
                Icon::Rotate180,
            ),
            Command::Flatten => {
                let flat = transform::flatten_layers(&tab.doc);
                tab.doc.replace(w, h, vec![flat]);
                tab.commit("Flatten", Icon::Flatten);
            }
            Command::LayerProperties => {
                self.doc_dialog = Some(DocDialog::layer_properties(tab.key, &tab.doc))
            }
            Command::RotateZoom => {
                self.doc_dialog = Some(DocDialog::rotate_zoom(tab.key, &tab.doc))
            }
            Command::Resize => self.doc_dialog = Some(DocDialog::resize(tab.key, &tab.doc)),
            Command::CanvasSize => {
                self.doc_dialog = Some(DocDialog::canvas_size(tab.key, &tab.doc))
            }
            _ => return false,
        }
        true
    }

    /// Show the open document dialog, previewing changes and committing on OK.
    pub(crate) fn run_doc_dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.doc_dialog.as_mut() else {
            return;
        };
        let outcome = dialog.show(ctx);
        let (primary, secondary) = (self.settings.primary, self.settings.secondary);
        let key = dialog.key;
        match outcome {
            Outcome::Pending { changed: true } if dialog.live_preview() => {
                let preview = dialog.apply(primary, secondary);
                if let Some(tab) = self.workspace.tab_mut(key) {
                    tab.doc = preview;
                }
            }
            Outcome::Pending { .. } => {}
            Outcome::Ok => {
                let dialog = self.doc_dialog.take().expect("open");
                let result = dialog.apply(primary, secondary);
                if let Some(tab) = self.workspace.tab_mut(key) {
                    let unchanged = result.revision() == dialog.original.revision();
                    tab.doc = result;
                    if !unchanged {
                        let icon = match dialog.kind {
                            doc_dialogs::Kind::LayerProperties(_) => Icon::LayerProperties,
                            doc_dialogs::Kind::Resize(_) => Icon::Resize,
                            doc_dialogs::Kind::CanvasSize(_) => Icon::CanvasSize,
                            doc_dialogs::Kind::RotateZoom(_) => Icon::RotateZoom,
                        };
                        tab.commit(dialog.history_name(), icon);
                    }
                }
            }
            Outcome::Cancel => {
                let dialog = self.doc_dialog.take().expect("open");
                if let Some(tab) = self.workspace.tab_mut(key) {
                    tab.doc = dialog.original;
                }
            }
        }
    }

    /// Layers > Import From File: the image becomes a new layer named after the file. A
    /// larger image grows the canvas (anchored top-left), as paint.net offers.
    pub(crate) fn import_layer(&mut self, key: u64, name: String, imported: Document) {
        let Some(tab) = self.workspace.tab_mut(key) else {
            return;
        };
        let (iw, ih) = (imported.width(), imported.height());
        let (w, h) = (tab.doc.width().max(iw), tab.doc.height().max(ih));
        if (w, h) != (tab.doc.width(), tab.doc.height()) {
            let layers = map_layers(&tab.doc, |l| {
                transform::canvas_resize(
                    &l.surface,
                    w,
                    h,
                    transform::Anchor::TopLeft,
                    ColorBgra::TRANSPARENT,
                )
            });
            tab.doc.replace(w, h, layers);
        }
        let mut surface = imported.layer(0).surface.clone();
        if (iw, ih) != (w, h) {
            surface = transform::canvas_resize(
                &surface,
                w,
                h,
                transform::Anchor::TopLeft,
                ColorBgra::TRANSPARENT,
            );
        }
        let a = tab.doc.active_index();
        tab.doc.insert_layer(a + 1, Layer::new(name, surface));
        tab.commit("Import From File", Icon::ImportFromFile);
    }

    /// Route canvas pointer events to the active tool.
    pub(crate) fn tool_events(&mut self, events: Vec<ToolEvent>) {
        use pnn_tools::ToolKind;
        for event in events {
            match self.tool {
                ToolKind::Pencil => self.pencil(event),
                ToolKind::RectangleSelect
                | ToolKind::EllipseSelect
                | ToolKind::LassoSelect
                | ToolKind::MagicWand => self.selection_tool_event(event),
                _ => {}
            }
        }
    }

    fn pencil(&mut self, event: ToolEvent) {
        match event {
            ToolEvent::Down { pos, button, .. } => {
                let Some(tab) = self.workspace.active() else {
                    return;
                };
                let color = match button {
                    ToolButton::Primary => self.settings.primary,
                    ToolButton::Secondary => self.settings.secondary,
                };
                let p = (pos[0].floor() as i64, pos[1].floor() as i64);
                self.stroke = Some(PencilStroke {
                    key: tab.key,
                    last: p,
                    color,
                    blend: self.settings.blend,
                    visited: HashSet::new(),
                });
                self.pencil_segment(p, p);
            }
            ToolEvent::Move { pos } => {
                let Some(last) = self.stroke.as_ref().map(|s| s.last) else {
                    return;
                };
                let p = (pos[0].floor() as i64, pos[1].floor() as i64);
                if p != last {
                    self.pencil_segment(last, p);
                }
            }
            ToolEvent::Up { .. } => {
                if let Some(stroke) = self.stroke.take()
                    && let Some(tab) = self.workspace.tab_mut(stroke.key)
                {
                    tab.commit("Pencil", Icon::Pencil);
                }
            }
        }
    }

    fn pencil_segment(&mut self, from: (i64, i64), to: (i64, i64)) {
        let Some(stroke) = self.stroke.as_mut() else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(stroke.key) else {
            return;
        };
        let (w, h) = (tab.doc.width() as i64, tab.doc.height() as i64);
        let points: Vec<(i64, i64)> = line(from, to)
            .into_iter()
            .filter(|&(x, y)| x >= 0 && y >= 0 && x < w && y < h && stroke.visited.insert((x, y)))
            .collect();
        stroke.last = to;
        if points.is_empty() {
            return;
        }
        let (x0, x1) = (
            points.iter().map(|p| p.0).min().unwrap(),
            points.iter().map(|p| p.0).max().unwrap(),
        );
        let (y0, y1) = (
            points.iter().map(|p| p.1).min().unwrap(),
            points.iter().map(|p| p.1).max().unwrap(),
        );
        let dirty = Rect {
            x: x0 as u32,
            y: y0 as u32,
            width: (x1 - x0 + 1) as u32,
            height: (y1 - y0 + 1) as u32,
        };
        let (color, blend) = (stroke.color, stroke.blend);
        let antialias = self.settings.selection_quality == pnn_tools::SelectionQuality::Antialiased;
        let Some(tab) = self
            .workspace
            .tab_mut(self.stroke.as_ref().map_or(0, |s| s.key))
        else {
            return;
        };
        // Painting is clipped to the selection; antialiased edges scale the paint's alpha.
        let mask = tab.selection_mask(antialias);
        let a = tab.doc.active_index();
        tab.doc.modify_layer(a, Some(dirty), |l| {
            for (x, y) in points {
                let (x, y) = (x as u32, y as u32);
                let coverage = mask.as_ref().map_or(255, |m| m.get(x, y));
                if coverage == 0 {
                    continue;
                }
                let old = l.surface.get(x, y);
                let new = match blend {
                    ToolBlend::Overwrite => color,
                    ToolBlend::Blend(mode) => mode.apply(old, color, 255),
                };
                let new = if coverage == 255 {
                    new
                } else {
                    crate::selection_tools::lerp_premultiplied(old, new, coverage)
                };
                l.surface.set(x, y, new);
            }
        });
    }
}

/// Bresenham line, both ends included.
pub fn line(from: (i64, i64), to: (i64, i64)) -> Vec<(i64, i64)> {
    let (mut x, mut y) = from;
    let dx = (to.0 - x).abs();
    let dy = -(to.1 - y).abs();
    let sx = if x < to.0 { 1 } else { -1 };
    let sy = if y < to.1 { 1 } else { -1 };
    let mut err = dx + dy;
    let mut out = Vec::with_capacity((dx - dy) as usize + 1);
    loop {
        out.push((x, y));
        if (x, y) == to {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bresenham_endpoints_and_continuity() {
        let l = line((0, 0), (5, 2));
        assert_eq!(l.first(), Some(&(0, 0)));
        assert_eq!(l.last(), Some(&(5, 2)));
        assert_eq!(l.len(), 6);
        for w in l.windows(2) {
            assert!((w[1].0 - w[0].0).abs() <= 1 && (w[1].1 - w[0].1).abs() <= 1);
        }
        assert_eq!(line((3, 3), (3, 3)), vec![(3, 3)]);
    }

    #[test]
    fn duplicate_gets_new_identity() {
        let l = Layer::new("Background", Surface::new_transparent(2, 2));
        let d = duplicate(&l);
        assert_ne!(d.id(), l.id());
        assert_eq!(d.name, "Background");
    }
}
