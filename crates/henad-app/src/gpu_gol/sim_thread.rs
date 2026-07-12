//! Dedicated OS thread that owns the GPU Game of Life state buffers and steps them flat-out,
//! decoupled from the UI frame rate — mirrors `henad_compute::sim_thread`'s CPU sim thread.
//!
//! # Synchronization
//!
//! `Device` and `Queue` are cheap, `Send + Sync` handles cloned from egui's own render state, so
//! this thread submits work on the same queue egui uses to render. wgpu serializes all
//! submissions to a given queue in the order `submit()` is called, and each submission is
//! atomic from the GPU's point of view (a later submission never observes a partially-executed
//! earlier one). So when this thread's display-texture write and egui's render-pass sample land
//! in different submissions, the render pass either sees the fully-written previous texture or
//! the fully-written next one — never a torn one. The one accepted cost is up to one frame of
//! staleness (the UI may sample last snapshot's texture instead of the newest one), which is
//! fine for a sim running orders of magnitude faster than the display refresh rate.
//!
//! # Batching
//!
//! Steps are batched `batch_size`-per-submission to keep submission overhead from competing with
//! egui's own per-frame submissions on the shared queue. Each step is still its own compute pass
//! (see `GpuGolCompute::dispatch_step_batch` for why — wgpu only synchronizes between passes, not
//! between dispatches within one pass, and the ping-pong buffers need that). The display compute
//! pass (state → texture) only runs when at least ~16ms have elapsed since the last one, so
//! "steps per snapshot" is emergent from how fast the batches run, exactly like the CPU sim
//! thread's `ticks_per_snapshot`. This snapshot cadence (`SNAPSHOT_INTERVAL`) is independent of
//! batch size and unaffected by anything below.
//!
//! `batch_size` itself is either a fixed, UI-set value, or adaptively controlled — see
//! `GpuGolCommand::SetAdaptive`/`SetTargetMs`/`SetBatchSize` and the `adaptive`/`fixed_batch_size`/
//! `target_ms` fields on `GpuGolSimLoop`. The problem adaptive mode solves: on a shared queue with
//! no preemption, a large fixed batch (e.g. 256 steps on a 4096x4096 grid) can take on the order
//! of 100ms+ of GPU execution time in one submission, and because egui's own render-pass
//! submissions share that queue, a big batch blocks egui's rendering behind it — visible as UI
//! stutter, even though the display texture is already decoupled from batch size (see above).
//!
//! Adaptive mode measures the wall-clock time to encode and submit each batch (a proxy for GPU
//! cost — see `step_batch`'s doc comment for the caveats on why this proxy was chosen and what
//! could make it unreliable), maintains an EMA of `time_per_step`, and picks the next batch size
//! so that `batch_size * time_per_step` tracks a user-set `target_ms` budget. This deliberately
//! does not use `TimestampQuery`, which stays diagnostic-only (surfaced as `gpu_us_per_step`) —
//! GPU timestamp correctness is a separate, already-tracked concern.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{GpuGolCompute, ReseedKind, seed_patterns, seed_random};

/// Default steps-per-submission in fixed mode, tunable at runtime from the UI.
pub const DEFAULT_BATCH_SIZE: u32 = 64;

/// Default per-batch wall-clock budget in adaptive mode, in milliseconds. ~8ms leaves roughly
/// half of a 60fps (16.6ms) frame free, so a single batch submission is unlikely to be the thing
/// that makes egui miss a frame even when it lands right before an egui submission on the same
/// queue.
pub const DEFAULT_TARGET_MS: f64 = 8.0;

/// Smoothing factor for the adaptive controller's exponential moving average of `time_per_step`.
/// 0.25 was chosen to react within a handful of batches to a real change in per-step cost (e.g.
/// after a reseed to a denser pattern, or a grid resize), while still averaging out per-batch
/// noise from OS scheduling jitter on the sim thread — a pure single-sample estimate was found
/// to make the controller's output jump around too much batch-to-batch.
const ADAPTIVE_EMA_ALPHA: f64 = 0.25;

