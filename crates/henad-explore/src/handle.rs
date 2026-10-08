//! Handles on sweeps and searches that run beside a host's frame loop, with the same API on every target.
//!
//! On native a sweep runs on a thread of its own, and a pause holds its runs between two slices of steps. In a
//! browser a pumped sweep steps one CPU run at a time from the host's frames. Either way the host reads the sweep's
//! events from a channel that loses none, and a progress record where the latest write wins.

use std::fmt;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use web_time::Instant;

use henad_compute::cpu::sim_thread::WakeFn;
use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::explore::measure::SeriesBuffer;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::{Plan, PlanError};
use henad_core::explore::spec::SweepSpec;
use henad_core::metadata::Backend;

use crate::exec::{ActiveRun, ActiveRuns, Concurrency, SweepControl};
use crate::output::OutputError;
use crate::output::manifest::ManifestError;
use crate::progress::{Progress, ProgressEvent};
#[cfg(target_arch = "wasm32")]
use crate::pumped::{PumpedSweep, SweepCommand};
use crate::search_run::{SearchPlan, SearchPlanError, SearchUpdate};
use crate::spec_file::SpecFileError;
use crate::sweep::{Provenance, SpecSource, SweepEnd, SweepOutline, SweepRecord, SweepWarning};

/// Bytes of series the events of a sweep carry in all, unless the host asks for another budget.
#[cfg(not(target_arch = "wasm32"))]
pub const DEFAULT_SERIES_BUDGET: usize = 256 << 20;

/// Bytes of series the events of a sweep carry in all, unless the host asks for another budget.
#[cfg(target_arch = "wasm32")]
pub const DEFAULT_SERIES_BUDGET: usize = 64 << 20;

/// Place a sweep writes its four files.
///
/// The variants differ between targets, and a match outside this crate ends in a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SweepOutput {
    /// Memory, handed over in the [`SweepRecord`] once the sweep ends.
    Memory,
    /// A directory that holds no results, created when missing.
    #[cfg(not(target_arch = "wasm32"))]
    Directory(std::path::PathBuf),
}

/// Settings of a sweep a [`SweepRun`] runs. None of them change its results.
///
/// [`Self::new`] returns the defaults, and a caller sets the other fields by assignment. A spec file's `[execution]`
/// settings go in before the caller's own, in the order
/// [`SweepOptions::apply_execution`](crate::sweep::SweepOptions::apply_execution) sets them.
#[derive(Clone)]
#[non_exhaustive]
pub struct SweepRunOptions {
    /// Number of runs stepped at once on native. A browser steps one run at a time whatever this asks.
    pub concurrency: Concurrency,
    /// Bytes of host memory the live runs can hold together, `None` for no limit.
    pub memory_budget: Option<u64>,
    /// Bytes of GPU memory the live runs of a GPU model can hold together, `None` for the device's largest buffer.
    pub gpu_memory_budget: Option<u64>,
    /// Whether a resume runs again the runs that ended on a fault.
    pub retry_failed: bool,
    /// Bytes of series the [`SweepEvent::RunFinished`] events carry in all.
    ///
    /// Once a run's series would pass it, that run and every later one arrive without their series. The files keep
    /// every series.
    pub series_budget: usize,
    /// Spec file the sweep was read from, for the manifest.
    pub spec_source: SpecSource,
    /// Build of the host, for the manifest.
    pub provenance: Provenance,
    /// Called after each event, so an idle host comes to collect it. It must not block.
    pub wake: Option<WakeFn>,
}

/// Prints whether a wake callback is set, in place of the callback.
impl std::fmt::Debug for SweepRunOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SweepRunOptions")
            .field("concurrency", &self.concurrency)
            .field("memory_budget", &self.memory_budget)
            .field("gpu_memory_budget", &self.gpu_memory_budget)
            .field("retry_failed", &self.retry_failed)
            .field("series_budget", &self.series_budget)
            .field("spec_source", &self.spec_source)
            .field("provenance", &self.provenance)
            .field("wake", &self.wake.is_some())
            .finish_non_exhaustive()
    }
}

