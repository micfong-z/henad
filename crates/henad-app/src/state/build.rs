//! Builds of the live simulation, and the teardown of the simulation before each. Neither blocks the UI thread.
//!
//! On native a build runs on its own thread, named `henad-model-build`. It builds the model and the runner that will
//! drive it, and the app takes the runner on a later frame. Commands sent while the build runs wait in its queue. A
//! browser cannot spawn a thread, and builds inline.
//!
//! A teardown stops the runner within [`STOP_WAIT`]. A runner whose pump has not returned by then, or a build
//! abandoned before it returned, is detached and kept until its thread ends. The next build waits on its own thread
//! until each detached thread has ended, or until [`DEFAULT_STOP_TIMEOUT`] has passed since the thread was stopped.
//! The old run's memory is then free before the new model allocates. A thread still running at the timeout retires the
//! pool it runs in (see [`ModelPool`]), and the build runs in a fresh pool.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::thread::JoinHandle;
use std::time::Duration;

use web_time::Instant;

use henad_compute::cpu::sim_thread::{SimCommand, SimThread, WakeFn};
use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::{BUILDING, Fault, FaultSink, catching};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::fault::catching_on;
use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
use henad_compute::runner::{DEFAULT_STOP_TIMEOUT, DetachedRunner, RunnerShutdown};
use henad_compute::simulation::RunSetup;
use henad_core::metadata::Backend;

use super::AppState;
use super::pool::ModelPool;
use crate::sim_runner::SimRunner;

/// Longest time a teardown waits on the UI thread for the runner to stop. A runner still running after it is detached.
///
/// A runner stops between two pumps, and a pump is one batch of steps.
pub const STOP_WAIT: Duration = Duration::from_millis(250);

/// Status shown while a build runs.
pub const BUILDING_MODEL: &str = "Building model…";

/// Status shown while a detached run or build has not stopped and is younger than [`DEFAULT_STOP_TIMEOUT`].
pub const STILL_STOPPING: &str = "Previous run is still stopping.";

/// Status shown once a detached run or build has run past [`DEFAULT_STOP_TIMEOUT`] without stopping.
pub const DID_NOT_STOP: &str = "Previous run did not stop. It will keep running until the app quits.";

/// Longest time between two repaints while a detached run or build is younger than [`DEFAULT_STOP_TIMEOUT`].
const STOPPING_REPAINT: Duration = Duration::from_millis(250);

/// Longest time between two repaints once every detached run or build is past [`DEFAULT_STOP_TIMEOUT`].
const STOPPED_REPAINT: Duration = Duration::from_secs(1);

/// Time between two looks of a waiting build at the threads stopped before it.
#[cfg(not(target_arch = "wasm32"))]
const SETTLE_POLL: Duration = Duration::from_millis(5);

// Phases of a build on its own thread, held in `PendingBuild::phase`. The build thread moves from one to the next by
// compare and swap, and the app swaps in `PHASE_CANCELLED` when it abandons the build.

/// The build waits for the threads stopped before it.
#[cfg(not(target_arch = "wasm32"))]
const PHASE_WAITING: u8 = 0;
/// The build builds the model and its runner.
const PHASE_BUILDING: u8 = 1;
/// The build has sent its result, or is about to.
const PHASE_DONE: u8 = 2;
/// The app abandoned the build. A build that sees it builds nothing, or drops what it built on its own thread.
const PHASE_CANCELLED: u8 = 3;

/// Runner a build returns, or the build's fault, with the generation of the pool the runner runs in.
struct Built {
    runner: Result<SimRunner, Fault>,
    generation: u64,
}

/// Build of the live simulation running on its own thread, and the commands sent to its runner meanwhile.
pub struct PendingBuild {
    /// Channel the build thread returns its result on.
    built: Receiver<Built>,
    thread: JoinHandle<()>,
    /// Phase of the build, one of [`PHASE_WAITING`], [`PHASE_BUILDING`], [`PHASE_DONE`] and [`PHASE_CANCELLED`].
    phase: Arc<AtomicU8>,
    /// Entry of the model being built. A fault of the build names it in the fault modal, and the Editor claims a
    /// fault of the code the Editor applied.
    model: ModelEntry,
    /// Commands sent since the build started, in order. The runner receives them when it arrives.
    queued: Vec<SimCommand>,
}

