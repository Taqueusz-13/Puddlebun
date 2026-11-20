//! Puddlebun — a high-quality rain simulator.
//! Stage 1: engine core, residential street, rain physics, procedural audio.

#![allow(clippy::too_many_arguments, clippy::needless_range_loop, clippy::field_reassign_with_default)]

mod app;
mod audio;
mod camera;
mod gpu;
mod headless;
mod render;
mod scene;
mod settings;
mod ui;
mod weather;

pub use egui_wgpu::wgpu;
pub use egui_winit::winit;

use anyhow::{bail, Result};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Args {
    pub screenshot: Option<PathBuf>,
    pub audio: Option<PathBuf>,
    pub size: (u32, u32),
    pub frames: u32,
    pub seconds: f32,
    pub shot: usize,
    pub particles: Option<u32>,
    pub rain: Option<f32>,
    pub exposure: Option<f32>,
    pub time: f32,
    pub reset_settings: bool,
    /// custom camera for headless renders: x,y,z,look_x,look_y,look_z
    pub cam: Option<[f32; 6]>,
}

impl Default for Args {
    fn default() -> Self {
        Self {
            screenshot: None,
            audio: None,
            size: (1280, 720),
            frames: 60,
            seconds: 10.0,
            shot: 0,
            particles: None,
            rain: None,
            exposure: None,
            time: 20.0,
            reset_settings: false,
            cam: None,
        }
    }
}

const HELP: &str = "\
Puddlebun — rain simulator

USAGE:
    puddlebun [OPTIONS]

OPTIONS (window):
    --reset-settings        start from default settings
    --particles N           maximum number of simulated drops

OPTIONS (headless, no window needed):
    --screenshot FILE.png   render offscreen and save a PNG
    --audio FILE.wav        render the procedural audio to a WAV
    --size WxH              resolution (default 1280x720)
    --frames N              frames to simulate before the screenshot (default 60)
    --seconds S             length of the audio render (default 10)
    --shot K                viewpoint 0-5 (default 0)
    --rain MMH              rain rate in mm/h
    --exposure EV           exposure in stops
    --cam x,y,z,lx,ly,lz    custom camera position and look-at point

KEYS:
    Tab panel · F free-fly · 1-6 viewpoints · N next · M mute · F11 fullscreen · F12 screenshot
";

fn parse_args() -> Result<Args> {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut val = || it.next().ok_or_else(|| anyhow::anyhow!("{arg} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                std::process::exit(0);
            }
            "--screenshot" => a.screenshot = Some(val()?.into()),
            "--audio" => a.audio = Some(val()?.into()),
            "--size" => {
                let v = val()?;
                let (w, h) = v.split_once('x').ok_or_else(|| anyhow::anyhow!("--size wants WxH"))?;
                a.size = (w.parse()?, h.parse()?);
            }
            "--frames" => a.frames = val()?.parse()?,
            "--seconds" => a.seconds = val()?.parse()?,
            "--shot" => a.shot = val()?.parse()?,
            "--particles" => a.particles = Some(val()?.parse()?),
            "--rain" => a.rain = Some(val()?.parse()?),
            "--exposure" => a.exposure = Some(val()?.parse()?),
            "--time" => a.time = val()?.parse()?,
            "--reset-settings" => a.reset_settings = true,
            "--cam" => {
                let v: Vec<f32> = val()?.split(',').map(|x| x.trim().parse()).collect::<Result<_, _>>()?;
                anyhow::ensure!(v.len() == 6, "--cam wants x,y,z,lookx,looky,lookz");
                a.cam = Some([v[0], v[1], v[2], v[3], v[4], v[5]]);
            }
            other => bail!("unknown option {other} (try --help)"),
        }
    }
    Ok(a)
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,puddlebun=info")).init();
    let args = parse_args()?;
    if args.screenshot.is_some() || args.audio.is_some() {
        return headless::run(&args);
    }
    app::run(args)
}
