// ============================================================================
// scene.wgsl — the street: procedural materials, wetness, puddles with live
// ripples, planar reflections, lamp lighting and fog.
// Used for both the main pass and the (mirrored) reflection pass.
// ============================================================================

@group(1) @binding(0) var refl_tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
@group(1) @binding(2) var<storage, read> ripple_h: array<f32>;

// Material ids (must match src/scene.rs)
const M_ROAD: u32 = 0u;
const M_PAVE: u32 = 1u;
const M_KERB: u32 = 2u;
const M_FACADE: u32 = 3u;
const M_ROOF: u32 = 4u;
const M_METAL: u32 = 5u;
const M_LAMP: u32 = 6u;
const M_WALL: u32 = 7u;
const M_BAY: u32 = 8u;
const M_CHIMNEY: u32 = 9u;
const M_POT: u32 = 10u;
const M_GARDEN: u32 = 11u;
const M_COPING: u32 = 12u;
const M_LEAD: u32 = 13u;
const M_TRIM: u32 = 14u;

struct VIn {
    @location(0) pos: vec3f,
    @location(1) mat: u32,
    @location(2) nrm: vec3f,
    @location(3) seed: f32,
    @location(4) uv: vec2f,
    @location(5) extra: vec2f,
}

struct VOut {
    @builtin(position) clip: vec4f,
    @location(0) wpos: vec3f,
    @location(1) nrm: vec3f,
    @location(2) uv: vec2f,
    @location(3) @interpolate(flat) mat: u32,
    @location(4) @interpolate(flat) seed: f32,
    @location(5) extra: vec2f,
}

@vertex
fn vs_main(v: VIn) -> VOut {
    var o: VOut;
    var pos = v.pos;
    if (v.mat >= 40u && v.extra.x > 0.5) {
        pos.y += frame.room.x;   // the lower sash slides up when the window opens
    }
    o.clip = frame.view_proj * vec4f(pos, 1.0);
    o.wpos = pos;
    o.nrm = v.nrm;
    o.uv = v.uv;
    o.mat = v.mat;
    o.seed = v.seed;
    o.extra = v.extra;
    return o;
}

struct Surf {
    albedo: vec3f,
    rough: f32,
    emit: vec3f,
    metal: f32,
    interior: vec3f,   // radiance seen through glass
    porosity: f32,     // how much the surface darkens when wet
    height: f32,       // bump height (m)
    glass: f32,        // 1 = window glass
    f0: f32,
    ground: f32,       // 1 = flat ground that receives planar reflections
    wet_scale: f32,    // how exposed the surface is to rain
    cutout: f32,       // 1 = see-through (grates, railings)
}

fn surf_default() -> Surf {
    var s: Surf;
    s.albedo = vec3f(0.5);
    s.rough = 0.8;
    s.emit = vec3f(0.0);
    s.metal = 0.0;
    s.interior = vec3f(0.0);
    s.porosity = 0.3;
    s.height = 0.0;
    s.glass = 0.0;
    s.f0 = 0.04;
    s.ground = 0.0;
    s.wet_scale = 1.0;
    s.cutout = 0.0;
    return s;
}

// ---------------------------------------------------------------- ground ----
fn asphalt(p: vec3f) -> Surf {
    var s = surf_default();
    let xz = p.xz;
    let patchy = fbm(xz * 0.07);
    // aggregate: smooth fine noise (no blocky cells)
    let fine = gnoise(xz * 55.0) * 0.5 + 0.5;
    let grit = gnoise(xz * 140.0 + vec2f(7.0, 1.0)) * 0.5 + 0.5;
    let mid = gnoise(xz * 3.0) * 0.5 + 0.5;
    // occasional hairline cracks and tar repairs
    let crack_zone = smoothstep(0.6, 0.7, fbm(xz * 0.11 + vec2f(9.0, 2.0)));
    let crack = (1.0 - smoothstep(0.0, 0.012, abs(gnoise(xz * 2.2 + vec2f(3.0, 7.0))))) * crack_zone;
    var a = 0.05 + 0.03 * patchy + 0.018 * fine * grit + 0.008 * mid;
    a *= 1.0 - crack * 0.45;
    s.albedo = vec3f(a, a, a * 1.03);
    s.rough = 0.9 - 0.12 * fine;
    s.porosity = 0.8;
    s.height = (fine - 0.5) * 0.0011 + (grit - 0.5) * 0.0005 + mid * 0.0006 - crack * 0.0015;
    s.ground = 1.0;
    return s;
}