/// Thread of a run or a build that the app stopped and that did not stop in time.
enum Detached {
    Runner(DetachedRunner),
    Build(JoinHandle<()>),
}

/// Detached thread of a run or a build, with the time the app asked it to stop.
pub struct Stopping {
    detached: Detached,
    since: Instant,
    /// Generation of the pool the thread runs in.
    generation: u64,
}

impl Stopping {
    /// Returns whether the thread has ended.
    fn is_finished(&self) -> bool {
        match &self.detached {
            Detached::Runner(runner) => runner.is_finished(),
            Detached::Build(thread) => thread.is_finished(),
        }
    }

    /// Returns whether [`DEFAULT_STOP_TIMEOUT`] has passed at `now` since the app asked the thread to stop.
    fn is_late(&self, now: Instant) -> bool {
        now.duration_since(self.since) >= DEFAULT_STOP_TIMEOUT
    }
}

/// Everything a build reads from the app, taken when the build starts.
struct BuildJob {
    setup: RunSetup,
    gpu_ctx: Option<GpuContext>,
    /// Sink a CPU runner stores a panic out of its loop in.
    faults: FaultSink,
    wake: WakeFn,
    target_tps: f64,
    uncapped: bool,
    /// Layout settings for a network model, `None` for any other model.
    layout: Option<SimCommand>,
    gpu_settings: GpuBatchSettings,
    #[cfg(not(target_arch = "wasm32"))]
    pools: ModelPool,
    /// Threads the app stopped before the build and that had not ended when it started.
    #[cfg(not(target_arch = "wasm32"))]
    predecessors: Vec<Arc<Stopping>>,
    #[cfg(not(target_arch = "wasm32"))]
    phase: Arc<AtomicU8>,
}

impl BuildJob {
    /// Waits until each thread stopped before the build has ended, or has run past [`DEFAULT_STOP_TIMEOUT`] and
    /// retired its pool. Returns the generation of the pool to build in and the pool, `None` for rayon's global pool.
    ///
    /// Returns `None` when the app abandons the build meanwhile.
    #[cfg(not(target_arch = "wasm32"))]
    fn settle(&self) -> Option<(u64, Option<Arc<rayon::ThreadPool>>)> {
        loop {
            if self.phase.load(Ordering::SeqCst) == PHASE_CANCELLED {
                return None;
            }
            let now = Instant::now();
            let mut waiting = false;
            for stopping in &self.predecessors {
                if stopping.is_finished() {
                    continue;
                }
                if stopping.is_late(now) {
                    self.pools.retire(stopping.generation);
                } else {
                    waiting = true;
                }
            }
            if !waiting {
                break;
            }
            std::thread::sleep(SETTLE_POLL);
        }
        let current = self.pools.current();
        self.phase
            .compare_exchange(PHASE_WAITING, PHASE_BUILDING, Ordering::SeqCst, Ordering::SeqCst)
            .ok()?;
        Some(current)
    }

    /// Builds the model and the runner that will drive it, on a worker of `pool` when it is `Some`.
    ///
    /// # Errors
    ///
    /// Returns the fault when the build panics, the GPU rejects it, or this machine has no device for a GPU model.
    #[cfg(not(target_arch = "wasm32"))]
    fn run(self, pool: Option<Arc<rayon::ThreadPool>>) -> Result<SimRunner, Fault> {
        match pool {
            // Inside the pool, so a CPU model's init runs its parallel passes there.
            Some(pool) => {
                let runner_pool = Arc::clone(&pool);
                pool.install(move || self.run_here(Some(runner_pool)))
            }
            None => self.run_here(None),
        }
    }

    /// Builds the model and its runner on the calling thread. The runner runs in `pool`, or in rayon's global pool for
    /// `None`.
    ///
    /// A GPU build runs inside error scopes pushed on the calling thread, the thread that makes every device call of the
    /// build. Scopes are thread-local, and scopes on another thread would leave a device error to the device's
    /// handler.
    fn run_here(self, pool: Option<Arc<rayon::ThreadPool>>) -> Result<SimRunner, Fault> {
        let gpu_ctx = self.gpu_ctx.clone();
        match gpu_ctx {
            Some(ctx) if self.setup.entry().metadata().backend == Backend::Gpu => {
                catching_on(&ctx, BUILDING, || self.build(pool))?
            }
            _ => self.build(pool),
        }
    }

