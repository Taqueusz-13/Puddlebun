//! Asynchronous GPU → CPU readback of raindrop impacts (for the audio engine).
//! A small ring of staging buffers so the GPU never waits on the CPU.

use super::rain::{GpuImpact, AUDIO_CAPACITY, AUDIO_EVENT_SIZE, COUNTERS_SIZE};
use crate::wgpu;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

const RING: usize = 3;
const FREE: u32 = 0;
const IN_FLIGHT: u32 = 1;
const READY: u32 = 2;

pub struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU32>,
    /// seconds of simulation this slot's events cover
    dt: f32,
}

#[derive(Default, Clone)]
pub struct Impacts {
    pub events: Vec<GpuImpact>,
    pub total: u32,
    #[allow(dead_code)]
    pub puddle: u32,
    pub dt: f32,
}

pub struct Readback {
    slots: Vec<Slot>,
    encoded: Option<usize>,
}

impl Readback {
    pub fn new(device: &wgpu::Device) -> Self {
        let size = COUNTERS_SIZE + AUDIO_CAPACITY as u64 * AUDIO_EVENT_SIZE;
        let slots = (0..RING)
            .map(|_| Slot {
                buffer: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("impact readback"),
                    size,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                state: Arc::new(AtomicU32::new(FREE)),
                dt: 0.0,
            })
            .collect();
        Self { slots, encoded: None }
    }

    /// Records copies of the counters + events into a free staging buffer.
    pub fn encode(&mut self, encoder: &mut wgpu::CommandEncoder, counters: &wgpu::Buffer, events: &wgpu::Buffer, dt: f32) {
        self.encoded = None;
        if let Some(i) = self.slots.iter().position(|s| s.state.load(Ordering::Acquire) == FREE) {
            let slot = &mut self.slots[i];
            encoder.copy_buffer_to_buffer(counters, 0, &slot.buffer, 0, COUNTERS_SIZE);
            encoder.copy_buffer_to_buffer(events, 0, &slot.buffer, COUNTERS_SIZE, AUDIO_CAPACITY as u64 * AUDIO_EVENT_SIZE);
            slot.state.store(IN_FLIGHT, Ordering::Release);
            slot.dt = dt;
            self.encoded = Some(i);
        }
    }

    /// Call right after `queue.submit`.
    pub fn after_submit(&mut self) {
        if let Some(i) = self.encoded.take() {
            let state = self.slots[i].state.clone();
            self.slots[i].buffer.map_async(wgpu::MapMode::Read, .., move |r| {
                state.store(if r.is_ok() { READY } else { FREE }, Ordering::Release);
            });
        }
    }

    /// Collects every slot whose data has arrived.
    pub fn collect(&mut self, device: &wgpu::Device) -> Vec<Impacts> {
        let _ = device.poll(wgpu::PollType::Poll);
        let mut out = Vec::new();
        for slot in &mut self.slots {
            if slot.state.load(Ordering::Acquire) != READY {
                continue;
            }
            {
                let Ok(view) = slot.buffer.get_mapped_range(..) else {
                    continue;
                };
                let bytes: &[u8] = &view;
                let counters: &[u32] = bytemuck::cast_slice(&bytes[..COUNTERS_SIZE as usize]);
                let n = counters[1].min(AUDIO_CAPACITY) as usize;
                let ev: &[GpuImpact] = bytemuck::cast_slice(&bytes[COUNTERS_SIZE as usize..COUNTERS_SIZE as usize + n * AUDIO_EVENT_SIZE as usize]);
                out.push(Impacts { events: ev.to_vec(), total: counters[2], puddle: counters[3], dt: slot.dt });
            }
            slot.buffer.unmap();
            slot.state.store(FREE, Ordering::Release);
        }
        out
    }

    /// Blocks until all in-flight readbacks are done (used by headless mode).
    pub fn wait_all(&mut self, device: &wgpu::Device) -> Vec<Impacts> {
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        self.collect(device)
    }
}
