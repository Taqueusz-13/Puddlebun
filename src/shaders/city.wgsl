// ============================================================================
// city.wgsl — materials for the neon city and the room by the window.
// Concatenated into the scene shader (uses Surf, brick_base, stone,
// window_surface from scene.wgsl).
// ============================================================================

const NEON_COLORS = array<vec3f, 7>(
    vec3f(1.0, 0.08, 0.45),
    vec3f(0.1, 0.85, 1.0),
    vec3f(1.0, 0.12, 0.05),
    vec3f(1.0, 0.55, 0.08),
    vec3f(0.25, 1.0, 0.35),
    vec3f(0.6, 0.2, 1.0),
    vec3f(0.75, 0.85, 1.0)
);

fn neon_color(i: u32) -> vec3f {
    var c = NEON_COLORS;
    return c[min(i, 6u)];
}

// Same as `neon_flicker` in src/scene.rs.
fn neon_flicker(seed: u32, t: f32) -> f32 {
    let kind = seed % 7u;
    if (kind == 0u) {
        let k = hash_uu(seed, u32(t * 14.0));
        return select(1.0, 0.15, u01(k) < 0.12);
    }
    if (kind == 1u) {
        return select(0.08, 1.0, fract(t * 0.45 + u01(hash_u(seed))) < 0.7);
    }
    return 1.0;
}

// ---- 5x7 neon tube font (generated) ----
var<private> FONT: array<vec2u, 37> = array<vec2u, 37>(
    vec2u(1033774u, 17969u),
    vec2u(509487u, 15921u),
    vec2u(34350u, 14881u),
    vec2u(575023u, 15921u),
    vec2u(492607u, 31777u),
    vec2u(492607u, 1057u),
    vec2u(951854u, 31281u),
    vec2u(1033777u, 17969u),
    vec2u(135310u, 14468u),
    vec2u(270620u, 6440u),
    vec2u(103729u, 17701u),
    vec2u(33825u, 31777u),
    vec2u(710513u, 17969u),
    vec2u(708145u, 17977u),
    vec2u(575022u, 14897u),
    vec2u(509487u, 1057u),
    vec2u(575022u, 22837u),
    vec2u(509487u, 17701u),
    vec2u(459838u, 15888u),
    vec2u(135327u, 4228u),
    vec2u(575025u, 14897u),
    vec2u(575025u, 4433u),
    vec2u(706097u, 10933u),
    vec2u(141873u, 17962u),
    vec2u(345649u, 4228u),
    vec2u(139807u, 31778u),
    vec2u(714286u, 14899u),
    vec2u(135364u, 14468u),
    vec2u(279086u, 31812u),
    vec2u(266527u, 14896u),
    vec2u(305544u, 8479u),
    vec2u(539711u, 14896u),
    vec2u(492620u, 14897u),
    vec2u(139807u, 2114u),
    vec2u(476718u, 14897u),
    vec2u(1001006u, 6416u),
    vec2u(0u, 0u)
);
var<private> WORDS: array<u32, 240> = array<u32, 240>(
    7u, 14u, 19u, 4u, 11u, 255u, 255u, 255u, 255u, 255u, 3u, 8u, 13u, 4u, 17u, 255u, 255u, 255u, 255u, 255u, 14u, 15u, 4u, 13u, 255u, 255u, 255u, 255u, 255u, 255u, 1u, 0u, 17u, 255u, 255u, 255u, 255u, 255u, 255u, 255u, 15u, 8u, 25u, 25u, 0u, 255u, 255u, 255u, 255u, 255u, 13u, 14u, 14u, 3u, 11u, 4u, 18u, 255u, 255u, 255u, 2u, 0u, 5u, 4u, 255u, 255u, 255u, 255u, 255u, 255u, 11u, 8u, 16u, 20u, 14u, 17u, 255u, 255u, 255u, 255u, 28u, 30u, 7u, 255u, 255u, 255u, 255u, 255u, 255u, 255u, 17u, 0u, 12u, 4u, 13u, 255u, 255u, 255u, 255u, 255u, 12u, 14u, 19u, 4u, 11u, 255u, 255u, 255u, 255u, 255u, 10u, 0u, 17u, 0u, 14u, 10u, 4u, 255u, 255u, 255u, 0u, 17u, 2u, 0u, 3u, 4u, 255u, 255u, 255u, 255u, 11u, 0u, 20u, 13u, 3u, 17u, 24u, 255u, 255u, 255u, 3u, 4u, 11u, 8u, 255u, 255u, 255u, 255u, 255u, 255u, 9u, 0u, 25u, 25u, 255u, 255u, 255u, 255u, 255u, 255u, 19u, 0u, 19u, 19u, 14u, 14u, 255u, 255u, 255u, 255u, 15u, 20u, 3u, 3u, 11u, 4u, 1u, 20u, 13u, 255u, 1u, 14u, 14u, 10u, 18u, 255u, 255u, 255u, 255u, 255u, 17u, 4u, 2u, 14u, 17u, 3u, 18u, 255u, 255u, 255u, 17u, 0u, 8u, 13u, 36u, 2u, 8u, 19u, 24u, 255u, 18u, 24u, 13u, 19u, 7u, 22u, 0u, 21u, 4u, 255u, 13u, 4u, 14u, 13u, 255u, 255u, 255u, 255u, 255u, 255u, 3u, 17u, 4u, 0u, 12u, 18u, 255u, 255u, 255u, 255u
);


