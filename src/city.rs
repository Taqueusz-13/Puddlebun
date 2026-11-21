//! The neon city: a New York street (tenements, fire escapes, water towers,
//! shopfronts with awnings and neon) with Night City scale behind it
//! (towers, a hologram), seen from a 4th-floor flat. Also builds that flat's
//! window and the room edge (sashes, sill, radiator, Puddlebun).
//!
//! Layout rules shared with the shaders live in world.wgsl (C_* constants).

use crate::scene::*;
use glam::{Quat, Vec2, Vec3};

// city materials (must match city.wgsl)
pub const M_CROAD: u32 = 20;
pub const M_CWALK: u32 = 21;
pub const M_CCURB: u32 = 22;
pub const M_CFACADE: u32 = 23;
pub const M_AWNING: u32 = 25;
pub const M_NEON: u32 = 26;
pub const M_SIGNBOX: u32 = 27;
pub const M_TOWER: u32 = 28;
pub const M_HOLO: u32 = 29;
pub const M_GRATE: u32 = 30;
pub const M_RAIL: u32 = 31;
pub const M_WOOD: u32 = 32;
pub const M_CORNICE: u32 = 33;
pub const M_STONE: u32 = 34;
pub const M_AVIATION: u32 = 35;
pub const M_ROOFTOP: u32 = 36;
pub const M_STAIR: u32 = 37;
pub const M_ACUNIT: u32 = 38;
// room materials (must match room section of city.wgsl)
pub const R_WALL: u32 = 40;
pub const R_TRIM: u32 = 41;
pub const R_SILL: u32 = 42;
pub const R_RADIATOR: u32 = 43;
pub const R_PLUSH: u32 = 44;
pub const R_PLUSH_PINK: u32 = 45;
pub const R_EYE: u32 = 46;
pub const R_SASH: u32 = 47;
pub const R_FLOOR: u32 = 48;
pub const R_BRASS: u32 = 49;

pub const C_ROAD_HALF: f32 = 6.0;
pub const C_CURB_OUT: f32 = 6.15;
pub const C_CURB_H: f32 = 0.15;
pub const C_FACADE: f32 = 10.0;
pub const C_MODULE: f32 = 7.6;
pub const C_Z0: f32 = -152.0;
pub const C_MODULES: i32 = 40;
const DEPTH: f32 = 14.0;

/// Same rule as `city_floors` in world.wgsl.
pub fn city_floors(m: i32) -> u32 {
    5 + hash_u((m as u32).wrapping_mul(2_654_435_761) ^ 0x5bd1_e995) % 4
}
pub fn city_height(m: i32) -> f32 {
    4.5 + city_floors(m) as f32 * 3.2
}
pub fn city_has_awning(m: i32) -> bool {
    u01(hash_uu(m as u32, 77)) > 0.35
}

// neon palette (index stored in the vertex, colours repeated in city.wgsl)
pub const NEON: [[f32; 3]; 7] = [
    [1.0, 0.08, 0.45], // hot pink
    [0.1, 0.85, 1.0],  // cyan
    [1.0, 0.12, 0.05], // red
    [1.0, 0.55, 0.08], // amber
    [0.25, 1.0, 0.35], // green
    [0.6, 0.2, 1.0],   // violet
    [0.75, 0.85, 1.0], // ice white
];

/// Words the signs can show (index stored in the vertex; table in city.wgsl).
pub const WORDS: [&str; 20] = [
    "HOTEL", "DINER", "OPEN", "BAR", "PIZZA", "NOODLES", "CAFE", "LIQUOR", "24H", "RAMEN",
    "MOTEL", "KARAOKE", "ARCADE", "LAUNDRY", "DELI", "JAZZ", "TATTOO", "PUDDLEBUN", "BOOKS", "RECORDS",
];

/// The window we look out of, and where our head sits.
#[derive(Clone, Copy, Debug)]
pub struct WindowGeom {
    pub wall_x: f32,
    pub z_min: f32,
    pub z_max: f32,
    pub y_min: f32,
    pub y_max: f32,
    pub glass_x: f32,
    pub inner_x: f32,
    pub sill_top: f32,
    /// upper sash glass: z0, z1, y0, y1
    pub upper: [f32; 4],
    /// lower sash glass when closed: z0, z1, y0, y1
    pub lower: [f32; 4],
    pub max_raise: f32,
    pub eye: Vec3,
    pub look: Vec3,
}

pub struct Room {
    pub mesh: MeshBuilder,
    pub lamps: Vec<StaticLight>,
    pub geom: WindowGeom,
}

struct Sign {
    word: usize,
    color: usize,
    seed: u32,
}

