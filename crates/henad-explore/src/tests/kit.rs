//! Checks of the testing kit itself: each broken model fails exactly the checks its bug breaks.

use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;

use henad_compute::entry::{ModelEntry, ModelSet, register_agent_model, register_grid_model, register_network_model};
use henad_compute::fault::install_panic_hook;

use crate::testing::{
    CheckSettings, ModelCheck, ModelReport, SkipReason, assert_set_conforms, check_model, check_model_set,
};
use crate::tests::broken::{
    BadId, Bug, BuggyState, CountsBuilds, CountsViews, DeclaresNumAgents, DividesByParam, EmptyPalette,
    InverseOfCountdown, ReadsPoolWidth, RepeatsActionId, RepeatsStatLabel, SharedAccumulator, ZeroesFullSubmissions,
};
use crate::tests::support::{entry, headless_device};

/// Returns the checks `report` lists as failed, by name.
fn failed(report: &ModelReport) -> BTreeSet<String> {
    report
        .failures()
        .iter()
        .map(|failure| failure.check().to_string())
        .collect()
}

/// Returns `checks` by name.
fn names(checks: &[ModelCheck]) -> BTreeSet<String> {
    checks.iter().map(ToString::to_string).collect()
}

/// Checks `entry` under `settings` with the panic hook installed.
fn check(entry: &ModelEntry, settings: &CheckSettings) -> ModelReport {
    install_panic_hook();
    check_model(entry, settings)
}

/// Asserts that `entry` fails exactly `expected` under the default settings.
fn assert_fails(entry: &ModelEntry, expected: &[ModelCheck]) {
    assert_fails_under(entry, &CheckSettings::default(), expected);
}

/// Asserts that `entry` fails exactly `expected` under `settings`.
fn assert_fails_under(entry: &ModelEntry, settings: &CheckSettings, expected: &[ModelCheck]) {
    let report = check(entry, settings);
    assert_eq!(failed(&report), names(expected), "{report}");
}

#[test]
fn a_bad_id_fails_the_model_id_check() {
    assert_fails(&register_grid_model::<BadId>(), &[ModelCheck::ModelId]);
}

#[test]
fn a_repeated_stat_label_fails_the_stat_labels_check() {
    assert_fails(&register_grid_model::<RepeatsStatLabel>(), &[ModelCheck::StatLabels]);
}

#[test]
fn a_repeated_action_id_fails_the_action_ids_check() {
    assert_fails(&register_grid_model::<RepeatsActionId>(), &[ModelCheck::ActionIds]);
}

#[test]
fn an_empty_palette_fails_the_palette_check() {
    assert_fails(&register_grid_model::<EmptyPalette>(), &[ModelCheck::Palette]);
}

#[test]
fn a_parameter_repeating_an_engine_id_fails_the_param_ids_check() {
    let model = register_agent_model::<DeclaresNumAgents>();
    let report = check(&model, &CheckSettings::default());
    assert_eq!(failed(&report), names(&[ModelCheck::ParamIds]), "{report}");
    assert!(
        report.failures()[0].message().contains("engine prepends 'num_agents'"),
        "{report}"
    );
}

#[test]
fn a_model_that_draws_no_random_number_fails_the_seed_sensitivity_check() {
    let model = register_grid_model::<DividesByParam>();
    assert_fails(&model, &[ModelCheck::SeedSensitivity]);

    // --8<-- [start:exempt]
    let exempt = CheckSettings::default().exempt(model.id(), ModelCheck::SeedSensitivity, "draws no random number");
    // --8<-- [end:exempt]
    let report = check(&model, &exempt);
    assert!(report.passed(), "{report}");
    assert_eq!(
        report.skip_reason(ModelCheck::SeedSensitivity),
        Some(&SkipReason::Exempt("draws no random number".to_owned()))
    );
}

#[test]
fn a_kernel_panic_fails_every_check_that_steps() {
    let model = register_grid_model::<DividesByParam>();
    let settings = CheckSettings::default().set_text(model.id(), "divisor", "0");
    let report = check(&model, &settings);
    assert_eq!(
        failed(&report),
        names(&[
            ModelCheck::ThreadCount,
            ModelCheck::SameSeed,
            ModelCheck::SeedSensitivity,
            ModelCheck::SamplingCadence,
        ]),
        "{report}"
    );
    let message = report.failures()[0].message();
    assert!(
        message.contains("divide by zero") && message.contains("broken.rs"),
        "{message}"
    );
}

#[test]
fn a_build_panic_fails_every_check_that_builds() {
    let model = register_grid_model::<DividesByParam>();
    let settings = CheckSettings::default().set_text(model.id(), "init_divisor", "0");
    let report = check(&model, &settings);
    assert_eq!(
        failed(&report),
        names(&[
            ModelCheck::ApplyModes,
            ModelCheck::Views,
            ModelCheck::ParallelJobs,
            ModelCheck::Actions,
            ModelCheck::StatCount,
            ModelCheck::ThreadCount,
            ModelCheck::SameSeed,
            ModelCheck::SeedSensitivity,
            ModelCheck::SamplingCadence,
        ]),
        "{report}"
    );
}

/// A stat that stops being finite is a run's status in a sweep, and no contract of the model.
#[test]
fn a_stat_that_stops_being_finite_breaks_no_contract() {
    assert_fails(
        &register_grid_model::<InverseOfCountdown>(),
        &[ModelCheck::SeedSensitivity],
    );
}

