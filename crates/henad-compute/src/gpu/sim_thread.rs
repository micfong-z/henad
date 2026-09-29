//! Owns a GPU-resident sim state and steps it in batched submissions.
//!
//! The GPU sibling of [`crate::cpu::sim_thread`], and driven the same way. [`crate::runner`] owns
//! the difference between a thread of its own and the host's frame loop.
//!
//! # Synchronization
//!
//! Submissions go on the same queue egui renders on, using handles cloned from egui's render
//! state. wgpu serializes submissions to a queue and each is atomic from the GPU's point of view,
//! so egui's render pass samples either the fully-written previous display texture or the
//! fully-written next one, never a torn one. The accepted cost is up to one frame of staleness,
//! which is nothing for a sim running orders of magnitude faster than the refresh rate.
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
use henad_core::action::Schedule;
use henad_core::model::SimState;

use crate::gpu::primitives::readback::StatsPoll;
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

    /// Record the model's declared action at `index`. False when it declares no such one.
    ///
    /// Nothing is recorded when it returns false, so the caller drops the encoder unsubmitted.
    ///
    /// Note that the action's seed is written through the queue, and the write lands when the next
    /// submission starts. Two presses of one action recorded into one encoder would both read the
    /// second seed, so each press needs a submission of its own.
    fn encode_action(&mut self, _encoder: &mut wgpu::CommandEncoder, _index: usize) -> bool {
        false
    }

    /// Record the display pass (state -> display texture) and the stats-reduction pass
    /// (state -> a handful of numbers), at the snapshot cadence rather than every step.
    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder);

    /// Records the stats-reduction passes of [`Self::encode_snapshot_passes`], without its display pass.
    ///
    /// The default records the display pass too.
    fn encode_stats_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.encode_snapshot_passes(encoder);
    }

    /// Start the async stats readback. Called immediately after the submission that
    /// [`Self::encode_snapshot_passes`] or [`Self::encode_stats_passes`] was recorded into, since mapping earlier
    /// races the copy.
    fn begin_stats_readback(&mut self);

    /// Complete an in-flight stats readback, updating what `SimState::stats()` returns.
    ///
    /// With `block = false` this must not wait on the GPU. It runs every loop iteration, and
    /// stalling until the queue drains is what this thread exists to avoid. `block = true` is for
    /// one-shot snapshots only, where a real value in the stats panel beats a few ms of latency.
    ///
    /// Returns the state of the readback after the poll.
    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll;

    /// True while a readback started by [`Self::begin_stats_readback`] has not landed yet.
    ///
    /// In a browser `block = true` above cannot block, so a one-shot snapshot publishes before its
    /// readback arrives and the loop has to come back for it.
    fn stats_readback_pending(&self) -> bool;

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

/// Display texture refresh and `Snapshot` publish cadence. Independent of batch size and of how
/// fast the sim is actually running.
const SNAPSHOT_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);

/// Refresh cadence for wall-clock TPS and the GPU timestamp readback.
const STATS_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

use std::sync::{Arc, Mutex};

use web_time::Instant;

use crate::cpu::sim_thread::{SimCommand, WakeFn};
use crate::gpu::timing::{ADAPTIVE_EMA_ALPHA, TimestampQuery, ema_update, next_batch_size, time_per_step_ms, tps_over};
use crate::gpu::{GpuContext, MAX_STEPS_PER_SUBMISSION};
use crate::runner::{Driver, Pace, RUN_TO_PUBLISH_INTERVAL, SharedSlot, SimLoop, SnapshotSlot};
use crate::snapshot::{Snapshot, SnapshotView};

/// How long to leave an outstanding batch alone before looking again.
///
/// Only ever reached where the wait cannot block. See [`Loop::await_previous`].
const OUTSTANDING_POLL: std::time::Duration = std::time::Duration::from_millis(1);

enum Command {
    Sim(SimCommand),
    Gpu(GpuCommand),
}

/// The batch the GPU is working on.
///
/// Native waits on the submission itself, which is both the completion signal and the point the
/// sample is taken. See [`Loop::await_previous`].
#[cfg(not(target_arch = "wasm32"))]
struct InFlight {
    submission: wgpu::SubmissionIndex,
    steps: u32,
    started_at: Instant,
}

