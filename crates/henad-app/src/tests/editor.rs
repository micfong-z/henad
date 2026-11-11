//! Tests of the Editor tab: a grid rule's model file applied to the running app, the problems of a text that is
//! refused, a fault of the applied code, and the line of an Apply beside another model's run.

use henad_compute::entry::ModelEntry;
use henad_compute::fault::{BUILDING, Fault, FaultKind};
use henad_compute::gpu::runtime_grid::{GridRule, LIFE_STEP};
use henad_core::diagnostic::CodeDiagnostic;
use henad_core::provenance::ModelSource;
use henad_explore::model_file::ModelFile;
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::sentence::PREVIEW_WAITS as SHEET_PREVIEW_WAITS;

use crate::state::AppState;
use crate::tests::panes::apply_sheet_edit;
use crate::tests::support::{
    accessible_texts, assert_built, edited, headless_app, set_grid_side, set_of, settle_preview, starter_text,
    step_past, with_oversized_buffer,
};
use crate::ui::dock::Tab;
use crate::ui::editor::code_view::{cursor_index, move_cursor};
use crate::ui::editor::preview::PreviewState;
use crate::ui::editor::{
    ApplyOutcome, EditableModel, EditorModel, FileStatus, NO_COMPUTE, apply, edit_model, editable_model, editor_ui,
    life_starter, move_cursor_to, new_graph_model, new_grid_rule, open_text, receive_open,
};
use crate::ui::files::{DialogFile, OpenResult};
use crate::ui::playback::press_build;

/// Line of the Life starter's step that a rule edit extends.
const BIRTH_LINE: &str = "    if (alive == 3u || (cell == ALIVE && alive == 2u)) {";

/// [`BIRTH_LINE`] with birth on 3 or 6, as in High Life.
const HIGHLIFE_LINE: &str = "    if (alive == 3u || (cell == DEAD && alive == 6u) || (cell == ALIVE && alive == 2u)) {";

/// Returns an app over the example models on a headless device that runs compute shaders, with the Life starter in
/// the Editor, or `None` to skip on a machine without a device.
fn app_with_starter() -> Option<AppState> {
    let mut app = headless_app()?;
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

/// Returns the model file text the Editor opens for `entry`, `None` for a model that is no grid rule applied at run
/// time.
fn rule_text(entry: &ModelEntry) -> Option<String> {
    match editable_model(entry.source().author(), entry.source().code()) {
        Some(EditableModel::Rule(text)) => Some(text),
        _ => None,
    }
}

/// Returns the source of the selected model, which Playback's Build builds when no preview of a sheet runs.
fn selected_source(app: &AppState) -> Option<ModelSource> {
    app.selected_entry().map(|entry| entry.source().clone())
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
    app.editor.model = EditorModel::Rule("an earlier text".to_owned());
    new_grid_rule(&mut app);

    let starter = ModelFile::from_rule(GridRule::life()).expect("the starter is a model file");
    assert_eq!(
        app.editor.model.file_text(),
        starter.to_toml(),
        "the starter is the rule's canonical text"
    );
    assert_eq!(app.editor.model.file_text(), life_starter());
    let read = ModelFile::parse(&app.editor.model.file_text()).expect("the starter reads back");
    assert_eq!(read.rule().expect("a grid rule").id, "life_rule");
    assert_eq!(read.rule().expect("a grid rule").step, LIFE_STEP);
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
    assert_built(&app, "the model");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    let life = app.loaded_entry().expect("the rule runs").source().clone();
    assert_eq!(
        life.code().map(henad_core::provenance::ModelCode::text),
        Some(app.editor.model.file_text().as_str()),
        "the starter is canonical, and the code is its text"
    );
    step_past(&mut app, 0);
    let life_stats = stats(&app);

    app.editor.model = EditorModel::Rule(edited(&app.editor.model.file_text(), BIRTH_LINE, HIGHLIFE_LINE));
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
    let next_build = selected_source(&app);
    app.editor
        .settle_outcome(Some(&highlife), next_build.as_ref(), app.pending_rebuild.is_some());
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Applied),
        "the rule runs after Rebuild"
    );
    app.wait_for_build();
    assert_built(&app, "the rebuilt model");
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

