//! Headless device, scratch directories and sweep helpers for the tests.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use henad_compute::entry::{ModelEntry, ModelState};
use henad_compute::fault::{Fault, catching};
use henad_compute::gpu::GpuContext;
use henad_compute::runner::{Pace, SimLoop as _};
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::{Plan, Shard};
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
use henad_core::export::csv::parse_records;
use henad_core::params::ParamValue;
use henad_models::example_models;

use henad_compute::fault::install_panic_hook;

use crate::exec::{ActiveRun, BatchEnd, Concurrency, ExecutionLayout, Executor, RunRequest, RunSink, SweepControl};
use crate::handle::{SweepEvent, SweepOutput, SweepRun};
use crate::output::manifest::{Manifest, RecordedBuild};
use crate::output::memory::SweepFiles;
use crate::output::runs_csv::{OUTCOME_COLUMNS, TIMING_COLUMNS};
use crate::output::{MANIFEST_FILE, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
use crate::probe::ProbeReport;
use crate::progress::{NoProgress, Progress, ProgressEvent};
use crate::pumped::PumpedSweep;
use crate::sweep::{ExploreError, Provenance, SweepOptions, SweepReport, SweepWarning, plan_spec, run_spec};
use crate::testing::{TestDeviceRequest, headless_test_device};

/// Returns a headless device raised to the example models' needs, or `None` to skip a GPU test on a machine without
/// a device.
///
/// # Panics
///
/// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
pub fn headless_device() -> Option<GpuContext> {
    headless_test_device(&TestDeviceRequest::raised(example_models().gpu_needs()))
}

/// Returns a headless device with the limits of `wgpu::Limits::default()`, the WebGPU baseline, or `None` to skip a
/// GPU test on a machine without a device.
///
/// # Panics
///
/// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
pub fn baseline_device() -> Option<GpuContext> {
    headless_test_device(&TestDeviceRequest::baseline())
}

/// Returns example model `id`, as a host with `gpu` as its device finds it.
///
/// # Panics
///
/// Panics when no model has the id, or for a GPU model when `gpu` is `None`.
pub fn entry(id: &str, gpu: Option<&GpuContext>) -> ModelEntry {
    example_models()
        .lookup(id, gpu)
        .cloned()
        .unwrap_or_else(|error| panic!("{error}"))
}

/// Returns `pairs` of parameter ids and value texts as a spec's fixed values.
pub fn fixed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(id, value)| (id.to_owned(), value.to_owned()))
        .collect()
}

/// Returns `raw` as a factor's levels written as text.
pub fn values(raw: &[&str]) -> LevelSpec {
    LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
}

/// Returns a factor's levels from `min` to `max`, for a design that samples between them.
pub fn range(min: f64, max: f64) -> LevelSpec {
    LevelSpec::Range { min, max, step: None }
}

/// Returns a block of every combination of `factors`.
pub fn factorial(factors: Vec<FactorSpec>) -> BlockSpec {
    BlockSpec {
        design: DesignKind::Factorial,
        factors,
        design_seed: None,
    }
}

/// Returns a fixed count of `count` lanes.
///
/// Panics if `count` is 0.
pub fn lane_count(count: usize) -> Concurrency {
    Concurrency::Fixed(NonZeroUsize::new(count).expect("a test lane count is above 0"))
}

/// Returns a spec over `model` on an 8 by 8 grid for 10 steps sampled every 2, with one factorial block over `param`
/// at `levels`.
pub fn spec_over(model: &str, param: &str, levels: &[&str]) -> SweepSpec {
    let mut spec = SweepSpec::new(model);
    spec.fixed = fixed(&[("grid_width", "8"), ("grid_height", "8")]);
    spec.run.steps = 10;
    spec.measure.stats_every = 2;
    spec.measure.series_every = 2;
    spec.blocks = vec![factorial(vec![FactorSpec::param(param, values(levels))])];
    spec
}

/// Returns `text` with `old` replaced by `new` once, where `text` must hold `old`.
#[cfg(feature = "model-files")]
pub fn edited(text: &str, old: &str, new: &str) -> String {
    assert!(text.contains(old), "the text holds no {old:?}");
    text.replacen(old, new, 1)
}

