//! Checks the sweep handle: its events and progress, pause and abort, both outputs, resume, and the pumped sweep a
//! browser runs.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use henad_compute::entry::ModelState;
use henad_compute::entry::{ModelEntry, register_gpu_grid_model};
use henad_compute::fault::Fault;
use henad_compute::gpu::GpuContext;
use henad_compute::runner::{Pace, SimLoop as _};
use henad_core::explore::factor::FactorSpec;
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::spec::SweepSpec;
use henad_core::explore::stop::StopSpec;
use henad_core::params::ParamValue;

use crate::exec::Concurrency;
use crate::handle::{SweepChannel, SweepEvent, SweepOutput, SweepPhase, SweepRun, SweepRunOptions, SweepStartError};
use crate::output::OutputError;
use crate::output::manifest::ManifestStatus;
use crate::pumped::PumpedSweep;
use crate::result_set::ResultSet;
use crate::sweep::{SpecPlan, SweepEnd, SweepRecord};
use crate::tests::broken::OutsideGpuSir;
use crate::tests::support::{
    ScratchDir, UntimedFiles, directory_files, drain, entry, factorial, fixed, headless_device, interrupted_after,
    lane_count, provenance, pump_until_idle, sir_spec, sweep, ticks_seen, untimed, values,
};

/// Maximum time a test waits for a sweep to reach a state.
const PATIENCE: Duration = Duration::from_secs(60);

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
    spec.blocks = vec![factorial(vec![FactorSpec::param("density", values(&["0", "0.3"]))])];
    spec
}

fn options() -> SweepRunOptions {
    SweepRunOptions::new(provenance())
}

/// Waits until `holds` holds for the progress of `run`.
///
/// # Panics
///
/// Panics when `holds` does not hold within [`PATIENCE`].
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

/// Runs `spec` over `entry` through a handle into `output`, and returns its events.
fn run_handle(entry: ModelEntry, spec: SweepSpec, output: SweepOutput, options: SweepRunOptions) -> Vec<SweepEvent> {
    let mut run = SweepRun::start(entry, None, spec, output, options).expect("the sweep starts");
    drain(&mut run, PATIENCE)
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
    let events = drain(&mut run, PATIENCE);

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
    assert_eq!(memory, UntimedFiles::new(&directory_files(&handle_dir, &[])));
    assert_eq!(memory, UntimedFiles::new(&directory_files(&cli_dir, &[])));
}

#[test]
fn the_pumped_sweep_writes_what_a_directory_sweep_writes() {
    let spec = sir_spec(2);
    let sir = entry("sir", None);
    let plan = Arc::new(spec.plan(&sir.schema()).expect("a valid spec"));
    let (channel, events, _) = SweepChannel::open(&options());
    let mut pumped =
        PumpedSweep::new(sir, spec.clone(), SpecPlan::Sweep(plan), channel, options()).expect("a CPU model pumps");
    let pumped_events = pump_until_idle(&mut pumped, &events, PATIENCE);
    let ids: Vec<u64> = outcomes(&pumped_events)
        .iter()
        .map(|outcome| outcome.run.run_id)
        .collect();
    assert_eq!(ids, [0, 1, 2, 3, 4, 5]);
    let pumped_record = record(&pumped_events);
    assert_eq!(pumped_record.report.outline.layout.cpu_lanes, 1);

    // The pumped sweep runs one run at a time, recorded as one lane.
    let scratch = ScratchDir::new("handle-pumped");
    let one_lane = lane_count(1);
    sweep(&entry("sir", None), None, &spec, scratch.path(), one_lane);
    assert_eq!(
        UntimedFiles::new(pumped_record.files.as_ref().expect("files in memory")),
        UntimedFiles::new(&directory_files(scratch.path(), &[]))
    );
    assert!(matches!(pumped.pump(), Pace::Idle), "an ended sweep has nothing due");
}

#[test]
fn a_paused_pumped_sweep_does_no_work_until_resumed() {
    let spec = endless_spec();
    let life = entry("game_of_life", None);
    let plan = Arc::new(spec.plan(&life.schema()).expect("a valid spec"));
    let (channel, events, _) = SweepChannel::open(&options());
    let mut pumped =
        PumpedSweep::new(life, spec, SpecPlan::Sweep(plan), channel, options()).expect("a CPU model pumps");
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
    let received = pump_until_idle(&mut pumped, &events, PATIENCE);
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
    let refused = PumpedSweep::new(gpu_sir, spec, SpecPlan::Sweep(plan), channel, options());
    assert!(matches!(refused, Err(SweepStartError::GpuNeedsNative)));
}

