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
        let w0 = TAU * f.clamp(20.0, sr * 0.45) / sr;
        let alpha = w0.sin() / (2.0 * q);
        let c = w0.cos();
        b.set((1.0 - c) * 0.5, 1.0 - c, (1.0 - c) * 0.5, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
        b
    }
    pub fn highpass(sr: f32, f: f32, q: f32) -> Self {
        let mut b = Self::default();
        let w0 = TAU * f.clamp(20.0, sr * 0.45) / sr;
        let alpha = w0.sin() / (2.0 * q);
        let c = w0.cos();
        b.set((1.0 + c) * 0.5, -(1.0 + c), (1.0 + c) * 0.5, 1.0 + alpha, -2.0 * c, 1.0 - alpha);
        b
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
}
