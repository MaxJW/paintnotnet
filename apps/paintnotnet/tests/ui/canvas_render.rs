//! Renders the real app offscreen through wgpu and checks canvas pixels against paint.net's
//! display rules: image pixels shown nearest-neighbour when zoomed in, transparency over the
//! 191/255 checkerboard, and the black dotted pixel grid at >= 200%.
//!
//! Set `PNN_RENDER_DUMP=<dir>` to also write the rendered frame there as a PNG.

use eframe::egui::vec2;
use egui_kittest::Harness;
use paintnotnet::app::PaintNotNetApp;
use paintnotnet::commands::{Command, Panel, Triggered};
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

/// The app showing `test_doc()` at `zoom`, nothing covering the canvas.
fn setup(zoom: f64, grid: bool) -> Harness<'static, PaintNotNetApp> {
    let mut h = Harness::builder()
        // Wide and tall enough that the options bar stays on two rows above the image.
        .with_size(vec2(960.0, 640.0))
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
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleDiagnostics));
    }
    // Floating windows would cover canvas pixels too.
    for panel in Panel::ALL {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::ToggleWindow(panel)));
    }
    if grid {
        h.state_mut()
            .execute(&ctx, Triggered::once(Command::TogglePixelGrid));
    }
    h.step();
    h.step();
    h
}

fn render(zoom: f64, grid: bool) -> Option<Rendered> {
    let mut h = setup(zoom, grid);
    capture(&mut h, zoom, grid)
}

fn capture(h: &mut Harness<'static, PaintNotNetApp>, zoom: f64, grid: bool) -> Option<Rendered> {
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

/// While a gradient's nub is dragged, the GPU draws it and the layer is untouched; on
/// release it is rendered into the layer, and the screen still shows the same.
#[test]
fn live_gradient_draws_on_the_gpu_then_lands_in_the_layer() {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
    let zoom = 4.0;
    let mut h = setup(zoom, false);
    {
        let s = h.state_mut().settings_mut();
        s.primary = ColorBgra::from_rgba(250, 200, 40, 255);
        s.secondary = ColorBgra::from_rgba(40, 60, 160, 255);
        s.gradient_type = pnn_tools::GradientType::Linear;
    }
    h.state_mut().set_tool(pnn_tools::ToolKind::Gradient);
    h.step();
    let to_screen = |h: &Harness<'_, PaintNotNetApp>, x: f64, y: f64| {
        let rect = h.state().canvas().rect.unwrap();
        let view = h.state().workspace().active().unwrap().view.unwrap();
        let s = view.image_to_screen([x, y], [rect.width() as f64, rect.height() as f64]);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    let press = |pressed, pos| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let start = to_screen(&h, 4.0, 24.0);
    h.event(Event::PointerMoved(start));
    h.step();
    h.event(press(true, start));
    h.step();
    for x in [20.0, 40.0, 60.0] {
        h.event(Event::PointerMoved(to_screen(&h, x, 24.0)));
        h.step();
    }
    // The canvas handles a move after the frame's gradient refresh: one more frame shows it.
    h.step();
    let layer_px = |h: &Harness<'_, PaintNotNetApp>, x, y| {
        h.state()
            .workspace()
            .active()
            .unwrap()
            .doc
            .layer(0)
            .surface
            .get(x, y)
    };
    // Mid-drag: the layer still holds the red block, the screen shows the gradient.
    assert_eq!(layer_px(&h, 10, 10), RED, "layer untouched while dragging");
    let Some(mid) = capture(&mut h, zoom, false) else {
        return;
    };
    assert_ne!(
        mid.image_px(10, 10),
        [220, 40, 40],
        "the GPU draws the gradient"
    );
    let near = |a: [u8; 3], b: ColorBgra| {
        a.iter()
            .zip([b.r, b.g, b.b])
            .all(|(&x, y)| x.abs_diff(y) <= 2)
    };
    // Release: the gradient is rendered into the layer, and the screen agrees with it.
    h.event(press(false, to_screen(&h, 60.0, 24.0)));
    h.step();
    h.step();
    let baked = layer_px(&h, 10, 10);
    assert_ne!(baked, RED, "the gradient is in the layer after release");
    let Some(after) = capture(&mut h, zoom, false) else {
        return;
    };
    for (x, y) in [(10, 10), (30, 20), (55, 40)] {
        let want = layer_px(&h, x, y);
        assert!(
            near(after.image_px(x, y), want),
            "({x},{y}) shows the layer"
        );
        // The GPU's drawing mid-drag matched the CPU's render (±2 levels).
        assert!(
            near(mid.image_px(x, y), want),
            "({x},{y}) mid-drag matched: {:?} vs {want:?}",
            mid.image_px(x, y)
        );
    }
}

