//! Checks that a resumed sweep keeps its finished runs, reruns the rest and ends with the files of a sweep in one go.

use std::fs;
use std::path::Path;
use std::time::Duration;

use henad_compute::entry::register_grid_model;
use henad_core::explore::factor::FactorSpec;
use henad_core::explore::plan::Shard;
use henad_core::explore::spec::SweepSpec;
use henad_core::metadata::{TimeScale, TimeUnit};
use serde_json::json;

use crate::exec::Concurrency;
use crate::output::manifest::{BuildRole, ManifestStatus, RecordedBuild};
use crate::output::resume::{ResumeError, check_time_scale};
use crate::output::{MANIFEST_FILE, OutputError, RUNS_FILE, SERIES_FILE, SUMMARY_FILE};
use crate::sweep::{ExploreError, SweepOptions, SweepWarning};
use crate::tests::broken::DividesByParam;
use crate::tests::support::{
    OutputTables, Recorder, ScratchDir, SecondResume, dry_run, entry, file_bytes, manifest, other_engine, provenance,
    rewrite_manifest, sir_spec, spec_over, sweep, sweep_options, sweep_with, values,
};

/// Runs a resume of `spec` into `output_dir`, and returns the ids of the runs it committed.
fn resume(entry_id: &str, spec: &SweepSpec, output_dir: &Path) -> Vec<u64> {
    let model = entry(entry_id, None);
    let mut progress = Recorder::default();
    sweep_with(&model, None, spec, output_dir, &sweep_options(true), &mut progress).expect("the sweep resumes");
    progress.committed
}

/// Names of the four files of an output directory.
const OUTPUT_FILES: [&str; 4] = [RUNS_FILE, SERIES_FILE, SUMMARY_FILE, MANIFEST_FILE];

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

    let committed = resume("sir", &spec, &resumed_dir);
    assert_eq!(committed, [2, 3, 4, 5], "runs 0 and 1 were kept");
    assert_eq!(OutputTables::read(&resumed_dir), OutputTables::read(&fresh_dir));
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
        resume("sir", &spec, &resumed_dir).is_empty(),
        "a complete directory has nothing left to run"
    );
    assert_eq!(OutputTables::read(&resumed_dir), OutputTables::read(&fresh_dir));
}

#[test]
fn a_second_resume_is_refused_while_the_first_holds_its_scan() {
    let sir = entry("sir", None);
    let spec = sir_spec(2);
    let scratch = ScratchDir::new("resume-locked");
    let fresh_dir = scratch.path().join("fresh");
    let resumed_dir = scratch.path().join("resumed");
    sweep(&sir, None, &spec, &fresh_dir, Concurrency::Auto);
    sweep(&sir, None, &spec, &resumed_dir, Concurrency::Auto);
    let runs = fs::read_to_string(resumed_dir.join(RUNS_FILE)).expect("runs.csv is written");
    fs::write(resumed_dir.join(RUNS_FILE), first_lines(&runs, 3)).expect("runs.csv is cut");

    let mut second = SecondResume::new(&sir, &spec, &resumed_dir);
    let report =
        sweep_with(&sir, None, &spec, &resumed_dir, &sweep_options(true), &mut second).expect("the first resume runs");
    let refused = second.result.expect("the first resume reports its outline");
    assert!(
        matches!(refused, Err(ExploreError::Output(OutputError::Locked { .. }))),
        "{refused:?}"
    );
    assert_eq!(report.outline.skipped, 2);
    assert_eq!(
        OutputTables::read(&resumed_dir),
        OutputTables::read(&fresh_dir),
        "no run is written twice"
    );
}

#[test]
fn a_resume_credits_a_session_its_process_left_unfinished() {
    let spec = sir_spec(2);
    let scratch = ScratchDir::new("resume-unfinished");
    sweep(&entry("sir", None), None, &spec, scratch.path(), Concurrency::Auto);

    // A process killed after its second run leaves two rows and the manifest it wrote at the start.
    let runs = fs::read_to_string(scratch.path().join(RUNS_FILE)).expect("runs.csv is written");
    fs::write(scratch.path().join(RUNS_FILE), first_lines(&runs, 3)).expect("runs.csv is cut");
    rewrite_manifest(scratch.path(), |recorded| {
        recorded.status = ManifestStatus::Running;
        recorded.results = None;
        recorded.sessions[0].ran = 0;
    });

    assert_eq!(resume("sir", &spec, scratch.path()), [2, 3, 4, 5]);
    let sessions: Vec<(u64, u64)> = manifest(scratch.path())
        .sessions
        .iter()
        .map(|session| (session.skipped, session.ran))
        .collect();
    assert_eq!(sessions, [(0, 2), (2, 4)]);
}

#[test]
fn a_resume_names_a_changed_model() {
    let sir = entry("sir", None);
    let spec = sir_spec(2);
    let scratch = ScratchDir::new("resume-model");
    sweep(&sir, None, &spec, scratch.path(), Concurrency::Auto);

    // Another version of the model changes the schema hash, and with it the plan hash.
    rewrite_manifest(scratch.path(), |recorded| {
        recorded.model.schema_hash = "0000000000000000".to_owned();
        recorded.plan.plan_hash = "0000000000000000".to_owned();
    });
    let error = sweep_with(
        &sir,
        None,
        &spec,
        scratch.path(),
        &sweep_options(true),
        &mut Recorder::default(),
    )
    .expect_err("another model");
    assert!(
        matches!(error, ExploreError::Resume(ResumeError::SchemaChanged { .. })),
        "{error:?}"
    );
}

