//! Puddlebun — a high-quality rain simulator.
//! Stage 2a: the window over the neon street — glass, head movement,
//! open-a-crack, muffled indoor audio, city life, the stylise look.

#![allow(clippy::too_many_arguments, clippy::needless_range_loop, clippy::field_reassign_with_default)]

mod app;
mod audio;
mod camera;
mod city;
mod glass;
mod gpu;
mod headless;
mod render;
mod room;
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
    /// 0 = neon window, 2-7 = street viewpoints
    pub spot: u32,
    pub particles: Option<u32>,
    pub rain: Option<f32>,
    pub exposure: Option<f32>,
    pub time: f32,
    pub reset_settings: bool,
    pub open: bool,
    pub stylise: Option<f32>,
    pub wind: Option<f32>,
    pub muffle: Option<f32>,
    pub city_life: Option<f32>,
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
            spot: 0,
            particles: None,
            rain: None,
            exposure: None,
            time: 20.0,
            reset_settings: false,
            open: false,
            stylise: None,
            wind: None,
            muffle: None,
            city_life: None,
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
    --spot K                0 = the neon window (default), 2-7 = street viewpoints
    --shot K                street viewpoint 0-5 (same as --spot 2+K)
    --open                  open the window a crack
    --rain MMH              rain rate in mm/h
    --wind MS               wind speed in m/s
    --stylise X             0 = realistic … 1 = full lo-fi look
    --muffle X              how muffled the closed window is (0-1)
    --city-life X           how much city life you hear (0-1)
    --exposure EV           exposure in stops
    --cam x,y,z,lx,ly,lz    custom camera position and look-at point

KEYS:
    Tab panel · 1 neon window · 3-8 street views · N next spot · O open/close the window
    mouse / scroll / W S: move your head and lean in at the window
    F free-fly (street) · M mute · F11 fullscreen · F12 screenshot · Ctrl+Q quit
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
            "--spot" => a.spot = val()?.parse()?,
            "--shot" => a.spot = 2 + val()?.parse::<u32>()?,
            "--open" => a.open = true,
            "--stylise" | "--stylize" => a.stylise = Some(val()?.parse()?),
            "--wind" => a.wind = Some(val()?.parse()?),
            "--muffle" => a.muffle = Some(val()?.parse()?),
            "--city-life" => a.city_life = Some(val()?.parse()?),
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