impl SweepRunOptions {
    /// Returns the options at their defaults, recording `provenance` in every manifest the sweep writes.
    pub fn new(provenance: Provenance) -> Self {
        Self {
            concurrency: Concurrency::Auto,
            memory_budget: None,
            gpu_memory_budget: None,
            retry_failed: false,
            series_budget: DEFAULT_SERIES_BUDGET,
            spec_source: SpecSource::default(),
            provenance,
            wake: None,
        }
    }
}

/// Event of a sweep, in the order it happened.
#[derive(Debug, Clone)]
pub enum SweepEvent {
    /// The sweep is planned and probed, and its runs start.
    Planned(Box<SweepOutline>),
    /// Something about the sweep that runs, though likely not as meant.
    Warned(SweepWarning),
    /// A run was written. Runs arrive in plan order, and a search's in the order it asks for them.
    RunFinished {
        outcome: Box<RunOutcome>,
        /// Whether the run's series passed the budget of [`SweepRunOptions::series_budget`] and was left out of
        /// `outcome`.
        series_dropped: bool,
    },
    /// A search was told the evaluations of one batch, after the runs of the batch.
    SearchBatchTold(Arc<SearchUpdate>),
    /// The sweep ran to its end or was aborted, as its record says. Nothing follows.
    Finished(Box<SweepRecord>),
    /// The sweep ended on an error of its own, outside any run, with the error and its causes. Nothing follows.
    Failed(String),
}

/// Stage a sweep has reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepPhase {
    /// Planning and probing, before the first run starts.
    Planning,
    Running,
    /// Held between two slices of steps. Runs in progress keep their state.
    Paused,
    /// Ran to its end or was aborted.
    Ended(SweepEnd),
    /// Ended on an error of its own, outside any run.
    Failed,
}

/// Progress of a sweep at the moment it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepProgress {
    pub phase: SweepPhase,
    /// Runs the sweep runs. Until the sweep is planned, every run of its plan.
    pub runs_total: u64,
    /// Runs a resumed directory held already. The sweep keeps them.
    pub runs_skipped: u64,
    /// Runs written so far.
    pub runs_done: u64,
    /// Runs that finished and wait for an earlier run to be written. An abort drops them.
    pub runs_waiting: u64,
    /// Written runs that ended on a fault or a timeout.
    pub runs_failed: u64,
    /// Time since the sweep started, pauses left out.
    pub elapsed: Duration,
    /// Time left at the pace so far, `None` before the first run is written and once the sweep ends.
    ///
    /// The pace counts the runs that finished and the share of its ticks each run in progress has stepped. A time too
    /// long for a [`Duration`], as a search with a budget of `u64::MAX` evaluations projects, reads as `None`.
    pub remaining: Option<Duration>,
    /// Runs in progress, in order of their ids.
    pub active_runs: Vec<ActiveRun>,
}

/// A sweep that cannot start.
#[derive(Debug)]
pub enum SweepStartError {
    /// A spec written for the model `spec_model`, handed the entry of `entry_model`.
    ModelMismatch { spec_model: String, entry_model: String },
    /// A spec the model refuses, for the reason inside.
    Plan(PlanError),
    /// A search spec the model refuses, for the reason inside.
    Search(SearchPlanError),
    /// A GPU model on a target that cannot step one in a sweep.
    GpuNeedsNative,
    /// An output directory that holds the results of a sweep, or that another sweep, search or merge is writing to,
    /// for the reason inside.
    Output(OutputError),
    /// A manifest that cannot be read, for the reason inside.
    Manifest(ManifestError),
    /// A manifest whose spec cannot be read back, for the reason inside.
    Spec(SpecFileError),
    /// A directory whose manifest names a shard outside its plan.
    Shard,
    /// Starting the sweep's thread failed.
    Spawn(std::io::Error),
}

impl fmt::Display for SweepStartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ModelMismatch {
                spec_model,
                entry_model,
            } => write!(f, "spec is for model '{spec_model}', expected '{entry_model}'"),
            Self::Plan(_) => f.write_str("cannot plan the sweep"),
            Self::Search(_) => f.write_str("cannot plan the search"),
            Self::GpuNeedsNative => f.write_str("a GPU sweep needs a native build"),
            Self::Output(_) => f.write_str("cannot write the results"),
            Self::Manifest(_) => f.write_str("cannot read the manifest of the sweep to resume"),
            Self::Spec(_) => f.write_str("cannot read the spec the manifest records"),
            Self::Shard => f.write_str("the manifest records an invalid shard"),
            Self::Spawn(_) => f.write_str("cannot start the sweep's thread"),
        }
    }
}

