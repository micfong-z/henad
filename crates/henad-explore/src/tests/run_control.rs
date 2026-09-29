//! Checks of the controls on one run: stop conditions, actions and their ticks as factors.

use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::plan::PlanWarning;
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
use henad_core::explore::stop::StopSpec;

use crate::exec::Concurrency;
use crate::sweep::SweepWarning;
use crate::tests::broken::RefusesActions;
use crate::tests::support::{Recorder, ScratchDir, Tables, entry, sweep, sweep_options, sweep_with};

fn values(raw: &[&str]) -> LevelSpec {
    LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
}

fn fixed(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|&(id, value)| (id.to_owned(), value.to_owned()))
        .collect()
}

/// Returns an SIR sweep of 2 replicates on a 24 by 24 grid, keeping every sample in the series.
fn sir_spec(steps: u64, stats_every: u64) -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "24"), ("grid_height", "24"), ("recovery_rate", "0.2")]);
    spec.run.steps = steps;
    spec.run.replicates = 2;
    spec.measure.stats_every = stats_every;
    spec.measure.series_every = stats_every;
    spec.seeds.root = 17;
    spec
}

/// Returns the first tick of `series` at or after `min_tick` whose value is at least `threshold`.
fn first_reaching(series: &[(u64, &str)], threshold: f64, min_tick: u64) -> Option<u64> {
    series
        .iter()
        .find(|&&(tick, value)| tick >= min_tick && value.parse::<f64>().is_ok_and(|value| value >= threshold))
        .map(|&(tick, _)| tick)
}

#[test]
fn a_run_stops_on_the_first_sample_where_the_condition_holds() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("stop");
    let spec = sir_spec(120, 4);
    sweep(&sir, None, &spec, &scratch.path().join("full"), Concurrency::Auto);
    let full = Tables::read(&scratch.path().join("full"));
    let threshold: f64 = full
        .series_of(0, "Recovered")
        .iter()
        .find(|&&(tick, _)| tick == 20)
        .and_then(|(_, value)| value.parse().ok())
        .expect("run 0 samples Recovered at tick 20");

    for (name, min_tick) in [("from-start", 0), ("from-40", 40)] {
        let mut stopping = spec.clone();
        stopping.run.stop =
            Some(StopSpec::parse(&format!("Recovered >= {threshold}"), min_tick).expect("a valid condition"));
        let output_dir = scratch.path().join(name);
        sweep(&sir, None, &stopping, &output_dir, Concurrency::Auto);
        let stopped = Tables::read(&output_dir);
        for run_id in 0..2 {
            let full_series = full.series_of(run_id, "Recovered");
            let expected = first_reaching(&full_series, threshold, min_tick).expect("Recovered never falls");
            let row = run_id as usize;
            assert_eq!(
                stopped.run_column("ticks")[row],
                expected.to_string(),
                "{name}, run {run_id}"
            );
            assert_eq!(stopped.run_column("stop_reason")[row], "condition");
            assert_eq!(stopped.run_column("status")[row], "ok");
            let kept: Vec<(u64, &str)> = full_series.into_iter().filter(|&(tick, _)| tick <= expected).collect();
            assert_eq!(
                stopped.series_of(run_id, "Recovered"),
                kept,
                "the series ends on the sample where the condition held"
            );
            let last = kept.last().map(|&(_, value)| value);
            assert_eq!(Some(stopped.run_column("Recovered:final")[row]), last);
        }
    }

    let mut never = spec.clone();
    never.run.stop = Some(StopSpec::parse("Recovered < 0", 0).expect("a valid condition"));
    let output_dir = scratch.path().join("never");
    sweep(&sir, None, &never, &output_dir, Concurrency::Auto);
    let unstopped = Tables::read(&output_dir);
    assert_eq!(unstopped.run_column("stop_reason"), ["steps", "steps"]);
    assert_eq!(
        unstopped.series, full.series,
        "a condition that never holds changes nothing"
    );
}

/// Returns an SIR sweep whose one action, `wave`, seeds an outbreak at each tick of `ticks`.
fn wave_spec(ticks: &[&str]) -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[
        ("grid_width", "32"),
        ("grid_height", "32"),
        ("infection_rate", "0.1"),
        ("recovery_rate", "0.1"),
        ("initial_infected_pct", "0.2"),
    ]);
    spec.run.steps = 40;
    spec.measure.stats_every = 2;
    spec.measure.series_every = 2;
    spec.seeds.root = 4;
    spec.actions = vec![ActionSpec {
        name: "wave".to_owned(),
        ..ActionSpec::new("seed_outbreak", 0)
    }];
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::action("wave", values(ticks))],
        design_seed: None,
    }];
    spec
}

#[test]
fn an_action_tick_factor_moves_the_action() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("action-tick");
    sweep(
        &sir,
        None,
        &wave_spec(&["0", "10", "30"]),
        scratch.path(),
        Concurrency::Auto,
    );
    let tables = Tables::read(scratch.path());
    assert_eq!(tables.run_column("action.wave"), ["0", "10", "30"]);
    assert_eq!(tables.summary_column("action.wave"), ["0", "10", "30"]);
    assert_eq!(tables.run_column("status"), ["ok", "ok", "ok"]);

    let [at_start, at_ten, at_thirty] = [0, 1, 2].map(|run_id| tables.series_of(run_id, "Infected"));
    assert_ne!(
        at_start[0], at_ten[0],
        "an action at tick 0 fires before the first sample"
    );
    for tick in (0..10).step_by(2) {
        let row = tick / 2;
        assert_eq!(at_ten[row], at_thirty[row], "tick {tick} comes before either action");
    }
    assert_ne!(
        at_ten[5], at_thirty[5],
        "the sample at tick 10 sees the action due there"
    );
    assert_eq!(at_ten[5].0, 10);
}

#[test]
fn an_action_due_after_the_last_tick_is_warned_about() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("late-action");
    let mut progress = Recorder::default();
    let report = sweep_with(
        &sir,
        None,
        &wave_spec(&["10", "41", "90"]),
        &sweep_options(scratch.path(), false),
        &mut progress,
    )
    .expect("the sweep runs");
    assert_eq!(report.counts.ok, 3);
    assert_eq!(
        progress.warnings,
        [SweepWarning::Plan(PlanWarning::ActionAfterEnd {
            name: "wave".to_owned(),
            latest_tick: 90,
            last_tick: 40,
            config_count: 2,
        })]
    );
}

#[test]
fn a_refused_action_is_noted_and_the_run_stays_ok() {
    let refusing = RefusesActions::wrap(entry("sir", None));
    let scratch = ScratchDir::new("refused-action");
    let mut spec = wave_spec(&["6"]);
    spec.actions.insert(0, ActionSpec::new("seed_outbreak", 0));
    sweep(
        &refusing,
        None,
        &spec,
        &scratch.path().join("refused"),
        Concurrency::Auto,
    );
    let refused = Tables::read(&scratch.path().join("refused"));
    assert_eq!(refused.run_column("status"), ["ok"]);
    assert_eq!(refused.run_column("stop_reason"), ["steps"]);
    assert_eq!(
        refused.run_column("note"),
        ["model refused action 'seed_outbreak' at tick 0; model refused action 'seed_outbreak' at tick 6"]
    );

    let mut quiet = spec.clone();
    quiet.actions.clear();
    quiet.blocks.clear();
    sweep(
        &entry("sir", None),
        None,
        &quiet,
        &scratch.path().join("quiet"),
        Concurrency::Auto,
    );
    let quiet = Tables::read(&scratch.path().join("quiet"));
    assert_eq!(refused.series, quiet.series, "a refused action changes nothing");
}
