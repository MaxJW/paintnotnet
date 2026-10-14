//! The app's UI and render tests, as one test binary (one link instead of one per file).
//! Tests that change process-wide state (the shortcut table) live in their own binaries.

mod adjust_ui;
mod canvas_render;
mod canvas_view_ui;
mod clipboard_ui;
mod colors_ui;
mod common;
mod diagnostics_chord;
mod dialog_ui;
mod effects_sheet;
mod effects_ui;
mod feather_ui;
mod fill_ui;
mod keymap_conformance;
mod line_ui;
mod ora_ui;
mod paint_ui;
mod phase1_ui;
mod phase3_ui;
mod phase4_ui;
mod picker_ui;
mod recovery_ui;
mod shapes_ui;
mod shell_render;
mod text_ui;
mod webp_ui;
