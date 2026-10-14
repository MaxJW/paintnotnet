//! KDE Plasma's global menu: the menu bar exported over D-Bus, so the Global Menu applet
//! (or the title-bar menu button) shows File, Edit, View, … for this window.
//!
//! How it fits together (the same route Qt takes on Plasma Wayland):
//! - the menu tree is served as `com.canonical.dbusmenu` at `/MenuBar` on the session bus
//!   (protocol: libdbusmenu's `com.canonical.dbusmenu.xml`; KDE's client is
//!   libdbusmenuqt in plasma-workspace);
//! - the window is linked to that address with the `org_kde_kwin_appmenu` Wayland protocol
//!   (plasma-wayland-protocols `appmenu.xml`), see [`wayland`];
//! - like Qt, it is only used while `com.canonical.AppMenu.Registrar` is on the bus (Plasma
//!   runs it while a global menu is in use); otherwise the in-window menu bar stays.
//!
//! Only Plasma on Wayland is supported; on X11 or other desktops the window keeps its own
//! menu bar.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{self, Key};
use serde::{Deserialize, Serialize};
use zvariant::{OwnedValue, Type, Value};

use crate::commands::{Binding, Chord, Command};
use crate::icons::Icon;
use crate::menus::{Entry, Menu, MenuContext};

const PATH: &str = "/MenuBar";
const INTERFACE: &str = "com.canonical.dbusmenu";
const REGISTRAR: &str = "com.canonical.AppMenu.Registrar";

/// One node of the exported menu tree; ids are indices into the item list (0 = root).
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// dbusmenu label: `_` marks the mnemonic, `__` is a literal underscore.
    pub label: String,
    pub separator: bool,
    pub enabled: bool,
    /// Check-mark items: on or off.
    pub toggle: Option<bool>,
    /// Shortcut as dbusmenu key names, e.g. `["Control", "Shift", "S"]`.
    pub shortcut: Option<Vec<String>>,
    pub icon: Option<Icon>,
    pub command: Option<Command>,
    pub children: Vec<i32>,
    /// Shown as a submenu even when it has no children yet.
    pub submenu: bool,
}

impl Item {
    fn node(label: String) -> Self {
        Self {
            label,
            separator: false,
            enabled: true,
            toggle: None,
            shortcut: None,
            icon: None,
            command: None,
            children: Vec::new(),
            submenu: true,
        }
    }
}

/// Flatten the menu bar into dbusmenu items with the current enabled/checked state.
pub fn build(menus: &[Menu], cx: &MenuContext, bindings: &[Binding]) -> Vec<Item> {
    fn add(items: &mut Vec<Item>, item: Item) -> i32 {
        items.push(item);
        (items.len() - 1) as i32
    }
    fn entries(
        items: &mut Vec<Item>,
        parent: i32,
        list: &[Entry],
        cx: &MenuContext,
        bindings: &[Binding],
    ) {
        for e in list {
            let id = match e {
                Entry::Separator => add(
                    items,
                    Item {
                        separator: true,
                        submenu: false,
                        ..Item::node(String::new())
                    },
                ),
                Entry::Item {
                    label,
                    command,
                    icon,
                } => {
                    let shown = if *command == Command::RepeatEffect {
                        match cx.last_effect {
                            Some(name) => format!("&Repeat {name}"),
                            None => "&Repeat".to_owned(),
                        }
                    } else {
                        (*label).to_owned()
                    };
                    add(
                        items,
                        Item {
                            enabled: (cx.enabled)(*command),
                            shortcut: shortcut(bindings, *command),
                            icon: *icon,
                            command: Some(*command),
                            submenu: false,
                            ..Item::node(dbus_label(&shown))
                        },
                    )
                }
                Entry::Check { label, command } => add(
                    items,
                    Item {
                        enabled: (cx.enabled)(*command),
                        toggle: Some((cx.checked)(*command)),
                        shortcut: shortcut(bindings, *command),
                        command: Some(*command),
                        submenu: false,
                        ..Item::node(dbus_label(label))
                    },
                ),
                Entry::Sub {
                    label,
                    entries: sub,
                } => {
                    let id = add(items, Item::node(dbus_label(label)));
                    entries(items, id, sub, cx, bindings);
                    id
                }
            };
            items[parent as usize].children.push(id);
        }
    }
    let mut items = vec![Item::node(String::new())];
    for menu in menus {
        let id = add(&mut items, Item::node(dbus_label(menu.title)));
        items[0].children.push(id);
        entries(&mut items, id, &menu.entries, cx, bindings);
    }
    items
}

