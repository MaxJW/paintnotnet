//! Text tool: click to place a caret, type in the primary colour, commit with Esc, Finish,
//! a click elsewhere (which starts new text), a tool change or another command.
//!
//! Behaviour from paint.net 5's docs (`TextTool.html` and the keyboard table): the text stays
//! editable until committed; the pulsing move nub below-right of the caret drags it (either
//! button); toolbar changes restyle the live text; Space doesn't pan while typing. While text
//! is live, plain keys go to the text (Enter is a new line), and only Ctrl/Alt chords reach the
//! keymap. Undo takes back live text. Rendering is in `pnn_tools::text`.

use std::sync::Arc;

use cosmic_text::{FontSystem, SwashCache};
use eframe::egui;
use pnn_core::{ColorBgra, Mask, Rect, Surface};
use pnn_tools::text::{self, LineLayout, TextStyle};
use pnn_tools::vector::{self, DrawOp, Drawn};
use pnn_tools::{TextMetric, ToolBlend};

use crate::app::PaintNotNetApp;
use crate::canvas::ToolEvent;
use crate::icons::Icon;

/// Nub grab radius in screen pixels.
const NUB_RADIUS: f64 = 8.0;
/// The move nub sits this far (screen px) right of and below the caret's foot.
const NUB_OFFSET: f64 = 10.0;

/// System fonts, loaded on first use of the Text tool.
pub struct Fonts {
    pub system: FontSystem,
    pub cache: SwashCache,
    pub families: Vec<String>,
}

