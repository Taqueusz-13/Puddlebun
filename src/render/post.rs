//! Bloom chain and the final tonemap.

use super::util::*;
use super::HDR_FORMAT;
use crate::wgpu;
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Default)]
pub struct PostParams {
    pub texel: [f32; 4],
    pub exposure: f32,
    pub bloom: f32,
    pub time: f32,
    pub encode_srgb: f32,
    pub vignette: f32,
    pub grain: f32,
    pub pad: [f32; 2],
}

struct PassRes {
    bg: wgpu::BindGroup,
    buf: wgpu::Buffer,
    params: PostParams,
}

pub struct Post {
    bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    down: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    tonemap: wgpu::RenderPipeline,
    levels: Vec<(wgpu::Texture, wgpu::TextureView)>,
    down_passes: Vec<PassRes>,
    up_passes: Vec<PassRes>,
    tone: Option<PassRes>,
    dummy: wgpu::TextureView,
    encode_srgb: bool,
}

impl Post {
    pub fn new(device: &wgpu::Device, output: wgpu::TextureFormat) -> Self {
        let f = wgpu::ShaderStages::FRAGMENT;
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post bgl"),
            entries: &[texture_entry(0, f), sampler_entry(1, f), uniform_entry(2, f, false), texture_entry(3, f)],
        });
        let module = shader(device, "post", &[COMMON_WGSL, include_str!("../shaders/post.wgsl")]);
        let pl = layout(device, "post layout", &[&bgl]);
        let down = render_pipeline(device, "bloom down", &pl, &module, "vs_full", "fs_down", &[], HDR_FORMAT, None, None);
        let up = render_pipeline(device, "bloom up", &pl, &module, "vs_full", "fs_up", &[], HDR_FORMAT, Some(ADDITIVE), None);
        let tonemap = render_pipeline(device, "tonemap", &pl, &module, "vs_full", "fs_tonemap", &[], output, None, None);
        let (_t, dummy) = texture(device, "post dummy", 1, 1, HDR_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING);
        Self {
            bgl,
            sampler: linear_sampler(device, wgpu::AddressMode::ClampToEdge),
            down,
            up,
            tonemap,
            levels: Vec::new(),
            down_passes: Vec::new(),
            up_passes: Vec::new(),
            tone: None,
            dummy,
            encode_srgb: !output.is_srgb(),
        }
    }

    fn pass(&self, device: &wgpu::Device, src: &wgpu::TextureView, bloom: &wgpu::TextureView, params: PostParams) -> PassRes {
        let buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("post params"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        buf.slice(..).get_mapped_range_mut().expect("mapped at creation").copy_from_slice(bytemuck::bytes_of(&params));
        buf.unmap();
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("post bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(src) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(bloom) },
            ],
        });
        PassRes { bg, buf, params }
    }

    pub fn resize(&mut self, device: &wgpu::Device, hdr: &wgpu::TextureView, w: u32, h: u32) {
        self.levels.clear();
        let (mut lw, mut lh) = ((w / 2).max(1), (h / 2).max(1));
        for i in 0..6 {
            let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
            self.levels.push(texture(device, &format!("bloom {i}"), lw, lh, HDR_FORMAT, usage));
            if lw <= 8 || lh <= 8 {
                break;
            }
            lw = (lw / 2).max(1);
            lh = (lh / 2).max(1);
        }
        let n = self.levels.len();
        let mut down = Vec::new();
        for i in 0..n {
            let (src, sw, sh, karis) = if i == 0 { (hdr, w, h, 1.0) } else {
                let t = &self.levels[i - 1].0;
                (&self.levels[i - 1].1, t.width(), t.height(), 0.0)
            };
            let p = PostParams { texel: [1.0 / sw as f32, 1.0 / sh as f32, karis, 1.0], ..Default::default() };
            down.push(self.pass(device, src, &self.dummy, p));
        }
        let mut up = Vec::new();
        for i in (1..n).rev() {
            let t = &self.levels[i].0;
            let p = PostParams { texel: [1.0 / t.width() as f32, 1.0 / t.height() as f32, 0.0, 1.0], ..Default::default() };
            up.push(self.pass(device, &self.levels[i].1, &self.dummy, p));
        }
        let tone = self.pass(device, hdr, &self.levels[0].1, PostParams::default());
        self.down_passes = down;
        self.up_passes = up;
        self.tone = Some(tone);
    }

    pub fn update(&mut self, queue: &wgpu::Queue, exposure: f32, bloom: f32, time: f32) {
        if let Some(t) = &mut self.tone {
            t.params.exposure = exposure;
            t.params.bloom = bloom;
            t.params.time = time;
            t.params.encode_srgb = if self.encode_srgb { 1.0 } else { 0.0 };
            t.params.vignette = 0.28;
            t.params.grain = 0.018;
            queue.write_buffer(&t.buf, 0, bytemuck::bytes_of(&t.params));
        }
    }

    pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, target: &wgpu::TextureView) {
        let n = self.levels.len();
        let run = |encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, pipe: &wgpu::RenderPipeline, bg: &wgpu::BindGroup, load: wgpu::LoadOp<wgpu::Color>| {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("post pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
        };
        for i in 0..n {
            run(encoder, &self.levels[i].1, &self.down, &self.down_passes[i].bg, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
        }
        for (k, i) in (1..n).rev().enumerate() {
            run(encoder, &self.levels[i - 1].1, &self.up, &self.up_passes[k].bg, wgpu::LoadOp::Load);
        }
        if let Some(t) = &self.tone {
            run(encoder, target, &self.tonemap, &t.bg, wgpu::LoadOp::Clear(wgpu::Color::BLACK));
        }
    }
}
