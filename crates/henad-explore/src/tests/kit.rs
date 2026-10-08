//! Checks of the testing kit itself: each broken model fails exactly the checks its bug breaks.

use std::collections::BTreeSet;
use std::panic::AssertUnwindSafe;

use henad_compute::entry::{
    ModelEntry, ModelSet, register_agent_model, register_gpu_agent_model, register_gpu_grid_model, register_grid_model,
    register_network_model,
};
use henad_compute::fault::install_panic_hook;
use henad_models::example_models;

use crate::testing::{
    CheckSettings, ModelCheck, ModelReport, SkipReason, assert_set_conforms, check_model, check_model_requiring,
    check_model_set,
};
use crate::tests::broken::{
    BadId, Bug, BuggyGpuState, BuggyState, CountsBuilds, CountsViews, DeclaresNumAgents, DefaultOutOfBounds,
    DividesByParam, EmptyPalette, GpuBug, InverseOfCountdown, OversizedGpuSir, PlacesCellByPoolWidth, PlacesCellBySeed,
    ReadsPoolWidth, RepeatsActionId, RepeatsStatLabel, SharedAccumulator, StatlessGpuBoids, UnnameableIds,
};
use crate::tests::support::{baseline_device, entry, headless_device};

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

/// Returns example model `id`, even a GPU model on a machine without a device.
fn example(id: &str) -> ModelEntry {
    example_models()
        .get(id)
        .cloned()
        .unwrap_or_else(|| panic!("the example models include {id}"))
}

/// Checks that build a GPU model. Every check that builds a model is in this list, apart from the CPU check
/// `ThreadCount`.
const GPU_BUILDING_CHECKS: [ModelCheck; 11] = [
    ModelCheck::ApplyModes,
    ModelCheck::Views,
    ModelCheck::ParallelJobs,
    ModelCheck::Actions,
    ModelCheck::StatCount,
    ModelCheck::SameSeed,
    ModelCheck::SeedSensitivity,
    ModelCheck::SamplingCadence,
    ModelCheck::BaselineBuild,
    ModelCheck::FullSubmission,
    ModelCheck::SampledSlice,
];

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

/// An id holding `=` or whitespace fails its check. The command line cannot accept it.
#[test]
fn an_id_the_command_line_cannot_name_fails_its_check() {
    let model = register_grid_model::<UnnameableIds>();
    let report = check(&model, &CheckSettings::default());
    assert_eq!(
        failed(&report),
        names(&[ModelCheck::ParamIds, ModelCheck::ActionIds]),
        "{report}"
    );
    let messages: Vec<&str> = report.failures().iter().map(|failure| failure.message()).collect();
    for refused in [
        "'rate=high' holds '='",
        "'spread rate' holds whitespace",
        "'action.delay' starts with",
    ] {
        assert!(messages[0].contains(refused), "{report}");
    }
    assert!(messages[1].contains("'clear=all' holds '='"), "{report}");
    assert!(
        !report.to_string().contains("'count'") && !report.to_string().contains("'spawn@centre'"),
        "{report}"
    );
}

#[test]
fn an_empty_palette_fails_the_palette_check() {
    assert_fails(&register_grid_model::<EmptyPalette>(), &[ModelCheck::Palette]);
}

/// The agent engine prepends `num_agents` and no `grid_width`, and the hint names the one id it prepends.
#[test]
fn a_parameter_repeating_an_engine_id_fails_the_param_ids_check() {
    let model = register_agent_model::<DeclaresNumAgents>();
    let report = check(&model, &CheckSettings::default());
    assert_eq!(failed(&report), names(&[ModelCheck::ParamIds]), "{report}");
    assert_eq!(
        report.failures()[0].message(),
        "Parameter ids 'num_agents', 'grid_width' are declared more than once. The engine prepends 'num_agents', and \
         a model does not declare it itself."
    );
}

