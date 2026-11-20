//! The real-time rain synthesiser. Runs on the audio thread: no allocation,
//! no locks. Impacts arrive through a lock-free ring buffer from the
//! simulation; everything else (the distant rain bed) is driven by the same
//! physical parameters the simulation uses.

use super::dsp::*;
use super::{AudioShared, Impact, SURF_GARDEN, SURF_PAVE, SURF_PUDDLE, SURF_ROAD, SURF_ROOF};
use std::sync::Arc;
use std::sync::atomic::Ordering;

const MAX_VOICES: usize = 224;
const HIST: usize = 64;

#[derive(Clone, Copy)]
struct Micro {
    at: u32,
    amp: f32,
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    t: u32,
    delay: u32,
    life: u32,
    gl: f32,
    gr: f32,
    itd: i32,
    shadow_l: f32,
    shadow_r: f32,
    lp_l: f32,
    lp_r: f32,
    hist: [f32; HIST],
    hpos: usize,
    rng: Rng,
    // impact transient
    tick_amp: f32,
    tick_env: f32,
    tick_decay: f32,
    bq: Biquad,
    // bubble (Minnaert resonance with rising pitch)
    b_amp: f32,
    b_start: u32,
    b_phase: f32,
    b_freq: f32,
    b_rise: f32,
    b_decay: f32,
    // spray droplets landing just after the impact
    micro: [Micro; 3],
    micro_env: f32,
    level: f32,
    /// 2π / sample rate
    tw: f32,
}

impl Default for Voice {
    fn default() -> Self {
        Self {
            active: false,
            t: 0,
            delay: 0,
            life: 0,
            gl: 0.0,
            gr: 0.0,
            itd: 0,
            shadow_l: 1.0,
            shadow_r: 1.0,
            lp_l: 0.0,
            lp_r: 0.0,
            hist: [0.0; HIST],
            hpos: 0,
            rng: Rng(1),
            tick_amp: 0.0,
            tick_env: 0.0,
            tick_decay: 0.0,
            bq: Biquad::default(),
            b_amp: 0.0,
            b_start: 0,
            b_phase: 0.0,
            b_freq: 0.0,
            b_rise: 0.0,
            b_decay: 0.0,
            micro: [Micro { at: u32::MAX, amp: 0.0 }; 3],
            micro_env: 0.0,
            level: 0.0,
            tw: std::f32::consts::TAU / 48_000.0,
        }
    }
}

impl Voice {
    fn start(&mut self, imp: &Impact, sr: f32, seed: u32) {
        let mut rng = Rng(seed | 1);
        let size = imp.size_mm.clamp(0.3, 6.0);
        let energy = (size / 2.0).powf(1.6) * (imp.speed / 7.0).clamp(0.2, 1.5).powf(1.5);
        let base = 0.32 * energy;
        self.active = true;
        self.t = 0;
        self.delay = imp.delay;
        self.gl = imp.gain_l;
        self.gr = imp.gain_r;
        self.itd = imp.itd.clamp(-(HIST as i32 - 2), HIST as i32 - 2);
        self.shadow_l = imp.shadow_l;
        self.shadow_r = imp.shadow_r;
        self.lp_l = 0.0;
        self.lp_r = 0.0;
        self.hist = [0.0; HIST];
        self.hpos = 0;
        self.b_amp = 0.0;
        self.micro = [Micro { at: u32::MAX, amp: 0.0 }; 3];
        self.micro_env = 0.0;
        self.tw = std::f32::consts::TAU / sr;

        // surface-specific impact character
        let size_f = (2.0 / size).sqrt();
        let (fc, q, tau_ms, amp, bubble_p, micro_n) = match imp.surface {
            SURF_PUDDLE => (1500.0 * size_f * rng.range(0.8, 1.25), 1.1, 2.2 + 0.6 * size, 0.55, 0.55, 1),
            SURF_ROAD => (2700.0 * size_f * rng.range(0.8, 1.25), 0.85, 1.3 + 0.4 * size, 0.8, 0.06, 2),
            SURF_PAVE => (3300.0 * size_f * rng.range(0.8, 1.25), 0.9, 1.1 + 0.35 * size, 0.85, 0.04, 2),
            SURF_ROOF => (2100.0 * rng.range(0.85, 1.2), 3.2, 6.0, 0.7, 0.0, 1),
            SURF_GARDEN => (850.0 * rng.range(0.8, 1.2), 0.6, 3.0, 0.3, 0.0, 0),
            _ => (2500.0, 0.9, 1.5, 0.6, 0.0, 1),
        };
        self.bq = Biquad::bandpass(sr, fc, q);
        self.tick_amp = base * amp;
        self.tick_env = 1.0;
        self.tick_decay = (-1.0 / (tau_ms * 0.001 * sr)).exp();

        // entrained air bubble: f0 = 3.26 / R (Minnaert), damping after van den Doel
        if size > 0.7 && rng.f32() < bubble_p {
            let r_mm = (size * rng.range(0.3, 0.8)).clamp(0.25, 3.5);
            let f0 = 3.26 / (r_mm * 0.001);
            let d = 0.043 * f0 + 0.0014 * f0.powf(1.5);
            self.b_freq = f0;
            self.b_rise = f0 * 0.35 * d / sr; // Hz per sample: f(t) = f0·(1 + ξ·d·t), ξ = 0.35
            self.b_decay = (-d / sr).exp();
            self.b_amp = base * 0.9 * (r_mm / 1.0).powf(0.6).min(2.0);
            self.b_start = (rng.range(0.5, 3.0) * 0.001 * sr) as u32;
            self.b_phase = 0.0;
        }
        for k in 0..micro_n.min(3) {
            if rng.f32() < 0.7 {
                self.micro[k] = Micro { at: (rng.range(6.0, 45.0) * 0.001 * sr) as u32, amp: base * rng.range(0.08, 0.22) };
            }
        }
        self.life = ((tau_ms * 0.001 * 9.0 + 0.06) * sr) as u32 + self.b_start + (0.03 * sr) as u32;
        self.rng = rng;
        self.level = base * (imp.gain_l + imp.gain_r);
    }

