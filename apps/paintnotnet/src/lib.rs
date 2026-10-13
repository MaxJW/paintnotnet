//! The egui shell. Split from `main.rs` so UI tests can drive the app headlessly.

pub mod actions;
pub mod app;
pub mod canvas;
pub mod clipboard;
pub mod commands;
pub mod diagnostics;
pub mod dialogs;
pub mod doc_dialogs;
pub mod file_ops;
pub mod fill_tools;
pub mod icons;
pub mod menus;
pub mod move_tools;
pub mod paint_tools;
pub mod palette;
pub mod panels;
pub mod selection_tools;
pub mod shell;
pub mod status;
pub mod text_tool;
pub mod theme;
pub mod toolbar;
pub mod vector_tools;
pub mod windows;
pub mod workspace;
