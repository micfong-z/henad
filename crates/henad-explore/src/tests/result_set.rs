//! Checks that a result set reads back what a sweep wrote, holds its series within a budget, and replays each run
//! as its plan does.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::outcome::RunOutcome;
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};

use crate::output::manifest::ManifestStatus;
use crate::output::read::ReadError;
use crate::output::{MANIFEST_FILE, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
use crate::progress::{Progress, ProgressEvent};
use crate::result_set::{ResultReplayError, ResultSet, ResultSetError, read_directory_series};
use crate::schema::model_schema;
use crate::sweep::SweepOptions;
use crate::tests::support::{ScratchDir, entry, sweep_with};

fn values(raw: &[&str]) -> LevelSpec {
    LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
}

/// Returns 3 configs of SIR on a 16 by 16 grid with 2 replicates of 30 steps, and an action whose tick varies.
fn sir_spec() -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = vec![
        ("grid_width".to_owned(), "16".to_owned()),
        ("grid_height".to_owned(), "16".to_owned()),
    ];
    spec.run.steps = 30;
    spec.run.replicates = 2;
    spec.measure.stats_every = 3;
    spec.measure.series_every = 6;
    spec.seeds.root = 8;
    spec.actions = vec![ActionSpec {
        name: "wave".to_owned(),
        ..ActionSpec::new("seed_outbreak", 12)
    }];
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Zip,
        factors: vec![
            FactorSpec::param("infection_rate", values(&["0.2", "0.4", "0.6"])),
            FactorSpec::action("wave", values(&["6", "12", "18"])),
        ],
        design_seed: None,
    }];
    spec
}

/// Progress that keeps every committed outcome.
#[derive(Debug, Default)]
struct Outcomes(Vec<RunOutcome>);

impl Progress for Outcomes {
    fn report(&mut self, event: &ProgressEvent<'_>) {
        if let ProgressEvent::RunCommitted(outcome) = event {
            self.0.push((*outcome).clone());
        }
    }
}

/// Runs the SIR sweep into `dir` and returns its committed outcomes.
fn run_sir_sweep(dir: &Path) -> Vec<RunOutcome> {
    run_sweep(dir, &sir_spec())
}

/// Runs `spec` over SIR into `dir` and returns its committed outcomes.
fn run_sweep(dir: &Path, spec: &SweepSpec) -> Vec<RunOutcome> {
    let options = SweepOptions {
        output_dir: Some(dir.to_owned()),
        ..SweepOptions::default()
    };
    let mut outcomes = Outcomes::default();
    sweep_with(&entry("sir", None), None, spec, &options, &mut outcomes).expect("the sweep runs");
    outcomes.0
}

/// Returns `outcome` with its timings zeroed, since `runs.csv` rounds them.
fn untimed(outcome: &RunOutcome) -> RunOutcome {
    RunOutcome {
        build_ms: 0.0,
        wall_ms: 0.0,
        ..outcome.clone()
    }
}

/// Returns every file of the directory `dir`, each as its name and bytes.
fn directory_entries(dir: &Path) -> Vec<(String, Vec<u8>)> {
    [MANIFEST_FILE, RUNS_FILE, SERIES_FILE, SUMMARY_FILE]
        .iter()
        .map(|&file| {
            let bytes = fs::read(dir.join(file)).expect("the file is written");
            (format!("picked/{file}"), bytes)
        })
        .collect()
}

#[test]
fn a_result_set_reads_back_every_run_of_a_directory() {
    let scratch = ScratchDir::new("result-set");
    let outcomes = run_sir_sweep(scratch.path());
    let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");

    assert!(set.is_complete());
    assert_eq!(set.dir(), Some(scratch.path()));
    let mut recorded_spec = sir_spec();
    recorded_spec.fixed.sort();
    assert_eq!(set.spec(), &recorded_spec, "fixed values come back by id");
    assert_eq!(set.stat_columns(), ["Susceptible", "Infected", "Recovered"]);
    assert_eq!(set.reducer_columns(), set.manifest().columns.reducers);
    let sir = entry("sir", None);
    let mut value_columns: Vec<String> = sir.param_descriptors.iter().map(|param| param.id.to_owned()).collect();
    value_columns.push("action.wave".to_owned());
    assert_eq!(set.value_columns(), value_columns);

    assert_eq!(set.runs().len(), outcomes.len());
    assert_eq!(set.held_series_count(), 6);
    for (recorded, outcome) in set.runs().iter().zip(&outcomes) {
        assert!(recorded.series_held);
        assert_eq!(untimed(&recorded.outcome), untimed(outcome));
        assert!(
            (recorded.outcome.wall_ms - outcome.wall_ms).abs() <= 1e-3,
            "runs.csv rounds a timing to three decimal places"
        );
    }
    let run = set.run(5).expect("run 5 is written");
    assert_eq!((run.block, run.values.last().map(String::as_str)), (0, Some("18")));
    assert!(set.run(6).is_none());

    let picked = ResultSet::from_files(directory_entries(scratch.path()), usize::MAX).expect("the files read");
    assert_eq!(picked.runs(), set.runs());
    assert_eq!(picked.dir(), None);
}

