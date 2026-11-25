//! The CPU runner [`SimThread`]. It steps a [`SimState`] and publishes [`Snapshot`]s for the host to draw.
//!
//! A capped run steps a batch of ticks at a target rate in ticks per second (TPS), and an uncapped one steps as fast
//! as it can. [`crate::runner`] decides whether the loop runs on its own thread or from the host's frame loop.

use crate::fault::{Fault, FaultSink};
use henad_core::action::Schedule;
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::send_sync::WasmNotSend;

use crate::runner::{
    DEFAULT_STOP_TIMEOUT, Driver, Pace, RUN_TO_PUBLISH_INTERVAL, RunToStride, RunnerShutdown, SharedSlot, SimLoop,
    SnapshotSlot, capped_interval,
};
use crate::snapshot::{CpuLayers, FireCounts, GridSnapshot, PointSnapshot, Snapshot, SnapshotView, StatSample};
use std::time::Duration;
use web_time::Instant;

/// Callback run after every publish, for an idle UI to come and collect the snapshot.
///
/// Without it a publish while the UI is idle, like a single step or the final one after pause,
/// sits unread until some unrelated input event wakes the event loop. The callback must not block.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub type WakeFn = std::sync::Arc<dyn Fn() + Send + Sync>;

/// Callback run after every publish, with no `Send` or `Sync` bound.
///
/// An `egui::Context` is not `Send` under atomics, and no other thread calls this callback.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub type WakeFn = std::sync::Arc<dyn Fn()>;

/// Commands sent from the UI thread to the simulation thread.
#[derive(Debug)]
pub enum SimCommand {
    /// Start stepping. It cancels a pending [`Self::RunTo`].
    Play,
    /// Stop stepping, and publish the state the run stopped at.
    Pause,
    /// Run one step and publish.
    StepOnce,
    /// Set the target rate in ticks per second.
    SetTargetTps(f64),
    /// Step as fast as possible when `true`, ignoring the target rate.
    SetUncapped(bool),
    /// Set the number of ticks one capped batch steps, at least 1.
    SetTicksPerSnapshot(u32),
    /// Set one parameter of the running model.
    SetParam {
        /// Position of the parameter in the full parameter list.
        index: usize,
        /// New value.
        value: ParamValue,
    },
    /// Run the model's declared action at this index, once.
    Act(usize),
    /// Turn the layout on or off, with a time budget per publish in milliseconds.
    ///
    /// The layout relaxes on every publish that follows a tick. While paused, it relaxes only if `while_paused` is set.
    SetLayout {
        /// Whether the layout runs.
        on: bool,
        /// Time in milliseconds that one publish may spend relaxing the layout, capped at
        /// [`MAX_VIEW_BUDGET_MS`](crate::runner::MAX_VIEW_BUDGET_MS).
        budget_ms: f32,
        /// Whether the layout keeps relaxing while paused.
        while_paused: bool,
    },
    /// Replace the actions fired at their ticks, and fire those due at the current tick at once unless it has fired.
    ///
    /// Each later action fires once, after the step that reaches its tick. A tick has fired once a step has reached
    /// it or an earlier schedule has fired it. No tick fires twice.
    SetSchedule(Schedule),
    /// Step as fast as possible to this tick, then pause and publish.
    ///
    /// `Play`, `Pause` and `StepOnce` cancel it. A tick at or behind the current one pauses at once.
    RunTo(u64),
    /// Stop the loop. A runner sends it when stopped or dropped.
    Shutdown,
}

/// Shortest time between two publishes that no command forced, whatever the rate of the sim.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(16);

/// Largest number of steps in one uncapped pump, so a bad estimate cannot buy a long stall.
const MAX_UNCAPPED_STEPS: u32 = 4096;

/// Wall-clock time in milliseconds that one uncapped pump aims to fill.
///
/// The frame driver returns the frame to the host between pumps, and one step per pump would pin a fast model to the
/// refresh rate. Matching the driver's own budget keeps a frame to one pump.
const UNCAPPED_PUMP_MS: f64 = crate::runner::PUMP_BUDGET_MS;

