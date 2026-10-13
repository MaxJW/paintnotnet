use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use pnn_core::Document;
use pnn_render::{CanvasRenderer, View, view};

use pnn_tools::{ToolKind, ToolSettings, tool_for_shortcut};

use crate::canvas::{self, ARROW_PAN_STEP, CanvasState};
use crate::commands::{Command, MultiTap, Panel, Triggered, Units, take_commands};
use crate::diagnostics::Diagnostics;
use crate::dialogs::{Modal, ModalResult, NewImage, SaveTarget};
use crate::file_ops::{FileEvent, FileOps, resolve_save_path};
use crate::icons::{Icon, Icons};
use crate::menus::{self, MenuBarState, MenuContext};
use crate::panels::Panels;
use crate::shell::{self, ImageListAction};
use crate::status::{self, StatusAction, StatusInfo, StatusState};
use crate::theme;
use crate::toolbar::{self, Row2Action, Row2State};
use crate::windows::{self, LayersAction, colors::ColorsState};
use crate::workspace::Workspace;

/// `--smoke-test N`: show diagnostics, run for N seconds (sweeping zoom and pan if an image
/// is open), print a summary line, then quit.
pub struct SmokeTest {
    duration: Duration,
    started: Option<Instant>,
}

impl SmokeTest {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            started: None,
        }
    }
}

pub struct PaintNotNetApp {
    pub(crate) workspace: Workspace,
    pub(crate) canvas: CanvasState,
    pub(crate) files: FileOps,
    pub(crate) modal: Option<Modal>,
    /// Errors waiting to be shown after the current modal.
    pub(crate) errors: Vec<(String, String)>,
    pub(crate) taps: MultiTap,
    pub(crate) diagnostics: Diagnostics,
    pub(crate) smoke: Option<SmokeTest>,
    pub(crate) new_image_size: (u32, u32),
    pub(crate) last_dir: Option<PathBuf>,
    /// Tabs to close once their save succeeds.
    pub(crate) close_after_save: HashSet<u64>,
    /// Untitled tabs waiting for a Save As dialog (Save All, Exit → Save All).
    pub(crate) save_as_queue: Vec<u64>,
    pub(crate) exit_after_save: bool,
    pub(crate) allow_close: bool,
    pub(crate) released: Vec<u64>,
    pub(crate) title: String,
    pub(crate) tool: ToolKind,
    pub(crate) previous_tool: ToolKind,
    pub(crate) settings: ToolSettings,
    pub(crate) panels: Panels,
    pub(crate) colors: ColorsState,
    pub(crate) menu: MenuBarState,
    row2: Row2State,
    pub(crate) status: StatusState,
    pub(crate) icons: Icons,
    pub(crate) rulers: bool,
    pub(crate) units: Units,
    /// Open Layer Properties / Resize / Canvas Size / Rotate-Zoom dialog.
    pub(crate) doc_dialog: Option<crate::doc_dialogs::DocDialog>,
    /// Pencil stroke in progress.
    pub(crate) stroke: Option<crate::actions::PencilStroke>,
    /// Selection being drawn.
    pub(crate) select_drag: Option<crate::selection_tools::SelectDrag>,
    /// Keyboard modifiers this frame (tools read Shift etc. mid-drag).
    pub(crate) modifiers: egui::Modifiers,
    /// Floating pixels / selection being moved.
    pub(crate) move_session: Option<crate::move_tools::MoveSession>,
    pub(crate) clipboard: Box<dyn crate::clipboard::Clipboard>,
    /// Image waiting for the "larger than the canvas" answer.
    pub(crate) pending_paste: Option<crate::clipboard::ClipImage>,
    /// Last effect run (Effects > Repeat, Ctrl+F).
    pub(crate) last_effect: Option<pnn_effects::EffectId>,
    /// Selection revision the toolbar's feather fields were last synced from.
    pub(crate) synced_selection: u64,
}

