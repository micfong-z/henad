//! Async readback of some `u32` counters reduced on the GPU.
//!
//! This exists so a GPU model can answer `SimState::stats()` without ever copying the grid back
//! to the CPU — reducing on-GPU and reading back a few bytes costs nothing measurable next to a
//! full grid readback.
//!
//! # Why the map is asynchronous
//!
//! Blocking on `map_async` right after submission would stall the sim thread at the display
//! cadence, capping throughput at roughly one in-flight batch per frame. Instead the map is
//! *started* right after submission and *completed* on some later loop iteration, whenever the
//! GPU gets around to it ([`CounterReadback::poll`] never blocks). The value a model reports is
//! therefore a few milliseconds stale, same as the display texture already accepts.

use std::mem::size_of;

/// A GPU-side `u32` accumulator of `count` counters, plus the staging buffer used to read it back
/// without blocking.
pub struct CounterReadback {
    /// The reduce shader's output. Cleared to 0 each time, accumulated into, then copied out.
    storage: wgpu::Buffer,
    staging: wgpu::Buffer,
    /// `Some` while a `map_async` is in flight.
    pending: Option<flume::Receiver<Result<(), wgpu::BufferAsyncError>>>,
    /// Whether a fresh value has been copied into `staging` and is waiting to be mapped.
    copied: bool,
    values: Vec<u32>,
}

impl CounterReadback {
    /// `count` must match the length of the `atomic<u32>` array the reduce shader declares.
    pub fn new(device: &wgpu::Device, label: &str, count: usize) -> Self {
        let size = (count * size_of::<u32>()) as u64;
        let storage = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{label}_storage")),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("{label}_staging")),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            storage,
            staging,
            pending: None,
            copied: false,
            values: vec![0; count],
        }
    }

    fn size(&self) -> u64 {
        (self.values.len() * size_of::<u32>()) as u64
    }

    /// Bind this as the reduce shader's `read_write` storage target.
    pub fn binding(&self) -> wgpu::BindingResource<'_> {
        self.storage.as_entire_binding()
    }

    /// Zero the accumulator. Must be recorded *before* the model's reduce pass.
    pub fn encode_clear(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.storage, 0, None);
    }

    /// Copy the accumulated totals into the staging buffer. Must be recorded *after* the model's
    /// reduce pass, in the same encoder (wgpu inserts the pass/copy barrier for us).
    ///
    /// Skipped while a previous map is still in flight — writing into a buffer that is mapped or
    /// pending-map is invalid, and dropping a sample is harmless (the next display tick takes
    /// another one).
    pub fn encode_copy(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.pending.is_some() {
            return;
        }
        encoder.copy_buffer_to_buffer(&self.storage, 0, &self.staging, 0, self.size());
        self.copied = true;
    }

    /// Start the async map. Call once, immediately after submitting the encoder that
    /// [`Self::encode_copy`] was recorded into — mapping before that submission would race the
    /// copy.
    pub fn begin_map(&mut self) {
        if self.pending.is_some() || !self.copied {
            return;
        }
        self.copied = false;
        let (tx, rx) = flume::bounded(1);
        self.staging.slice(..).map_async(wgpu::MapMode::Read, move |result| {
            drop(tx.send(result));
        });
        self.pending = Some(rx);
    }

    /// Non-blocking. If the in-flight map has completed, consume it and update [`Self::values`].
    ///
    /// `device.poll` is what actually runs wgpu's map callbacks on native, so this must be called
    /// on every loop iteration, not only when a value is expected.
    /// Returns whether a fresh value landed in [`Self::values`].
    pub fn poll(&mut self, device: &wgpu::Device) -> bool {
        let Some(rx) = self.pending.as_ref() else {
            return false;
        };

        drop(device.poll(wgpu::PollType::Poll));

        let Ok(result) = rx.try_recv() else {
            return false;
        };
        self.pending = None;
        self.finish_map(result)
    }

    /// Blocking counterpart to [`Self::poll`]: waits for the GPU to drain, then consumes the map.
    ///
    /// Only for one-shot snapshots (initial load, pause, step-once). Never call from the hot
    /// batching loop.
    /// Returns whether a fresh value landed in [`Self::values`].
    pub fn poll_blocking(&mut self, device: &wgpu::Device) -> bool {
        let Some(rx) = self.pending.take() else {
            return false;
        };
        if device.poll(wgpu::PollType::wait_indefinitely()).is_err() {
            return false;
        }
        let Ok(result) = rx.recv() else {
            return false;
        };
        self.finish_map(result)
    }

    /// Reads and unmaps the staging buffer after a completed `map_async`.
    fn finish_map(&mut self, result: Result<(), wgpu::BufferAsyncError>) -> bool {
        if let Err(err) = result {
            log::warn!("GPU stat readback failed: {err}");
            return false;
        }

        let slice = self.staging.slice(..);
        let data = slice.get_mapped_range();
        let words: &[u32] = bytemuck::cast_slice(&data);
        let n = self.values.len();
        self.values.copy_from_slice(&words[..n]);
        drop(data);
        self.staging.unmap();

        true
    }

    /// The most recently read-back values. All zero until the first readback completes.
    pub fn values(&self) -> &[u32] {
        &self.values
    }

    /// The most recently read-back values as floats, for [`crate::gpu::primitives::reduce`].
    /// The staging path only moves 4-byte words, so it is shared rather than duplicated
    /// per element type.
    pub fn values_f32(&self) -> impl Iterator<Item = f32> + '_ {
        self.values.iter().copied().map(f32::from_bits)
    }
}
