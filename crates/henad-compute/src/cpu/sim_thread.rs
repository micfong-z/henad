use crate::fault::FaultSink;
use henad_core::action::Schedule;
use henad_core::model::SimState;
use henad_core::params::ParamValue;

use crate::runner::{Driver, Pace, RUN_TO_PUBLISH_INTERVAL, SharedSlot, SimLoop, SnapshotSlot};
use crate::snapshot::{CpuLayers, GridSnapshot, PointSnapshot, Snapshot, SnapshotView};
use std::time::Duration;
use web_time::Instant;

/// Wall-clock seconds between capped batches.
fn capped_batch_interval_secs(target_tps: f64, ticks_per_snapshot: u32) -> f64 {
    let tps = if target_tps.is_finite() && target_tps > 0.0 {
        target_tps
    } else {
        1.0
    };
    f64::from(ticks_per_snapshot.max(1)) / tps
}

/// Called on every publish, so an idle UI knows to come and collect the snapshot.
///
/// Without it a publish while the UI is idle, like a single step or the final one after pause,
/// sits unread until some unrelated input event wakes the event loop. Must not block.
#[cfg(not(all(target_arch = "wasm32", target_feature = "atomics")))]
pub type WakeFn = std::sync::Arc<dyn Fn() + Send + Sync>;

/// An `egui::Context` is not `Send` under atomics, and no thread waits on this one anyway.
#[cfg(all(target_arch = "wasm32", target_feature = "atomics"))]
pub type WakeFn = std::sync::Arc<dyn Fn()>;

/// Commands sent from the UI thread to the simulation thread.
pub enum SimCommand {
    Play,
    Pause,
    StepOnce,
    SetTargetTps(f64),
    SetUncapped(bool),
    SetTicksPerSnapshot(u32),
    SetParam {
        index: usize,
        value: ParamValue,
    },
    /// Run the model's declared action at this index, once.
    Act(usize),
    /// Turn the layout on or off, with a time budget per publish in milliseconds.
    ///
    /// The layout relaxes after every tick. While paused, it relaxes only if `while_paused` is set.
    SetLayout {
        on: bool,
        budget_ms: f32,
        while_paused: bool,
    },
    /// Replace the actions fired at their ticks, and fire those due at the current tick at once.
    ///
    /// Each later action fires once, after the step that reaches its tick.
    SetSchedule(Schedule),
    /// Step as fast as possible to this tick, then pause and publish.
    ///
    /// `Play`, `Pause` and `StepOnce` cancel it. A tick at or behind the current one pauses at once.
    RunTo(u64),
    Shutdown,
}

/// Publish cadence. Independent of how fast the sim is running.
const PUBLISH_INTERVAL: Duration = Duration::from_millis(16);

/// Ceiling on a single uncapped pump, so a bad estimate cannot buy a long stall.
const MAX_UNCAPPED_STEPS: u32 = 4096;

/// Wall clock one uncapped pump aims to fill.
///
/// The threaded driver would happily run one step per pump. The frame driver hands the frame back
/// between pumps, and one step per frame pinned a fast model to the refresh rate. Matching the
/// driver's own budget keeps a frame to one pump.
const UNCAPPED_PUMP_MS: f64 = crate::runner::PUMP_BUDGET_MS;

