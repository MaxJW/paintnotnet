//! Contact sheet of every implemented effect at its defaults, for checking by eye.
//! Runs only with `PNN_EFFECT_SHEET=<photo> PNN_RENDER_DUMP=<dir>`; prints the order.

use pnn_core::{ColorBgra, Rect, Surface, transform};
use pnn_effects::framework::Env;

#[test]
fn effect_sheet() {
    let (Ok(photo), Ok(dir)) = (
        std::env::var("PNN_EFFECT_SHEET"),
        std::env::var("PNN_RENDER_DUMP"),
    ) else {
        return;
    };
    let doc = pnn_io::load(std::path::Path::new(&photo)).unwrap();
    let (cw, ch) = (320u32, 180u32);
    let src = transform::resample(
        &doc.layer(0).surface,
        cw,
        ch,
        transform::Resampling::Bicubic,
        false,
    );
    let cell = Rect {
        x: 0,
        y: 0,
        width: cw,
        height: ch,
    };
    let env = Env {
        primary: ColorBgra::BLACK,
        secondary: ColorBgra::WHITE,
        bounds: cell,
        selection: None,
    };
    let mut names = vec!["(original)"];
    let mut cells = vec![src.to_pixels()];
    for (i, e) in pnn_effects::EFFECTS.iter().enumerate() {
        if let Some(d) = pnn_effects::effect_def(pnn_effects::EffectId(i as u16)) {
            let r = (d.build)(&d.defaults(), &src, &env);
            cells.push(pnn_effects::framework::render_rect(r.as_ref(), &src, cell));
            names.push(e.name);
        }
    }
    let cols = 6u32;
    let rows = (cells.len() as u32).div_ceil(cols);
    let mut sheet = Surface::new_transparent(cols * cw, rows * ch);
    for (k, px) in cells.iter().enumerate() {
        let (ox, oy) = ((k as u32 % cols) * cw, (k as u32 / cols) * ch);
        for y in 0..ch {
            for x in 0..cw {
                sheet.set(ox + x, oy + y, px[(y * cw + x) as usize]);
            }
        }
    }
    let out = pnn_core::Document::from_layers(
        sheet.width(),
        sheet.height(),
        vec![pnn_core::Layer::new("Sheet", sheet)],
    );
    pnn_io::save(
        &out,
        &std::path::Path::new(&dir).join("effects-sheet.png"),
        pnn_io::FileFormat::Png,
        pnn_io::SaveOptions::default(),
    )
    .unwrap();
    for (k, n) in names.iter().enumerate() {
        println!("{:2} r{} c{}: {n}", k, k as u32 / cols, k as u32 % cols);
    }
}

/// Object and Selection effects need transparency: a sheet of them on a shape.
#[test]
fn object_effect_sheet() {
    let Ok(dir) = std::env::var("PNN_RENDER_DUMP") else {
        return;
    };
    if std::env::var("PNN_EFFECT_SHEET").is_err() {
        return;
    }
    let (cw, ch) = (240u32, 180u32);
    // A ring and a bar on transparency.
    let mut src = Surface::new_transparent(cw, ch);
    for y in 0..ch {
        for x in 0..cw {
            let (dx, dy) = (x as f32 - 90.0, y as f32 - 90.0);
            let d = (dx * dx + dy * dy).sqrt();
            let ring = (30.0..55.0).contains(&d);
            let bar = (150..215).contains(&x) && (40..140).contains(&y);
            if ring || bar {
                src.set(x, y, ColorBgra::from_rgba(240, 160, 40, 255));
            }
        }
    }
    let cell = Rect {
        x: 0,
        y: 0,
        width: cw,
        height: ch,
    };
    let mut sel = pnn_core::Mask::new(cw, ch, 0);
    for y in 30..150 {
        for x in 40..200 {
            sel.set(x, y, 255);
        }
    }
    let mut cells = vec![src.to_pixels()];
    let mut names = vec!["(original)".to_owned()];
    for (i, e) in pnn_effects::EFFECTS.iter().enumerate() {
        use pnn_effects::Category as C;
        if !matches!(e.category, C::Object | C::Selection | C::Adjustment) {
            continue;
        }
        let Some(d) = pnn_effects::effect_def(pnn_effects::EffectId(i as u16)) else {
            continue;
        };
        let env = Env {
            primary: ColorBgra::from_rgba(20, 20, 60, 255),
            secondary: ColorBgra::WHITE,
            bounds: cell,
            selection: (e.category == C::Selection).then(|| std::sync::Arc::new(sel.clone())),
        };
        let r = (d.build)(&d.defaults(), &src, &env);
        cells.push(pnn_effects::framework::render_rect(r.as_ref(), &src, cell));
        names.push(format!("{:?}: {}", e.category, e.name));
    }
    let cols = 4u32;
    let rows = (cells.len() as u32).div_ceil(cols);
    let mut sheet = Surface::new_transparent(cols * cw, rows * ch);
    for (k, px) in cells.iter().enumerate() {
        let (ox, oy) = ((k as u32 % cols) * cw, (k as u32 / cols) * ch);
        for y in 0..ch {
            for x in 0..cw {
                // Over mid-grey so shadows and transparency show.
                let p = px[(y * cw + x) as usize];
                let bg = ColorBgra::from_rgba(150, 150, 150, 255);
                sheet.set(
                    ox + x,
                    oy + y,
                    pnn_core::BlendMode::Normal.apply(bg, p, 255),
                );
            }
        }
    }
    let out = pnn_core::Document::from_layers(
        sheet.width(),
        sheet.height(),
        vec![pnn_core::Layer::new("Sheet", sheet)],
    );
    pnn_io::save(
        &out,
        &std::path::Path::new(&dir).join("object-effects-sheet.png"),
        pnn_io::FileFormat::Png,
        pnn_io::SaveOptions::default(),
    )
    .unwrap();
    for (k, n) in names.iter().enumerate() {
        println!("{:2} r{} c{}: {n}", k, k as u32 / cols, k as u32 % cols);
    }
}