/// "Model applied." and "Model unchanged." go once the model is edited after its Apply, a grid rule's text or a
/// sheet, and the list asks for Apply again. A frame with no edit keeps them.
#[test]
fn an_edit_after_apply_drops_the_applied_line() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    let applied = ApplyOutcome::Applied.summary().0;
    let unchanged = ApplyOutcome::Unchanged.summary().0;
    let press_apply = "Press Apply to check and install the model.";
    apply(&mut app);
    app.wait_for_build();
    assert!(holds_line(&editor_texts(&mut app), applied));
    assert!(
        holds_line(&editor_texts(&mut app), applied),
        "a frame with no edit keeps the line"
    );
    let EditorModel::Rule(text) = &mut app.editor.model else {
        panic!("the Editor holds the rule");
    };
    text.push_str("\n# Birth on 3, survival on 2 or 3.\n");
    let texts = editor_texts(&mut app);
    assert!(
        !holds_line(&texts, applied) && holds_line(&texts, press_apply),
        "{texts:?}"
    );
    assert_eq!(app.editor.outcome, None);

    apply(&mut app);
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Unchanged),
        "a comment leaves the code"
    );
    assert!(holds_line(&editor_texts(&mut app), unchanged));
    let EditorModel::Rule(text) = &mut app.editor.model else {
        panic!("the Editor holds the rule");
    };
    text.push('\n');
    assert!(!holds_line(&editor_texts(&mut app), unchanged));

    open_text(&mut app.editor, starter_text("game_of_life.toml").to_owned(), None);
    apply(&mut app);
    app.wait_for_build();
    assert!(holds_line(&editor_texts(&mut app), applied));
    let EditorModel::Sheet(sheet) = &mut app.editor.model else {
        panic!("the Editor holds the sheet");
    };
    apply_sheet_edit(
        sheet,
        SheetEdit::SetPurpose {
            purpose: Some("Cells live and die by their neighbours.".to_owned()),
        },
    );
    let texts = editor_texts(&mut app);
    assert!(
        !holds_line(&texts, applied) && holds_line(&texts, press_apply),
        "{texts:?}"
    );
}

/// Builds the example SIR model on a 64 by 64 grid and steps it past tick 0.
fn run_sir_past_tick_zero(app: &mut AppState) {
    app.select_model("sir");
    set_grid_side(app, 64);
    app.reset_simulation();
    step_past(app, 0);
}

/// Draws one frame of the Editor tab over `app`, and returns the value of every accessible node, or its name where it
/// has no value.
fn editor_texts(app: &mut AppState) -> Vec<String> {
    accessible_texts(|ui| editor_ui(ui, app), |node| node.value().or_else(|| node.label()))
}

/// Returns whether one of `texts` holds `line` anywhere in it.
fn holds_line(texts: &[String], line: &str) -> bool {
    texts.iter().any(|text| text.contains(line))
}

#[test]
fn applying_beside_another_run_past_tick_zero_waits_for_build() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    run_sir_past_tick_zero(&mut app);

    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Installed));
    assert_eq!(app.loaded_id(), Some("sir"), "the other model's run keeps going");
    assert_eq!(app.selected_model.as_deref(), Some("life_rule"));
    assert!(app.pending_rebuild.is_none(), "the dialog belongs to a run of the rule");

    app.reset_simulation();
    assert_eq!(app.loaded_id(), Some("life_rule"));
    let loaded = app.loaded_entry().map(|entry| entry.source().clone());
    let next_build = selected_source(&app);
    app.editor
        .settle_outcome(loaded.as_ref(), next_build.as_ref(), app.pending_rebuild.is_some());
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert_built(&app, "the model");
}

