//! Checks of the GPU tracks that step several runs on one device. Each test skips on a machine without a device unless
//! `HENAD_REQUIRE_GPU` is set.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use henad_compute::fault::{BUILDING, Fault, install_panic_hook};
use henad_compute::gpu::{GpuContext, GpuSimState, StatsPoll, stepping};
use henad_compute::snapshot::GpuSnapshot;
use henad_core::action::Fire;
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::measure::MeasurePlan;
use henad_core::explore::outcome::{PlannedRun, RunOutcome, RunStatus, StopReason};
use henad_core::explore::plan::Plan;
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
use henad_core::explore::stop::StopSpec;
use henad_core::metadata::Backend;
use henad_core::model::SimState;
use henad_core::params::ParamValue;
use henad_core::view::StatEntry;
use henad_models::registry::{ModelEntry, ModelState, model_registry};

use crate::exec::{ActiveRuns, BatchEnd, Concurrency, ExecutionLayout, Executor, RunRequest, SweepControl};
use crate::output::manifest::ManifestStatus;
use crate::progress::NoProgress;
use crate::sweep::{SweepEnd, SweepOptions};
use crate::tests::support::{
    Collected, ONE_TRACK, ScratchDir, Tables, entry, headless_device, manifest, planned, sweep, sweep_options,
    sweep_with, tracks,
};

/// Fault a [`HarnessState`] injects at a tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultInjection {
    /// A panic in the model's code, while recording the steps that reach the tick.
    Panic,
    /// A validation error from the device, for a buffer larger than it allows, while recording the steps that reach
    /// the tick.
    Validation,
    /// The same validation error outside every error scope, while polling the readback of the first sample from the
    /// tick on.
    UntracedValidation,
    /// A lost device. The readback of the first sample from the tick on fails.
    DeviceLoss,
}

/// Misbehaviour of a [`HarnessState`].
#[derive(Debug, Clone, Copy, Default)]
struct Misbehavior {
    /// Fault injected at the tick.
    fault: Option<(FaultInjection, u64)>,
    /// Polls each readback reports as pending after it landed.
    readback_delay: u32,
    /// Whether every action is refused.
    refuses_actions: bool,
}

/// Counts over every state an entry from [`instrument`] built.
#[derive(Debug, Default)]
struct StateCounts {
    /// States alive now.
    live: AtomicUsize,
    /// Most states alive at once.
    most_live: AtomicUsize,
    /// Highest tick any state recorded steps to.
    highest_tick: AtomicU64,
    /// Actions the states refused.
    refused_actions: AtomicUsize,
    /// Tick each state had recorded steps to when it was dropped.
    final_ticks: Mutex<Vec<u64>>,
}

/// GPU state that forwards to the state of a real model, and misbehaves as its [`Misbehavior`] asks.
struct HarnessState {
    state: Box<dyn GpuSimState>,
    misbehavior: Misbehavior,
    device: wgpu::Device,
    /// Polls the readback in flight still reports as pending.
    polls_left: u32,
    /// Tick of the sample whose readback is in flight.
    readback_tick: u64,
    counts: Arc<StateCounts>,
}

impl Drop for HarnessState {
    fn drop(&mut self) {
        self.counts.live.fetch_sub(1, Ordering::Relaxed);
        if let Ok(mut final_ticks) = self.counts.final_ticks.lock() {
            final_ticks.push(self.state.tick());
        }
    }
}

impl SimState for HarnessState {
    fn step(&mut self) {
        self.state.step();
    }

    fn tick(&self) -> u64 {
        self.state.tick()
    }

    fn stats(&self) -> Vec<StatEntry> {
        self.state.stats()
    }

    fn set_param(&mut self, index: usize, value: &ParamValue) -> bool {
        self.state.set_param(index, value)
    }

    fn population(&self) -> u64 {
        self.state.population()
    }

    fn heap_bytes(&self) -> usize {
        self.state.heap_bytes()
    }
}

