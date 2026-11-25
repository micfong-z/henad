//! The sim thread driving the loaded model, the CPU [`SimThread`] or the GPU [`GpuSimThread`], behind one handle.

use std::time::Duration;

use henad_compute::cpu::sim_thread::{SimCommand, SimThread};
use henad_compute::gpu::GpuStats;
use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
use henad_compute::runner::RunnerShutdown;
use henad_compute::snapshot::Snapshot;

#[cfg_attr(
    target_arch = "wasm32",
    expect(
        clippy::large_enum_variant,
        reason = "the app holds one runner at a time, so a box per variant would save nothing"
    )
)]
pub enum SimRunner {
    Cpu(SimThread),
    Gpu(GpuSimThread),
}

impl SimRunner {
    /// Stops the sim thread, and joins it once it stops or detaches it after `timeout`.
    pub fn stop(self, timeout: Duration) -> RunnerShutdown {
        match self {
            Self::Cpu(thread) => thread.stop(timeout),
            Self::Gpu(thread) => thread.stop(timeout),
        }
    }

    /// Sends a command to the sim thread.
    ///
    /// The GPU backend ignores `SetTicksPerSnapshot` and `SetLayout`, and sizes its batches itself.
    pub fn send(&mut self, command: SimCommand) {
        match self {
            Self::Cpu(thread) => thread.send(command),
            Self::Gpu(thread) => thread.send(command),
        }
    }

    pub fn take_snapshot(&mut self) -> Option<Snapshot> {
        match self {
            Self::Cpu(thread) => thread.take_snapshot(),
            Self::Gpu(thread) => thread.take_snapshot(),
        }
    }

    /// Takes back a consumed snapshot so its buffers can be reused. A GPU snapshot owns no cell data, so there
    /// is nothing to reuse.
    pub fn recycle(&mut self, snapshot: Snapshot) {
        match self {
            Self::Cpu(thread) => thread.recycle(snapshot),
            Self::Gpu(_) => drop(snapshot),
        }
    }

    /// Returns the GPU timing and batch size, `None` for a CPU model.
    ///
    /// The Pacing panel shows the batching controls instead of the CPU ones while this is `Some`.
    pub fn gpu_stats(&self) -> Option<GpuStats> {
        match self {
            Self::Cpu(_) => None,
            Self::Gpu(thread) => Some(thread.gpu_stats()),
        }
    }

    /// Sends `settings` to a GPU runner, and does nothing for a CPU runner.
    ///
    /// Each setting is sent, changed or not. A runner already at a setting stays there.
    pub fn set_gpu_batch(&mut self, settings: GpuBatchSettings) {
        if let Self::Gpu(thread) = self {
            thread.set_adaptive(settings.adaptive);
            thread.set_batch_size(settings.batch_size);
            thread.set_target_ms(settings.target_ms);
        }
    }

    /// Advances the simulation where the runner has no dedicated thread, and does nothing where it has one.
    pub fn update(&mut self, dt: f64) {
        match self {
            Self::Cpu(thread) => thread.update(dt),
            Self::Gpu(thread) => thread.update(dt),
        }
    }
}
