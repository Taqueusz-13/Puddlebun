// ============================================================================
// glass.wgsl — the window panes. Both sashes are ray-traced per pixel; drops
// from the CPU simulation are evaluated analytically (spatial grid), plus
// procedural mist, wet trails and condensation on the inside. The street
// behind is refracted through them.
// ============================================================================

struct GlassParams {
    upper: vec4f,     // upper sash glass: z0, z1, y0, y1 (world)
    lower: vec4f,     // lower sash glass when closed: z0, z1, y0, y1
    planes: vec4f,    // x upper glass plane, y lower glass plane, z raise (m), w time
    cond: vec4f,      // x condensation, y mist, z unused, w unused
    grid: vec4f,      // x cell size (m), y cells across, z cells up, w trail texel (m)
    lamp: vec4f,      // room lamp position, w unused
    lamp_col: vec4f,  // room lamp colour
}

struct GlassDrop {
    pos: vec2f,       // sash-local metres (u along +z, v up)
    r: f32,           // radius (m)
    stretch: f32,     // > 1 when sliding (tail above)
}

@group(1) @binding(0) var outside_tex: texture_2d<f32>;
@group(1) @binding(1) var blur_tex: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;
@group(1) @binding(3) var<uniform> gp: GlassParams;
@group(1) @binding(4) var<storage, read> drops: array<GlassDrop>;
@group(1) @binding(5) var<storage, read> cell_start: array<u32>;
@group(1) @binding(6) var<storage, read> cell_items: array<u32>;
@group(1) @binding(7) var trail_tex: texture_2d<f32>;

struct VOut {
    @builtin(position) clip: vec4f,
    @location(0) wpos: vec3f,
}

@vertex
fn vs_glass(@builtin(vertex_index) vi: u32) -> VOut {
    var corners = array<vec2f, 6>(
        vec2f(0.0, 0.0), vec2f(1.0, 0.0), vec2f(1.0, 1.0),
        vec2f(0.0, 0.0), vec2f(1.0, 1.0), vec2f(0.0, 1.0)
    );
    let c = corners[vi];
    let z = mix(frame.win_a.y, frame.win_a.z, c.x);
    let y = mix(frame.win_a.w, frame.win_b.x, c.y);
    let p = vec3f(gp.planes.x - 0.004, y, z);
    var o: VOut;
    o.clip = frame.view_proj * vec4f(p, 1.0);
    o.wpos = p;
    return o;
}

struct Layer {
    slope: vec2f,   // dh/du, dh/dv
    height: f32,
    cov: f32,       // 1 inside a drop
    film: f32,      // wet trail amount
}

fn trail_at(sash: u32, uv: vec2f) -> f32 {
    let dims = vec2f(textureDimensions(trail_tex));
    let rows = dims.y * 0.5;
    let tc = vec2f(uv.x / gp.grid.w, f32(sash) * rows + uv.y / gp.grid.w);
    let t = textureSampleLevel(trail_tex, samp, (tc + vec2f(0.5)) / dims, 0.0).r;
    return t;
}

fn eval_layer(sash: u32, uv: vec2f) -> Layer {
    var o: Layer;
    o.slope = vec2f(0.0);
    o.height = 0.0;
    o.cov = 0.0;
    let cs = gp.grid.x;
    let gw = i32(gp.grid.y);
    let gh = i32(gp.grid.z);
    let c = vec2i(floor(uv / cs));
    o.film = trail_at(sash, uv);
    if (c.x >= 0 && c.y >= 0 && c.x < gw && c.y < gh) {
        let ci = u32(sash) * u32(gw * gh) + u32(c.y * gw + c.x);
        let a = cell_start[ci];
        let b = min(cell_start[ci + 1u], a + 24u);
        for (var k = a; k < b; k++) {
            let d = drops[cell_items[k]];
            let dp = uv - d.pos;
            let ry = select(d.r, d.r * d.stretch, dp.y > 0.0);
            let q = vec2f(dp.x / d.r, dp.y / ry);
            let e = dot(q, q);
            if (e >= 1.0) {
                continue;
            }
            let s = sqrt(1.0 - e);
            let hh = s * d.r * 0.6;
            if (hh > o.height) {
                o.height = hh;
                o.slope = -0.6 * vec2f(q.x, q.y * d.r / ry) / max(s, 0.12);
                o.cov = 1.0;
            }
        }
    }
    // thin water film in fresh trails
    o.slope += o.film * vec2f(gnoise(uv * 260.0), gnoise(uv * 260.0 + vec2f(9.0, 2.0))) * 0.1;
    return o;
}

// Fine spray the simulation doesn't track: far too small to see one by one,
// it reads as a soft haze that runners wipe clean.
fn mist_haze(sash: u32, uv: vec2f, film: f32) -> f32 {
    let n = fbm(uv * 14.0 + vec2f(f32(sash) * 7.0, 2.0));
    let speck = gnoise(uv * 900.0) * 0.5 + 0.5;
    return gp.cond.y * (1.0 - smoothstep(0.08, 0.5, film)) * (0.6 + 0.6 * n) * (0.75 + 0.25 * speck);
}