    #[inline]
    fn sample(&mut self) -> (f32, f32) {
        if self.delay > 0 {
            self.delay -= 1;
            return (0.0, 0.0);
        }
        let t = self.t;
        self.t += 1;
        let mut x = 0.0;
        // transient: shaped noise through the surface resonance (+ a tiny click)
        if self.tick_env > 1e-4 {
            let n = self.rng.bipolar();
            x += self.bq.process(n * self.tick_env) * self.tick_amp * 2.5;
            if t < 2 {
                x += self.tick_amp * 0.35 * if t == 0 { 1.0 } else { -0.6 };
            }
            self.tick_env *= self.tick_decay;
        }
        // bubble
        if self.b_amp > 1e-5 && t >= self.b_start {
            self.b_phase += self.b_freq * self.tw;
            x += fast_sin(self.b_phase) * self.b_amp;
            self.b_amp *= self.b_decay;
            self.b_freq += self.b_rise;
        }
        // spray micro-ticks
        for m in &mut self.micro {
            if t == m.at {
                self.micro_env = m.amp;
            }
        }
        if self.micro_env > 1e-5 {
            x += self.rng.bipolar() * self.micro_env;
            self.micro_env *= 0.82;
        }

        // interaural delay
        self.hist[self.hpos] = x;
        let delayed = self.hist[(self.hpos + HIST - self.itd.unsigned_abs() as usize) % HIST];
        self.hpos = (self.hpos + 1) % HIST;
        let (l_in, r_in) = if self.itd >= 0 { (x, delayed) } else { (delayed, x) };
        // head shadow on the far ear
        self.lp_l += self.shadow_l * (l_in - self.lp_l);
        self.lp_r += self.shadow_r * (r_in - self.lp_r);

        if t > self.life {
            self.active = false;
        }
        (self.lp_l * self.gl, self.lp_r * self.gr)
    }
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

/// Statistical rain bed: everything too far away (or too small) to voice
/// individually, generated from the rain rate and surfaces around the listener.
struct Bed {
    rng: Rng,
    hi: [Biquad; 2],
    mid: [Biquad; 2],
    low: [Biquad; 2],
    hiss_hp: [Biquad; 2],
    hiss_lp: [Biquad; 2],
    roar: [Biquad; 2],
    wind_lp: Biquad,
    wind_env: OnePole,
    air: [OnePole; 2],
    bubbles: [MiniBubble; 24],
    rate: Smoothed,
    hiss: Smoothed,
    roar_lvl: Smoothed,
    puddle: Smoothed,
    wind: Smoothed,
    sr: f32,
}

impl Bed {
    fn new(sr: f32) -> Self {
        Self {
            rng: Rng(0x1234_5677),
            hi: [Biquad::bandpass(sr, 3600.0, 0.7), Biquad::bandpass(sr, 3300.0, 0.7)],
            mid: [Biquad::bandpass(sr, 1500.0, 0.9), Biquad::bandpass(sr, 1350.0, 0.9)],
            low: [Biquad::bandpass(sr, 520.0, 0.8), Biquad::bandpass(sr, 480.0, 0.8)],
            hiss_hp: [Biquad::highpass(sr, 900.0, 0.6), Biquad::highpass(sr, 900.0, 0.6)],
            hiss_lp: [Biquad::lowpass(sr, 8500.0, 0.6), Biquad::lowpass(sr, 8000.0, 0.6)],
            roar: [Biquad::lowpass(sr, 380.0, 0.6), Biquad::lowpass(sr, 360.0, 0.6)],
            wind_lp: Biquad::lowpass(sr, 260.0, 0.9),
            wind_env: OnePole::new(sr, 0.4),
            air: [OnePole::new(sr, 7500.0), OnePole::new(sr, 7000.0)],
            bubbles: [MiniBubble::default(); 24],
            rate: Smoothed::default(),
            hiss: Smoothed::default(),
            roar_lvl: Smoothed::default(),
            puddle: Smoothed::default(),
            wind: Smoothed::default(),
            sr,
        }
    }