impl GpuSimState for HarnessState {
    fn encode_steps(&mut self, encoder: &mut wgpu::CommandEncoder, count: u32, timestamps: Option<&wgpu::QuerySet>) {
        let start = self.state.tick();
        let end = start + u64::from(count);
        match self.misbehavior.fault {
            Some((FaultInjection::Panic, tick)) if start < tick && tick <= end => {
                panic!("the step pass failed at {tick}")
            }
            Some((FaultInjection::Validation, tick)) if start < tick && tick <= end => {
                raise_validation_error(&self.device);
            }
            _ => {}
        }
        self.state.encode_steps(encoder, count, timestamps);
        self.counts.highest_tick.fetch_max(end, Ordering::Relaxed);
    }

    fn encode_action(&mut self, encoder: &mut wgpu::CommandEncoder, index: usize) -> bool {
        if self.misbehavior.refuses_actions {
            self.counts.refused_actions.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        self.state.encode_action(encoder, index)
    }

    fn encode_snapshot_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.state.encode_snapshot_passes(encoder);
    }

    fn encode_stats_passes(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.state.encode_stats_passes(encoder);
    }

    fn begin_stats_readback(&mut self) {
        self.state.begin_stats_readback();
        self.polls_left = self.misbehavior.readback_delay;
        self.readback_tick = self.state.tick();
    }

    fn poll_stats_readback(&mut self, device: &wgpu::Device, block: bool) -> StatsPoll {
        match self.misbehavior.fault {
            Some((FaultInjection::DeviceLoss, tick)) if self.readback_tick >= tick => {
                self.device.destroy();
                return StatsPoll::Failed;
            }
            Some((FaultInjection::UntracedValidation, tick)) if self.readback_tick >= tick => {
                self.misbehavior.fault = None;
                raise_validation_error(&self.device);
            }
            _ => {}
        }
        let poll = self.state.poll_stats_readback(device, block);
        if poll != StatsPoll::Pending && self.polls_left > 0 && !block {
            self.polls_left -= 1;
            return StatsPoll::Pending;
        }
        poll
    }

    fn stats_readback_pending(&self) -> bool {
        self.state.stats_readback_pending() || self.polls_left > 0
    }

    fn view(&self) -> GpuSnapshot {
        self.state.view()
    }
}

/// Raises a validation error on `device` by asking it for a buffer larger than it allows.
fn raise_validation_error(device: &wgpu::Device) {
    drop(device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("henad_test_oversized"),
        size: device.limits().max_buffer_size + 4096,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    }));
}

/// Returns `model` with every state it builds held in a [`HarnessState`], set up as `misbehavior` asks for the
/// state's params and counted in `counts`.
fn instrument(
    model: ModelEntry,
    ctx: &GpuContext,
    counts: &Arc<StateCounts>,
    misbehavior: impl Fn(&[ParamValue]) -> Misbehavior + Send + Sync + 'static,
) -> ModelEntry {
    let create = model.create;
    let device = ctx.device.clone();
    let counts = Arc::clone(counts);
    ModelEntry {
        create: Box::new(move |params: &[ParamValue], seed| match create(params, seed)? {
            ModelState::Gpu(state) => {
                let live = counts.live.fetch_add(1, Ordering::Relaxed) + 1;
                counts.most_live.fetch_max(live, Ordering::Relaxed);
                Ok(ModelState::Gpu(Box::new(HarnessState {
                    state,
                    misbehavior: misbehavior(params),
                    device: device.clone(),
                    polls_left: 0,
                    readback_tick: 0,
                    counts: Arc::clone(&counts),
                })))
            }
            ModelState::Cpu(state) => Ok(ModelState::Cpu(state)),
        }),
        ..model
    }
}

/// Runs every run of `plan` on `executor`, and returns the outcomes with their timing zeroed.
fn outcomes(executor: &Executor<'_>, plan: &Plan) -> Vec<RunOutcome> {
    let requests: Vec<RunRequest<'_>> = plan.runs().map(|run| RunRequest::planned(plan, run)).collect();
    let mut collected = Collected::default();
    let end = executor.run_batch(&requests, &mut collected).expect("the batch runs");
    assert_eq!(end, BatchEnd::Complete);
    collected
        .0
        .into_iter()
        .map(|outcome| RunOutcome {
            build_ms: 0.0,
            wall_ms: 0.0,
            ..outcome
        })
        .collect()
}