/// Hard upper bound on the adaptive controller's output, independent of the budget/cost
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

const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(16);
const STATS_INTERVAL: Duration = Duration::from_secs(1);

/// Commands sent from the UI thread to the GPU sim thread.
enum GpuGolCommand {
    Pause,
    Resume,
    SetBatchSize(u32),
    SetAdaptive(bool),
    SetTargetMs(f64),
    Reseed(ReseedKind),
    Shutdown,
}

/// Latest wall-clock and GPU-side performance numbers, polled by the UI once per frame.
#[derive(Clone, Copy)]
pub struct GpuGolStats {
    pub wall_tps: f64,
    pub gpu_us_per_step: Option<f64>,
    /// Live batch size — the fixed value in `Fixed` mode, or the controller's current output in
    /// `Adaptive` mode.
    pub batch_size: u32,
    pub paused: bool,
    pub adaptive: bool,
}

impl Default for GpuGolStats {
    fn default() -> Self {
        Self {
            wall_tps: 0.0,
            gpu_us_per_step: None,
            batch_size: DEFAULT_BATCH_SIZE,
            paused: false,
            adaptive: false,
        }
    }
}

/// GPU timestamp-query resources, created only if the device supports `Features::TIMESTAMP_QUERY`.
struct TimestampQuery {
    query_set: wgpu::QuerySet,
    resolve_buffer: wgpu::Buffer,
    readback_buffer: wgpu::Buffer,
    /// Nanoseconds per timestamp tick, from `Queue::get_timestamp_period`.
    period_ns: f32,
}