impl PaintNotNetApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        smoke: Option<SmokeTest>,
        open: Vec<PathBuf>,
    ) -> Self {
        // Ctrl +/-/0 belong to the canvas zoom, not egui's UI scale.
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        theme::setup(&cc.egui_ctx);
        if let Some(rs) = cc.wgpu_render_state.as_ref() {
            CanvasRenderer::install(rs);
        }
        let mut diagnostics = Diagnostics::default();
        diagnostics.set_visible(cfg!(debug_assertions) || smoke.is_some());
        let mut files = FileOps::new(cc.egui_ctx.clone());
        files.open_paths(open);
        Self {
            workspace: Workspace::default(),
            canvas: CanvasState::default(),
            files,
            modal: None,
            errors: Vec::new(),
            taps: MultiTap::default(),
            diagnostics,
            smoke,
            new_image_size: (800, 600),
            last_dir: None,
            close_after_save: HashSet::new(),
            save_as_queue: Vec::new(),
            exit_after_save: false,
            allow_close: false,
            released: Vec::new(),
            title: String::new(),
            tool: ToolKind::DEFAULT,
            previous_tool: ToolKind::DEFAULT,
            settings: ToolSettings::default(),
            panels: Panels::default(),
            colors: ColorsState::default(),
            menu: MenuBarState::default(),
            row2: Row2State::default(),
            status: StatusState::default(),
            icons: Icons::load(&cc.egui_ctx),
            rulers: false,
            units: Units::Pixels,
            doc_dialog: None,
            stroke: None,
            select_drag: None,
            modifiers: egui::Modifiers::NONE,
            move_session: None,
            clipboard: Box::new(crate::clipboard::SystemClipboard::new()),
            pending_paste: None,
            last_effect: None,
            synced_selection: 0,
        }
    }

    pub fn diagnostics(&self) -> &Diagnostics {
        &self.diagnostics
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn workspace_mut(&mut self) -> &mut Workspace {
        &mut self.workspace
    }

    pub fn modal(&self) -> Option<&Modal> {
        self.modal.as_ref()
    }

    pub fn pixel_grid(&self) -> bool {
        self.canvas.pixel_grid
    }

    pub fn canvas(&self) -> &CanvasState {
        &self.canvas
    }

    /// The open Layer Properties / Resize / Canvas Size / Rotate-Zoom dialog.
    pub fn doc_dialog_mut(&mut self) -> Option<&mut crate::doc_dialogs::DocDialog> {
        self.doc_dialog.as_mut()
    }

    /// The toolbar's feather Preview toggle.
    pub fn set_feather_preview(&mut self, on: bool) {
        self.row2.feather_preview = on;
    }

    /// A menu is open (menus pause global shortcuts).
    pub fn menu_open(&self) -> bool {
        self.menu.open
    }

    pub fn tool(&self) -> ToolKind {
        self.tool
    }

    pub fn settings(&self) -> &ToolSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut ToolSettings {
        &mut self.settings
    }

    pub fn panels(&self) -> &Panels {
        &self.panels
    }

    pub fn rulers(&self) -> bool {
        self.rulers
    }

    pub fn units(&self) -> Units {
        self.units
    }

    pub fn set_tool(&mut self, tool: ToolKind) {
        if tool != self.tool {
            self.finish_move();
            self.previous_tool = self.tool;
            self.tool = tool;
        }
    }

    /// Snapshot of what decides command availability and check marks.
    pub fn command_state(&self) -> CommandState {
        CommandState {
            document: DOCUMENT_COMMANDS
                .iter()
                .copied()
                .chain(
                    (0..pnn_effects::EFFECTS.len() as u16)
                        .map(|i| Command::Effect(pnn_effects::EffectId(i)))
                        .filter(|c| c.implemented()),
                )
                .map(|c| (c, self.document_command_available(c).unwrap_or(false)))
                .collect(),
            has_doc: self.workspace.active().is_some(),
            has_selection: self
                .workspace
                .active()
                .is_some_and(|t| !t.doc.selection().is_empty()),
            repeat_effect: self.last_effect.is_some() && self.workspace.active().is_some(),
            any_dirty: self.workspace.tabs().iter().any(|t| t.is_dirty()),
            pixel_grid: self.canvas.pixel_grid,
            rulers: self.rulers,
            units: self.units,
            panels: Panel::ALL.map(|p| self.panels.visible(p)),
        }
    }

    /// Whether a command can run now (menus grey out the rest).
    pub fn availability(&self, command: Command) -> bool {
        self.command_state().enabled(command)
    }

    /// Check-mark state for toggle commands.
    pub fn checked(&self, command: Command) -> bool {
        self.command_state().checked(command)
    }

    /// Run a command as if chosen from a menu or shortcut.
    pub fn execute(&mut self, ctx: &egui::Context, t: Triggered) {
        if self.doc_dialog.is_some() || self.stroke.is_some() {
            return;
        }
        if t.command == Command::Finish {
            // Enter / Esc commits floating pixels; with nothing pending it deselects.
            if self.move_session.is_some() {
                self.finish_move();
            } else if self.select_drag.is_none() {
                self.selection_command(Command::Deselect);
            }
            return;
        }
        if matches!(t.command, Command::Undo | Command::Redo) {
            self.abandon_move();
        } else if !keeps_move_session(t.command) {
            self.finish_move();
        }
        if self.clipboard_command(t.command)
            || self.selection_command(t.command)
            || self.document_command(t.command)
        {
            return;
        }
        let vp = self.canvas.viewport;
        match t.command {
            Command::NewImage => {
                self.modal = Some(Modal::NewImage(NewImage::new(
                    self.new_image_size.0,
                    self.new_image_size.1,
                )))
            }
            Command::Open => self.files.pick_open(self.last_dir.clone()),
            Command::Close => {
                if let Some(key) = self.workspace.active().map(|t| t.key) {
                    self.close_tab(key);
                }
            }
            Command::Save => {
                if let Some(key) = self.workspace.active().map(|t| t.key) {
                    self.save_tab(key, false);
                }
            }
            Command::SaveAs => {
                if let Some(tab) = self.workspace.active() {
                    let (key, path, title) = (tab.key, tab.path.clone(), tab.title());
                    self.files.pick_save(key, path.as_deref(), &title);
                }
            }
            Command::SaveAll => self.save_all(),
            Command::Exit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::NextImage => self.workspace.cycle(true),
            Command::PreviousImage => self.workspace.cycle(false),
            Command::MoveImageLeft => self.workspace.move_active(false),
            Command::MoveImageRight => self.workspace.move_active(true),
            Command::SwitchToImage(n) => self.workspace.activate(n as usize - 1),
            Command::TogglePixelGrid => self.canvas.pixel_grid = !self.canvas.pixel_grid,
            Command::ToggleRulers => self.rulers = !self.rulers,
            Command::SetUnits(u) => self.units = u,
            Command::ToggleDiagnostics => self.diagnostics.set_visible(!self.diagnostics.visible()),
            Command::ToggleWindow(p) => self.panels.toggle(p),
            Command::ResetWindow(p) => self.panels.reset(p),
            Command::OpenMenu(id) => menus::request_open(&mut self.menu, id),
            Command::HelpTopics => open_url("https://paint.net/doc/latest/"),
            Command::About => self.modal = Some(Modal::About),
            Command::ToolShortcut(letter) => {
                if let Some(tool) = tool_for_shortcut(letter, t.shift, t.taps) {
                    self.set_tool(tool);
                }
            }
            Command::OpenToolDropdown => self.row2.open_tool_dropdown = true,
            Command::BrushWidth(d) => self.settings.nudge_brush_width(d as f32),
            Command::SwapColors => self.settings.swap_colors(),
            Command::SwitchActiveColor => self.settings.switch_active_color(),
            c if !c.implemented() => {}
            view_command => {
                let Some(tab) = self.workspace.active_mut() else {
                    return;
                };
                let img = tab.image_size();
                let Some(view) = tab.view.as_mut() else {
                    return;
                };
                match view_command {
                    Command::ZoomIn => view.zoom_centered(view::next_zoom_in(view.zoom), img, vp),
                    Command::ZoomOut => view.zoom_centered(view::next_zoom_out(view.zoom), img, vp),
                    Command::ActualSize => view.zoom_centered(1.0, img, vp),
                    Command::ZoomToWindow => {
                        // Second Ctrl+B restores the zoom from before the first.
                        if view.fit_to_window
                            && let Some(previous) = tab.view_before_fit.take()
                        {
                            *view = previous;
                            view.clamp(img, vp);
                        } else {
                            tab.view_before_fit = Some(*view);
                            *view = View::fitted(img, vp);
                        }
                    }
                    Command::ScrollPageUp => view.scroll_by([0.0, -vp[1]], img, vp),
                    Command::ScrollPageDown => view.scroll_by([0.0, vp[1]], img, vp),
                    Command::ScrollPageLeft => view.scroll_by([-vp[0], 0.0], img, vp),
                    Command::ScrollPageRight => view.scroll_by([vp[0], 0.0], img, vp),
                    Command::HomeKey if t.taps >= 2 => view.scroll_to_corner(false, img, vp),
                    Command::HomeKey => view.scroll_by([-vp[0], 0.0], img, vp),
                    Command::EndKey if t.taps >= 2 => view.scroll_to_corner(true, img, vp),
                    Command::EndKey => view.scroll_by([vp[0], 0.0], img, vp),
                    Command::ScrollToTopLeft => view.scroll_to_corner(false, img, vp),
                    Command::ScrollToBottomRight => view.scroll_to_corner(true, img, vp),
                    Command::CenterTopLeft => view.center_on([0.0, 0.0]),
                    Command::CenterBottomRight => view.center_on(img),
                    _ => {}
                }
            }
        }
    }

    fn close_tab(&mut self, key: u64) {
        let Some(tab) = self.workspace.tabs().iter().find(|t| t.key == key) else {
            return;
        };
        if tab.is_dirty() {
            self.modal = Some(Modal::UnsavedChanges {
                key,
                title: tab.title(),
            });
        } else {
            self.discard_tab(key);
        }
    }

    fn discard_tab(&mut self, key: u64) {
        if self.workspace.remove(key).is_some() {
            self.released.push(key);
        }
    }

    /// Save to the tab's file, or ask where (Save As) if it has none.
    fn save_tab(&mut self, key: u64, close_after: bool) {
        let Some(tab) = self.workspace.tabs().iter().find(|t| t.key == key) else {
            return;
        };
        match (tab.path.clone(), tab.format) {
            (Some(path), Some(format)) => self.begin_save(
                key,
                SaveTarget {
                    path,
                    format,
                    flattened: false,
                    close_after,
                },
            ),
            _ => {
                if close_after {
                    self.close_after_save.insert(key);
                }
                let (path, title) = (tab.path.clone(), tab.title());
                self.files.pick_save(key, path.as_deref(), &title);
            }
        }
    }

    fn begin_save(&mut self, key: u64, target: SaveTarget) {
        let Some(tab) = self.workspace.tab_mut(key) else {
            return;
        };
        if tab.doc.layers().len() > 1 && !target.flattened {
            self.modal = Some(Modal::FlattenForSave { key, then: target });
        } else if target.format.has_options() {
            self.modal = Some(Modal::JpegQuality {
                key,
                quality: tab.save_options.jpeg_quality,
                then: target,
            });
        } else {
            self.write(key, target);
        }
    }

    fn write(&mut self, key: u64, target: SaveTarget) {
        let Some(tab) = self.workspace.tab_mut(key) else {
            return;
        };
        if target.close_after {
            self.close_after_save.insert(key);
        }
        self.files.save(
            key,
            tab.doc.clone(),
            target.path,
            target.format,
            tab.save_options,
        );
    }

    fn save_all(&mut self) {
        let dirty: Vec<(u64, bool)> = self
            .workspace
            .tabs()
            .iter()
            .filter(|t| t.is_dirty())
            .map(|t| (t.key, t.path.is_some() && t.format.is_some()))
            .collect();
        for (key, has_file) in dirty {
            if has_file {
                // Save All reuses each image's last settings rather than prompting.
                let tab = self
                    .workspace
                    .tabs()
                    .iter()
                    .find(|t| t.key == key)
                    .expect("exists");
                let target = SaveTarget {
                    path: tab.path.clone().expect("has path"),
                    format: tab.format.expect("has format"),
                    flattened: false,
                    close_after: false,
                };
                self.write(key, target);
            } else {
                self.save_as_queue.push(key);
            }
        }
    }

    fn handle_file_events(&mut self) {
        for event in self.files.poll() {
            match event {
                FileEvent::Imported { key, path, result } => match result {
                    Ok(doc) => {
                        let name = path
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.last_dir = path.parent().map(|p| p.to_owned());
                        self.import_layer(key, name, doc);
                    }
                    Err(e) => self.errors.push(("Error Importing File".into(), e)),
                },
                FileEvent::Opened { path, result } => match result {
                    Ok(doc) => {
                        let format = pnn_io::FileFormat::from_path(&path);
                        self.last_dir = path.parent().map(|p| p.to_owned());
                        self.workspace.add(doc, Some(path), format);
                    }
                    Err(e) => self.errors.push(("Error Opening File".into(), e)),
                },
                FileEvent::PickedOpen(paths) => self.files.open_paths(paths),
                FileEvent::PickedSave { key, path } => match path.map(resolve_save_path) {
                    Some(Ok((path, format))) => {
                        let close_after = self.close_after_save.remove(&key);
                        self.begin_save(
                            key,
                            SaveTarget {
                                path,
                                format,
                                flattened: false,
                                close_after,
                            },
                        );
                    }
                    Some(Err(e)) => {
                        self.close_after_save.remove(&key);
                        self.errors.push(("Error Saving File".into(), e));
                    }
                    None => {
                        // Cancelled: abandon any close/exit waiting on this save.
                        self.close_after_save.remove(&key);
                        self.save_as_queue.clear();
                        self.exit_after_save = false;
                    }
                },
                FileEvent::Saved {
                    key,
                    path,
                    format,
                    revision,
                    result,
                } => match result {
                    Ok(()) => {
                        if let Some(tab) = self.workspace.tab_mut(key) {
                            tab.path = Some(path.clone());
                            tab.format = Some(format);
                            tab.mark_saved(revision);
                        }
                        self.last_dir = path.parent().map(|p| p.to_owned());
                        if self.close_after_save.remove(&key) {
                            self.discard_tab(key);
                        }
                    }
                    Err(e) => {
                        self.close_after_save.remove(&key);
                        self.exit_after_save = false;
                        self.errors.push(("Error Saving File".into(), e));
                    }
                },
            }
        }
    }

    fn handle_modal_result(&mut self, ctx: &egui::Context, result: ModalResult) {
        self.modal = None;
        match result {
            ModalResult::CreateImage { width, height } => {
                self.new_image_size = (width, height);
                self.workspace
                    .add(Document::new_blank(width, height), None, None);
            }
            ModalResult::Save { key } => self.save_tab(key, true),
            ModalResult::DontSave { key } => self.discard_tab(key),
            ModalResult::SaveAllAndExit => {
                self.exit_after_save = true;
                self.save_all();
            }
            ModalResult::ExitWithoutSaving => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ModalResult::JpegConfirmed { key, quality, then } => {
                if let Some(tab) = self.workspace.tab_mut(key) {
                    tab.save_options.jpeg_quality = quality;
                }
                self.write(key, then);
            }
            ModalResult::PasteExpand { new_layer, expand } => {
                if let Some(image) = self.pending_paste.take() {
                    self.paste(image, new_layer, expand);
                }
            }
            ModalResult::FlattenAndSave { key, mut then } => {
                if let Some(tab) = self.workspace.tab_mut(key) {
                    let (w, h) = (tab.doc.width(), tab.doc.height());
                    let flat = pnn_core::transform::flatten_layers(&tab.doc);
                    tab.doc.replace(w, h, vec![flat]);
                    tab.commit("Flatten", Icon::Flatten);
                }
                then.flattened = true;
                self.begin_save(key, then);
            }
            ModalResult::Cancelled => {
                self.exit_after_save = false;
                self.save_as_queue.clear();
            }
            ModalResult::Closed => {}
        }
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.viewport().close_requested()) || self.allow_close {
            return;
        }
        let dirty = self
            .workspace
            .tabs()
            .iter()
            .filter(|t| t.is_dirty())
            .count();
        if dirty > 0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.modal.is_none() {
                self.modal = Some(Modal::UnsavedOnExit { count: dirty });
            }
        }
    }

    /// Space + arrows pans (×10 with Ctrl).
    fn keyboard_pan(&mut self, ctx: &egui::Context) {
        let vp = self.canvas.viewport;
        let Some(tab) = self.workspace.active_mut() else {
            return;
        };
        let img = tab.image_size();
        let Some(view) = tab.view.as_mut() else {
            return;
        };
        let (space, ctrl, dirs) = ctx.input(|i| {
            let dirs: Vec<[f64; 2]> = [
                (egui::Key::ArrowLeft, [-1.0, 0.0]),
                (egui::Key::ArrowRight, [1.0, 0.0]),
                (egui::Key::ArrowUp, [0.0, -1.0]),
                (egui::Key::ArrowDown, [0.0, 1.0]),
            ]
            .into_iter()
            .filter(|(k, _)| i.key_pressed(*k))
            .map(|(_, d)| d)
            .collect();
            (i.key_down(egui::Key::Space), i.modifiers.ctrl, dirs)
        });
        if !space {
            let step = if ctrl { 10.0 } else { 1.0 };
            let moving = matches!(
                self.tool,
                ToolKind::MoveSelectedPixels | ToolKind::MoveSelection
            );
            if moving {
                for d in dirs {
                    self.nudge(d[0] * step, d[1] * step);
                }
            }
            return;
        }
        let step = if ctrl {
            ARROW_PAN_STEP * 10.0
        } else {
            ARROW_PAN_STEP
        };
        for d in dirs {
            view.scroll_by([d[0] * step, d[1] * step], img, vp);
        }
    }

    fn update_title(&mut self, ctx: &egui::Context) {
        let title = match self.workspace.active() {
            Some(tab) => format!(
                "{}{} - paintnotnet",
                if tab.is_dirty() { "*" } else { "" },
                tab.title()
            ),
            None => "paintnotnet".to_owned(),
        };
        if title != self.title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn run_smoke_test(&mut self, ctx: &egui::Context, now: Instant) {
        let Some(smoke) = &mut self.smoke else { return };
        let started = *smoke.started.get_or_insert(now);
        let elapsed = now - started;
        // PNN_SMOKE_FEATHER=1: drag the feather radius every frame on a large selection with
        // the preview overlay on (the live-slider case), instead of sweeping the view.
        if std::env::var_os("PNN_SMOKE_FEATHER").is_some() {
            self.row2.feather_preview = true;
            if let Some(tab) = self.workspace.active_mut() {
                let (w, h) = (tab.doc.width() as f64, tab.doc.height() as f64);
                let t = elapsed.as_secs_f64();
                let radius = 24.0 + 20.0 * (t * 3.0).sin() as f32;
                let base = if tab.doc.selection().is_empty() {
                    pnn_core::Selection::ellipse(w * 0.05, h * 0.07, w * 0.95, h * 0.93)
                } else {
                    tab.doc.selection().clone()
                };
                let feather = pnn_core::selection::Feather {
                    radius,
                    ..pnn_core::selection::Feather::NONE
                };
                tab.doc.set_selection(base.with_feather(feather));
            }
        } else if let Some(tab) = self.workspace.active_mut() {
            // Sweep zoom (about 1%–800%) and pan so the canvas redraws every frame.
            let vp = self.canvas.viewport;
            let img = tab.image_size();
            if let Some(view) = tab.view.as_mut() {
                let t = elapsed.as_secs_f64();
                let zoom = 2f64.powf((t * 0.7).sin() * 3.0) * view::fit_zoom(img, vp).max(0.01);
                view.zoom_centered(zoom, img, vp);
                view.center = [
                    img[0] * (0.5 + 0.4 * (t * 0.9).cos()),
                    img[1] * (0.5 + 0.4 * (t * 1.3).sin()),
                ];
                view.clamp(img, vp);
            }
        }
        if elapsed < smoke.duration {
            return;
        }
        let env = self.diagnostics.environment();
        let stats = self.diagnostics.stats();
        let image = self
            .workspace
            .active()
            .map(|t| format!("{}x{}", t.doc.width(), t.doc.height()));
        println!(
            "smoke-test: window_system={} monitor={:?} backend={} adapter={:?} image={} frames={} fps={:.1} mean_ms={:.2} p99_ms={:.2} max_ms={:.2}",
            env.map_or("unknown", |e| e.window_system),
            crate::diagnostics::monitor_label(ctx),
            env.map_or("unknown", |e| e.backend.as_str()),
            env.map_or("unknown", |e| e.adapter.as_str()),
            image.as_deref().unwrap_or("none"),
            stats.map_or(0, |s| s.frames),
            stats.map_or(0.0, |s| s.fps()),
            stats.map_or(0.0, |s| s.mean.as_secs_f64() * 1000.0),
            stats.map_or(0.0, |s| s.p99.as_secs_f64() * 1000.0),
            stats.map_or(0.0, |s| s.max.as_secs_f64() * 1000.0),
        );
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        self.smoke = None;
    }
}