    /// Builds the model and its runner, as [`Self::run_here`] does inside its error scopes.
    fn build(self, pool: Option<Arc<rayon::ThreadPool>>) -> Result<SimRunner, Fault> {
        let Self {
            setup,
            gpu_ctx,
            faults,
            wake,
            target_tps,
            uncapped,
            layout,
            gpu_settings,
            ..
        } = self;
        match setup.entry().build(setup.values(), setup.seed(), gpu_ctx.as_ref())? {
            ModelState::Cpu(state) => catching(BUILDING, move || {
                #[cfg(not(target_arch = "wasm32"))]
                let mut thread = match pool {
                    Some(pool) => SimThread::new_in(pool, state, target_tps, Some(wake), faults),
                    None => SimThread::new(state, target_tps, Some(wake), faults),
                };
                #[cfg(target_arch = "wasm32")]
                let mut thread = {
                    drop(pool);
                    SimThread::new(state, target_tps, Some(wake), faults)
                };
                if uncapped {
                    thread.send(SimCommand::SetUncapped(true));
                }
                if let Some(layout) = layout {
                    thread.send(layout);
                }
                SimRunner::Cpu(thread)
            }),
            ModelState::Gpu(state) => {
                // Unreachable in practice. Without a context the Model panel hides every GPU entry, and an entry built
                // without one returns a fault.
                let Some(ctx) = gpu_ctx else {
                    return Err(Fault::refused(BUILDING, "no GPU context is available"));
                };
                // `run_here` holds the error scopes and the panic catch around the whole build.
                #[cfg(not(target_arch = "wasm32"))]
                let thread = match pool {
                    Some(pool) => GpuSimThread::new_in(pool, ctx, state, gpu_settings, Some(wake), faults),
                    None => GpuSimThread::new(ctx, state, gpu_settings, Some(wake), faults),
                };
                #[cfg(target_arch = "wasm32")]
                let thread = {
                    drop(pool);
                    GpuSimThread::new(ctx, state, gpu_settings, Some(wake), faults)
                };
                Ok(SimRunner::Gpu(thread))
            }
        }
    }
}

impl AppState {
    /// Builds `setup` and the runner that will drive it, on the build thread on native and inline in a browser.
    ///
    /// The runner becomes [`Self::sim_thread`] once [`Self::poll_build`] takes it. A build that fails goes to the fault
    /// modal, or to the Editor for a fault of the code the Editor applied.
    pub(super) fn start_build(&mut self, setup: RunSetup) {
        let model = setup.entry().clone();
        let layout = setup.entry().topology_hint().edges.then(|| self.layout_command());
        let job = BuildJob {
            setup,
            gpu_ctx: self.gpu_ctx.clone(),
            faults: self.sim_faults.clone(),
            wake: self.repaint_waker(),
            target_tps: self.target_tps,
            uncapped: self.uncapped,
            layout,
            gpu_settings: self.gpu_batch_settings(),
            #[cfg(not(target_arch = "wasm32"))]
            pools: self.pools.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            predecessors: self
                .stopping
                .iter()
                .filter(|stopping| !stopping.is_finished())
                .cloned()
                .collect(),
            #[cfg(not(target_arch = "wasm32"))]
            phase: Arc::new(AtomicU8::new(PHASE_WAITING)),
        };

        #[cfg(target_arch = "wasm32")]
        {
            // A browser detaches no thread, and its one pool is the global pool.
            let built = Built {
                runner: job.run_here(None),
                generation: self.pools.current().0,
            };
            self.finish_build(built, Vec::new(), &model);
        }

        #[cfg(not(target_arch = "wasm32"))]
        self.spawn_build(job, model);
    }

