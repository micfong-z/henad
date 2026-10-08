//! Checks that a spec's floats survive the text of `manifest.json` bit for bit.

use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use henad_core::explore::factor::{FactorSpec, LevelSpec};
use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
use henad_core::explore::search::{SearchAlgorithm, SearchSpec};
use henad_core::explore::spec::SweepSpec;

use crate::exec::{Concurrency, SweepControl};
use crate::handle::{SweepEvent, SweepRun, SweepRunOptions};
use crate::output::manifest::ManifestStatus;
use crate::result_set::ResultSet;
use crate::sweep::{SweepEnd, SweepOptions};
use crate::tests::support::{CommitLimit, ScratchDir, entry, provenance, sweep_options, sweep_with};

/// Upper bound of the x axis, 17 significant digits that `serde_json`'s default parser reads with an error of one unit
/// in the last place.
const AXIS_MAX: f64 = 255.556_822_519_348_06;

/// Maximum time the resume may take.
const PATIENCE: Duration = Duration::from_secs(120);

/// Returns a Pattern Space Exploration of SIR on a 16 by 16 grid, with [`AXIS_MAX`] as the x axis's upper bound.
fn search_spec() -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = vec![
        ("grid_width".to_owned(), "16".to_owned()),
        ("grid_height".to_owned(), "16".to_owned()),
    ];
    spec.run.steps = 20;
    spec.measure.default_reducers = false;
    spec.measure.reducers = ["Infected:max", "Infected:argmax"]
        .map(|raw| raw.parse().expect("a valid reducer"))
        .to_vec();
    let range = |min, max| LevelSpec::Range { min, max, step: None };
    spec.search = Some(SearchSpec {
        algorithm: SearchAlgorithm::PatternSpaceExploration(PatternSpaceSettings {
            initial_samples: 4,
            ..PatternSpaceSettings::new(
                PatternAxis::bounded("Infected:max", 0.0, AXIS_MAX, 8),
                PatternAxis::bounded("Infected:argmax", 0.0, 20.0, 8),
            )
        }),
        max_evaluations: 12,
        batch_size: 4,
        objective: None,
        space: vec![
            FactorSpec::param("infection_rate", range(0.05, 0.9)),
            FactorSpec::param("recovery_rate", range(0.02, 0.3)),
        ],
    });
    spec
}

#[test]
fn a_search_resumes_from_a_manifest_holding_a_seventeen_digit_axis_bound() {
    let scratch = ScratchDir::new("manifest-floats");
    let spec = search_spec();
    let control = SweepControl::new();
    let interrupted = SweepOptions {
        concurrency: Concurrency::Fixed(NonZeroUsize::MIN),
        control: control.clone(),
        ..sweep_options(false)
    };
    let report = sweep_with(
        &entry("sir", None),
        None,
        &spec,
        scratch.path(),
        &interrupted,
        &mut CommitLimit::new(control, 5),
    )
    .expect("an abort is not an error");
    assert_eq!(report.end, SweepEnd::Aborted);

    let set = ResultSet::open_dir(scratch.path(), usize::MAX).expect("the folder reads back");
    let Some(SearchSpec {
        algorithm: SearchAlgorithm::PatternSpaceExploration(settings),
        ..
    }) = &set.spec().search
    else {
        panic!("the folder holds a Pattern Space Exploration");
    };
    assert_eq!(
        settings.x_axis.max.map(f64::to_bits),
        Some(AXIS_MAX.to_bits()),
        "the bound reads back bit for bit"
    );

    // The resume plans the spec that its manifest records, and rejects a search hash other than the recorded hash.
    let options = SweepRunOptions::new(provenance());
    let mut run =
        SweepRun::resume_directory(entry("sir", None), None, scratch.path(), options).expect("the resume starts");
    let deadline = Instant::now() + PATIENCE;
    let mut last = None;
    while !run.is_ended() {
        assert!(Instant::now() < deadline, "the resume did not end in time");
        run.update(0.0);
        match run.try_recv() {
            Some(event) => last = Some(event),
            None => std::thread::sleep(Duration::from_millis(2)),
        }
    }
    match last {
        Some(SweepEvent::Finished(record)) => assert_eq!(record.manifest.status, ManifestStatus::Complete),
        Some(SweepEvent::Failed(message)) => panic!("the resume failed: {message}"),
        _ => panic!("the resume sent no end"),
    }
}
