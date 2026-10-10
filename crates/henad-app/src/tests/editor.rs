//! Tests of the Editor tab: a grid rule's model file applied to the running app, the problems of a text that is
//! refused, and a fault of the applied code.

use std::sync::Arc;

use henad_compute::entry::ModelState;
use henad_compute::fault::{BUILDING, Fault, FaultKind};
use henad_compute::gpu::GpuContext;
use henad_compute::gpu::runtime_grid::{GridRule, LIFE_STEP};
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::params::ParamValue;
use henad_explore::model_file::ModelFile;

use crate::state::AppState;
use crate::tests::support::{set_of, step_past};
use crate::ui::dock::Tab;
use crate::ui::editor::code_view::cursor_index;
use crate::ui::editor::{
    ApplyOutcome, NO_COMPUTE, apply, edit_model, editable_text, life_starter, move_cursor_to, new_grid_rule,
    receive_open,
};
use crate::ui::files::{DialogFile, OpenResult};

/// Line of the Life starter's step that a rule edit extends.
const BIRTH_LINE: &str = "    if (alive == 3u || (cell == ALIVE && alive == 2u)) {";

/// Returns an app over the example models on a headless device that runs compute shaders, with the Life starter in
/// the Editor, or `None` to skip on a machine without a device.
fn app_with_starter() -> Option<AppState> {
    let mut app = AppState::headless(henad_models::example_models(), true)?;
    new_grid_rule(&mut app);
    Some(app)
}

/// Returns the scalar value of every stat of the latest snapshot.
fn stats(app: &AppState) -> Vec<f64> {
    app.snapshot
        .as_ref()
        .expect("a snapshot arrived")
        .stats
        .iter()
        .map(|entry| entry.value().scalar())
        .collect()
}

/// Returns `text` with `line` put after the line `after`, and the number of the new line, counted from 1.
fn insert_line(text: &str, after: &str, line: &str) -> (String, u32) {
    let index = text
        .lines()
        .position(|held| held == after)
        .expect("the text holds the line");
    let edited = text.replacen(&format!("{after}\n"), &format!("{after}\n{line}\n"), 1);
    (edited, u32::try_from(index + 2).expect("a short text"))
}

/// Returns the problems of the last Apply, which refused the text.
fn refused(app: &AppState) -> &[CodeDiagnostic] {
    match &app.editor.outcome {
        Some(ApplyOutcome::Refused(problems)) => problems,
        other => panic!("the text was not refused: {other:?}"),
    }
}

#[test]
fn new_grid_rule_starts_from_the_life_starter() {
    let Some(mut app) = AppState::headless(henad_models::example_models(), false) else {
        return;
    };
    app.editor.text = "an earlier text".to_owned();
    new_grid_rule(&mut app);

    let starter = ModelFile::from_rule(GridRule::life()).expect("the starter is a model file");
    assert_eq!(
        app.editor.text,
        starter.to_toml(),
        "the starter is the rule's canonical text"
    );
    assert_eq!(app.editor.text, life_starter());
    let read = ModelFile::parse(&app.editor.text).expect("the starter reads back");
    assert_eq!(read.rule().id, "life_rule");
    assert_eq!(read.rule().step, LIFE_STEP);
    assert_eq!(app.editor.outcome, None);
    assert_eq!(app.editor.file_name, None);
    assert_eq!(app.focus_request, Some(Tab::Editor));

    apply(&mut app);
    assert_eq!(
        refused(&app)
            .iter()
            .map(|problem| problem.message.as_str())
            .collect::<Vec<_>>(),
        [NO_COMPUTE],
        "a device without compute runs no rule"
    );
    assert!(app.models.get("life_rule").is_none(), "nothing was installed");
}

