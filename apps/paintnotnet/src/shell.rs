//! Document tabs in the title bar: one chip per open image with a live thumbnail, its name,
//! a dirty marker and a close button (paint.net's image list, restyled for design A).

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, Rect, Sense, TextureOptions, pos2, vec2};
use pnn_core::{ColorBgra, TILE_SIZE, TileCoord};

use crate::theme::{self, Palette};
use crate::workspace::{DocumentTab, Workspace};

/// Longest side of a tab thumbnail texture, in points.
const THUMB: f32 = 48.0;
const THUMB_THROTTLE: Duration = Duration::from_millis(250);
/// Thumbnail box inside a tab.
const THUMB_BOX: egui::Vec2 = vec2(30.0, 20.0);
const TAB_HEIGHT: f32 = 28.0;
const NAME_MAX: f32 = 180.0;
const CLOSE: f32 = 20.0;

pub enum ImageListAction {
    Activate(usize),
    Close(u64),
    /// Picked from an image's menu: run `command` on that image.
    Command {
        key: u64,
        command: crate::commands::Command,
    },
    Move {
        from: usize,
        to: usize,
    },
}

fn name_galley(
    ui: &egui::Ui,
    tab: &DocumentTab,
    color: Color32,
    max: f32,
) -> std::sync::Arc<egui::Galley> {
    let title = if tab.is_dirty() {
        format!("{} •", tab.title())
    } else {
        tab.title()
    };
    let mut job = egui::text::LayoutJob::single_section(
        title,
        egui::TextFormat::simple(egui::FontId::proportional(theme::text::BODY), color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max);
    ui.painter().layout_job(job)
}

/// The tabs, centred in the space `ui` has left (scrolling when they don't fit). Right-click
/// opens an image's menu below its tab; `open_menu` (Alt+−) opens the one for that image.
pub fn image_list(
    ui: &mut egui::Ui,
    ws: &Workspace,
    palette: &Palette,
    icons: &crate::icons::Icons,
    open_menu: &mut Option<u64>,
) -> Vec<ImageListAction> {
    let mut actions = Vec::new();
    let gap = 6.0;
    // Names shorten (with "…") to share the room before the tabs scroll: one tab in a narrow
    // window was cut off mid-word.
    let chrome = 4.0 + THUMB_BOX.x + 8.0 + 4.0 + CLOSE + 4.0;
    let n = ws.tabs().len().max(1) as f32;
    let name_max = ((ui.available_width() - gap * (n - 1.0)) / n - chrome).clamp(24.0, NAME_MAX);
    let widths: Vec<f32> = ws
        .tabs()
        .iter()
        .map(|t| chrome + name_galley(ui, t, palette.text, name_max).size().x)
        .collect();
    let total = widths.iter().sum::<f32>() + gap * widths.len().saturating_sub(1) as f32;
    let room = ui.available_width();
    egui::ScrollArea::horizontal()
        .id_salt("image_list")
        .max_width(room)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                if total < room {
                    ui.add_space(((room - total) / 2.0).floor());
                }
                for (i, (tab, &w)) in ws.tabs().iter().zip(&widths).enumerate() {
                    let selected = ws.active_index() == Some(i);
                    let id = egui::Id::new(("image_list_item", tab.key));
                    let (rect, close_rect) = tab_chip(ui, tab, selected, w, name_max, palette);
                    // Clicks and drags both: a press only becomes a drag (reordering) once the
                    // pointer moves, so a plain click switches images. (egui's
                    // `dnd_drag_source` senses drags only, which turned every click into one.)
                    let item = ui.interact(rect, id, Sense::click_and_drag());
                    if item.drag_started_by(egui::PointerButton::Primary) {
                        egui::DragAndDrop::set_payload(ui.ctx(), i);
                    }
                    if item.dragged_by(egui::PointerButton::Primary) {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    }
                    // After the tab, so the button gets the clicks over it.
                    let close = close_rect.is_some_and(|r| close_button(ui, r, tab.key, palette));
                    let title = tab.title();
                    item.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            true,
                            selected,
                            &title,
                        )
                    });
                    let from_keyboard = *open_menu == Some(tab.key);
                    if from_keyboard {
                        *open_menu = None;
                    }
                    let popup = egui::Popup::menu(&item).open_memory(
                        (item.secondary_clicked() || from_keyboard)
                            .then_some(egui::SetOpenCommand::Bool(true)),
                    );
                    if let Some(r) =
                        popup.show(|ui| crate::menus::image_menu_ui(ui, icons, from_keyboard))
                        && let Some(command) = r.inner
                    {
                        actions.push(ImageListAction::Command {
                            key: tab.key,
                            command,
                        });
                    }
                    let item = item.on_hover_text(tab.title());
                    if close {
                        actions.push(ImageListAction::Close(tab.key));
                    } else if item.clicked() {
                        actions.push(ImageListAction::Activate(i));
                    }
                    if item.middle_clicked() {
                        actions.push(ImageListAction::Close(tab.key));
                    }
                    if let Some(from) = item.dnd_release_payload::<usize>() {
                        actions.push(ImageListAction::Move { from: *from, to: i });
                    }
                }
            });
        });
    actions
}

