// ============================================================================
// rain_draw.wgsl — renders raindrops as motion-blurred streaks and splash
// droplets as tiny ballistic streaks (computed analytically from spawn time).
// ============================================================================

struct Drop {
    pos: vec3f,
    size: f32,
    vel: vec3f,
    seed: u32,
    light: vec4f,
}

struct SimParams {
    box_center: vec4f,
    box_y: vec4f,
    dsd: vec4f,
    counts: vec4u,
    audio: vec4f,
}

struct Splash {
    pos: vec3f,
    t0: f32,
    size: f32,
    kind: u32,
    seed: u32,
    speed: f32,
}

@group(1) @binding(0) var<uniform> sim: SimParams;
@group(1) @binding(1) var<storage, read> drops: array<Drop>;
@group(1) @binding(2) var<storage, read> splashes: array<Splash>;

struct VOut {
    @builtin(position) clip: vec4f,
    @location(0) col: vec3f,
    @location(1) uv: vec2f,
    @location(2) alpha: f32,
}

@vertex
fn vs_rain(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    var o: VOut;
    let d = drops[ii];
    let shutter = sim.box_y.z;
    let st = streak_vertex(d.pos, d.pos - d.vel * shutter, d.size * 0.001, vi);
    o.clip = st.clip;
    o.uv = st.uv;
    let near_fade = smoothstep(0.25, 1.2, st.depth);
    let tr = exp(-fog_sigma() * st.depth);
    o.col = d.light.rgb * st.energy * near_fade * tr;
    o.alpha = clamp(st.energy * 0.6, 0.0, 0.35) * near_fade;
    return o;
}

@fragment
fn fs_streak(in: VOut) -> @location(0) vec4f {
    let across = in.uv.y;
    let prof = exp(-across * across * 3.5);
    let ends = smoothstep(0.0, 0.12, in.uv.x) * smoothstep(1.0, 0.88, in.uv.x);
    let a = prof * ends;
    return vec4f(in.col * a, in.alpha * a);
}

const DROPLETS: u32 = 8u;

@vertex
fn vs_splash(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VOut {
    var o: VOut;
    o.clip = vec4f(0.0, 0.0, 2.0, 1.0);
    o.col = vec3f(0.0);
    o.uv = vec2f(0.0);
    o.alpha = 0.0;
    let s = splashes[ii / DROPLETS];
    let j = ii % DROPLETS;
    let age = frame.time.x - s.t0;
    let life = 0.16 + 0.06 * s.size;
    if (s.size <= 0.0 || age < 0.0 || age > life) {
        return o;
    }
    var h = hash_uu(s.seed, j);
    let az = u01(h) * TAU;
    h = hash_u(h);
    let puddle = s.kind == SURF_PUDDLE;
    var el = mix(0.3, 1.25, u01(h));
    h = hash_u(h);
    var speed = (0.45 + 0.9 * u01(h)) * (0.3 + 0.22 * s.size) * clamp(s.speed / 7.0, 0.3, 1.4);
    // on water the first droplet is the central jet
    if (puddle && j == 0u) {
        el = 1.5;
        speed = 0.35 + 0.18 * s.size;
    }
    let v0 = vec3f(cos(az) * cos(el), sin(el), sin(az) * cos(el)) * speed;
    let g = vec3f(0.0, -9.81, 0.0);
    let shutter = sim.box_y.z;
    let t1 = age;
    let t0 = max(age - shutter, 0.0);
    var head = s.pos + v0 * t1 + 0.5 * g * t1 * t1;
    var tail = s.pos + v0 * t0 + 0.5 * g * t0 * t0;
    head.y = max(head.y, s.pos.y + 0.001);
    tail.y = max(tail.y, s.pos.y + 0.001);
    let dsize = s.size * 0.001 * mix(0.2, 0.45, u01(hash_u(h)));
    let st = streak_vertex(head, tail, dsize, vi);
    o.clip = st.clip;
    o.uv = st.uv;
    let fade = 1.0 - smoothstep(life * 0.6, life, age);
    let tr = exp(-fog_sigma() * st.depth);
    o.col = drop_light(head) * st.energy * fade * tr * 5.0;
    o.alpha = clamp(st.energy * 0.5, 0.0, 0.3) * fade;
    return o;
}