/// The batch the GPU is working on.
///
/// `on_submitted_work_done` is the only completion signal in a browser, and the callback stamps
/// the duration itself. Nothing here can look sooner than the next frame, and measuring up to that
/// point would put a frame on every batch however fast the GPU was, walking the batch size to 1.
///
/// Zero means still running, so a real duration is stored as microseconds plus one.
#[cfg(target_arch = "wasm32")]
struct InFlight {
    elapsed_us: Arc<std::sync::atomic::AtomicU64>,
    steps: u32,
}

/// Steps a GPU-resident state in batched submissions. [`Driver`] decides what drives it.
struct Loop {
    ctx: GpuContext,
    state: Box<dyn GpuSimState>,
    slot: SharedSlot,
    gpu_stats: Arc<Mutex<GpuStats>>,
    wake: Option<WakeFn>,
    running: bool,
    /// A separate bool rather than a `BatchMode` enum, so each mode's state survives a toggle. The
    /// manual size is remembered while adaptive runs, and the target survives switching back to
    /// fixed.
    adaptive: bool,
    /// Manual batch size, used verbatim when `adaptive` is false.
    fixed_batch_size: u32,
    /// Per-batch wall-clock budget in milliseconds, used by the controller when `adaptive`.
    target_ms: f64,
    /// Size of the next batch. `fixed_batch_size` when not adaptive, the controller's last output
    /// when adaptive.
    batch_size: u32,
    /// EMA of measured wall-clock time per step, in milliseconds. `None` until the first batch has
    /// been measured.
    ema_time_per_step_ms: Option<f64>,
    in_flight: Option<InFlight>,
    step_count: u64,
    actual_tps: f64,
    gpu_us_per_step: Option<f64>,
    tps_timer: Instant,
    last_snapshot_publish: Instant,
    last_stats_publish: Instant,
    /// Number of snapshots published.
    serial: u64,
    /// `None` where the device granted no `TIMESTAMP_QUERY`, which is every browser.
    timestamp_query: Option<TimestampQuery>,
    /// Actions recorded after the step that reaches their tick.
    schedule: Schedule,
    /// Highest tick whose actions have had their turn to fire. `None` until the first step or schedule.
    fired_through: Option<u64>,
    /// Tick a pending [`SimCommand::RunTo`] stops at.
    run_to_target: Option<u64>,
}

impl SimLoop for Loop {
    type Command = Command;

    /// Publish before anything runs, so the viewport shows the seeded grid the moment the model is
    /// loaded rather than staying blank until Play.
    fn start(&mut self) {
        self.snapshot_now();
    }

    fn handle_command(&mut self, cmd: Command) -> bool {
        match cmd {
            Command::Sim(SimCommand::Play) => {
                self.run_to_target = None;
                self.running = true;
                self.reset_tps_window(Instant::now());
            }
            Command::Sim(SimCommand::Pause) => {
                self.run_to_target = None;
                self.running = false;
                self.actual_tps = 0.0;
                self.snapshot_now();
            }
            Command::Sim(SimCommand::StepOnce) => {
                self.run_to_target = None;
                let mut encoder = self.encoder("henad_gpu_step_once");
                self.state.encode_steps(&mut encoder, 1, None);
                self.ctx.queue.submit(Some(encoder.finish()));
                fire_due(&mut *self.state, &self.ctx, &self.schedule);
                self.fired_through = Some(self.state.tick());
                self.snapshot_now();
            }
            Command::Sim(SimCommand::SetParam { index, value }) => {
                if !self.state.set_param(index, &value) {
                    log::warn!("Failed to set param index {index} to {value:?}");
                }
            }
            // No GPU analogue: this loop runs flat-out and paces itself with the batch-size
            // controller instead of a TPS cap, and its snapshot cadence is wall-clock-driven rather
            // than a tick count. Accepted (rather than an error) so `HenadApp` can send the same
            // `SimCommand` stream to either backend without special-casing.
            Command::Sim(
                SimCommand::SetTargetTps(_)
                | SimCommand::SetUncapped(_)
                | SimCommand::SetTicksPerSnapshot(_)
                // No GPU model has a layout.
                | SimCommand::SetLayout { .. },
            ) => {}
            Command::Sim(SimCommand::Act(index)) => {
                let mut encoder = self.encoder("henad_gpu_action");
                if self.state.encode_action(&mut encoder, index) {
                    self.ctx.queue.submit(Some(encoder.finish()));
                    self.snapshot_now();
                } else {
                    log::warn!("Model has no action at index {index}");
                }
            }
            Command::Sim(SimCommand::SetSchedule(schedule)) => {
                self.schedule = schedule;
                if self.fired_through.is_none_or(|through| through < self.state.tick()) {
                    fire_due(&mut *self.state, &self.ctx, &self.schedule);
                    self.fired_through = Some(self.state.tick());
                }
                self.snapshot_now();
            }
            Command::Sim(SimCommand::RunTo(target)) => {
                self.run_to_target = Some(target);
                self.running = false;
                self.reset_tps_window(Instant::now());
            }
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

    fn pump(&mut self) -> Pace {
        // A device error raised by `submit` lands in the sink rather than unwinding. The loop finds
        // out about it here. Stepping on would only pile up more.
        if self.ctx.faults.is_set() {
            if self.running || self.run_to_target.is_some() {
                self.running = false;
                self.run_to_target = None;
                self.actual_tps = 0.0;
                self.publish_snapshot();
            }
            return Pace::Idle;
        }
        if !self.running && self.run_to_target.is_none() {
            return self.collect_late_stats();
        }
        if !self.await_previous() {
            return Pace::After(OUTSTANDING_POLL);
        }
        if self.run_to_target.is_some_and(|target| self.state.tick() >= target) {
            self.run_to_target = None;
            self.actual_tps = 0.0;
            self.snapshot_now();
            return self.collect_late_stats();
        }
        self.step_batch();
        Pace::Now
    }
}

impl Loop {
    fn encoder(&self, label: &str) -> wgpu::CommandEncoder {
        self.ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some(label) })
    }