/// Returns the executor of `model` over `measure` on `layout`.
fn executor<'e>(
    model: &'e ModelEntry,
    ctx: &'e GpuContext,
    measure: &Arc<MeasurePlan>,
    layout: ExecutionLayout,
) -> Executor<'e> {
    Executor::new(model, Some(ctx), Arc::clone(measure), layout, SweepControl::new()).expect("a device")
}

fn fixed_values(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(id, value)| (id.to_owned(), value.to_owned()))
        .collect()
}

/// Returns a block of every combination of `factors`.
fn factorial(factors: Vec<FactorSpec>) -> BlockSpec {
    BlockSpec {
        design: DesignKind::Factorial,
        factors,
        design_seed: None,
    }
}

fn values(levels: &[&str]) -> LevelSpec {
    LevelSpec::Values(levels.iter().map(|&level| level.to_owned()).collect())
}

/// Returns a spec of 4 configs of 2 replicates over `model`, varying `param` and the tick of `action`, stopped by
/// `stop` and sampled every 3 ticks.
fn interleaved_spec(
    model: &str,
    fixed: &[(&str, &str)],
    param: (&str, &[&str]),
    action: (&str, &[&str]),
    stop: &str,
) -> SweepSpec {
    let mut spec = SweepSpec::new(model);
    spec.fixed = fixed_values(fixed);
    spec.run.steps = 30;
    spec.run.replicates = 2;
    spec.run.stop = Some(StopSpec::parse(stop, 0).expect("a valid condition"));
    spec.measure.stats_every = 3;
    spec.measure.series_every = 3;
    spec.seeds.root = 9;
    spec.actions = vec![ActionSpec {
        name: "wave".to_owned(),
        ..ActionSpec::new(action.0, 0)
    }];
    spec.blocks = vec![factorial(vec![
        FactorSpec::param(param.0, values(param.1)),
        FactorSpec::action("wave", values(action.1)),
    ])];
    spec
}

#[test]
fn interleaved_gpu_runs_match_sequential_ones() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let cases = [
        interleaved_spec(
            "gpu_sir",
            &[
                ("grid_width", "32"),
                ("grid_height", "32"),
                ("recovery_rate", "0.1"),
                ("initial_infected_pct", "0.02"),
            ],
            ("infection_rate", &["0.1", "0.6"]),
            ("seed_outbreak", &["4", "12"]),
            "Infected >= 600",
        ),
        interleaved_spec(
            "gpu_game_of_life",
            &[("grid_width", "32"), ("grid_height", "32")],
            ("density", &["0.2", "0.4"]),
            ("clear", &["9", "20"]),
            "Alive <= 0",
        ),
    ];
    let scratch = ScratchDir::new("gpu-interleaved");
    for spec in cases {
        let model = entry(&spec.model, Some(&ctx));
        let [one, four] = [1, 4].map(|count| {
            let output_dir = scratch.path().join(format!("{}-{count}", spec.model));
            let concurrency = Concurrency::Fixed(count.try_into().expect("a track count above 0"));
            let report = sweep(&model, Some(&ctx), &spec, &output_dir, concurrency);
            assert_eq!(report.outline.layout.gpu_tracks, count);
            assert_eq!((report.counts.rows, report.counts.ok), (8, 8), "{}", spec.model);
            Tables::read(&output_dir)
        });
        assert_eq!(one, four, "{}", spec.model);
        let stop_reasons = one.run_column("stop_reason");
        assert!(stop_reasons.contains(&"condition"), "{}: {stop_reasons:?}", spec.model);
    }
}

