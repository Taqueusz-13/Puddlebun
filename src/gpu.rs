//! wgpu instance / adapter / device creation.

use crate::wgpu;
use anyhow::{Context, Result};

pub struct Gpu {
    #[allow(dead_code)]
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Gpu {
    pub fn instance(display: Option<Box<dyn wgpu::wgt::WgpuHasDisplayHandle>>) -> wgpu::Instance {
        let mut desc = match display {
            Some(d) => wgpu::InstanceDescriptor::new_with_display_handle(d),
            None => wgpu::InstanceDescriptor::new_without_display_handle(),
        };
        // Vulkan on Linux; WGPU_BACKEND can still override.
        desc.backends = wgpu::Backends::PRIMARY;
        let desc = desc.with_env();
        wgpu::Instance::new(desc)
    }

    pub async fn new(instance: wgpu::Instance, surface: Option<&wgpu::Surface<'_>>) -> Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: surface,
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .context("no suitable GPU adapter found (is a Vulkan driver installed?)")?;
        let info = adapter.get_info();
        log::info!("GPU: {} ({:?}, {})", info.name, info.backend, info.driver);

        let alimits = adapter.limits();
        let mut limits = wgpu::Limits::default();
        limits.max_storage_buffer_binding_size = alimits.max_storage_buffer_binding_size.min(512 << 20);
        limits.max_buffer_size = alimits.max_buffer_size.min(1 << 30);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("puddlebun device"),
                required_features: wgpu::Features::empty(),
                required_limits: limits,
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .context("couldn't create the GPU device")?;
        device.on_uncaptured_error(std::sync::Arc::new(|e| {
            log::error!("wgpu error: {e}");
        }));
        Ok(Self { instance, adapter, device, queue })
    }
}