#[test]
fn the_series_budget_holds_a_prefix_of_the_runs() {
    let scratch = ScratchDir::new("result-set-budget");
    run_sir_sweep(scratch.path());
    let whole = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");
    let series = &whole.runs()[0].outcome.series;
    let run_bytes = series.len() * (series.width() + 1) * size_of::<f64>();

    for budget in [2 * run_bytes, 3 * run_bytes - 1] {
        let set = ResultSet::open_dir(scratch.path(), budget).expect("the directory reads");
        let held: Vec<bool> = set.runs().iter().map(|run| run.series_held).collect();
        assert_eq!(held, [true, true, false, false, false, false], "budget {budget}");
        assert_eq!(set.runs()[1].outcome.series, whole.runs()[1].outcome.series);
        assert!(set.runs()[2].outcome.series.is_empty());
    }
    let none = ResultSet::open_dir(scratch.path(), 0).expect("the directory reads");
    assert_eq!(none.held_series_count(), 0);

    let dropped: BTreeSet<u64> = [2, 4].into();
    let read = none.read_run_series(&dropped, usize::MAX).expect("series.csv reads");
    assert_eq!(read.series.keys().copied().collect::<Vec<_>>(), [2, 4]);
    assert_eq!(read.series[&4], whole.runs()[4].outcome.series);
    assert!(read.dropped_runs.is_empty());

    let asked: BTreeSet<u64> = [1, 2, 4].into();
    for (budget, read_runs, dropped_runs) in [
        (2 * run_bytes, vec![1, 2], vec![4]),
        (2 * run_bytes - 1, vec![1], vec![2, 4]),
        (0, Vec::new(), vec![1, 2, 4]),
    ] {
        let read = none.read_run_series(&asked, budget).expect("series.csv reads");
        assert_eq!(
            read.series.keys().copied().collect::<Vec<_>>(),
            read_runs,
            "budget {budget}"
        );
        assert_eq!(
            read.dropped_runs.into_iter().collect::<Vec<_>>(),
            dropped_runs,
            "budget {budget}"
        );
        assert!(
            read.series
                .iter()
                .all(|(&run_id, series)| *series == whole.runs()[run_id as usize].outcome.series),
            "every run read is whole"
        );
    }

    // A run with no rows is neither read nor dropped.
    let read = read_directory_series(scratch.path(), none.stat_columns(), &[1, 99].into(), usize::MAX)
        .expect("series.csv reads");
    assert_eq!(read.series.keys().copied().collect::<Vec<_>>(), [1]);
    assert!(read.dropped_runs.is_empty());

    let mut without_series = directory_entries(scratch.path());
    without_series.retain(|(name, _)| !name.ends_with(SERIES_FILE));
    let set = ResultSet::from_files(without_series, usize::MAX).expect("series.csv is optional");
    assert_eq!(set.held_series_count(), 0);
    assert_eq!(set.runs().len(), 6);
}

#[test]
fn a_run_with_rows_on_both_sides_of_the_budget_is_held_without_its_series() {
    let scratch = ScratchDir::new("result-set-straddle");
    run_sir_sweep(scratch.path());
    let series = fs::read_to_string(scratch.path().join(SERIES_FILE)).expect("series.csv is written");
    let mut lines = series.lines();
    let header = lines.next().expect("series.csv has a header");
    let mut rows_by_run: Vec<Vec<&str>> = vec![Vec::new(); 6];
    for row in lines {
        let run_id: usize = row.split(',').next().and_then(|id| id.parse().ok()).expect("a run id");
        rows_by_run[run_id].push(row);
    }
    let split = rows_by_run[0].len() / 2;
    assert!(split > 0, "run 0 has rows on both sides of the split");

    // Run 0's rows go on both sides of run 1's, and the budget runs out inside run 1.
    let mut reordered = vec![header];
    reordered.extend(&rows_by_run[0][..split]);
    reordered.extend(&rows_by_run[1]);
    reordered.extend(&rows_by_run[0][split..]);
    for rows in &rows_by_run[2..] {
        reordered.extend(rows);
    }
    let reordered: String = reordered.iter().map(|line| format!("{line}\n")).collect();
    let mut picked = directory_entries(scratch.path());
    picked.retain(|(name, _)| !name.ends_with(SERIES_FILE));
    picked.push((SERIES_FILE.to_owned(), reordered.into_bytes()));
    // A held row takes its tick and each stat value as an `f64`, every column but the run id.
    let row_bytes = (header.split(',').count() - 1) * size_of::<f64>();
    let set = ResultSet::from_files(picked, (split + 1) * row_bytes).expect("the files read");

    assert_eq!(set.runs().len(), rows_by_run.len(), "every run of the sweep is read");
    assert_eq!(set.held_series_count(), 0, "no run keeps part of its series");
    for run in set.runs() {
        assert!(!run.series_held, "run {}", run.outcome.run.run_id);
        assert!(run.outcome.series.is_empty(), "run {}", run.outcome.run.run_id);
    }
}