#[test]
fn a_state_that_refuses_its_actions_fails_the_actions_check() {
    assert_fails(
        &BuggyState::wrap(entry("sir", None), Bug::RefusesActions),
        &[ModelCheck::Actions],
    );
}

#[test]
fn a_state_that_drops_a_stat_fails_the_stat_count_check() {
    assert_fails(
        &BuggyState::wrap(entry("sir", None), Bug::DropsLastStat),
        &[ModelCheck::StatCount],
    );
}

#[test]
fn a_state_that_accepts_every_edit_fails_the_apply_modes_check() {
    assert_fails(
        &BuggyState::wrap(entry("sir", None), Bug::AcceptsEveryEdit),
        &[ModelCheck::ApplyModes],
    );
}

#[test]
fn a_state_that_hides_its_grid_fails_the_views_check() {
    assert_fails(
        &BuggyState::wrap(entry("sir", None), Bug::HidesGrid),
        &[ModelCheck::Views],
    );
}

#[test]
fn a_build_that_differs_from_the_last_fails_every_check_comparing_two_builds() {
    assert_fails(
        &register_grid_model::<CountsBuilds>(),
        &[
            ModelCheck::ThreadCount,
            ModelCheck::SameSeed,
            ModelCheck::SamplingCadence,
        ],
    );
}

#[test]
fn a_view_preparation_that_writes_a_lane_fails_the_sampling_cadence_check() {
    assert_fails(&register_network_model::<CountsViews>(), &[ModelCheck::SamplingCadence]);
}

#[test]
fn a_build_that_reads_the_pool_width_fails_the_thread_count_check() {
    assert_fails(&register_grid_model::<ReadsPoolWidth>(), &[ModelCheck::ThreadCount]);
}

#[test]
fn a_shared_accumulator_fails_the_thread_count_check() {
    let model = register_agent_model::<SharedAccumulator>();
    let report = check(&model, &CheckSettings::default());
    let failure = report
        .failures()
        .iter()
        .find(|failure| failure.check() == ModelCheck::ThreadCount)
        .unwrap_or_else(|| panic!("{report}"));
    assert!(failure.message().contains("split into 14 jobs"), "{failure}");
}

/// A device the watchdog stopped reads zeros from then on, from every state on it. The check runs the single steps
/// first, and they read back what they computed.
#[test]
fn a_full_submission_that_reads_zeros_fails_the_full_submission_check() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let model = ZeroesFullSubmissions::wrap(entry("gpu_game_of_life", Some(&ctx)));
    let report = check(&model, &CheckSettings::default().gpu(ctx));
    assert_eq!(failed(&report), names(&[ModelCheck::FullSubmission]), "{report}");
}

/// A grid of 128 columns holds 64 rows a job, and 896 rows split into 14 jobs.
#[test]
fn the_thread_count_check_splits_a_grid_into_twice_the_high_thread_count() {
    let report = check(&entry("game_of_life", None), &CheckSettings::default());
    assert!(report.passed(), "{report}");
    assert_eq!(report.thread_count_jobs(), Some(14));
}

#[test]
fn a_cpu_model_skips_the_gpu_checks_as_another_backend() {
    let report = check(&entry("game_of_life", None), &CheckSettings::default());
    let skipped: Vec<(ModelCheck, &SkipReason)> = report
        .skipped()
        .iter()
        .map(|skipped| (skipped.check(), skipped.reason()))
        .collect();
    assert_eq!(
        skipped,
        [
            ModelCheck::DefaultsFit,
            ModelCheck::BaselineBuild,
            ModelCheck::FullSubmission,
            ModelCheck::SampledSlice
        ]
        .map(|check| (check, &SkipReason::OtherBackend))
    );
}

#[test]
fn an_exemption_of_a_check_that_does_not_apply_fails_it() {
    let model = entry("game_of_life", None);
    let settings = CheckSettings::default().exempt(model.id(), ModelCheck::FullSubmission, "no GPU");
    assert_fails_under(&model, &settings, &[ModelCheck::FullSubmission]);
}

#[test]
fn a_set_report_names_the_models_the_settings_name_and_the_set_lacks() {
    let mut models = ModelSet::new(crate::ENGINE_BUILD);
    models
        .insert(entry("game_of_life", None))
        .expect("an empty set takes it");
    let settings = CheckSettings::default()
        .set_text("game_of_lfe", "density", "0.5")
        .exempt("missing", ModelCheck::SameSeed, "a typo");
    let report = check_model_set(&models, &settings);
    assert_eq!(report.unknown_models(), ["game_of_lfe", "missing"]);
    assert!(!report.passed(), "{report}");
    assert!(report.reports()[0].passed(), "{report}");
}

#[test]
fn assert_set_conforms_panics_with_every_failure() {
    let mut models = ModelSet::new(crate::ENGINE_BUILD);
    models
        .insert(register_grid_model::<DividesByParam>())
        .expect("an empty set takes it");
    let panic = std::panic::catch_unwind(AssertUnwindSafe(|| {
        assert_set_conforms(&models, &CheckSettings::default());
    }))
    .expect_err("a model fails a check");
    let message = panic.downcast_ref::<String>().expect("the assert formats its message");
    assert!(
        message.starts_with("1 of 1 models failed their checks")
            && message.contains("Model 'divides_by_param' failed 1 of its checks: SeedSensitivity."),
        "{message}"
    );
}