/// Returns the number of steps that fit [`UNCAPPED_PUMP_MS`], from the measured cost of a step.
///
/// `engine_ms` is `None` until a step has been timed, and one step is enough to measure with.
fn uncapped_steps_for(engine_ms: Option<f64>, ticks_per_snapshot: u32) -> u32 {
    let Some(engine_ms) = engine_ms else {
        return 1;
    };
    let fits = if engine_ms > 0.0 {
        (UNCAPPED_PUMP_MS / engine_ms)
            .floor()
            .clamp(1.0, f64::from(MAX_UNCAPPED_STEPS)) as u32
    } else {
        MAX_UNCAPPED_STEPS
    };
    let stride = ticks_per_snapshot.max(1);
    if fits < stride { fits } else { fits - fits % stride }
}

/// Steps a `SimState` and publishes snapshots. [`Driver`] decides what drives it.
struct Loop {
    state: Box<dyn SimState>,
    slot: SharedSlot,
    wake: Option<WakeFn>,
    running: bool,
    target_tps: f64,
    uncapped: bool,
    ticks_per_snapshot: u32,
    step_count: u64,
    tps_timer: Instant,
    actual_tps: f64,
    last_publish: Instant,
    /// Number of snapshots published.
    serial: u64,
    /// Whether the state has a layout that is switched on.
    layout_on: bool,
    /// Whether the layout keeps relaxing while paused.
    relax_paused: bool,
    /// Whether a tick has run since the last publish.
    ticked: bool,
    /// Engine time per tick in milliseconds, as an exponential moving average. `None` until the first step has been
    /// timed.
    engine_ms: Option<f64>,
    /// When the next capped batch falls due.
    next_step_at: Instant,
    /// Actions fired after the step that reaches their tick.
    schedule: Schedule,
    /// Highest tick whose actions have had their turn to fire. `None` until the first step or schedule.
    fired_through: Option<u64>,
    /// Tick a pending [`SimCommand::RunTo`] stops at.
    run_to_target: Option<u64>,
    /// Ticks the pending run to a tick samples its stats at.
    run_to_stride: Option<RunToStride>,
    /// Stats sampled since the last publish.
    run_to_samples: Vec<StatSample>,
}

impl SimLoop for Loop {
    type Command = SimCommand;

    fn handle_command(&mut self, cmd: SimCommand) -> bool {
        match cmd {
            SimCommand::Play => {
                self.run_to_target = None;
                self.running = true;
                self.reset_tps_window();
                self.next_step_at = Instant::now();
            }
            SimCommand::Pause => {
                self.run_to_target = None;
                self.running = false;
                // A stopped sim runs at no rate. The GPU runner reports a pause the same way.
                self.actual_tps = 0.0;
                // Publish a final snapshot, so the UI shows the state it stopped at.
                self.force_publish_snapshot();
            }
            SimCommand::StepOnce => {
                self.run_to_target = None;
                self.timed_step();
                // One step is not a rate. `update_tps` would divide it by however long the pause
                // before it lasted and report a fraction of a tick per second.
                self.reset_tps_window();
                self.force_publish_snapshot();
            }
            SimCommand::SetTargetTps(tps) => {
                self.target_tps = tps;
                self.reclamp_deadline();
            }
            SimCommand::SetUncapped(v) => {
                self.uncapped = v;
            }
            SimCommand::SetTicksPerSnapshot(v) => {
                self.ticks_per_snapshot = v.max(1);
                self.reclamp_deadline();
            }
            SimCommand::SetParam { index, value } => {
                if !self.state.set_param(index, &value) {
                    log::warn!("Failed to set param index {index} to {value:?}");
                }
            }
            SimCommand::SetLayout {
                on,
                budget_ms,
                while_paused,
            } => {
                let budget_ms = budget_ms.min(crate::runner::MAX_VIEW_BUDGET_MS);
                // Called before the `&&`. Inside it, a switch-off would short-circuit and never reach the state.
                let accepted = self.state.set_layout(on, budget_ms);
                self.layout_on = on && accepted;
                self.relax_paused = while_paused;
                self.force_publish_snapshot();
            }
            SimCommand::Act(index) => {
                if self.state.act(index) {
                    // The tick has not moved, so nothing else would publish what the action did.
                    self.force_publish_snapshot();
                } else {
                    log::warn!("Model has no action at index {index}");
                }
            }
            SimCommand::SetSchedule(schedule) => {
                self.schedule = schedule;
                if self.fired_through.is_none_or(|through| through < self.state.tick()) {
                    self.fire_due();
                }
                self.force_publish_snapshot();
            }
            SimCommand::RunTo(target) => {
                self.run_to_target = Some(target);
                self.run_to_stride = Some(RunToStride::new(self.state.tick(), target));
                self.running = false;
                self.reset_tps_window();
            }
            SimCommand::Shutdown => return true,
        }
        false
    }