#[test]
fn a_series_header_with_other_columns_is_refused() {
    let scratch = ScratchDir::new("result-set-series-header");
    run_sir_sweep(scratch.path());
    let stat_columns = ResultSet::open_dir(scratch.path(), usize::MAX)
        .expect("the directory reads")
        .stat_columns()
        .to_vec();
    let series_path = scratch.path().join(SERIES_FILE);
    let series = fs::read_to_string(&series_path).expect("series.csv is written");
    let header_end = series.find('\n').expect("a header line");

    let renamed = format!("run_id,tick,Healthy,Infected,Recovered{}", &series[header_end..]);
    fs::write(&series_path, renamed).expect("series.csv is rewritten");
    let error = ResultSet::open_dir(scratch.path(), usize::MAX).expect_err("a stat column is renamed");
    assert!(
        matches!(
            &error,
            ResultSetError::Table(ReadError::BadField { record_number: 1, column, text, .. })
                if column == "Susceptible" && text == "Healthy"
        ),
        "{error:?}"
    );
    let error = read_directory_series(scratch.path(), &stat_columns, &[0].into(), usize::MAX)
        .expect_err("a stat column is renamed");
    assert!(
        matches!(error, ResultSetError::Table(ReadError::BadField { .. })),
        "{error:?}"
    );

    let renamed = format!("run_id,time,Susceptible,Infected,Recovered{}", &series[header_end..]);
    fs::write(&series_path, renamed).expect("series.csv is rewritten");
    let error = ResultSet::open_dir(scratch.path(), usize::MAX).expect_err("an id column is renamed");
    assert!(
        matches!(
            error,
            ResultSetError::Table(ReadError::MissingColumn { column: "tick", .. })
        ),
        "{error:?}"
    );
}

#[test]
fn a_directory_a_stopped_sweep_left_opens_with_its_written_runs() {
    let scratch = ScratchDir::new("result-set-stopped");
    run_sir_sweep(scratch.path());
    let whole = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");

    // A sweep killed while writing run 3 leaves its row cut short, and later runs' series rows.
    let runs = fs::read_to_string(scratch.path().join(RUNS_FILE)).expect("runs.csv is written");
    let fourth_row = runs
        .match_indices('\n')
        .nth(3)
        .map(|(index, _)| index + 1)
        .expect("four lines");
    fs::write(scratch.path().join(RUNS_FILE), &runs[..fourth_row + 10]).expect("runs.csv is cut");
    let manifest = fs::read_to_string(scratch.path().join(MANIFEST_FILE)).expect("the manifest is written");
    let running = manifest.replace("\"status\": \"complete\"", "\"status\": \"running\"");
    assert_ne!(running, manifest);
    fs::write(scratch.path().join(MANIFEST_FILE), running).expect("the manifest is rewritten");
    let mut series = fs::read_to_string(scratch.path().join(SERIES_FILE)).expect("series.csv is written");
    series.push_str("5,3,1");
    fs::write(scratch.path().join(SERIES_FILE), series).expect("series.csv gains a partial line");

    let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");
    assert!(!set.is_complete());
    assert_eq!(set.manifest().status, ManifestStatus::Running);
    assert_eq!(
        set.runs(),
        &whole.runs()[..3],
        "the runs written in full, their series whole"
    );
}

#[test]
fn a_runs_table_without_a_header_opens_with_no_runs() {
    let scratch = ScratchDir::new("result-set-no-header");
    run_sir_sweep(scratch.path());
    let runs_path = scratch.path().join(RUNS_FILE);
    let runs = fs::read_to_string(&runs_path).expect("runs.csv is written");
    let partial_header = &runs[..runs.find('\n').expect("a header line") / 2];

    // A sweep killed before its first run leaves both tables empty, or a header cut short.
    for runs_text in ["", partial_header] {
        fs::write(&runs_path, runs_text).expect("runs.csv is rewritten");
        fs::write(scratch.path().join(SERIES_FILE), "").expect("series.csv is emptied");
        let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");
        assert!(set.runs().is_empty(), "{runs_text:?}");
        assert!(set.value_columns().is_empty() && set.reducer_columns().is_empty());
        assert_eq!(set.stat_columns(), ["Susceptible", "Infected", "Recovered"]);
    }
}