    #[inline]
    fn sample(&mut self, rate: f32, hiss: f32, roar: f32, puddle: f32, wind: f32) -> (f32, f32) {
        let k = 0.0005;
        let rate = self.rate.follow(rate, k);
        let hiss = self.hiss.follow(hiss, k);
        let roar = self.roar_lvl.follow(roar, k);
        let puddle = self.puddle.follow(puddle, k);
        let wind = self.wind.follow(wind, k);
        let p = (rate / self.sr).min(0.6);
        let mut out = [0.0f32; 2];
        let mid = self.rng.bipolar();
        for (ch, o) in out.iter_mut().enumerate() {
            // sparse impulses ("dust") — thousands of distant drop taps
            let mut x = 0.0;
            if self.rng.f32() < p {
                let a = self.rng.f32();
                x = a * a * a * if self.rng.f32() < 0.5 { 1.0 } else { -1.0 };
            }
            let taps = self.hi[ch].process(x) * (1.0 - puddle * 0.6) + self.mid[ch].process(x) * (0.35 + puddle * 0.8) + self.low[ch].process(x) * 0.25;
            let n = self.rng.bipolar() * 0.8 + mid * 0.45;
            let h = self.hiss_lp[ch].process(self.hiss_hp[ch].process(n)) * hiss;
            let r = self.roar[ch].process(self.rng.bipolar()) * roar;
            *o = self.air[ch].process(taps * 1.6 + h) + r;
        }
        // distant puddle plinks
        if self.rng.f32() < rate * puddle * 0.012 / self.sr
            && let Some(b) = self.bubbles.iter_mut().find(|b| b.amp < 1e-4) {
                let f0 = self.rng.range(1300.0, 5200.0);
                let d = 0.043 * f0 + 0.0014 * f0.powf(1.5);
                *b = MiniBubble {
                    amp: self.rng.range(0.004, 0.02),
                    phase: 0.0,
                    freq: f0,
                    rise: f0 * 0.3 * d / self.sr,
                    decay: (-d / self.sr).exp(),
                    pan: self.rng.f32(),
                };
            }
        let tw = std::f32::consts::TAU / self.sr;
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
        // wind: low rumble that swells with gusts
        let env = self.wind_env.process(self.rng.f32());
        let w = self.wind_lp.process(self.rng.bipolar()) * wind * (0.6 + env);
        (out[0] + w, out[1] + w * 0.92)
    }
}

/// Feedback-delay-network reverb: the street's facades and the open sky.
struct Reverb {
    lines: [Vec<f32>; 8],
    pos: [usize; 8],
    gain: [f32; 8],
    damp: [OnePole; 8],
    pre: Vec<f32>,
    pre_pos: usize,
}

impl Reverb {
    fn new(sr: f32, rt60: f32) -> Self {
        let base = [1171usize, 1373, 1601, 1867, 2111, 2381, 2659, 2953];
        let scale = sr / 48_000.0;
        let lens: [usize; 8] = base.map(|l| ((l as f32 * scale) as usize).max(8));
        let gain = lens.map(|l| 10f32.powf(-3.0 * l as f32 / (sr * rt60)));
        Self {
            lines: lens.map(|l| vec![0.0; l]),
            pos: [0; 8],
            gain,
            damp: [OnePole::new(sr, 5200.0); 8],
            pre: vec![0.0; (0.014 * sr) as usize + 1],
            pre_pos: 0,
        }
    }

