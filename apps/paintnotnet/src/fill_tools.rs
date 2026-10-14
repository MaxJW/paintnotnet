//! Paint Bucket and Gradient.
//!
//! Paint Bucket follows paint.net 3.36 `tools/PaintBucketTool.cs` and `FloodToolBase.cs`
//! (MIT, see NOTICE): flood from the clicked pixel with the Magic Wand's tolerance test,
//! contiguous or global (Shift flips the toolbar mode), limited to the selection and only
//! when the click is inside it; filled with the brush (solid or hatch; right button swaps the
//! colours). Sampling Image (5.x) floods on the flattened image but fills the active layer.
//!
//! Gradient follows 3.36 `tools/GradientTool.cs`: drag from start to end; the two nubs stay
//! editable until another gradient is started, Enter, or a tool or document change; dragging
//! a nub moves it, Shift snaps to 15°, the right button draws with the colours swapped
//! (and toggles the swap when it grabs a nub). Enter and Esc commit (5.x docs: "Esc, Enter,
//! Ctrl+D, toolbar Finish: commit … tool drawing"; 3.36 cancelled on Esc); Undo takes a live
//! gradient back.

use std::sync::Arc;

use pnn_core::{Mask, Rect, Surface};
use pnn_tools::brush::{self, StrokeOp};
use pnn_tools::gradient::{self, Gradient, GradientOp};
use pnn_tools::{
    FillStyle, FloodMode, GradientColorMode, SamplingMode, ToleranceAlphaMode, ToolBlend,
};

use crate::app::PaintNotNetApp;
use crate::canvas::{ToolButton, ToolEvent};
use crate::icons::Icon;

/// Nub grab radius in screen pixels (as the move tools).
const NUB_RADIUS: f64 = 6.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Grip {
    /// Drawing a new gradient: the end follows the pointer.
    New,
    Start,
    End,
}

/// A gradient that is drawn but still editable.
pub struct GradientSession {
    key: u64,
    layer: usize,
    before: Surface,
    selection: Option<Arc<Mask>>,
    pub start: [f32; 2],
    pub end: [f32; 2],
    swap: bool,
    grip: Option<Grip>,
    /// What was last rendered, to re-render when colours or options change.
    rendered: Option<(Gradient, GradientOp)>,
}

