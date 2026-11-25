//! Checks that a spec's floats survive the text of `manifest.json` bit for bit.

use std::time::Duration;

use henad_core::explore::factor::FactorSpec;
use henad_core::explore::search::pse::{PatternAxis, PatternSpaceSettings};
use henad_core::explore::search::{SearchAlgorithm, SearchSpec};
use henad_core::explore::spec::SweepSpec;

use crate::handle::{SweepEvent, SweepRun, SweepRunOptions};
use crate::output::manifest::ManifestStatus;
use crate::result_set::ResultSet;
use crate::sweep::SweepEnd;
use crate::tests::support::{ScratchDir, drain, entry, fixed, interrupted_after, lane_count, provenance, range};

/// Upper bound of the x axis, 17 significant digits that `serde_json`'s default parser reads with an error of one unit
/// in the last place.
const AXIS_MAX: f64 = 255.556_822_519_348_06;

/// Maximum time the resume may take.
const PATIENCE: Duration = Duration::from_secs(120);

/// Returns a Pattern Space Exploration of SIR on a 16 by 16 grid, with [`AXIS_MAX`] as the x axis's upper bound.
fn search_spec() -> SweepSpec {
    let mut spec = SweepSpec::new("sir");
    spec.fixed = fixed(&[("grid_width", "16"), ("grid_height", "16")]);
    spec.run.steps = 20;
    spec.measure.default_reducers = false;
    spec.measure.reducers = ["Infected:max", "Infected:argmax"]
        .map(|raw| raw.parse().expect("a valid reducer"))
        .to_vec();
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
    let report = interrupted_after(&entry("sir", None), &spec, scratch.path(), lane_count(1), 5);
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
    match drain(&mut run, PATIENCE).last() {
        Some(SweepEvent::Finished(record)) => assert_eq!(record.manifest.status, ManifestStatus::Complete),
        Some(SweepEvent::Failed(message)) => panic!("the resume failed: {message}"),
        _ => panic!("the resume sent no end"),
    }
}