    /// Starts the build thread for `job`, the build of entry `model`.
    #[cfg(not(target_arch = "wasm32"))]
    fn spawn_build(&mut self, job: BuildJob, model: ModelEntry) {
        let (sender, built) = std::sync::mpsc::channel();
        let phase = Arc::clone(&job.phase);
        let wake = job.wake.clone();
        let thread_phase = Arc::clone(&phase);
        let spawned = std::thread::Builder::new()
            .name("henad-model-build".to_owned())
            .spawn(move || {
                let Some((generation, pool)) = job.settle() else {
                    return;
                };
                let runner = job.run(pool);
                // An abandoned build drops its runner here, off the UI thread.
                if thread_phase
                    .compare_exchange(PHASE_BUILDING, PHASE_DONE, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    drop(sender.send(Built { runner, generation }));
                    wake();
                }
            });
        match spawned {
            Ok(thread) => {
                self.building = Some(PendingBuild {
                    built,
                    thread,
                    phase,
                    model,
                    queued: Vec::new(),
                });
            }
            // No edit of the code fixes a thread that did not start, and the Editor never claims the fault.
            Err(error) => {
                let fault = Fault::refused(BUILDING, format!("the build thread did not start: {error}"));
                self.show_fault_in_modal(fault, Some(&model));
            }
        }
    }

