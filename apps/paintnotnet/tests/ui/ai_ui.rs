//! Phase 17: Remove Background and Select Subject. Nothing is downloaded without asking; with
//! a model at hand (`PNN_AI_MODELS=<dir>` holding the models for this computer's device), both
//! find the subject of a drawn "apple".

use crate::common::*;
use egui_kittest::kittest::{NodeT, Queryable};
use paintnotnet::commands::{Command, Triggered};
use pnn_core::{ColorBgra, Document, Layer, Surface};
use std::time::{Duration, Instant};

type H<'a> = egui_kittest::Harness<'a, paintnotnet::app::PaintNotNetApp>;

fn remove_background() -> Command {
    let id = pnn_effects::EFFECTS
        .iter()
        .position(|e| e.name == "Remove Background")
        .expect("in the catalog");
    Command::Effect(pnn_effects::EffectId(id as u16))
}

fn run(h: &mut H<'_>, c: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, Triggered::once(c));
    for _ in 0..4 {
        h.step();
    }
}

/// A red apple with a brown stem on a pale grey background.
fn apple() -> Document {
    let (w, h) = (320u32, 240u32);
    let mut s = Surface::new_transparent(w, h);
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = (
                (x as f64 + 0.5 - 160.0) / 50.0,
                (y as f64 + 0.5 - 120.0) / 60.0,
            );
            let c = if dx * dx + dy * dy <= 1.0 {
                ColorBgra::from_rgba(220, 60, 40, 255)
            } else if (150..166).contains(&x) && (40..66).contains(&y) {
                ColorBgra::from_rgba(90, 60, 30, 255)
            } else {
                ColorBgra::from_rgba(200, 205, 210, 255)
            };
            s.set(x, y, c);
        }
    }
    Document::from_layers(w, h, vec![Layer::new("Background", s)])
}

/// Step until `done` (the model runs on other threads), at most `limit`.
fn step_until(h: &mut H<'_>, limit: Duration, mut done: impl FnMut(&mut H<'_>) -> bool) {
    let start = Instant::now();
    while !done(h) {
        assert!(start.elapsed() < limit, "timed out");
        h.step();
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn remove_background_asks_before_downloading() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness_with(apple());
    h.state_mut().set_models_dir(dir.path().to_owned());
    let c = remove_background();
    assert!(h.state().command_state().enabled(c), "menu item enabled");
    run(&mut h, c);
    assert!(h.state_mut().effect_mut().is_some(), "dialog open");
    // Either the download question or, without ONNX Runtime, how to get it.
    let asks = h.query_by_label_contains("one-time download").is_some();
    let no_runtime = h
        .query_by_label_contains("ONNX Runtime isn't installed")
        .is_some();
    assert!(asks || no_runtime, "the dialog says what's needed");
    if asks {
        assert!(h.query_by_label_contains("Download").is_some());
    }
    // Nothing to apply yet, the layer untouched, nothing fetched.
    assert!(h.get_by_label("OK").accesskit_node().is_disabled());
    assert_eq!(
        doc(&h).layer(0).surface.to_pixels(),
        apple().layer(0).surface.to_pixels()
    );
    for _ in 0..20 {
        h.step();
    }
    assert_eq!(
        std::fs::read_dir(dir.path()).unwrap().count(),
        0,
        "nothing downloaded"
    );
    h.get_by_label("Cancel").click();
    for _ in 0..3 {
        h.step();
    }
    assert!(h.state_mut().effect_mut().is_none());
}

#[test]
fn select_subject_asks_before_downloading() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness_with(apple());
    h.state_mut().set_models_dir(dir.path().to_owned());
    let n = history_names(&h).len();
    assert!(h.state().command_state().enabled(Command::SelectSubject));
    run(&mut h, Command::SelectSubject);
    assert!(h.state().selecting_subject());
    assert!(
        h.query_by_label_contains("one-time download").is_some()
            || h.query_by_label_contains("ONNX Runtime isn't installed")
                .is_some()
    );
    h.get_by_label("Cancel").click();
    for _ in 0..3 {
        h.step();
    }
    assert!(!h.state().selecting_subject());
    assert!(doc(&h).selection().is_empty());
    assert_eq!(history_names(&h).len(), n);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

/// With the real model: `PNN_AI_MODELS=<dir> cargo test -p paintnotnet --test ui ai_ui`.
#[test]
fn removes_the_background_and_selects_the_subject_with_the_model() {
    let Some(dir) = std::env::var_os("PNN_AI_MODELS") else {
        eprintln!("PNN_AI_MODELS not set: skipped");
        return;
    };
    let mut h = harness_with(apple());
    h.state_mut().set_models_dir(dir.into());
    h.state_mut()
        .set_ai_worker(env!("CARGO_BIN_EXE_paintnotnet").into());
    run(&mut h, remove_background());
    // The preview arrives once the model has run: the background goes transparent.
    step_until(&mut h, Duration::from_secs(90), |h| {
        doc(h).layer(0).surface.get(5, 5).a == 0
    });
    h.get_by_label("OK").click();
    for _ in 0..4 {
        h.step();
    }
    assert_eq!(history_names(&h).last().unwrap(), "Remove Background");
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(160, 120).a, 255, "the apple stays");
    assert_eq!(s.get(157, 50).a, 255, "and its stem");
    assert_eq!(s.get(100, 120).a, 0, "beside it is gone");
    assert_eq!(
        s.get(160, 120),
        ColorBgra::from_rgba(220, 60, 40, 255),
        "colours kept"
    );

    // Undo, then Select Subject on the same pixels: no second run needed (cached), and the
    // selection follows the apple.
    press(&mut h, ctrl(), eframe::egui::Key::Z);
    run(&mut h, Command::SelectSubject);
    step_until(&mut h, Duration::from_secs(90), |h| {
        !h.state().selecting_subject()
    });
    assert_eq!(history_names(&h).last().unwrap(), "Select Subject");
    let sel = doc(&h).selection();
    assert!(sel.contains([160.0, 120.0]) && sel.contains([157.0, 50.0]));
    assert!(!sel.contains([5.0, 5.0]) && !sel.contains([100.0, 120.0]));
    // The outline is smooth: it follows the ellipse within a pixel along its sides.
    let [x0, _, x1, _] = sel.bounds().unwrap();
    assert!(
        (x0 - 110.0).abs() < 1.5 && (x1 - 210.0).abs() < 1.5,
        "{x0} {x1}"
    );
}

/// Fetches BiRefNet lite (115 MB fp16 / 224 MB fp32) from Hugging Face into a temporary
/// folder through the dialog: `cargo test -p paintnotnet --test ui -- --ignored downloads`.
#[test]
#[ignore = "downloads a model over the network"]
fn downloads_the_model_when_asked_then_runs_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness_with(apple());
    h.state_mut().set_models_dir(dir.path().to_owned());
    h.state_mut()
        .set_ai_worker(env!("CARGO_BIN_EXE_paintnotnet").into());
    run(&mut h, remove_background());
    h.get_by_label("Fast").click();
    for _ in 0..3 {
        h.step();
    }
    h.get_by_label_contains("Download").click();
    let mut saw_progress = false;
    step_until(&mut h, Duration::from_secs(600), |h| {
        saw_progress |= h
            .query_by_label_contains("Downloading BiRefNet lite")
            .is_some();
        doc(h).layer(0).surface.get(5, 5).a == 0
    });
    assert!(saw_progress, "progress shown");
    let files: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].starts_with("birefnet-lite-") && files[0].ends_with("-de15b22.onnx"));
}
