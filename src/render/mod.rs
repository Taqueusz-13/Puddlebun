//! The renderer: owns every GPU resource and records one frame:
//! compute (rain + ripples) → reflection pass → outside pass →
//! [glass blur → room + glass] → bloom/tonemap/stylise → UI.

pub mod glass;
pub mod post;
pub mod rain;
pub mod readback;
pub mod ripple;
pub mod util;

use crate::camera::CameraState;
use crate::city::{self, WindowGeom};
use crate::scene::{self, SceneKind, StaticLight, Tint, World};
use crate::settings::Settings;
use crate::weather::{self, WeatherState};
use crate::wgpu;
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use util::*;
use wgpu::util::DeviceExt;

pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const MAX_LIGHTS: usize = 64;
const MAX_LAMPS: usize = 40;

/// Must match `Frame` in common.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub struct FrameUniform {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub prev_view_proj: [[f32; 4]; 4],
    pub cam_pos: [f32; 4],
    pub cam_fwd: [f32; 4],
    pub cam_right: [f32; 4],
    pub cam_up: [f32; 4],
    pub screen: [f32; 4],
    pub time: [f32; 4],
    pub wind: [f32; 4],
    pub weather: [f32; 4],
    pub lamp: [f32; 4],
    pub sky: [f32; 4],
    pub ripple: [f32; 4],
    pub counts: [u32; 4],
    pub misc: [f32; 4],
    pub scene: [u32; 4],
    pub room: [f32; 4],
    pub win_a: [f32; 4],
    pub win_b: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct GpuLight {
    pub pos: [f32; 4],
    pub color: [f32; 4],
    pub dir: [f32; 4],
}

/// State of the window/room when looking out from inside.
pub struct RoomState<'a> {
    pub raise: f32,
    pub sill_wet: f32,
    pub glass: &'a crate::glass::GlassSim,
}

/// Everything needed to draw one frame.
pub struct FrameData<'a> {
    pub camera: CameraState,
    pub time: f32,
    pub dt: f32,
    pub frame_index: u32,
    pub weather: &'a WeatherState,
    pub settings: &'a Settings,
    pub scene: SceneKind,
    pub room: Option<RoomState<'a>>,
    pub fade: f32,
    /// first CPU_SPLASHES_OUTSIDE are outside, the rest inside the room
    pub cpu_splashes: &'a [rain::GpuSplash],
    pub drips: &'a [rain::GpuDrop],
}

pub struct EguiDraw<'a> {
    pub renderer: &'a mut egui_wgpu::Renderer,
    /// non-sRGB view of the output (egui blends in gamma space)
    pub view: &'a wgpu::TextureView,
    pub jobs: &'a [egui::ClippedPrimitive],
    pub screen: &'a egui_wgpu::ScreenDescriptor,
}

#[derive(Default, Clone, Copy, Debug)]
pub struct RenderStats {
    pub active_drops: u32,
    pub lights: u32,
    pub ripple_steps: u32,
}

struct Targets {
    outside: wgpu::Texture,
    outside_view: wgpu::TextureView,
    hdr: wgpu::Texture,
    hdr_view: wgpu::TextureView,
    depth: wgpu::TextureView,
    refl: wgpu::TextureView,
    refl_depth: wgpu::TextureView,
    refl_size: (u32, u32),
}

struct GpuMesh {
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    count: u32,
}

impl GpuMesh {
    fn new(device: &wgpu::Device, m: &scene::MeshBuilder, label: &str) -> Self {
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(&m.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(&m.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        Self { vbuf, ibuf, count: m.indices.len() as u32 }
    }
    fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_vertex_buffer(0, self.vbuf.slice(..));
        pass.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..self.count, 0, 0..1);
    }
}

struct GpuWorld {
    mesh: GpuMesh,
    room_mesh: Option<GpuMesh>,
    world: World,
}