    /// True once nothing is outstanding. False leaves the batch running, which only happens where
    /// the wait cannot block.
    ///
    /// Waits on the submission itself, which is both the completion signal and the point the sample
    /// is taken. The sample is one loop period, from the start of encoding a batch to the GPU
    /// finishing it. Encode plus submit alone reads CPU dispatch cost, orders of magnitude too low.
    #[cfg(not(target_arch = "wasm32"))]
    fn await_previous(&mut self) -> bool {
        let Some(prev) = self.in_flight.take() else {
            return true;
        };
        drop(self.ctx.device.poll(wgpu::PollType::Wait {
            submission_index: Some(prev.submission),
            timeout: None,
        }));
        self.record_sample(time_per_step_ms(prev.started_at.elapsed(), prev.steps));
        true
    }

    /// Reads the duration the completion callback stamped, and reports the batch still running
    /// until it lands. WebGPU's `Device::poll` returns immediately whatever the queue is doing, so
    /// there is nothing here to wait on.
    #[cfg(target_arch = "wasm32")]
    fn await_previous(&mut self) -> bool {
        use std::sync::atomic::Ordering;

        let Some(prev) = self.in_flight.as_ref() else {
            return true;
        };
        let Some(elapsed_us) = prev.elapsed_us.load(Ordering::Relaxed).checked_sub(1) else {
            return false;
        };
        let steps = prev.steps;
        self.in_flight = None;
        self.record_sample(time_per_step_ms(std::time::Duration::from_micros(elapsed_us), steps));
        true
    }

    /// Folds a batch's measured cost into the EMA the controller sizes the next one from.
    fn record_sample(&mut self, sample: f64) {
        let ema = ema_update(self.ema_time_per_step_ms, sample, ADAPTIVE_EMA_ALPHA);
        self.ema_time_per_step_ms = Some(ema);
        if self.adaptive {
            self.batch_size = next_batch_size(ema, self.target_ms);
        }
    }

    /// Remembers the batch just submitted, so the next pump can wait on it.
    #[cfg(not(target_arch = "wasm32"))]
    fn track(&mut self, submission: Option<wgpu::SubmissionIndex>, steps: u32, started_at: Instant) {
        self.in_flight = submission.map(|submission| InFlight {
            submission,
            steps,
            started_at,
        });
    }

    /// One registration covers every submission above.
    #[cfg(target_arch = "wasm32")]
    fn track(&mut self, submission: Option<wgpu::SubmissionIndex>, steps: u32, started_at: Instant) {
        use std::sync::atomic::{AtomicU64, Ordering};

        drop(submission);
        let elapsed_us = Arc::new(AtomicU64::new(0));
        let signal = Arc::clone(&elapsed_us);
        self.ctx.queue.on_submitted_work_done(move || {
            let micros = started_at.elapsed().as_micros().min(u128::from(u64::MAX - 1)) as u64;
            signal.store(micros + 1, Ordering::Relaxed);
        });
        self.in_flight = Some(InFlight { elapsed_us, steps });
    }

