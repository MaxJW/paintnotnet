//! Floating utility windows (Tools, History, Layers, Colors) inside the main window.
//!
//! Snapping follows paint.net 3.36 `Core/SnapManager.cs` (MIT, see NOTICE): windows snap to
//! the workspace's inner edges and to each other's outer edges within 15 px (30 px to pull a
//! snapped window free), keep a 3 px gap, and stay attached to the edges they snapped to when
//! the main window resizes. Default parking: Tools top-left, History top-right, Layers
//! bottom-right, Colors bottom-left (`AppWorkspace.ResetFloatingForm`).

use std::collections::HashMap;

use eframe::egui::{self, Pos2, Rect, Sense, Vec2, pos2, vec2};

use crate::commands::Panel;
use crate::theme::Palette;

const SNAP_PROXIMITY: f32 = 15.0;
const SNAP_GAP: f32 = 3.0;
const TITLE_HEIGHT: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HEdge {
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VEdge {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Workspace,
    Panel(Panel),
}

/// Where a window is attached. Unsnapped axes keep an offset from the target's top/left.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Snap {
    target: Target,
    h: Option<HEdge>,
    v: Option<VEdge>,
    x_offset: f32,
    y_offset: f32,
}

#[derive(Clone, Debug)]
struct PanelState {
    visible: bool,
    size: Vec2,
    /// Position when not snapped (relative to the workspace's top-left).
    free: Vec2,
    snap: Option<Snap>,
    /// Unsnapped position while the title bar is being dragged.
    drag_raw: Option<Pos2>,
    rect: Rect,
}

pub struct Panels {
    states: HashMap<Panel, PanelState>,
    workspace: Rect,
}

/// Default sizes, measured from paint.net 5's main window screenshot.
pub fn default_size(panel: Panel) -> Vec2 {
    match panel {
        Panel::Tools => vec2(52.0, 282.0),
        Panel::History => vec2(178.0, 300.0),
        Panel::Layers => vec2(178.0, 260.0),
        Panel::Colors => vec2(212.0, 250.0),
    }
}

fn default_snap(panel: Panel) -> Snap {
    let (h, v) = match panel {
        Panel::Tools => (HEdge::Top, VEdge::Left),
        Panel::History => (HEdge::Top, VEdge::Right),
        Panel::Layers => (HEdge::Bottom, VEdge::Right),
        Panel::Colors => (HEdge::Bottom, VEdge::Left),
    };
    Snap {
        target: Target::Workspace,
        h: Some(h),
        v: Some(v),
        x_offset: SNAP_GAP,
        y_offset: SNAP_GAP,
    }
}

impl Default for Panels {
    fn default() -> Self {
        let states = Panel::ALL
            .iter()
            .map(|&p| {
                (
                    p,
                    PanelState {
                        visible: true,
                        size: default_size(p),
                        free: Vec2::ZERO,
                        snap: Some(default_snap(p)),
                        drag_raw: None,
                        rect: Rect::NOTHING,
                    },
                )
            })
            .collect();
        Self {
            states,
            workspace: Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 700.0)),
        }
    }
}

impl Panels {
    pub fn visible(&self, panel: Panel) -> bool {
        self.states[&panel].visible
    }

    pub fn toggle(&mut self, panel: Panel) {
        let s = self.states.get_mut(&panel).expect("panel");
        s.visible = !s.visible;
    }

    pub fn set_visible(&mut self, panel: Panel, visible: bool) {
        self.states.get_mut(&panel).expect("panel").visible = visible;
    }

    /// Ctrl+Shift+F5…F8: original position and size, shown.
    pub fn reset(&mut self, panel: Panel) {
        let s = self.states.get_mut(&panel).expect("panel");
        s.visible = true;
        s.size = default_size(panel);
        s.snap = Some(default_snap(panel));
        s.drag_raw = None;
    }

    pub fn rect(&self, panel: Panel) -> Rect {
        self.states[&panel].rect
    }

    /// Size changes from content (Colors "More >>") keep the snapped edges in place.
    pub fn set_size(&mut self, panel: Panel, size: Vec2) {
        self.states.get_mut(&panel).expect("panel").size = size;
    }