impl Fonts {
    pub fn load() -> Self {
        let system = FontSystem::new();
        let mut families: Vec<String> = system
            .db()
            .faces()
            .flat_map(|f| f.families.first().map(|(n, _)| n.clone()))
            .collect();
        families.sort_by_key(|f| f.to_lowercase());
        families.dedup();
        Self {
            system,
            cache: SwashCache::new(),
            families,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Look {
    text: String,
    origin: [f32; 2],
    style: TextStyle,
    color: ColorBgra,
    op: DrawOp,
}

pub struct TextSession {
    key: u64,
    layer: usize,
    before: Surface,
    selection: Option<Arc<Mask>>,
    origin: [f32; 2],
    pub text: String,
    /// Byte offset of the caret.
    caret: usize,
    lines: Vec<LineLayout>,
    last: Option<Rect>,
    rendered: Option<Look>,
    /// Dragging the move nub: last pointer position.
    moving: Option<[f64; 2]>,
}

/// Caret and move nub to draw (image coordinates): (caret top, caret bottom, nub).
pub type TextHandles = ([f64; 2], [f64; 2], [f64; 2]);

impl PaintNotNetApp {
    fn text_look(&self, s: &TextSession) -> Look {
        let t = &self.settings;
        let dpi = self.workspace.tab(s.key).map_or(96.0, |tab| tab.doc.dpi);
        Look {
            text: s.text.clone(),
            origin: s.origin,
            style: TextStyle {
                family: t.font_family.clone(),
                size_px: text::size_px(t.font_size, t.text_metric == TextMetric::Points, dpi),
                bold: t.bold,
                italic: t.italic,
                underline: t.underline,
                strikeout: t.strikeout,
                align: t.text_align,
                antialias: t.antialiasing,
            },
            color: t.primary,
            op: match t.blend {
                ToolBlend::Overwrite => DrawOp::Overwrite,
                ToolBlend::Blend(m) => DrawOp::Blend(m),
            },
        }
    }

    /// Re-render live text if it or its style changed.
    pub(crate) fn refresh_text(&mut self) {
        let Some(s) = self.text.as_ref() else {
            return;
        };
        let look = self.text_look(s);
        if s.rendered.as_ref() == Some(&look) {
            return;
        }
        let fonts = self.fonts.get_or_insert_with(Fonts::load);
        let Some(s) = self.text.as_mut() else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(s.key) else {
            self.text = None;
            return;
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        let r = text::render(
            &mut fonts.system,
            &mut fonts.cache,
            &look.text,
            &look.style,
            look.origin,
            w,
            h,
        );
        let drawn = r.bounds.map(|b| {
            let c = look.color;
            Drawn {
                bounds: b,
                color: r
                    .coverage
                    .iter()
                    .map(|&k| c.with_alpha(((c.a as u32 * k as u32 + 127) / 255) as u8))
                    .collect(),
                solid: vec![c; r.coverage.len()],
                coverage: r.coverage.clone(),
            }
        });
        if let Some(area) = pnn_tools::brush::union(s.last, r.bounds) {
            let (before, selection) = (&s.before, s.selection.as_deref());
            tab.doc.modify_layer(s.layer, Some(area), |l| {
                vector::composite(
                    &mut l.surface,
                    before,
                    selection,
                    drawn.as_ref(),
                    area,
                    look.op,
                )
            });
        }
        s.last = r.bounds;
        s.lines = r.lines;
        s.rendered = Some(look);
    }

    pub(crate) fn text_event(&mut self, event: ToolEvent) {
        let zoom = self
            .workspace
            .active()
            .and_then(|t| t.view)
            .map_or(1.0, |v| v.zoom);
        match event {
            ToolEvent::Down { pos, .. } => {
                if let Some((_, _, nub)) = self.text_handles()
                    && (nub[0] - pos[0]).hypot(nub[1] - pos[1]) * zoom <= NUB_RADIUS
                    && let Some(s) = self.text.as_mut()
                {
                    s.moving = Some(pos);
                    return;
                }
                self.commit_text();
                let antialias = self.antialiased_selection();
                self.fonts.get_or_insert_with(Fonts::load);
                let Some(tab) = self.workspace.active_mut() else {
                    return;
                };
                let layer = tab.doc.active_index();
                self.text = Some(TextSession {
                    key: tab.key,
                    layer,
                    before: tab.doc.layer(layer).surface.clone(),
                    selection: tab.selection_mask(antialias),
                    origin: [pos[0] as f32, pos[1] as f32],
                    text: String::new(),
                    caret: 0,
                    lines: Vec::new(),
                    last: None,
                    rendered: None,
                    moving: None,
                });
            }
            ToolEvent::Move { pos } | ToolEvent::Up { pos } => {
                let up = matches!(event, ToolEvent::Up { .. });
                if let Some(s) = self.text.as_mut()
                    && let Some(last) = s.moving
                {
                    s.origin[0] += (pos[0] - last[0]) as f32;
                    s.origin[1] += (pos[1] - last[1]) as f32;
                    s.moving = (!up).then_some(pos);
                }
            }
        }
    }

    /// While text is live, take plain key presses and typed text for it, leaving Ctrl/Alt
    /// chords for the keymap.
    pub(crate) fn text_keyboard(&mut self, ctx: &egui::Context) {
        if self.text.is_none() || self.tool != pnn_tools::ToolKind::Text {
            return;
        }
        let events = ctx.input_mut(|i| {
            let mut taken = Vec::new();
            i.events.retain(|e| {
                let mine = match e {
                    egui::Event::Text(_) => true,
                    egui::Event::Key { modifiers, .. } => !modifiers.ctrl && !modifiers.alt,
                    _ => false,
                };
                if mine {
                    taken.push(e.clone());
                }
                !mine
            });
            taken
        });
        let mut commit = false;
        if let Some(s) = self.text.as_mut() {
            for e in events {
                match e {
                    egui::Event::Text(t) => {
                        s.text.insert_str(s.caret, &t);
                        s.caret += t.len();
                    }
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => edit_key(s, key, &mut commit),
                    _ => {}
                }
            }
        }
        if commit {
            self.commit_text();
        }
    }

    /// Caret and move nub for the live text on the active tab.
    pub(crate) fn text_handles(&self) -> Option<TextHandles> {
        let s = self.text.as_ref()?;
        if self.workspace.active().map(|t| t.key) != Some(s.key) {
            return None;
        }
        let zoom = self
            .workspace
            .active()
            .and_then(|t| t.view)
            .map_or(1.0, |v| v.zoom);
        let (x, top, h) = if s.lines.is_empty() {
            let size = self.settings.font_size as f64 * 96.0 / 72.0;
            (s.origin[0] as f64, s.origin[1] as f64, size * 1.2)
        } else {
            let (x, t, h) = text::caret(&s.lines, s.caret);
            (x as f64, t as f64, h as f64)
        };
        let off = NUB_OFFSET / zoom;
        Some(([x, top], [x, top + h], [x + off, top + h + off]))
    }

    /// Bake live text into history (nothing if it's empty).
    pub(crate) fn commit_text(&mut self) {
        self.refresh_text();
        let Some(s) = self.text.take() else {
            return;
        };
        if s.last.is_some()
            && let Some(tab) = self.workspace.tab_mut(s.key)
        {
            tab.commit("Text", Icon::Text);
        }
    }

    /// Drop live text, restoring the layer (Undo).
    pub(crate) fn cancel_text(&mut self) -> bool {
        let Some(s) = self.text.take() else {
            return false;
        };
        if let (Some(r), Some(tab)) = (s.last, self.workspace.tab_mut(s.key)) {
            tab.doc.modify_layer(s.layer, Some(r), |l| {
                vector::composite(&mut l.surface, &s.before, None, None, r, DrawOp::Overwrite)
            });
        }
        true
    }
}

/// Caret movement and editing keys.
fn edit_key(s: &mut TextSession, key: egui::Key, commit: &mut bool) {
    use egui::Key as K;
    let prev = |t: &str, i: usize| t[..i].char_indices().last().map_or(0, |(j, _)| j);
    let next = |t: &str, i: usize| t[i..].chars().next().map_or(i, |c| i + c.len_utf8());
    let line_start = |t: &str, i: usize| t[..i].rfind('\n').map_or(0, |j| j + 1);
    let line_end = |t: &str, i: usize| t[i..].find('\n').map_or(t.len(), |j| i + j);
    match key {
        K::Escape => *commit = true,
        K::Enter => {
            s.text.insert(s.caret, '\n');
            s.caret += 1;
        }
        K::Backspace if s.caret > 0 => {
            let p = prev(&s.text, s.caret);
            s.text.replace_range(p..s.caret, "");
            s.caret = p;
        }
        K::Delete if s.caret < s.text.len() => {
            let n = next(&s.text, s.caret);
            s.text.replace_range(s.caret..n, "");
        }
        K::ArrowLeft => s.caret = prev(&s.text, s.caret),
        K::ArrowRight => s.caret = next(&s.text, s.caret),
        K::Home => s.caret = line_start(&s.text, s.caret),
        K::End => s.caret = line_end(&s.text, s.caret),
        K::ArrowUp | K::ArrowDown => {
            // Same character column on the line above/below.
            let start = line_start(&s.text, s.caret);
            let col = s.text[start..s.caret].chars().count();
            let target = if key == K::ArrowUp {
                (start > 0).then(|| line_start(&s.text, start - 1))
            } else {
                let end = line_end(&s.text, s.caret);
                (end < s.text.len()).then_some(end + 1)
            };
            if let Some(t) = target {
                let end = line_end(&s.text, t);
                s.caret = s.text[t..end]
                    .char_indices()
                    .nth(col)
                    .map_or(end, |(j, _)| t + j);
            }
        }
        _ => {}
    }
}
