//! The windowed app: winit event loop, frame pacing, input, UI and audio glue.

use crate::audio::{self, AudioMsg, AudioOutput, AudioShared};
use crate::camera::{self, CamMode, CameraRig, HeadInput, MoveInput};
use crate::gpu::Gpu;
use crate::render::{EguiDraw, FrameData, Renderer};
use crate::room::RoomSim;
use crate::scene::SceneKind;
use crate::settings::Settings;
use crate::ui;
use crate::weather::Weather;
use crate::{wgpu, winit, Args};
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Fullscreen, Window, WindowId};

pub fn run(args: Args) -> Result<()> {
    let event_loop = EventLoop::new().context("couldn't create the event loop (no display?)")?;
    let mut app = App { args, settings: Settings::load(), running: None, error: None };
    if app.args.reset_settings {
        app.settings = Settings::default();
    }
    if let Some(p) = app.args.particles {
        app.settings.max_particles = p;
    }
    event_loop.run_app(&mut app)?;
    app.settings.save();
    if let Some(e) = app.error {
        return Err(e);
    }
    Ok(())
}

struct App {
    args: Args,
    settings: Settings,
    running: Option<Running>,
    error: Option<anyhow::Error>,
}

struct Running {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    renderer: Renderer,
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_rend: egui_wgpu::Renderer,
    audio: Option<AudioOutput>,
    audio_shared: Arc<AudioShared>,
    weather: Weather,
    cam: CameraRig,
    keys: HashSet<KeyCode>,
    rmb: bool,
    look: (f32, f32),
    last: Instant,
    next_frame: Instant,
    time: f32,
    frame: u32,
    focused: bool,
    occluded: bool,
    minimized: bool,
    stats: ui::Stats,
    last_save: Instant,
    saved: Settings,
    rng: fastrand::Rng,
    screenshot: bool,
    /// where we are (see camera::spot_scene)
    spot: u32,
    scene: SceneKind,
    transition: Option<Transition>,
    room: RoomSim,
    cursor: Option<glam::Vec2>,
    lean_scroll: f32,
    auto_timer: f32,
    toast: Option<(String, f32)>,
}

/// Fade to black, change place, fade back in.
struct Transition {
    target: u32,
    t: f32,
    switched: bool,
}

const FADE_OUT: f32 = 0.45;
const FADE_IN: f32 = 0.6;

impl App {
    fn init(&mut self, el: &ActiveEventLoop) -> Result<()> {
        let mut attrs = Window::default_attributes()
            .with_title("Puddlebun")
            .with_inner_size(winit::dpi::LogicalSize::new(1600.0, 900.0));
        #[cfg(target_os = "linux")]
        {
            use winit::platform::wayland::WindowAttributesExtWayland;
            attrs = attrs.with_name("puddlebun", "puddlebun");
        }
        let window = Arc::new(el.create_window(attrs)?);
        let instance = Gpu::instance(Some(Box::new(el.owned_display_handle())));
        let surface = instance.create_surface(window.clone())?;
        let gpu = pollster::block_on(Gpu::new(instance.clone(), Some(&surface)))?;
        let caps = surface.get_capabilities(&gpu.adapter);
        let format = caps.formats.iter().copied().find(|f| f.is_srgb()).unwrap_or(caps.formats[0]);
        let size = window.inner_size();
        let mut config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .context("the window surface isn't supported by this GPU")?;
        config.format = format;
        let ui_format = format.remove_srgb_suffix();
        config.view_formats = if ui_format != format { vec![ui_format] } else { vec![] };
        config.present_mode = wgpu::PresentMode::AutoVsync;
        config.desired_maximum_frame_latency = 2;
        surface.configure(&gpu.device, &config);

        let renderer = Renderer::new(gpu.device.clone(), gpu.queue.clone(), format, config.width, config.height, &self.settings);
        let egui_ctx = egui::Context::default();
        ui::style(&egui_ctx);
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &*window,
            Some(window.scale_factor() as f32),
            None,
            Some(gpu.device.limits().max_texture_dimension_2d as usize),
        );
        let egui_rend = egui_wgpu::Renderer::new(&gpu.device, ui_format, egui_wgpu::RendererOptions::default());

        let audio_shared = Arc::new(AudioShared::default());
        let audio = match audio::start_output(audio_shared.clone()) {
            Ok(a) => Some(a),
            Err(e) => {
                log::warn!("no audio: {e:#}");
                None
            }
        };
        let info = gpu.adapter.get_info();
        let mut stats = ui::Stats { gpu: info.name.clone(), spot_names: camera::spot_names(), ..Default::default() };
        stats.audio_device = audio.as_ref().map(|a| format!("{:.0} Hz audio", a.sample_rate)).unwrap_or_else(|| "no audio device".into());