    /// Takes the runner of a build that has finished, and sends it the commands sent meanwhile. A build that failed
    /// goes to the fault modal, or to the Editor for a fault of the code the Editor applied.
    ///
    /// Called once per frame. Does nothing while the build runs, and nothing in a browser, where builds run inline.
    pub fn poll_build(&mut self) {
        let Some(build) = &self.building else {
            return;
        };
        let built = match build.built.try_recv() {
            Ok(built) => built,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                // The build catches a panic of the model's code. A thread that ended without a result panicked
                // outside that catch, and the Editor never claims the fault.
                if let Some(build) = self.building.take() {
                    let fault = Fault::refused(BUILDING, "the build thread ended without a result");
                    self.show_fault_in_modal(fault, Some(&build.model));
                }
                return;
            }
        };
        if let Some(build) = self.building.take() {
            self.finish_build(built, build.queued, &build.model);
        }
    }

    /// Makes the runner of `built` the live simulation and sends it `queued` in order, or shows the fault of the build
    /// of entry `model`.
    ///
    /// A GPU runner takes the batch settings of the Pacing tab first, which can have changed during the build.
    fn finish_build(&mut self, built: Built, queued: Vec<SimCommand>, model: &ModelEntry) {
        match built.runner {
            Ok(mut runner) => {
                if let Some(gpu) = runner.as_gpu_mut() {
                    let settings = self.gpu_batch_settings();
                    gpu.set_adaptive(settings.adaptive);
                    gpu.set_batch_size(settings.batch_size);
                    gpu.set_target_ms(settings.target_ms);
                }
                for command in queued {
                    runner.send(command);
                }
                self.sim_thread = Some(runner);
                self.runner_generation = built.generation;
            }
            Err(fault) => self.show_fault(fault, Some(model)),
        }
    }

    /// Batch settings of the Pacing tab for a GPU runner.
    fn gpu_batch_settings(&self) -> GpuBatchSettings {
        GpuBatchSettings {
            adaptive: self.gpu_adaptive,
            batch_size: self.gpu_batch_size,
            target_ms: self.gpu_target_ms,
        }
    }

    /// Returns whether a build runs on its own thread. Always false in a browser, where builds run inline.
    pub fn is_building(&self) -> bool {
        self.building.is_some()
    }

    /// Returns whether a simulation runs or a build will deliver one.
    pub fn has_simulation(&self) -> bool {
        self.sim_thread.is_some() || self.is_building()
    }

    /// Sends `cmd` to the live simulation, or queues it for the runner of the build that runs. Does nothing without
    /// either.
    pub fn send_command(&mut self, cmd: SimCommand) {
        if let Some(thread) = &mut self.sim_thread {
            thread.send(cmd);
        } else if let Some(build) = &mut self.building {
            build.queued.push(cmd);
        }
    }

    /// Pool the app's CPU work runs in.
    pub fn model_pool(&self) -> &ModelPool {
        &self.pools
    }

    /// Stops the live simulation within [`STOP_WAIT`], and abandons a build that runs.
    ///
    /// A runner that does not stop in time and a build thread abandoned while it builds are detached, kept until they
    /// end, and counted by [`Self::stopping_status`]. A build still waiting for older threads ends without building.
    /// The sink of the runner's panics is replaced, so a detached runner's late panic reaches no later run.
    pub(super) fn stop_simulation(&mut self) {
        if let Some(build) = self.building.take() {
            match build.phase.swap(PHASE_CANCELLED, Ordering::SeqCst) {
                PHASE_BUILDING => {
                    log::warn!("Build of {} abandoned before it returned", build.model.name());
                    // The build took the current pool once every thread before it had ended or retired its pool, and
                    // no pool has been retired since.
                    let generation = self.pools.current().0;
                    self.stopping.push(Arc::new(Stopping {
                        detached: Detached::Build(build.thread),
                        since: Instant::now(),
                        generation,
                    }));
                }
                // Finished meanwhile. The runner is sent or about to be, and stops below like any other.
                PHASE_DONE => {
                    if let Ok(Built {
                        runner: Ok(runner),
                        generation,
                    }) = build.built.recv_timeout(STOP_WAIT)
                    {
                        self.sim_thread = Some(runner);
                        self.runner_generation = generation;
                    }
                }
                // Still waiting for the threads stopped before it. The build sees the cancel and ends without
                // building.
                _ => {}
            }
        }
        if let Some(runner) = self.sim_thread.take() {
            let since = Instant::now();
            if let RunnerShutdown::Detached(runner) = runner.stop(STOP_WAIT) {
                log::warn!("The simulation did not stop within {} ms", STOP_WAIT.as_millis());
                self.stopping.push(Arc::new(Stopping {
                    detached: Detached::Runner(runner),
                    since,
                    generation: self.runner_generation,
                }));
            }
        }
        self.sim_faults = FaultSink::new();
    }

    /// Drops the detached threads that have ended, and retires the pool of each that has run past
    /// [`DEFAULT_STOP_TIMEOUT`]. Repaints while any is left, so the status line follows them.
    pub fn poll_stopping(&mut self, ctx: &egui::Context) {
        self.stopping.retain(|stopping| !stopping.is_finished());
        if self.stopping.is_empty() {
            return;
        }
        let now = Instant::now();
        let mut young = false;
        for stopping in &self.stopping {
            if stopping.is_late(now) {
                self.pools.retire(stopping.generation);
            } else {
                young = true;
            }
        }
        ctx.request_repaint_after(if young { STOPPING_REPAINT } else { STOPPED_REPAINT });
    }

    /// Returns the status line of the detached runs and builds, or `None` when every one has ended.
    ///
    /// [`DID_NOT_STOP`] once any has run past [`DEFAULT_STOP_TIMEOUT`] since it was asked to stop, and
    /// [`STILL_STOPPING`] before.
    pub fn stopping_status(&self) -> Option<&'static str> {
        self.stopping_status_at(Instant::now())
    }

    /// Returns the status line of the detached runs and builds at time `now`.
    fn stopping_status_at(&self, now: Instant) -> Option<&'static str> {
        let mut running = self.stopping.iter().filter(|stopping| !stopping.is_finished());
        let first = running.next()?;
        Some(if first.is_late(now) || running.any(|stopping| stopping.is_late(now)) {
            DID_NOT_STOP
        } else {
            STILL_STOPPING
        })
    }
}

/// Stops the live simulation and abandons a build without waiting past [`STOP_WAIT`], as the app quits.
///
/// The detached threads are dropped with the app and end with the process. Otherwise the runner's own drop would wait
/// [`DEFAULT_STOP_TIMEOUT`].
impl Drop for AppState {
    fn drop(&mut self) {
        self.stop_simulation();
    }
}

#[cfg(test)]
impl AppState {
    /// Polls the build until it has finished, or panics after a minute.
    pub fn wait_for_build(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            self.poll_build();
            if !self.is_building() {
                return;
            }
            assert!(Instant::now() < deadline, "the build never finished");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Returns the status line of the detached runs and builds once [`DEFAULT_STOP_TIMEOUT`] has passed.
    pub fn stopping_status_after_the_timeout(&self) -> Option<&'static str> {
        self.stopping_status_at(Instant::now() + DEFAULT_STOP_TIMEOUT)
    }
}
