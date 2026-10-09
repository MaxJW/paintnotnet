//! Undo history as copy-on-write document snapshots.
//!
//! paint.net records each action as a memento in the History window and can jump to any
//! step. Here every step stores a whole `Document` clone. Untouched tiles are shared between
//! snapshots, so a step costs only the tiles that action changed. Memory is the unique tile
//! bytes across all steps; past the budget, the oldest steps are dropped.

use std::collections::HashSet;
use std::sync::Arc;

use crate::Document;

pub struct HistoryEntry<T> {
    pub name: String,
    /// Caller-defined tag, e.g. an icon.
    pub tag: T,
    document: Document,
    /// Tile bytes this step doesn't share with the step before it.
    unique_bytes: usize,
}

impl<T> HistoryEntry<T> {
    pub fn document(&self) -> &Document {
        &self.document
    }
}

pub struct History<T> {
    entries: Vec<HistoryEntry<T>>,
    current: usize,
    budget: usize,
}

/// Default budget: a quarter of a 32 GB machine.
pub const DEFAULT_HISTORY_BUDGET: usize = 8 << 30;

fn tile_ptrs(doc: &Document) -> HashSet<usize> {
    let mut set = HashSet::new();
    for layer in doc.layers() {
        let s = &layer.surface;
        for c in s.tile_coords() {
            if let Some(t) = s.tile(c) {
                set.insert(Arc::as_ptr(t) as usize);
            }
        }
    }
    set
}

/// Tile bytes in `doc` whose tiles aren't in `shared`.
fn bytes_not_in(doc: &Document, shared: &HashSet<usize>) -> usize {
    let tile = crate::tiles::TILE_PIXELS * 4;
    let mut seen = HashSet::new();
    let mut bytes = 0;
    for layer in doc.layers() {
        let s = &layer.surface;
        for c in s.tile_coords() {
            if let Some(t) = s.tile(c) {
                let p = Arc::as_ptr(t) as usize;
                if !shared.contains(&p) && seen.insert(p) {
                    bytes += tile;
                }
            }
        }
    }
    bytes
}

impl<T: Copy> History<T> {
    /// Start with the image as opened or created ("Open Image" / "New Image").
    pub fn new(name: impl Into<String>, tag: T, document: &Document) -> Self {
        let unique_bytes = bytes_not_in(document, &HashSet::new());
        Self {
            entries: vec![HistoryEntry {
                name: name.into(),
                tag,
                document: document.clone(),
                unique_bytes,
            }],
            current: 0,
            budget: DEFAULT_HISTORY_BUDGET,
        }
    }

