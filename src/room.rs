//! The room by the window: the sliding sash, drips off the window head and
//! the stone sill, spatter through the open crack, the wet inner sill, and
//! the raindrops on the glass. Shared by the windowed app and headless mode.

use crate::audio::{SURF_GLASS, SURF_METAL, SURF_PAVE, SURF_STONE, SURF_WOOD};
use crate::city::WindowGeom;
use crate::glass::GlassSim;
use crate::render::rain::{GpuDrop, GpuSplash, CPU_DROPS, CPU_SPLASHES, CPU_SPLASHES_OUTSIDE};
use crate::render::RoomState;
use crate::settings::Settings;
use crate::weather::WeatherState;
use glam::Vec3;

const OPEN_SECONDS: f32 = 1.25;
const CLOSE_SECONDS: f32 = 1.0;
const G: f32 = 9.81;

/// Something the audio should hear this frame.
#[derive(Clone, Copy, Debug)]
pub enum Sound {
    /// a drop landing somewhere; `inside` = in the room (not muffled by the glass)
    Hit { pos: Vec3, size_mm: f32, speed: f32, surface: u8, inside: bool },
    /// the lower sash starts sliding
    Slide { opening: bool, seconds: f32 },
}

/// A place where water gathers and lets go: a drip point.
struct Emitter {
    pos: Vec3,
    land_y: f32,
    surface: u8,
    /// how quickly this point drips compared to the others
    rate: f32,
    /// facade wetness feeding this point: 0 = window head (wind-driven), 1 = sill (all rain)
    sheltered: bool,
    next: f32,
}

struct Drip {
    pos: Vec3,
    vel: Vec3,
    size_mm: f32,
    seed: u32,
    land_y: f32,
    surface: u8,
}

struct SashAnim {
    from: f32,
    to: f32,
    t: f32,
    seconds: f32,
}

pub struct RoomSim {
    pub geom: WindowGeom,
    pub glass: GlassSim,
    /// how far the lower sash is raised (m)
    pub raise: f32,
    anim: Option<SashAnim>,
    open_target: bool,
    /// water on the inner sill, 0..1
    pub sill_wet: f32,
    /// how wet the facade above is (lags behind the rain)
    feed: f32,
    emitters: Vec<Emitter>,
    drips: Vec<Drip>,
    gpu_drips: Vec<GpuDrop>,
    splashes: Vec<GpuSplash>,
    head_out: usize,
    head_in: usize,
    spatter_acc: f32,
    pub sounds: Vec<Sound>,
    rng: fastrand::Rng,
    seed: u32,
}

