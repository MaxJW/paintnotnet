// Layers composited on the GPU: the same integer maths as `pnn-core`'s blend ops (paint.net
// 3.36 `UserBlendOps`), so the screen shows exactly what flattening gives. Pixels are B, G, R,
// A bytes packed little-endian in u32s. A chunk's buffers hold its tiles one after another
// (256 × 256 each); `slots` lists the tiles this dispatch touches.

struct Params {
    op: u32,         // 0 clear, 1 copy the layer, 2 blend the layer; +4: the layer is the
                     // live gradient drawn over it (see `gradient_pixel`); +8: the layer
                     // with floating pixels drawn over it (see `floating_over`)
    mode: u32,       // BlendMode, in paint.net's order
    opacity: u32,
    chunk_w: u32,    // the chunk's width in tiles, and its first tile
    tx0: u32,
    ty0: u32,
    _pad0: u32,
    _pad1: u32,
};

// The live gradient: `pnn_tools::gradient::GradientShader`.
struct Gradient {
    kind: u32,
    repeat: u32,
    flags: u32,      // 1 solid, 2 alpha only, 4 blended (else overwrite), 8 selection
    op_mode: u32,
    start: vec2<f32>,
    dtdx: f32,
    dtdy: f32,
    inv_len: f32,
    offset: f32,
    start_a: f32,
    end_a: f32,
    start_color: u32,
    end_color: u32,
    solid_color: u32,
    mask_absent: u32,
};

@group(0) @binding(0) var<storage, read_write> acc: array<u32>;
@group(0) @binding(1) var<storage, read> layer: array<u32>;
// Tile slots; for the gradient, bit 31 = the layer's tile is absent, bit 30 = the
// selection's tile is absent.
@group(0) @binding(2) var<storage, read> slots: array<u32>;
@group(0) @binding(3) var<uniform> p: Params;
@group(0) @binding(4) var<uniform> g: Gradient;
// Selection coverage, 4 bytes per u32, laid out as the layer.
@group(0) @binding(5) var<storage, read> mask: array<u32>;
// Floating pixels being moved (Move Selected Pixels): their rectangle, row by row.
@group(0) @binding(6) var<storage, read> floating: array<u32>;
@group(0) @binding(7) var<uniform> fl: Floating;

struct Floating {
    origin: vec2<i32>,  // where the rectangle was lifted from (image px)
    size: vec2<i32>,
    offset: vec2<i32>,  // how far it has moved
    image: vec2<i32>,   // the image's size: nothing lands beyond it
};

const TILE: u32 = 256u;

fn unpack(v: u32) -> vec4<u32> {
    return vec4<u32>(v & 0xffu, (v >> 8u) & 0xffu, (v >> 16u) & 0xffu, v >> 24u);
}

fn pack(c: vec4<u32>) -> u32 {
    return c.x | (c.y << 8u) | (c.z << 16u) | (c.w << 24u);
}

// INT_SCALE: multiply two 0..=255 values as if 0..=1, with paint.net's rounding.
fn int_scale(a: u32, b: u32) -> u32 {
    let r = a * b + 0x80u;
    return ((r >> 8u) + r) >> 8u;
}

fn reflect_f(a: u32, b: u32) -> u32 {
    if (b == 255u) {
        return 255u;
    }
    return min(a * a / (255u - b), 255u);
}

// F(A, B): A = lower (accumulated), B = the layer being applied.
fn f(mode: u32, a: u32, b: u32) -> u32 {
    switch mode {
        case 0u: { return b; }                                  // Normal
        case 1u: { return int_scale(a, b); }                    // Multiply
        case 2u: { return min(a + b, 255u); }                   // Additive
        case 3u: {                                              // Color Burn
            if (b == 0u) {
                return 0u;
            }
            let q = (255u - a) * 255u / b;
            return select(0u, 255u - q, q < 255u);
        }
        case 4u: {                                              // Color Dodge
            if (b == 255u) {
                return 255u;
            }
            return min(a * 255u / (255u - b), 255u);
        }
        case 5u: { return reflect_f(a, b); }                    // Reflect
        case 6u: { return reflect_f(b, a); }                    // Glow
        case 7u: {                                              // Overlay
            if (a < 128u) {
                return int_scale(2u * a, b);
            }
            return 255u - int_scale(2u * (255u - a), 255u - b);
        }
        case 8u: { return max(a, b) - min(a, b); }              // Difference
        case 9u: { return 255u - u32(abs(255 - i32(a) - i32(b))); } // Negation
        case 10u: { return max(a, b); }                         // Lighten
        case 11u: { return min(a, b); }                         // Darken
        case 12u: { return b + a - int_scale(b, a); }           // Screen
        default: { return a ^ b; }                              // Xor
    }
}

