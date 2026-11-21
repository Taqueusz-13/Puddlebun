//! The real-time rain synthesiser. Runs on the audio thread: no allocation,
//! no locks. Impacts arrive through a lock-free ring buffer from the
//! simulation; everything else (the rain bed, city life, the room) is driven
//! by the same physical parameters the simulation uses.
//!
//! Signal flow:
//!   voices(outside) + bed + city ─┬─ street reverb ─┐
//!                                 └───────────────┴─▶ outdoor
//!   outdoor ─▶ [inside: through the glass — muffled, leaking in at the crack]
//!   voices(inside) + radiator + room tone + sash ─▶ small room reverb
//!   ─▶ master ─▶ DC block ─▶ soft clip

use super::bed::{Bed, BedIn};
use super::city::{CityIn, CityLife};
use super::dsp::*;
use super::room::RoomSounds;
use super::{AudioMsg, AudioShared, Impact, SURF_AWNING, SURF_GARDEN, SURF_GLASS, SURF_METAL, SURF_PAVE, SURF_PUDDLE, SURF_ROAD, SURF_ROOF, SURF_STONE, SURF_WOOD};
use std::sync::atomic::Ordering;
use std::sync::Arc;

const MAX_VOICES: usize = 256;
const HIST: usize = 64;

#[derive(Clone, Copy)]
struct Micro {
    at: u32,
    amp: f32,
}

