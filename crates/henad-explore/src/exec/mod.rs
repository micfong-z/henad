//! Executors for batches of runs, with each outcome committed to a sink in request order.
//!
//! A CPU model runs in lanes. Each lane drives one run at a time on a thread pool of its own, and a single lane
//! drives its runs on rayon's global pool. A GPU model runs on tracks, several runs sharing the device from the
//! calling thread.

#[cfg(not(target_arch = "wasm32"))]
mod cpu;
#[cfg(not(target_arch = "wasm32"))]
mod gpu;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::num::{NonZeroUsize, ParseIntError};
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use web_time::Instant;

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::{Demand, GpuContext, MAX_STEPS_PER_SUBMISSION};
use henad_compute::runner::CAN_SPAWN_THREADS;
use henad_core::action::Schedule;
use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::outcome::{PlannedRun, RunOutcome};
use henad_core::explore::plan::Plan;
use henad_core::metadata::Backend;
use henad_core::params::ParamValue;

use crate::cursor::{CursorState, RunCursor};
use crate::probe::ProbeReport;

/// Jobs of one step for each thread of a lane.
pub const JOBS_PER_THREAD: usize = 4;

/// Population counted as one job when a model does not report its jobs.
pub const POPULATION_PER_JOB: u64 = 4096;

/// Wall time in milliseconds one slice of steps aims to take. A pause or an abort lands within about this long.
const SLICE_TARGET_MS: f64 = 20.0;

/// Most steps one slice can take.
const MAX_SLICE_STEPS: u64 = 1 << 20;

/// Most GPU tracks `choose_layout` picks on its own.
pub(crate) const MAX_AUTO_GPU_TRACKS: usize = 4;

/// Population from which `choose_layout` gives a GPU run the device to itself.
pub(crate) const LARGE_GPU_POPULATION: u64 = 1 << 20;

/// Number of runs a sweep keeps going at once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Concurrency {
    /// Chosen from a probe build by `choose_layout`.
    #[default]
    Auto,
    /// This many CPU lanes, or GPU tracks for a GPU model.
    Fixed(NonZeroUsize),
}

impl fmt::Display for Concurrency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Fixed(count) => write!(f, "{count}"),
        }
    }
}

impl FromStr for Concurrency {
    type Err = ParseIntError;

    /// Reads `auto`, or a count of at least 1.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        if raw == "auto" {
            Ok(Self::Auto)
        } else {
            raw.parse().map(Self::Fixed)
        }
    }
}

/// Machine resources a sweep can spread its runs over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ExecutionBudget {
    /// Worker threads the runs share.
    pub(crate) workers: usize,
    /// Bytes of host memory the live runs can hold together, `None` for no limit. The lanes are sized from the probed
    /// run, and a probed run larger than the budget leaves one lane.
    pub(crate) memory_budget: Option<u64>,
    /// Bytes of device memory the live GPU runs can hold together, `None` for no limit. A run larger than the budget
    /// runs alone.
    pub(crate) gpu_memory_budget: Option<u64>,
    /// Whether lanes can run on threads of their own.
    pub(crate) can_spawn_threads: bool,
}

impl ExecutionBudget {
    /// Returns the width of rayon's global pool as the workers, with no memory budget.
    pub(crate) fn detect() -> Self {
        Self {
            workers: rayon::current_num_threads(),
            memory_budget: None,
            gpu_memory_budget: None,
            can_spawn_threads: CAN_SPAWN_THREADS,
        }
    }
}

/// Returns the bytes of device memory GPU runs can hold together on `ctx`: `gpu_memory` when given, and otherwise the
/// device's largest buffer.
///
/// Note that wgpu reports no total for the device's memory. The largest buffer stands in for it.
pub fn gpu_memory_budget(gpu_memory: Option<u64>, ctx: &GpuContext) -> u64 {
    gpu_memory.unwrap_or_else(|| ctx.device.limits().max_buffer_size)
}

/// Lanes and GPU tracks a sweep runs on.
///
/// A CPU model has no GPU tracks, and a GPU model has no lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionLayout {
    /// Runs stepped at once on the CPU, each in its own lane.
    pub cpu_lanes: usize,
    /// Worker threads of each lane.
    pub threads_per_lane: usize,
    /// GPU runs alive at once.
    pub gpu_tracks: usize,
}

impl ExecutionLayout {
    /// Returns the bytes the live runs hold together, each as large as the probed run.
    ///
    /// A CPU run counts its host bytes, and a GPU run its device bytes.
    pub fn projected_bytes(&self, probe: &ProbeReport) -> u64 {
        let device_bytes = probe.demand.as_ref().map_or(0, |demand| demand.bytes());
        self.cpu_lanes as u64 * probe.heap_bytes + self.gpu_tracks as u64 * device_bytes
    }
}

