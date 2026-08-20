use henad_core::model::SimState;
use henad_core::params::ParamValue;

use crate::snapshot::{CpuLayers, GridSnapshot, PointSnapshot, Snapshot, SnapshotView};

/// Wall-clock seconds between capped batches.
fn capped_batch_interval_secs(target_tps: f64, ticks_per_snapshot: u32) -> f64 {
    let tps = if target_tps.is_finite() && target_tps > 0.0 {
        target_tps
    } else {
        1.0
    };
    f64::from(ticks_per_snapshot.max(1)) / tps
}

/// Whole batches owed by `accumulated` seconds, and the debt to carry forward.
///
/// Past `max_batches` the carried debt is clamped to one interval and the rest dropped, so a stall
/// can't bank debt that gets repaid as a burst. Same tolerance as the native resync.
#[cfg(any(target_arch = "wasm32", test))]
fn batches_owed(accumulated: f64, batch_interval: f64, max_batches: u32) -> (u32, f64) {
    let accumulated = if accumulated.is_finite() {
        accumulated.max(0.0)
    } else {
        0.0
    };
    if accumulated < batch_interval {
        return (0, accumulated);
    }
    let owed_exact = (accumulated / batch_interval).floor();
    let owed = owed_exact.min(f64::from(max_batches)) as u32;
    let carry = accumulated - f64::from(owed) * batch_interval;
    if owed_exact > f64::from(max_batches) {
        (owed, carry.min(batch_interval))
    } else {
        (owed, carry)
    }
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

// =====================================================================
// Native: threaded implementation
// =====================================================================
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread::JoinHandle;
    use web_time::Instant;

    use super::{SimCommand, Snapshot, WakeFn, build_snapshot};
    use crate::fault::{FaultSink, STEPPING, catching};
    use henad_core::model::SimState;

    /// The handoff point between the two threads. `fresh` is the newest publish waiting to be
    /// picked up, `spare` is a consumed one the UI handed back for its buffers.
    #[derive(Default)]
    struct SnapshotSlot {
        fresh: Option<Snapshot>,
        spare: Option<Snapshot>,
    }

    pub struct SimThread {
        cmd_tx: mpsc::Sender<SimCommand>,
        snapshot: Arc<Mutex<SnapshotSlot>>,
        handle: Option<JoinHandle<()>>,
    }

    struct SimLoop {
        state: Box<dyn SimState>,
        cmd_rx: mpsc::Receiver<SimCommand>,
        snapshot: Arc<Mutex<SnapshotSlot>>,
        wake: Option<WakeFn>,
        running: bool,
        target_tps: f64,
        uncapped: bool,
        ticks_per_snapshot: u32,
        step_count: u64,
        tps_timer: Instant,
        actual_tps: f64,
        last_publish: Instant,
        /// Smoothed engine time per tick (EMA).
        engine_ms: f64,
        /// When the next capped step should fire.
        next_step_at: Instant,
    }

    impl SimLoop {
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

                if self.uncapped {
                    for _ in 0..self.ticks_per_snapshot {
                        self.timed_step();
                    }
                    self.update_tps();
                    self.maybe_publish_snapshot();
                    while let Ok(cmd) = self.cmd_rx.try_recv() {
                        if self.handle_command(cmd) {
                            return;
                        }
                    }
                } else {
                    let now = Instant::now();
                    if now < self.next_step_at {
                        let wait = self.next_step_at - now;
                        match self.cmd_rx.recv_timeout(wait) {
                            Ok(cmd) => {
                                if self.handle_command(cmd) {
                                    return;
                                }
                                continue;
                            }
                            Err(mpsc::RecvTimeoutError::Timeout) => {}
                            Err(mpsc::RecvTimeoutError::Disconnected) => return,
                        }
                    }
                    while let Ok(cmd) = self.cmd_rx.try_recv() {
                        if self.handle_command(cmd) {
                            return;
                        }
                    }
                    if self.running {
                        let interval = self.batch_interval();
                        // Advance from the previous deadline, so the batch's own execution time
                        // doesn't stretch every period. Resync if the sim is running behind.
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
                    }
                }
            }
        }

        /// True when the thread should exit.
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

        fn batch_interval(&self) -> std::time::Duration {
            std::time::Duration::from_secs_f64(super::capped_batch_interval_secs(
                self.target_tps,
                self.ticks_per_snapshot,
            ))
        }

        /// Only ever moves the deadline earlier. Re-anchoring it to now would let a slider drag
        /// fire a batch per event and outrun the cap.
        fn reclamp_deadline(&mut self) {
            let limit = Instant::now() + self.batch_interval();
            if self.next_step_at > limit {
                self.next_step_at = limit;
            }
        }

        /// Step + measure engine time (EMA-smoothed).
        fn timed_step(&mut self) {
            let t0 = Instant::now();
            self.state.step();
            self.step_count += 1;
            let sample = t0.elapsed().as_secs_f64() * 1000.0;
            // EMA with α = 0.1
            self.engine_ms += 0.1 * (sample - self.engine_ms);
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
            if now.duration_since(self.last_publish).as_millis() < 16 {
                return;
            }
            self.last_publish = now;
            self.publish_snapshot();
        }

        fn force_publish_snapshot(&mut self) {
            self.last_publish = Instant::now();
            self.publish_snapshot();
        }

        /// Built outside the lock, or the UI thread would block on `take_snapshot` for the whole
        /// grid copy.
        fn publish_snapshot(&mut self) {
            let spare = self.snapshot.lock().ok().and_then(|mut slot| slot.spare.take());
            let snap = build_snapshot(spare, &mut *self.state, self.actual_tps, self.engine_ms);
            if let Ok(mut slot) = self.snapshot.lock() {
                // A `fresh` the UI never picked up is stale, so it becomes the next spare.
                slot.spare = slot.fresh.replace(snap);
            }
            // After the lock, so waking the UI can never make it block on us.
            if let Some(wake) = &self.wake {
                wake();
            }
        }
    }

    impl SimThread {
        /// `wake` is `None` only for a headless caller that polls on its own schedule.
        ///
        /// A panic out of the loop lands in `faults`. The GPU sibling has no such parameter and
        /// reads the same sink off its `GpuContext`.
        pub fn new(mut state: Box<dyn SimState>, target_tps: f64, wake: Option<WakeFn>, faults: FaultSink) -> Self {
            let (cmd_tx, cmd_rx) = mpsc::channel();
            // So the UI has something to draw before play is pressed.
            let snapshot = Arc::new(Mutex::new(SnapshotSlot {
                fresh: Some(build_snapshot(None, &mut *state, 0.0, 0.0)),
                spare: None,
            }));
            let snapshot_clone = Arc::clone(&snapshot);

            let on_fault = wake.clone();
            let sim_loop = SimLoop {
                state,
                cmd_rx,
                snapshot: snapshot_clone,
                wake,
                running: false,
                target_tps,
                uncapped: false,
                ticks_per_snapshot: 1,
                step_count: 0,
                tps_timer: Instant::now(),
                actual_tps: 0.0,
                last_publish: Instant::now(),
                engine_ms: 0.0,
                next_step_at: Instant::now(),
            };

            // Outside the loop. A catch per tick would sit in the hot path.
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
                handle: Some(handle),
            }
        }

        pub fn send(&mut self, cmd: SimCommand) {
            drop(self.cmd_tx.send(cmd));
        }

        /// `None` when nothing new has been published since the last take.
        pub fn take_snapshot(&mut self) -> Option<Snapshot> {
            self.snapshot.lock().ok()?.fresh.take()
        }

        /// Purely an optimisation, dropping it instead just means the next publish allocates.
        pub fn recycle(&mut self, snap: Snapshot) {
            if let Ok(mut slot) = self.snapshot.lock() {
                slot.spare = Some(snap);
            }
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
    }

    impl Drop for SimThread {
        fn drop(&mut self) {
            drop(self.cmd_tx.send(SimCommand::Shutdown));
            if let Some(h) = self.handle.take() {
                drop(h.join());
            }
        }
    }
}

