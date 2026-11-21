//! Sounds of the room itself: the radiator ticking and hissing, the low tone
//! of a quiet room at night, and the old sash window sliding in its frame.

use super::dsp::*;

/// A metallic tick from the radiator warming and cooling.
#[derive(Clone, Copy, Default)]
struct Tick {
    m: [Mode; 2],
    live: bool,
    t: u32,
    exc: f32,
}

pub struct RoomSounds {
    sr: f32,
    rng: Rng,
    ticks: [Tick; 4],
    burst_left: u32,
    next_tick: f32,
    next_burst: f32,
    hiss_hp: Biquad,
    hiss_lp: Biquad,
    hiss_w: Wander,
    hiss_on: f32,
    clunk: [Mode; 2],
    next_clunk: f32,
    tone_brown: Brown,
    tone_lp: Biquad,
    n: u32,
    // sash slide
    slide_t: f32,
    slide_dur: f32,
    slide_open: bool,
    slide_live: bool,
    slide_bp: Biquad,
    slide_bp2: Biquad,
    slide_jitter: f32,
    thump: [Mode; 2],
    rattle: [Mode; 1],
    air_hp: Biquad,
}

impl RoomSounds {
    pub fn new(sr: f32) -> Self {
        let rate = sr / 64.0;
        Self {
            sr,
            rng: Rng(0x7a9c_3141),
            ticks: [Tick::default(); 4],
            burst_left: 0,
            next_tick: 0.0,
            next_burst: 4.0,
            hiss_hp: Biquad::highpass(sr, 2600.0, 0.7),
            hiss_lp: Biquad::lowpass(sr, 9000.0, 0.7),
            hiss_w: Wander::new(rate, 6.0, 0.4),
            hiss_on: 0.0,
            clunk: [Mode::default(); 2],
            next_clunk: 90.0,
            tone_brown: Brown::default(),
            tone_lp: Biquad::lowpass(sr, 110.0, 0.7),
            n: 0,
            slide_t: 0.0,
            slide_dur: 1.0,
            slide_open: true,
            slide_live: false,
            slide_bp: Biquad::bandpass(sr, 720.0, 1.3),
            slide_bp2: Biquad::bandpass(sr, 1900.0, 2.0),
            slide_jitter: 0.0,
            thump: [Mode::default(); 2],
            rattle: [Mode::default(); 1],
            air_hp: Biquad::highpass(sr, 500.0, 0.7),
        }
    }

    pub fn slide(&mut self, opening: bool, seconds: f32) {
        self.slide_live = true;
        self.slide_t = 0.0;
        self.slide_dur = seconds.max(0.2);
        self.slide_open = opening;
        // the first nudge: the sash is stuck for a moment, then gives
        let sr = self.sr;
        self.thump = [Mode::new(sr, 140.0, 0.03, 0.7), Mode::new(sr, 420.0, 0.02, 0.4)];
        let x = 0.25;
        self.thump[0].process(x);
        self.thump[1].process(x);
    }

