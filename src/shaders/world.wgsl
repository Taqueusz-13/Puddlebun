// ============================================================================
// world.wgsl — frame/light bindings plus everything that describes the world:
// street layout, puddles, wind, sky, fog and how lamps light rain.
// ============================================================================

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<storage, read> lights: array<Light>;

// Street layout (metres). The street runs along +Z; X = 0 is the road centre.
const ROAD_HALF: f32 = 3.6;   // road edge / kerb face
const KERB_OUT: f32 = 3.75;   // back of kerb stone
const PAVE_OUT: f32 = 5.8;    // pavement meets front-garden wall
const FACADE_X: f32 = 8.0;    // house fronts
const KERB_H: f32 = 0.12;
const EAVE_H: f32 = 6.2;

// Surface kinds (shared with audio — see src/audio/mod.rs).
const SURF_ROAD: u32 = 0u;
const SURF_PUDDLE: u32 = 1u;
const SURF_PAVE: u32 = 2u;
const SURF_ROOF: u32 = 3u;
const SURF_GARDEN: u32 = 4u;

fn ground_height(p: vec2f) -> f32 {
    let ax = abs(p.x);
    if (ax < ROAD_HALF) {
        return 0.0;
    }
    if (ax < FACADE_X) {
        return KERB_H;
    }
    let r = ax - FACADE_X;
    if (r < 5.0) {
        return EAVE_H + r * 0.52;
    }
    if (r < 10.0) {
        return EAVE_H + (10.0 - r) * 0.52;
    }
    return KERB_H;
}

// How much standing water there is at a ground position (0 = none, 1 = deep).
fn puddle_amount(p: vec2f) -> f32 {
    let level = frame.weather.z;
    let ax = abs(p.x);
    // threshold in "standard deviations" of the surface noise, so the
    // puddle slider maps smoothly onto coverage
    let t = (level - 0.55) * 4.5 - 0.75;
    if (ax < ROAD_HALF) {
        // the road is cambered: water collects in the gutters, plus dips
        let gutter = smoothstep(2.5, 3.5, ax);
        let comb = fbm(p * vec2f(0.16, 0.12)) * 0.7 + fbm(p * 0.8 + vec2f(17.0, 3.0)) * 0.3;
        let hn = (comb - 0.5) / 0.076 - gutter * 1.3;
        return clamp((t - hn) * 0.8, 0.0, 1.0);
    }
    if (ax > KERB_OUT && ax < PAVE_OUT) {
        // sunken paving slabs hold shallow puddles
        let hn = (fbm(p * 0.5 + vec2f(41.0, 9.0)) - 0.5) / 0.1;
        return clamp((t - 0.9 - hn) * 0.9, 0.0, 1.0);
    }
    return 0.0;
}

fn surface_kind(p: vec3f) -> u32 {
    let ax = abs(p.x);
    if (ax < PAVE_OUT && puddle_amount(p.xz) > 0.35) {
        return SURF_PUDDLE;
    }
    if (ax < ROAD_HALF) {
        return SURF_ROAD;
    }
    if (ax < PAVE_OUT) {
        return SURF_PAVE;
    }
    if (ax > FACADE_X && p.y > EAVE_H - 0.2) {
        return SURF_ROOF;
    }
    return SURF_GARDEN;
}

// Wind at a point: the global (gusting) wind plus gust fronts that travel downwind.
fn wind_at(p: vec3f) -> vec3f {
    let w = frame.wind.xyz;
    let speed = length(w.xz);
    let t = frame.time.x;
    let q = (p.xz - w.xz * t) * 0.045;
    let gust = gnoise(q) * 0.6 + gnoise(q * 2.3 + vec2f(5.2, 1.3)) * 0.4;
    let g = 1.0 + gust * frame.wind.w * 0.9;
    let swirl = gnoise(q * 1.7 + vec2f(-3.0, 8.0)) * frame.wind.w * 0.35 * speed;
    let side = vec2f(-w.z, w.x) / max(speed, 0.001);
    let h = w.xz * g + side * swirl;
    return vec3f(h.x, w.y, h.y);
}