/// paint.net's `&File` → dbusmenu's `_File`.
pub fn dbus_label(label: &str) -> String {
    let mut out = String::with_capacity(label.len() + 1);
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '&' if chars.peek() == Some(&'&') => {
                chars.next();
                out.push('&');
            }
            '&' => out.push('_'),
            '_' => out.push_str("__"),
            c => out.push(c),
        }
    }
    out
}

fn shortcut(bindings: &[Binding], c: Command) -> Option<Vec<String>> {
    bindings
        .iter()
        .find(|b| b.command == c)
        .map(|b| shortcut_keys(&b.chord))
}

/// A chord as dbusmenu shortcut keys: modifiers "Control"/"Alt"/"Shift", then the key in
/// Qt's portable key-name form (Plasma's client joins these with `+` and parses them with
/// `QKeySequence::fromString`).
pub fn shortcut_keys(chord: &Chord) -> Vec<String> {
    let mut keys = Vec::new();
    if chord.ctrl {
        keys.push("Control".to_owned());
    }
    if chord.alt {
        keys.push("Alt".to_owned());
    }
    if chord.shift == Some(true) {
        keys.push("Shift".to_owned());
    }
    keys.push(
        match chord.key {
            Key::Escape => "Esc",
            Key::Insert => "Ins",
            Key::Delete => "Del",
            Key::PageUp => "PgUp",
            Key::PageDown => "PgDown",
            Key::Enter => "Return",
            Key::Comma => ",",
            Key::Minus => "-",
            Key::Period => ".",
            Key::Plus => "+",
            Key::Equals => "=",
            Key::Semicolon => ";",
            Key::Backslash => "\\",
            Key::Slash => "/",
            Key::OpenBracket => "[",
            Key::CloseBracket => "]",
            Key::Backtick => "`",
            Key::Quote => "'",
            k => k.name(),
        }
        .to_owned(),
    );
    keys
}

fn owned(v: impl Into<Value<'static>>) -> OwnedValue {
    // Only fails for file descriptors, which menus never carry.
    OwnedValue::try_from(v.into()).expect("plain value")
}

/// dbusmenu properties of one item, filtered to `names` (empty = all). Defaults (enabled,
/// visible, standard type) are left out, as the spec allows.
fn properties(item: &Item, names: &[String]) -> HashMap<String, OwnedValue> {
    let mut p = HashMap::new();
    let mut put = |k: &str, v: OwnedValue| {
        if names.is_empty() || names.iter().any(|n| n == k) {
            p.insert(k.to_owned(), v);
        }
    };
    if item.separator {
        put("type", owned("separator"));
        return p;
    }
    put("label", owned(item.label.clone()));
    if !item.enabled {
        put("enabled", owned(false));
    }
    if item.submenu {
        put("children-display", owned("submenu"));
    }
    if let Some(on) = item.toggle {
        put("toggle-type", owned("checkmark"));
        put("toggle-state", owned(i32::from(on)));
    }
    if let Some(keys) = &item.shortcut {
        put("shortcut", owned(vec![keys.clone()]));
    }
    if let Some(png) = item.icon.and_then(|i| i.png()) {
        put("icon-data", owned(png));
    }
    p
}

/// dbusmenu's layout node, `(ia{sv}av)`.
#[derive(Debug, Serialize, Deserialize, Type, Value, zvariant::OwnedValue)]
pub struct Layout {
    pub id: i32,
    pub properties: HashMap<String, OwnedValue>,
    pub children: Vec<OwnedValue>,
}

/// The layout below `id`, `depth` levels deep (-1 = all).
fn layout(items: &[Item], id: i32, depth: i32, names: &[String]) -> Layout {
    let item = &items[id as usize];
    let children = if depth == 0 {
        Vec::new()
    } else {
        item.children
            .iter()
            .map(|&c| {
                OwnedValue::try_from(layout(items, c, depth - 1, names)).expect("plain value")
            })
            .collect()
    };
    Layout {
        id,
        properties: properties(item, names),
        children,
    }
}

#[derive(Default)]
struct Model {
    revision: u32,
    items: Vec<Item>,
}

/// The `com.canonical.dbusmenu` object (runs on zbus's thread).
struct DbusMenu {
    model: Arc<Mutex<Model>>,
    picked: Sender<Command>,
    ctx: egui::Context,
}