pub struct Renderer {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub output_format: wgpu::TextureFormat,
    pub width: u32,
    pub height: u32,
    frame_buf: wgpu::Buffer,
    frame_refl_buf: wgpu::Buffer,
    lights_buf: wgpu::Buffer,
    frame_bg: wgpu::BindGroup,
    frame_refl_bg: wgpu::BindGroup,
    scene_bgl: wgpu::BindGroupLayout,
    scene_pipeline: wgpu::RenderPipeline,
    sky_pipeline: wgpu::RenderPipeline,
    worlds: Vec<GpuWorld>,
    scene_bg_main: Vec<wgpu::BindGroup>,
    scene_bg_refl: Vec<wgpu::BindGroup>,
    sampler: wgpu::Sampler,
    dummy: wgpu::TextureView,
    targets: Targets,
    pub rain: rain::Rain,
    pub ripple: ripple::Ripple,
    post: post::Post,
    glass: glass::GlassGpu,
    pub readback: readback::Readback,
    prev_view_proj: Mat4,
    reflection_scale: f32,
    pub stats: RenderStats,
}

impl Renderer {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, output_format: wgpu::TextureFormat, width: u32, height: u32, settings: &Settings) -> Self {
        let vfc = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT | wgpu::ShaderStages::COMPUTE;
        let frame_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame bgl"),
            entries: &[uniform_entry(0, vfc, false), storage_entry(1, vfc, true)],
        });
        let mk_uniform = |label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: std::mem::size_of::<FrameUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let frame_buf = mk_uniform("frame");
        let frame_refl_buf = mk_uniform("frame (reflection)");
        let lights_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lights"),
            size: (MAX_LIGHTS * std::mem::size_of::<GpuLight>()) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mk_frame_bg = |buf: &wgpu::Buffer| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("frame bg"),
                layout: &frame_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: lights_buf.as_entire_binding() },
                ],
            })
        };
        let frame_bg = mk_frame_bg(&frame_buf);
        let frame_refl_bg = mk_frame_bg(&frame_refl_buf);

        let f = wgpu::ShaderStages::FRAGMENT;
        let scene_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("scene bgl"),
            entries: &[texture_entry(0, f), sampler_entry(1, f), storage_entry(2, f, true)],
        });
        let scene_module = shader(
            &device,
            "scene",
            &[COMMON_WGSL, WORLD_WGSL, include_str!("../shaders/scene.wgsl"), include_str!("../shaders/city.wgsl")],
        );
        let scene_layout = layout(&device, "scene layout", &[&frame_bgl, &scene_bgl]);
        let vertex_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<scene::Vertex>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x3, 1 => Uint32, 2 => Float32x3, 3 => Float32, 4 => Float32x2, 5 => Float32x2
            ],
        };
        let scene_pipeline = render_pipeline(
            &device,
            "scene",
            &scene_layout,
            &scene_module,
            "vs_main",
            "fs_main",
            &[Some(vertex_layout)],
            HDR_FORMAT,
            None,
            Some(depth_state(true, wgpu::CompareFunction::Greater)),
        );
        let sky_pipeline = render_pipeline(
            &device,
            "sky",
            &scene_layout,
            &scene_module,
            "vs_sky",
            "fs_sky",
            &[],
            HDR_FORMAT,
            None,
            Some(depth_state(false, wgpu::CompareFunction::GreaterEqual)),
        );

        let mut worlds = Vec::new();
        for w in [scene::build_street(), city::build_city()] {
            let mesh = GpuMesh::new(&device, &w.mesh, "world mesh");
            let room_mesh = w.room.as_ref().map(|r| GpuMesh::new(&device, &r.mesh, "room mesh"));
            log::info!("{:?}: {} vertices, {} triangles, {} lights", w.kind, w.mesh.vertices.len(), w.mesh.indices.len() / 3, w.lights.len());
            worlds.push(GpuWorld { mesh, room_mesh, world: w });
        }

        let ripple = ripple::Ripple::new(&device);
        let rain = rain::Rain::new(&device, &frame_bgl, &ripple.impulses, settings.max_particles);
        let readback = readback::Readback::new(&device);
        let post = post::Post::new(&device, output_format);
        let glass = glass::GlassGpu::new(&device, &frame_bgl);
        let sampler = linear_sampler(&device, wgpu::AddressMode::ClampToEdge);
        let (_dt, dummy) = texture(&device, "dummy", 1, 1, HDR_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING);

        let targets = Self::make_targets(&device, width, height, settings.reflection_scale);
        let mut r = Self {
            device,
            queue,
            output_format,
            width,
            height,
            frame_buf,
            frame_refl_buf,
            lights_buf,
            frame_bg,
            frame_refl_bg,
            scene_bgl,
            scene_pipeline,
            sky_pipeline,
            worlds,
            scene_bg_main: Vec::new(),
            scene_bg_refl: Vec::new(),
            sampler,
            dummy,
            targets,
            rain,
            ripple,
            post,
            glass,
            readback,
            prev_view_proj: Mat4::IDENTITY,
            reflection_scale: settings.reflection_scale,
            stats: RenderStats::default(),
        };
        r.rebuild_size_dependent();
        r
    }

    fn make_targets(device: &wgpu::Device, w: u32, h: u32, refl_scale: f32) -> Targets {
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let (outside, outside_view) = texture(device, "outside", w, h, HDR_FORMAT, rt | wgpu::TextureUsages::COPY_SRC);
        let (hdr, hdr_view) = texture(device, "hdr", w, h, HDR_FORMAT, rt | wgpu::TextureUsages::COPY_DST);
        let (_b, depth) = texture(device, "depth", w, h, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let rw = ((w as f32 * refl_scale) as u32).max(1);
        let rh = ((h as f32 * refl_scale) as u32).max(1);
        let (_c, refl) = texture(device, "reflection", rw, rh, HDR_FORMAT, rt);
        let (_d, refl_depth) = texture(device, "reflection depth", rw, rh, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        Targets { outside, outside_view, hdr, hdr_view, depth, refl, refl_depth, refl_size: (rw, rh) }
    }

    fn rebuild_size_dependent(&mut self) {
        let mk = |view: &wgpu::TextureView, ripple: &wgpu::Buffer| {
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("scene bg"),
                layout: &self.scene_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: ripple.as_entire_binding() },
                ],
            })
        };
        self.scene_bg_main = (0..2).map(|i| mk(&self.targets.refl, &self.ripple.heights[i])).collect();
        self.scene_bg_refl = (0..2).map(|i| mk(&self.dummy, &self.ripple.heights[i])).collect();
        self.post.resize(&self.device, &self.targets.hdr_view, &self.targets.outside_view, self.width, self.height);
        self.glass.rebind(&self.device, &self.targets.outside_view, self.post.blurred_outside());
    }

    pub fn resize(&mut self, w: u32, h: u32, reflection_scale: f32) {
        let (w, h) = (w.max(1), h.max(1));
        if w == self.width && h == self.height && (reflection_scale - self.reflection_scale).abs() < 1e-3 {
            return;
        }
        self.width = w;
        self.height = h;
        self.reflection_scale = reflection_scale;
        self.targets = Self::make_targets(&self.device, w, h, reflection_scale);
        self.rebuild_size_dependent();
    }

    pub fn window_geom(&self, kind: SceneKind) -> Option<WindowGeom> {
        self.worlds.iter().find(|w| w.world.kind == kind).and_then(|w| w.world.room.as_ref().map(|r| r.geom))
    }

    fn light_color(l: &StaticLight, s: &Settings, time: f32) -> Vec3 {
        let base = match l.tint {
            Tint::Fixed => l.color,
            Tint::StreetLamp => weather::kelvin_to_rgb(s.lamp_kelvin) * s.lamp_intensity * 420.0,
            Tint::CityLamp => weather::kelvin_to_rgb(4300.0) * s.lamp_intensity * 380.0,
        };
        if l.flicker != 0 { base * scene::neon_flicker(l.flicker, time) } else { base }
    }

    fn to_gpu(l: &StaticLight, c: Vec3) -> GpuLight {
        GpuLight {
            pos: [l.pos.x, l.pos.y, l.pos.z, l.radius],
            color: [c.x, c.y, c.z, l.kind as f32],
            dir: [l.dir.x, l.dir.y, l.dir.z, l.cos_cut],
        }
    }

    /// Nearest lamps/neon first (they light the rain and glow in the mist),
    /// then fill lights and window spill, then the room lamps.
    fn build_lights(&self, gw: &GpuWorld, f: &FrameData) -> (Vec<GpuLight>, u32) {
        let s = f.settings;
        let cam = f.camera.pos;
        let w = &gw.world;
        let mut lamps: Vec<(f32, &StaticLight)> = w.lights.iter().map(|l| (l.pos.distance(cam) - l.radius * 0.5, l)).collect();
        lamps.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<GpuLight> = lamps.iter().take(MAX_LAMPS).map(|(_, l)| Self::to_gpu(l, Self::light_color(l, s, f.time))).collect();
        let lamp_count = out.len() as u32;
        let room_lamps: Vec<&StaticLight> = if f.room.is_some() { w.room.as_ref().map(|r| r.lamps.iter().collect()).unwrap_or_default() } else { Vec::new() };
        let budget = MAX_LIGHTS - out.len() - room_lamps.len();
        let mut fills: Vec<(f32, GpuLight)> = w
            .fill_lights
            .iter()
            .filter_map(|l| {
                let d = l.pos.distance(cam);
                (d < 70.0).then(|| (d, Self::to_gpu(l, Self::light_color(l, s, f.time))))
            })
            .collect();
        let warm = weather::kelvin_to_rgb(2700.0);
        for win in w.windows.iter().filter(|w| scene::window_is_lit(w.house, w.window, s.window_lights)) {
            let d = win.pos.distance(cam);
            if d > 55.0 {
                continue;
            }
            let c = warm * if win.window == 0 { 2.6 } else { 1.6 };
            fills.push((d, GpuLight {
                pos: [win.pos.x, win.pos.y, win.pos.z, 9.0],
                color: [c.x, c.y, c.z, 1.0],
                dir: [win.normal.x, win.normal.y - 0.25, win.normal.z, 0.0],
            }));
        }
        fills.sort_by(|a, b| a.0.total_cmp(&b.0));
        out.extend(fills.into_iter().take(budget).map(|(_, l)| l));
        for l in room_lamps {
            out.push(Self::to_gpu(l, l.color));
        }
        (out, lamp_count)
    }

    #[allow(clippy::too_many_arguments)]
    fn frame_uniform(&self, cam: &CameraState, f: &FrameData, gw: &GpuWorld, w: u32, h: u32, reflection: bool, n_lights: u32, lamps: u32, active: u32, prev: Mat4) -> FrameUniform {
        let aspect = w as f32 / h as f32;
        let vp = cam.proj(aspect) * cam.view();
        let focal = (h as f32 * 0.5) / (cam.fov_y * 0.5).tan();
        let s = f.settings;
        let lamp = weather::kelvin_to_rgb(s.lamp_kelvin);
        let glow = gw.world.sky * s.sky_glow;
        let ws = f.weather;
        let right = cam.right();
        let up = cam.up();
        let (scene_id, interior) = (if gw.world.kind == SceneKind::City { 1 } else { 0 }, f.room.is_some() as u32);
        let (room, win_a, win_b) = match (&f.room, gw.world.room.as_ref()) {
            (Some(rs), Some(r)) => {
                let g = &r.geom;
                (
                    [rs.raise, rs.glass.cond, s.stylise, rs.sill_wet],
                    [g.wall_x, g.z_min, g.z_max, g.y_min],
                    [g.y_max, g.glass_x, g.sill_top, r.lamps.len() as f32],
                )
            }
            _ => ([0.0, 0.0, s.stylise, 0.0], [0.0; 4], [0.0; 4]),
        };
        FrameUniform {
            view_proj: vp.to_cols_array_2d(),
            inv_view_proj: vp.inverse().to_cols_array_2d(),
            prev_view_proj: prev.to_cols_array_2d(),
            cam_pos: [cam.pos.x, cam.pos.y, cam.pos.z, 0.0],
            cam_fwd: [cam.forward.x, cam.forward.y, cam.forward.z, focal],
            cam_right: [right.x, right.y, right.z, 0.0],
            cam_up: [up.x, up.y, up.z, 0.0],
            screen: [w as f32, h as f32, 1.0 / w as f32, 1.0 / h as f32],
            time: [f.time, f.dt, f.frame_index as f32, if reflection { 1.0 } else { 0.0 }],
            wind: [ws.wind.x, ws.wind.y, ws.wind.z, ws.gust],
            weather: [ws.intensity, ws.wetness, ws.puddle_level, s.fog],
            lamp: [lamp.x, lamp.y, lamp.z, s.lamp_intensity],
            sky: [glow.x, glow.y, glow.z, 1.0],
            ripple: [ripple::TEXEL, ripple::RES as f32, ripple::TEXEL * ripple::RES as f32, s.ripple_strength],
            counts: [n_lights, lamps, active, self.rain.max_particles],
            misc: [2f32.powf(s.exposure_ev), s.window_lights, s.bloom, 0.0],
            scene: [scene_id, interior, 0, 0],
            room,
            win_a,
            win_b,
        }
    }

    /// How many particles to simulate for the current rain rate. Real rain has
    /// far more drops than we can afford; the *relative* numbers follow
    /// Marshall–Palmer exactly, the absolute count is scaled to the budget.
    pub fn active_drops(&self, weather: &WeatherState, box_volume: f32) -> u32 {
        let n = weather::number_density(weather.intensity, weather::D_MIN_VISIBLE, weather::D_MAX);
        let want = n * box_volume * 0.045;
        (want as u32).min(self.rain.max_particles)
    }

    fn color_attachment(view: &wgpu::TextureView, load: wgpu::LoadOp<wgpu::Color>) -> Option<wgpu::RenderPassColorAttachment<'_>> {
        Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load, store: wgpu::StoreOp::Store } })
    }

    fn depth_attachment(view: &wgpu::TextureView) -> Option<wgpu::RenderPassDepthStencilAttachment<'_>> {
        Some(wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
            stencil_ops: None,
        })
    }

    /// Records and submits a whole frame into `target`.
    pub fn render(&mut self, f: &FrameData, target: &wgpu::TextureView, egui: Option<EguiDraw>) {
        let (w, h) = (self.width, self.height);
        let cam = f.camera;
        let s = f.settings;
        let wi = self.worlds.iter().position(|g| g.world.kind == f.scene).unwrap_or(0);
        let interior = f.room.is_some() && self.worlds[wi].room_mesh.is_some();

        // rain volume: a 28 m box ahead of the camera. From the window it's a
        // tighter box between our facade and the one opposite, a bit denser:
        // nothing further away is visible through the rain anyway.
        let (half, boost) = if interior { (10.0, 2.2) } else { (14.0, 1.0) };
        let fwd_flat = Vec3::new(cam.forward.x, 0.0, cam.forward.z).normalize_or(Vec3::Z);
        let mut center = cam.pos + fwd_flat * 8.0;
        let mut x_limit = 1.0e6;
        if interior {
            x_limit = city::C_FACADE;
            center.x = center.x.min(x_limit - half);
        }
        let top = (cam.pos.y + 14.0).max(16.0);
        let volume = (2.0 * half) * (2.0 * half) * top;
        let active = self.active_drops(f.weather, volume * boost);

        let (lights, lamps) = self.build_lights(&self.worlds[wi], f);
        let n_lights = lights.len() as u32;
        self.queue.write_buffer(&self.lights_buf, 0, bytemuck::cast_slice(&lights));

        let fu = self.frame_uniform(&cam, f, &self.worlds[wi], w, h, false, n_lights, lamps, active, self.prev_view_proj);
        let (rw, rh) = self.targets.refl_size;
        let fr = self.frame_uniform(&cam.mirrored(), f, &self.worlds[wi], rw, rh, true, n_lights, lamps, active, Mat4::IDENTITY);
        self.queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&fu));
        self.queue.write_buffer(&self.frame_refl_buf, 0, bytemuck::bytes_of(&fr));
        self.prev_view_proj = Mat4::from_cols_array_2d(&fu.view_proj);

        let lambda = weather::mp_lambda(f.weather.intensity);
        let sim = rain::SimParams {
            box_center: [center.x, 0.0, center.z, half],
            box_y: [0.0, top, s.shutter, f.dt.clamp(0.0, 0.05)],
            dsd: [lambda, weather::D_MIN_VISIBLE, weather::D_MAX, x_limit],
            counts: [active, scene::hash_u(f.frame_index.wrapping_mul(2_654_435_761)), rain::SPLASH_CAPACITY, rain::AUDIO_CAPACITY],
            audio: [12.0, 0.7, 30.0, 220.0 * s.ripple_strength],
        };
        self.queue.write_buffer(&self.rain.sim_buf, 0, bytemuck::bytes_of(&sim));
        // reset per-frame counters (keep the splash ring head)
        self.queue.write_buffer(&self.rain.counters, 4, bytemuck::cast_slice(&[0u32; 3]));
        // CPU-driven drips and splashes
        let n_drips = f.drips.len().min(rain::CPU_DROPS as usize);
        if n_drips > 0 {
            self.queue.write_buffer(&self.rain.drops, self.rain.max_particles as u64 * rain::DROP_STRIDE, bytemuck::cast_slice(&f.drips[..n_drips]));
        }
        let n_spl = f.cpu_splashes.len().min(rain::CPU_SPLASHES as usize);
        if n_spl > 0 {
            self.queue.write_buffer(&self.rain.cpu_splashes, 0, bytemuck::cast_slice(&f.cpu_splashes[..n_spl]));
        }
        if let (Some(rs), Some(room)) = (&f.room, self.worlds[wi].world.room.as_ref()) {
            let lamp = room.lamps.first().map(|l| (l.pos, l.color)).unwrap_or((Vec3::ZERO, Vec3::ZERO));
            self.glass.upload(&self.queue, rs.glass, &room.geom, rs.raise, f.time, lamp.0, lamp.1);
        }

        let flux = weather::number_flux(f.weather.intensity, 0.3, weather::D_MAX) * 0.8;
        let steps = self.ripple.prepare(&self.queue, f.dt, flux, lambda, s.ripple_strength);
        self.post.update(&self.queue, 2f32.powf(s.exposure_ev), s.bloom * 0.08, f.time, s.stylise, f.fade);

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("simulation"), timestamp_writes: None });
            self.rain.dispatch(&mut cpass, &self.frame_bg, active);
            self.ripple.dispatch(&mut cpass, steps);
        }
        self.readback.encode(&mut encoder, &self.rain.counters, &self.rain.audio_events, f.dt);
        let cur = self.ripple.cur;
        let gw = &self.worlds[wi];

        // reflection pass (mirrored camera, reduced resolution)
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("reflection"),
                color_attachments: &[Self::color_attachment(&self.targets.refl, wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
                depth_stencil_attachment: Self::depth_attachment(&self.targets.refl_depth),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_refl_bg, &[]);
            pass.set_bind_group(1, &self.scene_bg_refl[cur], &[]);
            pass.set_pipeline(&self.scene_pipeline);
            gw.mesh.draw(&mut pass);
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
        }

        // outside pass
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("outside"),
                color_attachments: &[Self::color_attachment(&self.targets.outside_view, wgpu::LoadOp::Clear(wgpu::Color::BLACK))],
                depth_stencil_attachment: Self::depth_attachment(&self.targets.depth),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_bg, &[]);
            pass.set_bind_group(1, &self.scene_bg_main[cur], &[]);
            pass.set_pipeline(&self.scene_pipeline);
            gw.mesh.draw(&mut pass);
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
            self.rain.draw(&mut pass, &self.frame_bg, active, n_drips as u32);
            self.rain.draw_cpu_splashes(&mut pass, &self.frame_bg, 0..(n_spl as u32).min(rain::CPU_SPLASHES_OUTSIDE));
        }

        if interior {
            self.post.encode_blur(&mut encoder);
        }
        encoder.copy_texture_to_texture(
            self.targets.outside.as_image_copy(),
            self.targets.hdr.as_image_copy(),
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );

        if interior {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("room"),
                color_attachments: &[Self::color_attachment(&self.targets.hdr_view, wgpu::LoadOp::Load)],
                depth_stencil_attachment: Self::depth_attachment(&self.targets.depth),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_bg, &[]);
            pass.set_bind_group(1, &self.scene_bg_refl[cur], &[]);
            pass.set_pipeline(&self.scene_pipeline);
            if let Some(rm) = &gw.room_mesh {
                rm.draw(&mut pass);
            }
            self.glass.draw(&mut pass, &self.frame_bg);
            if n_spl as u32 > rain::CPU_SPLASHES_OUTSIDE {
                self.rain.draw_cpu_splashes(&mut pass, &self.frame_bg, rain::CPU_SPLASHES_OUTSIDE..n_spl as u32);
            }
        }

        self.post.encode(&mut encoder, target);

        let mut extra_cmds = Vec::new();
        if let Some(e) = egui {
            extra_cmds = e.renderer.update_buffers(&self.device, &self.queue, &mut encoder, e.jobs, e.screen);
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Self::color_attachment(e.view, wgpu::LoadOp::Load)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut pass = pass.forget_lifetime();
            e.renderer.render(&mut pass, e.jobs, e.screen);
        }

        extra_cmds.push(encoder.finish());
        self.queue.submit(extra_cmds);
        self.readback.after_submit();

        self.stats = RenderStats { active_drops: active, lights: n_lights, ripple_steps: steps as u32 };
    }
}
