use henad_core::model::SimState;
use henad_core::params::ParamValue;

use crate::snapshot::{GridSnapshot, PointSnapshot, Snapshot, SnapshotView};

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
    use std::time::Instant;

    use super::{SimCommand, Snapshot, build_snapshot};
    use henad_core::model::SimState;

    pub struct SimThread {
        cmd_tx: mpsc::Sender<SimCommand>,
        snapshot: Arc<Mutex<Option<Snapshot>>>,
        handle: Option<JoinHandle<()>>,
    }

    struct SimLoop {
        state: Box<dyn SimState>,
        cmd_rx: mpsc::Receiver<SimCommand>,
        snapshot: Arc<Mutex<Option<Snapshot>>>,
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
                    // Block until a command arrives
                    let Ok(cmd) = self.cmd_rx.recv() else {
                        return;
                    };
                    if self.handle_command(cmd) {
                        return;
                    }
                    continue;
                }

                if self.uncapped {
                    // Run a batch of steps, then check for commands
                    for _ in 0..self.ticks_per_snapshot {
                        self.timed_step();
                    }
                    self.update_tps();
                    self.maybe_publish_snapshot();
                    // Drain pending commands
                    while let Ok(cmd) = self.cmd_rx.try_recv() {
                        if self.handle_command(cmd) {
                            return;
                        }
                    }
                } else {
                    // Capped: wait until next deadline or command
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
                    // Drain any pending commands before stepping
                    while let Ok(cmd) = self.cmd_rx.try_recv() {
                        if self.handle_command(cmd) {
                            return;
                        }
                    }
                    if self.running {
                        let interval = self.batch_interval();
                        // Advance from the previous deadline, so the batch's own
                        // execution time doesn't stretch every period. Resync if sim is running behind.
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

        /// Returns true if the thread should exit.
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
                    // Publish one final snapshot so UI shows latest state
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

        fn publish_snapshot(&self) {
            let snap = build_snapshot(&*self.state, self.actual_tps, self.engine_ms);
            if let Ok(mut slot) = self.snapshot.lock() {
                *slot = Some(snap);
            }
        }
    }

    impl SimThread {
        /// Spawn a new sim thread with the given initial state and target TPS.
        pub fn new(state: Box<dyn SimState>, target_tps: f64) -> Self {
            let (cmd_tx, cmd_rx) = mpsc::channel();
            // Publish initial snapshot so UI has data before play is pressed.
            let initial = build_snapshot(&*state, 0.0, 0.0);
            let snapshot: Arc<Mutex<Option<Snapshot>>> = Arc::new(Mutex::new(Some(initial)));
            let snapshot_clone = Arc::clone(&snapshot);

            let sim_loop = SimLoop {
                state,
                cmd_rx,
                snapshot: snapshot_clone,
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

            let handle = std::thread::spawn(move || sim_loop.run());

            Self {
                cmd_tx,
                snapshot,
                handle: Some(handle),
            }
        }

        /// Send a command to the sim thread.
        pub fn send(&mut self, cmd: SimCommand) {
            drop(self.cmd_tx.send(cmd));
        }

        /// Take the latest snapshot (returns None if no new snapshot since last take).
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
    use super::{SimCommand, Snapshot, build_snapshot};
    use henad_core::model::SimState;

    /// Ceiling on catch-up batches per `update()`, so a backgrounded tab handing back a
    /// multi-second `dt` can't dump all of it into one frame. 1000 TPS at 60 fps owes ~17.
    const MAX_BATCHES_PER_FRAME: u32 = 64;

    pub struct SimThread {
        state: Box<dyn SimState>,
        running: bool,
        target_tps: f64,
        uncapped: bool,
        ticks_per_snapshot: u32,
        accumulated_time: f64,
        actual_tps: f64,
        snapshot: Option<Snapshot>,
    }

    impl SimThread {
        pub fn new(state: Box<dyn SimState>, target_tps: f64) -> Self {
            // Publish initial snapshot so UI has data before play is pressed.
            let initial = Some(build_snapshot(&*state, 0.0, 0.0));
            Self {
                state,
                running: false,
                target_tps,
                uncapped: false,
                ticks_per_snapshot: 1,
                accumulated_time: 0.0,
                actual_tps: 0.0,
                snapshot: initial,
            }
        }

        pub fn send(&mut self, cmd: SimCommand) {
            match cmd {
                SimCommand::Play => self.running = true,
                SimCommand::Pause => {
                    self.running = false;
                    self.accumulated_time = 0.0;
                    self.snapshot = Some(build_snapshot(&*self.state, self.actual_tps, 0.0));
                }
                SimCommand::StepOnce => {
                    self.state.step();
                    self.snapshot = Some(build_snapshot(&*self.state, self.actual_tps, 0.0));
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

            if self.uncapped {
                for _ in 0..self.ticks_per_snapshot {
                    self.state.step();
                }
            } else {
                // Same batch cadence as the native loop, so both backends run at `target_tps`.
                let batch_interval = super::capped_batch_interval_secs(self.target_tps, self.ticks_per_snapshot);
                self.accumulated_time += dt;
                let (batches, carry) =
                    super::batches_owed(self.accumulated_time, batch_interval, MAX_BATCHES_PER_FRAME);
                for _ in 0..batches {
                    for _ in 0..self.ticks_per_snapshot {
                        self.state.step();
                    }
                }
                self.accumulated_time = carry;
            }

            self.snapshot = Some(build_snapshot(&*self.state, self.actual_tps, 0.0));
        }
    }
}

fn build_snapshot(state: &dyn SimState, actual_tps: f64, engine_ms: f64) -> Snapshot {
    let view = if let Some(gv) = state.grid_view() {
        SnapshotView::Grid(GridSnapshot {
            width: gv.width,
            height: gv.height,
            cells: gv.cells.to_vec(),
            palette: gv.palette,
        })
    } else if let Some(pv) = state.point_view() {
        SnapshotView::Points(PointSnapshot {
            pos_x: pv.pos_x.to_vec(),
            pos_y: pv.pos_y.to_vec(),
            world_w: pv.world_w,
            world_h: pv.world_h,
            palette: pv.palette,
        })
    } else {
        SnapshotView::None
    };

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

    #[test]
    fn capped_batching_holds_the_target_rate() {
        let ticks = Arc::new(AtomicU64::new(0));
        let mut thread = SimThread::new(Box::new(Counter(Arc::clone(&ticks))), 50.0);
        thread.send(SimCommand::SetTicksPerSnapshot(10));
        thread.play();
        std::thread::sleep(std::time::Duration::from_millis(1000));
        thread.pause();

        let n = ticks.load(Ordering::Relaxed);
        assert!((20..=150).contains(&n), "ran {n} ticks in 1s at 50 TPS");
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