/// `DefaultSetup` and every check that builds with `RunSetup::from_parts` reject the default, and each failure
/// names the parameter and its bounds.
#[test]
fn a_default_outside_its_bounds_fails_every_check_through_a_run_setup() {
    let report = check(&register_grid_model::<DefaultOutOfBounds>(), &CheckSettings::default());
    assert_eq!(
        failed(&report),
        names(&[
            ModelCheck::DefaultSetup,
            ModelCheck::ThreadCount,
            ModelCheck::SameSeed,
            ModelCheck::SeedSensitivity,
            ModelCheck::SamplingCadence,
        ]),
        "{report}"
    );
    for failure in report.failures() {
        assert!(
            failure
                .message()
                .contains("parameter 'initial_infected': 500 is outside 1..=100"),
            "{failure}"
        );
    }
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

/// A stat that stops being finite sets a run's status in a sweep, and breaks no contract of the model.
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
fn a_state_that_reports_no_jobs_fails_the_parallel_jobs_check() {
    assert_fails(
        &BuggyState::wrap(entry("sir", None), Bug::ReportsNoJobs),
        &[ModelCheck::ParallelJobs],
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

/// The model's stats are the same at any pool width, and its exported state is not.
#[test]
fn a_state_that_reads_the_pool_width_fails_the_thread_count_check() {
    let report = check(
        &register_grid_model::<PlacesCellByPoolWidth>(),
        &CheckSettings::default(),
    );
    assert_eq!(failed(&report), names(&[ModelCheck::ThreadCount]), "{report}");
    assert!(
        report.failures()[0].message().contains("differ in the exported state"),
        "{report}"
    );
}

/// Two seeds give the same stats and different exported states.
#[test]
fn a_seed_that_moves_the_state_alone_passes_the_seed_sensitivity_check() {
    assert_fails(&register_grid_model::<PlacesCellBySeed>(), &[]);
}

/// Checks the shared accumulator up to [`SHARED_ACCUMULATOR_ATTEMPTS`] times, until `ThreadCount` reports it.
///
/// Note that the model's result depends on the order in which its chunks take a lock, and a pool of seven workers can
/// take the lock in the single thread's order by chance, under a loaded machine most of all. Any one check can then
/// pass. [`a_build_that_reads_the_pool_width_fails_the_thread_count_check`] pins `ThreadCount` deterministically.
#[test]
fn a_shared_accumulator_fails_the_thread_count_check() {
    let model = register_agent_model::<SharedAccumulator>();
    let mut reports = Vec::new();
    for _ in 0..SHARED_ACCUMULATOR_ATTEMPTS {
        let report = check(&model, &CheckSettings::default());
        if let Some(failure) = report
            .failures()
            .iter()
            .find(|failure| failure.check() == ModelCheck::ThreadCount)
        {
            assert!(failure.message().contains("split into 14 jobs"), "{failure}");
            return;
        }
        reports.push(report.to_string());
    }
    panic!(
        "ThreadCount passed the shared accumulator on {SHARED_ACCUMULATOR_ATTEMPTS} attempts:\n{}",
        reports.join("\n")
    );
}

/// Number of attempts that [`a_shared_accumulator_fails_the_thread_count_check`] makes before it fails.
const SHARED_ACCUMULATOR_ATTEMPTS: usize = 5;

/// A device that the watchdog stopped reads zeros from then on, from every state on it. The check runs the single steps
/// first, and they read back what they computed.
#[test]
fn a_full_submission_that_reads_zeros_fails_the_full_submission_check() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let model = BuggyGpuState::wrap(entry("gpu_game_of_life", Some(&ctx)), GpuBug::ZeroesFullSubmissions);
    let report = check(&model, &CheckSettings::default().gpu(ctx));
    assert_eq!(failed(&report), names(&[ModelCheck::FullSubmission]), "{report}");
}

/// A model that does not replay exactly runs one full submission alone, and fails when every stat reads zero.
#[test]
fn an_inexact_full_submission_that_reads_zeros_fails_the_full_submission_check() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let model = BuggyGpuState::wrap(entry("gpu_boids", Some(&ctx)), GpuBug::ZeroesFullSubmissions);
    assert_fails_under(
        &model,
        &CheckSettings::default().gpu(ctx),
        &[ModelCheck::FullSubmission],
    );
}

/// A model with no stats has no value to read zero, and its completed readback and its tick show the steps ran.
#[test]
fn a_model_with_no_stats_passes_the_full_submission_check() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let model = register_gpu_agent_model::<StatlessGpuBoids>();
    assert_fails_under(&model, &CheckSettings::default().gpu(ctx), &[]);
}

