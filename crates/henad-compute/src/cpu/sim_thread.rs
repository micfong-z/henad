use crate::fault::FaultSink;
use henad_core::model::SimState;
use henad_core::params::ParamValue;

use crate::runner::{Driver, Pace, SharedSlot, SimLoop, SnapshotSlot};
use crate::snapshot::{CpuLayers, GridSnapshot, PointSnapshot, Snapshot, SnapshotView};
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
    SetParam { index: usize, value: ParamValue },
    Shutdown,
}

/// Publish cadence. Independent of how fast the sim is running.
const PUBLISH_INTERVAL_MS: u128 = 16;

/// Ceiling on a single uncapped batch, so a bad estimate cannot buy a long stall.
const MAX_UNCAPPED_BATCH: u32 = 4096;

/// Wall clock one uncapped batch aims to fill.
///
/// The threaded driver would happily run one step per pump. The frame driver hands the frame back
/// between pumps, and one step per frame pinned a fast model to the refresh rate. Matching the
/// driver's own budget keeps a frame to one batch.
const UNCAPPED_BATCH_MS: f64 = crate::runner::PUMP_BUDGET_MS;

/// Batches that fit [`UNCAPPED_BATCH_MS`], from the measured cost of a step.
///
/// `engine_ms` is `None` until a step has been timed, and one batch is enough to measure with.
fn uncapped_batch_for(engine_ms: Option<f64>, ticks_per_snapshot: u32) -> u32 {
    let Some(engine_ms) = engine_ms else {
        return 1;
    };
    let per_batch_ms = engine_ms * f64::from(ticks_per_snapshot.max(1));
    if per_batch_ms <= 0.0 {
        return MAX_UNCAPPED_BATCH;
    }
    let fits = (UNCAPPED_BATCH_MS / per_batch_ms).floor();
    fits.clamp(1.0, f64::from(MAX_UNCAPPED_BATCH)) as u32
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
    /// Smoothed engine time per tick (EMA). `None` until the first step has been timed.
    engine_ms: Option<f64>,
    /// When the next capped batch falls due.
    next_step_at: Instant,
}

impl SimLoop for Loop {
    type Command = SimCommand;