fn word_len(w: u32) -> u32 {
    var n = 0u;
    for (var i = 0u; i < 10u; i++) {
        if (WORDS[w * 10u + i] == 255u) {
            break;
        }
        n += 1u;
    }
    return n;
}

fn glyph_bit(g: u32, c: vec2i) -> bool {
    if (c.x < 0 || c.x > 4 || c.y < 0 || c.y > 6 || g > 35u) {
        return false;
    }
    let f = FONT[g];
    if (c.y < 4) {
        return ((f.x >> u32(c.y * 5 + c.x)) & 1u) == 1u;
    }
    return ((f.y >> u32((c.y - 4) * 5 + c.x)) & 1u) == 1u;
}

fn seg_dist(p: vec2f, a: vec2f, b: vec2f) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / dot(ba, ba), 0.0, 1.0);
    return length(pa - ba * h);
}

// Distance (in glyph pixels) to the neon tube of glyph g; p: x 0..5, y 0..7 (y down).
fn neon_dist(g: u32, p: vec2f) -> f32 {
    let c = vec2i(floor(p));
    var d = 9.0;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let q = c + vec2i(dx, dy);
            if (!glyph_bit(g, q)) {
                continue;
            }
            let qc = vec2f(q) + vec2f(0.5);
            d = min(d, length(p - qc));
            if (glyph_bit(g, q + vec2i(1, 0))) {
                d = min(d, seg_dist(p, qc, qc + vec2f(1.0, 0.0)));
            }
            if (glyph_bit(g, q + vec2i(0, 1))) {
                d = min(d, seg_dist(p, qc, qc + vec2f(0.0, 1.0)));
            }
            if (glyph_bit(g, q + vec2i(1, 1)) && !glyph_bit(g, q + vec2i(1, 0)) && !glyph_bit(g, q + vec2i(0, 1))) {
                d = min(d, seg_dist(p, qc, qc + vec2f(1.0, 1.0)));
            }
            if (glyph_bit(g, q + vec2i(-1, 1)) && !glyph_bit(g, q + vec2i(-1, 0)) && !glyph_bit(g, q + vec2i(0, 1))) {
                d = min(d, seg_dist(p, qc, qc + vec2f(-1.0, 1.0)));
            }
        }
    }
    return d;
}

