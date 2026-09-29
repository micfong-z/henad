//! Checks that a resumed sweep keeps its finished runs, reruns the rest and ends with the files of a sweep in one go.

use std::fs;
use std::path::Path;
use std::time::Duration;

use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::plan::Shard;
use henad_core::explore::spec::{ActionSpec, BlockSpec, SweepSpec};
use henad_models::registry::register_grid_model;

use crate::exec::Concurrency;
use crate::output::manifest::ManifestStatus;
use crate::output::resume::ResumeError;
use crate::output::{RUNS_FILE, SERIES_FILE};
use crate::sweep::{ExploreError, Provenance, SpecSource, SweepOptions, SweepWarning, run_sweep};
use crate::tests::broken::DividesByParam;
use crate::tests::support::{
    Recorder, ScratchDir, Tables, entry, manifest, provenance, sweep, sweep_options, sweep_with,
};

fn values(raw: &[&str]) -> LevelSpec {
    LevelSpec::Values(raw.iter().map(|&text| text.to_owned()).collect())
}

/// Returns 3 configs of SIR on a 16 by 16 grid with `replicates` replicates, and an action whose tick varies.
fn sir_spec(replicates: u64) -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = vec![
        ("grid_width".to_owned(), "16".to_owned()),
        ("grid_height".to_owned(), "16".to_owned()),
    ];
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

/// Runs a resume of `spec` into `output_dir` with `options`, and returns the ids of the runs it committed.
fn resume(entry_id: &str, spec: &SweepSpec, output_dir: &Path, options: SweepOptions) -> Vec<u64> {
    let model = entry(entry_id, None);
    let mut progress = Recorder::default();
    let options = SweepOptions {
        output_dir: Some(output_dir.to_owned()),
        resume: true,
        ..options
    };
    sweep_with(&model, None, spec, &options, &mut progress).expect("the sweep resumes");
    progress.committed
}

/// Returns the text of the first `line_count` lines of `text`.
fn first_lines(text: &str, line_count: usize) -> &str {
    let end = text
        .match_indices('\n')
        .nth(line_count - 1)
        .map_or(text.len(), |(index, _)| index + 1);
    &text[..end]
}

#[test]
fn a_resumed_sweep_skips_finished_runs_and_matches_a_fresh_one() {
    let sir = entry("sir", None);
    let spec = sir_spec(2);
    let scratch = ScratchDir::new("resume");
    let fresh_dir = scratch.path().join("fresh");
    let resumed_dir = scratch.path().join("resumed");
    sweep(&sir, None, &spec, &fresh_dir, Concurrency::Auto);
    sweep(&sir, None, &spec, &resumed_dir, Concurrency::Auto);

    // A process that ended while writing run 2 leaves its row cut short, and the series rows of later runs.
    let runs = fs::read_to_string(resumed_dir.join(RUNS_FILE)).expect("runs.csv is written");
    let kept = first_lines(&runs, 3);
    let partial = &first_lines(&runs, 4)[kept.len()..][..20];
    fs::write(resumed_dir.join(RUNS_FILE), format!("{kept}{partial}")).expect("runs.csv is cut");
    let mut series = fs::read_to_string(resumed_dir.join(SERIES_FILE)).expect("series.csv is written");
    series.push_str("5,1");
    fs::write(resumed_dir.join(SERIES_FILE), series).expect("series.csv gains a partial line");

    let committed = resume("sir", &spec, &resumed_dir, SweepOptions::default());
    assert_eq!(committed, [2, 3, 4, 5], "runs 0 and 1 were kept");
    assert_eq!(Tables::read(&resumed_dir), Tables::read(&fresh_dir));
    let recorded = manifest(&resumed_dir);
    assert_eq!(recorded.status, ManifestStatus::Complete);
    let sessions: Vec<(u64, u64)> = recorded
        .sessions
        .iter()
        .map(|session| (session.skipped, session.ran))
        .collect();
    assert_eq!(sessions, [(0, 6), (2, 4)]);
    assert_eq!(recorded.results.map(|results| results.rows), Some(6));

    assert!(
        resume("sir", &spec, &resumed_dir, SweepOptions::default()).is_empty(),
        "a complete directory has nothing left to run"
    );
    assert_eq!(Tables::read(&resumed_dir), Tables::read(&fresh_dir));
}

