use henad_core::model::SimState;
use henad_core::params::ParamValue;

use crate::snapshot::{GridSnapshot, PointSnapshot, Snapshot, SnapshotView};

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
                        let step_interval = std::time::Duration::from_secs_f64(1.0 / self.target_tps);
                        self.next_step_at = Instant::now() + step_interval;
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
                }
                SimCommand::SetUncapped(v) => {
                    self.uncapped = v;
                }
                SimCommand::SetTicksPerSnapshot(v) => {
                    self.ticks_per_snapshot = v.max(1);
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
                self.accumulated_time += dt;
                let step_interval = 1.0 / self.target_tps;
                let mut steps = 0u32;
                while self.accumulated_time >= step_interval {
                    self.state.step();
                    self.accumulated_time -= step_interval;
                    steps += 1;
                    if steps >= self.ticks_per_snapshot {
                        self.accumulated_time = 0.0;
                        break;
                    }
                }
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