impl TimestampQuery {
    const BUFFER_SIZE: u64 = 2 * std::mem::size_of::<u64>() as u64;

    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        if !device.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let query_set = device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("gpu_gol_timestamp_query_set"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        });
        let resolve_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_gol_timestamp_resolve"),
            size: Self::BUFFER_SIZE,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let readback_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_gol_timestamp_readback"),
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
    fn resolve_after(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        write_submission: wgpu::SubmissionIndex,
    ) {
        drop(device.poll(wgpu::PollType::Wait {
            submission_index: Some(write_submission),
            timeout: None,
        }));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_gol_timestamp_resolve_encoder"),
        });
        encoder.resolve_query_set(&self.query_set, 0..2, &self.resolve_buffer, 0);
        encoder.copy_buffer_to_buffer(
            &self.resolve_buffer,
            0,
            &self.readback_buffer,
            0,
            Self::BUFFER_SIZE,
        );
        queue.submit(Some(encoder.finish()));
    }

    /// Blocking readback of the two timestamps written by the last stamped batch, called at most
    /// once per `STATS_INTERVAL` — the stall this introduces is negligible next to a sim running
    /// at thousands of TPS.
    fn read_gpu_us_per_step(&self, device: &wgpu::Device, batch_size: u32) -> Option<f64> {
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

struct GpuGolSimLoop {
    device: wgpu::Device,
    queue: wgpu::Queue,
    compute: GpuGolCompute,
    cmd_rx: mpsc::Receiver<GpuGolCommand>,
    stats: Arc<Mutex<GpuGolStats>>,
    running: bool,
    /// Whether the controller is currently in adaptive mode. Kept as a separate bool (rather
    /// than folding `fixed_batch_size`/`target_ms` into a `BatchMode` enum) so each mode's state
    /// survives toggling — the manual fixed size is remembered while adaptive is active, and the
    /// adaptive controller's target/EMA survive switching back to fixed.
    adaptive: bool,
    /// Manual batch size, used verbatim when `adaptive` is false.
    fixed_batch_size: u32,
    /// Per-batch wall-clock budget in milliseconds, used by the controller when `adaptive` is
    /// true.
    target_ms: f64,
    /// Live batch size to use for the next batch: `fixed_batch_size` when not adaptive, or the
    /// controller's last output when adaptive.
    batch_size: u32,
    /// Exponential moving average of measured wall-clock time per step, in milliseconds. `None`
    /// until the first batch has been measured. Only used/updated in adaptive mode.
    ema_time_per_step_ms: Option<f64>,
    step_count: u64,
    tps_timer: Instant,
    last_display_publish: Instant,
    last_stats_publish: Instant,
    timestamp_query: Option<TimestampQuery>,
}

impl GpuGolSimLoop {
    fn run(mut self) {
        loop {
            if !self.running {
                let Ok(cmd) = self.cmd_rx.recv() else {
                    return;
                };
                if self.handle_command(cmd) {
                    return;
                }
                continue;
            }

            self.step_batch();

            while let Ok(cmd) = self.cmd_rx.try_recv() {
                if self.handle_command(cmd) {
                    return;
                }
            }
        }
    }

    /// Returns true if the thread should exit.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "consumes the command to extract owned data (e.g. ReseedKind), matching henad_compute::sim_thread's SimCommand handling"
    )]
    fn handle_command(&mut self, cmd: GpuGolCommand) -> bool {
        match cmd {
            GpuGolCommand::Pause => {
                self.running = false;
                self.publish_stats(0.0, None);
            }
            GpuGolCommand::Resume => {
                self.running = true;
                self.tps_timer = Instant::now();
                self.step_count = 0;
            }
            GpuGolCommand::SetBatchSize(n) => {
                self.fixed_batch_size = n.max(1);
                if !self.adaptive {
                    self.batch_size = self.fixed_batch_size;
                }
            }
            GpuGolCommand::SetAdaptive(enabled) => {
                self.adaptive = enabled;
                if enabled {
                    // Reset the estimator so a stale EMA from a previous adaptive session (e.g.
                    // measured on a different grid size) doesn't bias the first few batches.
                    self.ema_time_per_step_ms = None;
                } else {
                    self.batch_size = self.fixed_batch_size;
                }
            }
            GpuGolCommand::SetTargetMs(target_ms) => {
                self.target_ms = target_ms.max(0.1);
            }
            GpuGolCommand::Reseed(kind) => {
                let cells = match kind {
                    ReseedKind::Patterns => seed_patterns(self.compute.width, self.compute.height),
                    ReseedKind::Random { seed, density } => {
                        seed_random(self.compute.width, self.compute.height, density, seed)
                    }
                };
                self.compute.reseed(&self.queue, &cells);
                // Refresh the display texture immediately, even if paused, so reseeding shows
                // the new grid right away instead of waiting for the next running step_batch.
                let mut encoder =
                    self.device
                        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                            label: Some("gpu_gol_reseed_display_encoder"),
                        });
                self.compute.dispatch_display(&mut encoder);
                self.queue.submit(Some(encoder.finish()));
                self.last_display_publish = Instant::now();
            }
            GpuGolCommand::Shutdown => return true,
        }
        false
    }

    /// Records and submits one batch of steps (plus, at snapshot/stats cadence, the display pass
    /// and/or a timestamped-query resolve), then updates the published stats when due.
    ///
    /// The timestamp-query resolve is deliberately *not* recorded into the same command buffer
    /// as the writes — see `TimestampQuery::resolve_after` for why.
    ///
    /// Also times the encode+submit portion (`now` to `batch_wall_elapsed` below) as the
    /// adaptive controller's cost signal. This is deliberately wall-clock CPU time, not a GPU
    /// timestamp query: `queue.submit()` isn't required to block for GPU completion, so this is
    /// not a direct measure of GPU execution time. The assumption underpinning this choice —
    /// that on a queue kept continuously busy by back-to-back batches from this thread, with no
    /// other CPU work in between, the *rate* at which `submit()` calls can be issued ends up
    /// backpressured by how fast the GPU drains the queue — is plausible but has **not** been
    /// empirically verified in this environment (no way to drive the GUI here and watch it
    /// react to a deliberately slow vs. fast grid). If it doesn't hold on some backend/platform
    /// (e.g. `submit()` returns immediately regardless of queue depth), this call instead mostly
    /// measures CPU-side dispatch-recording cost, which scales close to linearly with
    /// `batch_size` — so `time_per_step` would stay roughly constant regardless of true GPU
    /// load, and the controller would regulate encode cost rather than the GPU-stutter problem
    /// it's meant to solve. Flagging this as the main open risk of this design rather than
    /// asserting it works. It's cheap either way (no readback stall) and unaffected by the
    /// `TimestampQuery` correctness issue tracked separately. Occasionally this call also
    /// includes the display pass (`SNAPSHOT_INTERVAL` cadence) or the timestamp resolve/copy
    /// (`STATS_INTERVAL` cadence); both are infrequent and cheap next to a multi-step batch, so
    /// they're accepted as minor noise on the EMA rather than measured separately.
    fn step_batch(&mut self) {
        let now = Instant::now();
        let want_timing = self.timestamp_query.is_some()
            && now.duration_since(self.last_stats_publish) >= STATS_INTERVAL;

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gpu_gol_sim_encoder"),
            });

        let query_set = if want_timing {
            self.timestamp_query.as_ref().map(|tq| &tq.query_set)
        } else {
            None
        };
        self.compute
            .dispatch_step_batch(&mut encoder, self.batch_size, query_set);

        if now.duration_since(self.last_display_publish) >= SNAPSHOT_INTERVAL {
            self.compute.dispatch_display(&mut encoder);
            self.last_display_publish = now;
        }

        let batch_size_submitted = self.batch_size;
        let write_submission = self.queue.submit(Some(encoder.finish()));
        let batch_wall_elapsed = Instant::now().duration_since(now);
        self.step_count += u64::from(batch_size_submitted);

        if want_timing {
            let tq = self
                .timestamp_query
                .as_ref()
                .expect("want_timing implies timestamp_query is Some");
            tq.resolve_after(&self.device, &self.queue, write_submission);
        }

        if self.adaptive {
            self.update_adaptive_batch_size(batch_wall_elapsed, batch_size_submitted);
        }

        if want_timing {
            let gpu_us_per_step = self
                .timestamp_query
                .as_ref()
                .expect("want_timing implies timestamp_query is Some")
                .read_gpu_us_per_step(&self.device, batch_size_submitted);
            let wall_tps = self.step_count as f64
                / now
                    .duration_since(self.tps_timer)
                    .as_secs_f64()
                    .max(f64::EPSILON);
            self.step_count = 0;
            self.tps_timer = now;
            self.last_stats_publish = now;
            self.publish_stats(wall_tps, gpu_us_per_step);
        } else if self.timestamp_query.is_none()
            && now.duration_since(self.tps_timer) >= STATS_INTERVAL
        {
            // No GPU timing support on this device/backend — still publish wall-clock TPS on
            // the same cadence so the UI keeps updating.
            let wall_tps = self.step_count as f64
                / now
                    .duration_since(self.tps_timer)
                    .as_secs_f64()
                    .max(f64::EPSILON);
            self.step_count = 0;
            self.tps_timer = now;
            self.publish_stats(wall_tps, None);
        }
    }

    fn publish_stats(&self, wall_tps: f64, gpu_us_per_step: Option<f64>) {
        if let Ok(mut stats) = self.stats.lock() {
            stats.wall_tps = wall_tps;
            stats.gpu_us_per_step = gpu_us_per_step;
            stats.batch_size = self.batch_size;
            stats.paused = !self.running;
            stats.adaptive = self.adaptive;
        }
    }

    /// Updates the EMA of wall-clock time per step from the just-measured batch, then recomputes
    /// `self.batch_size` (the size the *next* batch will use). Thin wrapper around the pure
    /// `ema_update`/`next_batch_size` functions below, which carry the actual controller math
    /// and are unit-tested directly since this method itself needs a live `wgpu::Device` to
    /// reach (it's only ever called from `step_batch`).
    fn update_adaptive_batch_size(&mut self, elapsed: Duration, batch_size_submitted: u32) {
        let time_per_step_ms =
            elapsed.as_secs_f64() * 1000.0 / f64::from(batch_size_submitted.max(1));
        let ema = ema_update(
            self.ema_time_per_step_ms,
            time_per_step_ms,
            ADAPTIVE_EMA_ALPHA,
        );
        self.ema_time_per_step_ms = Some(ema);
        self.batch_size = next_batch_size(ema, self.target_ms);
    }
}

