//! Raindrops on the window glass, simulated on the CPU.
//!
//! * Rain hits the outside of the panes at a rate set by the rain rate and how
//!   much the wind blows towards the window.
//! * Drops bead up and stick until they're big enough to overcome contact-angle
//!   hysteresis (~2 mm radius), then run down in stick-slip jerks, meandering,
//!   preferring existing wet trails, swallowing the drops in their path and
//!   shedding small beads behind them.
//! * Trails stay wet for a while and wipe away the fine mist.
//! * Condensation builds on the inside and clears when the window is opened.
//!
//! Each frame the drops are binned into a grid that the glass shader walks.

use bytemuck::{Pod, Zeroable};
use glam::Vec2;

pub const CELL: f32 = 0.012;
pub const GW: usize = 84;
pub const GH: usize = 64;
pub const TRAIL_TEXEL: f32 = 0.004;
pub const TW: usize = 250;
pub const TH: usize = 192;
pub const MAX_DROPS: usize = 6144;
/// Drops are drawn a little larger than simulated so they read on screen.
pub const VIS_SCALE: f32 = 1.35;
pub const MAX_ITEMS: usize = MAX_DROPS * 12;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct GpuDrop {
    pub pos: [f32; 2],
    pub r: f32,
    pub stretch: f32,
}

#[derive(Clone, Copy, Debug)]
struct Drop {
    sash: u8,
    pos: Vec2,
    r: f32,
    vel: f32,
    sliding: bool,
    crit: f32,
    travel: f32,
    next_bead: f32,
    seed: f32,
    alive: bool,
}

/// A drop hitting the glass this frame (for the audio): world z, radius in mm.
#[derive(Clone, Copy, Debug)]
pub struct Tap {
    pub z: f32,
    pub r_mm: f32,
}

pub struct GlassSim {
    drops: Vec<Drop>,
    rng: fastrand::Rng,
    trail: Vec<f32>,
    pub trail_u8: Vec<u8>,
    friction: Vec<f32>,
    /// sash glass sizes (upper, lower), metres
    pub size: [Vec2; 2],
    /// world z of the left edge of the glass
    pub z0: f32,
    pub mist: f32,
    pub cond: f32,
    pub taps: Vec<Tap>,
    pub gpu: Vec<GpuDrop>,
    pub cell_start: Vec<u32>,
    pub cell_items: Vec<u32>,
    grid_drops: Vec<u32>,
    time: f32,
}

fn smooth_noise(t: f32, seed: f32) -> f32 {
    (t * 1.7 + seed).sin() * 0.6 + (t * 3.1 + seed * 2.3).sin() * 0.3 + (t * 0.53 + seed * 0.7).sin() * 0.4
}

impl GlassSim {
    pub fn new(upper: [f32; 4], lower: [f32; 4]) -> Self {
        let mut rng = fastrand::Rng::with_seed(0x9a55);
        let friction = (0..64 * 64).map(|_| 0.35 + rng.f32() * 0.95).collect();
        let size = [Vec2::new(upper[1] - upper[0], upper[3] - upper[2]), Vec2::new(lower[1] - lower[0], lower[3] - lower[2])];
        Self {
            drops: Vec::with_capacity(MAX_DROPS),
            rng,
            trail: vec![0.0; 2 * TW * TH],
            trail_u8: vec![0; 2 * TW * TH],
            friction,
            size,
            z0: upper[0],
            mist: 0.0,
            cond: 0.0,
            taps: Vec::new(),
            gpu: Vec::with_capacity(MAX_DROPS),
            cell_start: vec![0; 2 * GW * GH + 1],
            cell_items: Vec::with_capacity(MAX_ITEMS),
            grid_drops: Vec::new(),
            time: 0.0,
        }
    }

    pub fn drop_count(&self) -> usize {
        self.drops.len()
    }

    fn friction_at(&self, sash: u8, p: Vec2) -> f32 {
        // smooth value noise over a 64x64 lattice (~2 cm)
        let q = p / 0.02 + Vec2::splat(sash as f32 * 17.0);
        let i = q.floor();
        let f = q - i;
        let at = |x: i32, y: i32| self.friction[((y.rem_euclid(64)) * 64 + x.rem_euclid(64)) as usize];
        let (x, y) = (i.x as i32, i.y as i32);
        let a = at(x, y) + (at(x + 1, y) - at(x, y)) * f.x;
        let b = at(x, y + 1) + (at(x + 1, y + 1) - at(x, y + 1)) * f.x;
        a + (b - a) * f.y
    }

    fn trail_index(sash: u8, p: Vec2) -> Option<usize> {
        let x = (p.x / TRAIL_TEXEL) as i32;
        let y = (p.y / TRAIL_TEXEL) as i32;
        if x < 0 || y < 0 || x >= TW as i32 || y >= TH as i32 {
            return None;
        }
        Some(sash as usize * TW * TH + y as usize * TW + x as usize)
    }