#[test]
fn a_paused_sweep_holds_every_run_until_resumed() {
    // Two lanes step both runs at once, on a machine of one worker too.
    let options = SweepRunOptions {
        concurrency: lane_count(2),
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
    let events = drain(&mut run, PATIENCE);
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
    events.extend(drain(&mut run, PATIENCE));

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
    let report = interrupted_after(&entry("sir", None), &spec, &resumed_dir, lane_count(1), 2);
    assert_eq!(report.end, SweepEnd::Aborted);
    let kept = report.counts.rows;
    assert_eq!(kept, 2, "the directory lacks runs for the resume to run");

    let mut run =
        SweepRun::resume_directory(entry("sir", None), None, &resumed_dir, options()).expect("the resume starts");
    let events = drain(&mut run, PATIENCE);
    let SweepEvent::Planned(outline) = &events[0] else {
        panic!("the outline comes first");
    };
    assert_eq!((outline.skipped, outline.pending), (kept, 6 - kept));
    let ids: Vec<u64> = outcomes(&events).iter().map(|outcome| outcome.run.run_id).collect();
    assert_eq!(ids, (kept..6).collect::<Vec<_>>());
    assert_eq!(record(&events).manifest.status, ManifestStatus::Complete);
    assert_eq!(
        UntimedFiles::new(&directory_files(&resumed_dir, &[])).runs,
        UntimedFiles::new(&directory_files(&fresh_dir, &[])).runs
    );
    let resumed = UntimedFiles::new(&directory_files(&resumed_dir, &[]));
    let fresh = UntimedFiles::new(&directory_files(&fresh_dir, &[]));
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
    let events = drain(&mut run, PATIENCE);
    let record = record(&events);
    assert_eq!((record.report.end, record.report.counts.ok), (SweepEnd::Complete, 2));
    let memory = UntimedFiles::new(record.files.as_ref().expect("files in memory"));
    let directory = UntimedFiles::new(&directory_files(scratch.path(), &[]));
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
    let events = drain(&mut run, PATIENCE);
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

/// A sweep passed no device builds the entry that it was passed on its own device, never an entry looked up by id.
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
    let events = drain(&mut run, PATIENCE);
    let record = record(&events);
    assert_eq!((record.report.end, record.report.counts.ok), (SweepEnd::Complete, 2));
    assert_eq!(record.manifest.model.id, "outside_gpu_sir");
    assert!(
        record.manifest.runtime.adapter.is_some(),
        "the manifest records the sweep's device"
    );
}

/// Name of the thread each build ran on, in the order the builds started.
type BuildThreads = Arc<Mutex<Vec<Option<String>>>>;

/// Returns `entry` with the name of the thread each build runs on recorded in `threads`.
fn recording_threads(entry: ModelEntry, threads: &BuildThreads) -> ModelEntry {
    let threads = Arc::clone(threads);
    entry.wrap_factory(move |create| {
        Arc::new(
            move |params: &[ParamValue], seed: Option<u64>, gpu: Option<&GpuContext>| -> Result<ModelState, Fault> {
                threads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(std::thread::current().name().map(str::to_owned));
                create(params, seed, gpu)
            },
        )
    })
}

/// Returns a pool of two workers, each named `henad-test-build-` and its index, and a test of whether a thread name
/// names one of them.
fn build_pool() -> (Arc<rayon::ThreadPool>, impl Fn(&Option<String>) -> bool) {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .thread_name(|worker| format!("henad-test-build-{worker}"))
        .build()
        .expect("the pool builds");
    let in_pool = |name: &Option<String>| {
        name.as_deref()
            .is_some_and(|name| name.starts_with("henad-test-build-"))
    };
    (Arc::new(pool), in_pool)
}

/// The probes of a sweep build in the pool its options name, and its CPU runs in the pools of their lanes.
#[test]
fn a_sweep_builds_its_probes_in_its_build_pool() {
    let threads = BuildThreads::default();
    let mut spec = SweepSpec::new("game_of_life");
    spec.fixed = fixed(&[("grid_width", "16"), ("grid_height", "16")]);
    spec.run.steps = 4;
    spec.blocks = vec![factorial(vec![FactorSpec::param("density", values(&["0.2", "0.4"]))])];
    let (pool, in_pool) = build_pool();
    let mut pooled = options();
    pooled.build_pool = Some(pool);
    let events = run_handle(
        recording_threads(entry("game_of_life", None), &threads),
        spec,
        SweepOutput::Memory,
        pooled,
    );
    assert_eq!(record(&events).report.counts.ok, 2);

    let threads = threads.lock().unwrap_or_else(PoisonError::into_inner).clone();
    let built_in_pool: Vec<bool> = threads.iter().map(&in_pool).collect();
    assert!(built_in_pool.len() >= 4, "two probes and two runs: {threads:?}");
    assert_eq!(
        built_in_pool[..2],
        [true, true],
        "the first and the last config probe in the pool: {threads:?}"
    );
    let runs = &threads[threads.len() - 2..];
    assert!(
        runs.iter()
            .all(|name| name.as_deref().is_some_and(|name| name.starts_with("henad-lane-"))),
        "the runs build in their lanes: {threads:?}"
    );
}

/// The probe and each run of a GPU model build in the pool a sweep's options name.
#[test]
fn a_gpu_sweep_builds_its_runs_in_its_build_pool() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let threads = BuildThreads::default();
    let (pool, in_pool) = build_pool();
    let mut pooled = options();
    pooled.build_pool = Some(pool);
    let gpu_sir = recording_threads(entry("gpu_sir", Some(&ctx)), &threads);
    let mut run =
        SweepRun::start(gpu_sir, Some(ctx), gpu_sir_spec(), SweepOutput::Memory, pooled).expect("the sweep starts");
    let events = drain(&mut run, PATIENCE);
    assert_eq!(record(&events).report.counts.ok, 2);

    let threads = threads.lock().unwrap_or_else(PoisonError::into_inner).clone();
    assert_eq!(threads.len(), 3, "a probe and two runs: {threads:?}");
    assert!(threads.iter().all(in_pool), "{threads:?}");
}