/// Width of the menus + toolbar row 1 column in the top band.
const MENU_COLUMN_WIDTH: f32 = 345.0;

impl PaintNotNetApp {
    /// paint.net 5's top band: menus and toolbar row 1 on the left, the image list beside
    /// them, utility window toggles + settings + help on the right.
    fn top_band(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // The two sides of the band run one after the other; share state through cells.
        let picked: std::cell::Cell<Option<Command>> = std::cell::Cell::new(None);
        let menu = std::cell::RefCell::new(std::mem::take(&mut self.menu));
        let mut list_actions = Vec::new();
        egui::Panel::top("top_band").show(ui, |ui| {
            let state = self.command_state();
            let availability = |c: Command| state.enabled(c);
            let checked = |c: Command| state.checked(c);
            let cx = MenuContext {
                enabled: &availability,
                checked: &checked,
                last_effect: None,
            };
            // Left: menus + toolbar row 1 (natural width), then the image list.
            // Right: utility window toggles, settings, help — always visible.
            egui::Sides::new().shrink_left().show(
                ui,
                |ui| {
                    ui.horizontal_top(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(MENU_COLUMN_WIDTH, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_max_width(MENU_COLUMN_WIDTH);
                                if let Some(c) =
                                    menus::menu_bar(ui, &mut menu.borrow_mut(), &cx, &self.icons)
                                {
                                    picked.set(Some(c));
                                }
                                if let Some(c) =
                                    toolbar::row1(ui, &self.icons, &availability, &checked)
                                {
                                    picked.set(Some(c));
                                }
                            },
                        );
                        ui.add_space(6.0);
                        list_actions = shell::image_list(ui, &self.workspace);
                    });
                },
                |ui| {
                    if let Some(c) =
                        menus::help_button(ui, &mut menu.borrow_mut(), &cx, &self.icons)
                    {
                        picked.set(Some(c));
                    }
                    let settings = ui
                        .add_enabled(
                            availability(Command::Settings),
                            egui::Button::image(self.icons.image(Icon::Settings)).frame(false),
                        )
                        .on_hover_text("Settings (Alt+X)")
                        .on_disabled_hover_text("Settings (Alt+X)");
                    if settings.clicked() {
                        picked.set(Some(Command::Settings));
                    }
                    ui.separator();
                    for (panel, icon) in [
                        (Panel::Colors, Icon::ColorsWindow),
                        (Panel::Layers, Icon::LayersWindow),
                        (Panel::History, Icon::HistoryWindow),
                        (Panel::Tools, Icon::ToolsWindow),
                    ] {
                        let command = Command::ToggleWindow(panel);
                        let tip = format!(
                            "{} ({})",
                            panel.name(),
                            command.shortcut_text().unwrap_or_default()
                        );
                        let button = egui::Button::image(self.icons.image(icon))
                            .selected(self.panels.visible(panel));
                        if ui.add(button).on_hover_text(tip).clicked() {
                            // Ctrl+Shift+click resets the window, as in paint.net.
                            let reset = ui.input(|i| i.modifiers.ctrl && i.modifiers.shift);
                            picked.set(Some(if reset {
                                Command::ResetWindow(panel)
                            } else {
                                command
                            }));
                        }
                    }
                },
            );
        });
        for action in list_actions {
            match action {
                ImageListAction::Activate(i) => self.workspace.activate(i),
                ImageListAction::Close(key) => self.close_tab(key),
                ImageListAction::Move { from, to } => self.workspace.move_tab(from, to),
            }
        }
        self.menu = menu.into_inner();
        if let Some(c) = picked.get() {
            self.execute(ctx, Triggered::once(c));
        }
    }

    fn floating_windows(&mut self, ctx: &egui::Context, workspace: egui::Rect, now: Instant) {
        let palette = theme::palette(ctx);
        let colors_width = self.colors.width();
        if (self.panels.size(Panel::Colors).x - colors_width).abs() > 0.5 {
            let h = self.panels.size(Panel::Colors).y;
            self.panels
                .set_size(Panel::Colors, egui::vec2(colors_width, h));
        }
        let mut picked_tool = None;
        let mut layer_actions = Vec::new();
        let mut history_action = None;
        let availability: Vec<(Command, bool)> = [
            Command::AddNewLayer,
            Command::DeleteLayer,
            Command::DuplicateLayer,
            Command::MergeLayerDown,
            Command::MoveLayerUp,
            Command::MoveLayerDown,
            Command::LayerProperties,
            Command::Undo,
            Command::Redo,
        ]
        .into_iter()
        .map(|c| (c, self.availability(c)))
        .collect();
        let enabled = |c: Command| availability.iter().any(|&(k, v)| k == c && v);
        let tool = self.tool;
        let icons = &self.icons;
        let settings = &mut self.settings;
        let colors = &mut self.colors;
        let workspace_tabs = &mut self.workspace;
        self.panels
            .show(ctx, workspace, &palette, |panel, ui| match panel {
                Panel::Tools => picked_tool = windows::tools(ui, tool, icons, &palette),
                Panel::History => {
                    history_action =
                        windows::history(ui, workspace_tabs.active(), icons, &palette, &enabled)
                }
                Panel::Layers => {
                    layer_actions = windows::layers(
                        ui,
                        workspace_tabs.active_mut(),
                        icons,
                        &palette,
                        &enabled,
                        now,
                    )
                }
                Panel::Colors => colors.ui(ui, settings, icons),
            });
        if let Some(tool) = picked_tool {
            self.set_tool(tool);
        }
        for action in layer_actions {
            match action {
                LayersAction::Select(i) => {
                    if let Some(tab) = self.workspace.active_mut() {
                        tab.doc.set_active_index(i);
                    }
                }
                LayersAction::ToggleVisible(i) => {
                    if let Some(tab) = self.workspace.active_mut() {
                        tab.doc.modify_layer(i, None, |l| l.visible = !l.visible);
                        tab.commit("Layer Visibility", Icon::LayerProperties);
                    }
                }
                LayersAction::Move { from, to } => {
                    if let Some(tab) = self.workspace.active_mut() {
                        tab.doc.move_layer(from, to);
                        let (name, icon) = if to > from {
                            ("Move Layer Up", Icon::MoveLayerUp)
                        } else {
                            ("Move Layer Down", Icon::MoveLayerDown)
                        };
                        tab.commit(name, icon);
                    }
                }
                LayersAction::Properties(_) => {
                    self.execute(ctx, Triggered::once(Command::LayerProperties))
                }
                LayersAction::Command(c) => self.execute(ctx, Triggered::once(c)),
            }
        }
        match history_action {
            Some(windows::HistoryAction::Jump(i)) => {
                if let Some(tab) = self.workspace.active_mut() {
                    tab.jump_to(i);
                }
            }
            Some(windows::HistoryAction::Command(c)) => self.execute(ctx, Triggered::once(c)),
            None => {}
        }
    }
}