    pub fn size(&self, panel: Panel) -> Vec2 {
        self.states[&panel].size
    }

    fn target_rect(&self, target: Target) -> Rect {
        match target {
            Target::Workspace => self.workspace,
            Target::Panel(p) => self.states[&p].rect,
        }
    }

    /// Position for a window of `size` under `snap`.
    fn place(&self, snap: &Snap, size: Vec2) -> Pos2 {
        let t = self.target_rect(snap.target);
        let interior = snap.target == Target::Workspace;
        let x = match (snap.v, interior) {
            (Some(VEdge::Left), true) => t.min.x + snap.x_offset,
            (Some(VEdge::Right), true) => t.max.x - snap.x_offset - size.x,
            (Some(VEdge::Left), false) => t.min.x - snap.x_offset - size.x,
            (Some(VEdge::Right), false) => t.max.x + snap.x_offset,
            (None, _) => t.min.x + snap.x_offset,
        };
        let y = match (snap.h, interior) {
            (Some(HEdge::Top), true) => t.min.y + snap.y_offset,
            (Some(HEdge::Bottom), true) => t.max.y - snap.y_offset - size.y,
            (Some(HEdge::Top), false) => t.min.y - snap.y_offset - size.y,
            (Some(HEdge::Bottom), false) => t.max.y + snap.y_offset,
            (None, _) => t.min.y + snap.y_offset,
        };
        pos2(x, y)
    }

    /// Port of `SnapManager.DetermineNewSnapDescription` for one candidate target.
    fn snap_to(&self, rect: Rect, target: Target, already: bool) -> Option<Snap> {
        let t = self.target_rect(target);
        if !t.is_positive() {
            return None;
        }
        let proximity = if already {
            SNAP_PROXIMITY * 2.0
        } else {
            SNAP_PROXIMITY
        };
        let overlap = |a0: f32, a1: f32, b0: f32, b1: f32| a0 <= b1 && b0 <= a1;
        let vert = overlap(rect.min.y, rect.max.y, t.min.y, t.max.y);
        let horiz = overlap(rect.min.x, rect.max.x, t.min.x, t.max.x);
        let interior = target == Target::Workspace;
        let (left, right, top, bottom) = if interior {
            (
                (rect.min.x - t.min.x).abs(),
                (rect.max.x - t.max.x).abs(),
                (rect.min.y - t.min.y).abs(),
                (rect.max.y - t.max.y).abs(),
            )
        } else {
            (
                (rect.min.x - t.max.x).abs(),
                (rect.max.x - t.min.x).abs(),
                (rect.min.y - t.max.y).abs(),
                (rect.max.y - t.min.y).abs(),
            )
        };
        let close = |d: f32| d < proximity;
        let v = if !vert {
            None
        } else if (close(left) && !interior) || (close(right) && interior) {
            Some(VEdge::Right)
        } else if (close(right) && !interior) || (close(left) && interior) {
            Some(VEdge::Left)
        } else {
            None
        };
        let h = if !horiz {
            None
        } else if (close(top) && !interior) || (close(bottom) && interior) {
            Some(HEdge::Bottom)
        } else if (close(bottom) && !interior) || (close(top) && interior) {
            Some(HEdge::Top)
        } else {
            None
        };
        if h.is_none() && v.is_none() {
            return None;
        }
        // Snapped to one edge only: keep the other axis where it is.
        let (h, y_offset) = match h {
            Some(h) => (Some(h), SNAP_GAP),
            None if interior => (Some(HEdge::Top), rect.min.y - t.min.y),
            None => (None, rect.min.y - t.min.y),
        };
        let (v, x_offset) = match v {
            Some(v) => (Some(v), SNAP_GAP),
            None if interior => (Some(VEdge::Left), rect.min.x - t.min.x),
            None => (None, rect.min.x - t.min.x),
        };
        Some(Snap {
            target,
            h,
            v,
            x_offset,
            y_offset,
        })
    }

