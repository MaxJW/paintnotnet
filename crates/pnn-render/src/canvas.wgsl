// Canvas display: composite texture over paint.net's checkerboard, plus the pixel grid.
//
// Checkerboard and grid follow paint.net 3.36 `Core/SurfaceBoxBaseRenderer.cs` and
// `Core/SurfaceBoxGridRenderer.cs` (MIT, see NOTICE):
// - checker: 8 px squares, grey 191 / white 255, in scaled-canvas coordinates
//   (fixed size on screen, scrolls with the image).
// - grid (zoom >= 200%): black dots on every other screen pixel along the first screen
//   row/column of each image pixel.

struct View {
    origin: vec2<f32>,      // screen px (within the callback viewport) of image (0,0)
    zoom: f32,              // screen px per image px
    ppp: f32,               // pixels per point
    viewport: vec2<f32>,    // callback viewport size in px
    image_size: vec2<f32>,
    grid: f32,              // 1.0 = pixel grid enabled
    srgb_target: f32,       // 1.0 = framebuffer is sRGB, so output linear values
    overlay: f32,           // 1.0 = tint outside the selection (feather preview)
    ants: f32,              // 0 = off; else 1 + dash offset: marching ants from the mask
};

struct Chunk {
    origin: vec2<f32>,      // image px
    size: vec2<f32>,        // image px
};

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var nearest_sampler: sampler;
@group(0) @binding(2) var linear_sampler: sampler;
@group(1) @binding(0) var<uniform> chunk: Chunk;
@group(1) @binding(1) var chunk_tex: texture_2d<f32>;
@group(1) @binding(2) var overlay_tex: texture_2d<f32>;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) img: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let uv = corners[index];
    let img = chunk.origin + uv * chunk.size;
    let screen = view.origin + img * view.zoom;
    let ndc = vec2(screen.x / view.viewport.x * 2.0 - 1.0, 1.0 - screen.y / view.viewport.y * 2.0);
    var out: VsOut;
    out.position = vec4(ndc, 0.0, 1.0);
    out.uv = uv;
    out.img = img;
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// Selected (coverage at least half) at image point `img`. Outside the image is unselected;
// beyond this chunk (a neighbour across a chunk seam) counts as `fallback`.
fn selected_at(img: vec2<f32>, fallback: bool) -> bool {
    if (any(img < vec2(0.0)) || any(img >= view.image_size)) {
        return false;
    }
    let local = img - chunk.origin;
    if (any(local < vec2(0.0)) || any(local >= chunk.size)) {
        return fallback;
    }
    return textureLoad(overlay_tex, vec2<i32>(floor(local)), 0).r >= 0.5;
}

// On the inside edge of the selection: selected, with a neighbour one screen pixel away
// that isn't. Works at any zoom, whatever the selection's complexity.
fn ants_edge(img: vec2<f32>) -> bool {
    if (!selected_at(img, true)) {
        return false;
    }
    let d = 1.0 / view.zoom;
    return !selected_at(img + vec2(d, 0.0), true) || !selected_at(img - vec2(d, 0.0), true)
        || !selected_at(img + vec2(0.0, d), true) || !selected_at(img - vec2(0.0, d), true);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // Both samples in uniform control flow; nearest when zoomed in (crisp pixels, as
    // paint.net), trilinear over premultiplied mips when zoomed out.
    let near = textureSample(chunk_tex, nearest_sampler, in.uv);
    let lin = textureSample(chunk_tex, linear_sampler, in.uv);
    let c = select(lin, near, view.zoom >= 1.0);
    let coverage = textureSample(overlay_tex, linear_sampler, in.uv).r;

    // Screen pixel relative to the image origin.
    let scr = vec2<i32>(floor(in.img * view.zoom));
    let checker_px = vec2<i32>(floor(in.img * view.zoom / view.ppp));
    let v = select(191.0, 255.0, ((checker_px.x ^ checker_px.y) & 8) != 0) / 255.0;
    var rgb = c.rgb + vec3(v) * (1.0 - c.a);

    if (view.grid > 0.5 && view.zoom >= 2.0) {
        let within = fract(in.img) * view.zoom; // screen px into the current image pixel
        let on_h = within.y < 1.0 && (scr.x & 1) == 0;
        let on_v = within.x < 1.0 && (scr.y & 1) == 0;
        if (on_h || on_v) {
            rgb = vec3(0.0);
        }
    }

    if (view.overlay > 0.5) {
        // Quick-mask style: unselected areas tinted red, fading with the feather.
        let tint = vec3(0.85, 0.12, 0.12);
        rgb = mix(rgb, tint, (1.0 - coverage) * 0.5);
    }

    if (view.ants > 0.5 && ants_edge(in.img)) {
        // paint.net's ants: white with black dashes, marching. Dashes run diagonally in screen
        // space, so they march along horizontal and vertical edges alike.
        let p = vec2<i32>(floor(in.position.xy));
        let phase = (p.x + p.y + i32(view.ants - 1.0)) & 7;
        rgb = select(vec3(1.0), vec3(0.0), phase < 4);
    }

    if (view.srgb_target > 0.5) {
        rgb = srgb_to_linear(rgb);
    }
    return vec4(rgb, 1.0);
}