/// One tab. Returns its rect and where its close button goes when it should show one.
fn tab_chip(
    ui: &mut egui::Ui,
    tab: &DocumentTab,
    selected: bool,
    width: f32,
    name_max: f32,
    palette: &Palette,
) -> (Rect, Option<Rect>) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, TAB_HEIGHT), Sense::hover());
    let hovered = ui.rect_contains_pointer(rect);
    if selected {
        ui.painter().rect_filled(
            rect,
            theme::radius::CONTROL,
            palette.hover.gamma_multiply(1.4),
        );
    } else if hovered {
        ui.painter()
            .rect_filled(rect, theme::radius::CONTROL, palette.hover);
    }
    let thumb_box = Rect::from_min_size(
        pos2(rect.min.x + 4.0, rect.center().y - THUMB_BOX.y / 2.0),
        THUMB_BOX,
    );
    if let Some(tex) = &tab.thumbnail {
        let size = tex.size_vec2();
        let scale = (THUMB_BOX.x / size.x).min(THUMB_BOX.y / size.y);
        let shown = Rect::from_center_size(thumb_box.center(), size * scale);
        egui::Image::new((tex.id(), shown.size()))
            .corner_radius(3)
            .paint_at(ui, shown);
    }
    let text_color = if selected {
        palette.text
    } else {
        palette.weak_text
    };
    let galley = name_galley(ui, tab, text_color, name_max);
    ui.painter().galley(
        pos2(
            thumb_box.max.x + 8.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        text_color,
    );
    let close = (selected || hovered).then(|| {
        Rect::from_center_size(
            pos2(rect.max.x - 4.0 - CLOSE / 2.0, rect.center().y),
            vec2(CLOSE, CLOSE),
        )
    });
    (rect, close)
}

fn close_button(ui: &mut egui::Ui, rect: Rect, key: u64, palette: &Palette) -> bool {
    let close = ui
        .interact(rect, ui.id().with(("close", key)), Sense::click())
        .on_hover_text("Close (Ctrl+W)");
    if close.hovered() {
        ui.painter()
            .rect_filled(rect, theme::radius::SMALL, palette.hover);
    }
    let c = rect.center();
    let stroke = egui::Stroke::new(1.5, palette.weak_text);
    ui.painter()
        .line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], stroke);
    ui.painter()
        .line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], stroke);
    close.clicked()
}

