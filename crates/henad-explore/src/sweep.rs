//! Sweeps that plan a spec against a model, run every planned run and write the results to a directory or to memory.

#[cfg(not(target_arch = "wasm32"))]
use std::borrow::Cow;
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::explore::measure::{MeasureError, MeasurePlan};
use henad_core::explore::outcome::PlannedRun;
#[cfg(not(target_arch = "wasm32"))]
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::{Plan, PlanError, PlanWarning, PlannedBlock, Shard};
use henad_core::explore::search::SearchReport;
use henad_core::explore::spec::SweepSpec;
use henad_core::metadata::Backend;
use henad_core::provenance::BuildInfo;

use crate::exec::{
    ActiveRuns, BatchEnd, Concurrency, ExecutionBudget, ExecutionError, ExecutionLayout, SweepControl, choose_layout,
    gpu_memory_budget,
};
#[cfg(not(target_arch = "wasm32"))]
use crate::exec::{Executor, RunRequest, RunSink};
#[cfg(not(target_arch = "wasm32"))]
use crate::handle::SweepOutput;
#[cfg(not(target_arch = "wasm32"))]
use crate::output::OutputWriter;
use crate::output::manifest::{
    BuildRole, FORMAT, FORMAT_VERSION, Manifest, ManifestBlock, ManifestColumns, ManifestDesignTable,
    ManifestExecution, ManifestMode, ManifestModel, ManifestPlan, ManifestRuntime, ManifestSearch, ManifestSeeds,
    ManifestSession, ManifestSpecSource, ManifestStatus, ManifestTimestamps, RecordedBuild, ResultCounts, now_unix_ms,
    rfc3339,
};
use crate::output::memory::SweepFiles;
use crate::output::resume::{ResumeError, ResumeScan};
use crate::output::runs_csv::column_names;
use crate::output::{OutputDir, OutputError, runs_csv, series_csv};
use crate::probe::{CapacityError, ProbeError, ProbeReport, TimedProbe, check_capacity};
#[cfg(not(target_arch = "wasm32"))]
use crate::progress::ProgressMeter;
use crate::progress::{Progress, ProgressEvent};
use crate::schema::{backend_name, schema_json};
use crate::search_run::{SearchOutline, SearchPlanError};
use crate::spec_file::{DesignTableFile, ExecutionTable, SpecFile};

/// Spec file a sweep was read from.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpecSource {
    /// Path of the file as given, `None` for a spec built from flags.
    pub path: Option<PathBuf>,
    /// Text of the file as written.
    pub toml: Option<String>,
    /// Design tables the file reads.
    pub tables: Vec<DesignTableFile>,
}

impl SpecSource {
    /// Returns the source of `file`, read from `path` as the text `toml`.
    pub(crate) fn loaded(path: &Path, toml: String, file: &SpecFile) -> Self {
        Self {
            path: Some(path.to_owned()),
            toml: Some(toml),
            tables: file.tables(),
        }
    }
}

impl From<&SpecSource> for ManifestSpecSource {
    fn from(source: &SpecSource) -> Self {
        Self {
            path: source.path.as_ref().map(|path| path.display().to_string()),
            toml: source.toml.clone(),
            tables: source
                .tables
                .iter()
                .map(|table| ManifestDesignTable {
                    path: table.path.display().to_string(),
                    fnv1a64: hex(table.fnv1a64),
                })
                .collect(),
        }
    }
}

impl From<&ManifestSpecSource> for SpecSource {
    /// Reads the source a manifest records. A table hash that is not 16 hexadecimal digits leaves its table out.
    fn from(source: &ManifestSpecSource) -> Self {
        Self {
            path: source.path.as_ref().map(PathBuf::from),
            toml: source.toml.clone(),
            tables: source
                .tables
                .iter()
                .filter_map(|table| {
                    Some(DesignTableFile {
                        path: PathBuf::from(&table.path),
                        fnv1a64: u64::from_str_radix(&table.fnv1a64, 16).ok()?,
                    })
                })
                .collect(),
        }
    }
}

/// Builds of Henad and of the host binary, and the host's command line, for the manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    engine: RecordedBuild,
    host: RecordedBuild,
    arguments: Vec<String>,
}

impl Provenance {
    /// Returns the provenance of a sweep that the host binary `host` runs with the command line `arguments`.
    ///
    /// The engine's build is [`ENGINE_BUILD`](crate::ENGINE_BUILD), as [`RecordedBuild::engine`] records it. A host
    /// passes `henad_core::build_info!()` from its own crate.
    pub fn new(host: BuildInfo, arguments: Vec<String>) -> Self {
        Self {
            engine: RecordedBuild::engine(),
            host: RecordedBuild::from(&host),
            arguments,
        }
    }

    /// Henad's build, as [`RecordedBuild::engine`] records it.
    pub fn engine(&self) -> &RecordedBuild {
        &self.engine
    }

    /// Build of the host binary.
    pub fn host(&self) -> &RecordedBuild {
        &self.host
    }

    /// Command line of the host binary.
    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    /// Returns the provenance with `engine` instead of Henad's own build.
    #[cfg(test)]
    pub(crate) fn with_engine(self, engine: RecordedBuild) -> Self {
        Self { engine, ..self }
    }
}

/// Settings of a sweep that never change its results.
///
/// [`Self::new`] returns the defaults, and a caller sets the other fields by assignment. A spec file's `[execution]`
/// table goes in through [`Self::apply_execution`], before the caller's own settings.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SweepOptions {
    /// Number of runs stepped at once, as CPU lanes or GPU tracks.
    pub concurrency: Concurrency,
    /// Host memory budget in bytes for all live runs together, `None` for no limit. The lanes are sized from the probed
    /// run, and a probed run larger than the budget leaves one lane.
    pub memory_budget: Option<u64>,
    /// Device memory budget in bytes for all live GPU runs together, `None` for the device's largest buffer. A run
    /// larger than the budget runs alone.
    pub gpu_memory_budget: Option<u64>,
    /// Switch that pauses or aborts the sweep from another thread.
    pub control: SweepControl,
    /// Share of the plan's runs that this sweep executes.
    pub shard: Shard,
    /// Whether to add to a directory that holds runs of the same plan, running only the runs it lacks.
    ///
    /// A directory that holds no results starts afresh, and so does a sweep held in memory.
    pub resume: bool,
    /// Whether a resume reruns the runs that ended on a fault. A run that timed out always runs again.
    ///
    /// Note that a sweep held in memory resumes nothing, so no run runs again.
    pub retry_failed: bool,
    /// Table that lists each run in progress, `None` when nothing watches the runs.
    pub(crate) active_runs: Option<ActiveRuns>,
    /// Spec file the sweep was read from, for the manifest.
    pub spec_source: SpecSource,
    /// Build of the host and its command line, for the manifest.
    pub provenance: Provenance,
}

impl SweepOptions {
    /// Returns the options at their defaults, recording `provenance` in every manifest the sweep writes.
    pub fn new(provenance: Provenance) -> Self {
        Self {
            concurrency: Concurrency::Auto,
            memory_budget: None,
            gpu_memory_budget: None,
            control: SweepControl::new(),
            shard: Shard::WHOLE,
            resume: false,
            retry_failed: false,
            active_runs: None,
            spec_source: SpecSource::default(),
            provenance,
        }
    }

    /// Copies the concurrency and memory settings of a spec's `[execution]` table into the options.
    ///
    /// Note that this overwrites all three. A caller with its own settings applies the table first and those settings
    /// after it.
    pub fn apply_execution(&mut self, execution: &ExecutionTable) {
        self.concurrency = execution.concurrent;
        self.memory_budget = execution.memory;
        self.gpu_memory_budget = execution.gpu_memory;
    }
}