    fn pump(&mut self) -> Pace {
        if let Some(target) = self.run_to_target {
            return self.advance_to_target(target);
        }
        if !self.running {
            return self.relax_while_paused();
        }
        if self.uncapped {
            for _ in 0..uncapped_steps_for(self.engine_ms, self.ticks_per_snapshot) {
                self.timed_step();
            }
            self.update_tps();
            self.maybe_publish_snapshot();
            return Pace::Now;
        }

        let now = Instant::now();
        if now < self.next_step_at {
            return Pace::After(self.next_step_at - now);
        }
        // Advance from the previous deadline, so the batch's own execution time doesn't stretch
        // every period. Resync if the sim is running behind.
        let interval = self.batch_interval();
        self.next_step_at += interval;
        let now = Instant::now();
        if self.next_step_at + interval < now {
            self.next_step_at = now + interval;
        }
        for _ in 0..self.ticks_per_snapshot {
            self.timed_step();
        }
        self.update_tps();
        self.maybe_publish_snapshot();

        let now = Instant::now();
        if now >= self.next_step_at {
            Pace::Now
        } else {
            Pace::After(self.next_step_at - now)
        }
    }
}

impl Loop {
    /// Returns a paused loop over `state`, capped at `target_tps`, and the slot holding `first` as its first publish.
    fn new(state: Box<dyn SimState>, first: Snapshot, target_tps: f64, wake: Option<WakeFn>) -> (Self, SharedSlot) {
        let slot = SnapshotSlot::with_initial(first);
        let now = Instant::now();
        let sim = Self {
            state,
            slot: SharedSlot::clone(&slot),
            wake,
            running: false,
            target_tps,
            uncapped: false,
            ticks_per_snapshot: 1,
            step_count: 0,
            tps_timer: now,
            actual_tps: 0.0,
            last_publish: now,
            serial: 0,
            layout_on: false,
            relax_paused: false,
            ticked: false,
            engine_ms: None,
            next_step_at: now,
            schedule: Schedule::default(),
            fired_through: None,
            run_to_target: None,
            run_to_stride: None,
            run_to_samples: Vec::new(),
        };
        (sim, slot)
    }

    /// Keeps publishing while paused, so the layout can keep relaxing if requested. Otherwise returns [`Pace::Idle`].
    fn relax_while_paused(&mut self) -> Pace {
        if !(self.layout_on && self.relax_paused) {
            return Pace::Idle;
        }
        let since = Instant::now().duration_since(self.last_publish);
        if since < PUBLISH_INTERVAL {
            return Pace::After(PUBLISH_INTERVAL.saturating_sub(since));
        }
        self.force_publish_snapshot();
        Pace::After(PUBLISH_INTERVAL)
    }

    /// Steps uncapped toward `target`, never past it, and pauses there with a final publish.
    fn advance_to_target(&mut self, target: u64) -> Pace {
        let remaining = target.saturating_sub(self.state.tick());
        let steps = u64::from(uncapped_steps_for(self.engine_ms, 1)).min(remaining);
        for _ in 0..steps {
            self.timed_step();
            let tick = self.state.tick();
            if self.run_to_stride.as_mut().is_some_and(|stride| stride.take_due(tick)) {
                // A model can compute its stats in `prepare_view`, so the sample prepares the view as a publish does.
                self.state.prepare_view();
                self.run_to_samples.push(StatSample::new(tick, self.state.stats()));
            }
        }
        if steps == remaining {
            self.run_to_target = None;
            self.actual_tps = 0.0;
            self.force_publish_snapshot();
            return self.relax_while_paused();
        }
        self.update_tps();
        if Instant::now().duration_since(self.last_publish) >= RUN_TO_PUBLISH_INTERVAL {
            self.force_publish_snapshot();
        }
        Pace::Now
    }

    fn batch_interval(&self) -> std::time::Duration {
        capped_interval(self.target_tps, self.ticks_per_snapshot)
    }

    /// Pulls the next deadline in to at most one batch interval from now. It never moves the deadline later.
    ///
    /// Re-anchoring it to now would let a slider drag fire a batch per event and outrun the cap.
    fn reclamp_deadline(&mut self) {
        let limit = Instant::now() + self.batch_interval();
        if self.next_step_at > limit {
            self.next_step_at = limit;
        }
    }