fn mat_road(p: vec3f) -> Surf {
    var s = asphalt(p);
    let xz = p.xz;
    let ax = abs(p.x);
    // double yellow lines along the kerb on some stretches
    let seg = fract(p.z / 95.0 + 0.3);
    if (seg < 0.35) {
        let l1 = 1.0 - step(0.05, abs(ax - 3.38));
        let l2 = 1.0 - step(0.05, abs(ax - 3.22));
        let wear = smoothstep(0.25, 0.6, fbm(xz * 5.0));
        let line = max(l1, l2) * wear;
        s.albedo = mix(s.albedo, vec3f(0.52, 0.4, 0.05), line);
        s.rough = mix(s.rough, 0.5, line);
        s.porosity = mix(s.porosity, 0.15, line);
        s.height += line * 0.0015;
    }
    // gully grates by the kerb every 18 m
    let gz = p.z - 18.0 * round(p.z / 18.0);
    if (ax > 3.12 && ax < 3.58 && abs(gz) < 0.3) {
        let slots = step(0.45, fract(p.z * 14.0));
        let rim = 1.0 - step(0.03, min(min(ax - 3.12, 3.58 - ax), 0.3 - abs(gz)));
        s.albedo = vec3f(0.03) + vec3f(0.035) * max(slots, rim);
        s.metal = 0.7;
        s.rough = 0.4;
        s.porosity = 0.0;
        s.height = -0.006 * (1.0 - max(slots, rim));
    }
    // a couple of manhole covers in the road
    let mz = p.z - 47.0 * round(p.z / 47.0);
    let md = length(vec2f(p.x - 1.1, mz));
    if (md < 0.32) {
        let ring = smoothstep(0.29, 0.3, md);
        let grid = step(0.5, fract((p.x + mz) * 9.0)) * step(0.5, fract((p.x - mz) * 9.0));
        s.albedo = vec3f(0.035 + 0.02 * grid + 0.03 * ring);
        s.metal = 0.8;
        s.rough = 0.35;
        s.porosity = 0.0;
        s.height = 0.002 * grid;
    }
    return s;
}

fn mat_pave(p: vec3f) -> Surf {
    var s = surf_default();
    let ax = abs(p.x);
    let q = vec2f((ax - KERB_OUT) / 0.6, p.z / 0.9);
    let cell = vec2i(floor(q));
    let f = fract(q);
    let joint = min(min(f.x, 1.0 - f.x) * 0.6, min(f.y, 1.0 - f.y) * 0.9);
    let j = 1.0 - smoothstep(0.002, 0.007, joint);
    let h = rand2(cell + vec2i(91, 17));
    let a = 0.2 + 0.08 * h + 0.05 * (fbm(p.xz * 2.0) - 0.5);
    s.albedo = mix(vec3f(a, a * 0.98, a * 0.94), vec3f(0.07), j);
    s.rough = 0.85;
    s.porosity = 0.55;
    s.height = -j * 0.004 + (h - 0.5) * 0.002;
    s.ground = 1.0;
    return s;
}

fn mat_kerb(p: vec3f, n: vec3f) -> Surf {
    var s = surf_default();
    let blk = fract(p.z / 0.915);
    let j = 1.0 - smoothstep(0.0, 0.006, min(blk, 1.0 - blk) * 0.915);
    let h = rand2(vec2i(i32(floor(p.z / 0.915)), 7));
    let a = 0.28 + 0.08 * h + 0.04 * fbm(p.xz * 6.0);
    s.albedo = mix(vec3f(a, a, a * 0.96), vec3f(0.1), j);
    s.rough = 0.75;
    s.porosity = 0.45;
    s.height = -j * 0.003;
    s.ground = select(0.0, 1.0, n.y > 0.5);
    return s;
}

fn mat_garden(p: vec3f) -> Surf {
    var s = surf_default();
    // small front gardens: gravel and old tiles with tufts of grass
    let n1 = fbm(p.xz * 1.3);
    let g = gnoise(p.xz * 45.0) * 0.5 + 0.5;
    let tiles = step(0.5, fbm(p.xz * 0.35 + vec2f(4.0, 8.0)));
    let tq = fract(p.xz / 0.3);
    let tj = 1.0 - smoothstep(0.0, 0.04, min(min(tq.x, 1.0 - tq.x), min(tq.y, 1.0 - tq.y)));
    let grass = smoothstep(0.55, 0.7, n1) * (1.0 - tiles);
    var col = mix(vec3f(0.16, 0.15, 0.13) * (0.7 + 0.6 * g), vec3f(0.2, 0.09, 0.06) * (1.0 - tj * 0.6), tiles);
    col = mix(col, vec3f(0.05, 0.08, 0.035), grass);
    s.albedo = col;
    s.rough = 0.85;
    s.porosity = 0.55;
    s.height = g * 0.003 * (1.0 - tiles) - tj * 0.003 * tiles;
    s.ground = 1.0;
    return s;
}