/// A stop returns once the timeout passes, and leaves a sweep held in its model's build running. A sweep that reads
/// the abort stops in time.
#[test]
fn a_stop_waits_for_a_held_sweep_no_longer_than_its_timeout() {
    /// Time the stop may take past its timeout, for the scheduler.
    const SLACK: Duration = Duration::from_secs(1);

    let (release, hold) = std::sync::mpsc::channel::<()>();
    let hold = Arc::new(Mutex::new(hold));
    let builds = Arc::new(AtomicUsize::new(0));
    let held = {
        let builds = Arc::clone(&builds);
        entry("game_of_life", None).wrap_factory(move |create| {
            Arc::new(
                move |params: &[ParamValue],
                      seed: Option<u64>,
                      gpu: Option<&GpuContext>|
                      -> Result<ModelState, Fault> {
                    builds.fetch_add(1, Ordering::SeqCst);
                    // Waits until the test drops `release`.
                    hold.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .recv()
                        .unwrap_or_default();
                    create(params, seed, gpu)
                },
            )
        })
    };
    let run = SweepRun::start(held, None, endless_spec(), SweepOutput::Memory, options()).expect("the sweep starts");
    let deadline = Instant::now() + PATIENCE;
    while builds.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "the probe never started");
        std::thread::sleep(Duration::from_millis(2));
    }
    let timeout = Duration::from_millis(100);
    let started = Instant::now();
    assert!(!run.stop(timeout), "a sweep held in its build stopped");
    let waited = started.elapsed();
    assert!(
        waited >= timeout && waited < timeout + SLACK,
        "the stop waited {waited:?}"
    );
    drop(release);

    let run = SweepRun::start(
        entry("game_of_life", None),
        None,
        endless_spec(),
        SweepOutput::Memory,
        options(),
    )
    .expect("the sweep starts");
    wait_for(&run, "a run in progress", |progress| !progress.active_runs.is_empty());
    assert!(run.stop(PATIENCE), "the sweep did not stop");
}