    /// Picks up a readback a one-shot snapshot could not wait for, and republishes once it lands.
    ///
    /// Browsers only. `poll_stats_readback(block = true)` returns without waiting there, so the
    /// snapshot goes out reporting whatever the previous readback left, zeroes after a build.
    /// Nothing else looks again while paused, and the wake is what brings a frame-driven loop back.
    fn collect_late_stats(&mut self) -> Pace {
        if !self.state.stats_readback_pending() {
            return Pace::Idle;
        }
        self.state.poll_stats_readback(&self.ctx.device, false);
        if self.state.stats_readback_pending() {
            if let Some(wake) = &self.wake {
                wake();
            }
            return Pace::After(OUTSTANDING_POLL);
        }
        self.publish_snapshot();
        Pace::Idle
    }

    /// Returns the steps from the state's tick to the next tick an action is due at.
    fn steps_to_next_action(&self) -> Option<u64> {
        let tick = self.state.tick();
        self.schedule.next_due_after(tick).map(|due| due - tick)
    }

    /// Refresh the display texture and stats and publish a snapshot right now. Used for one-shot
    /// updates (initial, pause, step-once).
    ///
    /// The outstanding batch is dropped rather than timed. Its sample would cover the one-shot work
    /// too, and on native the readback has drained the queue underneath it anyway.
    fn snapshot_now(&mut self) {
        let mut encoder = self.encoder("henad_gpu_snapshot_now");
        self.state.encode_snapshot_passes(&mut encoder);
        self.ctx.queue.submit(Some(encoder.finish()));
        self.state.begin_stats_readback();
        self.state.poll_stats_readback(&self.ctx.device, true);
        self.in_flight = None;

        self.last_snapshot_publish = Instant::now();
        self.publish_snapshot();
        self.publish_gpu_stats();
    }

    /// Records and submits one batch of steps, plus the display, stats and timestamp-resolve work
    /// at their own cadences, then updates published state.
    ///
    /// Only the first submission of a batch carries the timestamps, so the reported per-step time
    /// divides by that chunk rather than by the batch.
    ///
    /// The timestamp resolve deliberately does not share a command buffer with the writes. See
    /// `TimestampQuery::resolve_after`.
    ///
    /// A batch stops at a pending run-to target, and a submission ends at each tick an action is due at. The actions
    /// follow in submissions of their own, and the snapshot passes after them.
    fn step_batch(&mut self) {
        let now = Instant::now();
        let want_timing =
            self.timestamp_query.is_some() && now.duration_since(self.last_stats_publish) >= STATS_INTERVAL;
        let (batch_size_submitted, snapshot_interval) = match self.run_to_target {
            Some(target) => {
                let remaining = target.saturating_sub(self.state.tick());
                let steps = u64::from(self.batch_size).min(remaining) as u32;
                (steps, RUN_TO_PUBLISH_INTERVAL)
            }
            None => (self.batch_size, SNAPSHOT_INTERVAL),
        };
        // The batch that reaches the target leaves the snapshot to the blocking one the next pump takes there.
        let reaches_target = self
            .run_to_target
            .is_some_and(|target| self.state.tick() + u64::from(batch_size_submitted) >= target);
        let want_snapshot = !reaches_target && now.duration_since(self.last_snapshot_publish) >= snapshot_interval;

        let query_set = if want_timing {
            self.timestamp_query.as_ref().map(TimestampQuery::query_set)
        } else {
            None
        };

        let mut submitted = 0;
        let mut stamped_steps = None;
        let mut write_submission = None;
        while submitted < batch_size_submitted {
            let steps_to_action = self.steps_to_next_action();
            let chunk = MAX_STEPS_PER_SUBMISSION.min(batch_size_submitted - submitted);
            let chunk = steps_to_action.map_or(chunk, |steps| steps.min(u64::from(chunk)) as u32);
            let action_due = steps_to_action == Some(u64::from(chunk));
            let mut encoder = self.encoder("henad_gpu_sim_encoder");

            let stamp = query_set.filter(|_| stamped_steps.is_none());
            if stamp.is_some() {
                stamped_steps = Some(chunk);
            }
            self.state.encode_steps(&mut encoder, chunk, stamp);

            submitted += chunk;
            let snapshot_here = want_snapshot && submitted >= batch_size_submitted;
            if snapshot_here && !action_due {
                self.state.encode_snapshot_passes(&mut encoder);
            }
            write_submission = Some(self.ctx.queue.submit(Some(encoder.finish())));
            if action_due {
                write_submission = fire_due(&mut *self.state, &self.ctx, &self.schedule).or(write_submission);
                if snapshot_here {
                    let mut encoder = self.encoder("henad_gpu_snapshot");
                    self.state.encode_snapshot_passes(&mut encoder);
                    write_submission = Some(self.ctx.queue.submit(Some(encoder.finish())));
                }
            }
        }
        // Every tick the batch passed has had its turn. A due action ended its chunk and fired above.
        self.fired_through = Some(self.state.tick());

        self.step_count += u64::from(batch_size_submitted);

        if want_snapshot {
            self.state.begin_stats_readback();
        }
        // Non-blocking: picks up a readback started on an earlier pump if the GPU has caught up
        // with it by now. Never stalls.
        self.state.poll_stats_readback(&self.ctx.device, false);

        if want_timing && let (Some(tq), Some(submission)) = (self.timestamp_query.as_ref(), write_submission.clone()) {
            tq.resolve_after(&self.ctx.device, &self.ctx.queue, submission);
        }

        if want_timing {
            if let (Some(tq), Some(steps)) = (self.timestamp_query.as_ref(), stamped_steps) {
                self.gpu_us_per_step = tq.read_gpu_us_per_step(&self.ctx.device, steps);
            }
            self.refresh_tps(now);
            self.last_stats_publish = now;
        } else if self.timestamp_query.is_none() && now.duration_since(self.tps_timer) >= STATS_INTERVAL {
            // No GPU timing on this device, so refresh wall-clock TPS on the same cadence anyway,
            // to keep the UI updating.
            self.refresh_tps(now);
        }

        if want_snapshot {
            self.last_snapshot_publish = now;
            self.publish_snapshot();
            self.publish_gpu_stats();
        }

        self.track(write_submission, batch_size_submitted, now);
    }

