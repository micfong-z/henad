//! GPU timing (diagnostic) and the adaptive-batching controller (load-bearing).
//!
//! These two are deliberately separate concerns. [`TimestampQuery`] measures true GPU execution
//! time and is *only* surfaced as a readout. The controller ([`ema_update`] / [`next_batch_size`])
//! runs off wall-clock time instead — see [`crate::gpu::sim_thread`] for why.

use std::time::Duration;

/// Default steps-per-submission in fixed mode, tunable at runtime from the UI.
pub const DEFAULT_BATCH_SIZE: u32 = 64;

/// Default per-batch wall-clock budget in adaptive mode, in milliseconds.
///
/// ~8ms leaves roughly
/// half of a 60fps (16.6ms) frame free, so a single batch submission is unlikely to be the thing
/// that makes egui miss a frame even when it lands right before an egui submission on the same
/// queue.
pub const DEFAULT_TARGET_MS: f64 = 8.0;

/// Smoothing factor for the adaptive controller's EMA of `time_per_step`.
///
/// 0.25 was chosen to react within a handful of batches to a real change in per-step cost (e.g.
/// after a reseed to a denser pattern, or a grid resize), while still averaging out per-batch
/// noise from OS scheduling jitter on the sim thread — a pure single-sample estimate was found
/// to make the controller's output jump around too much batch-to-batch.
pub const ADAPTIVE_EMA_ALPHA: f64 = 0.25;

/// Hard upper bound on the adaptive controller's output.
///
/// Independent of the budget/cost
/// division. Without this, a very cheap grid (tiny grid, or a GPU idling with headroom) could
/// drive `target_ms / time_per_step` into tens of thousands of steps per batch; besides being
/// unnecessary (the point is just to stay under budget), an oversized batch also means the
/// controller reacts slowly to a subsequent slowdown (e.g. resizing to a much bigger grid),
/// since that oversized batch is already committed and won't be measured until it completes.
/// 4096 is comfortably above `DEFAULT_BATCH_SIZE` (64) and the old fixed-mode slider's max
/// (2000) so it rarely binds in practice, while still bounding worst-case encode/submit latency.
/// `pub` so the UI's read-only "live batch size" slider can use the same bound as its range,
/// rather than risk silently clamping (and thus misreporting) a controller output above it.
pub const MAX_BATCH_SIZE: u32 = 4096;

/// GPU timestamp-query resources, created only if the device supports `Features::TIMESTAMP_QUERY`.
pub struct TimestampQuery {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    readback_buffer: wgpu::Buffer,
    /// Nanoseconds per timestamp tick, from `Queue::get_timestamp_period`.
    period_ns: f32,
}

impl TimestampQuery {
    const BUFFER_SIZE: u64 = 2 * std::mem::size_of::<u64>() as u64;

