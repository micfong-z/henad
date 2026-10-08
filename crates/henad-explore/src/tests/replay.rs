//! Checks that the live loops replay a run as a sweep and `--export-stats` step it.

use std::ops::ControlFlow;
use std::sync::Arc;

use henad_compute::cpu::sim_thread::SimThread;
use henad_compute::entry::ModelState;
use henad_compute::fault::FaultSink;
use henad_compute::gpu::sim_thread::{GpuBatchSettings, GpuSimThread};
use henad_compute::simulation::RunSetup;
use henad_compute::snapshot::Snapshot;
use henad_core::action::Schedule;
use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::measure::Sampler;
use henad_core::explore::outcome::RunStatus;
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
use henad_core::explore::value::resolve_params;
use henad_core::export::stats_csv::StatColumns;
use henad_core::view::StatEntry;

use crate::exec::Concurrency;
use crate::result_set::ResultSet;
use crate::tests::support::{Collected, ScratchDir, entry, headless_device, lanes, planned, run_plan, sweep};

fn values(raw: &[&str]) -> LevelSpec {
    LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
}

fn fixed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(id, value)| (id.to_owned(), value.to_owned()))
        .collect()
}

/// Takes snapshots from `take` until a snapshot reports `tick`, or gives up after ten seconds.
fn snapshot_at(mut take: impl FnMut() -> Option<Snapshot>, tick: u64) -> Option<Snapshot> {
    for _ in 0..1000 {
        if let Some(snap) = take()
            && snap.tick == tick
        {
            return Some(snap);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

/// Returns the values of `stats` in the columns of `columns`.
fn row(columns: &StatColumns, tick: u64, stats: &[StatEntry]) -> Vec<f64> {
    let mut values = Vec::new();
    columns
        .extract(tick, stats, &mut values)
        .expect("the stats fit the columns");
    values
}

/// Returns the values of `stats` in the columns a folder recorded, named by `names`, in the folder's order.
///
/// # Panics
///
/// Panics when `stats` has no column of one of the names.
fn recorded_row(names: &[String], tick: u64, stats: &[StatEntry]) -> Vec<f64> {
    let columns = StatColumns::plan(stats);
    let values = row(&columns, tick, stats);
    names
        .iter()
        .map(|name| {
            let column = columns
                .resolve(name)
                .unwrap_or_else(|| panic!("the replay has no column '{name}'"));
            values[column]
        })
        .collect()
}

#[test]
fn a_replay_of_a_planned_run_matches_its_sweep_row() {
    let sir = entry("sir", None);
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[
        ("grid_width", "32"),
        ("grid_height", "32"),
        ("recovery_rate", "0.05"),
        ("initial_infected_pct", "0.01"),
    ]);
    spec.run.warmup = 5;
    spec.run.steps = 25;
    spec.run.replicates = 2;
    spec.measure.stats_every = 5;
    spec.measure.series_every = 5;
    spec.seeds.root = 11;
    spec.actions = vec![ActionSpec {
        name: "wave".to_owned(),
        ..ActionSpec::new("seed_outbreak", 20)
    }];
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![
            FactorSpec::param("infection_rate", values(&["0.03", "0.06"])),
            FactorSpec::action("wave", values(&["0", "20"])),
        ],
        design_seed: None,
    }];
    let (plan, measure) = planned(&sir, None, &spec);
    let mut outcomes = Collected::default();
    run_plan(&sir, None, &plan, &measure, lanes(2, 1), &mut outcomes);

    // Run 1 fires its action before the first step, and run 7 after the step reaching tick 20.
    for run_id in [1, 7] {
        let outcome = &outcomes.0[run_id as usize];
        assert_eq!(outcome.run.run_id, run_id);
        assert_eq!(outcome.status, RunStatus::Ok, "run {run_id}");

        let replay = plan.replay(run_id).expect("the run is planned");
        assert_eq!(replay.ticks, outcome.ticks);
        let Ok(ModelState::Cpu(state)) = sir.build(&replay.params, Some(replay.seed), None) else {
            panic!("SIR builds on the CPU");
        };
        let mut thread = SimThread::new(state, 60.0, None, FaultSink::new());
        thread.set_schedule(replay.schedule.clone());
        // The live loop runs to each sampled tick in turn, and its snapshots use the sweep's own sampler.
        let mut sampler = Sampler::new(Arc::clone(&measure));
        let mut reached = None;
        for tick in measure.sample_ticks() {
            thread.run_to(tick);
            let snapshot = snapshot_at(|| thread.take_snapshot(), tick).expect("the replay reaches each sampled tick");
            let stops = sampler.push(tick, &snapshot.stats).expect("the stats fit the columns");
            assert!(!stops, "the spec has no stop condition");
            reached = Some(snapshot);
        }
        let replayed = sampler.finish();
        assert_eq!(
            replayed.series.len(),
            6,
            "run {run_id}: ticks 5 to 30, every fifth tick"
        );
        assert_eq!(
            replayed.series, outcome.series,
            "run {run_id}: the replay samples the sweep's series"
        );
        assert_eq!(
            replayed.reducers, outcome.reducers,
            "run {run_id}: the replay folds to the sweep's row"
        );
        let reached = reached.expect("the run samples at least once");
        assert_eq!(reached.population, outcome.population);
    }
}