        let geom = renderer.window_geom(SceneKind::City).context("the city has no window")?;
        let weather = Weather::new(&self.settings);
        let room = RoomSim::new(geom);
        let spot = if self.settings.spot == camera::SPOT_HIGH_RISE { camera::SPOT_NEON_WINDOW } else { self.settings.spot };
        let now = Instant::now();
        self.running = Some(Running {
            window,
            instance,
            surface,
            config,
            renderer,
            egui_ctx,
            egui_state,
            egui_rend,
            audio,
            audio_shared,
            weather,
            cam: CameraRig::new(),
            keys: HashSet::new(),
            rmb: false,
            look: (0.0, 0.0),
            last: now,
            next_frame: now,
            time: 0.0,
            frame: 0,
            focused: true,
            occluded: false,
            minimized: false,
            stats,
            last_save: now,
            saved: self.settings.clone(),
            rng: fastrand::Rng::new(),
            screenshot: false,
            spot,
            scene: camera::spot_scene(spot),
            transition: None,
            room,
            cursor: None,
            lean_scroll: 0.0,
            auto_timer: 0.0,
            toast: None,
        });
        let r = self.running.as_mut().unwrap();
        r.arrive(spot, &mut self.settings);
        Ok(())
    }
}

impl Running {
    fn reconfigure(&mut self) {
        let size = self.window.inner_size();
        if size.width == 0 || size.height == 0 {
            self.minimized = true;
            return;
        }
        self.minimized = false;
        self.config.width = size.width;
        self.config.height = size.height;
        self.surface.configure(&self.renderer.device, &self.config);
    }

