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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub appearance: Appearance,
    pub mouse_wheel: MouseWheel,
    /// Show the menus in KDE Plasma's global menu when one is running.
    pub global_menu: bool,
    /// Minutes between autosaves for crash recovery; 0 turns it off.
    pub autosave_minutes: u32,
    /// Rebound shortcuts: command id → chords ("Ctrl+Shift+K"); empty = unbound.
    pub shortcuts: BTreeMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            appearance: Appearance::System,
            mouse_wheel: MouseWheel::Zoom,
            global_menu: true,
            autosave_minutes: 1,
            shortcuts: BTreeMap::new(),
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
    use eframe::egui::Key;

    #[test]
    fn round_trips_and_ignores_junk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.toml");
        let mut c = Config {
            appearance: Appearance::Light,
            autosave_minutes: 5,
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
