// Frosted glass behind panels and dialogs (design A).
//
// The canvas is drawn a second time at a quarter of its size into the backdrop, blurred with
// separable Gaussian passes (`fs_blur`), and every glass shape samples that blur under itself
// (`fs_glass`): a rounded rectangle, a touch more saturated, then mixed with the panel's tint
// (more of it the further the backdrop's brightness is from the tint's, to keep text readable).

// One parameter block for both passes; blurring uses only `rect.xy`.
struct Params {
    rect: vec4<f32>,        // glass: min.xy, max.xy in framebuffer px; blur: step between
                            // taps in backdrop texels (x or y only) in .xy
    area: vec4<f32>,        // backdrop area: min.xy, size.xy in framebuffer px
    tint: vec4<f32>,        // sRGB rgb + opacity of the tint over the blur
    radius: f32,            // corner radius, px
    saturation: f32,        // 1 = unchanged
    srgb_target: f32,       // 1 = framebuffer is sRGB, so work in linear values
    _pad: f32,
};

@group(0) @binding(0) var<uniform> glass: Params;
@group(0) @binding(1) var source: texture_2d<f32>;
@group(0) @binding(2) var linear_sampler: sampler;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle covering the whole viewport.
@vertex
fn vs_fullscreen(@builtin(vertex_index) index: u32) -> VsOut {
    let p = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VsOut;
    out.position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2(p.x, 1.0 - p.y);
    return out;
}

// 9-tap Gaussian (sigma ≈ 2 taps) folded into 5 bilinear samples.
@fragment
fn fs_blur(in: VsOut) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(source));
    let d = glass.rect.xy * texel;
    var c = textureSample(source, linear_sampler, in.uv) * 0.2270270270;
    c += textureSample(source, linear_sampler, in.uv + d * 1.3846153846) * 0.3162162162;
    c += textureSample(source, linear_sampler, in.uv - d * 1.3846153846) * 0.3162162162;
    c += textureSample(source, linear_sampler, in.uv + d * 3.2307692308) * 0.0702702703;
    c += textureSample(source, linear_sampler, in.uv - d * 3.2307692308) * 0.0702702703;
    return vec4(c.rgb, 1.0);
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

// Signed distance from `p` to a rounded rectangle (negative inside).
fn rounded_rect(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, r: f32) -> f32 {
    let center = (lo + hi) * 0.5;
    let half = (hi - lo) * 0.5;
    let q = abs(p - center) - half + vec2(r);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

// How much more tint goes over a backdrop whose brightness differs from the tint's, so
// panel text stays readable over white canvases (dark theme) and black ones (light theme).
const CONTRAST_BOOST: f32 = 0.85;

@fragment
fn fs_glass(in: VsOut) -> @location(0) vec4<f32> {
    let p = in.position.xy;
    let coverage = clamp(0.5 - rounded_rect(p, glass.rect.xy, glass.rect.zw, glass.radius), 0.0, 1.0);
    let uv = (p - glass.area.xy) / glass.area.zw;
    // Mix in sRGB, as CSS does: linear-light mixing lets bright backdrops dominate.
    var c = textureSample(source, linear_sampler, uv).rgb;
    if (glass.srgb_target > 0.5) {
        c = linear_to_srgb(c);
    }
    let weights = vec3(0.2126, 0.7152, 0.0722);
    let luma = dot(c, weights);
    c = clamp(mix(vec3(luma), c, glass.saturation), vec3(0.0), vec3(1.0));
    let tint = glass.tint.rgb;
    let diff = abs(luma - dot(tint, weights));
    let amount = glass.tint.a + (1.0 - glass.tint.a) * CONTRAST_BOOST * diff;
    c = mix(c, tint, amount);
    if (glass.srgb_target > 0.5) {
        c = srgb_to_linear(c);
    }
    // Premultiplied, as egui blends.
    return vec4(c * coverage, coverage);
}
