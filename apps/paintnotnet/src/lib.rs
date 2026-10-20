//! The egui shell. Split from `main.rs` so UI tests can drive the app headlessly.

/// The app's ID (reverse DNS of the author's domain, maxwilson.co.uk): the Wayland app id, the
/// .desktop file, icon and AppStream names, and the single-instance D-Bus name.
pub const APP_ID: &str = "uk.co.maxwilson.paintnotnet";

pub mod actions;
pub mod ai;
pub mod app;
pub mod autosave;
pub mod canvas;
pub mod clipboard;
pub mod commands;
pub mod config;
pub mod controls;
pub mod diagnostics;
pub mod dialogs;
pub mod doc_dialogs;
pub mod effect_runner;
pub mod file_ops;
pub mod fill_tools;
pub mod glass;
pub mod global_menu;
pub mod icons;
pub mod menus;
pub mod move_tools;
pub mod paint_tools;
pub mod palette;
pub mod panels;
pub mod recent_thumbs;
pub mod selection_tools;
pub mod session_log;
pub mod settings_ui;
pub mod shell;
pub mod single_instance;
pub mod status;
pub mod text_tool;
pub mod theme;
pub mod toolbar;
pub mod vector_tools;
pub mod wayland;
pub mod windows;
pub mod workspace;
