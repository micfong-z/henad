//! Dedicated OS thread that owns a GPU-resident sim state and steps it in batched submissions,
//! decoupled from the UI frame rate. This is the GPU version of [`crate::sim_thread`]'s CPU sim thread.
//!
//! # [`GpuSimState`] is a runner interface, not a model-authoring shortcut
//!
//! `GridModel` is a *model-authoring* abstraction: implement some consts and pure functions, and
//! the engine derives everything. [`GpuSimState`] is not that. It is the interface this thread
//! drives, exactly as `SimState` is the interface the CPU thread drives — the minimum needed to
//! record work into an encoder that this crate then submits and paces.
//!
//! The model-authoring counterpart is `henad_core::gpu_grid_model::GpuGridModel`, implemented by
//! [`crate::gpu::gpu_grid_engine::GpuGridState`]. A GPU model that does not fit
//! the grid mould could still implement this trait by hand.
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
//! (see the model's `encode_steps` for why — wgpu only synchronizes between passes, not between
//! dispatches within one pass, and ping-pong buffers need that). The display compute pass (state
//! -> texture) and the stats-reduction pass only run when at least [`SNAPSHOT_INTERVAL`] has
//! elapsed since the last one, so "steps per snapshot" is emergent from how fast the batches run,
//! exactly like the CPU sim thread's `ticks_per_snapshot`. This cadence is independent of batch
//! size and unaffected by anything below.
//!
//! `batch_size` itself is either a fixed, UI-set value, or adaptively controlled. The problem
//! adaptive mode solves: on a shared queue with no preemption, a large fixed batch (e.g. 256
//! steps on a 4096x4096 grid) can take on the order of 100ms+ of GPU execution time in one
//! submission, and because egui's own render-pass submissions share that queue, a big batch
//! blocks egui's rendering behind it — visible as UI stutter, even though the display texture is
//! already decoupled from batch size (see above).
//!
//! Adaptive mode measures the wall-clock time to encode and submit each batch (a proxy for GPU
//! cost — see [`GpuSimLoop::step_batch`] for the caveats on why this proxy was chosen and what
//! could make it unreliable), maintains an EMA of `time_per_step`, and picks the next batch size
//! so that `batch_size * time_per_step` tracks a user-set `target_ms` budget. This deliberately
//! does not use `TimestampQuery`, which stays diagnostic-only (surfaced as `gpu_us_per_step`).

use henad_core::model::SimState;

use crate::gpu::timing::{DEFAULT_BATCH_SIZE, DEFAULT_TARGET_MS};
use crate::snapshot::GpuSnapshot;

/// The interface [`GpuSimThread`] drives. See the module docs: this is a *runner* interface (the
/// GPU analogue of how `SimState` is consumed by the CPU thread), not a model-authoring trait.
///
/// A GPU model's grid never leaves the GPU. `SimState::stats()` is therefore expected to report
/// whatever the last completed [`Self::poll_stats_readback`] produced — a value a few
/// milliseconds stale, not a fresh CPU-side count of the grid.
pub trait GpuSimState: SimState {
    /// Record `count` steps into `encoder`, advancing the model's own tick counter by `count`.
    ///
    /// If `timestamps` is `Some`, stamp the beginning of the first step and the end of the last
    /// into query indices 0 and 1, so the caller can measure GPU time for the whole batch.
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>);

    /// Record the display pass (state -> display texture) and the stats-reduction pass
    /// (state -> a handful of numbers), at the snapshot cadence rather than every step.
    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder);

    /// Start the async stats readback. Called immediately after the submission that
    /// [`Self::encode_snapshot_passes`] was recorded into — mapping earlier would race the copy.
    fn begin_stats_readback(&mut self);

    /// Complete an in-flight stats readback, updating what `SimState::stats()` returns.
    ///
    /// With `block = false` this must not wait on the GPU: it is called every loop iteration, and
    /// stalling the sim thread until the queue drains is precisely what this thread exists to
    /// avoid. `block = true` is used only for one-shot snapshots (initial, pause, step-once),
    /// where the stats panel showing a real value matters more than a few ms of latency.
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
    /// Live batch size — the fixed value in fixed mode, or the controller's current output in
    /// adaptive mode.
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