/// What command availability and check marks depend on, copied out of the app so menu
/// closures don't borrow it.
#[derive(Clone, Debug)]
pub struct CommandState {
    /// Availability of document commands (Undo, layer and image commands).
    pub document: Vec<(Command, bool)>,
    pub has_doc: bool,
    pub has_selection: bool,
    pub repeat_effect: bool,
    pub any_dirty: bool,
    pub pixel_grid: bool,
    pub rulers: bool,
    pub units: Units,
    pub panels: [bool; 4],
}

/// Commands whose availability depends on the document (see `actions.rs`).
const DOCUMENT_COMMANDS: &[Command] = &[
    Command::FeatherSelection,
    Command::Undo,
    Command::Redo,
    Command::AddNewLayer,
    Command::DeleteLayer,
    Command::DuplicateLayer,
    Command::MergeLayerDown,
    Command::ToggleLayerVisibility,
    Command::ImportFromFile,
    Command::FlipLayerHorizontal,
    Command::FlipLayerVertical,
    Command::RotateLayer180,
    Command::RotateZoom,
    Command::GoToTopLayer,
    Command::GoToLayerAbove,
    Command::GoToLayerBelow,
    Command::GoToBottomLayer,
    Command::MoveLayerToTop,
    Command::MoveLayerUp,
    Command::MoveLayerDown,
    Command::MoveLayerToBottom,
    Command::LayerProperties,
    Command::FlipImageHorizontal,
    Command::FlipImageVertical,
    Command::RotateImage90Clockwise,
    Command::RotateImage90CounterClockwise,
    Command::RotateImage180,
    Command::Resize,
    Command::CanvasSize,
    Command::Flatten,
];