/// Exponential moving average update: `prev` is `None` on the very first sample (in which case
/// the sample seeds the EMA directly, rather than blending against an arbitrary starting value),
/// `Some` on every subsequent call.
fn ema_update(prev: Option<f64>, sample: f64, alpha: f64) -> f64 {
    match prev {
        Some(prev) => alpha.mul_add(sample, (1.0 - alpha) * prev),
        None => sample,
    }
}

/// Proportional controller: picks the batch size that would make `batch_size * ema_ms` land on
/// `target_ms`, clamped to `[1, MAX_BATCH_SIZE]`. `ema_ms` is clamped away from zero so a
/// (theoretically impossible, but not `f64`-impossible) zero or negative EMA can't produce a
/// division blow-up.
fn next_batch_size(ema_ms: f64, target_ms: f64) -> u32 {
    let raw = target_ms / ema_ms.max(f64::EPSILON);
    // `raw` is always finite and non-negative here (both operands are non-negative, and the
    // denominator is bounded away from zero above), so the `as u32` cast — which saturates
    // rather than wraps for float-to-int in Rust — just needs the follow-up `.clamp()` to land
    // it in range.
    (raw as u32).clamp(1, MAX_BATCH_SIZE)
}

/// Handle to the GPU sim thread: send commands, poll the latest stats. Dropping it shuts the
/// thread down and joins it (mirrors `henad_compute::sim_thread::SimThread`'s `Drop`).
pub struct GpuGolHandle {
    cmd_tx: mpsc::Sender<GpuGolCommand>,
    stats: Arc<Mutex<GpuGolStats>>,
    handle: Option<JoinHandle<()>>,
}

