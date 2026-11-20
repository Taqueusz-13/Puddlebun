//! Procedural, positional rain audio.
//!
//! * Drops hitting the ground near the listener are read back from the GPU
//!   simulation and voiced individually (surface-dependent synthesis).
//! * Everything further away is a statistical "bed" driven by the same rain
//!   rate, wind and puddle level.

pub mod dsp;
pub mod engine;

use crate::camera::CameraState;
use crate::render::rain::GpuImpact;
use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

pub use engine::AudioEngine;

// surface kinds (must match world.wgsl)
pub const SURF_ROAD: u8 = 0;
pub const SURF_PUDDLE: u8 = 1;
pub const SURF_PAVE: u8 = 2;
pub const SURF_ROOF: u8 = 3;
pub const SURF_GARDEN: u8 = 4;

/// f32 stored in an AtomicU32.
#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
}

/// Parameters shared between the UI/simulation thread and the audio thread.
pub struct AudioShared {
    pub master: AtomicF32,
    pub near: AtomicF32,
    pub far: AtomicF32,
    pub reverb: AtomicF32,
    pub intensity: AtomicF32,
    pub wind: AtomicF32,
    pub puddle: AtomicF32,
    pub muted: AtomicBool,
    pub voices: AtomicU32,
}

impl Default for AudioShared {
    fn default() -> Self {
        Self {
            master: AtomicF32::new(0.8),
            near: AtomicF32::new(1.0),
            far: AtomicF32::new(1.0),
            reverb: AtomicF32::new(0.5),
            intensity: AtomicF32::new(5.0),
            wind: AtomicF32::new(2.0),
            puddle: AtomicF32::new(0.5),
            muted: AtomicBool::new(false),
            voices: AtomicU32::new(0),
        }
    }
}

/// One drop impact, already positioned relative to the listener.
#[derive(Clone, Copy, Default, Debug)]
pub struct Impact {
    pub gain_l: f32,
    pub gain_r: f32,
    /// interaural delay in samples; positive = right ear hears it later
    pub itd: i32,
    /// one-pole coefficients for head shadow (1 = no filtering)
    pub shadow_l: f32,
    pub shadow_r: f32,
    pub size_mm: f32,
    pub speed: f32,
    pub surface: u8,
    /// start offset in samples (spreads a frame's impacts over the frame)
    pub delay: u32,
}

/// Individually voiced impacts per second (the loudest ones); the rest are
/// covered by the statistical bed.
pub const VOICES_PER_SECOND: f32 = 2400.0;

/// Turns GPU impacts into listener-relative voices. Keeps the loudest few.
pub fn spatialise(events: &[GpuImpact], cam: &CameraState, sr: f32, frame_dt: f32, rng: &mut fastrand::Rng) -> Vec<Impact> {
    let max = (VOICES_PER_SECOND * frame_dt).ceil().max(1.0) as usize;
    let right = cam.right();
    let fwd = glam::Vec3::new(cam.forward.x, 0.0, cam.forward.z).normalize_or(glam::Vec3::Z);
    let mut out: Vec<(f32, Impact)> = events
        .iter()
        .map(|e| {
            let p = glam::Vec3::from(e.pos);
            let rel = p - cam.pos;
            let dist = rel.length().max(0.35);
            let az = rel.dot(right).atan2(rel.dot(fwd));
            let pan = az.sin().clamp(-1.0, 1.0);
            let theta = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
            let behind = if rel.dot(fwd) < 0.0 { 0.85 } else { 1.0 };
            let g = (0.8 / dist).min(1.15) * behind;
            let itd = (-pan * 0.00066 * sr).round() as i32;
            let far = 1.0 - 0.8 * pan.abs();
            let (shadow_l, shadow_r) = if pan > 0.0 { (far, 1.0) } else { (1.0, far) };
            let delay = (rng.f32() * frame_dt.min(0.05) * sr) as u32;
            let imp = Impact {
                gain_l: theta.cos() * g,
                gain_r: theta.sin() * g,
                itd,
                shadow_l,
                shadow_r,
                size_mm: e.size,
                speed: e.speed,
                surface: e.kind.min(255) as u8,
                delay,
            };
            let loud = e.size.powf(1.6) * g;
            (loud, imp)
        })
        .collect();
    if out.len() > max {
        out.select_nth_unstable_by(max, |a, b| b.0.total_cmp(&a.0));
        out.truncate(max);
    }
    out.into_iter().map(|(_, i)| i).collect()
}

/// A running output stream.
pub struct AudioOutput {
    _stream: cpal::Stream,
    pub producer: rtrb::Producer<Impact>,
    pub sample_rate: f32,
}

pub fn start_output(shared: Arc<AudioShared>) -> Result<AudioOutput> {
    let host = cpal::default_host();
    let device = host.default_output_device().context("no audio output device")?;
    let device_name = device.description().map(|d| format!("{d:?}")).unwrap_or_else(|_| "default".into());
    let supported = device.default_output_config().context("no default output config")?;
    let sr = supported.sample_rate();
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let config = supported.config();
    let (producer, consumer) = rtrb::RingBuffer::new(8192);
    let engine = AudioEngine::new(sr as f32, consumer, shared.clone());
    log::info!("audio: {device_name} @ {sr} Hz, {channels} ch, {format:?}");

    let stream = match format {
        cpal::SampleFormat::F32 => build::<f32>(&device, config, channels, engine)?,
        cpal::SampleFormat::I16 => build::<i16>(&device, config, channels, engine)?,
        cpal::SampleFormat::U16 => build::<u16>(&device, config, channels, engine)?,
        cpal::SampleFormat::I32 => build::<i32>(&device, config, channels, engine)?,
        other => anyhow::bail!("unsupported sample format {other:?}"),
    };
    stream.play().context("couldn't start audio stream")?;
    Ok(AudioOutput { _stream: stream, producer, sample_rate: sr as f32 })
}

fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, channels: usize, mut engine: AudioEngine) -> Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let mut left = vec![0.0f32; 4096];
    let mut right = vec![0.0f32; 4096];
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
            let frames = data.len() / channels.max(1);
            let mut done = 0;
            while done < frames {
                let n = (frames - done).min(left.len());
                engine.render(&mut left[..n], &mut right[..n]);
                for i in 0..n {
                    let frame = &mut data[(done + i) * channels..(done + i + 1) * channels];
                    match channels {
                        1 => frame[0] = T::from_sample((left[i] + right[i]) * 0.5),
                        _ => {
                            frame[0] = T::from_sample(left[i]);
                            frame[1] = T::from_sample(right[i]);
                            for s in frame.iter_mut().skip(2) {
                                *s = T::from_sample(0.0f32);
                            }
                        }
                    }
                }
                done += n;
            }
        },
        |e| log::warn!("audio stream error: {e}"),
        None,
    )?;
    Ok(stream)
}

/// Writes 16-bit stereo PCM WAV (used by headless mode for audio previews).
pub fn write_wav(path: &std::path::Path, sr: u32, left: &[f32], right: &[f32]) -> Result<()> {
    let n = left.len().min(right.len());
    let data_len = (n * 4) as u32;
    let mut b = Vec::with_capacity(44 + data_len as usize);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&sr.to_le_bytes());
    b.extend_from_slice(&(sr * 4).to_le_bytes());
    b.extend_from_slice(&4u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..n {
        for s in [left[i], right[i]] {
            let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            b.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(path, b)?;
    Ok(())
}
