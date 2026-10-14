// Composite tiles to display textures: premultiply each composited tile (`composite.wgsl`'s
// output) into the chunk's level 0, then build its mip chain with a 2×2 box filter,
// (sum + 2) / 4 per channel on premultiplied bytes, as `display_tile` did on the CPU.

struct TileOrigin {
    x: u32,          // texel origin in the chunk (level 0)
    y: u32,
    slot: u32,       // the tile's place in the composite buffer
    _pad: u32,
};

struct Level {
    k: u32,          // the mip level being written
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
};

// Level 0 pass.
@group(0) @binding(0) var<storage, read> raw: array<u32>;
@group(0) @binding(1) var<storage, read> tiles0: array<TileOrigin>;
@group(0) @binding(2) var level0: texture_storage_2d<rgba8unorm, write>;

// Mip passes.
@group(0) @binding(3) var parent: texture_2d<f32>;
@group(0) @binding(4) var<storage, read> tiles: array<TileOrigin>;
@group(0) @binding(5) var child: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(6) var<uniform> level: Level;

const TILE: u32 = 256u;

fn bytes(c: vec4<u32>) -> vec4<f32> {
    return vec4<f32>(c) / 255.0;
}

// One raw pixel (B, G, R, A bytes, little-endian u32) premultiplied, as RGBA bytes.
@compute @workgroup_size(16, 16)
fn premultiply(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = tiles0[id.z];
    let px = vec2<u32>(t.x + id.x, t.y + id.y);
    let dims = textureDimensions(level0);
    if (id.x >= TILE || id.y >= TILE || px.x >= dims.x || px.y >= dims.y) {
        return;
    }
    let v = raw[t.slot * TILE * TILE + id.y * TILE + id.x];
    let b = v & 0xffu;
    let g = (v >> 8u) & 0xffu;
    let r = (v >> 16u) & 0xffu;
    let a = v >> 24u;
    // (c·a + 127) / 255, as the CPU did.
    let c = vec4<u32>((r * a + 127u) / 255u, (g * a + 127u) / 255u, (b * a + 127u) / 255u, a);
    textureStore(level0, vec2<i32>(px), bytes(c));
}

@compute @workgroup_size(16, 16)
fn downsample(@builtin(global_invocation_id) id: vec3<u32>) {
    let t = tiles[id.z];
    let k = level.k;
    let stride = TILE >> k;
    if (id.x >= stride || id.y >= stride) {
        return;
    }
    let px = vec2<u32>(t.x >> k, t.y >> k) + id.xy;
    let dims = textureDimensions(child);
    if (px.x >= dims.x || px.y >= dims.y) {
        return;
    }
    let pdims = textureDimensions(parent);
    var sum = vec4<u32>(0u);
    for (var j = 0u; j < 2u; j++) {
        for (var i = 0u; i < 2u; i++) {
            let q = px * 2u + vec2<u32>(i, j);
            if (q.x < pdims.x && q.y < pdims.y) {
                sum += vec4<u32>(round(textureLoad(parent, vec2<i32>(q), 0) * 255.0));
            }
        }
    }
    textureStore(child, vec2<i32>(px), bytes((sum + 2u) / 4u));
}