/// Size and layout of a planned sweep or search.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepOutline {
    /// Id of the model.
    pub model: String,
    /// Backend the model runs on.
    pub backend: Backend,
    /// Number of configs in a sweep's plan, `None` for a search, whose budget is [`SearchOutline::max_evaluations`].
    pub configs: Option<u64>,
    /// Number of runs of each config, or of each evaluation of a search.
    pub replicates: u64,
    /// Number of runs in the whole plan, or in a search's whole budget.
    pub runs: u64,
    /// Blocks of the plan, each with the seed its design drew from.
    pub blocks: Vec<PlannedBlock>,
    /// Share of the plan's runs that this sweep executes.
    pub shard: Shard,
    /// Number of runs in the shard that the output directory already holds and a resume keeps.
    pub skipped: u64,
    /// Number of runs this sweep executes.
    pub pending: u64,
    /// Lanes or tracks the runs are spread over.
    pub layout: ExecutionLayout,
    /// Projected size in bytes of all live runs together.
    pub projected_bytes: u64,
    /// Number of rows that the pending runs add to `series.csv` once each reaches its final tick.
    pub series_rows: u64,
    /// Names of the stat columns of `series.csv`, before CSV escaping.
    pub stat_columns: Vec<String>,
    /// Names of the reducer columns of `runs.csv`, before CSV escaping.
    pub reducer_columns: Vec<String>,
    /// Whether the sweep is planned and probed alone, with nothing run.
    pub dry_run: bool,
    /// Budget and space of a search, `None` for a sweep.
    pub search: Option<SearchOutline>,
}

/// Warning about a sweep that still runs, but likely not as intended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepWarning {
    /// A warning of the plan.
    Plan(PlanWarning),
    /// A directory whose sessions so far ran the build `recorded` for `role`, and a build `current` that differs.
    ///
    /// A resume and a search resume compare the build that runs, `current`, against every build recorded by the
    /// sessions that wrote runs. A merge compares the builds of the lowest shard that records builds with those of the
    /// other shards. It sets `recorded` to a build of that shard and `current` to a build of another shard that matches
    /// none of that shard's builds, and sets `between_shards`. Two builds that
    /// [are treated as the same build](RecordedBuild::reads_as) are reported once, as builds that cannot be told apart.
    BuildChanged {
        /// Role of both builds, engine or model.
        role: BuildRole,
        /// Build that a session of the directory records, or a build of the lowest shard that records builds.
        recorded: Box<RecordedBuild>,
        /// Build that runs, or a build of another shard.
        current: Box<RecordedBuild>,
        /// Whether `current` is another shard's build, met by a merge, instead of the build that runs.
        between_shards: bool,
    },
    /// Runs of the plan that no merged directory holds.
    MissingRuns {
        /// Number of missing runs.
        count: u64,
        /// Lowest missing run ids in ascending order, at most [`MAX_LISTED_RUNS`](crate::merge::MAX_LISTED_RUNS).
        first: Vec<u64>,
    },
}

impl fmt::Display for SweepWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(warning) => warning.fmt(f),
            Self::BuildChanged {
                role,
                recorded,
                current,
                between_shards,
            } => {
                let (recorded_text, current_text) = (recorded.describe(), current.describe());
                let role_name = role.as_str();
                if recorded.reads_as(current) {
                    let subject = if *between_shards {
                        format!("the shards ran the same {role_name} build")
                    } else {
                        format!("the directory's runs so far came from this {role_name} build")
                    };
                    write!(
                        f,
                        "cannot tell whether {subject}, {current_text}. Neither build records a commit or a source \
                         hash"
                    )?;
                } else if *between_shards {
                    write!(
                        f,
                        "the shards ran different {role_name} builds: {recorded_text} in one, and {current_text} in \
                         another"
                    )?;
                } else {
                    write!(
                        f,
                        "the directory's runs so far came from the {role_name} build {recorded_text}, and this build \
                         is {current_text}"
                    )?;
                }
                if recorded_text == current_text
                    && let Some(difference) = recorded.crate_difference(current)
                {
                    write!(f, ". They differ in {difference}")?;
                }
                if *role == BuildRole::Model
                    && let Some(advice) = unidentified_model_advice(recorded, current)
                {
                    write!(f, ". {advice}")?;
                }
                Ok(())
            }
            Self::MissingRuns { count, first } => {
                let first: Vec<String> = first.iter().map(ToString::to_string).collect();
                let (runs, verb, pronoun) = if *count == 1 {
                    ("run", "is", "it")
                } else {
                    ("runs", "are", "them")
                };
                write!(
                    f,
                    "{count} {runs} of the plan {verb} missing, starting with {}. Resume the merged directory to run \
                     {pronoun}",
                    first.join(", ")
                )
            }
        }
    }
}

/// End of a sweep that did not fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SweepEnd {
    /// A dry run, planned and probed with nothing run.
    Planned,
    /// Every run of the shard is written.
    Complete,
    /// The control aborted the sweep. A resume runs the rest.
    Aborted,
    /// The GPU device was lost. A resume runs the rest.
    DeviceLost,
}

/// Result of a sweep that did not fail.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepReport {
    /// Outline of the sweep, as [`ProgressEvent::Planned`] carries it.
    pub outline: SweepOutline,
    /// End of the sweep.
    pub end: SweepEnd,
    /// Rows in `runs.csv` once the sweep ends, by status, the rows a resume kept included. Zero for a dry run.
    pub counts: ResultCounts,
    /// Time from the start of planning to the end of the sweep.
    pub elapsed: Duration,
    /// Directory the results were written to, `None` for a dry run or a sweep held in memory.
    pub output_dir: Option<PathBuf>,
}

/// Report, manifest and files of a sweep that ran to its end or was aborted.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepRecord {
    /// Report of the sweep, as [`ProgressEvent::Ended`] carries it.
    pub report: SweepReport,
    /// Manifest with the sweep's final status, as `manifest.json` holds it.
    pub manifest: Manifest,
    /// Files of a sweep held in memory, `None` for a sweep written to a directory.
    pub files: Option<SweepFiles>,
    /// Standing of a search at its end, `None` for a sweep.
    pub search: Option<SearchReport>,
}

/// A sweep that cannot run to its end.
///
/// The variants differ between targets, and a match outside this crate ends in a wildcard arm.
#[derive(Debug)]
#[non_exhaustive]
pub enum ExploreError {
    /// A spec that the model rejects.
    Plan(PlanError),
    /// Configs the device cannot host.
    Capacity(CapacityError),
    /// A probe build that failed.
    Probe(ProbeError),
    /// Reducers that do not bind to the columns of the probe build.
    Measure(MeasureError),
    /// A directory the sweep cannot resume into.
    Resume(ResumeError),
    /// Results that cannot be written.
    Output(OutputError),
    /// A batch of runs that cannot run.
    Execution(ExecutionError),
    /// A search spec that the model or the options reject.
    Search(SearchPlanError),
    /// A GPU model with no device, on a machine where no device can be acquired.
    #[cfg(not(target_arch = "wasm32"))]
    Device(crate::device::DeviceError),
}

impl fmt::Display for ExploreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(_) => f.write_str("cannot plan the sweep"),
            Self::Capacity(_) => f.write_str("the sweep does not fit this GPU"),
            Self::Probe(_) => f.write_str("cannot probe the model"),
            Self::Measure(_) => f.write_str("cannot bind the reducers to the model's stat columns"),
            Self::Resume(_) => f.write_str("cannot resume the sweep"),
            Self::Output(_) => f.write_str("cannot write the results"),
            Self::Execution(_) => f.write_str("cannot run the sweep"),
            Self::Search(_) => f.write_str("cannot plan the search"),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Device(_) => f.write_str("cannot acquire a GPU device for the sweep"),
        }
    }
}

impl std::error::Error for ExploreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Plan(error) => Some(error),
            Self::Capacity(error) => Some(error),
            Self::Probe(error) => Some(error),
            Self::Measure(error) => Some(error),
            Self::Resume(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Execution(error) => Some(error),
            Self::Search(error) => Some(error),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Device(error) => Some(error),
        }
    }
}

impl From<PlanError> for ExploreError {
    fn from(error: PlanError) -> Self {
        Self::Plan(error)
    }
}

impl From<CapacityError> for ExploreError {
    fn from(error: CapacityError) -> Self {
        Self::Capacity(error)
    }
}

impl From<ProbeError> for ExploreError {
    fn from(error: ProbeError) -> Self {
        Self::Probe(error)
    }
}

impl From<MeasureError> for ExploreError {
    fn from(error: MeasureError) -> Self {
        Self::Measure(error)
    }
}

impl From<ResumeError> for ExploreError {
    fn from(error: ResumeError) -> Self {
        Self::Resume(error)
    }
}