impl DbusMenu {
    fn activate(&self, id: i32, event: &str) -> bool {
        log::debug!("global menu: {event} on item {id}");
        let model = self.model.lock().expect("menu model");
        let Some(item) = model.items.get(id as usize) else {
            return false;
        };
        if event == "clicked"
            && item.enabled
            && let Some(c) = item.command
        {
            let _ = self.picked.send(c);
            self.ctx.request_repaint();
        }
        true
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl DbusMenu {
    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        property_names: Vec<String>,
    ) -> zbus::fdo::Result<(u32, Layout)> {
        let model = self.model.lock().expect("menu model");
        if parent_id < 0 || parent_id as usize >= model.items.len() {
            return Err(zbus::fdo::Error::InvalidArgs(format!(
                "no menu item {parent_id}"
            )));
        }
        Ok((
            model.revision,
            layout(&model.items, parent_id, recursion_depth, &property_names),
        ))
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let model = self.model.lock().expect("menu model");
        let all: Vec<i32> = (0..model.items.len() as i32).collect();
        let ids = if ids.is_empty() { &all } else { &ids };
        ids.iter()
            .filter_map(|&id| {
                let item = model.items.get(usize::try_from(id).ok()?)?;
                Some((id, properties(item, &property_names)))
            })
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        let model = self.model.lock().expect("menu model");
        usize::try_from(id)
            .ok()
            .and_then(|i| model.items.get(i))
            .and_then(|item| properties(item, std::slice::from_ref(&name)).remove(&name))
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("{id}: no {name}")))
    }

    fn event(&self, id: i32, event_id: String, _data: OwnedValue, _timestamp: u32) {
        self.activate(id, &event_id);
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        events
            .into_iter()
            .filter(|(id, event, _, _)| !self.activate(*id, event))
            .map(|(id, ..)| id)
            .collect()
    }

    /// The layout is kept current, so no menu ever needs refreshing before it opens.
    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> String {
        "ltr".to_owned()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        "normal".to_owned()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The exported menu and its link to the window.
pub struct GlobalMenu {
    conn: zbus::blocking::Connection,
    model: Arc<Mutex<Model>>,
    picked: Receiver<Command>,
    /// Whether Plasma's menu registrar is running (checked on a background thread).
    registrar: Arc<AtomicBool>,
    linked: bool,
    last_build: Option<Instant>,
}

impl GlobalMenu {
    /// Export the menu and get ready to link it to the window. `None` (with a log line)
    /// unless the window's Wayland compositor has KDE's appmenu protocol (`link`) and a
    /// session bus is reachable.
    pub fn new(
        cc: &eframe::CreationContext,
        link: Option<&crate::wayland::WindowLink>,
    ) -> Option<Self> {
        if !link.is_some_and(|l| l.has_appmenu()) {
            log::info!("global menu: no Wayland appmenu protocol, keeping the menu bar");
            return None;
        }
        let model = Arc::new(Mutex::new(Model::default()));
        let (tx, rx) = channel();
        let server = DbusMenu {
            model: model.clone(),
            picked: tx,
            ctx: cc.egui_ctx.clone(),
        };
        let conn = zbus::blocking::connection::Builder::session()
            .and_then(|b| b.serve_at(PATH, server))
            .and_then(|b| b.build())
            .map_err(|e| log::warn!("global menu: session bus: {e}"))
            .ok()?;
        let registrar = Arc::new(AtomicBool::new(false));
        watch_registrar(conn.clone(), registrar.clone(), cc.egui_ctx.clone());
        Some(Self {
            conn,
            model,
            picked: rx,
            registrar,
            linked: false,
            last_build: None,
        })
    }

    /// Whether the desktop is showing the menu (so the in-window bar should hide).
    pub fn active(&self) -> bool {
        self.linked
    }

    /// Call every frame: links or unlinks the window, refreshes the exported state, and
    /// returns the commands picked from the desktop's menu.
    pub fn update(
        &mut self,
        link: &mut crate::wayland::WindowLink,
        wanted: bool,
        menus: &[Menu],
        cx: &MenuContext,
    ) -> Vec<Command> {
        let want = wanted && self.registrar.load(Ordering::Relaxed);
        if self.last_build.is_none_or(|t| t.elapsed() >= REFRESH) {
            self.last_build = Some(Instant::now());
            // Switched off: export an empty menu, so a desktop that still remembers this
            // window has nothing to show.
            let source: &[Menu] = if want { menus } else { &[] };
            let items = build(source, cx, &crate::commands::bindings());
            let mut model = self.model.lock().expect("menu model");
            if model.items != items {
                model.items = items;
                model.revision += 1;
                let revision = model.revision;
                drop(model);
                if let Err(e) = self.conn.emit_signal(
                    None::<&str>,
                    PATH,
                    INTERFACE,
                    "LayoutUpdated",
                    &(revision, 0i32),
                ) {
                    log::warn!("global menu: {e}");
                }
            }
        }
        if want != self.linked {
            self.linked = want;
            if want {
                let name = self.conn.unique_name().map(|n| n.to_string());
                match name {
                    Some(name) => link.set_menu_address(&name, PATH),
                    None => self.linked = false,
                }
            } else {
                link.clear_menu();
            }
        }
        self.picked.try_iter().collect()
    }
}

/// How often the exported enabled/checked state is refreshed.
const REFRESH: Duration = Duration::from_millis(100);

/// Keep `present` in step with whether Plasma's menu registrar is on the bus (Qt does the
/// same with a service watcher), repainting when it changes.
fn watch_registrar(conn: zbus::blocking::Connection, present: Arc<AtomicBool>, ctx: egui::Context) {
    let spawned = std::thread::Builder::new()
        .name("global-menu-registrar".into())
        .spawn(move || {
            let Ok(dbus) = zbus::blocking::fdo::DBusProxy::new(&conn) else {
                return;
            };
            let Ok(name) = zbus::names::BusName::try_from(REGISTRAR) else {
                return;
            };
            loop {
                let now = dbus.name_has_owner(name.clone()).unwrap_or(false);
                if present.swap(now, Ordering::Relaxed) != now {
                    ctx.request_repaint();
                }
                std::thread::sleep(Duration::from_secs(2));
            }
        });
    if let Err(e) = spawned {
        log::warn!("global menu: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::Command as C;
    use crate::menus::menus;

    fn items() -> Vec<Item> {
        let enabled = |c: C| c != C::Undo;
        let checked = |c: C| c == C::TogglePixelGrid;
        let cx = MenuContext {
            enabled: &enabled,
            checked: &checked,
            last_effect: Some("Gaussian Blur"),
        };
        build(&menus(), &cx, &crate::commands::bindings())
    }

    #[test]
    fn labels_use_underscore_mnemonics() {
        assert_eq!(dbus_label("&File"), "_File");
        assert_eq!(dbus_label("Save &As..."), "Save _As...");
        assert_eq!(dbus_label("R&&D my_file"), "R&D my__file");
    }

    #[test]
    fn tree_has_every_menu_with_state_and_shortcuts() {
        let items = items();
        let titles: Vec<&str> = items[0]
            .children
            .iter()
            .map(|&i| items[i as usize].label.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "_File",
                "_Edit",
                "_View",
                "_Image",
                "_Layers",
                "_Adjustments",
                "Effe_cts",
                "_Help"
            ]
        );
        let find = |c: C| items.iter().find(|i| i.command == Some(c)).unwrap();
        assert!(!find(C::Undo).enabled);
        assert_eq!(
            find(C::Undo).shortcut.as_deref(),
            Some(&["Control".to_owned(), "Z".to_owned()][..])
        );
        assert_eq!(find(C::TogglePixelGrid).toggle, Some(true));
        assert_eq!(find(C::RepeatEffect).label, "_Repeat Gaussian Blur");
        assert_eq!(
            find(C::SaveAs).shortcut.as_deref(),
            Some(&["Control".to_owned(), "Shift".to_owned(), "S".to_owned()][..])
        );
        // Effects categories are submenus with children.
        let effects = &items[items[0].children[6] as usize];
        assert!(effects.children.iter().any(|&c| {
            let it = &items[c as usize];
            it.submenu && !it.children.is_empty()
        }));
    }

    #[test]
    fn switched_off_exports_an_empty_menu() {
        let enabled = |_: C| true;
        let checked = |_: C| false;
        let cx = MenuContext {
            enabled: &enabled,
            checked: &checked,
            last_effect: None,
        };
        let items = build(&[], &cx, &crate::commands::bindings());
        assert_eq!(items.len(), 1, "just the root");
        assert!(layout(&items, 0, -1, &[]).children.is_empty());
    }

    #[test]
    fn layout_signature_and_depth() {
        assert_eq!(Layout::SIGNATURE.to_string(), "(ia{sv}av)");
        let items = items();
        let top = layout(&items, 0, 1, &[]);
        assert_eq!(top.children.len(), 8);
        let one = Layout::try_from(top.children[0].try_clone().unwrap()).unwrap();
        assert!(one.children.is_empty(), "depth 1 stops below the titles");
        assert_eq!(
            one.properties
                .get("children-display")
                .map(|v| v.to_string()),
            Some("\"submenu\"".to_owned())
        );
        let full = layout(&items, 0, -1, &["label".to_owned()]);
        let file = Layout::try_from(full.children[0].try_clone().unwrap()).unwrap();
        assert!(!file.children.is_empty());
        assert_eq!(file.properties.len(), 1);
    }
}