// Neon text distance for a word laid out on a sign of `size` (m), at uv (m).
// Returns (distance in glyph pixels, char index or -1).
fn sign_text(word: u32, vertical: bool, uv: vec2f, size: vec2f) -> vec2f {
    let n = word_len(word);
    if (!vertical) {
        let unit = size.y / 9.0;
        let total = f32(n) * 6.0 - 1.0;
        let x0 = (size.x / unit - total) * 0.5;
        let gx = uv.x / unit - x0;
        let gy = (size.y - uv.y) / unit - 1.0;
        let ci = floor(gx / 6.0);
        if (ci < 0.0 || ci >= f32(n)) {
            return vec2f(9.0, -1.0);
        }
        let g = WORDS[word * 10u + u32(ci)];
        if (g > 35u) {
            return vec2f(9.0, ci);
        }
        return vec2f(neon_dist(g, vec2f(gx - ci * 6.0, gy)), ci);
    }
    let unit = size.x / 7.0;
    let total = f32(n) * 8.0;
    let y0 = (size.y / unit - total) * 0.5;
    let gyy = (size.y - uv.y) / unit - y0;
    let ci = floor(gyy / 8.0);
    if (ci < 0.0 || ci >= f32(n)) {
        return vec2f(9.0, -1.0);
    }
    let g = WORDS[word * 10u + u32(ci)];
    if (g > 35u) {
        return vec2f(9.0, ci);
    }
    return vec2f(neon_dist(g, vec2f(uv.x / unit - 1.0, gyy - ci * 8.0 - 0.5)), ci);
}

fn mat_neon(p: vec3f, uv: vec2f, seed: f32, extra: vec2f) -> Surf {
    var s = surf_default();
    let packed = u32(seed + 0.5);
    let word = packed % 32u;
    let col = (packed / 32u) % 8u;
    let vertical = ((packed / 256u) % 2u) == 1u;
    let sseed = packed / 512u;
    let size = extra;
    let t = frame.time.x;
    let st = sign_text(word, vertical, uv, size);
    var d = st.x;
    // some signs have a border tube
    if (sseed % 3u == 0u) {
        let unit = select(size.y / 9.0, size.x / 7.0, vertical);
        let q = uv / unit;
        let e = size / unit;
        let bd = min(min(q.x - 0.45, e.x - 0.45 - q.x), min(q.y - 0.45, e.y - 0.45 - q.y));
        d = min(d, abs(bd));
    }
    var flick = neon_flicker(sseed, t);
    // a single letter with a dying tube
    if (st.y >= 0.0 && hash_uu(sseed, u32(st.y)) % 9u == 0u) {
        flick *= select(1.0, 0.1, u01(hash_uu(sseed + u32(st.y), u32(t * 9.0))) < 0.3);
    }
    let c = neon_color(col);
    let core = smoothstep(0.34, 0.1, d);
    let hot = smoothstep(0.16, 0.0, d);
    s.emit = (c * (core * 16.0 + exp(-d * 1.8) * 1.4) + vec3f(1.0) * hot * 7.0) * flick;
    s.albedo = vec3f(0.025, 0.025, 0.03);
    s.rough = 0.4;
    s.porosity = 0.0;
    s.wet_scale = 0.4;
    return s;
}