impl GpuGolHandle {
    /// Spawns the GPU sim thread, taking ownership of `compute` and cloned `device`/`queue`
    /// handles. Starts running immediately (unpaused).
    pub fn spawn(
        device: wgpu::Device,
        queue: wgpu::Queue,
        compute: GpuGolCompute,
        initial_batch_size: u32,
    ) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let batch_size = initial_batch_size.max(1);
        let stats = Arc::new(Mutex::new(GpuGolStats {
            batch_size,
            ..GpuGolStats::default()
        }));
        let stats_clone = Arc::clone(&stats);
        let timestamp_query = TimestampQuery::new(&device, &queue);

        let sim_loop = GpuGolSimLoop {
            device,
            queue,
            compute,
            cmd_rx,
            stats: stats_clone,
            running: true,
            adaptive: false,
            fixed_batch_size: batch_size,
            target_ms: DEFAULT_TARGET_MS,
            batch_size,
            ema_time_per_step_ms: None,
            step_count: 0,
            tps_timer: Instant::now(),
            last_display_publish: Instant::now(),
            last_stats_publish: Instant::now(),
            timestamp_query,
        };

        let handle = std::thread::spawn(move || sim_loop.run());

        Self {
            cmd_tx,
            stats,
            handle: Some(handle),
        }
    }

    pub fn pause(&self) {
        self.send(GpuGolCommand::Pause);
    }

    pub fn resume(&self) {
        self.send(GpuGolCommand::Resume);
    }

    /// Sets the manual batch size used in fixed mode. Has no visible effect while adaptive mode
    /// is on (the controller drives `batch_size` instead), but is remembered for when it's
    /// turned back off.
    pub fn set_batch_size(&self, batch_size: u32) {
        self.send(GpuGolCommand::SetBatchSize(batch_size));
    }

    /// Turns adaptive batching on or off. Fixed mode's manual batch size and adaptive mode's
    /// target/EMA are each preserved independently across toggles.
    pub fn set_adaptive(&self, enabled: bool) {
        self.send(GpuGolCommand::SetAdaptive(enabled));
    }

    /// Sets the per-batch wall-clock time budget (milliseconds) used by the adaptive controller.
    /// Has no effect while fixed mode is active, but is remembered for when adaptive is turned
    /// on.
    pub fn set_target_ms(&self, target_ms: f64) {
        self.send(GpuGolCommand::SetTargetMs(target_ms));
    }

    pub(crate) fn reseed(&self, kind: ReseedKind) {
        self.send(GpuGolCommand::Reseed(kind));
    }

    /// Latest published stats. Cheap: a single mutex lock and copy.
    pub fn stats(&self) -> GpuGolStats {
        self.stats
            .lock()
            .map_or_else(|_| GpuGolStats::default(), |s| *s)
    }

    fn send(&self, cmd: GpuGolCommand) {
        drop(self.cmd_tx.send(cmd));
    }
}