// `BLEND(lhs, rhs, F)`.
fn blend(lhs: vec4<u32>, rhs: vec4<u32>, mode: u32) -> vec4<u32> {
    let y = int_scale(lhs.w, 255u - rhs.w);
    let total = y + rhs.w;
    if (total == 0u) {
        return vec4<u32>(0u);
    }
    let x = int_scale(lhs.w, rhs.w);
    let z = rhs.w - x;
    var out = vec4<u32>(0u, 0u, 0u, total);
    for (var k = 0u; k < 3u; k++) {
        let l = lhs[k];
        let r = rhs[k];
        out[k] = (l * y + r * z + f(mode, l, r) * x) / total;
    }
    return out;
}

// `pnn_core::color::lerp_premultiplied`.
fn lerp_premultiplied(a: vec4<u32>, b: vec4<u32>, t: u32) -> vec4<u32> {
    let out_a = (a.w * (255u - t) + b.w * t) / 255u;
    if (out_a == 0u) {
        return vec4<u32>(0u);
    }
    var out = vec4<u32>(0u, 0u, 0u, out_a);
    for (var k = 0u; k < 3u; k++) {
        out[k] = min((a[k] * a.w * (255u - t) + b[k] * b.w * t) / (out_a * 255u), 255u);
    }
    return out;
}

const PI: f32 = 3.14159265358979;

fn mirror(u: f32) -> f32 {
    let m = u - 2.0 * floor(u / 2.0);
    return select(m, 2.0 - m, m > 1.0);
}

// `Lerp::at` (pnn-tools gradient.rs): position along the gradient.
fn lerp_at(x: f32, y: f32) -> f32 {
    let dx = x - g.start.x;
    let dy = y - g.start.y;
    var u = 0.0;
    switch g.kind {
        case 0u: { u = dx * g.dtdx + dy * g.dtdy; }
        case 1u: { u = abs(dx * g.dtdx + dy * g.dtdy); }
        case 2u: {
            u = abs(dx * g.dtdx + dy * g.dtdy) + abs(dx * g.dtdy - dy * g.dtdx);
        }
        case 3u: { u = sqrt(dx * dx + dy * dy) * g.inv_len; }
        case 4u: {
            var t = atan2(dy, dx) / PI + g.offset;
            if (t > 1.0) {
                t -= 2.0;
            } else if (t < -1.0) {
                t += 2.0;
            }
            return clamp(abs(t), 0.0, 1.0);
        }
        default: {
            var a = (atan2(dy, dx) - g.offset) / (2.0 * PI);
            if (g.kind == 6u) {
                a = -a;
            }
            let v = a + sqrt(dx * dx + dy * dy) * g.inv_len;
            if (g.repeat == 2u) {
                return mirror(v);
            }
            return v - floor(v);
        }
    }
    switch g.repeat {
        case 1u: { return u - floor(u); }
        case 2u: { return mirror(u); }
        default: { return clamp(u, 0.0, 1.0); }
    }
}

fn dither(x: u32, y: u32) -> f32 {
    let v = 0.06711056 * f32(x) + 0.00583715 * f32(y);
    return fract(52.982918 * fract(v)) - 0.5;
}

fn quantize(v: f32, d: f32) -> u32 {
    return u32(clamp(v + d + 0.5, 0.0, 255.0));
}