/// Returns the rows a blocking run of `run` samples, stepping to each sampled tick and waiting on its stats.
fn blocking_rows(
    model: &ModelEntry,
    ctx: &GpuContext,
    plan: &Plan,
    measure: &MeasurePlan,
    run: PlannedRun,
) -> Vec<(u64, Vec<f64>)> {
    let config = plan.config(run.config_id).expect("the run's config");
    let schedule = plan.schedule(config);
    let Ok(ModelState::Gpu(mut state)) = (model.create)(&config.params, Some(run.seed)) else {
        panic!("{} builds on the GPU", model.id);
    };
    let mut refused = stepping::run_due(&mut *state, ctx, &schedule).len();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    for tick in measure.sample_ticks() {
        let steps = tick - state.tick();
        let acted = stepping::run_steps_acting(&mut *state, ctx, steps, &schedule, Fire::AfterStep);
        refused += acted.expect("the steps run").len();
        let stats = stepping::sample_stats(&mut *state, ctx);
        measure
            .columns()
            .extract(tick, &stats, &mut row)
            .expect("the layout holds");
        rows.push((tick, row.clone()));
    }
    assert_eq!(refused, 0, "{} takes its action", model.id);
    rows
}

#[test]
fn a_pipelined_gpu_sample_matches_a_blocking_one() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let cases = [
        (
            "gpu_sir",
            &[("grid_width", "32"), ("grid_height", "32")][..],
            "seed_outbreak",
        ),
        (
            "gpu_game_of_life",
            &[("grid_width", "32"), ("grid_height", "32")][..],
            "randomise",
        ),
        (
            "gpu_ants",
            &[("num_agents", "1000"), ("world_width", "64"), ("world_height", "64")][..],
            "reset_colony",
        ),
    ];
    for (id, fixed, action) in cases {
        let model = entry(id, Some(&ctx));
        let mut spec = SweepSpec::new(id);
        spec.fixed = fixed_values(fixed);
        spec.run.steps = 20;
        spec.run.replicates = 2;
        spec.measure.stats_every = 1;
        spec.measure.series_every = 1;
        spec.actions = vec![ActionSpec::new(action, 7)];
        let (plan, measure) = planned(&model, Some(&ctx), &spec);
        for outcome in outcomes(&executor(&model, &ctx, &measure, ONE_TRACK), &plan) {
            assert_eq!(outcome.status, RunStatus::Ok, "{id}: {:?}", outcome.note);
            let pipelined: Vec<(u64, Vec<f64>)> =
                outcome.series.rows().map(|(tick, row)| (tick, row.to_vec())).collect();
            assert_eq!(pipelined.len(), 21, "{id}: ticks 0 to 20");
            assert_eq!(
                pipelined,
                blocking_rows(&model, &ctx, &plan, &measure, outcome.run),
                "{id}, run {}",
                outcome.run.run_id
            );
        }
    }
}

#[test]
fn an_interleaved_queue_executes_every_step() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let fixed_by_model = |id: &str| match id {
        "gpu_sir" | "gpu_game_of_life" => fixed_values(&[("grid_width", "64"), ("grid_height", "64")]),
        "gpu_ants" => fixed_values(&[("num_agents", "1000"), ("world_width", "64"), ("world_height", "64")]),
        "gpu_boids" => fixed_values(&[("num_agents", "2000"), ("world_width", "400"), ("world_height", "400")]),
        _ => panic!("{id} has no case"),
    };
    let models: Vec<ModelEntry> = model_registry(Some(ctx.clone()))
        .into_iter()
        .filter(|model| model.metadata.backend == Backend::Gpu)
        .collect();
    assert_eq!(models.len(), 4, "every GPU model has a case");
    for model in models {
        let counts = Arc::new(StateCounts::default());
        let model = instrument(model, &ctx, &counts, |_| Misbehavior::default());
        let mut spec = SweepSpec::new(&model.id);
        spec.fixed = fixed_by_model(&model.id);
        spec.run.steps = 150;
        spec.run.replicates = 8;
        spec.measure.stats_every = 50;
        spec.measure.series_every = 50;
        let (plan, measure) = planned(&model, Some(&ctx), &spec);
        let finished = outcomes(&executor(&model, &ctx, &measure, tracks(8)), &plan);
        assert_eq!(
            counts.most_live.load(Ordering::Relaxed),
            8,
            "{}: every run was live at once",
            model.id
        );
        for outcome in &finished {
            let id = &model.id;
            assert_eq!(
                (outcome.status, outcome.ticks),
                (RunStatus::Ok, 150),
                "{id}: {:?}",
                outcome.note
            );
            assert_eq!(outcome.series.ticks(), [0, 50, 100, 150], "{id}");
            let last = outcome.series.row(3);
            assert!(last.iter().any(|&value| value != 0.0), "{id}: {last:?}");
        }
        let final_ticks = counts.final_ticks.lock().expect("the ticks are recorded").clone();
        assert_eq!(final_ticks.len(), 9, "{}: the probe and 8 runs", model.id);
        assert!(
            final_ticks[1..].iter().all(|&tick| tick == 150),
            "{}: {final_ticks:?}",
            model.id
        );
    }
}

