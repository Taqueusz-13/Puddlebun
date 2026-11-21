//! Tiny DSP building blocks (allocation-free, real-time safe).

use std::f32::consts::{PI, TAU};

/// xorshift32 — fast, good enough for audio noise.
#[derive(Clone, Copy)]
pub struct Rng(pub u32);

impl Rng {
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// Uniform in [0, 1).
    #[inline]
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / 16_777_216.0)
    }
    /// Uniform in [-1, 1).
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }
    #[inline]
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f32()
    }
}

/// RBJ-cookbook biquad (transposed direct form II).
#[derive(Clone, Copy, Default)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn set(&mut self, b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) {
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }
    pub fn bandpass(sr: f32, f: f32, q: f32) -> Self {
        let mut b = Self::default();
        b.set_bandpass(sr, f, q);
        b
    }
    pub fn set_bandpass(&mut self, sr: f32, f: f32, q: f32) {
        let w0 = TAU * f.clamp(20.0, sr * 0.45) / sr;
        let alpha = w0.sin() / (2.0 * q);
        self.set(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * w0.cos(), 1.0 - alpha);
    }
    pub fn lowpass(sr: f32, f: f32, q: f32) -> Self {
        let mut b = Self::default();
        b.set_lowpass(sr, f, q);
        b
    }
    pub fn set_lowpass(&mut self, sr: f32, f: f32, q: f32) {
        let w0 = TAU * f.clamp(20.0, sr * 0.45) / sr;
        let alpha = w0.sin() / (2.0 * q);
        let c = w0.cos();
        self.set((1.0 - c) * 0.5, 1.0 - c, (1.0 - c) * 0.5, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }
    pub fn highpass(sr: f32, f: f32, q: f32) -> Self {
        let mut b = Self::default();
        b.set_highpass(sr, f, q);
        b
    }
    pub fn set_highpass(&mut self, sr: f32, f: f32, q: f32) {
        let w0 = TAU * f.clamp(20.0, sr * 0.45) / sr;
        let alpha = w0.sin() / (2.0 * q);
        let c = w0.cos();
        self.set((1.0 + c) * 0.5, -(1.0 + c), (1.0 + c) * 0.5, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
    }
    /// Peaking EQ (boost or cut `gain_db` around `f`).
    pub fn peaking(sr: f32, f: f32, q: f32, gain_db: f32) -> Self {
        let mut b = Self::default();
        b.set_peaking(sr, f, q, gain_db);
        b
    }
    pub fn set_peaking(&mut self, sr: f32, f: f32, q: f32, gain_db: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = TAU * f.clamp(20.0, sr * 0.45) / sr;
        let alpha = w0.sin() / (2.0 * q);
        let c = w0.cos();
        self.set(1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a);
    }
    /// Low shelf (shelf slope 1).
    pub fn low_shelf(sr: f32, f: f32, gain_db: f32) -> Self {
        let mut b = Self::default();
        b.set_low_shelf(sr, f, gain_db);
        b
    }
    pub fn set_low_shelf(&mut self, sr: f32, f: f32, gain_db: f32) {
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = TAU * f.clamp(20.0, sr * 0.45) / sr;
        let c = w0.cos();
        let alpha = w0.sin() / 2.0 * 2f32.sqrt();
        let sa = 2.0 * a.sqrt() * alpha;
        self.set(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        );
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// One-pole low-pass: y += c·(x − y).
#[derive(Clone, Copy, Default)]
pub struct OnePole {
    pub c: f32,
    pub y: f32,
}

impl OnePole {
    pub fn new(sr: f32, f: f32) -> Self {
        Self { c: Self::coef(sr, f), y: 0.0 }
    }
    pub fn coef(sr: f32, f: f32) -> f32 {
        1.0 - (-TAU * f / sr).exp()
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        self.y += self.c * (x - self.y);
        self.y
    }
}

/// Smoothly follows a target value (for parameters).
#[derive(Clone, Copy, Default)]
pub struct Smoothed {
    pub value: f32,
}

impl Smoothed {
    #[inline]
    pub fn follow(&mut self, target: f32, k: f32) -> f32 {
        self.value += (target - self.value) * k;
        self.value
    }
}

/// Pink noise (Paul Kellet's refined filter), unit-ish level.
#[derive(Clone, Copy, Default)]
pub struct Pink {
    b: [f32; 7],
}

impl Pink {
    #[inline]
    pub fn process(&mut self, white: f32) -> f32 {
        let b = &mut self.b;
        b[0] = 0.99886 * b[0] + white * 0.0555179;
        b[1] = 0.99332 * b[1] + white * 0.0750759;
        b[2] = 0.96900 * b[2] + white * 0.153_852;
        b[3] = 0.86650 * b[3] + white * 0.3104856;
        b[4] = 0.55000 * b[4] + white * 0.5329522;
        b[5] = -0.7616 * b[5] - white * 0.0168980;
        let out = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + white * 0.5362;
        b[6] = white * 0.115926;
        out * 0.2
    }
}

/// Brown(ish) noise: leaky integrated white noise, unit-ish level.
#[derive(Clone, Copy, Default)]
pub struct Brown {
    y: f32,
}

impl Brown {
    #[inline]
    pub fn process(&mut self, white: f32) -> f32 {
        self.y = self.y * 0.997 + white * 0.06;
        self.y.clamp(-4.0, 4.0) * 0.8
    }
}

/// Damped two-pole resonator (a single vibrating mode).
#[derive(Clone, Copy, Default)]
pub struct Mode {
    a1: f32,
    a2: f32,
    g: f32,
    y1: f32,
    y2: f32,
}

impl Mode {
    /// `tau` = decay time constant in seconds.
    pub fn new(sr: f32, f: f32, tau: f32, gain: f32) -> Self {
        let r = (-1.0 / (tau.max(1e-4) * sr)).exp();
        let w = TAU * f.clamp(10.0, sr * 0.45) / sr;
        Self { a1: 2.0 * r * w.cos(), a2: -r * r, g: gain * (1.0 - r * r).sqrt() * 2.0, y1: 0.0, y2: 0.0 }
    }
    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x * self.g + self.a1 * self.y1 + self.a2 * self.y2;
        self.y2 = self.y1;
        self.y1 = y;
        y
    }
    #[inline]
    pub fn energy(&self) -> f32 {
        self.y1.abs() + self.y2.abs()
    }
}

/// Ornstein–Uhlenbeck random walk: wanders around 0 with std `sd`,
/// forgetting itself over `tau` seconds. Stepped at `rate` Hz.
#[derive(Clone, Copy)]
pub struct Wander {
    pub x: f32,
    k: f32,
    s: f32,
}

impl Wander {
    pub fn new(rate: f32, tau: f32, sd: f32) -> Self {
        let k = 1.0 / (tau * rate);
        Self { x: 0.0, k, s: sd * (2.0 * k).sqrt() }
    }
    #[inline]
    pub fn step(&mut self, rng: &mut Rng) -> f32 {
        // sum of 3 uniforms ≈ gaussian (var 1/4 each → scale)
        let g = (rng.bipolar() + rng.bipolar() + rng.bipolar()) * 0.577;
        self.x += -self.x * self.k + g * self.s;
        self.x
    }
}

/// Padé approximation of tanh — a gentle soft clipper.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let x = x.clamp(-3.0, 3.0);
    x * (27.0 + x * x) / (27.0 + 9.0 * x * x)
}

/// Fast sine for |x| up to a few π (Bhaskara-style parabolic approximation, refined).
#[inline]
pub fn fast_sin(x: f32) -> f32 {
    // wrap to [-π, π]
    let x = x - TAU * (x / TAU + 0.5).floor();
    let y = 4.0 / PI * x - 4.0 / (PI * PI) * x * x.abs();
    0.225 * (y * y.abs() - y) + y
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_sin_is_accurate() {
        for i in 0..1000 {
            let x = (i as f32 / 1000.0 - 0.5) * 20.0;
            assert!((fast_sin(x) - x.sin()).abs() < 0.002, "x={x}");
        }
    }

    #[test]
    fn bandpass_passes_centre_and_rejects_far() {
        let sr = 48_000.0;
        let gain_at = |f: f32| {
            let mut bp = Biquad::bandpass(sr, 2000.0, 1.0);
            let mut peak = 0.0f32;
            for n in 0..48_000 {
                let y = bp.process((TAU * f * n as f32 / sr).sin());
                if n > 4000 {
                    peak = peak.max(y.abs());
                }
            }
            peak
        };
        assert!(gain_at(2000.0) > 0.9);
        assert!(gain_at(100.0) < 0.1);
    }

    #[test]
    fn noises_are_bounded_and_coloured() {
        let mut rng = Rng(12345);
        let (mut p, mut b) = (Pink::default(), Brown::default());
        let (mut ps, mut bs) = (0.0f32, 0.0f32);
        for _ in 0..200_000 {
            let w = rng.bipolar();
            let x = p.process(w);
            let y = b.process(w);
            assert!(x.abs() < 3.0 && y.abs() < 4.0);
            ps += x * x;
            bs += y * y;
        }
        let (pr, br) = ((ps / 200_000.0).sqrt(), (bs / 200_000.0).sqrt());
        assert!(pr > 0.05 && pr < 1.0, "pink rms {pr}");
        assert!(br > 0.05 && br < 2.0, "brown rms {br}");
    }

    #[test]
    fn mode_rings_at_its_frequency() {
        let sr = 48_000.0;
        let mut m = Mode::new(sr, 1000.0, 0.05, 1.0);
        let mut last_sign = 0.0f32;
        let mut crossings = 0;
        for n in 0..4800 {
            let y = m.process(if n == 0 { 1.0 } else { 0.0 });
            if y.signum() != last_sign && n > 0 {
                crossings += 1;
            }
            last_sign = y.signum();
        }
        // 1 kHz over 0.1 s → ~200 zero crossings
        assert!((180..=220).contains(&crossings), "{crossings}");
        assert!(m.energy() < 0.2);
    }

    #[test]
    fn shelf_and_peak_have_the_right_gain() {
        let sr = 48_000.0;
        let gain_at = |mut f: Biquad, hz: f32| {
            let mut peak = 0.0f32;
            for n in 0..48_000 {
                let y = f.process((TAU * hz * n as f32 / sr).sin());
                if n > 8000 {
                    peak = peak.max(y.abs());
                }
            }
            20.0 * peak.log10()
        };
        assert!((gain_at(Biquad::peaking(sr, 2000.0, 1.0, 6.0), 2000.0) - 6.0).abs() < 0.3);
        assert!(gain_at(Biquad::peaking(sr, 2000.0, 1.0, 6.0), 200.0).abs() < 0.5);
        assert!((gain_at(Biquad::low_shelf(sr, 200.0, 6.0), 40.0) - 6.0).abs() < 0.5);
        assert!(gain_at(Biquad::low_shelf(sr, 200.0, 6.0), 5000.0).abs() < 0.3);
    }
}
