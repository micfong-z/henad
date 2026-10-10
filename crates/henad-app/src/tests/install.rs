//! Tests of [`AppState::install_models`] and of what the panels, the Sweep tab and the Results tab make of a model
//! whose code the set has replaced.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use henad_compute::entry::ModelEntry;
use henad_compute::fault::{Fault, STEPPING};
use henad_core::action::{Schedule, Scheduled};
use henad_core::explore::replay::Replay;
use henad_core::explore::spec::SweepSpec;
use henad_core::helpers::choice_param;
use henad_core::params::ParamValue;
use henad_explore::output::details::{ChoiceForm, params_by_id_json};
use henad_explore::output::manifest::{BuildRole, RecordedBuild};

use crate::icons::material_design_icons::MDI_RESTART;
use crate::state::{AppState, OpenAt, PendingRebuild, remap_schedule, remap_values, same_code};
use crate::tests::support::{counter, edited_sir, recoded, set_of, step_past};
use crate::ui::export::metadata::run_details;
use crate::ui::params::INVALID_SEED;
use crate::ui::params::{ParamsNoticeTone, notice_content};
use crate::ui::results::table::replay_warnings;

/// Returns an app over the example models whose adapter, as the app sees it, cannot run compute shaders, or `None`
/// to skip on a machine without a device. SIR is selected at a size of 64 by 64.
fn app_with_small_sir() -> Option<AppState> {
    let mut app = AppState::headless(henad_models::example_models(), false)?;
    app.select_model("sir");
    set_value(&mut app, "grid_width", ParamValue::U32(64));
    set_value(&mut app, "grid_height", ParamValue::U32(64));
    Some(app)
}

/// Sets the selection's value of parameter `id` to `value`.
fn set_value(app: &mut AppState, id: &str, value: ParamValue) {
    let index = app
        .selected_entry()
        .and_then(|entry| entry.param_index(id))
        .expect("the selection declares the parameter");
    app.param_values[index] = value;
}

/// Returns the selection's value of parameter `id`.
fn value(app: &AppState, id: &str) -> ParamValue {
    let index = app
        .selected_entry()
        .and_then(|entry| entry.param_index(id))
        .expect("the selection declares the parameter");
    app.param_values[index].clone()
}

/// Returns the example entry `id` of the app's set.
fn example(app: &AppState, id: &str) -> ModelEntry {
    app.models.get(id).cloned().expect("an example model")
}

#[test]
fn the_panels_describe_the_running_entry() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    app.reset_simulation();
    step_past(&mut app, 0);
    let loaded_values = app.loaded_values.clone();

    app.install_models(&set_of(edited_sir()));
    assert!(app.pending_rebuild.is_some(), "a run past tick 0 asks first");
    app.pending_rebuild = None;

    let loaded = app.loaded_entry().expect("the run keeps going");
    assert_eq!(loaded.source(), sir.source());
    assert_eq!(loaded.name(), "SIR Epidemic");
    assert_eq!(app.selected_entry().map(ModelEntry::name), Some("SIR Edited"));
    assert!(!app.selection_is_loaded());
    assert!(app.code_pending());

    let details: serde_json::Value = serde_json::from_str(&run_details(&app)).expect("the run details are JSON");
    assert_eq!(details["model_name"], "SIR Epidemic");
    assert_eq!(
        details["model_source"],
        serde_json::to_value(RecordedBuild::from(sir.source())).expect("a build serialises")
    );
    assert_eq!(
        details["params"],
        params_by_id_json(sir.param_descriptors(), &loaded_values, ChoiceForm::Name),
        "the parameters of the running code"
    );

    let vaccination = app
        .selected_entry()
        .and_then(|entry| entry.param_index("vaccination_rate"))
        .expect("the edit declares a vaccination rate");
    assert!(
        !app.send_live_param(vaccination, ParamValue::F32(0.5)),
        "an index of the new code reached the running state"
    );
    assert_eq!(app.loaded_values, loaded_values);

    app.select_model("boids");
    app.select_model("sir");
    assert_eq!(
        app.param_values,
        remap_values(
            sir.param_descriptors(),
            &loaded_values,
            edited_sir().param_descriptors()
        )
        .values,
        "the running values, carried to the new code"
    );

    app.report_fault(Fault::refused(STEPPING, "a test fault"));
    assert_eq!(
        app.fault.as_ref().and_then(|shown| shown.model.as_deref()),
        Some("SIR Epidemic"),
        "a fault of the running code names its model"
    );
}

