//! Sweeps that plan a spec against a model, run every planned run and write the results to a directory or to memory.

use std::fmt;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use henad_compute::fault::install_panic_hook;
use henad_compute::gpu::GpuContext;
use henad_compute::runtime_info::RuntimeInfo;
use henad_core::explore::measure::{MeasureError, MeasurePlan};
use henad_core::explore::outcome::{PlannedRun, RunOutcome};
use henad_core::explore::plan::{Plan, PlanError, PlanWarning, PlannedBlock, Shard};
use henad_core::explore::search::SearchReport;
use henad_core::explore::spec::SweepSpec;
use henad_core::metadata::Backend;
use henad_models::registry::ModelEntry;

use crate::exec::{
    ActiveRuns, BatchEnd, Concurrency, ExecutionBudget, ExecutionError, ExecutionLayout, Executor, RunRequest, RunSink,
    SweepControl, choose_layout, gpu_memory_budget,
};
use crate::output::manifest::{
    FORMAT, FORMAT_VERSION, Manifest, ManifestBlock, ManifestColumns, ManifestDesignTable, ManifestEngine,
    ManifestExecution, ManifestMode, ManifestModel, ManifestPlan, ManifestRuntime, ManifestSearch, ManifestSeeds,
    ManifestSession, ManifestSpecSource, ManifestStatus, ManifestTimestamps, ResultCounts, now_unix_ms, rfc3339,
};
use crate::output::memory::SweepFiles;
use crate::output::resume::{ResumeError, ResumeScan};
use crate::output::runs_csv::column_names;
use crate::output::{OutputDir, OutputError, OutputWriter, runs_csv, series_csv};
use crate::probe::{CapacityError, ProbeError, ProbeReport, check_capacity};
use crate::progress::{Progress, ProgressEvent, ProgressMeter};
use crate::schema::{backend_name, model_schema, schema_json};
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
    pub fn loaded(path: &Path, toml: String, file: &SpecFile) -> Self {
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

/// Build of the host binary and its command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Provenance {
    pub engine_name: String,
    pub engine_version: String,
    /// Short hash of the commit the host was built from, empty when unknown.
    pub commit: String,
    pub commit_date: String,
    pub debug_build: bool,
    pub argv: Vec<String>,
}

/// Settings of a sweep that never change its results.
#[derive(Debug, Clone, Default)]
pub struct SweepOptions {
    /// Directory the results go in. A dry run needs none.
    pub output_dir: Option<PathBuf>,
    pub concurrency: Concurrency,
    /// Bytes of host memory the live runs can hold together, `None` for no limit.
    pub memory_budget: Option<u64>,
    /// Cap on the bytes of device memory the live GPU runs hold together, `None` for the device's largest buffer.
    pub gpu_memory: Option<u64>,
    /// Whether to plan and probe the sweep and stop there, writing nothing.
    pub dry_run: bool,
    /// Switch that pauses or aborts the sweep from another thread.
    pub control: SweepControl,
    /// Share of the plan's runs the sweep runs.
    pub shard: Shard,
    /// Whether to add to a directory that holds runs of the same plan, running only the runs it lacks.
    ///
    /// A directory that holds no results starts afresh.
    pub resume: bool,
    /// Whether a resume runs again the runs that ended on a fault. A run that timed out always runs again.
    pub retry_failed: bool,
    /// Table each run in progress is listed in, `None` when nothing watches the runs.
    pub active_runs: Option<ActiveRuns>,
}

