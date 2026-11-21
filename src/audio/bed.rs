//! The statistical rain bed: everything too far away (or too small) to voice
//! individually, generated from the same physical parameters as the
//! simulation. Built from several layers so it has weight and keeps moving:
//!
//! * wash  — pink noise shaped like a wall of distant impacts
//! * body  — the low roar of a whole street (or city) being rained on
//! * patter — thousands of distant taps on different materials, in clumps
//! * awnings, metal pings, puddle plinks, a gurgling drainpipe, wind
//!
//! Slow random walks and gust swells keep it from ever sounding looped.

use super::dsp::*;

const BLOCK: u32 = 64;

/// Parameters for the bed (set once per audio callback).
#[derive(Clone, Copy, Default)]
pub struct BedIn {
    pub mm_h: f32,
    /// wind speed (m/s)
    pub wind: f32,
    pub puddle: f32,
    /// 0 = residential street, 1 = neon city
    pub city: f32,
}

#[derive(Clone, Copy, Default)]
struct MiniBubble {
    amp: f32,
    phase: f32,
    freq: f32,
    rise: f32,
    decay: f32,
    pan: f32,
}

/// A far-off drop hitting something metal: two inharmonic modes.
#[derive(Clone, Copy, Default)]
struct Ping {
    m: [Mode; 2],
    pan: f32,
    live: bool,
    t: u32,
}

pub struct Bed {
    sr: f32,
    rng: Rng,
    // wash
    pink: [Pink; 2],
    wash_hp: [Biquad; 2],
    wash_pk: [Biquad; 2],
    wash_lp: [Biquad; 2],
    // body
    brown: [Brown; 2],
    body_lp: [Biquad; 2],
    body_lp2: [Biquad; 2],
    rumble_lp: [Biquad; 2],
    // patter banks: hard, mid, low thud, soft (leaves)
    banks: [[Biquad; 4]; 2],
    tap_air: [OnePole; 2],
    // awnings across the street (city)
    awn: [Biquad; 3],
    awn_pan: [f32; 3],
    pings: [Ping; 10],
    bubbles: [MiniBubble; 24],
    // drainpipe
    pipe_bp: Biquad,
    pipe_bp2: Biquad,
    pipe_hp: Biquad,
    pipe_brown: Brown,
    pipe_am: Wander,
    pipe_bubbles: [MiniBubble; 6],
    feed: f32,
    // wind
    wind_lp: [Biquad; 2],
    whistle: Biquad,
    wind_env: OnePole,
    wind_slow: f32,
    wind_fast: f32,
    // evolution
    swell_w: Wander,
    bright_w: Wander,
    dens_w: Wander,
    t: f32,
    n: u32,
    // per-block derived values
    rate: f32,
    dens: f32,
    swell: f32,
    wash_g: f32,
    body_g: f32,
    rumble_g: f32,
    tap_g: f32,
    bank_w: [f32; 4],
    awn_rate: f32,
    awn_g: f32,
    ping_rate: f32,
    plink_rate: f32,
    pipe_g: f32,
    wind_g: f32,
    whistle_g: f32,
    gust_dev: f32,
    city: f32,
}