/// Returns the line of `text` that holds the first `needle`, counted from 1.
///
/// Panics when `text` holds no `needle`.
#[cfg(feature = "model-files")]
pub fn line_of(text: &str, needle: &str) -> u32 {
    let at = text
        .find(needle)
        .unwrap_or_else(|| panic!("the text holds no {needle:?}"));
    line_at(text, at)
}

/// Returns the line of `text` that holds the byte at `offset`, counted from 1.
#[cfg(feature = "model-files")]
pub fn line_at(text: &str, offset: usize) -> u32 {
    u32::try_from(text[..offset].matches('\n').count() + 1).expect("a short text")
}

/// Returns 3 configs of SIR on a 16 by 16 grid with `replicates` replicates of 30 steps, and an action whose tick
/// varies.
pub fn sir_spec(replicates: u64) -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "16"), ("grid_height", "16")]);
    spec.run.steps = 30;
    spec.run.replicates = replicates;
    spec.measure.stats_every = 3;
    spec.measure.series_every = 6;
    spec.seeds.root = 8;
    spec.actions = vec![ActionSpec::new("seed_outbreak", 12)];
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Zip,
        factors: vec![
            FactorSpec::param("infection_rate", values(&["0.2", "0.4", "0.6"])),
            FactorSpec::action("seed_outbreak", values(&["6", "12", "18"])),
        ],
        design_seed: None,
    }];
    spec
}

/// Returns `entry` with a build that faults every time, for a test that checks the model is refused before any build.
pub fn unbuildable(entry: ModelEntry) -> ModelEntry {
    entry.wrap_factory(|_| {
        Arc::new(
            |_params: &[ParamValue], _seed: Option<u64>, _gpu: Option<&GpuContext>| -> Result<ModelState, Fault> {
                catching("building a model that is never built", || -> ModelState {
                    panic!("the model was built")
                })
            },
        )
    })
}

/// Path under the system's temporary directory, unique to one test, removed with its contents on drop.
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Returns a path, named after `name`, where nothing exists yet.
    pub fn new(name: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let index = NEXT.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("henad-explore-{name}-{}-{index}", std::process::id()));
        // An earlier process with the same id can have left the directory behind.
        remove(&path);
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        remove(&self.path);
    }
}

/// Removes the directory at `path` and everything in it, if it exists.
fn remove(path: &Path) {
    if path.exists() {
        std::fs::remove_dir_all(path).expect("the scratch directory can be removed");
    }
}

/// Returns an engine build that [`RecordedBuild::same_build`] tells apart from Henad's own build, clean or dirty.
pub fn other_engine() -> RecordedBuild {
    let mut other = RecordedBuild::engine();
    other.commit = "0ther000".to_owned();
    other.dirty = Some(false);
    other.source_hash = Some(other.source_hash.unwrap_or_default() ^ 1);
    other
}

/// Returns the provenance of a sweep that the tests run, with henad-explore's own build as the host build.
pub fn provenance() -> Provenance {
    Provenance::new(henad_core::build_info!(), vec!["henad-explore-tests".to_owned()])
}

/// Runs `spec` over `entry` into `output_dir` at `concurrency`, reporting nowhere.
///
/// The panic hook is installed first, as a host's `main` installs it, so a fault carries the site of its panic.
///
/// # Panics
///
/// Panics when the sweep fails.
pub fn sweep(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    spec: &SweepSpec,
    output_dir: &Path,
    concurrency: Concurrency,
) -> SweepReport {
    let mut options = sweep_options(false);
    options.concurrency = concurrency;
    sweep_with(entry, gpu, spec, output_dir, &options, &mut NoProgress).expect("the sweep runs")
}

/// Runs `spec` over `entry` into `output_dir` with `options`, reporting to `progress`.
///
/// The panic hook is installed first, as in [`sweep`].
///
/// # Errors
///
/// Returns the error of the sweep.
pub fn sweep_with(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    spec: &SweepSpec,
    output_dir: &Path,
    options: &SweepOptions,
    progress: &mut dyn Progress,
) -> Result<SweepReport, ExploreError> {
    install_panic_hook();
    let output = SweepOutput::Directory(output_dir.to_owned());
    run_spec(entry, gpu, spec, output, options, progress).map(|record| record.report)
}