impl CommandState {
    pub fn enabled(&self, command: Command) -> bool {
        if !command.implemented() {
            return false;
        }
        if let Some(&(_, on)) = self.document.iter().find(|(c, _)| *c == command) {
            return on;
        }
        match command {
            Command::Close
            | Command::Save
            | Command::SaveAs
            | Command::ZoomIn
            | Command::ZoomOut
            | Command::ZoomToWindow
            | Command::ActualSize
            | Command::TogglePixelGrid
            | Command::ToggleRulers => self.has_doc,
            Command::SaveAll => self.any_dirty,
            Command::SelectAll | Command::InvertSelection => self.has_doc,
            Command::RepeatEffect => self.repeat_effect,
            Command::Deselect
            | Command::EraseSelection
            | Command::FillSelection
            | Command::FillSelectionSecondary
            | Command::CropToSelection
            | Command::ZoomToSelection
            | Command::CopySelection => self.has_selection,
            Command::Cut | Command::Copy | Command::CopyMerged => self.has_doc,
            _ => true,
        }
    }

    pub fn checked(&self, command: Command) -> bool {
        match command {
            Command::TogglePixelGrid => self.pixel_grid,
            Command::ToggleRulers => self.rulers,
            Command::SetUnits(u) => self.units == u,
            Command::ToggleWindow(p) => {
                self.panels[Panel::ALL.iter().position(|&x| x == p).unwrap_or(0)]
            }
            _ => false,
        }
    }
}

