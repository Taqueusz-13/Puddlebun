//! Headless mode: render frames (and optionally the audio) without a window.
//! Used for screenshots and for testing on machines without a display.
//!
//!   puddlebun --screenshot window.png --size 1920x1080 --frames 90
//!   puddlebun --screenshot street.png --shot 2
//!   puddlebun --audio rain.wav --seconds 15 --open

use crate::audio::{self, AudioEngine, AudioMsg, AudioShared};
use crate::camera::{self, CameraRig, HeadInput};
use crate::room::RoomSim;
use crate::scene::SceneKind;
use crate::gpu::Gpu;
use crate::render::{util, FrameData, Renderer};
use crate::settings::Settings;
use crate::weather::Weather;
use crate::{wgpu, Args};
use anyhow::{Context, Result};
use std::sync::Arc;

pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Renders one extra frame into an offscreen texture and reads it back.
pub fn capture_frame(renderer: &mut Renderer, fd: &FrameData) -> Result<Image> {
    let (w, h) = (renderer.width, renderer.height);
    let format = renderer.output_format;
    let (tex, view) = util::texture(
        &renderer.device,
        "capture",
        w,
        h,
        format,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    renderer.render(fd, &view, None);
    read_texture(&renderer.device, &renderer.queue, &tex, format)
}

pub fn read_texture(device: &wgpu::Device, queue: &wgpu::Queue, tex: &wgpu::Texture, format: wgpu::TextureFormat) -> Result<Image> {
    let (w, h) = (tex.width(), tex.height());
    let bpr = util::align_up(w as u64 * 4, 256) as u32;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("capture readback"),
        size: bpr as u64 * h as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("capture") });
    enc.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bpr), rows_per_image: Some(h) } },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buf.map_async(wgpu::MapMode::Read, .., move |r| {
        let _ = tx.send(r);
    });
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    rx.recv().context("readback channel closed")?.map_err(|e| anyhow::anyhow!("map failed: {e:?}"))?;
    let data = buf.get_mapped_range(..).map_err(|e| anyhow::anyhow!("{e:?}"))?;
    let bgra = matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
    let supported = bgra || matches!(format, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb);
    anyhow::ensure!(supported, "can't capture surface format {format:?}");
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as usize {
        let row = &data[y * bpr as usize..y * bpr as usize + w as usize * 4];
        for px in row.chunks_exact(4) {
            if bgra {
                rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
            } else {
                rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
    }
    drop(data);
    buf.unmap();
    Ok(Image { width: w, height: h, rgba })
}

pub fn save_png(path: &std::path::Path, img: &Image) -> Result<()> {
    let file = std::fs::File::create(path)?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width, img.height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header()?;
    writer.write_image_data(&img.rgba)?;
    Ok(())
}

pub fn run(args: &Args) -> Result<()> {
    let mut settings = Settings::default();
    settings.living_weather = false;
    if let Some(p) = args.particles {
        settings.max_particles = p;
    }
    if let Some(r) = args.rain {
        settings.intensity_mm_h = r;
    }
    if let Some(e) = args.exposure {
        settings.exposure_ev = e;
    }
    if let Some(v) = args.wind {
        settings.wind_speed = v;
    }
    if let Some(v) = args.stylise {
        settings.stylise = v;
    }
    if let Some(v) = args.muffle {
        settings.muffle = v;
    }
    if let Some(v) = args.city_life {
        settings.city_life = v;
    }
    settings.window_open = args.open;
    settings.sanitize();
    let (w, h) = args.size;
    let instance = Gpu::instance(None);
    let gpu = pollster::block_on(Gpu::new(instance, None))?;
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let mut renderer = Renderer::new(gpu.device.clone(), gpu.queue.clone(), format, w, h, &settings);
    let (tex, view) = util::texture(&gpu.device, "offscreen", w, h, format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);

    let mut weather = Weather::new(&settings);
    let mut cam = CameraRig::new();
    let spot = if args.spot == camera::SPOT_HIGH_RISE { camera::SPOT_NEON_WINDOW } else { args.spot.min(camera::SPOT_COUNT - 1) };
    let scene = camera::spot_scene(spot);
    let interior = scene == SceneKind::City;
    let mut room = RoomSim::new(renderer.window_geom(SceneKind::City).context("the city has no window")?);
    if interior {
        cam.sit_at_window(room.geom);
        room.prewarm(&weather.state, &settings);
    } else {
        cam.jump_to((spot - 2) as usize);
    }

    let fps = 30.0f32;
    let dt = 1.0 / fps;
    let frames = if args.audio.is_some() { (args.seconds * fps).ceil() as u32 } else { args.frames };

    // offline audio engine fed by the same GPU impacts
    let sr = 48_000u32;
    let shared = Arc::new(AudioShared::default());
    let (mut producer, consumer) = rtrb::RingBuffer::<AudioMsg>::new(1 << 16);
    let mut engine = AudioEngine::new(sr as f32, consumer, shared.clone());
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut rng = fastrand::Rng::with_seed(7);
    let mut total_impacts = 0u64;
    let mut total_voiced = 0u64;
    let started = std::time::Instant::now();

    for i in 0..frames {
        let t = args.time + i as f32 * dt;
        weather.update(dt, &settings);
        if interior {
            room.update(dt, t, &weather.state, &settings);
        }
        let mut cs = cam.update(dt, &Default::default(), &HeadInput::default(), 1e9, false, None);
        if let Some(c) = args.cam {
            let pos = glam::Vec3::new(c[0], c[1], c[2]);
            cs.pos = pos;
            cs.forward = (glam::Vec3::new(c[3], c[4], c[5]) - pos).normalize();
        }
        let fd = FrameData {
            camera: cs,
            time: t,
            dt,
            frame_index: i,
            weather: &weather.state,
            settings: &settings,
            scene,
            room: if interior { Some(room.state()) } else { None },
            fade: 0.0,
            cpu_splashes: if interior { room.splashes() } else { &[] },
            drips: if interior { room.drips() } else { &[] },
        };
        renderer.render(&fd, &view, None);
        if args.audio.is_some() {
            audio::update_shared(&shared, &settings, &weather.state, scene, interior, room.open(), 0.0);
            for imp in renderer.readback.wait_all(&gpu.device) {
                total_impacts += imp.total as u64;
                let voices = audio::spatialise(&imp.events, &cs, sr as f32, dt, &mut rng);
                total_voiced += voices.len() as u64;
                for v in voices {
                    let _ = producer.push(AudioMsg::Impact(v));
                }
            }
            if interior {
                for m in room.audio_msgs(&cs, sr as f32, dt, &mut rng) {
                    total_voiced += 1;
                    let _ = producer.push(m);
                }
            }
            let n = (sr as f32 * dt) as usize;
            let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
            engine.render(&mut l, &mut r);
            left.extend_from_slice(&l);
            right.extend_from_slice(&r);
        } else {
            let _ = renderer.readback.wait_all(&gpu.device);
        }
        if i % 10 == 0 {
            log::info!(
                "frame {i}/{frames} ({} drops, {} on the glass, {:.1}s elapsed)",
                renderer.stats.active_drops,
                if interior { room.glass.drop_count() } else { 0 },
                started.elapsed().as_secs_f32()
            );
        }
    }

    if let Some(path) = &args.screenshot {
        let img = read_texture(&gpu.device, &gpu.queue, &tex, format)?;
        save_png(path, &img)?;
        println!("saved {} ({}x{}, {})", path.display(), w, h, camera::spot_names()[spot as usize]);
    }
    if let Some(path) = &args.audio {
        audio::write_wav(path, sr, &left, &right)?;
        let peak = left.iter().chain(right.iter()).fold(0.0f32, |m, v| m.max(v.abs()));
        let rms = (left.iter().chain(right.iter()).map(|v| v * v).sum::<f32>() / (left.len() * 2).max(1) as f32).sqrt();
        let secs = left.len() as f32 / sr as f32;
        println!(
            "saved {} ({:.1}s) peak {:.3} rms {:.4} ({:.1} dBFS) · impacts {:.0}/s · voiced {:.0}/s",
            path.display(),
            secs,
            peak,
            rms,
            20.0 * rms.max(1e-9).log10(),
            total_impacts as f32 / secs,
            total_voiced as f32 / secs
        );
    }
    println!("{} frames in {:.1}s", frames, started.elapsed().as_secs_f32());
    Ok(())
}