/// Plans `spec` over `entry` as a dry run reads `folder`, reporting to `progress`.
///
/// # Errors
///
/// Returns the error of the plan.
pub fn dry_run(
    entry: &ModelEntry,
    spec: &SweepSpec,
    folder: Option<&Path>,
    options: &SweepOptions,
    progress: &mut dyn Progress,
) -> Result<SweepReport, ExploreError> {
    plan_spec(entry, None, spec, folder, options, progress)
}

/// Returns the options of a sweep for this build, resumed when `resume` is set.
pub fn sweep_options(resume: bool) -> SweepOptions {
    let mut options = SweepOptions::new(provenance());
    options.resume = resume;
    options
}

/// Returns the options of shard `index` of `count` of a sweep for this build.
///
/// # Panics
///
/// Panics when `index` is not below `count`.
pub fn shard_options(index: u64, count: u64) -> SweepOptions {
    SweepOptions {
        shard: Shard::new(index, count).expect("a valid shard"),
        ..sweep_options(false)
    }
}

/// Progress that keeps the ids of the committed runs and the warnings.
#[derive(Debug, Default)]
pub struct Recorder {
    pub committed: Vec<u64>,
    pub warnings: Vec<SweepWarning>,
}

impl Progress for Recorder {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        match event {
            ProgressEvent::RunCommitted(outcome) => self.committed.push(outcome.run.run_id),
            ProgressEvent::Warned(warning) => self.warnings.push((*warning).clone()),
            ProgressEvent::Planned(_)
            | ProgressEvent::Progressed(_)
            | ProgressEvent::SearchBatchTold(_)
            | ProgressEvent::Ended(_) => {}
        }
    }
}

/// Progress that resumes `spec` over `entry` in `dir` a second time once the first resume reports its outline, after
/// its scan, and keeps the result of the second resume.
#[derive(Debug)]
pub struct SecondResume<'a> {
    entry: &'a ModelEntry,
    spec: &'a SweepSpec,
    dir: &'a Path,
    pub result: Option<Result<SweepReport, ExploreError>>,
}

impl<'a> SecondResume<'a> {
    pub fn new(entry: &'a ModelEntry, spec: &'a SweepSpec, dir: &'a Path) -> Self {
        Self {
            entry,
            spec,
            dir,
            result: None,
        }
    }
}

impl Progress for SecondResume<'_> {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        if matches!(event, ProgressEvent::Planned(_)) && self.result.is_none() {
            let options = sweep_options(true);
            self.result = Some(sweep_with(
                self.entry,
                None,
                self.spec,
                self.dir,
                &options,
                &mut NoProgress,
            ));
        }
    }
}

/// Reads the manifest of the output directory `dir`.
///
/// # Panics
///
/// Panics when the manifest is missing or unreadable.
pub fn manifest(dir: &Path) -> Manifest {
    Manifest::read(&dir.join(MANIFEST_FILE)).expect("the manifest reads back")
}

/// Rewrites the manifest of the output directory `dir` after `change` edits it.
///
/// # Panics
///
/// Panics when the manifest is missing, unreadable or cannot be written.
pub fn rewrite_manifest(dir: &Path, change: impl FnOnce(&mut Manifest)) {
    let mut recorded = manifest(dir);
    change(&mut recorded);
    let bytes = serde_json::to_vec_pretty(&recorded).expect("the manifest writes as JSON");
    std::fs::write(dir.join(MANIFEST_FILE), bytes).expect("the manifest is written");
}

/// Three CSV files of an output directory, as their text and as records of fields.
///
/// Two readings are equal when `series.csv` and `summary.csv` match byte for byte, and `runs.csv` matches apart from
/// the fields of its timing columns.
#[derive(Debug, PartialEq, Eq)]
pub struct OutputTables {
    /// Text of `runs.csv`, with the fields of the timing columns emptied.
    runs_text: String,
    series_text: String,
    summary_text: String,
    /// Records of `runs.csv`, with the timing columns emptied.
    pub runs: Vec<Vec<String>>,
    pub series: Vec<Vec<String>>,
    pub summary: Vec<Vec<String>>,
}