    #[inline]
    fn sample(&mut self, input: f32) -> (f32, f32) {
        let x = self.pre[self.pre_pos];
        self.pre[self.pre_pos] = input;
        self.pre_pos = (self.pre_pos + 1) % self.pre.len();
        let mut outs = [0.0f32; 8];
        let mut sum = 0.0;
        for i in 0..8 {
            outs[i] = self.lines[i][self.pos[i]];
            sum += outs[i];
        }
        // Householder feedback matrix: y = x − (2/N)·Σx
        let fb = sum * 0.25;
        for i in 0..8 {
            let v = self.damp[i].process((outs[i] - fb) * self.gain[i]);
            let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
            let len = self.lines[i].len();
            self.lines[i][self.pos[i]] = v + x * sign * 0.35;
            self.pos[i] = (self.pos[i] + 1) % len;
        }
        let l = (outs[0] + outs[2] + outs[4] + outs[6]) * 0.3;
        let r = (outs[1] + outs[3] + outs[5] + outs[7]) * 0.3;
        (l, r)
    }
}

pub struct AudioEngine {
    sr: f32,
    voices: Box<[Voice; MAX_VOICES]>,
    bed: Bed,
    reverb: Reverb,
    consumer: rtrb::Consumer<Impact>,
    shared: Arc<AudioShared>,
    master: Smoothed,
    dc: [f32; 4],
    near: Smoothed,
    far: Smoothed,
    wet: Smoothed,
    seed: u32,
}

impl AudioEngine {
    pub fn new(sr: f32, consumer: rtrb::Consumer<Impact>, shared: Arc<AudioShared>) -> Self {
        Self {
            sr,
            voices: Box::new([Voice::default(); MAX_VOICES]),
            bed: Bed::new(sr),
            reverb: Reverb::new(sr, 1.15),
            consumer,
            shared,
            master: Smoothed::default(),
            dc: [0.0; 4],
            near: Smoothed::default(),
            far: Smoothed::default(),
            wet: Smoothed::default(),
            seed: 0x9e37_79b9,
        }
    }

    fn take_events(&mut self) {
        while let Ok(imp) = self.consumer.pop() {
            self.seed = crate::scene::hash_u(self.seed.wrapping_add(0x6d2b_79f5));
            let slot = match self.voices.iter().position(|v| !v.active) {
                Some(i) => i,
                None => {
                    // steal the quietest voice
                    let (i, v) = self
                        .voices
                        .iter()
                        .enumerate()
                        .min_by(|a, b| a.1.level.total_cmp(&b.1.level))
                        .unwrap();
                    let new_level = 0.32 * (imp.size_mm / 2.0).powf(1.6) * (imp.gain_l + imp.gain_r);
                    if v.level > new_level {
                        continue;
                    }
                    i
                }
            };
            let seed = self.seed;
            let sr = self.sr;
            self.voices[slot].start(&imp, sr, seed);
        }
    }

    /// Renders `left.len()` frames of stereo audio.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.take_events();
        let sh = &self.shared;
        let muted = sh.muted.load(Ordering::Relaxed);
        let master_t = if muted { 0.0 } else { sh.master.get() };
        let near_t = sh.near.get();
        let far_t = sh.far.get();
        let wet_t = sh.reverb.get();
        let mm_h = sh.intensity.get().max(0.0);
        let wind = sh.wind.get();
        let puddle = sh.puddle.get().clamp(0.0, 1.0);

        // bed parameters from the rain physics
        let rate = (220.0 * mm_h.powf(0.7)).min(26_000.0);
        let hiss = 0.0045 * mm_h.sqrt();
        let roar = 0.003 * mm_h.powf(0.75).min(60.0);
        let wind_lvl = 0.0035 * (wind / 6.0).powi(2).min(6.0);

        let k = 0.002;
        let mut active = 0;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let master = self.master.follow(master_t, k);
            let near = self.near.follow(near_t, k);
            let far = self.far.follow(far_t, k);
            let wet = self.wet.follow(wet_t, k);
            let (mut nl, mut nr) = (0.0, 0.0);
            for v in self.voices.iter_mut() {
                if v.active {
                    let (a, b) = v.sample();
                    nl += a;
                    nr += b;
                }
            }
            let (bl, br) = self.bed.sample(rate, hiss, roar, puddle, wind_lvl);
            let send = (nl + nr) * 0.18 * near + (bl + br) * 0.5 * far;
            let (rl, rr) = self.reverb.sample(send * wet);
            let ol = nl * near + bl * far + rl;
            let or = nr * near + br * far + rr;
            // DC blocker (~10 Hz high-pass)
            let yl = ol - self.dc[0] + 0.9987 * self.dc[1];
            self.dc[0] = ol;
            self.dc[1] = yl;
            let yr = or - self.dc[2] + 0.9987 * self.dc[3];
            self.dc[2] = or;
            self.dc[3] = yr;
            *l = soft_clip(yl * master * 1.4) * 0.92;
            *r = soft_clip(yr * master * 1.4) * 0.92;
        }
        for v in self.voices.iter() {
            if v.active {
                active += 1;
            }
        }
        sh.voices.store(active, Ordering::Relaxed);
    }
}
