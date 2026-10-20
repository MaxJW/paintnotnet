//! Phase 4b: clipboard commands through real key events, with an in-memory clipboard
//! (the desktop clipboard is never touched).

use std::cell::RefCell;
use std::rc::Rc;

use crate::common::*;
use eframe::egui::{Key, Modifiers, PointerButton};
use egui_kittest::kittest::Queryable;
use paintnotnet::clipboard::{ClipImage, MemoryClipboard};
use pnn_core::{ColorBgra, Document, Layer, Surface};

const RED: ColorBgra = ColorBgra::from_rgba(255, 0, 0, 255);

fn setup() -> (
    egui_kittest::Harness<'static, paintnotnet::app::PaintNotNetApp>,
    Rc<RefCell<MemoryClipboard>>,
) {
    let mut s = Surface::new_transparent(100, 80);
    s.fill(ColorBgra::WHITE);
    for y in 10..20 {
        for x in 10..20 {
            s.set(x, y, RED);
        }
    }
    let mut h = harness_with(Document::from_layers(
        100,
        80,
        vec![Layer::new("Background", s)],
    ));
    let clip = Rc::new(RefCell::new(MemoryClipboard::default()));
    h.state_mut().set_clipboard(Box::new(clip.clone()));
    (h, clip)
}

fn select(
    h: &mut egui_kittest::Harness<'static, paintnotnet::app::PaintNotNetApp>,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) {
    press(h, Modifiers::NONE, Key::S);
    drag(
        h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(x0, y0), (x1, y1)],
    );
}

#[test]
fn copy_paste_floats_then_finishes() {
    let (mut h, clip) = setup();
    select(&mut h, 10.0, 10.0, 20.0, 20.0);
    press(&mut h, ctrl(), Key::C);
    let img = clip.borrow().image.clone().expect("copied");
    assert_eq!((img.width, img.height), (10, 10));
    assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
    press(&mut h, ctrl(), Key::V);
    assert_eq!(
        h.state().tool(),
        pnn_tools::ToolKind::MoveSelectedPixels,
        "paste switches to Move Selected Pixels"
    );
    // The view shows the whole image, so the paste lands at the top-left.
    let b = doc(&h).selection().pixel_bounds(100, 80).unwrap();
    assert_eq!((b.x, b.y, b.width, b.height), (0, 0, 10, 10));
    drag(
        &mut h,
        PointerButton::Primary,
        Modifiers::NONE,
        &[(5.0, 5.0), (55.0, 45.0)],
    );
    press(&mut h, Modifiers::NONE, Key::Enter);
    let s = &doc(&h).layer(0).surface;
    assert_eq!(s.get(52, 42), RED, "pasted pixels dropped where moved");
    assert_eq!(s.get(15, 15), RED, "original untouched");
    let names = history_names(&h);
    assert_eq!(
        &names[names.len() - 3..],
        ["Paste", "Move Pixels", "Finish Pixels"]
    );
}

#[test]
fn cut_leaves_a_hole() {
    let (mut h, clip) = setup();
    select(&mut h, 10.0, 10.0, 20.0, 20.0);
    press(&mut h, ctrl(), Key::X);
    assert!(clip.borrow().image.is_some());
    assert_eq!(doc(&h).layer(0).surface.get(15, 15), ColorBgra::TRANSPARENT);
    assert_eq!(history_names(&h).last().unwrap(), "Cut");
}

#[test]
fn paste_into_new_layer_and_new_image() {
    let (mut h, clip) = setup();
    *clip.borrow_mut() = MemoryClipboard {
        image: Some(ClipImage {
            width: 2,
            height: 3,
            rgba: [0, 0, 255, 255].repeat(6),
        }),
        text: None,
    };
    press(&mut h, ctrl() | Modifiers::SHIFT, Key::V);
    press(&mut h, Modifiers::NONE, Key::Enter);
    assert_eq!(doc(&h).layers().len(), 2);
    assert_eq!(
        doc(&h).layer(1).surface.get(1, 2),
        ColorBgra::from_rgba(0, 0, 255, 255)
    );
    assert_eq!(doc(&h).layer(0).surface.get(1, 2), ColorBgra::WHITE);
    press(&mut h, ctrl() | Modifiers::ALT, Key::V);
    assert_eq!(h.state().workspace().tabs().len(), 2);
    assert_eq!((doc(&h).width(), doc(&h).height()), (2, 3));
}

#[test]
fn larger_paste_asks_to_expand_canvas() {
    let (mut h, clip) = setup();
    clip.borrow_mut().image = Some(ClipImage {
        width: 150,
        height: 20,
        rgba: vec![128; 150 * 20 * 4],
    });
    press(&mut h, ctrl(), Key::V);
    h.get_by_label("Expand canvas").click();
    h.step();
    h.step();
    assert_eq!((doc(&h).width(), doc(&h).height()), (150, 80));
}