    pub fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
        self.trim();
    }

    pub fn entries(&self) -> &[HistoryEntry<T>] {
        &self.entries
    }

    /// Index of the step the document is at.
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn can_undo(&self) -> bool {
        self.current > 0
    }

    pub fn can_redo(&self) -> bool {
        self.current + 1 < self.entries.len()
    }

    /// Record the document after an action. Steps after the current one are discarded.
    pub fn push(&mut self, name: impl Into<String>, tag: T, document: &Document) {
        self.entries.truncate(self.current + 1);
        let prev = tile_ptrs(&self.entries[self.current].document);
        let unique_bytes = bytes_not_in(document, &prev);
        self.entries.push(HistoryEntry {
            name: name.into(),
            tag,
            document: document.clone(),
            unique_bytes,
        });
        self.current += 1;
        self.trim();
    }

    pub fn undo(&mut self) -> Option<&Document> {
        self.jump(self.current.checked_sub(1)?)
    }

    pub fn redo(&mut self) -> Option<&Document> {
        self.jump(self.current + 1)
    }

    /// Clicking a step in the History window.
    pub fn jump(&mut self, index: usize) -> Option<&Document> {
        if index >= self.entries.len() || index == self.current {
            return None;
        }
        self.current = index;
        Some(&self.entries[index].document)
    }

    /// Unique tile bytes held by history.
    pub fn memory_bytes(&self) -> usize {
        self.entries.iter().map(|e| e.unique_bytes).sum()
    }

    /// Drop the oldest steps while over budget (always keeping the current step and one
    /// before it). The new first step's unique bytes become all of its tiles.
    fn trim(&mut self) {
        while self.memory_bytes() > self.budget && self.current >= 2 {
            self.entries.remove(0);
            self.current -= 1;
            let first = &mut self.entries[0];
            first.unique_bytes = bytes_not_in(&first.document, &HashSet::new());
            if self.entries.len() > 1 {
                let p = tile_ptrs(&self.entries[0].document);
                self.entries[1].unique_bytes = bytes_not_in(&self.entries[1].document, &p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ColorBgra, Rect};

    fn paint(doc: &mut Document, x: u32, y: u32) {
        doc.modify_layer(
            0,
            Some(Rect {
                x,
                y,
                width: 1,
                height: 1,
            }),
            |l| l.surface.set(x, y, ColorBgra::BLACK),
        );
    }

    #[test]
    fn undo_redo_and_jump() {
        let mut doc = Document::new_blank(600, 600);
        let mut h = History::new("New Image", (), &doc);
        paint(&mut doc, 1, 1);
        h.push("Pencil", (), &doc);
        paint(&mut doc, 300, 300);
        h.push("Pencil", (), &doc);
        assert_eq!(h.current(), 2);
        let back = h.undo().unwrap().clone();
        assert_eq!(back.layer(0).surface.get(300, 300), ColorBgra::WHITE);
        assert_eq!(back.layer(0).surface.get(1, 1), ColorBgra::BLACK);
        let first = h.jump(0).unwrap();
        assert_eq!(first.layer(0).surface.get(1, 1), ColorBgra::WHITE);
        assert!(h.redo().is_some());
        assert_eq!(h.current(), 1);
        // A new action after undo discards the redo steps.
        h.push("Pencil", (), &back);
        assert_eq!(h.entries().len(), 3);
        assert!(!h.can_redo());
    }

    #[test]
    fn steps_only_cost_the_tiles_they_change() {
        let mut doc = Document::new_blank(1024, 1024);
        // new_blank shares one white tile everywhere: 1 tile of unique memory.
        let mut h = History::new("New Image", (), &doc);
        assert_eq!(h.memory_bytes(), 256 * 256 * 4);
        paint(&mut doc, 5, 5);
        h.push("Pencil", (), &doc);
        assert_eq!(
            h.memory_bytes(),
            2 * 256 * 256 * 4,
            "one copied tile per small edit"
        );
    }

    #[test]
    fn budget_drops_oldest_steps() {
        let mut doc = Document::new_blank(512, 512);
        let mut h = History::new("New Image", (), &doc);
        h.set_budget(6 * 256 * 256 * 4);
        for i in 0..10 {
            paint(&mut doc, (i * 60) % 512, (i * 37) % 512);
            h.push("Pencil", (), &doc);
        }
        assert!(h.memory_bytes() <= 6 * 256 * 256 * 4);
        assert!(h.entries().len() < 11);
        assert_eq!(h.current(), h.entries().len() - 1);
    }

    /// Roadmap acceptance: 500 steps on a 4K image, each touching one tile, stay small
    /// (one tile per step, not one image per step) and all undo/redo correctly.
    #[test]
    fn five_hundred_steps_on_4k() {
        let mut doc = Document::new_blank(3840, 2160);
        let mut h = History::new("New Image", (), &doc);
        let t0 = std::time::Instant::now();
        for i in 0..500u32 {
            let (x, y) = ((i * 977) % 3840, (i * 613) % 2160);
            doc.modify_layer(
                0,
                Some(Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                }),
                |l| {
                    l.surface
                        .set(x, y, ColorBgra::from_rgba(i as u8, 0, 0, 255))
                },
            );
            h.push("Pencil", (), &doc);
        }
        let tile = 256 * 256 * 4;
        // At most one new tile per step, plus the shared white tile.
        assert!(h.memory_bytes() <= 501 * tile, "{} bytes", h.memory_bytes());
        // A naive full-copy history would need 500 × 33 MB.
        assert!(h.memory_bytes() < 3840 * 2160 * 4 * 5);
        for _ in 0..500 {
            assert!(h.undo().is_some());
        }
        assert_eq!(
            h.entries()[h.current()]
                .document()
                .layer(0)
                .surface
                .get(0, 0),
            ColorBgra::WHITE
        );
        for _ in 0..500 {
            assert!(h.redo().is_some());
        }
        let last = h.entries()[h.current()].document();
        let (x, y) = ((499 * 977) % 3840, (499 * 613) % 2160);
        assert_eq!(
            last.layer(0).surface.get(x, y),
            ColorBgra::from_rgba(499u32 as u8, 0, 0, 255)
        );
        assert!(t0.elapsed().as_secs_f64() < 20.0, "took {:?}", t0.elapsed());
    }
}