// Street lamps throw light downwards with a soft cut-off; windows emit outwards.
fn lamp_profile(l: Light, to_point: vec3f) -> f32 {
    let c = dot(to_point, l.dir.xyz);
    if (l.color.w < 0.5) {
        return smoothstep(l.dir.w, l.dir.w + 0.3, c) * (0.55 + 0.45 * c) + 0.012;
    }
    return max(c, 0.0) * 0.9 + 0.1;
}

fn light_atten(d2: f32, radius: f32) -> f32 {
    let r2 = radius * radius;
    let x = d2 / r2;
    let w = clamp(1.0 - x * x, 0.0, 1.0);
    return w * w / (d2 + 0.25);
}

// Extinction coefficient of the misty, rainy air (1/m).
fn fog_sigma() -> f32 {
    let r = max(frame.weather.x, 0.0);
    return frame.weather.w * 0.011 + 0.0013 * pow(r, 0.6);
}

// Streaky moving texture used to show far-away rain inside lamp light.
fn rain_streak_noise(p: vec3f) -> f32 {
    let t = frame.time.x;
    let w = frame.wind.xz;
    let q = vec3f(p.x - w.x * t * 0.9, p.y + t * 6.5, p.z - w.y * t * 0.9);
    let a = vnoise3(q * vec3f(9.0, 0.35, 9.0));
    let b = vnoise3(q * vec3f(23.0, 0.8, 23.0) + vec3f(7.0, 3.0, 1.0));
    return a * 0.65 + b * 0.35;
}

// Single scattering of one lamp along a view ray, solved analytically
// (inverse-square light in a homogeneous medium).
fn lamp_inscatter(ro: vec3f, rd: vec3f, tmax: f32, l: Light, sigma: f32) -> vec3f {
    let rel = l.pos.xyz - ro;
    let t0 = dot(rel, rd);
    let h2 = max(dot(rel, rel) - t0 * t0, 0.0036);
    let h = sqrt(h2);
    let integ = (atan2(tmax - t0, h) - atan2(-t0, h)) / h;
    let tc = clamp(t0, 0.0, tmax);
    let xc = ro + rd * tc;
    let to_pt = normalize(xc - l.pos.xyz + vec3f(0.0, -0.001, 0.0));
    let prof = lamp_profile(l, to_pt);
    let phase = hg(dot(to_pt, -rd), 0.7) * 0.75 + 0.25 * 0.25 * INV_PI;
    let shimmer = 0.8 + 0.4 * rain_streak_noise(xc);
    let ext = exp(-sigma * (tc + sqrt(h2 + 0.0)));
    return l.color.rgb * (sigma * 0.5) * integ * phase * prof * ext * shimmer;
}

fn fog_ambient() -> vec3f {
    return frame.sky.rgb * frame.sky.w * 0.28 + vec3f(0.6, 0.7, 1.0) * frame.misc.w * 0.6;
}

// Applies extinction and in-scattering (ambient glow + every street lamp).
fn apply_fog(col: vec3f, ro: vec3f, rd: vec3f, t: f32) -> vec3f {
    let s = fog_sigma();
    let tr = exp(-s * t);
    var ins = fog_ambient() * (1.0 - tr);
    let n = frame.counts.y;
    for (var i = 0u; i < n; i++) {
        let l = lights[i];
        // skip lamps that are far from this ray entirely
        let rel = l.pos.xyz - ro;
        let t0 = clamp(dot(rel, rd), 0.0, t);
        let d = distance(ro + rd * t0, l.pos.xyz);
        if (d > l.pos.w) {
            continue;
        }
        ins += lamp_inscatter(ro, rd, t, l, s);
    }
    return col * tr + ins;
}

// Overcast night sky lit from below by the town.
fn sky_radiance(rd: vec3f) -> vec3f {
    let glow = frame.sky.rgb * frame.sky.w;
    let up = rd.y;
    let horizon = exp(-max(up, 0.0) * 2.2);
    var c = vec3f(0.002, 0.0024, 0.0035) + glow * (0.4 + 0.6 * horizon);
    if (up > 0.0) {
        let t = frame.time.x;
        let uv = rd.xz / (up + 0.15) * 1.6 + frame.wind.xz * t * 0.0025;
        let cl = fbm(uv * 0.55) * 0.7 + fbm(uv * 1.9 + vec2f(5.0, 2.0)) * 0.3;
        // clouds are lit from below by the town; gaps are darker
        let dens = smoothstep(0.32, 0.72, cl);
        c *= mix(0.55, 1.25, dens);
    } else {
        c = glow * 0.8;
    }
    // lightning lights the clouds (stage 4 drives misc.w)
    c += vec3f(0.55, 0.62, 0.9) * frame.misc.w * (0.4 + 0.6 * max(up, 0.0));
    return c;
}