    /// Gets the next swapchain image, recovering from lost/outdated surfaces.
    fn acquire(&mut self) -> Option<wgpu::SurfaceTexture> {
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Some(t),
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.reconfigure();
                None
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                match self.instance.create_surface(self.window.clone()) {
                    Ok(s) => {
                        self.surface = s;
                        self.reconfigure();
                    }
                    Err(e) => log::error!("couldn't recreate surface: {e}"),
                }
                None
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded | wgpu::CurrentSurfaceTexture::Validation => None,
        }
    }

    fn handle_key(&mut self, code: KeyCode, settings: &mut Settings, el: &ActiveEventLoop) {
        match code {
            KeyCode::Tab => settings.show_panel = !settings.show_panel,
            KeyCode::KeyF => self.cam.toggle_free(),
            KeyCode::KeyN => self.go_to_spot(camera::next_spot(self.spot), settings),
            KeyCode::KeyO => {
                if self.scene == SceneKind::City {
                    settings.window_open = !settings.window_open;
                }
            }
            KeyCode::KeyM => settings.muted = !settings.muted,
            KeyCode::F12 => self.screenshot = true,
            KeyCode::F11 => {
                if self.window.fullscreen().is_some() {
                    self.window.set_fullscreen(None);
                } else {
                    self.window.set_fullscreen(Some(Fullscreen::Borderless(None)));
                }
            }
            KeyCode::Escape => {
                if self.cam.mode == CamMode::Free {
                    self.cam.toggle_free();
                } else if self.window.fullscreen().is_some() {
                    self.window.set_fullscreen(None);
                }
            }
            KeyCode::KeyQ if self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight) => el.exit(),
            KeyCode::Digit1 => self.go_to_spot(camera::SPOT_NEON_WINDOW, settings),
            KeyCode::Digit2 => self.go_to_spot(camera::SPOT_HIGH_RISE, settings),
            KeyCode::Digit3 | KeyCode::Digit4 | KeyCode::Digit5 | KeyCode::Digit6 | KeyCode::Digit7 | KeyCode::Digit8 => {
                let i = match code {
                    KeyCode::Digit3 => 0,
                    KeyCode::Digit4 => 1,
                    KeyCode::Digit5 => 2,
                    KeyCode::Digit6 => 3,
                    KeyCode::Digit7 => 4,
                    _ => 5,
                };
                self.go_to_spot(2 + i, settings);
            }
            _ => {}
        }
    }

    /// Moves to another place: a glide along the street, or a fade between
    /// the street and the window.
    fn go_to_spot(&mut self, target: u32, settings: &mut Settings) {
        if target == camera::SPOT_HIGH_RISE {
            self.toast = Some(("The high-rise window arrives in stage 2b".into(), 3.0));
            return;
        }
        if self.transition.is_some() || (target == self.spot && self.cam.mode != CamMode::Free) {
            return;
        }
        self.auto_timer = 0.0;
        if camera::spot_scene(target) == SceneKind::Street && self.scene == SceneKind::Street {
            self.cam.go_to((target - 2) as usize);
            self.spot = target;
            settings.spot = target;
        } else {
            self.transition = Some(Transition { target, t: 0.0, switched: false });
        }
    }

    /// Lands at a spot (cut, no glide): used at start-up and at the black
    /// point of a fade.
    fn arrive(&mut self, target: u32, settings: &mut Settings) {
        self.spot = target;
        settings.spot = target;
        self.scene = camera::spot_scene(target);
        match self.scene {
            SceneKind::City => {
                self.cam.sit_at_window(self.room.geom);
                self.room.prewarm(&self.weather.state, settings);
            }
            SceneKind::Street => self.cam.jump_to((target - 2) as usize),
        }
    }

    /// Advances a running fade; returns the fade amount (0 = clear, 1 = black).
    fn step_transition(&mut self, dt: f32, settings: &mut Settings) -> f32 {
        let Some(tr) = &mut self.transition else { return 0.0 };
        tr.t += dt;
        if !tr.switched && tr.t >= FADE_OUT {
            tr.switched = true;
            let target = tr.target;
            self.arrive(target, settings);
        }
        let Some(tr) = &self.transition else { return 0.0 };
        if tr.t < FADE_OUT {
            (tr.t / FADE_OUT).clamp(0.0, 1.0)
        } else if tr.t < FADE_OUT + FADE_IN {
            1.0 - ((tr.t - FADE_OUT) / FADE_IN).clamp(0.0, 1.0)
        } else {
            self.transition = None;
            0.0
        }
    }

    fn head_input(&self) -> HeadInput {
        let k = |c: KeyCode| if self.keys.contains(&c) { 1.0 } else { 0.0 };
        HeadInput {
            cursor: self.cursor,
            lean_scroll: self.lean_scroll,
            lean_keys: k(KeyCode::KeyW) + k(KeyCode::ArrowUp) - k(KeyCode::KeyS) - k(KeyCode::ArrowDown),
        }
    }

    fn move_input(&self) -> MoveInput {
        let k = |c: KeyCode| if self.keys.contains(&c) { 1.0 } else { 0.0 };
        MoveInput {
            forward: k(KeyCode::KeyW) - k(KeyCode::KeyS),
            right: k(KeyCode::KeyD) - k(KeyCode::KeyA),
            up: k(KeyCode::Space) + k(KeyCode::KeyE) - k(KeyCode::KeyC) - k(KeyCode::KeyQ),
            fast: self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight),
            look_dx: self.look.0,
            look_dy: self.look.1,
        }
    }

    fn frame(&mut self, settings: &mut Settings) {
        let now = Instant::now();
        let dt = (now - self.last).as_secs_f32().min(0.1);
        self.last = now;
        self.time += dt;
        self.frame = self.frame.wrapping_add(1);
        if dt > 0.0 {
            let k = 1.0 - (-dt / 0.5f32).exp();
            self.stats.fps += (1.0 / dt - self.stats.fps) * k;
            self.stats.frame_ms += (dt * 1000.0 - self.stats.frame_ms) * k;
        }

        // simulation
        let fade = self.step_transition(dt, settings);
        self.weather.update(dt, settings);
        let input = self.move_input();
        let head = self.head_input();
        self.look = (0.0, 0.0);
        self.lean_scroll = 0.0;
        let fov = settings.fov_deg.to_radians();
        let cam_state = self.cam.update(dt, &input, &head, settings.drift_interval_s, false, Some(fov));
        if settings.auto_switch && self.transition.is_none() && self.cam.mode != CamMode::Free {
            self.auto_timer += dt;
            if self.auto_timer > settings.drift_interval_s {
                self.go_to_spot(camera::next_spot(self.spot), settings);
            }
        } else {
            self.auto_timer = 0.0;
        }
        let interior = self.scene == SceneKind::City;
        if interior {
            self.room.update(dt, self.time, &self.weather.state, settings);
        }
        if let Some((_, t)) = &mut self.toast {
            *t -= dt;
            if *t <= 0.0 {
                self.toast = None;
            }
        }

        // audio: parameters + impacts read back from the GPU + the room
        let sh = &self.audio_shared;
        audio::update_shared(sh, settings, &self.weather.state, self.scene, interior, self.room.open(), fade);
        for imp in self.renderer.readback.collect(&self.renderer.device) {
            if imp.dt > 0.0 {
                let k = 1.0 - (-imp.dt / 0.7f32).exp();
                self.stats.impacts_per_s += (imp.total as f32 / imp.dt - self.stats.impacts_per_s) * k;
                self.stats.audible_per_s += (imp.events.len() as f32 / imp.dt - self.stats.audible_per_s) * k;
            }
            if let Some(a) = &mut self.audio {
                let voices = audio::spatialise(&imp.events, &cam_state, a.sample_rate, imp.dt, &mut self.rng);
                for v in voices {
                    if a.producer.push(AudioMsg::Impact(v)).is_err() {
                        break;
                    }
                }
            }
        }
        if interior && let Some(a) = &mut self.audio {
            for m in self.room.audio_msgs(&cam_state, a.sample_rate, dt, &mut self.rng) {
                if a.producer.push(m).is_err() {
                    break;
                }
            }
        }
        let ws = &self.weather.state;
        self.stats.voices = sh.voices.load(std::sync::atomic::Ordering::Relaxed);
        self.stats.intensity = ws.intensity;
        self.stats.wind = ws.wind.length();
        self.stats.puddle = ws.puddle_level;
        self.stats.shower = ws.shower;
        self.stats.active_drops = self.renderer.stats.active_drops;
        self.stats.lights = self.renderer.stats.lights;
        self.stats.ripple_steps = self.renderer.stats.ripple_steps;
        self.stats.glass_drops = if interior { self.room.glass.drop_count() as u32 } else { 0 };
        self.stats.spot = self.spot;
        self.stats.free = self.cam.mode == CamMode::Free;
        self.stats.window_moving = self.room.is_moving();
        self.stats.toast = self.toast.as_ref().map(|(m, t)| (m.clone(), t.min(1.0)));

        // UI
        let hint_alpha = ((9.0 - self.time) / 2.0).clamp(0.0, 1.0) * if settings.show_panel { 0.0 } else { 1.0 };
        let raw = self.egui_state.take_egui_input(&self.window);
        let stats = self.stats.clone();
        let mut actions = ui::UiActions::default();
        let mut out = self.egui_ctx.run_pass(raw, |ctx| {
            actions = ui::draw(ctx, settings, &stats, hint_alpha);
        });
        self.egui_state.handle_platform_output(&self.window, std::mem::take(&mut out.platform_output));
        if actions.reset {
            let keep_panel = settings.show_panel;
            *settings = Settings::default();
            settings.show_panel = keep_panel;
        }
        if actions.screenshot {
            self.screenshot = true;
        }
        if let Some(t) = actions.go_spot {
            self.go_to_spot(t, settings);
        }
        if actions.toggle_free {
            self.cam.toggle_free();
        }
        let ppp = out.pixels_per_point;
        let jobs = self.egui_ctx.tessellate(std::mem::take(&mut out.shapes), ppp);
        // egui requires every texture delta to be consumed
        let mut tdelta = std::mem::take(&mut out.textures_delta);
        for (id, deltas) in tdelta.set.drain() {
            for delta in deltas.iter() {
                self.egui_rend.update_texture(&self.renderer.device, &self.renderer.queue, id, delta);
            }
        }
        let to_free: Vec<egui::TextureId> = tdelta.free.drain().collect();

        let w = self.config.width;
        let h = self.config.height;
        self.renderer.resize(w, h, settings.reflection_scale);

        if let Some(frame_tex) = self.acquire() {
            let view = frame_tex.texture.create_view(&wgpu::TextureViewDescriptor::default());
            let ui_view = frame_tex.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.config.format.remove_srgb_suffix()),
                ..Default::default()
            });
            let screen = egui_wgpu::ScreenDescriptor { size_in_pixels: [w, h], pixels_per_point: ppp };
            let interior = self.scene == SceneKind::City;
            let fd = FrameData {
                camera: cam_state,
                time: self.time,
                dt,
                frame_index: self.frame,
                weather: &self.weather.state,
                settings,
                scene: self.scene,
                room: if interior { Some(self.room.state()) } else { None },
                fade,
                cpu_splashes: if interior { self.room.splashes() } else { &[] },
                drips: if interior { self.room.drips() } else { &[] },
            };
            self.renderer.render(&fd, &view, Some(EguiDraw { renderer: &mut self.egui_rend, view: &ui_view, jobs: &jobs, screen: &screen }));
            self.window.pre_present_notify();
            self.renderer.queue.present(frame_tex);
        }
        for id in &to_free {
            self.egui_rend.free_texture(id);
        }

        if self.screenshot {
            self.screenshot = false;
            let interior = self.scene == SceneKind::City;
            let fd = FrameData {
                camera: cam_state,
                time: self.time,
                dt: 0.0,
                frame_index: self.frame,
                weather: &self.weather.state,
                settings,
                scene: self.scene,
                room: if interior { Some(self.room.state()) } else { None },
                fade,
                cpu_splashes: if interior { self.room.splashes() } else { &[] },
                drips: if interior { self.room.drips() } else { &[] },
            };
            match crate::headless::capture_frame(&mut self.renderer, &fd) {
                Ok(img) => {
                    std::thread::spawn(move || {
                        let dir = dirs::picture_dir()
                            .or_else(|| dirs::home_dir().map(|h| h.join("Pictures")))
                            .unwrap_or_else(|| std::path::PathBuf::from("."))
                            .join("Puddlebun");
                        let _ = std::fs::create_dir_all(&dir);
                        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
                        let path = dir.join(format!("puddlebun-{stamp}.png"));
                        match crate::headless::save_png(&path, &img) {
                            Ok(()) => log::info!("screenshot saved to {}", path.display()),
                            Err(e) => log::warn!("screenshot failed: {e}"),
                        }
                    });
                }
                Err(e) => log::warn!("screenshot failed: {e}"),
            }
        }

        // autosave settings
        if self.last_save.elapsed() > Duration::from_secs(15) {
            self.last_save = Instant::now();
            if *settings != self.saved {
                settings.save();
                self.saved = settings.clone();
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.running.is_none()
            && let Err(e) = self.init(el) {
                self.error = Some(e);
                el.exit();
            }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(r) = self.running.as_mut() else { return };
        let resp = r.egui_state.on_window_event(&r.window, &event);
        match event {
            WindowEvent::CloseRequested => {
                self.settings.save();
                el.exit();
            }
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => r.reconfigure(),
            WindowEvent::Focused(f) => {
                r.focused = f;
                if !f {
                    r.keys.clear();
                    r.rmb = false;
                }
            }
            WindowEvent::Occluded(o) => r.occluded = o,
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let typing = r.egui_ctx.egui_wants_keyboard_input();
                    match event.state {
                        ElementState::Pressed => {
                            if !typing {
                                if !event.repeat {
                                    r.handle_key(code, &mut self.settings, el);
                                }
                                r.keys.insert(code);
                            }
                        }
                        ElementState::Released => {
                            r.keys.remove(&code);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { state, button: MouseButton::Right, .. } => {
                r.rmb = state == ElementState::Pressed && !resp.consumed;
            }
            WindowEvent::CursorMoved { position, .. } => {
                if !r.egui_ctx.egui_wants_pointer_input() {
                    let w = r.config.width.max(1) as f32;
                    let h = r.config.height.max(1) as f32;
                    let p = glam::Vec2::new(position.x as f32 / w * 2.0 - 1.0, position.y as f32 / h * 2.0 - 1.0);
                    r.cursor = Some(p.clamp(glam::Vec2::splat(-1.0), glam::Vec2::splat(1.0)));
                }
            }
            WindowEvent::CursorLeft { .. } => r.cursor = None,
            WindowEvent::MouseWheel { delta, .. } => {
                if !resp.consumed && !r.egui_ctx.egui_wants_pointer_input() {
                    r.lean_scroll += match delta {
                        MouseScrollDelta::LineDelta(_, y) => y,
                        MouseScrollDelta::PixelDelta(p) => p.y as f32 / 60.0,
                    };
                }
            }
            WindowEvent::RedrawRequested
                if !r.minimized => {
                    r.frame(&mut self.settings);
                }
            _ => {}
        }
    }

    fn device_event(&mut self, _el: &ActiveEventLoop, _id: winit::event::DeviceId, event: DeviceEvent) {
        let Some(r) = self.running.as_mut() else { return };
        if let DeviceEvent::MouseMotion { delta } = event
            && r.rmb && r.cam.mode == CamMode::Free {
                r.look.0 += delta.0 as f32;
                r.look.1 += delta.1 as f32;
            }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        let Some(r) = self.running.as_mut() else { return };
        if r.minimized || r.occluded {
            // nothing visible: don't render, but the audio thread keeps raining
            el.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250)));
            return;
        }
        if r.focused {
            el.set_control_flow(ControlFlow::Poll);
            r.window.request_redraw();
        } else {
            // be a good background citizen: ~30 fps when unfocused
            let now = Instant::now();
            if now >= r.next_frame {
                r.next_frame = now + Duration::from_micros(33_333);
                r.window.request_redraw();
            }
            el.set_control_flow(ControlFlow::WaitUntil(r.next_frame));
        }
    }

    fn exiting(&mut self, _el: &ActiveEventLoop) {
        self.settings.save();
    }
}