    /// Returns `None` when the device lacks `TIMESTAMP_QUERY`, in which case timing is simply
    /// not reported.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("henad_gpu_timestamp_query_set"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_gpu_timestamp_resolve"),
            size: Self::BUFFER_SIZE,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("henad_gpu_timestamp_readback"),
            size: Self::BUFFER_SIZE,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Some(Self {
            query_set,
            resolve_buffer,
            readback_buffer,
            period_ns: queue.get_timestamp_period(),
        })
    }

    /// The query set a model stamps its first/last step pass into (indices 0 and 1).
    pub fn query_set(&self) -> &wgpu::QuerySet {
        &self.query_set
    }

    /// Resolves the timestamps written by `write_submission` into `readback_buffer`, in a
    /// *separate* command buffer submitted only after `write_submission` has fully completed on
    /// the GPU.
    ///
    /// This split is required, not cosmetic: recording `resolve_query_set` into the *same*
    /// command buffer as the timestamp writes (the original implementation) is accepted by wgpu
    /// but is unreliable in practice — at least on the Metal backend, the driver's counter
    /// sample buffer is only guaranteed populated after the writing command buffer's completion
    /// handler has run, so a resolve issued earlier in the same command buffer can read back
    /// whatever value happened to be resident from an *earlier* submission. Confirmed empirically
    /// (see `tests::gpu_timing_readback_is_stable_over_many_batches`, which failed 197/200 times
    /// with the single-submission version — reading a bit-for-bit stale `end` timestamp from one
    /// submission prior, which is frequently *less than* the fresh `start` timestamp and so
    /// saturates to a reported 0). Waiting for the writing submission before resolving in a
    /// follow-up submission eliminates the staleness entirely.
    pub fn resolve_after(&self, device: &wgpu::Device, queue: &wgpu::Queue, write_submission: wgpu::SubmissionIndex) {
        drop(device.poll(wgpu::PollType::Wait {
            submission_index: Some(write_submission),
            timeout: None,
        }));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_timestamp_resolve_encoder"),
        });
        encoder.resolve_query_set(&self.query_set, 0..2, &self.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(&self.resolve_buffer, 0, &self.readback_buffer, 0, Self::BUFFER_SIZE);
        queue.submit(Some(encoder.finish()));
    }

    /// Blocking readback of the two timestamps written by the last stamped batch, called at most
    /// once per stats interval — the stall this introduces is negligible next to a sim running at
    /// thousands of TPS.
    pub fn read_gpu_us_per_step(&self, device: &wgpu::Device, batch_size: u32) -> Option<f64> {
        let slice = self.readback_buffer.slice(..);
        let (tx, rx) = flume::bounded(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            drop(tx.send(result));
        });
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;

        let data = slice.get_mapped_range();
        let ticks: &[u64] = bytemuck::cast_slice(&data);
        let (start, end) = (ticks[0], ticks[1]);
        drop(data);
        self.readback_buffer.unmap();

        let elapsed_ns = end.saturating_sub(start) as f64 * f64::from(self.period_ns);
        Some(elapsed_ns / 1000.0 / f64::from(batch_size.max(1)))
    }
}

/// Exponential moving average update.
///
/// `prev` is `None` on the very first sample (in which case
/// the sample seeds the EMA directly, rather than blending against an arbitrary starting value),
/// `Some` on every subsequent call.
pub fn ema_update(prev: Option<f64>, sample: f64, alpha: f64) -> f64 {
    match prev {
        Some(prev) => alpha.mul_add(sample, (1.0 - alpha) * prev),
        None => sample,
    }
}

/// Proportional controller for the batch size.
///
/// Picks the batch size that would make `batch_size * ema_ms` land on
/// `target_ms`, clamped to `[1, MAX_BATCH_SIZE]`. `ema_ms` is clamped away from zero so a
/// (theoretically impossible, but not `f64`-impossible) zero or negative EMA can't produce a
/// division blow-up.
pub fn next_batch_size(ema_ms: f64, target_ms: f64) -> u32 {
    let raw = target_ms / ema_ms.max(f64::EPSILON);
    // `raw` is always finite and non-negative here (both operands are non-negative, and the
    // denominator is bounded away from zero above), so the `as u32` cast — which saturates
    // rather than wraps for float-to-int in Rust — just needs the follow-up `.clamp()` to land
    // it in range.
    (raw as u32).clamp(1, MAX_BATCH_SIZE)
}

/// Converts a measured batch duration into a per-step cost in milliseconds.
pub fn time_per_step_ms(elapsed: Duration, batch_size_submitted: u32) -> f64 {
    elapsed.as_secs_f64() * 1000.0 / f64::from(batch_size_submitted.max(1))
}

/// Shortest window worth dividing by. A refresh is meant to cover a whole stats interval, so a
/// window an order of magnitude under one is two clocks having fallen out of step.
pub const MIN_TPS_WINDOW: Duration = Duration::from_millis(100);