/// Returns the layout for `runs` runs of a model on `backend`, sized from one probe build.
///
/// A CPU lane gets one thread per [`JOBS_PER_THREAD`] jobs of a step, up to every worker, and the workers are
/// split into lanes of that width. A model that reports no jobs counts one per [`POPULATION_PER_JOB`] of its
/// population. [`Concurrency::Fixed`] sets the lane count instead and splits the workers evenly.
///
/// Either way the lanes are capped by `runs` and by the memory budget, and a single lane takes every worker. One lane
/// runs even when the probe holds more than the budget. A target that cannot spawn threads gets one lane, whatever
/// `concurrency` asks.
///
/// A GPU model gets as many tracks as the GPU memory budget holds runs like the probe, up to
/// [`MAX_AUTO_GPU_TRACKS`], and one track from a population of [`LARGE_GPU_POPULATION`]. [`Concurrency::Fixed`] sets
/// the track count instead. The tracks are capped by `runs`.
pub(crate) fn choose_layout(
    concurrency: Concurrency,
    resources: &ExecutionBudget,
    backend: Backend,
    probe: &ProbeReport,
    runs: u64,
) -> ExecutionLayout {
    let runs = usize::try_from(runs).unwrap_or(usize::MAX);
    if backend == Backend::Gpu {
        let tracks = match concurrency {
            Concurrency::Fixed(tracks) => tracks.get(),
            Concurrency::Auto if probe.population >= LARGE_GPU_POPULATION => 1,
            Concurrency::Auto => {
                let demand = probe.demand.as_ref().map_or(0, Demand::bytes);
                let tracks_by_memory = match resources.gpu_memory_budget {
                    Some(budget) if demand > 0 => usize::try_from(budget / demand).unwrap_or(usize::MAX),
                    _ => usize::MAX,
                };
                tracks_by_memory.min(MAX_AUTO_GPU_TRACKS)
            }
        };
        return ExecutionLayout {
            cpu_lanes: 0,
            threads_per_lane: 0,
            gpu_tracks: tracks.min(runs).max(1),
        };
    }
    let workers = resources.workers.max(1);
    let one_lane = ExecutionLayout {
        cpu_lanes: 1,
        threads_per_lane: workers,
        gpu_tracks: 0,
    };
    if !resources.can_spawn_threads {
        return one_lane;
    }
    let (lanes, threads_per_lane) = match concurrency {
        Concurrency::Auto => {
            let jobs = probe
                .parallel_jobs
                .unwrap_or_else(|| usize::try_from(probe.population / POPULATION_PER_JOB).unwrap_or(usize::MAX));
            let threads = jobs.div_ceil(JOBS_PER_THREAD).clamp(1, workers);
            (workers / threads, threads)
        }
        Concurrency::Fixed(lanes) => (lanes.get(), (workers / lanes.get()).max(1)),
    };
    let lanes_by_memory = match resources.memory_budget {
        Some(budget) if probe.heap_bytes > 0 => usize::try_from(budget / probe.heap_bytes).unwrap_or(usize::MAX),
        _ => usize::MAX,
    };
    let lanes = lanes.min(runs).min(lanes_by_memory);
    if lanes <= 1 {
        one_lane
    } else {
        ExecutionLayout {
            cpu_lanes: lanes,
            threads_per_lane,
            gpu_tracks: 0,
        }
    }
}

const RUNNING: u8 = 0;
const PAUSED: u8 = 1;
const ABORTED: u8 = 2;

/// Pause, resume and abort switch for a sweep, shared by clones.
///
/// Executors call [`Self::proceed`] between slices of steps. An abort is final.
#[derive(Debug, Clone, Default)]
pub struct SweepControl {
    shared: Arc<ControlState>,
}

#[derive(Debug, Default)]
struct ControlState {
    /// One of [`RUNNING`], [`PAUSED`] and [`ABORTED`].
    mode: AtomicU8,
    /// Held while the mode changes, so a paused run never misses its release.
    lock: Mutex<()>,
    resumed: Condvar,
}

impl SweepControl {
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds every run at its next slice. Note that a pause after an abort changes nothing.
    pub fn pause(&self) {
        let _guard = self.shared.lock.lock().unwrap_or_else(PoisonError::into_inner);
        if self.shared.mode.load(Ordering::Acquire) == RUNNING {
            self.shared.mode.store(PAUSED, Ordering::Release);
        }
    }

    /// Lets paused runs continue.
    pub fn resume(&self) {
        let _guard = self.shared.lock.lock().unwrap_or_else(PoisonError::into_inner);
        if self.shared.mode.load(Ordering::Acquire) == PAUSED {
            self.shared.mode.store(RUNNING, Ordering::Release);
            self.shared.resumed.notify_all();
        }
    }

    /// Ends every run at its next slice, paused runs included.
    pub fn abort(&self) {
        let _guard = self.shared.lock.lock().unwrap_or_else(PoisonError::into_inner);
        self.shared.mode.store(ABORTED, Ordering::Release);
        self.shared.resumed.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        self.shared.mode.load(Ordering::Acquire) == PAUSED
    }

    pub fn is_aborted(&self) -> bool {
        self.shared.mode.load(Ordering::Acquire) == ABORTED
    }

    /// Returns whether a run can take its next slice, blocking while the sweep is paused.
    ///
    /// Returns `false` once the sweep is aborted.
    pub fn proceed(&self) -> bool {
        match self.shared.mode.load(Ordering::Acquire) {
            RUNNING => true,
            PAUSED => self.wait_while_paused(),
            _ => false,
        }
    }

    fn wait_while_paused(&self) -> bool {
        let mut guard = self.shared.lock.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            match self.shared.mode.load(Ordering::Acquire) {
                RUNNING => return true,
                PAUSED => {
                    guard = self.shared.resumed.wait(guard).unwrap_or_else(PoisonError::into_inner);
                }
                _ => return false,
            }
        }
    }
}

/// Runs in progress and the tick each has reached, and the finished runs waiting to be committed, shared by clones.
#[derive(Debug, Clone, Default)]
pub struct ActiveRuns {
    shared: Arc<Mutex<ActiveRunsTable>>,
}

/// Contents of an [`ActiveRuns`] table.
#[derive(Debug, Default)]
struct ActiveRunsTable {
    /// Runs in progress, by run id.
    stepping: BTreeMap<u64, ActiveRun>,
    /// Ids of the runs that finished and wait for an earlier run to be committed.
    waiting: BTreeSet<u64>,
}

/// One run in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveRun {
    pub run: PlannedRun,
    /// Tick the run has reached.
    pub tick: u64,
    /// Tick the run ends on unless its stop condition holds sooner.
    pub end_tick: u64,
}

impl ActiveRuns {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the runs in progress, in order of their ids.
    pub fn list(&self) -> Vec<ActiveRun> {
        self.lock().stepping.values().copied().collect()
    }

    /// Number of runs that finished and wait for an earlier run to be committed.
    ///
    /// Note that a batch that ends before committing every run leaves the runs still waiting in the count.
    pub fn waiting_count(&self) -> usize {
        self.lock().waiting.len()
    }

    /// Adds `run` at tick 0, ending at `end_tick`, and returns the watch that moves it on and removes it on drop.
    pub fn watch(&self, run: PlannedRun, end_tick: u64) -> RunWatch {
        self.lock()
            .stepping
            .insert(run.run_id, ActiveRun { run, tick: 0, end_tick });
        RunWatch {
            active_runs: self.clone(),
            run_id: run.run_id,
        }
    }

