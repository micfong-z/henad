//! A thin enum over "whichever sim thread backend is driving the selected model".
//!
//! The CPU [`SimThread`] and the GPU `GpuSimThread` deliberately stay separate concrete types —
//! the CPU one steps a `SimState` once per loop iteration, the GPU one encodes N steps into a
//! single submission, and unifying them would mean rewriting the GPU batching controller. But
//! their *handles* were built to the same shape (`send` / `play` / `pause` / `step_once` /
//! `take_snapshot`), so the app can hold this enum and stay almost entirely backend-agnostic.
//!
//! The GPU arm does not exist on wasm: there is no OS thread to run it on, and the registry hands
//! out no GPU models there anyway (it is given no `GpuContext`).

use henad_compute::cpu::sim_thread::{SimCommand, SimThread};
use henad_compute::snapshot::Snapshot;

#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::GpuStats;
#[cfg(not(target_arch = "wasm32"))]
use henad_compute::gpu::sim_thread::GpuSimThread;

pub enum SimRunner {
    Cpu(SimThread),
    #[cfg(not(target_arch = "wasm32"))]
    Gpu(GpuSimThread),
}

impl SimRunner {
    /// Send a command that both backends understand. The GPU backend ignores the CPU pacing
    /// commands (`SetTargetTps`, `SetUncapped`, `SetTicksPerSnapshot`) — it paces itself with the
    /// adaptive batch-size controller instead.
    pub fn send(&mut self, cmd: SimCommand) {
        match self {
            Self::Cpu(t) => t.send(cmd),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Gpu(t) => t.send(cmd),
        }
    }

    pub fn play(&mut self) {
        match self {
            Self::Cpu(t) => t.play(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Gpu(t) => t.play(),
        }
    }

    pub fn pause(&mut self) {
        match self {
            Self::Cpu(t) => t.pause(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Gpu(t) => t.pause(),
        }
    }

    pub fn step_once(&mut self) {
        match self {
            Self::Cpu(t) => t.step_once(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Gpu(t) => t.step_once(),
        }
    }

    pub fn take_snapshot(&mut self) -> Option<Snapshot> {
        match self {
            Self::Cpu(t) => t.take_snapshot(),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Gpu(t) => t.take_snapshot(),
        }
    }

    /// Hands a consumed snapshot back for its buffers. A GPU snapshot owns no cell data, so there
    /// is nothing to reuse.
    pub fn recycle(&mut self, snap: Snapshot) {
        match self {
            Self::Cpu(t) => t.recycle(snap),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Gpu(_) => drop(snap),
        }
    }

    /// `Some` only for a GPU-backed model — the app uses this to decide whether to show the
    /// GPU-only batching controls.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn gpu_stats(&self) -> Option<GpuStats> {
        match self {
            Self::Cpu(_) => None,
            Self::Gpu(t) => Some(t.gpu_stats()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn as_gpu_mut(&mut self) -> Option<&mut GpuSimThread> {
        match self {
            Self::Cpu(_) => None,
            Self::Gpu(t) => Some(t),
        }
    }

    /// Drives the simulation synchronously on wasm, where there is no sim thread.
    #[cfg(target_arch = "wasm32")]
    pub fn update(&mut self, dt: f64) {
        match self {
            Self::Cpu(t) => t.update(dt),
        }
    }
}
