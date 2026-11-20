//! Rain physics on the CPU side: the drop-size distribution, terminal
//! velocities, gusting wind and "living weather" (slowly drifting showers).
//!
//! The GPU simulation (src/shaders/rain_sim.wgsl) uses the same formulas;
//! this module decides the parameters and checks them with unit tests.

use crate::settings::Settings;
use glam::Vec3;

/// Marshall–Palmer intercept N0 (m⁻³ mm⁻¹).
pub const MP_N0: f32 = 8000.0;
/// Smallest drop we render as a particle (mm). Smaller ones are mist/fog.
pub const D_MIN_VISIBLE: f32 = 0.5;
/// Largest stable raindrop (mm); bigger ones break up.
pub const D_MAX: f32 = 5.5;

/// Marshall–Palmer slope Λ (mm⁻¹) for a rain rate R (mm/h).
pub fn mp_lambda(rate_mm_h: f32) -> f32 {
    4.1 * rate_mm_h.max(0.05).powf(-0.21)
}

/// Terminal fall speed (m/s) of a drop of diameter `d_mm` (Atlas et al. 1973).
pub fn terminal_velocity(d_mm: f32) -> f32 {
    (9.65 - 10.3 * (-0.6 * d_mm).exp()).max(0.3)
}

/// Number of drops per m³ with diameters in [dmin, dmax].
pub fn number_density(rate: f32, dmin: f32, dmax: f32) -> f32 {
    let l = mp_lambda(rate);
    MP_N0 / l * ((-l * dmin).exp() - (-l * dmax).exp())
}

/// Number of drops hitting each m² of ground per second, sizes in [dmin, dmax].
pub fn number_flux(rate: f32, dmin: f32, dmax: f32) -> f32 {
    let l = mp_lambda(rate);
    let steps = 400;
    let dd = (dmax - dmin) / steps as f32;
    (0..steps)
        .map(|i| {
            let d = dmin + (i as f32 + 0.5) * dd;
            MP_N0 * (-l * d).exp() * terminal_velocity(d) * dd
        })
        .sum()
}

/// Rain rate (mm/h) implied by the distribution — used to sanity-check physics.
#[cfg(test)]
pub fn implied_rate(rate: f32) -> f32 {
    let l = mp_lambda(rate);
    let steps = 600;
    let (dmin, dmax) = (0.05, 7.0);
    let dd = (dmax - dmin) / steps as f32;
    let integral: f32 = (0..steps)
        .map(|i| {
            let d = dmin + (i as f32 + 0.5) * dd;
            MP_N0 * (-l * d).exp() * d.powi(3) * terminal_velocity(d) * dd
        })
        .sum();
    // R = 6π·10⁻⁴ ∫ N(D) D³ v(D) dD   (N in m⁻³mm⁻¹, D in mm, v in m/s)
    6.0 * std::f32::consts::PI * 1e-4 * integral
}

/// Mean-reverting random process (Ornstein–Uhlenbeck), exact update.
#[derive(Clone, Debug)]
struct Ou {
    x: f32,
    tau: f32,
    sigma: f32,
}

impl Ou {
    fn new(tau: f32, sigma: f32) -> Self {
        Self { x: 0.0, tau, sigma }
    }
    fn step(&mut self, dt: f32, rng: &mut fastrand::Rng) -> f32 {
        let a = (-dt / self.tau).exp();
        let noise = gaussian(rng);
        self.x = self.x * a + self.sigma * (1.0 - a * a).sqrt() * noise;
        self.x
    }
}

fn gaussian(rng: &mut fastrand::Rng) -> f32 {
    // Box–Muller
    let u1 = rng.f32().max(1e-7);
    let u2 = rng.f32();
    (-2.0 * u1.ln()).sqrt() * (std::f32::consts::TAU * u2).cos()
}

/// Everything the renderer and audio need to know about the weather right now.
#[derive(Clone, Debug)]
pub struct WeatherState {
    pub intensity: f32,
    pub wind: Vec3,
    pub gust: f32,
    pub puddle_level: f32,
    pub wetness: f32,
    pub shower: f32,
}

pub struct Weather {
    pub state: WeatherState,
    rng: fastrand::Rng,
    rain_ou: Ou,
    wind_speed_ou: Ou,
    wind_dir_ou: Ou,
    gust_ou: Ou,
    shower_timer: f32,
    shower_t: f32,
    shower_len: f32,
    shower_gain: f32,
}

impl Weather {
    pub fn new(s: &Settings) -> Self {
        Self {
            state: WeatherState {
                intensity: s.intensity_mm_h,
                wind: Vec3::ZERO,
                gust: 0.0,
                puddle_level: s.puddle_level,
                wetness: s.wetness,
                shower: 0.0,
            },
            rng: fastrand::Rng::with_seed(0x5eed_cafe),
            rain_ou: Ou::new(80.0, 0.45),
            wind_speed_ou: Ou::new(60.0, 0.3),
            wind_dir_ou: Ou::new(150.0, 0.35),
            gust_ou: Ou::new(3.5, 1.0),
            shower_timer: 240.0,
            shower_t: -1.0,
            shower_len: 0.0,
            shower_gain: 0.0,
        }
    }

