//! paint.net's menu bar: the full menu tree (unimplemented items visible but disabled),
//! icons, shortcut text, `&` mnemonics, Alt+letter to open, arrows to navigate.
//!
//! Labels and mnemonics follow paint.net 3.36 `Strings.resx`, extended with the 5.x items
//! from the 5.x docs.

use std::borrow::Cow;

use eframe::egui::{self, Color32, FontId, Key, Popup, text::LayoutJob};
use pnn_effects::{Adjustment, Category, effects_in};

use crate::commands::{Command, MenuId, Units};
use crate::icons::{Icon, Icons};

pub enum Entry {
    Item {
        label: Cow<'static, str>,
        command: Command,
        icon: Option<Icon>,
    },
    /// A toggle item shown with a check mark.
    Check {
        label: &'static str,
        command: Command,
    },
    Separator,
    Sub {
        label: String,
        entries: Vec<Entry>,
    },
    /// File > Open Recent, filled from `MenuContext::recent` when shown.
    Recent,
}

fn item(label: &'static str, command: Command, icon: Icon) -> Entry {
    Entry::Item {
        label: Cow::Borrowed(label),
        command,
        icon: Some(icon),
    }
}

fn plain(label: &'static str, command: Command) -> Entry {
    Entry::Item {
        label: Cow::Borrowed(label),
        command,
        icon: None,
    }
}

const SEP: Entry = Entry::Separator;

pub struct Menu {
    pub id: MenuId,
    pub title: &'static str,
    pub entries: Vec<Entry>,
}