#[test]
fn gpu_admission_respects_the_memory_budget() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let counts = Arc::new(StateCounts::default());
    let model = instrument(entry("gpu_sir", Some(&ctx)), &ctx, &counts, |_| Misbehavior::default());
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed_values(&[("grid_width", "32"), ("grid_height", "32")]);
    spec.run.steps = 100;
    spec.run.replicates = 6;
    spec.measure.stats_every = 10;
    spec.measure.series_every = 10;
    let (plan, measure) = planned(&model, Some(&ctx), &spec);
    let params = &plan.config(0).expect("the plan has a config").params;
    let demand = model.demand(params).expect("a GPU model has a demand").bytes();

    let sequential = outcomes(&executor(&model, &ctx, &measure, ONE_TRACK), &plan);
    counts.most_live.store(0, Ordering::Relaxed);
    let budgeted = executor(&model, &ctx, &measure, tracks(4)).with_gpu_memory(Some(demand * 5 / 2));
    assert_eq!(outcomes(&budgeted, &plan), sequential);
    assert_eq!(counts.most_live.load(Ordering::Relaxed), 2, "two runs fit the budget");
}

#[test]
fn an_out_of_memory_build_waits_for_a_track_to_finish() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let counts = Arc::new(StateCounts::default());
    let model = instrument(entry("gpu_sir", Some(&ctx)), &ctx, &counts, |_| Misbehavior::default());
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed_values(&[("grid_width", "32"), ("grid_height", "32")]);
    spec.run.steps = 100;
    spec.run.replicates = 6;
    spec.measure.stats_every = 10;
    spec.measure.series_every = 10;
    let (plan, measure) = planned(&model, Some(&ctx), &spec);
    let sequential = outcomes(&executor(&model, &ctx, &measure, ONE_TRACK), &plan);

    // Each build attempt records whether it was refused and how many states had been dropped before it.
    let attempts = Arc::new(Mutex::new(Vec::<(bool, usize)>::new()));
    let create = model.create;
    let (build_counts, build_attempts) = (Arc::clone(&counts), Arc::clone(&attempts));
    let refusing_entry = ModelEntry {
        create: Box::new(move |params: &[ParamValue], seed| {
            let refused = build_counts.live.load(Ordering::Relaxed) >= 2;
            let dropped = build_counts.final_ticks.lock().expect("the ticks are recorded").len();
            build_attempts
                .lock()
                .expect("the attempts are recorded")
                .push((refused, dropped));
            if refused {
                let error = wgpu::Error::OutOfMemory {
                    source: "the device is out of memory".into(),
                };
                return Err(Fault::device(BUILDING, error));
            }
            create(params, seed)
        }),
        ..model
    };
    counts.most_live.store(0, Ordering::Relaxed);
    let finished = outcomes(&executor(&refusing_entry, &ctx, &measure, tracks(4)), &plan);
    assert_eq!(finished, sequential, "the refused builds ran once a track was free");
    let attempts = attempts.lock().expect("the attempts are recorded").clone();
    assert_eq!(
        attempts.len(),
        8,
        "six builds and one refusal at each of 4 and 3 tracks: {attempts:?}"
    );
    for pair in attempts.windows(2) {
        let [(refused, dropped), (_, next_dropped)] = [pair[0], pair[1]];
        assert!(
            !refused || next_dropped > dropped,
            "a refused build waits for a live run to end: {attempts:?}"
        );
    }
    assert_eq!(counts.most_live.load(Ordering::Relaxed), 2);
}