#[test]
fn applying_an_edit_installs_and_rebuilds_from_tick_zero() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    apply(&mut app);
    assert!(app.is_building(), "the rule builds on the build thread");
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Applied),
        "the outcome reads as applied from the start of the build"
    );
    assert_eq!(app.selected_model.as_deref(), Some("life_rule"));
    assert_eq!(app.loaded_id(), Some("life_rule"), "nothing ran, so the rule was built");
    app.wait_for_build();
    assert!(app.sim_thread.is_some() && app.fault.is_none(), "the build failed");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    let life = app.loaded_entry().expect("the rule runs").source().clone();
    assert_eq!(
        life.code().map(henad_core::provenance::ModelCode::text),
        Some(app.editor.text.as_str()),
        "the starter is canonical, and the code is its text"
    );
    step_past(&mut app, 0);
    let life_stats = stats(&app);

    // Birth on 3 or 6, as in HighLife.
    let edited = app.editor.text.replace(
        BIRTH_LINE,
        "    if (alive == 3u || (cell == DEAD && alive == 6u) || (cell == ALIVE && alive == 2u)) {",
    );
    assert_ne!(edited, app.editor.text);
    app.editor.text = edited;
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::AwaitingRebuild));
    assert!(
        app.pending_rebuild.is_some(),
        "a run past tick 0 waits on the Rebuild model dialog"
    );
    assert_eq!(
        app.loaded_entry().map(|entry| entry.source()),
        Some(&life),
        "the run keeps its code until Rebuild"
    );
    assert!(app.code_pending());

    app.rebuild_loaded();
    assert!(app.is_building(), "Rebuild builds on the build thread");
    let highlife = app.loaded_entry().expect("the rule was rebuilt").source().clone();
    assert_ne!(highlife, life);
    assert!(
        app.snapshot.as_ref().is_none_or(|snapshot| snapshot.tick == 0),
        "the rebuild starts at tick 0"
    );
    app.editor
        .settle_outcome(Some(&highlife), app.pending_rebuild.is_some());
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Applied),
        "the rule runs after Rebuild"
    );
    app.wait_for_build();
    assert!(app.sim_thread.is_some() && app.fault.is_none(), "the rebuild failed");
    step_past(&mut app, 0);
    assert_ne!(
        stats(&app),
        life_stats,
        "birth on 6 changes the first tick from the same seed"
    );

    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Unchanged));
    assert!(app.pending_rebuild.is_none());
    assert!(
        app.snapshot.as_ref().is_some_and(|snapshot| snapshot.tick > 0),
        "the run kept going"
    );
}

#[test]
fn applying_beside_another_run_past_tick_zero_waits_for_build() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    app.select_model("sir");
    for (index, size) in [(0, 64), (1, 64)] {
        app.param_values[index] = ParamValue::U32(size);
    }
    app.reset_simulation();
    step_past(&mut app, 0);

    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Installed));
    assert_eq!(app.loaded_id(), Some("sir"), "the other model's run keeps going");
    assert_eq!(app.selected_model.as_deref(), Some("life_rule"));
    assert!(app.pending_rebuild.is_none(), "the dialog belongs to a run of the rule");

    app.reset_simulation();
    assert_eq!(app.loaded_id(), Some("life_rule"));
    let loaded = app.loaded_entry().map(|entry| entry.source().clone());
    app.editor
        .settle_outcome(loaded.as_ref(), app.pending_rebuild.is_some());
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert!(app.sim_thread.is_some() && app.fault.is_none(), "the build failed");
}

#[test]
fn applying_again_after_cancel_asks_to_rebuild_again() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    apply(&mut app);
    step_past(&mut app, 0);
    let life = app.loaded_entry().expect("the rule runs").source().clone();
    app.editor.text = app.editor.text.replace(
        BIRTH_LINE,
        "    if (alive == 3u || (cell == DEAD && alive == 6u) || (cell == ALIVE && alive == 2u)) {",
    );
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::AwaitingRebuild));

    // Cancel in the Rebuild model dialog.
    app.pending_rebuild = None;
    app.editor.settle_outcome(Some(&life), app.pending_rebuild.is_some());
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Installed),
        "a canceled dialog leaves the rule waiting for Build"
    );

    apply(&mut app);
    assert!(
        app.pending_rebuild.is_some(),
        "Apply of the same text did not open the dialog again"
    );
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::AwaitingRebuild));
    assert_eq!(
        app.loaded_entry().map(|entry| entry.source()),
        Some(&life),
        "the run keeps its code until Rebuild"
    );
}