// `color_mix`: (b, g, r, a) at t, unrounded.
fn color_mix(ca: vec4<u32>, cb: vec4<u32>, t: f32) -> vec4<f32> {
    let ca_a = (1.0 - t) * f32(ca.w);
    let cb_a = t * f32(cb.w);
    let total = ca_a + cb_a;
    if (total <= 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>((vec3<f32>(ca.xyz) * ca_a + vec3<f32>(cb.xyz) * cb_a) / total, total);
}

// `Gradient::render` for one pixel over `before` with coverage `cov`.
fn gradient_pixel(x: u32, y: u32, before: vec4<u32>, cov: u32) -> vec4<u32> {
    if (cov == 0u) {
        return before;
    }
    let solid = (g.flags & 1u) != 0u;
    let blended = (g.flags & 4u) != 0u;
    var t = 1.0;
    if (!solid) {
        t = clamp(lerp_at(f32(x), f32(y)), 0.0, 1.0);
    }
    let d = dither(x, y);
    var px: vec4<u32>;
    if ((g.flags & 2u) != 0u) {
        var a = u32(g.end_a);
        if (!solid) {
            a = quantize(g.start_a + (g.end_a - g.start_a) * t, d);
        }
        px = vec4<u32>(before.xyz, select(a, int_scale(before.w, a), blended));
    } else {
        var src = unpack(g.solid_color);
        if (!solid) {
            let step = u32(t * 4095.0 + 0.5);
            let c = color_mix(unpack(g.start_color), unpack(g.end_color), f32(step) / 4095.0);
            src = vec4<u32>(quantize(c.x, d), quantize(c.y, d), quantize(c.z, d), quantize(c.w, d));
        }
        px = select(src, blend(before, src, g.op_mode), blended);
    }
    if (cov != 255u) {
        px = lerp_premultiplied(before, px, cov);
    }
    return px;
}

// The floating pixels over `before` at image pixel (x, y): `transform::draw_transformed` for
// a whole-pixel move (Normal over what's below).
fn floating_over(x: u32, y: u32, before: vec4<u32>) -> vec4<u32> {
    let at = vec2<i32>(i32(x), i32(y));
    let q = at - fl.offset - fl.origin;
    if (any(q < vec2<i32>(0)) || any(q >= fl.size) || any(at >= fl.image)) {
        return before;
    }
    let px = unpack(floating[u32(q.y * fl.size.x + q.x)]);
    if (px.w == 0u) {
        return before;
    }
    return blend(before, px, 0u);
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let entry = slots[id.z];
    let slot = entry & 0x3fffffffu;
    let i = slot * TILE * TILE + id.y * TILE + id.x;
    var src: vec4<u32>;
    if ((p.op & 8u) != 0u) {
        var before = vec4<u32>(0u);
        if ((entry & 0x80000000u) == 0u) {
            before = unpack(layer[i]);
        }
        let x = (p.tx0 + slot % p.chunk_w) * TILE + id.x;
        let y = (p.ty0 + slot / p.chunk_w) * TILE + id.y;
        src = floating_over(x, y, before);
        // An absent layer tile with nothing floating here: as the CPU, leave what's below
        // exactly as it is (blending nothing would clear transparent pixels' colour).
        if ((entry & 0x80000000u) != 0u && src.w == 0u && (p.op & 3u) != 1u) {
            return;
        }
    } else if (p.op >= 4u) {
        var before = vec4<u32>(0u);
        if ((entry & 0x80000000u) == 0u) {
            before = unpack(layer[i]);
        }
        var cov = 255u;
        if ((g.flags & 8u) != 0u) {
            cov = g.mask_absent;
            if ((entry & 0x40000000u) == 0u) {
                cov = (mask[i / 4u] >> ((i % 4u) * 8u)) & 0xffu;
            }
        }
        let x = (p.tx0 + slot % p.chunk_w) * TILE + id.x;
        let y = (p.ty0 + slot / p.chunk_w) * TILE + id.y;
        src = gradient_pixel(x, y, before, cov);
    } else if (p.op != 0u) {
        src = unpack(layer[i]);
    }
    switch p.op & 3u {
        case 0u: {
            acc[i] = 0u;
        }
        case 1u: {
            acc[i] = pack(src);
        }
        default: {
            if (p.opacity != 255u) {
                src.w = int_scale(src.w, p.opacity);
            }
            acc[i] = pack(blend(unpack(acc[i]), src, p.mode));
        }
    }
}
