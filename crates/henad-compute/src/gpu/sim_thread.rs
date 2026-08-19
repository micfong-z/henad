//! Dedicated OS thread that owns a GPU-resident sim state and steps it in batched submissions,
//! decoupled from the UI frame rate. The GPU sibling of [`crate::cpu::sim_thread`].
//!
//! # Synchronization
//!
//! This thread submits on the same queue egui renders on, using `Send + Sync` handles cloned from
//! egui's render state. wgpu serializes submissions to a queue and each is atomic from the GPU's
//! point of view, so egui's render pass samples either the fully-written previous display texture
//! or the fully-written next one, never a torn one. The accepted cost is up to one frame of
//! staleness, which is nothing for a sim running orders of magnitude faster than the refresh rate.
//!
//! # Batching
//!
//! Steps go out `batch_size` per batch, split across submissions of at most
//! [`crate::gpu::MAX_STEPS_PER_SUBMISSION`] steps each. Each step is still its own compute pass,
//! since wgpu only synchronizes between passes and the ping-pong needs that. The display and
//! stats-reduction passes run only once [`SNAPSHOT_INTERVAL`] has elapsed, so steps per snapshot
//! is emergent and independent of batch size.
//!
//! One batch is outstanding at a time. Left unbounded, egui's own submissions queue behind a
//! dozen batches of sim work and every frame pays for all of them.
//!
//! `batch_size` is either fixed from the UI or adaptively controlled. Adaptive mode keeps an EMA
//! of `time_per_step` and picks a batch size so that `batch_size * time_per_step` tracks a
//! user-set `target_ms`. It deliberately does not use `TimestampQuery`, which stays
//! diagnostic-only.
use henad_core::model::SimState;

use crate::gpu::timing::{DEFAULT_BATCH_SIZE, DEFAULT_TARGET_MS};
use crate::snapshot::GpuSnapshot;

/// The interface [`GpuSimThread`] drives, the GPU counterpart of how the CPU thread drives
/// `SimState`. Not a model-authoring trait, which is
/// `henad_core::authoring::model::gpu_grid_model::GpuGridModel`.
///
/// A GPU model's grid never leaves the GPU, so `SimState::stats()` reports whatever the last
/// completed [`Self::poll_stats_readback`] produced, a few milliseconds stale.
pub trait GpuSimState: SimState {
    /// Record `count` steps into `encoder`, advancing the model's own tick counter by `count`.
    ///
    /// `count` must not exceed [`crate::gpu::MAX_STEPS_PER_SUBMISSION`] for one submission.
    ///
    /// If `timestamps` is `Some`, stamp the beginning of the first step and the end of the last
    /// into query indices 0 and 1, so the caller can measure GPU time over `count` steps.
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>);

    /// Record the display pass (state -> display texture) and the stats-reduction pass
    /// (state -> a handful of numbers), at the snapshot cadence rather than every step.
    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder);

    /// Start the async stats readback. Called immediately after the submission that
    /// [`Self::encode_snapshot_passes`] was recorded into, since mapping earlier races the copy.
    fn begin_stats_readback(&mut self);

    /// Complete an in-flight stats readback, updating what `SimState::stats()` returns.
    ///
    /// With `block = false` this must not wait on the GPU. It runs every loop iteration, and
    /// stalling until the queue drains is what this thread exists to avoid. `block = true` is for
    /// one-shot snapshots only, where a real value in the stats panel beats a few ms of latency.
    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool);

    /// The layers the UI draws. Cloned into every snapshot, so keep it to `Arc` clones of things
    /// built once at construction.
    fn view(&self) -> GpuSnapshot;
}

/// Live GPU-runner numbers that have no CPU counterpart, polled by the UI once per frame.
///
/// Everything with a CPU equivalent (tick, TPS, population, stats) travels in the `Snapshot`
/// instead, so the existing toolbar/stats panels need no GPU special-casing.
#[derive(Clone, Copy, Debug)]
pub struct GpuStats {
    /// True GPU execution time per step, if the adapter supports timestamp queries.
    pub gpu_us_per_step: Option<f64>,
    /// Live batch size. The fixed value in fixed mode, the controller's output in adaptive mode.
    pub batch_size: u32,
    pub adaptive: bool,
}

impl Default for GpuStats {
    fn default() -> Self {
        Self {
            gpu_us_per_step: None,
            batch_size: DEFAULT_BATCH_SIZE,
            adaptive: false,
        }
    }
}

