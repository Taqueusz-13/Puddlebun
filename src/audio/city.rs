//! City life: sparse sounds that come and go under the rain. Wet tyres
//! hissing past, the el-train rumbling over its joints, a siren somewhere far
//! off, a horn, neon buzzing, the city's low hum. All scaled by the
//! "city life" slider; in the residential street only the odd car passes.

use super::dsp::*;
use std::f32::consts::TAU;

const BLOCK: u32 = 64;

#[derive(Clone, Copy, Default)]
pub struct CityIn {
    /// 0..1 slider
    pub life: f32,
    /// 0 = residential street, 1 = neon city
    pub city: f32,
    pub mm_h: f32,
}

/// One car swishing past on the wet road.
#[derive(Clone, Copy, Default)]
struct Car {
    live: bool,
    t: f32,
    dur: f32,
    dir: f32,
    level: f32,
    eng_f: f32,
    eng_phase: f32,
    speed: f32,
    dist: f32,
    hiss: [Biquad; 2],
    spray: Biquad,
    eng_lp: Biquad,
}

#[derive(Clone, Copy, Default)]
struct Siren {
    live: bool,
    t: f32,
    dur: f32,
    phase: f32,
    period: f32,
    yelp: bool,
    pan: f32,
    drift: f32,
    level: f32,
    lp: [Biquad; 2],
}

#[derive(Clone, Copy, Default)]
struct Train {
    live: bool,
    t: f32,
    dur: f32,
    dir: f32,
    level: f32,
    clack_t: f32,
    clack_period: f32,
    clack_env: f32,
    clack_bp: Biquad,
    rumble: Brown,
    rumble_lp: Biquad,
    body_bp: Biquad,
    screech_phase: f32,
    screech_at: f32,
}

#[derive(Clone, Copy, Default)]
struct Horn {
    live: bool,
    t: f32,
    dur: f32,
    f: f32,
    ph: [f32; 2],
    pan: f32,
    level: f32,
    lp: Biquad,
}

pub struct CityLife {
    sr: f32,
    rng: Rng,
    n: u32,
    cars: [Car; 3],
    siren: Siren,
    train: Train,
    horn: Horn,
    hum_brown: [Brown; 2],
    hum_lp: [Biquad; 2],
    murmur: [Biquad; 2],
    neon_ph: f32,
    neon_crackle: f32,
    hum_w: Wander,
    // per-block
    life: f32,
    city: f32,
    car_rate: f32,
}

/// Approach/recede envelope for something passing at closest distance `d`
/// (fraction of the pass length), peaking half-way.
fn pass_env(k: f32, d: f32) -> (f32, f32) {
    let x = (k - 0.5) * 2.0;
    let g = d * d / (d * d + x * x);
    // radial velocity sign (approaching > 0) for Doppler
    let radial = -x / (d * d + x * x).sqrt();
    (g, radial)
}

impl CityLife {
    pub fn new(sr: f32) -> Self {
        let rate = sr / BLOCK as f32;
        Self {
            sr,
            rng: Rng(0x5151_7a7a),
            n: 0,
            cars: [Car::default(); 3],
            siren: Siren::default(),
            train: Train::default(),
            horn: Horn::default(),
            hum_brown: [Brown::default(); 2],
            hum_lp: [Biquad::lowpass(sr, 170.0, 0.7), Biquad::lowpass(sr, 160.0, 0.7)],
            murmur: [Biquad::bandpass(sr, 420.0, 0.8), Biquad::bandpass(sr, 390.0, 0.8)],
            neon_ph: 0.0,
            neon_crackle: 0.0,
            hum_w: Wander::new(rate, 12.0, 0.25),
            life: 0.0,
            city: 0.0,
            car_rate: 0.0,
        }
    }