    /// Show every visible panel. `contents` draws a panel's body.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        workspace: Rect,
        palette: &Palette,
        mut contents: impl FnMut(Panel, &mut egui::Ui),
    ) {
        self.workspace = workspace;
        // Panels snapped to other panels are placed after their targets.
        let order = [Panel::Tools, Panel::Colors, Panel::History, Panel::Layers];
        for panel in order {
            if !self.states[&panel].visible {
                continue;
            }
            let state = self.states[&panel].clone();
            let pos = match (state.drag_raw, &state.snap) {
                (Some(raw), _) => self.drag_snap(panel, raw, state.size),
                (None, Some(snap)) => self.place(snap, state.size),
                (None, None) => workspace.min + state.free,
            };
            let pos = clamp_into(pos, state.size, workspace);
            let resizable = matches!(panel, Panel::History | Panel::Layers);
            let frame = egui::Frame::window(&ctx.global_style())
                .fill(palette.window)
                .stroke(egui::Stroke::new(1.0, palette.window_border))
                .inner_margin(egui::Margin::same(0))
                .corner_radius(0);
            let mut close = false;
            let mut drag_delta = None;
            let response = egui::Window::new(panel.name())
                .id(egui::Id::new(("panel", panel.name())))
                .title_bar(false)
                .movable(false)
                .collapsible(false)
                .resizable(resizable)
                .frame(frame)
                .fixed_pos(pos)
                .min_size(vec2(40.0, 60.0))
                .default_size(state.size)
                .max_width(if resizable {
                    f32::INFINITY
                } else {
                    state.size.x
                })
                .show(ctx, |ui| {
                    let (title_rect, title_response) =
                        title_bar(ui, panel.name(), palette, &mut close);
                    let _ = title_rect;
                    if title_response.dragged() {
                        drag_delta = Some(title_response.drag_delta());
                    }
                    if title_response.drag_stopped() {
                        drag_delta = Some(Vec2::ZERO);
                    }
                    egui::Frame::new()
                        .inner_margin(egui::Margin::symmetric(4, 3))
                        .show(ui, |ui| contents(panel, ui));
                });
            let Some(response) = response else { continue };
            let rect = response.response.rect;
            let s = self.states.get_mut(&panel).expect("panel");
            s.rect = rect;
            // Track the real size (content-sized windows can grow) so placement keeps them
            // inside the workspace and snapped edges stay put.
            s.size = rect.size();
            if close {
                s.visible = false;
            }
            match drag_delta {
                Some(d) if d != Vec2::ZERO => {
                    let raw = s.drag_raw.unwrap_or(rect.min) + d;
                    s.drag_raw = Some(raw);
                }
                Some(_) => {
                    // Drag finished: commit the snap (or free position).
                    if let Some(raw) = s.drag_raw.take() {
                        let size = s.size;
                        let snap = self.best_snap(panel, Rect::from_min_size(raw, size));
                        let s = self.states.get_mut(&panel).expect("panel");
                        s.snap = snap;
                        s.free = raw - workspace.min;
                    }
                }
                None => {}
            }
        }
    }

    fn drag_snap(&self, panel: Panel, raw: Pos2, size: Vec2) -> Pos2 {
        match self.best_snap(panel, Rect::from_min_size(raw, size)) {
            Some(snap) => self.place(&snap, size),
            None => raw,
        }
    }

    fn best_snap(&self, panel: Panel, rect: Rect) -> Option<Snap> {
        let current = self.states[&panel].snap.map(|s| s.target);
        let mut targets = vec![Target::Workspace];
        targets.extend(
            Panel::ALL
                .iter()
                .filter(|&&p| p != panel && self.states[&p].visible)
                .map(|&p| Target::Panel(p)),
        );
        // A panel may not snap to one that is snapped to it (no cycles).
        targets.retain(|t| match t {
            Target::Panel(p) => self.states[p].snap.map(|s| s.target) != Some(Target::Panel(panel)),
            Target::Workspace => true,
        });
        targets
            .into_iter()
            .filter_map(|t| self.snap_to(rect, t, current == Some(t)))
            .next()
    }
}