#[test]
fn an_opened_run_is_modified_after_its_source_changes() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    let replay = Replay {
        model: "sir".to_owned(),
        params: app.param_values.clone(),
        seed: 3,
        schedule: Schedule::default(),
        ticks: 10,
        label: "Sweep run 0: config 0, replicate 0".to_owned(),
    };
    app.open_run(replay, OpenAt::Start).expect("the run opens");
    app.wait_for_build();
    let run = app.opened_run.as_ref().expect("an opened run");
    assert_eq!(run.entry.source(), sir.source());
    assert!(!run.modified);

    let builds = counter();
    app.install_models(&set_of(recoded(&sir, "edited sir", &builds)));
    app.wait_for_build();
    assert_eq!(builds.load(Ordering::SeqCst), 1, "the run at tick 0 was rebuilt");
    assert!(
        app.opened_run.as_ref().is_some_and(|run| run.modified),
        "the run was built from other code"
    );

    app.install_models(&set_of(sir.clone()));
    assert_eq!(app.loaded_entry().map(ModelEntry::source), Some(sir.source()));
    assert!(
        app.opened_run.as_ref().is_some_and(|run| !run.modified),
        "the run's own code and values build it again"
    );
}

#[test]
fn install_keeps_values_when_the_schema_matches() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    set_value(&mut app, "infection_rate", ParamValue::F32(0.45));
    set_value(&mut app, "initial_infected_pct", ParamValue::F32(0.02));
    let initial = sir.param_index("initial_infected_pct").expect("SIR declares it");
    app.pending_reload[initial] = true;
    app.seed = Some(5);
    app.seed_text = "5".to_owned();
    app.schedule = Schedule::from_entries(vec![Scheduled {
        index: 0,
        id: "seed_outbreak".to_owned(),
        tick: 20,
    }]);
    let (values, marks, schedule) = (
        app.param_values.clone(),
        app.pending_reload.clone(),
        app.schedule.clone(),
    );

    let builds = counter();
    app.install_models(&set_of(recoded(&sir, "same declarations", &builds)));
    assert_eq!(app.param_values, values);
    assert_eq!(app.pending_reload, marks);
    assert_eq!(app.schedule, schedule);
    assert_eq!((app.seed, app.seed_text.as_str()), (Some(5), "5"));
    assert_eq!(
        builds.load(Ordering::SeqCst),
        0,
        "a model that is not loaded stays unbuilt"
    );

    app.reset_simulation();
    app.wait_for_build();
    assert_eq!(builds.load(Ordering::SeqCst), 1, "Build builds the installed entry");
    let rebuilds = counter();
    app.install_models(&set_of(recoded(&sir, "second edit", &rebuilds)));
    app.wait_for_build();
    assert_eq!(
        rebuilds.load(Ordering::SeqCst),
        1,
        "the run at tick 0 was rebuilt at once"
    );
    assert_eq!(app.pending_rebuild, None);
    assert_eq!(app.loaded_values, values);
    assert_eq!(app.loaded_seed, Some(5));
    assert_eq!(app.loaded_schedule, schedule);
    assert!(app.selection_is_loaded());
}

