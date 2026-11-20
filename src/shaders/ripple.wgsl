// ============================================================================
// ripple.wgsl — puddle surface simulation (2D wave equation on a toroidal
// tile, 5 mm texels). Impacts from simulated drops arrive as atomic impulses;
// the many drops too small to simulate individually are injected here at the
// physical Marshall–Palmer flux rate.
// ============================================================================

struct RippleParams {
    res: u32,
    seed: u32,
    rate: f32,       // probability of a drop per 2x2 cell per step
    c2: f32,         // (c·dt/dx)^2
    damping: f32,    // per-step amplitude factor
    amp_scale: f32,  // impulse (µm) per mm^1.5 of drop
    lam: f32,        // Marshall–Palmer slope (1/mm)
    pad: f32,
}

@group(0) @binding(0) var<uniform> rp: RippleParams;
@group(0) @binding(1) var<storage, read> h_cur: array<f32>;
@group(0) @binding(2) var<storage, read_write> h_prev: array<f32>;
@group(0) @binding(3) var<storage, read_write> imp: array<atomic<i32>>;

fn idx(x: i32, y: i32) -> u32 {
    let r = i32(rp.res);
    return u32((((y % r) + r) % r) * r + (((x % r) + r) % r));
}

@compute @workgroup_size(16, 16)
fn cs_inject(@builtin(global_invocation_id) gid: vec3u) {
    let half_res = rp.res / 2u;
    if (gid.x >= half_res || gid.y >= half_res) {
        return;
    }
    let h = hash_uuu(gid.x, gid.y, rp.seed);
    if (u01(h) >= rp.rate) {
        return;
    }
    let h2 = hash_u(h);
    // drop diameter from the size distribution (0.3 mm .. ~4 mm)
    let d = 0.3 - log(1.0 - u01(h2) * 0.995) / rp.lam;
    let amp = rp.amp_scale * pow(d, 1.5);
    let cx = i32(gid.x * 2u + ((h2 >> 3u) & 1u));
    let cy = i32(gid.y * 2u + ((h2 >> 4u) & 1u));
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let w = select(select(1.0, 2.0, dx == 0 || dy == 0), 4.0, dx == 0 && dy == 0) / 16.0;
            atomicAdd(&imp[idx(cx + dx, cy + dy)], i32(amp * w));
        }
    }
}

@compute @workgroup_size(16, 16)
fn cs_step(@builtin(global_invocation_id) gid: vec3u) {
    if (gid.x >= rp.res || gid.y >= rp.res) {
        return;
    }
    let x = i32(gid.x);
    let y = i32(gid.y);
    let i = idx(x, y);
    let c = h_cur[i];
    // 9-point Laplacian (more isotropic rings than the 5-point one)
    let e = h_cur[idx(x + 1, y)] + h_cur[idx(x - 1, y)] + h_cur[idx(x, y + 1)] + h_cur[idx(x, y - 1)];
    let k = h_cur[idx(x + 1, y + 1)] + h_cur[idx(x - 1, y + 1)] + h_cur[idx(x + 1, y - 1)] + h_cur[idx(x - 1, y - 1)];
    let lap = (4.0 * e + k - 20.0 * c) / 6.0;
    let impulse = f32(atomicExchange(&imp[i], 0)) * 1e-6;
    let next = (2.0 * c - h_prev[i] + rp.c2 * lap) * rp.damping - impulse;
    h_prev[i] = clamp(next, -0.01, 0.01);
}