fn clamp_into(pos: Pos2, size: Vec2, area: Rect) -> Pos2 {
    pos2(
        pos.x
            .min(area.max.x - size.x.min(area.width()))
            .max(area.min.x),
        pos.y
            .min(area.max.y - size.y.min(area.height()))
            .max(area.min.y),
    )
}

/// paint.net-style window title: name on the left, red ✕ on the right. Returns the title's
/// drag response.
fn title_bar(
    ui: &mut egui::Ui,
    title: &str,
    palette: &Palette,
    close: &mut bool,
) -> (Rect, egui::Response) {
    let width = ui.available_width().max(40.0);
    let (rect, response) =
        ui.allocate_exact_size(vec2(width, TITLE_HEIGHT), Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, palette.window);
    let close_rect =
        Rect::from_center_size(pos2(rect.max.x - 11.0, rect.center().y), vec2(15.0, 13.0));
    let text_clip = Rect::from_min_max(rect.min, pos2(close_rect.min.x - 2.0, rect.max.y));
    ui.painter_at(text_clip).text(
        pos2(rect.min.x + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(12.0),
        palette.text,
    );
    let close_response = ui.interact(close_rect, ui.id().with("close"), Sense::click());
    let fill = if close_response.hovered() {
        palette.close.gamma_multiply(1.2)
    } else {
        palette.close
    };
    painter.rect_filled(close_rect, 0.0, fill);
    let c = close_rect.center();
    let stroke = egui::Stroke::new(1.3, egui::Color32::WHITE);
    painter.line_segment([c + vec2(-3.0, -3.0), c + vec2(3.0, 3.0)], stroke);
    painter.line_segment([c + vec2(-3.0, 3.0), c + vec2(3.0, -3.0)], stroke);
    if close_response.clicked() {
        *close = true;
    }
    (rect, response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panels(workspace: Rect) -> Panels {
        Panels {
            workspace,
            ..Default::default()
        }
    }

    const WS: Rect = Rect {
        min: pos2(0.0, 100.0),
        max: pos2(1000.0, 800.0),
    };

    #[test]
    fn default_parking() {
        let p = panels(WS);
        let tools = p.place(&default_snap(Panel::Tools), default_size(Panel::Tools));
        assert_eq!(tools, pos2(3.0, 103.0));
        let layers = p.place(&default_snap(Panel::Layers), vec2(178.0, 260.0));
        assert_eq!(layers, pos2(1000.0 - 3.0 - 178.0, 800.0 - 3.0 - 260.0));
    }

    #[test]
    fn snaps_to_workspace_edge_within_15px() {
        let p = panels(WS);
        let near_right =
            Rect::from_min_size(pos2(1000.0 - 178.0 - 10.0, 400.0), vec2(178.0, 100.0));
        let snap = p
            .snap_to(near_right, Target::Workspace, false)
            .expect("snaps");
        assert_eq!(snap.v, Some(VEdge::Right));
        assert_eq!(snap.h, Some(HEdge::Top), "free axis keeps its offset");
        assert_eq!(snap.y_offset, 300.0);
        let far = Rect::from_min_size(pos2(500.0, 400.0), vec2(178.0, 100.0));
        assert!(p.snap_to(far, Target::Workspace, false).is_none());
    }

    #[test]
    fn snapped_windows_follow_workspace_resize() {
        let mut p = panels(WS);
        let snap = default_snap(Panel::History);
        let before = p.place(&snap, vec2(178.0, 300.0));
        p.workspace = Rect::from_min_max(WS.min, pos2(1400.0, 800.0));
        let after = p.place(&snap, vec2(178.0, 300.0));
        assert_eq!(after.x - before.x, 400.0);
    }

    #[test]
    fn harder_to_unsnap() {
        let p = panels(WS);
        let r = Rect::from_min_size(pos2(25.0, 300.0), vec2(50.0, 100.0));
        assert!(
            p.snap_to(r, Target::Workspace, false).is_none(),
            "25 px away: free"
        );
        assert!(
            p.snap_to(r, Target::Workspace, true).is_some(),
            "already snapped: 30 px sticks"
        );
    }
}