#[test]
fn install_remaps_values_by_id_when_the_schema_changes() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    set_value(&mut app, "infection_rate", ParamValue::F32(0.6));
    set_value(&mut app, "initial_infected_pct", ParamValue::F32(0.02));
    for id in ["grid_width", "initial_infected_pct"] {
        app.pending_reload[sir.param_index(id).expect("SIR declares it")] = true;
    }
    app.seed = Some(5);
    app.schedule = Schedule::from_entries(vec![Scheduled {
        index: 0,
        id: "seed_outbreak".to_owned(),
        tick: 20,
    }]);

    app.install_models(&set_of(edited_sir()));
    let edited = app.selected_entry().cloned().expect("the edit is selected");
    assert_eq!(edited.name(), "SIR Edited");
    assert_eq!(app.param_values.len(), edited.param_descriptors().len());
    assert_eq!(value(&app, "grid_width"), ParamValue::U32(64));
    assert_eq!(
        value(&app, "infection_rate"),
        ParamValue::F32(0.2),
        "a value the new bounds reject takes the new default"
    );
    assert_eq!(value(&app, "initial_infected_pct"), ParamValue::F32(0.02));
    assert_eq!(value(&app, "vaccination_rate"), ParamValue::F32(0.0), "a new parameter");
    let marked: Vec<&str> = edited
        .param_descriptors()
        .iter()
        .zip(&app.pending_reload)
        .filter(|(_, marked)| **marked)
        .map(|(descriptor, _)| descriptor.id())
        .collect();
    assert_eq!(marked, ["grid_width", "initial_infected_pct"]);
    assert_eq!(
        app.schedule.entries(),
        [Scheduled {
            index: 1,
            id: "seed_outbreak".to_owned(),
            tick: 20,
        }],
        "the outbreak moved behind the new action"
    );
    assert_eq!(app.seed, Some(5));
    assert!(
        app.build_setup().is_some_and(|setup| setup.is_ok()),
        "Build is available"
    );

    app.schedule = Schedule::from_entries(vec![
        Scheduled {
            index: 0,
            id: "vaccinate".to_owned(),
            tick: 5,
        },
        Scheduled {
            index: 1,
            id: "seed_outbreak".to_owned(),
            tick: 20,
        },
    ]);
    app.install_models(&set_of(sir.clone()));
    assert_eq!(
        app.schedule.entries(),
        [Scheduled {
            index: 0,
            id: "seed_outbreak".to_owned(),
            tick: 20,
        }],
        "an action the code no longer declares is dropped"
    );
    assert_eq!(value(&app, "infection_rate"), ParamValue::F32(0.2));
    assert_eq!(
        value(&app, "recovery_rate"),
        ParamValue::F32(0.05),
        "back at its default"
    );
}

#[test]
fn a_choice_is_carried_by_the_name_of_its_option() {
    let previous = [choice_param("network", "Network", &["ring", "lattice", "random"], 0)];
    let current = [choice_param("network", "Network", &["random", "ring"], 1)];
    let carried = |index| remap_values(&previous, &[ParamValue::Choice(index)], &current);
    assert_eq!(carried(2).values, [ParamValue::Choice(0)]);
    assert_eq!(carried(0).values, [ParamValue::Choice(1)]);
    assert_eq!(
        carried(1).values,
        [ParamValue::Choice(1)],
        "an option dropped takes the default"
    );
    assert_eq!(carried(1).defaulted, 1);
    assert_eq!(carried(2).defaulted, 0);

    let schedule = Schedule::from_entries(vec![Scheduled {
        index: 0,
        id: "gone".to_owned(),
        tick: 1,
    }]);
    assert_eq!(remap_schedule(&schedule, &[]).dropped, 1);
}

#[test]
fn results_flag_runs_of_replaced_code() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    let mut spec = SweepSpec::new("sir");
    spec.run.steps = 3;
    let plan = Arc::new(spec.plan(&sir.schema()).expect("a valid spec"));
    app.results.begin_sweep(plan, &sir, None);
    let warnings = |app: &AppState| replay_warnings(app.results.store().expect("the sweep's results"));
    assert_eq!(warnings(&app), []);

    app.install_models(&set_of(recoded(&sir, "edited sir", &counter())));
    let store = app.results.store().expect("the sweep's results");
    assert_eq!(store.changed_builds, [BuildRole::Model]);
    assert!(store.schema_matches);
    assert_eq!(
        warnings(&app),
        [(
            "Model build changed since this sweep. The replay might differ.".to_owned(),
            None
        )]
    );

    app.install_models(&set_of(edited_sir()));
    assert_eq!(
        warnings(&app),
        [(
            "Model parameters, stats or actions changed since this sweep. The replay might differ.".to_owned(),
            None
        )]
    );

    app.install_models(&set_of(sir.clone()));
    assert_eq!(warnings(&app), [], "the sweep's own code again");

    app.install_models(&set_of(recoded(&example(&app, "boids"), "edited boids", &counter())));
    assert_eq!(warnings(&app), [], "another model's code changed");
}