// =====================================================================
// WASM: synchronous fallback
// =====================================================================
#[cfg(target_arch = "wasm32")]
mod wasm {
    use web_time::Instant;

    use super::{SimCommand, Snapshot, WakeFn, build_snapshot};
    use crate::fault::FaultSink;
    use henad_core::model::SimState;

    /// Ceiling on catch-up batches per `update()`, so a backgrounded tab handing back a
    /// multi-second `dt` can't dump all of it into one frame. 1000 TPS at 60 fps owes ~17.
    const MAX_BATCHES_PER_FRAME: u32 = 64;

    /// Wall clock one frame may spend stepping when uncapped.
    ///
    /// The browser wants the rest of the frame to paint in. Native runs uncapped on a thread of
    /// its own and just spins.
    const UNCAPPED_BUDGET_MS: f64 = 6.0;

    /// Ceiling on uncapped batches per frame. [`UNCAPPED_BUDGET_MS`] is the real limit and this
    /// only bounds what a bad estimate can do.
    const MAX_UNCAPPED_BATCHES_PER_FRAME: u32 = 4096;

    pub struct SimThread {
        state: Box<dyn SimState>,
        running: bool,
        target_tps: f64,
        uncapped: bool,
        ticks_per_snapshot: u32,
        accumulated_time: f64,
        actual_tps: f64,
        /// Steps since the TPS window opened.
        step_count: u64,
        tps_timer: Instant,
        /// Smoothed engine time per tick (EMA). `None` until the first step has been timed.
        engine_ms: Option<f64>,
        snapshot: Option<Snapshot>,
        /// Handed back by the UI so a republish refills its buffers instead of allocating.
        spare: Option<Snapshot>,
        wake: Option<WakeFn>,
    }

