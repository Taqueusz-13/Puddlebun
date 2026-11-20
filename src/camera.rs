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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CamMode {
    Drift,
    Free,
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
}

const GLIDE_SECONDS: f32 = 9.0;
const DOLLY_PERIOD: f32 = 80.0;

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
        self.glide_from = Some(self.last);
        self.glide_t = 0.0;
        self.current = i;
        self.shot_time = 0.0;
        self.since_switch = 0.0;
        self.mode = CamMode::Drift;
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

    pub fn update(&mut self, dt: f32, input: &MoveInput, drift_interval: f32, auto_advance: bool, fov_override: Option<f32>) -> CameraState {
        let state = match self.mode {
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
