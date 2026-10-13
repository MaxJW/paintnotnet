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
