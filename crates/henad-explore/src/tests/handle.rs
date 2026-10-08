//! Checks the sweep handle: its events and progress, pause and abort, both outputs, resume, and the pumped sweep a
//! browser runs.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use henad_compute::entry::{ModelEntry, register_gpu_grid_model};
use henad_compute::fault::Fault;
use henad_compute::runner::{Pace, SimLoop as _};
use henad_core::authoring::model::binding::BindingDecl;
use henad_core::authoring::model::gpu_grid_model::{GpuGridAction, GpuGridModel};
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
use henad_core::explore::stop::StopSpec;
use henad_core::export::csv::parse_records;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::view::{StatDescriptor, StatValue};
use henad_models::gpu_sir::GpuSir;

use crate::exec::{Concurrency, SweepControl};
use crate::handle::{SweepChannel, SweepEvent, SweepOutput, SweepPhase, SweepRun, SweepRunOptions, SweepStartError};
use crate::output::manifest::{Manifest, ManifestStatus};
use crate::output::memory::SweepFiles;
use crate::output::{MANIFEST_FILE, OutputError, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
use crate::pumped::PumpedSweep;
use crate::result_set::ResultSet;
use crate::sweep::{SweepEnd, SweepOptions, SweepRecord};
use crate::tests::support::{
    CommitLimit, ScratchDir, entry, headless_device, provenance, sweep, sweep_options, sweep_with, ticks_seen,
    without_clocks, without_timing,
};

/// Longest a test waits for a sweep to reach a state.
const PATIENCE: Duration = Duration::from_secs(60);

fn values(raw: &[&str]) -> LevelSpec {
    LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
}

fn fixed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(id, value)| (id.to_owned(), value.to_owned()))
        .collect()
}

/// Returns 3 configs of SIR on a 16 by 16 grid with `replicates` replicates of 30 steps, and an action whose tick
/// varies.
fn sir_spec(replicates: u64) -> SweepSpec {
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

/// Returns 2 runs of Game of Life on a 64 by 64 grid that step for far longer than any test waits.
fn endless_spec() -> SweepSpec {
    let mut spec = SweepSpec::new("game_of_life");
    spec.fixed = fixed(&[("grid_width", "64"), ("grid_height", "64")]);
    spec.run.steps = 1 << 40;
    spec.run.replicates = 2;
    spec.measure.stats_every = 1 << 40;
    spec.measure.series_every = 0;
    spec
}

/// Returns 16 runs of Game of Life on a 64 by 64 grid, under the stop condition `Alive <= 0`.
///
/// The first 8 start empty and stop at tick 0. The last 8 start at a density of 0.3 and step as the runs of
/// [`endless_spec`] do, for far longer than any test waits. Every run samples at tick 0 and at its end only.
fn stopping_then_endless_spec() -> SweepSpec {
    let mut spec = endless_spec();
    spec.run.replicates = 8;
    spec.run.stop = Some(StopSpec::parse("Alive <= 0", 0).expect("a valid condition"));
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param("density", values(&["0", "0.3"]))],
        design_seed: None,
    }];
    spec
}

fn options() -> SweepRunOptions {
    SweepRunOptions::new(provenance())
}

