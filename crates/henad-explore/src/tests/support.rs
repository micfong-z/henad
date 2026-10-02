//! Headless device, scratch directories and sweep helpers for the tests.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use henad_compute::entry::ModelEntry;
use henad_compute::gpu::GpuContext;
use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::Plan;
use henad_core::explore::spec::SweepSpec;
use henad_core::export::csv::parse_records;
use henad_models::example_models;

use henad_compute::fault::{FaultSink, install_panic_hook};

use crate::device::acquire_headless;
use crate::exec::{ActiveRun, BatchEnd, Concurrency, ExecutionLayout, Executor, RunRequest, RunSink, SweepControl};
use crate::handle::SweepOutput;
use crate::output::manifest::{Manifest, RecordedBuild};
use crate::output::runs_csv::{OUTCOME_COLUMNS, TIMING_COLUMNS};
use crate::output::{MANIFEST_FILE, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
use crate::probe::ProbeReport;
use crate::progress::{NoProgress, Progress, ProgressEvent};
use crate::sweep::{ExploreError, Provenance, SweepOptions, SweepReport, SweepWarning, plan_spec, run_spec};

const REQUIRE_GPU: &str = "HENAD_REQUIRE_GPU";

/// Returns whether `HENAD_REQUIRE_GPU` is set to something other than empty or 0.
fn gpu_required() -> bool {
    std::env::var_os(REQUIRE_GPU).is_some_and(|value| !value.is_empty() && value != "0")
}

/// Returns a headless device, or `None` to skip a GPU test on a machine without one.
///
/// # Panics
///
/// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
pub fn headless_device() -> Option<GpuContext> {
    match acquire_headless(example_models().gpu_needs()) {
        Ok(ctx) => Some(ctx),
        Err(error) => {
            assert!(!gpu_required(), "{REQUIRE_GPU} is set but {error}");
            None
        }
    }
}

/// Returns a headless device with the limits of `wgpu::Limits::default()`, the WebGPU baseline, or `None` to skip a
/// GPU test on a machine without one.
///
/// # Panics
///
/// Panics when `HENAD_REQUIRE_GPU` is set and no device is available.
pub fn baseline_device() -> Option<GpuContext> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let device = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
        .map_err(|error| error.to_string())
        .and_then(|adapter| {
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("henad-explore-baseline"),
                ..Default::default()
            }))
            .map_err(|error| error.to_string())
        });
    match device {
        Ok((device, queue)) => Some(GpuContext::new(
            device,
            queue,
            wgpu::TextureFormat::Rgba8Unorm,
            FaultSink::new(),
        )),
        Err(error) => {
            assert!(!gpu_required(), "{REQUIRE_GPU} is set but {error}");
            None
        }
    }
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

/// Path under the system's temporary directory, unique to one test, removed with its contents on drop.
pub struct ScratchDir {
    path: PathBuf,
}

impl ScratchDir {
    /// Returns a path named after `name` that nothing exists at yet.
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

/// Returns an engine build that [`RecordedBuild::same_build`] tells apart from Henad's own, clean or dirty.
pub fn other_engine() -> RecordedBuild {
    let mut other = RecordedBuild::engine();
    other.commit = "0ther000".to_owned();
    other.dirty = Some(false);
    other.source_hash = Some(other.source_hash.unwrap_or_default() ^ 1);
    other
}

/// Returns the provenance of a sweep the tests run, with henad-explore's own build as the host's.
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

/// Reads the manifest of the output directory `dir`.
///
/// # Panics
///
/// Panics when the manifest is missing or unreadable.
pub fn manifest(dir: &Path) -> Manifest {
    Manifest::read(&dir.join(MANIFEST_FILE)).expect("the manifest reads back")
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
    let requests: Vec<RunRequest<'_>> = plan.runs().map(|run| RunRequest::planned(plan, run)).collect();
    let end = executor.run_batch(&requests, sink).expect("the batch runs");
    assert_eq!(end, BatchEnd::Complete);
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