/// The menus in menu-bar order. Help lives behind the "?" icon (Alt+H), as in paint.net 5.
pub fn menus() -> Vec<Menu> {
    use Command as C;
    vec![
        Menu {
            id: MenuId::File,
            title: "&File",
            entries: vec![
                item("&New...", C::NewImage, Icon::New),
                item("&Open...", C::Open, Icon::Open),
                Entry::Recent,
                Entry::Sub {
                    label: "Ac&quire".into(),
                    entries: vec![item(
                        "From &Scanner or Camera...",
                        C::Acquire,
                        Icon::Acquire,
                    )],
                },
                SEP,
                item("&Close", C::Close, Icon::Close),
                SEP,
                item("&Save", C::Save, Icon::Save),
                item("Save &As...", C::SaveAs, Icon::SaveAs),
                item("Save A&ll", C::SaveAll, Icon::SaveAll),
                SEP,
                item("&Print...", C::Print, Icon::Print),
                SEP,
                item("E&xit", C::Exit, Icon::Exit),
            ],
        },
        Menu {
            id: MenuId::Edit,
            title: "&Edit",
            entries: vec![
                item("&Undo", C::Undo, Icon::Undo),
                item("&Redo", C::Redo, Icon::Redo),
                SEP,
                item("Cu&t", C::Cut, Icon::Cut),
                item("&Copy", C::Copy, Icon::Copy),
                item("Copy Mer&ged", C::CopyMerged, Icon::CopyMerged),
                item("&Paste", C::Paste, Icon::Paste),
                item(
                    "Paste into New &Layer",
                    C::PasteIntoNewLayer,
                    Icon::PasteNewLayer,
                ),
                item(
                    "Paste into New I&mage",
                    C::PasteIntoNewImage,
                    Icon::PasteNewImage,
                ),
                SEP,
                item("Copy Selectio&n", C::CopySelection, Icon::CopySelection),
                item(
                    "Paste Selection (Replace)",
                    C::PasteSelection,
                    Icon::PasteSelection,
                ),
                SEP,
                item("&Erase Selection", C::EraseSelection, Icon::EraseSelection),
                item("&Fill Selection", C::FillSelection, Icon::FillSelection),
                item(
                    "&Invert Selection",
                    C::InvertSelection,
                    Icon::InvertSelection,
                ),
                item("Select &All", C::SelectAll, Icon::SelectAll),
                item("Select Su&bject", C::SelectSubject, Icon::SelectSubject),
                item("&Deselect", C::Deselect, Icon::Deselect),
                SEP,
                item(
                    "Feat&her Selection...",
                    C::FeatherSelection,
                    Icon::FeatherSelection,
                ),
            ],
        },
        Menu {
            id: MenuId::View,
            title: "&View",
            entries: vec![
                item("Zoom &In", C::ZoomIn, Icon::ZoomIn),
                item("Zoom &Out", C::ZoomOut, Icon::ZoomOut),
                item("Zoom to &Window", C::ZoomToWindow, Icon::ZoomToWindow),
                item(
                    "Zoom to &Selection",
                    C::ZoomToSelection,
                    Icon::ZoomToSelection,
                ),
                item("&Actual Size", C::ActualSize, Icon::ActualSize),
                SEP,
                Entry::Check {
                    label: "Pixel &Grid",
                    command: C::TogglePixelGrid,
                },
                Entry::Check {
                    label: "&Rulers",
                    command: C::ToggleRulers,
                },
                SEP,
                Entry::Sub {
                    label: "&Units".into(),
                    entries: vec![
                        Entry::Check {
                            label: "&Pixels",
                            command: C::SetUnits(Units::Pixels),
                        },
                        Entry::Check {
                            label: "&Inches",
                            command: C::SetUnits(Units::Inches),
                        },
                        Entry::Check {
                            label: "&Centimeters",
                            command: C::SetUnits(Units::Centimeters),
                        },
                    ],
                },
            ],
        },
        Menu {
            id: MenuId::Image,
            title: "&Image",
            entries: vec![
                item("Cro&p to Selection", C::CropToSelection, Icon::Crop),
                item("&Resize...", C::Resize, Icon::Resize),
                item("Canvas &Size...", C::CanvasSize, Icon::CanvasSize),
                SEP,
                item(
                    "Flip &Horizontal",
                    C::FlipImageHorizontal,
                    Icon::FlipImageHorizontal,
                ),
                item(
                    "Flip &Vertical",
                    C::FlipImageVertical,
                    Icon::FlipImageVertical,
                ),
                SEP,
                item(
                    "Rotate &90° Clockwise",
                    C::RotateImage90Clockwise,
                    Icon::Rotate90Cw,
                ),
                item(
                    "Rotate 90° &Counter-Clockwise",
                    C::RotateImage90CounterClockwise,
                    Icon::Rotate90Ccw,
                ),
                item("Rotate &180°", C::RotateImage180, Icon::Rotate180),
                SEP,
                plain("C&olor Profile...", C::ColorProfile),
                SEP,
                item("&Flatten", C::Flatten, Icon::Flatten),
            ],
        },
        Menu {
            id: MenuId::Layers,
            title: "&Layers",
            entries: vec![
                item("&Add New Layer", C::AddNewLayer, Icon::AddLayer),
                item("De&lete Layer", C::DeleteLayer, Icon::DeleteLayer),
                item("&Duplicate Layer", C::DuplicateLayer, Icon::DuplicateLayer),
                item("&Merge Layer Down", C::MergeLayerDown, Icon::MergeLayerDown),
                item(
                    "&Toggle Layer Visibility",
                    C::ToggleLayerVisibility,
                    Icon::ToggleLayerVisibility,
                ),
                SEP,
                item(
                    "&Import From File...",
                    C::ImportFromFile,
                    Icon::ImportFromFile,
                ),
                SEP,
                item(
                    "Flip &Horizontal",
                    C::FlipLayerHorizontal,
                    Icon::FlipLayerHorizontal,
                ),
                item(
                    "Flip &Vertical",
                    C::FlipLayerVertical,
                    Icon::FlipLayerVertical,
                ),
                item("Rotate 1&80°", C::RotateLayer180, Icon::Rotate180),
                item("&Rotate / Zoom...", C::RotateZoom, Icon::RotateZoom),
                SEP,
                item("Go to Top Layer", C::GoToTopLayer, Icon::GoToTopLayer),
                item("Go to Layer Above", C::GoToLayerAbove, Icon::GoToLayerAbove),
                item("Go to Layer Below", C::GoToLayerBelow, Icon::GoToLayerBelow),
                item(
                    "Go to Bottom Layer",
                    C::GoToBottomLayer,
                    Icon::GoToBottomLayer,
                ),
                SEP,
                item("Move Layer to Top", C::MoveLayerToTop, Icon::MoveLayerToTop),
                item("Move Layer &Up", C::MoveLayerUp, Icon::MoveLayerUp),
                item("Move Layer Dow&n", C::MoveLayerDown, Icon::MoveLayerDown),
                item(
                    "Move Layer to Bottom",
                    C::MoveLayerToBottom,
                    Icon::MoveLayerToBottom,
                ),
                SEP,
                item(
                    "Layer &Properties...",
                    C::LayerProperties,
                    Icon::LayerProperties,
                ),
            ],
        },
        Menu {
            id: MenuId::Adjustments,
            title: "&Adjustments",
            entries: adjustment_entries(),
        },
        Menu {
            id: MenuId::Effects,
            title: "Effe&cts",
            entries: effect_entries(),
        },
        Menu {
            id: MenuId::Help,
            title: "&Help",
            entries: vec![
                item("&Help Topics", C::HelpTopics, Icon::Help),
                SEP,
                item("&About paintnotnet", C::About, Icon::About),
            ],
        },
    ]
}