#[test]
fn a_dry_run_counts_the_runs_a_resume_skips_and_changes_nothing() {
    let sir = entry("sir", None);
    let spec = sir_spec(2);
    let scratch = ScratchDir::new("resume-dry-run");
    sweep(&sir, None, &spec, scratch.path(), Concurrency::Auto);
    let runs = fs::read_to_string(scratch.path().join(RUNS_FILE)).expect("runs.csv is written");
    let cut = first_lines(&runs, 4).to_owned();
    fs::write(scratch.path().join(RUNS_FILE), &cut).expect("runs.csv is cut");
    let series = fs::read(scratch.path().join(SERIES_FILE)).expect("series.csv is written");

    let options = SweepOptions {
        dry_run: true,
        ..sweep_options(scratch.path(), true)
    };
    let mut progress = Recorder::default();
    let provenance = Provenance {
        commit: "other".to_owned(),
        ..provenance()
    };
    let report = run_sweep(
        &sir,
        None,
        None,
        &spec,
        &SpecSource::default(),
        &provenance,
        &options,
        &mut progress,
    )
    .expect("the dry run plans");
    assert_eq!((report.outline.skipped, report.outline.pending), (3, 3));
    assert_eq!(
        progress.warnings,
        [SweepWarning::CommitChanged {
            recorded: "test".to_owned(),
            current: "other".to_owned(),
        }]
    );
    assert_eq!(
        fs::read_to_string(scratch.path().join(RUNS_FILE)).expect("runs.csv is kept"),
        cut
    );
    assert_eq!(
        fs::read(scratch.path().join(SERIES_FILE)).expect("series.csv is kept"),
        series,
        "a dry run leaves the orphan rows for the resume"
    );
}

#[test]
fn resume_refuses_a_changed_spec() {
    let sir = entry("sir", None);
    let spec = sir_spec(2);
    let scratch = ScratchDir::new("resume-changed");
    sweep(&sir, None, &spec, scratch.path(), Concurrency::Auto);
    let runs = fs::read(scratch.path().join(RUNS_FILE)).expect("runs.csv is written");

    let mut longer = spec.clone();
    longer.run.steps = 31;
    let mut other_seed = spec.clone();
    other_seed.seeds.root = 9;
    let mut moved = spec.clone();
    moved.blocks[0].factors[1] = FactorSpec::action("seed_outbreak", values(&["6", "12", "19"]));
    for changed in [longer, other_seed, moved] {
        let error = sweep_with(
            &sir,
            None,
            &changed,
            &sweep_options(scratch.path(), true),
            &mut Recorder::default(),
        )
        .expect_err("another plan");
        assert!(
            matches!(error, ExploreError::Resume(ResumeError::PlanChanged { .. })),
            "{error:?}"
        );
    }
    let sharded = SweepOptions {
        shard: Shard::new(1, 2).expect("a valid shard"),
        ..sweep_options(scratch.path(), true)
    };
    let error = sweep_with(&sir, None, &spec, &sharded, &mut Recorder::default()).expect_err("another shard");
    assert!(
        matches!(error, ExploreError::Resume(ResumeError::ShardChanged { .. })),
        "{error:?}"
    );
    assert_eq!(
        fs::read(scratch.path().join(RUNS_FILE)).expect("runs.csv is kept"),
        runs,
        "a refused resume changes nothing"
    );

    let mut timed = spec.clone();
    timed.run.timeout = Some(Duration::from_secs(600));
    assert!(
        resume("sir", &timed, scratch.path(), SweepOptions::default()).is_empty(),
        "the timeout is no part of the plan"
    );
}