/// Receives the events of `run` until its last one, and returns them.
///
/// # Panics
///
/// Panics when the sweep has not ended within [`PATIENCE`].
fn drain(run: &mut SweepRun) -> Vec<SweepEvent> {
    let deadline = Instant::now() + PATIENCE;
    let mut events = Vec::new();
    while !run.is_ended() {
        assert!(Instant::now() < deadline, "the sweep did not end in time");
        run.update(0.0);
        match run.try_recv() {
            Some(event) => events.push(event),
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    events
}

/// Waits until `holds` holds for the progress of `run`.
///
/// # Panics
///
/// Panics when it does not within [`PATIENCE`].
fn wait_for(run: &SweepRun, condition: &str, holds: impl Fn(&crate::handle::SweepProgress) -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while !holds(&run.progress()) {
        assert!(Instant::now() < deadline, "the sweep never reached: {condition}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Returns the record the events end with.
///
/// # Panics
///
/// Panics when they end on a failure.
fn record(events: &[SweepEvent]) -> &SweepRecord {
    match events.last() {
        Some(SweepEvent::Finished(record)) => record,
        Some(SweepEvent::Failed(message)) => panic!("the sweep failed: {message}"),
        _ => panic!("the events have no end"),
    }
}

/// Returns the outcomes the events carry, in the order they arrived.
fn outcomes(events: &[SweepEvent]) -> Vec<&RunOutcome> {
    events
        .iter()
        .filter_map(|event| match event {
            SweepEvent::RunFinished { outcome, .. } => Some(&**outcome),
            _ => None,
        })
        .collect()
}

/// Four files of a sweep, with the timing columns of `runs.csv` emptied and the manifest's clock readings cleared.
#[derive(Debug, PartialEq)]
struct UntimedFiles {
    runs: Vec<Vec<String>>,
    series: String,
    summary: String,
    manifest: Manifest,
}

impl UntimedFiles {
    fn new(files: &SweepFiles) -> Self {
        let text = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).expect("the files are UTF-8");
        let manifest: Manifest = serde_json::from_slice(&files.manifest).expect("the manifest reads back");
        Self {
            runs: without_timing(parse_records(&text(&files.runs)).expect("runs.csv is valid CSV")),
            series: text(&files.series),
            summary: text(&files.summary),
            manifest: without_clocks(manifest),
        }
    }
}

/// Returns the four files of the output directory `dir`.
fn directory_files(dir: &Path) -> SweepFiles {
    let read = |file: &str| std::fs::read(dir.join(file)).expect("the file is written");
    SweepFiles {
        runs: read(RUNS_FILE),
        series: read(SERIES_FILE),
        summary: read(SUMMARY_FILE),
        manifest: read(MANIFEST_FILE),
        search_tables: Vec::new(),
    }
}

/// Runs `spec` over `entry` through a handle into `output`, and returns its events.
fn run_handle(entry: ModelEntry, spec: SweepSpec, output: SweepOutput, options: SweepRunOptions) -> Vec<SweepEvent> {
    let mut run = SweepRun::start(entry, None, spec, output, options).expect("the sweep starts");
    drain(&mut run)
}

#[test]
fn a_sweep_sends_its_outline_every_run_in_order_and_its_record() {
    let wakes = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&wakes);
    let options = SweepRunOptions {
        wake: Some(Arc::new(move || {
            counter.fetch_add(1, Ordering::Relaxed);
        })),
        ..options()
    };
    let mut run =
        SweepRun::start(entry("sir", None), None, sir_spec(2), SweepOutput::Memory, options).expect("the sweep starts");
    assert_eq!(run.plan().run_count(), 6);
    let events = drain(&mut run);

    let SweepEvent::Planned(outline) = &events[0] else {
        panic!("the outline comes first");
    };
    assert_eq!((outline.pending, outline.skipped), (6, 0));
    assert_eq!(outline.stat_columns, ["Susceptible", "Infected", "Recovered"]);
    let ids: Vec<u64> = outcomes(&events).iter().map(|outcome| outcome.run.run_id).collect();
    assert_eq!(ids, [0, 1, 2, 3, 4, 5]);
    assert!(events.iter().all(|event| !matches!(
        event,
        SweepEvent::RunFinished {
            series_dropped: true,
            ..
        }
    )));

    let record = record(&events);
    assert_eq!(record.report.end, SweepEnd::Complete);
    assert_eq!(
        (record.report.counts.rows, record.report.output_dir.as_ref()),
        (6, None)
    );
    assert_eq!(record.manifest.status, ManifestStatus::Complete);
    assert_eq!(record.manifest.columns.reducers, outline.reducer_columns);
    let files = record.files.as_ref().expect("a sweep in memory hands its files over");
    assert_eq!(String::from_utf8_lossy(&files.runs).lines().count(), 7);

    let progress = run.progress();
    assert_eq!(progress.phase, SweepPhase::Ended(SweepEnd::Complete));
    assert_eq!(
        (progress.runs_total, progress.runs_done, progress.runs_failed),
        (6, 6, 0)
    );
    assert_eq!(progress.remaining, None);
    assert!(progress.active_runs.is_empty());
    assert!(run.try_recv().is_none(), "nothing follows the record");
    // The sweep's thread wakes the host after it sends an event. Dropping the handle joins that thread, and the wake
    // of the last event has run by then.
    drop(run);
    assert_eq!(
        wakes.load(Ordering::Relaxed),
        events.len(),
        "every event wakes the host once"
    );

    // The events and the files agree, the series included.
    let set = ResultSet::from_files(files.clone().into_entries(), usize::MAX).expect("the files read back");
    for (recorded, sent) in set.runs().iter().zip(outcomes(&events)) {
        let untimed = |outcome: &RunOutcome| RunOutcome {
            build_ms: 0.0,
            wall_ms: 0.0,
            ..outcome.clone()
        };
        assert_eq!(untimed(&recorded.outcome), untimed(sent));
    }
}

#[test]
fn memory_output_equals_directory_output() {
    let scratch = ScratchDir::new("handle-outputs");
    let handle_dir = scratch.path().join("handle");
    let cli_dir = scratch.path().join("cli");
    let spec = sir_spec(2);

    let in_memory = run_handle(entry("sir", None), spec.clone(), SweepOutput::Memory, options());
    let in_directory = run_handle(
        entry("sir", None),
        spec.clone(),
        SweepOutput::Directory(handle_dir.clone()),
        options(),
    );
    let record = record(&in_directory);
    assert_eq!(record.report.output_dir.as_deref(), Some(handle_dir.as_path()));
    assert!(record.files.is_none(), "a directory keeps its files");
    sweep(&entry("sir", None), None, &spec, &cli_dir, Concurrency::Auto);

    let memory = UntimedFiles::new(self::record(&in_memory).files.as_ref().expect("files in memory"));
    // Every byte matches apart from the timing columns and the clock readings of the manifest.
    assert_eq!(memory, UntimedFiles::new(&directory_files(&handle_dir)));
    assert_eq!(memory, UntimedFiles::new(&directory_files(&cli_dir)));
}

/// Drives `sweep` until it has nothing due, and returns the events `events` received meanwhile.
fn pump_until_idle(sweep: &mut PumpedSweep, events: &std::sync::mpsc::Receiver<SweepEvent>) -> Vec<SweepEvent> {
    let deadline = Instant::now() + PATIENCE;
    while matches!(sweep.pump(), Pace::Now) {
        assert!(Instant::now() < deadline, "the pumped sweep never went idle");
    }
    events.try_iter().collect()
}

#[test]
fn the_pumped_sweep_writes_what_a_directory_sweep_writes() {
    let spec = sir_spec(2);
    let sir = entry("sir", None);
    let plan = Arc::new(spec.plan(&sir.schema()).expect("a valid spec"));
    let (channel, events, _) = SweepChannel::open(&options());
    let mut pumped = PumpedSweep::new(sir, spec.clone(), plan, None, channel, options()).expect("a CPU model pumps");
    let pumped_events = pump_until_idle(&mut pumped, &events);
    let ids: Vec<u64> = outcomes(&pumped_events)
        .iter()
        .map(|outcome| outcome.run.run_id)
        .collect();
    assert_eq!(ids, [0, 1, 2, 3, 4, 5]);
    let pumped_record = record(&pumped_events);
    assert_eq!(pumped_record.report.outline.layout.cpu_lanes, 1);

    // The pumped sweep runs one run at a time, recorded as one lane.
    let scratch = ScratchDir::new("handle-pumped");
    let one_lane = Concurrency::Fixed(std::num::NonZeroUsize::MIN);
    sweep(&entry("sir", None), None, &spec, scratch.path(), one_lane);
    assert_eq!(
        UntimedFiles::new(pumped_record.files.as_ref().expect("files in memory")),
        UntimedFiles::new(&directory_files(scratch.path()))
    );
    assert!(matches!(pumped.pump(), Pace::Idle), "an ended sweep has nothing due");
}

#[test]
fn a_paused_pumped_sweep_does_no_work_until_resumed() {
    let spec = endless_spec();
    let life = entry("game_of_life", None);
    let plan = Arc::new(spec.plan(&life.schema()).expect("a valid spec"));
    let (channel, events, _) = SweepChannel::open(&options());
    let mut pumped = PumpedSweep::new(life, spec, plan, None, channel, options()).expect("a CPU model pumps");
    for _ in 0..20 {
        assert!(matches!(pumped.pump(), Pace::Now));
    }
    let reached = pumped.active_runs().list();
    assert_eq!(reached.len(), 1, "one run at a time");
    assert!(reached[0].tick > 0, "the run has stepped");

    pumped.handle_command(crate::pumped::SweepCommand::Pause);
    for _ in 0..5 {
        assert!(matches!(pumped.pump(), Pace::Idle), "a paused sweep has nothing due");
    }
    assert_eq!(pumped.active_runs().list(), reached, "a paused run keeps its tick");

    pumped.handle_command(crate::pumped::SweepCommand::Resume);
    assert!(matches!(pumped.pump(), Pace::Now));
    assert!(pumped.active_runs().list()[0].tick > reached[0].tick);

    pumped.handle_command(crate::pumped::SweepCommand::Abort);
    let received = pump_until_idle(&mut pumped, &events);
    let record = record(&received);
    assert_eq!(record.report.end, SweepEnd::Aborted);
    assert_eq!(record.report.counts.rows, 0);
    assert!(outcomes(&received).is_empty(), "no run reached its end");
    assert!(pumped.active_runs().list().is_empty());
}

#[test]
fn the_pumped_sweep_refuses_a_gpu_model() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let spec = SweepSpec::new("gpu_sir");
    let plan = Arc::new(spec.plan(&gpu_sir.schema()).expect("a valid spec"));
    let (channel, _events, _) = SweepChannel::open(&options());
    let refused = PumpedSweep::new(gpu_sir, spec, plan, None, channel, options());
    assert!(matches!(refused, Err(SweepStartError::GpuNeedsNative)));
}

#[test]
fn a_paused_sweep_holds_every_run_until_resumed() {
    // Two lanes step both runs at once, on a machine of one worker too.
    let options = SweepRunOptions {
        concurrency: Concurrency::Fixed(std::num::NonZeroUsize::new(2).expect("2 is above 0")),
        ..options()
    };
    let mut run = SweepRun::start(
        entry("game_of_life", None),
        None,
        endless_spec(),
        SweepOutput::Memory,
        options,
    )
    .expect("the sweep starts");
    wait_for(&run, "both runs stepping", |progress| {
        progress.active_runs.len() == 2 && progress.active_runs.iter().all(|active| active.tick > 0)
    });

    run.pause();
    let paused = run.progress();
    assert_eq!(paused.phase, SweepPhase::Paused);
    // A lane finishes the slice in flight, records its tick and holds at its next check of the control. Each run
    // therefore moves at most once after the pause, however long that slice takes.
    let seen = ticks_seen(|| run.progress().active_runs);
    let held = run.progress();
    assert_eq!(seen.len(), 2, "both runs stay in progress: {seen:?}");
    assert!(
        seen.values().all(|ticks| ticks.len() <= 2),
        "no run steps while paused: {seen:?}"
    );
    assert_eq!(held.elapsed, paused.elapsed, "the clock stops while paused");

    run.resume();
    assert_eq!(run.progress().phase, SweepPhase::Running);
    wait_for(&run, "a run stepping again", |progress| {
        progress
            .active_runs
            .iter()
            .zip(&held.active_runs)
            .any(|(now, before)| now.tick > before.tick)
    });

    run.abort();
    let events = drain(&mut run);
    assert_eq!(record(&events).report.end, SweepEnd::Aborted);
    assert!(outcomes(&events).is_empty(), "no run reached its end");
    let progress = run.progress();
    assert_eq!(progress.phase, SweepPhase::Ended(SweepEnd::Aborted));
    assert!(progress.active_runs.is_empty());
}

#[test]
fn an_aborted_sweep_finishes_with_the_runs_so_far() {
    let mut run = SweepRun::start(
        entry("game_of_life", None),
        None,
        stopping_then_endless_spec(),
        SweepOutput::Memory,
        options(),
    )
    .expect("the sweep starts");
    let mut events = Vec::new();
    let deadline = Instant::now() + PATIENCE;
    while outcomes(&events).len() < 3 {
        assert!(Instant::now() < deadline, "three runs never finished");
        match run.try_recv() {
            Some(event) => events.push(event),
            None => std::thread::sleep(Duration::from_millis(1)),
        }
    }
    run.abort();
    events.extend(drain(&mut run));

    let finished = outcomes(&events).len() as u64;
    let record = record(&events);
    assert_eq!(record.report.end, SweepEnd::Aborted);
    assert_eq!(record.manifest.status, ManifestStatus::Aborted);
    assert_eq!(record.report.counts.rows, finished);
    assert!(finished <= 8, "no endless run finished: {finished}");
    let ids: Vec<u64> = outcomes(&events).iter().map(|outcome| outcome.run.run_id).collect();
    assert_eq!(ids, (0..finished).collect::<Vec<_>>(), "the runs so far are a prefix");

    let files = record.files.clone().expect("files in memory");
    let set = ResultSet::from_files(files.into_entries(), usize::MAX).expect("the files read back");
    assert_eq!(set.runs().len() as u64, finished);
    assert!(!set.is_complete());
    assert_eq!(run.progress().runs_done, finished);
}

#[test]
fn series_past_the_budget_arrive_without_their_series() {
    let events = run_handle(entry("sir", None), sir_spec(2), SweepOutput::Memory, options());
    let series = &outcomes(&events)[0].series;
    let run_bytes = series.len() * (series.width() + 1) * size_of::<f64>();

    let budgeted = SweepRunOptions {
        series_budget: 2 * run_bytes,
        ..options()
    };
    let events = run_handle(entry("sir", None), sir_spec(2), SweepOutput::Memory, budgeted);
    let held: Vec<(usize, bool)> = events
        .iter()
        .filter_map(|event| match event {
            SweepEvent::RunFinished {
                outcome,
                series_dropped,
            } => Some((outcome.series.len(), *series_dropped)),
            _ => None,
        })
        .collect();
    let rows = series.len();
    assert_eq!(
        held,
        [(rows, false), (rows, false), (0, true), (0, true), (0, true), (0, true)]
    );
    let files = record(&events).files.clone().expect("files in memory");
    assert_eq!(
        String::from_utf8_lossy(&files.series).lines().count(),
        1 + 6 * rows,
        "the files keep every series"
    );
}

#[test]
fn a_sweep_that_cannot_run_is_refused_at_its_start() {
    let refused = SweepRun::start(
        entry("game_of_life", None),
        None,
        sir_spec(1),
        SweepOutput::Memory,
        options(),
    );
    assert!(matches!(refused, Err(SweepStartError::ModelMismatch { .. })));

    let mut unknown = sir_spec(1);
    unknown.blocks[0].factors[0] = FactorSpec::param("no_such_param", values(&["1", "2", "3"]));
    let refused = SweepRun::start(entry("sir", None), None, unknown, SweepOutput::Memory, options());
    assert!(matches!(refused, Err(SweepStartError::Plan(_))));

    let scratch = ScratchDir::new("handle-refused");
    sweep(
        &entry("sir", None),
        None,
        &sir_spec(1),
        scratch.path(),
        Concurrency::Auto,
    );
    let refused = SweepRun::start(
        entry("sir", None),
        None,
        sir_spec(1),
        SweepOutput::Directory(scratch.path().to_owned()),
        options(),
    );
    assert!(matches!(
        refused,
        Err(SweepStartError::Output(OutputError::HoldsResults { .. }))
    ));
}

#[test]
fn a_resumed_directory_runs_only_the_runs_it_lacks() {
    let scratch = ScratchDir::new("handle-resume");
    let fresh_dir = scratch.path().join("fresh");
    let resumed_dir = scratch.path().join("resumed");
    let spec = sir_spec(2);
    sweep(&entry("sir", None), None, &spec, &fresh_dir, Concurrency::Auto);

    // One lane aborts once two runs are written, before the third starts.
    let control = SweepControl::new();
    let aborted = SweepOptions {
        concurrency: Concurrency::Fixed(std::num::NonZeroUsize::MIN),
        control: control.clone(),
        ..sweep_options(false)
    };
    let report = sweep_with(
        &entry("sir", None),
        None,
        &spec,
        &resumed_dir,
        &aborted,
        &mut CommitLimit::new(control, 2),
    )
    .expect("an abort is not an error");
    assert_eq!(report.end, SweepEnd::Aborted);
    let kept = report.counts.rows;
    assert_eq!(kept, 2, "the directory lacks runs for the resume to run");

    let mut run =
        SweepRun::resume_directory(entry("sir", None), None, &resumed_dir, options()).expect("the resume starts");
    let events = drain(&mut run);
    let SweepEvent::Planned(outline) = &events[0] else {
        panic!("the outline comes first");
    };
    assert_eq!((outline.skipped, outline.pending), (kept, 6 - kept));
    let ids: Vec<u64> = outcomes(&events).iter().map(|outcome| outcome.run.run_id).collect();
    assert_eq!(ids, (kept..6).collect::<Vec<_>>());
    assert_eq!(record(&events).manifest.status, ManifestStatus::Complete);
    assert_eq!(
        UntimedFiles::new(&directory_files(&resumed_dir)).runs,
        UntimedFiles::new(&directory_files(&fresh_dir)).runs
    );
    let resumed = UntimedFiles::new(&directory_files(&resumed_dir));
    let fresh = UntimedFiles::new(&directory_files(&fresh_dir));
    assert_eq!((resumed.series, resumed.summary), (fresh.series, fresh.summary));
}

/// Returns 2 runs of GPU SIR on a 32 by 32 grid of 20 steps.
fn gpu_sir_spec() -> SweepSpec {
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed(&[("grid_width", "32"), ("grid_height", "32")]);
    spec.run.steps = 20;
    spec.run.replicates = 2;
    spec.measure.stats_every = 4;
    spec.measure.series_every = 4;
    spec
}

#[test]
fn a_gpu_sweep_runs_on_the_handle_thread() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let spec = gpu_sir_spec();
    let scratch = ScratchDir::new("handle-gpu");
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    sweep(&gpu_sir, Some(&ctx), &spec, scratch.path(), Concurrency::Auto);

    let mut run = SweepRun::start(gpu_sir, Some(ctx), spec, SweepOutput::Memory, options()).expect("the sweep starts");
    let events = drain(&mut run);
    let record = record(&events);
    assert_eq!((record.report.end, record.report.counts.ok), (SweepEnd::Complete, 2));
    let memory = UntimedFiles::new(record.files.as_ref().expect("files in memory"));
    let directory = UntimedFiles::new(&directory_files(scratch.path()));
    assert_eq!(memory, directory);
}

#[test]
fn a_gpu_sweep_handed_no_device_steps_on_its_own() {
    let Some(host) = headless_device() else {
        return;
    };
    host.faults
        .set_once(Fault::refused("drawing the viewport", "a fault of the host's"));

    let gpu_sir = entry("gpu_sir", Some(&host));
    let budgets = SweepRunOptions {
        memory_budget: Some(1 << 30),
        gpu_memory_budget: Some(1 << 26),
        ..options()
    };
    let mut run =
        SweepRun::start(gpu_sir, None, gpu_sir_spec(), SweepOutput::Memory, budgets).expect("the sweep starts");
    let events = drain(&mut run);
    let record = record(&events);
    assert_eq!((record.report.end, record.report.counts.ok), (SweepEnd::Complete, 2));
    let execution = &record.manifest.execution;
    assert_eq!(
        (execution.memory_budget, execution.gpu_memory_budget),
        (Some(1 << 30), Some(1 << 26)),
        "the sweep keeps both budgets it is handed"
    );
    assert!(
        host.faults.take().is_some(),
        "the host's fault stays on the host's device"
    );
    assert!(
        record.manifest.runtime.adapter.is_some(),
        "the manifest records the sweep's device"
    );
}

/// GPU SIR under an id the example set does not hold, as a model of a crate downstream registers one.
struct OutsideGpuSir;

impl GpuGridModel for OutsideGpuSir {
    const NAME: &'static str = "SIR Epidemic (GPU, outside the example set)";
    const ID: &'static str = "outside_gpu_sir";
    const DESCRIPTION: &'static str = GpuSir::DESCRIPTION;
    const PALETTE: &'static [[u8; 4]] = GpuSir::PALETTE;
    const WORKGROUP_SIZE: u32 = GpuSir::WORKGROUP_SIZE;
    const STATS: &'static [StatDescriptor] = GpuSir::STATS;
    const ACTIONS: &'static [GpuGridAction] = GpuSir::ACTIONS;
    const BUFFERS: &'static [&'static str] = GpuSir::BUFFERS;
    const STEP_BINDINGS: &'static [BindingDecl] = GpuSir::STEP_BINDINGS;
    const DISPLAY_BINDINGS: &'static [BindingDecl] = GpuSir::DISPLAY_BINDINGS;
    const REDUCE_BINDINGS: &'static [BindingDecl] = GpuSir::REDUCE_BINDINGS;
    const STEP_SHADER: &'static str = GpuSir::STEP_SHADER;
    const DISPLAY_SHADER: &'static str = GpuSir::DISPLAY_SHADER;
    const REDUCE_SHADER: &'static str = GpuSir::REDUCE_SHADER;
    const REPLAYS_EXACTLY: bool = GpuSir::REPLAYS_EXACTLY;

    fn param_descriptors() -> Vec<ParamDescriptor> {
        GpuSir::param_descriptors()
    }

    fn dims(params: &[ParamValue]) -> (u32, u32) {
        GpuSir::dims(params)
    }

    fn buffer_lens(width: u32, height: u32) -> Vec<usize> {
        GpuSir::buffer_lens(width, height)
    }

    fn step_dims(width: u32, height: u32) -> (u32, u32) {
        GpuSir::step_dims(width, height)
    }

    fn seed_buffers(width: u32, height: u32, params: &[ParamValue], seed: Option<u64>) -> Vec<Vec<u32>> {
        GpuSir::seed_buffers(width, height, params, seed)
    }

    fn step_params_bytes(width: u32, height: u32, params: &[ParamValue]) -> Vec<u8> {
        GpuSir::step_params_bytes(width, height, params)
    }

    fn action_params_bytes(action: usize, width: u32, height: u32, params: &[ParamValue], seed: u32) -> Vec<u8> {
        GpuSir::action_params_bytes(action, width, height, params, seed)
    }

    fn stats(counts: &[u32]) -> Vec<StatValue> {
        GpuSir::stats(counts)
    }
}

/// A sweep handed no device builds the entry it was handed on its own device, never one looked up by id.
#[test]
fn a_gpu_model_outside_the_example_set_sweeps_on_its_own_device() {
    if headless_device().is_none() {
        return;
    }
    let outside = register_gpu_grid_model::<OutsideGpuSir>();
    assert!(henad_models::example_models().get(outside.id()).is_none());
    let mut spec = gpu_sir_spec();
    spec.model = outside.id().to_owned();

    let mut run = SweepRun::start(outside, None, spec, SweepOutput::Memory, options()).expect("the sweep starts");
    let events = drain(&mut run);
    let record = record(&events);
    assert_eq!((record.report.end, record.report.counts.ok), (SweepEnd::Complete, 2));
    assert_eq!(record.manifest.model.id, "outside_gpu_sir");
    assert!(
        record.manifest.runtime.adapter.is_some(),
        "the manifest records the sweep's device"
    );
}