fn adjustment_entries() -> Vec<Entry> {
    let mnemonics = [
        'A', 'B', 'r', 'C', 'x', 'g', 'H', 'v', 'I', 'L', 'P', 'S', 'T',
    ];
    let label = |name: &str, m: Option<char>, dialog: bool| {
        let mut label = name.to_owned();
        if let Some(i) = m.and_then(|m| label.find(m)) {
            label.insert(i, '&');
        }
        if dialog {
            label.push_str("...");
        }
        &*Box::leak(label.into_boxed_str())
    };
    let mut items: Vec<(&str, Entry)> = Adjustment::ALL
        .iter()
        .zip(mnemonics)
        .map(|(&a, m)| {
            (
                a.name(),
                Entry::Item {
                    label: Cow::Borrowed(label(a.name(), Some(m), a.has_dialog())),
                    command: Command::Adjustment(a),
                    icon: adjustment_icon(a),
                },
            )
        })
        .collect();
    // The extra adjustments sit among the stock ones, in alphabetical order like them.
    for (id, e) in effects_in(Category::Adjustment) {
        let m = match e.name {
            "Color Balance" => Some('n'),
            "Transparency" => Some('y'),
            _ => None,
        };
        items.push((
            e.name,
            Entry::Item {
                label: Cow::Borrowed(label(e.name, m, true)),
                command: Command::Effect(id),
                icon: None,
            },
        ));
    }
    items.sort_by_key(|(name, _)| name.to_lowercase());
    items.into_iter().map(|(_, e)| e).collect()
}

fn effect_entries() -> Vec<Entry> {
    let mut entries = vec![plain("Repeat", Command::RepeatEffect), SEP];
    for category in Category::ALL {
        let items = effects_in(category)
            .map(|(id, e)| Entry::Item {
                label: Cow::Owned(format!("{}...", e.name)),
                command: Command::Effect(id),
                icon: None,
            })
            .collect();
        entries.push(Entry::Sub {
            label: category.name().to_owned(),
            entries: items,
        });
    }
    entries
}

/// Split "&File" into ("File", Some(index of 'F')).
pub fn parse_mnemonic(label: &str) -> (String, Option<usize>) {
    match label.find('&') {
        Some(i) => {
            let mut s = label.to_owned();
            s.remove(i);
            (s, Some(i))
        }
        None => (label.to_owned(), None),
    }
}

pub fn mnemonic_char(label: &str) -> Option<char> {
    let (text, i) = parse_mnemonic(label);
    i.and_then(|i| text[i..].chars().next())
        .map(|c| c.to_ascii_lowercase())
}

fn label_job(label: &str, underline: bool, color: Color32) -> LayoutJob {
    let (text, i) = parse_mnemonic(label);
    let font = FontId::proportional(crate::theme::text::BODY);
    let mut job = LayoutJob::default();
    let normal = egui::TextFormat {
        font_id: font.clone(),
        color,
        ..Default::default()
    };
    match (i, underline) {
        (Some(i), true) => {
            let end = i + text[i..].chars().next().map_or(0, char::len_utf8);
            job.append(&text[..i], 0.0, normal.clone());
            job.append(
                &text[i..end],
                0.0,
                egui::TextFormat {
                    underline: egui::Stroke::new(1.0, color),
                    ..normal.clone()
                },
            );
            job.append(&text[end..], 0.0, normal);
        }
        _ => job.append(&text, 0.0, normal),
    }
    job
}