    /// Steps once, folds the step's cost into the smoothed engine time, then fires the actions due.
    ///
    /// The first sample is taken whole. Easing it in from zero would leave `uncapped_steps_for`
    /// reading far too fast, and a frame would be spent paying for that.
    fn timed_step(&mut self) {
        let t0 = Instant::now();
        self.state.step();
        self.step_count += 1;
        self.ticked = true;
        let sample = t0.elapsed().as_secs_f64() * 1000.0;
        // Exponential moving average with a weight of 0.1.
        self.engine_ms = Some(match self.engine_ms {
            Some(prev) => prev + 0.1 * (sample - prev),
            None => sample,
        });
        self.fire_due();
    }

    /// Fires the scheduled actions due at the state's current tick, and records that tick as fired.
    fn fire_due(&mut self) {
        self.fired_through = Some(self.state.tick());
        for refused in self.schedule.run_due(&mut *self.state) {
            log::warn!("Model rejected action '{}' at tick {}", refused.id, refused.tick);
        }
    }

    /// Starts the window `update_tps` divides by, and drops the steps it had counted.
    fn reset_tps_window(&mut self) {
        self.tps_timer = Instant::now();
        self.step_count = 0;
    }

    fn update_tps(&mut self) {
        let elapsed = self.tps_timer.elapsed().as_secs_f64();
        if elapsed >= 1.0 {
            self.actual_tps = self.step_count as f64 / elapsed;
            self.step_count = 0;
            self.tps_timer = Instant::now();
        }
    }

    fn maybe_publish_snapshot(&mut self) {
        let now = Instant::now();
        if now.duration_since(self.last_publish) < PUBLISH_INTERVAL {
            return;
        }
        self.last_publish = now;
        self.publish_snapshot();
    }

    fn force_publish_snapshot(&mut self) {
        self.last_publish = Instant::now();
        self.publish_snapshot();
    }

    /// Builds a snapshot and publishes it.
    ///
    /// The snapshot is built outside the lock. Otherwise the UI thread would block on `take_snapshot` for the whole
    /// grid copy.
    fn publish_snapshot(&mut self) {
        let spare = crate::runner::claim_spare(&self.slot);
        let engine_ms = self.engine_ms.unwrap_or(0.0);
        self.serial += 1;
        // Actions and setting changes also publish, and must not move the nodes of a paused network.
        let relax = self.layout_on && (self.ticked || self.relax_paused);
        self.ticked = false;
        let mut snap = build_snapshot(spare, &mut *self.state, self.actual_tps, engine_ms, self.serial, relax);
        snap.run_to_samples = std::mem::take(&mut self.run_to_samples);
        crate::runner::publish(&self.slot, snap);
        // After the lock, so waking the UI can never make it block on us.
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

/// Handle on a running CPU simulation.
///
/// The sim runs on its own thread on native targets. In a browser [`Self::update`] steps it from the host's
/// frame loop.
pub struct SimThread {
    driver: Driver<Loop>,
    slot: SharedSlot,
}

impl std::fmt::Debug for SimThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SimThread").finish_non_exhaustive()
    }
}

impl SimThread {
    /// Starts a paused runner over `state`, capped at `target_tps` ticks per second.
    ///
    /// `wake` is `None` only for a headless caller that polls on its own schedule.
    ///
    /// A panic out of the loop is stored in `faults`.
    pub fn new(mut state: Box<dyn SimState>, target_tps: f64, wake: Option<WakeFn>, faults: FaultSink) -> Self {
        // The first snapshot gives the UI something to draw before play is pressed.
        let first = build_snapshot(None, &mut *state, 0.0, 0.0, 0, false);
        let (sim, slot, on_fault) = Self::parts(state, first, target_tps, wake, faults);
        let driver = Driver::spawn(sim, on_fault);
        Self { driver, slot }
    }