    fn trail_at(&self, sash: u8, p: Vec2) -> f32 {
        Self::trail_index(sash, p).map(|i| self.trail[i]).unwrap_or(0.0)
    }

    fn paint_trail(&mut self, sash: u8, p: Vec2, r: f32) {
        let n = (r / TRAIL_TEXEL).ceil() as i32;
        for dx in -n..=n {
            if let Some(i) = Self::trail_index(sash, p + Vec2::new(dx as f32 * TRAIL_TEXEL, 0.0)) {
                let w = 1.0 - (dx.abs() as f32 / (n as f32 + 1.0)).powi(2);
                self.trail[i] = self.trail[i].max(w);
            }
        }
    }

    fn spawn(&mut self, sash: u8, pos: Vec2, r: f32, tap: bool) {
        if self.drops.len() >= MAX_DROPS - 8 {
            return;
        }
        let crit = 0.0020 * self.rng.f32().mul_add(0.45, 0.8) * self.friction_at(sash, pos).sqrt();
        self.drops.push(Drop {
            sash,
            pos,
            r,
            vel: 0.0,
            sliding: false,
            crit,
            travel: 0.0,
            next_bead: r * 4.0,
            seed: self.rng.f32() * 100.0,
            alive: true,
        });
        if tap {
            self.taps.push(Tap { z: self.z0 + pos.x, r_mm: r * 1000.0 });
        }
    }