/// What the menus need to know about app state.
pub struct MenuContext<'a> {
    pub enabled: &'a dyn Fn(Command) -> bool,
    pub checked: &'a dyn Fn(Command) -> bool,
    /// Shown as "Repeat <name>" in the Effects menu.
    pub last_effect: Option<&'a str>,
    /// File > Open Recent, newest first.
    pub recent: &'a [std::path::PathBuf],
}

/// File > Open Recent as paint.net 3.36 fills it (`FileMenu.cs`): "&1 name" … newest first,
/// then "&Clear this list"; "(none)" (disabled) when empty. Labels from its `Strings.resx`.
pub fn recent_menu(recent: &[std::path::PathBuf]) -> Entry {
    let mut entries: Vec<Entry> = recent
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().replace('&', "&&"))
                .unwrap_or_default();
            let mnemonic = if i < 9 { "&" } else { "" };
            Entry::Item {
                label: Cow::Owned(format!("{mnemonic}{} {name}", i + 1)),
                command: Command::OpenRecent(i as u8),
                icon: None,
            }
        })
        .collect();
    if entries.is_empty() {
        // Disabled: Clear is only available with something to clear.
        entries.push(plain("(none)", Command::ClearRecentFiles));
    } else {
        entries.push(SEP);
        entries.push(item(
            "&Clear this list",
            Command::ClearRecentFiles,
            Icon::EraseSelection,
        ));
    }
    Entry::Sub {
        label: "Open &Recent".into(),
        entries,
    }
}

#[derive(Default)]
pub struct MenuBarState {
    /// Open this menu on the next frame (Alt+letter, arrow keys).
    pub open_request: Option<MenuId>,
    /// Opened from the keyboard: show mnemonics and focus the first item.
    keyboard: bool,
    focus_first: bool,
    /// A menu popup was open during the last frame (global shortcuts pause meanwhile).
    pub open: bool,
    /// Frames to keep keyboard mode while a requested menu is still opening.
    grace: u8,
    /// Which top-level menu is open this frame, and which was last frame (hovering another
    /// title while one is open switches to it, as in Windows menu bars).
    open_menu: Option<MenuId>,
    was_open: Option<MenuId>,
}

pub fn request_open(state: &mut MenuBarState, id: MenuId) {
    state.open_request = Some(id);
    state.keyboard = true;
    state.focus_first = true;
    // The popup appears a frame after it is requested.
    state.grace = 2;
}

/// Draw one top-level menu button (used for the menu bar and the Help "?" icon).
/// Returns the chosen command.
pub fn menu_button(
    ui: &mut egui::Ui,
    menu: &Menu,
    button: egui::Button<'_>,
    state: &mut MenuBarState,
    cx: &MenuContext,
    icons: &Icons,
    all: &[MenuId],
) -> Option<Command> {
    let mut picked = None;
    let (response, inner) = egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
        ui.set_min_width(220.0);
        state.open = true;
        state.open_menu = Some(menu.id);
        let underline = state.keyboard || ui.input(|i| i.modifiers.alt);
        picked = entries_ui(
            ui,
            &menu.entries,
            cx,
            icons,
            underline,
            &mut state.focus_first,
        );
        // Keyboard: mnemonic letters, Left/Right to neighbouring menus.
        let (left, right) = ui.input(|i| {
            (
                i.key_pressed(Key::ArrowLeft),
                i.key_pressed(Key::ArrowRight),
            )
        });
        let pos = all.iter().position(|&m| m == menu.id).unwrap_or(0);
        if left || right {
            let next = if right {
                (pos + 1) % all.len()
            } else {
                (pos + all.len() - 1) % all.len()
            };
            ui.close();
            request_open(state, all[next]);
        }
        if picked.is_some() {
            ui.close();
        }
    });
    if state.open_request == Some(menu.id) {
        Popup::open_id(ui.ctx(), Popup::default_response_id(&response));
        state.open_request = None;
    } else if inner.is_none() && state.was_open.is_some_and(|m| m != menu.id) && response.hovered()
    {
        Popup::open_id(ui.ctx(), Popup::default_response_id(&response));
        state.was_open = Some(menu.id);
    }
    if inner.is_none() && response.clicked() {
        state.keyboard = false;
    }
    picked
}