#[test]
fn a_gpu_fault_in_one_track_leaves_the_others_running() {
    let Some(ctx) = headless_device() else {
        return;
    };
    install_panic_hook();
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed_values(&[("grid_width", "32"), ("grid_height", "32")]);
    spec.run.steps = 30;
    spec.run.replicates = 2;
    spec.measure.stats_every = 3;
    spec.measure.series_every = 3;
    spec.blocks = vec![factorial(vec![FactorSpec::param(
        "infection_rate",
        values(&["0.2", "0.3", "0.4", "0.5"]),
    )])];
    let plain = entry("gpu_sir", Some(&ctx));
    let (plan, measure) = planned(&plain, Some(&ctx), &spec);
    let sequential = outcomes(&executor(&plain, &ctx, &measure, ONE_TRACK), &plan);

    let counts = Arc::new(StateCounts::default());
    let faulting = instrument(plain, &ctx, &counts, |params| {
        let fault = match params[2] {
            ParamValue::F32(0.3) => Some((FaultInjection::Panic, 5)),
            ParamValue::F32(0.4) => Some((FaultInjection::Validation, 5)),
            _ => None,
        };
        Misbehavior {
            fault,
            ..Misbehavior::default()
        }
    });
    let finished = outcomes(&executor(&faulting, &ctx, &measure, tracks(4)), &plan);
    assert_eq!(counts.most_live.load(Ordering::Relaxed), 4);
    for (outcome, clean) in finished.iter().zip(&sequential) {
        let note = outcome.note.as_deref().unwrap_or_default();
        match outcome.run.config_id {
            1 | 2 => {
                let status = if outcome.run.config_id == 1 {
                    RunStatus::Panicked
                } else {
                    RunStatus::GpuError
                };
                assert_eq!(
                    (outcome.status, outcome.stop_reason),
                    (status, StopReason::Fault),
                    "{note}"
                );
                assert_eq!(outcome.ticks, 3, "the last sample before the fault at tick 5");
                assert_eq!(outcome.series.ticks(), [0, 3]);
                for row in 0..2 {
                    assert_eq!(outcome.series.row(row), clean.series.row(row));
                }
                assert!(note.starts_with("while stepping the simulation"), "{note}");
            }
            _ => assert_eq!(outcome, clean, "a run on another track is untouched"),
        }
    }
}

#[test]
fn an_untraced_gpu_fault_fails_every_live_run() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed_values(&[("grid_width", "32"), ("grid_height", "32")]);
    spec.run.steps = 30;
    spec.run.stop = Some(StopSpec::parse("Susceptible >= 0", 0).expect("a valid condition"));
    spec.measure.stats_every = 3;
    spec.measure.series_every = 3;
    spec.blocks = vec![factorial(vec![FactorSpec::param(
        "infection_rate",
        values(&["0.2", "0.3", "0.4", "0.5"]),
    )])];
    let plain = entry("gpu_sir", Some(&ctx));
    let (plan, measure) = planned(&plain, Some(&ctx), &spec);

    // Every run stops at its first sample, which reads back slowly enough for the fault to arrive first.
    let counts = Arc::new(StateCounts::default());
    let faulting = instrument(plain, &ctx, &counts, |params| Misbehavior {
        fault: matches!(params[2], ParamValue::F32(0.3)).then_some((FaultInjection::UntracedValidation, 0)),
        readback_delay: 50,
        ..Misbehavior::default()
    });
    let finished = outcomes(&executor(&faulting, &ctx, &measure, tracks(4)), &plan);
    assert_eq!(counts.most_live.load(Ordering::Relaxed), 4);
    for outcome in &finished {
        assert_eq!(
            (outcome.status, outcome.stop_reason),
            (RunStatus::GpuError, StopReason::Fault),
            "run {}: {:?}",
            outcome.run.run_id,
            outcome.note
        );
        assert_eq!(outcome.ticks, 0);
        assert_eq!(
            outcome.series.ticks(),
            [0],
            "the sample reading back at the fault lands"
        );
    }
}