    /// Advances the simulation.
    /// `exposure`: how much rain is driven onto the glass (0 = sheltered).
    /// `raise`: how far the lower sash is raised (m). `open`: 0..1.
    pub fn update(&mut self, dt: f32, rain_mm_h: f32, exposure: f32, raise: f32, open: f32, condensation: f32) {
        let dt = dt.clamp(0.0, 0.05);
        self.time += dt;
        self.taps.clear();

        // --- new impacts ---
        let rate_per_m2 = 42.0 * rain_mm_h.max(0.0).powf(0.6) * (exposure + 0.12);
        let lower_exposed = (self.size[1].y - raise).max(0.0);
        let areas = [self.size[0].x * self.size[0].y, self.size[1].x * lower_exposed];
        let total = areas[0] + areas[1];
        let expected = rate_per_m2 * total * dt;
        let mut n = expected.floor() as usize;
        if self.rng.f32() < expected.fract() {
            n += 1;
        }
        for _ in 0..n {
            let sash = if self.rng.f32() * total < areas[0] { 0u8 } else { 1u8 };
            let sz = self.size[sash as usize];
            let h = if sash == 0 { sz.y } else { lower_exposed };
            let pos = Vec2::new(self.rng.f32() * sz.x, self.rng.f32() * h);
            let r_mm = (0.4 + -0.55 * (1.0 - self.rng.f32() * 0.98).ln()).min(2.6);
            let r = r_mm * 0.001;
            // a hit on an existing drop just feeds it
            if let Some(j) = self.find_overlap(sash, pos, r) {
                let d = &mut self.drops[j];
                d.r = (d.r.powi(3) + r.powi(3)).cbrt();
                self.taps.push(Tap { z: self.z0 + pos.x, r_mm });
            } else {
                self.spawn(sash, pos, r, true);
            }
            // big hits throw a few tiny satellite droplets
            if r_mm > 1.2 {
                for _ in 0..(self.rng.u32(1..4)) {
                    let a = self.rng.f32() * std::f32::consts::TAU;
                    let d = r * (1.5 + self.rng.f32() * 3.0);
                    let p = pos + Vec2::new(a.cos(), a.sin()) * d;
                    if p.x > 0.0 && p.y > 0.0 && p.x < sz.x && p.y < sz.y {
                        let rs = r * (0.12 + self.rng.f32() * 0.18);
                        self.spawn(sash, p, rs, false);
                    }
                }
            }
        }

        // --- runners ---
        let k_inertia = 1.0 - (-dt / 0.06).exp();
        let count = self.drops.len();
        for i in 0..count {
            if !self.drops[i].alive {
                continue;
            }
            let mut d = self.drops[i];
            if !d.sliding && d.r > d.crit {
                d.sliding = true;
            }
            if d.sliding {
                let fr = self.friction_at(d.sash, d.pos);
                let target = ((0.03 + 70.0 * (d.r - d.crit)).clamp(0.02, 0.4)) * fr;
                // stick-slip: rough patches can pin a small runner for a moment
                let pinned = fr < 0.5 && d.r < d.crit * 1.3 && smooth_noise(self.time * 2.0, d.seed) > 0.4;
                let target = if pinned { 0.0 } else { target };
                d.vel += (target - d.vel) * k_inertia;
                let dv = d.vel * dt;
                // meander, and lean towards wetter glass (old trails)
                let probe = 2.5 * TRAIL_TEXEL + d.r;
                let ahead = Vec2::new(0.0, -0.008);
                let tl = self.trail_at(d.sash, d.pos + ahead - Vec2::new(probe, 0.0));
                let tr = self.trail_at(d.sash, d.pos + ahead + Vec2::new(probe, 0.0));
                let wobble = smooth_noise(self.time * 1.3, d.seed) * 0.35;
                d.pos.x += (wobble + (tr - tl) * 0.9) * dv;
                d.pos.y -= dv;
                d.travel += dv;
                if dv > 0.0 {
                    self.paint_trail(d.sash, d.pos, d.r * 0.8);
                }
                // shed a bead now and then
                if d.travel > d.next_bead && d.r > 0.0009 {
                    let rb = d.r * self.rng.f32().mul_add(0.16, 0.22);
                    let bead_pos = d.pos + Vec2::new((self.rng.f32() - 0.5) * d.r * 0.4, d.r * 1.1);
                    d.r = (d.r.powi(3) - rb.powi(3)).max(1e-12).cbrt();
                    d.next_bead = d.travel + d.r * self.rng.f32().mul_add(4.0, 3.0);
                    self.drops[i] = d;
                    self.spawn(d.sash, bead_pos, rb, false);
                    if let Some(b) = self.drops.last_mut() {
                        b.crit *= 1.6; // beads in a trail stay put
                    }
                    d = self.drops[i];
                }
                if d.r < d.crit * 0.8 {
                    d.sliding = false;
                    d.vel = 0.0;
                }
                let w = self.size[d.sash as usize].x;
                if d.pos.y < 0.0 || d.pos.x < -0.002 || d.pos.x > w + 0.002 {
                    d.alive = false;
                }
            }
            self.drops[i] = d;
        }

        // --- merging: runners swallow what they touch ---
        self.build_grid(1.0);
        for i in 0..self.drops.len() {
            if !self.drops[i].alive || !self.drops[i].sliding {
                continue;
            }
            let d = self.drops[i];
            let c = Self::cell_of(d.pos);
            for cy in (c.1 - 1)..=(c.1 + 1) {
                for cx in (c.0 - 1)..=(c.0 + 1) {
                    if cx < 0 || cy < 0 || cx >= GW as i32 || cy >= GH as i32 {
                        continue;
                    }
                    let ci = d.sash as usize * GW * GH + cy as usize * GW + cx as usize;
                    let (a, b) = (self.cell_start[ci] as usize, self.cell_start[ci + 1] as usize);
                    for k in a..b {
                        let j = self.grid_drops[k] as usize;
                        if j == i || !self.drops[j].alive || self.drops[j].sash != d.sash {
                            continue;
                        }
                        let o = self.drops[j];
                        if o.pos.distance(self.drops[i].pos) < (o.r + self.drops[i].r) * 0.85 {
                            let me = &mut self.drops[i];
                            let v_me = me.r.powi(3);
                            let v_o = o.r.powi(3);
                            me.pos = (me.pos * v_me + o.pos * v_o) / (v_me + v_o);
                            me.r = (v_me + v_o).cbrt();
                            self.drops[j].alive = false;
                        }
                    }
                }
            }
        }
        self.drops.retain(|d| d.alive);

        // --- trails dry, mist builds, condensation follows the window ---
        let decay = (-dt / 7.0).exp();
        for (t, b) in self.trail.iter_mut().zip(self.trail_u8.iter_mut()) {
            *t *= decay;
            *b = (*t * 255.0) as u8;
        }
        let mist_target = (0.05 + 0.12 * rain_mm_h.max(0.0).powf(0.4) * (exposure + 0.25)).min(0.42);
        self.mist += (mist_target - self.mist) * (1.0 - (-dt / 40.0).exp());
        let cond_target = condensation * (1.0 - 0.85 * open);
        let tau = if cond_target < self.cond { 12.0 } else { 45.0 };
        self.cond += (cond_target - self.cond) * (1.0 - (-dt / tau).exp());

        self.build_gpu();
    }

    fn cell_of(p: Vec2) -> (i32, i32) {
        ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
    }