impl From<OutputError> for ExploreError {
    fn from(error: OutputError) -> Self {
        Self::Output(error)
    }
}

impl From<ExecutionError> for ExploreError {
    fn from(error: ExecutionError) -> Self {
        Self::Execution(error)
    }
}

impl From<SearchPlanError> for ExploreError {
    fn from(error: SearchPlanError) -> Self {
        Self::Search(error)
    }
}

/// Runs the sweep, or the search that a spec's `[search]` table describes, and blocks until it ends. Native only.
///
/// A sweep plans the spec, checks every config against the device, probes the first config without a fault and the
/// last config, and chooses a layout from the larger probe. It then writes the manifest with status `running`,
/// streams each run to `runs.csv` and `series.csv` in plan order, rebuilds `summary.csv` from `runs.csv`, and
/// replaces the manifest with its final status. Runs reach the output in plan order whatever order they finish in,
/// and a search's runs in the order that it requests them.
///
/// `gpu` is the device a GPU model runs on, shared with the host, its
/// [`FaultSink`](henad_compute::fault::FaultSink) included. Note that the sink holds one fault, and whichever side
/// reads it first takes it. A fault that the sweep takes first ends every live run, whichever side raised it, and a
/// fault that the host takes first leaves the runs going. A host that renders on its device passes `None`. When `gpu`
/// is `None`, a device for a GPU model is acquired once the spec is planned, sized to its [`ModelEntry::gpu_needs`].
///
/// The manifest records the adapter of the device the sweep runs on when its context carries
/// [`RuntimeInfo`](henad_compute::runtime_info::RuntimeInfo). A context acquired here carries it, and a host's own
/// context carries it once built with [`GpuContext::with_runtime_info`]. A bare [`SweepSpec`] carries no execution
/// settings. [`LoadedSpec`](crate::spec_file::LoadedSpec) keeps a spec file's `[execution]` table for
/// [`SweepOptions::apply_execution`].
///
/// With `options.resume`, a directory holding runs of the same plan keeps the runs that its [`ResumeScan`]
/// keeps, and the sweep runs the rest. Both tables then list every run in order of its id, as a sweep run in one go
/// would. A sweep held in memory starts afresh whatever `options.resume` and `options.retry_failed` say.
/// A run that faults is recorded with its status, and the sweep carries on. A sweep that fails once its manifest is
/// written marks the manifest `failed` when it can.
///
/// # Errors
///
/// Returns [`ExploreError`] when the spec cannot be planned, no device can be acquired, a config does not fit the
/// device, the probe build fails, the output directory holds results and is not resumed, another sweep, search or
/// merge is writing to it, the directory cannot be resumed, the results cannot be written, or a batch cannot run.
#[cfg(not(target_arch = "wasm32"))]
pub fn run_spec(
    model: &ModelEntry,
    gpu: Option<&GpuContext>,
    spec: &SweepSpec,
    output: SweepOutput,
    options: &SweepOptions,
    progress: &mut dyn Progress,
) -> Result<SweepRecord, ExploreError> {
    use crate::search_run::{run_search_in_memory, run_search_into_directory};

    let (planned, device) = plan_then_acquire(model, gpu, spec, crate::device::acquire_headless)?;
    let gpu = device.as_deref();
    let runtime = ManifestRuntime::new(gpu.and_then(GpuContext::runtime_info));
    let dir = match output {
        SweepOutput::Memory => None,
        SweepOutput::Directory(dir) => Some(dir),
    };
    let folder = dir.as_deref();
    let inputs = SweepInputs {
        entry: model,
        gpu,
        runtime: &runtime,
        spec,
        source: &options.spec_source,
        provenance: &options.provenance,
        options,
        folder,
        dry_run: false,
    };
    match (folder, planned) {
        (None, SpecPlan::Sweep(plan)) => run_in_memory(&inputs, Some(plan), progress),
        (Some(dir), SpecPlan::Sweep(plan)) => run_into_directory(&inputs, Some(plan), dir, progress),
        (None, SpecPlan::Search(plan)) => run_search_in_memory(&inputs, Some(plan), progress),
        (Some(dir), SpecPlan::Search(plan)) => run_search_into_directory(&inputs, Some(plan), dir, progress),
    }
}

/// Plan of a spec, for a sweep or a search.
#[cfg(not(target_arch = "wasm32"))]
enum SpecPlan {
    Sweep(Arc<Plan>),
    Search(Arc<crate::search_run::SearchPlan>),
}

#[cfg(not(target_arch = "wasm32"))]
impl SpecPlan {
    /// Plans `spec` against `entry`, as a search when it has a `[search]` table.
    fn new(entry: &ModelEntry, spec: &SweepSpec) -> Result<Self, ExploreError> {
        let schema = entry.schema();
        Ok(if spec.search.is_some() {
            Self::Search(Arc::new(crate::search_run::SearchPlan::new(spec, &schema)?))
        } else {
            Self::Sweep(Arc::new(spec.plan(&schema)?))
        })
    }
}

/// Plans `spec` and probes its configs as `--dry-run` does, writing nothing. Native only.
///
/// `gpu` is a device the host shares with the probe builds, its [`FaultSink`](henad_compute::fault::FaultSink)
/// included, as [`run_spec`] describes. When `gpu` is `None`, a device for a GPU model is acquired for its
/// [`ModelEntry::gpu_needs`] once the spec is planned, since a probe builds the model. With `folder` and without
/// `options.resume`, rejects a folder that holds results, as `--out` does. With both, reads the folder as a resume
/// would, counts the runs it would skip and run, and compares the recorded builds.
///
/// # Errors
///
/// Returns [`ExploreError`] when the spec cannot be planned, no device can be acquired, a config does not fit the
/// device, the probe build fails, `folder` holds results and is not resumed, or `folder` cannot be resumed.
#[cfg(not(target_arch = "wasm32"))]
pub fn plan_spec(
    model: &ModelEntry,
    gpu: Option<&GpuContext>,
    spec: &SweepSpec,
    folder: Option<&Path>,
    options: &SweepOptions,
    progress: &mut dyn Progress,
) -> Result<SweepReport, ExploreError> {
    use crate::search_run::SearchPreparation;

    let (planned, device) = plan_then_acquire(model, gpu, spec, crate::device::acquire_headless)?;
    let gpu = device.as_deref();
    let runtime = ManifestRuntime::new(gpu.and_then(GpuContext::runtime_info));
    let inputs = SweepInputs {
        entry: model,
        gpu,
        runtime: &runtime,
        spec,
        source: &options.spec_source,
        provenance: &options.provenance,
        options,
        folder,
        dry_run: true,
    };
    let report = match planned {
        SpecPlan::Search(plan) => {
            let preparation = SearchPreparation::new(&inputs, Some(plan), None)?;
            preparation.announce(&inputs, progress);
            preparation.report(SweepEnd::Planned, ResultCounts::default(), None)
        }
        SpecPlan::Sweep(plan) => {
            let preparation = SweepPreparation::new(&inputs, Some(plan), None)?;
            preparation.announce(&inputs, progress);
            preparation.report(SweepEnd::Planned, ResultCounts::default(), None)
        }
    };
    progress.report(&ProgressEvent::Ended(&report));
    Ok(report)
}

/// Plans `spec` against `model`, then returns the plan and the device a sweep of it runs on, acquired through
/// `acquire` for a GPU model when `gpu` is `None`.
///
/// A spec that the model rejects reports its own error before any device is acquired, on a machine without an adapter
/// as well.
///
/// # Errors
///
/// Returns [`ExploreError`] when the spec cannot be planned, and [`ExploreError::Device`] when a GPU model needs a
/// device and `acquire` fails.
#[cfg(not(target_arch = "wasm32"))]
fn plan_then_acquire<'a>(
    model: &ModelEntry,
    gpu: Option<&'a GpuContext>,
    spec: &SweepSpec,
    acquire: impl FnOnce(henad_compute::gpu::GpuNeeds) -> Result<GpuContext, crate::device::DeviceError>,
) -> Result<(SpecPlan, Option<Cow<'a, GpuContext>>), ExploreError> {
    let planned = SpecPlan::new(model, spec)?;
    let device = device_or_acquired(model, gpu, acquire)?;
    Ok((planned, device))
}