#[test]
fn applying_during_another_build_replaces_it_and_cancel_waits_for_build() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    app.select_model("sir");
    for index in [0, 1] {
        app.param_values[index] = ParamValue::U32(64);
    }
    app.reset_simulation();
    assert!(app.is_building(), "SIR builds on the build thread");

    apply(&mut app);
    assert!(app.is_building(), "the rule builds in place of SIR");
    assert_eq!(app.loaded_id(), Some("life_rule"), "SIR's build was not abandoned");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert!(app.sim_thread.is_some() && app.fault.is_none(), "the build failed");
    assert_eq!(app.loaded_id(), Some("life_rule"));

    // An edit at tick 0 builds at once, and Cancel stops that build.
    app.editor.text = app.editor.text.replace(
        BIRTH_LINE,
        "    if (alive == 3u || (cell == DEAD && alive == 6u) || (cell == ALIVE && alive == 2u)) {",
    );
    apply(&mut app);
    assert!(app.is_building(), "an edit at tick 0 builds at once");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.offload_simulation();
    assert!(
        !app.is_building() && app.sim_thread.is_none() && app.loaded_id().is_none(),
        "Cancel left a build or a run"
    );
    app.editor.settle_outcome(None, app.pending_rebuild.is_some());
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Installed),
        "the rule waits for Build after Cancel"
    );

    app.reset_simulation();
    let loaded = app.loaded_entry().map(|entry| entry.source().clone());
    app.editor
        .settle_outcome(loaded.as_ref(), app.pending_rebuild.is_some());
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied), "Build runs the rule");
    app.wait_for_build();
    assert!(app.sim_thread.is_some() && app.fault.is_none(), "the build failed");
}

#[test]
fn a_code_edit_that_fails_validation_keeps_the_running_model() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    apply(&mut app);
    step_past(&mut app, 0);
    let running = app.loaded_entry().expect("the rule runs").source().clone();
    let starter = app.editor.text.clone();
    let keeps_running = |app: &AppState| {
        assert_eq!(app.loaded_entry().map(|entry| entry.source()), Some(&running));
        assert_eq!(app.models.get("life_rule").map(|entry| entry.source()), Some(&running));
        assert!(app.sim_thread.is_some(), "the run was torn down");
        assert!(!app.is_building(), "a refused text started a build");
        assert!(app.pending_rebuild.is_none());
        assert!(app.fault.is_none());
    };

    let (looped, loop_line) = insert_line(&starter, "    let alive = moore_count(ALIVE);", "    loop {}");
    app.editor.text = looped;
    apply(&mut app);
    let problem = refused(&app).first().expect("a problem");
    assert_eq!(problem.line, Some(loop_line), "{problem}");
    assert_eq!(problem.column, Some(5), "{problem}");
    keeps_running(&app);

    let (unclosed, _) = insert_line(&starter, "    return DEAD;", "    let x = (1u;");
    app.editor.text = unclosed;
    apply(&mut app);
    assert!(
        refused(&app).iter().all(|problem| problem.line.is_some()),
        "every syntax problem has its line: {:?}",
        refused(&app)
    );
    keeps_running(&app);

    app.editor.text = starter.replace("width = 256", "width = ");
    apply(&mut app);
    let width_line = starter
        .lines()
        .position(|line| line == "width = 256")
        .map(|index| u32::try_from(index + 1).expect("a short text"));
    assert_eq!(refused(&app).first().and_then(|problem| problem.line), width_line);
    keeps_running(&app);

    step_past(&mut app, 1);
    assert!(
        app.snapshot.as_ref().is_some_and(|snapshot| snapshot.tick > 1),
        "the old run steps on"
    );
}

