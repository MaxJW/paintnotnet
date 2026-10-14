// Effects on the GPU: exact ports of the CPU renderers in `pnn-effects` (3.36's fixed-point
// maths), so a GPU preview matches what the CPU would render. Pixels are B, G, R, A bytes
// packed little-endian in u32s; the output covers the effect's bounds only.

struct Params {
    w: u32,          // source size
    h: u32,
    bx: u32,         // the band rendered (bw × bh from bx, by)
    by: u32,
    bw: u32,
    bh: u32,
    oy: u32,         // the band's first row in the output (bw wide)
    n: i32,          // effect-specific
    a: i32,
    b: i32,
    c: i32,
    _pad: u32,
};

@group(0) @binding(0) var<storage, read> src: array<u32>;
@group(0) @binding(1) var<storage, read_write> dst: array<u32>;
@group(0) @binding(2) var<uniform> p: Params;
@group(0) @binding(3) var<storage, read> points: array<vec2<f32>>;

fn unpack(v: u32) -> vec4<u32> {
    // (b, g, r, a)
    return vec4<u32>(v & 0xffu, (v >> 8u) & 0xffu, (v >> 16u) & 0xffu, v >> 24u);
}

fn pack(c: vec4<u32>) -> u32 {
    return (c.x & 0xffu) | ((c.y & 0xffu) << 8u) | ((c.z & 0xffu) << 16u) | (c.w << 24u);
}

fn texel(x: u32, y: u32) -> vec4<u32> {
    return unpack(src[y * p.w + x]);
}

// 3.36 `Surface.GetBilinearSample` + `BlendColors4W16IP` (blurs.rs `bilinear`).
fn bilinear(x: f32, y: f32) -> vec4<u32> {
    let w = f32(p.w);
    let h = f32(p.h);
    if (x < 0.0 || y < 0.0 || x >= w || y >= h) {
        return vec4<u32>(0u);
    }
    let iu = u32(floor(x));
    let iv = u32(floor(y));
    let fx = u32(256.0 * (x - f32(iu)));
    let fy = u32(256.0 * (y - f32(iv)));
    let ix = 256u - fx;
    let iy = 256u - fy;
    let right = select(iu + 1u, iu, iu == p.w - 1u);
    let bottom = select(iv + 1u, iv, iv == p.h - 1u);
    let c0 = texel(iu, iv);
    let c1 = texel(right, iv);
    let c2 = texel(iu, bottom);
    let c3 = texel(right, bottom);
    let w0 = ix * iy;
    let w1 = fx * iy;
    let w2 = ix * fy;
    let w3 = fx * fy;
    let af = c0.w * w0 + c1.w * w1 + c2.w * w2 + c3.w * w3;
    let a = (af + 32768u) >> 16u;
    if (a == 0u) {
        return vec4<u32>(0u);
    }
    // Sums of a·c·w fit u32: at most 255·255·65536.
    let b = (c0.w * c0.x * w0 + c1.w * c1.x * w1 + c2.w * c2.x * w2 + c3.w * c3.x * w3) / af;
    let g = (c0.w * c0.y * w0 + c1.w * c1.y * w1 + c2.w * c2.y * w2 + c3.w * c3.y * w3) / af;
    let r = (c0.w * c0.z * w0 + c1.w * c1.z * w1 + c2.w * c2.z * w2 + c3.w * c3.z * w3) / af;
    return vec4<u32>(b, g, r, a);
}

// The output pixel this invocation writes, or false outside the bounds.
fn in_bounds(id: vec3<u32>) -> bool {
    return id.x < p.bw && id.y < p.bh;
}

fn store(id: vec3<u32>, c: vec4<u32>) {
    dst[(p.oy + id.y) * p.bw + id.x] = pack(c);
}

// Motion Blur: the alpha-weighted average (`blend`) of bilinear samples along a line.
@compute @workgroup_size(16, 16)
fn motion(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_bounds(id)) {
        return;
    }
    let x = f32(p.bx + id.x);
    let y = f32(p.by + id.y);
    let wl = f32(p.w - 1u);
    let hl = f32(p.h - 1u);
    var count = 0u;
    var a_sum = 0u;
    var num = vec3<u32>(0u);
    for (var i = 0; i < p.n; i++) {
        let q = points[i];
        let px = q.x + x;
        let py = q.y + y;
        if (px >= 0.0 && py >= 0.0 && px <= wl && py <= hl) {
            let c = bilinear(px, py);
            count += 1u;
            a_sum += c.w;
            num += c.w * c.xyz;
        }
    }
    if (count == 0u) {
        store(id, vec4<u32>(0u));
        return;
    }
    let a = a_sum / count;
    if (a_sum == 0u) {
        store(id, vec4<u32>(0u, 0u, 0u, a));
        return;
    }
    store(id, vec4<u32>(num / a_sum, a));
}