#[derive(Clone, Copy)]
struct Voice {
    active: bool,
    inside: bool,
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
    // resonant modes of what was hit (metal, glass, wood, or the ground's thump)
    modes: [Mode; 3],
    n_modes: usize,
    mode_drive: f32,
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
            inside: false,
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
            modes: [Mode::default(); 3],
            n_modes: 0,
            mode_drive: 0.0,
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

/// How loud a surface is compared to the ground (glass taps from inside are soft).
fn surface_gain(s: u8) -> f32 {
    match s {
        SURF_GLASS => 0.5,
        SURF_WOOD => 0.3,
        SURF_METAL => 0.9,
        SURF_STONE => 1.0,
        _ => 1.0,
    }
}

fn impact_level(imp: &Impact) -> f32 {
    let size = imp.size_mm.clamp(0.3, 6.0);
    let energy = (size / 2.0).powf(1.6) * (imp.speed / 7.0).clamp(0.2, 1.5).powf(1.5);
    0.32 * energy * surface_gain(imp.surface) * (imp.gain_l + imp.gain_r)
}

impl Voice {
    fn start(&mut self, imp: &Impact, sr: f32, seed: u32) {
        let mut rng = Rng(seed | 1);
        let size = imp.size_mm.clamp(0.3, 6.0);
        let energy = (size / 2.0).powf(1.6) * (imp.speed / 7.0).clamp(0.2, 1.5).powf(1.5);
        let base = 0.32 * energy * surface_gain(imp.surface);
        self.active = true;
        self.inside = imp.inside;
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
        self.n_modes = 0;
        self.mode_drive = 0.0;
        self.tw = std::f32::consts::TAU / sr;

        // surface-specific impact character:
        // (tick centre, Q, tick decay ms, tick amp, bubble chance, spray ticks)
        let size_f = (2.0 / size).sqrt();
        let (fc, q, tau_ms, amp, bubble_p, micro_n) = match imp.surface {
            SURF_PUDDLE => (1500.0 * size_f * rng.range(0.8, 1.25), 1.1, 2.2 + 0.6 * size, 0.55, 0.55, 1),
            SURF_ROAD => (2700.0 * size_f * rng.range(0.8, 1.25), 0.85, 1.3 + 0.4 * size, 0.8, 0.06, 2),
            SURF_PAVE => (3300.0 * size_f * rng.range(0.8, 1.25), 0.9, 1.1 + 0.35 * size, 0.85, 0.04, 2),
            SURF_ROOF => (2100.0 * rng.range(0.85, 1.2), 3.2, 6.0, 0.7, 0.0, 1),
            SURF_GARDEN => (850.0 * rng.range(0.8, 1.2), 0.6, 3.0, 0.3, 0.0, 0),
            SURF_METAL => (4200.0 * rng.range(0.85, 1.2), 1.2, 0.8, 0.5, 0.0, 1),
            SURF_GLASS => (5200.0 * rng.range(0.85, 1.2), 1.0, 0.45, 0.6, 0.0, 0),
            SURF_AWNING => (900.0 * rng.range(0.85, 1.2), 0.7, 2.0, 0.35, 0.0, 0),
            SURF_WOOD => (1400.0 * rng.range(0.85, 1.2), 1.0, 1.5, 0.7, 0.18, 1),
            SURF_STONE => (3200.0 * size_f * rng.range(0.85, 1.2), 0.9, 1.0 + 0.3 * size, 0.8, 0.8, 2),
            _ => (2500.0, 0.9, 1.5, 0.6, 0.0, 1),
        };
        self.bq = Biquad::bandpass(sr, fc, q);
        self.tick_amp = base * amp;
        self.tick_env = 1.0;
        self.tick_decay = (-1.0 / (tau_ms * 0.001 * sr)).exp();

        // resonant modes: what the hit object rings with
        let mut tau_max = 0.0f32;
        let mut add_mode = |v: &mut Self, f: f32, tau: f32, g: f32| {
            if v.n_modes < 3 {
                v.modes[v.n_modes] = Mode::new(sr, f, tau, g);
                v.n_modes += 1;
                tau_max = tau_max.max(tau);
            }
        };
        match imp.surface {
            SURF_METAL => {
                // a beam-like fire escape slat: inharmonic, rings for a while
                let f1 = rng.range(1600.0, 2700.0);
                add_mode(self, f1, rng.range(0.05, 0.12), 1.0);
                add_mode(self, f1 * 2.76, rng.range(0.03, 0.06), 0.6);
                add_mode(self, f1 * 5.40, rng.range(0.015, 0.03), 0.35);
                self.mode_drive = base * 0.9;
            }
            SURF_GLASS => {
                // the pane's low body and a brighter plate mode
                add_mode(self, rng.range(210.0, 380.0), rng.range(0.015, 0.028), 1.0);
                add_mode(self, rng.range(900.0, 1500.0), rng.range(0.006, 0.012), 0.5);
                self.mode_drive = base * 0.8;
            }
            SURF_AWNING => {
                add_mode(self, rng.range(160.0, 320.0), rng.range(0.025, 0.045), 1.0);
                self.mode_drive = base * 1.2;
            }
            SURF_WOOD => {
                add_mode(self, rng.range(520.0, 720.0), rng.range(0.008, 0.016), 1.0);
                self.mode_drive = base * 0.5;
            }
            SURF_ROAD | SURF_PAVE | SURF_PUDDLE | SURF_STONE if size > 1.5 => {
                // a little low-end weight for the bigger drops nearby
                add_mode(self, rng.range(150.0, 260.0), rng.range(0.005, 0.009), 1.0);
                self.mode_drive = base * 0.13 * (size / 2.0).min(2.5);
            }
            _ => {}
        }

        // entrained air bubble: f0 = 3.26 / R (Minnaert), damping after van den Doel
        if size > 0.7 && rng.f32() < bubble_p {
            let r_mm = if imp.surface == SURF_STONE { size * rng.range(0.3, 0.6) } else { size * rng.range(0.3, 0.8) }.clamp(0.25, 3.5);
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
        let tail = (tau_ms * 0.001 * 9.0 + 0.06).max(tau_max * 6.0);
        self.life = (tail * sr) as u32 + self.b_start + (0.03 * sr) as u32;
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
        let mut exc = 0.0;
        if self.tick_env > 1e-4 {
            let n = self.rng.bipolar();
            exc = n * self.tick_env;
            x += self.bq.process(exc) * self.tick_amp * 2.5;
            if t < 2 {
                x += self.tick_amp * 0.35 * if t == 0 { 1.0 } else { -0.6 };
            }
            self.tick_env *= self.tick_decay;
        }
        // modes, struck by the transient
        if self.n_modes > 0 {
            let drive = if t < 3 { [1.0, -0.6, 0.2][t as usize] } else { exc * 0.15 } * self.mode_drive;
            for m in &mut self.modes[..self.n_modes] {
                x += m.process(drive);
            }
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

/// Feedback-delay-network reverb.
struct Reverb {
    lines: [Vec<f32>; 8],
    pos: [usize; 8],
    gain: [f32; 8],
    damp: [OnePole; 8],
    pre: Vec<f32>,
    pre_pos: usize,
}

impl Reverb {
    fn new(sr: f32, rt60: f32, size: f32, damp_hz: f32, predelay: f32) -> Self {
        let base = [1171usize, 1373, 1601, 1867, 2111, 2381, 2659, 2953];
        let scale = sr / 48_000.0 * size;
        let lens: [usize; 8] = base.map(|l| ((l as f32 * scale) as usize).max(8));
        let gain = lens.map(|l| 10f32.powf(-3.0 * l as f32 / (sr * rt60)));
        Self {
            lines: lens.map(|l| vec![0.0; l]),
            pos: [0; 8],
            gain,
            damp: [OnePole::new(sr, damp_hz); 8],
            pre: vec![0.0; (predelay * sr) as usize + 1],
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

/// The closed window between the outdoor mix and our ears.
struct Glass {
    lp1: [Biquad; 2],
    lp2: [Biquad; 2],
    shelf: [Biquad; 2],
    open_lp: [Biquad; 2],
    cutoff: f32,
}

impl Glass {
    fn new(sr: f32) -> Self {
        Self {
            lp1: [Biquad::lowpass(sr, 1600.0, 0.6); 2],
            lp2: [Biquad::lowpass(sr, 1600.0, 0.6); 2],
            shelf: [Biquad::low_shelf(sr, 120.0, 5.0); 2],
            open_lp: [Biquad::lowpass(sr, 9000.0, 0.6); 2],
            cutoff: 1600.0,
        }
    }

    fn set_muffle(&mut self, sr: f32, muffle: f32) {
        let c = 3800.0 * 0.18f32.powf(muffle.clamp(0.0, 1.0));
        if (c / self.cutoff - 1.0).abs() > 0.02 {
            self.cutoff = c;
            for ch in 0..2 {
                self.lp1[ch].set_lowpass(sr, c, 0.6);
                self.lp2[ch].set_lowpass(sr, c * 1.15, 0.6);
            }
        }
    }
}

pub struct AudioEngine {
    sr: f32,
    voices: Box<[Voice; MAX_VOICES]>,
    bed: Bed,
    city: CityLife,
    room: RoomSounds,
    street_verb: Reverb,
    room_verb: Reverb,
    glass: Glass,
    consumer: rtrb::Consumer<AudioMsg>,
    shared: Arc<AudioShared>,
    master: Smoothed,
    near: Smoothed,
    far: Smoothed,
    wet: Smoothed,
    interior: Smoothed,
    open: Smoothed,
    closed_gain: Smoothed,
    fade: Smoothed,
    city_mix: Smoothed,
    dc: [f32; 4],
    seed: u32,
    n: u32,
}

impl AudioEngine {
    pub fn new(sr: f32, consumer: rtrb::Consumer<AudioMsg>, shared: Arc<AudioShared>) -> Self {
        let interior = shared.interior.get();
        let city = shared.scene.load(Ordering::Relaxed) as f32;
        Self {
            sr,
            voices: Box::new([Voice::default(); MAX_VOICES]),
            bed: Bed::new(sr),
            city: CityLife::new(sr),
            room: RoomSounds::new(sr),
            street_verb: Reverb::new(sr, 1.4, 1.0, 5200.0, 0.014),
            room_verb: Reverb::new(sr, 0.42, 0.32, 3600.0, 0.003),
            glass: Glass::new(sr),
            consumer,
            shared,
            master: Smoothed::default(),
            near: Smoothed::default(),
            far: Smoothed::default(),
            wet: Smoothed::default(),
            interior: Smoothed { value: interior },
            open: Smoothed::default(),
            closed_gain: Smoothed { value: 0.3 },
            fade: Smoothed::default(),
            city_mix: Smoothed { value: city },
            dc: [0.0; 4],
            seed: 0x9e37_79b9,
            n: 0,
        }
    }

    fn take_events(&mut self) {
        while let Ok(msg) = self.consumer.pop() {
            let imp = match msg {
                AudioMsg::Slide { opening, seconds } => {
                    self.room.slide(opening, seconds);
                    continue;
                }
                AudioMsg::Impact(i) => i,
            };
            self.seed = crate::scene::hash_u(self.seed.wrapping_add(0x6d2b_79f5));
            let slot = match self.voices.iter().position(|v| !v.active) {
                Some(i) => i,
                None => {
                    // steal the quietest voice
                    let (i, v) = self.voices.iter().enumerate().min_by(|a, b| a.1.level.total_cmp(&b.1.level)).unwrap();
                    if v.level > impact_level(&imp) {
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
        let city_t = sh.scene.load(Ordering::Relaxed).min(1) as f32;
        let interior_t = sh.interior.get().clamp(0.0, 1.0);
        let muffle = sh.muffle.get().clamp(0.0, 1.0);
        let open_t = sh.open.get().clamp(0.0, 1.0);
        let life = sh.city_life.get().clamp(0.0, 1.0);
        let fade_t = sh.fade.get().clamp(0.0, 1.0);

        self.glass.set_muffle(self.sr, muffle);
        let closed_t = 10f32.powf(-(3.0 + 11.0 * muffle) / 20.0);

        let k = 0.002;
        let mut active = 0;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.n = self.n.wrapping_add(1);
            let master = self.master.follow(master_t, k);
            let near = self.near.follow(near_t, k);
            let far = self.far.follow(far_t, k);
            let wet = self.wet.follow(wet_t, k);
            let interior = self.interior.follow(interior_t, 0.00006);
            let open = self.open.follow(open_t, 0.0004);
            let closed = self.closed_gain.follow(closed_t, 0.0005);
            let fade = self.fade.follow(fade_t, 0.001);
            let city = self.city_mix.follow(city_t, 0.00006);

            let (mut ol, mut or) = (0.0, 0.0);
            let (mut il, mut ir) = (0.0, 0.0);
            for v in self.voices.iter_mut() {
                if v.active {
                    let (a, b) = v.sample();
                    if v.inside {
                        il += a;
                        ir += b;
                    } else {
                        ol += a;
                        or += b;
                    }
                }
            }
            let bp = BedIn { mm_h, wind, puddle, city };
            let (bl, br) = self.bed.sample(&bp);
            let (cl, cr) = self.city.sample(&CityIn { life, city, mm_h });

            // outdoors, with the street's echo
            let send = (ol + or) * 0.18 * near + (bl + br) * 0.5 * far + (cl + cr) * 0.6;
            let (rl, rr) = self.street_verb.sample(send * wet);
            let mut out_l = ol * near + bl * far + cl + rl;
            let mut out_r = or * near + br * far + cr + rr;

            if interior > 0.0005 {
                // through the glass: low-passed, a little boom in the low end,
                // narrowed to a source in front of us; plus the leak at the crack
                let g = &mut self.glass;
                let ml = g.shelf[0].process(g.lp2[0].process(g.lp1[0].process(out_l)));
                let mr = g.shelf[1].process(g.lp2[1].process(g.lp1[1].process(out_r)));
                let mid = (ml + mr) * 0.5;
                let side = (ml - mr) * 0.5 * 0.35;
                let open_k = 0.6 * open.sqrt();
                let leak_l = g.open_lp[0].process(out_l) * open_k;
                let leak_r = g.open_lp[1].process(out_r) * open_k;
                let tl = (mid + side) * closed * (1.0 - 0.5 * open_k) + leak_l;
                let tr = (mid - side) * closed * (1.0 - 0.5 * open_k) + leak_r;

                // the room: taps on the pane, spatter on the sill, the radiator
                let (sl, sr) = self.room.sample(1.0);
                let in_l = il * near + sl;
                let in_r = ir * near + sr;
                let (vl, vr) = self.room_verb.sample((in_l + in_r) * 0.5 * 0.5 + (tl + tr) * 0.5 * 0.12);
                let room_l = tl + in_l + vl;
                let room_r = tr + in_r + vr;
                out_l += (room_l - out_l) * interior;
                out_r += (room_r - out_r) * interior;
            }

            let gain = master * (1.0 - 0.7 * fade);
            // DC blocker (~10 Hz high-pass)
            let yl = out_l - self.dc[0] + 0.9987 * self.dc[1];
            self.dc[0] = out_l;
            self.dc[1] = yl;
            let yr = out_r - self.dc[2] + 0.9987 * self.dc[3];
            self.dc[2] = out_r;
            self.dc[3] = yr;
            *l = soft_clip(yl * gain * 1.4) * 0.92;
            *r = soft_clip(yr * gain * 1.4) * 0.92;
        }
        for v in self.voices.iter() {
            if v.active {
                active += 1;
            }
        }
        self.shared.voices.store(active, Ordering::Relaxed);
    }
}