    /// Removes run `run_id` from the runs waiting to be committed.
    pub fn mark_committed(&self, run_id: u64) {
        self.lock().waiting.remove(&run_id);
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ActiveRunsTable> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Entry of one run in an [`ActiveRuns`] table, removed when the watch is dropped.
#[derive(Debug)]
pub struct RunWatch {
    active_runs: ActiveRuns,
    run_id: u64,
}

impl RunWatch {
    /// Records that the run has reached `tick`.
    pub fn reach(&self, tick: u64) {
        if let Some(active) = self.active_runs.lock().stepping.get_mut(&self.run_id) {
            active.tick = tick;
        }
    }

    /// Moves the finished run from the runs in progress to the runs waiting to be committed.
    pub fn finish(self) {
        self.active_runs.lock().waiting.insert(self.run_id);
    }
}

impl Drop for RunWatch {
    fn drop(&mut self) {
        self.active_runs.lock().stepping.remove(&self.run_id);
    }
}

/// One run for an executor to build and drive.
#[derive(Debug, Clone)]
pub struct RunRequest<'p> {
    pub run: PlannedRun,
    /// Key naming the results of the run.
    pub run_key: u64,
    /// Values of the run's config, one per parameter.
    pub params: &'p [ParamValue],
    /// Actions of the run's config, in the order they fire.
    pub schedule: Schedule,
}

impl<'p> RunRequest<'p> {
    /// Returns the request for `run` of `plan`.
    ///
    /// # Panics
    ///
    /// Panics when the config of `run` is not in `plan`.
    pub fn planned(plan: &'p Plan, run: PlannedRun) -> Self {
        let config = plan
            .config(run.config_id)
            .expect("a planned run's config is in its plan");
        Self {
            run,
            run_key: plan.run_key(&run),
            params: &config.params,
            schedule: plan.schedule(config),
        }
    }
}

/// Receiver of the outcomes of a batch.
pub trait RunSink {
    /// Takes a finished run. Runs arrive on the calling thread in request order.
    ///
    /// # Errors
    ///
    /// Returns the error that stopped the sink. The batch ends with it, and the executor aborts its control.
    fn commit(&mut self, outcome: RunOutcome) -> io::Result<()>;

    /// Sees a run as it finishes, before [`Self::commit`] takes it. Runs arrive on the calling thread in the order
    /// they finish.
    fn finished(&mut self, _outcome: &RunOutcome) {}
}

/// End of a batch that did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchEnd {
    /// Every request was committed.
    Complete,
    /// The control aborted the batch. The runs committed before it are a prefix of the requests.
    Aborted,
    /// The GPU device was lost. The runs committed before it are a prefix of the requests, and the rest have no
    /// outcome.
    DeviceLost,
}

/// Command buffers and steps each GPU track keeps on the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GpuTrackDepth {
    /// Command buffers of one track the device can hold at once.
    pub submissions_per_track: usize,
    /// Most steps one command buffer holds, at most [`MAX_STEPS_PER_SUBMISSION`].
    pub steps_per_submission: u32,
}

impl Default for GpuTrackDepth {
    /// Returns two command buffers per track of [`MAX_STEPS_PER_SUBMISSION`] steps each.
    fn default() -> Self {
        Self {
            submissions_per_track: 2,
            steps_per_submission: MAX_STEPS_PER_SUBMISSION,
        }
    }
}

/// A batch that cannot run.
#[derive(Debug)]
pub enum ExecutionError {
    /// A GPU model with no device to step it on.
    NoDevice,
    /// Building a lane's thread pool failed.
    Pool(rayon::ThreadPoolBuildError),
    /// Spawning a lane's thread failed.
    Spawn(io::Error),
    /// The sink refused a finished run.
    Sink(io::Error),
    /// A lane's thread panicked outside any run, and its runs were lost.
    LanePanicked,
}

impl fmt::Display for ExecutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoDevice => f.write_str("a GPU model needs a GPU device"),
            Self::Pool(_) => f.write_str("cannot build a lane's thread pool"),
            Self::Spawn(_) => f.write_str("cannot start a lane's thread"),
            Self::Sink(_) => f.write_str("cannot write a finished run"),
            Self::LanePanicked => f.write_str("a lane's thread panicked outside any run"),
        }
    }
}

impl std::error::Error for ExecutionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Pool(error) => Some(error),
            Self::Spawn(error) | Self::Sink(error) => Some(error),
            Self::NoDevice | Self::LanePanicked => None,
        }
    }
}

/// Runner of batches of one model, with its lane pools built once.
#[derive(Debug)]
pub struct Executor<'a> {
    entry: &'a ModelEntry,
    #[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "a browser steps no GPU track"))]
    gpu: Option<&'a GpuContext>,
    measure: Arc<MeasurePlan>,
    layout: ExecutionLayout,
    control: SweepControl,
    /// Wall time after which a run is abandoned, read as [`Self::with_timeout`] describes.
    timeout: Option<Duration>,
    /// Table each run in progress is listed in, `None` when nothing watches the runs.
    active_runs: Option<ActiveRuns>,
    /// One pool per lane. Empty when a single lane steps on rayon's global pool.
    pools: Vec<rayon::ThreadPool>,
    /// Cap on the bytes of device memory the live GPU runs hold together, `None` for the device's largest buffer. A
    /// run larger than the cap runs alone.
    #[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "a browser steps no GPU track"))]
    gpu_memory: Option<u64>,
    #[cfg_attr(target_arch = "wasm32", expect(dead_code, reason = "a browser steps no GPU track"))]
    track_depth: GpuTrackDepth,
}

impl<'a> Executor<'a> {
    /// Returns an executor for `entry` sampled as `measure` asks, with a pool for each lane of `layout`.
    ///
    /// A single lane as wide as rayon's global pool uses that pool. Note that a target that cannot spawn threads
    /// runs a CPU model in a single lane, whatever `layout` asks.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError::NoDevice`] for a GPU model with no device in `gpu`, and [`ExecutionError::Pool`]
    /// when a lane's pool cannot be built.
    pub fn new(
        entry: &'a ModelEntry,
        gpu: Option<&'a GpuContext>,
        measure: Arc<MeasurePlan>,
        layout: ExecutionLayout,
        control: SweepControl,
    ) -> Result<Self, ExecutionError> {
        let mut layout = layout;
        let mut pools = Vec::new();
        match entry.metadata().backend {
            Backend::Gpu if gpu.is_none() => return Err(ExecutionError::NoDevice),
            Backend::Gpu => {}
            Backend::Cpu => {
                let global_width = rayon::current_num_threads();
                if !CAN_SPAWN_THREADS || (layout.cpu_lanes <= 1 && layout.threads_per_lane == global_width) {
                    layout.cpu_lanes = 1;
                    layout.threads_per_lane = global_width;
                } else {
                    pools = (0..layout.cpu_lanes.max(1))
                        .map(|lane| {
                            rayon::ThreadPoolBuilder::new()
                                .num_threads(layout.threads_per_lane.max(1))
                                .thread_name(move |worker| format!("henad-lane-{lane}-{worker}"))
                                .build()
                        })
                        .collect::<Result<_, _>>()
                        .map_err(ExecutionError::Pool)?;
                }
            }
        }
        Ok(Self {
            entry,
            gpu,
            measure,
            layout,
            control,
            timeout: None,
            active_runs: None,
            pools,
            gpu_memory: None,
            track_depth: GpuTrackDepth::default(),
        })
    }