#[test]
fn retry_failed_reruns_only_failures() {
    let model = register_grid_model::<DividesByParam>();
    let mut spec = SweepSpec::new("divides_by_param");
    spec.fixed = vec![
        ("grid_width".to_owned(), "8".to_owned()),
        ("grid_height".to_owned(), "8".to_owned()),
    ];
    spec.run.steps = 10;
    spec.run.replicates = 2;
    spec.measure.stats_every = 2;
    spec.measure.series_every = 2;
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param("divisor", values(&["1", "0", "2"]))],
        design_seed: None,
    }];
    let scratch = ScratchDir::new("retry-failed");
    sweep(&model, None, &spec, scratch.path(), Concurrency::Auto);
    let first = Tables::read(scratch.path());
    assert_eq!(
        first.run_column("status"),
        ["ok", "ok", "panicked", "panicked", "ok", "ok"]
    );

    let resumed = |retry_failed: bool| {
        let options = SweepOptions {
            retry_failed,
            ..sweep_options(scratch.path(), true)
        };
        let mut progress = Recorder::default();
        sweep_with(&model, None, &spec, &options, &mut progress).expect("the sweep resumes");
        progress.committed
    };
    assert!(resumed(false).is_empty(), "a failed run counts as finished");
    assert_eq!(resumed(true), [2, 3]);
    assert_eq!(Tables::read(scratch.path()), first, "the retried runs fail as they did");
    let sessions: Vec<(u64, u64)> = manifest(scratch.path())
        .sessions
        .iter()
        .map(|session| (session.skipped, session.ran))
        .collect();
    assert_eq!(sessions, [(0, 6), (6, 0), (4, 2)]);
}

#[test]
fn raising_replicates_on_resume_runs_only_the_new_ones() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("more-replicates");
    let resumed_dir = scratch.path().join("resumed");
    sweep(&sir, None, &sir_spec(2), &resumed_dir, Concurrency::Auto);
    let committed = resume("sir", &sir_spec(3), &resumed_dir, SweepOptions::default());
    assert_eq!(committed, [2, 5, 8], "replicate 2 of each config");

    let fresh_dir = scratch.path().join("fresh");
    sweep(&sir, None, &sir_spec(3), &fresh_dir, Concurrency::Auto);
    let resumed = Tables::read(&resumed_dir);
    assert_eq!(resumed, Tables::read(&fresh_dir));
    assert_eq!(resumed.run_column("rep"), ["0", "1", "2", "0", "1", "2", "0", "1", "2"]);
    let recorded = manifest(&resumed_dir);
    assert_eq!((recorded.plan.replicates, recorded.plan.runs), (3, 9));

    // Only run 0, replicate 0 of config 0, stays, so every kept row fits the lower count.
    let runs = fs::read_to_string(resumed_dir.join(RUNS_FILE)).expect("runs.csv is written");
    fs::write(resumed_dir.join(RUNS_FILE), first_lines(&runs, 2)).expect("runs.csv is cut");
    let error = sweep_with(
        &sir,
        None,
        &sir_spec(1),
        &sweep_options(&resumed_dir, true),
        &mut Recorder::default(),
    )
    .expect_err("fewer replicates");
    assert!(
        matches!(
            error,
            ExploreError::Resume(ResumeError::ReplicatesLowered {
                recorded: 3,
                current: 1
            })
        ),
        "{error:?}"
    );
}

#[test]
fn a_run_past_its_timeout_is_recorded_and_resume_retries_it() {
    let sir = entry("sir", None);
    let scratch = ScratchDir::new("timeout");
    let mut timed = sir_spec(2);
    timed.run.timeout = Some(Duration::ZERO);
    let resumed_dir = scratch.path().join("resumed");
    let report = sweep(&sir, None, &timed, &resumed_dir, Concurrency::Auto);
    assert_eq!((report.counts.rows, report.counts.failed), (6, 6));
    let timed_out = Tables::read(&resumed_dir);
    assert!(
        timed_out
            .run_column("status")
            .iter()
            .all(|&status| status == "timed_out")
    );
    assert!(
        timed_out
            .run_column("stop_reason")
            .iter()
            .all(|&reason| reason == "timeout")
    );
    for ticks in timed_out.run_column("ticks") {
        assert!(ticks.parse::<u64>().is_ok_and(|ticks| ticks < 30), "{ticks}");
    }
    assert!(timed_out.run_column("note")[0].starts_with("timed out after 0 s"));

    let committed = resume("sir", &sir_spec(2), &resumed_dir, SweepOptions::default());
    assert_eq!(committed, [0, 1, 2, 3, 4, 5], "a timed-out run is never finished");
    let fresh_dir = scratch.path().join("fresh");
    sweep(&sir, None, &sir_spec(2), &fresh_dir, Concurrency::Auto);
    assert_eq!(Tables::read(&resumed_dir), Tables::read(&fresh_dir));
}
