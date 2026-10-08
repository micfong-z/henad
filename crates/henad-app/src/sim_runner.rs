//! The sim thread driving the loaded model, the CPU [`SimThread`] or the GPU [`GpuSimThread`], behind one handle.

use henad_compute::cpu::sim_thread::{SimCommand, SimThread};
use henad_compute::gpu::GpuStats;
use henad_compute::gpu::sim_thread::GpuSimThread;
use henad_compute::snapshot::Snapshot;

pub enum SimRunner {
    Cpu(SimThread),
    Gpu(GpuSimThread),
}

impl SimRunner {
    /// Sends a command to the sim thread.
    ///
    /// The GPU backend ignores the pacing and layout commands (`SetTargetTps`, `SetUncapped`, `SetTicksPerSnapshot`
    /// and `SetLayout`), and paces itself by its batch size.
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

    /// Takes back a consumed snapshot so its buffers can be reused. A GPU snapshot owns no cell data, so there
    /// is nothing to reuse.
    pub fn recycle(&mut self, snap: Snapshot) {
        match self {
            Self::Cpu(t) => t.recycle(snap),
            Self::Gpu(_) => drop(snap),
        }
    }

    /// Returns the GPU timing and batch size, `None` for a CPU model.
    ///
    /// The Pacing panel shows the batching controls instead of the CPU ones while this is `Some`.
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

    /// Advances the simulation where the runner has no dedicated thread, and does nothing where it has one.
    pub fn update(&mut self, dt: f64) {
        match self {
            Self::Cpu(t) => t.update(dt),
            Self::Gpu(t) => t.update(dt),
        }
    }
}
