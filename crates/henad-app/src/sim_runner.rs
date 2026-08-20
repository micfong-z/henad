//! A thin enum over "whichever sim thread backend is driving the selected model".
//!
//! The CPU [`SimThread`] and the GPU [`GpuSimThread`] stay separate concrete types, since one steps
//! a `SimState` per loop iteration and the other encodes N steps into one submission. Their
//! *handles* share a shape, so the app can hold this enum and stay backend-agnostic.

use henad_compute::cpu::sim_thread::{SimCommand, SimThread};
use henad_compute::gpu::GpuStats;
use henad_compute::gpu::sim_thread::GpuSimThread;
use henad_compute::snapshot::Snapshot;

pub enum SimRunner {
    Cpu(SimThread),
    Gpu(GpuSimThread),
}

impl SimRunner {
    /// Send a command that both backends understand. The GPU backend ignores the CPU pacing
    /// commands (`SetTargetTps`, `SetUncapped`, `SetTicksPerSnapshot`) and paces itself with the
    /// adaptive batch-size controller instead.
    pub fn send(&mut self, cmd: SimCommand) {
        match self {
            Self::Cpu(t) => t.send(cmd),
            Self::Gpu(t) => t.send(cmd),
        }
    }

    pub fn play(&mut self) {
        match self {
            Self::Cpu(t) => t.play(),
            Self::Gpu(t) => t.play(),
        }
    }

    pub fn pause(&mut self) {
        match self {
            Self::Cpu(t) => t.pause(),
            Self::Gpu(t) => t.pause(),
        }
    }

    pub fn step_once(&mut self) {
        match self {
            Self::Cpu(t) => t.step_once(),
            Self::Gpu(t) => t.step_once(),
        }
    }

    pub fn take_snapshot(&mut self) -> Option<Snapshot> {
        match self {
            Self::Cpu(t) => t.take_snapshot(),
            Self::Gpu(t) => t.take_snapshot(),
        }
    }

    /// Hands a consumed snapshot back for its buffers. A GPU snapshot owns no cell data, so there
    /// is nothing to reuse.
    pub fn recycle(&mut self, snap: Snapshot) {
        match self {
            Self::Cpu(t) => t.recycle(snap),
            Self::Gpu(_) => drop(snap),
        }
    }

    /// `Some` only for a GPU-backed model, which is how the app decides whether to show the
    /// GPU-only batching controls.
    pub fn gpu_stats(&self) -> Option<GpuStats> {
        match self {
            Self::Cpu(_) => None,
            Self::Gpu(t) => Some(t.gpu_stats()),
        }
    }

    pub fn as_gpu_mut(&mut self) -> Option<&mut GpuSimThread> {
        match self {
            Self::Cpu(_) => None,
            Self::Gpu(t) => Some(t),
        }
    }

    /// Drives the simulation on wasm, where neither backend has a thread of its own.
    #[cfg(target_arch = "wasm32")]
    pub fn update(&mut self, dt: f64) {
        match self {
            Self::Cpu(t) => t.update(dt),
            Self::Gpu(t) => t.update(dt),
        }
    }
}