impl Drop for GpuGolHandle {
    fn drop(&mut self) {
        drop(self.cmd_tx.send(GpuGolCommand::Shutdown));
        if let Some(h) = self.handle.take() {
            drop(h.join());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu_gol::{build, seed_random};

    /// Like `gpu_gol::tests::headless_device`, but requests `TIMESTAMP_QUERY` explicitly (mirrors
    /// what `main.rs` does when the adapter supports it), since the default test device requests
    /// no features at all.
    fn headless_timing_device() -> Option<(wgpu::Device, wgpu::Queue)> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .ok()?;
        if !adapter.features().contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("gpu_gol_timing_test_device"),
            required_features: wgpu::Features::TIMESTAMP_QUERY,
            ..Default::default()
        }))
        .ok()?;
        Some((device, queue))
    }

    /// Regression test for "GPU time/step flickers to 0/None during a sustained run": runs many
    /// batches back to back exactly like `GpuGolSimLoop::step_batch` records/resolves/reads a
    /// timestamped batch, but takes a reading on *every* iteration instead of once/second, to
    /// shake out an intermittent zero or failed readback far more aggressively than the real
    /// once-per-second cadence would in a short-lived interactive session.
    #[test]
    fn gpu_timing_readback_is_stable_over_many_batches() {
        let Some((device, queue)) = headless_timing_device() else {
            log::warn!(
                "skipping gpu_timing_readback_is_stable_over_many_batches: \
                 no adapter with TIMESTAMP_QUERY available"
            );
            return;
        };

        let width = 256;
        let height = 256;
        let initial = seed_random(width, height, 0.3, 7);
        let (mut compute, _render) = build(
            &device,
            &queue,
            wgpu::TextureFormat::Rgba8Unorm,
            width,
            height,
            &initial,
        );

        let tq = TimestampQuery::new(&device, &queue).expect("device has TIMESTAMP_QUERY");
        let batch_size = 64;
        let iterations = 200;
        let mut zero_count = 0usize;
        let mut none_count = 0usize;

        for _ in 0..iterations {
            let mut encoder =
                device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
            compute.dispatch_step_batch(&mut encoder, batch_size, Some(&tq.query_set));
            let write_submission = queue.submit(Some(encoder.finish()));

            tq.resolve_after(&device, &queue, write_submission);

            match tq.read_gpu_us_per_step(&device, batch_size) {
                Some(us) if us <= 0.0 => zero_count += 1,
                Some(_) => {}
                None => none_count += 1,
            }
        }

        assert_eq!(
            none_count, 0,
            "readback failed (returned None) on {none_count}/{iterations} back-to-back batches"
        );
        assert_eq!(
            zero_count, 0,
            "readback returned 0 (end timestamp <= start timestamp) on \
             {zero_count}/{iterations} back-to-back batches"
        );
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