fn entries_ui(
    ui: &mut egui::Ui,
    entries: &[Entry],
    cx: &MenuContext,
    icons: &Icons,
    underline: bool,
    focus_first: &mut bool,
) -> Option<Command> {
    let mut picked = None;
    let text_color = ui.visuals().text_color();
    let typed: Vec<char> = ui.input(|i| {
        i.events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if modifiers.is_none() || *modifiers == egui::Modifiers::ALT => key
                    .name()
                    .chars()
                    .next()
                    .filter(|_| key.name().len() == 1)
                    .map(|c| c.to_ascii_lowercase()),
                _ => None,
            })
            .collect()
    });
    if ui.input(|i| i.key_pressed(Key::ArrowDown)) {
        ui.memory_mut(|m| m.move_focus(egui::FocusDirection::Down));
    }
    if ui.input(|i| i.key_pressed(Key::ArrowUp)) {
        ui.memory_mut(|m| m.move_focus(egui::FocusDirection::Up));
    }
    for entry in entries {
        match entry {
            Entry::Separator => {
                ui.separator();
            }
            Entry::Item {
                label,
                command,
                icon,
            } => {
                let enabled = (cx.enabled)(*command);
                let shown = if *command == Command::RepeatEffect {
                    match cx.last_effect {
                        Some(name) => format!("&Repeat {name}"),
                        None => "&Repeat".to_owned(),
                    }
                } else {
                    label.to_string()
                };
                let job = label_job(&shown, underline, text_color);
                let mut button = match icon {
                    Some(icon) => egui::Button::image_and_text(icons.image(*icon), job),
                    // Keep labels aligned with items that have icons.
                    None => egui::Button::image_and_text(icons.blank(), job),
                };
                if let Some(text) = command.shortcut_text() {
                    button = button.shortcut_text(text);
                }
                let response = ui.add_enabled(enabled, button);
                if *focus_first && enabled {
                    response.request_focus();
                    *focus_first = false;
                }
                let mnemonic = mnemonic_char(&shown);
                if response.clicked() || (enabled && mnemonic.is_some_and(|m| typed.contains(&m))) {
                    picked = Some(*command);
                }
            }
            Entry::Check { label, command } => {
                let enabled = (cx.enabled)(*command);
                let mut on = (cx.checked)(*command);
                let job = label_job(label, underline, text_color);
                // Nearly square, as `controls::checkbox` (the controls' radius drew a circle).
                let response = ui
                    .scope(|ui| {
                        let w = &mut ui.visuals_mut().widgets;
                        for v in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                            v.corner_radius = egui::CornerRadius::same(3);
                        }
                        ui.add_enabled(enabled, egui::Checkbox::new(&mut on, job))
                    })
                    .inner;
                let mnemonic = mnemonic_char(label);
                if response.clicked() || (enabled && mnemonic.is_some_and(|m| typed.contains(&m))) {
                    picked = Some(*command);
                }
            }
            Entry::Recent => {
                let recent = [recent_menu(cx.recent)];
                if let Some(c) = entries_ui(ui, &recent, cx, icons, underline, focus_first) {
                    picked = Some(c);
                }
            }
            Entry::Sub { label, entries } => {
                let job = label_job(label, underline, text_color);
                let (_, inner) = egui::containers::menu::SubMenuButton::new(job).ui(ui, |ui| {
                    ui.set_min_width(200.0);
                    let mut no_focus = false;
                    entries_ui(ui, entries, cx, icons, underline, &mut no_focus)
                });
                if let Some(Some(c)) = inner.map(|r| r.inner) {
                    picked = Some(c);
                }
            }
        }
    }
    picked
}