    fn handle_command(&mut self, cmd: SimCommand) -> bool {
        match cmd {
            SimCommand::Play => {
                self.running = true;
                self.tps_timer = Instant::now();
                self.step_count = 0;
                self.next_step_at = Instant::now();
            }
            SimCommand::Pause => {
                self.running = false;
                // Publish a final snapshot, so the UI shows the state it stopped at.
                self.force_publish_snapshot();
            }
            SimCommand::StepOnce => {
                self.timed_step();
                self.update_tps();
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
            SimCommand::Shutdown => return true,
        }
        false
    }

    fn pump(&mut self) -> Pace {
        if !self.running {
            return Pace::Idle;
        }
        if self.uncapped {
            let batches = uncapped_batch_for(self.engine_ms, self.ticks_per_snapshot);
            for _ in 0..u64::from(batches) * u64::from(self.ticks_per_snapshot) {
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
    /// The first sample is taken whole. Easing it in from zero would leave `uncapped_batch`
    /// reading far too fast, and a frame would be spent paying for that.
    fn timed_step(&mut self) {
        let t0 = Instant::now();
        self.state.step();
        self.step_count += 1;
        let sample = t0.elapsed().as_secs_f64() * 1000.0;
        // EMA with a = 0.1
        self.engine_ms = Some(match self.engine_ms {
            Some(prev) => prev + 0.1 * (sample - prev),
            None => sample,
        });
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
        if now.duration_since(self.last_publish).as_millis() < PUBLISH_INTERVAL_MS {
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
        let snap = build_snapshot(spare, &mut *self.state, self.actual_tps, engine_ms);
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
        let slot = SnapshotSlot::with_initial(build_snapshot(None, &mut *state, 0.0, 0.0));
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
            engine_ms: None,
            next_step_at: now,
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
fn build_snapshot(reuse: Option<Snapshot>, state: &mut dyn SimState, actual_tps: f64, engine_ms: f64) -> Snapshot {
    // The model turns its state into something drawable here rather than every tick.
    state.prepare_view();
    // Destructured up front so both layers can claim buffers without moving `recycled` twice.
    let recycled = match reuse.map(|s| s.view) {
        Some(SnapshotView::Cpu(layers)) => layers,
        _ => CpuLayers::default(),
    };
    let mut cells = recycled.grid.map(|g| g.cells).unwrap_or_default();
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

    let view = SnapshotView::Cpu(CpuLayers { grid, points });

    Snapshot {
        tick: state.tick(),
        population: state.population(),
        heap_bytes: state.heap_bytes(),
        actual_tps,
        engine_ms,
        view,
        stats: state.stats(),
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod pacing_timing_tests {
    use super::{SimCommand, SimThread};
    use crate::fault::{FaultSink, STEPPING};
    use henad_core::model::SimState;
    use henad_core::params::ParamValue;
    use henad_core::view::StatEntry;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

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

    /// A model author's bug, from the engine's point of view.
    struct Exploding(u64);

    impl SimState for Exploding {
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
            Box::new(Exploding(0)),
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
}

#[cfg(test)]
mod tests {
    use super::{MAX_UNCAPPED_BATCH, UNCAPPED_BATCH_MS, capped_batch_interval_secs, uncapped_batch_for};

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

    /// A frame is handed back between pumps, so a batch has to be worth a frame's work.
    #[test]
    fn an_uncapped_batch_fills_the_budget() {
        // A step costing a tenth of the budget earns ten of them.
        assert_eq!(uncapped_batch_for(Some(UNCAPPED_BATCH_MS / 10.0), 1), 10);
        // One costing more than the budget still earns one.
        assert_eq!(uncapped_batch_for(Some(UNCAPPED_BATCH_MS * 5.0), 1), 1);
    }

    /// `ticks_per_snapshot` steps run per batch, so the budget buys proportionally fewer batches.
    #[test]
    fn an_uncapped_batch_accounts_for_the_snapshot_stride() {
        assert_eq!(uncapped_batch_for(Some(UNCAPPED_BATCH_MS / 10.0), 5), 2);
    }

    /// Before anything has been timed, and where a step measures as free.
    #[test]
    fn an_unmeasured_step_is_bounded() {
        assert_eq!(uncapped_batch_for(None, 1), 1);
        assert_eq!(uncapped_batch_for(Some(0.0), 1), MAX_UNCAPPED_BATCH);
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::build_snapshot;
    use crate::snapshot::SnapshotView;
    use henad_core::model::SimState;
    use henad_core::params::ParamValue;
    use henad_core::view::{GridView, PointView, StatEntry};

    const PALETTE: &[[u8; 4]] = &[[1, 2, 3, 4], [5, 6, 7, 8]];

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
        let snap = build_snapshot(None, &mut state, 0.0, 0.0);
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
        let snap = build_snapshot(None, &mut state, 0.0, 0.0);
        let points = layers(&snap.view).points.as_ref().expect("agent layer was dropped");
        assert!(points.color.is_empty());
        assert_eq!(points.pos_x.len(), 2);
    }

    /// The colour lane has to recycle alongside the position lanes.
    #[test]
    fn recycling_reuses_the_color_lane_across_a_length_change() {
        let mut big = Composite::new(64, true);
        let first = build_snapshot(None, &mut big, 0.0, 0.0);
        let capacity = layers(&first.view)
            .points
            .as_ref()
            .map(|p| p.color.capacity())
            .unwrap_or_default();
        assert!(capacity >= 64);

        let mut small = Composite::new(5, true);
        let second = build_snapshot(Some(first), &mut small, 0.0, 0.0);
        let points = layers(&second.view).points.as_ref().expect("agent layer was dropped");
        assert_eq!(points.color, vec![0, 1, 0, 1, 0]);
        assert_eq!(points.color.capacity(), capacity, "the color lane reallocated");
        assert_eq!(points.pos_x.len(), 5);
    }
}