    pub fn update(&mut self, dt: f32, s: &Settings) {
        let dt = dt.clamp(0.0, 0.25);
        let rng = &mut self.rng;

        // --- rain rate ---
        let mut rate = s.intensity_mm_h;
        let mut shower = 0.0;
        if s.living_weather {
            let x = self.rain_ou.step(dt, rng);
            rate *= x.exp();
            // occasional heavier showers that build and fade over a couple of minutes
            self.shower_timer -= dt;
            if self.shower_t < 0.0 && self.shower_timer <= 0.0 {
                self.shower_t = 0.0;
                self.shower_len = 90.0 + rng.f32() * 150.0;
                self.shower_gain = 1.5 + rng.f32() * 3.0;
                self.shower_timer = 300.0 + rng.f32() * 600.0;
            }
            if self.shower_t >= 0.0 {
                self.shower_t += dt;
                let p = (self.shower_t / self.shower_len).clamp(0.0, 1.0);
                shower = (std::f32::consts::PI * p).sin().powf(1.5);
                rate *= 1.0 + (self.shower_gain - 1.0) * shower;
                if self.shower_t >= self.shower_len {
                    self.shower_t = -1.0;
                }
            }
        }
        let rate = rate.clamp(0.1, 150.0);
        // smooth so sudden slider moves don't pop
        let k = 1.0 - (-dt / 1.5).exp();
        self.state.intensity += (rate - self.state.intensity) * k;
        self.state.shower = shower;

        // --- wind ---
        let mut speed = s.wind_speed;
        let mut dir = s.wind_dir_deg.to_radians();
        if s.living_weather {
            speed *= self.wind_speed_ou.step(dt, rng).exp() * (1.0 + 0.6 * shower);
            dir += self.wind_dir_ou.step(dt, rng);
        }
        let g = self.gust_ou.step(dt, rng);
        let gust = (g * s.gustiness).max(-0.6);
        let mag = speed * (1.0 + gust * 0.7);
        let target = Vec3::new(dir.sin() * mag, 0.0, dir.cos() * mag);
        let kw = 1.0 - (-dt / 0.6).exp();
        self.state.wind += (target - self.state.wind) * kw;
        self.state.gust = s.gustiness * (0.5 + 0.5 * g.abs().min(1.0));

        // --- ground water ---
        self.state.wetness = s.wetness;
        if s.auto_puddles {
            // puddles creep up while it rains and dry slowly afterwards
            let target = (0.18 + 0.16 * (1.0 + self.state.intensity).ln()).clamp(0.0, 0.95);
            let tau = if target > self.state.puddle_level { 150.0 } else { 900.0 };
            self.state.puddle_level += (target - self.state.puddle_level) * (1.0 - (-dt / tau).exp());
        } else {
            let k = 1.0 - (-dt / 0.5).exp();
            self.state.puddle_level += (s.puddle_level - self.state.puddle_level) * k;
        }
    }
}

/// Colour of a black-body light source at `kelvin`, normalised to luminance 1
/// (Tanner Helland's fit, linearised).
pub fn kelvin_to_rgb(kelvin: f32) -> Vec3 {
    let t = kelvin / 100.0;
    let r = if t <= 66.0 { 255.0 } else { 329.698_73 * (t - 60.0).powf(-0.133_204_76) };
    let g = if t <= 66.0 {
        99.470_8 * t.ln() - 161.119_57
    } else {
        288.122_16 * (t - 60.0).powf(-0.075_514_85)
    };
    let b = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        138.517_73 * (t - 10.0).ln() - 305.044_8
    };
    let srgb = Vec3::new(r, g, b).clamp(Vec3::ZERO, Vec3::splat(255.0)) / 255.0;
    let lin = Vec3::new(
        srgb_to_linear(srgb.x),
        srgb_to_linear(srgb.y),
        srgb_to_linear(srgb.z),
    );
    let lum = lin.dot(Vec3::new(0.2126, 0.7152, 0.0722)).max(1e-4);
    lin / lum
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_velocity_matches_measurements() {
        // Gunn & Kinzer (1949): 1 mm ≈ 4.0 m/s, 2 mm ≈ 6.5 m/s, 4 mm ≈ 8.8 m/s
        assert!((terminal_velocity(1.0) - 4.0).abs() < 0.4);
        assert!((terminal_velocity(2.0) - 6.5).abs() < 0.4);
        assert!((terminal_velocity(4.0) - 8.8).abs() < 0.4);
    }

    #[test]
    fn distribution_reproduces_rain_rate() {
        // Marshall–Palmer with Atlas velocities should roughly give back R.
        for r in [1.0f32, 5.0, 20.0, 60.0] {
            let implied = implied_rate(r);
            let ratio = implied / r;
            assert!((0.6..1.6).contains(&ratio), "R={r}: implied {implied}");
        }
    }

    #[test]
    fn heavier_rain_has_bigger_drops() {
        assert!(mp_lambda(50.0) < mp_lambda(2.0));
        assert!(number_density(20.0, 2.0, 5.5) > number_density(2.0, 2.0, 5.5));
    }

    #[test]
    fn flux_is_physically_plausible() {
        // moderate rain: thousands of drops per m² per second
        let f = number_flux(5.0, 0.3, 6.0);
        assert!(f > 500.0 && f < 20_000.0, "flux {f}");
    }

    #[test]
    fn kelvin_colours() {
        let sodium = kelvin_to_rgb(2000.0);
        let led = kelvin_to_rgb(5000.0);
        assert!(sodium.x > sodium.z * 3.0);
        assert!(led.z > sodium.z);
    }
}