#[test]
fn a_sheet_applied_beside_another_run_drops_the_build_line_once_its_preview_loads() {
    let Some(mut app) = headless_app() else {
        return;
    };
    run_sir_past_tick_zero(&mut app);
    open_text(&mut app.editor, starter_text("game_of_life.toml").to_owned(), None);
    settle_preview(&mut app);
    assert_eq!(app.editor.preview.state, PreviewState::Waiting);

    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Installed));
    assert_eq!(app.loaded_id(), Some("sir"), "the other model's run keeps going");
    let installed = ApplyOutcome::Installed.summary().0;
    let texts = editor_texts(&mut app);
    assert!(
        holds_line(&texts, SHEET_PREVIEW_WAITS) && holds_line(&texts, installed),
        "{texts:?}"
    );

    // Offload ends the run, and the sheet's preview takes its place. Build then replays the preview.
    app.offload_simulation();
    assert!(
        holds_line(&editor_texts(&mut app), installed),
        "Build still builds the sheet"
    );
    settle_preview(&mut app);
    assert!(app.build_replays_preview());
    let texts = editor_texts(&mut app);
    assert!(
        !holds_line(&texts, installed),
        "the Editor names Build, which replays the preview"
    );
    assert_eq!(app.editor.outcome, None);
    assert!(holds_line(
        &texts,
        "Press Play to run it, or Apply to run the model at its own size."
    ));

    apply(&mut app);
    assert_eq!(app.preview(), None, "Apply replaced the preview");
    assert_eq!(app.loaded_id(), Some("sheet_game_of_life"));
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert_built(&app, "the model");
}

#[test]
fn a_graph_applied_beside_another_run_keeps_the_build_line_over_its_preview() {
    let Some(mut app) = headless_app() else {
        return;
    };
    run_sir_past_tick_zero(&mut app);
    new_graph_model(&mut app, "sir.toml");
    settle_preview(&mut app);
    assert_eq!(app.editor.preview.state, PreviewState::Waiting);

    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Installed));
    app.offload_simulation();
    settle_preview(&mut app);
    assert!(app.preview().is_some(), "the graph's preview loads once the run ends");
    let installed = ApplyOutcome::Installed.summary().0;
    assert!(
        holds_line(&editor_texts(&mut app), installed),
        "Build builds the applied graph behind its preview"
    );

    press_build(&mut app);
    assert_eq!(app.preview(), None, "Build replaced the preview");
    assert_eq!(app.loaded_id(), Some("node_sir"));
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Installed));
    editor_texts(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert_built(&app, "the model");
}

#[test]
fn a_grid_rule_applied_beside_another_run_keeps_the_build_line_after_offload() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    run_sir_past_tick_zero(&mut app);
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Installed));

    app.offload_simulation();
    settle_preview(&mut app);
    assert_eq!(app.preview(), None, "a grid rule has no preview");
    let installed = ApplyOutcome::Installed.summary().0;
    assert!(
        holds_line(&editor_texts(&mut app), installed),
        "Build builds the applied rule"
    );

    press_build(&mut app);
    assert_eq!(app.loaded_id(), Some("life_rule"));
    editor_texts(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert_built(&app, "the model");
}

#[test]
fn selecting_another_model_drops_the_build_line() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    run_sir_past_tick_zero(&mut app);
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Installed));

    // Build now builds SIR again.
    app.select_model("sir");
    let installed = ApplyOutcome::Installed.summary().0;
    assert!(!holds_line(&editor_texts(&mut app), installed));
    assert_eq!(app.editor.outcome, None);
    app.select_model("life_rule");
    editor_texts(&mut app);
    assert_eq!(app.editor.outcome, None, "a dropped line stays dropped");
}