// ----------------------------------------------------------------- brick ----
fn brick_base(uv: vec2f, seed: f32) -> Surf {
    var s = surf_default();
    let bw = 0.225;
    let bh = 0.075;
    let row = floor(uv.y / bh);
    let odd = (i32(row) & 1) == 1;
    let x = uv.x + select(0.0, bw * 0.5, odd);
    let col = floor(x / bw);
    let fx = (x / bw - col) * bw;
    let fy = (uv.y / bh - row) * bh;
    let edge = min(min(fx, bw - fx), min(fy, bh - fy));
    let mortar = 1.0 - smoothstep(0.0035, 0.006, edge);
    let sd = u32(seed + 0.5);
    let hb = u01(hash_uuu(bitcast<u32>(i32(col)), bitcast<u32>(i32(row)), sd));
    let pal = u01(hash_u(sd * 7u + 3u));
    var base = vec3f(0.30, 0.115, 0.07);
    if (pal > 0.66) {
        base = vec3f(0.36, 0.27, 0.16);
    } else if (pal > 0.33) {
        base = vec3f(0.25, 0.13, 0.085);
    }
    let grime = fbm(uv * vec2f(1.4, 0.5) + vec2f(seed * 1.37, 0.0));
    var c = base * (0.65 + 0.7 * hb) * (0.65 + 0.55 * grime);
    if (hb > 0.94) {
        c *= 0.55;
    }
    s.albedo = mix(c, vec3f(0.3, 0.29, 0.27) * (0.75 + 0.35 * grime), mortar);
    s.rough = mix(0.85, 0.95, mortar);
    s.porosity = mix(0.45, 0.7, mortar);
    s.height = -mortar * 0.006 + hb * 0.0012;
    // rain streaks running down the wall
    let streak = fbm(vec2f(uv.x * 3.0, uv.y * 0.15 + seed));
    s.wet_scale = 0.55 + 0.45 * smoothstep(0.35, 0.7, streak);
    return s;
}

// ---------------------------------------------------- windows & interiors ----
// Looks through a window into a procedurally generated room ("interior mapping").
// lp = position inside the window (m), wsize = window size, tan_u = world
// direction in which lp.x increases, n = outward facade normal.
fn interior_radiance(lp: vec2f, wsize: vec2f, n: vec3f, tan_u: vec3f, v: vec3f, rs: u32, lit: bool) -> vec3f {
    let d = -v;
    let inward = -n;
    let dl = vec3f(dot(d, tan_u), d.y, dot(d, inward));
    if (dl.z <= 0.002) {
        return vec3f(0.0);
    }
    let shop = (rs & 0x80000000u) != 0u;
    let rw = wsize.x + 1.9;
    let depth = select(3.0 + 1.5 * u01(hash_u(rs + 11u)), 5.5, shop);
    let floor_y = -0.9;
    let ceil_y = floor_y + 2.65;
    let o = vec3f(lp.x - wsize.x * 0.5, lp.y, 0.0);

    var tx = 1e6;
    if (abs(dl.x) > 1e-5) {
        tx = select((-rw * 0.5 - o.x) / dl.x, (rw * 0.5 - o.x) / dl.x, dl.x > 0.0);
    }
    var ty = 1e6;
    if (abs(dl.y) > 1e-5) {
        ty = select((floor_y - o.y) / dl.y, (ceil_y - o.y) / dl.y, dl.y > 0.0);
    }
    let tz = depth / dl.z;
    let t = min(min(tx, ty), tz);
    let h = o + dl * t;

    // room palette
    let pick = u01(hash_u(rs + 3u));
    var wall = vec3f(0.62, 0.55, 0.42);
    if (pick > 0.8) {
        wall = vec3f(0.33, 0.42, 0.34);
    } else if (pick > 0.6) {
        wall = vec3f(0.55, 0.32, 0.25);
    } else if (pick > 0.4) {
        wall = vec3f(0.38, 0.43, 0.5);
    } else if (pick > 0.2) {
        wall = vec3f(0.66, 0.62, 0.55);
    }
    var albedo = wall;
    if (t == tz) {
        // back wall: a picture frame or shelving
        let pf = step(abs(h.x - (u01(hash_u(rs + 5u)) - 0.5) * rw * 0.5), 0.35) * step(abs(h.y - 0.9), 0.25);
        albedo = mix(wall * (0.9 + 0.1 * sin(h.x * 40.0)), vec3f(0.2, 0.15, 0.1), pf);
    } else if (t == ty) {
        if (dl.y > 0.0) {
            albedo = vec3f(0.75, 0.73, 0.7);
        } else {
            albedo = vec3f(0.24, 0.15, 0.09) * (0.8 + 0.2 * sin(h.x * 25.0));
        }
    } else {
        albedo = wall * 0.88;
    }

    var rad: vec3f;
    if (lit) {
        let lamp_pos = vec3f(0.0, ceil_y - 0.45, depth * 0.55);
        let dd = distance(h, lamp_pos);
        var warm = vec3f(1.0, 0.62, 0.32);
        var bright = 0.25 + 0.75 * u01(hash_u(rs + 7u));
        if (in_city()) {
            let tp = u01(hash_u(rs + 23u));
            if (tp > 0.85) {
                warm = vec3f(0.75, 0.3, 1.0);
            } else if (tp > 0.72) {
                warm = vec3f(0.3, 0.85, 1.0);
            } else if (tp > 0.55) {
                warm = vec3f(0.85, 0.9, 1.0);
            }
        }
        if (shop) {
            bright = 2.2;
            // shelves of stock
            albedo *= 0.7 + 0.5 * step(0.5, fract(h.y * 3.0)) * step(0.3, fract(h.x * 1.7));
        }
        rad = albedo * warm * bright / (0.5 + dd * dd * 0.3);
        // the lampshade itself
        let to_lamp = lamp_pos - o;
        let along = dot(to_lamp, normalize(dl));
        let miss = length(to_lamp - normalize(dl) * along);
        rad += warm * 3.0 * bright * smoothstep(0.22, 0.12, miss) * step(0.0, along);
        // some rooms have a TV flickering
        if (u01(hash_u(rs + 13u)) > 0.75) {
            let fl = 0.6 + 0.4 * sin(frame.time.x * 7.0 + f32(rs & 255u)) * sin(frame.time.x * 2.3);
            rad += vec3f(0.25, 0.35, 0.6) * fl * 0.5 * albedo;
        }
    } else {
        rad = albedo * (frame.sky.rgb * frame.sky.w * 0.25 + frame.lamp.rgb * 0.004);
    }
    if (shop) {
        return rad;
    }

    // curtains, partly drawn
    let bright_c = 0.35 + 0.75 * u01(hash_u(rs + 7u));
    let openness = u01(hash_u(rs + 17u));
    if (openness < 0.75) {
        let tc = 0.12 / dl.z;
        let xc = o.x + dl.x * tc + wsize.x * 0.5;
        let cover = (0.75 - openness) * 0.7;
        if (xc < wsize.x * cover || xc > wsize.x * (1.0 - cover)) {
            let cpick = u01(hash_u(rs + 19u));
            let cloth = mix(vec3f(0.45, 0.12, 0.1), vec3f(0.6, 0.55, 0.35), cpick);
            let folds = 0.65 + 0.35 * sin(xc * 38.0);
            if (lit) {
                rad = cloth * folds * vec3f(1.0, 0.62, 0.32) * 0.5 * bright_c;
            } else {
                rad = cloth * folds * 0.01;
            }
        }
    }
    return rad;
}

