//! The residential street, built procedurally: terraced houses with bay
//! windows, chimneys, garden walls, pavements, kerbs and street lamps.
//! Materials are shaded procedurally in src/shaders/scene.wgsl.

use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};

// Material ids (must match scene.wgsl)
pub const M_ROAD: u32 = 0;
pub const M_PAVE: u32 = 1;
pub const M_KERB: u32 = 2;
pub const M_FACADE: u32 = 3;
pub const M_ROOF: u32 = 4;
pub const M_METAL: u32 = 5;
pub const M_LAMP: u32 = 6;
pub const M_WALL: u32 = 7;
pub const M_BAY: u32 = 8;
pub const M_CHIMNEY: u32 = 9;
pub const M_POT: u32 = 10;
pub const M_GARDEN: u32 = 11;
pub const M_COPING: u32 = 12;
pub const M_LEAD: u32 = 13;

pub const ROAD_HALF: f32 = 3.6;
pub const KERB_OUT: f32 = 3.75;
pub const KERB_H: f32 = 0.12;
pub const PAVE_OUT: f32 = 5.8;
pub const WALL_OUT: f32 = 6.0;
pub const FACADE_X: f32 = 8.0;
pub const HOUSE_DEPTH: f32 = 10.0;
pub const HOUSE_W: f32 = 5.2;
pub const EAVE_H: f32 = 6.2;
pub const RIDGE_H: f32 = 8.8;
pub const Z_START: f32 = -78.0;
pub const HOUSES_PER_SIDE: u32 = 64;

/// lowbias32 — identical to `hash_u` in common.wgsl.
pub fn hash_u(x: u32) -> u32 {
    let mut v = x;
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^= v >> 16;
    v
}
pub fn hash_uu(a: u32, b: u32) -> u32 {
    hash_u(a ^ hash_u(b ^ 0x9e37_79b9))
}
pub fn u01(h: u32) -> f32 {
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub mat: u32,
    pub normal: [f32; 3],
    pub seed: f32,
    pub uv: [f32; 2],
    pub extra: [f32; 2],
}

