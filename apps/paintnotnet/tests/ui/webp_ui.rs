//! WebP saving through the Save Configuration dialog.

use crate::common::*;
use eframe::egui::Key;
use egui_kittest::kittest::Queryable;
use pnn_core::{ColorBgra, Document, Layer, Surface};
use pnn_io::FileFormat;

#[test]
fn ctrl_s_on_a_webp_asks_for_settings_then_saves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("picture.webp");
    let mut s = Surface::new_transparent(40, 30);
    for y in 0..30 {
        for x in 0..40 {
            s.set(
                x,
                y,
                ColorBgra::from_rgba((x * 6) as u8, (y * 8) as u8, 90, 200),
            );
        }
    }
    let doc = Document::from_layers(40, 30, vec![Layer::new("Background", s)]);
    let mut h = harness();
    h.state_mut()
        .workspace_mut()
        .add(doc.clone(), Some(path.clone()), Some(FileFormat::WebP));
    h.step();
    press(&mut h, ctrl(), Key::S);
    // pdn-webp's settings.
    for label in ["Preset", "Lossless"] {
        h.get_by_label(label);
    }
    for slider in ["Quality", "Effort"] {
        h.get_by_role_and_label(eframe::egui::accesskit::Role::Slider, slider);
    }
    h.get_by_label("Lossless").click();
    h.step();
    h.get_by_label("OK").click();
    for _ in 0..200 {
        h.step();
        if path.exists()
            && h.state()
                .workspace()
                .active()
                .is_some_and(|t| !t.is_dirty())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let back = pnn_io::load(&path).expect("saved WebP loads");
    assert_eq!(
        back.layer(0).surface.to_pixels(),
        doc.layer(0).surface.to_pixels(),
        "lossless WebP round-trips exactly"
    );
}