/// The Help menu, behind paint.net 5's "?" icon.
pub fn help_button(
    ui: &mut egui::Ui,
    state: &mut MenuBarState,
    cx: &MenuContext,
    icons: &Icons,
) -> Option<Command> {
    let all_menus = menus();
    let order: Vec<MenuId> = all_menus.iter().map(|m| m.id).collect();
    let help = all_menus
        .iter()
        .find(|m| m.id == MenuId::Help)
        .expect("help menu");
    let button = egui::Button::image(icons.image(Icon::Help)).frame_when_inactive(false);
    menu_button(ui, help, button, state, cx, icons, &order)
}

/// Natural width of the menu bar (egui's `MenuBar` otherwise takes all the width it gets).
pub fn menu_bar_width(ui: &egui::Ui) -> f32 {
    // The bar's buttons use egui's menu style (its own, smaller button padding).
    let mut style = (**ui.style()).clone();
    egui::containers::menu::menu_style(&mut style);
    let pad = style.spacing.button_padding.x * 2.0 + style.spacing.item_spacing.x;
    menus()
        .iter()
        .filter(|m| m.id != MenuId::Help)
        .map(|m| {
            let job = label_job(m.title, false, ui.visuals().text_color());
            ui.painter().layout_job(job).size().x + pad
        })
        .sum()
}

/// Draw the menu bar (File … Effects). Returns the chosen command.
pub fn menu_bar(
    ui: &mut egui::Ui,
    state: &mut MenuBarState,
    cx: &MenuContext,
    icons: &Icons,
) -> Option<Command> {
    state.open = false;
    state.was_open = state.open_menu.take();
    let all_menus = menus();
    let order: Vec<MenuId> = all_menus.iter().map(|m| m.id).collect();
    let alt = ui.input(|i| i.modifiers.alt);
    let mut picked = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in all_menus.iter().filter(|m| m.id != MenuId::Help) {
            let title = label_job(menu.title, alt || state.keyboard, ui.visuals().text_color());
            let button = egui::Button::new(title).frame_when_inactive(false);
            if let Some(c) = menu_button(ui, menu, button, state, cx, icons, &order) {
                picked = Some(c);
            }
        }
    });
    if state.open {
        state.grace = 0;
    } else if state.open_request.is_none() {
        if state.grace > 0 {
            state.grace -= 1;
        } else {
            state.keyboard = false;
        }
    }
    picked
}

/// The 3.36 menu icon of an adjustment (5.x-only ones have none).
pub fn adjustment_icon(a: Adjustment) -> Option<Icon> {
    match a {
        Adjustment::AutoLevel => Some(Icon::AutoLevel),
        Adjustment::BlackAndWhite => Some(Icon::BlackAndWhite),
        Adjustment::BrightnessContrast => Some(Icon::BrightnessContrast),
        Adjustment::Curves => Some(Icon::Curves),
        Adjustment::HueSaturation => Some(Icon::HueSaturation),
        Adjustment::InvertColors => Some(Icon::InvertColors),
        Adjustment::Levels => Some(Icon::Levels),
        Adjustment::Posterize => Some(Icon::Posterize),
        Adjustment::Sepia => Some(Icon::Sepia),
        Adjustment::Exposure => Some(Icon::Exposure),
        Adjustment::HighlightsShadows => Some(Icon::HighlightsShadows),
        Adjustment::TemperatureTint => Some(Icon::TemperatureTint),
        _ => None,
    }
}