#[derive(Default)]
pub struct MeshBuilder {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshBuilder {
    /// Adds a quad. Corners in order around the quad; uv per corner (metres).
    #[allow(clippy::too_many_arguments)]
    pub fn quad(&mut self, p: [Vec3; 4], n: Vec3, uv: [Vec2; 4], mat: u32, seed: f32, extra: Vec2) {
        let base = self.vertices.len() as u32;
        for k in 0..4 {
            self.vertices.push(Vertex {
                pos: p[k].to_array(),
                mat,
                normal: n.to_array(),
                seed,
                uv: uv[k].to_array(),
                extra: extra.to_array(),
            });
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Axis-aligned box. `faces` bits: 1 +X, 2 -X, 4 +Y, 8 -Y, 16 +Z, 32 -Z.
    /// UVs are world metres relative to `uvo`: ±X faces (z, y), ±Z faces (x, y), ±Y faces (x, z).
    #[allow(clippy::too_many_arguments)]
    pub fn cuboid(&mut self, min: Vec3, max: Vec3, mat: u32, seed: f32, faces: u8, uvo: Vec3, extra: Vec2) {
        let (a, b) = (min, max);
        let uvx = |p: Vec3| Vec2::new(p.z - uvo.z, p.y - uvo.y);
        let uvz = |p: Vec3| Vec2::new(p.x - uvo.x, p.y - uvo.y);
        let uvy = |p: Vec3| Vec2::new(p.x - uvo.x, p.z - uvo.z);
        if faces & 1 != 0 {
            let q = [Vec3::new(b.x, a.y, a.z), Vec3::new(b.x, a.y, b.z), Vec3::new(b.x, b.y, b.z), Vec3::new(b.x, b.y, a.z)];
            self.quad(q, Vec3::X, q.map(uvx), mat, seed, extra);
        }
        if faces & 2 != 0 {
            let q = [Vec3::new(a.x, a.y, b.z), Vec3::new(a.x, a.y, a.z), Vec3::new(a.x, b.y, a.z), Vec3::new(a.x, b.y, b.z)];
            self.quad(q, -Vec3::X, q.map(uvx), mat, seed, extra);
        }
        if faces & 4 != 0 {
            let q = [Vec3::new(a.x, b.y, a.z), Vec3::new(b.x, b.y, a.z), Vec3::new(b.x, b.y, b.z), Vec3::new(a.x, b.y, b.z)];
            self.quad(q, Vec3::Y, q.map(uvy), mat, seed, extra);
        }
        if faces & 8 != 0 {
            let q = [Vec3::new(a.x, a.y, b.z), Vec3::new(b.x, a.y, b.z), Vec3::new(b.x, a.y, a.z), Vec3::new(a.x, a.y, a.z)];
            self.quad(q, -Vec3::Y, q.map(uvy), mat, seed, extra);
        }
        if faces & 16 != 0 {
            let q = [Vec3::new(b.x, a.y, b.z), Vec3::new(a.x, a.y, b.z), Vec3::new(a.x, b.y, b.z), Vec3::new(b.x, b.y, b.z)];
            self.quad(q, Vec3::Z, q.map(uvz), mat, seed, extra);
        }
        if faces & 32 != 0 {
            let q = [Vec3::new(a.x, a.y, a.z), Vec3::new(b.x, a.y, a.z), Vec3::new(b.x, b.y, a.z), Vec3::new(a.x, b.y, a.z)];
            self.quad(q, -Vec3::Z, q.map(uvz), mat, seed, extra);
        }
    }

    pub fn cylinder(&mut self, base: Vec3, radius: f32, height: f32, segs: u32, mat: u32, seed: f32) {
        for i in 0..segs {
            let a0 = i as f32 / segs as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / segs as f32 * std::f32::consts::TAU;
            let d0 = Vec3::new(a0.cos(), 0.0, a0.sin());
            let d1 = Vec3::new(a1.cos(), 0.0, a1.sin());
            let p = [
                base + d0 * radius,
                base + d1 * radius,
                base + d1 * radius + Vec3::Y * height,
                base + d0 * radius + Vec3::Y * height,
            ];
            let n = (d0 + d1).normalize();
            let u0 = a0 * radius;
            let u1 = a1 * radius;
            let uv = [Vec2::new(u0, 0.0), Vec2::new(u1, 0.0), Vec2::new(u1, height), Vec2::new(u0, height)];
            self.quad(p, n, uv, mat, seed, Vec2::ZERO);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct LampDef {
    /// Where the light is emitted (just under the lamp head).
    pub pos: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub struct WindowDef {
    pub pos: Vec3,
    pub normal: Vec3,
    pub house: u32,
    pub window: u32,
}

pub struct Street {
    pub mesh: MeshBuilder,
    pub lamps: Vec<LampDef>,
    pub windows: Vec<WindowDef>,
}

pub fn house_id(side: f32, i: u32) -> u32 {
    if side > 0.0 { 1000 + i } else { 2000 + i }
}

pub fn build_street() -> Street {
    let mut m = MeshBuilder::default();
    let z0 = Z_START;
    let z1 = Z_START + HOUSES_PER_SIDE as f32 * HOUSE_W;
    let zero = Vec2::ZERO;

    // road surface
    m.quad(
        [Vec3::new(-ROAD_HALF, 0.0, z0), Vec3::new(ROAD_HALF, 0.0, z0), Vec3::new(ROAD_HALF, 0.0, z1), Vec3::new(-ROAD_HALF, 0.0, z1)],
        Vec3::Y,
        [Vec2::new(-ROAD_HALF, z0), Vec2::new(ROAD_HALF, z0), Vec2::new(ROAD_HALF, z1), Vec2::new(-ROAD_HALF, z1)],
        M_ROAD,
        0.0,
        zero,
    );

    for side in [1.0f32, -1.0] {
        let sx = |x: f32| x * side;
        // an x-range on this side of the street, as (min, max)
        let span = |a: f32, b: f32| -> (f32, f32) { if side > 0.0 { (a, b) } else { (-b, -a) } };

        // kerb stones (top + road-facing face)
        let (kx0, kx1) = span(ROAD_HALF, KERB_OUT);
        let kface = if side > 0.0 { 2 } else { 1 };
        m.cuboid(Vec3::new(kx0, 0.0, z0), Vec3::new(kx1, KERB_H, z1), M_KERB, 0.0, 4 | kface, Vec3::ZERO, zero);
        // pavement
        let (px0, px1) = span(KERB_OUT, PAVE_OUT);
        m.cuboid(Vec3::new(px0, 0.0, z0), Vec3::new(px1, KERB_H, z1), M_PAVE, 0.0, 4, Vec3::ZERO, zero);
        // front gardens
        let (gx0, gx1) = span(WALL_OUT, FACADE_X);
        m.cuboid(Vec3::new(gx0, 0.0, z0), Vec3::new(gx1, KERB_H, z1), M_GARDEN, 0.0, 4, Vec3::ZERO, zero);
        // ground behind the houses (seen when flying)
        let (bx0, bx1) = span(FACADE_X + HOUSE_DEPTH, FACADE_X + HOUSE_DEPTH + 14.0);
        m.cuboid(Vec3::new(bx0, 0.0, z0), Vec3::new(bx1, KERB_H, z1), M_GARDEN, 0.0, 4, Vec3::ZERO, zero);

        for i in 0..HOUSES_PER_SIDE {
            let hz = z0 + i as f32 * HOUSE_W;
            let hid = house_id(side, i);
            let seed = hid as f32;
            let uvo = Vec3::new(0.0, 0.0, hz);
            let road_face = if side > 0.0 { 2u8 } else { 1u8 };
            let back_face = if side > 0.0 { 1u8 } else { 2u8 };

            // main facade (street side) and back wall
            let (fx0, fx1) = span(FACADE_X, FACADE_X + HOUSE_DEPTH);
            m.cuboid(Vec3::new(fx0, 0.0, hz), Vec3::new(fx1, EAVE_H, hz + HOUSE_W), M_FACADE, seed, road_face, uvo, zero);
            m.cuboid(Vec3::new(fx0, 0.0, hz), Vec3::new(fx1, EAVE_H, hz + HOUSE_W), M_WALL, seed, back_face, uvo, zero);
            if i == 0 {
                m.cuboid(Vec3::new(fx0, 0.0, hz), Vec3::new(fx1, EAVE_H, hz + HOUSE_W), M_WALL, seed, 32, uvo, zero);
            }
            if i == HOUSES_PER_SIDE - 1 {
                m.cuboid(Vec3::new(fx0, 0.0, hz), Vec3::new(fx1, EAVE_H, hz + HOUSE_W), M_WALL, seed, 16, uvo, zero);
            }

            // pitched slate roof with a small overhang at the front
            let eave_x = sx(FACADE_X - 0.25);
            let ridge_x = sx(FACADE_X + HOUSE_DEPTH * 0.5);
            let back_x = sx(FACADE_X + HOUSE_DEPTH + 0.25);
            let eave_y = EAVE_H - 0.12;
            let slope_len = ((ridge_x - eave_x).powi(2) + (RIDGE_H - eave_y).powi(2)).sqrt();
            let nf = Vec3::new(-(RIDGE_H - eave_y) * side, (ridge_x - eave_x).abs(), 0.0).normalize();
            m.quad(
                [Vec3::new(eave_x, eave_y, hz), Vec3::new(eave_x, eave_y, hz + HOUSE_W), Vec3::new(ridge_x, RIDGE_H, hz + HOUSE_W), Vec3::new(ridge_x, RIDGE_H, hz)],
                nf,
                [Vec2::new(hz, 0.0), Vec2::new(hz + HOUSE_W, 0.0), Vec2::new(hz + HOUSE_W, slope_len), Vec2::new(hz, slope_len)],
                M_ROOF,
                seed,
                zero,
            );
            let nb = Vec3::new(-nf.x, nf.y, 0.0);
            m.quad(
                [Vec3::new(back_x, eave_y, hz), Vec3::new(back_x, eave_y, hz + HOUSE_W), Vec3::new(ridge_x, RIDGE_H, hz + HOUSE_W), Vec3::new(ridge_x, RIDGE_H, hz)],
                nb,
                [Vec2::new(hz, 0.0), Vec2::new(hz + HOUSE_W, 0.0), Vec2::new(hz + HOUSE_W, slope_len), Vec2::new(hz, slope_len)],
                M_ROOF,
                seed,
                zero,
            );
            // fascia board under the eave
            let (ex0, ex1) = span(FACADE_X - 0.25, FACADE_X);
            m.cuboid(Vec3::new(ex0, eave_y - 0.18, hz), Vec3::new(ex1, eave_y, hz + HOUSE_W), M_LEAD, seed, 1 | 2 | 8, uvo, zero);

            // chimney stack on every other party wall
            if i % 2 == 0 {
                let (cx0, cx1) = span(FACADE_X + HOUSE_DEPTH * 0.5 - 0.55, FACADE_X + HOUSE_DEPTH * 0.5 + 0.55);
                let ctop = RIDGE_H + 1.2;
                m.cuboid(Vec3::new(cx0, RIDGE_H - 1.0, hz - 0.42), Vec3::new(cx1, ctop, hz + 0.42), M_CHIMNEY, seed, 1 | 2 | 4 | 16 | 32, uvo, zero);
                let pots = 2 + (hash_u(hid) % 3);
                for k in 0..pots {
                    let pz = hz - 0.28 + 0.56 * (k as f32 + 0.5) / pots as f32;
                    let pxc = (cx0 + cx1) * 0.5;
                    m.cylinder(Vec3::new(pxc, ctop, pz), 0.11, 0.42 + 0.15 * u01(hash_uu(hid, k)), 8, M_POT, seed);
                }
            }

            // ground-floor bay window
            let (bx0b, bx1b) = span(FACADE_X - 0.8, FACADE_X);
            let bay_min = Vec3::new(bx0b, KERB_H, hz + 0.5);
            let bay_max = Vec3::new(bx1b, 2.9, hz + 2.9);
            let outer_face = if side > 0.0 { 2u8 } else { 1u8 };
            // front face: uv.x = z - hz, uv.y = height
            m.cuboid(bay_min, bay_max, M_BAY, seed, outer_face, uvo, Vec2::new(0.0, side));
            // side faces, built so uv.x runs from the bay front towards the wall
            let bay_front_x = sx(FACADE_X - 0.8);
            let side_extra = Vec2::new(0.0, side);
            for (z, n) in [(hz + 0.5, -Vec3::Z), (hz + 2.9, Vec3::Z)] {
                let (xa, xb) = (sx(FACADE_X - 0.8), sx(FACADE_X));
                let q = [Vec3::new(xa, KERB_H, z), Vec3::new(xb, KERB_H, z), Vec3::new(xb, 2.9, z), Vec3::new(xa, 2.9, z)];
                let uv = q.map(|p| Vec2::new((p.x - bay_front_x).abs(), p.y));
                m.quad(q, n, uv, M_BAY, seed, side_extra);
            }
            // little lead roof on the bay
            let (rx0, rx1) = span(FACADE_X - 0.9, FACADE_X);
            m.cuboid(Vec3::new(rx0, 2.9, hz + 0.4), Vec3::new(rx1, 3.12, hz + 3.0), M_LEAD, seed, 1 | 2 | 4 | 16 | 32, uvo, zero);

            // garden wall with coping, gap for the gate in front of the door
            let (wx0, wx1) = span(PAVE_OUT, WALL_OUT);
            let wall_h = 0.75;
            for (za, zb) in [(hz, hz + 3.55), (hz + 4.65, hz + HOUSE_W)] {
                m.cuboid(Vec3::new(wx0, KERB_H, za), Vec3::new(wx1, wall_h, zb), M_WALL, seed, 1 | 2 | 16 | 32, uvo, zero);
                m.cuboid(Vec3::new(wx0 - 0.03, wall_h, za), Vec3::new(wx1 + 0.03, wall_h + 0.07, zb), M_COPING, seed, 1 | 2 | 4 | 16 | 32, uvo, zero);
            }
        }
    }

    // street lamps every 30 m, alternating sides
    let mut lamps = Vec::new();
    for k in -3i32..=8 {
        let z = 15.0 + 30.0 * k as f32;
        let side = if k.rem_euclid(2) == 0 { 1.0 } else { -1.0 };
        let post = Vec3::new(3.98 * side, KERB_H, z);
        m.cylinder(post, 0.075, 6.0, 10, M_METAL, 0.0);
        m.cylinder(post, 0.13, 0.6, 10, M_METAL, 0.0);
        // arm reaching over the road
        let arm_end = 2.75 * side;
        let (ax0, ax1) = if side > 0.0 { (arm_end, 3.98) } else { (-3.98, arm_end) };
        m.cuboid(Vec3::new(ax0, 5.95, z - 0.04), Vec3::new(ax1, 6.05, z + 0.04), M_METAL, 0.0, 63, Vec3::ZERO, zero);
        // lamp head (LED panel underneath)
        let hx = arm_end;
        m.cuboid(Vec3::new(hx - 0.32, 5.84, z - 0.14), Vec3::new(hx + 0.32, 5.96, z + 0.14), M_LAMP, 0.0, 63, Vec3::new(hx, 0.0, z), zero);
        lamps.push(LampDef { pos: Vec3::new(hx, 5.78, z) });
    }

    // windows that can spill light onto the street
    let mut windows = Vec::new();
    for side in [1.0f32, -1.0] {
        for i in 0..HOUSES_PER_SIDE {
            let hz = z0 + i as f32 * HOUSE_W;
            let hid = house_id(side, i);
            let n = Vec3::new(-side, 0.0, 0.0);
            windows.push(WindowDef { pos: Vec3::new(side * (FACADE_X - 1.1), 1.7, hz + 1.7), normal: n, house: hid, window: 0 });
            windows.push(WindowDef { pos: Vec3::new(side * (FACADE_X - 0.4), 4.4, hz + 1.7), normal: n, house: hid, window: 1 });
            windows.push(WindowDef { pos: Vec3::new(side * (FACADE_X - 0.4), 4.4, hz + 4.1), normal: n, house: hid, window: 2 });
        }
    }

    Street { mesh: m, lamps, windows }
}

/// Same rule as `is_lit` in scene.wgsl.
pub fn window_is_lit(house: u32, window: u32, fraction: f32) -> bool {
    u01(hash_uu(house, window)) < fraction
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn street_builds() {
        let s = build_street();
        assert!(s.mesh.vertices.len() > 1000);
        assert_eq!(s.mesh.indices.len() % 3, 0);
        assert!(s.lamps.len() >= 10);
        assert!(s.mesh.indices.iter().all(|&i| (i as usize) < s.mesh.vertices.len()));
    }

    #[test]
    fn hash_is_well_distributed() {
        // a quarter of windows lit -> roughly a quarter reported lit
        let lit = (0..4000).filter(|&h| window_is_lit(1000 + h, 1, 0.25)).count();
        assert!((800..1200).contains(&lit), "{lit}");
        assert_eq!(hash_u(12345), hash_u(12345));
        assert_ne!(hash_u(1), hash_u(2));
    }
}