// (q · m) >> 10 exactly, where the 64-bit product may not fit i32 but the result does
// (the CPU's i64 `((fx >> 4) * amount) >> 10`).
fn mul_shr10(q: i32, m: i32) -> i32 {
    let hi = (q >> 16) * m;                 // signed high part
    let lo = u32(q & 0xffff) * u32(m);      // non-negative low part
    return hi * 64 + i32(lo >> 10u);
}

fn add_alpha_weighted(acc: ptr<function, vec4<i32>>, count: ptr<function, i32>, c: vec4<u32>) {
    let a = i32(c.w);
    (*acc) += vec4<i32>(i32(c.z) * a, i32(c.y) * a, i32(c.x) * a, a);
    (*count) += 1;
}

fn finish(id: vec3<u32>, acc: vec4<i32>, count: i32) {
    // acc = (Σ r·a, Σ g·a, Σ b·a, Σ a)
    if (acc.w == 0) {
        store(id, vec4<u32>(0u));
        return;
    }
    let r = u32(clamp(acc.x / acc.w, 0, 255));
    let g = u32(clamp(acc.y / acc.w, 0, 255));
    let b = u32(clamp(acc.z / acc.w, 0, 255));
    let a = u32(clamp(acc.w / count, 0, 255));
    store(id, vec4<u32>(b, g, r, a));
}

// Zoom Blur (3.36, fixed point): a = amount, b = fcx, c = fcy.
@compute @workgroup_size(16, 16)
fn zoom(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_bounds(id)) {
        return;
    }
    let w = i32(p.w);
    let h = i32(p.h);
    let x = i32(p.bx + id.x);
    let y = i32(p.by + id.y);
    var acc = vec4<i32>(0);
    var count = 0;
    add_alpha_weighted(&acc, &count, texel(u32(x), u32(y)));
    var fx = (x << 16u) - p.b;
    var fy = (y << 16u) - p.c;
    for (var i = 0; i < 64; i++) {
        fx -= mul_shr10(fx >> 4u, p.a);
        fy -= mul_shr10(fy >> 4u, p.a);
        let u = (fx + p.b + 32768) >> 16u;
        let v = (fy + p.c + 32768) >> 16u;
        if (u >= 0 && v >= 0 && u < w && v < h) {
            add_alpha_weighted(&acc, &count, texel(u32(u), u32(v)));
        }
    }
    finish(id, acc, count);
}

// 3.36 Radial Blur's fixed-point rotation step (wrapping i32, as `rotate`).
fn rotate(f: vec2<i32>, fr: i32) -> vec2<i32> {
    let cx = f.x;
    let cy = f.y;
    let fr2 = (fr * fr) >> 11u;
    return vec2<i32>(
        cx - (((cy >> 8u) * fr) >> 8u) - (((cx >> 14u) * fr2) >> 8u),
        cy + (((cx >> 8u) * fr) >> 8u) - (((cy >> 14u) * fr2) >> 8u),
    );
}

// Radial Blur: n = steps, a = fsr (step angle), b = fcx, c = fcy.
@compute @workgroup_size(16, 16)
fn radial(@builtin(global_invocation_id) id: vec3<u32>) {
    if (!in_bounds(id)) {
        return;
    }
    let w = i32(p.w);
    let h = i32(p.h);
    let x = i32(p.bx + id.x);
    let y = i32(p.by + id.y);
    var acc = vec4<i32>(0);
    var count = 0;
    add_alpha_weighted(&acc, &count, texel(u32(x), u32(y)));
    let f = vec2<i32>((x << 16u) - p.b, (y << 16u) - p.c);
    var o1 = f;
    var o2 = f;
    for (var i = 0; i < p.n; i++) {
        o1 = rotate(o1, p.a);
        o2 = rotate(o2, -p.a);
        let u1 = (o1.x + p.b + 32768) >> 16u;
        let v1 = (o1.y + p.c + 32768) >> 16u;
        if (u1 > 0 && v1 > 0 && u1 < w && v1 < h) {
            add_alpha_weighted(&acc, &count, texel(u32(u1), u32(v1)));
        }
        let u2 = (o2.x + p.b + 32768) >> 16u;
        let v2 = (o2.y + p.c + 32768) >> 16u;
        if (u2 > 0 && v2 > 0 && u2 < w && v2 < h) {
            add_alpha_weighted(&acc, &count, texel(u32(u2), u32(v2)));
        }
    }
    finish(id, acc, count);
}