/// Size and layout of a planned sweep or search.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepOutline {
    /// Id of the model.
    pub model: String,
    pub backend: Backend,
    /// Configs of a sweep's plan, `None` for a search, whose budget [`SearchOutline::max_evaluations`] gives.
    pub configs: Option<u64>,
    pub replicates: u64,
    /// Runs of the whole plan.
    pub runs: u64,
    /// Blocks of the plan, each with the seed its design drew from.
    pub blocks: Vec<PlannedBlock>,
    /// Share of the runs the sweep takes.
    pub shard: Shard,
    /// Runs of the shard the output directory holds already, and a resume keeps.
    pub skipped: u64,
    /// Runs the sweep runs.
    pub pending: u64,
    pub layout: ExecutionLayout,
    /// Bytes the live runs are projected to hold together.
    pub projected_bytes: u64,
    /// Rows the pending runs add to `series.csv` once each reaches its final tick.
    pub series_rows: u64,
    /// Names of the stat columns of `series.csv`, before CSV escaping.
    pub stat_columns: Vec<String>,
    /// Names of the reducer columns of `runs.csv`, before CSV escaping.
    pub reducer_columns: Vec<String>,
    pub dry_run: bool,
    /// Budget and space of a search, `None` for a sweep.
    pub search: Option<SearchOutline>,
}

/// Something about a sweep that runs, though likely not as meant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SweepWarning {
    /// A warning of the plan.
    Plan(PlanWarning),
    /// A resumed directory whose last session ran the build `recorded`, and this one is `current`.
    CommitChanged { recorded: String, current: String },
    /// Runs of the plan that no merged directory holds, `count` in all. `first` lists the lowest ids.
    MissingRuns { count: u64, first: Vec<u64> },
}

impl fmt::Display for SweepWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(warning) => warning.fmt(f),
            Self::CommitChanged { recorded, current } => write!(
                f,
                "the directory's runs so far came from build {recorded}, and this build is {current}"
            ),
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
    pub outline: SweepOutline,
    pub end: SweepEnd,
    /// Rows in `runs.csv` once the sweep ends, by status, the rows a resume kept included. Zero for a dry run.
    pub counts: ResultCounts,
    /// Time from the start of planning to the end of the sweep.
    pub elapsed: Duration,
    /// Directory the results went to, `None` for a dry run or a sweep held in memory.
    pub output_dir: Option<PathBuf>,
}

/// Report, manifest and files of a sweep that ran to its end or was aborted.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepRecord {
    pub report: SweepReport,
    /// Manifest with the sweep's final status, as `manifest.json` holds it.
    pub manifest: Manifest,
    /// Files of a sweep held in memory, `None` for one written to a directory.
    pub files: Option<SweepFiles>,
    /// Standing of a search at its end, `None` for a sweep.
    pub search: Option<SearchReport>,
}

/// A sweep that cannot run to its end.
#[derive(Debug)]
pub enum ExploreError {
    /// A spec the model refuses, for the reason inside.
    Plan(PlanError),
    /// Configs the device cannot host.
    Capacity(CapacityError),
    /// A probe build that failed.
    Probe(ProbeError),
    /// Reducers that do not bind to the columns of the probe build.
    Measure(MeasureError),
    /// A sweep with no output directory that is not a dry run.
    NoOutput,
    /// A directory the sweep cannot resume into, for the reason inside.
    Resume(ResumeError),
    /// Results that cannot be written.
    Output(OutputError),
    /// A batch of runs that cannot run.
    Execution(ExecutionError),
    /// A search spec the model or the options refuse, for the reason inside.
    Search(SearchPlanError),
}

impl fmt::Display for ExploreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Plan(_) => f.write_str("cannot plan the sweep"),
            Self::Capacity(_) => f.write_str("the sweep does not fit this GPU"),
            Self::Probe(_) => f.write_str("cannot probe the model"),
            Self::Measure(_) => f.write_str("cannot bind the reducers to the model's stat columns"),
            Self::NoOutput => f.write_str("a sweep needs an output directory unless it is a dry run"),
            Self::Resume(_) => f.write_str("cannot resume the sweep"),
            Self::Output(_) => f.write_str("cannot write the results"),
            Self::Execution(_) => f.write_str("cannot run the sweep"),
            Self::Search(_) => f.write_str("cannot plan the search"),
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
            Self::NoOutput => None,
            Self::Resume(error) => Some(error),
            Self::Output(error) => Some(error),
            Self::Execution(error) => Some(error),
            Self::Search(error) => Some(error),
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