fn fog_mask(uv: vec2f, size: vec2f, lower_sash: bool) -> f32 {
    let de = min(min(uv.x, size.x - uv.x), min(uv.y, size.y - uv.y));
    let edge = 1.0 - smoothstep(0.015, 0.2, de);
    let bottom = select(0.0, 1.0 - smoothstep(0.0, 0.4, uv.y), lower_sash);
    let n = fbm(uv * 9.0 + vec2f(3.0, 1.0)) - 0.5;
    let fine = gnoise(uv * 120.0) * 0.08;
    return clamp((edge * 1.1 + bottom * 0.7 - 0.25 + n * 0.9 + fine) * 1.4, 0.0, 1.0);
}

@fragment
fn fs_glass(in: VOut) -> @location(0) vec4f {
    let ro = frame.cam_pos.xyz;
    let rd = normalize(in.wpos - ro);
    let raise = gp.planes.z;
    let screen = in.clip.xy * frame.screen.zw;

    // ray vs the two sash planes
    let tu = (gp.planes.x - ro.x) / rd.x;
    let qu = ro + rd * tu;
    let uv_u = vec2f(qu.z - gp.upper.x, qu.y - gp.upper.z);
    let size_u = vec2f(gp.upper.y - gp.upper.x, gp.upper.w - gp.upper.z);
    let in_u = all(uv_u >= vec2f(0.0)) && all(uv_u <= size_u);

    let tl = (gp.planes.y - ro.x) / rd.x;
    let ql = ro + rd * tl;
    let uv_l = vec2f(ql.z - gp.lower.x, ql.y - (gp.lower.z + raise));
    let size_l = vec2f(gp.lower.y - gp.lower.x, gp.lower.w - gp.lower.z);
    let in_l = all(uv_l >= vec2f(0.0)) && all(uv_l <= size_l);

    if (!in_u && !in_l) {
        // the gap under a raised sash: open air
        return vec4f(textureSampleLevel(outside_tex, samp, screen, 0.0).rgb, 1.0);
    }

    var slope = vec2f(0.0);
    var cov = 0.0;
    var film = 0.0;
    var haze = 0.0;
    if (in_u) {
        let l = eval_layer(0u, uv_u);
        slope += l.slope;
        cov = max(cov, l.cov);
        film = max(film, l.film);
        haze = max(haze, mist_haze(0u, uv_u, l.film));
    }
    if (in_l) {
        let l = eval_layer(1u, uv_l);
        slope += l.slope;
        cov = max(cov, l.cov);
        film = max(film, l.film);
        haze = max(haze, mist_haze(1u, uv_l, l.film));
    }
    slope = clamp(slope, vec2f(-2.0), vec2f(2.0));

    // glass u runs along +z, v along +y: map to screen space
    let su = vec2f(frame.cam_right.z, -frame.cam_up.z);
    let sv = vec2f(frame.cam_right.y, -frame.cam_up.y);
    let off = (slope.x * su + slope.y * sv) * 0.06;
    let uv_r = clamp(screen - off, vec2f(0.001), vec2f(0.999));

    var col = textureSampleLevel(outside_tex, samp, uv_r, 0.0).rgb;
    let blurred = textureSampleLevel(blur_tex, samp, uv_r, 0.0).rgb;
    // drops: a thin dark rim (total internal reflection at grazing angles) and
    // a bright core focusing the lights behind
    let rim = smoothstep(0.75, 1.0, clamp(length(slope) * 0.5, 0.0, 1.0));
    // a drop is a tiny wide-angle lens: it gathers light from a broad patch of
    // the street behind, so it glows with the neon around it
    let gathered = textureSampleLevel(blur_tex, samp, clamp(screen - off * 1.6, vec2f(0.001), vec2f(0.999)), 0.0).rgb;
    // (never much darker than the glass around it, or drops read as dirt)
    let lens = max(mix(col, gathered * 1.45, 0.55), col * 0.92) * (1.0 - 0.12 * rim);
    col = mix(col, lens, cov);
    // the spray haze scatters the street's light into a soft glow
    let mist_k = clamp(haze * 1.4, 0.0, 0.72);
    col = mix(col, blurred * 1.15, mist_k);

    // condensation on the inside of the innermost pane
    var fog = 0.0;
    if (in_l) {
        fog = fog_mask(uv_l, size_l, true);
    } else {
        fog = fog_mask(uv_u, size_u, false);
    }
    fog *= gp.cond.x;
    let lamp_dir = normalize(gp.lamp.xyz - in.wpos);
    let room_scatter = gp.lamp_col.rgb * 0.018 * (0.4 + 0.6 * max(lamp_dir.x, 0.0));
    let fogged = blurred * 0.75 + room_scatter + frame.sky.rgb * frame.sky.w * 0.25;
    col = mix(col, fogged, clamp(fog * (1.0 - cov * 0.3), 0.0, 0.92));

    // the window faintly mirrors the warm room, and catches the lamp
    let nv = abs(rd.x);
    let fres = 0.04 + 0.96 * pow(1.0 - nv, 5.0);
    col += vec3f(1.0, 0.72, 0.45) * 0.012 * fres;
    let n = normalize(vec3f(1.0, -slope.y * 0.6, -slope.x * 0.6));
    let h = normalize(lamp_dir - rd);
    let spec = pow(max(dot(n, h), 0.0), 180.0) * (0.4 + 2.5 * cov);
    col += gp.lamp_col.rgb * spec * 0.25;
    return vec4f(col, 1.0);
}