/// GPU-runner-specific commands, on top of the shared [`crate::sim_thread::SimCommand`].
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
    use crate::gpu::GpuContext;
    use crate::gpu::timing::{ADAPTIVE_EMA_ALPHA, TimestampQuery, ema_update, next_batch_size, time_per_step_ms};
    use crate::sim_thread::{SimCommand, WakeFn};
    use crate::snapshot::{Snapshot, SnapshotView};

    /// How often the display texture is refreshed and a `Snapshot` published. Independent of
    /// batch size and of how fast the sim is actually running.
    const SNAPSHOT_INTERVAL: Duration = Duration::from_millis(16);
    /// How often wall-clock TPS and the (expensive, blocking) GPU timestamp readback are refreshed.
    const STATS_INTERVAL: Duration = Duration::from_secs(1);

    enum Command {
        Sim(SimCommand),
        Gpu(GpuCommand),
    }

    struct GpuSimLoop {
        ctx: GpuContext,
        state: Box<dyn GpuSimState>,
        cmd_rx: mpsc::Receiver<Command>,
        snapshot: Arc<Mutex<Option<Snapshot>>>,
        gpu_stats: Arc<Mutex<GpuStats>>,
        wake: Option<WakeFn>,
        running: bool,
        /// Whether the controller is currently in adaptive mode. Kept as a separate bool (rather
        /// than folding `fixed_batch_size`/`target_ms` into a `BatchMode` enum) so each mode's
        /// state survives toggling — the manual fixed size is remembered while adaptive is
        /// active, and the adaptive controller's target/EMA survive switching back to fixed.
        adaptive: bool,
        /// Manual batch size, used verbatim when `adaptive` is false.
        fixed_batch_size: u32,
        /// Per-batch wall-clock budget in milliseconds, used by the controller when `adaptive`.
        target_ms: f64,
        /// Live batch size for the next batch: `fixed_batch_size` when not adaptive, or the
        /// controller's last output when adaptive.
        batch_size: u32,
        /// EMA of measured wall-clock time per step, in milliseconds. `None` until the first
        /// batch has been measured. Only used/updated in adaptive mode.
        ema_time_per_step_ms: Option<f64>,
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
                    self.tps_timer = Instant::now();
                    self.step_count = 0;
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
                    if enabled {
                        // Reset the estimator so a stale EMA from a previous adaptive session
                        // (e.g. measured on a different grid size) doesn't bias the first batches.
                        self.ema_time_per_step_ms = None;
                    } else {
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

            self.last_snapshot_publish = Instant::now();
            self.publish_snapshot();
            self.publish_gpu_stats();
        }

        /// Records and submits one batch of steps (plus, at snapshot/stats cadence, the display
        /// and stats passes and/or a timestamped-query resolve), then updates published state.
        ///
        /// The timestamp-query resolve is deliberately *not* recorded into the same command
        /// buffer as the writes — see `TimestampQuery::resolve_after` for why.
        ///
        /// Also times the encode+submit portion as the adaptive controller's cost signal. This is
        /// deliberately wall-clock CPU time, not a GPU timestamp query: `queue.submit()` isn't
        /// required to block for GPU completion, so this is not a direct measure of GPU execution
        /// time. The assumption underpinning this choice — that on a queue kept continuously busy
        /// by back-to-back batches from this thread, with no other CPU work in between, the *rate*
        /// at which `submit()` calls can be issued ends up backpressured by how fast the GPU
        /// drains the queue — is plausible but has **not** been empirically verified. If it
        /// doesn't hold on some backend/platform (e.g. `submit()` returns immediately regardless
        /// of queue depth), this instead mostly measures CPU-side dispatch-recording cost, which
        /// scales close to linearly with `batch_size` — so `time_per_step` would stay roughly
        /// constant regardless of true GPU load, and the controller would regulate encode cost
        /// rather than the GPU-stutter problem it's meant to solve. Flagging this as the main open
        /// risk of this design rather than asserting it works. It's cheap either way (no readback
        /// stall) and unaffected by the `TimestampQuery` correctness issue tracked separately.
        fn step_batch(&mut self) {
            let now = Instant::now();
            let want_timing =
                self.timestamp_query.is_some() && now.duration_since(self.last_stats_publish) >= STATS_INTERVAL;
            let want_snapshot = now.duration_since(self.last_snapshot_publish) >= SNAPSHOT_INTERVAL;

            let mut encoder = self.encoder("henad_gpu_sim_encoder");

            let query_set = if want_timing {
                self.timestamp_query.as_ref().map(TimestampQuery::query_set)
            } else {
                None
            };
            self.state.encode_steps(&mut encoder, self.batch_size, query_set);

            if want_snapshot {
                self.state.encode_snapshot_passes(&mut encoder);
            }

            let batch_size_submitted = self.batch_size;
            let write_submission = self.ctx.queue.submit(Some(encoder.finish()));
            let batch_wall_elapsed = Instant::now().duration_since(now);
            self.step_count += u64::from(batch_size_submitted);

            if want_snapshot {
                self.state.begin_stats_readback();
            }
            // Non-blocking: picks up a readback started on an earlier iteration if the GPU has
            // caught up with it by now. Never stalls this thread.
            self.state.poll_stats_readback(&self.ctx.device, false);

            if want_timing && let Some(tq) = self.timestamp_query.as_ref() {
                tq.resolve_after(&self.ctx.device, &self.ctx.queue, write_submission);
            }

            if self.adaptive {
                let sample = time_per_step_ms(batch_wall_elapsed, batch_size_submitted);
                let ema = ema_update(self.ema_time_per_step_ms, sample, ADAPTIVE_EMA_ALPHA);
                self.ema_time_per_step_ms = Some(ema);
                self.batch_size = next_batch_size(ema, self.target_ms);
            }

            if want_timing {
                if let Some(tq) = self.timestamp_query.as_ref() {
                    self.gpu_us_per_step = tq.read_gpu_us_per_step(&self.ctx.device, batch_size_submitted);
                }
                self.refresh_tps(now);
                self.last_stats_publish = now;
            } else if self.timestamp_query.is_none() && now.duration_since(self.tps_timer) >= STATS_INTERVAL {
                // No GPU timing support on this device/backend — still refresh wall-clock TPS on
                // the same cadence so the UI keeps updating.
                self.refresh_tps(now);
            }

            if want_snapshot {
                self.last_snapshot_publish = now;
                self.publish_snapshot();
                self.publish_gpu_stats();
            }
        }

        fn refresh_tps(&mut self, now: Instant) {
            let elapsed = now.duration_since(self.tps_timer).as_secs_f64().max(f64::EPSILON);
            self.actual_tps = self.step_count as f64 / elapsed;
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

    /// Handle to the GPU sim thread.
    ///
    /// Deliberately shaped like
    /// [`crate::sim_thread::SimThread`] — `send`/`play`/`pause`/`step_once`/`take_snapshot` — so
    /// `henad-app` can hold a thin enum over the two backends instead of special-casing GPU
    /// everywhere. Dropping it shuts the thread down and joins it.
    pub struct GpuSimThread {
        cmd_tx: mpsc::Sender<Command>,
        snapshot: Arc<Mutex<Option<Snapshot>>>,
        gpu_stats: Arc<Mutex<GpuStats>>,
        handle: Option<JoinHandle<()>>,
    }

    impl GpuSimThread {
        /// Spawns the GPU sim thread, taking ownership of `state` and a cloned [`GpuContext`].
        /// Starts paused, like [`crate::sim_thread::SimThread`].
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
                step_count: 0,
                actual_tps: 0.0,
                gpu_us_per_step: None,
                tps_timer: Instant::now(),
                last_snapshot_publish: Instant::now(),
                last_stats_publish: Instant::now(),
                timestamp_query,
            };

            let handle = std::thread::spawn(move || sim_loop.run());

            Self {
                cmd_tx,
                snapshot,
                gpu_stats,
                handle: Some(handle),
            }
        }

        /// Send a shared command. Pacing commands (`SetTargetTps`, `SetUncapped`,
        /// `SetTicksPerSnapshot`) are accepted and ignored — see `handle_command`.
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
