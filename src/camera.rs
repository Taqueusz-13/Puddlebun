//! Camera: slow drift between framed viewpoints, or free-fly (WASD + mouse).

use glam::{Mat4, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct CameraState {
    pub pos: Vec3,
    pub forward: Vec3,
    pub fov_y: f32,
}

impl CameraState {
    pub fn right(&self) -> Vec3 {
        self.forward.cross(Vec3::Y).normalize_or(Vec3::X)
    }
    pub fn up(&self) -> Vec3 {
        self.right().cross(self.forward).normalize()
    }
    pub fn view(&self) -> Mat4 {
        glam::camera::rh::view::look_to_mat4(self.pos, self.forward.normalize(), Vec3::Y)
    }
    /// Reverse-Z infinite perspective (depth 1 = near, 0 = infinity).
    pub fn proj(&self, aspect: f32) -> Mat4 {
        glam::camera::rh::proj::directx::perspective_infinite_reverse(self.fov_y, aspect, 0.05)
    }
    /// The camera mirrored in the ground plane (y = 0), for puddle reflections.
    pub fn mirrored(&self) -> CameraState {
        CameraState {
            pos: Vec3::new(self.pos.x, -self.pos.y, self.pos.z),
            forward: Vec3::new(self.forward.x, -self.forward.y, self.forward.z),
            fov_y: self.fov_y,
        }
    }
}

pub struct Shot {
    pub name: &'static str,
    pub from: Vec3,
    pub to: Vec3,
    pub look: Vec3,
    pub fov_deg: f32,
}

pub fn residential_shots() -> Vec<Shot> {
    vec![
        Shot {
            name: "Down the street",
            from: Vec3::new(2.3, 1.6, -5.0),
            to: Vec3::new(2.3, 1.6, 3.0),
            look: Vec3::new(0.3, 1.5, 60.0),
            fov_deg: 55.0,
        },
        Shot {
            name: "Gutter",
            from: Vec3::new(2.55, 0.42, 9.0),
            to: Vec3::new(2.75, 0.42, 11.0),
            look: Vec3::new(2.7, 0.0, 16.5),
            fov_deg: 50.0,
        },
        Shot {
            name: "Under the lamp",
            from: Vec3::new(-4.4, 1.65, 5.0),
            to: Vec3::new(-4.4, 1.65, 8.5),
            look: Vec3::new(2.6, 4.3, 15.5),
            fov_deg: 60.0,
        },
        Shot {
            name: "Up high",
            from: Vec3::new(0.0, 3.5, -20.0),
            to: Vec3::new(0.0, 3.2, -13.0),
            look: Vec3::new(0.0, 1.0, 45.0),
            fov_deg: 52.0,
        },
        Shot {
            name: "Bay windows",
            from: Vec3::new(4.7, 1.6, 26.0),
            to: Vec3::new(4.9, 1.6, 29.5),
            look: Vec3::new(7.6, 1.7, 37.0),
            fov_deg: 55.0,
        },
        Shot {
            name: "Puddle mirror",
            from: Vec3::new(-1.3, 0.22, 21.0),
            to: Vec3::new(-0.9, 0.22, 23.5),
            look: Vec3::new(0.3, 1.5, 52.0),
            fov_deg: 55.0,
        },
    ]
}

/// Places you can be: 0 = the neon window, 1 = the high-rise window
/// (stage 2b), 2-7 = the residential street viewpoints.
pub const SPOT_NEON_WINDOW: u32 = 0;
pub const SPOT_HIGH_RISE: u32 = 1;
pub const SPOT_COUNT: u32 = 8;

pub fn spot_scene(spot: u32) -> crate::scene::SceneKind {
    if spot < 2 { crate::scene::SceneKind::City } else { crate::scene::SceneKind::Street }
}

/// The spot after this one (skips places that don't exist yet).
pub fn next_spot(spot: u32) -> u32 {
    match spot {
        0 | 1 => 2,
        s if s + 1 >= SPOT_COUNT => 0,
        s => s + 1,
    }
}

pub fn spot_names() -> Vec<String> {
    let mut v = vec!["Neon window".to_string(), "High-rise window".to_string()];
    v.extend(residential_shots().iter().map(|s| s.name.to_string()));
    v
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CamMode {
    Drift,
    Free,
    /// sitting at the window: your head moves a little, you can lean in
    Window,
}

/// Input for the head-behind-the-glass view.
#[derive(Default, Clone, Copy)]
pub struct HeadInput {
    /// cursor position in -1..1 (x right, y down); None when outside the window
    pub cursor: Option<glam::Vec2>,
    /// scroll wheel lean (positive = closer)
    pub lean_scroll: f32,
    /// W/S held: +1 lean in, -1 lean back
    pub lean_keys: f32,
}

#[derive(Default)]
pub struct MoveInput {
    pub forward: f32,
    pub right: f32,
    pub up: f32,
    pub fast: bool,
    pub look_dx: f32,
    pub look_dy: f32,
}

pub struct CameraRig {
    pub mode: CamMode,
    pub shots: Vec<Shot>,
    pub current: usize,
    shot_time: f32,
    since_switch: f32,
    glide_from: Option<CameraState>,
    glide_t: f32,
    free_pos: Vec3,
    yaw: f32,
    pitch: f32,
    last: CameraState,
    window: Option<crate::city::WindowGeom>,
    head: glam::Vec2,
    lean: f32,
    lean_target: f32,
    idle: f32,
    last_cursor: Option<glam::Vec2>,
    t: f32,
}

const GLIDE_SECONDS: f32 = 9.0;
/// Vertical field of view at the window (degrees).
const WINDOW_FOV: f32 = 62.0;

/// One full back-and-forth drift (20 s each way).
const DOLLY_PERIOD: f32 = 40.0;

fn smootherstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

impl CameraRig {
    pub fn new() -> Self {
        let shots = residential_shots();
        let mut rig = Self {
            mode: CamMode::Drift,
            shots,
            current: 0,
            shot_time: 0.0,
            since_switch: 0.0,
            glide_from: None,
            glide_t: 0.0,
            free_pos: Vec3::ZERO,
            yaw: 0.0,
            pitch: 0.0,
            last: CameraState { pos: Vec3::new(0.0, 1.6, 0.0), forward: Vec3::Z, fov_y: 55f32.to_radians() },
            window: None,
            head: glam::Vec2::ZERO,
            lean: 0.0,
            lean_target: 0.0,
            idle: 10.0,
            last_cursor: None,
            t: 0.0,
        };
        rig.last = rig.shot_pose(0, 0.0);
        rig
    }

    fn shot_pose(&self, i: usize, t: f32) -> CameraState {
        let s = &self.shots[i];
        // slow ping-pong dolly with eased ends
        let k = 0.5 - 0.5 * (std::f32::consts::TAU * t / DOLLY_PERIOD).cos();
        let mut pos = s.from.lerp(s.to, k);
        // a hint of hand-held breathing
        pos += Vec3::new((t * 0.37).sin() * 0.012, (t * 0.53).sin() * 0.008, 0.0);
        let look = s.look + Vec3::new((t * 0.21).sin() * 0.15, (t * 0.17).sin() * 0.06, 0.0);
        CameraState { pos, forward: (look - pos).normalize(), fov_y: s.fov_deg.to_radians() }
    }

    pub fn go_to(&mut self, i: usize) {
        let i = i % self.shots.len();
        self.glide_from = if self.mode == CamMode::Window { None } else { Some(self.last) };
        self.glide_t = 0.0;
        self.current = i;
        self.shot_time = 0.0;
        self.since_switch = 0.0;
        self.mode = CamMode::Drift;
    }

    /// Sits down at a window (cut, no glide).
    pub fn sit_at_window(&mut self, g: crate::city::WindowGeom) {
        self.window = Some(g);
        self.mode = CamMode::Window;
        self.head = glam::Vec2::ZERO;
        self.lean = 0.0;
        self.lean_target = 0.0;
        self.glide_from = None;
        self.last = CameraState { pos: g.eye, forward: (g.look - g.eye).normalize(), fov_y: WINDOW_FOV.to_radians() };
    }

    fn window_pose(&mut self, dt: f32, input: &HeadInput) -> CameraState {
        let g = self.window.expect("window mode without a window");
        self.t += dt;
        let t = self.t;
        let moved = match (input.cursor, self.last_cursor) {
            (Some(a), Some(b)) => a.distance(b) > 0.002,
            (Some(_), None) => true,
            _ => false,
        };
        self.last_cursor = input.cursor;
        if moved || input.lean_scroll != 0.0 || input.lean_keys != 0.0 {
            self.idle = 0.0;
        } else {
            self.idle += dt;
        }
        let mut target = input.cursor.map(|c| glam::Vec2::new(c.x, -c.y)).unwrap_or(glam::Vec2::ZERO);
        // idle: drift gently as if breathing and shifting in the chair
        let sway = glam::Vec2::new((t * 0.11).sin() * 0.3 + (t * 0.071).sin() * 0.2, (t * 0.17).sin() * 0.18 + (t * 0.05).sin() * 0.1);
        let k_idle = ((self.idle - 2.5) / 2.5).clamp(0.0, 1.0);
        target = target * (1.0 - k_idle * 0.6) + sway * k_idle;
        self.head += (target - self.head) * (1.0 - (-dt * 3.5).exp());
        self.lean_target = (self.lean_target + input.lean_scroll * 0.06 + input.lean_keys * dt * 0.45).clamp(0.0, 0.75);
        self.lean += (self.lean_target - self.lean) * (1.0 - (-dt * 5.0).exp());
        let breathe = (t * std::f32::consts::TAU * 0.22).sin() * 0.004;
        // looking out along -x: right is -z
        let eye = g.eye + Vec3::new(-self.lean, self.head.y * 0.26 + breathe, -self.head.x * 0.42);
        let look = g.look + Vec3::new(0.0, self.head.y * 2.2 - self.lean * 11.0, -self.head.x * 2.6);
        CameraState { pos: eye, forward: (look - eye).normalize(), fov_y: WINDOW_FOV.to_radians() }
    }

    /// Cuts straight to a viewpoint (no glide).
    pub fn jump_to(&mut self, i: usize) {
        self.go_to(i);
        self.glide_from = None;
        self.last = self.shot_pose(self.current, 0.0);
    }

    pub fn next(&mut self) {
        self.go_to(self.current + 1);
    }

    pub fn toggle_free(&mut self) {
        match self.mode {
            CamMode::Window => {}
            CamMode::Drift => {
                self.mode = CamMode::Free;
                self.free_pos = self.last.pos;
                let f = self.last.forward;
                self.yaw = f.x.atan2(f.z);
                self.pitch = f.y.clamp(-0.99, 0.99).asin();
            }
            CamMode::Free => {
                self.glide_from = Some(self.last);
                self.glide_t = 0.0;
                self.mode = CamMode::Drift;
            }
        }
    }

    pub fn update(&mut self, dt: f32, input: &MoveInput, head: &HeadInput, drift_interval: f32, auto_advance: bool, fov_override: Option<f32>) -> CameraState {
        let state = match self.mode {
            CamMode::Window => self.window_pose(dt, head),
            CamMode::Drift => {
                self.shot_time += dt;
                self.since_switch += dt;
                if auto_advance && self.since_switch > drift_interval {
                    self.next();
                }
                let target = self.shot_pose(self.current, self.shot_time);
                if let Some(from) = self.glide_from {
                    self.glide_t += dt / GLIDE_SECONDS;
                    let k = smootherstep(self.glide_t);
                    let mut pos = from.pos.lerp(target.pos, k);
                    // drone-like arc when travelling a long way
                    let dist = from.pos.distance(target.pos);
                    pos.y += (std::f32::consts::PI * k).sin() * (dist * 0.08).min(2.5);
                    let fwd = from.forward.lerp(target.forward, k).normalize_or(target.forward);
                    let fov = from.fov_y + (target.fov_y - from.fov_y) * k;
                    if self.glide_t >= 1.0 {
                        self.glide_from = None;
                    }
                    CameraState { pos, forward: fwd, fov_y: fov }
                } else {
                    target
                }
            }
            CamMode::Free => {
                self.yaw -= input.look_dx * 0.0025;
                self.pitch = (self.pitch - input.look_dy * 0.0025).clamp(-1.5, 1.5);
                let fwd = Vec3::new(self.yaw.sin() * self.pitch.cos(), self.pitch.sin(), self.yaw.cos() * self.pitch.cos());
                let right = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
                let speed = if input.fast { 12.0 } else { 3.0 };
                let mv = fwd * input.forward + right * input.right + Vec3::Y * input.up;
                self.free_pos += mv.normalize_or_zero() * speed * dt;
                self.free_pos.y = self.free_pos.y.max(0.15);
                CameraState { pos: self.free_pos, forward: fwd, fov_y: self.last.fov_y }
            }
        };
        let mut state = state;
        if let Some(f) = fov_override
            && self.mode == CamMode::Free {
                state.fov_y = f;
            }
        self.last = state;
        state
    }
}