impl PaintNotNetApp {
    pub(crate) fn paint_bucket_event(&mut self, event: ToolEvent) {
        let ToolEvent::Down {
            pos,
            button,
            modifiers,
        } = event
        else {
            return;
        };
        let antialias = self.antialiased_selection();
        let s = &self.settings;
        let mut contiguous = s.flood_mode == FloodMode::Contiguous;
        if modifiers.shift {
            contiguous = !contiguous;
        }
        let (fore, back) = match button {
            ToolButton::Primary => (s.primary, s.secondary),
            ToolButton::Secondary => (s.secondary, s.primary),
        };
        let op = match s.blend {
            ToolBlend::Overwrite => StrokeOp::Overwrite,
            ToolBlend::Blend(m) => StrokeOp::Blend(m),
        };
        let (fill, tolerance, sampling) = (s.fill, s.tolerance, s.sampling);
        let tol = pnn_core::selection::Tolerance {
            pct: tolerance,
            premultiplied: s.tolerance_alpha == ToleranceAlphaMode::Premultiplied,
            perceptual: s.tolerance_space == pnn_tools::ToleranceSpace::Perceptual,
        };
        let Some(tab) = self.workspace.active_mut() else {
            return;
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        if pos[0] < 0.0 || pos[1] < 0.0 || pos[0] >= w as f64 || pos[1] >= h as f64 {
            return;
        }
        let seed = (pos[0] as u32, pos[1] as u32);
        let selection = tab.selection_mask(antialias);
        if selection
            .as_ref()
            .is_some_and(|m| m.get(seed.0, seed.1) == 0)
        {
            return;
        }
        let layer = tab.doc.active_index();
        let pixels = match sampling {
            SamplingMode::Layer => tab.doc.layer(layer).surface.to_pixels(),
            SamplingMode::Image => pnn_core::flatten(&tab.doc).to_pixels(),
        };
        let mut stencil = pnn_core::selection::flood_stencil(&pixels, w, h, seed, tol, contiguous);
        if let Some(m) = &selection {
            // 3.36 limits the flood to the selection; a contiguous fill doesn't leak
            // through unselected pixels.
            if contiguous {
                stencil = limited_flood(&pixels, w, h, seed, tol, m);
            } else {
                for (i, s) in stencil.iter_mut().enumerate() {
                    *s &= m.get(i as u32 % w, i as u32 / w) > 0;
                }
            }
        }
        let mut mask: Mask = pnn_core::TiledBuffer::new(w, h, 0);
        let coverage: Vec<u8> = stencil.iter().map(|&s| if s { 255 } else { 0 }).collect();
        mask.write_pixels(&coverage);
        mask.compact();
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for (y, row) in stencil.chunks(w as usize).enumerate() {
            if let (Some(a), Some(b)) = (row.iter().position(|&s| s), row.iter().rposition(|&s| s))
            {
                (x0, x1) = (x0.min(a as u32), x1.max(b as u32));
                (y0, y1) = (y0.min(y as u32), y1.max(y as u32));
            }
        }
        let bounds = (x0 <= x1).then(|| Rect {
            x: x0,
            y: y0,
            width: x1 - x0 + 1,
            height: y1 - y0 + 1,
        });
        let Some(r) = bounds else {
            return;
        };
        let before = tab.doc.layer(layer).surface.clone();
        tab.doc.modify_layer(layer, Some(r), |l| {
            brush::composite(
                &mut l.surface,
                &before,
                &mask,
                selection.as_deref(),
                r,
                op,
                |x, y| match fill {
                    FillStyle::Solid => fore,
                    FillStyle::Hatch(i) if brush::hatch_on(i, x, y) => fore,
                    FillStyle::Hatch(_) => back,
                },
            )
        });
        tab.commit("Paint Bucket", Icon::PaintBucket);
    }

    /// The gradient the current settings describe for `session`.
    fn gradient_spec(&self, session: &GradientSession) -> (Gradient, GradientOp) {
        let s = &self.settings;
        let alpha_only = s.gradient_color_mode == GradientColorMode::Transparency;
        let (mut start_color, mut end_color) = (s.primary, s.secondary);
        if session.swap {
            if alpha_only {
                // 3.36: reverse and invert the alphas.
                let a = start_color.a;
                start_color = start_color.with_alpha(255 - end_color.a);
                end_color = end_color.with_alpha(255 - a);
            } else {
                std::mem::swap(&mut start_color, &mut end_color);
            }
        }
        let (mut start, mut end) = (session.start, session.end);
        if self.modifiers.shift && session.grip.is_some() {
            if session.grip == Some(Grip::Start) {
                start = gradient::constrain(end, start);
            } else {
                end = gradient::constrain(start, end);
            }
        }
        let op = match s.blend {
            ToolBlend::Overwrite => GradientOp::Overwrite,
            ToolBlend::Blend(m) => GradientOp::Blend(m),
        };
        (
            Gradient {
                kind: s.gradient_type,
                repeat: s.gradient_repeat,
                start,
                end,
                start_color,
                end_color,
                alpha_only,
            },
            op,
        )
    }

    /// Re-render the live gradient if its points, colours or options changed.
    pub(crate) fn refresh_gradient(&mut self) {
        crate::diagnostics::timed("gradient", || self.refresh_gradient_now())
    }

    fn refresh_gradient_now(&mut self) {
        let Some(session) = self.gradient.as_ref() else {
            return;
        };
        let spec = self.gradient_spec(session);
        if session.rendered == Some(spec) {
            return;
        }
        let Some(session) = self.gradient.as_mut() else {
            return;
        };
        let Some(tab) = self.workspace.tab_mut(session.key) else {
            self.gradient = None;
            return;
        };
        let (w, h) = (tab.doc.width(), tab.doc.height());
        let r = Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        };
        let (before, selection) = (&session.before, session.selection.as_deref());
        tab.doc.modify_layer(session.layer, None, |l| {
            spec.0.render(&mut l.surface, before, selection, r, spec.1)
        });
        session.rendered = Some(spec);
    }