fn window_surface(base: Surf, lp: vec2f, wsize: vec2f, n: vec3f, tan_u: vec3f, v: vec3f, rs: u32, lit: bool) -> Surf {
    var s = base;
    let fr = 0.065;
    let dx = min(lp.x, wsize.x - lp.x);
    let dy = min(lp.y, wsize.y - lp.y);
    var frame_m = 1.0 - step(fr, min(dx, dy));
    frame_m = max(frame_m, 1.0 - step(0.03, abs(lp.y - wsize.y * 0.5)));
    frame_m = max(frame_m, 1.0 - step(0.016, abs(lp.x - wsize.x * 0.5)));
    if (frame_m > 0.5) {
        s.albedo = vec3f(0.46, 0.46, 0.43);
        s.rough = 0.45;
        s.porosity = 0.05;
        s.height = 0.012;
        s.metal = 0.0;
        s.glass = 0.0;
        return s;
    }
    s.glass = 1.0;
    s.albedo = vec3f(0.0);
    s.rough = 0.03;
    s.porosity = 0.0;
    s.f0 = 0.04;
    s.height = -0.015;
    s.interior = interior_radiance(lp, wsize, n, tan_u, v, rs, lit);
    return s;
}

fn is_lit(house: u32, win: u32) -> bool {
    return u01(hash_uu(house, win)) < frame.misc.y;
}

fn stone(base: Surf, uv: vec2f) -> Surf {
    var s = base;
    let g = fbm(uv * 4.0);
    s.albedo = vec3f(0.42, 0.4, 0.36) * (0.75 + 0.35 * g);
    s.rough = 0.8;
    s.porosity = 0.5;
    s.height = 0.008;
    return s;
}

