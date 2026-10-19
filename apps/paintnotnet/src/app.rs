use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui;
use pnn_core::Document;
use pnn_render::{CanvasRenderer, View, view};

use pnn_tools::{DashStyle, LineCap, ToolKind, ToolSettings, tool_for_shortcut};

use crate::canvas::{self, ARROW_PAN_STEP, CanvasState};
use crate::commands::{Command, MultiTap, Panel, Triggered, Units, take_commands};
use crate::diagnostics::Diagnostics;
use crate::dialogs::{Modal, ModalResult, NewImage, SaveTarget};
use crate::file_ops::{FileEvent, FileOps, resolve_save_path};
use crate::icons::{Icon, Icons};
use crate::menus::{self, MenuBarState, MenuContext};
use crate::panels::{Panels, Part};
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
    /// `PNN_SMOKE_COMMAND` has run.
    command_run: bool,
}

impl SmokeTest {
    pub fn new(duration: Duration) -> Self {
        Self {
            duration,
            started: None,
            command_run: false,
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
    /// A document dialog's result being worked out in the background (large images).
    pub(crate) doc_job: Option<crate::actions::DocJob>,
    /// A tool click's work running in the background (Magic Wand, Paint Bucket).
    pub(crate) click_job: Option<crate::selection_tools::ClickJob>,
    /// For repaints requested from background work.
    pub(crate) ctx: Option<egui::Context>,
    /// Draw moved pixels on the CPU even with a GPU (tests compare the two paths).
    pub(crate) cpu_floating: bool,
    /// Notes slow and stuck frames in the session log (set by `main`).
    pub(crate) watchdog: Option<crate::session_log::Watchdog>,
    /// Largest floating-pixel block the GPU can draw (its storage buffer limit; 0 = no GPU,
    /// moves are drawn on the CPU).
    pub(crate) gpu_float_limit: u64,
    /// Pencil stroke in progress.
    pub(crate) stroke: Option<crate::actions::PencilStroke>,
    pub(crate) brush: Option<crate::paint_tools::BrushStroke>,
    pub(crate) gradient: Option<crate::fill_tools::GradientSession>,
    pub(crate) vector: Option<crate::vector_tools::VectorSession>,
    pub(crate) text: Option<crate::text_tool::TextSession>,
    /// System fonts, loaded when the Text tool is first used.
    pub(crate) fonts: Option<crate::text_tool::Fonts>,
    pub(crate) clone_source: Option<crate::paint_tools::CloneSource>,
    /// Recolor Ctrl/Shift picking in progress: (button, adjust).
    pub(crate) recolor_picking: Option<(crate::canvas::ToolButton, bool)>,
    /// Color Picker drag in progress: (button, Ctrl held at press).
    pub(crate) picker: Option<(crate::canvas::ToolButton, bool)>,
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
    /// Autosave for crash recovery (enabled by `main`, so tests don't touch the real folder).
    pub(crate) autosave: Option<crate::autosave::Autosave>,
    pub(crate) config: crate::config::Config,
    /// Where settings are saved (set by `main`; tests keep settings in memory).
    pub(crate) config_path: Option<std::path::PathBuf>,
    /// KDE Plasma global menu export (`None` off Plasma Wayland and in tests).
    pub(crate) global_menu: Option<crate::global_menu::GlobalMenu>,
    /// The window's Wayland objects (global menu link, activation). Never dropped: it
    /// borrows winit's display, which may be gone by the time the app is dropped.
    pub(crate) window_link: Option<std::mem::ManuallyDrop<crate::wayland::WindowLink>>,
    /// Receives files from later launches (`single_instance`).
    pub(crate) instance: Option<crate::single_instance::Instance>,
    pub(crate) settings_open: Option<crate::settings_ui::SettingsState>,
    pub(crate) palette: Option<crate::settings_ui::PaletteState>,
    /// The adjustment/effect dialog in progress.
    pub(crate) effect: Option<crate::effect_runner::EffectSession>,
    /// Remove Background / Select Subject: the AI model and recent mattes.
    pub(crate) ai: crate::ai::Ai,
    /// Select Subject in progress.
    pub(crate) select_subject: Option<crate::ai::SubjectSession>,
    /// Effects with an exact GPU port render there (`None`: no wgpu, or `PNN_GPU_EFFECTS=0`).
    pub(crate) gpu_effects: Option<std::sync::Arc<pnn_render::GpuEffects>>,
    /// Live previews (the gradient while its nubs move) are drawn by the GPU compositor
    /// (`false`: no wgpu, or `PNN_GPU_PREVIEW=0`).
    pub(crate) gpu_preview: bool,
    /// Last applied effect and its settings, for Repeat (Ctrl+F).
    pub(crate) last_applied: Option<(
        crate::effect_runner::EffectRef,
        Vec<pnn_effects::framework::Value>,
        Option<crate::effect_runner::Custom>,
    )>,
    /// Selection revision the toolbar's feather fields were last synced from.
    pub(crate) synced_selection: u64,
    /// Brush width and hardness when the Alt + right-drag resize started.
    pub(crate) resize_base: Option<(f32, u8)>,
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
        let gpu_effects = cc
            .wgpu_render_state
            .as_ref()
            .filter(|_| std::env::var("PNN_GPU_EFFECTS").as_deref() != Ok("0"))
            .map(|rs| std::sync::Arc::new(pnn_render::GpuEffects::new(&rs.device, &rs.queue)));
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
            doc_job: None,
            click_job: None,
            ctx: Some(cc.egui_ctx.clone()),
            gpu_float_limit: 0,
            watchdog: None,
            cpu_floating: false,
            stroke: None,
            brush: None,
            gradient: None,
            vector: None,
            text: None,
            fonts: None,
            clone_source: None,
            recolor_picking: None,
            picker: None,
            select_drag: None,
            modifiers: egui::Modifiers::NONE,
            move_session: None,
            clipboard: Box::new(crate::clipboard::SystemClipboard::new()),
            pending_paste: None,
            last_effect: None,
            autosave: None,
            config: crate::config::Config::default(),
            config_path: None,
            global_menu: None,
            window_link: None,
            instance: None,
            settings_open: None,
            palette: None,
            effect: None,
            ai: Default::default(),
            select_subject: None,
            gpu_effects,
            gpu_preview: cc.wgpu_render_state.is_some()
                && std::env::var("PNN_GPU_PREVIEW").as_deref() != Ok("0"),
            last_applied: None,
            synced_selection: 0,
            resize_base: None,
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

    /// The open adjustment/effect dialog (tests).
    /// Keep AI models in `dir` (tests; the app uses `~/.local/share/paintnotnet/models`).
    pub fn set_models_dir(&mut self, dir: PathBuf) {
        self.ai = crate::ai::Ai::with_dir(dir);
    }

    /// Run the AI helper from `exe` (tests: the built app, not the test binary).
    pub fn set_ai_worker(&mut self, exe: PathBuf) {
        self.ai.set_worker_exe(exe);
    }

    /// Select Subject is waiting (for the download question, a download or the model).
    pub fn selecting_subject(&self) -> bool {
        self.select_subject.is_some()
    }

    /// A document dialog's result is being worked out in the background.
    pub fn doc_job_running(&self) -> bool {
        self.doc_job.is_some()
    }

    /// A tool click's work (Magic Wand, Paint Bucket) is running in the background.
    pub fn click_job_running(&self) -> bool {
        self.click_job.is_some()
    }

    /// Draw moved pixels on the CPU even with a GPU (tests).
    pub fn set_cpu_floating(&mut self, on: bool) {
        self.cpu_floating = on;
    }

    /// Log slow and stuck frames (the app, not tests).
    pub fn enable_watchdog(&mut self) {
        self.watchdog = Some(crate::session_log::Watchdog::start());
    }

    /// What the app is busy with, for the session log.
    fn activity(&self) -> String {
        let mut parts = vec![format!("tool {:?}", self.tool)];
        if let Some(tab) = self.workspace.active() {
            parts.push(format!(
                "image {}×{}, {} layers, selection {} pts",
                tab.doc.width(),
                tab.doc.height(),
                tab.doc.layers().len(),
                tab.doc
                    .selection()
                    .contours()
                    .iter()
                    .map(Vec::len)
                    .sum::<usize>()
            ));
        }
        if let Some(s) = &self.move_session {
            parts.push(format!(
                "moving {:?} (gpu drag {}, m {:?})",
                s.kind, s.gpu_drag, s.m
            ));
        }
        if self.effect.is_some() {
            parts.push("effect dialog".into());
        }
        if self.doc_dialog.is_some() || self.doc_job.is_some() {
            parts.push("document dialog".into());
        }
        if self.click_job.is_some() {
            parts.push("background click job".into());
        }
        parts.join(", ")
    }

    pub fn effect_mut(&mut self) -> Option<&mut crate::effect_runner::EffectSession> {
        self.effect.as_mut()
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
            self.finish_brush_stroke();
            self.commit_gradient();
            self.commit_vector();
            self.commit_text();
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
            repeat_effect: (self.last_effect.is_some() || self.last_applied.is_some())
                && self.workspace.active().is_some(),
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
        log::info!("command {:?}", t.command);
        if self.doc_dialog.is_some()
            || self.doc_job.is_some()
            || self.click_job.is_some()
            || self.effect.is_some()
            || self.select_subject.is_some()
            || self.stroke.is_some()
            || self.brush.is_some()
        {
            return;
        }
        if t.command == Command::Finish {
            // Enter / Esc commits floating pixels or a live gradient (paint.net 5 commits on
            // Esc too; 3.36 cancelled the gradient); with nothing pending it deselects.
            if self.gradient.is_some() || self.vector.is_some() || self.text.is_some() {
                self.commit_gradient();
                self.commit_vector();
                self.commit_text();
            } else if self.move_session.is_some() {
                self.finish_move();
            } else if self.select_drag.is_none() {
                self.selection_command(Command::Deselect);
            }
            return;
        }
        if t.command == Command::Undo
            && (self.cancel_gradient() | self.cancel_vector() | self.cancel_text())
        {
            // Undo takes back the live gradient first, as if it had been committed.
            return;
        }
        if matches!(t.command, Command::Undo | Command::Redo) {
            self.abandon_move();
        } else if !keeps_move_session(t.command) {
            self.finish_move();
        }
        if !keeps_move_session(t.command) {
            self.commit_gradient();
            self.commit_vector();
            self.commit_text();
        }
        if self.clipboard_command(t.command)
            || self.selection_command(t.command)
            || self.document_command(t.command)
        {
            return;
        }
        let vp = self.canvas.viewport;
        match t.command {
            Command::Settings => {
                self.settings_open = Some(Default::default());
            }
            Command::CommandPalette => {
                self.palette = Some(Default::default());
            }
            Command::Adjustment(a) => {
                self.open_effect(ctx, crate::effect_runner::EffectRef::Adjustment(a))
            }
            Command::Effect(id) => {
                self.open_effect(ctx, crate::effect_runner::EffectRef::Effect(id))
            }
            Command::SelectSubject => self.select_subject(ctx),
            Command::RepeatEffect => {
                if !self.repeat_effect(ctx)
                    && let Some(id) = self.last_effect
                {
                    // Dialog-based effects reopen their dialog.
                    self.document_command(Command::Effect(id));
                }
            }
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
            // With the menus in the desktop's global menu there's no in-window bar to open.
            Command::OpenMenu(_) if self.global_menu_active() => {}
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
            Command::LineStartCap => {
                self.settings.start_cap = next(&LineCap::ALL, self.settings.start_cap)
            }
            Command::LineEndCap => {
                self.settings.end_cap = next(&LineCap::ALL, self.settings.end_cap)
            }
            Command::LineStyle => self.settings.dash = next(&DashStyle::ALL, self.settings.dash),
            Command::CycleShape { forward } => {
                self.settings.shape = self.settings.shape.cycle(forward)
            }
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
        if let Some(a) = self.autosave.as_mut() {
            a.forget(key);
        }
        if self.workspace.remove(key).is_some() {
            self.released.push(key);
        }
    }

    /// Export the menus to KDE Plasma's global menu (made by `main` from the window).
    pub fn set_global_menu(&mut self, menu: Option<crate::global_menu::GlobalMenu>) {
        self.global_menu = menu;
    }

    /// The window's Wayland objects (made by `main`).
    pub fn set_window_link(&mut self, link: Option<crate::wayland::WindowLink>) {
        self.window_link = link.map(std::mem::ManuallyDrop::new);
    }

    /// Take files from later launches (`single_instance`).
    pub fn set_instance(&mut self, instance: crate::single_instance::Instance) {
        self.instance = Some(instance);
    }

    /// Open files handed over by later launches and raise the window for them.
    fn take_handoffs(&mut self, ctx: &egui::Context) {
        let Some(instance) = &self.instance else {
            return;
        };
        for handoff in instance.take() {
            self.files.open_paths(handoff.paths);
            let activated = handoff.activation_token.as_deref().is_some_and(|token| {
                self.window_link
                    .as_ref()
                    .is_some_and(|link| link.activate(token))
            });
            if !activated {
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
    }

    /// Whether the menus are showing in the desktop's global menu instead of the window.
    pub fn global_menu_active(&self) -> bool {
        self.global_menu.as_ref().is_some_and(|g| g.active())
    }

    /// Keep the global menu's state current and run what was picked from it. While a dialog
    /// is open everything shows disabled and clicks are ignored, as the in-window bar would be
    /// unreachable.
    fn sync_global_menu(&mut self, ctx: &egui::Context, modal_open: bool) {
        let Some(mut global) = self.global_menu.take() else {
            return;
        };
        let state = self.command_state();
        let enabled = |c: Command| !modal_open && state.enabled(c);
        let checked = |c: Command| state.checked(c);
        let cx = MenuContext {
            enabled: &enabled,
            checked: &checked,
            last_effect: self.last_effect_name(),
        };
        let Some(link) = self.window_link.as_deref_mut() else {
            self.global_menu = Some(global);
            return;
        };
        let picked = global.update(link, self.config.global_menu, &menus::menus(), &cx);
        self.global_menu = Some(global);
        if !modal_open {
            for c in picked {
                if self.availability(c) {
                    self.execute(ctx, Triggered::once(c));
                }
            }
        }
    }

    /// Use `config` (loaded by `main` from `path`; changes are saved back there).
    pub fn set_config(
        &mut self,
        ctx: &egui::Context,
        config: crate::config::Config,
        path: Option<std::path::PathBuf>,
    ) {
        self.config = config;
        self.config_path = path;
        self.apply_config(ctx);
    }

    /// Test access to the settings.
    pub fn config(&self) -> &crate::config::Config {
        &self.config
    }

    /// Turn on autosave into `state_dir`, offering to recover images a crashed session left.
    pub fn enable_autosave(
        &mut self,
        state_dir: std::path::PathBuf,
        interval: std::time::Duration,
    ) {
        let a = crate::autosave::Autosave::new(&state_dir, interval);
        let items = a.recoverable();
        self.autosave = Some(a);
        if !items.is_empty() && self.modal.is_none() {
            self.modal = Some(Modal::Recover { items });
        }
    }

    fn run_autosave(&mut self, now: Instant) {
        let Some(a) = self.autosave.as_mut() else {
            return;
        };
        if !a.due(now) {
            return;
        }
        let docs = self
            .workspace
            .tabs()
            .iter()
            .map(|t| {
                (
                    t.key,
                    t.doc.clone(),
                    t.is_dirty(),
                    t.title(),
                    t.path.clone(),
                )
            })
            .collect();
        a.tick(now, docs);
    }

    /// Open the crashed session's images as unsaved documents (with their original paths, so
    /// Save puts them back).
    fn recover(&mut self) {
        let Some(a) = self.autosave.as_ref() else {
            return;
        };
        for r in a.recoverable() {
            match pnn_io::load(&r.file) {
                Ok(doc) => {
                    let format = r
                        .original
                        .as_deref()
                        .and_then(pnn_io::FileFormat::from_path);
                    let key = self.workspace.add(doc, r.original.clone(), format);
                    if let Some(tab) = self.workspace.tab_mut(key) {
                        tab.mark_saved(0);
                    }
                }
                Err(e) => log::warn!("recovery: {e}"),
            }
        }
        if let Some(a) = self.autosave.as_ref() {
            a.clear_recoverable();
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
        if tab.doc.layers().len() > 1 && !target.flattened && !target.format.layered() {
            self.modal = Some(Modal::FlattenForSave { key, then: target });
        } else if target.format.has_options() {
            self.modal = Some(Modal::SaveConfig {
                key,
                options: tab.save_options,
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
                        if let Some(a) = self.autosave.as_mut() {
                            a.forget(key);
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
            ModalResult::Recover => self.recover(),
            ModalResult::DiscardRecovered => {
                if let Some(a) = self.autosave.as_ref() {
                    a.clear_recoverable();
                }
            }
            ModalResult::SaveConfigured { key, options, then } => {
                if let Some(tab) = self.workspace.tab_mut(key) {
                    tab.save_options = options;
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
            } else {
                for d in dirs {
                    self.nudge_vector(d[0] * step, d[1] * step);
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
        let duration = smoke.duration;
        // PNN_SMOKE_LAYERS=<n>: stack n copies of the image over it first (blend modes in
        // turn, half opacity), to measure compositing.
        if let Ok(n) = std::env::var("PNN_SMOKE_LAYERS")
            && elapsed.is_zero()
            && let Some(tab) = self.workspace.active_mut()
        {
            let base = tab.doc.layer(0).clone();
            for i in 0..n.parse::<usize>().unwrap_or(0) {
                let mut l = pnn_core::Layer::new(format!("Layer {}", i + 2), base.surface.clone());
                l.blend_mode = pnn_core::BlendMode::ALL[(i + 1) % pnn_core::BlendMode::ALL.len()];
                l.opacity = 128;
                tab.doc.insert_layer(tab.doc.layers().len(), l);
            }
        }
        // PNN_SMOKE_WAND=x,y: one Magic Wand click there (image px) as soon as an image is open,
        // to time a complex selection.
        if self.smoke.as_ref().is_some_and(|s| !s.command_run)
            && let Ok(at) = std::env::var("PNN_SMOKE_WAND")
            && let Some(key) = self.workspace.active().map(|t| t.key)
        {
            if let Some(s) = self.smoke.as_mut() {
                s.command_run = true;
            }
            let p: Vec<f64> = at
                .split(',')
                .filter_map(|v| v.trim().parse().ok())
                .collect();
            if let [x, y] = p[..] {
                let t = Instant::now();
                self.magic_wand(key, [x, y], pnn_core::CombineMode::Replace, true);
                log::info!("smoke: magic wand took {:?}", t.elapsed());
            }
        }
        // PNN_SMOKE_COMMAND=<command id, e.g. SelectSubject>: run that menu command once, as
        // soon as an image is open.
        if self.smoke.as_ref().is_some_and(|s| !s.command_run)
            && self.workspace.active().is_some()
            && let Ok(id) = std::env::var("PNN_SMOKE_COMMAND")
        {
            if let Some(s) = self.smoke.as_mut() {
                s.command_run = true;
            }
            if let Some(c) = crate::menus::all_commands()
                .into_iter()
                .find(|c| c.id() == id)
            {
                self.execute(ctx, Triggered::once(c));
            }
        }
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
        } else if let Ok(name) = std::env::var("PNN_SMOKE_EFFECT") {
            // PNN_SMOKE_EFFECT=1: Posterize open, its levels changing every frame (the live
            // preview of a whole-image adjustment). PNN_SMOKE_EFFECT=<effect name>: that
            // effect, its integer settings changing every frame.
            if self.effect.is_none() {
                let target = pnn_effects::Category::ALL
                    .into_iter()
                    .flat_map(pnn_effects::effects_in)
                    .find(|(_, e)| e.name == name)
                    .map_or(
                        crate::effect_runner::EffectRef::Adjustment(
                            pnn_effects::Adjustment::Posterize,
                        ),
                        |(id, _)| crate::effect_runner::EffectRef::Effect(id),
                    );
                self.open_effect(ctx, target);
            }
            let levels = 2 + (elapsed.as_secs_f64() * 20.0) as i32 % 30;
            if let Some(e) = self.effect.as_mut() {
                e.set_all_ints(ctx, levels);
            }
        } else if let Ok(kind) = std::env::var("PNN_SMOKE_GRADIENT") {
            // PNN_SMOKE_GRADIENT=<type index>: drag a live gradient's end nub every frame.
            let Some(tab) = self.workspace.active() else {
                return;
            };
            let (w, h) = (tab.doc.width() as f64, tab.doc.height() as f64);
            let t = elapsed.as_secs_f64();
            let pos = [
                w * (0.5 + 0.4 * (t * 1.1).cos()),
                h * (0.5 + 0.4 * (t * 1.7).sin()),
            ];
            if self.gradient.is_none() {
                self.tool = ToolKind::Gradient;
                let i: usize = kind.parse().unwrap_or(0);
                self.settings.gradient_type = pnn_tools::GradientType::ALL[i.min(6)];
                self.gradient_event(crate::canvas::ToolEvent::Down {
                    pos: [w / 2.0, h / 2.0],
                    button: crate::canvas::ToolButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            self.gradient_event(crate::canvas::ToolEvent::Move { pos });
        } else if let Ok(width) = std::env::var("PNN_SMOKE_BRUSH") {
            // PNN_SMOKE_BRUSH=<width>: paint one long Paintbrush stroke, a pointer move per
            // frame, across the image.
            let Some(tab) = self.workspace.active() else {
                return;
            };
            let (w, h) = (tab.doc.width() as f64, tab.doc.height() as f64);
            let t = elapsed.as_secs_f64();
            let pos = [
                w * (0.5 + 0.42 * (t * 0.8).cos()),
                h * (0.5 + 0.4 * (t * 1.9).sin()),
            ];
            if self.brush.is_none() {
                self.tool = ToolKind::Paintbrush;
                self.settings.brush_width = width.parse().unwrap_or(50.0);
                self.settings.primary = pnn_core::ColorBgra::from_rgba(200, 40, 40, 160);
                self.brush_tool_event(crate::canvas::ToolEvent::Down {
                    pos,
                    button: crate::canvas::ToolButton::Primary,
                    modifiers: egui::Modifiers::NONE,
                });
            } else {
                self.brush_tool_event(crate::canvas::ToolEvent::Move { pos });
            }
        } else if std::env::var_os("PNN_SMOKE_COMMAND").is_some()
            || std::env::var_os("PNN_SMOKE_WAND").is_some()
        {
            // Leave the view alone, to see what the command did.
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
        if elapsed < duration {
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
        if std::env::var_os("PNN_PROFILE").is_some() {
            println!("profile: {}", crate::diagnostics::profile_summary());
        }
        self.allow_close = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        self.smoke = None;
    }
}

/// Height of the title bar and of the status bar.
const TITLE_BAR_HEIGHT: f32 = 44.0;
const STATUS_BAR_HEIGHT: f32 = 30.0;

impl PaintNotNetApp {
    /// The title bar: menus on the left, the document tabs in the middle, then command
    /// search, the panel toggles (paint.net's top-right icons), Settings and Help.
    fn title_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // The two sides of the bar run one after the other; share state through cells.
        let picked: std::cell::Cell<Option<Command>> = std::cell::Cell::new(None);
        let menu = std::cell::RefCell::new(std::mem::take(&mut self.menu));
        let global_menu = self.global_menu_active();
        let own_buttons = !self.config.system_title_bar;
        let palette = theme::palette(ctx);
        let mut list_actions = Vec::new();
        egui::Panel::top("title_bar")
            .exact_size(TITLE_BAR_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(palette.band)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
            .show(ui, |ui| {
                let state = self.command_state();
                let availability = |c: Command| state.enabled(c);
                let checked = |c: Command| state.checked(c);
                let cx = MenuContext {
                    enabled: &availability,
                    checked: &checked,
                    last_effect: self.last_effect_name(),
                };
                let full = ui.max_rect();
                // Under everything else in the bar: its empty space moves the window.
                let drag_area = own_buttons.then(|| {
                    ui.interact(
                        ui.max_rect().expand2(egui::vec2(12.0, 0.0)),
                        ui.id().with("title bar drag"),
                        egui::Sense::click_and_drag(),
                    )
                });
                let row =
                    |layout: egui::Layout| egui::UiBuilder::new().max_rect(full).layout(layout);
                let left = ui
                    .scope_builder(
                        row(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| {
                            logo(ui, &palette);
                            if !global_menu {
                                let size =
                                    egui::vec2(menus::menu_bar_width(ui), ui.available_height());
                                let bar = ui.allocate_ui_with_layout(
                                    size,
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        menus::menu_bar(
                                            ui,
                                            &mut menu.borrow_mut(),
                                            &cx,
                                            &self.icons,
                                        )
                                    },
                                );
                                if let Some(c) = bar.inner {
                                    picked.set(Some(c));
                                }
                            }
                            if global_menu {
                                menu.borrow_mut().open = false;
                            }
                        },
                    )
                    .response
                    .rect;
                let right = ui
                    .scope_builder(
                        row(egui::Layout::right_to_left(egui::Align::Center)),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 2.0;
                            ui.spacing_mut().button_padding = egui::vec2(6.0, 6.0);
                            if own_buttons {
                                shell::window_buttons(ui, &self.icons);
                                ui.separator();
                            }
                            if let Some(c) =
                                menus::help_button(ui, &mut menu.borrow_mut(), &cx, &self.icons)
                            {
                                picked.set(Some(c));
                            }
                            let tip = format!(
                                "Settings ({})",
                                Command::Settings.shortcut_text().unwrap_or_default()
                            );
                            let settings = ui
                                .add_enabled(
                                    availability(Command::Settings),
                                    egui::Button::image(self.icons.image(Icon::Settings))
                                        .frame_when_inactive(false),
                                )
                                .on_hover_text(&tip)
                                .on_disabled_hover_text(&tip);
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
                                    .frame_when_inactive(false)
                                    .selected(self.panels.visible(panel));
                                if ui.add(button).on_hover_text(tip).clicked() {
                                    // Ctrl+Shift+click resets the panel, as in paint.net.
                                    let reset = ui.input(|i| i.modifiers.ctrl && i.modifiers.shift);
                                    picked.set(Some(if reset {
                                        Command::ResetWindow(panel)
                                    } else {
                                        command
                                    }));
                                }
                            }
                            ui.separator();
                            if search_field(ui, &palette).clicked() {
                                picked.set(Some(Command::CommandPalette));
                            }
                        },
                    )
                    .response
                    .rect;
                // The tabs, centred in the space between.
                let middle = egui::Rect::from_min_max(
                    egui::pos2(left.max.x + 16.0, full.min.y),
                    egui::pos2(right.min.x - 16.0, full.max.y),
                );
                if middle.width() > 60.0 {
                    ui.scope_builder(
                        egui::UiBuilder::new()
                            .max_rect(middle)
                            .layout(egui::Layout::left_to_right(egui::Align::Center)),
                        |ui| list_actions = shell::image_list(ui, &self.workspace, &palette),
                    );
                }
                if let Some(bar) = drag_area {
                    shell::title_bar_drag(ctx, &bar);
                }
            });
        if own_buttons {
            shell::resize_edges(ctx);
        }
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

    /// The tool options bar: the quick buttons (New … Rulers), then the tool's options.
    fn options_bar(&mut self, ctx: &egui::Context, panels: &mut Panels) {
        let palette = theme::palette(ctx);
        // The toolbar's Finish commits pending tool work, as Enter does.
        let finish_enabled = self.gradient.is_some()
            || self.vector.is_some()
            || self.text.is_some()
            || self.move_session.is_some();
        let state = self.command_state();
        let availability = |c: Command| state.enabled(c);
        let checked = |c: Command| state.checked(c);
        let mut quick = None;
        let mut action = None;
        panels.show_options(ctx, &palette, |ui| {
            ui.horizontal_wrapped(|ui| {
                quick = crate::controls::group(ui, "quick buttons", |ui| {
                    toolbar::row1(ui, &self.icons, &availability, &checked)
                });
                ui.separator();
                action = toolbar::row2(
                    ui,
                    self.tool,
                    &mut self.settings,
                    &mut self.row2,
                    &self.icons,
                    &palette,
                    finish_enabled,
                );
            });
        });
        match action {
            Some(Row2Action::SelectTool(tool)) => self.set_tool(tool),
            Some(Row2Action::Finish) => {
                self.commit_gradient();
                self.commit_vector();
                self.commit_text();
                self.finish_move();
            }
            None => {}
        }
        if let Some(c) = quick {
            self.execute(ctx, Triggered::once(c));
        }
    }

    /// The tool rail and the right dock (Colors, Layers, History).
    fn docked_panels(&mut self, ctx: &egui::Context, panels: &mut Panels, now: Instant) {
        let palette = theme::palette(ctx);
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
        panels.show_rail(ctx, &palette, |ui, max_height| {
            picked_tool = windows::tools(ui, tool, icons, &palette, max_height)
        });
        let settings = &mut self.settings;
        let colors = &mut self.colors;
        let workspace_tabs = &mut self.workspace;
        panels.show_dock(ctx, &palette, |panel, part, ui| match (panel, part) {
            (Panel::Colors, Part::Actions) => colors.actions(ui, icons),
            (Panel::Colors, Part::Body) => colors.ui(ui, settings, icons),
            (Panel::Layers, Part::Actions) => {
                if let Some(c) = windows::layers_actions(ui, icons, &enabled) {
                    layer_actions.push(LayersAction::Command(c));
                }
            }
            (Panel::Layers, Part::Body) => layer_actions.extend(windows::layers(
                ui,
                workspace_tabs.active_mut(),
                &palette,
                now,
            )),
            (Panel::History, Part::Actions) => {
                if let Some(c) = windows::history_actions(ui, icons, &enabled) {
                    history_action = Some(windows::HistoryAction::Command(c));
                }
            }
            (Panel::History, Part::Body) => {
                if let Some(a) = windows::history(ui, workspace_tabs.active(), icons, &palette) {
                    history_action = Some(a);
                }
            }
            (Panel::Tools, _) => {}
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

/// The app mark at the left of the title bar.
fn logo(ui: &mut egui::Ui, palette: &theme::Palette) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
    let r = egui::Rect::from_center_size(rect.center(), egui::vec2(18.0, 18.0));
    ui.painter().rect_filled(r, 5, palette.accent);
    let p = |x: f32, y: f32| r.min + egui::vec2(x, y) * (18.0 / 24.0) - egui::vec2(3.0, 3.0) * 0.75;
    ui.painter()
        .add(egui::epaint::QuadraticBezierShape::from_points_stroke(
            [p(7.0, 16.0), p(12.0, 14.0), p(17.0, 8.0)],
            false,
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(2.0, palette.on_accent),
        ));
    ui.add_space(6.0);
}

/// The title bar's "Search commands" box; clicking it opens the command palette.
fn search_field(ui: &mut egui::Ui, palette: &theme::Palette) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(220.0, 28.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Search commands")
    });
    let fill = if response.hovered() {
        palette.hover.gamma_multiply(1.6)
    } else {
        palette.hover
    };
    ui.painter().rect(
        rect,
        theme::radius::CONTROL,
        fill,
        egui::Stroke::new(1.0, palette.window_border),
        egui::StrokeKind::Inside,
    );
    // Magnifier.
    let c = egui::pos2(rect.min.x + 15.0, rect.center().y - 1.0);
    let stroke = egui::Stroke::new(1.5, palette.faint_text);
    ui.painter().circle_stroke(c, 4.5, stroke);
    ui.painter()
        .line_segment([c + egui::vec2(3.3, 3.3), c + egui::vec2(6.5, 6.5)], stroke);
    ui.painter().text(
        egui::pos2(rect.min.x + 28.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        "Search commands",
        egui::FontId::proportional(theme::text::BODY),
        palette.faint_text,
    );
    let keys = Command::CommandPalette.shortcut_text().unwrap_or_default();
    let galley = ui.painter().layout_no_wrap(
        keys,
        egui::FontId::monospace(theme::text::SMALL),
        palette.faint_text,
    );
    let chip = egui::Rect::from_min_size(
        egui::pos2(
            rect.max.x - 8.0 - galley.size().x - 12.0,
            rect.center().y - 10.0,
        ),
        egui::vec2(galley.size().x + 12.0, 20.0),
    );
    ui.painter()
        .rect_filled(chip, theme::radius::SMALL, palette.hover);
    ui.painter().galley(
        chip.center() - galley.size() / 2.0,
        galley,
        palette.faint_text,
    );
    response.on_hover_text("Search commands")
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
            Command::SelectAll | Command::InvertSelection | Command::SelectSubject => self.has_doc,
            Command::RepeatEffect => self.repeat_effect,
            Command::Adjustment(_) => self.has_doc,
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
    /// Alt + right-drag on the canvas: sideways scales the brush (100 pt doubles or halves
    /// it), up and down change its hardness (2 pt per percent).
    fn brush_resize(&mut self) {
        let Some(r) = self.canvas.resize else {
            self.resize_base = None;
            return;
        };
        if r.started || self.resize_base.is_none() {
            self.resize_base = Some((self.settings.brush_width, self.settings.hardness));
        }
        let Some((width, hardness)) = self.resize_base else {
            return;
        };
        let scaled = width * 2f32.powf(r.delta.x / 100.0);
        self.settings.brush_width = crate::controls::round_width(
            scaled.clamp(pnn_tools::MIN_BRUSH_WIDTH, pnn_tools::MAX_BRUSH_WIDTH),
        );
        if self.canvas.brush.is_some_and(|b| b.hardness.is_some()) {
            let h = f32::from(hardness) - r.delta.y / 2.0;
            self.settings.hardness = h.round().clamp(0.0, 100.0) as u8;
        }
    }

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
            | Command::LineStartCap
            | Command::LineStyle
            | Command::LineEndCap
            | Command::CycleShape { .. }
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
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        // After a window move or resize, the button release the desktop kept.
        if let Some(release) = shell::take_release(ctx) {
            raw_input.events.insert(0, release);
        }
    }

    fn on_exit(&mut self) {
        // A clean exit leaves nothing to recover.
        if let Some(a) = self.autosave.as_mut() {
            a.finish();
        }
        // A preview still rendering on a worker holds the GPU device: cancel it and let it
        // let go, so the device is destroyed here rather than on that thread while the
        // process exits (which crashes the NVIDIA driver).
        self.effect = None;
        self.select_subject = None;
        // Free the AI model (and its GPU memory) here, not as the process exits.
        self.ai.unload();
        if let Some(gpu) = self.gpu_effects.take() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while std::sync::Arc::strong_count(&gpu) > 1 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        if self.watchdog.is_some() {
            let activity = self.activity();
            if let Some(w) = self.watchdog.as_mut() {
                w.frame_begin(activity);
            }
        }
        self.frame(ui, frame);
        if let Some(w) = self.watchdog.as_mut() {
            w.frame_end();
        }
    }
}

impl PaintNotNetApp {
    fn frame(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let now = Instant::now();
        let ctx = ui.ctx().clone();
        self.run_autosave(now);
        self.modifiers = ctx.input(|i| i.modifiers);
        self.take_handoffs(&ctx);
        self.handle_file_events();

        let modal_open = self.modal.is_some()
            || self.effect.is_some()
            || self.select_subject.is_some()
            || self.settings_open.is_some()
            || self.palette.is_some()
            || self.files.dialog_open()
            || self.menu.open;
        if !modal_open && !ctx.egui_wants_keyboard_input() {
            self.text_keyboard(&ctx);
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

        self.icons.update(&ctx);
        // No glass until the canvas draws its backdrop this frame.
        crate::glass::set_backdrop(&ctx, None);
        self.sync_global_menu(&ctx, modal_open && !self.menu.open);
        self.title_bar(ui, &ctx);
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
        let band = theme::palette(&ctx).band;
        let status_action = egui::Panel::bottom("status")
            .exact_size(STATUS_BAR_HEIGHT)
            .frame(
                egui::Frame::new()
                    .fill(band)
                    .inner_margin(egui::Margin::symmetric(16, 0)),
            )
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

        // Pan, the move tools and Text keep the system's grab, move and I-beam cursors; the
        // rest get a drawn crosshair with the tool's icon.
        self.canvas.tool_cursor = Some(match self.tool {
            ToolKind::Pan => canvas::ToolCursor::System(egui::CursorIcon::Grab),
            ToolKind::Text => canvas::ToolCursor::System(egui::CursorIcon::Text),
            // As paint.net: what a drag from here would do.
            ToolKind::MoveSelectedPixels | ToolKind::MoveSelection => {
                use crate::move_tools::MoveHover;
                use egui::CursorIcon as C;
                match self.move_hover() {
                    Some(MoveHover::Handle(i)) => canvas::ToolCursor::System(
                        [
                            C::ResizeNorthWest,
                            C::ResizeNorth,
                            C::ResizeNorthEast,
                            C::ResizeEast,
                            C::ResizeSouthEast,
                            C::ResizeSouth,
                            C::ResizeSouthWest,
                            C::ResizeWest,
                        ][i],
                    ),
                    Some(MoveHover::Outside) => {
                        canvas::ToolCursor::Drawn(self.icons.texture(Icon::RotateCursor).id())
                    }
                    _ => canvas::ToolCursor::System(C::Move),
                }
            }
            tool => canvas::ToolCursor::Drawn(self.icons.texture(Icon::for_tool(tool)).id()),
        });
        self.refresh_gradient();
        self.refresh_vector();
        self.refresh_text();
        self.canvas.text_handles = self.text_handles();
        if self.tool == ToolKind::Text && self.row2.font_families.is_empty() {
            let fonts = self.fonts.get_or_insert_with(crate::text_tool::Fonts::load);
            self.row2.font_families = fonts.families.clone();
        }
        self.canvas.vector_handles = self.vector_handles();
        self.canvas.handles = self.move_handles();
        self.canvas.gradient_nubs = self.gradient_nubs();
        // Tools with a width can be resized with Alt + right-drag; brushes have a hardness too.
        let brushes = matches!(
            self.tool,
            ToolKind::Paintbrush | ToolKind::Eraser | ToolKind::CloneStamp
        );
        self.canvas.brush = matches!(
            self.tool,
            ToolKind::Paintbrush
                | ToolKind::Eraser
                | ToolKind::CloneStamp
                | ToolKind::Recolor
                | ToolKind::LineCurve
                | ToolKind::Shapes
        )
        .then(|| canvas::BrushInfo {
            width: self.settings.brush_width,
            hardness: brushes.then(|| f32::from(self.settings.hardness) / 100.0),
        });
        self.canvas.brush_outline = matches!(
            self.tool,
            ToolKind::Paintbrush | ToolKind::Eraser | ToolKind::Recolor | ToolKind::CloneStamp
        )
        .then_some(self.settings.brush_width);
        self.canvas.clone_source = if self.tool == ToolKind::CloneStamp {
            self.clone_source_marker(self.canvas.cursor)
        } else {
            None
        };
        let preview = self.row2.feather_preview
            || self
                .doc_dialog
                .as_ref()
                .is_some_and(|d| d.wants_feather_preview());
        let antialias = self.antialiased_selection();
        self.canvas.antialias = antialias;
        self.gpu_float_limit = frame
            .wgpu_render_state()
            .filter(|_| !self.cpu_floating)
            .map_or(0, |rs| rs.device.limits().max_storage_buffer_binding_size);
        // Floating pixels being dragged are drawn by the GPU compositor.
        let nearest = self.settings.resampling == pnn_tools::Resampling::NearestNeighbor;
        let floating = self.move_session.as_ref().and_then(|s| {
            let tab = self.workspace.tab(s.key)?;
            Some((s.key, s.floating_preview(&tab.doc, nearest)))
        });
        for tab in self.workspace.tabs_mut() {
            tab.gpu.floating = floating
                .as_ref()
                .filter(|(k, _)| *k == tab.key)
                .and_then(|(_, f)| f.clone());
        }
        let active = self.workspace.active().map(|t| t.key);
        self.canvas.ants_transform = self
            .move_session
            .as_ref()
            .filter(|s| Some(s.key) == active)
            .and_then(|s| s.ants_transform());
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
        let mut panels = std::mem::take(&mut self.panels);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let rect = ui.max_rect();
                let free = panels.arrange(ui.ctx(), rect);
                match self.workspace.active_mut() {
                    Some(tab) => canvas::show(ui, tab, &mut self.canvas, free, gpu),
                    None => {
                        self.canvas.cursor = None;
                        ui.painter()
                            .rect_filled(rect, 0.0, theme::palette(ui.ctx()).surround);
                    }
                }
            });
        self.options_bar(&ctx, &mut panels);
        self.docked_panels(&ctx, &mut panels, now);
        if panels.take_changed() {
            self.config.dock = panels.to_dock();
            self.save_config();
        }
        self.panels = panels;
        self.brush_resize();
        let tool_events = std::mem::take(&mut self.canvas.events);
        // A finished Magic Wand / Paint Bucket job lands first; while one runs, tool input
        // waits (it works on a snapshot of the image).
        let click_running = self.take_click_job();
        if click_running {
            ctx.set_cursor_icon(egui::CursorIcon::Progress);
        }
        if self.modal.is_none()
            && self.doc_dialog.is_none()
            && self.doc_job.is_none()
            && !click_running
        {
            self.tool_events(tool_events);
        }
        self.run_doc_dialog(&ctx);
        self.run_effect(&ctx);
        self.run_select_subject(&ctx);
        self.ai.tick();
        self.run_settings(&ctx);
        self.run_palette(&ctx);

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
        if let Some(result) = self.modal.as_mut().and_then(|m| m.show(&ctx, &self.icons)) {
            self.handle_modal_result(&ctx, result);
        }

        self.update_title(&ctx);
        self.diagnostics.show(&ctx);
        self.run_smoke_test(&ctx, now);
    }
}

/// The item after `v` in `all`, wrapping (`,` `.` `/` cycle the line caps and style).
fn next<T: Copy + PartialEq>(all: &[T], v: T) -> T {
    let i = all.iter().position(|&x| x == v).unwrap_or(0);
    all[(i + 1) % all.len()]
}