#[test]
fn a_state_that_records_no_stats_passes_fails_the_sampled_slice_check() {
    let Some(ctx) = headless_device() else {
        return;
    };
    let model = BuggyGpuState::wrap(entry("gpu_boids", Some(&ctx)), GpuBug::SkipsStatsPasses);
    assert_fails_under(&model, &CheckSettings::default().gpu(ctx), &[ModelCheck::SampledSlice]);
}

/// The checks at the small check values pass, and every check that builds at the defaults fails on a baseline
/// device.
#[test]
fn defaults_past_the_baseline_fail_every_check_at_the_defaults() {
    let model = register_gpu_grid_model::<OversizedGpuSir>();
    let report = check_model_requiring(&model, &CheckSettings::default(), false);
    assert_eq!(failed(&report), names(&[ModelCheck::DefaultsFit]), "{report}");

    let Some(ctx) = baseline_device() else {
        return;
    };
    assert_fails_under(
        &model,
        &CheckSettings::default().gpu(ctx),
        &[
            ModelCheck::DefaultsFit,
            ModelCheck::Actions,
            ModelCheck::BaselineBuild,
            ModelCheck::FullSubmission,
            ModelCheck::SampledSlice,
        ],
    );
}

#[test]
fn a_required_gpu_fails_every_check_a_missing_device_skips() {
    let model = example("gpu_sir");
    let skipped = check_model_requiring(&model, &CheckSettings::default(), false);
    assert!(skipped.passed(), "{skipped}");
    let no_device: Vec<ModelCheck> = skipped
        .skipped()
        .iter()
        .filter(|skipped_check| *skipped_check.reason() == SkipReason::NoDevice)
        .map(|skipped_check| skipped_check.check())
        .collect();
    assert_eq!(no_device, GPU_BUILDING_CHECKS);

    let required = check_model_requiring(&model, &CheckSettings::default(), true);
    assert_eq!(failed(&required), names(&GPU_BUILDING_CHECKS), "{required}");
    assert!(
        required
            .failures()
            .iter()
            .all(|failure| failure.message().starts_with("HENAD_REQUIRE_GPU is set")),
        "{required}"
    );
}

/// The override fails where no device runs the checks, as it fails on a device.
#[test]
fn an_override_of_an_undeclared_parameter_fails_without_a_device() {
    let model = example("gpu_sir");
    let settings = CheckSettings::default().set_text("gpu_sir", "num_boids", "4096");
    for gpu_required in [false, true] {
        let report = check_model_requiring(&model, &settings, gpu_required);
        assert_eq!(failed(&report), names(&GPU_BUILDING_CHECKS), "{report}");
        assert!(
            report
                .failures()
                .iter()
                .all(|failure| failure.message().contains("parameter 'num_boids'")),
            "{report}"
        );
    }
}

/// A grid of 128 columns holds 64 rows per job, and 896 rows split into 14 jobs.
#[test]
fn the_thread_count_check_splits_a_grid_into_twice_the_high_thread_count() {
    let report = check(&entry("game_of_life", None), &CheckSettings::default());
    assert!(report.passed(), "{report}");
    assert_eq!(report.thread_count_jobs(), Some(14));
}

/// A grid of 1024 columns holds 8 rows per job, and 112 rows split into 14 jobs.
#[test]
fn the_thread_count_check_shrinks_a_grid_an_override_widens() {
    let settings = CheckSettings::default().set_text("game_of_life", "grid_width", "1024");
    let report = check(&entry("game_of_life", None), &settings);
    assert!(report.passed(), "{report}");
    assert_eq!(report.thread_count_jobs(), Some(14));
}

#[test]
fn an_override_of_one_job_skips_the_thread_count_check_naming_it() {
    let settings = CheckSettings::default().set_text("boids", "num_agents", "8");
    let report = check(&entry("boids", None), &settings);
    assert_eq!(
        report.skip_reason(ModelCheck::ThreadCount),
        Some(&SkipReason::OneJobAtOverride("num_agents".to_owned()))
    );
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
    assert!(
        report
            .to_string()
            .starts_with("1 model checked, and none failed. The settings name 2 models the set lacks.\n"),
        "{report}"
    );
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
        message.starts_with("1 model checked, and 1 failed.\n")
            && message.contains("Model 'divides_by_param' failed 1 of its checks: SeedSensitivity."),
        "{message}"
    );
}