#[test]
fn install_works_for_every_model_type() {
    let Some(mut app) = AppState::headless(henad_models::example_models(), true) else {
        return;
    };
    for id in ["sir", "boids", "virus_network", "gpu_game_of_life", "gpu_boids"] {
        let entry = example(&app, id);
        app.select_model(id);
        app.reset_simulation();
        app.wait_for_build();
        assert!(app.sim_thread.is_some(), "{id} built");

        let builds = counter();
        let first = recoded(&entry, &format!("{id}, first edit"), &builds);
        app.install_models(&set_of(first.clone()));
        app.wait_for_build();
        assert_eq!(builds.load(Ordering::SeqCst), 1, "{id} was rebuilt at tick 0");
        assert_eq!(app.loaded_entry().map(ModelEntry::source), Some(first.source()), "{id}");
        assert!(app.sim_thread.is_some() && app.selection_is_loaded(), "{id}");

        step_past(&mut app, 0);
        let rebuilds = counter();
        let second = recoded(&entry, &format!("{id}, second edit"), &rebuilds);
        app.install_models(&set_of(second.clone()));
        assert_eq!(
            app.pending_rebuild,
            Some(PendingRebuild {
                model: id.to_owned(),
                defaulted_params: 0,
                dropped_actions: 0,
            }),
            "{id}"
        );
        assert_eq!(rebuilds.load(Ordering::SeqCst), 0, "{id} waits on the modal");
        app.rebuild_loaded();
        app.wait_for_build();
        assert_eq!(rebuilds.load(Ordering::SeqCst), 1, "{id}");
        assert_eq!(
            app.loaded_entry().map(ModelEntry::source),
            Some(second.source()),
            "{id}"
        );
        assert!(app.snapshot.is_none(), "{id} starts again from tick 0");
        assert_eq!(app.pending_rebuild, None);
    }
}

#[test]
fn a_loaded_run_past_tick_zero_asks_before_rebuilding() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    set_value(&mut app, "infection_rate", ParamValue::F32(0.6));
    app.reset_simulation();
    step_past(&mut app, 0);
    let tick = app.snapshot.as_ref().map(|snapshot| snapshot.tick);

    app.install_models(&set_of(edited_sir()));
    assert_eq!(
        app.pending_rebuild,
        Some(PendingRebuild {
            model: "sir".to_owned(),
            defaulted_params: 2,
            dropped_actions: 0,
        }),
        "the infection rate out of the new bounds and the new vaccination rate"
    );
    assert_eq!(app.loaded_entry().map(ModelEntry::source), Some(sir.source()));
    assert_eq!(app.snapshot.as_ref().map(|snapshot| snapshot.tick), tick);

    // Cancel, then an edit of the kernel alone. The modal still counts against the run, whose values the panel lost
    // to the first edit.
    app.pending_rebuild = None;
    let kernel_edit = recoded(&edited_sir(), "kernel edit", &counter());
    app.install_models(&set_of(kernel_edit.clone()));
    assert_eq!(
        app.pending_rebuild.as_ref().map(|pending| pending.defaulted_params),
        Some(2),
        "the run's values that the new code does not take"
    );

    app.rebuild_loaded();
    app.wait_for_build();
    assert_eq!(app.pending_rebuild, None);
    assert_eq!(app.loaded_entry().map(ModelEntry::name), Some("SIR Edited"));
    assert_eq!(app.loaded_entry().map(ModelEntry::source), Some(kernel_edit.source()));
    assert!(app.snapshot.is_none(), "the run starts again from tick 0");
    assert!(app.selection_is_loaded());
    assert_eq!(app.loaded_values, app.param_values);

    // A model loaded while another one is selected rebuilds from the values it runs with.
    step_past(&mut app, 0);
    let running = app.loaded_values.clone();
    app.select_model("boids");
    let boids_values = app.param_values.clone();
    let builds = counter();
    let restored = recoded(&sir, "edited sir", &builds);
    app.install_models(&set_of(restored.clone()));
    let expected = remap_values(edited_sir().param_descriptors(), &running, sir.param_descriptors());
    assert_eq!(
        app.pending_rebuild,
        Some(PendingRebuild {
            model: "sir".to_owned(),
            defaulted_params: expected.defaulted,
            dropped_actions: 0,
        })
    );
    app.rebuild_loaded();
    app.wait_for_build();
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(app.loaded_entry().map(ModelEntry::source), Some(restored.source()));
    assert_eq!(app.loaded_values, expected.values);
    assert_eq!(app.selected_model.as_deref(), Some("boids"), "the selection stays");
    assert_eq!(app.param_values, boids_values);
}

