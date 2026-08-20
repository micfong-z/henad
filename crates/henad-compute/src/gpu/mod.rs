//! GPU engine machinery, the sibling of [`crate::cpu`], for models whose state lives in GPU
//! buffers and never round-trips to the CPU.
//!
//! Nothing here ever *creates* a `wgpu::Device`. It is injected via [`GpuContext`], cloned from
//! whoever owns acquisition, which is what stops this crate depending on egui/eframe.
//!
//! Concrete models live in `henad-models`, as CPU models do. A model contributes shaders, seed
//! data and metadata. Every wgpu object is built here.

pub mod agent_engine;
pub mod capacity;
pub mod fault;
pub mod grid_engine;
pub mod limits;
pub mod primitives;
pub mod sim_thread;
pub mod timing;
pub mod view;

#[cfg(test)]
mod tests;

pub use agent_engine::{GpuAgentModelDescriptor, GpuAgentState};
pub use capacity::Demand;
pub use grid_engine::{GpuGridModelDescriptor, GpuGridState};
pub use primitives::spatial_hash::{GpuSpatialHash, HashGrid};
pub use sim_thread::{GpuSimState, GpuStats};
pub use view::agents::GpuAgents;
pub use view::display::{DisplayTarget, GpuDisplay};

#[cfg(test)]
use tests::support::headless_context;

pub use sim_thread::GpuSimThread;

use crate::fault::{Fault, FaultSink};

/// Steps one command buffer may hold.
pub const MAX_STEPS_PER_SUBMISSION: u32 = 64;

/// Injected GPU handles. Cheap to clone.
///
/// `target_format` is part of the context rather than a per-call argument because a model builds
/// its display render pipeline once, and a pipeline is tied to its colour target format.
#[derive(Clone)]
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub target_format: wgpu::TextureFormat,
    /// Landing spot for an error nothing else caught. See [`GpuContext::new`].
    pub faults: FaultSink,
}

impl GpuContext {
    /// Also takes over the device's error handling. Left to wgpu, every error is fatal.
    ///
    /// Errors raised inside a [`fault::catching_on`] go to that scope. Everything else, including
    /// egui's own rendering on the same device, lands in `faults` for the host to pick up. On the
    /// web this is the only route. [`fault::catching_on`] pushes no scopes there.
    ///
    /// A `GPUInternalError` still ends the web build. wgpu converts an error with
    /// `Error::from_js`. Anything other than a `GPUValidationError` or a `GPUOutOfMemoryError`
    /// panics there. A model provokes those two, and the handler reports them normally.
    pub fn new(
        device: wgpu::Device,
        queue: wgpu::Queue,
        target_format: wgpu::TextureFormat,
        faults: FaultSink,
    ) -> Self {
        let sink = faults.clone();
        device.on_uncaptured_error(std::sync::Arc::new(move |error: wgpu::Error| {
            log::error!("unhandled GPU error: {error}");
            sink.set_once(Fault::device("running on the GPU", error));
        }));
        Self {
            device,
            queue,
            target_format,
            faults,
        }
    }
}