    fn control(&mut self, p: &CityIn) {
        let dt = BLOCK as f32 / self.sr;
        self.life = p.life.clamp(0.0, 1.0);
        self.city = p.city.clamp(0.0, 1.0);
        self.hum_w.step(&mut self.rng);
        let sr = self.sr;
        let life = self.life;
        let city = self.city;
        if life <= 0.001 {
            return;
        }
        // mean gaps: city cars ~12 s at full life, street ~50 s
        self.car_rate = life * (city / 12.0 + (1.0 - city) / 50.0) * 1.6;
        if self.rng.f32() < self.car_rate * dt
            && let Some(c) = self.cars.iter_mut().find(|c| !c.live) {
                let speed = self.rng.range(8.0, 15.0);
                *c = Car {
                    live: true,
                    t: 0.0,
                    dur: 70.0 / speed * self.rng.range(0.8, 1.2),
                    dir: if self.rng.f32() < 0.5 { -1.0 } else { 1.0 },
                    level: self.rng.range(0.5, 1.0),
                    eng_f: self.rng.range(32.0, 55.0),
                    eng_phase: 0.0,
                    speed,
                    dist: self.rng.range(0.07, 0.16),
                    hiss: [Biquad::bandpass(sr, self.rng.range(900.0, 1500.0), 0.55), Biquad::bandpass(sr, self.rng.range(900.0, 1500.0), 0.55)],
                    spray: Biquad::highpass(sr, 3200.0, 0.7),
                    eng_lp: Biquad::lowpass(sr, 380.0, 0.7),
                };
            }
        if city > 0.5 {
            if !self.siren.live && self.rng.f32() < life / 160.0 * dt {
                let yelp = self.rng.f32() < 0.25;
                self.siren = Siren {
                    live: true,
                    t: 0.0,
                    dur: self.rng.range(14.0, 26.0),
                    phase: 0.0,
                    period: if yelp { 0.32 } else { self.rng.range(3.6, 4.8) },
                    yelp,
                    pan: self.rng.range(0.1, 0.9),
                    drift: self.rng.range(-0.02, 0.02),
                    level: self.rng.range(0.5, 1.0),
                    lp: [Biquad::lowpass(sr, 1700.0, 0.6), Biquad::lowpass(sr, 1500.0, 0.6)],
                };
            }
            if !self.train.live && self.rng.f32() < life / 240.0 * dt {
                let dur = self.rng.range(14.0, 22.0);
                self.train = Train {
                    live: true,
                    t: 0.0,
                    dur,
                    dir: if self.rng.f32() < 0.5 { -1.0 } else { 1.0 },
                    level: self.rng.range(0.6, 1.0),
                    clack_t: 0.0,
                    clack_period: self.rng.range(0.8, 1.1),
                    clack_env: 0.0,
                    clack_bp: Biquad::bandpass(sr, 1900.0, 1.4),
                    rumble: Brown::default(),
                    rumble_lp: Biquad::lowpass(sr, 85.0, 0.8),
                    body_bp: Biquad::bandpass(sr, 150.0, 1.5),
                    screech_phase: 0.0,
                    screech_at: if self.rng.f32() < 0.4 { self.rng.range(0.35, 0.65) } else { 2.0 },
                };
            }
            if !self.horn.live && self.rng.f32() < life / 75.0 * dt {
                self.horn = Horn {
                    live: true,
                    t: 0.0,
                    dur: self.rng.range(0.2, 0.6),
                    f: self.rng.range(340.0, 450.0),
                    ph: [0.0; 2],
                    pan: self.rng.range(0.1, 0.9),
                    level: self.rng.range(0.3, 1.0),
                    lp: Biquad::lowpass(sr, 2200.0, 0.6),
                };
            }
        }
    }