/// GPU-runner-specific commands, on top of the shared [`crate::cpu::sim_thread::SimCommand`].
pub enum GpuCommand {
    SetBatchSize(u32),
    SetAdaptive(bool),
    SetTargetMs(f64),
}

/// Initial adaptive-batching settings, so a freshly spawned thread starts in whatever mode the UI
/// is already showing rather than snapping back to the default.
#[derive(Clone, Copy, Debug)]
pub struct GpuBatchSettings {
    pub adaptive: bool,
    pub batch_size: u32,
    pub target_ms: f64,
}

impl Default for GpuBatchSettings {
    fn default() -> Self {
        Self {
            adaptive: true,
            batch_size: DEFAULT_BATCH_SIZE,
            target_ms: DEFAULT_TARGET_MS,
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use super::{GpuBatchSettings, GpuCommand, GpuSimState, GpuStats};
    use crate::cpu::sim_thread::{SimCommand, WakeFn};
    use crate::fault::{STEPPING, catching};
    use crate::gpu::timing::{
        ADAPTIVE_EMA_ALPHA, TimestampQuery, ema_update, next_batch_size, time_per_step_ms, tps_over,
    };
    use crate::gpu::{GpuContext, MAX_STEPS_PER_SUBMISSION};
    use crate::snapshot::{Snapshot, SnapshotView};

    /// Display texture refresh and `Snapshot` publish cadence. Independent of batch size and of
    /// how fast the sim is actually running.
    const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(16);
    /// Refresh cadence for wall-clock TPS and the expensive, blocking GPU timestamp readback.
    const STATS_INTERVAL: Duration = Duration::from_secs(1);

    enum Command {
        Sim(SimCommand),
        Gpu(GpuCommand),
    }

    /// The batch the GPU is working on.
    struct InFlight {
        submission: wgpu::SubmissionIndex,
        steps: u32,
        started_at: Instant,
    }

    struct GpuSimLoop {
        ctx: GpuContext,
        state: Box<dyn GpuSimState>,
        cmd_rx: mpsc::Receiver<Command>,
        snapshot: Arc<Mutex<Option<Snapshot>>>,
        gpu_stats: Arc<Mutex<GpuStats>>,
        wake: Option<WakeFn>,
        running: bool,
        /// A separate bool rather than a `BatchMode` enum, so each mode's state survives a
        /// toggle. The manual size is remembered while adaptive runs, and the target survives
        /// switching back to fixed.
        adaptive: bool,
        /// Manual batch size, used verbatim when `adaptive` is false.
        fixed_batch_size: u32,
        /// Per-batch wall-clock budget in milliseconds, used by the controller when `adaptive`.
        target_ms: f64,
        /// Size of the next batch. `fixed_batch_size` when not adaptive, the controller's last
        /// output when adaptive.
        batch_size: u32,
        /// EMA of measured wall-clock time per step, in milliseconds. `None` until the first
        /// batch has been measured.
        ema_time_per_step_ms: Option<f64>,
        in_flight: Option<InFlight>,
        step_count: u64,
        actual_tps: f64,
        gpu_us_per_step: Option<f64>,
        tps_timer: Instant,
        last_snapshot_publish: Instant,
        last_stats_publish: Instant,
        timestamp_query: Option<TimestampQuery>,
    }

    impl GpuSimLoop {
        fn run(mut self) {
            // Publish a snapshot before anything runs, so the viewport shows the seeded grid the
            // moment the model is loaded rather than staying blank until Play. Mirrors
            // `SimThread::new` publishing an initial snapshot.
            self.snapshot_now();

            loop {
                // A device error raised by `submit` lands in the sink rather than unwinding. The
                // loop finds out about it here. Stepping on would only pile up more.
                if self.ctx.faults.is_set() {
                    self.running = false;
                    self.actual_tps = 0.0;
                    self.publish_snapshot();
                    return;
                }

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
        fn handle_command(&mut self, cmd: Command) -> bool {
            match cmd {
                Command::Sim(SimCommand::Play) => {
                    self.running = true;
                    self.reset_tps_window(Instant::now());
                }
                Command::Sim(SimCommand::Pause) => {
                    self.running = false;
                    self.actual_tps = 0.0;
                    self.snapshot_now();
                }
                Command::Sim(SimCommand::StepOnce) => {
                    let mut encoder = self.encoder("henad_gpu_step_once");
                    self.state.encode_steps(&mut encoder, 1, None);
                    self.ctx.queue.submit(Some(encoder.finish()));
                    self.snapshot_now();
                }
                Command::Sim(SimCommand::SetParam { index, value }) => {
                    if !self.state.set_param(index, &value) {
                        log::warn!("Failed to set param index {index} to {value:?}");
                    }
                }
                // No GPU analogue: this thread runs flat-out and paces itself with the batch-size
                // controller instead of a TPS cap, and its snapshot cadence is wall-clock-driven
                // rather than a tick count. Accepted (rather than an error) so `HenadApp` can send
                // the same `SimCommand` stream to either backend without special-casing.
                Command::Sim(
                    SimCommand::SetTargetTps(_) | SimCommand::SetUncapped(_) | SimCommand::SetTicksPerSnapshot(_),
                ) => {}
                Command::Sim(SimCommand::Shutdown) => return true,
                Command::Gpu(GpuCommand::SetBatchSize(n)) => {
                    self.fixed_batch_size = n.max(1);
                    if !self.adaptive {
                        self.batch_size = self.fixed_batch_size;
                    }
                }
                Command::Gpu(GpuCommand::SetAdaptive(enabled)) => {
                    self.adaptive = enabled;
                    if !enabled {
                        self.batch_size = self.fixed_batch_size;
                    }
                    self.publish_gpu_stats();
                }
                Command::Gpu(GpuCommand::SetTargetMs(target_ms)) => {
                    self.target_ms = target_ms.max(0.1);
                }
            }
            false
        }

        fn encoder(&self, label: &str) -> wgpu::CommandEncoder {
            self.ctx
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
        }

        /// Refresh the display texture + stats and publish a snapshot right now, blocking on the
        /// stats readback. Used for one-shot updates (initial, pause, step-once) where there is
        /// no next loop iteration to pick the readback up asynchronously.
        fn snapshot_now(&mut self) {
            let mut encoder = self.encoder("henad_gpu_snapshot_now");
            self.state.encode_snapshot_passes(&mut encoder);
            self.ctx.queue.submit(Some(encoder.finish()));
            self.state.begin_stats_readback();
            self.state.poll_stats_readback(&self.ctx.device, true);
            // The blocking readback drained the queue, outstanding batch included.
            self.in_flight = None;

            self.last_snapshot_publish = Instant::now();
            self.publish_snapshot();
            self.publish_gpu_stats();
        }

        /// The sample is one loop period, from the start of encoding a batch to the GPU finishing
        /// it. Encode plus submit alone reads CPU dispatch cost, orders of magnitude too low.
        fn await_previous(&mut self) {
            let Some(prev) = self.in_flight.take() else {
                return;
            };
            drop(self.ctx.device.poll(wgpu::PollType::Wait {
                submission_index: Some(prev.submission),
                timeout: None,
            }));

            let sample = time_per_step_ms(prev.started_at.elapsed(), prev.steps);
            let ema = ema_update(self.ema_time_per_step_ms, sample, ADAPTIVE_EMA_ALPHA);
            self.ema_time_per_step_ms = Some(ema);
            if self.adaptive {
                self.batch_size = next_batch_size(ema, self.target_ms);
            }
        }

        /// Records and submits one batch of steps, plus the display, stats and timestamp-resolve
        /// work at their own cadences, then updates published state.
        ///
        /// Only the first submission of a batch carries the timestamps, so the reported per-step
        /// time divides by that chunk rather than by the batch.
        ///
        /// The timestamp resolve deliberately does not share a command buffer with the writes.
        /// See `TimestampQuery::resolve_after`.
        fn step_batch(&mut self) {
            self.await_previous();

            let now = Instant::now();
            let want_timing =
                self.timestamp_query.is_some() && now.duration_since(self.last_stats_publish) >= STATS_INTERVAL;
            let want_snapshot = now.duration_since(self.last_snapshot_publish) >= SNAPSHOT_INTERVAL;

            let batch_size_submitted = self.batch_size;
            let query_set = if want_timing {
                self.timestamp_query.as_ref().map(TimestampQuery::query_set)
            } else {
                None
            };

            let mut submitted = 0;
            let mut stamped_steps = None;
            let mut write_submission = None;
            while submitted < batch_size_submitted {
                let chunk = MAX_STEPS_PER_SUBMISSION.min(batch_size_submitted - submitted);
                let mut encoder = self.ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("henad_gpu_sim_encoder"),
                });

                let stamp = query_set.filter(|_| stamped_steps.is_none());
                if stamp.is_some() {
                    stamped_steps = Some(chunk);
                }
                self.state.encode_steps(&mut encoder, chunk, stamp);

                submitted += chunk;
                if want_snapshot && submitted >= batch_size_submitted {
                    self.state.encode_snapshot_passes(&mut encoder);
                }
                write_submission = Some(self.ctx.queue.submit(Some(encoder.finish())));
            }

            self.step_count += u64::from(batch_size_submitted);

            if want_snapshot {
                self.state.begin_stats_readback();
            }
            // Non-blocking: picks up a readback started on an earlier iteration if the GPU has
            // caught up with it by now. Never stalls this thread.
            self.state.poll_stats_readback(&self.ctx.device, false);

            if want_timing
                && let (Some(tq), Some(submission)) = (self.timestamp_query.as_ref(), write_submission.clone())
            {
                tq.resolve_after(&self.ctx.device, &self.ctx.queue, submission);
            }

            if want_timing {
                if let (Some(tq), Some(steps)) = (self.timestamp_query.as_ref(), stamped_steps) {
                    self.gpu_us_per_step = tq.read_gpu_us_per_step(&self.ctx.device, steps);
                }
                self.refresh_tps(now);
                self.last_stats_publish = now;
            } else if self.timestamp_query.is_none() && now.duration_since(self.tps_timer) >= STATS_INTERVAL {
                // No GPU timing on this device, so refresh wall-clock TPS on the same cadence
                // anyway, to keep the UI updating.
                self.refresh_tps(now);
            }

            if want_snapshot {
                self.last_snapshot_publish = now;
                self.publish_snapshot();
                self.publish_gpu_stats();
            }

            self.in_flight = write_submission.map(|submission| InFlight {
                submission,
                steps: batch_size_submitted,
                started_at: now,
            });
        }

        /// Both clocks together. `want_timing` is gated on `last_stats_publish` but divides by
        /// `tps_timer`, so resetting one without the other makes the next refresh divide a whole
        /// batch by whatever tiny gap is between them.
        fn reset_tps_window(&mut self, now: Instant) {
            self.tps_timer = now;
            self.last_stats_publish = now;
            self.step_count = 0;
        }

        fn refresh_tps(&mut self, now: Instant) {
            let Some(tps) = tps_over(self.step_count, now.duration_since(self.tps_timer)) else {
                // Leave the window open rather than reporting a rate over nothing. `step_count`
                // keeps accumulating, so the next refresh covers both.
                return;
            };
            self.actual_tps = tps;
            self.step_count = 0;
            self.tps_timer = now;
        }

        fn publish_snapshot(&self) {
            let snap = Snapshot {
                tick: self.state.tick(),
                population: self.state.population(),
                heap_bytes: self.state.heap_bytes(),
                actual_tps: self.actual_tps,
                // Report true GPU cost per step where the toolbar shows CPU engine time. Falls
                // back to 0 when the adapter has no timestamp support, same as "unknown".
                engine_ms: self.gpu_us_per_step.unwrap_or(0.0) / 1000.0,
                view: SnapshotView::Gpu(self.state.view()),
                stats: self.state.stats(),
            };
            if let Ok(mut slot) = self.snapshot.lock() {
                *slot = Some(snap);
            }
            // After the lock, so waking the UI can never make it block on us.
            if let Some(wake) = &self.wake {
                wake();
            }
        }

        fn publish_gpu_stats(&self) {
            if let Ok(mut stats) = self.gpu_stats.lock() {
                *stats = GpuStats {
                    gpu_us_per_step: self.gpu_us_per_step,
                    batch_size: self.batch_size,
                    adaptive: self.adaptive,
                };
            }
        }
    }

    /// Handle to the GPU sim thread. Dropping it shuts the thread down and joins it.
    ///
    /// Shaped like [`crate::cpu::sim_thread::SimThread`], so `henad-app` can hold a thin enum
    /// over the two backends instead of special-casing GPU everywhere.
    pub struct GpuSimThread {
        cmd_tx: mpsc::Sender<Command>,
        snapshot: Arc<Mutex<Option<Snapshot>>>,
        gpu_stats: Arc<Mutex<GpuStats>>,
        handle: Option<JoinHandle<()>>,
    }

    impl GpuSimThread {
        /// Spawns the GPU sim thread, taking ownership of `state` and a cloned [`GpuContext`].
        /// Starts paused, like [`crate::cpu::sim_thread::SimThread`].
        pub fn new(
            ctx: GpuContext,
            state: Box<dyn GpuSimState>,
            settings: GpuBatchSettings,
            wake: Option<WakeFn>,
        ) -> Self {
            let (cmd_tx, cmd_rx) = mpsc::channel();
            let batch_size = settings.batch_size.max(1);

            let snapshot: Arc<Mutex<Option<Snapshot>>> = Arc::new(Mutex::new(None));
            let gpu_stats = Arc::new(Mutex::new(GpuStats {
                gpu_us_per_step: None,
                batch_size,
                adaptive: settings.adaptive,
            }));

            let timestamp_query = TimestampQuery::new(&ctx.device, &ctx.queue);
            let faults = ctx.faults.clone();
            let on_fault = wake.clone();

            let sim_loop = GpuSimLoop {
                ctx,
                state,
                cmd_rx,
                snapshot: Arc::clone(&snapshot),
                gpu_stats: Arc::clone(&gpu_stats),
                wake,
                running: false,
                adaptive: settings.adaptive,
                fixed_batch_size: batch_size,
                target_ms: settings.target_ms,
                batch_size,
                ema_time_per_step_ms: None,
                in_flight: None,
                step_count: 0,
                actual_tps: 0.0,
                gpu_us_per_step: None,
                tps_timer: Instant::now(),
                last_snapshot_publish: Instant::now(),
                last_stats_publish: Instant::now(),
                timestamp_query,
            };

            // Outside the loop. A catch per batch would sit in the hot path.
            let handle = std::thread::spawn(move || {
                if let Err(fault) = catching(STEPPING, || sim_loop.run()) {
                    log::error!("{fault}");
                    faults.set_once(fault);
                    if let Some(wake) = &on_fault {
                        wake();
                    }
                }
            });

            Self {
                cmd_tx,
                snapshot,
                gpu_stats,
                handle: Some(handle),
            }
        }

        /// Pacing commands (`SetTargetTps`, `SetUncapped`, `SetTicksPerSnapshot`) are accepted
        /// and ignored. See `handle_command`.
        pub fn send(&mut self, cmd: SimCommand) {
            drop(self.cmd_tx.send(Command::Sim(cmd)));
        }

        pub fn take_snapshot(&mut self) -> Option<Snapshot> {
            self.snapshot.lock().ok()?.take()
        }

        pub fn play(&mut self) {
            self.send(SimCommand::Play);
        }

        pub fn pause(&mut self) {
            self.send(SimCommand::Pause);
        }

        pub fn step_once(&mut self) {
            self.send(SimCommand::StepOnce);
        }

        /// Sets the manual batch size used in fixed mode. Has no visible effect while adaptive
        /// mode is on (the controller drives `batch_size` instead), but is remembered for when it
        /// is turned back off.
        pub fn set_batch_size(&mut self, batch_size: u32) {
            drop(self.cmd_tx.send(Command::Gpu(GpuCommand::SetBatchSize(batch_size))));
        }

        /// Turns adaptive batching on or off. Fixed mode's manual batch size and adaptive mode's
        /// target/EMA are each preserved independently across toggles.
        pub fn set_adaptive(&mut self, enabled: bool) {
            drop(self.cmd_tx.send(Command::Gpu(GpuCommand::SetAdaptive(enabled))));
        }

        /// Sets the per-batch wall-clock budget (ms) used by the adaptive controller. Has no
        /// effect while fixed mode is active, but is remembered for when adaptive is turned on.
        pub fn set_target_ms(&mut self, target_ms: f64) {
            drop(self.cmd_tx.send(Command::Gpu(GpuCommand::SetTargetMs(target_ms))));
        }

        /// Latest published GPU-runner stats. Cheap: a single mutex lock and copy.
        pub fn gpu_stats(&self) -> GpuStats {
            self.gpu_stats.lock().map_or_else(|_| GpuStats::default(), |s| *s)
        }
    }

    impl Drop for GpuSimThread {
        fn drop(&mut self) {
            drop(self.cmd_tx.send(Command::Sim(SimCommand::Shutdown)));
            if let Some(h) = self.handle.take() {
                drop(h.join());
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::GpuSimThread;
