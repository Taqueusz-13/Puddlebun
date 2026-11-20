//! Puddle ripple simulation (wave equation on a 10 m toroidal tile, 5 mm texels).

use super::util::*;
use crate::wgpu;
use bytemuck::{Pod, Zeroable};

pub const RES: u32 = 2048;
pub const TEXEL: f32 = 0.005;
/// Fixed simulation step (s).
pub const STEP: f32 = 1.0 / 200.0;
const MAX_STEPS: usize = 6;
/// Ripple phase speed for ~1 cm capillary-gravity waves (m/s).
const WAVE_SPEED: f32 = 0.25;
/// Ripples fade within about this time (s).
const DAMP_TIME: f32 = 0.35;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct RippleParams {
    res: u32,
    seed: u32,
    rate: f32,
    c2: f32,
    damping: f32,
    amp_scale: f32,
    lam: f32,
    pad: f32,
}

pub struct Ripple {
    pub heights: [wgpu::Buffer; 2],
    pub impulses: wgpu::Buffer,
    params: wgpu::Buffer,
    param_stride: u64,
    bgs: [wgpu::BindGroup; 2],
    inject: wgpu::ComputePipeline,
    step: wgpu::ComputePipeline,
    /// index of the buffer holding the current heights
    pub cur: usize,
    accum: f32,
    step_counter: u32,
    pending: Vec<RippleParams>,
}

impl Ripple {
    pub fn new(device: &wgpu::Device) -> Self {
        let n = (RES * RES) as u64;
        let mk = |label: &str| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: n * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let heights = [mk("ripple heights A"), mk("ripple heights B")];
        let impulses = mk("ripple impulses");
        let align = device.limits().min_uniform_buffer_offset_alignment as u64;
        let param_stride = align_up(std::mem::size_of::<RippleParams>() as u64, align);
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ripple params"),
            size: param_stride * MAX_STEPS as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let c = wgpu::ShaderStages::COMPUTE;
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ripple bgl"),
            entries: &[uniform_entry(0, c, true), storage_entry(1, c, true), storage_entry(2, c, false), storage_entry(3, c, false)],
        });
        let size = std::num::NonZeroU64::new(std::mem::size_of::<RippleParams>() as u64);
        let mk_bg = |cur: usize| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ripple bg"),
                layout: &bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding { buffer: &params, offset: 0, size }),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: heights[cur].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: heights[1 - cur].as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: impulses.as_entire_binding() },
                ],
            })
        };
        let bgs = [mk_bg(0), mk_bg(1)];
        let module = shader(device, "ripple", &[COMMON_WGSL, include_str!("../shaders/ripple.wgsl")]);
        let pl = layout(device, "ripple layout", &[&bgl]);
        let inject = compute_pipeline(device, "ripple inject", &pl, &module, "cs_inject");
        let step = compute_pipeline(device, "ripple step", &pl, &module, "cs_step");
        Self {
            heights,
            impulses,
            params,
            param_stride,
            bgs,
            inject,
            step,
            cur: 0,
            accum: 0.0,
            step_counter: 0,
            pending: Vec::new(),
        }
    }

    /// Works out how many fixed steps to run this frame and uploads their parameters.
    /// `flux` = drops per m² per second that are too small to simulate as particles.
    pub fn prepare(&mut self, queue: &wgpu::Queue, dt: f32, flux: f32, lambda: f32, strength: f32) -> usize {
        self.accum = (self.accum + dt).min(STEP * MAX_STEPS as f32);
        self.pending.clear();
        let c = WAVE_SPEED * STEP / TEXEL;
        let damping = (-STEP / DAMP_TIME).exp();
        let cell_area = (2.0 * TEXEL) * (2.0 * TEXEL);
        let rate = (flux * cell_area * STEP).min(0.5);
        while self.accum >= STEP && self.pending.len() < MAX_STEPS {
            self.accum -= STEP;
            self.step_counter = self.step_counter.wrapping_add(1);
            self.pending.push(RippleParams {
                res: RES,
                seed: crate::scene::hash_u(self.step_counter.wrapping_mul(747_796_405)),
                rate,
                c2: c * c,
                damping,
                amp_scale: 260.0 * strength,
                lam: lambda,
                pad: 0.0,
            });
        }
        for (i, p) in self.pending.iter().enumerate() {
            queue.write_buffer(&self.params, i as u64 * self.param_stride, bytemuck::bytes_of(p));
        }
        self.pending.len()
    }

    pub fn dispatch(&mut self, pass: &mut wgpu::ComputePass, steps: usize) {
        let groups = RES.div_ceil(16);
        let half_groups = (RES / 2).div_ceil(16);
        for i in 0..steps {
            let offset = (i as u64 * self.param_stride) as u32;
            pass.set_bind_group(0, &self.bgs[self.cur], &[offset]);
            pass.set_pipeline(&self.inject);
            pass.dispatch_workgroups(half_groups, half_groups, 1);
            pass.set_pipeline(&self.step);
            pass.dispatch_workgroups(groups, groups, 1);
            self.cur = 1 - self.cur;
        }
    }
}