/// Adds a flat neon sign (text across `u`) on a plane.
#[allow(clippy::too_many_arguments)]
fn sign_quad(m: &mut MeshBuilder, lights: &mut Vec<StaticLight>, origin: Vec3, u_dir: Vec3, v_dir: Vec3, w: f32, h: f32, normal: Vec3, s: &Sign, vertical: bool, strength: f32) {
    let p = [origin, origin + u_dir * w, origin + u_dir * w + v_dir * h, origin + v_dir * h];
    let uv = [Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)];
    // an exact integer in the float: word + 32·colour + 256·vertical + 512·seed
    let packed = s.word as u32 + 32 * s.color as u32 + 256 * vertical as u32 + 512 * (s.seed % 4096);
    m.quad(p, normal, uv, M_NEON, packed as f32, Vec2::new(w, h));
    let c = Vec3::from(NEON[s.color]);
    let center = origin + u_dir * (w * 0.5) + v_dir * (h * 0.5);
    lights.push(StaticLight {
        pos: center + normal * 0.7,
        radius: 26.0,
        kind: 1,
        dir: normal,
        cos_cut: 0.0,
        color: c * strength * (w * h).sqrt(),
        tint: Tint::Fixed,
        flicker: s.seed,
    });
}

fn cuboid(m: &mut MeshBuilder, a: Vec3, b: Vec3, mat: u32, seed: f32, faces: u8) {
    m.cuboid(a, b, mat, seed, faces, Vec3::ZERO, Vec2::ZERO);
}

/// Fire escape platform + railing + stairs down (x from wall at `wx` outwards by `depth`).
fn fire_escape_level(m: &mut MeshBuilder, wx: f32, out: f32, z0: f32, z1: f32, y: f32, stairs_left: bool, seed: f32) {
    let xo = wx + out; // outer edge (out is negative on the opposite side)
    let (xa, xb) = if xo < wx { (xo, wx) } else { (wx, xo) };
    // slatted platform (top and bottom faces)
    let q = [Vec3::new(xa, y, z0), Vec3::new(xb, y, z0), Vec3::new(xb, y, z1), Vec3::new(xa, y, z1)];
    let uvp = q.map(|p| Vec2::new(p.z, p.x));
    m.quad(q, Vec3::Y, uvp, M_GRATE, seed, Vec2::ZERO);
    // railing: front, and both ends
    let rh = 0.95;
    let front = [Vec3::new(xo, y, z0), Vec3::new(xo, y, z1), Vec3::new(xo, y + rh, z1), Vec3::new(xo, y + rh, z0)];
    m.quad(front, Vec3::new((xo - wx).signum(), 0.0, 0.0), front.map(|p| Vec2::new(p.z - z0, p.y - y)), M_RAIL, seed, Vec2::new(rh, 0.0));
    for z in [z0, z1] {
        let side = [Vec3::new(xa, y, z), Vec3::new(xb, y, z), Vec3::new(xb, y + rh, z), Vec3::new(xa, y + rh, z)];
        m.quad(side, Vec3::Z, side.map(|p| Vec2::new(p.x - xa, p.y - y)), M_RAIL, seed, Vec2::new(rh, 0.0));
    }
    // stairs down to the level below
    let sw = 0.62;
    let (sx0, sx1) = if xo < wx { (wx - 0.15 - sw, wx - 0.15) } else { (wx + 0.15, wx + 0.15 + sw) };
    let (za, zb) = if stairs_left { (z1 - 0.3, z0 + 0.6) } else { (z0 + 0.3, z1 - 0.6) };
    let drop = 3.2;
    let st = [Vec3::new(sx0, y, za), Vec3::new(sx1, y, za), Vec3::new(sx1, y - drop, zb), Vec3::new(sx0, y - drop, zb)];
    let len = ((zb - za).powi(2) + drop * drop).sqrt();
    let n = Vec3::new(0.0, (zb - za).abs(), drop * (zb - za).signum()).normalize();
    m.quad(st, n, [Vec2::new(0.0, 0.0), Vec2::new(sw, 0.0), Vec2::new(sw, len), Vec2::new(0.0, len)], M_STAIR, seed, Vec2::ZERO);
}