impl Bed {
    pub fn new(sr: f32) -> Self {
        let rate = sr / BLOCK as f32;
        Self {
            sr,
            rng: Rng(0x1234_5677),
            pink: [Pink::default(); 2],
            wash_hp: [Biquad::highpass(sr, 140.0, 0.6); 2],
            wash_pk: [Biquad::peaking(sr, 2600.0, 0.7, 4.0); 2],
            wash_lp: [Biquad::lowpass(sr, 9500.0, 0.6); 2],
            brown: [Brown::default(); 2],
            body_lp: [Biquad::lowpass(sr, 340.0, 0.6); 2],
            body_lp2: [Biquad::lowpass(sr, 600.0, 0.6); 2],
            rumble_lp: [Biquad::lowpass(sr, 95.0, 0.7); 2],
            banks: [
                [Biquad::bandpass(sr, 3600.0, 0.8), Biquad::bandpass(sr, 1500.0, 0.9), Biquad::bandpass(sr, 520.0, 1.0), Biquad::bandpass(sr, 950.0, 0.5)],
                [Biquad::bandpass(sr, 3300.0, 0.8), Biquad::bandpass(sr, 1380.0, 0.9), Biquad::bandpass(sr, 480.0, 1.0), Biquad::bandpass(sr, 900.0, 0.5)],
            ],
            tap_air: [OnePole::new(sr, 7500.0), OnePole::new(sr, 7000.0)],
            awn: [Biquad::bandpass(sr, 172.0, 7.0), Biquad::bandpass(sr, 238.0, 7.0), Biquad::bandpass(sr, 327.0, 6.0)],
            awn_pan: [0.2, 0.35, 0.8],
            pings: [Ping::default(); 10],
            bubbles: [MiniBubble::default(); 24],
            pipe_bp: Biquad::bandpass(sr, 290.0, 2.5),
            pipe_bp2: Biquad::bandpass(sr, 610.0, 3.0),
            pipe_hp: Biquad::highpass(sr, 2200.0, 0.7),
            pipe_brown: Brown::default(),
            pipe_am: Wander::new(rate, 1.2, 0.45),
            pipe_bubbles: [MiniBubble::default(); 6],
            feed: 0.0,
            wind_lp: [Biquad::lowpass(sr, 260.0, 0.9), Biquad::lowpass(sr, 240.0, 0.9)],
            whistle: Biquad::bandpass(sr, 700.0, 3.0),
            wind_env: OnePole::new(sr, 0.4),
            wind_slow: 0.0,
            wind_fast: 0.0,
            swell_w: Wander::new(rate, 9.0, 0.10),
            bright_w: Wander::new(rate, 25.0, 0.12),
            dens_w: Wander::new(rate, 3.5, 0.28),
            t: 0.0,
            n: 0,
            rate: 0.0,
            dens: 1.0,
            swell: 1.0,
            wash_g: 0.0,
            body_g: 0.0,
            rumble_g: 0.0,
            tap_g: 0.0,
            bank_w: [0.35, 0.3, 0.15, 0.2],
            awn_rate: 0.0,
            awn_g: 0.0,
            ping_rate: 0.0,
            plink_rate: 0.0,
            pipe_g: 0.0,
            wind_g: 0.0,
            whistle_g: 0.0,
            gust_dev: 0.0,
            city: 0.0,
        }
    }

    /// Control-rate update: the slow evolution and everything derived from it.
    fn control(&mut self, p: &BedIn) {
        let dt = BLOCK as f32 / self.sr;
        self.t += dt;
        let t = self.t;
        let mm = p.mm_h.max(0.0);
        let city = p.city.clamp(0.0, 1.0);
        self.city = city;

        // gusts: fast wind relative to its slow average
        self.wind_fast += (p.wind - self.wind_fast) * (dt / 0.5).min(1.0);
        self.wind_slow += (p.wind - self.wind_slow) * (dt / 20.0).min(1.0);
        self.gust_dev = ((self.wind_fast - self.wind_slow) / self.wind_slow.max(1.5)).clamp(-0.5, 1.0);

        let sw = self.swell_w.step(&mut self.rng);
        let br = self.bright_w.step(&mut self.rng);
        let de = self.dens_w.step(&mut self.rng);
        use std::f32::consts::TAU;
        self.swell = (1.0 + 0.10 * (TAU * 0.029 * t).sin() + 0.07 * (TAU * 0.011 * t + 1.3).sin() + sw + 0.35 * self.gust_dev).clamp(0.55, 1.7);
        let bright = (br + 0.18 * self.gust_dev).clamp(-0.35, 0.45);
        self.dens = (1.0 + de).clamp(0.35, 1.9);

        // drains fill and empty slowly
        let tau = if mm > self.feed { 30.0 } else { 120.0 };
        self.feed += (mm - self.feed) * (dt / tau).min(1.0);

        let sr = self.sr;
        if self.n.is_multiple_of(8) {
            for ch in 0..2 {
                let s = 1.0 - 0.06 * ch as f32;
                self.wash_pk[ch].set_peaking(sr, 2500.0 * (1.0 + bright) * s, 0.7, 3.5 + 2.0 * city);
                self.wash_lp[ch].set_lowpass(sr, 9000.0 * (1.0 + 0.5 * bright) * s, 0.6);
            }
            self.whistle.set_bandpass(sr, 480.0 + 700.0 * self.gust_dev.max(0.0) + 120.0 * (TAU * 0.07 * t).sin(), 3.0);
        }

        let r = mm.powf(0.5);
        self.wash_g = 0.055 * r * self.swell * (1.0 + 0.25 * city);
        self.body_g = 0.05 * mm.powf(0.8).min(45.0) * (0.75 + 0.45 * city) * self.swell;
        self.rumble_g = 0.04 * mm.powf(0.85).min(50.0) * (0.6 + 0.6 * city) * self.swell;
        self.rate = ((210.0 + 70.0 * city) * mm.powf(0.7)).min(26_000.0);
        self.tap_g = 0.9 * (1.0 + 0.3 * (self.swell - 1.0));
        let pud = p.puddle.clamp(0.0, 1.0);
        self.bank_w = [
            (0.34 + 0.12 * city) * (1.0 - 0.5 * pud),
            0.26 + 0.05 * city + 0.6 * pud,
            0.15 + 0.08 * city,
            0.25 * (1.0 - city),
        ];
        self.awn_rate = 45.0 * mm.powf(0.7) * city;
        self.awn_g = 0.5 * city;
        self.ping_rate = 5.0 * mm.powf(0.6) * (0.3 + 0.7 * city);
        self.plink_rate = self.rate * pud * 0.012;
        self.pipe_g = 0.012 * (self.feed / 6.0).max(0.0).powf(0.8).min(2.5);
        self.wind_g = 0.0035 * (p.wind / 6.0).powi(2).min(6.0) * (1.0 + 0.5 * self.gust_dev);
        self.whistle_g = 0.0012 * ((p.wind - 7.0).max(0.0) / 6.0).powi(2).min(4.0) * (0.4 + self.gust_dev.max(0.0));
    }

