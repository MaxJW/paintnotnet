//! Settings dialog (Alt+X, the gear icon) and the command palette (Ctrl+K).
//!
//! Neither exists in paint.net 5 in this form (its Settings dialog has no shortcut editor), so
//! both are paintnotnet additions, kept in paint.net's plain dialog style.

use eframe::egui;

use crate::app::PaintNotNetApp;
use crate::commands::{Chord, Command, Triggered};
use crate::config::{Appearance, MouseWheel};
use crate::menus::{all_commands, command_label};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    General,
    Keyboard,
    Models,
}

#[derive(Default)]
pub struct SettingsState {
    tab: Tab,
    /// AI models: the file whose Delete was pressed, waiting for Yes.
    confirm_delete: Option<std::path::PathBuf>,
    /// The search box has been focused since the Keyboard tab opened.
    search_focused: bool,
    search: String,
    /// Waiting for a key press to bind to this command.
    capturing: Option<Command>,
    message: Option<String>,
}

#[derive(Default)]
pub struct PaletteState {
    query: String,
    selected: usize,
    focused: bool,
}

/// Commands matching every word of `query` (case-insensitive), in menu order.
pub fn palette_matches(query: &str) -> Vec<(Command, String)> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    all_commands()
        .into_iter()
        .filter(|c| c.implemented() && !matches!(c, Command::CommandPalette))
        .map(|c| (c, command_label(c)))
        .filter(|(_, l)| {
            let l = l.to_lowercase();
            words.iter().all(|w| l.contains(w.as_str()))
        })
        .collect()
}