// The hologram billboard: cycling words over a shifting gradient.
fn mat_holo(uv: vec2f, extra: vec2f) -> Surf {
    var s = surf_default();
    s.albedo = vec3f(0.0);
    s.rough = 0.5;
    s.porosity = 0.0;
    let t = frame.time.x;
    let n = uv / extra;
    let words = array<u32, 4>(17u, 20u, 5u, 21u);
    var wsel = words;
    let wi = u32(t / 7.0) % 4u;
    let word = wsel[wi];
    // glitchy horizontal offsets now and then
    let band = floor(n.y * 24.0);
    let glitch = select(0.0, (u01(hash_uu(u32(band), u32(t * 6.0))) - 0.5) * 0.06, u01(hash_u(u32(t * 3.0))) > 0.86);
    let gx = uv.x + glitch * extra.x;
    let nlen = f32(word_len(word));
    let th = extra.x / (6.0 * nlen + 1.0) * 9.0;
    let ty = extra.y * 0.5 - th * 0.5;
    var d = 9.0;
    if (uv.y > ty && uv.y < ty + th) {
        d = sign_text(word, false, vec2f(gx, uv.y - ty), vec2f(extra.x, th)).x;
    }
    let grad = mix(vec3f(1.0, 0.1, 0.6), vec3f(0.1, 0.8, 1.0), 0.5 + 0.5 * sin(n.x * 3.0 + n.y * 2.0 + t * 0.4));
    let scan = 0.65 + 0.35 * sin(n.y * 420.0 + t * 8.0);
    let fade = smoothstep(0.0, 0.06, n.x) * smoothstep(1.0, 0.94, n.x) * smoothstep(0.0, 0.06, n.y) * smoothstep(1.0, 0.94, n.y);
    let text = smoothstep(0.5, 0.15, d) * 6.0 + exp(-d * 0.9) * 0.8;
    let field = 0.18 + 0.12 * sin(n.y * 9.0 - t * 1.3);
    s.emit = grad * (text + field) * scan * fade * 1.4;
    return s;
}

fn city_window_lit(m: u32, floor_i: u32, col: u32) -> bool {
    return u01(hash_uu(m * 97u + floor_i * 5u + col, 31u)) < frame.misc.y * 1.35;
}

fn mat_cfacade(p: vec3f, n: vec3f, uv: vec2f, seed: f32, v: vec3f) -> Surf {
    let m = u32(seed + 0.5);
    let tan_u = vec3f(0.0, 0.0, 1.0);
    var s: Surf;
    let painted = u01(hash_uu(m, 3u)) > 0.78;
    if (painted) {
        s = surf_default();
        let pc = u01(hash_uu(m, 4u));
        s.albedo = mix(vec3f(0.3, 0.32, 0.36), vec3f(0.42, 0.36, 0.3), pc) * (0.85 + 0.25 * fbm(uv * 1.7));
        s.rough = 0.8;
        s.porosity = 0.5;
        s.height = gnoise(uv * 22.0) * 0.0015;
        let streak = fbm(vec2f(uv.x * 3.0, uv.y * 0.15 + seed));
        s.wet_scale = 0.55 + 0.45 * smoothstep(0.35, 0.7, streak);
    } else {
        s = brick_base(uv, seed);
    }
    let u = uv.x;
    let y = uv.y;
    if (y < 4.5) {
        return shopfront(s, p, n, uv, m, v, tan_u);
    }
    let fy = (y - 4.5) / 3.2;
    let fl = floor(fy);
    let ly = (y - 4.5) - fl * 3.2;
    let um = u - C_MODULE * floor(u / C_MODULE);
    let colf = floor((um - 0.05) / 2.5);
    let cu = 1.3 + colf * 2.5;
    let lx = um - (cu - 0.5);
    let wsize = vec2f(1.0, 1.7);
    if (colf >= 0.0 && colf <= 2.0) {
        if (ly > 2.65 && ly < 2.85 && lx > -0.1 && lx < 1.1) {
            return stone(s, uv);
        }
        if (ly > 0.85 && ly < 0.95 && lx > -0.12 && lx < 1.12) {
            var st = stone(s, uv);
            st.height = 0.012;
            return st;
        }
        if (lx > 0.0 && lx < 1.0 && ly > 0.95 && ly < 2.65) {
            let rs = hash_uu(m * 131u + u32(fl) * 7u, u32(colf) + 300u);
            return window_surface(s, vec2f(lx, ly - 0.95), wsize, n, tan_u, v, rs, city_window_lit(m, u32(fl), u32(colf)));
        }
    }
    return s;
}