fn mat_facade(p: vec3f, n: vec3f, uv: vec2f, seed: f32, v: vec3f) -> Surf {
    var s = brick_base(uv, seed);
    let u = uv.x;
    let y = uv.y;
    let house = u32(seed + 0.5);
    let tan_u = vec3f(0.0, 0.0, 1.0);

    // decorative string course between the floors
    if (y > 3.12 && y < 3.3) {
        s = stone(s, uv);
        s.height = 0.006;
    }

    // first-floor sash windows
    var has = false;
    var wmin = vec2f(0.0);
    var wmax = vec2f(0.0);
    var wid = 0u;
    if (y > 3.4 && y < 5.65) {
        if (u > 0.6 && u < 2.8) {
            wmin = vec2f(0.75, 3.55);
            wmax = vec2f(2.65, 5.35);
            wid = 1u;
            has = true;
        }
        if (u > 3.45 && u < 4.75) {
            wmin = vec2f(3.55, 3.55);
            wmax = vec2f(4.65, 5.35);
            wid = 2u;
            has = true;
        }
    }
    if (has) {
        if (y >= wmax.y && y < wmax.y + 0.22 && u > wmin.x - 0.1 && u < wmax.x + 0.1) {
            s = stone(s, uv);
        } else if (y < wmin.y && y > wmin.y - 0.1 && u > wmin.x - 0.12 && u < wmax.x + 0.12) {
            s = stone(s, uv);
            s.height = 0.015;
        } else if (u > wmin.x && u < wmax.x && y > wmin.y && y < wmax.y) {
            let rs = hash_uu(house, wid + 100u);
            s = window_surface(s, vec2f(u, y) - wmin, wmax - wmin, n, tan_u, v, rs, is_lit(house, wid));
        }
    }

    // front door with fanlight
    if (u > 3.45 && u < 4.75 && y < 2.8) {
        let dmin = vec2f(3.6, 0.12);
        let dmax = vec2f(4.6, 2.2);
        if (u > dmin.x && u < dmax.x && y < dmax.y) {
            let pick = u01(hash_u(house * 31u + 5u));
            var col = vec3f(0.035, 0.05, 0.13);
            if (pick > 0.83) {
                col = vec3f(0.6, 0.6, 0.57);
            } else if (pick > 0.66) {
                col = vec3f(0.28, 0.03, 0.03);
            } else if (pick > 0.5) {
                col = vec3f(0.025, 0.11, 0.06);
            } else if (pick > 0.33) {
                col = vec3f(0.015, 0.015, 0.015);
            } else if (pick > 0.16) {
                col = vec3f(0.04, 0.13, 0.13);
            }
            let lx = u - dmin.x;
            let ly = y - dmin.y;
            // raised and fielded panels
            let px = min(abs(lx - 0.27), abs(lx - 0.73));
            let panel = step(px, 0.17) * (step(abs(ly - 0.55), 0.38) + step(abs(ly - 1.5), 0.42));
            s.albedo = col;
            s.rough = 0.25;
            s.porosity = 0.0;
            s.metal = 0.0;
            s.height = 0.006 * panel;
            // brass letterbox and knob
            if (abs(lx - 0.5) < 0.13 && abs(ly - 1.02) < 0.025) {
                s.albedo = vec3f(0.6, 0.45, 0.2);
                s.metal = 1.0;
                s.rough = 0.25;
            }
            s.wet_scale = 0.2;
        } else if (u > dmin.x && u < dmax.x && y > 2.27 && y < 2.62) {
            let rs = hash_uu(house, 103u);
            var fl = window_surface(s, vec2f(u - dmin.x, y - 2.27), vec2f(1.0, 0.35), n, tan_u, v, rs, is_lit(house, 3u));
            s = fl;
        } else {
            // painted door surround
            s.albedo = vec3f(0.55, 0.55, 0.52);
            s.rough = 0.5;
            s.porosity = 0.1;
            s.height = 0.01;
        }
    }
    return s;
}

// Bay window: brick base, three glass panes on the front, one each side.
fn mat_bay(p: vec3f, n: vec3f, uv: vec2f, seed: f32, extra: vec2f, v: vec3f) -> Surf {
    var s = brick_base(uv, seed);
    let house = u32(seed + 0.5);
    let front = abs(n.x) > 0.5;
    let u0 = select(0.0, 0.5, front);
    let wtot = select(0.8, 2.4, front);
    let lu = uv.x - u0;
    let y = uv.y;
    if (y > 0.66 && y < 0.78) {
        s = stone(s, uv);
        s.height = 0.012;
    } else if (y >= 2.6) {
        s.albedo = vec3f(0.58, 0.57, 0.53);
        s.rough = 0.5;
        s.porosity = 0.1;
        s.height = select(0.0, 0.01, fract(y * 10.0) > 0.5);
        s.wet_scale = 0.5;
    } else if (y >= 0.78) {
        let pane = clamp(floor(lu / 0.8), 0.0, wtot / 0.8 - 1.0);
        let lx = lu - pane * 0.8;
        var tan_u = vec3f(0.0, 0.0, 1.0);
        if (!front) {
            tan_u = vec3f(extra.y, 0.0, 0.0);
        }
        let rs = hash_uu(house, 100u);
        s = window_surface(s, vec2f(lx, y - 0.78), vec2f(0.8, 1.82), n, tan_u, v, rs, is_lit(house, 0u));
    }
    return s;
}

fn mat_roof(p: vec3f, uv: vec2f) -> Surf {
    var s = surf_default();
    let row = floor(uv.y / 0.23);
    let off = select(0.0, 0.15, (i32(row) & 1) == 1);
    let col = floor((uv.x + off) / 0.3);
    let fy = fract(uv.y / 0.23);
    let fx = fract((uv.x + off) / 0.3);
    let gap = 1.0 - smoothstep(0.0, 0.03, min(fx, 1.0 - fx));
    let h = rand2(vec2i(i32(col), i32(row)));
    let moss = smoothstep(0.62, 0.8, fbm(uv * 0.8 + vec2f(9.0, 1.0)));
    s.albedo = mix(vec3f(0.075, 0.08, 0.095) * (0.75 + 0.5 * h), vec3f(0.06, 0.08, 0.03), moss * 0.6);
    s.rough = 0.55;
    s.porosity = 0.08;
    s.height = fy * 0.008 - gap * 0.003;
    return s;
}