    /// Mono room-side sound (radiator, room tone, sash). Returns (l, r).
    #[inline]
    pub fn sample(&mut self, radiator: f32) -> (f32, f32) {
        let sr = self.sr;
        let dt = 1.0 / sr;
        self.n = self.n.wrapping_add(1);
        let mut l = 0.0;
        let mut r = 0.0;

        if radiator > 0.0 {
            // ticks come in little bursts
            self.next_burst -= dt;
            if self.next_burst <= 0.0 {
                self.next_burst = self.rng.range(6.0, 22.0);
                self.burst_left = 1 + (self.rng.f32() * 5.0) as u32;
                self.next_tick = 0.0;
            }
            if self.burst_left > 0 {
                self.next_tick -= dt;
                if self.next_tick <= 0.0 {
                    self.burst_left -= 1;
                    self.next_tick = self.rng.range(0.15, 1.6);
                    if let Some(t) = self.ticks.iter_mut().find(|t| !t.live) {
                        let f = self.rng.range(2300.0, 3800.0);
                        let tau = self.rng.range(0.012, 0.04);
                        let amp = self.rng.range(0.3, 1.0);
                        *t = Tick {
                            m: [Mode::new(sr, f, tau, amp), Mode::new(sr, self.rng.range(950.0, 1300.0), tau * 1.4, amp * 0.6)],
                            live: true,
                            t: 0,
                            exc: 0.05,
                        };
                    }
                }
            }
            for t in &mut self.ticks {
                if t.live {
                    let x = if t.t < 3 { t.exc * [1.0, -0.7, 0.3][t.t as usize] } else { 0.0 };
                    t.t += 1;
                    let y = (t.m[0].process(x) + t.m[1].process(x)) * radiator;
                    // the radiator is just below and a little left of the window
                    l += y * 0.6;
                    r += y * 0.4;
                    if t.t > 32 && t.m[0].energy() < 1e-6 && t.m[1].energy() < 1e-6 {
                        t.live = false;
                    }
                }
            }
            // a faint valve hiss that comes and goes
            if self.n.is_multiple_of(64) {
                self.hiss_w.step(&mut self.rng);
                let target = if self.hiss_w.x > 0.25 { 1.0 } else { 0.0 };
                self.hiss_on += (target - self.hiss_on) * 0.01;
            }
            if self.hiss_on > 0.001 {
                let h = self.hiss_lp.process(self.hiss_hp.process(self.rng.bipolar())) * 0.0011 * self.hiss_on * radiator;
                l += h * 0.6;
                r += h * 0.4;
            }
            // and very rarely, a clunk deep in the pipes
            self.next_clunk -= dt;
            if self.next_clunk <= 0.0 {
                self.next_clunk = self.rng.range(120.0, 400.0);
                self.clunk = [Mode::new(sr, self.rng.range(80.0, 110.0), 0.12, 1.0), Mode::new(sr, self.rng.range(380.0, 470.0), 0.05, 0.5)];
                self.clunk[0].process(0.05);
                self.clunk[1].process(0.05);
            }
            let c = (self.clunk[0].process(0.0) + self.clunk[1].process(0.0)) * radiator;
            l += c * 0.55;
            r += c * 0.45;
        }

        // room tone: the faint low air of a room at night
        let tone = self.tone_lp.process(self.tone_brown.process(self.rng.bipolar())) * 0.0015;
        l += tone;
        r += tone;

        // the sash sliding: wood on wood, stick-slip, then a soft thump
        if self.slide_live {
            self.slide_t += dt;
            let k = self.slide_t / self.slide_dur;
            if k >= 1.0 {
                self.slide_live = false;
                let g = if self.slide_open { 0.35 } else { 1.0 };
                self.thump = [Mode::new(sr, 120.0, 0.05, g), Mode::new(sr, 330.0, 0.03, g * 0.6)];
                self.rattle = [Mode::new(sr, 760.0, 0.08, 0.3)];
                let x = 0.4;
                self.thump[0].process(x);
                self.thump[1].process(x);
                self.rattle[0].process(x * 0.3);
            } else {
                // stuck for the first moment, then sliding
                let moving = if k < 0.14 { 0.25 } else { (1.0 - ((k - 0.6) / 0.4).max(0.0)).max(0.2) };
                if self.rng.f32() < 55.0 * dt {
                    self.slide_jitter = self.rng.range(0.4, 1.0);
                }
                self.slide_jitter *= 0.9985;
                let n = self.rng.bipolar();
                let rub = self.slide_bp.process(n) * 0.9 + self.slide_bp2.process(n) * 0.25;
                let air = self.air_hp.process(self.rng.bipolar()) * if self.slide_open { k * 0.004 } else { 0.0 };
                let y = rub * moving * (0.25 + self.slide_jitter) * 0.03 + air;
                l += y * 0.5;
                r += y * 0.5;
            }
        }
        let th = self.thump[0].process(0.0) + self.thump[1].process(0.0) + self.rattle[0].process(0.0);
        l += th * 0.5;
        r += th * 0.5;
        (l, r)
    }
}