#[test]
fn copy_and_paste_selection_as_json() {
    let (mut h, clip) = setup();
    select(&mut h, 3.0, 4.0, 9.0, 19.0);
    press(&mut h, ctrl() | Modifiers::ALT | Modifiers::SHIFT, Key::C);
    assert_eq!(
        clip.borrow().text.as_deref(),
        Some(r#"{"polygonList": ["3,4,9,4,9,19,3,19,3,4"]}"#)
    );
    press(&mut h, ctrl(), Key::D);
    press(&mut h, ctrl() | Modifiers::ALT | Modifiers::SHIFT, Key::V);
    let b = doc(&h).selection().pixel_bounds(100, 80).unwrap();
    assert_eq!((b.x, b.y, b.width, b.height), (3, 4, 6, 15));
}

/// On a large image Cut builds the copy and clears the layer on the effect threads; the
/// clipboard, the layer and one "Cut" step land together.
#[test]
fn large_cut_lands_from_the_background() {
    use paintnotnet::commands::{Command, Triggered};
    let (w, hh) = (1500u32, 1000u32);
    let mut s = Surface::new_transparent(w, hh);
    s.fill(ColorBgra::WHITE);
    for y in 10..20 {
        for x in 10..20 {
            s.set(x, y, RED);
        }
    }
    let mut h = harness_with(Document::from_layers(
        w,
        hh,
        vec![Layer::new("Background", s)],
    ));
    let clip = Rc::new(RefCell::new(MemoryClipboard::default()));
    h.state_mut().set_clipboard(Box::new(clip.clone()));
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.doc
        .set_selection(pnn_core::Selection::rect(10.0, 10.0, 20.0, 20.0));
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, Triggered::once(Command::Cut));
    assert!(h.state().click_job_running(), "in the background");
    let start = std::time::Instant::now();
    while h.state().click_job_running() {
        assert!(start.elapsed().as_secs() < 10, "the cut never landed");
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.step();
    }
    let img = clip.borrow().image.clone().expect("copied");
    assert_eq!((img.width, img.height), (10, 10));
    assert_eq!(&img.rgba[0..4], &[255, 0, 0, 255]);
    assert_eq!(doc(&h).layer(0).surface.get(15, 15).a, 0, "cut out");
    assert_eq!(doc(&h).layer(0).surface.get(25, 15), ColorBgra::WHITE);
    assert_eq!(history_names(&h).last().unwrap(), "Cut");
}

/// On a large image Paste reads the clipboard and draws the pixels on the effect threads;
/// the float and its history step land together, as on a small image.
#[test]
fn large_paste_lands_from_the_background() {
    use paintnotnet::commands::{Command, Triggered};
    let (w, hh) = (1500u32, 1000u32);
    let mut s = Surface::new_transparent(w, hh);
    s.fill(ColorBgra::WHITE);
    let mut h = harness_with(Document::from_layers(
        w,
        hh,
        vec![Layer::new("Background", s)],
    ));
    let clip = Rc::new(RefCell::new(MemoryClipboard {
        image: Some(ClipImage {
            width: 4,
            height: 3,
            rgba: [0, 0, 255, 128].repeat(12),
        }),
        text: None,
    }));
    h.state_mut().set_clipboard(Box::new(clip.clone()));
    let ctx = h.ctx.clone();
    let wait = |h: &mut egui_kittest::Harness<'static, paintnotnet::app::PaintNotNetApp>| {
        let start = std::time::Instant::now();
        while h.state().click_job_running() {
            assert!(start.elapsed().as_secs() < 10, "the paste never landed");
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.step();
        }
    };
    h.state_mut().execute(&ctx, Triggered::once(Command::Paste));
    assert!(h.state().click_job_running(), "in the background");
    wait(&mut h);
    assert_eq!(h.state().tool(), pnn_tools::ToolKind::MoveSelectedPixels);
    // At the visible top-left, as on a small image.
    let b = doc(&h).selection().pixel_bounds(w, hh).unwrap();
    assert_eq!((b.width, b.height), (4, 3));
    let (x, y) = (b.x, b.y);
    // Half-transparent blue over white, as the float draws it.
    let over = doc(&h).layer(0).surface.get(x + 3, y + 2);
    assert!(
        over.b > 200 && over.r < 140 && over.r > 110 && over.a == 255,
        "{over:?}"
    );
    assert_eq!(doc(&h).layer(0).surface.get(x + 4, y + 2), ColorBgra::WHITE);
    assert_eq!(history_names(&h).last().unwrap(), "Paste");

    press(&mut h, Modifiers::NONE, Key::Enter);
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::PasteIntoNewLayer));
    wait(&mut h);
    assert_eq!(doc(&h).layers().len(), 2);
    assert_eq!(doc(&h).active_index(), 1);
    assert_eq!(
        doc(&h).layer(1).surface.get(x, y),
        ColorBgra::from_rgba(0, 0, 255, 128)
    );
    assert_eq!(doc(&h).layer(1).surface.get(x + 5, y).a, 0);
    let names = history_names(&h);
    assert_eq!(
        &names[names.len() - 2..],
        ["Finish Pixels", "Paste into New Layer"].map(String::from)
    );
}