/// Returns the device a sweep of `entry` runs on: `gpu` for a GPU model, or a device acquired for its needs when
/// `gpu` is `None`.
///
/// Returns `None` for a CPU model, whatever `gpu` is. A CPU sweep uses no device, and its manifest records no
/// adapter.
///
/// # Errors
///
/// Returns [`ExploreError::Device`] when a GPU model needs a device and no device can be acquired.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn sweep_device<'a>(
    entry: &ModelEntry,
    gpu: Option<&'a GpuContext>,
) -> Result<Option<Cow<'a, GpuContext>>, ExploreError> {
    device_or_acquired(entry, gpu, crate::device::acquire_headless)
}

/// Returns the device a sweep of `entry` runs on, as [`sweep_device`] does with `acquire` instead of
/// [`acquire_headless`](crate::device::acquire_headless).
///
/// # Errors
///
/// Returns [`ExploreError::Device`] when a GPU model needs a device and `acquire` fails.
#[cfg(not(target_arch = "wasm32"))]
fn device_or_acquired<'a>(
    entry: &ModelEntry,
    gpu: Option<&'a GpuContext>,
    acquire: impl FnOnce(henad_compute::gpu::GpuNeeds) -> Result<GpuContext, crate::device::DeviceError>,
) -> Result<Option<Cow<'a, GpuContext>>, ExploreError> {
    match (entry.gpu_needs(), gpu) {
        (None, _) => Ok(None),
        (Some(_), Some(ctx)) => Ok(Some(Cow::Borrowed(ctx))),
        (Some(needs), None) => {
            let ctx = acquire(needs).map_err(ExploreError::Device)?;
            Ok(Some(Cow::Owned(ctx)))
        }
    }
}

/// Runs the plan of `inputs.spec` into the directory `output_dir`, as [`run_spec`] does, and returns its record.
///
/// `plan`, when given, is the plan of `inputs.spec`, and the spec is planned here otherwise.
///
/// # Errors
///
/// Returns the errors of [`run_spec`].
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_into_directory(
    inputs: &SweepInputs<'_>,
    plan: Option<Arc<Plan>>,
    output_dir: &Path,
    progress: &mut dyn Progress,
) -> Result<SweepRecord, ExploreError> {
    let preparation = SweepPreparation::new(inputs, plan, None)?;
    preparation.announce(inputs, progress);
    let record = preparation.write_directory(inputs, output_dir, progress)?;
    progress.report(&ProgressEvent::Ended(&record.report));
    Ok(record)
}

/// Runs the plan of `inputs.spec`, holding its four files in memory, and returns its record.
///
/// `plan`, when given, is the plan of `inputs.spec`, and the spec is planned here otherwise. The files hold the bytes
/// a directory would. The caller passes `inputs` with no `folder`.
///
/// # Errors
///
/// Returns the errors of [`run_spec`] that do not come from a directory.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_in_memory(
    inputs: &SweepInputs<'_>,
    plan: Option<Arc<Plan>>,
    progress: &mut dyn Progress,
) -> Result<SweepRecord, ExploreError> {
    use crate::output::memory::memory_writer;

    let preparation = SweepPreparation::new(inputs, plan, None)?;
    preparation.announce(inputs, progress);
    let mut manifest = preparation.manifest(inputs)?;
    let writer = memory_writer(
        &preparation.plan,
        inputs.entry.param_descriptors(),
        &preparation.measure,
    )?;
    let (end, writer) = preparation.run_pending(inputs, writer, progress)?;
    let counts = writer.counts();
    let end = finish_manifest(&mut manifest, end, counts);
    let files = SweepFiles::assemble(writer, &manifest)?;
    let record = SweepRecord {
        report: preparation.report(end, counts, None),
        manifest,
        files: Some(files),
        search: None,
    };
    progress.report(&ProgressEvent::Ended(&record.report));
    Ok(record)
}

/// Model, device, spec and records of one sweep, and the host's settings.
pub(crate) struct SweepInputs<'a> {
    pub(crate) entry: &'a ModelEntry,
    pub(crate) gpu: Option<&'a GpuContext>,
    pub(crate) runtime: &'a ManifestRuntime,
    pub(crate) spec: &'a SweepSpec,
    pub(crate) source: &'a SpecSource,
    pub(crate) provenance: &'a Provenance,
    pub(crate) options: &'a SweepOptions,
    /// Directory the results go in or a dry run reads, `None` for a sweep held in memory.
    pub(crate) folder: Option<&'a Path>,
    /// Whether the sweep is planned and probed and stops there, writing nothing.
    pub(crate) dry_run: bool,
}

/// A sweep planned, checked and probed, with its layout chosen.
pub(crate) struct SweepPreparation {
    plan: Arc<Plan>,
    probe: ProbeReport,
    measure: Arc<MeasurePlan>,
    /// Runs of the shard that this sweep executes, in plan order.
    pending: Vec<PlannedRun>,
    /// Directory the sweep resumes, `None` for a sweep that starts afresh.
    resumed: Option<ResumeScan>,
    /// Directory a resume holds locked from before its scan until its last write, `None` for a dry run or a sweep
    /// that starts afresh.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "a sweep in a browser writes no directory")
    )]
    locked: Option<OutputDir>,
    outline: SweepOutline,
    /// Clock reading when planning started.
    started: Instant,
    started_unix_ms: u64,
}

