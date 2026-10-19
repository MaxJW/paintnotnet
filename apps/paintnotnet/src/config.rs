//! User settings, saved as TOML in `$XDG_CONFIG_HOME/paintnotnet/settings.toml`
//! (`~/.config/paintnotnet/settings.toml`). Unknown or invalid entries are ignored, so an
//! edited or older file never stops the app starting.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::commands::{Chord, Command};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    /// Follow the desktop's light/dark setting.
    #[default]
    System,
    Dark,
    Light,
}

/// What the mouse wheel does over the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseWheel {
    /// Zoom about the pointer; Shift+wheel and touchpad scrolling still scroll.
    #[default]
    Zoom,
    /// paint.net's behaviour: scroll, Ctrl+wheel zooms.
    Scroll,
}

/// The docked panels' layout, remembered between sessions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Dock {
    /// Width of the right-hand dock (Colors, Layers, History), in points.
    pub width: f32,
    /// Height of the History section, in points.
    pub history_height: f32,
    /// Panels hidden with F5–F8 ("Tools", "History", "Layers", "Colors").
    pub hidden: Vec<String>,
    /// Sections collapsed to their title.
    pub collapsed: Vec<String>,
}

impl Default for Dock {
    fn default() -> Self {
        Self {
            width: 300.0,
            history_height: 192.0,
            hidden: Vec::new(),
            collapsed: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub appearance: Appearance,
    pub mouse_wheel: MouseWheel,
    /// Show the menus in KDE Plasma's global menu when one is running.
    pub global_menu: bool,
    /// Use the desktop's title bar instead of paintnotnet's own window buttons.
    pub system_title_bar: bool,
    /// Minutes between autosaves for crash recovery; 0 turns it off.
    pub autosave_minutes: u32,
    /// Rebound shortcuts: command id → chords ("Ctrl+Shift+K"); empty = unbound.
    pub shortcuts: BTreeMap<String, Vec<String>>,
    pub dock: Dock,
    /// File > Open Recent, newest first (at most `RECENT_MAX`).
    pub recent_files: Vec<PathBuf>,
}

/// Files kept in File > Open Recent (paint.net 3.36 `AppWorkspace.defaultMostRecentFilesMax`).
pub const RECENT_MAX: usize = 8;

impl Config {
    /// Put `path` at the top of the recent files (once), dropping the oldest past
    /// `RECENT_MAX`.
    pub fn note_recent(&mut self, path: &std::path::Path) {
        self.recent_files.retain(|p| p != path);
        self.recent_files.insert(0, path.to_owned());
        self.recent_files.truncate(RECENT_MAX);
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            mouse_wheel: MouseWheel::Zoom,
            global_menu: true,
            system_title_bar: false,
            autosave_minutes: 1,
            shortcuts: BTreeMap::new(),
            dock: Dock::default(),
            recent_files: Vec::new(),
        }
    }
}

pub fn default_path() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("XDG_CONFIG_HOME").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d).join("paintnotnet/settings.toml"));
    }
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/paintnotnet/settings.toml"))
}

impl Config {
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                log::warn!("{}: {e}; using defaults", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    /// The shortcut overrides as commands and chords (unknown ids and chords dropped).
    pub fn overrides(&self) -> Vec<(Command, Vec<Chord>)> {
        let all = crate::menus::all_commands();
        self.shortcuts
            .iter()
            .filter_map(|(id, chords)| {
                let c = all.iter().copied().find(|c| c.id() == *id)?;
                Some((c, chords.iter().filter_map(|t| Chord::parse(t)).collect()))
            })
            .collect()
    }

    pub fn set_override(&mut self, c: Command, chords: &[Chord]) {
        self.shortcuts
            .insert(c.id(), chords.iter().map(|ch| ch.text()).collect());
    }

    pub fn reset_override(&mut self, c: Command) {
        self.shortcuts.remove(&c.id());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_newest_first_once_at_most_eight() {
        let mut c = Config::default();
        for i in 0..10 {
            c.note_recent(Path::new(&format!("/p/{i}.png")));
        }
        assert_eq!(c.recent_files.len(), RECENT_MAX);
        assert_eq!(c.recent_files[0], PathBuf::from("/p/9.png"));
        assert_eq!(c.recent_files[7], PathBuf::from("/p/2.png"));
        c.note_recent(Path::new("/p/5.png"));
        assert_eq!(
            c.recent_files[0],
            PathBuf::from("/p/5.png"),
            "moved to the top"
        );
        assert_eq!(c.recent_files.len(), RECENT_MAX, "not twice");
    }
    use eframe::egui::Key;

    #[test]
    fn round_trips_and_ignores_junk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut c = Config {
            appearance: Appearance::Light,
            autosave_minutes: 5,
            dock: Dock {
                width: 340.0,
                history_height: 150.0,
                hidden: vec!["Tools".into()],
                collapsed: vec!["Colors".into()],
            },
            ..Config::default()
        };
        let chord = Chord {
            ctrl: true,
            alt: false,
            shift: Some(true),
            key: Key::J,
        };
        c.set_override(Command::Settings, &[chord]);
        c.save(&path).unwrap();
        let back = Config::load(&path);
        assert_eq!(back, c);
        assert_eq!(back.overrides(), vec![(Command::Settings, vec![chord])]);
        std::fs::write(&path, "appearance = \"Nope\"\n[[[").unwrap();
        assert_eq!(Config::load(&path), Config::default());
        assert_eq!(
            Chord::parse("Ctrl++").map(|c| c.key),
            Some(Key::Plus),
            "plus key"
        );
    }
}