#[test]
fn a_gpu_schedule_matches_export_stats() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let params = resolve_params(
        gpu_sir.param_descriptors(),
        &fixed(&[
            ("grid_width", "64"),
            ("grid_height", "64"),
            ("initial_infected_pct", "0.02"),
        ]),
    )
    .expect("valid values");
    let raw = [
        "seed_outbreak@0",
        "seed_outbreak@6",
        "seed_outbreak@6",
        "seed_outbreak@21",
        "seed_outbreak@40",
    ]
    .map(str::to_owned);
    let schedule = Schedule::parse(&raw, "gpu_sir", gpu_sir.action_descriptors()).expect("declared actions");
    let (seed, total) = (42, 40);
    let build = || match gpu_sir.build(&params, Some(seed), Some(&ctx)) {
        Ok(ModelState::Gpu(state)) => state,
        Ok(ModelState::Cpu(_)) | Err(_) => panic!("gpu_sir builds on the GPU"),
    };

    // The path `--export-stats --stats-every 40` takes: the build fires tick 0's actions, and the run samples ticks 0
    // and 40.
    let mut exported = RunSetup::from_parts(&gpu_sir, &params, Some(seed), schedule.clone())
        .expect("a valid setup")
        .build(Some(&ctx))
        .expect("gpu_sir builds");
    let mut samples = Vec::new();
    let end = exported
        .run_sampled::<()>(total, total, |sample| {
            samples.push((sample.tick(), sample.entries().to_vec()));
            ControlFlow::Continue(())
        })
        .expect("the steps run");
    assert!(end.is_continue());
    let ticks: Vec<u64> = samples.iter().map(|(tick, _)| *tick).collect();
    assert_eq!(ticks, [0, total]);
    let expected = samples.pop().map(|(_, entries)| entries).expect("a final sample");

    // In batches of 16, submissions end at ticks 6, 16, 21, 32 and 40, and each action goes in its own submission.
    let settings = GpuBatchSettings {
        adaptive: false,
        batch_size: 16,
        ..GpuBatchSettings::default()
    };
    let mut thread = GpuSimThread::new(ctx.clone(), build(), settings, None);
    thread.set_schedule(schedule);
    thread.run_to(total);
    let reached = snapshot_at(|| thread.take_snapshot(), total).expect("the loop reaches its target");
    let columns = StatColumns::plan(&expected);
    assert_eq!(
        row(&columns, total, &reached.stats),
        row(&columns, total, &expected),
        "the live loop fires and samples as --export-stats does"
    );
}

#[test]
fn a_replayed_run_from_a_result_set_matches_its_row() {
    let sir = entry("sir", None);
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "32"), ("grid_height", "32"), ("recovery_rate", "0.05")]);
    spec.run.warmup = 4;
    spec.run.steps = 26;
    spec.run.replicates = 2;
    spec.measure.stats_every = 5;
    spec.measure.series_every = 5;
    spec.seeds.root = 17;
    spec.actions = vec![ActionSpec::new("seed_outbreak", 9)];
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![
            FactorSpec::param("infection_rate", values(&["0.04", "0.08"])),
            FactorSpec::action("seed_outbreak", values(&["0", "9"])),
        ],
        design_seed: None,
    }];
    let scratch = ScratchDir::new("replay-result-set");
    sweep(&sir, None, &spec, scratch.path(), Concurrency::Auto);
    let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");
    let infected = set
        .reducer_columns()
        .iter()
        .position(|name| name == "Infected:final")
        .expect("the default reducers include the final value");
    let infected_column = set
        .stat_columns()
        .iter()
        .position(|name| name == "Infected")
        .expect("SIR counts its infected");

    assert_eq!(set.runs().len(), 8, "2 rates by 2 action ticks by 2 replicates");
    for recorded in set.runs() {
        let outcome = &recorded.outcome;
        assert_eq!(outcome.status, RunStatus::Ok);
        let replay = set.replay(sir.schema(), outcome.run.run_id).expect("the run replays");
        assert_eq!(replay.ticks, outcome.ticks);
        let Ok(ModelState::Cpu(state)) = sir.build(&replay.params, Some(replay.seed), None) else {
            panic!("SIR builds on the CPU");
        };
        let mut thread = SimThread::new(state, 60.0, None, FaultSink::new());
        thread.set_schedule(replay.schedule.clone());
        thread.run_to(replay.ticks);
        let reached = snapshot_at(|| thread.take_snapshot(), replay.ticks).expect("the replay reaches its last tick");
        let (last_tick, last_row) = outcome.series.rows().last().expect("a run keeps its final sample");
        assert_eq!(last_tick, replay.ticks);
        let replayed = recorded_row(set.stat_columns(), reached.tick, &reached.stats);
        assert_eq!(
            replayed, last_row,
            "run {}: the replay ends on its row's series",
            outcome.run.run_id
        );
        assert_eq!(
            Some(replayed[infected_column]),
            outcome.reducers[infected],
            "run {}: Infected at the end",
            outcome.run.run_id
        );
        assert_eq!(reached.population, outcome.population);
    }
}