    /// Starts a paused runner over `state` in `pool`, capped at `target_tps` ticks per second.
    ///
    /// The first snapshot, the loop's start, each pump and each command run on a worker of `pool`. A model step that
    /// never returns then holds workers of `pool` alone, and the global pool stays whole. `wake` and `faults` are as
    /// for [`Self::new`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new_in(
        pool: std::sync::Arc<rayon::ThreadPool>,
        mut state: Box<dyn SimState>,
        target_tps: f64,
        wake: Option<WakeFn>,
        faults: FaultSink,
    ) -> Self {
        let first = pool.install(|| build_snapshot(None, &mut *state, 0.0, 0.0, 0, false));
        let (sim, slot, on_fault) = Self::parts(state, first, target_tps, wake, faults);
        let driver = Driver::spawn_in(pool, sim, on_fault);
        Self { driver, slot }
    }

    /// Returns the loop over `state` from its `first` snapshot, the slot it publishes to and the handler that stores a
    /// panic out of the loop in `faults`.
    fn parts(
        state: Box<dyn SimState>,
        first: Snapshot,
        target_tps: f64,
        wake: Option<WakeFn>,
        faults: FaultSink,
    ) -> (Loop, SharedSlot, impl FnOnce(Fault) + WasmNotSend + 'static) {
        let (sim, slot) = Loop::new(state, first, target_tps, wake.clone());
        let on_fault = move |fault| {
            faults.set_once(fault);
            if let Some(wake) = &wake {
                wake();
            }
        };
        (sim, slot, on_fault)
    }

    /// Stops the loop, and joins its thread once it stops or detaches the thread after `timeout`.
    ///
    /// The loop stops between two pumps. A model step that never returns keeps the detached thread running, and
    /// [`RunnerShutdown::Detached`] returns its handle.
    pub fn stop(mut self, timeout: Duration) -> RunnerShutdown {
        self.driver.shutdown(SimCommand::Shutdown, timeout)
    }

    /// Sends a command to the loop.
    pub fn send(&mut self, cmd: SimCommand) {
        self.driver.send(cmd);
    }

    /// Takes the latest snapshot, or `None` when nothing new has been published since the last take.
    pub fn take_snapshot(&mut self) -> Option<Snapshot> {
        crate::runner::take_snapshot(&self.slot)
    }

    /// Stores a taken snapshot as the spare for the next publish to refill.
    ///
    /// This is only an optimisation. Dropping the snapshot instead only means the next publish allocates.
    pub fn recycle(&mut self, snap: Snapshot) {
        crate::runner::recycle(&self.slot, snap);
    }

    /// Sends [`SimCommand::Play`].
    pub fn play(&mut self) {
        self.send(SimCommand::Play);
    }

    /// Sends [`SimCommand::Pause`].
    pub fn pause(&mut self) {
        self.send(SimCommand::Pause);
    }

    /// Sends [`SimCommand::StepOnce`].
    pub fn step_once(&mut self) {
        self.send(SimCommand::StepOnce);
    }

    /// Sends [`SimCommand::SetSchedule`].
    pub fn set_schedule(&mut self, schedule: Schedule) {
        self.send(SimCommand::SetSchedule(schedule));
    }

    /// Sends [`SimCommand::RunTo`].
    pub fn run_to(&mut self, tick: u64) {
        self.send(SimCommand::RunTo(tick));
    }

    /// Advances the sim when the driver runs inside the frame loop, and does nothing when the driver has its own
    /// thread.
    pub fn update(&mut self, dt: f64) {
        self.driver.update(dt);
    }
}

/// Stops the loop as [`SimThread::stop`] does with [`DEFAULT_STOP_TIMEOUT`], and logs a detached thread.
impl Drop for SimThread {
    fn drop(&mut self) {
        if let RunnerShutdown::Detached(_) = self.driver.shutdown(SimCommand::Shutdown, DEFAULT_STOP_TIMEOUT) {
            log::warn!(
                "The simulation thread did not stop within {} s, and runs on detached",
                DEFAULT_STOP_TIMEOUT.as_secs_f64()
            );
        }
    }
}

fn refill<T: Copy>(dst: &mut Vec<T>, src: &[T]) {
    dst.clear();
    dst.extend_from_slice(src);
}