impl std::error::Error for SweepStartError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Search(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Manifest(error) => Some(error),
            Self::Spec(error) => Some(error),
            Self::Spawn(error) => Some(error),
            Self::ModelMismatch { .. } | Self::GpuNeedsNative | Self::Shard => None,
        }
    }
}

/// Handle on a running sweep or search.
///
/// Dropping the handle aborts the sweep. On native it then waits for the sweep's thread to write its files.
#[derive(Debug)]
pub struct SweepRun {
    plan: Arc<Plan>,
    /// Plan of a search, `None` for a sweep.
    search_plan: Option<Arc<SearchPlan>>,
    control: SweepControl,
    active_runs: ActiveRuns,
    events: Receiver<SweepEvent>,
    shared_progress: SharedProgress,
    clock: PauseClock,
    /// Whether the last event, [`SweepEvent::Finished`] or [`SweepEvent::Failed`], has been received.
    ended: bool,
    #[cfg(not(target_arch = "wasm32"))]
    thread: Option<std::thread::JoinHandle<()>>,
    #[cfg(target_arch = "wasm32")]
    driver: henad_compute::runner::Driver<PumpedSweep>,
}

impl SweepRun {
    /// Plans `spec` against `entry` and starts the sweep, or the search a `spec` with a search runs, writing its
    /// files to `output`.
    ///
    /// `gpu` is a device the host shares with the sweep, its [`FaultSink`](henad_compute::fault::FaultSink) included.
    /// Note that the sink holds one fault, and whichever side reads it first takes it. A fault the sweep takes ends
    /// every live run, whichever side raised it, and one the host takes first leaves the runs going. Handed no device,
    /// the sweep acquires one on its own thread for a GPU model, sized to the entry's
    /// [`gpu_needs`](ModelEntry::gpu_needs), and builds `entry` on it. The manifest records the adapter of a shared
    /// device only when its context carries [`RuntimeInfo`](henad_compute::runtime_info::RuntimeInfo), as
    /// [`GpuContext::with_runtime_info`] attaches it.
    ///
    /// Planning happens before this returns. The probe build and the runs happen after it, on native on a thread of
    /// the sweep's own and in a browser in [`Self::update`].
    ///
    /// # Errors
    ///
    /// Returns [`SweepStartError`] when `spec` names another model, cannot be planned, or asks a browser for a GPU
    /// model, when `output` is a directory that holds results, or when the sweep's thread cannot start. A device the
    /// sweep cannot acquire fails the sweep with a [`SweepEvent::Failed`].
    pub fn start(
        entry: ModelEntry,
        gpu: Option<GpuContext>,
        spec: SweepSpec,
        output: SweepOutput,
        options: SweepRunOptions,
    ) -> Result<Self, SweepStartError> {
        let (plan, search_plan) = plan_for_start(&entry, &spec)?;
        match &output {
            SweepOutput::Memory => {}
            #[cfg(not(target_arch = "wasm32"))]
            SweepOutput::Directory(dir) => {
                crate::output::OutputDir::check_free(dir).map_err(SweepStartError::Output)?;
            }
        }
        // A browser steps CPU models only.
        #[cfg(target_arch = "wasm32")]
        drop((gpu, output));
        Self::launch(SweepLaunch {
            entry,
            spec,
            plan,
            search_plan,
            options,
            #[cfg(not(target_arch = "wasm32"))]
            gpu,
            #[cfg(not(target_arch = "wasm32"))]
            output,
            #[cfg(not(target_arch = "wasm32"))]
            shard: henad_core::explore::plan::Shard::WHOLE,
            #[cfg(not(target_arch = "wasm32"))]
            resume: false,
        })
    }