#[test]
fn a_stop_discards_the_speculative_steps() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed_values(&[
        ("grid_width", "32"),
        ("grid_height", "32"),
        ("recovery_rate", "0.2"),
        ("initial_infected_pct", "0.2"),
    ]);
    spec.run.steps = 60;
    spec.measure.stats_every = 5;
    spec.measure.series_every = 5;
    let plain = entry("gpu_sir", Some(&ctx));
    let (plan, measure) = planned(&plain, Some(&ctx), &spec);
    let full = outcomes(&executor(&plain, &ctx, &measure, ONE_TRACK), &plan).remove(0);
    let recovered_column = (0..measure.columns().len())
        .find(|&column| measure.columns().name(column) == "Recovered")
        .expect("gpu_sir has a Recovered column");
    let threshold = full
        .series
        .rows()
        .find(|&(tick, _)| tick == 20)
        .map(|(_, row)| row[recovered_column])
        .expect("the run samples tick 20");
    let (stop_tick, stop_position) = full
        .series
        .rows()
        .position(|(_, row)| row[recovered_column] >= threshold)
        .map(|position| (full.series.ticks()[position], position))
        .expect("Recovered reaches its value at tick 20");

    spec.run.stop = Some(StopSpec::parse(&format!("Recovered >= {threshold}"), 0).expect("a valid condition"));
    spec.actions = vec![
        ActionSpec {
            name: "early".to_owned(),
            ..ActionSpec::new("seed_outbreak", stop_tick - 2)
        },
        ActionSpec {
            name: "late".to_owned(),
            ..ActionSpec::new("seed_outbreak", stop_tick + 3)
        },
    ];
    let counts = Arc::new(StateCounts::default());
    let slow = instrument(plain, &ctx, &counts, |_| Misbehavior {
        readback_delay: 20,
        refuses_actions: true,
        ..Misbehavior::default()
    });
    let (plan, measure) = planned(&slow, Some(&ctx), &spec);
    let stopped = outcomes(&executor(&slow, &ctx, &measure, ONE_TRACK), &plan).remove(0);

    assert!(
        counts.highest_tick.load(Ordering::Relaxed) > stop_tick + 3,
        "the steps past the stop were recorded"
    );
    assert_eq!(
        counts.refused_actions.load(Ordering::Relaxed),
        2,
        "the late action was recorded ahead of the stop"
    );
    assert_eq!(
        (stopped.status, stopped.stop_reason),
        (RunStatus::Ok, StopReason::Condition)
    );
    assert_eq!(stopped.ticks, stop_tick);
    let kept: Vec<(u64, &[f64])> = full.series.rows().take(stop_position + 1).collect();
    assert_eq!(stopped.series.rows().collect::<Vec<_>>(), kept);
    assert_eq!(
        stopped.note.as_deref(),
        Some(format!("model refused action 'seed_outbreak' at tick {}", stop_tick - 2).as_str()),
        "only the action before the stop is noted"
    );
}