impl OutputTables {
    /// Reads the tables of the output directory `dir`.
    ///
    /// # Panics
    ///
    /// Panics when a table is missing or is not valid CSV.
    pub fn read(dir: &Path) -> Self {
        let read = |file: &str| std::fs::read_to_string(dir.join(file)).expect("the table is written");
        let records = |text: &str| parse_records(text).expect("the table is valid CSV");
        let (runs_text, series_text, summary_text) = (read(RUNS_FILE), read(SERIES_FILE), read(SUMMARY_FILE));
        let runs = without_timing(records(&runs_text));
        let timing = timing_positions(&runs[0]);
        Self {
            runs_text: with_fields_emptied(&runs_text, &timing),
            series: records(&series_text),
            summary: records(&summary_text),
            runs,
            series_text,
            summary_text,
        }
    }

    /// Returns the field in column `name` of every run of `runs.csv`, in run order.
    ///
    /// # Panics
    ///
    /// Panics when there is no such column.
    pub fn run_column(&self, name: &str) -> Vec<&str> {
        column(&self.runs, name)
    }

    /// Returns the tick and the field in column `name` of every row of run `run_id` in `series.csv`.
    ///
    /// # Panics
    ///
    /// Panics when there is no such column.
    pub fn series_of(&self, run_id: u64, name: &str) -> Vec<(u64, &str)> {
        let index = self.series[0]
            .iter()
            .position(|header| header == name)
            .unwrap_or_else(|| panic!("series.csv has no column '{name}'"));
        let run_id = run_id.to_string();
        self.series[1..]
            .iter()
            .filter(|record| record[0] == run_id)
            .map(|record| (record[1].parse().expect("a tick"), record[index].as_str()))
            .collect()
    }

    /// Returns the field in column `name` of every config of `summary.csv`, in config order.
    ///
    /// # Panics
    ///
    /// Panics when there is no such column.
    pub fn summary_column(&self, name: &str) -> Vec<&str> {
        column(&self.summary, name)
    }
}

/// Returns the field in column `name` of every record of `table` after its header.
fn column<'t>(table: &'t [Vec<String>], name: &str) -> Vec<&'t str> {
    let index = table[0]
        .iter()
        .position(|header| header == name)
        .unwrap_or_else(|| panic!("the table has no column '{name}'"));
    table[1..].iter().map(|record| record[index].as_str()).collect()
}

/// Returns `runs`, the records of a `runs.csv`, with every timing field emptied.
///
/// # Panics
///
/// Panics when the header lacks the outcome columns.
pub fn without_timing(mut runs: Vec<Vec<String>>) -> Vec<Vec<String>> {
    let timing = timing_positions(&runs[0]);
    for record in &mut runs[1..] {
        for &column in &timing {
            record[column].clear();
        }
    }
    runs
}

/// Returns `manifest` with its clock readings cleared: when the sweep started and finished, and when each session
/// started.
pub fn without_clocks(mut manifest: Manifest) -> Manifest {
    manifest.timestamps.started_unix_ms = 0;
    manifest.timestamps.started.clear();
    manifest.timestamps.finished_unix_ms = None;
    manifest.timestamps.finished = None;
    for session in &mut manifest.sessions {
        session.started.clear();
    }
    manifest
}

/// Files of a sweep or a search, with the timing columns of `runs.csv` emptied and the manifest's clock readings
/// cleared.
#[derive(Debug, PartialEq)]
pub struct UntimedFiles {
    /// Records of `runs.csv`.
    pub runs: Vec<Vec<String>>,
    /// Text of `series.csv`.
    pub series: String,
    /// Text of `summary.csv`.
    pub summary: String,
    /// Manifest read back.
    pub manifest: Manifest,
    /// Text of each table a search writes, with its file name.
    pub search_tables: Vec<(&'static str, String)>,
}

impl UntimedFiles {
    /// Returns `files` with their timings left out.
    pub fn new(files: &SweepFiles) -> Self {
        let text = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).expect("the files are UTF-8");
        let manifest: Manifest = serde_json::from_slice(&files.manifest).expect("the manifest reads back");
        Self {
            runs: without_timing(parse_records(&text(&files.runs)).expect("runs.csv is valid CSV")),
            series: text(&files.series),
            summary: text(&files.summary),
            manifest: without_clocks(manifest),
            search_tables: files
                .search_tables
                .iter()
                .map(|(name, bytes)| (*name, text(bytes)))
                .collect(),
        }
    }
}