#[test]
fn picked_files_are_known_by_their_contents() {
    let scratch = ScratchDir::new("result-set-renamed");
    run_sir_sweep(scratch.path());
    let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");

    // A browser saves a repeated download as `runs (1).csv`.
    let renamed: Vec<(String, Vec<u8>)> = directory_entries(scratch.path())
        .into_iter()
        .map(|(name, bytes)| (name.replace('.', " (1)."), bytes))
        .collect();
    assert!(renamed.iter().any(|(name, _)| name == "picked/runs (1).csv"));
    let picked = ResultSet::from_files(renamed.clone(), usize::MAX).expect("the renamed files read");
    assert_eq!(picked.runs(), set.runs());
    assert_eq!(picked.held_series_count(), 6, "the series are found too");

    let mut twice = renamed;
    let runs = twice
        .iter()
        .find(|(name, _)| name.ends_with(".csv") && name.contains("runs"))
        .cloned()
        .expect("runs.csv is picked");
    twice.push(("runs.csv".to_owned(), runs.1));
    let error = ResultSet::from_files(twice, usize::MAX).expect_err("two runs tables");
    assert!(
        matches!(error, ResultSetError::Duplicate { file: RUNS_FILE }),
        "{error:?}"
    );
}

#[test]
fn missing_files_are_named() {
    let scratch = ScratchDir::new("result-set-missing");
    run_sir_sweep(scratch.path());
    for required in [MANIFEST_FILE, RUNS_FILE] {
        let mut picked = directory_entries(scratch.path());
        picked.retain(|(name, _)| !name.ends_with(required));
        let error = ResultSet::from_files(picked, usize::MAX).expect_err("a required file is missing");
        assert!(
            matches!(error, ResultSetError::Missing { file } if file == required),
            "{error:?}"
        );
    }
    fs::remove_file(scratch.path().join(RUNS_FILE)).expect("runs.csv is removed");
    let error = ResultSet::open_dir(scratch.path(), usize::MAX).expect_err("runs.csv is missing");
    assert!(matches!(error, ResultSetError::Missing { file: RUNS_FILE }));
    let error = ResultSet::open_dir(&scratch.path().join("absent"), usize::MAX).expect_err("no directory");
    assert!(matches!(error, ResultSetError::Missing { file: MANIFEST_FILE }));
}

#[test]
fn a_result_set_replays_each_run_as_its_plan_does() {
    let scratch = ScratchDir::new("result-set-replay");
    run_sir_sweep(scratch.path());
    let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the directory reads");
    let sir = entry("sir", None);
    assert!(set.schema_matches(&sir));
    assert!(!set.schema_matches(&entry("game_of_life", None)));

    let plan = sir_spec().plan(&model_schema(&sir)).expect("a valid spec");
    assert_eq!(set.plan(&sir).as_ref(), Ok(&plan));
    for run_id in 0..6 {
        assert_eq!(set.replay(&sir, run_id).ok(), plan.replay(run_id), "run {run_id}");
    }
    assert!(matches!(
        set.replay(&sir, 6),
        Err(ResultReplayError::UnknownRun { run_id: 6 })
    ));
    assert!(matches!(
        set.replay(&entry("game_of_life", None), 0),
        Err(ResultReplayError::Plan(_))
    ));
}

#[test]
fn a_sweep_replay_refuses_a_row_of_another_sweep() {
    let scratch = ScratchDir::new("result-set-mispicked");
    let (first, second) = (scratch.path().join("first"), scratch.path().join("second"));
    run_sir_sweep(&first);
    let mut other_spec = sir_spec();
    other_spec.blocks[0].factors[0] = FactorSpec::param("infection_rate", values(&["0.3", "0.5", "0.7"]));
    run_sweep(&second, &other_spec);
    let own = ResultSet::open_dir(&first, usize::MAX).expect("the directory reads");

    // The rows of the second sweep carry the ids, replicates and seeds of the first's, with other rates.
    let mut picked = directory_entries(&first);
    picked.retain(|(name, _)| !name.ends_with(RUNS_FILE));
    picked.push((
        RUNS_FILE.to_owned(),
        fs::read(second.join(RUNS_FILE)).expect("runs.csv is written"),
    ));
    let mixed = ResultSet::from_files(picked, usize::MAX).expect("the files read");
    let sir = entry("sir", None);
    for run_id in 0..6 {
        let position = run_id as usize;
        assert_eq!(mixed.runs()[position].outcome.run, own.runs()[position].outcome.run);
        assert!(own.replay(&sir, run_id).is_ok(), "run {run_id}");
        assert!(
            matches!(mixed.replay(&sir, run_id), Err(ResultReplayError::Mismatch { run_id: id }) if id == run_id),
            "run {run_id}"
        );
    }
}