fn shopfront(base: Surf, p: vec3f, n: vec3f, uv: vec2f, m: u32, v: vec3f, tan_u: vec3f) -> Surf {
    var s = base;
    let u = uv.x - C_MODULE * floor(uv.x / C_MODULE);
    let y = uv.y;
    let closed = u01(hash_uu(m, 5u)) <= 0.25;
    // pilasters and the sign band
    if (u < 0.35 || u > C_MODULE - 0.35 || y > 3.75) {
        s = surf_default();
        let pc = u01(hash_uu(m, 9u));
        s.albedo = mix(vec3f(0.05, 0.05, 0.055), vec3f(0.14, 0.05, 0.04), step(0.6, pc)) * (0.8 + 0.4 * fbm(uv * 3.0));
        s.rough = 0.35;
        s.porosity = 0.05;
        s.height = select(0.0, 0.01, y > 3.75 && y < 3.8);
        return s;
    }
    if (closed && y < 3.6) {
        // roll-down shutter
        s = surf_default();
        let rib = fract(y * 9.0);
        s.albedo = vec3f(0.17, 0.17, 0.18) * (0.75 + 0.35 * fbm(uv * vec2f(0.5, 6.0)));
        s.metal = 0.6;
        s.rough = 0.45;
        s.porosity = 0.0;
        s.height = (rib - 0.5) * 0.004;
        return s;
    }
    let rs = hash_uu(m, 777u) | 0x80000000u;
    if (u > 0.6 && u < 5.3 && y > 0.55 && y < 3.6) {
        return window_surface(s, vec2f(u - 0.6, y - 0.55), vec2f(4.7, 3.05), n, tan_u, v, rs, true);
    }
    if (u > 5.75 && u < 6.95 && y > 0.15 && y < 3.0) {
        return window_surface(s, vec2f(u - 5.75, y - 0.15), vec2f(1.2, 2.85), n, tan_u, v, rs ^ 1u, true);
    }
    s = surf_default();
    s.albedo = vec3f(0.04, 0.045, 0.05);
    s.rough = 0.3;
    s.porosity = 0.0;
    return s;
}

fn mat_awning(uv: vec2f, seed: f32, extra: vec2f) -> Surf {
    var s = surf_default();
    let m = u32(seed + 0.5);
    let pick = hash_uu(m, 21u) % 6u;
    var a = vec3f(0.4, 0.04, 0.04);
    var b = vec3f(0.62, 0.6, 0.55);
    if (pick == 1u) { a = vec3f(0.03, 0.18, 0.08); }
    if (pick == 2u) { a = vec3f(0.03, 0.05, 0.16); b = vec3f(0.55, 0.5, 0.38); }
    if (pick == 3u) { a = vec3f(0.02, 0.02, 0.025); b = a; }
    if (pick == 4u) { a = vec3f(0.25, 0.03, 0.06); b = a * 1.4; }
    if (pick == 5u) { a = vec3f(0.6, 0.22, 0.03); }
    let stripe = step(0.5, fract(uv.x / 0.5));
    s.albedo = mix(a, b, stripe) * (0.8 + 0.3 * fbm(uv * 4.0));
    s.rough = 0.75;
    s.porosity = 0.6;
    s.height = sin(uv.x * 6.28 / 0.5) * 0.002;
    return s;
}

fn mat_tower(uv: vec2f, seed: f32) -> Surf {
    var s = surf_default();
    let t = u32(seed + 0.5);
    let cell = vec2f(floor(uv.x / 1.6), floor(uv.y / 3.6));
    let f = vec2f(fract(uv.x / 1.6), fract(uv.y / 3.6));
    let mull = 1.0 - step(0.08, min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y) * 1.8));
    let dens = 0.15 + 0.5 * u01(hash_uu(t, 1u));
    let band = fbm(vec2f(cell.y * 0.21, f32(t)));
    let lit = u01(cell2(vec2i(cell) + vec2i(i32(t) * 37, 11))) < dens * (0.25 + band);
    let tint = u01(hash_uu(t, 2u));
    var lc = vec3f(0.75, 0.85, 1.0);
    if (tint > 0.7) { lc = vec3f(1.0, 0.75, 0.5); }
    if (tint > 0.9) { lc = vec3f(0.7, 0.4, 1.0); }
    s.albedo = mix(vec3f(0.03, 0.035, 0.045), vec3f(0.02), mull);
    s.rough = mix(0.12, 0.5, mull);
    s.metal = 0.3;
    s.porosity = 0.0;
    s.f0 = 0.06;
    if (lit && mull < 0.5) {
        s.emit = lc * (0.6 + 0.8 * u01(cell2(vec2i(cell) + vec2i(5, i32(t))))) * 1.3;
    }
    // LED strips up some corners (Night City)
    if (u01(hash_uu(t, 3u)) > 0.7 && uv.x < 0.5) {
        s.emit += neon_color(hash_uu(t, 4u) % 7u) * 4.0;
    }
    s.wet_scale = 0.6;
    return s;
}

