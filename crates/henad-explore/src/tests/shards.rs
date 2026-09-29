//! Checks that shards of a sweep merge into the files of the sweep run in one go.

use std::path::PathBuf;

use henad_core::explore::design::DesignKind;
use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::plan::Shard;
use henad_core::explore::spec::{BlockSpec, SweepSpec};

use crate::exec::Concurrency;
use crate::merge::{MergeError, merge};
use crate::output::OutputDir;
use crate::output::manifest::ManifestStatus;
use crate::sweep::{SweepOptions, SweepWarning};
use crate::tests::support::{Recorder, ScratchDir, Tables, entry, manifest, sweep, sweep_options, sweep_with};

/// Returns 3 configs of SIR on a 16 by 16 grid with 3 replicates each, 9 runs.
fn sir_spec() -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = vec![
        ("grid_width".to_owned(), "16".to_owned()),
        ("grid_height".to_owned(), "16".to_owned()),
    ];
    spec.run.steps = 24;
    spec.run.replicates = 3;
    spec.measure.stats_every = 4;
    spec.measure.series_every = 8;
    spec.seeds.root = 12;
    spec.blocks = vec![BlockSpec {
        design: DesignKind::Factorial,
        factors: vec![FactorSpec::param(
            "infection_rate",
            LevelSpec::Values(vec!["0.2".to_owned(), "0.4".to_owned(), "0.6".to_owned()]),
        )],
        design_seed: None,
    }];
    spec
}

#[test]
fn merged_shards_equal_an_unsharded_sweep() {
    let sir = entry("sir", None);
    let spec = sir_spec();
    let scratch = ScratchDir::new("shards");
    let whole_dir = scratch.path().join("whole");
    sweep(&sir, None, &spec, &whole_dir, Concurrency::Auto);

    let shard_dirs: Vec<PathBuf> = (0..3)
        .map(|index| {
            let shard_dir = scratch.path().join(format!("shard-{index}"));
            let options = SweepOptions {
                shard: Shard::new(index, 3).expect("a valid shard"),
                ..sweep_options(&shard_dir, false)
            };
            let mut progress = Recorder::default();
            let report = sweep_with(&sir, None, &spec, &options, &mut progress).expect("the shard runs");
            assert_eq!(report.outline.pending, 3);
            let expected: Vec<u64> = (0..9).filter(|run_id| run_id % 3 == index).collect();
            assert_eq!(progress.committed, expected);
            shard_dir
        })
        .collect();

    let merged_dir = scratch.path().join("merged");
    let inputs = [shard_dirs[2].clone(), shard_dirs[0].clone(), shard_dirs[1].clone()];
    let mut progress = Recorder::default();
    let report = merge(&inputs, &merged_dir, &mut progress).expect("the shards merge");
    assert_eq!((report.counts.rows, report.missing), (9, 0));
    assert!(progress.warnings.is_empty());
    assert_eq!(Tables::read(&merged_dir), Tables::read(&whole_dir));
    let merged = manifest(&merged_dir);
    assert_eq!(merged.status, ManifestStatus::Complete);
    assert_eq!((merged.shard.index, merged.shard.count), (0, 1));
    assert_eq!(merged.sessions.len(), 3);
    let listed = merged.merged_shards.expect("the merge lists its inputs");
    assert_eq!(listed, inputs.map(|dir| dir.display().to_string()));

    let same = merge(
        &[shard_dirs[0].clone(), shard_dirs[0].clone()],
        &scratch.path().join("same"),
        &mut Recorder::default(),
    )
    .expect_err("one shard twice");
    assert!(matches!(same, MergeError::SameShard { .. }), "{same:?}");
    assert!(!scratch.path().join("same").exists(), "a refused merge writes nothing");
}

#[test]
fn a_shard_whose_ids_its_manifest_does_not_give_is_refused() {
    let sir = entry("sir", None);
    let spec = sir_spec();
    let scratch = ScratchDir::new("renumbered-shard");
    let shard_dir = scratch.path().join("shard-0");
    let options = SweepOptions {
        shard: Shard::new(0, 3).expect("a valid shard"),
        ..sweep_options(&shard_dir, false)
    };
    sweep_with(&sir, None, &spec, &options, &mut Recorder::default()).expect("the shard runs");

    // A resume that raised the replicate count wrote its manifest, and ended before renumbering the rows.
    let mut raised = manifest(&shard_dir);
    raised.plan.replicates = 4;
    raised.plan.runs = 12;
    OutputDir::open(&shard_dir)
        .and_then(|dir| dir.write_manifest(&raised))
        .expect("the manifest writes");
    let merged_dir = scratch.path().join("merged");
    let error = merge(&[shard_dir], &merged_dir, &mut Recorder::default()).expect_err("the ids are stale");
    assert!(
        matches!(
            error,
            MergeError::RunIdDiffers {
                run_id: 3,
                config_id: 1,
                rep: 0,
                ..
            }
        ),
        "{error:?}"
    );
    assert!(!merged_dir.exists(), "a refused merge writes nothing");
}

#[test]
fn a_merge_missing_a_shard_is_filled_in_by_a_resume() {
    let sir = entry("sir", None);
    let spec = sir_spec();
    let scratch = ScratchDir::new("missing-shard");
    let shard_dirs: Vec<PathBuf> = [0, 2]
        .into_iter()
        .map(|index| {
            let shard_dir = scratch.path().join(format!("shard-{index}"));
            let options = SweepOptions {
                shard: Shard::new(index, 3).expect("a valid shard"),
                ..sweep_options(&shard_dir, false)
            };
            sweep_with(&sir, None, &spec, &options, &mut Recorder::default()).expect("the shard runs");
            shard_dir
        })
        .collect();
    let merged_dir = scratch.path().join("merged");
    let mut progress = Recorder::default();
    let report = merge(&shard_dirs, &merged_dir, &mut progress).expect("the shards merge");
    assert_eq!((report.counts.rows, report.missing), (6, 3));
    assert_eq!(
        progress.warnings,
        [SweepWarning::MissingRuns {
            count: 3,
            first: vec![1, 4, 7]
        }]
    );
    assert_eq!(manifest(&merged_dir).status, ManifestStatus::Incomplete);

    let mut progress = Recorder::default();
    sweep_with(&sir, None, &spec, &sweep_options(&merged_dir, true), &mut progress).expect("the merge resumes");
    assert_eq!(progress.committed, [1, 4, 7]);
    let whole_dir = scratch.path().join("whole");
    sweep(&sir, None, &spec, &whole_dir, Concurrency::Auto);
    assert_eq!(Tables::read(&merged_dir), Tables::read(&whole_dir));
    assert_eq!(manifest(&merged_dir).status, ManifestStatus::Complete);
}