    /// Returns the executor with each run abandoned once its stepping and sampling pass `timeout`.
    ///
    /// The timeout is checked between slices of steps. A GPU run counts its share of the wall time since its build.
    /// The time spent visiting the tracks is split evenly between the live runs, and the time the batch spent paused is
    /// left out.
    pub fn with_timeout(self, timeout: Option<Duration>) -> Self {
        Self { timeout, ..self }
    }

    /// Returns the executor with each run listed in `active_runs` from its build to its end.
    pub fn with_active_runs(self, active_runs: Option<ActiveRuns>) -> Self {
        Self { active_runs, ..self }
    }

    /// Returns the executor with a budget of `gpu_memory` bytes of device memory for the live GPU runs, as
    /// [`gpu_memory_budget`] reads it.
    ///
    /// A run is built once its demand fits the budget beside the demand of the live runs. A run larger than the budget
    /// is built once no other run is live, and runs alone.
    pub fn with_gpu_memory(self, gpu_memory: Option<u64>) -> Self {
        Self { gpu_memory, ..self }
    }

    /// Returns the executor with each GPU track keeping `depth` on the device.
    ///
    /// # Panics
    ///
    /// Panics when `depth` holds no command buffer or no step, or more steps than one submission can hold.
    pub fn with_track_depth(self, depth: GpuTrackDepth) -> Self {
        assert!(
            depth.submissions_per_track > 0 && (1..=MAX_STEPS_PER_SUBMISSION).contains(&depth.steps_per_submission),
            "{depth:?} cannot step a track"
        );
        Self {
            track_depth: depth,
            ..self
        }
    }

    /// Layout the executor runs on, after [`Self::new`] adjusted it to the target.
    pub fn layout(&self) -> ExecutionLayout {
        self.layout
    }

    pub fn control(&self) -> &SweepControl {
        &self.control
    }

    /// Runs every request and commits each outcome to `sink` in request order.
    ///
    /// Runs that finish early wait in memory until every earlier request is committed.
    ///
    /// # Errors
    ///
    /// Returns [`ExecutionError`] when a lane's thread cannot start or panics outside a run, or `sink` refuses a run.
    pub fn run_batch(&self, requests: &[RunRequest<'_>], sink: &mut dyn RunSink) -> Result<BatchEnd, ExecutionError> {
        match (self.entry.metadata().backend, self.pools.as_slice()) {
            #[cfg(not(target_arch = "wasm32"))]
            (Backend::Gpu, _) => {
                let ctx = self.gpu.ok_or(ExecutionError::NoDevice)?;
                gpu::run_on_tracks(self, ctx, requests, sink)
            }
            // A browser cannot block on the device. Each run's cursor refuses its GPU model.
            #[cfg(target_arch = "wasm32")]
            (Backend::Gpu, _) => self.run_in_order(requests, sink, Placement::GlobalPool),
            (Backend::Cpu, []) => self.run_in_order(requests, sink, Placement::GlobalPool),
            (Backend::Cpu, [pool]) => self.run_in_order(requests, sink, Placement::LanePool(pool)),
            #[cfg(not(target_arch = "wasm32"))]
            (Backend::Cpu, pools) => cpu::run_in_lanes(self, pools, requests, sink),
            // A browser cannot start a lane's thread, so it builds no lane pools.
            #[cfg(target_arch = "wasm32")]
            (Backend::Cpu, _) => self.run_in_order(requests, sink, Placement::GlobalPool),
        }
    }

    /// Runs `requests` one at a time, each placed by `placement`, and commits each as it finishes.
    fn run_in_order(
        &self,
        requests: &[RunRequest<'_>],
        sink: &mut dyn RunSink,
        placement: Placement<'_>,
    ) -> Result<BatchEnd, ExecutionError> {
        for request in requests {
            let Some(outcome) = self.drive_in(placement, request) else {
                return Ok(BatchEnd::Aborted);
            };
            sink.finished(&outcome);
            self.commit(sink, outcome)?;
        }
        Ok(BatchEnd::Complete)
    }

    /// Drives the run of `request` on the threads `placement` names.
    ///
    /// Entering the pool once per run keeps each parallel pass of the run's kernels starting on a worker. From
    /// outside the pool every pass would be injected, parking the calling thread once per pass.
    fn drive_in(&self, placement: Placement<'_>, request: &RunRequest<'_>) -> Option<RunOutcome> {
        match placement {
            Placement::GlobalPool => run_in_global_pool(|| self.drive(request)),
            Placement::LanePool(pool) => run_in_pool(pool, || self.drive(request)),
        }
    }

    /// Builds the run of `request` and drives it to its end, or returns `None` once the control aborts.
    fn drive(&self, request: &RunRequest<'_>) -> Option<RunOutcome> {
        if !self.control.proceed() {
            return None;
        }
        // Each run sizes its slices afresh. A slice carried over from a lighter run could hold off the control for
        // minutes.
        let mut slice = SliceSize::default();
        let watch = self
            .active_runs
            .as_ref()
            .map(|active_runs| active_runs.watch(request.run, self.measure.total()));
        let mut cursor = RunCursor::new(self.entry, &self.measure, request, self.timeout);
        loop {
            let started = Instant::now();
            let state = cursor.advance(slice.steps());
            slice.adapt(started.elapsed());
            if let CursorState::Finished(outcome) = state {
                if let Some(watch) = watch {
                    watch.finish();
                }
                return Some(outcome);
            }
            if let Some(watch) = &watch {
                watch.reach(cursor.tick());
            }
            if !self.control.proceed() {
                return None;
            }
        }
    }

    /// Commits `outcome` to `sink`, aborting the control when the sink refuses it.
    fn commit(&self, sink: &mut dyn RunSink, outcome: RunOutcome) -> Result<(), ExecutionError> {
        if let Some(active_runs) = &self.active_runs {
            active_runs.mark_committed(outcome.run.run_id);
        }
        sink.commit(outcome).map_err(|error| {
            self.control.abort();
            ExecutionError::Sink(error)
        })
    }
}

/// Runs `task` inside rayon's global pool, so each parallel pass starts on a worker.
#[cfg(not(target_arch = "wasm32"))]
fn run_in_global_pool<R: Send>(task: impl FnOnce() -> R + Send) -> R {
    rayon::scope(|_| task())
}

/// Runs `task` on the calling thread. In a browser the frame loop pumps from outside the pool, as the live loop does.
#[cfg(target_arch = "wasm32")]
fn run_in_global_pool<R>(task: impl FnOnce() -> R) -> R {
    task()
}

/// Runs `task` inside `pool`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_in_pool<R: Send>(pool: &rayon::ThreadPool, task: impl FnOnce() -> R + Send) -> R {
    pool.install(task)
}

/// Runs `task` on the calling thread. A browser cannot start a lane's thread, so no lane pool is built there.
#[cfg(target_arch = "wasm32")]
pub(crate) fn run_in_pool<R>(_pool: &rayon::ThreadPool, task: impl FnOnce() -> R) -> R {
    task()
}

/// Threads a CPU run is built and stepped on.
#[derive(Clone, Copy)]
enum Placement<'p> {
    /// Rayon's global pool.
    GlobalPool,
    /// The pool of one lane.
    LanePool(&'p rayon::ThreadPool),
}

/// Steps a lane takes between two checks of its control, sized to take about a target wall time.
///
/// The default target is [`SLICE_TARGET_MS`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct SliceSize {
    steps: u64,
    /// Wall time in milliseconds one slice aims to take.
    target_ms: f64,
}