/// How much rain the wind drives onto our window (0 = none; it faces -x).
pub fn exposure(w: &WeatherState) -> f32 {
    (w.wind.x.max(0.0) / 6.5 * 2.2 + w.wind.length() * 0.06).clamp(0.0, 1.6)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl RoomSim {
    pub fn new(geom: WindowGeom) -> Self {
        let mut emitters = Vec::new();
        let mut add = |pos: Vec3, land_y: f32, surface: u8, sheltered: bool, k: u32| {
            let seed = crate::scene::hash_uu(k, 0xd819);
            let rate = 0.45 + 1.2 * crate::scene::u01(seed);
            emitters.push(Emitter { pos, land_y, surface, rate, sheltered, next: crate::scene::u01(seed ^ 77) * 3.0 });
        };
        // drip groove under the window head, just outside the glass
        for (k, z) in [-0.43f32, -0.2, 0.03, 0.27, 0.46].into_iter().enumerate() {
            add(Vec3::new(10.012, geom.y_max - 0.004, z), 7.98, SURF_STONE, false, k as u32);
        }
        // front edge of the stone sill, onto the fire escape grating
        for (k, z) in [-0.52f32, -0.14, 0.31, 0.6].into_iter().enumerate() {
            add(Vec3::new(9.815, 7.875, z), 7.2, SURF_METAL, true, 10 + k as u32);
        }
        // fire escape railing, all the way down to the pavement
        for (k, z) in [-2.9f32, -1.7, -0.4, 0.5].into_iter().enumerate() {
            add(Vec3::new(8.86, 8.14, z), 0.15, SURF_PAVE, true, 20 + k as u32);
        }
        Self {
            glass: GlassSim::new(geom.upper, geom.lower),
            geom,
            raise: 0.0,
            anim: None,
            open_target: false,
            sill_wet: 0.0,
            feed: 0.0,
            emitters,
            drips: Vec::new(),
            gpu_drips: Vec::new(),
            splashes: vec![GpuSplash::default(); CPU_SPLASHES as usize],
            head_out: 0,
            head_in: 0,
            spatter_acc: 0.0,
            sounds: Vec::new(),
            rng: fastrand::Rng::with_seed(0x005e_ed0f_9a55),
            seed: 1,
        }
    }

    /// 0 = shut, 1 = open as far as it goes (a 10 cm crack).
    pub fn open(&self) -> f32 {
        (self.raise / self.geom.max_raise).clamp(0.0, 1.0)
    }

    pub fn is_moving(&self) -> bool {
        self.anim.is_some()
    }

    fn feed_target(w: &WeatherState) -> f32 {
        (w.intensity / 6.0).max(0.0).powf(0.6).min(3.0)
    }

    /// Settles everything so the window isn't dry when you arrive.
    pub fn prewarm(&mut self, w: &WeatherState, s: &Settings) {
        self.open_target = s.window_open;
        self.raise = if s.window_open { self.geom.max_raise } else { 0.0 };
        self.anim = None;
        self.feed = Self::feed_target(w);
        self.glass.prewarm(45.0, w.intensity, exposure(w), s.condensation);
        if s.window_open {
            self.sill_wet = (w.intensity / 8.0).min(0.8);
        }
    }

    pub fn state(&self) -> RoomState<'_> {
        RoomState { raise: self.raise, sill_wet: self.sill_wet, glass: &self.glass }
    }

    pub fn splashes(&self) -> &[GpuSplash] {
        &self.splashes
    }

    pub fn drips(&self) -> &[GpuDrop] {
        &self.gpu_drips
    }

    fn next_seed(&mut self) -> u32 {
        self.seed = crate::scene::hash_u(self.seed.wrapping_add(0x9e37_79b9));
        self.seed
    }

    fn splash(&mut self, inside: bool, pos: Vec3, size: f32, speed: f32, kind: u32, time: f32) {
        let seed = self.next_seed();
        let s = GpuSplash { pos: pos.into(), t0: time, size, kind, seed, speed };
        let n_out = CPU_SPLASHES_OUTSIDE as usize;
        let n_in = CPU_SPLASHES as usize - n_out;
        if inside {
            self.splashes[n_out + self.head_in] = s;
            self.head_in = (self.head_in + 1) % n_in;
        } else {
            self.splashes[self.head_out] = s;
            self.head_out = (self.head_out + 1) % n_out;
        }
    }

    pub fn update(&mut self, dt: f32, time: f32, w: &WeatherState, s: &Settings) {
        let dt = dt.clamp(0.0, 0.1);
        self.sounds.clear();

        // --- the sash: it sticks for a moment, then slides ---
        if s.window_open != self.open_target {
            self.open_target = s.window_open;
            let to = if s.window_open { self.geom.max_raise } else { 0.0 };
            let seconds = if s.window_open { OPEN_SECONDS } else { CLOSE_SECONDS };
            let from = self.raise;
            let seconds = seconds * ((to - from).abs() / self.geom.max_raise).max(0.35);
            self.anim = Some(SashAnim { from, to, t: 0.0, seconds });
            self.sounds.push(Sound::Slide { opening: s.window_open, seconds });
        }
        if let Some(a) = &mut self.anim {
            a.t += dt / a.seconds;
            let t = a.t.min(1.0);
            let k = 0.05 * smoothstep(0.0, 0.14, t) + 0.95 * smoothstep(0.2, 1.0, t);
            self.raise = a.from + (a.to - a.from) * k;
            if a.t >= 1.0 {
                self.raise = a.to;
                self.anim = None;
            }
        }
        let open = self.open();
        let ex = exposure(w);

        // --- glass ---
        self.glass.update(dt, w.intensity, ex, self.raise, open, s.condensation);
        let glass_x = self.geom.glass_x;
        let mid_y = 0.5 * (self.geom.y_min + self.geom.y_max);
        for t in &self.glass.taps {
            let speed = 4.0 + 2.5 * (t.r_mm / 1.5).min(1.0);
            self.sounds.push(Sound::Hit { pos: Vec3::new(glass_x, mid_y, t.z), size_mm: t.r_mm * 2.0, speed, surface: SURF_GLASS, inside: true });
        }

        // --- facade wetness feeding the drip points ---
        let target = Self::feed_target(w);
        let tau = if target > self.feed { 25.0 } else { 110.0 };
        self.feed += (target - self.feed) * (1.0 - (-dt / tau).exp());

        // --- drips letting go ---
        for i in 0..self.emitters.len() {
            let e = &self.emitters[i];
            let wet = if e.sheltered { self.feed } else { self.feed * (0.3 + 0.7 * ex) };
            if wet < 0.03 {
                continue;
            }
            let mut next = e.next - dt * wet * e.rate;
            if next <= 0.0 {
                let jitter = self.rng.f32();
                // pendant drops tear off at ~4–5 mm; sometimes a quick double
                next += if jitter < 0.12 { 0.18 } else { 1.6 + 2.4 * self.rng.f32() };
                let size_mm = 3.6 + 1.5 * self.rng.f32();
                let seed = self.next_seed();
                let e = &self.emitters[i];
                let mut vel = Vec3::new(0.0, -0.25, 0.0);
                vel.x += w.wind.x * 0.004;
                vel.z += w.wind.z * 0.004;
                if self.drips.len() < CPU_DROPS as usize {
                    self.drips.push(Drip { pos: e.pos, vel, size_mm, seed, land_y: e.land_y, surface: e.surface });
                }
            }
            self.emitters[i].next = next;
        }

        // --- falling drips ---
        let mut landed = Vec::new();
        self.drips.retain_mut(|d| {
            d.vel.y -= G * dt;
            // a little air drag on the way down
            d.vel += (Vec3::new(w.wind.x, 0.0, w.wind.z) * 0.05 - Vec3::new(d.vel.x, 0.0, d.vel.z)) * (dt * 0.6);
            d.pos += d.vel * dt;
            d.pos.x = d.pos.x.min(10.1);
            if d.pos.y <= d.land_y {
                landed.push((Vec3::new(d.pos.x, d.land_y, d.pos.z), d.size_mm, d.vel.length(), d.surface));
                false
            } else {
                true
            }
        });
        for (pos, size, speed, surface) in landed {
            self.sounds.push(Sound::Hit { pos, size_mm: size, speed, surface, inside: false });
            self.splash(false, pos + Vec3::Y * 0.002, size * 0.8, speed, 2, time);
        }
        self.gpu_drips.clear();
        self.gpu_drips.extend(self.drips.iter().map(|d| GpuDrop {
            pos: d.pos.into(),
            size: d.size_mm,
            vel: d.vel.into(),
            seed: d.seed,
            // w = 1: lit in the shader (CPU drops have no precomputed light)
            light: [0.0, 0.0, 0.0, 1.0],
        }));

        // --- spatter through the crack onto the inner sill ---
        let mut spat_rate = 0.0;
        if open > 0.02 && w.intensity > 0.05 {
            let width = self.geom.z_max - self.geom.z_min;
            spat_rate = 30.0 * w.intensity.powf(0.6) * (ex + 0.15) * open * width;
            self.spatter_acc += spat_rate * dt;
            let reach = 0.09 + 0.05 * w.wind.x.clamp(0.0, 6.0);
            while self.spatter_acc >= 1.0 {
                self.spatter_acc -= 1.0;
                let depth = self.rng.f32().powf(1.3) * reach;
                let x = (self.geom.inner_x + 0.028 + depth).min(10.5);
                let z = self.geom.z_min + 0.06 + self.rng.f32() * (width - 0.12);
                let pos = Vec3::new(x, self.geom.sill_top + 0.002, z);
                let size = 0.5 + 1.7 * self.rng.f32().powi(2);
                let speed = 2.0 + 3.0 * self.rng.f32();
                self.splash(true, pos, size, speed, 2, time);
                self.sounds.push(Sound::Hit { pos, size_mm: size, speed, surface: SURF_WOOD, inside: true });
            }
        }
        // the sill soaks up spatter and dries slowly
        self.sill_wet = (self.sill_wet + spat_rate * 0.00025 * dt).min(1.0);
        self.sill_wet *= (-dt / 300.0).exp();
    }
}