// How bright a raindrop (or splash droplet) at `p` looks: every lamp nearby,
// strongly forward-scattered when the lamp is behind the drop.
fn drop_light(p: vec3f) -> vec3f {
    let to_cam = normalize(frame.cam_pos.xyz - p);
    var c = frame.sky.rgb * frame.sky.w * 0.25;
    let n = frame.counts.y;
    for (var i = 0u; i < n; i++) {
        let l = lights[i];
        let lv = l.pos.xyz - p;
        let d2 = dot(lv, lv);
        if (d2 > l.pos.w * l.pos.w) {
            continue;
        }
        let ldir = lv * inverseSqrt(d2);
        let prof = lamp_profile(l, -ldir);
        let cs = dot(-ldir, to_cam);
        let phase = 0.06 + 2.2 * hg(cs, 0.82) + 0.6 * hg(cs, 0.35) + 0.15 * hg(cs, -0.5);
        c += l.color.rgb * prof * light_atten(d2, l.pos.w) * phase;
    }
    return c;
}

// --------------------------------------------------------------------------
// Motion-blurred streak quads (used by rain and splash droplets).
// --------------------------------------------------------------------------
struct Streak {
    clip: vec4f,
    uv: vec2f,      // x: 0 tail -> 1 head, y: -1..1 across
    energy: f32,    // brightness scale that conserves the drop's light
    depth: f32,     // view depth (m)
}

fn streak_vertex(head_in: vec3f, tail_in: vec3f, diameter: f32, vi: u32) -> Streak {
    var o: Streak;
    o.clip = vec4f(0.0, 0.0, 2.0, 1.0);   // outside the clip volume = culled
    o.uv = vec2f(0.0);
    o.energy = 0.0;
    o.depth = 0.0;
    let near = 0.12;
    var head = head_in;
    var tail = tail_in;
    var ch = frame.view_proj * vec4f(head, 1.0);
    var ct = frame.view_proj * vec4f(tail, 1.0);
    if (ch.w < near && ct.w < near) {
        return o;
    }
    if (ch.w < near) {
        head = mix(tail, head, (ct.w - near) / (ct.w - ch.w));
        ch = frame.view_proj * vec4f(head, 1.0);
    }
    if (ct.w < near) {
        tail = mix(head, tail, (ch.w - near) / (ch.w - ct.w));
        ct = frame.view_proj * vec4f(tail, 1.0);
    }
    let half_px = 0.5 * frame.screen.xy;
    let sh = ch.xy / ch.w * half_px;
    let st = ct.xy / ct.w * half_px;
    var axis = sh - st;
    let len = length(axis);
    if (len > 1e-4) {
        axis = axis / len;
    } else {
        axis = vec2f(0.0, 1.0);
    }
    let perp = vec2f(-axis.y, axis.x);
    let depth = max(ch.w, near);
    let true_w = diameter * frame.cam_fwd.w / depth;
    let w = max(true_w, 1.25);

    var corners = array<vec2f, 6>(
        vec2f(0.0, -1.0), vec2f(1.0, -1.0), vec2f(0.0, 1.0),
        vec2f(1.0, -1.0), vec2f(1.0, 1.0), vec2f(0.0, 1.0)
    );
    let c = corners[vi % 6u];
    let along = c.x;
    let across = c.y;
    let p0 = st - axis * w * 0.75;
    let p1 = sh + axis * w * 0.75;
    let px = mix(p0, p1, along) + perp * across * w * 0.9;
    let cw = mix(ct.w, ch.w, along);
    let cz = mix(ct.z / ct.w, ch.z / ch.w, along);
    o.clip = vec4f(px / half_px * cw, cz * cw, cw);
    o.uv = vec2f(along, across);
    // light of the drop spread over the streak's area
    // x4: we simulate ~1/20th of the real number of drops, so each one stands in for many
    o.energy = (true_w * true_w) / (w * (len + w)) * 4.0;
    o.depth = depth;
    return o;
}