// ----------------------------------------------------------- dispatcher ----
fn eval_material(mat: u32, p: vec3f, n: vec3f, uv: vec2f, seed: f32, extra: vec2f, v: vec3f) -> Surf {
    switch (mat) {
        case 0u: { return mat_road(p); }
        case 1u: { return mat_pave(p); }
        case 2u: { return mat_kerb(p, n); }
        case 3u: { return mat_facade(p, n, uv, seed, v); }
        case 4u: { return mat_roof(p, uv); }
        case 5u: {
            var s = surf_default();
            s.albedo = vec3f(0.035, 0.045, 0.04);
            s.rough = 0.38;
            s.porosity = 0.0;
            s.metal = 0.3;
            return s;
        }
        case 6u: {
            var s = surf_default();
            s.albedo = vec3f(0.03);
            s.rough = 0.3;
            s.metal = 0.5;
            s.porosity = 0.0;
            if (n.y < -0.5) {
                // LED panel on the underside of the lamp head
                s.emit = frame.lamp.rgb * frame.lamp.w * 2600.0;
                s.albedo = vec3f(0.9);
                s.rough = 0.1;
            }
            return s;
        }
        case 7u: {
            var s = brick_base(uv * vec2f(1.0, 1.0) + vec2f(seed * 0.37, 0.0), seed + 500.0);
            return s;
        }
        case 8u: { return mat_bay(p, n, uv, seed, extra, v); }
        case 9u: { return brick_base(uv, seed + 900.0); }
        case 10u: {
            var s = surf_default();
            s.albedo = vec3f(0.32, 0.14, 0.08) * (0.8 + 0.3 * fbm(uv * 8.0));
            s.rough = 0.7;
            s.porosity = 0.4;
            return s;
        }
        case 11u: { return mat_garden(p); }
        case 12u: {
            var s = stone(surf_default(), uv);
            s.ground = 0.0;
            return s;
        }
        case 13u: {
            var s = surf_default();
            s.albedo = vec3f(0.12, 0.125, 0.13) * (0.8 + 0.3 * fbm(uv * 3.0));
            s.rough = 0.35;
            s.metal = 0.6;
            s.porosity = 0.0;
            return s;
        }
        default: { return eval_city_material(mat, p, n, uv, seed, extra, v); }
    }
    return surf_default();
}

// --------------------------------------------------------------- ripples ----
fn ripple_index(c: vec2i) -> u32 {
    let r = i32(frame.ripple.y);
    let x = ((c.x % r) + r) % r;
    let y = ((c.y % r) + r) % r;
    return u32(y * r + x);
}

fn rh(c: vec2i) -> f32 {
    return ripple_h[ripple_index(c)];
}

fn ripple_grad(c: vec2i) -> vec2f {
    return vec2f(rh(c + vec2i(1, 0)) - rh(c - vec2i(1, 0)), rh(c + vec2i(0, 1)) - rh(c - vec2i(0, 1)));
}

// Slope of the simulated water surface (bilinear), in height-per-metre.
fn sim_ripple_slope(p: vec2f) -> vec2f {
    let ts = frame.ripple.x;
    let q = p / ts - vec2f(0.5);
    let i = vec2i(floor(q));
    let f = fract(q);
    let g00 = ripple_grad(i);
    let g10 = ripple_grad(i + vec2i(1, 0));
    let g01 = ripple_grad(i + vec2i(0, 1));
    let g11 = ripple_grad(i + vec2i(1, 1));
    let g = mix(mix(g00, g10, f.x), mix(g01, g11, f.x), f.y);
    return g / (2.0 * ts);
}

// Cheap statistical ripples for puddles beyond the simulated area.
fn proc_ripple_slope(p: vec2f) -> vec2f {
    let t = frame.time.x;
    let rate = clamp(frame.weather.x / 8.0, 0.05, 3.0);
    var g = vec2f(0.0);
    for (var layer = 0; layer < 2; layer++) {
        let cs = 0.22;
        let q = p / cs + vec2f(f32(layer) * 0.5);
        let c = vec2i(floor(q));
        let h = cell2(c + vec2i(layer * 131, 7));
        let center = (vec2f(c) + vec2f(0.3) + vec2f(u01(h), u01(hash_u(h))) * 0.4) * cs;
        let period = 1.6 / rate;
        let ph = fract(t / period + u01(hash_u(h + 9u)));
        let age = ph * period;
        let d = distance(p - vec2f(f32(layer) * 0.5 * cs), center);
        let r = age * 0.25;
        let x = d - r;
        let env = exp(-x * x * 900.0) * exp(-age * 3.0) * smoothstep(0.11, 0.06, d);
        let w = cos(x * 260.0) * env;
        let dir = (p - vec2f(f32(layer) * 0.5 * cs) - center) / max(d, 1e-4);
        g += dir * w;
    }
    return g * 0.9;
}

fn puddle_normal(p: vec3f, dist: f32) -> vec3f {
    let fade = 1.0 - smoothstep(6.5, 10.0, dist);
    var slope = proc_ripple_slope(p.xz) * (1.0 - fade);
    if (fade > 0.0) {
        slope += sim_ripple_slope(p.xz) * frame.ripple.w * fade * 4.0;
    }
    // a little wind chop
    let w = frame.wind.xz * frame.time.x * 0.3;
    let chop = vec2f(gnoise(p.xz * 6.0 - w), gnoise(p.xz * 6.0 + vec2f(3.0, 1.0) - w)) * 0.015 * clamp(length(frame.wind.xz) / 8.0, 0.0, 1.0);
    slope += chop;
    return normalize(vec3f(-slope.x, 1.0, -slope.y));
}