/// On a large image the gradient is rendered into the layer on the effect threads after the
/// drag (the GPU shows it meanwhile), with exactly the pixels of rendering it at once.
#[test]
fn large_gradient_lands_from_the_background() {
    use eframe::egui::{Event, Modifiers, PointerButton, Pos2};
    let (w, hh) = (1500u32, 1000u32);
    let mut s = Surface::new_transparent(w, hh);
    s.fill(ColorBgra::from_rgba(220, 40, 40, 255));
    let mut h = Harness::builder()
        .with_size(vec2(960.0, 640.0))
        .wgpu()
        .build_eframe(|cc| PaintNotNetApp::new(cc, None, Vec::new()));
    h.state_mut().workspace_mut().add(
        Document::from_layers(w, hh, vec![Layer::new("Background", s.clone())]),
        None,
        None,
    );
    h.step();
    h.state_mut().set_tool(pnn_tools::ToolKind::Gradient);
    h.step();
    let to_screen = |h: &Harness<'_, PaintNotNetApp>, x: f64, y: f64| {
        let rect = h.state().canvas().rect.unwrap();
        let view = h.state().workspace().active().unwrap().view.unwrap();
        let s = view.image_to_screen([x, y], [rect.width() as f64, rect.height() as f64]);
        Pos2::new(rect.min.x + s[0] as f32, rect.min.y + s[1] as f32)
    };
    let press = |pressed, pos| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let start = to_screen(&h, 200.0, 500.0);
    h.event(Event::PointerMoved(start));
    h.step();
    h.event(press(true, start));
    h.step();
    for x in [600.0, 1000.0, 1300.0] {
        h.event(Event::PointerMoved(to_screen(&h, x, 500.0)));
        h.step();
    }
    h.event(press(false, to_screen(&h, 1300.0, 500.0)));
    h.step();
    let layer = |h: &Harness<'_, PaintNotNetApp>| {
        h.state()
            .workspace()
            .active()
            .unwrap()
            .doc
            .layer(0)
            .surface
            .to_pixels()
    };
    let before = s.to_pixels();
    let t = std::time::Instant::now();
    while layer(&h) == before {
        assert!(t.elapsed().as_secs() < 10, "the render never landed");
        std::thread::sleep(std::time::Duration::from_millis(5));
        h.step();
    }
    let (a, b) = h.state().live_gradient().expect("still editable");
    let st = h.state().settings();
    let g = pnn_tools::gradient::Gradient {
        kind: st.gradient_type,
        repeat: st.gradient_repeat,
        start: a,
        end: b,
        start_color: st.primary,
        end_color: st.secondary,
        alpha_only: false,
    };
    let op = match st.blend {
        pnn_tools::ToolBlend::Overwrite => pnn_tools::gradient::GradientOp::Overwrite,
        pnn_tools::ToolBlend::Blend(m) => pnn_tools::gradient::GradientOp::Blend(m),
    };
    let mut want = s.clone();
    let all = pnn_core::Rect {
        x: 0,
        y: 0,
        width: w,
        height: hh,
    };
    g.render(&mut want, &s, None, all, op);
    assert!(
        layer(&h) == want.to_pixels(),
        "the background render matches"
    );
}