#[test]
fn a_lost_device_leaves_a_directory_a_resume_completes() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed_values(&[("grid_width", "32"), ("grid_height", "32")]);
    spec.run.steps = 400;
    spec.run.replicates = 4;
    spec.measure.stats_every = 10;
    spec.measure.series_every = 10;
    spec.blocks = vec![factorial(vec![FactorSpec::param(
        "infection_rate",
        values(&["0.2", "0.3"]),
    )])];
    let counts = Arc::new(StateCounts::default());
    let losing = instrument(entry("gpu_sir", Some(&ctx)), &ctx, &counts, |params| Misbehavior {
        fault: matches!(params[2], ParamValue::F32(0.3)).then_some((FaultInjection::DeviceLoss, 200)),
        ..Misbehavior::default()
    });
    let scratch = ScratchDir::new("gpu-lost");
    let lost_dir = scratch.path().join("lost");
    let active_runs = ActiveRuns::new();
    let options = SweepOptions {
        concurrency: Concurrency::Fixed(2.try_into().expect("2 is above 0")),
        active_runs: Some(active_runs.clone()),
        ..sweep_options(&lost_dir, false)
    };
    let report = sweep_with(&losing, Some(&ctx), &spec, &options, &mut NoProgress)
        .expect("a lost device ends the sweep without an error of its own");
    assert!(active_runs.list().is_empty(), "no run is left in progress");
    assert_eq!(active_runs.waiting_count(), 0, "no lost run waits to be committed");
    assert_eq!(report.end, SweepEnd::DeviceLost);
    assert_eq!(
        (report.counts.rows, report.counts.ok),
        (4, 4),
        "the runs of config 0 alone are written"
    );
    assert_eq!(manifest(&lost_dir).status, ManifestStatus::Incomplete);

    let Some(fresh_ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&fresh_ctx));
    let resumed = SweepOptions {
        concurrency: options.concurrency,
        ..sweep_options(&lost_dir, true)
    };
    let report = sweep_with(&gpu_sir, Some(&fresh_ctx), &spec, &resumed, &mut NoProgress).expect("the resume runs");
    assert_eq!((report.end, report.counts.ok), (SweepEnd::Complete, 8));
    let fresh_dir = scratch.path().join("fresh");
    sweep(&gpu_sir, Some(&fresh_ctx), &spec, &fresh_dir, options.concurrency);
    assert_eq!(Tables::read(&lost_dir), Tables::read(&fresh_dir));
}

/// Guard that aborts its control when dropped, so a failed check never leaves a batch running.
struct AbortOnDrop(SweepControl);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[test]
fn gpu_tracks_hold_on_a_pause_and_end_on_an_abort() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let model = entry("gpu_sir", Some(&ctx));
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed_values(&[("grid_width", "32"), ("grid_height", "32")]);
    spec.run.steps = 100_000_000;
    spec.run.replicates = 8;
    spec.measure.stats_every = 1000;
    spec.measure.series_every = 0;
    let (plan, measure) = planned(&model, Some(&ctx), &spec);
    let control = SweepControl::new();
    let active_runs = ActiveRuns::new();
    let ticks = || -> Vec<u64> { active_runs.list().iter().map(|run| run.tick).collect() };
    let wait_until = |condition: &dyn Fn() -> bool| {
        let started = Instant::now();
        while !condition() {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "the tracks never got there"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    };
    std::thread::scope(|scope| {
        let batch = scope.spawn(|| {
            let executor = Executor::new(&model, Some(&ctx), Arc::clone(&measure), tracks(4), control.clone())
                .expect("a device")
                .with_active_runs(Some(active_runs.clone()));
            let requests: Vec<RunRequest<'_>> = plan.runs().map(|run| RunRequest::planned(&plan, run)).collect();
            let mut collected = Collected::default();
            let end = executor.run_batch(&requests, &mut collected).expect("the batch runs");
            (end, collected.0.len())
        });
        let _abort_on_drop = AbortOnDrop(control.clone());
        wait_until(&|| ticks().len() == 4 && ticks().iter().all(|&tick| tick > 0));
        control.pause();
        std::thread::sleep(Duration::from_millis(100));
        let paused = ticks();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(ticks(), paused, "a paused batch records nothing");
        control.resume();
        wait_until(&|| ticks().iter().zip(&paused).all(|(tick, before)| tick > before));
        let aborted_at = Instant::now();
        control.abort();
        let (end, committed) = batch.join().expect("the batch thread finished");
        assert!(
            aborted_at.elapsed() < Duration::from_secs(5),
            "the tracks stopped within a slice"
        );
        assert_eq!((end, committed), (BatchEnd::Aborted, 0));
    });
    assert!(active_runs.list().is_empty(), "an aborted track leaves the table");
}