    #[inline]
    pub fn sample(&mut self, p: &BedIn) -> (f32, f32) {
        if self.n.is_multiple_of(BLOCK) {
            self.control(p);
        }
        self.n = self.n.wrapping_add(1);
        let sr = self.sr;
        let mut out = [0.0f32; 2];
        let mid = self.rng.bipolar();
        let p_tap = (self.rate * self.dens / sr).min(0.6);
        let bw = self.bank_w;
        for (ch, o) in out.iter_mut().enumerate() {
            // wash
            let w = self.rng.bipolar() * 0.8 + mid * 0.45;
            let wash = self.wash_lp[ch].process(self.wash_pk[ch].process(self.wash_hp[ch].process(self.pink[ch].process(w)))) * self.wash_g;
            // body + rumble
            let b = self.brown[ch].process(self.rng.bipolar() * 0.7 + mid * 0.5);
            let body = self.body_lp2[ch].process(self.body_lp[ch].process(b)) * self.body_g * 0.05;
            let rumble = self.rumble_lp[ch].process(b) * self.rumble_g * 0.05;
            // patter: sparse impulses, each ringing one material's resonance
            let mut taps = 0.0;
            if self.rng.f32() < p_tap {
                let a = self.rng.f32();
                let x = a * a * a * if self.rng.f32() < 0.5 { 1.0 } else { -1.0 };
                let mut pick = self.rng.f32() * (bw[0] + bw[1] + bw[2] + bw[3]);
                let mut k = 0;
                while k < 3 && pick > bw[k] {
                    pick -= bw[k];
                    k += 1;
                }
                taps = self.banks[ch][k].process(x * [1.0, 1.0, 1.1, 0.7][k]);
                for j in 0..4 {
                    if j != k {
                        taps += self.banks[ch][j].process(0.0);
                    }
                }
            } else {
                for j in 0..4 {
                    taps += self.banks[ch][j].process(0.0);
                }
            }
            *o = self.tap_air[ch].process(taps * self.tap_g) + wash + body + rumble;
        }

        // awnings drumming
        if self.awn_g > 0.0 && self.rng.f32() < self.awn_rate / sr {
            let k = (self.rng.f32() * 3.0) as usize;
            let a = self.rng.f32();
            let x = a * a * self.awn_g;
            let y = self.awn[k.min(2)].process(x);
            out[0] += y * (1.0 - self.awn_pan[k.min(2)]);
            out[1] += y * self.awn_pan[k.min(2)];
            for j in 0..3 {
                if j != k.min(2) {
                    let y = self.awn[j].process(0.0);
                    out[0] += y * (1.0 - self.awn_pan[j]);
                    out[1] += y * self.awn_pan[j];
                }
            }
        } else {
            for j in 0..3 {
                let y = self.awn[j].process(0.0);
                out[0] += y * (1.0 - self.awn_pan[j]);
                out[1] += y * self.awn_pan[j];
            }
        }

        // far metal pings (fire escapes, AC boxes, railings)
        if self.rng.f32() < self.ping_rate / sr
            && let Some(pg) = self.pings.iter_mut().find(|p| !p.live) {
                let f1 = self.rng.range(1500.0, 4800.0);
                let f2 = f1 * self.rng.range(2.2, 2.9);
                let tau = self.rng.range(0.04, 0.16);
                let amp = self.rng.range(0.15, 0.6) * self.rng.f32();
                pg.m = [Mode::new(sr, f1, tau, amp), Mode::new(sr, f2, tau * 0.6, amp * 0.5)];
                pg.pan = self.rng.f32();
                pg.live = true;
                pg.t = 0;
            }
        for pg in &mut self.pings {
            if pg.live {
                let x = if pg.t == 0 { 0.02 } else { 0.0 };
                pg.t += 1;
                let y = pg.m[0].process(x) + pg.m[1].process(x);
                out[0] += y * (1.0 - pg.pan);
                out[1] += y * pg.pan;
                if pg.t > 64 && pg.m[0].energy() < 1e-6 {
                    pg.live = false;
                }
            }
        }

        // distant puddle plinks
        let tw = std::f32::consts::TAU / sr;
        if self.rng.f32() < self.plink_rate / sr
            && let Some(b) = self.bubbles.iter_mut().find(|b| b.amp < 1e-4) {
                let f0 = self.rng.range(1300.0, 5200.0);
                let d = 0.043 * f0 + 0.0014 * f0.powf(1.5);
                *b = MiniBubble { amp: self.rng.range(0.004, 0.02), phase: 0.0, freq: f0, rise: f0 * 0.3 * d / sr, decay: (-d / sr).exp(), pan: self.rng.f32() };
            }
        for b in &mut self.bubbles {
            if b.amp > 1e-4 {
                b.phase += b.freq * tw;
                b.freq += b.rise;
                let s = fast_sin(b.phase) * b.amp;
                out[0] += s * (1.0 - b.pan);
                out[1] += s * b.pan;
                b.amp *= b.decay;
            }
        }

        // the drainpipe: a resonant tube of running water with the odd glug
        if self.pipe_g > 1e-5 {
            if self.n % BLOCK == 1 {
                self.pipe_am.step(&mut self.rng);
            }
            let am = (1.0 + self.pipe_am.x).clamp(0.2, 2.2);
            let flow = self.pipe_brown.process(self.rng.bipolar());
            let tube = self.pipe_bp.process(flow) * 1.6 + self.pipe_bp2.process(flow) * 0.8;
            let trickle = self.pipe_hp.process(self.rng.bipolar()) * 0.05;
            let mut y = (tube + trickle) * self.pipe_g * am;
            if self.rng.f32() < 5.0 * (self.feed / 6.0).min(3.0) / sr
                && let Some(b) = self.pipe_bubbles.iter_mut().find(|b| b.amp < 1e-4) {
                    let f0 = self.rng.range(260.0, 900.0);
                    let d = 0.043 * f0 + 0.0014 * f0.powf(1.5);
                    *b = MiniBubble { amp: self.rng.range(0.2, 0.7), phase: 0.0, freq: f0, rise: f0 * 0.5 * d / sr, decay: (-d / sr).exp(), pan: 0.0 };
                }
            for b in &mut self.pipe_bubbles {
                if b.amp > 1e-4 {
                    b.phase += b.freq * tw;
                    b.freq += b.rise;
                    y += fast_sin(b.phase) * b.amp * self.pipe_g;
                    b.amp *= b.decay;
                }
            }
            // street: the house downpipe on the right; city: ours, on the left
            let pan = 0.78 - 0.5 * self.city;
            out[0] += y * (1.0 - pan);
            out[1] += y * pan;
        }

        // wind: low rumble swelling with gusts, a whistle when it's strong
        let env = self.wind_env.process(self.rng.f32());
        let wn = self.rng.bipolar();
        let wl = self.wind_lp[0].process(wn) * self.wind_g * (0.6 + env);
        let wr = self.wind_lp[1].process(self.rng.bipolar() * 0.5 + wn * 0.5) * self.wind_g * (0.6 + env);
        let wh = self.whistle.process(self.rng.bipolar()) * self.whistle_g;
        (out[0] + wl + wh * 0.8, out[1] + wr + wh)
    }
}