/// Plans `spec` against `entry`, runs the planned runs of `options.shard` and writes the results to
/// `options.output_dir`.
///
/// The sweep plans the spec, checks every config against the device, probes the first config without a fault and
/// the last config, and chooses a layout from the larger probe. It then writes the manifest with status `running`,
/// streams each run to `runs.csv` and `series.csv` in plan order, rebuilds `summary.csv` from `runs.csv`, and
/// replaces the manifest with its final status. A dry run stops after the probe and writes nothing.
///
/// With `options.resume`, a directory holding runs of the same plan keeps the runs its [`ResumeScan`] keeps, and
/// the sweep runs the rest. Both tables then list every run in order of its id, as a sweep run in one go would.
///
/// `gpu` is the device a GPU model steps on, and `runtime` describes it. `source` and `provenance` go into the
/// manifest as they are. A run that faults is recorded with its status, and the sweep carries on. A sweep that fails
/// once its manifest is written marks the manifest `failed` when it can.
///
/// # Errors
///
/// Returns [`ExploreError`] when the spec cannot be planned, a config does not fit the device, the probe build
/// fails, the output directory holds results and is not resumed, the directory cannot be resumed, the results cannot
/// be written, or a batch cannot run.
#[expect(
    clippy::too_many_arguments,
    reason = "the model and device, the spec and its record, and the host's settings"
)]
pub fn run_sweep(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    runtime: Option<&RuntimeInfo>,
    spec: &SweepSpec,
    source: &SpecSource,
    provenance: &Provenance,
    options: &SweepOptions,
    progress: &mut dyn Progress,
) -> Result<SweepReport, ExploreError> {
    install_panic_hook();
    let output_dir = match (options.output_dir.as_deref(), options.dry_run) {
        (_, true) => None,
        (Some(output_dir), false) => Some(output_dir),
        (None, false) => return Err(ExploreError::NoOutput),
    };
    let runtime = ManifestRuntime::new(runtime);
    let inputs = SweepInputs {
        entry,
        gpu,
        runtime: &runtime,
        spec,
        source,
        provenance,
        options,
    };
    let preparation = SweepPreparation::new(&inputs, None)?;
    preparation.announce(provenance, progress);
    let Some(output_dir) = output_dir else {
        let report = preparation.report(SweepEnd::Planned, ResultCounts::default(), None);
        progress.report(&ProgressEvent::Ended(&report));
        return Ok(report);
    };
    let record = preparation.write_directory(&inputs, output_dir, progress)?;
    progress.report(&ProgressEvent::Ended(&record.report));
    Ok(record.report)
}

/// Runs `plan`, the plan of `inputs.spec`, into the directory `output_dir` as [`run_sweep`] does, and returns its
/// record.
///
/// # Errors
///
/// Returns the errors of [`run_sweep`].
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_into_directory(
    inputs: &SweepInputs<'_>,
    plan: Arc<Plan>,
    output_dir: &Path,
    progress: &mut dyn Progress,
) -> Result<SweepRecord, ExploreError> {
    install_panic_hook();
    let preparation = SweepPreparation::new(inputs, Some(plan))?;
    preparation.announce(inputs.provenance, progress);
    let record = preparation.write_directory(inputs, output_dir, progress)?;
    progress.report(&ProgressEvent::Ended(&record.report));
    Ok(record)
}

/// Runs `plan`, the plan of `inputs.spec`, holding its four files in memory, and returns its record.
///
/// The files hold the bytes a directory would. The caller's `inputs.options` names no output directory and no
/// resume.
///
/// # Errors
///
/// Returns the errors of [`run_sweep`] that do not come from a directory.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn run_in_memory(
    inputs: &SweepInputs<'_>,
    plan: Arc<Plan>,
    progress: &mut dyn Progress,
) -> Result<SweepRecord, ExploreError> {
    use crate::output::memory::memory_writer;

    install_panic_hook();
    let preparation = SweepPreparation::new(inputs, Some(plan))?;
    preparation.announce(inputs.provenance, progress);
    let mut manifest = preparation.manifest(inputs)?;
    let writer = memory_writer(&preparation.plan, &inputs.entry.param_descriptors, &preparation.measure)?;
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
}

