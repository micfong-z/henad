//! GPU engine machinery: the sibling of [`crate::grid_engine`] / [`crate::sim_thread`] for models
//! whose state lives in GPU buffers and never round-trips to the CPU.
//!
//! # Device ownership
//!
//! Nothing in here ever *creates* a `wgpu::Device`. The device and queue are injected via
//! [`GpuContext`], cloned from whoever owns GPU acquisition — today that is egui's `RenderState`
//! in `henad-app`; later it could be a headless CLI runner. Keeping acquisition out of this crate
//! is what stops `henad-compute` from growing a dependency on egui/eframe.
//!
//! # What lives where
//!
//! This module holds the *reusable* half: the injected context, the fullscreen-triangle display
//! target that turns a GPU texture into something the UI can sample ([`display`]), the async
//! counter readback used for on-GPU stat reduction ([`readback`]), GPU timing plus the
//! adaptive-batching controller ([`timing`]), the batching sim thread ([`sim_thread`]), and the
//! generic engine that builds a runnable state out of a `GpuGridModel` ([`gpu_grid_engine`]).
//!
//! Concrete GPU models live in `henad-models`, exactly as CPU models live there and lean on
//! [`crate::grid_engine`]. The split follows the CPU one: a model contributes shaders, seed data,
//! and metadata; every wgpu object — buffers, layouts, pipelines, bind groups — is built here.

pub mod agent_display;
pub mod dispatch;
pub mod display;
pub mod gpu_grid_engine;
pub mod limits;
pub mod pipeline;
pub mod prefix_scan;
pub mod readback;
pub mod reduce;
pub mod sim_thread;
pub mod spatial_hash;
pub mod timing;

#[cfg(test)]
mod test_support;

pub use agent_display::GpuAgents;
pub use display::{DisplayTarget, GpuDisplay};
pub use gpu_grid_engine::{GpuGridModelDescriptor, GpuGridState};
pub use sim_thread::{GpuSimState, GpuStats};
pub use spatial_hash::{GpuSpatialHash, HashGrid};

#[cfg(test)]
use test_support::headless_context;

#[cfg(not(target_arch = "wasm32"))]
pub use sim_thread::GpuSimThread;

/// Injected GPU handles. Cheap to clone — `Device` and `Queue` are refcounted `Send + Sync`
/// handles, not owned resources — so every GPU model factory can capture its own clone.
///
/// `target_format` is the surface format the final render pass writes to. It is part of the
/// context (rather than passed per-call) because a GPU model builds its display render pipeline
/// once, at construction, and a pipeline is tied to its colour target format.
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
