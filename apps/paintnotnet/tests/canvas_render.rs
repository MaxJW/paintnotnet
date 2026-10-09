//! Renders the real app offscreen through wgpu and checks canvas pixels against paint.net's
//! display rules: image pixels shown nearest-neighbour when zoomed in, transparency over the
//! 191/255 checkerboard, and the black dotted pixel grid at >= 200%.
//!
//! Set `PNN_RENDER_DUMP=<dir>` to also write the rendered frame there as a PNG.

use eframe::egui::vec2;
use egui_kittest::Harness;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::commands::{Command, Triggered};
use pnn_core::{ColorBgra, Document, Layer, Surface};
use pnn_render::View;

const RED: ColorBgra = ColorBgra::from_rgba(220, 40, 40, 255);
const HALF_BLUE: ColorBgra = ColorBgra::from_rgba(0, 0, 255, 128);

fn test_doc() -> Document {
    let mut s = Surface::new_transparent(64, 48);
    for y in 8..30 {
        for x in 8..40 {
            s.set(x, y, RED);
        }
    }
    s.set(50, 40, HALF_BLUE);
    Document::from_layers(64, 48, vec![Layer::new("Background", s)])
}

struct Rendered {
    image: image::RgbaImage,
    /// Screen position (physical px) of image pixel (0,0)'s top-left corner.
    origin: [f64; 2],
    zoom: f64,
}

impl Rendered {
    /// Screen pixel at the centre of image pixel (x, y).
    fn image_px(&self, x: u32, y: u32) -> [u8; 3] {
        let sx = self.origin[0] + (x as f64 + 0.5) * self.zoom;
        let sy = self.origin[1] + (y as f64 + 0.5) * self.zoom;
        self.screen_px(sx as u32, sy as u32)
    }

    fn screen_px(&self, x: u32, y: u32) -> [u8; 3] {
        let p = self.image.get_pixel(x, y);
        [p[0], p[1], p[2]]
    }
}

fn render(zoom: f64, grid: bool) -> Option<Rendered> {
    let mut h = Harness::builder()
        .with_size(vec2(640.0, 480.0))
        .wgpu()
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.state_mut().workspace_mut().add(test_doc(), None, None);
    h.step();
    let tab = h.state_mut().workspace_mut().active_mut().unwrap();
    tab.view = Some(View {
        zoom,
        center: [32.0, 24.0],
        fit_to_window: false,
    });
    let ctx = h.ctx.clone();
    if h.state().diagnostics().visible() {
        // The debug overlay would cover canvas pixels.
        h.state_mut().execute(
            &ctx,
            Triggered {
                command: Command::ToggleDiagnostics,
                taps: 1,
            },
        );
    }
    if grid {
        let toggle = Triggered {
            command: Command::TogglePixelGrid,
            taps: 1,
        };
        h.state_mut().execute(&ctx, toggle);
    }
    h.step();
    h.step();
    let image = match h.render() {
        Ok(image) => image,
        Err(e) => {
            eprintln!("skipping: no wgpu adapter for offscreen rendering ({e})");
            return None;
        }
    };
    if let Ok(dir) = std::env::var("PNN_RENDER_DUMP") {
        image
            .save(format!("{dir}/canvas-zoom{zoom}-grid{grid}.png"))
            .unwrap();
    }
    let ppp = h.ctx.pixels_per_point() as f64;
    let rect = h.state().canvas().rect.expect("canvas laid out");
    let vp = [rect.width() as f64 * ppp, rect.height() as f64 * ppp];
    let view = h.state().workspace().active().unwrap().view.unwrap();
    let o = view.image_to_screen([0.0, 0.0], vp);
    Some(Rendered {
        image,
        origin: [
            rect.min.x as f64 * ppp + o[0],
            rect.min.y as f64 * ppp + o[1],
        ],
        zoom,
    })
}

fn checker(screen_rel_x: u32, screen_rel_y: u32) -> u8 {
    if ((screen_rel_x ^ screen_rel_y) & 8) != 0 {
        255
    } else {
        191
    }
}

#[test]
fn zoomed_in_pixels_and_checkerboard() {
    let Some(r) = render(4.0, false) else { return };
    assert_eq!(
        r.image_px(10, 10),
        [220, 40, 40],
        "opaque pixel shown exactly"
    );
    // Transparent pixel: checkerboard value at that screen position.
    for (x, y) in [(0, 0), (2, 0), (3, 3), (60, 2)] {
        let sx = (x as f64 + 0.5) * r.zoom;
        let sy = (y as f64 + 0.5) * r.zoom;
        let v = checker(sx as u32, sy as u32);
        assert_eq!(
            r.image_px(x, y),
            [v, v, v],
            "checkerboard at image ({x},{y})"
        );
    }
    // Half-transparent blue over the checkerboard, blended as paint.net does (±1 rounding).
    let sx = (50.5 * r.zoom) as u32;
    let sy = (40.5 * r.zoom) as u32;
    let v = checker(sx, sy) as f64;
    let got = r.image_px(50, 40);
    let expected = [
        v * (127.0 / 255.0),
        v * (127.0 / 255.0),
        255.0 * (128.0 / 255.0) + v * (127.0 / 255.0),
    ];
    for ch in 0..3 {
        assert!(
            (got[ch] as f64 - expected[ch]).abs() <= 1.5,
            "channel {ch}: got {got:?}, expected {expected:?}"
        );
    }
}

#[test]
fn pixel_grid_draws_black_dots_on_pixel_edges() {
    let Some(r) = render(8.0, true) else { return };
    // Top edge of image pixel (10,10): screen row at its top, every other screen pixel black.
    let row = (r.origin[1] + 10.0 * r.zoom) as u32;
    let x0 = (r.origin[0] + 10.0 * r.zoom) as u32;
    let mut black = 0;
    for x in x0 + 1..x0 + 7 {
        let rel = x - r.origin[0] as u32;
        let p = r.screen_px(x, row);
        if rel.is_multiple_of(2) {
            assert_eq!(p, [0, 0, 0], "grid dot at screen x {x}");
            black += 1;
        } else {
            assert_eq!(p, [220, 40, 40], "gap in dotted grid at screen x {x}");
        }
    }
    assert_eq!(black, 3);
    // Interior of the pixel is untouched.
    assert_eq!(r.image_px(10, 10), [220, 40, 40]);
}

#[test]
fn no_grid_below_200_percent() {
    let Some(r) = render(1.0, true) else { return };
    assert_eq!(r.image_px(10, 10), [220, 40, 40]);
    assert_eq!(r.image_px(11, 10), [220, 40, 40]);
}
