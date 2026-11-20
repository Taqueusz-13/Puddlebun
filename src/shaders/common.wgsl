// ============================================================================
// common.wgsl — pure types and helpers shared by every Puddlebun shader.
// (No bindings in here; world.wgsl adds the frame/light bindings.)
// ============================================================================

const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;
const INV_PI: f32 = 0.31830988618;

// Per-frame constants. Must match `FrameUniform` in src/render/mod.rs.
struct Frame {
    view_proj: mat4x4f,
    inv_view_proj: mat4x4f,
    prev_view_proj: mat4x4f,
    cam_pos: vec4f,     // xyz camera position
    cam_fwd: vec4f,     // xyz forward, w = focal length in pixels
    cam_right: vec4f,   // xyz right
    cam_up: vec4f,      // xyz up
    screen: vec4f,      // width, height, 1/width, 1/height
    time: vec4f,        // x seconds, y dt, z frame index, w = 1 in the reflection pass
    wind: vec4f,        // xyz wind velocity (m/s), w gust strength 0..1
    weather: vec4f,     // x rain mm/h, y wetness 0..1, z puddle level 0..1, w fog 0..1
    lamp: vec4f,        // rgb lamp colour (luminance 1), w lamp intensity
    sky: vec4f,         // rgb urban sky glow colour, w brightness
    ripple: vec4f,      // x texel size (m), y resolution, z tile size (m), w strength
    counts: vec4u,      // x lights, y street lamps (first in list), z active drops, w max drops
    misc: vec4f,        // x exposure, y window-light fraction, z bloom, w lightning flash
}

// A light. Street lamps come first in the array (counts.y of them).
struct Light {
    pos: vec4f,     // xyz position, w radius of influence (m)
    color: vec4f,   // rgb intensity (W/sr-ish), w kind: 0 = street lamp, 1 = window
    dir: vec4f,     // xyz main emission direction, w cos(cut-off) for lamps
}

// --------------------------------------------------------------------------
// Hashing (lowbias32). Identical to `hash_u` in src/scene.rs so the CPU and GPU
// agree on things like which windows are lit.
// --------------------------------------------------------------------------
fn hash_u(x: u32) -> u32 {
    var v = x;
    v = v ^ (v >> 16u);
    v = v * 0x7feb352du;
    v = v ^ (v >> 15u);
    v = v * 0x846ca68bu;
    v = v ^ (v >> 16u);
    return v;
}
fn hash_uu(a: u32, b: u32) -> u32 { return hash_u(a ^ hash_u(b ^ 0x9e3779b9u)); }
fn hash_uuu(a: u32, b: u32, c: u32) -> u32 { return hash_u(a ^ hash_uu(b, c)); }
fn u01(h: u32) -> f32 { return f32(h >> 8u) * (1.0 / 16777216.0); }
fn cell2(c: vec2i) -> u32 { return hash_uu(bitcast<u32>(c.x), bitcast<u32>(c.y)); }
fn cell3(c: vec3i) -> u32 { return hash_uuu(bitcast<u32>(c.x), bitcast<u32>(c.y), bitcast<u32>(c.z)); }
fn rand2(c: vec2i) -> f32 { return u01(cell2(c)); }

// Advances a per-thread RNG state and returns a uniform number in [0, 1).
fn rnd(state: ptr<function, u32>) -> f32 {
    *state = hash_u(*state + 0x6d2b79f5u);
    return u01(*state);
}

// --------------------------------------------------------------------------
// Noise
// --------------------------------------------------------------------------
fn grad2(c: vec2i) -> vec2f {
    let a = u01(cell2(c)) * TAU;
    return vec2f(cos(a), sin(a));
}

// 2D gradient noise, roughly in [-1, 1].
fn gnoise(p: vec2f) -> f32 {
    let i = vec2i(floor(p));
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = dot(grad2(i), f);
    let b = dot(grad2(i + vec2i(1, 0)), f - vec2f(1.0, 0.0));
    let c = dot(grad2(i + vec2i(0, 1)), f - vec2f(0.0, 1.0));
    let d = dot(grad2(i + vec2i(1, 1)), f - vec2f(1.0, 1.0));
    return 1.4142 * mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// 4-octave fractal noise, roughly in [0, 1].
fn fbm(p: vec2f) -> f32 {
    var s = 0.0;
    var a = 0.5;
    var q = p;
    for (var i = 0; i < 4; i++) {
        s += a * gnoise(q);
        q = mat2x2f(1.6, 1.2, -1.2, 1.6) * q + vec2f(3.1, 1.7);
        a *= 0.5;
    }
    return clamp(0.5 + 0.55 * s, 0.0, 1.0);
}

// 3D value noise in [0, 1].
fn vnoise3(p: vec3f) -> f32 {
    let i = vec3i(floor(p));
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = u01(cell3(i));
    let b = u01(cell3(i + vec3i(1, 0, 0)));
    let c = u01(cell3(i + vec3i(0, 1, 0)));
    let d = u01(cell3(i + vec3i(1, 1, 0)));
    let e = u01(cell3(i + vec3i(0, 0, 1)));
    let g = u01(cell3(i + vec3i(1, 0, 1)));
    let h = u01(cell3(i + vec3i(0, 1, 1)));
    let k = u01(cell3(i + vec3i(1, 1, 1)));
    let x0 = mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
    let x1 = mix(mix(e, g, u.x), mix(h, k, u.x), u.y);
    return mix(x0, x1, u.z);
}

// --------------------------------------------------------------------------
// Shading helpers
// --------------------------------------------------------------------------
fn d_ggx(nh: f32, a: f32) -> f32 {
    let a2 = a * a;
    let d = nh * nh * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d + 1e-7);
}

fn v_smith(nv: f32, nl: f32, a: f32) -> f32 {
    let a2 = a * a;
    let gv = nl * sqrt(nv * nv * (1.0 - a2) + a2);
    let gl = nv * sqrt(nl * nl * (1.0 - a2) + a2);
    return 0.5 / max(gv + gl, 1e-5);
}

fn f_schlick(f0: vec3f, vh: f32) -> vec3f {
    return f0 + (vec3f(1.0) - f0) * pow(1.0 - vh, 5.0);
}

// Henyey–Greenstein phase function.
fn hg(c: f32, g: f32) -> f32 {
    let g2 = g * g;
    return (1.0 - g2) / (4.0 * PI * pow(max(1.0 + g2 - 2.0 * g * c, 1e-4), 1.5));
}

fn luminance(c: vec3f) -> f32 {
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

// Bump mapping without tangents (Mikkelsen 2010, "surface gradient").
// MUST be called from uniform control flow (it uses derivatives).
fn perturb_normal(n: vec3f, pos: vec3f, height: f32) -> vec3f {
    let dpx = dpdx(pos);
    let dpy = dpdy(pos);
    let dhx = dpdx(height);
    let dhy = dpdy(height);
    let r1 = cross(dpy, n);
    let r2 = cross(n, dpx);
    let det = dot(dpx, r1);
    let grad = sign(det) * (dhx * r1 + dhy * r2);
    let nn = abs(det) * n - grad;
    let l = length(nn);
    if (l < 1e-8) {
        return n;
    }
    return nn / l;
}