pub fn build_city() -> World {
    let mut m = MeshBuilder::default();
    let mut lights: Vec<StaticLight> = Vec::new();
    let mut fills: Vec<StaticLight> = Vec::new();
    let z_lo = C_Z0;
    let z_hi = C_Z0 + C_MODULES as f32 * C_MODULE;
    let zero = Vec2::ZERO;

    // --- street surface ---
    m.quad(
        [Vec3::new(-C_ROAD_HALF, 0.0, z_lo), Vec3::new(C_ROAD_HALF, 0.0, z_lo), Vec3::new(C_ROAD_HALF, 0.0, z_hi), Vec3::new(-C_ROAD_HALF, 0.0, z_hi)],
        Vec3::Y,
        [Vec2::new(-C_ROAD_HALF, z_lo), Vec2::new(C_ROAD_HALF, z_lo), Vec2::new(C_ROAD_HALF, z_hi), Vec2::new(-C_ROAD_HALF, z_hi)],
        M_CROAD,
        0.0,
        zero,
    );
    for side in [1.0f32, -1.0] {
        let span = |a: f32, b: f32| if side > 0.0 { (a, b) } else { (-b, -a) };
        let (k0, k1) = span(C_ROAD_HALF, C_CURB_OUT);
        let kface = if side > 0.0 { 2 } else { 1 };
        cuboid(&mut m, Vec3::new(k0, 0.0, z_lo), Vec3::new(k1, C_CURB_H, z_hi), M_CCURB, 0.0, 4 | kface);
        let (w0, w1) = span(C_CURB_OUT, C_FACADE);
        cuboid(&mut m, Vec3::new(w0, 0.0, z_lo), Vec3::new(w1, C_CURB_H, z_hi), M_CWALK, 0.0, 4);
    }

    // --- the opposite row of tenements ---
    let mut sign_seed = 1u32;
    for i in 0..C_MODULES {
        let z0 = C_Z0 + i as f32 * C_MODULE;
        let z1 = z0 + C_MODULE;
        let h = city_height(i);
        let seed = (3000 + i) as f32;
        let x0 = -C_FACADE - DEPTH;
        let x1 = -C_FACADE;
        // facade (+X face) with its window grid and shopfront
        m.cuboid(Vec3::new(x0, 0.0, z0), Vec3::new(x1, h, z1), M_CFACADE, seed, 1, Vec3::new(0.0, 0.0, z0), zero);
        cuboid(&mut m, Vec3::new(x0, 0.0, z0), Vec3::new(x1, h, z1), M_ROOFTOP, seed, 4);
        // exposed side walls above the neighbours
        for (zf, nb, n) in [(z0, i - 1, -Vec3::Z), (z1, i + 1, Vec3::Z)] {
            let hn = if (0..C_MODULES).contains(&nb) { city_height(nb) } else { 0.0 };
            if h > hn + 0.01 {
                let q = [Vec3::new(x0, hn, zf), Vec3::new(x1, hn, zf), Vec3::new(x1, h, zf), Vec3::new(x0, h, zf)];
                m.quad(q, n, q.map(|p| Vec2::new(p.x, p.y)), M_WALL, seed + 500.0, zero);
            }
        }
        // cornice
        cuboid(&mut m, Vec3::new(x1, h - 0.8, z0), Vec3::new(x1 + 0.45, h - 0.2, z1), M_CORNICE, seed, 1 | 4 | 8 | 16 | 32);
        // awning over the shop
        if city_has_awning(i) {
            let (ax0, ay0, ax1, ay1) = (x1, 4.05, x1 + 1.6, 3.45);
            let q = [Vec3::new(ax0, ay0, z0 + 0.4), Vec3::new(ax0, ay0, z1 - 0.4), Vec3::new(ax1, ay1, z1 - 0.4), Vec3::new(ax1, ay1, z0 + 0.4)];
            let n = Vec3::new(ay0 - ay1, ax1 - ax0, 0.0).normalize();
            let sl = ((ax1 - ax0).powi(2) + (ay0 - ay1).powi(2)).sqrt();
            m.quad(q, n, [Vec2::new(0.0, sl), Vec2::new(C_MODULE - 0.8, sl), Vec2::new(C_MODULE - 0.8, 0.0), Vec2::new(0.0, 0.0)], M_AWNING, i as f32, zero);
            let v = [Vec3::new(ax1, ay1 - 0.28, z0 + 0.4), Vec3::new(ax1, ay1 - 0.28, z1 - 0.4), Vec3::new(ax1, ay1, z1 - 0.4), Vec3::new(ax1, ay1, z0 + 0.4)];
            m.quad(v, Vec3::X, [Vec2::new(0.0, -0.28), Vec2::new(C_MODULE - 0.8, -0.28), Vec2::new(C_MODULE - 0.8, 0.0), Vec2::new(0.0, 0.0)], M_AWNING, i as f32, Vec2::new(1.0, 0.0));
        }
        // warm light spilling out of the shop window
        if u01(hash_uu(i as u32, 5)) > 0.25 {
            let warm = if u01(hash_uu(i as u32, 6)) > 0.5 { Vec3::new(1.0, 0.75, 0.5) } else { Vec3::new(0.8, 0.9, 1.0) };
            fills.push(StaticLight { pos: Vec3::new(x1 + 0.8, 2.0, z0 + 3.0), radius: 12.0, kind: 1, dir: Vec3::X, cos_cut: 0.0, color: warm * 7.0, tint: Tint::Fixed, flicker: 0 });
        }
        // neon: a sign above the shop and sometimes a vertical blade sign
        let hs = hash_uu(i as u32, 11);
        if u01(hs) > 0.3 {
            let s = Sign { word: (hash_u(hs) % WORDS.len() as u32) as usize, color: (hash_u(hs + 1) % 7) as usize, seed: sign_seed };
            sign_seed += 1;
            let n = WORDS[s.word].len() as f32;
            let hgt = 0.72;
            let w = (hgt / 9.0 * 6.0 * (n + 1.0)).min(C_MODULE - 0.6);
            let zc = z0 + C_MODULE * 0.5 + w * 0.5;
            // text runs towards -z so it reads correctly from across the street
            sign_quad(&mut m, &mut lights, Vec3::new(x1 + 0.03, 4.55, zc), -Vec3::Z, Vec3::Y, w, hgt, Vec3::X, &s, false, 22.0);
        }
        if u01(hash_uu(i as u32, 12)) > 0.62 {
            let s = Sign { word: (hash_uu(i as u32, 13) % WORDS.len() as u32) as usize, color: (hash_uu(i as u32, 14) % 7) as usize, seed: sign_seed };
            sign_seed += 1;
            let n = WORDS[s.word].len() as f32;
            let w = 0.95;
            let hgt = (w / 7.0 * 8.0 * (n + 0.5)).min(h - 7.0);
            let zb = z0 + 0.55;
            let yb = 6.2;
            cuboid(&mut m, Vec3::new(x1, yb - 0.05, zb - 0.05), Vec3::new(x1 + w + 0.12, yb + hgt + 0.05, zb + 0.05), M_SIGNBOX, i as f32, 63);
            // both faces, each reading correctly
            sign_quad(&mut m, &mut lights, Vec3::new(x1 + 0.12, yb, zb + 0.055), Vec3::X, Vec3::Y, w, hgt, Vec3::Z, &s, true, 16.0);
            sign_quad(&mut m, &mut lights, Vec3::new(x1 + 0.12 + w, yb, zb - 0.055), -Vec3::X, Vec3::Y, w, hgt, -Vec3::Z, &s, true, 0.0);
            lights.pop(); // one light per blade sign is plenty
            if let Some(l) = lights.last_mut() {
                l.kind = 2;
            }
        }
        // fire escape zig-zagging up the front
        if u01(hash_uu(i as u32, 91)) > 0.5 {
            let floors = city_floors(i);
            for f in 1..floors {
                let y = 4.5 + (f - 1) as f32 * 3.2 + 0.05;
                fire_escape_level(&mut m, x1, 1.15, z0 + 0.7, z0 + 5.6, y, f % 2 == 0, seed);
            }
        }
        // air-conditioners hanging out of windows
        for f in 0..city_floors(i) {
            for (k, uc) in [1.3f32, 3.8, 6.3].iter().enumerate() {
                if u01(hash_uu(i as u32 * 64 + f * 4 + k as u32, 33)) > 0.86 {
                    let ys = 4.5 + f as f32 * 3.2 + 0.95;
                    let zc = z0 + uc;
                    cuboid(&mut m, Vec3::new(x1, ys - 0.02, zc - 0.32), Vec3::new(x1 + 0.48, ys + 0.38, zc + 0.32), M_ACUNIT, seed, 1 | 4 | 8 | 16 | 32);
                }
            }
        }
        // water tower on some roofs
        if h > 19.0 && u01(hash_uu(i as u32, 41)) > 0.62 {
            let c = Vec3::new(x0 + 6.0, h, z0 + C_MODULE * 0.5);
            for (dx, dz) in [(-1.1f32, -1.1f32), (1.1, -1.1), (1.1, 1.1), (-1.1, 1.1)] {
                cuboid(&mut m, c + Vec3::new(dx - 0.07, 0.0, dz - 0.07), c + Vec3::new(dx + 0.07, 2.5, dz + 0.07), M_METAL, 0.0, 1 | 2 | 16 | 32);
            }
            cuboid(&mut m, c + Vec3::new(-1.4, 2.4, -1.4), c + Vec3::new(1.4, 2.55, 1.4), M_METAL, 0.0, 63);
            m.cylinder(c + Vec3::new(0.0, 2.55, 0.0), 1.55, 3.3, 20, M_WOOD, i as f32);
            m.frustum(c + Vec3::new(0.0, 5.85, 0.0), 1.7, 0.12, 1.1, 20, M_ROOFTOP, i as f32);
        }
    }

    // --- our own building (x > 10), with a hole for our window ---
    let ours = 9999.0;
    let wz = 0.55;
    let (wy0, wy1) = (8.05, 9.75);
    let top = 31.3;
    let uvo = Vec3::new(0.0, 0.0, -3.8);
    let face = 2u8; // -X face
    cuboid_uv(&mut m, Vec3::new(C_FACADE, 0.0, z_lo), Vec3::new(C_FACADE + DEPTH, top, -wz), M_CFACADE, ours, face, uvo);
    cuboid_uv(&mut m, Vec3::new(C_FACADE, 0.0, wz), Vec3::new(C_FACADE + DEPTH, top, z_hi), M_CFACADE, ours, face, uvo);
    cuboid_uv(&mut m, Vec3::new(C_FACADE, 0.0, -wz), Vec3::new(C_FACADE + DEPTH, wy0, wz), M_CFACADE, ours, face, uvo);
    cuboid_uv(&mut m, Vec3::new(C_FACADE, wy1, -wz), Vec3::new(C_FACADE + DEPTH, top, wz), M_CFACADE, ours, face, uvo);
    // outside part of the window reveal and the stone sill
    let rq = |m: &mut MeshBuilder, q: [Vec3; 4], n: Vec3| m.quad(q, n, q.map(|p| Vec2::new(p.z + p.x, p.y)), M_STONE, 1.0, Vec2::ZERO);
    rq(&mut m, [Vec3::new(10.0, wy0, -wz), Vec3::new(10.14, wy0, -wz), Vec3::new(10.14, wy1, -wz), Vec3::new(10.0, wy1, -wz)], Vec3::Z);
    rq(&mut m, [Vec3::new(10.0, wy0, wz), Vec3::new(10.14, wy0, wz), Vec3::new(10.14, wy1, wz), Vec3::new(10.0, wy1, wz)], -Vec3::Z);
    rq(&mut m, [Vec3::new(10.0, wy1, -wz), Vec3::new(10.14, wy1, -wz), Vec3::new(10.14, wy1, wz), Vec3::new(10.0, wy1, wz)], -Vec3::Y);
    cuboid(&mut m, Vec3::new(9.82, 7.88, -0.68), Vec3::new(10.14, 7.98, 0.68), M_STONE, 2.0, 1 | 2 | 4 | 8 | 16 | 32);
    cuboid(&mut m, Vec3::new(10.0, 7.98, -wz), Vec3::new(10.14, wy0, wz), M_STONE, 2.0, 4 | 2);
    // our fire escape, right outside the window
    fire_escape_level(&mut m, C_FACADE, -1.15, -3.4, 0.75, 7.2, true, ours);
    // ground floor shop signs on our side (seen in the wet road)
    for k in 0..12 {
        let z0 = -60.0 + k as f32 * 10.0 + 1.3;
        let hs = hash_uu(k, 1234);
        if u01(hs) > 0.45 {
            let s = Sign { word: (hash_u(hs) % WORDS.len() as u32) as usize, color: (hash_u(hs + 3) % 7) as usize, seed: sign_seed };
            sign_seed += 1;
            let w = 3.4;
            sign_quad(&mut m, &mut lights, Vec3::new(C_FACADE - 0.03, 4.55, z0), Vec3::Z, Vec3::Y, w, 0.72, -Vec3::X, &s, false, 18.0);
        }
    }

    // --- the hero signs right across from the window ---
    {
        let x1 = -C_FACADE + 0.04;
        let s = Sign { word: 0, color: 0, seed: 1000 }; // HOTEL, hot pink, vertical
        let w = 1.3;
        let hgt = w / 7.0 * 8.0 * 5.5;
        cuboid(&mut m, Vec3::new(x1 - 0.04, 9.9, 2.1 - 0.08), Vec3::new(x1 + 0.08, 10.0 + hgt + 0.1, 2.1 + w + 0.08), M_SIGNBOX, 7.0, 63);
        sign_quad(&mut m, &mut lights, Vec3::new(x1 + 0.09, 10.0, 2.1 + w), -Vec3::Z, Vec3::Y, w, hgt, Vec3::X, &s, true, 26.0);
        let s2 = Sign { word: 5, color: 1, seed: 1002 }; // NOODLES, cyan
        let hh = 0.9;
        let ww = hh / 9.0 * 6.0 * 8.0;
        sign_quad(&mut m, &mut lights, Vec3::new(x1 + 0.03, 13.6, -9.5 + ww), -Vec3::Z, Vec3::Y, ww, hh, Vec3::X, &s2, false, 22.0);
    }

    // --- cobra-head street lamps ---
    for k in 0..10 {
        for side in [1.0f32, -1.0] {
            let z = -144.0 + 32.0 * k as f32 + if side > 0.0 { 0.0 } else { 16.0 };
            let px = 6.45 * side;
            m.cylinder(Vec3::new(px, C_CURB_H, z), 0.09, 8.8, 10, M_METAL, 0.0);
            let ax = px - 2.2 * side;
            let (a0, a1) = if side > 0.0 { (ax, px) } else { (px, ax) };
            cuboid(&mut m, Vec3::new(a0, 8.75, z - 0.05), Vec3::new(a1, 8.88, z + 0.05), M_METAL, 0.0, 63);
            cuboid(&mut m, Vec3::new(ax - 0.36, 8.66, z - 0.16), Vec3::new(ax + 0.36, 8.8, z + 0.16), M_LAMP, 0.0, 63);
            lights.push(StaticLight::lamp(Vec3::new(ax, 8.58, z), Tint::CityLamp));
        }
    }

    // --- distant towers (Night City scale) ---
    let mut rng = fastrand::Rng::with_seed(0xc17e);
    for t in 0..70u32 {
        let far_end = t >= 56;
        let (cx, cz) = if far_end {
            (rng.f32() * 160.0 - 80.0, (if rng.bool() { 1.0 } else { -1.0 }) * (190.0 + rng.f32() * 260.0))
        } else {
            (-48.0 - rng.f32().powf(0.7) * 380.0, rng.f32() * 760.0 - 380.0)
        };
        let w = 18.0 + rng.f32() * 38.0;
        let d = 18.0 + rng.f32() * 32.0;
        let dist = (cx * cx + cz * cz).sqrt();
        let h = 45.0 + rng.f32() * 120.0 + (dist / 400.0) * 140.0;
        let seed = (5000 + t) as f32;
        let a = Vec3::new(cx - w * 0.5, 0.0, cz - d * 0.5);
        let b = Vec3::new(cx + w * 0.5, h, cz + d * 0.5);
        m.cuboid(a, b, M_TOWER, seed, 1 | 2 | 16 | 32, a, zero);
        cuboid(&mut m, a, b, M_ROOFTOP, seed, 4);
        if h > 120.0 {
            for (dx, dz) in [(a.x + 0.5, a.z + 0.5), (b.x - 0.5, b.z - 0.5)] {
                cuboid(&mut m, Vec3::new(dx - 0.4, h, dz - 0.4), Vec3::new(dx + 0.4, h + 0.8, dz + 0.4), M_AVIATION, seed, 63);
            }
        }
    }
    // the big hologram billboard on a tower facing us
    {
        let (cx, cz, w, d, h) = (-110.0f32, -30.0f32, 46.0f32, 34.0f32, 190.0f32);
        let a = Vec3::new(cx - d * 0.5, 0.0, cz - w * 0.5);
        let b = Vec3::new(cx + d * 0.5, h, cz + w * 0.5);
        m.cuboid(a, b, M_TOWER, 6100.0, 1 | 2 | 16 | 32, a, zero);
        cuboid(&mut m, a, b, M_ROOFTOP, 6100.0, 4);
        let fx = b.x + 0.5;
        let (y0, y1, z0, z1) = (78.0, 140.0, cz - 19.0, cz + 19.0);
        let q = [Vec3::new(fx, y0, z1), Vec3::new(fx, y0, z0), Vec3::new(fx, y1, z0), Vec3::new(fx, y1, z1)];
        m.quad(q, Vec3::X, [Vec2::new(0.0, 0.0), Vec2::new(z1 - z0, 0.0), Vec2::new(z1 - z0, y1 - y0), Vec2::new(0.0, y1 - y0)], M_HOLO, 1.0, Vec2::new(z1 - z0, y1 - y0));
        lights.push(StaticLight { pos: Vec3::new(fx + 8.0, 110.0, cz), radius: 140.0, kind: 1, dir: Vec3::X, cos_cut: 0.0, color: Vec3::new(0.7, 0.3, 1.0) * 900.0, tint: Tint::Fixed, flicker: 0 });
    }

    World {
        kind: SceneKind::City,
        mesh: m,
        lights,
        fill_lights: fills,
        windows: Vec::new(),
        sky: Vec3::new(0.62, 0.32, 0.85) * 0.05,
        room: Some(build_room()),
    }
}