impl PaintNotNetApp {
    /// The toolbar's feather fields edit the current selection live (one history step when
    /// the edit ends) and show the feather of whatever is selected.
    fn toolbar_feather(&mut self) {
        let (changed, committed) = (
            std::mem::take(&mut self.row2.feather_changed),
            std::mem::take(&mut self.row2.feather_committed),
        );
        let feather = self.settings.feather;
        let Some(tab) = self.workspace.active_mut() else {
            return;
        };
        if tab.doc.selection().is_empty() {
            return;
        }
        if changed && tab.doc.selection().feather() != feather {
            let sel = tab.doc.selection().clone().with_feather(feather);
            tab.doc.set_selection(sel);
        }
        if committed
            && tab.history.entries()[tab.history.current()]
                .document()
                .selection()
                .feather()
                != feather
        {
            tab.commit("Feather Selection", Icon::Selection);
        }
        let rev = tab.doc.selection_revision();
        if !changed && rev != self.synced_selection {
            self.settings.feather = tab.doc.selection().feather();
        }
        self.synced_selection = tab.doc.selection_revision();
    }
}

/// Commands that don't drop floating pixels: viewing, scrolling, windows, colours, tool
/// options.
fn keeps_move_session(c: Command) -> bool {
    matches!(
        c,
        Command::ZoomIn
            | Command::ZoomOut
            | Command::ZoomToWindow
            | Command::ZoomToSelection
            | Command::ActualSize
            | Command::TogglePixelGrid
            | Command::ToggleRulers
            | Command::SetUnits(_)
            | Command::ToggleWindow(_)
            | Command::ResetWindow(_)
            | Command::OpenMenu(_)
            | Command::ScrollPageUp
            | Command::ScrollPageDown
            | Command::ScrollPageLeft
            | Command::ScrollPageRight
            | Command::HomeKey
            | Command::EndKey
            | Command::ScrollToTopLeft
            | Command::ScrollToBottomRight
            | Command::CenterTopLeft
            | Command::CenterBottomRight
            | Command::BrushWidth(_)
            | Command::SwapColors
            | Command::SwitchActiveColor
            | Command::OpenToolDropdown
            | Command::ToggleDiagnostics
            | Command::HelpTopics
            | Command::About
    )
}