fn cutout_grate(uv: vec2f) -> bool {
    return fract(uv.y / 0.045) > 0.62;
}

fn mat_iron(cut: bool) -> Surf {
    var s = surf_default();
    s.albedo = vec3f(0.035, 0.035, 0.038);
    s.rough = 0.45;
    s.metal = 0.6;
    s.porosity = 0.0;
    s.cutout = select(0.0, 1.0, cut);
    return s;
}

fn mat_rail(uv: vec2f, extra: vec2f) -> Surf {
    let h = extra.x;
    let v = uv.y;
    let solid = v > h - 0.045 || (v > 0.46 && v < 0.49) || v < 0.035 || abs(fract(uv.x / 0.12) - 0.5) * 0.12 < 0.008;
    return mat_iron(!solid);
}

fn mat_stair(uv: vec2f) -> Surf {
    let solid = uv.x < 0.04 || uv.x > 0.58 || fract(uv.y / 0.26) < 0.42;
    var s = mat_iron(!solid);
    if (solid && uv.x > 0.04 && uv.x < 0.58) {
        s.cutout = select(0.0, 1.0, fract(uv.x / 0.05) > 0.7);
    }
    return s;
}

fn mat_wood(uv: vec2f, seed: f32) -> Surf {
    var s = surf_default();
    let stave = fract(uv.x / 0.14);
    let hoop = fract(uv.y / 0.75) < 0.05;
    let h = rand2(vec2i(i32(floor(uv.x / 0.14)), i32(seed)));
    s.albedo = vec3f(0.13, 0.085, 0.055) * (0.65 + 0.6 * h) * (0.8 + 0.3 * fbm(uv * vec2f(2.0, 0.3)));
    s.rough = 0.8;
    s.porosity = 0.6;
    s.height = -(1.0 - smoothstep(0.0, 0.06, min(stave, 1.0 - stave))) * 0.004;
    if (hoop) {
        s.albedo = vec3f(0.05);
        s.metal = 0.6;
        s.rough = 0.4;
        s.porosity = 0.0;
        s.height = 0.004;
    }
    return s;
}

fn mat_cornice(p: vec3f) -> Surf {
    var s = surf_default();
    let dent = step(0.5, fract(p.z / 0.22));
    s.albedo = vec3f(0.16, 0.17, 0.16) * (0.75 + 0.4 * fbm(p.zy * 3.0));
    s.rough = 0.55;
    s.porosity = 0.3;
    s.height = dent * 0.01 * step(fract(p.y), 0.3);
    return s;
}

fn mat_city_road(p: vec3f) -> Surf {
    var s = asphalt(p);
    let ax = abs(p.x);
    // double yellow centre line
    let yl = (1.0 - step(0.05, abs(ax - 0.13))) * smoothstep(0.2, 0.55, fbm(p.xz * 5.0));
    // dashed white lane lines
    let wl = (1.0 - step(0.06, abs(ax - 3.0))) * step(fract(p.z / 9.0), 0.33);
    // crosswalk
    let cz = p.z - 120.0 * round(p.z / 120.0);
    let cw = step(abs(cz - 38.0), 2.0) * step(0.5, fract(p.x / 0.9)) * step(ax, 5.6);
    let paint = max(max(yl, wl), cw) * smoothstep(0.25, 0.6, fbm(p.xz * 3.0 + vec2f(9.0)));
    let pc = select(vec3f(0.6, 0.6, 0.58), vec3f(0.55, 0.42, 0.05), yl > 0.5);
    s.albedo = mix(s.albedo, pc, paint);
    s.rough = mix(s.rough, 0.5, paint);
    s.porosity = mix(s.porosity, 0.15, paint);
    s.height += paint * 0.0015;
    return s;
}