#[test]
fn a_fault_in_the_apply_window_is_shown_in_the_editor() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    let report = |app: &mut AppState, message: &str| {
        // A fault the device reports after the build reaches the sink, as on the web, and `logic` reports it.
        app.render_ctx
            .faults
            .set_once(Fault::refused("running on the GPU", message));
        let fault = app.render_ctx.faults.take().expect("the fault is in the sink");
        app.report_fault(fault);
    };

    // The build thread returns the build's own faults. A fault in a sink while the rule builds comes from a detached
    // run or from rendering, and goes to the fault dialog.
    apply(&mut app);
    assert!(app.is_building(), "the rule builds on the build thread");
    report(&mut app, "a fault of a detached run");
    assert!(app.fault.is_some(), "a fault during the build went to the Editor");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    assert!(!app.is_building(), "the fault left the build running");
    app.fault = None;

    apply(&mut app);
    app.wait_for_build();
    assert_eq!(app.loaded_id(), Some("life_rule"));
    app.focus_request = None;
    // A lost device is not the code's fault, and no edit of the text fixes it.
    app.report_fault(Fault::device_lost("running on the GPU"));
    assert!(app.fault.is_some(), "a lost device went to the Editor");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    assert_eq!(app.focus_request, None);
    app.fault = None;

    apply(&mut app);
    app.wait_for_build();
    assert_eq!(app.loaded_id(), Some("life_rule"));
    report(&mut app, "the device refused the step");
    assert!(app.fault.is_none(), "the fault dialog opened for the edit's fault");
    match &app.editor.outcome {
        Some(ApplyOutcome::Faulted(problems)) => {
            assert_eq!(problems.len(), 1);
            assert!(
                problems[0].message.contains("the device refused the step"),
                "{problems:?}"
            );
        }
        other => panic!("the fault is not the edit's: {other:?}"),
    }
    assert_eq!(app.focus_request, Some(Tab::Editor));
    assert_eq!(app.loaded_id(), None, "the faulted run is offloaded");

    apply(&mut app);
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Applied),
        "Apply builds the rule again"
    );
    step_past(&mut app, 0);
    report(&mut app, "a fault past tick 0");
    assert!(app.fault.is_some(), "a fault past tick 0 is the run's");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.fault = None;

    app.select_model("sir");
    for index in [0, 1] {
        app.param_values[index] = ParamValue::U32(64);
    }
    app.reset_simulation();
    app.wait_for_build();
    report(&mut app, "a fault of another model");
    assert!(app.fault.is_some(), "another model's fault went to the Editor");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
}

#[test]
fn a_build_fault_of_the_applied_code_reaches_the_editor_from_the_build_thread() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    apply(&mut app);
    app.wait_for_build();
    let applied = app.loaded_entry().expect("the rule runs").source().clone();
    // The applied rule under a factory that first creates a buffer past the device's limit. Its build fails on the
    // build thread, as the build of a rule the device refuses does.
    let refused = app
        .models
        .get("life_rule")
        .cloned()
        .expect("the rule was installed")
        .wrap_factory(|inner| {
            Arc::new(
                move |params: &[ParamValue],
                      seed: Option<u64>,
                      gpu: Option<&GpuContext>|
                      -> Result<ModelState, Fault> {
                    if let Some(ctx) = gpu {
                        drop(ctx.device.create_buffer(&wgpu::BufferDescriptor {
                            label: Some("henad_app_test_oversized"),
                            size: ctx.device.limits().max_buffer_size + 4096,
                            usage: wgpu::BufferUsages::STORAGE,
                            mapped_at_creation: false,
                        }));
                    }
                    inner(params, seed, gpu)
                },
            )
        });
    app.install_models(&set_of(refused));
    assert!(
        app.pending_rebuild.is_none() && app.sim_thread.is_some(),
        "an entry of the same code changed the run"
    );

    // Build at tick 0 builds the applied code again, and its fault arrives once the build thread returns.
    app.focus_request = None;
    app.reset_simulation();
    assert!(app.is_building(), "the rule builds on the build thread");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert!(app.fault.is_none(), "the build's fault opened the fault dialog");
    match &app.editor.outcome {
        Some(ApplyOutcome::Faulted(problems)) => {
            assert_eq!(problems.len(), 1, "{problems:?}");
            assert!(!problems[0].message.is_empty());
        }
        other => panic!("the build's fault is not the edit's: {other:?}"),
    }
    assert_eq!(app.focus_request, Some(Tab::Editor));
    assert!(
        app.sim_thread.is_none() && app.loaded_id().is_none(),
        "the faulted build left a run"
    );
    assert!(
        app.render_ctx.faults.take().is_none(),
        "the error escaped the build's scopes to the device's handler"
    );

    // Once the Editor holds another text, a fault of the same code is the run's.
    new_grid_rule(&mut app);
    assert!(!app.editor.is_applied(&applied));
    app.focus_request = None;
    app.reset_simulation();
    app.wait_for_build();
    let shown = app.fault.as_ref().expect("the fault dialog opened");
    assert_eq!(shown.fault.during, BUILDING);
    assert!(matches!(shown.fault.kind, FaultKind::Device(_)), "{:?}", shown.fault);
    assert_eq!(app.editor.outcome, None);
    assert_eq!(app.focus_request, None);
}