#[test]
fn cancel_keeps_the_old_run_and_flags_the_code_change() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    app.reset_simulation();
    step_past(&mut app, 0);
    let builds = counter();
    app.install_models(&set_of(recoded(&sir, "edited sir", &builds)));
    assert!(app.pending_rebuild.is_some());

    // Cancel in the Rebuild model modal.
    app.pending_rebuild = None;
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    assert!(app.sim_thread.is_some(), "the old run keeps going");
    assert_eq!(app.loaded_entry().map(ModelEntry::source), Some(sir.source()));
    assert!(app.code_pending() && !app.selection_is_loaded());
    let notice = notice_content(&app, 0, &[]).expect("the panel shows a notice");
    assert_eq!(notice.tone, ParamsNoticeTone::Warn);
    assert_eq!(notice.title, "Reload needed");
    assert_eq!(
        notice.detail,
        format!("Model code changed. Press {MDI_RESTART}\u{a0}Build to apply.")
    );
    let tick = app.snapshot.as_ref().map_or(0, |snapshot| snapshot.tick);
    step_past(&mut app, tick);
    assert_eq!(
        app.loaded_entry().map(ModelEntry::source),
        Some(sir.source()),
        "the old run steps on"
    );

    app.reset_simulation();
    app.wait_for_build();
    assert_eq!(builds.load(Ordering::SeqCst), 1, "Build builds the new code");
    assert!(app.selection_is_loaded() && !app.code_pending());
    assert_eq!(notice_content(&app, 0, &[]), None);
}

#[test]
fn an_install_during_a_run_to_asks_first() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    app.reset_simulation();
    app.run_to(50);
    assert!(app.snapshot.is_none(), "no snapshot has arrived yet");

    let builds = counter();
    app.install_models(&set_of(recoded(&sir, "edited sir", &builds)));
    assert_eq!(builds.load(Ordering::SeqCst), 0, "the run to tick 50 was kept");
    assert!(app.pending_rebuild.is_some());
    assert_eq!(app.run_to_target, Some(50));
}

#[test]
fn a_rebuild_the_new_code_refuses_leaves_the_old_run_alone() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    app.reset_simulation();
    app.wait_for_build();
    app.seed_text = "not a seed".to_owned();

    let builds = counter();
    app.install_models(&set_of(recoded(&sir, "edited sir", &builds)));
    assert_eq!(
        builds.load(Ordering::SeqCst),
        0,
        "a run at tick 0 Build refuses is not rebuilt"
    );
    assert!(app.pending_rebuild.is_some(), "the modal shows the reason instead");
    assert_eq!(app.rebuild_refusal().as_deref(), Some(INVALID_SEED));

    app.rebuild_loaded();
    assert_eq!(builds.load(Ordering::SeqCst), 0);
    assert!(app.sim_thread.is_some(), "the old run is left alone");
    assert_eq!(app.loaded_entry().map(ModelEntry::source), Some(sir.source()));

    app.seed_text = String::new();
    assert_eq!(app.rebuild_refusal(), None);
    app.rebuild_loaded();
    app.wait_for_build();
    assert_eq!(builds.load(Ordering::SeqCst), 1);
    assert_eq!(app.pending_rebuild, None);
}

#[test]
fn equal_sources_with_other_declarations_are_other_code() {
    let Some(mut app) = app_with_small_sir() else {
        return;
    };
    let sir = example(&app, "sir");
    // Two entries of one source that declare different parameters.
    let builds = counter();
    let first = recoded(&sir, "one source", &builds);
    let second = recoded(&edited_sir(), "one source", &builds);
    assert_eq!(first.source(), second.source());
    assert!(!same_code(&first, &second), "the declarations differ");

    app.install_models(&set_of(first));
    app.reset_simulation();
    step_past(&mut app, 0);
    assert!(app.selection_is_loaded());

    app.install_models(&set_of(second));
    assert!(app.pending_rebuild.is_some(), "a run past tick 0 asks first");
    assert!(!app.selection_is_loaded() && app.code_pending());
    let vaccination = app
        .selected_entry()
        .and_then(|entry| entry.param_index("vaccination_rate"))
        .expect("the edit declares a vaccination rate");
    assert!(
        !app.send_live_param(vaccination, ParamValue::F32(0.5)),
        "an index of the new declarations reached the running state"
    );
}