    /// Both clocks together. `want_timing` is gated on `last_stats_publish` but divides by
    /// `tps_timer`, so resetting one without the other makes the next refresh divide a whole batch
    /// by whatever tiny gap is between them.
    fn reset_tps_window(&mut self, now: Instant) {
        self.tps_timer = now;
        self.last_stats_publish = now;
        self.step_count = 0;
    }

    fn refresh_tps(&mut self, now: Instant) {
        let Some(tps) = tps_over(self.step_count, now.duration_since(self.tps_timer)) else {
            // Leave the window open rather than reporting a rate over nothing. `step_count` keeps
            // accumulating, so the next refresh covers both.
            return;
        };
        self.actual_tps = tps;
        self.step_count = 0;
        self.tps_timer = now;
    }

    fn publish_snapshot(&mut self) {
        self.serial += 1;
        let snap = Snapshot {
            serial: self.serial,
            // The GPU path has no `prepare_view`.
            view_ms: 0.0,
            tick: self.state.tick(),
            population: self.state.population(),
            heap_bytes: self.state.heap_bytes(),
            actual_tps: self.actual_tps,
            // Report true GPU cost per step where the toolbar shows CPU engine time. Falls back to
            // 0 when the adapter has no timestamp support, same as "unknown".
            engine_ms: self.gpu_us_per_step.unwrap_or(0.0) / 1000.0,
            view: SnapshotView::Gpu(self.state.view()),
            stats: self.state.stats(),
        };
        crate::runner::publish(&self.slot, snap);
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

/// Records the actions of `schedule` due at the tick of `state`, each in a submission of its own as
/// [`GpuSimState::encode_action`] requires, and returns the last of those submissions.
fn fire_due(state: &mut dyn GpuSimState, ctx: &GpuContext, schedule: &Schedule) -> Option<wgpu::SubmissionIndex> {
    let tick = state.tick();
    let mut last = None;
    for action in schedule.due(tick) {
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("henad_gpu_scheduled_action"),
        });
        if state.encode_action(&mut encoder, action.index) {
            last = Some(ctx.queue.submit(Some(encoder.finish())));
        } else {
            log::warn!("Model refused action '{}' at tick {tick}", action.id);
        }
    }
    last
}

/// Handle on a running GPU simulation.
///
/// Shaped like [`crate::cpu::sim_thread::SimThread`], so `henad-app` can hold a thin enum over the
/// two backends instead of special-casing GPU everywhere.
pub struct GpuSimThread {
    driver: Driver<Loop>,
    slot: SharedSlot,
    gpu_stats: Arc<Mutex<GpuStats>>,
}

