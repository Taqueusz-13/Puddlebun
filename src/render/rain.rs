//! GPU raindrops: buffers, simulation dispatch, streak + splash drawing and
//! the audio-event buffer the CPU reads back.

use super::util::*;
use crate::wgpu;
use bytemuck::{Pod, Zeroable};

pub const DROP_STRIDE: u64 = 48;
pub const SPLASH_CAPACITY: u32 = 32_768;
pub const SPLASH_DROPLETS: u32 = 8;
pub const AUDIO_CAPACITY: u32 = 2048;
pub const AUDIO_EVENT_SIZE: u64 = 32;
pub const COUNTERS_SIZE: u64 = 16;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct SimParams {
    pub box_center: [f32; 4],
    pub box_y: [f32; 4],
    pub dsd: [f32; 4],
    pub counts: [u32; 4],
    pub audio: [f32; 4],
}

/// One impact read back from the GPU (matches `AudioEvent` in rain_sim.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, Default)]
pub struct GpuImpact {
    pub pos: [f32; 3],
    pub size: f32,
    pub kind: u32,
    pub speed: f32,
    pub pad: [f32; 2],
}

pub struct Rain {
    pub max_particles: u32,
    pub sim_buf: wgpu::Buffer,
    pub counters: wgpu::Buffer,
    pub audio_events: wgpu::Buffer,
    update_pipeline: wgpu::ComputePipeline,
    update_bg: wgpu::BindGroup,
    rain_pipeline: wgpu::RenderPipeline,
    splash_pipeline: wgpu::RenderPipeline,
    draw_bg: wgpu::BindGroup,
    _drops: wgpu::Buffer,
    _splashes: wgpu::Buffer,
}

impl Rain {
    pub fn new(
        device: &wgpu::Device,
        frame_bgl: &wgpu::BindGroupLayout,
        ripple_impulses: &wgpu::Buffer,
        max_particles: u32,
    ) -> Self {
        let storage_rw = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC;
        let drops = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("drops"),
            size: max_particles as u64 * DROP_STRIDE,
            usage: storage_rw,
            mapped_at_creation: false,
        });
        let splashes = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("splashes"),
            size: SPLASH_CAPACITY as u64 * 32,
            usage: storage_rw,
            mapped_at_creation: false,
        });
        let counters = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rain counters"),
            size: COUNTERS_SIZE,
            usage: storage_rw,
            mapped_at_creation: false,
        });
        let audio_events = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("audio events"),
            size: AUDIO_CAPACITY as u64 * AUDIO_EVENT_SIZE,
            usage: storage_rw,
            mapped_at_creation: false,
        });
        let sim_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sim params"),
            size: std::mem::size_of::<SimParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let c = wgpu::ShaderStages::COMPUTE;
        let update_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rain update bgl"),
            entries: &[
                uniform_entry(0, c, false),
                storage_entry(1, c, false),
                storage_entry(2, c, false),
                storage_entry(3, c, false),
                storage_entry(4, c, false),
                storage_entry(5, c, false),
            ],
        });
        let update_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rain update bg"),
            layout: &update_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: sim_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: drops.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: splashes.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: counters.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: audio_events.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: ripple_impulses.as_entire_binding() },
            ],
        });
        let sim_module = shader(device, "rain_sim", &[COMMON_WGSL, WORLD_WGSL, include_str!("../shaders/rain_sim.wgsl")]);
        let update_layout = layout(device, "rain update layout", &[frame_bgl, &update_bgl]);
        let update_pipeline = compute_pipeline(device, "rain update", &update_layout, &sim_module, "cs_update");

        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let draw_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("rain draw bgl"),
            entries: &[uniform_entry(0, vf, false), storage_entry(1, vf, true), storage_entry(2, vf, true)],
        });
        let draw_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("rain draw bg"),
            layout: &draw_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: sim_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: drops.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: splashes.as_entire_binding() },
            ],
        });
        let draw_module = shader(device, "rain_draw", &[COMMON_WGSL, WORLD_WGSL, include_str!("../shaders/rain_draw.wgsl")]);
        let draw_layout = layout(device, "rain draw layout", &[frame_bgl, &draw_bgl]);
        let depth = depth_state(false, wgpu::CompareFunction::Greater);
        let rain_pipeline = render_pipeline(
            device,
            "rain streaks",
            &draw_layout,
            &draw_module,
            "vs_rain",
            "fs_streak",
            &[],
            super::HDR_FORMAT,
            Some(ADDITIVE_PREMUL),
            Some(depth.clone()),
        );
        let splash_pipeline = render_pipeline(
            device,
            "splashes",
            &draw_layout,
            &draw_module,
            "vs_splash",
            "fs_streak",
            &[],
            super::HDR_FORMAT,
            Some(ADDITIVE_PREMUL),
            Some(depth),
        );

        Self {
            max_particles,
            sim_buf,
            counters,
            audio_events,
            update_pipeline,
            update_bg,
            rain_pipeline,
            splash_pipeline,
            draw_bg,
            _drops: drops,
            _splashes: splashes,
        }
    }

    pub fn dispatch(&self, pass: &mut wgpu::ComputePass, frame_bg: &wgpu::BindGroup, active: u32) {
        if active == 0 {
            return;
        }
        pass.set_pipeline(&self.update_pipeline);
        pass.set_bind_group(0, frame_bg, &[]);
        pass.set_bind_group(1, &self.update_bg, &[]);
        pass.dispatch_workgroups(active.div_ceil(256), 1, 1);
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass, frame_bg: &wgpu::BindGroup, active: u32) {
        pass.set_bind_group(0, frame_bg, &[]);
        pass.set_bind_group(1, &self.draw_bg, &[]);
        pass.set_pipeline(&self.splash_pipeline);
        pass.draw(0..6, 0..SPLASH_CAPACITY * SPLASH_DROPLETS);
        if active > 0 {
            pass.set_pipeline(&self.rain_pipeline);
            pass.draw(0..6, 0..active);
        }
    }
}