#[test]
fn applying_again_after_cancel_asks_to_rebuild_again() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    apply(&mut app);
    step_past(&mut app, 0);
    let life = app.loaded_entry().expect("the rule runs").source().clone();
    app.editor.model = EditorModel::Rule(edited(&app.editor.model.file_text(), BIRTH_LINE, HIGHLIFE_LINE));
    apply(&mut app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::AwaitingRebuild));

    // Cancel in the Rebuild model dialog.
    app.pending_rebuild = None;
    let next_build = selected_source(&app);
    app.editor
        .settle_outcome(Some(&life), next_build.as_ref(), app.pending_rebuild.is_some());
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
    set_grid_side(&mut app, 64);
    app.reset_simulation();
    assert!(app.is_building(), "SIR builds on the build thread");

    apply(&mut app);
    assert!(app.is_building(), "the rule builds in place of SIR");
    assert_eq!(app.loaded_id(), Some("life_rule"), "SIR's build was not abandoned");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert_built(&app, "the model");
    assert_eq!(app.loaded_id(), Some("life_rule"));

    // An edit at tick 0 builds at once, and Cancel stops that build.
    app.editor.model = EditorModel::Rule(edited(&app.editor.model.file_text(), BIRTH_LINE, HIGHLIFE_LINE));
    apply(&mut app);
    assert!(app.is_building(), "an edit at tick 0 builds at once");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.offload_simulation();
    assert!(
        !app.is_building() && app.sim_thread.is_none() && app.loaded_id().is_none(),
        "Cancel left a build or a run"
    );
    let next_build = selected_source(&app);
    app.editor
        .settle_outcome(None, next_build.as_ref(), app.pending_rebuild.is_some());
    assert_eq!(
        app.editor.outcome,
        Some(ApplyOutcome::Installed),
        "the rule waits for Build after Cancel"
    );

    app.reset_simulation();
    let loaded = app.loaded_entry().map(|entry| entry.source().clone());
    let next_build = selected_source(&app);
    app.editor
        .settle_outcome(loaded.as_ref(), next_build.as_ref(), app.pending_rebuild.is_some());
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied), "Build runs the rule");
    app.wait_for_build();
    assert_built(&app, "the model");
}