fn mat_cwalk(p: vec3f) -> Surf {
    var s = surf_default();
    let q = p.xz / 1.52;
    let cell = vec2i(floor(q));
    let f = fract(q);
    let joint = min(min(f.x, 1.0 - f.x), min(f.y, 1.0 - f.y)) * 1.52;
    let j = 1.0 - smoothstep(0.002, 0.008, joint);
    let h = rand2(cell + vec2i(7, 3));
    let a = 0.22 + 0.08 * h + 0.05 * (fbm(p.xz * 1.5) - 0.5);
    s.albedo = mix(vec3f(a, a * 0.98, a * 0.96), vec3f(0.07), j);
    s.rough = 0.85;
    s.porosity = 0.5;
    s.height = -j * 0.004 + (h - 0.5) * 0.002;
    s.ground = 1.0;
    // steel cellar doors near the buildings
    let ax = abs(p.x);
    let mz = p.z - C_MODULE * floor(p.z / C_MODULE);
    if (ax > 8.4 && ax < 9.7 && mz > 2.0 && mz < 3.5 && u01(hash_u(u32(i32(floor(p.z / C_MODULE)) + 900))) > 0.5) {
        let dp = step(0.5, fract((p.x + p.z) * 14.0)) * step(0.5, fract((p.x - p.z) * 14.0));
        s.albedo = vec3f(0.08 + dp * 0.03);
        s.metal = 0.8;
        s.rough = 0.35;
        s.porosity = 0.0;
        s.height = dp * 0.0015;
    }
    return s;
}

fn eval_city_material(mat: u32, p: vec3f, n: vec3f, uv: vec2f, seed: f32, extra: vec2f, v: vec3f) -> Surf {
    switch (mat) {
        case 20u: { return mat_city_road(p); }
        case 21u: { return mat_cwalk(p); }
        case 22u: { return mat_kerb(p, n); }
        case 23u: { return mat_cfacade(p, n, uv, seed, v); }
        case 25u: { return mat_awning(uv, seed, extra); }
        case 26u: { return mat_neon(p, uv, seed, extra); }
        case 27u: {
            var s = surf_default();
            s.albedo = vec3f(0.03);
            s.rough = 0.35;
            s.metal = 0.5;
            s.porosity = 0.0;
            return s;
        }
        case 28u: { return mat_tower(uv, seed); }
        case 29u: { return mat_holo(uv, extra); }
        case 30u: { return mat_iron(cutout_grate(uv)); }
        case 31u: { return mat_rail(uv, extra); }
        case 32u: { return mat_wood(uv, seed); }
        case 33u: { return mat_cornice(p); }
        case 34u: { return stone(surf_default(), uv); }
        case 35u: {
            var s = surf_default();
            s.albedo = vec3f(0.1, 0.01, 0.01);
            let blink = step(0.55, fract(frame.time.x * 0.6 + u01(hash_u(u32(seed)))));
            s.emit = vec3f(1.0, 0.05, 0.02) * 40.0 * blink;
            return s;
        }
        case 36u: {
            var s = surf_default();
            s.albedo = vec3f(0.045, 0.045, 0.05) * (0.8 + 0.4 * fbm(p.xz * 0.5));
            s.rough = 0.6;
            s.porosity = 0.3;
            return s;
        }
        case 37u: { return mat_stair(uv); }
        case 38u: {
            var s = surf_default();
            let gr = step(0.5, fract(p.y * 30.0));
            s.albedo = vec3f(0.42, 0.4, 0.36) * (0.8 + 0.2 * gr);
            s.rough = 0.5;
            s.metal = 0.3;
            s.porosity = 0.0;
            s.height = gr * 0.001;
            return s;
        }
        default: { return eval_room_material(mat, p, n, uv, seed, extra, v); }
    }
    return surf_default();
}