    /// Resumes the sweep whose results the directory `dir` holds, running only the runs it lacks.
    ///
    /// The spec, source and shard come from the directory's manifest, and the sweep runs as
    /// [`crate::sweep::run_spec`] resumes one. Note that the execution settings come from `options` alone. The
    /// `[execution]` table the manifest records is not applied. `gpu` is the device a GPU model steps on, as in
    /// [`Self::start`].
    ///
    /// # Errors
    ///
    /// Returns [`SweepStartError`] when another sweep, search or merge is writing to `dir`, the manifest or its spec
    /// cannot be read, the spec names another model or cannot be planned, or the sweep's thread cannot start. A
    /// directory whose runs do not fit the plan, or a device the sweep cannot acquire, fails the sweep with a
    /// [`SweepEvent::Failed`].
    #[cfg(not(target_arch = "wasm32"))]
    pub fn resume_directory(
        entry: ModelEntry,
        gpu: Option<GpuContext>,
        dir: &std::path::Path,
        options: SweepRunOptions,
    ) -> Result<Self, SweepStartError> {
        use crate::output::manifest::Manifest;
        use crate::output::{MANIFEST_FILE, OutputDir};
        use crate::spec_file::SpecFile;

        OutputDir::check_unlocked(dir).map_err(SweepStartError::Output)?;
        let manifest = Manifest::read(&dir.join(MANIFEST_FILE)).map_err(SweepStartError::Manifest)?;
        let spec = SpecFile::from_json(&manifest.spec)
            .and_then(SpecFile::into_spec)
            .map_err(SweepStartError::Spec)?;
        let (plan, search_plan) = plan_for_start(&entry, &spec)?;
        let shard = manifest.shard.to_shard().ok_or(SweepStartError::Shard)?;
        let options = SweepRunOptions {
            spec_source: SpecSource::from(&manifest.spec_source),
            ..options
        };
        Self::launch(SweepLaunch {
            entry,
            spec,
            plan,
            search_plan,
            options,
            gpu,
            output: SweepOutput::Directory(dir.to_owned()),
            shard,
            resume: true,
        })
    }