impl SweepPreparation {
    /// Checks and probes `plan`, the plan of `inputs.spec`, and chooses its layout. With no `plan`, the spec is
    /// planned first. `probe`, when given, replaces the report that [`ProbeReport::for_plan`] returns for the plan,
    /// and its clock readings replace the ones taken on entry.
    pub(crate) fn new(
        inputs: &SweepInputs<'_>,
        plan: Option<Arc<Plan>>,
        probe: Option<TimedProbe>,
    ) -> Result<Self, ExploreError> {
        let (entry, gpu, options) = (inputs.entry, inputs.gpu, inputs.options);
        debug_assert!(
            inputs.spec.search.is_none(),
            "a spec with a [search] table runs as a search"
        );
        let (started, started_unix_ms, probe) = match probe {
            Some(timed) => (timed.started, timed.started_unix_ms, Some(timed.report)),
            None => (Instant::now(), now_unix_ms(), None),
        };
        let plan = match plan {
            Some(plan) => plan,
            None => Arc::new(inputs.spec.plan(&entry.schema())?),
        };
        let resume_dir = inputs
            .folder
            .filter(|output_dir| options.resume && OutputDir::holds_results(output_dir));
        let locked = match (resume_dir, inputs.folder) {
            (None, Some(output_dir)) => {
                OutputDir::check_free(output_dir)?;
                None
            }
            // The directory is locked before the scan and held until the last write. Otherwise another writer could
            // change the tables between the two.
            (Some(output_dir), _) if !inputs.dry_run => Some(OutputDir::open(output_dir)?),
            _ => None,
        };
        if let Some(ctx) = gpu {
            check_capacity(entry, &plan, &ctx.device.limits())?;
        }
        let probe = match probe {
            Some(probe) => probe,
            None => ProbeReport::for_plan(entry, gpu, &plan)?,
        };
        let measure = MeasurePlan::new(plan.run_settings(), plan.measure_settings(), probe.columns.clone())?;
        let resumed = resume_dir
            .map(|output_dir| {
                let columns = column_names(entry.param_descriptors(), plan.actions(), measure.reducers().names());
                ResumeScan::read(
                    output_dir,
                    &plan,
                    options.shard,
                    options.retry_failed,
                    &runs_csv::header_line(&columns),
                    &series_csv::header_line(measure.columns()),
                )
            })
            .transpose()?;
        let pending: Vec<PlannedRun> = plan
            .runs_in_shard(options.shard)
            .filter(|run| {
                resumed
                    .as_ref()
                    .is_none_or(|scan| !scan.finished().contains(&run.run_id))
            })
            .collect();

        // The layout is sized from the first config that builds without a fault or the last config, whichever holds
        // more.
        let last_probe = ProbeReport::for_last_config(entry, gpu, &plan, &probe);
        let sizing_probe = match &last_probe {
            Some(last_probe) if last_probe.footprint() > probe.footprint() => last_probe,
            _ => &probe,
        };
        let pending_count = pending.len() as u64;
        let (layout, projected_bytes) = sized_layout(entry, gpu, options, sizing_probe, pending_count)?;
        let outline = SweepOutline {
            model: entry.id().to_owned(),
            backend: entry.metadata().backend,
            configs: Some(plan.configs().len() as u64),
            replicates: plan.replicates(),
            runs: plan.run_count(),
            blocks: plan.blocks().to_vec(),
            shard: options.shard,
            skipped: resumed.as_ref().map_or(0, |scan| scan.finished().len() as u64),
            pending: pending_count,
            layout,
            projected_bytes,
            series_rows: pending_count.saturating_mul(measure.series_row_count()),
            stat_columns: (0..measure.columns().len())
                .map(|column| measure.columns().name(column).to_owned())
                .collect(),
            reducer_columns: measure.reducers().names().to_vec(),
            dry_run: inputs.dry_run,
            search: None,
        };
        Ok(Self {
            plan,
            probe,
            measure: Arc::new(measure),
            pending,
            resumed,
            locked,
            outline,
            started,
            started_unix_ms,
        })
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn plan(&self) -> &Arc<Plan> {
        &self.plan
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn measure(&self) -> &Arc<MeasurePlan> {
        &self.measure
    }

    /// Runs this sweep executes, in plan order.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn pending(&self) -> &[PlannedRun] {
        &self.pending
    }

    /// Reports the outline, then each warning of the plan and of a resume under a build that differs from the engine
    /// or model build of `inputs`.
    pub(crate) fn announce(&self, inputs: &SweepInputs<'_>, progress: &mut dyn Progress) {
        progress.report(&ProgressEvent::Planned(&self.outline));
        for warning in self.warnings(inputs.provenance, inputs.entry) {
            progress.report(&ProgressEvent::Warned(&warning));
        }
    }

    /// Returns the warnings of the plan, and of a resume under a build that differs from the engine build of
    /// `provenance` or the model build of `entry`.
    fn warnings(&self, provenance: &Provenance, entry: &ModelEntry) -> Vec<SweepWarning> {
        let mut warnings: Vec<SweepWarning> = self.plan.warnings().iter().cloned().map(SweepWarning::Plan).collect();
        if let Some(scan) = &self.resumed {
            warnings.extend(build_warnings(&scan.recorded, provenance, entry));
        }
        warnings
    }

    /// Writes the manifest with status `running`, runs every pending run into `output_dir` and replaces the manifest
    /// with the sweep's final status.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_directory(
        &self,
        inputs: &SweepInputs<'_>,
        output_dir: &Path,
        progress: &mut dyn Progress,
    ) -> Result<SweepRecord, ExploreError> {
        let created;
        let dir = if let Some(dir) = &self.locked {
            dir
        } else {
            created = OutputDir::create(output_dir)?;
            &created
        };
        let mut manifest = self.manifest(inputs)?;
        dir.write_manifest(&manifest)?;
        let (end, counts) = match self.run_into(dir, inputs, progress) {
            Ok(finished) => finished,
            Err(error) => {
                manifest.fail(now_unix_ms());
                // The sweep's own error is the one to report. A manifest that cannot be written says `running`.
                drop(dir.write_manifest(&manifest));
                return Err(error);
            }
        };
        let end = finish_manifest(&mut manifest, end, counts);
        dir.write_manifest(&manifest)?;
        Ok(SweepRecord {
            report: self.report(end, counts, Some(output_dir.to_owned())),
            manifest,
            files: None,
            search: None,
        })
    }

    /// Repairs a resumed directory, runs every pending run into `dir`, puts the tables in run order and rebuilds the
    /// summary.
    ///
    /// Returns the end of the batch and the counts of the rows `runs.csv` holds.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_into(
        &self,
        dir: &OutputDir,
        inputs: &SweepInputs<'_>,
        progress: &mut dyn Progress,
    ) -> Result<(BatchEnd, ResultCounts), ExploreError> {
        if let Some(scan) = &self.resumed {
            scan.repair(dir)?;
        }
        let params = inputs.entry.param_descriptors();
        let writer = if self.resumed.is_some() {
            dir.append_writer(&self.plan, params)?
        } else {
            dir.open_writer(&self.plan, params, &self.measure)?
        };
        let (end, writer) = self.run_pending(inputs, writer, progress)?;
        let written = writer.counts();
        writer.finish().map_err(|source| OutputError::Write {
            path: dir.path().to_owned(),
            source,
        })?;
        let kept = self.resumed.as_ref().map(ResumeScan::counts).unwrap_or_default();
        let last_kept = self.resumed.as_ref().and_then(|scan| scan.finished().last().copied());
        if self.pending.first().is_some_and(|run| Some(run.run_id) < last_kept) {
            dir.order_tables()?;
        }
        dir.write_summary()?;
        Ok((end, kept + written))
    }

    /// Runs every pending run and commits each to `writer` in plan order, reporting each to `progress`.
    ///
    /// Returns the end of the batch and the writer.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_pending<W: Write>(
        &self,
        inputs: &SweepInputs<'_>,
        writer: OutputWriter<W>,
        progress: &mut dyn Progress,
    ) -> Result<(BatchEnd, OutputWriter<W>), ExploreError> {
        let options = inputs.options;
        let executor = Executor::new(
            inputs.entry,
            inputs.gpu,
            Arc::clone(&self.measure),
            self.outline.layout,
            options.control.clone(),
        )?
        .with_timeout(self.plan.run_settings().timeout)
        .with_active_runs(options.active_runs.clone())
        .with_gpu_memory_budget(options.gpu_memory_budget);
        let requests: Vec<RunRequest<'_>> = self
            .pending
            .iter()
            .map(|&run| RunRequest::planned(&self.plan, run))
            .collect();
        let mut sink = SweepSink {
            writer,
            progress,
            meter: ProgressMeter::new(self.outline.pending),
        };
        let end = executor.run_batch(&requests, &mut sink)?;
        Ok((end, sink.writer))
    }

    /// Returns the manifest of the sweep while it runs.
    ///
    /// A resume keeps the start time, sessions and merged shards the directory recorded.
    ///
    /// # Errors
    ///
    /// Returns [`OutputError::Manifest`] when the spec cannot be written as JSON.
    pub(crate) fn manifest(&self, inputs: &SweepInputs<'_>) -> Result<Manifest, OutputError> {
        running_manifest(
            inputs,
            &ManifestParts {
                mode: ManifestMode::Sweep,
                plan: &self.plan,
                probe: &self.probe,
                outline: &self.outline,
                manifest_plan: self.manifest_plan(),
                started_unix_ms: self.started_unix_ms,
                recorded: self.resumed.as_ref().map(|scan| &scan.recorded),
                search: None,
            },
        )
    }

    /// Returns the record of the plan's hashes, counts and blocks.
    fn manifest_plan(&self) -> ManifestPlan {
        ManifestPlan {
            plan_hash: hex(self.plan.plan_hash()),
            results_fingerprint: hex(self.plan.results_fingerprint()),
            configs: self.outline.configs,
            replicates: self.outline.replicates,
            runs: self.outline.runs,
            blocks: self
                .plan
                .blocks()
                .iter()
                .map(|block| ManifestBlock {
                    design: block.design.as_str().to_owned(),
                    configs: block.configs.end - block.configs.start,
                    design_seed: block.design_seed,
                })
                .collect(),
        }
    }

    pub(crate) fn report(&self, end: SweepEnd, counts: ResultCounts, output_dir: Option<PathBuf>) -> SweepReport {
        SweepReport {
            outline: self.outline.clone(),
            end,
            counts,
            elapsed: self.started.elapsed(),
            output_dir,
        }
    }
}