// Samples the mirrored scene with a roughness-dependent vertical stretch,
// which gives wet-road lamp reflections their streaky look.
fn planar_reflection(suv: vec2f, n: vec3f, rough: f32) -> vec3f {
    let fwd = normalize(vec3f(frame.cam_fwd.x, 0.0, frame.cam_fwd.z) + vec3f(1e-5, 0.0, 0.0));
    let right = frame.cam_right.xyz;
    let k = frame.cam_fwd.w * frame.screen.z;
    let off = vec2f(dot(n, right) * 2.0 * k, -dot(n, fwd) * 2.0 * k * frame.screen.x * frame.screen.w);
    let base = suv + off * 0.5;
    let spread = rough * rough * 0.45 + rough * 0.015;
    var acc = vec3f(0.0);
    var wsum = 0.0;
    for (var i = 0; i < 9; i++) {
        let t = f32(i) / 8.0;
        let oy = mix(-0.35, 1.0, t) * spread;
        let ox = (t - 0.5) * spread * 0.12;
        let w = exp(-pow((t - 0.25) * 2.2, 2.0));
        let uv = clamp(base + vec2f(ox, oy), vec2f(0.001), vec2f(0.999));
        acc += textureSampleLevel(refl_tex, samp, uv, 0.0).rgb * w;
        wsum += w;
    }
    return acc / wsum;
}

// ------------------------------------------------------------- fragment ----
@fragment
fn fs_main(in: VOut) -> @location(0) vec4f {
    let p = in.wpos;
    var n0 = normalize(in.nrm);
    let cam = frame.cam_pos.xyz;
    let v = normalize(cam - p);
    // double-sided geometry: face the viewer
    if (dot(n0, v) < 0.0 && in.mat != M_LAMP) {
        n0 = -n0;
    }
    let refl_pass = frame.time.w > 0.5;
    let dist = distance(cam, p);

    var s = eval_material(in.mat, p, n0, in.uv, in.seed, in.extra, v);

    let wet = clamp(frame.weather.y * s.wet_scale, 0.0, 1.0);
    var pud = 0.0;
    if (s.ground > 0.5 && n0.y > 0.5) {
        pud = puddle_amount(p.xz) * smoothstep(0.2, 0.6, frame.weather.y);
    }

    // bump (must stay in uniform control flow)
    let bump = s.height * (1.0 - pud * 0.95);
    var n = perturb_normal(n0, p, bump);

    // wet surface model: porous materials darken, everything gets glossier
    s.albedo *= 1.0 - 0.6 * s.porosity * wet;
    let film = gnoise(p.xz * 9.0 + vec2f(2.0, 5.0)) * 0.5 + 0.5;
    let wet_rough = mix(0.06, 0.32, s.porosity * 0.7) * mix(0.55, 1.5, film * s.ground + 0.5 * (1.0 - s.ground));
    s.rough = mix(s.rough, wet_rough, wet * (1.0 - s.glass));
    if (pud > 0.001) {
        let pn = puddle_normal(p, dist);
        let k = smoothstep(0.0, 0.6, pud);
        n = normalize(mix(n, pn, k));
        s.rough = mix(s.rough, 0.015, k);
        s.f0 = mix(s.f0, 0.02, k);
        s.albedo *= 1.0 - 0.5 * k;
    }

    let nv = max(dot(n, v), 1e-3);
    let a = max(s.rough * s.rough, 0.0025);
    let a_spec = max(a, 0.012);
    let f0 = mix(vec3f(s.f0), s.albedo, s.metal);
    let diff = s.albedo * (1.0 - s.metal) * (1.0 - s.glass);

    var col = s.emit;
    let interior = in.mat >= 40u;

    // direct light
    let nl_count = frame.counts.x;
    for (var i = 0u; i < nl_count; i++) {
        let l = lights[i];
        let room_lamp = l.color.w > 2.5;
        if (room_lamp && !interior) {
            continue;   // room lamps don't light the street
        }
        let lv = l.pos.xyz - p;
        let d2 = dot(lv, lv);
        if (d2 > l.pos.w * l.pos.w) {
            continue;
        }
        let ldir = lv * inverseSqrt(d2);
        let nl = dot(n, ldir);
        if (nl <= 0.0) {
            continue;
        }
        var vis = 1.0;
        if (interior && !room_lamp) {
            vis = window_visibility(p, l.pos.xyz);
            if (vis <= 0.0) {
                continue;
            }
        }
        let e = l.color.rgb * lamp_profile(l, -ldir) * light_atten(d2, l.pos.w) * nl * vis;
        let h = normalize(ldir + v);
        let nh = max(dot(n, h), 0.0);
        let vh = max(dot(v, h), 0.0);
        let f = f_schlick(f0, vh);
        // lamps aren't points: widen the highlight by the source size, keep energy
        let src_r = select(0.35, 0.25, l.color.w < 0.5);
        let a_l = clamp(a_spec + src_r / (2.0 * sqrt(d2)), 0.0, 1.0);
        let norm = (a_spec / a_l) * (a_spec / a_l);
        let spec = d_ggx(nh, a_l) * norm * v_smith(nv, nl, a_l) * f;
        // the ground already sees lamp heads in the planar reflection
        let spec_w = select(1.0, 0.45, s.ground > 0.5 && !refl_pass);
        col += e * (diff * INV_PI * (vec3f(1.0) - f) + spec * spec_w);
        // soft fabric sheen on the plush
        if (in.mat == 44u || in.mat == 45u) {
            col += e * s.albedo * pow(1.0 - nv, 3.0) * 0.6;
        }
    }

    if (interior) {
        // warm room bounce plus the glow of the city coming in through the window
        let wc = vec3f(frame.win_b.y, (frame.win_a.w + frame.win_b.x) * 0.5, (frame.win_a.y + frame.win_a.z) * 0.5);
        let tw = wc - p;
        let fw = max(dot(n, normalize(tw)), 0.0) / (1.0 + dot(tw, tw) * 1.5);
        let amb_in = vec3f(1.0, 0.72, 0.48) * 0.012 * (0.6 + 0.4 * n.y) + frame.sky.rgb * frame.sky.w * fw * 2.5;
        col += diff * amb_in;
        let r = reflect(-v, n);
        let toward_window = smoothstep(0.2, 0.9, dot(r, normalize(tw)));
        let env_in = vec3f(1.0, 0.75, 0.5) * 0.02 + frame.sky.rgb * frame.sky.w * toward_window * 2.0;
        let fenv_in = f0 + (max(vec3f(1.0 - s.rough), f0) - f0) * pow(1.0 - nv, 5.0);
        col += env_in * fenv_in * (1.0 - s.rough * 0.7);
        if (s.cutout > 0.5) {
            discard;
        }
        return vec4f(col, 1.0);
    }

    // ambient from the glowing sky
    let amb = frame.sky.rgb * frame.sky.w * (0.35 + 0.65 * (n.y * 0.5 + 0.5)) * 0.5;
    col += diff * amb;

    // environment / mirror reflections
    let fenv = f0 + (max(vec3f(1.0 - s.rough), f0) - f0) * pow(1.0 - nv, 5.0);
    var env: vec3f;
    if (s.ground > 0.5 && !refl_pass) {
        let suv = in.clip.xy * frame.screen.zw;
        env = planar_reflection(suv, n, s.rough);
    } else {
        let r = reflect(-v, n);
        env = sky_radiance(r) * (1.0 - s.rough * 0.6);
    }
    col += env * fenv;

    // glass shows the room behind it
    if (s.glass > 0.5) {
        let fg = 0.04 + 0.96 * pow(1.0 - nv, 5.0);
        col += s.interior * (1.0 - fg);
    }

    col = apply_fog(col, cam, -v, dist);

    // the reflection pass must not draw the ground itself
    if (refl_pass && ((p.y < 0.2 && n0.y > 0.5) || p.y < -0.02)) {
        discard;
    }
    if (s.cutout > 0.5) {
        discard;
    }
    return vec4f(col, 1.0);
}