impl RoomSim {
    /// This frame's room sounds as audio messages, placed around the listener.
    pub fn audio_msgs(&self, cam: &crate::camera::CameraState, sr: f32, frame_dt: f32, rng: &mut fastrand::Rng) -> Vec<crate::audio::AudioMsg> {
        use crate::audio::{hit, AudioMsg};
        // at most ~600 glass/spatter voices a second; drips always get through
        let budget = (600.0 * frame_dt).ceil() as usize;
        let mut small = 0;
        let mut out = Vec::with_capacity(self.sounds.len().min(budget + 8));
        for s in &self.sounds {
            match *s {
                Sound::Slide { opening, seconds } => out.push(AudioMsg::Slide { opening, seconds }),
                Sound::Hit { pos, size_mm, speed, surface, inside } => {
                    let is_drip = surface == SURF_STONE || surface == SURF_METAL || surface == SURF_PAVE;
                    if !is_drip {
                        small += 1;
                        if small > budget {
                            continue;
                        }
                    }
                    let delay = (rng.f32() * frame_dt.min(0.05) * sr) as u32;
                    out.push(AudioMsg::Impact(hit(pos, cam, sr, size_mm, speed, surface, inside, delay)));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneKind;

    fn rainy() -> WeatherState {
        WeatherState { intensity: 8.0, wind: Vec3::new(3.0, 0.0, 1.0), ..Default::default() }
    }

    #[test]
    fn drips_fall_and_land() {
        let world = crate::city::build_city();
        assert_eq!(world.kind, SceneKind::City);
        let geom = world.room.as_ref().unwrap().geom;
        let mut r = RoomSim::new(geom);
        let s = Settings::default();
        let w = rainy();
        r.prewarm(&w, &s);
        let mut hits = 0;
        let mut seen_drips = 0;
        for i in 0..600 {
            r.update(1.0 / 60.0, i as f32 / 60.0, &w, &s);
            seen_drips = seen_drips.max(r.drips().len());
            hits += r.sounds.iter().filter(|s| matches!(s, Sound::Hit { surface, .. } if *surface == SURF_STONE || *surface == SURF_METAL)).count();
        }
        assert!(seen_drips > 0, "no drips in 10 s");
        assert!(hits > 3, "only {hits} drip landings in 10 s");
    }

    #[test]
    fn sash_opens_and_spatters() {
        let geom = crate::city::build_room().geom;
        let mut r = RoomSim::new(geom);
        let mut s = Settings::default();
        let w = rainy();
        r.prewarm(&w, &s);
        assert_eq!(r.open(), 0.0);
        s.window_open = true;
        let mut slid = false;
        let mut spatter = 0;
        for i in 0..240 {
            r.update(1.0 / 60.0, i as f32 / 60.0, &w, &s);
            slid |= r.sounds.iter().any(|s| matches!(s, Sound::Slide { opening: true, .. }));
            spatter += r.sounds.iter().filter(|s| matches!(s, Sound::Hit { surface, .. } if *surface == SURF_WOOD)).count();
        }
        assert!(slid);
        assert!((r.open() - 1.0).abs() < 1e-4);
        assert!(spatter > 20, "only {spatter} spatter drops");
        assert!(r.sill_wet > 0.0);
        // and closes again
        s.window_open = false;
        for i in 0..120 {
            r.update(1.0 / 60.0, 4.0 + i as f32 / 60.0, &w, &s);
        }
        assert_eq!(r.open(), 0.0);
    }
}