/// Returns the files of the output directory `dir`: the four files of every sweep, then the search tables
/// `search_tables` names.
pub fn directory_files(dir: &Path, search_tables: &[&'static str]) -> SweepFiles {
    let read = |file: &str| std::fs::read(dir.join(file)).expect("the file is written");
    SweepFiles {
        runs: read(RUNS_FILE),
        series: read(SERIES_FILE),
        summary: read(SUMMARY_FILE),
        manifest: read(MANIFEST_FILE),
        search_tables: search_tables.iter().map(|&file| (file, read(file))).collect(),
    }
}

/// Returns the bytes of each of `files` in the output directory `dir`, with its file name.
pub fn file_bytes(dir: &Path, files: &[&str]) -> Vec<(String, Vec<u8>)> {
    files
        .iter()
        .map(|&file| {
            let bytes = std::fs::read(dir.join(file)).expect("the file is written");
            (file.to_owned(), bytes)
        })
        .collect()
}

/// Receives the events of `run` until the last event, and returns them.
///
/// Panics when the run has not ended within `patience`.
pub fn drain(run: &mut SweepRun, patience: Duration) -> Vec<SweepEvent> {
    let deadline = Instant::now() + patience;
    let mut events = Vec::new();
    while !run.is_ended() {
        assert!(Instant::now() < deadline, "the run did not end within {patience:?}");
        run.update(0.0);
        match run.try_recv() {
            Some(event) => events.push(event),
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    events
}

/// Drives `sweep` until it has nothing due, and returns the events `events` received meanwhile.
///
/// Panics when the sweep still has work due after `patience`.
pub fn pump_until_idle(sweep: &mut PumpedSweep, events: &Receiver<SweepEvent>, patience: Duration) -> Vec<SweepEvent> {
    let deadline = Instant::now() + patience;
    while matches!(sweep.pump(), Pace::Now) {
        assert!(Instant::now() < deadline, "the pumped sweep never went idle");
    }
    events.try_iter().collect()
}

/// Returns the positions of the timing columns in `header`, the header of a `runs.csv`.
///
/// The timing columns are found by their place among the outcome columns, the last run of [`OUTCOME_COLUMNS`] in the
/// header. A parameter named like a timing column is compared as any other.
///
/// # Panics
///
/// Panics when the header has no run of the outcome columns.
fn timing_positions(header: &[String]) -> Vec<usize> {
    let outcomes_start = header
        .windows(OUTCOME_COLUMNS.len())
        .rposition(|window| window.iter().zip(OUTCOME_COLUMNS).all(|(name, column)| name == column))
        .expect("runs.csv has the outcome columns");
    TIMING_COLUMNS
        .iter()
        .map(|timing| {
            let offset = OUTCOME_COLUMNS
                .iter()
                .position(|column| column == timing)
                .expect("a timing column is an outcome column");
            outcomes_start + offset
        })
        .collect()
}

/// Returns `text`, the text of a CSV table, with the fields at `positions` emptied in every record after the header.
///
/// Every other byte is kept. A quoted field can hold commas and line breaks, so the walk tracks the quotes to tell a
/// separator from the text of a field.
fn with_fields_emptied(text: &str, positions: &[usize]) -> String {
    let mut kept = String::with_capacity(text.len());
    let (mut in_header, mut position, mut quoted) = (true, 0, false);
    for character in text.chars() {
        let separator = !quoted && matches!(character, ',' | '\n');
        if character == '"' {
            quoted = !quoted;
        }
        if separator || in_header || !positions.contains(&position) {
            kept.push(character);
        }
        if character == ',' && separator {
            position += 1;
        } else if separator {
            in_header = false;
            position = 0;
        }
    }
    kept
}

/// Progress that aborts `control` once `limit` runs are committed.
///
/// On one lane the sweep then ends with exactly `limit` runs written. Its next run checks the control before it
/// starts.
pub struct CommitLimit {
    control: SweepControl,
    limit: usize,
    committed: usize,
}

impl CommitLimit {
    pub fn new(control: SweepControl, limit: usize) -> Self {
        Self {
            control,
            limit,
            committed: 0,
        }
    }
}

impl Progress for CommitLimit {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        if let ProgressEvent::RunCommitted(_) = event {
            self.committed += 1;
            if self.committed == self.limit {
                self.control.abort();
            }
        }
    }
}

/// Runs `spec` over `entry` into `output_dir` at `concurrency` and aborts it once `commits` runs are written, and
/// returns its report.
///
/// # Panics
///
/// Panics when the sweep fails.
pub fn interrupted_after(
    entry: &ModelEntry,
    spec: &SweepSpec,
    output_dir: &Path,
    concurrency: Concurrency,
    commits: usize,
) -> SweepReport {
    let control = SweepControl::new();
    let options = SweepOptions {
        concurrency,
        control: control.clone(),
        ..sweep_options(false)
    };
    sweep_with(
        entry,
        None,
        spec,
        output_dir,
        &options,
        &mut CommitLimit::new(control, commits),
    )
    .expect("an abort is not an error")
}

/// Time a test watches paused runs for. A run that kept stepping would move many times in it.
pub const HOLD_WINDOW: Duration = Duration::from_millis(500);

/// Reads `active_runs` over [`HOLD_WINDOW`], and returns every tick each run was seen at, by run id.
pub fn ticks_seen(active_runs: impl Fn() -> Vec<ActiveRun>) -> BTreeMap<u64, BTreeSet<u64>> {
    let mut seen: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
    let deadline = Instant::now() + HOLD_WINDOW;
    while Instant::now() < deadline {
        for active in active_runs() {
            seen.entry(active.run.run_id).or_default().insert(active.tick);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    seen
}

/// Returns the plan of `spec` over `entry`, and its measure plan bound to the columns of a probe build.
///
/// # Panics
///
/// Panics when the spec cannot be planned or the probe fails.
pub fn planned(entry: &ModelEntry, gpu: Option<&GpuContext>, spec: &SweepSpec) -> (Plan, Arc<MeasurePlan>) {
    let plan = spec.plan(&entry.schema()).expect("a valid spec");
    let probe = ProbeReport::for_plan(entry, gpu, &plan).expect("the probe builds");
    let measure =
        MeasurePlan::new(plan.run_settings(), plan.measure_settings(), probe.columns).expect("the columns bind");
    (plan, Arc::new(measure))
}

/// Runs every run of `plan` on an executor with `layout`, and commits each outcome to `sink`.
///
/// # Panics
///
/// Panics when the batch cannot run or does not complete.
pub fn run_plan(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    plan: &Plan,
    measure: &Arc<MeasurePlan>,
    layout: ExecutionLayout,
    sink: &mut dyn RunSink,
) {
    let executor =
        Executor::new(entry, gpu, Arc::clone(measure), layout, SweepControl::new()).expect("the executor builds");
    run_batch(&executor, plan, sink);
}

/// Runs every run of `plan` on `executor`, and commits each outcome to `sink`.
///
/// # Panics
///
/// Panics when the batch cannot run or does not complete.
pub fn run_batch(executor: &Executor<'_>, plan: &Plan, sink: &mut dyn RunSink) {
    let requests: Vec<RunRequest<'_>> = plan.runs().map(|run| RunRequest::planned(plan, run)).collect();
    let end = executor.run_batch(&requests, sink).expect("the batch runs");
    assert_eq!(end, BatchEnd::Complete);
}

/// Returns `outcome` with its timings zeroed, since `runs.csv` rounds them.
pub fn untimed(outcome: &RunOutcome) -> RunOutcome {
    RunOutcome {
        build_ms: 0.0,
        wall_ms: 0.0,
        ..outcome.clone()
    }
}

/// Final tick of every case of a sampling-cadence test. It is not a multiple of the coarser cadence, so the final tick
/// is sampled in addition.
pub const CADENCE_STEPS: u64 = 25;

/// Cadences each case of a sampling-cadence test compares.
pub const CADENCES: [u64; 2] = [1, 10];

/// Rows of a series, each as its tick and the bits of its values.
pub type RowBits = Vec<(u64, Vec<u64>)>;

/// Runs `model` at the fixed values `fixed_values` for [`CADENCE_STEPS`] steps from seed root 5 on `layout`, sampled
/// every `stats_every` ticks, and returns the rows sampled at the ticks every cadence of [`CADENCES`] samples, each as
/// its tick and value bits, with the plan and the measure plan of the run.
///
/// # Panics
///
/// Panics when the run stops before its last step.
pub fn cadence_rows(
    model: &ModelEntry,
    gpu: Option<&GpuContext>,
    layout: ExecutionLayout,
    fixed_values: &[(&str, &str)],
    stats_every: u64,
) -> (RowBits, Plan, Arc<MeasurePlan>) {
    let mut spec = SweepSpec::new(model.id().to_owned());
    spec.fixed = fixed(fixed_values);
    spec.run.steps = CADENCE_STEPS;
    spec.measure.stats_every = stats_every;
    spec.measure.series_every = stats_every;
    spec.seeds.root = 5;
    let (plan, measure) = planned(model, gpu, &spec);
    let mut collected = Collected::default();
    run_plan(model, gpu, &plan, &measure, layout, &mut collected);
    let outcome = &collected.0[0];
    assert_eq!(outcome.ticks, CADENCE_STEPS, "{:?}", outcome.note);
    let rows = outcome
        .series
        .rows()
        .filter(|&(tick, _)| tick == CADENCE_STEPS || tick % CADENCES[1] == 0)
        .map(|(tick, values)| (tick, values.iter().map(|value| value.to_bits()).collect()))
        .collect();
    (rows, plan, measure)
}

/// Asserts that `cases` names every example model that `wanted` picks and no other, for the test `test`.
pub fn assert_covers(cases: &[&str], wanted: impl Fn(&ModelEntry) -> bool, test: &str) {
    let models = example_models();
    let mut registered: Vec<&str> = models
        .iter()
        .filter(|model| wanted(model))
        .map(ModelEntry::id)
        .collect();
    registered.sort_unstable();
    let mut covered = cases.to_vec();
    covered.sort_unstable();
    assert_eq!(covered, registered, "{test}");
}

/// Sink that keeps every committed outcome.
#[derive(Debug, Default)]
pub struct Collected(pub Vec<RunOutcome>);

impl RunSink for Collected {
    fn commit(&mut self, outcome: RunOutcome) -> io::Result<()> {
        self.0.push(outcome);
        Ok(())
    }
}

/// Layout of `cpu_lanes` CPU lanes of `threads_per_lane` threads each.
pub fn lanes(cpu_lanes: usize, threads_per_lane: usize) -> ExecutionLayout {
    ExecutionLayout {
        cpu_lanes,
        threads_per_lane,
        gpu_tracks: 0,
    }
}

/// Layout of `count` GPU tracks.
pub fn tracks(count: usize) -> ExecutionLayout {
    ExecutionLayout {
        cpu_lanes: 0,
        threads_per_lane: 0,
        gpu_tracks: count,
    }
}

/// Layout of one GPU track.
pub const ONE_TRACK: ExecutionLayout = ExecutionLayout {
    cpu_lanes: 0,
    threads_per_lane: 0,
    gpu_tracks: 1,
};

#[test]
fn only_the_timing_fields_of_the_outcome_columns_are_emptied() {
    let header = concat!(
        "run_id,build_ms,",
        "status,stop_reason,ticks,population,build_ms,wall_ms,steps_per_s,",
        "Infected:max,note\n"
    );
    let record = "0,7,ok,steps,30,256,1.5,2.25,13333,\"4, or \"\"5\"\"\",\"line\r\nbreak\"\r\n";
    let text = format!("{header}{record}");
    let header_fields = &parse_records(&text).expect("valid CSV")[0];
    assert_eq!(
        timing_positions(header_fields),
        [6, 7, 8],
        "the parameter named build_ms is kept"
    );
    assert_eq!(
        with_fields_emptied(&text, &[6, 7, 8]),
        format!("{header}0,7,ok,steps,30,256,,,,\"4, or \"\"5\"\"\",\"line\r\nbreak\"\r\n"),
        "every other byte is kept, quotes and line endings included"
    );
}