/// Refresh a tab's thumbnail if its document changed (throttled while it keeps changing).
pub fn update_thumbnail(ctx: &egui::Context, tab: &mut DocumentTab, now: Instant) {
    if tab.thumbnail.is_some() && tab.thumbnail_revision == tab.doc.revision() {
        return;
    }
    if let Some(t) = tab.thumbnail_updated
        && now.duration_since(t) < THUMB_THROTTLE
    {
        ctx.request_repaint_after(THUMB_THROTTLE);
        return;
    }
    tab.compositor.update(&tab.doc);
    let image = thumbnail_pixels(tab, THUMB * ctx.pixels_per_point());
    match &mut tab.thumbnail {
        Some(handle) => handle.set(image, TextureOptions::LINEAR),
        None => {
            tab.thumbnail =
                Some(ctx.load_texture(format!("thumb-{}", tab.key), image, TextureOptions::LINEAR))
        }
    }
    tab.thumbnail_revision = tab.doc.revision();
    tab.thumbnail_updated = Some(now);
}

/// The image at most `longest` pixels on its long side, over a small version of paint.net's
/// checkerboard, sampled from the tab's compositor (which the caller has brought up to date).
pub fn thumbnail_pixels(tab: &DocumentTab, longest: f32) -> ColorImage {
    let (w, h) = (tab.doc.width() as f32, tab.doc.height() as f32);
    let scale = (longest / w.max(h)).min(1.0);
    let (tw, th) = (
        ((w * scale).round() as usize).max(1),
        ((h * scale).round() as usize).max(1),
    );
    let mut pixels = Vec::with_capacity(tw * th);
    for y in 0..th {
        for x in 0..tw {
            let sx = (((x as f32 + 0.5) / scale) as u32).min(tab.doc.width() - 1);
            let sy = (((y as f32 + 0.5) / scale) as u32).min(tab.doc.height() - 1);
            let c = TileCoord {
                tx: sx / TILE_SIZE,
                ty: sy / TILE_SIZE,
            };
            let p = tab.compositor.tile(c).map_or(ColorBgra::TRANSPARENT, |t| {
                t.get(sx % TILE_SIZE, sy % TILE_SIZE)
            });
            let v = if ((x ^ y) & 4) != 0 { 255 } else { 191 };
            let a = p.a as u32;
            let mix = |c: u8| ((c as u32 * a + v * (255 - a)) / 255) as u8;
            pixels.push(Color32::from_rgb(mix(p.r), mix(p.g), mix(p.b)));
        }
    }
    ColorImage::new([tw, th], pixels)
}

/// The window buttons of paintnotnet's own title bar (when the desktop's is off), right to
/// left: close, maximise/restore, minimise. Close asks about unsaved images as the
/// desktop's close does.
pub fn window_buttons(ui: &mut egui::Ui, icons: &crate::icons::Icons) {
    use crate::icons::Icon;
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    // Named for screen readers (and tests); the icons say the same.
    let button = |ui: &mut egui::Ui, icon: Icon, name: &'static str| {
        let r = ui
            .add(egui::Button::image(icons.image(icon)).frame_when_inactive(false))
            .on_hover_text(name);
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        r.clicked()
    };
    // Close turns red under the pointer, as desktops draw it.
    let close = ui
        .scope(|ui| {
            let red = Color32::from_rgb(0xd9, 0x2d, 0x3a);
            let widgets = &mut ui.visuals_mut().widgets;
            for w in [&mut widgets.hovered, &mut widgets.active] {
                w.weak_bg_fill = red;
                w.bg_fill = red;
            }
            button(ui, Icon::WindowClose, "Close")
        })
        .inner;
    if close {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let (icon, name) = if maximized {
        (Icon::WindowRestore, "Restore")
    } else {
        (Icon::WindowMaximize, "Maximize")
    };
    if button(ui, icon, name) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if button(ui, Icon::WindowMinimize, "Minimize") {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

/// The desktop takes over the pointer to move or resize the window and keeps the button's
/// release, so egui would go on thinking the button is down, and hovers nothing while a
/// button is down (the wheel over the canvas stopped zooming, tooltips stopped, until the
/// next click). Remember to give egui that release on the next frame (`take_release`).
fn release_after_handover(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("release after handover"), true));
}