#[test]
fn a_code_edit_that_fails_validation_keeps_the_running_model() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    apply(&mut app);
    step_past(&mut app, 0);
    let running = app.loaded_entry().expect("the rule runs").source().clone();
    let starter = app.editor.model.file_text();
    let keeps_running = |app: &AppState| {
        assert_eq!(app.loaded_entry().map(|entry| entry.source()), Some(&running));
        assert_eq!(app.models.get("life_rule").map(|entry| entry.source()), Some(&running));
        assert!(app.sim_thread.is_some(), "the run was torn down");
        assert!(!app.is_building(), "a refused text started a build");
        assert!(app.pending_rebuild.is_none());
        assert!(app.fault.is_none());
    };

    let (looped, loop_line) = insert_line(&starter, "    let alive = moore_count(ALIVE);", "    loop {}");
    app.editor.model = EditorModel::Rule(looped);
    apply(&mut app);
    let problem = refused(&app).first().expect("a problem");
    assert_eq!(problem.line, Some(loop_line), "{problem}");
    assert_eq!(problem.column, Some(5), "{problem}");
    keeps_running(&app);

    let (unclosed, _) = insert_line(&starter, "    return DEAD;", "    let x = (1u;");
    app.editor.model = EditorModel::Rule(unclosed);
    apply(&mut app);
    assert!(
        refused(&app).iter().all(|problem| problem.line.is_some()),
        "every syntax problem has its line: {:?}",
        refused(&app)
    );
    keeps_running(&app);

    app.editor.model = EditorModel::Rule(starter.replace("width = 256", "width = "));
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
    set_grid_side(&mut app, 64);
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
    let refused = with_oversized_buffer(app.models.get("life_rule").cloned().expect("the rule was installed"));
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
    let (looped, loop_line) = insert_line(
        &app.editor.model.file_text(),
        "    let alive = moore_count(ALIVE);",
        "    loop {}",
    );
    app.editor.model = EditorModel::Rule(looped);
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
    let byte = app
        .editor
        .model
        .file_text()
        .find("loop {}")
        .expect("the text holds the loop");
    assert_eq!(
        cursor_index(&ctx),
        Some(app.editor.model.file_text()[..byte].chars().count()),
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
    let applied = app.editor.model.file_text();
    assert_eq!(app.selected_entry().and_then(rule_text), Some(applied.clone()));
    assert_eq!(
        app.models.get("sir").and_then(rule_text),
        None,
        "a compiled model has no model file"
    );

    app.editor.model = EditorModel::Rule(format!("{applied}# an edit since Apply\n"));
    edit_model(&mut app);
    assert!(
        app.editor.model.file_text().ends_with("# an edit since Apply\n"),
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
    assert_eq!(app.editor.model.file_text(), "format = \"henad-model\"\n");
    assert_eq!(app.editor.file_name.as_deref(), Some("other.toml"));
    assert_eq!(
        app.editor.file_status.as_ref().map(FileStatus::text),
        Some("Opened other.toml")
    );
    assert_eq!(app.editor.outcome, None);

    edit_model(&mut app);
    assert_eq!(
        app.editor.model.file_text(),
        applied,
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
        app.editor.model.file_text(),
        applied,
        "a file that is not text replaced the Editor's"
    );
    assert_eq!(
        app.editor.file_status.as_ref().map(FileStatus::text),
        Some("Open failed: binary.toml is not UTF-8 text")
    );
}

#[test]
fn an_apply_the_device_cannot_hold_builds_nothing() {
    let Some(mut app) = app_with_starter() else {
        return;
    };
    // A device whose buffers cannot hold one row of the starter's grid.
    app.runtime.granted.max_storage_buffer_binding_size = 512;
    app.runtime.granted.max_buffer_size = 512;
    apply(&mut app);
    match &app.editor.outcome {
        Some(ApplyOutcome::Faulted(problems)) => {
            assert_eq!(problems.len(), 1, "{problems:?}");
            assert!(
                problems[0].message.starts_with("Too large for this device: "),
                "{problems:?}"
            );
        }
        other => panic!("the size refusal is not the edit's: {other:?}"),
    }
    assert!(app.models.get("life_rule").is_some(), "the rule was installed");
    assert!(!app.has_simulation(), "a build started");
    assert!(app.fault.is_none());

    app.reset_simulation();
    assert!(!app.has_simulation(), "Build built a model the device cannot hold");
}

/// Runs one frame of the Editor tab over `app` with `events`.
fn editor_frame(ctx: &egui::Context, app: &mut AppState, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
        events,
        ..egui::RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| editor_ui(ui, app));
    output.textures_delta.clear();
}

#[test]
fn typing_into_an_empty_editor_starts_a_grid_rule() {
    let Some(mut app) = headless_app() else {
        return;
    };
    assert!(matches!(app.editor.model, EditorModel::Empty));
    let ctx = egui::Context::default();
    editor_frame(&ctx, &mut app, Vec::new());
    assert!(
        matches!(app.editor.model, EditorModel::Empty),
        "a frame with no input left {:?}",
        app.editor.model
    );

    move_cursor(&ctx, 0);
    editor_frame(&ctx, &mut app, Vec::new());
    editor_frame(&ctx, &mut app, vec![egui::Event::Text("# typed".to_owned())]);
    assert_eq!(
        app.editor.model.rule_text(),
        Some("# typed"),
        "the typed text was dropped"
    );
    assert!(!app.editor.model.is_empty());

    editor_frame(&ctx, &mut app, vec![egui::Event::Text(" more".to_owned())]);
    assert_eq!(app.editor.model.rule_text(), Some("# typed more"));
}