    pub(crate) fn gradient_event(&mut self, event: ToolEvent) {
        let zoom = self
            .workspace
            .active()
            .and_then(|t| t.view)
            .map_or(1.0, |v| v.zoom);
        let near = |a: [f32; 2], p: [f64; 2]| {
            let d = ((a[0] as f64 - p[0]).powi(2) + (a[1] as f64 - p[1]).powi(2)).sqrt();
            d * zoom <= NUB_RADIUS
        };
        match event {
            ToolEvent::Down { pos, button, .. } => {
                let p = [pos[0] as f32, pos[1] as f32];
                if let Some(s) = self.gradient.as_mut()
                    && self.workspace.active().map(|t| t.key) == Some(s.key)
                {
                    // 3.36 `PointToNub`: the start nub only when it's the nearer one.
                    let ds = (s.start[0] as f64 - pos[0]).hypot(s.start[1] as f64 - pos[1]);
                    let de = (s.end[0] as f64 - pos[0]).hypot(s.end[1] as f64 - pos[1]);
                    let grip = if near(s.start, pos) && ds < de {
                        Some(Grip::Start)
                    } else if near(s.end, pos) {
                        Some(Grip::End)
                    } else {
                        None
                    };
                    if let Some(g) = grip {
                        s.grip = Some(g);
                        match g {
                            Grip::Start => s.start = p,
                            _ => s.end = p,
                        }
                        if button == ToolButton::Secondary {
                            s.swap = !s.swap;
                        }
                        self.refresh_gradient();
                        return;
                    }
                }
                self.commit_gradient();
                let antialias = self.antialiased_selection();
                let Some(tab) = self.workspace.active_mut() else {
                    return;
                };
                let layer = tab.doc.active_index();
                self.gradient = Some(GradientSession {
                    key: tab.key,
                    layer,
                    before: tab.doc.layer(layer).surface.clone(),
                    selection: tab.selection_mask(antialias),
                    start: p,
                    end: p,
                    swap: button == ToolButton::Secondary,
                    grip: Some(Grip::New),
                    rendered: None,
                });
                self.refresh_gradient();
            }
            ToolEvent::Move { pos } => {
                let Some(s) = self.gradient.as_mut() else {
                    return;
                };
                let p = [pos[0] as f32, pos[1] as f32];
                // Rendered once per frame by `refresh_gradient`, however many moves arrive.
                match s.grip {
                    Some(Grip::Start) => s.start = p,
                    Some(Grip::End | Grip::New) => s.end = p,
                    None => {}
                }
            }
            ToolEvent::Up { pos } => {
                if let Some(s) = self.gradient.as_mut()
                    && s.grip.is_some()
                {
                    let p = [pos[0] as f32, pos[1] as f32];
                    match s.grip {
                        Some(Grip::Start) => s.start = p,
                        _ => s.end = p,
                    }
                    // Keep the constrained points once the drag ends.
                    self.refresh_gradient();
                    if let Some(s) = self.gradient.as_mut()
                        && let Some((g, _)) = s.rendered
                    {
                        (s.start, s.end) = (g.start, g.end);
                        s.grip = None;
                    }
                }
            }
        }
    }

    /// Bake the live gradient into history (Enter, new gradient, tool or document change).
    pub(crate) fn commit_gradient(&mut self) {
        self.refresh_gradient();
        let Some(s) = self.gradient.take() else {
            return;
        };
        if let Some(tab) = self.workspace.tab_mut(s.key) {
            tab.commit("Gradient", Icon::Gradient);
        }
    }

    /// Drop the live gradient, restoring the layer (Undo).
    pub(crate) fn cancel_gradient(&mut self) -> bool {
        let Some(s) = self.gradient.take() else {
            return false;
        };
        if let Some(tab) = self.workspace.tab_mut(s.key) {
            tab.doc
                .modify_layer(s.layer, None, |l| l.surface = s.before.clone());
        }
        true
    }

    /// Gradient nubs to draw (image coordinates).
    pub(crate) fn gradient_nubs(&self) -> Option<[[f64; 2]; 2]> {
        let s = self.gradient.as_ref()?;
        if self.workspace.active().map(|t| t.key) != Some(s.key) {
            return None;
        }
        let (g, _) = s.rendered?;
        Some([
            [g.start[0] as f64, g.start[1] as f64],
            [g.end[0] as f64, g.end[1] as f64],
        ])
    }
}

/// Contiguous flood that only spreads through selected pixels (3.36's `limitRegion`).
fn limited_flood(
    pixels: &[pnn_core::ColorBgra],
    w: u32,
    h: u32,
    seed: (u32, u32),
    tolerance: pnn_core::selection::Tolerance,
    selection: &Mask,
) -> Vec<bool> {
    // Every matching pixel, then the 4-connected part of (matching ∧ selected) holding the
    // seed.
    let candidates = pnn_core::selection::flood_stencil(pixels, w, h, seed, tolerance, false);
    let (wu, hu) = (w as usize, h as usize);
    let ok = |i: usize| candidates[i] && selection.get((i % wu) as u32, (i / wu) as u32) > 0;
    let mut out = vec![false; wu * hu];
    let mut stack = vec![seed.1 as usize * wu + seed.0 as usize];
    while let Some(i) = stack.pop() {
        if out[i] || !ok(i) {
            continue;
        }
        out[i] = true;
        let (x, y) = (i % wu, i / wu);
        if x > 0 {
            stack.push(i - 1);
        }
        if x + 1 < wu {
            stack.push(i + 1);
        }
        if y > 0 {
            stack.push(i - wu);
        }
        if y + 1 < hu {
            stack.push(i + wu);
        }
    }
    out
}