/// The release owed after a move or resize (see `release_after_handover`), once; the app's
/// `raw_input_hook` puts it in front of the next frame's input.
pub fn take_release(ctx: &egui::Context) -> Option<egui::Event> {
    let id = egui::Id::new("release after handover");
    ctx.data_mut(|d| d.remove_temp::<bool>(id))?;
    let pos = ctx.input(|i| i.pointer.latest_pos()).unwrap_or_default();
    Some(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Default::default(),
    })
}

/// Dragging the title bar's empty space moves the window; double-clicking it maximises or
/// restores. `bar` is the background response, made before the bar's widgets (which sit on
/// top of it and keep their own clicks).
pub fn title_bar_drag(ctx: &egui::Context, bar: &egui::Response) {
    if bar.double_clicked() {
        let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    } else if bar.drag_started_by(egui::PointerButton::Primary) {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        release_after_handover(ctx);
    }
}

/// Without the desktop's frame the window resizes from thin strips along its edges and
/// corners (not while maximised or full screen).
pub fn resize_edges(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection as D;
    let (maximized, fullscreen) = ctx.input(|i| {
        let v = i.viewport();
        (v.maximized.unwrap_or(false), v.fullscreen.unwrap_or(false))
    });
    if maximized || fullscreen {
        return;
    }
    const EDGE: f32 = 5.0;
    const CORNER: f32 = 12.0;
    let r = ctx.content_rect();
    let (l, t, rt, b) = (r.min.x, r.min.y, r.max.x, r.max.y);
    let strips = [
        (
            D::NorthWest,
            Rect::from_min_max(pos2(l, t), pos2(l + CORNER, t + CORNER)),
        ),
        (
            D::NorthEast,
            Rect::from_min_max(pos2(rt - CORNER, t), pos2(rt, t + CORNER)),
        ),
        (
            D::SouthWest,
            Rect::from_min_max(pos2(l, b - CORNER), pos2(l + CORNER, b)),
        ),
        (
            D::SouthEast,
            Rect::from_min_max(pos2(rt - CORNER, b - CORNER), pos2(rt, b)),
        ),
        (
            D::North,
            Rect::from_min_max(pos2(l + CORNER, t), pos2(rt - CORNER, t + EDGE)),
        ),
        (
            D::South,
            Rect::from_min_max(pos2(l + CORNER, b - EDGE), pos2(rt - CORNER, b)),
        ),
        (
            D::West,
            Rect::from_min_max(pos2(l, t + CORNER), pos2(l + EDGE, b - CORNER)),
        ),
        (
            D::East,
            Rect::from_min_max(pos2(rt - EDGE, t + CORNER), pos2(rt, b - CORNER)),
        ),
    ];
    for (dir, rect) in strips {
        let cursor = match dir {
            D::North => egui::CursorIcon::ResizeNorth,
            D::South => egui::CursorIcon::ResizeSouth,
            D::West => egui::CursorIcon::ResizeWest,
            D::East => egui::CursorIcon::ResizeEast,
            D::NorthWest => egui::CursorIcon::ResizeNorthWest,
            D::NorthEast => egui::CursorIcon::ResizeNorthEast,
            D::SouthWest => egui::CursorIcon::ResizeSouthWest,
            D::SouthEast => egui::CursorIcon::ResizeSouthEast,
        };
        egui::Area::new(egui::Id::new(("resize edge", dir as u8)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .show(ctx, |ui| {
                let (_, response) = ui.allocate_exact_size(rect.size(), Sense::drag());
                let response = response.on_hover_cursor(cursor);
                if response.is_pointer_button_down_on()
                    && ctx.input(|i| i.pointer.primary_pressed())
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
                    release_after_handover(ctx);
                }
            });
    }
}
