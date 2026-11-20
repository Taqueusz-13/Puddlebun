//! User settings, saved automatically to `~/.config/puddlebun/settings.toml`.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    // --- weather ---
    /// Base rain rate in mm/h (living weather drifts around this).
    pub intensity_mm_h: f32,
    /// Base wind speed (m/s).
    pub wind_speed: f32,
    /// Direction the wind blows towards, degrees (0 = down the street, +Z).
    pub wind_dir_deg: f32,
    /// 0 = steady, 1 = very gusty.
    pub gustiness: f32,
    /// Slowly varying showers and wind (preview of stage 2's modulation system).
    pub living_weather: bool,

    // --- world ---
    pub wetness: f32,
    pub puddle_level: f32,
    /// Puddles fill with rain and dry out by themselves.
    pub auto_puddles: bool,
    pub fog: f32,
    /// Street lamp colour temperature (K). ~2000 K = old sodium, 4000 K = LED.
    pub lamp_kelvin: f32,
    pub lamp_intensity: f32,
    /// Fraction of windows with the lights on.
    pub window_lights: f32,
    pub sky_glow: f32,
    /// Exposure in stops.
    pub exposure_ev: f32,
    pub bloom: f32,
    /// Camera shutter time used for motion-blurred streaks (s).
    pub shutter: f32,
    pub ripple_strength: f32,

    // --- audio ---
    pub muted: bool,
    pub master_volume: f32,
    pub near_volume: f32,
    pub far_volume: f32,
    pub reverb: f32,

    // --- camera ---
    pub drift: bool,
    /// Seconds between automatic moves to the next viewpoint.
    pub drift_interval_s: f32,
    pub fov_deg: f32,

    // --- quality ---
    pub max_particles: u32,
    pub reflection_scale: f32,

    // --- ui ---
    pub show_panel: bool,
    pub show_stats: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            intensity_mm_h: 6.0,
            wind_speed: 2.5,
            wind_dir_deg: 70.0,
            gustiness: 0.45,
            living_weather: true,
            wetness: 1.0,
            puddle_level: 0.55,
            auto_puddles: false,
            fog: 0.35,
            lamp_kelvin: 2500.0,
            lamp_intensity: 1.0,
            window_lights: 0.35,
            sky_glow: 1.0,
            exposure_ev: 0.0,
            bloom: 0.6,
            shutter: 1.0 / 60.0,
            ripple_strength: 1.0,
            muted: false,
            master_volume: 0.8,
            near_volume: 1.0,
            far_volume: 1.0,
            reverb: 0.5,
            drift: true,
            drift_interval_s: 600.0,
            fov_deg: 55.0,
            max_particles: 1_000_000,
            reflection_scale: 0.5,
            show_panel: false,
            show_stats: false,
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("puddlebun").join("settings.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Settings>(&text) {
                Ok(mut s) => {
                    s.sanitize();
                    s
                }
                Err(e) => {
                    log::warn!("couldn't parse {}: {e} — using defaults", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match toml::to_string_pretty(self) {
            Ok(text) => {
                let header = "# Puddlebun settings — saved automatically. Safe to edit while the app is closed.\n\n";
                if let Err(e) = std::fs::write(&path, format!("{header}{text}")) {
                    log::warn!("couldn't save settings: {e}");
                }
            }
            Err(e) => log::warn!("couldn't serialise settings: {e}"),
        }
    }

    /// Clamp everything into sane ranges (protects against hand-edited files).
    pub fn sanitize(&mut self) {
        self.intensity_mm_h = self.intensity_mm_h.clamp(0.1, 120.0);
        self.wind_speed = self.wind_speed.clamp(0.0, 25.0);
        self.gustiness = self.gustiness.clamp(0.0, 1.0);
        self.wetness = self.wetness.clamp(0.0, 1.0);
        self.puddle_level = self.puddle_level.clamp(0.0, 1.0);
        self.fog = self.fog.clamp(0.0, 1.0);
        self.lamp_kelvin = self.lamp_kelvin.clamp(1700.0, 6500.0);
        self.lamp_intensity = self.lamp_intensity.clamp(0.0, 4.0);
        self.window_lights = self.window_lights.clamp(0.0, 1.0);
        self.sky_glow = self.sky_glow.clamp(0.0, 4.0);
        self.exposure_ev = self.exposure_ev.clamp(-4.0, 4.0);
        self.bloom = self.bloom.clamp(0.0, 3.0);
        self.shutter = self.shutter.clamp(1.0 / 1000.0, 1.0 / 15.0);
        self.ripple_strength = self.ripple_strength.clamp(0.0, 3.0);
        self.master_volume = self.master_volume.clamp(0.0, 1.5);
        self.near_volume = self.near_volume.clamp(0.0, 2.0);
        self.far_volume = self.far_volume.clamp(0.0, 2.0);
        self.reverb = self.reverb.clamp(0.0, 1.0);
        self.drift_interval_s = self.drift_interval_s.clamp(20.0, 3600.0);
        self.fov_deg = self.fov_deg.clamp(25.0, 100.0);
        self.max_particles = self.max_particles.clamp(10_000, 4_000_000);
        self.reflection_scale = self.reflection_scale.clamp(0.25, 1.0);
    }
}
