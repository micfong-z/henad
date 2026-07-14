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
//! single-`u32` readback used for on-GPU stat reduction ([`readback`]), GPU timing plus the
//! adaptive-batching controller ([`timing`]), and the batching sim thread ([`sim_thread`]).
//!
//! Concrete GPU models (shaders, pipelines, bind groups, what a "cell" means) live in
//! `henad-models`, exactly as CPU models live there and lean on [`crate::grid_engine`].

pub mod display;
pub mod readback;
pub mod sim_thread;
pub mod timing;

pub use display::{DisplayTarget, GpuDisplay};
pub use sim_thread::{GpuSimState, GpuStats};

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