/// One save options dialog for every type: its File type dropdown changes the format (and
/// the extension), the settings follow the type, and the flatten question comes after.
#[test]
fn save_options_file_type_switches_the_format() {
    let dir = tempfile::tempdir().unwrap();
    let jpg = dir.path().join("picture.jpg");
    let mut s = Surface::new_transparent(40, 30);
    s.fill(ColorBgra::from_rgba(30, 140, 200, 255));
    let doc = Document::from_layers(
        40,
        30,
        vec![
            Layer::new("Background", s),
            Layer::new("Layer 2", Surface::new_transparent(40, 30)),
        ],
    );
    let mut h = harness();
    h.state_mut()
        .workspace_mut()
        .add(doc, Some(jpg.clone()), Some(FileFormat::Jpeg));
    h.step();
    press(&mut h, ctrl(), Key::S);
    h.get_by_role_and_label(eframe::egui::accesskit::Role::Slider, "Quality");
    // The dropdown's selected type is its accessible value.
    h.get_by(|n| n.value().as_deref() == Some("JPEG (.jpg)"))
        .click();
    h.step();
    h.get_by_label("PNG (.png)").click();
    h.step();
    h.step();
    assert!(h.query_by_label("Quality").is_none(), "PNG has no quality");
    h.get_by_label("OK").click();
    // Two layers, a flat format: asked now, after the type (a new window takes a frame to
    // size itself before it takes clicks).
    for _ in 0..4 {
        h.step();
    }
    h.get_by_label("Flatten").click();
    let png = dir.path().join("picture.png");
    for _ in 0..200 {
        h.step();
        if png.exists()
            && h.state()
                .workspace()
                .active()
                .is_some_and(|t| !t.is_dirty())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(png.exists() && !jpg.exists(), "saved as PNG");
    let tab = h.state().workspace().active().unwrap();
    assert_eq!(tab.path.as_deref(), Some(png.as_path()));
    assert_eq!(tab.format, Some(FileFormat::Png));
    assert_eq!(
        h.state().recent_files().first(),
        Some(&std::fs::canonicalize(&png).unwrap()),
        "a save goes to the top of File > Open Recent"
    );
    assert_eq!(
        pnn_io::load(&png).unwrap().layer(0).surface.get(5, 5),
        ColorBgra::from_rgba(30, 140, 200, 255)
    );
}

/// File > Open Recent: newest first, an entry opens its file (and moves to the top), and Clear
/// this list asks first (paint.net 3.36's question), then empties it.
#[test]
fn open_recent_lists_saved_files_opens_and_clears() {
    use paintnotnet::commands::{Command, Triggered};
    let dir = tempfile::tempdir().unwrap();
    let mut s = Surface::new_transparent(20, 10);
    s.fill(ColorBgra::from_rgba(200, 30, 30, 255));
    let saved = dir.path().join("first.png");
    pnn_io::save(
        &Document::from_layers(20, 10, vec![Layer::new("Background", s)]),
        &saved,
        FileFormat::Png,
        Default::default(),
    )
    .unwrap();
    let mut h = harness();
    let ctx = h.ctx.clone();
    assert!(
        !h.state().availability(Command::ClearRecentFiles),
        "nothing to clear"
    );
    h.state_mut().note_recent(&saved);
    h.state_mut().note_recent(&dir.path().join("second.png"));
    let canonical = std::fs::canonicalize(&saved).unwrap();
    assert_eq!(h.state().recent_files()[1], canonical, "older second");
    let files = h.state_mut().recent_menu_files(&ctx);
    let menu = paintnotnet::menus::recent_menu(&files);
    let paintnotnet::menus::Entry::Sub { label, entries } = menu else {
        panic!("a submenu")
    };
    assert_eq!(label, "Open &Recent");
    let paintnotnet::menus::Entry::File { label, thumb, .. } = &entries[1] else {
        panic!("a file")
    };
    assert_eq!(label, "&2 first.png");
    assert!(thumb.is_none(), "never open here, so no thumbnail yet");
    let thumbs = dir.path().join("thumbs");
    h.state_mut().set_recent_thumbs_dir(Some(thumbs.clone()));

    let tabs = h.state().workspace().tabs().len();
    h.state_mut()
        .execute(&ctx, Triggered::once(Command::OpenRecent(1)));
    for _ in 0..200 {
        h.step();
        if h.state().workspace().tabs().len() > tabs {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let tab = h.state().workspace().active().unwrap();
    assert_eq!(tab.doc.width(), 20, "first.png opened");
    assert_eq!(
        h.state().recent_files()[0],
        canonical,
        "opening moves it to the top"
    );
    // Its thumbnail (3.36 makes one on every open and save): kept on disk, shown in the menu.
    for _ in 0..10 {
        h.step();
    }
    let files = h.state_mut().recent_menu_files(&ctx);
    let png = files[0].thumb.as_ref().expect("a thumbnail").png.clone();
    let img = image::load_from_memory(&png).unwrap().to_rgba8();
    assert_eq!(img.dimensions(), (96, 96));
    assert_eq!(
        img.get_pixel(48, 48).0,
        [200, 30, 30, 255],
        "the image's red"
    );
    assert_eq!(img.get_pixel(48, 4).0[3], 0, "centred in a clear square");
    assert!(files[1].thumb.is_none());
    assert_eq!(std::fs::read_dir(&thumbs).unwrap().count(), 1);
    // A new session finds it in the folder.
    h.state_mut().set_recent_thumbs_dir(Some(thumbs.clone()));
    let files = h.state_mut().recent_menu_files(&ctx);
    assert_eq!(files[0].thumb.as_ref().unwrap().png, png);

    h.state_mut()
        .execute(&ctx, Triggered::once(Command::ClearRecentFiles));
    for _ in 0..4 {
        h.step();
    }
    h.get_by_label("Are you sure you want to clear the Open Recent list?");
    h.get_by_label("Yes").click();
    h.step();
    h.step();
    assert!(h.state().recent_files().is_empty());
    assert_eq!(
        std::fs::read_dir(&thumbs).unwrap().count(),
        0,
        "thumbnails go with the list"
    );
}