/// A sweep planned, checked and probed, with its layout chosen.
pub(crate) struct SweepPreparation {
    plan: Arc<Plan>,
    probe: ProbeReport,
    measure: Arc<MeasurePlan>,
    /// Runs of the shard the sweep runs, in plan order.
    pending: Vec<PlannedRun>,
    /// Directory the sweep resumes, `None` for a sweep that starts afresh.
    resumed: Option<ResumeScan>,
    outline: SweepOutline,
    /// Clock reading when planning started.
    started: Instant,
    started_unix_ms: u64,
}

impl SweepPreparation {
    /// Checks and probes `plan`, the plan of `inputs.spec`, and chooses its layout. With no `plan`, the spec is
    /// planned first.
    pub(crate) fn new(inputs: &SweepInputs<'_>, plan: Option<Arc<Plan>>) -> Result<Self, ExploreError> {
        let (entry, gpu, options) = (inputs.entry, inputs.gpu, inputs.options);
        if inputs.spec.search.is_some() {
            return Err(ExploreError::Search(SearchPlanError::NotASweep));
        }
        let started = Instant::now();
        let started_unix_ms = now_unix_ms();
        let plan = match plan {
            Some(plan) => plan,
            None => Arc::new(inputs.spec.plan(&model_schema(entry))?),
        };
        let resume_dir = options
            .output_dir
            .as_deref()
            .filter(|output_dir| options.resume && OutputDir::holds_results(output_dir));
        if let (None, Some(output_dir)) = (resume_dir, &options.output_dir) {
            OutputDir::check_free(output_dir)?;
        }
        if let Some(ctx) = gpu {
            check_capacity(entry, &plan, &ctx.device.limits())?;
        }
        let probe = ProbeReport::for_plan(entry, gpu, &plan)?;
        let measure = MeasurePlan::new(plan.run_settings(), plan.measure_settings(), probe.columns.clone())?;
        let resumed = resume_dir
            .map(|output_dir| {
                let columns = column_names(&entry.param_descriptors, plan.actions(), measure.reducers().names());
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

        // The layout is sized from whichever of the first and last configs holds more.
        let last_probe = ProbeReport::for_last_config(entry, gpu, &plan, &probe);
        let sizing_probe = match &last_probe {
            Some(last_probe) if last_probe.footprint() > probe.footprint() => last_probe,
            _ => &probe,
        };
        let pending_count = pending.len() as u64;
        let (layout, projected_bytes) = sized_layout(entry, gpu, options, sizing_probe, pending_count)?;
        let outline = SweepOutline {
            model: entry.id.clone(),
            backend: entry.metadata.backend,
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
            dry_run: options.dry_run,
            search: None,
        };
        Ok(Self {
            plan,
            probe,
            measure: Arc::new(measure),
            pending,
            resumed,
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

    /// Runs the sweep runs, in plan order.
    #[cfg(any(target_arch = "wasm32", test))]
    pub(crate) fn pending(&self) -> &[PlannedRun] {
        &self.pending
    }

    /// Reports the outline, then each warning of the plan and of a resume under another build than `provenance`.
    pub(crate) fn announce(&self, provenance: &Provenance, progress: &mut dyn Progress) {
        progress.report(&ProgressEvent::Planned(&self.outline));
        for warning in self.warnings(provenance) {
            progress.report(&ProgressEvent::Warned(&warning));
        }
    }

    /// Returns the warnings of the plan, and of a resume under another build than `provenance`.
    fn warnings(&self, provenance: &Provenance) -> Vec<SweepWarning> {
        let mut warnings: Vec<SweepWarning> = self.plan.warnings().iter().cloned().map(SweepWarning::Plan).collect();
        if let Some(scan) = &self.resumed
            && scan.recorded.engine.commit != provenance.commit
        {
            warnings.push(SweepWarning::CommitChanged {
                recorded: scan.recorded.engine.commit.clone(),
                current: provenance.commit.clone(),
            });
        }
        warnings
    }

    /// Writes the manifest with status `running`, runs every pending run into `output_dir` and replaces the manifest
    /// with the sweep's final status.
    fn write_directory(
        &self,
        inputs: &SweepInputs<'_>,
        output_dir: &Path,
        progress: &mut dyn Progress,
    ) -> Result<SweepRecord, ExploreError> {
        let dir = if self.resumed.is_some() {
            OutputDir::open(output_dir)?
        } else {
            OutputDir::create(output_dir)?
        };
        let mut manifest = self.manifest(inputs)?;
        dir.write_manifest(&manifest)?;
        let (end, counts) = match self.run_into(&dir, inputs, progress) {
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
    fn run_into(
        &self,
        dir: &OutputDir,
        inputs: &SweepInputs<'_>,
        progress: &mut dyn Progress,
    ) -> Result<(BatchEnd, ResultCounts), ExploreError> {
        if let Some(scan) = &self.resumed {
            scan.repair(dir)?;
        }
        let params = &inputs.entry.param_descriptors;
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
        .with_gpu_memory(options.gpu_memory);
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
    let backend = entry.metadata.backend;
    let detected = ExecutionBudget::detect();
    let unbudgeted = choose_layout(options.concurrency, &detected, backend, sizing_probe, runs);
    let rebuilt = if unbudgeted.cpu_lanes > 1 && unbudgeted.threads_per_lane != detected.workers {
        Some(sizing_probe.rebuilt_on(entry, unbudgeted.threads_per_lane)?)
    } else {
        None
    };
    let resources = ExecutionBudget {
        memory_budget: options.memory_budget,
        gpu_memory_budget: gpu.map(|ctx| gpu_memory_budget(options.gpu_memory, ctx)),
        ..detected
    };
    let lane_probe = rebuilt.as_ref().unwrap_or(sizing_probe);
    let layout = choose_layout(options.concurrency, &resources, backend, lane_probe, runs);
    let run_probe = if layout.cpu_lanes > 1 { lane_probe } else { sizing_probe };
    Ok((layout, layout.projected_bytes(run_probe)))
}

/// Parts of a manifest that differ between a sweep and a search.
pub(crate) struct ManifestParts<'a> {
    pub(crate) mode: ManifestMode,
    /// Plan the runs come from, the plan of a search's fixed values alone for a search.
    pub(crate) plan: &'a Plan,
    pub(crate) probe: &'a ProbeReport,
    pub(crate) outline: &'a SweepOutline,
    pub(crate) manifest_plan: ManifestPlan,
    pub(crate) started_unix_ms: u64,
    /// Manifest of the directory a resume adds to, `None` for a start afresh.
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
    let backend = backend_name(entry.metadata.backend).to_owned();
    let mut spec_file = SpecFile::from(inputs.spec);
    spec_file.execution = ExecutionTable {
        concurrent: options.concurrency,
        memory: options.memory_budget,
        gpu_memory: options.gpu_memory,
    };
    let outline = parts.outline;
    let layout = outline.layout;
    let seeds = parts.plan.seed_settings();
    let spec = serde_json::to_value(&spec_file).map_err(OutputError::Manifest)?;
    let session = ManifestSession {
        started: rfc3339(parts.started_unix_ms),
        commit: provenance.commit.clone(),
        skipped: outline.skipped,
        ran: 0,
    };
    let (started_unix_ms, sessions, merged_shards) = match parts.recorded {
        Some(recorded) => {
            let mut sessions = recorded.sessions.clone();
            sessions.push(session);
            (
                recorded.timestamps.started_unix_ms,
                sessions,
                recorded.merged_shards.clone(),
            )
        }
        None => (parts.started_unix_ms, vec![session], None),
    };
    Ok(Manifest {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        mode: parts.mode,
        status: ManifestStatus::Running,
        engine: ManifestEngine {
            name: provenance.engine_name.clone(),
            version: provenance.engine_version.clone(),
            commit: provenance.commit.clone(),
            commit_date: provenance.commit_date.clone(),
            debug_build: provenance.debug_build,
        },
        model: ManifestModel {
            id: entry.id.clone(),
            name: entry.name.clone(),
            backend: backend.clone(),
            schema_hash: hex(parts.plan.schema_hash()),
            schema: schema_json(entry, Some(parts.probe)),
        },
        spec,
        spec_source: inputs.source.into(),
        argv: provenance.argv.clone(),
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
            gpu_memory_budget: options.gpu_memory,
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
struct SweepSink<'s, W: Write> {
    writer: OutputWriter<W>,
    progress: &'s mut dyn Progress,
    meter: ProgressMeter,
}

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

    use super::{ExploreError, SpecSource, SweepEnd, SweepOptions, SweepWarning, run_sweep};
    use crate::exec::Concurrency;
    use crate::output::manifest::{Manifest, ManifestStatus};
    use crate::output::{MANIFEST_FILE, OutputError, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
    use crate::probe::ProbeReport;
    use crate::progress::{NoProgress, Progress, ProgressEvent};
    use crate::schema::model_schema;
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

    fn options(output_dir: Option<PathBuf>, dry_run: bool) -> SweepOptions {
        SweepOptions {
            output_dir,
            dry_run,
            ..SweepOptions::default()
        }
    }

    #[test]
    fn a_dry_run_plans_and_probes_and_writes_nothing() {
        let sir = entry("sir", None);
        let scratch = ScratchDir::new("dry-run");
        let mut progress = Recorded::default();
        let report = run_sweep(
            &sir,
            None,
            None,
            &small_sweep(),
            &SpecSource::default(),
            &provenance(),
            &options(Some(scratch.path().to_owned()), true),
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

        let error = run_sweep(
            &sir,
            None,
            None,
            &small_sweep(),
            &SpecSource::default(),
            &provenance(),
            &options(None, false),
            &mut NoProgress,
        )
        .expect_err("no output directory");
        assert!(matches!(error, ExploreError::NoOutput), "{error:?}");
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
            dry_run: true,
            ..SweepOptions::default()
        };
        let report = run_sweep(
            &ants,
            None,
            None,
            &spec,
            &SpecSource::default(),
            &provenance(),
            &two_lanes,
            &mut NoProgress,
        )
        .expect("the dry run plans");
        let layout = report.outline.layout;
        assert_eq!(layout.cpu_lanes, 2);

        let plan = spec.plan(&model_schema(&ants)).expect("a valid spec");
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
                dry_run: true,
                ..SweepOptions::default()
            };
            let report = run_sweep(
                &life,
                None,
                None,
                &spec,
                &SpecSource::default(),
                &provenance(),
                &one_lane,
                &mut NoProgress,
            )
            .expect("the dry run plans");
            let plan = spec.plan(&model_schema(&life)).expect("a valid spec");
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
        let sweep_options = options(Some(scratch.path().to_owned()), false);
        let report = run_sweep(
            &sir,
            None,
            None,
            &spec,
            &source,
            &provenance(),
            &sweep_options,
            &mut progress,
        )
        .expect("the sweep runs");
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

        let again = run_sweep(
            &sir,
            None,
            None,
            &spec,
            &source,
            &provenance(),
            &sweep_options,
            &mut NoProgress,
        )
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
        let sweep_options = options(Some(scratch.path().to_owned()), false);
        sweep_options.control.abort();
        let report = run_sweep(
            &sir,
            None,
            None,
            &small_sweep(),
            &SpecSource::default(),
            &provenance(),
            &sweep_options,
            &mut NoProgress,
        )
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