    #[inline]
    pub fn sample(&mut self, p: &CityIn) -> (f32, f32) {
        if self.n.is_multiple_of(BLOCK) {
            self.control(p);
        }
        self.n = self.n.wrapping_add(1);
        let life = self.life;
        if life <= 0.001 && !self.cars.iter().any(|c| c.live) {
            return (0.0, 0.0);
        }
        let dt = 1.0 / self.sr;
        let mut l = 0.0;
        let mut r = 0.0;
        let city = self.city;
        let wet = (p.mm_h / 4.0).clamp(0.3, 1.5);

        // the low hum of a city (and much less of it on a quiet street)
        let hum_g = life * (0.012 * city + 0.002) * (1.0 + self.hum_w.x).clamp(0.3, 2.0);
        if hum_g > 0.0 {
            for ch in 0..2 {
                let b = self.hum_brown[ch].process(self.rng.bipolar());
                let y = self.hum_lp[ch].process(b) * hum_g * 0.6 + self.murmur[ch].process(self.rng.bipolar()) * hum_g * 0.25;
                if ch == 0 {
                    l += y;
                } else {
                    r += y;
                }
            }
        }

        // neon: 120 Hz buzz with the odd crackle
        if city > 0.5 && life > 0.0 {
            self.neon_ph += TAU * 120.0 * dt;
            if self.neon_ph > TAU {
                self.neon_ph -= TAU;
            }
            if self.rng.f32() < 0.4 * dt {
                self.neon_crackle = 1.0;
            }
            let crack = if self.neon_crackle > 0.01 { self.rng.bipolar() * self.neon_crackle * 0.5 } else { 0.0 };
            self.neon_crackle *= 0.9993;
            let s = self.neon_ph.sin();
            let buzz = (s + 0.5 * (2.0 * self.neon_ph).sin() + 0.3 * (3.0 * self.neon_ph).sin()).clamp(-1.2, 1.2) + crack;
            let y = buzz * 0.0007 * life;
            l += y * 0.65;
            r += y * 0.35;
        }

        // cars hissing past
        for c in &mut self.cars {
            if !c.live {
                continue;
            }
            c.t += dt;
            let k = c.t / c.dur;
            if k >= 1.0 {
                c.live = false;
                continue;
            }
            let (g, radial) = pass_env(k, c.dist);
            let fade = (k * 12.0).min(1.0) * ((1.0 - k) * 12.0).min(1.0);
            let a = g * fade * c.level * 0.05;
            let pan = (0.5 + 0.45 * c.dir * (k - 0.5) * 2.0).clamp(0.02, 0.98);
            let doppler = 1.0 + radial * c.speed / 343.0;
            c.eng_phase += TAU * c.eng_f * doppler * dt;
            if c.eng_phase > TAU {
                c.eng_phase -= TAU;
            }
            let eng = c.eng_lp.process((c.eng_phase.sin() + 0.5 * (2.0 * c.eng_phase).sin() + 0.3 * (3.0 * c.eng_phase).sin()) * 0.5 + self.rng.bipolar() * 0.2);
            let n = self.rng.bipolar();
            let hl = c.hiss[0].process(n * 0.8 + self.rng.bipolar() * 0.4);
            let hr = c.hiss[1].process(n * 0.8 + self.rng.bipolar() * 0.4);
            let spray = c.spray.process(self.rng.bipolar()) * 0.35 * wet;
            let yl = (hl * wet + spray) * a + eng * a * 0.4 * g;
            let yr = (hr * wet + spray) * a + eng * a * 0.4 * g;
            l += yl * (1.0 - pan) * 1.4;
            r += yr * pan * 1.4;
        }

        // a siren somewhere far off
        if self.siren.live {
            let s = &mut self.siren;
            s.t += dt;
            if s.t >= s.dur {
                s.live = false;
            } else {
                let env = (s.t / 3.0).min(1.0) * ((s.dur - s.t) / 4.0).min(1.0);
                let ph = (s.t / s.period) * TAU;
                let sweep = 0.5 - 0.5 * ph.cos();
                let f = if s.yelp { 900.0 + 500.0 * sweep } else { 680.0 + 560.0 * sweep };
                s.phase += TAU * f * dt;
                if s.phase > TAU {
                    s.phase -= TAU;
                }
                let x = s.phase.sin() + 0.35 * (2.0 * s.phase).sin() + 0.15 * (3.0 * s.phase).sin();
                s.pan = (s.pan + s.drift * dt).clamp(0.05, 0.95);
                let a = env * s.level * 0.006 * life;
                l += s.lp[0].process(x) * a * (1.0 - s.pan);
                r += s.lp[1].process(x) * a * s.pan;
            }
        }

        // the el-train: rumble, the clack of the joints, maybe a screech
        if self.train.live {
            let t = &mut self.train;
            t.t += dt;
            let k = t.t / t.dur;
            if k >= 1.0 {
                t.live = false;
            } else {
                let (g, _) = pass_env(k, 0.22);
                let fade = (k * 8.0).min(1.0) * ((1.0 - k) * 8.0).min(1.0);
                let a = g * fade * t.level * life;
                t.clack_t += dt;
                if t.clack_t >= t.clack_period {
                    t.clack_t -= t.clack_period;
                }
                // two bogies over each joint
                let c = t.clack_t;
                if (c < dt) || ((c - 0.13).abs() < dt * 0.5) {
                    t.clack_env = 1.0;
                }
                let clack = t.clack_bp.process(self.rng.bipolar() * t.clack_env) * 0.6;
                t.clack_env *= 0.997;
                let rum = t.rumble.process(self.rng.bipolar());
                let body = t.rumble_lp.process(rum) * 0.9 + t.body_bp.process(rum) * 0.5;
                let mut y = (body * 0.045 + clack * 0.02) * a;
                if (k - t.screech_at).abs() < 0.12 {
                    let e = 1.0 - ((k - t.screech_at).abs() / 0.12);
                    t.screech_phase += TAU * (3400.0 + 120.0 * (t.t * 31.0).sin()) * dt;
                    if t.screech_phase > TAU {
                        t.screech_phase -= TAU;
                    }
                    y += t.screech_phase.sin() * e * e * 0.0025 * a;
                }
                let pan = (0.5 + 0.4 * t.dir * (k - 0.5) * 2.0).clamp(0.05, 0.95);
                l += y * (1.0 - pan) * 1.4;
                r += y * pan * 1.4;
            }
        }

        // a horn, briefly
        if self.horn.live {
            let h = &mut self.horn;
            h.t += dt;
            if h.t >= h.dur {
                h.live = false;
            } else {
                let env = (h.t / 0.02).min(1.0) * ((h.dur - h.t) / 0.04).min(1.0);
                h.ph[0] += TAU * h.f * dt;
                h.ph[1] += TAU * h.f * 1.26 * dt;
                for p in &mut h.ph {
                    if *p > TAU {
                        *p -= TAU;
                    }
                }
                let x = (h.ph[0].sin() * 2.5).clamp(-1.0, 1.0) + (h.ph[1].sin() * 2.5).clamp(-1.0, 1.0);
                let y = h.lp.process(x) * env * h.level * 0.0025 * life;
                l += y * (1.0 - h.pan);
                r += y * h.pan;
            }
        }
        (l, r)
    }
}