/// Returns the layout for `runs` runs of `entry` sized from `sizing_probe`, and the bytes the live runs are projected
/// to hold.
///
/// Note that a probe sizes its buffers to rayon's global pool, and a run in a lane to the lane's pool. When the
/// layout splits the workers into lanes, the probe is built again on a pool as wide as a lane and the lanes are
/// budgeted from that build.
///
/// # Errors
///
/// Returns [`ExploreError::Probe`] when the probe cannot be built again on a lane's pool.
pub(crate) fn sized_layout(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    options: &SweepOptions,
    sizing_probe: &ProbeReport,
    runs: u64,
) -> Result<(ExecutionLayout, u64), ExploreError> {
    let backend = entry.metadata().backend;
    let detected = ExecutionBudget::detect();
    let unbudgeted = choose_layout(options.concurrency, &detected, backend, sizing_probe, runs);
    let rebuilt = if unbudgeted.cpu_lanes > 1 && unbudgeted.threads_per_lane != detected.workers {
        Some(sizing_probe.rebuilt_on(entry, unbudgeted.threads_per_lane)?)
    } else {
        None
    };
    let resources = ExecutionBudget {
        memory_budget: options.memory_budget,
        gpu_memory_budget: gpu.map(|ctx| gpu_memory_budget(options.gpu_memory_budget, ctx)),
        ..detected
    };
    let lane_probe = rebuilt.as_ref().unwrap_or(sizing_probe);
    let layout = choose_layout(options.concurrency, &resources, backend, lane_probe, runs);
    let run_probe = if layout.cpu_lanes > 1 { lane_probe } else { sizing_probe };
    Ok((layout, layout.projected_bytes(run_probe)))
}

/// Returns the advice for a model build that records neither a commit nor a source hash, checking the build that
/// runs first, or `None` when `recorded` and `current` both record a commit or a source hash.
///
/// An entry that was never inserted into a [`ModelSet`](henad_compute::entry::ModelSet) records no build at all, and
/// a build script alone cannot provide a build.
fn unidentified_model_advice(recorded: &RecordedBuild, current: &RecordedBuild) -> Option<&'static str> {
    let unidentified = [current, recorded].into_iter().find(|build| !build.is_identified())?;
    Some(if unidentified.package.is_empty() {
        "The model's entry records no build. Insert it into a `ModelSet` built with `henad::build_info!()` in a crate \
         whose build script calls `henad_build::stamp_commit()`"
    } else {
        "The model's build records no commit or source hash. Call `henad_build::stamp_commit()` in the build script \
         of the crate that registers it"
    })
}

/// Returns a [`SweepWarning::BuildChanged`] for each build that [`Manifest::recorded_builds`] lists for `recorded` and
/// differs from the engine build of `provenance`, or from the build that registered `entry`.
pub(crate) fn build_warnings(recorded: &Manifest, provenance: &Provenance, entry: &ModelEntry) -> Vec<SweepWarning> {
    let model = RecordedBuild::from(entry.source());
    [(BuildRole::Engine, provenance.engine()), (BuildRole::Model, &model)]
        .into_iter()
        .flat_map(|(role, current)| {
            recorded
                .recorded_builds(role)
                .into_iter()
                .filter(|build| !build.same_build(current))
                .map(move |build| SweepWarning::BuildChanged {
                    role,
                    recorded: Box::new(build),
                    current: Box::new(current.clone()),
                    between_shards: false,
                })
        })
        .collect()
}

/// Parts of a manifest that differ between a sweep and a search.
pub(crate) struct ManifestParts<'a> {
    pub(crate) mode: ManifestMode,
    /// Plan the runs come from. For a search, the plan of its fixed values alone.
    pub(crate) plan: &'a Plan,
    pub(crate) probe: &'a ProbeReport,
    pub(crate) outline: &'a SweepOutline,
    pub(crate) manifest_plan: ManifestPlan,
    pub(crate) started_unix_ms: u64,
    /// Manifest of the directory a resume adds to, `None` for a fresh start.
    pub(crate) recorded: Option<&'a Manifest>,
    pub(crate) search: Option<ManifestSearch>,
}

/// Returns the manifest, with status `running`, of the sweep or search that `inputs` and `parts` describe.
///
/// A resume keeps the start time, sessions and merged shards the directory recorded.
///
/// # Errors
///
/// Returns [`OutputError::Manifest`] when the spec cannot be written as JSON.
pub(crate) fn running_manifest(inputs: &SweepInputs<'_>, parts: &ManifestParts<'_>) -> Result<Manifest, OutputError> {
    let (entry, provenance, options) = (inputs.entry, inputs.provenance, inputs.options);
    let backend = backend_name(entry.metadata().backend).to_owned();
    let mut spec_file = SpecFile::from(inputs.spec);
    spec_file.execution = ExecutionTable {
        concurrent: options.concurrency,
        memory: options.memory_budget,
        gpu_memory: options.gpu_memory_budget,
    };
    let outline = parts.outline;
    let layout = outline.layout;
    let seeds = parts.plan.seed_settings();
    let spec = serde_json::to_value(&spec_file).map_err(OutputError::Manifest)?;
    let session = ManifestSession {
        started: rfc3339(parts.started_unix_ms),
        commit: provenance.engine().commit.clone(),
        skipped: outline.skipped,
        ran: 0,
        engine: Some(provenance.engine().clone()),
        host: Some(provenance.host().clone()),
        model_source: Some(RecordedBuild::from(entry.source())),
    };
    let (started_unix_ms, sessions, merged_shards) = match parts.recorded {
        Some(recorded) => {
            let mut recorded = recorded.clone();
            recorded.record_session_engines();
            let status = recorded.status;
            let mut sessions = recorded.sessions;
            // A session whose process ended before `Manifest::finish` never had its runs counted.
            if matches!(status, ManifestStatus::Running | ManifestStatus::Failed)
                && let Some(last) = sessions.last_mut()
            {
                last.ran = outline.skipped.saturating_sub(last.skipped);
            }
            sessions.push(session);
            (recorded.timestamps.started_unix_ms, sessions, recorded.merged_shards)
        }
        None => (parts.started_unix_ms, vec![session], None),
    };
    Ok(Manifest {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        mode: parts.mode,
        status: ManifestStatus::Running,
        engine: provenance.engine().clone(),
        model: ManifestModel {
            id: entry.id().to_owned(),
            name: entry.name().to_owned(),
            backend: backend.clone(),
            schema_hash: hex(parts.plan.schema_hash()),
            schema: schema_json(entry, Some(parts.probe)),
            replays_exactly: entry.metadata().replays_exactly,
        },
        spec,
        spec_source: inputs.source.into(),
        argv: provenance.arguments().to_vec(),
        plan: parts.manifest_plan.clone(),
        seeds: ManifestSeeds {
            root: seeds.root,
            scheme: seeds.scheme.as_str().to_owned(),
            formula: seeds.scheme.formula().to_owned(),
        },
        columns: ManifestColumns {
            stats: outline.stat_columns.clone(),
            reducers: outline.reducer_columns.clone(),
        },
        shard: options.shard.into(),
        execution: ManifestExecution {
            backend,
            concurrency: options.concurrency.to_string(),
            cpu_lanes: layout.cpu_lanes,
            threads_per_lane: layout.threads_per_lane,
            gpu_tracks: layout.gpu_tracks,
            projected_bytes: outline.projected_bytes,
            memory_budget: options.memory_budget,
            gpu_memory_budget: options.gpu_memory_budget,
        },
        runtime: inputs.runtime.clone(),
        timestamps: ManifestTimestamps::started_at(started_unix_ms),
        sessions,
        results: None,
        merged_shards,
        search: parts.search.clone(),
    })
}

/// Marks `manifest` as ended after a batch that ended as `end` with `counts` rows written, and returns the end of the
/// sweep.
pub(crate) fn finish_manifest(manifest: &mut Manifest, end: BatchEnd, counts: ResultCounts) -> SweepEnd {
    let (status, end) = match end {
        BatchEnd::Complete => (ManifestStatus::Complete, SweepEnd::Complete),
        BatchEnd::Aborted => (ManifestStatus::Aborted, SweepEnd::Aborted),
        BatchEnd::DeviceLost => (ManifestStatus::Incomplete, SweepEnd::DeviceLost),
    };
    manifest.finish(status, counts, now_unix_ms());
    end
}

/// Sink that writes each run to the output and reports the sweep's progress.
#[cfg(not(target_arch = "wasm32"))]
struct SweepSink<'s, W: Write> {
    writer: OutputWriter<W>,
    progress: &'s mut dyn Progress,
    meter: ProgressMeter,
}