// How much of a light outside reaches a point inside the room: the ray must
// pass through the window opening.
fn window_visibility(p: vec3f, lp: vec3f) -> f32 {
    let xg = frame.win_b.y;
    if (p.x <= xg + 0.001) {
        return 1.0;
    }
    let d = lp - p;
    if (d.x >= -1e-4) {
        return 0.0;
    }
    let t = (xg - p.x) / d.x;
    let q = p + d * t;
    let mz = smoothstep(frame.win_a.y - 0.02, frame.win_a.y + 0.03, q.z) * smoothstep(frame.win_a.z + 0.02, frame.win_a.z - 0.03, q.z);
    let my = smoothstep(frame.win_a.w - 0.02, frame.win_a.w + 0.03, q.y) * smoothstep(frame.win_b.x + 0.02, frame.win_b.x - 0.03, q.y);
    return mz * my;
}

// ------------------------------------------------------------------ sky ----
struct SkyOut {
    @builtin(position) clip: vec4f,
    @location(0) ndc: vec2f,
}

@vertex
fn vs_sky(@builtin(vertex_index) vi: u32) -> SkyOut {
    var o: SkyOut;
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    let ndc = vec2f(x * 2.0 - 1.0, y * 2.0 - 1.0);
    o.clip = vec4f(ndc, 0.0, 1.0);
    o.ndc = ndc;
    return o;
}

@fragment
fn fs_sky(in: SkyOut) -> @location(0) vec4f {
    let wp = frame.inv_view_proj * vec4f(in.ndc, 1.0, 1.0);
    let rd = normalize(wp.xyz / wp.w - frame.cam_pos.xyz);
    var c = sky_radiance(rd);
    c = apply_fog(c, frame.cam_pos.xyz, rd, 180.0);
    return vec4f(c, 1.0);
}
