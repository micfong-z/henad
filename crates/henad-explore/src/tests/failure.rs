//! Checks that a run that fails, or samples a value that is not finite, is recorded and leaves the rest running.

use std::num::NonZeroUsize;

use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::spec::{BlockSpec, SweepSpec};
use henad_models::registry::register_grid_model;

use crate::exec::Concurrency;
use crate::output::manifest::{Manifest, ManifestStatus};
use crate::output::{MANIFEST_FILE, SUMMARY_FILE};
use crate::sweep::ExploreError;
use crate::tests::broken::{DividesByParam, InverseOfCountdown};
use crate::tests::support::{Recorder, ScratchDir, Tables, manifest, sweep, sweep_options, sweep_with};

/// Returns a spec over `model` on an 8 by 8 grid, with one factorial block over `param` at `levels`.
fn spec_over(model: &str, param: &str, levels: &[&str]) -> SweepSpec {
    let mut spec = SweepSpec::new(model);
    spec.fixed = vec![
        ("grid_width".to_owned(), "8".to_owned()),
        ("grid_height".to_owned(), "8".to_owned()),
    ];
    spec.run.steps = 10;
    spec.measure.stats_every = 2;
    spec.measure.series_every = 2;
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param(
            param,
            LevelSpec::Values(levels.iter().map(|&level| level.to_owned()).collect()),
        )],
        design_seed: None,
    }];
    spec
}

#[test]
fn a_panicking_run_is_recorded_and_the_rest_complete() {
    let model = register_grid_model::<DividesByParam>();
    let mut spec = spec_over("divides_by_param", "divisor", &["1", "0", "2"]);
    spec.run.replicates = 2;
    let scratch = ScratchDir::new("panicking-run");
    for count in [1, 3] {
        let output_dir = scratch.path().join(format!("lanes-{count}"));
        let lanes = Concurrency::Fixed(NonZeroUsize::new(count).expect("a test lane count is above 0"));
        let report = sweep(&model, None, &spec, &output_dir, lanes);
        assert_eq!(
            (report.counts.rows, report.counts.ok, report.counts.failed),
            (6, 4, 2),
            "{count} lanes"
        );

        let tables = Tables::read(&output_dir);
        assert_eq!(
            tables.run_column("status"),
            ["ok", "ok", "panicked", "panicked", "ok", "ok"]
        );
        assert_eq!(tables.run_column("stop_reason")[2..4], ["fault", "fault"]);
        assert_eq!(tables.run_column("ticks"), ["10", "10", "0", "0", "10", "10"]);
        for note in &tables.run_column("note")[2..4] {
            assert!(note.starts_with("while stepping the simulation"), "{note}");
            assert!(
                note.contains("broken.rs:"),
                "the note names the panic's file and line: {note}"
            );
        }
        assert!(tables.run_column("note")[4].is_empty());
        let run_two: Vec<&str> = tables
            .series
            .iter()
            .filter(|record| record[0] == "2")
            .map(|record| record[1].as_str())
            .collect();
        assert_eq!(run_two, ["0"], "the failed run keeps the sample before its first step");

        assert_eq!(tables.summary_column("divisor"), ["1", "0", "2"]);
        let counts = ["runs", "ok", "failed"].map(|name| tables.summary_column(name)[1]);
        assert_eq!(counts, ["2", "0", "2"]);

        let manifest: Manifest = serde_json::from_str(
            &std::fs::read_to_string(output_dir.join(MANIFEST_FILE)).expect("the manifest is written"),
        )
        .expect("the manifest reads back");
        assert_eq!(manifest.status, ManifestStatus::Complete);
        assert_eq!(manifest.results, Some(report.counts));
    }
}

#[test]
fn a_first_config_that_panics_while_building_is_recorded_like_any_other() {
    let model = register_grid_model::<DividesByParam>();
    let spec = spec_over("divides_by_param", "init_divisor", &["0", "1"]);
    let scratch = ScratchDir::new("first-config-panics");
    let report = sweep(&model, None, &spec, scratch.path(), Concurrency::Auto);
    assert_eq!((report.counts.rows, report.counts.ok, report.counts.failed), (2, 1, 1));

    let tables = Tables::read(scratch.path());
    assert_eq!(tables.run_column("status"), ["panicked", "ok"]);
    assert_eq!(tables.run_column("ticks"), ["0", "10"]);
    let note = tables.run_column("note")[0];
    assert!(note.starts_with("while building the model"), "{note}");
}

#[test]
fn a_non_finite_stat_is_recorded_as_non_finite() {
    let model = register_grid_model::<InverseOfCountdown>();
    let spec = spec_over("inverse_of_countdown", "countdown", &["4", "100"]);
    let scratch = ScratchDir::new("non-finite-stat");
    let report = sweep(&model, None, &spec, scratch.path(), Concurrency::Auto);
    assert_eq!(
        (
            report.counts.rows,
            report.counts.ok,
            report.counts.non_finite,
            report.counts.failed
        ),
        (2, 1, 1, 0)
    );

    let tables = Tables::read(scratch.path());
    assert_eq!(tables.run_column("status"), ["non_finite", "ok"]);
    assert_eq!(tables.run_column("stop_reason"), ["steps", "steps"]);
    assert_eq!(tables.run_column("ticks"), ["10", "10"]);
    assert_eq!(tables.run_column("note"), ["Inverse is not finite at tick 4", ""]);
    let reducers: Vec<&str> = ["final", "min", "max", "mean"]
        .into_iter()
        .map(|kind| tables.run_column(&format!("Inverse:{kind}"))[0])
        .collect();
    assert_eq!(
        reducers,
        ["0.5", "0.25", "0.5", "0.375"],
        "the reducers skip the samples at ticks 4 to 10"
    );
    let inverse: Vec<&str> = tables
        .series
        .iter()
        .filter(|record| record[0] == "0")
        .map(|record| record[2].as_str())
        .collect();
    assert_eq!(
        inverse,
        ["0.25", "0.5", "", "", "", ""],
        "the series keeps them as empty cells"
    );

    let counts = ["runs", "ok", "failed"].map(|name| tables.summary_column(name)[0]);
    assert_eq!(
        counts,
        ["1", "0", "0"],
        "a run that is not finite is neither ok nor failed"
    );
}

#[test]
fn a_sweep_that_fails_once_its_manifest_is_written_is_marked_failed() {
    let model = register_grid_model::<DividesByParam>();
    let spec = spec_over("divides_by_param", "divisor", &["1", "2"]);
    let scratch = ScratchDir::new("failed-sweep");
    sweep(&model, None, &spec, scratch.path(), Concurrency::Auto);
    // A directory in place of summary.csv stops the resume after it has written its manifest.
    std::fs::remove_file(scratch.path().join(SUMMARY_FILE)).expect("summary.csv is written");
    std::fs::create_dir(scratch.path().join(SUMMARY_FILE)).expect("a directory takes its place");
    let error = sweep_with(
        &model,
        None,
        &spec,
        &sweep_options(scratch.path(), true),
        &mut Recorder::default(),
    )
    .expect_err("summary.csv cannot be written");
    assert!(matches!(error, ExploreError::Output(_)), "{error:?}");
    let recorded = manifest(scratch.path());
    assert_eq!(recorded.status, ManifestStatus::Failed);
    assert_eq!(recorded.results, None);
    assert!(recorded.timestamps.finished.is_some());
}