    impl SimThread {
        /// `wake` still matters with no thread to wake from, since `send` runs inside `ui()`, after
        /// the frame's snapshot poll in `logic()`.
        ///
        /// `faults` matches the native runner's signature and is never written to. Wasm panics
        /// abort without unwinding, leaving nothing here to catch.
        pub fn new(mut state: Box<dyn SimState>, target_tps: f64, wake: Option<WakeFn>, faults: FaultSink) -> Self {
            drop(faults);
            // So the UI has something to draw before play is pressed.
            let initial = Some(build_snapshot(None, &mut *state, 0.0, 0.0));
            Self {
                state,
                running: false,
                target_tps,
                uncapped: false,
                ticks_per_snapshot: 1,
                accumulated_time: 0.0,
                actual_tps: 0.0,
                step_count: 0,
                tps_timer: Instant::now(),
                engine_ms: None,
                snapshot: initial,
                spare: None,
                wake,
            }
        }

        /// An unclaimed `snapshot` is stale by definition, so it is the first buffer to reuse.
        fn republish(&mut self) {
            let reuse = self.snapshot.take().or_else(|| self.spare.take());
            let engine_ms = self.engine_ms.unwrap_or(0.0);
            self.snapshot = Some(build_snapshot(reuse, &mut *self.state, self.actual_tps, engine_ms));
            if let Some(wake) = &self.wake {
                wake();
            }
        }

        pub fn send(&mut self, cmd: SimCommand) {
            match cmd {
                SimCommand::Play => {
                    self.running = true;
                    self.tps_timer = Instant::now();
                    self.step_count = 0;
                }
                SimCommand::Pause => {
                    self.running = false;
                    self.accumulated_time = 0.0;
                    self.republish();
                }
                SimCommand::StepOnce => {
                    self.run_steps(1);
                    self.republish();
                }
                SimCommand::SetTargetTps(tps) => self.target_tps = tps,
                SimCommand::SetUncapped(v) => {
                    self.uncapped = v;
                    if !v {
                        self.accumulated_time = 0.0;
                    }
                }
                SimCommand::SetTicksPerSnapshot(v) => self.ticks_per_snapshot = v.max(1),
                SimCommand::SetParam { index, value } => {
                    self.state.set_param(index, &value);
                }
                SimCommand::Shutdown => {}
            }
        }

        pub fn take_snapshot(&mut self) -> Option<Snapshot> {
            self.snapshot.take()
        }