/// A resume refuses a directory whose runs were timed on another time scale. The schema hash leaves the time scale
/// out, and a compiled model's changed code only warns.
#[test]
fn a_resume_refuses_a_changed_time_scale() {
    let sir = entry("sir", None);
    let spec = sir_spec(2);
    let scratch = ScratchDir::new("resume-time-scale");
    sweep(&sir, None, &spec, scratch.path(), Concurrency::Auto);

    let timed = |unit: TimeUnit, tick_length: &str| {
        let mut recorded = manifest(scratch.path());
        recorded.model.schema["time"] = json!({ "unit": unit.as_str(), "tick_length": tick_length });
        recorded
    };
    let days = TimeScale::new(TimeUnit::Day, "infection_rate");
    assert!(check_time_scale(&timed(TimeUnit::Day, "infection_rate"), Some(&days)).is_ok());
    for (recorded, what) in [
        (timed(TimeUnit::Hour, "infection_rate"), "another unit"),
        (timed(TimeUnit::Day, "recovery_rate"), "another tick length"),
        (manifest(scratch.path()), "no time scale"),
    ] {
        assert!(
            matches!(
                check_time_scale(&recorded, Some(&days)),
                Err(ResumeError::TimeScaleChanged { .. })
            ),
            "a directory with {what} passed"
        );
    }

    // A build whose model counted its ticks in days wrote the directory.
    rewrite_manifest(scratch.path(), |recorded| {
        recorded.model.schema["time"] = json!({ "unit": "day", "tick_length": "infection_rate" });
    });
    let error = sweep_with(
        &sir,
        None,
        &spec,
        scratch.path(),
        &sweep_options(true),
        &mut Recorder::default(),
    )
    .expect_err("another time scale");
    let ExploreError::Resume(error @ ResumeError::TimeScaleChanged { .. }) = error else {
        panic!("{error:?}");
    };
    assert_eq!(
        error.to_string(),
        "the directory holds runs on a different time scale (unit day and tick-length parameter 'infection_rate', \
         expected no time scale)"
    );
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

    let mut options = sweep_options(true);
    options.provenance = provenance().with_engine(other_engine());
    let mut progress = Recorder::default();
    let report = dry_run(&sir, &spec, Some(scratch.path()), &options, &mut progress).expect("the dry run plans");
    assert_eq!((report.outline.skipped, report.outline.pending), (3, 3));
    assert_eq!(
        progress.warnings,
        [SweepWarning::BuildChanged {
            role: BuildRole::Engine,
            recorded: Box::new(RecordedBuild::engine()),
            current: Box::new(other_engine()),
            between_shards: false,
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
    let written = file_bytes(scratch.path(), &OUTPUT_FILES);

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
            scratch.path(),
            &sweep_options(true),
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
        ..sweep_options(true)
    };
    let error =
        sweep_with(&sir, None, &spec, scratch.path(), &sharded, &mut Recorder::default()).expect_err("another shard");
    assert!(
        matches!(error, ExploreError::Resume(ResumeError::ShardChanged { .. })),
        "{error:?}"
    );
    for ((file, before), (_, after)) in written.iter().zip(file_bytes(scratch.path(), &OUTPUT_FILES)) {
        assert!(*before == after, "a refused resume leaves {file} as it was");
    }

    let mut timed = spec.clone();
    timed.run.timeout = Some(Duration::from_secs(600));
    assert!(
        resume("sir", &timed, scratch.path()).is_empty(),
        "the timeout is no part of the plan"
    );
}

#[test]
fn retry_failed_reruns_only_failures() {
    let model = register_grid_model::<DividesByParam>();
    let mut spec = spec_over("divides_by_param", "divisor", &["1", "0", "2"]);
    spec.run.replicates = 2;
    let scratch = ScratchDir::new("retry-failed");
    sweep(&model, None, &spec, scratch.path(), Concurrency::Auto);
    let first = OutputTables::read(scratch.path());
    assert_eq!(
        first.run_column("status"),
        ["ok", "ok", "panicked", "panicked", "ok", "ok"]
    );

    let resumed = |retry_failed: bool| {
        let options = SweepOptions {
            retry_failed,
            ..sweep_options(true)
        };
        let mut progress = Recorder::default();
        sweep_with(&model, None, &spec, scratch.path(), &options, &mut progress).expect("the sweep resumes");
        progress.committed
    };
    assert!(resumed(false).is_empty(), "a failed run counts as finished");
    assert_eq!(resumed(true), [2, 3]);
    assert_eq!(
        OutputTables::read(scratch.path()),
        first,
        "the retried runs fail as they did"
    );
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
    let committed = resume("sir", &sir_spec(3), &resumed_dir);
    assert_eq!(committed, [2, 5, 8], "replicate 2 of each config");

    let fresh_dir = scratch.path().join("fresh");
    sweep(&sir, None, &sir_spec(3), &fresh_dir, Concurrency::Auto);
    let resumed = OutputTables::read(&resumed_dir);
    assert_eq!(resumed, OutputTables::read(&fresh_dir));
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
        &resumed_dir,
        &sweep_options(true),
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
    let timed_out = OutputTables::read(&resumed_dir);
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

    let committed = resume("sir", &sir_spec(2), &resumed_dir);
    assert_eq!(committed, [0, 1, 2, 3, 4, 5], "a timed-out run is never finished");
    let fresh_dir = scratch.path().join("fresh");
    sweep(&sir, None, &sir_spec(2), &fresh_dir, Concurrency::Auto);
    assert_eq!(OutputTables::read(&resumed_dir), OutputTables::read(&fresh_dir));
}
