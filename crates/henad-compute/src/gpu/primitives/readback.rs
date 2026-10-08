//! Async readback of some `u32` counters reduced on the GPU.
//!
//! A GPU model answers `SimState::stats()` from these counters without copying its state back to the CPU. The map
//! starts right after submission and completes on a later loop iteration, and [`CounterReadback::poll`] never blocks.
//! A reported value is therefore a few milliseconds stale, as the display texture already is.

use std::mem::size_of;

/// State of a stats readback after a poll.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatsPoll {
    /// A readback has begun and not finished.
    Pending,
    /// No readback is in flight, and either no readback has begun or the latest one to finish succeeded.
    Landed,
    /// No readback is in flight, and the latest one to finish failed and left the earlier values in place.
    Failed,
}

/// A GPU-side `u32` accumulator of `count` counters, plus the staging buffer used to read it back
/// without blocking.
#[derive(Debug)]
pub struct CounterReadback {
    /// The reduce shader's output. Cleared to 0 each time, accumulated into, then copied out.
    storage: wgpu::Buffer,
    staging: wgpu::Buffer,
    /// `Some` while a `map_async` is in flight.
    pending: Option<flume::Receiver<Result<(), wgpu::BufferAsyncError>>>,
    /// Set once a fresh value is in `staging` and waiting to be mapped.
    copied: bool,
    /// Whether the latest map to finish failed.
    failed: bool,
    values: Vec<u32>,
}