impl PaintNotNetApp {
    /// Apply the settings: appearance, shortcut overrides, autosave interval.
    pub(crate) fn apply_config(&mut self, ctx: &egui::Context) {
        ctx.set_theme(match self.config.appearance {
            Appearance::System => egui::ThemePreference::System,
            Appearance::Dark => egui::ThemePreference::Dark,
            Appearance::Light => egui::ThemePreference::Light,
        });
        crate::commands::set_overrides(self.config.overrides());
        self.canvas.wheel = self.config.mouse_wheel;
        ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(
            self.config.system_title_bar,
        ));
        self.panels.load(&self.config.dock);
        if let Some(a) = self.autosave.as_mut() {
            a.set_interval(self.config.autosave_minutes);
        }
    }

    fn config_changed(&mut self, ctx: &egui::Context) {
        self.apply_config(ctx);
        self.save_config();
    }

    /// Write the settings file (tests have no path and keep settings in memory).
    pub(crate) fn save_config(&self) {
        if let Some(p) = &self.config_path
            && let Err(e) = self.config.save(p)
        {
            log::warn!("saving settings: {e}");
        }
    }

    pub(crate) fn run_settings(&mut self, ctx: &egui::Context) {
        let Some(mut st) = self.settings_open.take() else {
            return;
        };
        // Shortcut capture: the next key press (with its modifiers) becomes the binding.
        if let Some(c) = st.capturing {
            let pressed = ctx.input_mut(|i| {
                let mut found = None;
                i.events.retain(|e| match e {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } if found.is_none() => {
                        found = Some((*key, *modifiers));
                        false
                    }
                    _ => true,
                });
                found
            });
            if let Some((key, m)) = pressed {
                st.capturing = None;
                if key == egui::Key::Escape && !m.ctrl && !m.alt && !m.shift {
                    st.message = None;
                } else {
                    let chord = Chord {
                        ctrl: m.ctrl,
                        alt: m.alt,
                        shift: Some(m.shift),
                        key,
                    };
                    st.message = self.bind(c, chord);
                    self.config_changed(ctx);
                }
            }
        }
        let mut open = true;
        let mut changed = false;
        let path = self.config_path.clone();
        crate::glass::window(
            ctx,
            egui::Window::new("Settings")
                .id(egui::Id::new("settings_dialog"))
                .open(&mut open)
                .collapsible(false)
                .resizable(true)
                .default_size(egui::vec2(560.0, 460.0))
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center()),
            |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut st.tab, Tab::General, "General");
                    if ui
                        .selectable_value(&mut st.tab, Tab::Keyboard, "Keyboard")
                        .clicked()
                    {
                        st.search_focused = false;
                    }
                    ui.selectable_value(&mut st.tab, Tab::Models, "AI models");
                });
                ui.separator();
                if st.tab == Tab::Models {
                    self.models_tab_ui(ui, &mut st);
                } else if st.tab == Tab::General {
                    ui.label(egui::RichText::new("Appearance").strong());
                    ui.horizontal(|ui| {
                        for (a, name) in [
                            (Appearance::System, "Follow system"),
                            (Appearance::Dark, "Dark"),
                            (Appearance::Light, "Light"),
                        ] {
                            changed |= ui
                                .radio_value(&mut self.config.appearance, a, name)
                                .changed();
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Mouse wheel over the image").strong());
                    ui.horizontal(|ui| {
                        for (w, name) in [
                            (MouseWheel::Zoom, "Zoom (Shift+wheel scrolls)"),
                            (
                                MouseWheel::Scroll,
                                "Scroll (Ctrl+wheel zooms, as paint.net)",
                            ),
                        ] {
                            changed |= ui
                                .radio_value(&mut self.config.mouse_wheel, w, name)
                                .changed();
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Menus").strong());
                    changed |= ui
                        .checkbox(
                            &mut self.config.global_menu,
                            "Show the menus in the desktop's global menu (KDE Plasma)",
                        )
                        .changed();
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Window").strong());
                    changed |= ui
                        .checkbox(
                            &mut self.config.system_title_bar,
                            "Use the desktop's title bar (instead of paintnotnet's window buttons)",
                        )
                        .changed();
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Crash recovery").strong());
                    ui.horizontal(|ui| {
                        ui.label("Autosave every");
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut self.config.autosave_minutes)
                                    .range(0..=60),
                            )
                            .changed();
                        ui.label("minutes (0 = off)");
                    });
                    ui.add_space(8.0);
                    if let Some(p) = &path {
                        ui.label(egui::RichText::new(format!("Saved to {}", p.display())).weak());
                    }
                } else {
                    changed |= self.keyboard_tab_ui(ui, &mut st);
                }
            },
        );
        if changed {
            self.config_changed(ctx);
        }
        if open {
            self.settings_open = Some(st);
        }
    }

    /// The AI models tab: where they're kept, what runs them, each file with its size and a
    /// Delete button (asks first).
    fn models_tab_ui(&mut self, ui: &mut egui::Ui, st: &mut SettingsState) {
        let dir = self.ai.models_dir().to_owned();
        ui.label(
            "Remove Background and Select Subject run these models on this computer. They are \
             downloaded when first needed, after asking.",
        );
        ui.add_space(4.0);
        ui.label(egui::RichText::new(format!("Kept in {}", crate::ai::tilde(&dir))).weak());
        ui.label(
            egui::RichText::new(match pnn_ai::runtime() {
                Ok(rt) => format!("Runs on the {}", rt.device.label()),
                Err(e) => e.to_string(),
            })
            .weak(),
        );
        ui.add_space(8.0);
        let files = crate::ai::model_files(&dir);
        let mut delete = None;
        egui::Grid::new("model_files")
            .num_columns(3)
            .spacing(egui::vec2(16.0, 8.0))
            .show(ui, |ui| {
                for f in &files {
                    ui.label(&f.label)
                        .on_hover_text(f.path.display().to_string());
                    ui.label(match f.size {
                        Some(s) => crate::ai::megabytes(s),
                        None => "not downloaded".to_owned(),
                    });
                    if f.size.is_some() {
                        if st.confirm_delete.as_ref() == Some(&f.path) {
                            ui.horizontal(|ui| {
                                ui.label("Delete?");
                                if ui.button("Yes").clicked() {
                                    delete = Some(f.path.clone());
                                }
                                if ui.button("No").clicked() {
                                    st.confirm_delete = None;
                                }
                            });
                        } else if ui.button("Delete").clicked() {
                            st.confirm_delete = Some(f.path.clone());
                        }
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
        let total: u64 = files.iter().filter_map(|f| f.size).sum();
        ui.add_space(8.0);
        ui.label(egui::RichText::new(format!("In all: {}", crate::ai::megabytes(total))).weak());
        if let Some(path) = delete {
            st.confirm_delete = None;
            // The helper may have it open; end it first (Linux would keep the data until then).
            self.ai.unload();
            if let Err(e) = std::fs::remove_file(&path) {
                self.errors.push((
                    "AI models".into(),
                    format!("Couldn't delete {}: {e}", path.display()),
                ));
            }
        }
    }

    /// Bind `chord` to `c`, taking it away from any command that had it. Returns a note about
    /// what was replaced.
    fn bind(&mut self, c: Command, chord: Chord) -> Option<String> {
        let clash = crate::commands::bindings()
            .into_iter()
            .find(|b| {
                b.command != c
                    && b.chord.key == chord.key
                    && b.chord.ctrl == chord.ctrl
                    && b.chord.alt == chord.alt
                    && b.chord.shift.is_none_or(|s| Some(s) == chord.shift)
            })
            .map(|b| b.command);
        let note = clash.map(|other| {
            // Keep the other command's remaining shortcuts.
            let rest: Vec<Chord> = crate::commands::bindings()
                .into_iter()
                .filter(|b| b.command == other && b.chord.key != chord.key)
                .map(|b| b.chord)
                .collect();
            self.config.set_override(other, &rest);
            format!(
                "{} was used by “{}”, which no longer has it.",
                chord.text(),
                command_label(other)
            )
        });
        self.config.set_override(c, &[chord]);
        note
    }

    fn keyboard_tab_ui(&mut self, ui: &mut egui::Ui, st: &mut SettingsState) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Search:");
            let edit = ui.add(egui::TextEdit::singleline(&mut st.search).desired_width(220.0));
            // Type to filter as soon as the tab opens.
            if !st.search_focused {
                edit.request_focus();
                st.search_focused = true;
            }
            if ui.button("Reset all").clicked() {
                self.config.shortcuts.clear();
                st.message = Some("All shortcuts are back to paint.net's.".into());
                changed = true;
            }
        });
        if let Some(c) = st.capturing {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!(
                    "Press the new shortcut for “{}” (Esc cancels)…",
                    command_label(c)
                ),
            );
        } else if let Some(m) = &st.message {
            ui.label(m);
        }
        ui.separator();
        let words = st.search.to_lowercase();
        let bindings = crate::commands::bindings();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("shortcuts")
                .striped(true)
                .num_columns(3)
                .show(ui, |ui| {
                    for c in all_commands() {
                        let label = command_label(c);
                        let keys: Vec<String> = bindings
                            .iter()
                            .filter(|b| b.command == c)
                            .map(|b| b.chord.text())
                            .collect();
                        let keys = keys.join(", ");
                        if !words.is_empty()
                            && !label.to_lowercase().contains(&words)
                            && !keys.to_lowercase().contains(&words)
                        {
                            continue;
                        }
                        ui.label(&label);
                        let overridden = self.config.shortcuts.contains_key(&c.id());
                        let text = if keys.is_empty() {
                            "—".to_owned()
                        } else {
                            keys
                        };
                        ui.label(if overridden {
                            egui::RichText::new(text).strong()
                        } else {
                            egui::RichText::new(text)
                        });
                        ui.horizontal(|ui| {
                            if ui
                                .small_button("Change")
                                .on_hover_text("Press a new key combination")
                                .clicked()
                            {
                                st.capturing = Some(c);
                            }
                            if ui.small_button("Clear").clicked() {
                                self.config.set_override(c, &[]);
                                changed = true;
                            }
                            if overridden && ui.small_button("Reset").clicked() {
                                self.config.reset_override(c);
                                changed = true;
                            }
                        });
                        ui.end_row();
                    }
                });
        });
        changed
    }

    /// Ctrl+K: search box over every command; Enter runs the highlighted one.
    pub(crate) fn run_palette(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.palette.take() else {
            return;
        };
        let state = self.command_state();
        let matches: Vec<(Command, String)> = palette_matches(&p.query)
            .into_iter()
            .filter(|(c, _)| state.enabled(*c))
            .collect();
        let (up, down, enter, esc) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if down {
            p.selected = (p.selected + 1).min(matches.len().saturating_sub(1));
        }
        if up {
            p.selected = p.selected.saturating_sub(1);
        }
        p.selected = p.selected.min(matches.len().saturating_sub(1));
        let mut run = None;
        crate::glass::window(
            ctx,
            egui::Window::new("Command palette")
                .id(egui::Id::new("command_palette"))
                .title_bar(false)
                .collapsible(false)
                .resizable(false)
                .fixed_size(egui::vec2(460.0, 0.0))
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0)),
            |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut p.query)
                        .hint_text("Type a command…")
                        .desired_width(f32::INFINITY),
                );
                if !p.focused {
                    edit.request_focus();
                    p.focused = true;
                }
                if edit.changed() {
                    p.selected = 0;
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for (i, (c, label)) in matches.iter().enumerate().take(200) {
                            let keys = c.shortcut_text().unwrap_or_default();
                            let r = ui.add(
                                egui::Button::new(egui::RichText::new(label))
                                    .selected(i == p.selected)
                                    .right_text(keys)
                                    .min_size(egui::vec2(ui.available_width(), 0.0)),
                            );
                            if i == p.selected && (up || down) {
                                r.scroll_to_me(None);
                            }
                            if r.clicked() {
                                run = Some(*c);
                            }
                        }
                        if matches.is_empty() {
                            ui.label(egui::RichText::new("No matching command").weak());
                        }
                    });
            },
        );
        if enter && let Some((c, _)) = matches.get(p.selected) {
            run = Some(*c);
        }
        if let Some(c) = run {
            self.execute(ctx, Triggered::once(c));
        } else if !esc {
            self.palette = Some(p);
        }
    }
}