/// Builds a snapshot of `state`, refilling the buffers of `reuse`.
///
/// `reuse` comes back from the UI thread through `recycle`, and a publish is then a copy without a fresh
/// multi-megabyte allocation. Every view is consulted, so a composite model publishes its field and its agents.
pub(super) fn build_snapshot(
    reuse: Option<Snapshot>,
    state: &mut dyn SimState,
    actual_tps: f64,
    engine_ms: f64,
    serial: u64,
    relax: bool,
) -> Snapshot {
    // The model turns its state into something drawable here rather than every tick.
    let view_started = Instant::now();
    state.prepare_view();
    if relax {
        state.relax_layout();
    }
    let view_ms = view_started.elapsed().as_secs_f64() * 1000.0;
    // The recycled layers are destructured up front, so both layers can claim buffers without moving `recycled` twice.
    let recycled = match reuse.map(|s| s.view) {
        Some(SnapshotView::Cpu(layers)) => layers,
        _ => CpuLayers::default(),
    };
    let (mut cells, mut grid_palette) = match recycled.grid {
        Some(g) => (g.cells, g.palette),
        None => (Vec::new(), Vec::new()),
    };
    let mut spare_edges = recycled.edges;
    let (mut pos_x, mut pos_y, mut color, mut point_palette) = match recycled.points {
        Some(p) => (p.pos_x, p.pos_y, p.color, p.palette),
        None => (Vec::new(), Vec::new(), Vec::new(), Vec::new()),
    };

    let grid = state.grid_view().map(|gv| {
        refill(&mut cells, gv.cells);
        refill(&mut grid_palette, gv.palette);
        GridSnapshot {
            width: gv.width,
            height: gv.height,
            cells: std::mem::take(&mut cells),
            palette: std::mem::take(&mut grid_palette),
        }
    });

    let points = state.point_view().map(|pv| {
        refill(&mut pos_x, pv.pos_x);
        refill(&mut pos_y, pv.pos_y);
        refill(&mut color, pv.color.unwrap_or(&[]));
        refill(&mut point_palette, pv.palette);
        PointSnapshot {
            pos_x: std::mem::take(&mut pos_x),
            pos_y: std::mem::take(&mut pos_y),
            world_w: pv.world_w,
            world_h: pv.world_h,
            color: std::mem::take(&mut color),
            palette: std::mem::take(&mut point_palette),
        }
    });

    let edges = state.edge_view().map(|ev| {
        let mut snap = spare_edges.take().unwrap_or_default();
        // Copied only if the version or the length changed.
        if snap.version != ev.version || snap.src.len() != ev.src.len() {
            refill(&mut snap.src, ev.src);
            refill(&mut snap.dst, ev.dst);
            refill(&mut snap.color, ev.color.unwrap_or(&[]));
            snap.version = ev.version;
        }
        refill(&mut snap.palette, ev.palette);
        snap.directed = ev.directed;
        snap
    });

    let view = SnapshotView::Cpu(CpuLayers { grid, points, edges });

    Snapshot {
        tick: state.tick(),
        serial,
        population: state.population(),
        heap_bytes: state.heap_bytes(),
        actual_tps,
        engine_ms,
        view_ms,
        view,
        stats: state.stats(),
        fire_counts: FireCounts::default(),
        run_to_samples: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_UNCAPPED_STEPS, UNCAPPED_PUMP_MS, uncapped_steps_for};

    /// The frame is returned to the host between pumps, so a pump has to be worth a frame's work.
    #[test]
    fn an_uncapped_pump_fills_the_budget() {
        // A step costing a tenth of the budget earns ten steps.
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS / 10.0), 1), 10);
        // A step costing more than the budget still earns one step.
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS * 5.0), 1), 1);
    }

    /// A publish lands on a stride boundary, so what fits is rounded down to whole strides.
    #[test]
    fn an_uncapped_pump_runs_whole_snapshot_strides() {
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS / 10.0), 5), 10);
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS / 12.0), 5), 10);
    }

    /// The floor is one step. A whole stride would make a slow model with a large stride run a whole stride
    /// anyway, however long that took.
    #[test]
    fn a_stride_too_slow_for_the_budget_is_not_run_whole() {
        // Three steps fit, where a stride is a hundred.
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS / 3.0), 100), 3);
        // Not even one fits.
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS * 2.0), 100), 1);
    }

    /// Checks the pump size before any step is timed, and when a step measures as free.
    #[test]
    fn an_unmeasured_step_is_bounded() {
        assert_eq!(uncapped_steps_for(None, 1), 1);
        assert_eq!(uncapped_steps_for(None, 100), 1);
        assert_eq!(uncapped_steps_for(Some(0.0), 1), MAX_UNCAPPED_STEPS);
        assert!(uncapped_steps_for(Some(0.0), 100) <= MAX_UNCAPPED_STEPS);
    }
}
