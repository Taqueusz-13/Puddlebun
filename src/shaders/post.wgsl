// ============================================================================
// post.wgsl — physically based bloom (dual filter, Jimenez 2014) and the
// final AgX tonemap with vignette and a touch of film grain.
// ============================================================================

struct PostParams {
    texel: vec4f,     // xy = 1/size of the source texture, z = karis (1 on first downsample), w = radius
    exposure: f32,
    bloom: f32,
    time: f32,
    encode_srgb: f32, // 1 if the output target is not an sRGB format
    vignette: f32,
    grain: f32,
    pad0: f32,
    pad1: f32,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> pp: PostParams;
@group(0) @binding(3) var bloom_tex: texture_2d<f32>;

struct FsIn {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_full(@builtin(vertex_index) vi: u32) -> FsIn {
    var o: FsIn;
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    o.clip = vec4f(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2f(x, y);
    return o;
}

fn smp(uv: vec2f) -> vec3f {
    return textureSampleLevel(src, samp, uv, 0.0).rgb;
}

fn karis_weight(c: vec3f) -> f32 {
    return 1.0 / (1.0 + max(max(c.r, c.g), c.b) * 0.25);
}

@fragment
fn fs_down(in: FsIn) -> @location(0) vec4f {
    let t = pp.texel.xy;
    let uv = in.uv;
    let a = smp(uv + t * vec2f(-2.0, -2.0));
    let b = smp(uv + t * vec2f(0.0, -2.0));
    let c = smp(uv + t * vec2f(2.0, -2.0));
    let d = smp(uv + t * vec2f(-2.0, 0.0));
    let e = smp(uv);
    let f = smp(uv + t * vec2f(2.0, 0.0));
    let g = smp(uv + t * vec2f(-2.0, 2.0));
    let h = smp(uv + t * vec2f(0.0, 2.0));
    let i = smp(uv + t * vec2f(2.0, 2.0));
    let j = smp(uv + t * vec2f(-1.0, -1.0));
    let k = smp(uv + t * vec2f(1.0, -1.0));
    let l = smp(uv + t * vec2f(-1.0, 1.0));
    let m = smp(uv + t * vec2f(1.0, 1.0));
    var col: vec3f;
    if (pp.texel.z > 0.5) {
        // Karis average on the first pass stops single bright splash glints
        // from turning into flickering bloom blobs.
        let g0 = (a + b + d + e) * 0.25;
        let g1 = (b + c + e + f) * 0.25;
        let g2 = (d + e + g + h) * 0.25;
        let g3 = (e + f + h + i) * 0.25;
        let g4 = (j + k + l + m) * 0.25;
        let w0 = karis_weight(g0) * 0.125;
        let w1 = karis_weight(g1) * 0.125;
        let w2 = karis_weight(g2) * 0.125;
        let w3 = karis_weight(g3) * 0.125;
        let w4 = karis_weight(g4) * 0.5;
        col = (g0 * w0 + g1 * w1 + g2 * w2 + g3 * w3 + g4 * w4) / (w0 + w1 + w2 + w3 + w4);
    } else {
        col = e * 0.125 + (a + c + g + i) * 0.03125 + (b + d + f + h) * 0.0625 + (j + k + l + m) * 0.125;
    }
    return vec4f(max(col, vec3f(0.0)), 1.0);
}

@fragment
fn fs_up(in: FsIn) -> @location(0) vec4f {
    let t = pp.texel.xy * pp.texel.w;
    let uv = in.uv;
    var col = smp(uv) * 4.0;
    col += (smp(uv + vec2f(-t.x, 0.0)) + smp(uv + vec2f(t.x, 0.0)) + smp(uv + vec2f(0.0, -t.y)) + smp(uv + vec2f(0.0, t.y))) * 2.0;
    col += smp(uv + vec2f(-t.x, -t.y)) + smp(uv + vec2f(t.x, -t.y)) + smp(uv + vec2f(-t.x, t.y)) + smp(uv + vec2f(t.x, t.y));
    return vec4f(col / 16.0, 1.0);
}

// ----------------------------------------------------------------- AgX ----
fn agx_contrast(x: vec3f) -> vec3f {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2 - 40.14 * x4 * x + 31.96 * x4 - 6.868 * x2 * x + 0.4298 * x2 + 0.1191 * x - 0.00232;
}

fn agx(val: vec3f) -> vec3f {
    let m = mat3x3f(
        0.842479062253094, 0.0423282422610123, 0.0423756549057051,
        0.0784335999999992, 0.878468636469772, 0.0784336,
        0.0792237451477643, 0.0791661274605434, 0.879142973793104
    );
    let min_ev = -12.47393;
    let max_ev = 4.026069;
    var v = m * val;
    v = clamp(log2(max(v, vec3f(1e-10))), vec3f(min_ev), vec3f(max_ev));
    v = (v - min_ev) / (max_ev - min_ev);
    return agx_contrast(v);
}

fn agx_look(v: vec3f) -> vec3f {
    // slightly "punchy": a bit more saturation and contrast for night scenes
    let luma = dot(v, vec3f(0.2126, 0.7152, 0.0722));
    let c = pow(max(v, vec3f(0.0)), vec3f(1.08));
    return luma + 1.15 * (c - luma);
}

fn agx_eotf(val: vec3f) -> vec3f {
    let inv = mat3x3f(
        1.19687900512017, -0.0528968517574562, -0.0529716355144438,
        -0.0980208811401368, 1.15190312990417, -0.0980434501171241,
        -0.0990297440797205, -0.0989611768448433, 1.15107367264116
    );
    let v = inv * val;
    return pow(max(v, vec3f(0.0)), vec3f(2.2));
}

fn hash12(p: vec2f) -> f32 {
    let q = vec2u(vec2i(floor(p)) + vec2i(65536, 65536));
    return f32(hash_uu(q.x, q.y + u32(pp.time * 60.0)) >> 8u) / 16777216.0;
}

fn srgb_encode(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

@fragment
fn fs_tonemap(in: FsIn) -> @location(0) vec4f {
    let uv = in.uv;
    let sty = pp.texel.z;
    var hdr = textureSampleLevel(src, samp, uv, 0.0).rgb;
    var bl = textureSampleLevel(bloom_tex, samp, uv, 0.0).rgb;
    // stylised: more, warmer glow (film halation)
    bl *= mix(vec3f(1.0), vec3f(1.25, 0.95, 0.85), sty);
    hdr = hdr + bl * pp.bloom * (1.0 + 1.3 * sty);
    hdr *= pp.exposure;
    let c = agx_eotf(agx_look(agx(hdr)));
    // store display-referred (sRGB-encoded) values for the stylise pass
    return vec4f(srgb_encode(clamp(c, vec3f(0.0), vec3f(1.0))), 1.0);
}

fn ldr(uv: vec2f) -> vec3f {
    return textureSampleLevel(src, samp, uv, 0.0).rgb;
}

// Generalised Kuwahara with 8 polynomial-weighted sectors: flattens detail
// into soft painted patches while keeping edges.
fn kuwahara(uv: vec2f, radius_px: f32) -> vec3f {
    let texel = pp.texel.xy;
    let step = radius_px / 4.0;
    var m: array<vec4f, 8>;
    var sq: array<vec3f, 8>;
    for (var k = 0; k < 8; k++) {
        m[k] = vec4f(0.0);
        sq[k] = vec3f(0.0);
    }
    let zeta = 2.0 / 4.0;
    let zc = 0.58;
    let sinzc = sin(zc);
    let eta = (zeta + cos(zc)) / (sinzc * sinzc);
    for (var y = -4; y <= 4; y++) {
        for (var x = -4; x <= 4; x++) {
            var v = vec2f(f32(x), f32(y)) / 4.0;
            if (dot(v, v) > 1.0) {
                continue;
            }
            let c = ldr(uv + vec2f(f32(x), f32(y)) * step * texel);
            var w: array<f32, 8>;
            var vxx = zeta - eta * v.x * v.x;
            var vyy = zeta - eta * v.y * v.y;
            var z = max(0.0, v.y + vxx);
            w[0] = z * z;
            z = max(0.0, -v.x + vyy);
            w[2] = z * z;
            z = max(0.0, -v.y + vxx);
            w[4] = z * z;
            z = max(0.0, v.x + vyy);
            w[6] = z * z;
            let r = 0.70710678 * vec2f(v.x - v.y, v.x + v.y);
            vxx = zeta - eta * r.x * r.x;
            vyy = zeta - eta * r.y * r.y;
            z = max(0.0, r.y + vxx);
            w[1] = z * z;
            z = max(0.0, -r.x + vyy);
            w[3] = z * z;
            z = max(0.0, -r.y + vxx);
            w[5] = z * z;
            z = max(0.0, r.x + vyy);
            w[7] = z * z;
            var sum = 0.0;
            for (var k = 0; k < 8; k++) {
                sum += w[k];
            }
            let g = exp(-3.125 * dot(v, v)) / max(sum, 1e-5);
            for (var k = 0; k < 8; k++) {
                let wk = w[k] * g;
                m[k] += vec4f(c * wk, wk);
                sq[k] += c * c * wk;
            }
        }
    }
    var out = vec3f(0.0);
    var wsum = 0.0;
    for (var k = 0; k < 8; k++) {
        if (m[k].w <= 1e-6) {
            continue;
        }
        let mean = m[k].rgb / m[k].w;
        let var3 = abs(sq[k] / m[k].w - mean * mean);
        let sigma2 = var3.r + var3.g + var3.b;
        // weights get tiny next to bright specks; normalise by their real sum
        // (clamping the sum would darken those pixels to black)
        let w = 1.0 / (1.0 + pow(min(sigma2 * 8000.0, 1.0e8), 4.0));
        out += mean * w;
        wsum += w;
    }
    return out / max(wsum, 1.0e-37);
}

@fragment
fn fs_stylise(in: FsIn) -> @location(0) vec4f {
    let uv = in.uv;
    let sty = pp.texel.z;
    var c = ldr(uv);
    if (sty > 0.01) {
        let radius = mix(1.0, 5.5, sty) * pp.texel.w;
        let k = kuwahara(uv, radius);
        c = mix(c, k, smoothstep(0.0, 0.35, sty));
        // soft ink edges where the picture changes sharply — measured at a
        // coarser scale so tiny bright specks (drops) don't get dark outlines,
        // and only on the darker side of an edge
        let t = pp.texel.xy * mix(2.0, 3.0, sty) * pp.texel.w;
        let l = vec3f(0.299, 0.587, 0.114);
        let px = dot(ldr(uv + vec2f(t.x, 0.0)), l);
        let nx = dot(ldr(uv - vec2f(t.x, 0.0)), l);
        let py = dot(ldr(uv + vec2f(0.0, t.y)), l);
        let ny = dot(ldr(uv - vec2f(0.0, t.y)), l);
        // a real edge shows at two scales; a tiny speck (a drop on the glass)
        // only at the small one, so it doesn't get a dark halo
        let t2 = t * 2.5;
        let gx2 = dot(ldr(uv + vec2f(t2.x, 0.0)) - ldr(uv - vec2f(t2.x, 0.0)), l);
        let gy2 = dot(ldr(uv + vec2f(0.0, t2.y)) - ldr(uv - vec2f(0.0, t2.y)), l);
        let g1 = length(vec2f(px - nx, py - ny));
        let g2 = length(vec2f(gx2, gy2));
        let edge = smoothstep(0.12, 0.4, min(g1, g2));
        let lum = dot(c, l);
        let darker = step(lum, (px + nx + py + ny) * 0.25);
        c *= 1.0 - edge * darker * 0.3 * sty * smoothstep(0.85, 0.3, lum);
        // lo-fi grade: lifted, cool-violet shadows, warm peachy highlights, richer colour
        let lum2 = dot(c, l);
        let shadow = (1.0 - lum2) * (1.0 - lum2);
        c += vec3f(0.035, 0.025, 0.075) * shadow * sty;
        c = mix(c, c * vec3f(1.06, 0.98, 0.9), smoothstep(0.4, 1.0, lum2) * sty);
        c = mix(vec3f(lum2), c, 1.0 + 0.22 * sty);
        // gentle tonal banding
        let bands = 9.0;
        let q = floor(lum2 * bands + 0.5) / bands;
        c *= mix(1.0, (q + 0.02) / (lum2 + 0.02), 0.22 * sty);
        // paper grain
        let paper = gnoise(in.clip.xy * 0.35) * 0.5 + gnoise(in.clip.xy * 1.3) * 0.25;
        c *= 1.0 + paper * 0.035 * sty;
    }
    // vignette and film grain
    let d = uv - vec2f(0.5);
    c *= max(1.0 - pp.vignette * dot(d, d) * 1.6, 0.0);
    let n = hash12(in.clip.xy) - 0.5;
    c += n * pp.grain * (1.0 - c * 0.7);
    // the exposure field carries the scene fade here (0 = clear, 1 = black)
    c = clamp(c, vec3f(0.0), vec3f(1.0)) * (1.0 - clamp(pp.exposure, 0.0, 1.0));
    if (pp.encode_srgb > 0.5) {
        return vec4f(c, 1.0);
    }
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3f(2.4));
    return vec4f(select(hi, lo, c <= vec3f(0.04045)), 1.0);
}