/// Steps that fit [`UNCAPPED_PUMP_MS`], from the measured cost of a step.
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
    /// Smoothed engine time per tick (EMA). `None` until the first step has been timed.
    engine_ms: Option<f64>,
    /// When the next capped batch falls due.
    next_step_at: Instant,
    /// Actions fired after the step that reaches their tick.
    schedule: Schedule,
    /// Tick a pending [`SimCommand::RunTo`] stops at.
    run_to_target: Option<u64>,
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
                self.fire_due();
                self.force_publish_snapshot();
            }
            SimCommand::RunTo(target) => {
                self.run_to_target = Some(target);
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
    /// Keeps publishing while paused, so the layout can keep relaxing if asked to. Otherwise returns [`Pace::Idle`].
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
        std::time::Duration::from_secs_f64(capped_batch_interval_secs(self.target_tps, self.ticks_per_snapshot))
    }

    /// Only ever moves the deadline earlier. Re-anchoring it to now would let a slider drag fire a
    /// batch per event and outrun the cap.
    fn reclamp_deadline(&mut self) {
        let limit = Instant::now() + self.batch_interval();
        if self.next_step_at > limit {
            self.next_step_at = limit;
        }
    }

    /// Step, and fold its cost into the smoothed engine time.
    ///
    /// The first sample is taken whole. Easing it in from zero would leave `uncapped_steps_for`
    /// reading far too fast, and a frame would be spent paying for that.
    fn timed_step(&mut self) {
        let t0 = Instant::now();
        self.state.step();
        self.step_count += 1;
        self.ticked = true;
        let sample = t0.elapsed().as_secs_f64() * 1000.0;
        // EMA with a = 0.1
        self.engine_ms = Some(match self.engine_ms {
            Some(prev) => prev + 0.1 * (sample - prev),
            None => sample,
        });
        self.fire_due();
    }

    /// Fires the scheduled actions due at the state's current tick.
    fn fire_due(&mut self) {
        for refused in self.schedule.run_due(&mut *self.state) {
            log::warn!("Model refused action '{}' at tick {}", refused.id, refused.tick);
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

    /// Built outside the lock, or the UI thread would block on `take_snapshot` for the whole grid
    /// copy.
    fn publish_snapshot(&mut self) {
        let spare = crate::runner::claim_spare(&self.slot);
        let engine_ms = self.engine_ms.unwrap_or(0.0);
        self.serial += 1;
        // Actions and setting changes also publish, and must not move the nodes of a paused network.
        let relax = self.layout_on && (self.ticked || self.relax_paused);
        self.ticked = false;
        let snap = build_snapshot(spare, &mut *self.state, self.actual_tps, engine_ms, self.serial, relax);
        crate::runner::publish(&self.slot, snap);
        // After the lock, so waking the UI can never make it block on us.
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

/// Handle on a running simulation. The sim itself is off the UI thread wherever the platform has
/// somewhere to put it.
pub struct SimThread {
    driver: Driver<Loop>,
    slot: SharedSlot,
}

impl SimThread {
    /// `wake` is `None` only for a headless caller that polls on its own schedule.
    ///
    /// A panic out of the loop lands in `faults`. The GPU sibling has no such parameter and reads
    /// the same sink off its `GpuContext`.
    pub fn new(mut state: Box<dyn SimState>, target_tps: f64, wake: Option<WakeFn>, faults: FaultSink) -> Self {
        // So the UI has something to draw before play is pressed.
        let slot = SnapshotSlot::with_initial(build_snapshot(None, &mut *state, 0.0, 0.0, 0, false));
        let now = Instant::now();
        let sim = Loop {
            state,
            slot: SharedSlot::clone(&slot),
            wake: wake.clone(),
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
            run_to_target: None,
        };

        let driver = Driver::spawn(sim, move |fault| {
            faults.set_once(fault);
            if let Some(wake) = &wake {
                wake();
            }
        });

        Self { driver, slot }
    }

    pub fn send(&mut self, cmd: SimCommand) {
        self.driver.send(cmd);
    }

    /// `None` when nothing new has been published since the last take.
    pub fn take_snapshot(&mut self) -> Option<Snapshot> {
        crate::runner::take_snapshot(&self.slot)
    }

    /// Purely an optimisation, dropping it instead just means the next publish allocates.
    pub fn recycle(&mut self, snap: Snapshot) {
        crate::runner::recycle(&self.slot, snap);
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

    /// Advances the sim where the driver has no thread of its own. A no-op where it has.
    pub fn update(&mut self, dt: f64) {
        self.driver.update(dt);
    }
}

impl Drop for SimThread {
    fn drop(&mut self) {
        self.driver.shutdown(SimCommand::Shutdown);
    }
}

fn refill<T: Copy>(dst: &mut Vec<T>, src: &[T]) {
    dst.clear();
    dst.extend_from_slice(src);
}

/// Refills `reuse`'s buffers, so a publish is a copy and not also a fresh multi-megabyte
/// allocation. `reuse` comes back from the UI thread via `recycle`.
///
/// Both views are consulted, so a composite model publishes its field and its agents.
fn build_snapshot(
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
    // Destructured up front so both layers can claim buffers without moving `recycled` twice.
    let recycled = match reuse.map(|s| s.view) {
        Some(SnapshotView::Cpu(layers)) => layers,
        _ => CpuLayers::default(),
    };
    let mut cells = recycled.grid.map(|g| g.cells).unwrap_or_default();
    let mut spare_edges = recycled.edges;
    let (mut pos_x, mut pos_y, mut color) = match recycled.points {
        Some(p) => (p.pos_x, p.pos_y, p.color),
        None => (Vec::new(), Vec::new(), Vec::new()),
    };

    let grid = state.grid_view().map(|gv| {
        refill(&mut cells, gv.cells);
        GridSnapshot {
            width: gv.width,
            height: gv.height,
            cells: std::mem::take(&mut cells),
            palette: gv.palette,
        }
    });

    let points = state.point_view().map(|pv| {
        refill(&mut pos_x, pv.pos_x);
        refill(&mut pos_y, pv.pos_y);
        refill(&mut color, pv.color.unwrap_or(&[]));
        PointSnapshot {
            pos_x: std::mem::take(&mut pos_x),
            pos_y: std::mem::take(&mut pos_y),
            world_w: pv.world_w,
            world_h: pv.world_h,
            color: std::mem::take(&mut color),
            palette: pv.palette,
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
        snap.palette = ev.palette;
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
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod pacing_timing_tests {
    use super::{SimCommand, SimThread};
    use crate::fault::{FaultSink, STEPPING};
    use crate::snapshot::Snapshot;
    use henad_core::action::{Schedule, Scheduled};
    use henad_core::model::SimState;
    use henad_core::params::ParamValue;
    use henad_core::view::StatEntry;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex};

    struct Counter(Arc<AtomicU64>);

    impl SimState for Counter {
        fn step(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn tick(&self) -> u64 {
            self.0.load(Ordering::Relaxed)
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

    /// Counts steps as [`Counter`] does, and records the tick each action runs at.
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
        fn act(&mut self, _index: usize) -> bool {
            self.fired.lock().expect("action log").push(self.tick());
            true
        }
        fn population(&self) -> u64 {
            0
        }
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    /// Records every layout switch that the loop passes on to the state.
    struct LayoutSwitches(Arc<Mutex<Vec<bool>>>);

    impl SimState for LayoutSwitches {
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
        fn set_layout(&mut self, on: bool, _budget_ms: f32) -> bool {
            self.0.lock().expect("switch log").push(on);
            true
        }
        fn population(&self) -> u64 {
            0
        }
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    /// Counts how many times the layout relaxes.
    struct Relaxes(Arc<AtomicU64>);

    impl SimState for Relaxes {
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
        fn set_layout(&mut self, _on: bool, _budget_ms: f32) -> bool {
            true
        }
        fn relax_layout(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
        fn population(&self) -> u64 {
            0
        }
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    /// A model author's bug, from the engine's point of view.
    struct DivideByZero(u64);

    impl SimState for DivideByZero {
        fn step(&mut self) {
            let zero: u64 = std::hint::black_box(0);
            self.0 = 1 / zero;
        }
        fn tick(&self) -> u64 {
            self.0
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

    /// A panicking kernel used to take the thread with it and leave the UI polling a viewport
    /// that never updated again. The panic still prints. This test is noisy by design.
    #[test]
    fn a_panicking_step_lands_in_the_sink_instead_of_killing_the_thread() {
        let faults = FaultSink::new();
        let wakes = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&wakes);

        let mut thread = SimThread::new(
            Box::new(DivideByZero(0)),
            1000.0,
            Some(Arc::new(move || {
                counter.fetch_add(1, Ordering::Relaxed);
            })),
            faults.clone(),
        );
        thread.play();

        for _ in 0..200 {
            if faults.is_set() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let fault = faults.take().expect("the panic should have reached the sink");
        assert_eq!(fault.during, STEPPING);
        assert!(fault.to_string().contains("divide by zero"), "{fault}");
        // Without the wake the UI would sit idle and never come and look.
        assert!(wakes.load(Ordering::Relaxed) > 0, "the UI was never woken");
    }

    #[test]
    fn capped_batching_holds_the_target_rate() {
        let ticks = Arc::new(AtomicU64::new(0));
        let mut thread = SimThread::new(Box::new(Counter(Arc::clone(&ticks))), 50.0, None, FaultSink::new());
        thread.send(SimCommand::SetTicksPerSnapshot(10));
        thread.play();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        thread.pause();

        let n = ticks.load(Ordering::Relaxed);
        assert!((20..=150).contains(&n), "ran {n} ticks in 1s at 50 TPS");
    }

    /// Blocks until `wakes` reaches `want`, or gives up.
    fn wait_for_wakes(wakes: &Arc<AtomicU64>, want: u64) -> u64 {
        for _ in 0..200 {
            let seen = wakes.load(Ordering::Relaxed);
            if seen >= want {
                return seen;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        wakes.load(Ordering::Relaxed)
    }

    /// Long enough that nothing else can publish. Both callers have already stopped the loop.
    fn settle() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    /// Pause used to leave the last running rate on screen, where the GPU runner already reported
    /// zero, and a step after a long pause was divided by the whole pause.
    #[test]
    fn a_pause_and_a_step_after_it_report_no_rate() {
        let ticks = Arc::new(AtomicU64::new(0));
        let mut thread = SimThread::new(Box::new(Counter(Arc::clone(&ticks))), 1000.0, None, FaultSink::new());

        thread.play();
        // A TPS window is a second wide, and nothing is reported until one closes.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        let running = thread.take_snapshot().expect("a running sim publishes");
        assert!(
            running.actual_tps > 0.0,
            "the window never closed, so the test proves nothing"
        );

        thread.pause();
        settle();
        let paused = thread.take_snapshot().expect("pause publishes a final snapshot");
        assert_eq!(paused.actual_tps, 0.0, "a paused sim reported a rate");

        // The stale window a single step used to be divided by.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        thread.step_once();
        settle();
        let stepped = thread.take_snapshot().expect("a step publishes");
        assert_eq!(stepped.actual_tps, 0.0, "one step over a long pause reported a rate");
    }

    /// Switching the layout off must reach the state.
    /// Otherwise the next publish would still run the layout, and a paused network would keep moving whenever an
    /// action or a step publishes.
    #[test]
    fn switching_the_layout_off_reaches_the_state() {
        let switches = Arc::new(Mutex::new(Vec::new()));
        let mut thread = SimThread::new(
            Box::new(LayoutSwitches(Arc::clone(&switches))),
            50.0,
            None,
            FaultSink::new(),
        );
        thread.send(SimCommand::SetLayout {
            on: true,
            budget_ms: 1.0,
            while_paused: false,
        });
        thread.send(SimCommand::SetLayout {
            on: false,
            budget_ms: 1.0,
            while_paused: false,
        });
        settle();
        assert_eq!(*switches.lock().expect("switch log"), [true, false]);
    }

    /// While paused, the layout moves on a step, and otherwise only when asked to relax while paused.
    #[test]
    fn a_paused_layout_relaxes_on_a_step_or_when_asked() {
        let relaxes = Arc::new(AtomicU64::new(0));
        let count = || relaxes.load(Ordering::Relaxed);
        let layout = |while_paused| SimCommand::SetLayout {
            on: true,
            budget_ms: 1.0,
            while_paused,
        };
        let mut thread = SimThread::new(Box::new(Relaxes(Arc::clone(&relaxes))), 50.0, None, FaultSink::new());

        thread.send(layout(false));
        settle();
        assert_eq!(count(), 0, "a paused publish relaxed");

        thread.step_once();
        settle();
        assert_eq!(count(), 1, "a step relaxes once and no more");

        thread.send(layout(true));
        settle();
        assert!(count() >= 3, "relaxing while paused stopped at {}", count());

        thread.send(layout(false));
        settle();
        let stopped = count();
        settle();
        assert_eq!(count(), stopped, "the layout kept relaxing once told to stop");
    }

    /// A run to a tick ends paused, and a paused layout relaxes after it as it does after Pause.
    #[test]
    fn a_paused_layout_keeps_relaxing_after_a_run_to() {
        let relaxes = Arc::new(AtomicU64::new(0));
        let count = || relaxes.load(Ordering::Relaxed);
        let mut thread = SimThread::new(Box::new(Relaxes(Arc::clone(&relaxes))), 50.0, None, FaultSink::new());

        thread.send(SimCommand::SetLayout {
            on: true,
            budget_ms: 1.0,
            while_paused: true,
        });
        settle();
        thread.run_to(0);
        settle();
        let reached = count();
        settle();
        assert!(count() > reached, "the layout stopped relaxing at the run's target");
    }

    /// A snapshot nobody is told about is a snapshot nobody draws. Stepping used to only refresh
    /// the viewport once you moved the mouse.
    #[test]
    fn a_publish_while_paused_wakes_the_ui() {
        let ticks = Arc::new(AtomicU64::new(0));
        let wakes = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&wakes);

        let mut thread = SimThread::new(
            Box::new(Counter(Arc::clone(&ticks))),
            50.0,
            Some(Arc::new(move || {
                counter.fetch_add(1, Ordering::Relaxed);
            })),
            FaultSink::new(),
        );

        thread.step_once();
        assert!(
            wait_for_wakes(&wakes, 1) >= 1,
            "a single step published without waking the UI"
        );

        // Pause force-publishes a final snapshot too.
        let before = wakes.load(Ordering::Relaxed);
        thread.pause();
        assert!(
            wait_for_wakes(&wakes, before + 1) > before,
            "pausing published a final snapshot without waking the UI"
        );
    }

    /// Takes snapshots until one reports `tick`, or gives up after five seconds.
    fn snapshot_at(thread: &mut SimThread, tick: u64) -> Option<Snapshot> {
        for _ in 0..500 {
            if let Some(snap) = thread.take_snapshot()
                && snap.tick == tick
            {
                return Some(snap);
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        None
    }

    /// Returns a schedule of one action at each of `ticks`, in the order given.
    fn schedule_at(ticks: &[u64]) -> Schedule {
        let entries = ticks
            .iter()
            .map(|&tick| Scheduled {
                index: 0,
                id: "mark".to_owned(),
                tick,
            })
            .collect();
        Schedule::from_entries(entries)
    }

    /// Returns a thread over an [`ActionRecorder`] state capped at 1000 TPS, its tick and its action log.
    fn action_recorder() -> (SimThread, Arc<AtomicU64>, Arc<Mutex<Vec<u64>>>) {
        let ticks = Arc::new(AtomicU64::new(0));
        let fired = Arc::new(Mutex::new(Vec::new()));
        let state = ActionRecorder {
            ticks: Arc::clone(&ticks),
            fired: Arc::clone(&fired),
        };
        let thread = SimThread::new(Box::new(state), 1000.0, None, FaultSink::new());
        (thread, ticks, fired)
    }

    fn fired_ticks(fired: &Arc<Mutex<Vec<u64>>>) -> Vec<u64> {
        fired.lock().expect("action log").clone()
    }

    #[test]
    fn run_to_stops_at_the_target_and_pauses() {
        let ticks = Arc::new(AtomicU64::new(0));
        // Capped at 1 TPS, so only an uncapped run reaches the target in time.
        let mut thread = SimThread::new(Box::new(Counter(Arc::clone(&ticks))), 1.0, None, FaultSink::new());
        thread.run_to(20_000);
        let reached = snapshot_at(&mut thread, 20_000).expect("the run never published its target");
        assert_eq!(reached.actual_tps, 0.0, "a paused run reported a rate");
        settle();
        assert_eq!(ticks.load(Ordering::Relaxed), 20_000, "the run went past its target");
        assert!(thread.take_snapshot().is_none(), "a paused run kept publishing");

        thread.run_to(10);
        assert!(
            snapshot_at(&mut thread, 20_000).is_some(),
            "a run to a tick behind the current one pauses and publishes"
        );
        settle();
        assert_eq!(ticks.load(Ordering::Relaxed), 20_000);
    }

    #[test]
    fn a_schedule_fires_once_per_tick_after_the_step_reaching_it() {
        let (mut thread, ticks, fired) = action_recorder();
        thread.set_schedule(schedule_at(&[3, 1, 3, 9, 10, 25, 60]));
        thread.run_to(10);
        assert!(snapshot_at(&mut thread, 10).is_some());
        assert_eq!(
            fired_ticks(&fired),
            [1, 3, 3, 9, 10],
            "the step reaching the target fires its actions"
        );

        thread.step_once();
        thread.step_once();
        assert!(snapshot_at(&mut thread, 12).is_some());
        assert_eq!(
            fired_ticks(&fired),
            [1, 3, 3, 9, 10],
            "the target's actions fired twice"
        );

        thread.play();
        for _ in 0..500 {
            if ticks.load(Ordering::Relaxed) >= 30 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        thread.pause();
        settle();
        let end = ticks.load(Ordering::Relaxed).max(80);
        thread.run_to(end);
        assert!(snapshot_at(&mut thread, end).is_some());
        assert_eq!(fired_ticks(&fired), [1, 3, 3, 9, 10, 25, 60]);
    }

    #[test]
    fn set_schedule_fires_what_is_due_now() {
        let (mut thread, ticks, fired) = action_recorder();
        assert!(
            thread.take_snapshot().is_some(),
            "a new thread publishes its first state"
        );
        thread.set_schedule(schedule_at(&[0, 1, 0]));
        assert!(snapshot_at(&mut thread, 0).is_some(), "setting a schedule publishes");
        assert_eq!(
            fired_ticks(&fired),
            [0, 0],
            "both actions due at tick 0 fire before any step"
        );
        assert_eq!(ticks.load(Ordering::Relaxed), 0);

        thread.run_to(5);
        assert!(snapshot_at(&mut thread, 5).is_some());
        thread.set_schedule(schedule_at(&[5, 6]));
        assert!(snapshot_at(&mut thread, 5).is_some());
        assert_eq!(
            fired_ticks(&fired),
            [0, 0, 1, 5],
            "a replaced schedule fires the reached tick at once"
        );

        thread.step_once();
        assert!(snapshot_at(&mut thread, 6).is_some());
        assert_eq!(fired_ticks(&fired), [0, 0, 1, 5, 6]);
    }

    #[test]
    fn play_cancels_a_run_to() {
        let ticks = Arc::new(AtomicU64::new(0));
        let mut thread = SimThread::new(Box::new(Counter(Arc::clone(&ticks))), 20.0, None, FaultSink::new());
        thread.run_to(u64::MAX);
        std::thread::sleep(std::time::Duration::from_millis(100));
        thread.play();
        settle();
        let resumed = ticks.load(Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(500));
        let played = ticks.load(Ordering::Relaxed) - resumed;
        thread.pause();
        assert!(
            resumed > 1000,
            "the run to a tick stepped {resumed} ticks, and never ran uncapped"
        );
        assert!((1..=30).contains(&played), "played {played} ticks in 0.5 s at 20 TPS");
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_UNCAPPED_STEPS, UNCAPPED_PUMP_MS, capped_batch_interval_secs, uncapped_steps_for};

    /// The regression. Batching used to multiply the tick rate by the batch size.
    #[test]
    fn batching_does_not_change_effective_tick_rate() {
        for &tps in &[1.0, 30.0, 250.0, 1000.0] {
            for &batch in &[1, 2, 10, 137, 1000] {
                let interval = capped_batch_interval_secs(tps, batch);
                let effective = f64::from(batch) / interval;
                assert!(
                    (effective - tps).abs() < 1e-9,
                    "tps {tps}, batch {batch}: effective {effective}"
                );
            }
        }
    }

    #[test]
    fn interval_is_batch_size_over_tps() {
        assert!((capped_batch_interval_secs(30.0, 10) - 1.0 / 3.0).abs() < 1e-12);
        assert!((capped_batch_interval_secs(60.0, 1) - 1.0 / 60.0).abs() < 1e-12);
    }

    /// Guards against a `Duration::from_secs_f64` panic on a degenerate target rate.
    #[test]
    fn non_positive_tps_yields_a_finite_interval() {
        for &tps in &[0.0, -5.0, f64::NAN, f64::INFINITY] {
            let secs = capped_batch_interval_secs(tps, 4);
            assert!(secs.is_finite() && secs > 0.0, "tps {tps} gave {secs}");
            assert!(std::time::Duration::from_secs_f64(secs) > std::time::Duration::ZERO);
        }
    }

    #[test]
    fn zero_ticks_per_snapshot_is_treated_as_one() {
        assert!((capped_batch_interval_secs(50.0, 0) - capped_batch_interval_secs(50.0, 1)).abs() < 1e-12);
    }

    /// A frame is handed back between pumps, so a pump has to be worth a frame's work.
    #[test]
    fn an_uncapped_pump_fills_the_budget() {
        // A step costing a tenth of the budget earns ten of them.
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS / 10.0), 1), 10);
        // One costing more than the budget still earns one.
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS * 5.0), 1), 1);
    }

    /// A publish lands on a stride boundary, so what fits is rounded down to whole strides.
    #[test]
    fn an_uncapped_pump_runs_whole_snapshot_strides() {
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS / 10.0), 5), 10);
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS / 12.0), 5), 10);
    }

    /// The regression. The floor used to be a whole stride, so a slow model with a large stride ran
    /// one anyway and spent however long that took.
    #[test]
    fn a_stride_too_slow_for_the_budget_is_not_run_whole() {
        // Three steps fit, where a stride is a hundred.
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS / 3.0), 100), 3);
        // Not even one fits.
        assert_eq!(uncapped_steps_for(Some(UNCAPPED_PUMP_MS * 2.0), 100), 1);
    }

    /// Before anything has been timed, and where a step measures as free.
    #[test]
    fn an_unmeasured_step_is_bounded() {
        assert_eq!(uncapped_steps_for(None, 1), 1);
        assert_eq!(uncapped_steps_for(None, 100), 1);
        assert_eq!(uncapped_steps_for(Some(0.0), 1), MAX_UNCAPPED_STEPS);
        assert!(uncapped_steps_for(Some(0.0), 100) <= MAX_UNCAPPED_STEPS);
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::build_snapshot;
    use crate::snapshot::SnapshotView;
    use henad_core::model::SimState;
    use henad_core::params::ParamValue;
    use henad_core::view::{EdgeView, GridView, PointView, StatEntry};

    const PALETTE: &[[u8; 4]] = &[[1, 2, 3, 4], [5, 6, 7, 8]];

    /// A model with nodes and edges.
    struct Graph {
        pos: Vec<f32>,
        src: Vec<u32>,
        dst: Vec<u32>,
        color: Vec<u8>,
        version: u64,
    }

    impl Graph {
        fn new(edges: usize, version: u64) -> Self {
            Self {
                pos: vec![0.0; 8],
                src: (0..edges as u32).collect(),
                dst: (0..edges as u32).map(|i| i + 1).collect(),
                color: vec![0; edges],
                version,
            }
        }
    }

    impl SimState for Graph {
        fn step(&mut self) {}
        fn tick(&self) -> u64 {
            0
        }
        fn point_view(&self) -> Option<PointView<'_>> {
            Some(PointView {
                pos_x: &self.pos,
                pos_y: &self.pos,
                world_w: 1.0,
                world_h: 1.0,
                color: None,
                palette: PALETTE,
            })
        }
        fn edge_view(&self) -> Option<EdgeView<'_>> {
            Some(EdgeView {
                src: &self.src,
                dst: &self.dst,
                color: Some(&self.color),
                palette: PALETTE,
                directed: false,
                version: self.version,
            })
        }
        fn stats(&self) -> Vec<StatEntry> {
            Vec::new()
        }
        fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
            false
        }
        fn population(&self) -> u64 {
            self.pos.len() as u64
        }
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    /// A model with a field and agents, the shape `build_snapshot` used to collapse.
    struct Composite {
        cells: Vec<u8>,
        pos_x: Vec<f32>,
        pos_y: Vec<f32>,
        color: Vec<u8>,
        with_color: bool,
    }

    impl Composite {
        fn new(agents: usize, with_color: bool) -> Self {
            Self {
                cells: vec![1; 12],
                pos_x: (0..agents).map(|i| i as f32).collect(),
                pos_y: (0..agents).map(|i| i as f32 * 2.0).collect(),
                color: (0..agents).map(|i| (i % 2) as u8).collect(),
                with_color,
            }
        }
    }

    impl SimState for Composite {
        fn step(&mut self) {}
        fn tick(&self) -> u64 {
            0
        }
        fn grid_view(&self) -> Option<GridView<'_>> {
            Some(GridView {
                width: 4,
                height: 3,
                cells: &self.cells,
                palette: PALETTE,
            })
        }
        fn point_view(&self) -> Option<PointView<'_>> {
            Some(PointView {
                pos_x: &self.pos_x,
                pos_y: &self.pos_y,
                world_w: 4.0,
                world_h: 3.0,
                color: self.with_color.then_some(&self.color),
                palette: PALETTE,
            })
        }
        fn stats(&self) -> Vec<StatEntry> {
            Vec::new()
        }
        fn set_param(&mut self, _index: usize, _value: &ParamValue) -> bool {
            false
        }
        fn population(&self) -> u64 {
            self.pos_x.len() as u64
        }
        fn heap_bytes(&self) -> usize {
            0
        }
    }

    fn layers(view: &SnapshotView) -> &crate::snapshot::CpuLayers {
        match view {
            SnapshotView::Cpu(l) => l,
            SnapshotView::Gpu(_) => panic!("expected a CPU snapshot"),
        }
    }

    /// The regression. Publishing used to reach `point_view` only when there was no grid, so a
    /// composite model silently dropped every agent.
    #[test]
    fn a_composite_model_publishes_both_layers() {
        let mut state = Composite::new(3, true);
        let snap = build_snapshot(None, &mut state, 0.0, 0.0, 0, false);
        let layers = layers(&snap.view);

        let grid = layers.grid.as_ref().expect("field layer was dropped");
        assert_eq!((grid.width, grid.height), (4, 3));
        assert_eq!(grid.cells.len(), 12);

        let points = layers.points.as_ref().expect("agent layer was dropped");
        assert_eq!(points.pos_x, vec![0.0, 1.0, 2.0]);
        assert_eq!(points.pos_y, vec![0.0, 2.0, 4.0]);
        assert_eq!(points.color, vec![0, 1, 0]);
    }

    /// An absent lane must arrive empty, which is what the renderer reads as uniform.
    #[test]
    fn a_model_without_a_color_lane_publishes_an_empty_one() {
        let mut state = Composite::new(2, false);
        let snap = build_snapshot(None, &mut state, 0.0, 0.0, 0, false);
        let points = layers(&snap.view).points.as_ref().expect("agent layer was dropped");
        assert!(points.color.is_empty());
        assert_eq!(points.pos_x.len(), 2);
    }

    /// The colour lane has to recycle alongside the position lanes.
    #[test]
    fn recycling_reuses_the_color_lane_across_a_length_change() {
        let mut big = Composite::new(64, true);
        let first = build_snapshot(None, &mut big, 0.0, 0.0, 0, false);
        let capacity = layers(&first.view)
            .points
            .as_ref()
            .map(|p| p.color.capacity())
            .unwrap_or_default();
        assert!(capacity >= 64);

        let mut small = Composite::new(5, true);
        let second = build_snapshot(Some(first), &mut small, 0.0, 0.0, 0, false);
        let points = layers(&second.view).points.as_ref().expect("agent layer was dropped");
        assert_eq!(points.color, vec![0, 1, 0, 1, 0]);
        assert_eq!(points.color.capacity(), capacity, "the color lane reallocated");
        assert_eq!(points.pos_x.len(), 5);
    }

    #[test]
    fn an_unchanged_edge_list_is_handed_back_untouched() {
        let mut model = Graph::new(500, 7);
        let first = build_snapshot(None, &mut model, 0.0, 0.0, 1, false);
        let ptr = layers(&first.view).edges.as_ref().expect("edges").src.as_ptr();

        // The version is unchanged, so nothing is copied.
        model.src[0] = 999;
        let second = build_snapshot(Some(first), &mut model, 0.0, 0.0, 2, false);
        let edges = layers(&second.view).edges.as_ref().expect("edges");
        assert_eq!(edges.src.as_ptr(), ptr, "the edge list reallocated");
        assert_eq!(edges.src[0], 0, "an unchanged version was copied anyway");
    }

    #[test]
    fn a_changed_edge_list_is_refilled_into_the_same_room() {
        let mut model = Graph::new(500, 7);
        let first = build_snapshot(None, &mut model, 0.0, 0.0, 1, false);
        let capacity = layers(&first.view).edges.as_ref().expect("edges").src.capacity();

        model.version = 8;
        model.src[0] = 999;
        let second = build_snapshot(Some(first), &mut model, 0.0, 0.0, 2, false);
        let edges = layers(&second.view).edges.as_ref().expect("edges");
        assert_eq!(edges.src[0], 999, "the change did not reach the snapshot");
        assert_eq!(edges.version, 8);
        assert_eq!(edges.src.capacity(), capacity, "the edge list reallocated");
    }

    #[test]
    fn an_edge_list_that_changed_length_is_refilled() {
        let mut model = Graph::new(500, 7);
        let first = build_snapshot(None, &mut model, 0.0, 0.0, 1, false);

        let mut shorter = Graph::new(3, 7);
        let second = build_snapshot(Some(first), &mut shorter, 0.0, 0.0, 2, false);
        let edges = layers(&second.view).edges.as_ref().expect("edges");
        assert_eq!(edges.src.len(), 3, "a shorter list was passed through whole");
    }

    #[test]
    fn a_model_without_edges_publishes_none() {
        let mut graph = Graph::new(4, 1);
        let first = build_snapshot(None, &mut graph, 0.0, 0.0, 1, false);
        assert!(layers(&first.view).edges.is_some());

        let mut plain = Composite::new(4, true);
        let second = build_snapshot(Some(first), &mut plain, 0.0, 0.0, 2, false);
        assert!(
            layers(&second.view).edges.is_none(),
            "the edge layer outlived its model"
        );
    }

    #[test]
    fn the_serial_is_whatever_the_publish_was_given() {
        let mut model = Graph::new(2, 1);
        assert_eq!(build_snapshot(None, &mut model, 0.0, 0.0, 41, false).serial, 41);
        assert_eq!(build_snapshot(None, &mut model, 0.0, 0.0, 42, false).serial, 42);
    }
}