#[cfg(not(target_arch = "wasm32"))]
impl<W: Write> RunSink for SweepSink<'_, W> {
    fn commit(&mut self, outcome: RunOutcome) -> io::Result<()> {
        self.writer.write_run(&outcome)?;
        self.progress.report(&ProgressEvent::RunCommitted(&outcome));
        Ok(())
    }

    fn finished(&mut self, outcome: &RunOutcome) {
        if let Some(update) = self.meter.record_finished_run(outcome.status) {
            self.progress.report(&ProgressEvent::Progressed(update));
        }
    }
}

/// Returns `hash` as 16 hexadecimal digits.
pub(crate) fn hex(hash: u64) -> String {
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;
    use std::path::PathBuf;

    use henad_core::explore::design::DesignKind;
    use henad_core::explore::factor::{FactorSpec, LevelSpec};
    use henad_core::explore::spec::{BlockSpec, SweepSpec};

    use std::path::Path;

    use henad_compute::entry::{ModelEntry, register_grid_model};
    use henad_core::provenance::BuildInfo;
    use henad_models::game_of_life::GameOfLifeModel;

    use super::{
        ExploreError, SpecSource, SweepEnd, SweepOptions, SweepOutline, SweepReport, SweepWarning, plan_spec,
        plan_then_acquire, run_spec,
    };
    use crate::device::DeviceError;
    use crate::exec::Concurrency;
    use crate::handle::SweepOutput;
    use crate::output::manifest::{BuildRole, Manifest, ManifestStatus, RecordedBuild};
    use crate::output::{MANIFEST_FILE, OutputError, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
    use crate::probe::ProbeReport;
    use crate::progress::{NoProgress, Progress, ProgressEvent};
    use crate::spec_file::SpecFile;
    use crate::tests::support::{ScratchDir, entry, provenance};

    /// Progress that keeps one line per event, leaving out the updates that depend on the clock.
    #[derive(Debug, Default)]
    struct Recorded(Vec<String>);

    impl Progress for Recorded {
        fn report(&mut self, event: &ProgressEvent<'_>) {
            let line = match event {
                ProgressEvent::Planned(outline) => format!("planned {}", outline.runs),
                ProgressEvent::Warned(warning) => format!("warned {warning}"),
                ProgressEvent::RunCommitted(outcome) => format!("run {}", outcome.run.run_id),
                ProgressEvent::Progressed(_) | ProgressEvent::SearchBatchTold(_) => return,
                ProgressEvent::Ended(report) => format!("ended {:?}", report.end),
            };
            self.0.push(line);
        }
    }

    /// Returns 2 configs of 2 replicates of SIR on a 12 by 12 grid, 6 steps each.
    fn small_sweep() -> SweepSpec {
        let mut spec = SweepSpec::new("sir");
        spec.fixed = vec![
            ("grid_width".to_owned(), "12".to_owned()),
            ("grid_height".to_owned(), "12".to_owned()),
        ];
        spec.run.steps = 6;
        spec.run.replicates = 2;
        spec.seeds.root = 3;
        spec.blocks = vec![BlockSpec {
            design: DesignKind::Factorial,
            factors: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Values(vec!["0.2".to_owned(), "0.4".to_owned()]),
            )],
            design_seed: None,
        }];
        spec
    }

    fn options() -> SweepOptions {
        SweepOptions::new(provenance())
    }

    /// Runs `spec` over `entry` into `output_dir` with `options`, reporting to `progress`.
    fn run_into(
        entry: &ModelEntry,
        spec: &SweepSpec,
        output_dir: &Path,
        options: &SweepOptions,
        progress: &mut dyn Progress,
    ) -> Result<SweepReport, ExploreError> {
        run_spec(
            entry,
            None,
            spec,
            SweepOutput::Directory(output_dir.to_owned()),
            options,
            progress,
        )
        .map(|record| record.report)
    }

    #[test]
    fn a_dry_run_plans_and_probes_and_writes_nothing() {
        let sir = entry("sir", None);
        let scratch = ScratchDir::new("dry-run");
        let mut progress = Recorded::default();
        let report = plan_spec(
            &sir,
            None,
            &small_sweep(),
            Some(scratch.path()),
            &options(),
            &mut progress,
        )
        .expect("the dry run plans");
        assert_eq!(report.end, SweepEnd::Planned);
        assert_eq!(
            (report.outline.configs, report.outline.runs, report.outline.series_rows),
            (Some(2), 4, 4 * 7)
        );
        assert!(report.outline.dry_run && report.outline.projected_bytes > 0);
        assert_eq!((report.counts.rows, report.output_dir), (0, None));
        assert!(!scratch.path().exists(), "a dry run writes nothing");
        assert_eq!(progress.0, ["planned 4", "ended Planned"]);
    }

    /// Progress that keeps the outline a sweep announces.
    #[derive(Default)]
    struct Outlined(Option<SweepOutline>);

    impl Progress for Outlined {
        fn report(&mut self, event: &ProgressEvent<'_>) {
            if let ProgressEvent::Planned(outline) = event {
                self.0 = Some((*outline).clone());
            }
        }
    }

    /// Checks that a dry run plans what the corresponding sweep or search announces, a dry run's flag aside.
    #[test]
    fn plan_spec_matches_a_dry_run() {
        let sir = entry("sir", None);
        let scratch = ScratchDir::new("plan-spec");
        let mut search = small_sweep();
        search.blocks.clear();
        search.search = Some(henad_core::explore::search::SearchSpec {
            algorithm: henad_core::explore::search::SearchAlgorithm::Random,
            max_evaluations: 4,
            batch_size: 2,
            objective: Some(henad_core::explore::search::Objective {
                column: "Infected:max".to_owned(),
                goal: henad_core::explore::search::Goal::Minimize,
                aggregate: henad_core::explore::search::Aggregate::Mean,
            }),
            space: vec![FactorSpec::param(
                "infection_rate",
                LevelSpec::Range {
                    min: 0.1,
                    max: 0.5,
                    step: None,
                },
            )],
        });
        for (name, spec) in [("sweep", small_sweep()), ("search", search)] {
            let planned = plan_spec(&sir, None, &spec, None, &options(), &mut NoProgress).expect("the spec plans");
            let mut announced = Outlined::default();
            let report =
                run_into(&sir, &spec, &scratch.path().join(name), &options(), &mut announced).expect("the spec runs");
            assert_eq!(report.end, SweepEnd::Complete, "{name}");
            let announced = announced.0.expect("a sweep announces its outline");
            assert!(planned.outline.dry_run && !announced.dry_run, "{name}");
            assert_eq!(
                SweepOutline {
                    dry_run: false,
                    ..planned.outline
                },
                announced,
                "{name}"
            );
        }
    }

    #[test]
    fn the_projected_bytes_are_measured_on_a_pool_as_wide_as_a_lane() {
        let ants = entry("ants", None);
        let mut spec = SweepSpec::new("ants");
        spec.fixed = vec![
            ("num_agents".to_owned(), "300".to_owned()),
            ("world_width".to_owned(), "64".to_owned()),
            ("world_height".to_owned(), "64".to_owned()),
        ];
        spec.run.replicates = 2;
        let two_lanes = SweepOptions {
            concurrency: Concurrency::Fixed(NonZeroUsize::new(2).expect("2 is above 0")),
            ..options()
        };
        let report = plan_spec(&ants, None, &spec, None, &two_lanes, &mut NoProgress).expect("the dry run plans");
        let layout = report.outline.layout;
        assert_eq!(layout.cpu_lanes, 2);

        let plan = spec.plan(&ants.schema()).expect("a valid spec");
        let probe = ProbeReport::for_plan(&ants, None, &plan).expect("ants builds");
        let lane = probe
            .rebuilt_on(&ants, layout.threads_per_lane)
            .expect("ants builds on a lane's pool");
        assert_eq!(report.outline.projected_bytes, 2 * lane.heap_bytes);
    }

    #[test]
    fn the_projection_takes_the_larger_of_the_first_and_last_configs() {
        let life = entry("game_of_life", None);
        for widths in [["16", "512"], ["512", "16"]] {
            let mut spec = SweepSpec::new("game_of_life");
            spec.fixed = vec![("grid_height".to_owned(), "256".to_owned())];
            spec.blocks = vec![BlockSpec {
                design: DesignKind::Factorial,
                factors: vec![FactorSpec::param(
                    "grid_width",
                    LevelSpec::Values(widths.map(str::to_owned).to_vec()),
                )],
                design_seed: None,
            }];
            let one_lane = SweepOptions {
                concurrency: Concurrency::Fixed(NonZeroUsize::MIN),
                ..options()
            };
            let report = plan_spec(&life, None, &spec, None, &one_lane, &mut NoProgress).expect("the dry run plans");
            let plan = spec.plan(&life.schema()).expect("a valid spec");
            let wide = usize::from(widths[0] == "16");
            let run = plan.run(wide as u64).expect("each config has a run");
            let params = &plan.config(run.config_id).expect("the run's config").params;
            let largest = ProbeReport::build(&life, None, params, Some(run.seed)).expect("the model builds");
            assert_eq!(report.outline.projected_bytes, largest.heap_bytes, "widths {widths:?}");
        }
    }

    #[test]
    fn a_sweep_records_its_plan_and_ends_complete() {
        let sir = entry("sir", None);
        let spec = small_sweep();
        let scratch = ScratchDir::new("manifest");
        let source = SpecSource {
            path: Some(PathBuf::from("specs/small.toml")),
            toml: Some("model = \"sir\"\n".to_owned()),
            tables: Vec::new(),
        };
        let mut progress = Recorded::default();
        let sweep_options = SweepOptions {
            spec_source: source.clone(),
            ..options()
        };
        let report = run_into(&sir, &spec, scratch.path(), &sweep_options, &mut progress).expect("the sweep runs");
        assert_eq!(report.end, SweepEnd::Complete);
        assert_eq!(report.output_dir.as_deref(), Some(scratch.path()));
        assert_eq!(
            progress.0,
            ["planned 4", "run 0", "run 1", "run 2", "run 3", "ended Complete"]
        );
        for file in [RUNS_FILE, SERIES_FILE, SUMMARY_FILE] {
            assert!(scratch.path().join(file).is_file(), "{file}");
        }

        let text = std::fs::read_to_string(scratch.path().join(MANIFEST_FILE)).expect("the manifest is written");
        let manifest: Manifest = serde_json::from_str(&text).expect("the manifest reads back");
        assert_eq!(manifest.status, ManifestStatus::Complete);
        assert_eq!(manifest.results, Some(report.counts));
        assert_eq!((manifest.plan.configs, manifest.plan.runs), (Some(2), 4));
        assert_eq!(manifest.plan.blocks[0].design, "factorial");
        assert_eq!(manifest.plan.plan_hash.len(), 16);
        assert_eq!(manifest.seeds.root, 3);
        assert_eq!(manifest.columns.stats, ["Susceptible", "Infected", "Recovered"]);
        assert_eq!(manifest.columns.reducers.len(), 12);
        assert_eq!(manifest.spec_source.toml, source.toml);
        assert_eq!(manifest.spec_source.path.as_deref(), Some("specs/small.toml"));
        assert_eq!(manifest.sessions[0].ran, 4);
        assert!(manifest.timestamps.finished_unix_ms >= Some(manifest.timestamps.started_unix_ms));
        assert!(manifest.model.schema["stat_columns"].is_array());
        let written: SpecFile = serde_json::from_value(manifest.spec).expect("the spec reads back");
        let mut sorted = spec.clone();
        sorted.fixed.sort();
        assert_eq!(
            written.into_spec().expect("a valid spec"),
            sorted,
            "fixed values come back by id"
        );

        let again = run_into(&sir, &spec, scratch.path(), &sweep_options, &mut NoProgress)
            .expect_err("the directory holds results");
        assert!(
            matches!(again, ExploreError::Output(OutputError::HoldsResults { .. })),
            "{again:?}"
        );
    }

    #[test]
    fn an_aborted_sweep_says_so_and_keeps_its_headers() {
        let sir = entry("sir", None);
        let scratch = ScratchDir::new("aborted");
        let sweep_options = options();
        sweep_options.control.abort();
        let report = run_into(&sir, &small_sweep(), scratch.path(), &sweep_options, &mut NoProgress)
            .expect("an aborted sweep is not an error");
        assert_eq!((report.end, report.counts.rows), (SweepEnd::Aborted, 0));
        let text = std::fs::read_to_string(scratch.path().join(MANIFEST_FILE)).expect("the manifest is written");
        let manifest: Manifest = serde_json::from_str(&text).expect("the manifest reads back");
        assert_eq!(manifest.status, ManifestStatus::Aborted);
        for file in [RUNS_FILE, SUMMARY_FILE] {
            let table = std::fs::read_to_string(scratch.path().join(file)).expect("the table is written");
            assert_eq!(table.lines().count(), 1, "{file} holds its header alone");
        }
    }

    /// Returns the model build warning for `recorded` against `current`.
    fn model_warning(recorded: &RecordedBuild, current: &RecordedBuild, between_shards: bool) -> String {
        SweepWarning::BuildChanged {
            role: BuildRole::Model,
            recorded: Box::new(recorded.clone()),
            current: Box::new(current.clone()),
            between_shards,
        }
        .to_string()
    }

    #[test]
    fn a_warning_between_unidentified_builds_says_they_cannot_be_told_apart() {
        let bare = RecordedBuild::from(register_grid_model::<GameOfLifeModel>().source());
        let text = model_warning(&bare, &bare, false);
        assert!(
            text.starts_with("cannot tell whether the directory's runs so far came from this model build"),
            "{text}"
        );
        assert!(
            text.ends_with(
                "Insert it into a `ModelSet` built with `henad::build_info!()` in a crate whose build script calls \
                 `henad_build::stamp_commit()`"
            ),
            "an entry from no set records no build: {text}"
        );
        let text = model_warning(&bare, &bare, true);
        assert!(
            text.starts_with("cannot tell whether the shards ran the same model build"),
            "{text}"
        );

        let unstamped = RecordedBuild::from(&BuildInfo::__from_env(
            "my-model",
            "0.1.0",
            Some(""),
            None,
            Some(""),
            None,
            false,
        ));
        let stamped = RecordedBuild::from(entry("game_of_life", None).source());
        let text = model_warning(&stamped, &unstamped, false);
        assert!(
            text.starts_with("the directory's runs so far came from the model build"),
            "{text}"
        );
        assert!(
            text.ends_with("Call `henad_build::stamp_commit()` in the build script of the crate that registers it"),
            "a build script can stamp a set's build: {text}"
        );
    }

    /// Checks that a GPU spec that the model rejects fails as a plan, before any device is acquired.
    #[test]
    fn a_gpu_spec_is_planned_before_its_device_is_acquired() {
        let gpu_sir = henad_models::example_models()
            .get("gpu_sir")
            .cloned()
            .expect("the model is registered");
        let mut spec = SweepSpec::new("gpu_sir");
        spec.fixed = vec![("infection_rate".to_owned(), "2".to_owned())];
        let mut acquisitions = 0;
        let mut refuse = |_| {
            acquisitions += 1;
            Err(DeviceError::BelowBaseline {
                adapter: "test".to_owned(),
                limit: "max_buffer_size",
            })
        };
        let refused = plan_then_acquire(&gpu_sir, None, &spec, &mut refuse).err();
        assert!(matches!(refused, Some(ExploreError::Plan(_))), "{refused:?}");
        let planned = plan_spec(&gpu_sir, None, &spec, None, &options(), &mut NoProgress);
        assert!(matches!(planned, Err(ExploreError::Plan(_))), "{planned:?}");
        let ran = run_spec(&gpu_sir, None, &spec, SweepOutput::Memory, &options(), &mut NoProgress);
        assert!(matches!(ran, Err(ExploreError::Plan(_))), "{ran:?}");

        spec.fixed.clear();
        let unacquired = plan_then_acquire(&gpu_sir, None, &spec, &mut refuse).err();
        assert!(matches!(unacquired, Some(ExploreError::Device(_))), "{unacquired:?}");
        assert_eq!(acquisitions, 1, "only the spec the model accepts asks for a device");
    }

    #[test]
    fn missing_runs_are_counted_in_words() {
        let missing = |count| SweepWarning::MissingRuns { count, first: vec![3] }.to_string();
        assert_eq!(
            missing(1),
            "1 run of the plan is missing, starting with 3. Resume the merged directory to run it"
        );
        assert_eq!(
            missing(2),
            "2 runs of the plan are missing, starting with 3. Resume the merged directory to run them"
        );
    }
}