    /// Plan of the sweep's configs and runs, or of a search's fixed values and actions alone.
    pub fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }

    /// Plan of a search, `None` for a sweep.
    ///
    /// A search's configs are chosen as it runs. Each arrives in a [`SweepEvent::SearchBatchTold`], and
    /// [`SearchPlan::replay`] rebuilds any of its runs.
    pub fn search_plan(&self) -> Option<&Arc<SearchPlan>> {
        self.search_plan.as_ref()
    }

    /// Holds every run at its next slice of steps.
    pub fn pause(&mut self) {
        if self.ended || self.control.is_paused() || self.control.is_aborted() {
            return;
        }
        self.clock.pause();
        self.control.pause();
        #[cfg(target_arch = "wasm32")]
        self.driver.send(SweepCommand::Pause);
    }

    /// Lets a paused sweep continue.
    pub fn resume(&mut self) {
        if !self.control.is_paused() {
            return;
        }
        self.clock.resume();
        self.control.resume();
        #[cfg(target_arch = "wasm32")]
        self.driver.send(SweepCommand::Resume);
    }

    /// Ends the sweep at its next slice of steps, paused runs included. The runs written so far stay in its files.
    pub fn abort(&mut self) {
        self.clock.resume();
        self.control.abort();
        #[cfg(target_arch = "wasm32")]
        self.driver.send(SweepCommand::Abort);
    }

    /// Returns whether the sweep is paused.
    pub fn is_paused(&self) -> bool {
        self.control.is_paused()
    }

    /// Returns whether the last event, [`SweepEvent::Finished`] or [`SweepEvent::Failed`], has been received.
    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Steps the sweep within a frame's budget in a browser. On native the sweep's thread runs itself.
    #[cfg(target_arch = "wasm32")]
    pub fn update(&mut self, dt: f64) {
        self.driver.update(dt);
    }

    /// Steps the sweep within a frame's budget in a browser. On native the sweep's thread runs itself.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn update(&mut self, _dt: f64) {}

    /// Returns the next event, or `None` when none is waiting.
    pub fn try_recv(&mut self) -> Option<SweepEvent> {
        let event = self.events.try_recv().ok()?;
        if matches!(event, SweepEvent::Finished(_) | SweepEvent::Failed(_)) {
            self.ended = true;
        }
        Some(event)
    }

    /// Returns the progress of the sweep as it stands.
    pub fn progress(&self) -> SweepProgress {
        let progress_state = self.shared_progress.lock();
        let phase = match progress_state.final_phase {
            Some(phase) => phase,
            None if self.control.is_paused() => SweepPhase::Paused,
            None if progress_state.runs_total.is_some() => SweepPhase::Running,
            None => SweepPhase::Planning,
        };
        let ended = progress_state.final_phase.is_some();
        let runs_total = progress_state.runs_total.unwrap_or_else(|| {
            self.search_plan
                .as_ref()
                .map_or_else(|| self.plan.run_count(), |search_plan| search_plan.run_count())
        });
        let runs_done = progress_state.runs_done;
        let elapsed = self.clock.elapsed(progress_state.ended_at.unwrap_or_else(Instant::now));
        let runs_waiting = if ended {
            0
        } else {
            self.active_runs.waiting_count() as u64
        };
        let active_runs = self.active_runs.list();
        let remaining = if ended || runs_done == 0 {
            None
        } else {
            remaining_time(elapsed, runs_total, runs_done + runs_waiting, &active_runs)
        };
        SweepProgress {
            phase,
            runs_total,
            runs_skipped: progress_state.runs_skipped,
            runs_done,
            runs_waiting,
            runs_failed: progress_state.runs_failed,
            elapsed,
            remaining,
            active_runs,
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn launch(launch: SweepLaunch) -> Result<Self, SweepStartError> {
        use crate::output::manifest::ManifestRuntime;
        use crate::search_run::{run_search_in_memory, run_search_into_directory};
        use crate::sweep::{SweepInputs, SweepOptions, run_in_memory, run_into_directory, sweep_device};
        use henad_compute::fault::catching;

        let (mut channel, events, shared_progress) = SweepChannel::open(&launch.options);
        let control = SweepControl::new();
        let active_runs = ActiveRuns::new();
        let plan = Arc::clone(&launch.plan);
        let search_plan = launch.search_plan.clone();
        let mut sweep_options = SweepOptions::new(launch.options.provenance.clone());
        sweep_options.concurrency = launch.options.concurrency;
        sweep_options.memory_budget = launch.options.memory_budget;
        sweep_options.gpu_memory_budget = launch.options.gpu_memory_budget;
        sweep_options.control = control.clone();
        sweep_options.shard = launch.shard;
        sweep_options.resume = launch.resume;
        sweep_options.retry_failed = launch.options.retry_failed;
        sweep_options.active_runs = Some(active_runs.clone());
        sweep_options.spec_source = launch.options.spec_source.clone();
        let thread = std::thread::Builder::new()
            .name("henad-sweep".to_owned())
            .spawn(move || {
                let SweepLaunch {
                    entry,
                    gpu,
                    spec,
                    plan,
                    search_plan,
                    output,
                    ..
                } = launch;
                let device = match catching(ACQUIRING_DEVICE, || sweep_device(&entry, gpu.as_ref())) {
                    Ok(Ok(device)) => device,
                    Ok(Err(error)) => return channel.fail(&error),
                    Err(fault) => return channel.fail(&fault),
                };
                let gpu = device.as_deref();
                let runtime = ManifestRuntime::new(gpu.and_then(GpuContext::runtime_info));
                let folder = match &output {
                    SweepOutput::Memory => None,
                    SweepOutput::Directory(dir) => Some(dir.as_path()),
                };
                let inputs = SweepInputs {
                    entry: &entry,
                    gpu,
                    runtime: &runtime,
                    spec: &spec,
                    source: &sweep_options.spec_source,
                    provenance: &sweep_options.provenance,
                    options: &sweep_options,
                    folder,
                    dry_run: false,
                };
                let ran = catching(RUNNING_SWEEP, || match (folder, search_plan) {
                    (None, None) => run_in_memory(&inputs, Some(plan), &mut channel),
                    (Some(dir), None) => run_into_directory(&inputs, Some(plan), dir, &mut channel),
                    (None, Some(search_plan)) => run_search_in_memory(&inputs, Some(search_plan), &mut channel),
                    (Some(dir), Some(search_plan)) => {
                        run_search_into_directory(&inputs, Some(search_plan), dir, &mut channel)
                    }
                });
                match ran {
                    Ok(Ok(record)) => channel.finish(record),
                    Ok(Err(error)) => channel.fail(&error),
                    Err(fault) => channel.fail(&fault),
                }
            })
            .map_err(SweepStartError::Spawn)?;
        Ok(Self {
            plan,
            search_plan,
            control,
            active_runs,
            events,
            shared_progress,
            clock: PauseClock::new(),
            ended: false,
            thread: Some(thread),
        })
    }

    #[cfg(target_arch = "wasm32")]
    fn launch(launch: SweepLaunch) -> Result<Self, SweepStartError> {
        let (channel, events, shared_progress) = SweepChannel::open(&launch.options);
        let plan = Arc::clone(&launch.plan);
        let search_plan = launch.search_plan.clone();
        let sweep = PumpedSweep::new(
            launch.entry,
            launch.spec,
            launch.plan,
            launch.search_plan,
            channel,
            launch.options,
        )?;
        let (control, active_runs) = (sweep.control().clone(), sweep.active_runs().clone());
        Ok(Self {
            plan,
            search_plan,
            control,
            active_runs,
            events,
            shared_progress,
            clock: PauseClock::new(),
            ended: false,
            driver: henad_compute::runner::Driver::spawn(sweep, |_| {}),
        })
    }
}

impl Drop for SweepRun {
    fn drop(&mut self) {
        self.control.abort();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(thread) = self.thread.take() {
            drop(thread.join());
        }
    }
}

/// Task a panic outside any run reports as its `during`.
#[cfg(not(target_arch = "wasm32"))]
const RUNNING_SWEEP: &str = "running the sweep";

/// Task a panic while the sweep acquires its device reports as its `during`.
#[cfg(not(target_arch = "wasm32"))]
const ACQUIRING_DEVICE: &str = "acquiring the sweep's GPU device";

/// Sweep to launch, planned.
///
/// A browser runs the whole plan of a CPU model into memory, so the fields that choose otherwise are native only.
struct SweepLaunch {
    entry: ModelEntry,
    spec: SweepSpec,
    plan: Arc<Plan>,
    search_plan: Option<Arc<SearchPlan>>,
    options: SweepRunOptions,
    #[cfg(not(target_arch = "wasm32"))]
    gpu: Option<GpuContext>,
    #[cfg(not(target_arch = "wasm32"))]
    output: SweepOutput,
    #[cfg(not(target_arch = "wasm32"))]
    shard: henad_core::explore::plan::Shard,
    #[cfg(not(target_arch = "wasm32"))]
    resume: bool,
}

/// Returns the plan of `spec` over `entry` that a handle starts, and for a search its search plan. For a search, the
/// first plan is the search plan's base.
///
/// # Errors
///
/// Returns [`SweepStartError`] when the spec names another model, or a browser a GPU model, or the spec cannot be
/// planned.
fn plan_for_start(
    entry: &ModelEntry,
    spec: &SweepSpec,
) -> Result<(Arc<Plan>, Option<Arc<SearchPlan>>), SweepStartError> {
    if spec.model != entry.id() {
        return Err(SweepStartError::ModelMismatch {
            spec_model: spec.model.clone(),
            entry_model: entry.id().to_owned(),
        });
    }
    if cfg!(target_arch = "wasm32") && entry.metadata().backend == Backend::Gpu {
        return Err(SweepStartError::GpuNeedsNative);
    }
    if spec.search.is_some() {
        let search_plan = SearchPlan::new(spec, &entry.schema()).map_err(SweepStartError::Search)?;
        return Ok((Arc::clone(search_plan.base()), Some(Arc::new(search_plan))));
    }
    let plan = spec.plan(&entry.schema()).map_err(SweepStartError::Plan)?;
    Ok((Arc::new(plan), None))
}

/// Time a sweep has run, pauses left out.
#[derive(Debug, Clone, Copy)]
struct PauseClock {
    start: Instant,
    /// Start of the pause in progress.
    paused_since: Option<Instant>,
    /// Length of the pauses that have ended.
    pause_total: Duration,
}

impl PauseClock {
    fn new() -> Self {
        Self {
            start: Instant::now(),
            paused_since: None,
            pause_total: Duration::ZERO,
        }
    }

    fn pause(&mut self) {
        self.paused_since.get_or_insert_with(Instant::now);
    }

    fn resume(&mut self) {
        if let Some(since) = self.paused_since.take() {
            self.pause_total += since.elapsed();
        }
    }

    /// Returns the time run up to `now`.
    fn elapsed(&self, now: Instant) -> Duration {
        let current_pause = self
            .paused_since
            .map_or(Duration::ZERO, |since| now.saturating_duration_since(since));
        now.saturating_duration_since(self.start)
            .saturating_sub(self.pause_total + current_pause)
    }
}

/// Returns the time left for `runs_total` runs after `elapsed`, at the pace of the `runs_finished` runs that finished
/// and the share of its ticks each run of `active_runs` has stepped.
///
/// Returns `None` before any run has made progress, and for a time too long for a [`Duration`].
fn remaining_time(
    elapsed: Duration,
    runs_total: u64,
    runs_finished: u64,
    active_runs: &[ActiveRun],
) -> Option<Duration> {
    let stepped: f64 = active_runs
        .iter()
        .filter(|active| active.end_tick > 0)
        .map(|active| (active.tick as f64 / active.end_tick as f64).min(1.0))
        .sum();
    let done = runs_finished as f64 + stepped;
    if done <= 0.0 {
        return None;
    }
    let left = (runs_total as f64 - done).max(0.0);
    Duration::try_from_secs_f64(elapsed.as_secs_f64() * left / done).ok()
}

/// Counts and stage of a sweep, written by its channel and read by its handle.
#[derive(Debug, Default)]
struct ProgressState {
    /// Runs the sweep runs, set once the sweep is planned.
    runs_total: Option<u64>,
    /// Runs a resumed directory held already, set once the sweep is planned.
    runs_skipped: u64,
    runs_done: u64,
    runs_failed: u64,
    /// Stage the sweep ended in, `None` while it runs.
    final_phase: Option<SweepPhase>,
    ended_at: Option<Instant>,
}

/// Progress state shared by a sweep's channel and its handle.
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedProgress(Arc<Mutex<ProgressState>>);

impl SharedProgress {
    fn lock(&self) -> std::sync::MutexGuard<'_, ProgressState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Sender of a sweep's events to its handle, and writer of the progress the handle reads.
pub(crate) struct SweepChannel {
    sender: Sender<SweepEvent>,
    shared_progress: SharedProgress,
    wake: Option<WakeFn>,
    series_budget: usize,
    /// Bytes of series the events have carried.
    series_bytes: usize,
    /// Whether a series has passed the budget. Every later run arrives without its series.
    series_budget_spent: bool,
}

impl SweepChannel {
    /// Returns a channel with the wake and series budget of `options`, the end its events arrive at, and the progress
    /// state it writes.
    pub(crate) fn open(options: &SweepRunOptions) -> (Self, Receiver<SweepEvent>, SharedProgress) {
        let (sender, events) = mpsc::channel();
        let shared_progress = SharedProgress::default();
        let channel = Self {
            sender,
            shared_progress: shared_progress.clone(),
            wake: options.wake.clone(),
            series_budget: options.series_budget,
            series_bytes: 0,
            series_budget_spent: false,
        };
        (channel, events, shared_progress)
    }

    /// Sends the record of a sweep that ran to its end or was aborted.
    pub(crate) fn finish(&self, record: SweepRecord) {
        {
            let mut progress_state = self.shared_progress.lock();
            progress_state.final_phase = Some(SweepPhase::Ended(record.report.end));
            progress_state.ended_at = Some(Instant::now());
        }
        self.send(SweepEvent::Finished(Box::new(record)));
    }

    /// Sends `error`, with its causes, as the end of a sweep that failed.
    pub(crate) fn fail(&self, error: &dyn std::error::Error) {
        {
            let mut progress_state = self.shared_progress.lock();
            progress_state.final_phase = Some(SweepPhase::Failed);
            progress_state.ended_at = Some(Instant::now());
        }
        self.send(SweepEvent::Failed(describe(error)));
    }

    /// Returns `outcome` with its series left out once the series have passed the budget, and whether it was.
    fn apply_series_budget(&mut self, outcome: &RunOutcome) -> (RunOutcome, bool) {
        let series = &outcome.series;
        let bytes = series.len() * (series.width() + 1) * size_of::<f64>();
        self.series_budget_spent |= self.series_bytes + bytes > self.series_budget;
        if !self.series_budget_spent {
            self.series_bytes += bytes;
            return (outcome.clone(), false);
        }
        let without_series = RunOutcome {
            run: outcome.run,
            run_key: outcome.run_key,
            status: outcome.status,
            stop_reason: outcome.stop_reason,
            ticks: outcome.ticks,
            population: outcome.population,
            build_ms: outcome.build_ms,
            wall_ms: outcome.wall_ms,
            reducers: outcome.reducers.clone(),
            series: SeriesBuffer::new(series.width()),
            note: outcome.note.clone(),
        };
        (without_series, !series.is_empty())
    }

    fn send(&self, event: SweepEvent) {
        // A handle that is gone takes no more events, and the sweep runs on to write its files.
        drop(self.sender.send(event));
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

impl Progress for SweepChannel {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        match event {
            ProgressEvent::Planned(outline) => {
                {
                    let mut progress_state = self.shared_progress.lock();
                    progress_state.runs_total = Some(outline.pending);
                    progress_state.runs_skipped = outline.skipped;
                }
                self.send(SweepEvent::Planned(Box::new((*outline).clone())));
            }
            ProgressEvent::Warned(warning) => self.send(SweepEvent::Warned((*warning).clone())),
            ProgressEvent::RunCommitted(outcome) => {
                {
                    let mut progress_state = self.shared_progress.lock();
                    progress_state.runs_done += 1;
                    if outcome.status.is_failure() {
                        progress_state.runs_failed += 1;
                    }
                }
                let (outcome, series_dropped) = self.apply_series_budget(outcome);
                self.send(SweepEvent::RunFinished {
                    outcome: Box::new(outcome),
                    series_dropped,
                });
            }
            ProgressEvent::SearchBatchTold(update) => {
                self.send(SweepEvent::SearchBatchTold(Arc::new((*update).clone())));
            }
            ProgressEvent::Progressed(_) | ProgressEvent::Ended(_) => {}
        }
    }
}

/// Returns `error` followed by each of its causes, joined by colons.
fn describe(error: &dyn std::error::Error) -> String {
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use henad_core::explore::outcome::PlannedRun;

    use super::remaining_time;
    use crate::exec::ActiveRun;

    /// Returns run `run_id` in progress at `tick` of `end_tick`.
    fn active(run_id: u64, tick: u64, end_tick: u64) -> ActiveRun {
        ActiveRun {
            run: PlannedRun {
                run_id,
                config_id: run_id,
                rep: 0,
                seed: 1,
            },
            tick,
            end_tick,
        }
    }

    #[test]
    fn the_remaining_time_counts_the_runs_in_progress() {
        let seconds = |remaining: Option<Duration>| remaining.expect("an estimate").as_secs_f64();
        assert!((seconds(remaining_time(Duration::from_secs(8), 10, 4, &[])) - 12.0).abs() < 1e-9);
        // Four runs started together, and one finished as the other three neared their end.
        let nearly_done = [active(1, 90, 100), active(2, 90, 100), active(3, 90, 100)];
        let estimate = seconds(remaining_time(Duration::from_secs(37), 8, 1, &nearly_done));
        assert!(
            (estimate - 43.0).abs() < 1e-6,
            "1 run and 2.7 of the next three took 37 s, and 4.3 runs are left, not {estimate}"
        );
        assert_eq!(
            remaining_time(Duration::from_secs(3), 10, 0, &[active(0, 0, 100)]),
            None
        );
        assert_eq!(
            remaining_time(Duration::from_secs(3), 10, 10, &[]),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn a_remaining_time_past_the_largest_duration_reads_as_none() {
        assert_eq!(remaining_time(Duration::from_secs(2), u64::MAX, 1, &[]), None);
    }
}
