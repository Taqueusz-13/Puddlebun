//! GPU resources for the window glass (see src/glass.rs for the simulation).

use super::util::*;
use super::HDR_FORMAT;
use crate::city::WindowGeom;
use crate::glass::{GlassSim, GW, GH, MAX_DROPS, MAX_ITEMS, TH, TW};
use crate::wgpu;
use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
struct GlassParams {
    upper: [f32; 4],
    lower: [f32; 4],
    planes: [f32; 4],
    cond: [f32; 4],
    grid: [f32; 4],
    lamp: [f32; 4],
    lamp_col: [f32; 4],
}

pub struct GlassGpu {
    params: wgpu::Buffer,
    drops: wgpu::Buffer,
    cell_start: wgpu::Buffer,
    cell_items: wgpu::Buffer,
    trail: wgpu::Texture,
    trail_view: wgpu::TextureView,
    bgl: wgpu::BindGroupLayout,
    bg: Option<wgpu::BindGroup>,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

impl GlassGpu {
    pub fn new(device: &wgpu::Device, frame_bgl: &wgpu::BindGroupLayout) -> Self {
        let st = |label: &str, size: u64| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let params = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("glass params"),
            size: std::mem::size_of::<GlassParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let drops = st("glass drops", (MAX_DROPS * 16) as u64);
        let cell_start = st("glass cells", ((2 * GW * GH + 1) * 4) as u64);
        let cell_items = st("glass items", (MAX_ITEMS * 4) as u64);
        let trail = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glass trails"),
            size: wgpu::Extent3d { width: TW as u32, height: (2 * TH) as u32, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let trail_view = trail.create_view(&wgpu::TextureViewDescriptor::default());
        let f = wgpu::ShaderStages::FRAGMENT;
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("glass bgl"),
            entries: &[
                texture_entry(0, f),
                texture_entry(1, f),
                sampler_entry(2, f),
                uniform_entry(3, vf, false),
                storage_entry(4, f, true),
                storage_entry(5, f, true),
                storage_entry(6, f, true),
                texture_entry(7, f),
            ],
        });
        let module = shader(device, "glass", &[COMMON_WGSL, WORLD_WGSL, include_str!("../shaders/glass.wgsl")]);
        let pl = layout(device, "glass layout", &[frame_bgl, &bgl]);
        let pipeline = render_pipeline(
            device,
            "glass",
            &pl,
            &module,
            "vs_glass",
            "fs_glass",
            &[],
            HDR_FORMAT,
            None,
            Some(depth_state(true, wgpu::CompareFunction::Greater)),
        );
        Self {
            params,
            drops,
            cell_start,
            cell_items,
            trail,
            trail_view,
            bgl,
            bg: None,
            pipeline,
            sampler: linear_sampler(device, wgpu::AddressMode::ClampToEdge),
        }
    }

    /// Must be called when the outside / blur textures are recreated.
    pub fn rebind(&mut self, device: &wgpu::Device, outside: &wgpu::TextureView, blurred: &wgpu::TextureView) {
        self.bg = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("glass bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(outside) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(blurred) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: self.params.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: self.drops.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: self.cell_start.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: self.cell_items.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&self.trail_view) },
            ],
        }));
    }

    pub fn upload(&self, queue: &wgpu::Queue, sim: &GlassSim, geom: &WindowGeom, raise: f32, time: f32, lamp: Vec3, lamp_col: Vec3) {
        let p = GlassParams {
            upper: geom.upper,
            lower: geom.lower,
            planes: [geom.glass_x, geom.inner_x, raise, time],
            cond: [sim.cond, sim.mist, 0.0, 0.0],
            grid: [crate::glass::CELL, GW as f32, GH as f32, crate::glass::TRAIL_TEXEL],
            lamp: [lamp.x, lamp.y, lamp.z, 0.0],
            lamp_col: [lamp_col.x, lamp_col.y, lamp_col.z, 0.0],
        };
        queue.write_buffer(&self.params, 0, bytemuck::bytes_of(&p));
        if !sim.gpu.is_empty() {
            queue.write_buffer(&self.drops, 0, bytemuck::cast_slice(&sim.gpu));
        }
        queue.write_buffer(&self.cell_start, 0, bytemuck::cast_slice(&sim.cell_start));
        if !sim.cell_items.is_empty() {
            queue.write_buffer(&self.cell_items, 0, bytemuck::cast_slice(&sim.cell_items));
        }
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &self.trail, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &sim.trail_u8,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(TW as u32), rows_per_image: Some((2 * TH) as u32) },
            wgpu::Extent3d { width: TW as u32, height: (2 * TH) as u32, depth_or_array_layers: 1 },
        );
    }

    pub fn draw(&self, pass: &mut wgpu::RenderPass, frame_bg: &wgpu::BindGroup) {
        if let Some(bg) = &self.bg {
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, frame_bg, &[]);
            pass.set_bind_group(1, bg, &[]);
            pass.draw(0..6, 0..1);
        }
    }
}
