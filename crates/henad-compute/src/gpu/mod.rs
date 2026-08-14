//! GPU engine machinery, the sibling of [`crate::cpu`], for models whose state lives in GPU
//! buffers and never round-trips to the CPU.
//!
//! Nothing here ever *creates* a `wgpu::Device`. It is injected via [`GpuContext`], cloned from
//! whoever owns acquisition, which is what stops this crate depending on egui/eframe.
//!
//! Concrete models live in `henad-models`, as CPU models do. A model contributes shaders, seed
//! data and metadata. Every wgpu object is built here.

pub mod agent_engine;
pub mod grid_engine;
pub mod limits;
pub mod primitives;
pub mod sim_thread;
pub mod timing;
pub mod view;

#[cfg(test)]
mod test_support;

pub use agent_engine::{GpuAgentModelDescriptor, GpuAgentState};
pub use grid_engine::{GpuGridModelDescriptor, GpuGridState};
pub use primitives::spatial_hash::{GpuSpatialHash, HashGrid};
pub use sim_thread::{GpuSimState, GpuStats};
pub use view::agents::GpuAgents;
pub use view::display::{DisplayTarget, GpuDisplay};

#[cfg(test)]
use test_support::headless_context;

#[cfg(not(target_arch = "wasm32"))]
pub use sim_thread::GpuSimThread;

/// Injected GPU handles. Cheap to clone.
///
/// `target_format` is part of the context rather than a per-call argument because a model builds
/// its display render pipeline once, and a pipeline is tied to its colour target format.
#[derive(Clone)]
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,
}

impl GpuContext {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, target_format: wgpu::TextureFormat) -> Self {
        Self {
            device,
            queue,
            target_format,
        }
    }
}