impl GpuSimThread {
    /// Starts paused, like [`crate::cpu::sim_thread::SimThread`].
    pub fn new(ctx: GpuContext, state: Box<dyn GpuSimState>, settings: GpuBatchSettings, wake: Option<WakeFn>) -> Self {
        let batch_size = settings.batch_size.max(1);
        let gpu_stats = Arc::new(Mutex::new(GpuStats {
            gpu_us_per_step: None,
            batch_size,
            adaptive: settings.adaptive,
        }));
        // Left empty. `start` publishes the first one after a blocking readback, and anything
        // built here would report stats the GPU has not produced yet.
        let slot = SnapshotSlot::empty();

        let timestamp_query = TimestampQuery::new(&ctx.device, &ctx.queue);
        let now = Instant::now();
        let faults = ctx.faults.clone();
        let sim = Loop {
            ctx,
            state,
            slot: SharedSlot::clone(&slot),
            gpu_stats: Arc::clone(&gpu_stats),
            wake: wake.clone(),
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
            tps_timer: now,
            last_snapshot_publish: now,
            last_stats_publish: now,
            serial: 0,
            timestamp_query,
            schedule: Schedule::default(),
            fired_through: None,
            run_to_target: None,
        };

        let driver = Driver::spawn(sim, move |fault| {
            faults.set_once(fault);
            if let Some(wake) = &wake {
                wake();
            }
        });

        Self {
            driver,
            slot,
            gpu_stats,
        }
    }

    pub fn send(&mut self, cmd: SimCommand) {
        self.driver.send(Command::Sim(cmd));
    }