/// Every command reachable from the menus or the keymap (the Settings shortcut list, the
/// command palette and the settings file all use it).
pub fn all_commands() -> Vec<Command> {
    fn walk(entries: &[Entry], out: &mut Vec<Command>) {
        for e in entries {
            match e {
                Entry::Item { command, .. } | Entry::Check { command, .. } => {
                    if !out.contains(command) {
                        out.push(*command);
                    }
                }
                Entry::Sub { entries, .. } => walk(entries, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for m in menus() {
        walk(&m.entries, &mut out);
    }
    for b in crate::commands::BINDINGS {
        if !out.contains(&b.command) {
            out.push(b.command);
        }
    }
    out
}

/// A command's menu label ("Gaussian Blur", "Save As"), or a readable name for commands
/// that only have a shortcut.
pub fn command_label(c: Command) -> String {
    fn find(entries: &[Entry], c: Command, path: &str) -> Option<String> {
        for e in entries {
            match e {
                Entry::Item { label, command, .. } if *command == c => {
                    let l = parse_mnemonic(label).0.trim_end_matches("...").to_owned();
                    return Some(if path.is_empty() {
                        l
                    } else {
                        format!("{path} › {l}")
                    });
                }
                Entry::Check { label, command, .. } if *command == c => {
                    let l = parse_mnemonic(label).0.trim_end_matches("...").to_owned();
                    return Some(if path.is_empty() {
                        l
                    } else {
                        format!("{path} › {l}")
                    });
                }
                Entry::Sub { label, entries } => {
                    let p = parse_mnemonic(label).0;
                    if let Some(found) = find(entries, c, &p) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    for m in menus() {
        if let Some(l) = find(&m.entries, c, "") {
            return format!("{} › {l}", parse_mnemonic(m.title).0);
        }
    }
    // Shortcut-only commands: a readable form of their id.
    let id = c.id();
    let mut out = String::new();
    for (i, ch) in id.char_indices() {
        if ch.is_uppercase() && i > 0 && !id[..i].ends_with('(') {
            out.push(' ');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mnemonics_parse() {
        assert_eq!(
            parse_mnemonic("Save &As..."),
            ("Save As...".to_owned(), Some(5))
        );
        assert_eq!(mnemonic_char("Effe&cts"), Some('c'));
        assert_eq!(mnemonic_char("Exit"), None);
    }

    #[test]
    fn mnemonics_unique_within_each_menu() {
        fn check(title: &str, entries: &[Entry]) {
            let mut seen = std::collections::HashMap::new();
            for e in entries {
                let label = match e {
                    Entry::Item { label, .. } => label.to_string(),
                    Entry::Check { label, .. } => label.to_string(),
                    Entry::Sub { label, entries } => {
                        check(label, entries);
                        label.clone()
                    }
                    Entry::Recent => {
                        let recent: Vec<std::path::PathBuf> =
                            (0..8).map(|i| format!("/tmp/{i}.png").into()).collect();
                        let Entry::Sub { label, entries } = recent_menu(&recent) else {
                            unreachable!()
                        };
                        check(&label, &entries);
                        label
                    }
                    Entry::Separator => continue,
                };
                if let Some(m) = mnemonic_char(&label)
                    && let Some(prev) = seen.insert(m, label.clone())
                {
                    panic!("menu {title}: '{m}' used by both {prev:?} and {label:?}");
                }
            }
        }
        for m in menus() {
            check(m.title, &m.entries);
        }
    }

    #[test]
    fn top_level_mnemonics_match_documented_alt_keys() {
        let expected = [
            (MenuId::File, 'f'),
            (MenuId::Edit, 'e'),
            (MenuId::View, 'v'),
            (MenuId::Image, 'i'),
            (MenuId::Layers, 'l'),
            (MenuId::Adjustments, 'a'),
            (MenuId::Effects, 'c'),
            (MenuId::Help, 'h'),
        ];
        for m in menus() {
            let want = expected.iter().find(|(id, _)| *id == m.id).unwrap().1;
            assert_eq!(mnemonic_char(m.title), Some(want), "{}", m.title);
        }
    }

    #[test]
    fn file_menu_documented_sequences() {
        // "Alt+F, R" Open Recent and "Alt+F, Q" Acquire (KeyboardMouseCommands.html).
        let file = menus().into_iter().find(|m| m.id == MenuId::File).unwrap();
        let labels: Vec<String> = file
            .entries
            .iter()
            .filter_map(|e| match e {
                Entry::Item { label, .. } => Some(label.to_string()),
                Entry::Sub { label, .. } => Some(label.clone()),
                _ => None,
            })
            .collect();
        assert!(
            labels
                .iter()
                .any(|l| mnemonic_char(l) == Some('q') && parse_mnemonic(l).0.contains("Acquire"))
        );
    }
}