#[test]
fn a_diagnostic_moves_the_cursor_to_its_line() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    let (looped, loop_line) = insert_line(&app.editor.text, "    let alive = moore_count(ALIVE);", "    loop {}");
    app.editor.text = looped;
    apply(&mut app);
    let problem = refused(&app).first().expect("a problem").clone();
    assert_eq!(problem.line, Some(loop_line));

    let ctx = egui::Context::default();
    move_cursor_to(
        &ctx,
        &mut app.editor,
        problem.line.expect("a line"),
        problem.column.expect("a column"),
    );
    let byte = app.editor.text.find("loop {}").expect("the text holds the loop");
    assert_eq!(
        cursor_index(&ctx),
        Some(app.editor.text[..byte].chars().count()),
        "the cursor is not at the loop"
    );
    assert!(
        ctx.memory(|memory| memory.has_focus(crate::ui::editor::code_view::code_id())),
        "the code field has no focus"
    );
}

#[test]
fn edit_model_shows_the_code_the_rule_runs() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    apply(&mut app);
    app.wait_for_build();
    let applied = app.editor.text.clone();
    assert_eq!(app.selected_entry().and_then(editable_text), Some(applied.as_str()));
    assert_eq!(
        app.models.get("sir").and_then(editable_text),
        None,
        "a compiled model has no model file"
    );

    app.editor.text.push_str("# an edit since Apply\n");
    edit_model(&mut app);
    assert!(
        app.editor.text.ends_with("# an edit since Apply\n"),
        "an edit of the applied code was lost"
    );

    receive_open(
        &mut app,
        OpenResult::Files(vec![DialogFile {
            name: "other.toml".to_owned(),
            bytes: b"format = \"henad-model\"\n".to_vec(),
            path: None,
        }]),
    );
    assert_eq!(app.editor.text, "format = \"henad-model\"\n");
    assert_eq!(app.editor.file_name.as_deref(), Some("other.toml"));
    assert_eq!(app.editor.file_status.as_deref(), Some("Opened other.toml"));
    assert_eq!(app.editor.outcome, None);

    edit_model(&mut app);
    assert_eq!(
        app.editor.text, applied,
        "Edit model loads the code the selected rule runs"
    );

    receive_open(
        &mut app,
        OpenResult::Files(vec![DialogFile {
            name: "binary.toml".to_owned(),
            bytes: vec![0xFF, 0xFE],
            path: None,
        }]),
    );
    assert_eq!(
        app.editor.text, applied,
        "a file that is not text replaced the Editor's"
    );
    assert_eq!(
        app.editor.file_status.as_deref(),
        Some("Open failed: binary.toml is not UTF-8 text")
    );
}