/// Open a URL in the desktop's browser.
fn open_url(url: &str) {
    if let Err(e) = std::process::Command::new("xdg-open").arg(url).spawn() {
        log::warn!("could not open {url}: {e}");
    }
}

impl eframe::App for PaintNotNetApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let now = Instant::now();
        let ctx = ui.ctx().clone();
        self.modifiers = ctx.input(|i| i.modifiers);
        self.handle_file_events();

        let modal_open = self.modal.is_some() || self.files.dialog_open() || self.menu.open;
        if !modal_open && !ctx.egui_wants_keyboard_input() {
            for t in take_commands(&ctx, &mut self.taps, now) {
                self.execute(&ctx, t);
            }
            self.keyboard_pan(&ctx);
        }
        self.handle_close_request(&ctx);
        self.diagnostics.record_frame(frame, now);

        if let Some(rs) = frame.wgpu_render_state() {
            let mut renderer = rs.renderer.write();
            if let Some(canvas) = renderer.callback_resources.get_mut::<CanvasRenderer>() {
                for key in self.released.drain(..) {
                    canvas.release(key);
                }
            }
        }

        self.top_band(ui, &ctx);
        egui::Panel::top("tool_options").show(ui, |ui| {
            let finish_enabled = false;
            let palette = theme::palette(&ctx);
            match toolbar::row2(
                ui,
                self.tool,
                &mut self.settings,
                &mut self.row2,
                &self.icons,
                &palette,
                finish_enabled,
            ) {
                Some(Row2Action::SelectTool(tool)) => self.set_tool(tool),
                Some(Row2Action::Finish) | None => {}
            }
        });
        self.toolbar_feather();
        let active = self.workspace.active();
        let info = StatusInfo {
            tool: self.tool,
            busy: self.files.busy(),
            selection_size: active.map(|t| {
                t.doc
                    .selection()
                    .pixel_bounds(t.doc.width(), t.doc.height())
                    .map_or((t.doc.width(), t.doc.height()), |r| (r.width, r.height))
            }),
            cursor: self.canvas.cursor,
            zoom: active.and_then(|t| t.view.map(|v| v.zoom)),
            units: self.units,
            dpi: active.map_or(pnn_core::DEFAULT_DPI, |t| t.doc.dpi),
        };
        let status_action = egui::Panel::bottom("status")
            .show(ui, |ui| {
                status::status_bar(ui, &info, &mut self.status, &self.icons)
            })
            .inner;
        match status_action {
            Some(StatusAction::Command(c)) => self.execute(&ctx, Triggered::once(c)),
            Some(StatusAction::SetZoom(zoom)) => {
                let vp = self.canvas.viewport;
                if let Some(tab) = self.workspace.active_mut() {
                    let img = tab.image_size();
                    if let Some(view) = tab.view.as_mut() {
                        view.zoom_centered(zoom, img, vp);
                    }
                }
            }
            None => {}
        }

        self.canvas.tool_cursor = match self.tool {
            ToolKind::Pencil
            | ToolKind::RectangleSelect
            | ToolKind::EllipseSelect
            | ToolKind::LassoSelect
            | ToolKind::MagicWand => Some(egui::CursorIcon::Crosshair),
            ToolKind::MoveSelectedPixels | ToolKind::MoveSelection => Some(egui::CursorIcon::Move),
            _ => None,
        };
        self.canvas.handles = self.move_handles();
        let preview = self.row2.feather_preview
            || self
                .doc_dialog
                .as_ref()
                .is_some_and(|d| d.wants_feather_preview());
        let antialias = self.antialiased_selection();
        self.canvas.overlay = match self.workspace.active_mut() {
            Some(tab) if preview => tab
                .selection_mask(antialias)
                .map(|m| (tab.doc.selection_revision() * 2 + u64::from(antialias), m)),
            _ => None,
        };
        let gpu = frame.wgpu_render_state();
        self.canvas.rulers = self.rulers.then(|| {
            (
                self.units,
                self.workspace
                    .active()
                    .map_or(pnn_core::DEFAULT_DPI, |t| t.doc.dpi),
            )
        });
        let workspace_rect = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let rect = ui.max_rect();
                match self.workspace.active_mut() {
                    Some(tab) => canvas::show(ui, tab, &mut self.canvas, gpu),
                    None => {
                        self.canvas.cursor = None;
                        ui.painter()
                            .rect_filled(rect, 0.0, theme::palette(ui.ctx()).surround);
                    }
                }
                rect
            })
            .inner;
        self.floating_windows(&ctx, workspace_rect, now);
        let tool_events = std::mem::take(&mut self.canvas.events);
        if self.modal.is_none() && self.doc_dialog.is_none() {
            self.tool_events(tool_events);
        }
        self.run_doc_dialog(&ctx);

        for tab in self.workspace.tabs_mut() {
            shell::update_thumbnail(&ctx, tab, now);
        }

        // Queued Save As dialogs (Save All with untitled images), one at a time.
        if self.modal.is_none() && !self.files.dialog_open() && !self.save_as_queue.is_empty() {
            let key = self.save_as_queue.remove(0);
            if let Some(tab) = self.workspace.tabs().iter().find(|t| t.key == key) {
                let (path, title) = (tab.path.clone(), tab.title());
                self.files.pick_save(key, path.as_deref(), &title);
            }
        }
        if self.exit_after_save
            && !self.files.busy()
            && self.save_as_queue.is_empty()
            && self.workspace.tabs().iter().all(|t| !t.is_dirty())
        {
            self.exit_after_save = false;
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        if self.modal.is_none()
            && let Some((title, message)) = (!self.errors.is_empty()).then(|| self.errors.remove(0))
        {
            self.modal = Some(Modal::Error { title, message });
        }
        if let Some(result) = self.modal.as_mut().and_then(|m| m.show(&ctx)) {
            self.handle_modal_result(&ctx, result);
        }

        self.update_title(&ctx);
        self.diagnostics.show(&ctx);
        self.run_smoke_test(&ctx, now);
    }
}