    fn find_overlap(&self, sash: u8, p: Vec2, r: f32) -> Option<usize> {
        // uses last frame's grid; good enough for impacts
        let c = Self::cell_of(p);
        if c.0 < 0 || c.1 < 0 || c.0 >= GW as i32 || c.1 >= GH as i32 || self.grid_drops.is_empty() {
            return None;
        }
        let ci = sash as usize * GW * GH + c.1 as usize * GW + c.0 as usize;
        let (a, b) = (self.cell_start[ci] as usize, self.cell_start[ci + 1] as usize);
        (a..b).map(|k| self.grid_drops[k] as usize).find(|&j| {
            j < self.drops.len() && self.drops[j].sash == sash && self.drops[j].pos.distance(p) < self.drops[j].r + r * 0.5
        })
    }

    /// Bins drops (by bounding box) into the grid: counting sort into CSR.
    fn build_grid(&mut self, scale: f32) {
        let cells = 2 * GW * GH;
        let mut counts = vec![0u32; cells + 1];
        let bbox = |d: &Drop| {
            let up = if d.sliding { 1.0 + (d.vel / 0.15).min(1.2) } else { 1.0 };
            let r = d.r * scale;
            let x0 = ((d.pos.x - r) / CELL).floor().max(0.0) as usize;
            let x1 = ((d.pos.x + r) / CELL).floor().min(GW as f32 - 1.0).max(0.0) as usize;
            let y0 = ((d.pos.y - r) / CELL).floor().max(0.0) as usize;
            let y1 = ((d.pos.y + r * up) / CELL).floor().min(GH as f32 - 1.0).max(0.0) as usize;
            (x0, x1, y0, y1)
        };
        for d in &self.drops {
            let (x0, x1, y0, y1) = bbox(d);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    counts[d.sash as usize * GW * GH + y * GW + x] += 1;
                }
            }
        }
        let mut acc = 0u32;
        for c in counts.iter_mut() {
            let v = *c;
            *c = acc;
            acc += v;
        }
        self.cell_start.copy_from_slice(&counts);
        let mut fill = counts;
        self.grid_drops.clear();
        self.grid_drops.resize(acc as usize, 0);
        for (i, d) in self.drops.iter().enumerate() {
            let (x0, x1, y0, y1) = bbox(d);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let ci = d.sash as usize * GW * GH + y * GW + x;
                    self.grid_drops[fill[ci] as usize] = i as u32;
                    fill[ci] += 1;
                }
            }
        }
    }

    fn build_gpu(&mut self) {
        self.build_grid(VIS_SCALE);
        self.gpu.clear();
        for d in &self.drops {
            let stretch = if d.sliding { 1.0 + (d.vel / 0.15).min(1.2) } else { 1.0 };
            self.gpu.push(GpuDrop { pos: d.pos.to_array(), r: d.r * VIS_SCALE, stretch });
        }
        self.cell_items.clear();
        let max = MAX_ITEMS.min(self.grid_drops.len());
        self.cell_items.extend_from_slice(&self.grid_drops[..max]);
        if self.grid_drops.len() > MAX_ITEMS {
            // extremely rare: clamp offsets so the shader never reads past the end
            for c in self.cell_start.iter_mut() {
                *c = (*c).min(MAX_ITEMS as u32);
            }
        }
    }

    /// Runs the simulation for a while so the glass isn't empty on arrival.
    pub fn prewarm(&mut self, seconds: f32, rain_mm_h: f32, exposure: f32, condensation: f32) {
        let steps = (seconds / 0.05) as usize;
        for _ in 0..steps {
            self.update(0.05, rain_mm_h, exposure, 0.0, 0.0, condensation);
        }
        self.taps.clear();
        self.cond = condensation;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_accumulate_and_run() {
        let mut g = GlassSim::new([-0.5, 0.5, 8.93, 9.69], [-0.5, 0.5, 8.16, 8.88]);
        g.prewarm(30.0, 8.0, 0.6, 0.5);
        let n = g.drop_count();
        assert!(n > 100, "only {n} drops after 30 s");
        assert!(n < MAX_DROPS);
        // some drops should be sliding with trails painted
        assert!(g.trail.iter().any(|&t| t > 0.1), "no trails");
        // grid must be consistent
        assert_eq!(g.cell_start.len(), 2 * GW * GH + 1);
        assert!(*g.cell_start.last().unwrap() as usize <= g.cell_items.len());
        for &i in &g.cell_items {
            assert!((i as usize) < g.gpu.len());
        }
    }

    #[test]
    fn no_rain_no_new_drops() {
        let mut g = GlassSim::new([-0.5, 0.5, 8.93, 9.69], [-0.5, 0.5, 8.16, 8.88]);
        g.update(0.05, 0.0, 0.0, 0.0, 0.0, 0.0);
        let before = g.drop_count();
        for _ in 0..20 {
            g.update(0.05, 0.0, 0.0, 0.0, 0.0, 0.0);
        }
        // the 0.12 baseline still lets a little through even at 0 mm/h? rate ∝ R^0.6 = 0
        assert_eq!(g.drop_count(), before);
    }
}