/// Steps per second over `elapsed`, or `None` when the window is too short to mean anything.
///
/// A whole batch divided by a near-zero window reads as a plausible-looking billion, not as an
/// obvious error, so this refuses rather than reporting it.
pub fn tps_over(step_count: u64, elapsed: Duration) -> Option<f64> {
    (elapsed >= MIN_TPS_WINDOW).then(|| step_count as f64 / elapsed.as_secs_f64())
}

#[cfg(test)]
mod tps_window_tests {
    use super::{MIN_TPS_WINDOW, tps_over};
    use std::time::Duration;

    #[test]
    fn a_normal_window_reports_the_rate() {
        let tps = tps_over(320, Duration::from_secs(1)).expect("a one second window is usable");
        assert!((tps - 320.0).abs() < 1e-9, "expected 320, got {tps}");
    }

    /// The failure this exists for: a whole batch against the gap between two clocks reads as
    /// 1.5e9 TPS, which looks like a number rather than like an error.
    #[test]
    fn a_near_zero_window_reports_nothing() {
        assert_eq!(tps_over(64, Duration::from_nanos(42)), None);
        assert_eq!(tps_over(64, Duration::ZERO), None);
    }

    #[test]
    fn the_cutoff_itself_is_usable() {
        assert!(tps_over(10, MIN_TPS_WINDOW).is_some());
        assert_eq!(tps_over(10, MIN_TPS_WINDOW / 2), None);
    }
}

#[cfg(test)]
mod adaptive_controller_tests {
    use super::{MAX_BATCH_SIZE, ema_update, next_batch_size};

    #[test]
    fn ema_first_sample_seeds_directly() {
        assert!((ema_update(None, 3.7, 0.25) - 3.7).abs() < f64::EPSILON);
    }

    #[test]
    fn ema_blends_subsequent_samples() {
        // alpha * sample + (1 - alpha) * prev, with alpha = 0.25.
        let ema = ema_update(Some(4.0), 8.0, 0.25);
        assert!((ema - 5.0).abs() < 1e-9, "expected 5.0, got {ema}");
    }

    #[test]
    fn ema_alpha_zero_ignores_new_samples() {
        let ema = ema_update(Some(4.0), 100.0, 0.0);
        assert!((ema - 4.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ema_alpha_one_tracks_new_sample_exactly() {
        let ema = ema_update(Some(4.0), 100.0, 1.0);
        assert!((ema - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn controller_known_ratio_produces_expected_batch_size() {
        // 8ms budget / 0.1ms-per-step ema = 80 steps.
        assert_eq!(next_batch_size(0.1, 8.0), 80);
    }

    #[test]
    fn controller_very_cheap_step_clamps_to_max() {
        // A near-zero per-step cost would naively imply a huge batch size; the hard cap must win.
        assert_eq!(next_batch_size(0.000_001, 8.0), MAX_BATCH_SIZE);
    }

    #[test]
    fn controller_very_expensive_step_clamps_to_one() {
        // A per-step cost far above the budget must not produce a batch size of 0.
        assert_eq!(next_batch_size(1_000.0, 8.0), 1);
    }

    #[test]
    fn controller_exact_budget_match_rounds_down_not_up() {
        // 8.0 / 3.0 = 2.667 steps; truncating (not rounding) keeps the batch under budget rather
        // than over it, which matches the "stay under the frame budget" intent.
        assert_eq!(next_batch_size(3.0, 8.0), 2);
    }

    #[test]
    fn controller_never_returns_zero_even_at_zero_ema() {
        // Defensive: a degenerate zero EMA (shouldn't occur given `.max(f64::EPSILON)` clamping
        // inside `next_batch_size`, but worth pinning as a regression guard) must not divide by
        // zero into NaN/inf and must still clamp to a valid, non-zero batch size.
        assert_eq!(next_batch_size(0.0, 8.0), MAX_BATCH_SIZE);
    }
}