// ------------------------------------------------------------------ room ----
fn eval_room_material(mat: u32, p: vec3f, n: vec3f, uv: vec2f, seed: f32, extra: vec2f, v: vec3f) -> Surf {
    var s = surf_default();
    s.wet_scale = 0.0;
    s.interior = vec3f(0.0);
    switch (mat) {
        case 40u: {
            // painted plaster: a muted dusty sage (warm under the lamp, cool by the window), ceiling paler
            let c = select(vec3f(0.33, 0.36, 0.34), vec3f(0.55, 0.54, 0.52), seed > 0.5);
            s.albedo = c * (0.92 + 0.1 * fbm(uv * 7.0));
            s.rough = 0.85;
            s.height = gnoise(uv * 40.0) * 0.0004 + fbm(uv * 3.0) * 0.001;
        }
        case 41u, 47u: {
            // layers of old gloss paint on wood
            s.albedo = select(vec3f(0.6, 0.57, 0.5), vec3f(0.55, 0.52, 0.46), mat == 47u);
            s.rough = 0.38;
            s.height = gnoise(uv * 18.0) * 0.0006 + gnoise(uv * 70.0) * 0.0002;
        }
        case 42u: {
            // varnished wooden sill, wet near the window when it's been open
            let grain = sin(p.z * 90.0 + gnoise(p.xz * vec2f(8.0, 2.0)) * 6.0) * 0.5 + 0.5;
            s.albedo = vec3f(0.3, 0.16, 0.08) * (0.75 + 0.35 * grain);
            s.rough = 0.32;
            s.height = grain * 0.0003;
            let near = 1.0 - smoothstep(10.22, 10.42, p.x);
            let patchy = smoothstep(0.35, 0.65, fbm(p.xz * 18.0));
            s.wet_scale = frame.room.w * near * (0.4 + 0.6 * patchy);
            s.porosity = 0.25;
        }
        case 43u: {
            // cast-iron radiator section: two rounded columns
            let u = uv.x;
            let c0 = max(0.0, 1.0 - pow((u - 0.055) / 0.045, 2.0));
            let c1 = max(0.0, 1.0 - pow((u - 0.145) / 0.045, 2.0));
            // painted cream long ago
            s.albedo = vec3f(0.42, 0.4, 0.35);
            s.metal = 0.15;
            s.rough = 0.6;
            s.height = (sqrt(c0) + sqrt(c1)) * 0.012 + gnoise(p.yz * 90.0) * 0.0004;
        }
        case 44u, 45u: {
            s.albedo = select(vec3f(0.8, 0.76, 0.72), vec3f(0.9, 0.55, 0.62), mat == 45u);
            s.rough = 0.95;
            s.height = gnoise(uv * vec2f(200.0, 140.0)) * 0.0004;
        }
        case 46u: {
            s.albedo = vec3f(0.01);
            s.rough = 0.08;
        }
        case 48u: {
            let plank = floor(p.z / 0.14);
            let h = rand2(vec2i(i32(plank), 3));
            s.albedo = vec3f(0.24, 0.13, 0.07) * (0.7 + 0.5 * h);
            s.rough = 0.45;
            s.height = -(1.0 - smoothstep(0.0, 0.004, abs(fract(p.z / 0.14) - 0.5) * 0.14 - 0.066)) * 0.002;
        }
        case 49u: {
            s.albedo = vec3f(0.75, 0.55, 0.25);
            s.metal = 1.0;
            s.rough = 0.3;
        }
        default: {}
    }
    return s;
}