#[test]
fn a_replayed_gpu_run_from_a_result_set_matches_its_row() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let gpu_sir = entry("gpu_sir", Some(&ctx));
    let mut spec = SweepSpec::new("gpu_sir");
    spec.fixed = fixed(&[
        ("grid_width", "64"),
        ("grid_height", "64"),
        ("initial_infected_pct", "0.02"),
    ]);
    spec.run.steps = 40;
    spec.run.replicates = 2;
    spec.measure.stats_every = 8;
    spec.measure.series_every = 8;
    spec.seeds.root = 5;
    spec.actions = vec![ActionSpec::new("seed_outbreak", 21)];
    let scratch = ScratchDir::new("replay-result-set-gpu");
    sweep(&gpu_sir, Some(&ctx), &spec, scratch.path(), Concurrency::Auto);
    let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");

    assert_eq!(set.runs().len(), 2, "one config of 2 replicates");
    for recorded in set.runs() {
        let outcome = &recorded.outcome;
        assert_eq!(outcome.status, RunStatus::Ok, "{:?}", outcome.note);
        let replay = set
            .replay(gpu_sir.schema(), outcome.run.run_id)
            .expect("the run replays");
        let Ok(ModelState::Gpu(state)) = gpu_sir.build(&replay.params, Some(replay.seed), Some(&ctx)) else {
            panic!("gpu_sir builds on the GPU");
        };
        let settings = GpuBatchSettings {
            adaptive: false,
            batch_size: 16,
            ..GpuBatchSettings::default()
        };
        let mut thread = GpuSimThread::new(ctx.clone(), state, settings, None);
        thread.set_schedule(replay.schedule.clone());
        thread.run_to(replay.ticks);
        let reached = snapshot_at(|| thread.take_snapshot(), replay.ticks).expect("the loop reaches its target");
        let (_, last_row) = outcome.series.rows().last().expect("a run keeps its final sample");
        assert_eq!(
            recorded_row(set.stat_columns(), reached.tick, &reached.stats),
            last_row,
            "run {}: the live loop ends on its row's series",
            outcome.run.run_id
        );
    }
}

/// Checks that a [`Simulation`] stepped from a planned run's replay samples the series that the run cursor wrote,
/// including actions at tick 0, mid-run and on the last tick.
///
/// The simulation samples every fifth tick through `run_sampled`, and the sweep's own sampler keeps the ticks the
/// sweep samples, from the warm-up on.
#[test]
fn a_simulation_follows_the_run_cursor() {
    let sir = entry("sir", None);
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "32"), ("grid_height", "32"), ("recovery_rate", "0.05")]);
    spec.run.warmup = 5;
    spec.run.steps = 26;
    spec.run.replicates = 2;
    spec.measure.stats_every = 5;
    spec.measure.series_every = 5;
    spec.seeds.root = 4;
    spec.actions = vec![
        ActionSpec {
            name: "wave".to_owned(),
            ..ActionSpec::new("seed_outbreak", 0)
        },
        ActionSpec {
            name: "last".to_owned(),
            ..ActionSpec::new("seed_outbreak", 31)
        },
    ];
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::action("wave", values(&["0", "12"]))],
        design_seed: None,
    }];
    let (plan, measure) = planned(&sir, None, &spec);
    let mut outcomes = Collected::default();
    run_plan(&sir, None, &plan, &measure, lanes(1, 1), &mut outcomes);

    let sample_ticks: Vec<u64> = measure.sample_ticks().collect();
    assert_eq!(
        sample_ticks,
        [5, 10, 15, 20, 25, 30, 31],
        "the last tick is off the sampling boundary"
    );
    for outcome in &outcomes.0 {
        let run_id = outcome.run.run_id;
        assert_eq!(outcome.status, RunStatus::Ok, "run {run_id}");
        let replay = plan.replay(run_id).expect("the run is planned");
        let mut simulation = RunSetup::from_replay(&sir, &replay)
            .expect("the replay fits SIR")
            .build(None)
            .expect("SIR builds");
        let mut sampler = Sampler::new(Arc::clone(&measure));
        let flow = simulation
            .run_sampled(replay.ticks, 5, |sample| {
                if sample_ticks.contains(&sample.tick()) {
                    let stops = sampler
                        .push(sample.tick(), sample.entries())
                        .expect("the stats fit the columns");
                    assert!(!stops, "the spec has no stop condition");
                }
                ControlFlow::<()>::Continue(())
            })
            .expect("SIR steps");
        assert!(flow.is_continue());
        assert_eq!(simulation.tick(), replay.ticks);
        let followed = sampler.finish();
        assert_eq!(followed.series, outcome.series, "run {run_id}");
        assert_eq!(followed.reducers, outcome.reducers, "run {run_id}");
    }
}