impl Default for SliceSize {
    fn default() -> Self {
        Self::aiming_at(SLICE_TARGET_MS)
    }
}

impl SliceSize {
    /// Returns a slice of one step that grows toward `target_ms` milliseconds.
    pub(crate) fn aiming_at(target_ms: f64) -> Self {
        Self { steps: 1, target_ms }
    }

    pub(crate) fn steps(&self) -> u64 {
        self.steps
    }

    /// Doubles the slice after one that took under half the target, and scales it down to the target after one over
    /// twice the target.
    pub(crate) fn adapt(&mut self, elapsed: Duration) {
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        if elapsed_ms < self.target_ms / 2.0 {
            self.steps = (self.steps * 2).min(MAX_SLICE_STEPS);
        } else if elapsed_ms > self.target_ms * 2.0 {
            let scaled = self.steps as f64 * self.target_ms / elapsed_ms;
            self.steps = (scaled as u64).max(1);
        }
    }
}

/// Items that arrive out of order, handed out in index order.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct ReorderBuffer<T> {
    /// Index of the next item to hand out.
    next: usize,
    waiting: BTreeMap<usize, T>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<T> Default for ReorderBuffer<T> {
    fn default() -> Self {
        Self {
            next: 0,
            waiting: BTreeMap::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl<T> ReorderBuffer<T> {
    fn insert(&mut self, index: usize, item: T) {
        debug_assert!(index >= self.next, "item {index} was already handed out");
        self.waiting.insert(index, item);
    }

    /// Returns the next item in index order, once it has arrived.
    fn pop_ready(&mut self) -> Option<T> {
        let item = self.waiting.remove(&self.next)?;
        self.next += 1;
        Some(item)
    }

    /// Number of items handed out.
    fn handed_out(&self) -> usize {
        self.next
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use henad_compute::entry::{ModelEntry, ModelState};
    use henad_compute::fault::Fault;
    use henad_compute::gpu::Demand;
    use henad_compute::gpu::GpuContext;
    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::measure::MeasurePlan;
    use henad_core::explore::outcome::{PlannedRun, RunOutcome, RunStatus};
    use henad_core::explore::plan::Plan;
    use henad_core::explore::spec::{BlockSpec, SweepSpec};
    use henad_core::export::StatColumns;
    use henad_core::metadata::Backend;
    use henad_core::params::ParamValue;
    use henad_models::example_models;

    use super::{
        ActiveRuns, BatchEnd, Concurrency, ExecutionBudget, ExecutionError, ExecutionLayout, Executor,
        LARGE_GPU_POPULATION, ReorderBuffer, RunRequest, RunSink, SliceSize, SweepControl, choose_layout,
    };
    use crate::probe::ProbeReport;
    use crate::tests::support::{lanes, tracks};

    fn probe(parallel_jobs: Option<usize>, population: u64, heap_bytes: u64) -> ProbeReport {
        ProbeReport {
            params: Vec::new(),
            seed: None,
            columns: StatColumns::plan(&[]),
            heap_bytes,
            population,
            parallel_jobs,
            demand: None,
        }
    }

    fn resources(workers: usize) -> ExecutionBudget {
        ExecutionBudget {
            workers,
            memory_budget: None,
            gpu_memory_budget: None,
            can_spawn_threads: true,
        }
    }

    /// Returns a probe of a GPU model with `population` and a demand of `bytes` bytes of device memory.
    fn gpu_probe(population: u64, bytes: usize) -> ProbeReport {
        let mut demand = Demand::default();
        demand.push("cells".to_owned(), bytes / 4);
        ProbeReport {
            demand: Some(demand),
            ..probe(None, population, 0)
        }
    }

    fn fixed(count: usize) -> Concurrency {
        Concurrency::Fixed(NonZeroUsize::new(count).expect("a test count is above 0"))
    }

    #[test]
    fn a_small_model_gets_one_thread_per_lane() {
        let small = probe(Some(3), 4096, 1 << 20);
        let layout = choose_layout(Concurrency::Auto, &resources(8), Backend::Cpu, &small, 100);
        assert_eq!(layout, lanes(8, 1));
    }

    #[test]
    fn a_wide_model_gets_wide_lanes() {
        let wide = probe(Some(9), 0, 0);
        let layout = choose_layout(Concurrency::Auto, &resources(8), Backend::Cpu, &wide, 100);
        assert_eq!(
            layout,
            lanes(2, 3),
            "9 jobs take 3 threads, and 8 workers make 2 such lanes"
        );
        let widest = probe(Some(64), 0, 0);
        let layout = choose_layout(Concurrency::Auto, &resources(8), Backend::Cpu, &widest, 100);
        assert_eq!(layout, lanes(1, 8), "a model as wide as the machine runs alone");
    }

    #[test]
    fn a_model_without_jobs_is_sized_by_population() {
        let unsplit = probe(None, 16 * 4096, 0);
        let layout = choose_layout(Concurrency::Auto, &resources(8), Backend::Cpu, &unsplit, 100);
        assert_eq!(layout, lanes(2, 4));
    }

    #[test]
    fn lanes_are_capped_by_runs_and_memory() {
        let small = probe(Some(1), 0, 1000);
        let few_runs = choose_layout(Concurrency::Auto, &resources(8), Backend::Cpu, &small, 3);
        assert_eq!(few_runs, lanes(3, 1));
        let one_run = choose_layout(Concurrency::Auto, &resources(8), Backend::Cpu, &small, 1);
        assert_eq!(one_run, lanes(1, 8), "a single lane takes every worker");

        let budget = ExecutionBudget {
            memory_budget: Some(4500),
            ..resources(8)
        };
        let layout = choose_layout(Concurrency::Auto, &budget, Backend::Cpu, &small, 100);
        assert_eq!(layout, lanes(4, 1));
        assert_eq!(layout.projected_bytes(&small), 4000);
        let tight = ExecutionBudget {
            memory_budget: Some(10),
            ..resources(8)
        };
        let layout = choose_layout(Concurrency::Auto, &tight, Backend::Cpu, &small, 100);
        assert_eq!(layout, lanes(1, 8), "one run goes ahead even past the budget");
    }

    #[test]
    fn a_fixed_lane_count_splits_the_workers() {
        let small = probe(Some(1), 0, 0);
        assert_eq!(
            choose_layout(fixed(3), &resources(8), Backend::Cpu, &small, 100),
            lanes(3, 2)
        );
        assert_eq!(
            choose_layout(fixed(16), &resources(8), Backend::Cpu, &small, 100),
            lanes(16, 1)
        );
        assert_eq!(
            choose_layout(fixed(4), &resources(8), Backend::Cpu, &small, 2),
            lanes(2, 2)
        );
        assert_eq!(
            choose_layout(fixed(1), &resources(8), Backend::Cpu, &small, 100),
            lanes(1, 8)
        );
    }

    #[test]
    fn a_target_without_threads_runs_one_lane() {
        let small = probe(Some(1), 0, 0);
        let threadless = ExecutionBudget {
            can_spawn_threads: false,
            ..resources(8)
        };
        assert_eq!(
            choose_layout(fixed(4), &threadless, Backend::Cpu, &small, 100),
            lanes(1, 8)
        );
    }

    #[test]
    fn gpu_tracks_are_sized_by_the_memory_budget_and_the_population() {
        let small = gpu_probe(64 * 64, 1000);
        let gpu_budget = |bytes| ExecutionBudget {
            gpu_memory_budget: Some(bytes),
            ..resources(8)
        };
        let layout = |concurrency, budget: &ExecutionBudget, probe: &ProbeReport, runs| {
            choose_layout(concurrency, budget, Backend::Gpu, probe, runs)
        };
        assert_eq!(layout(Concurrency::Auto, &gpu_budget(1 << 30), &small, 100), tracks(4));
        assert_eq!(layout(Concurrency::Auto, &gpu_budget(2500), &small, 100), tracks(2));
        assert_eq!(
            layout(Concurrency::Auto, &gpu_budget(500), &small, 100),
            tracks(1),
            "a run larger than the budget runs alone"
        );
        assert_eq!(layout(Concurrency::Auto, &gpu_budget(1 << 30), &small, 3), tracks(3));
        assert_eq!(layout(Concurrency::Auto, &resources(8), &small, 100), tracks(4));

        let large = gpu_probe(LARGE_GPU_POPULATION, 1000);
        assert_eq!(layout(Concurrency::Auto, &gpu_budget(1 << 30), &large, 100), tracks(1));

        assert_eq!(
            layout(fixed(6), &gpu_budget(2500), &large, 100),
            tracks(6),
            "a fixed count is kept"
        );
        assert_eq!(layout(fixed(6), &gpu_budget(2500), &small, 2), tracks(2));
    }

    #[test]
    fn concurrency_reads_auto_or_a_positive_count() {
        assert_eq!("auto".parse(), Ok(Concurrency::Auto));
        assert_eq!("3".parse(), Ok(fixed(3)));
        assert!("0".parse::<Concurrency>().is_err());
        assert!("many".parse::<Concurrency>().is_err());
        assert_eq!(fixed(3).to_string(), "3");
        assert_eq!(Concurrency::Auto.to_string(), "auto");
    }

    #[test]
    fn the_slice_grows_on_fast_steps_and_shrinks_on_slow_ones() {
        let mut slice = SliceSize::default();
        for _ in 0..6 {
            slice.adapt(Duration::from_micros(10));
        }
        assert_eq!(slice.steps, 64);
        slice.adapt(Duration::from_millis(20));
        assert_eq!(slice.steps, 64, "a slice near the target keeps its size");
        slice.adapt(Duration::from_millis(160));
        assert_eq!(slice.steps, 8, "a slice eight times too long shrinks eightfold");
        slice.adapt(Duration::from_secs(1));
        assert_eq!(slice.steps, 1, "a slice never drops below one step");
    }

    #[test]
    fn the_reorder_buffer_hands_items_out_in_index_order() {
        let mut buffer = ReorderBuffer::default();
        buffer.insert(2, 'c');
        buffer.insert(1, 'b');
        assert_eq!(buffer.pop_ready(), None, "item 0 has not arrived");
        buffer.insert(0, 'a');
        let mut handed_out = Vec::new();
        while let Some(item) = buffer.pop_ready() {
            handed_out.push(item);
        }
        assert_eq!(handed_out, ['a', 'b', 'c']);
        buffer.insert(4, 'e');
        assert_eq!(buffer.pop_ready(), None, "item 3 has not arrived");
        assert_eq!(buffer.handed_out(), 3);
    }

    #[test]
    fn pause_blocks_until_resume() {
        let control = SweepControl::new();
        control.pause();
        assert!(control.is_paused());
        let (sender, receiver) = mpsc::channel();
        let waiter = control.clone();
        let thread = std::thread::spawn(move || sender.send(waiter.proceed()));
        assert!(
            receiver.recv_timeout(Duration::from_millis(100)).is_err(),
            "a paused control holds the run"
        );
        control.resume();
        assert_eq!(receiver.recv_timeout(Duration::from_secs(10)), Ok(true));
        assert!(thread.join().is_ok_and(|sent| sent.is_ok()), "the waiter reported");
        assert!(control.proceed(), "a resumed control lets runs through");
    }

    #[test]
    fn abort_releases_a_paused_run_and_is_final() {
        let control = SweepControl::new();
        control.pause();
        let (sender, receiver) = mpsc::channel();
        let waiter = control.clone();
        let thread = std::thread::spawn(move || sender.send(waiter.proceed()));
        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        control.abort();
        assert_eq!(receiver.recv_timeout(Duration::from_secs(10)), Ok(false));
        assert!(thread.join().is_ok_and(|sent| sent.is_ok()), "the waiter reported");
        control.pause();
        control.resume();
        assert!(control.is_aborted(), "neither a pause nor a resume undoes an abort");
        assert!(!control.proceed());
    }

    #[test]
    fn a_finished_run_waits_until_it_is_committed() {
        let active_runs = ActiveRuns::new();
        let run = |run_id| PlannedRun {
            run_id,
            config_id: 0,
            rep: run_id,
            seed: 1,
        };
        let (first, second, third) = (
            active_runs.watch(run(0), 10),
            active_runs.watch(run(1), 10),
            active_runs.watch(run(2), 10),
        );
        second.reach(4);
        assert_eq!(active_runs.list().len(), 3);
        assert_eq!(active_runs.list()[1].tick, 4);

        third.finish();
        assert_eq!(active_runs.list().len(), 2, "a finished run is no longer in progress");
        assert_eq!(active_runs.waiting_count(), 1, "run 2 waits for runs 0 and 1");
        drop(first);
        assert_eq!(active_runs.waiting_count(), 1, "an abandoned run never waits");
        active_runs.mark_committed(2);
        assert_eq!(active_runs.waiting_count(), 0);
        drop(second);
        assert!(active_runs.list().is_empty());
    }

    /// Sink that keeps every outcome, and aborts its control once `abort_after` runs have finished.
    struct KeepingSink {
        committed: Vec<RunOutcome>,
        finished: usize,
        abort_after: Option<(usize, SweepControl)>,
    }

    impl KeepingSink {
        fn new() -> Self {
            Self {
                committed: Vec::new(),
                finished: 0,
                abort_after: None,
            }
        }
    }

    impl RunSink for KeepingSink {
        fn commit(&mut self, outcome: RunOutcome) -> std::io::Result<()> {
            self.committed.push(outcome);
            Ok(())
        }

        fn finished(&mut self, _outcome: &RunOutcome) {
            self.finished += 1;
            if let Some((count, control)) = &self.abort_after
                && self.finished >= *count
            {
                control.abort();
            }
        }
    }

    fn entry(id: &str) -> ModelEntry {
        example_models().get(id).cloned().expect("the model is registered")
    }

    /// Returns a plan over `entry` with three grid sizes and `replicates` replicates of `steps` steps each.
    fn plan(entry: &ModelEntry, steps: u64, replicates: u64) -> (Plan, Arc<MeasurePlan>) {
        let mut spec = SweepSpec::new(entry.id().to_owned());
        spec.run.steps = steps;
        spec.run.replicates = replicates;
        spec.measure.stats_every = 3;
        spec.measure.series_every = 6;
        spec.fixed = vec![("grid_height".to_owned(), "24".to_owned())];
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param(
                "grid_width",
                LevelSpec::Values(vec!["16".to_owned(), "20".to_owned(), "24".to_owned()]),
            )],
            design_seed: None,
        }];
        let plan = spec.plan(&entry.schema()).expect("a valid spec");
        let probe = ProbeReport::for_plan(entry, None, &plan).expect("the probe builds");
        let measure =
            MeasurePlan::new(plan.run_settings(), plan.measure_settings(), probe.columns).expect("the columns bind");
        (plan, Arc::new(measure))
    }

    fn requests(plan: &Plan) -> Vec<RunRequest<'_>> {
        plan.runs().map(|run| RunRequest::planned(plan, run)).collect()
    }

    /// Returns the committed outcomes with their timing zeroed.
    fn run(entry: &ModelEntry, plan: &Plan, measure: &Arc<MeasurePlan>, layout: ExecutionLayout) -> Vec<RunOutcome> {
        let executor =
            Executor::new(entry, None, Arc::clone(measure), layout, SweepControl::new()).expect("the lane pools build");
        let mut sink = KeepingSink::new();
        let end = executor.run_batch(&requests(plan), &mut sink).expect("the batch runs");
        assert_eq!(end, BatchEnd::Complete);
        assert_eq!(sink.finished, sink.committed.len());
        sink.committed
            .into_iter()
            .map(|outcome| RunOutcome {
                build_ms: 0.0,
                wall_ms: 0.0,
                ..outcome
            })
            .collect()
    }

    #[test]
    fn a_batch_commits_the_same_outcomes_in_request_order_at_any_lane_count() {
        let entry = entry("sir");
        let (plan, measure) = plan(&entry, 40, 3);
        let global = rayon::current_num_threads();
        let alone = run(&entry, &plan, &measure, lanes(1, global));
        assert_eq!(alone.len(), 9);
        let ids: Vec<u64> = alone.iter().map(|outcome| outcome.run.run_id).collect();
        assert_eq!(ids, (0..9).collect::<Vec<_>>());
        assert!(
            alone
                .iter()
                .all(|outcome| outcome.status == RunStatus::Ok && outcome.ticks == 40)
        );
        assert_eq!(alone[0].series.ticks(), [0, 6, 12, 18, 24, 30, 36, 40]);
        for layout in [lanes(1, 2), lanes(3, 1), lanes(4, 2)] {
            assert_eq!(run(&entry, &plan, &measure, layout), alone, "{layout:?}");
        }
    }

    #[test]
    fn an_abort_ends_every_lane() {
        let entry = entry("game_of_life");
        let (plan, measure) = plan(&entry, 1_000_000, 4);
        let requests = requests(&plan);
        for layout in [lanes(1, rayon::current_num_threads()), lanes(3, 1)] {
            let control = SweepControl::new();
            let executor = Executor::new(&entry, None, Arc::clone(&measure), layout, control.clone())
                .expect("the lane pools build");
            let mut sink = KeepingSink::new();
            let aborter = control.clone();
            let timer = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                aborter.abort();
            });
            let end = executor.run_batch(&requests, &mut sink).expect("the batch runs");
            assert!(timer.join().is_ok(), "the timer thread finished");
            assert_eq!(end, BatchEnd::Aborted, "{layout:?}");
            assert!(sink.committed.is_empty(), "no run reached a million steps: {layout:?}");
        }
    }

    /// Sink that aborts its control a short while after the first run finishes.
    struct AbortsAfterFirstRun {
        control: SweepControl,
        committed: usize,
        /// Thread that aborts the control, handing back the time it did.
        aborter: Option<std::thread::JoinHandle<Instant>>,
    }

    impl RunSink for AbortsAfterFirstRun {
        fn commit(&mut self, _outcome: RunOutcome) -> std::io::Result<()> {
            self.committed += 1;
            Ok(())
        }

        fn finished(&mut self, _outcome: &RunOutcome) {
            if self.aborter.is_none() {
                let control = self.control.clone();
                self.aborter = Some(std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(50));
                    let aborted_at = Instant::now();
                    control.abort();
                    aborted_at
                }));
            }
        }
    }

    #[test]
    fn an_abort_lands_within_a_slice_of_a_heavy_run_after_a_light_one() {
        let entry = entry("game_of_life");
        let mut spec = SweepSpec::new("game_of_life");
        spec.run.steps = 20_000;
        spec.measure.stats_every = 1000;
        spec.measure.series_every = 0;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Zip,
            factors: ["grid_width", "grid_height"]
                .map(|id| FactorSpec::param(id, LevelSpec::Values(vec!["8".to_owned(), "2048".to_owned()])))
                .to_vec(),
            design_seed: None,
        }];
        let plan = spec.plan(&entry.schema()).expect("a valid spec");
        let probe = ProbeReport::for_plan(&entry, None, &plan).expect("the probe builds");
        let measure = Arc::new(
            MeasurePlan::new(plan.run_settings(), plan.measure_settings(), probe.columns).expect("the columns bind"),
        );
        let control = SweepControl::new();
        let layout = lanes(1, rayon::current_num_threads());
        let executor =
            Executor::new(&entry, None, measure, layout, control.clone()).expect("a single lane needs no pool");
        let mut sink = AbortsAfterFirstRun {
            control,
            committed: 0,
            aborter: None,
        };
        let end = executor.run_batch(&requests(&plan), &mut sink).expect("the batch runs");
        let aborted_at = sink
            .aborter
            .take()
            .expect("the light run finished")
            .join()
            .expect("the aborting thread finished");
        assert_eq!(end, BatchEnd::Aborted);
        assert_eq!(sink.committed, 1, "only the light run finished");
        let latency = aborted_at.elapsed();
        assert!(
            latency < Duration::from_secs(5),
            "the heavy run stopped {latency:?} after the abort"
        );
    }

    /// Sink that panics on the first run it sees finish.
    struct PanickingSink;

    impl RunSink for PanickingSink {
        fn commit(&mut self, _outcome: RunOutcome) -> std::io::Result<()> {
            Ok(())
        }

        fn finished(&mut self, _outcome: &RunOutcome) {
            panic!("the sink cannot take a run");
        }
    }

    #[test]
    fn a_panicking_sink_aborts_every_lane() {
        let entry = entry("game_of_life");
        let (plan, measure) = plan(&entry, 200, 4);
        let requests = requests(&plan);
        let control = SweepControl::new();
        let executor = Executor::new(&entry, None, Arc::clone(&measure), lanes(3, 1), control.clone())
            .expect("the lane pools build");
        let batch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            executor.run_batch(&requests, &mut PanickingSink)
        }));
        assert!(batch.is_err(), "the sink's panic reaches the caller");
        assert!(control.is_aborted(), "the lanes were told to stop");
    }

    #[test]
    fn a_panicking_lane_ends_the_batch_with_an_error() {
        let plain = entry("game_of_life");
        let (plan, measure) = plan(&plain, 20, 2);
        // The build panics past the registry's catch, as a fault in the executor itself would.
        let panicking = plain.wrap_factory(|_| {
            Arc::new(
                |_params: &[ParamValue], _seed: Option<u64>, _gpu: Option<&GpuContext>| -> Result<ModelState, Fault> {
                    panic!("the lane cannot build a run")
                },
            )
        });
        let control = SweepControl::new();
        let executor =
            Executor::new(&panicking, None, measure, lanes(3, 1), control.clone()).expect("the lane pools build");
        let batch = executor.run_batch(&requests(&plan), &mut KeepingSink::new());
        assert!(matches!(batch, Err(ExecutionError::LanePanicked)), "{batch:?}");
        assert!(control.is_aborted(), "the other lanes were told to stop");
    }

    #[test]
    fn runs_committed_before_an_abort_are_a_prefix_of_the_requests() {
        let entry = entry("game_of_life");
        let (plan, measure) = plan(&entry, 2000, 4);
        let requests = requests(&plan);
        let control = SweepControl::new();
        let executor = Executor::new(&entry, None, Arc::clone(&measure), lanes(3, 1), control.clone())
            .expect("the lane pools build");
        let mut sink = KeepingSink::new();
        sink.abort_after = Some((2, control));
        let end = executor.run_batch(&requests, &mut sink).expect("the batch runs");
        assert_eq!(end, BatchEnd::Aborted);
        let ids: Vec<u64> = sink.committed.iter().map(|outcome| outcome.run.run_id).collect();
        assert_eq!(ids, (0..ids.len() as u64).collect::<Vec<_>>());
        assert!(ids.len() < requests.len());
    }
}
