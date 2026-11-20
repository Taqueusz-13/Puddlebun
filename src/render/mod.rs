//! The renderer: owns every GPU resource and records one frame:
//! compute (rain + ripples) → reflection pass → main pass → bloom/tonemap → UI.

pub mod post;
pub mod rain;
pub mod readback;
pub mod ripple;
pub mod util;

use crate::camera::CameraState;
use crate::scene::{self, Street};
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
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default, Debug)]
pub struct GpuLight {
    pub pos: [f32; 4],
    pub color: [f32; 4],
    pub dir: [f32; 4],
}

/// Everything needed to draw one frame.
pub struct FrameData<'a> {
    pub camera: CameraState,
    pub time: f32,
    pub dt: f32,
    pub frame_index: u32,
    pub weather: &'a WeatherState,
    pub settings: &'a Settings,
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
    hdr: wgpu::TextureView,
    depth: wgpu::TextureView,
    refl: wgpu::TextureView,
    refl_depth: wgpu::TextureView,
    refl_size: (u32, u32),
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
    vbuf: wgpu::Buffer,
    ibuf: wgpu::Buffer,
    index_count: u32,
    scene_bg_main: Vec<wgpu::BindGroup>,
    scene_bg_refl: Vec<wgpu::BindGroup>,
    sampler: wgpu::Sampler,
    dummy: wgpu::TextureView,
    targets: Targets,
    pub rain: rain::Rain,
    pub ripple: ripple::Ripple,
    post: post::Post,
    pub readback: readback::Readback,
    street: Street,
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
        let scene_module = shader(&device, "scene", &[COMMON_WGSL, WORLD_WGSL, include_str!("../shaders/scene.wgsl")]);
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

        let street = scene::build_street();
        let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("street vertices"),
            contents: bytemuck::cast_slice(&street.mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let ibuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("street indices"),
            contents: bytemuck::cast_slice(&street.mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });
        let index_count = street.mesh.indices.len() as u32;
        log::info!("street: {} vertices, {} triangles, {} lamps", street.mesh.vertices.len(), index_count / 3, street.lamps.len());

        let ripple = ripple::Ripple::new(&device);
        let rain = rain::Rain::new(&device, &frame_bgl, &ripple.impulses, settings.max_particles);
        let readback = readback::Readback::new(&device);
        let post = post::Post::new(&device, output_format);
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
            vbuf,
            ibuf,
            index_count,
            scene_bg_main: Vec::new(),
            scene_bg_refl: Vec::new(),
            sampler,
            dummy,
            targets,
            rain,
            ripple,
            post,
            readback,
            street,
            prev_view_proj: Mat4::IDENTITY,
            reflection_scale: settings.reflection_scale,
            stats: RenderStats::default(),
        };
        r.rebuild_size_dependent();
        r
    }

    fn make_targets(device: &wgpu::Device, w: u32, h: u32, refl_scale: f32) -> Targets {
        let rt = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let (_a, hdr) = texture(device, "hdr", w, h, HDR_FORMAT, rt);
        let (_b, depth) = texture(device, "depth", w, h, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        let rw = ((w as f32 * refl_scale) as u32).max(1);
        let rh = ((h as f32 * refl_scale) as u32).max(1);
        let (_c, refl) = texture(device, "reflection", rw, rh, HDR_FORMAT, rt);
        let (_d, refl_depth) = texture(device, "reflection depth", rw, rh, DEPTH_FORMAT, wgpu::TextureUsages::RENDER_ATTACHMENT);
        Targets { hdr, depth, refl, refl_depth, refl_size: (rw, rh) }
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
        self.post.resize(&self.device, &self.targets.hdr, self.width, self.height);
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

    /// Street lamps plus the nearest lit windows, in the GPU format.
    fn build_lights(&self, f: &FrameData) -> (Vec<GpuLight>, u32) {
        let s = f.settings;
        let lamp_rgb = weather::kelvin_to_rgb(s.lamp_kelvin) * s.lamp_intensity * 420.0;
        let mut lights: Vec<GpuLight> = self
            .street
            .lamps
            .iter()
            .map(|l| GpuLight {
                pos: [l.pos.x, l.pos.y, l.pos.z, 38.0],
                color: [lamp_rgb.x, lamp_rgb.y, lamp_rgb.z, 0.0],
                dir: [0.0, -1.0, 0.0, 0.2],
            })
            .collect();
        let lamp_count = lights.len() as u32;
        let cam = f.camera.pos;
        let warm = weather::kelvin_to_rgb(2700.0);
        let mut wins: Vec<(f32, GpuLight)> = self
            .street
            .windows
            .iter()
            .filter(|w| scene::window_is_lit(w.house, w.window, s.window_lights))
            .filter_map(|w| {
                let d = w.pos.distance(cam);
                if d > 55.0 {
                    return None;
                }
                let strength = if w.window == 0 { 2.6 } else { 1.6 };
                let c = warm * strength;
                Some((d, GpuLight {
                    pos: [w.pos.x, w.pos.y, w.pos.z, 9.0],
                    color: [c.x, c.y, c.z, 1.0],
                    dir: [w.normal.x, w.normal.y - 0.25, w.normal.z, 0.0],
                }))
            })
            .collect();
        wins.sort_by(|a, b| a.0.total_cmp(&b.0));
        for (_, l) in wins.into_iter().take(MAX_LIGHTS - lights.len()) {
            lights.push(l);
        }
        (lights, lamp_count)
    }

    fn frame_uniform(&self, cam: &CameraState, f: &FrameData, w: u32, h: u32, reflection: bool, n_lights: u32, lamps: u32, active: u32, prev: Mat4) -> FrameUniform {
        let aspect = w as f32 / h as f32;
        let vp = cam.proj(aspect) * cam.view();
        let focal = (h as f32 * 0.5) / (cam.fov_y * 0.5).tan();
        let s = f.settings;
        let lamp = weather::kelvin_to_rgb(s.lamp_kelvin);
        let glow = Vec3::new(1.0, 0.62, 0.4) * 0.034 * s.sky_glow;
        let ws = f.weather;
        let right = cam.right();
        let up = cam.up();
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

    /// Records and submits a whole frame into `target`.
    pub fn render(&mut self, f: &FrameData, target: &wgpu::TextureView, egui: Option<EguiDraw>) {
        let (w, h) = (self.width, self.height);
        let cam = f.camera;
        let s = f.settings;

        // rain volume: a 28 m box centred a little ahead of the camera
        let half = 14.0;
        let fwd_flat = Vec3::new(cam.forward.x, 0.0, cam.forward.z).normalize_or(Vec3::Z);
        let center = cam.pos + fwd_flat * 8.0;
        let top = (cam.pos.y + 14.0).max(16.0);
        let volume = (2.0 * half) * (2.0 * half) * top;
        let active = self.active_drops(f.weather, volume);

        let (lights, lamps) = self.build_lights(f);
        let n_lights = lights.len() as u32;
        self.queue.write_buffer(&self.lights_buf, 0, bytemuck::cast_slice(&lights));

        let fu = self.frame_uniform(&cam, f, w, h, false, n_lights, lamps, active, self.prev_view_proj);
        let (rw, rh) = self.targets.refl_size;
        let fr = self.frame_uniform(&cam.mirrored(), f, rw, rh, true, n_lights, lamps, active, Mat4::IDENTITY);
        self.queue.write_buffer(&self.frame_buf, 0, bytemuck::bytes_of(&fu));
        self.queue.write_buffer(&self.frame_refl_buf, 0, bytemuck::bytes_of(&fr));
        self.prev_view_proj = Mat4::from_cols_array_2d(&fu.view_proj);

        let lambda = weather::mp_lambda(f.weather.intensity);
        let sim = rain::SimParams {
            box_center: [center.x, 0.0, center.z, half],
            box_y: [0.0, top, s.shutter, f.dt.clamp(0.0, 0.05)],
            dsd: [lambda, weather::D_MIN_VISIBLE, weather::D_MAX, 0.0],
            counts: [active, scene::hash_u(f.frame_index.wrapping_mul(2_654_435_761)), rain::SPLASH_CAPACITY, rain::AUDIO_CAPACITY],
            audio: [12.0, 0.7, 30.0, 220.0 * s.ripple_strength],
        };
        self.queue.write_buffer(&self.rain.sim_buf, 0, bytemuck::bytes_of(&sim));
        // reset per-frame counters (keep the splash ring head)
        self.queue.write_buffer(&self.rain.counters, 4, bytemuck::cast_slice(&[0u32; 3]));

        let flux = weather::number_flux(f.weather.intensity, 0.3, weather::D_MAX) * 0.8;
        let steps = self.ripple.prepare(&self.queue, f.dt, flux, lambda, s.ripple_strength);
        self.post.update(&self.queue, 2f32.powf(s.exposure_ev), s.bloom * 0.08, f.time);

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("simulation"), timestamp_writes: None });
            self.rain.dispatch(&mut cpass, &self.frame_bg, active);
            self.ripple.dispatch(&mut cpass, steps);
        }
        self.readback.encode(&mut encoder, &self.rain.counters, &self.rain.audio_events, f.dt);
        let cur = self.ripple.cur;

        // reflection pass (mirrored camera, half resolution)
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("reflection"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.refl,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.refl_depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_refl_bg, &[]);
            pass.set_bind_group(1, &self.scene_bg_refl[cur], &[]);
            pass.set_pipeline(&self.scene_pipeline);
            pass.set_vertex_buffer(0, self.vbuf.slice(..));
            pass.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.index_count, 0, 0..1);
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
        }

        // main pass
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.hdr,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.targets.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(0.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_bind_group(0, &self.frame_bg, &[]);
            pass.set_bind_group(1, &self.scene_bg_main[cur], &[]);
            pass.set_pipeline(&self.scene_pipeline);
            pass.set_vertex_buffer(0, self.vbuf.slice(..));
            pass.set_index_buffer(self.ibuf.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.index_count, 0, 0..1);
            pass.set_pipeline(&self.sky_pipeline);
            pass.draw(0..3, 0..1);
            self.rain.draw(&mut pass, &self.frame_bg, active);
        }

        self.post.encode(&mut encoder, target);

        let mut extra_cmds = Vec::new();
        if let Some(e) = egui {
            extra_cmds = e.renderer.update_buffers(&self.device, &self.queue, &mut encoder, e.jobs, e.screen);
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: e.view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
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
