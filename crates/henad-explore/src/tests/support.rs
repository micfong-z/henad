//! Headless device, scratch directories and sweep helpers for the tests.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use henad_compute::gpu::GpuContext;
use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::plan::Plan;
use henad_core::explore::spec::SweepSpec;
use henad_core::export::csv::parse_records;
use henad_models::registry::{ModelEntry, model_registry};

use henad_compute::fault::FaultSink;

use crate::device::acquire_headless;
use crate::exec::{BatchEnd, Concurrency, ExecutionLayout, Executor, RunRequest, RunSink, SweepControl};
use crate::output::manifest::Manifest;
use crate::output::runs_csv::TIMING_COLUMNS;
use crate::output::{MANIFEST_FILE, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
use crate::probe::ProbeReport;
use crate::progress::{NoProgress, Progress, ProgressEvent};
use crate::schema::model_schema;
use crate::sweep::{ExploreError, Provenance, SpecSource, SweepOptions, SweepReport, SweepWarning, run_sweep};

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
    match acquire_headless() {
        Ok((ctx, _)) => Some(ctx),
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

/// Returns the registry entry of model `id`, registered with `gpu` as its device.
///
/// # Panics
///
/// Panics when no model has the id.
pub fn entry(id: &str, gpu: Option<&GpuContext>) -> ModelEntry {
    model_registry(gpu.cloned())
        .into_iter()
        .find(|entry| entry.id == id)
        .expect("the model is registered")
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

/// Returns a record of a build that is not a real one.
pub fn provenance() -> Provenance {
    Provenance {
        engine_name: "henad".to_owned(),
        engine_version: "0.0.0-test".to_owned(),
        commit: "test".to_owned(),
        commit_date: String::new(),
        debug_build: cfg!(debug_assertions),
        argv: vec!["henad-explore-tests".to_owned()],
    }
}

/// Runs `spec` over `entry` into `output_dir` at `concurrency`, reporting nowhere.
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
    let options = SweepOptions {
        output_dir: Some(output_dir.to_owned()),
        concurrency,
        ..SweepOptions::default()
    };
    run_sweep(
        entry,
        gpu,
        None,
        spec,
        &SpecSource::default(),
        &provenance(),
        &options,
        &mut NoProgress,
    )
    .expect("the sweep runs")
}

/// Runs `spec` over `entry` with `options`, reporting to `progress`.
///
/// # Errors
///
/// Returns the error of the sweep.
pub fn sweep_with(
    entry: &ModelEntry,
    gpu: Option<&GpuContext>,
    spec: &SweepSpec,
    options: &SweepOptions,
    progress: &mut dyn Progress,
) -> Result<SweepReport, ExploreError> {
    run_sweep(
        entry,
        gpu,
        None,
        spec,
        &SpecSource::default(),
        &provenance(),
        options,
        progress,
    )
}

/// Returns the options of a sweep into `output_dir`, resumed when `resume` is set.
pub fn sweep_options(output_dir: &Path, resume: bool) -> SweepOptions {
    SweepOptions {
        output_dir: Some(output_dir.to_owned()),
        resume,
        ..SweepOptions::default()
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

/// Reads the manifest of the output directory `dir`.
///
/// # Panics
///
/// Panics when the manifest is missing or unreadable.
pub fn manifest(dir: &Path) -> Manifest {
    Manifest::read(&dir.join(MANIFEST_FILE)).expect("the manifest reads back")
}

/// Three CSV files of an output directory, each as records of fields.
#[derive(Debug, PartialEq, Eq)]
pub struct Tables {
    /// Records of `runs.csv`, with the timing columns emptied.
    pub runs: Vec<Vec<String>>,
    pub series: Vec<Vec<String>>,
    pub summary: Vec<Vec<String>>,
}

impl Tables {
    /// Reads the tables of the output directory `dir`.
    ///
    /// # Panics
    ///
    /// Panics when a table is missing or is not valid CSV.
    pub fn read(dir: &Path) -> Self {
        let read = |file: &str| {
            let text = std::fs::read_to_string(dir.join(file)).expect("the table is written");
            parse_records(&text).expect("the table is valid CSV")
        };
        Self {
            runs: without_timing(read(RUNS_FILE)),
            series: read(SERIES_FILE),
            summary: read(SUMMARY_FILE),
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
/// Panics when the header lacks a timing column.
pub fn without_timing(mut runs: Vec<Vec<String>>) -> Vec<Vec<String>> {
    let timing: Vec<usize> = runs[0]
        .iter()
        .enumerate()
        .filter(|(_, name)| TIMING_COLUMNS.contains(&name.as_str()))
        .map(|(column, _)| column)
        .collect();
    assert_eq!(timing.len(), TIMING_COLUMNS.len(), "runs.csv has every timing column");
    for record in &mut runs[1..] {
        for &column in &timing {
            record[column].clear();
        }
    }
    runs
}

/// Returns the plan of `spec` over `entry`, and its measure plan bound to the columns of a probe build.
///
/// # Panics
///
/// Panics when the spec cannot be planned or the probe fails.
pub fn planned(entry: &ModelEntry, gpu: Option<&GpuContext>, spec: &SweepSpec) -> (Plan, Arc<MeasurePlan>) {
    let plan = spec.plan(&model_schema(entry)).expect("a valid spec");
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