fn cuboid_uv(m: &mut MeshBuilder, a: Vec3, b: Vec3, mat: u32, seed: f32, faces: u8, uvo: Vec3) {
    m.cuboid(a, b, mat, seed, faces, uvo, Vec2::ZERO);
}

/// Our room: wall around the window, jambs, double-hung sashes, sill,
/// radiator and Puddlebun.
pub fn build_room() -> Room {
    let mut m = MeshBuilder::default();
    let wall = 10.35;
    let (oz0, oz1, oy0, oy1) = (-0.55f32, 0.55f32, 8.05f32, 9.75f32);
    let floor = 7.1;
    let ceil = 10.3;
    let o = Vec2::ZERO;
    let wq = |m: &mut MeshBuilder, z0: f32, z1: f32, y0: f32, y1: f32| {
        let q = [Vec3::new(wall, y0, z1), Vec3::new(wall, y0, z0), Vec3::new(wall, y1, z0), Vec3::new(wall, y1, z1)];
        m.quad(q, Vec3::X, q.map(|p| Vec2::new(p.z, p.y)), R_WALL, 0.0, Vec2::ZERO);
    };
    wq(&mut m, -3.2, oz0, floor, ceil);
    wq(&mut m, oz1, 3.2, floor, ceil);
    wq(&mut m, oz0, oz1, oy1, ceil);
    wq(&mut m, oz0, oz1, floor, oy0);
    // floor + ceiling
    let fq = [Vec3::new(wall, floor, -3.2), Vec3::new(13.5, floor, -3.2), Vec3::new(13.5, floor, 3.2), Vec3::new(wall, floor, 3.2)];
    m.quad(fq, Vec3::Y, fq.map(|p| Vec2::new(p.x, p.z)), R_FLOOR, 0.0, o);
    let cq = [Vec3::new(wall, ceil, -3.2), Vec3::new(13.5, ceil, -3.2), Vec3::new(13.5, ceil, 3.2), Vec3::new(wall, ceil, 3.2)];
    m.quad(cq, -Vec3::Y, cq.map(|p| Vec2::new(p.x, p.z)), R_WALL, 1.0, o);

    // jambs (inside part of the reveal) and head
    let jx0 = 10.12;
    let jq = |m: &mut MeshBuilder, q: [Vec3; 4], n: Vec3| m.quad(q, n, q.map(|p| Vec2::new(p.x + p.z, p.y)), R_TRIM, 0.0, Vec2::ZERO);
    jq(&mut m, [Vec3::new(jx0, oy0, oz0), Vec3::new(wall, oy0, oz0), Vec3::new(wall, oy1, oz0), Vec3::new(jx0, oy1, oz0)], Vec3::Z);
    jq(&mut m, [Vec3::new(jx0, oy0, oz1), Vec3::new(wall, oy0, oz1), Vec3::new(wall, oy1, oz1), Vec3::new(jx0, oy1, oz1)], -Vec3::Z);
    jq(&mut m, [Vec3::new(jx0, oy1, oz0), Vec3::new(wall, oy1, oz0), Vec3::new(wall, oy1, oz1), Vec3::new(jx0, oy1, oz1)], -Vec3::Y);
    // stops between the sash tracks
    for z in [oz0, oz1 - 0.012] {
        cuboid(&mut m, Vec3::new(10.163, oy0, z), Vec3::new(10.172, oy1, z + 0.012), R_TRIM, 0.0, 63);
        cuboid(&mut m, Vec3::new(10.215, oy0, z), Vec3::new(10.232, oy1, z + 0.012), R_TRIM, 0.0, 63);
    }
    // stool (inner sill), apron and casing
    cuboid(&mut m, Vec3::new(10.12, 8.02, -0.7), Vec3::new(10.52, 8.07, 0.7), R_SILL, 0.0, 1 | 4 | 8 | 16 | 32);
    cuboid(&mut m, Vec3::new(wall, 7.9, -0.62), Vec3::new(wall + 0.025, 8.02, 0.62), R_TRIM, 0.0, 1 | 8 | 16 | 32);
    cuboid(&mut m, Vec3::new(wall, oy0 + 0.02, -0.67), Vec3::new(wall + 0.022, 9.87, oz0), R_TRIM, 0.0, 1 | 4 | 16 | 32);
    cuboid(&mut m, Vec3::new(wall, oy0 + 0.02, oz1), Vec3::new(wall + 0.022, 9.87, 0.67), R_TRIM, 0.0, 1 | 4 | 16 | 32);
    cuboid(&mut m, Vec3::new(wall, oy1, -0.72), Vec3::new(wall + 0.028, 9.89, 0.72), R_TRIM, 0.0, 1 | 4 | 8 | 16 | 32);

    // upper sash (outer track, fixed)
    let (ux0, ux1) = (10.12, 10.16);
    let sash = |m: &mut MeshBuilder, x0: f32, x1: f32, y0: f32, y1: f32, rail_lo: f32, rail_hi: f32, moving: f32| {
        let e = Vec2::new(moving, 0.0);
        m.cuboid(Vec3::new(x0, y0, oz0), Vec3::new(x1, y1, oz0 + 0.05), R_SASH, 0.0, 63, Vec3::ZERO, e);
        m.cuboid(Vec3::new(x0, y0, oz1 - 0.05), Vec3::new(x1, y1, oz1), R_SASH, 0.0, 63, Vec3::ZERO, e);
        m.cuboid(Vec3::new(x0, y0, oz0 + 0.05), Vec3::new(x1, y0 + rail_lo, oz1 - 0.05), R_SASH, 0.0, 63, Vec3::ZERO, e);
        m.cuboid(Vec3::new(x0, y1 - rail_hi, oz0 + 0.05), Vec3::new(x1, y1, oz1 - 0.05), R_SASH, 0.0, 63, Vec3::ZERO, e);
    };
    sash(&mut m, ux0, ux1, 8.86, oy1, 0.07, 0.06, 0.0);
    // lower sash (inner track, slides up)
    let (lx0, lx1) = (10.172, 10.212);
    sash(&mut m, lx0, lx1, 8.07, 8.95, 0.09, 0.07, 1.0);
    // brass sash lock + lifts (move with the lower sash)
    m.cuboid(Vec3::new(10.19, 8.95, -0.05), Vec3::new(10.232, 8.968, 0.05), R_BRASS, 0.0, 63, Vec3::ZERO, Vec2::new(1.0, 0.0));
    for z in [-0.32f32, 0.32] {
        m.cuboid(Vec3::new(10.212, 8.1, z - 0.03), Vec3::new(10.232, 8.13, z + 0.03), R_BRASS, 0.0, 63, Vec3::ZERO, Vec2::new(1.0, 0.0));
    }

    // cast-iron radiator under the window
    let (rx0, rx1) = (10.4, 10.6);
    let n_sec = 14;
    for k in 0..n_sec {
        let z = -0.45 + k as f32 * 0.064;
        m.cuboid(Vec3::new(rx0, 7.22, z), Vec3::new(rx1, 7.82, z + 0.05), R_RADIATOR, k as f32, 63, Vec3::new(rx0, 0.0, z), Vec2::ZERO);
    }
    for y in [7.3f32, 7.74] {
        let a = Vec3::new(10.5 - 0.022, y - 0.022, -0.45);
        let b = Vec3::new(10.5 + 0.022, y + 0.022, -0.45 + n_sec as f32 * 0.064 - 0.014);
        cuboid(&mut m, a, b, R_RADIATOR, 99.0, 63);
    }
    for z in [-0.44f32, 0.42] {
        cuboid(&mut m, Vec3::new(10.45, floor, z), Vec3::new(10.55, 7.22, z + 0.03), R_RADIATOR, 98.0, 63);
    }
    m.cylinder(Vec3::new(10.5, floor, 0.5), 0.016, 0.22, 8, R_BRASS, 0.0);
    m.cylinder(Vec3::new(10.5, 7.3, 0.5), 0.028, 0.05, 10, R_BRASS, 0.0);

    // Puddlebun the plush, looking out at the rain
    let base = Vec3::new(10.33, 8.07, 0.36);
    let yaw = Quat::from_rotation_y(0.55);
    let put = |m: &mut MeshBuilder, off: Vec3, r: Vec3, rot: Quat, mat: u32| {
        m.ellipsoid(base + yaw * off, r, yaw * rot, 18, 12, mat, 0.0, Vec2::ZERO);
    };
    put(&mut m, Vec3::new(0.0, 0.055, 0.0), Vec3::new(0.05, 0.058, 0.046), Quat::IDENTITY, R_PLUSH);
    let head = Vec3::new(-0.012, 0.128, 0.0);
    put(&mut m, head, Vec3::new(0.043, 0.04, 0.043), Quat::IDENTITY, R_PLUSH);
    for s in [-1.0f32, 1.0] {
        let tilt = Quat::from_rotation_x(0.22 * s) * Quat::from_rotation_z(0.15);
        let ear = head + Vec3::new(0.004, 0.058, 0.018 * s);
        put(&mut m, ear, Vec3::new(0.011, 0.045, 0.0085), tilt, R_PLUSH);
        put(&mut m, ear + tilt * Vec3::new(-0.006, 0.003, 0.0), Vec3::new(0.005, 0.036, 0.0055), tilt, R_PLUSH_PINK);
        put(&mut m, Vec3::new(-0.036, 0.012, 0.022 * s), Vec3::new(0.026, 0.013, 0.016), Quat::IDENTITY, R_PLUSH);
        put(&mut m, head + Vec3::new(-0.034, 0.009, 0.024 * s), Vec3::new(0.0055, 0.0065, 0.0055), Quat::IDENTITY, R_EYE);
    }
    put(&mut m, Vec3::new(0.05, 0.035, 0.0), Vec3::new(0.018, 0.018, 0.018), Quat::IDENTITY, R_PLUSH);
    put(&mut m, head + Vec3::new(-0.041, -0.008, 0.0), Vec3::new(0.006, 0.005, 0.007), Quat::IDENTITY, R_PLUSH_PINK);

    let warm = crate::weather::kelvin_to_rgb(2700.0);
    let lamps = vec![
        // a low lamp off to the right behind you, and a little light from the hall
        StaticLight { pos: Vec3::new(11.9, 8.35, 1.6), radius: 9.0, kind: 3, dir: Vec3::Y, cos_cut: 0.0, color: warm * 0.55, tint: Tint::Fixed, flicker: 0 },
        StaticLight { pos: Vec3::new(12.8, 9.4, -2.2), radius: 7.0, kind: 3, dir: Vec3::Y, cos_cut: 0.0, color: warm * 0.12, tint: Tint::Fixed, flicker: 0 },
    ];

    Room {
        mesh: m,
        lamps,
        geom: WindowGeom {
            wall_x: wall,
            z_min: oz0,
            z_max: oz1,
            y_min: oy0,
            y_max: oy1,
            glass_x: 10.14,
            inner_x: 10.192,
            sill_top: 8.07,
            upper: [-0.5, 0.5, 8.93, 9.69],
            lower: [-0.5, 0.5, 8.16, 8.88],
            max_raise: 0.1,
            eye: Vec3::new(11.6, 8.72, 0.1),
            look: Vec3::new(-12.0, 3.7, -0.4),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn city_builds() {
        let w = build_city();
        assert!(w.mesh.vertices.len() > 5000);
        assert!(w.lights.len() > 20);
        let room = w.room.as_ref().unwrap();
        assert!(room.mesh.indices.iter().all(|&i| (i as usize) < room.mesh.vertices.len()));
        assert!(w.mesh.indices.iter().all(|&i| (i as usize) < w.mesh.vertices.len()));
    }

    #[test]
    fn floors_in_range() {
        for m in 0..C_MODULES {
            assert!((5..=8).contains(&city_floors(m)));
        }
    }

    #[test]
    fn words_fit_the_font() {
        for w in WORDS {
            assert!(w.len() <= 10);
            assert!(w.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()));
        }
    }
}