impl CounterReadback {
    /// Creates the accumulator and its staging buffer for `count` counters.
    ///
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
            failed: false,
            values: vec![0; count],
        }
    }

    fn size(&self) -> u64 {
        (self.values.len() * size_of::<u32>()) as u64
    }

    /// Returns the accumulator, to bind as the reduce shader's `read_write` storage target.
    pub fn binding(&self) -> wgpu::BindingResource<'_> {
        self.storage.as_entire_binding()
    }

    /// Clears the accumulator to zero. Record it before the model's reduce pass.
    pub fn encode_clear(&self, encoder: &mut wgpu::CommandEncoder) {
        encoder.clear_buffer(&self.storage, 0, None);
    }

    /// Copies the accumulated totals into the staging buffer. Record it after the model's
    /// reduce pass, in the same encoder, where wgpu inserts the barrier between the pass and the copy.
    ///
    /// Note that the copy is skipped while a previous map is still in flight, since writing into a buffer that is
    /// mapped or pending a map is invalid. The sample is then dropped, and the stats repeat the older values with no
    /// error.
    pub fn encode_copy(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.pending.is_some() {
            return;
        }
        encoder.copy_buffer_to_buffer(&self.storage, 0, &self.staging, 0, self.size());
        self.copied = true;
    }

    /// Starts the async map. Call it once, immediately after submitting the encoder that
    /// [`Self::encode_copy`] was recorded into. Mapping any earlier races the copy.
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

    /// Returns whether a map started by [`Self::begin_map`] has not been consumed yet.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Consumes the in-flight map if it has completed, updating [`Self::values`], and returns the state of the
    /// readback after the poll. It never blocks.
    ///
    /// On native, `device.poll` runs wgpu's map callbacks. Call this on every loop iteration, whether or not a value
    /// is expected.
    pub fn poll(&mut self, device: &wgpu::Device) -> StatsPoll {
        let Some(rx) = self.pending.as_ref() else {
            return self.status();
        };

        drop(device.poll(wgpu::PollType::Poll));

        match rx.try_recv() {
            Ok(result) => {
                self.pending = None;
                self.failed = !self.finish_map(result);
            }
            // The callback was dropped without running, so no value can arrive.
            Err(flume::TryRecvError::Disconnected) => self.abandon_map(),
            Err(flume::TryRecvError::Empty) => {}
        }
        self.status()
    }

    /// Waits for the GPU to drain, then consumes the map, and returns the state of the readback after the wait. It is
    /// the blocking counterpart of [`Self::poll`].
    ///
    /// It is meant for one-shot snapshots and samples. Never call it from the hot batching loop.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn poll_blocking(&mut self, device: &wgpu::Device) -> StatsPoll {
        if let Some(rx) = self.pending.as_ref() {
            let received = match device.poll(wgpu::PollType::wait_indefinitely()) {
                Ok(_) => rx.recv().ok(),
                // The map can have finished before the wait failed.
                Err(_) => rx.try_recv().ok(),
            };
            match received {
                Some(result) => {
                    self.pending = None;
                    self.failed = !self.finish_map(result);
                }
                None => self.abandon_map(),
            }
        }
        self.status()
    }

    /// Polls once without waiting. WebGPU's `poll` is a no-op and the map resolves on the JS microtask
    /// queue, so a wait here hangs the tab. The value arrives on a later [`Self::poll`] instead.
    #[cfg(target_arch = "wasm32")]
    pub fn poll_blocking(&mut self, device: &wgpu::Device) -> StatsPoll {
        self.poll(device)
    }

    fn status(&self) -> StatsPoll {
        if self.pending.is_some() {
            StatsPoll::Pending
        } else if self.failed {
            StatsPoll::Failed
        } else {
            StatsPoll::Landed
        }
    }

    /// Gives up the map in flight as failed, and unmaps the staging buffer.
    ///
    /// Otherwise the buffer stays mapped or pending, and the next [`Self::encode_copy`] records a copy into it.
    fn abandon_map(&mut self) {
        self.pending = None;
        self.failed = true;
        self.staging.unmap();
    }

    /// Reads and unmaps the staging buffer after a completed `map_async`.
    fn finish_map(&mut self, result: Result<(), wgpu::BufferAsyncError>) -> bool {
        if let Err(err) = result {
            log::warn!("GPU stat readback failed: {err}");
            return false;
        }

        let slice = self.staging.slice(..);
        // Unmap on the error path too, or the next `map_async` finds the buffer still mapped.
        let data = match slice.get_mapped_range() {
            Ok(data) => data,
            Err(err) => {
                log::warn!("GPU stat readback failed: {err}");
                self.staging.unmap();
                return false;
            }
        };
        let words: &[u32] = bytemuck::cast_slice(&data);
        let n = self.values.len();
        self.values.copy_from_slice(&words[..n]);
        drop(data);
        self.staging.unmap();

        true
    }

    /// Most recently read-back values, all zero until the first readback completes.
    pub fn values(&self) -> &[u32] {
        &self.values
    }

    /// Most recently read-back values, each word's bits reinterpreted as an `f32`, for
    /// [`crate::gpu::primitives::reduce`].
    pub fn values_f32(&self) -> impl Iterator<Item = f32> + '_ {
        self.values.iter().copied().map(f32::from_bits)
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::{CounterReadback, StatsPoll};
    use crate::gpu::{GpuContext, headless_context};

    /// Writes `values` into the accumulator, submits their copy to the staging buffer and begins the map.
    fn begin_read_back(ctx: &GpuContext, readback: &mut CounterReadback, values: &[u32]) {
        ctx.queue
            .write_buffer(&readback.storage, 0, bytemuck::cast_slice(values));
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        readback.encode_copy(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        readback.begin_map();
    }

    #[test]
    fn a_completed_map_polls_as_landed() {
        let Some(ctx) = headless_context("henad_readback_landed_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_completed_map_polls_as_landed: no adapter");
            return;
        };
        let mut readback = CounterReadback::new(&ctx.device, "henad_readback_landed_test", 3);

        begin_read_back(&ctx, &mut readback, &[1, 2, 3]);
        assert!(readback.is_pending(), "a begun map is pending");
        assert_eq!(readback.poll_blocking(&ctx.device), StatsPoll::Landed);
        assert_eq!(readback.values(), [1, 2, 3]);

        begin_read_back(&ctx, &mut readback, &[4, 5, 6]);
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the copy runs");
        assert_eq!(
            readback.poll(&ctx.device),
            StatsPoll::Landed,
            "a map the device completed lands on the next poll"
        );
        assert_eq!(readback.values(), [4, 5, 6]);
        assert_eq!(
            readback.poll(&ctx.device),
            StatsPoll::Landed,
            "a poll with nothing in flight reports the latest map"
        );
    }

    #[test]
    fn a_failed_map_polls_as_failed_and_keeps_the_values() {
        let Some(ctx) = headless_context("henad_readback_failed_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_failed_map_polls_as_failed_and_keeps_the_values: no adapter");
            return;
        };
        let mut readback = CounterReadback::new(&ctx.device, "henad_readback_failed_test", 3);
        begin_read_back(&ctx, &mut readback, &[1, 2, 3]);
        assert_eq!(readback.poll_blocking(&ctx.device), StatsPoll::Landed);

        // A destroyed staging buffer rejects the map.
        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        readback.encode_copy(&mut encoder);
        ctx.queue.submit(Some(encoder.finish()));
        readback.staging.destroy();
        readback.begin_map();

        assert_eq!(readback.poll_blocking(&ctx.device), StatsPoll::Failed);
        assert_eq!(readback.values(), [1, 2, 3], "a failed map leaves the values it found");
        assert_eq!(readback.poll(&ctx.device), StatsPoll::Failed);
        assert!(ctx.faults.take().is_some(), "the device reports the refused map");
    }

    /// A map given up without being read must not leave the staging buffer mapped or pending. The next copy into it
    /// would be a validation error.
    #[test]
    fn an_abandoned_map_leaves_the_next_readback_working() {
        let Some(ctx) = headless_context("henad_readback_abandoned_test", wgpu::Features::empty()) else {
            log::warn!("skipping an_abandoned_map_leaves_the_next_readback_working: no adapter");
            return;
        };
        let mut readback = CounterReadback::new(&ctx.device, "henad_readback_abandoned_test", 3);

        begin_read_back(&ctx, &mut readback, &[1, 2, 3]);
        readback.abandon_map();
        assert_eq!(
            readback.poll(&ctx.device),
            StatsPoll::Failed,
            "an abandoned map polls as failed"
        );

        // This time the map completes before it is given up.
        begin_read_back(&ctx, &mut readback, &[4, 5, 6]);
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the map runs");
        readback.abandon_map();

        begin_read_back(&ctx, &mut readback, &[7, 8, 9]);
        assert_eq!(readback.poll_blocking(&ctx.device), StatsPoll::Landed);
        assert_eq!(readback.values(), [7, 8, 9]);
        assert!(ctx.faults.take().is_none(), "a copy went into a mapped staging buffer");
    }
}