        /// Hands a consumed snapshot back for the next republish to refill.
        pub fn recycle(&mut self, snap: Snapshot) {
            self.spare = Some(snap);
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

        /// Called from `eframe::App::update()` on WASM. Runs steps synchronously.
        pub fn update(&mut self, dt: f64) {
            if !self.running {
                return;
            }

            let batches = if self.uncapped {
                self.uncapped_batches()
            } else {
                // Same batch cadence as the native loop, so both backends run at `target_tps`.
                let batch_interval = super::capped_batch_interval_secs(self.target_tps, self.ticks_per_snapshot);
                self.accumulated_time += dt;
                let (batches, carry) =
                    super::batches_owed(self.accumulated_time, batch_interval, MAX_BATCHES_PER_FRAME);
                self.accumulated_time = carry;
                batches
            };

            self.run_steps(u64::from(batches) * u64::from(self.ticks_per_snapshot));
            self.update_tps();

            // Nothing advanced, so the last publish is still current. Rebuilding it would re-copy
            // the grid and re-run `stats()` for no new data.
            if batches > 0 {
                self.republish();
            }
        }

        /// Batches that fit [`UNCAPPED_BUDGET_MS`], from the measured cost of a step.
        ///
        /// One batch per frame was the old answer, which pinned any model faster than the display
        /// to the refresh rate.
        fn uncapped_batches(&self) -> u32 {
            let Some(engine_ms) = self.engine_ms else {
                return 1;
            };
            let per_batch_ms = engine_ms * f64::from(self.ticks_per_snapshot.max(1));
            if per_batch_ms <= 0.0 {
                return MAX_UNCAPPED_BATCHES_PER_FRAME;
            }
            let fits = (UNCAPPED_BUDGET_MS / per_batch_ms).floor();
            fits.clamp(1.0, f64::from(MAX_UNCAPPED_BATCHES_PER_FRAME)) as u32
        }

        /// Steps `count` times, timing the run as a whole.
        ///
        /// One clock read per call. `Instant::now` is `performance.now` here, and at a thousand
        /// steps a second a per-step read would show up in the measurement itself.
        fn run_steps(&mut self, count: u64) {
            if count == 0 {
                return;
            }
            let t0 = Instant::now();
            for _ in 0..count {
                self.state.step();
            }
            let per_step = t0.elapsed().as_secs_f64() * 1000.0 / count as f64;
            // EMA with a = 0.1, as the native loop uses. The first sample is taken whole. Easing
            // it in from zero would leave `uncapped_batches` reading ten times too fast, and a
            // frame would be spent paying for that.
            self.engine_ms = Some(match self.engine_ms {
                Some(prev) => prev + 0.1 * (per_step - prev),
                None => per_step,
            });
            self.step_count += count;
        }

        fn update_tps(&mut self) {
            let elapsed = self.tps_timer.elapsed().as_secs_f64();
            if elapsed >= 1.0 {
                self.actual_tps = self.step_count as f64 / elapsed;
                self.step_count = 0;
                self.tps_timer = Instant::now();
            }
        }
    }
}

/// Overwrites `dst` with `src`, keeping `dst`'s allocation when it is already large enough.
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

#[cfg(not(target_arch = "wasm32"))]
pub use native::SimThread;
#[cfg(target_arch = "wasm32")]
pub use wasm::SimThread;

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
    use super::{batches_owed, capped_batch_interval_secs};

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
    fn backlog_below_one_interval_runs_nothing_and_is_carried() {
        let (n, carry) = batches_owed(0.007, 0.01, 64);
        assert_eq!(n, 0);
        assert!((carry - 0.007).abs() < 1e-12);
    }

    #[test]
    fn backlog_runs_whole_batches_and_carries_the_remainder() {
        let (n, carry) = batches_owed(0.035, 0.01, 64);
        assert_eq!(n, 3);
        assert!((carry - 0.005).abs() < 1e-9);
    }

    /// A backgrounded tab resumes rather than replaying its whole absence.
    #[test]
    fn backlog_past_the_ceiling_is_dropped_to_one_interval() {
        let (n, carry) = batches_owed(30.0, 0.01, 64);
        assert_eq!(n, 64);
        assert!((carry - 0.01).abs() < 1e-12, "carry {carry}");
    }

    /// Hitting the ceiling exactly is not behind, so nothing is discarded.
    #[test]
    fn backlog_exactly_at_the_ceiling_keeps_its_remainder() {
        let (n, carry) = batches_owed(0.645, 0.01, 64);
        assert_eq!(n, 64);
        assert!((carry - 0.005).abs() < 1e-9, "carry {carry}");
    }

    #[test]
    fn backlog_ignores_degenerate_accumulated_time() {
        for &acc in &[f64::NAN, f64::INFINITY, -1.0] {
            let (n, carry) = batches_owed(acc, 0.01, 64);
            assert_eq!(n, 0, "acc {acc}");
            assert!(carry.is_finite() && carry >= 0.0, "acc {acc} -> {carry}");
        }
    }

    #[test]
    fn zero_ticks_per_snapshot_is_treated_as_one() {
        assert!((capped_batch_interval_secs(50.0, 0) - capped_batch_interval_secs(50.0, 1)).abs() < 1e-12);
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