    pub fn take_snapshot(&mut self) -> Option<Snapshot> {
        crate::runner::take_snapshot(&self.slot)
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

    pub fn set_schedule(&mut self, schedule: Schedule) {
        self.send(SimCommand::SetSchedule(schedule));
    }

    pub fn run_to(&mut self, tick: u64) {
        self.send(SimCommand::RunTo(tick));
    }

    /// Sets the manual batch size used in fixed mode. Has no visible effect while adaptive mode is
    /// on, but is remembered for when it is turned back off.
    pub fn set_batch_size(&mut self, batch_size: u32) {
        self.driver.send(Command::Gpu(GpuCommand::SetBatchSize(batch_size)));
    }

    /// Turns adaptive batching on or off. Fixed mode's manual batch size and adaptive mode's target
    /// are each preserved independently across toggles.
    pub fn set_adaptive(&mut self, enabled: bool) {
        self.driver.send(Command::Gpu(GpuCommand::SetAdaptive(enabled)));
    }

    /// Sets the per-batch wall-clock budget (ms) used by the adaptive controller.
    pub fn set_target_ms(&mut self, target_ms: f64) {
        self.driver.send(Command::Gpu(GpuCommand::SetTargetMs(target_ms)));
    }

    pub fn gpu_stats(&self) -> GpuStats {
        self.gpu_stats.lock().map(|s| *s).unwrap_or_default()
    }

    /// Advances the sim where the driver has no thread of its own. A no-op where it has.
    pub fn update(&mut self, dt: f64) {
        self.driver.update(dt);
    }
}

impl Drop for GpuSimThread {
    fn drop(&mut self) {
        self.driver.shutdown(Command::Sim(SimCommand::Shutdown));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    use super::{GpuBatchSettings, GpuSimState, GpuSimThread, StatsPoll};
    use crate::gpu::{GpuContext, headless_context};
    use crate::snapshot::{GpuSnapshot, Snapshot};
    use henad_core::action::{Schedule, Scheduled};
    use henad_core::model::SimState;
    use henad_core::params::ParamValue;
    use henad_core::view::StatEntry;

    /// Population lands on the second poll rather than on the blocking one, which is how a browser
    /// behaves: `poll_blocking` there is an ordinary poll and the map resolves a frame later.
    struct LateStats {
        polls_left: u32,
        population: u64,
    }

    const LANDED: u64 = 7;

    impl SimState for LateStats {
        fn step(&mut self) {}
        fn tick(&self) -> u64 {
            0
        }
        fn stats(&self) -> Vec<StatEntry> {
            Vec::new()
        }
        fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
            false
        }
        fn population(&self) -> u64 {
            self.population
        }
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    impl GpuSimState for LateStats {
        fn encode_steps(&mut self, _encoder: &mut wgpu::CommandEncoder, _count: u32, _t: Option<&wgpu::QuerySet>) {}
        fn encode_snapshot_passes(&mut self, _encoder: &mut wgpu::CommandEncoder) {}
        fn begin_stats_readback(&mut self) {}

        fn poll_stats_readback(&mut self, _device: &wgpu::Device, _block: bool) -> StatsPoll {
            self.polls_left = self.polls_left.saturating_sub(1);
            if self.polls_left > 0 {
                return StatsPoll::Pending;
            }
            self.population = LANDED;
            StatsPoll::Landed
        }

        fn stats_readback_pending(&self) -> bool {
            self.polls_left > 0
        }

        fn view(&self) -> GpuSnapshot {
            GpuSnapshot {
                display: None,
                agents: None,
            }
        }
    }

    /// The regression, as the web build shows it. The initial snapshot goes out before the readback
    /// lands, and a paused loop used to go idle without looking again, so population and the stats
    /// panel read zero until the first Play.
    #[test]
    fn a_readback_landing_after_the_initial_snapshot_is_published() {
        let Some(ctx) = headless_context("henad_late_stats_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_readback_landing_after_the_initial_snapshot_is_published: no adapter");
            return;
        };
        let state = LateStats {
            polls_left: 2,
            population: 0,
        };
        let mut thread = GpuSimThread::new(ctx, Box::new(state), GpuBatchSettings::default(), None);

        let mut seen = None;
        for _ in 0..200 {
            if let Some(snap) = thread.take_snapshot() {
                seen = Some(snap.population);
                if seen == Some(LANDED) {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(seen, Some(LANDED), "the late readback was never published");
    }

    /// Counts encoded steps as its tick, and records the tick each action is encoded at.
    struct ActionRecorder {
        ticks: Arc<AtomicU64>,
        fired: Arc<Mutex<Vec<u64>>>,
    }

    impl SimState for ActionRecorder {
        fn step(&mut self) {
            self.ticks.fetch_add(1, Ordering::Relaxed);
        }
        fn tick(&self) -> u64 {
            self.ticks.load(Ordering::Relaxed)
        }
        fn stats(&self) -> Vec<StatEntry> {
            Vec::new()
        }
        fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
            false
        }
        fn population(&self) -> u64 {
            0
        }
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    impl GpuSimState for ActionRecorder {
        fn encode_steps(
            &mut self,
            _encoder: &mut wgpu::CommandEncoder,
            count: u32,
            _timestamps: Option<&wgpu::QuerySet>,
        ) {
            self.ticks.fetch_add(u64::from(count), Ordering::Relaxed);
        }
        fn encode_action(&mut self, _encoder: &mut wgpu::CommandEncoder, _index: usize) -> bool {
            self.fired.lock().expect("action log").push(self.tick());
            true
        }
        fn encode_snapshot_passes(&mut self, _encoder: &mut wgpu::CommandEncoder) {}
        fn begin_stats_readback(&mut self) {}
        fn poll_stats_readback(&mut self, _device: &wgpu::Device, _block: bool) -> StatsPoll {
            StatsPoll::Landed
        }
        fn stats_readback_pending(&self) -> bool {
            false
        }
        fn view(&self) -> GpuSnapshot {
            GpuSnapshot {
                display: None,
                agents: None,
            }
        }
    }

    /// Returns a thread over an [`ActionRecorder`] state in batches of a fixed 100 steps, its tick and its action log.
    fn action_recorder(ctx: GpuContext) -> (GpuSimThread, Arc<AtomicU64>, Arc<Mutex<Vec<u64>>>) {
        let ticks = Arc::new(AtomicU64::new(0));
        let fired = Arc::new(Mutex::new(Vec::new()));
        let state = ActionRecorder {
            ticks: Arc::clone(&ticks),
            fired: Arc::clone(&fired),
        };
        let settings = GpuBatchSettings {
            adaptive: false,
            batch_size: 100,
            ..GpuBatchSettings::default()
        };
        (GpuSimThread::new(ctx, Box::new(state), settings, None), ticks, fired)
    }

    /// Takes snapshots until one reports `tick`, or gives up after five seconds, and returns it with the highest
    /// tick seen on the way.
    fn snapshot_at(thread: &mut GpuSimThread, tick: u64) -> (Option<Snapshot>, u64) {
        let mut highest = 0;
        for _ in 0..500 {
            if let Some(snap) = thread.take_snapshot() {
                highest = highest.max(snap.tick);
                if snap.tick == tick {
                    return (Some(snap), highest);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        (None, highest)
    }

    /// Waits long enough that nothing else can publish once the loop has paused.
    fn settle() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    #[test]
    fn run_to_stops_at_the_target() {
        let Some(ctx) = headless_context("henad_run_to_test", wgpu::Features::empty()) else {
            log::warn!("skipping run_to_stops_at_the_target: no adapter");
            return;
        };
        let (mut thread, ticks, _) = action_recorder(ctx);
        // Batches of 100 go out as submissions of 64 and 36, and the target falls inside the eleventh.
        thread.run_to(1003);
        let (reached, highest) = snapshot_at(&mut thread, 1003);
        let reached = reached.expect("the run never published its target");
        assert_eq!(highest, 1003, "a snapshot went past the target");
        assert_eq!(reached.actual_tps, 0.0, "a paused run reported a rate");
        settle();
        assert_eq!(ticks.load(Ordering::Relaxed), 1003, "the run went past its target");
        assert!(thread.take_snapshot().is_none(), "a paused run kept publishing");

        thread.run_to(10);
        assert!(
            snapshot_at(&mut thread, 1003).0.is_some(),
            "a run to a tick behind the current one pauses and publishes"
        );
        settle();
        assert_eq!(ticks.load(Ordering::Relaxed), 1003);
    }

    #[test]
    fn a_gpu_schedule_fires_once_per_tick_after_the_step_reaching_it() {
        let Some(ctx) = headless_context("henad_gpu_schedule_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_gpu_schedule_fires_once_per_tick_after_the_step_reaching_it: no adapter");
            return;
        };
        let (mut thread, ticks, fired) = action_recorder(ctx);
        let entries = [0, 5, 64, 64, 100, 101, 250]
            .map(|tick| Scheduled {
                index: 0,
                id: "mark".to_owned(),
                tick,
            })
            .to_vec();
        thread.set_schedule(Schedule::from_entries(entries));
        thread.run_to(100);
        assert!(snapshot_at(&mut thread, 100).0.is_some());
        let fired_ticks = || fired.lock().expect("action log").clone();
        assert_eq!(fired_ticks(), [0, 5, 64, 64, 100], "each action fires at its own tick");

        thread.step_once();
        assert!(snapshot_at(&mut thread, 101).0.is_some());
        thread.run_to(200);
        assert!(snapshot_at(&mut thread, 200).0.is_some());
        assert_eq!(fired_ticks(), [0, 5, 64, 64, 100, 101]);
        assert_eq!(ticks.load(Ordering::Relaxed), 200);
    }

    /// The regression. A second schedule used to record the actions of the tick the loop sat at a second time.
    #[test]
    fn a_replaced_gpu_schedule_never_fires_a_tick_again() {
        let Some(ctx) = headless_context("henad_gpu_replaced_schedule_test", wgpu::Features::empty()) else {
            log::warn!("skipping a_replaced_gpu_schedule_never_fires_a_tick_again: no adapter");
            return;
        };
        let (mut thread, _, fired) = action_recorder(ctx);
        let schedule = |ticks: &[u64]| {
            let entries = ticks
                .iter()
                .map(|&tick| Scheduled {
                    index: 0,
                    id: "mark".to_owned(),
                    tick,
                })
                .collect();
            Schedule::from_entries(entries)
        };
        let fired_ticks = || fired.lock().expect("action log").clone();

        thread.set_schedule(schedule(&[0, 40]));
        thread.set_schedule(schedule(&[0, 40, 90]));
        thread.run_to(40);
        assert!(snapshot_at(&mut thread, 40).0.is_some());
        assert_eq!(fired_ticks(), [0, 40], "a second schedule at tick 0 fired it again");

        thread.set_schedule(schedule(&[40, 90]));
        thread.run_to(90);
        assert!(snapshot_at(&mut thread, 90).0.is_some());
        assert_eq!(
            fired_ticks(),
            [0, 40, 90],
            "a schedule at a tick a batch reached fired it again"
        );

        // Tick 150 ends a batch with nothing due, and has had its turn all the same.
        thread.run_to(150);
        assert!(snapshot_at(&mut thread, 150).0.is_some());
        thread.set_schedule(schedule(&[150, 151]));
        thread.step_once();
        assert!(snapshot_at(&mut thread, 151).0.is_some());
        assert_eq!(fired_ticks(), [0, 40, 90, 151]);
    }
}
