//! Tests of the ids the Editor gives a new model: a sheet from Blank or from a starter, the grid rule and a node graph
//! from the New model dialog never take the id of a model compiled into the app, so Apply adds the new model beside it.

use henad_compute::entry::{ModelEntry, ModelSet, register_grid_model};
use henad_core::authoring::model::grid_model::GridModel;
use henad_core::grid::Grid2D;
use henad_core::params::{ParamDescriptor, ParamValue};
use henad_core::provenance::CodeOrigin;
use henad_core::topology::NeighborhoodKind;
use henad_core::view::{StatDescriptor, StatValue};
use henad_graph::sheet::document::{PropertyType, SheetLiteral, SheetValue, Slot, StartRule, StartsAs};
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::sentence::UNTITLED_MODEL;

use crate::state::AppState;
use crate::tests::panes::apply_sheet_edit;
use crate::tests::support::{edited, headless_app, settle_preview, starter_text};
use crate::ui::editor::sheet::new_model::{create_blank_agent_model, create_blank_model, create_model};
use crate::ui::editor::{
    ApplyOutcome, EditorModel, ReplaceQuestion, apply, apply_replacing, new_graph_model, new_grid_rule, new_model_id,
    replace_question_modal,
};

/// Declares a grid model compiled into the app under `$id` and `$name`, as a program of the user's own registers one.
macro_rules! compiled_model {
    ($model:ident, $id:literal, $name:literal) => {
        struct $model;

        impl GridModel for $model {
            const NAME: &'static str = $name;
            const ID: &'static str = $id;
            const DESCRIPTION: &'static str = "A model of the user's own program, registered only by tests";
            const PALETTE: &'static [[u8; 4]] = &[[0, 0, 0, 0xFF]];
            const NEIGHBORHOOD: NeighborhoodKind = NeighborhoodKind::Moore;
            const STATS: &'static [StatDescriptor] = &[StatDescriptor::new("Cells", [0xFF, 0xFF, 0xFF, 0xFF])];
            type Params = ();

            fn param_descriptors() -> Vec<ParamDescriptor> {
                Vec::new()
            }

            fn from_params(_params: &[ParamValue]) {}

            fn init(_grid: &mut Grid2D<u8>, _params: &[ParamValue], _rng: &mut u64) {}

            fn step_cell(cell: u8, _neighbors: &[u8], _params: &(), _rng: &mut u64) -> u8 {
                cell
            }

            fn stats(grid: &Grid2D<u8>) -> Vec<StatValue> {
                vec![StatValue::Scalar(grid.current().len() as f64)]
            }
        }
    };
}

compiled_model!(HarbourSeals, "harbour_seals", "Harbour seals");
compiled_model!(SheetHarbourSeals, "sheet_harbour_seals", "Harbour seals (sheet)");
compiled_model!(OwnVote, "vote", "Vote");
compiled_model!(OwnLifeRule, "life_rule", "Life rule of my own");
compiled_model!(OwnNodeSir, "node_sir", "SIR of my own");

/// Returns the example models with the user's compiled models beside them.
fn models_with_the_users_own() -> ModelSet {
    let mut models = ModelSet::new(henad_core::build_info!());
    let own = [
        register_grid_model::<HarbourSeals>(),
        register_grid_model::<SheetHarbourSeals>(),
        register_grid_model::<OwnVote>(),
        register_grid_model::<OwnLifeRule>(),
        register_grid_model::<OwnNodeSir>(),
    ];
    for entry in henad_models::example_models().iter().cloned().chain(own) {
        models.insert(entry).expect("the ids are unique");
    }
    models
}

/// Returns an app over the example models and the user's compiled models on a headless device that runs compute
/// shaders, or `None` to skip on a machine without a device.
fn app() -> Option<AppState> {
    AppState::headless(models_with_the_users_own(), true)
}

/// Returns the id of the sheet in the Editor.
fn sheet_id(app: &AppState) -> String {
    app.editor.model.sheet().expect("a sheet").document().sheet.id.clone()
}

/// Adds a Number property that starts at 0 to the cells of the blank sheet in the Editor, so the sheet builds.
fn add_a_number(app: &mut AppState) {
    let EditorModel::Sheet(sheet) = &mut app.editor.model else {
        panic!("the Editor holds a sheet");
    };
    sheet
        .edit(false, false, |builder| {
            let added = builder.apply(SheetEdit::AddProperty {
                thing: "cells".to_owned(),
                name: "Level".to_owned(),
                property_type: PropertyType::Number,
            })?;
            let mut start = StartsAs::new(StartRule::Value);
            start.value = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.0)));
            builder
                .apply(SheetEdit::SetStartsAs {
                    property: added.into_iter().next().unwrap_or_default(),
                    starts_as: Some(Box::new(start)),
                })
                .map(drop)
        })
        .expect("a blank world of cells takes a Number that starts at 0");
}

/// Waits for the preview of the Editor's model, applies it, and waits for the build.
fn apply_and_build(app: &mut AppState) {
    settle_preview(app);
    app.wait_for_build();
    apply(app);
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();
    assert!(app.fault.is_none(), "the applied model failed to build");
}

/// Asserts that the set still holds `entry`, a compiled model, under its id with its source.
fn assert_kept(app: &AppState, entry: &ModelEntry) {
    let kept = app
        .models
        .get(entry.id())
        .unwrap_or_else(|| panic!("{} left the set", entry.id()));
    assert_eq!(kept.source(), entry.source(), "{}", entry.id());
    assert_eq!(kept.source().origin(), CodeOrigin::Compiled, "{}", entry.id());
}

#[test]
fn a_held_id_takes_the_prefix_and_then_a_number_and_runtime_code_holds_none() {
    let models = models_with_the_users_own();
    assert_eq!(new_model_id(&models, "my_model", "sheet_"), "my_model");
    assert_eq!(new_model_id(&models, "game_of_life", "sheet_"), "sheet_game_of_life");
    assert_eq!(new_model_id(&models, "sir", "sheet_"), "sheet_sir");
    assert_eq!(new_model_id(&models, "vote", "sheet_"), "sheet_vote");
    // `sheet_harbour_seals` is compiled too, and the id takes the next number.
    assert_eq!(
        new_model_id(&models, "harbour_seals", "sheet_"),
        "sheet_harbour_seals_2"
    );
    // An id that already starts with the prefix takes a number alone, and an empty prefix the same.
    assert_eq!(new_model_id(&models, "node_sir", "node_"), "node_sir_2");
    assert_eq!(new_model_id(&models, "life_rule", ""), "life_rule_2");

    // A model applied from the Editor is runtime code, and a new model may take its id and replace it.
    let mut models = models;
    let applied = henad_explore::model_file::ModelFile::parse(&edited(
        starter_text("game_of_life.toml"),
        "id = \"sheet_game_of_life\"",
        "id = \"my_life\"",
    ))
    .and_then(|file| file.entry())
    .expect("the edited starter builds");
    models.insert(applied).expect("a new id");
    assert_eq!(new_model_id(&models, "my_life", "sheet_"), "my_life");
}

#[test]
fn a_blank_model_named_after_an_example_takes_its_own_id_and_apply_keeps_the_example() {
    let Some(mut app) = app() else {
        return;
    };
    let example = app
        .models
        .get("game_of_life")
        .expect("the example Game of Life")
        .clone();
    create_blank_model(&mut app, "Game of Life");
    assert_eq!(sheet_id(&app), "sheet_game_of_life");
    add_a_number(&mut app);
    apply_and_build(&mut app);
    assert_eq!(app.loaded_id(), Some("sheet_game_of_life"));
    assert_kept(&app, &example);

    // A second blank model of the name takes the same id, and replaces the sheet applied first.
    create_blank_model(&mut app, "Game of Life");
    assert_eq!(sheet_id(&app), "sheet_game_of_life");

    // A blank model of agents follows the same rule.
    create_blank_agent_model(&mut app, "Boids");
    assert_eq!(sheet_id(&app), "sheet_boids");
}

#[test]
fn a_blank_model_named_after_the_users_own_model_takes_its_own_id_and_apply_keeps_it() {
    let Some(mut app) = app() else {
        return;
    };
    let own = app.models.get("harbour_seals").expect("the user's model").clone();
    let own_sheet = app.models.get("sheet_harbour_seals").expect("the user's model").clone();
    create_blank_model(&mut app, "Harbour seals");
    assert_eq!(sheet_id(&app), "sheet_harbour_seals_2");
    add_a_number(&mut app);
    apply_and_build(&mut app);
    assert_eq!(app.loaded_id(), Some("sheet_harbour_seals_2"));
    assert_kept(&app, &own);
    assert_kept(&app, &own_sheet);
}

#[test]
fn starters_keep_their_ids_unless_a_compiled_model_holds_them() {
    let Some(mut app) = app() else {
        return;
    };
    // The Life starter's id is free, and stays under a typed name.
    create_model(&mut app, "game_of_life.toml", "My life");
    assert_eq!(sheet_id(&app), "sheet_game_of_life");
    // The user's program holds `vote`, and the vote starter takes `sheet_vote`.
    create_model(&mut app, "vote.toml", "Majority vote");
    assert_eq!(sheet_id(&app), "sheet_vote");

    new_grid_rule(&mut app);
    let rule = henad_explore::model_file::ModelFile::parse(&app.editor.model.file_text()).expect("the rule reads");
    assert_eq!(rule.id(), "life_rule_2");

    new_graph_model(&mut app, "sir.toml");
    let graph = app.editor.model.graph().expect("a graph").document();
    assert_eq!(
        (graph.id.as_str(), graph.target.id.as_str()),
        ("node_sir_2", "node_sir_2")
    );
    new_graph_model(&mut app, "vote.toml");
    assert_eq!(
        app.editor.model.graph().expect("a graph").document().target.id,
        "node_vote"
    );

    // Over the example models alone, each keeps the id it ships with.
    let Some(mut app) = headless_app() else {
        return;
    };
    new_grid_rule(&mut app);
    assert_eq!(app.editor.model.file_text(), crate::ui::editor::life_starter());
    new_graph_model(&mut app, "sir.toml");
    assert_eq!(
        app.editor.model.graph().expect("a graph").document().target.id,
        "node_sir"
    );
}

/// Renames the sheet in the Editor to `name`. The sheet keeps its id.
fn rename_sheet(app: &mut AppState, name: &str) {
    let EditorModel::Sheet(sheet) = &mut app.editor.model else {
        panic!("the Editor holds a sheet");
    };
    apply_sheet_edit(sheet, SheetEdit::SetSheetName { name: name.to_owned() });
}

/// Returns the name of the model of id `id` in the set.
fn name_of(app: &AppState, id: &str) -> String {
    app.models.get(id).expect("the set holds the id").name().to_owned()
}

/// Runs one frame of the dialog that asks before Apply replaces a model, with `events`.
fn replace_frame(ctx: &egui::Context, app: &mut AppState, events: Vec<egui::Event>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))),
        events,
        ..egui::RawInput::default()
    };
    let mut output = ctx.run_ui(input, |ui| replace_question_modal(ui.ctx(), app));
    output.textures_delta.clear();
}

/// Returns the events of a press of `key`.
fn key_press(key: egui::Key) -> Vec<egui::Event> {
    [true, false]
        .map(|pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        })
        .to_vec()
}

/// Apply asks before it replaces a model applied from another sheet under the same id, a sheet of another name, and
/// leaves the applied model until Replace. A second sheet of the same name, and a rename of the sheet the Editor
/// applied last, replace the applied model without asking.
#[test]
fn apply_asks_before_it_replaces_a_model_applied_from_another_sheet() {
    let Some(mut app) = app() else {
        return;
    };
    create_blank_model(&mut app, UNTITLED_MODEL);
    let id = sheet_id(&app);
    add_a_number(&mut app);
    apply_and_build(&mut app);
    create_blank_model(&mut app, UNTITLED_MODEL);
    assert_eq!(sheet_id(&app), id);
    add_a_number(&mut app);
    apply_and_build(&mut app);
    assert_eq!(app.editor.replace_question, None, "a sheet of the same name");
    rename_sheet(&mut app, "Chance SIR");
    apply(&mut app);
    assert_eq!(app.editor.replace_question, None, "the sheet the Editor applied last");
    assert_eq!(name_of(&app, &id), "Chance SIR");
    app.wait_for_build();

    // Another sheet, renamed before its Apply.
    create_blank_model(&mut app, UNTITLED_MODEL);
    add_a_number(&mut app);
    rename_sheet(&mut app, "Rates SIR");
    apply(&mut app);
    let question = ReplaceQuestion {
        name: "Chance SIR".to_owned(),
        id: id.clone(),
    };
    assert_eq!(app.editor.replace_question.as_ref(), Some(&question));
    assert_eq!(
        name_of(&app, &id),
        "Chance SIR",
        "the applied model waits on the dialog"
    );
    assert_eq!(app.editor.outcome, None);

    // Escape cancels, and Enter presses Replace.
    let ctx = egui::Context::default();
    replace_frame(&ctx, &mut app, Vec::new());
    replace_frame(&ctx, &mut app, key_press(egui::Key::Escape));
    assert_eq!(app.editor.replace_question, None);
    assert_eq!(name_of(&app, &id), "Chance SIR", "Cancel keeps the applied model");
    apply(&mut app);
    assert_eq!(app.editor.replace_question.as_ref(), Some(&question));
    replace_frame(&ctx, &mut app, Vec::new());
    replace_frame(&ctx, &mut app, key_press(egui::Key::Enter));
    assert_eq!(app.editor.replace_question, None);
    assert_eq!(name_of(&app, &id), "Rates SIR", "Replace applies the sheet");
    assert_eq!(app.editor.outcome, Some(ApplyOutcome::Applied));
    app.wait_for_build();

    // The question goes with the model it asked about, and a later Replace applies nothing.
    create_blank_model(&mut app, UNTITLED_MODEL);
    add_a_number(&mut app);
    rename_sheet(&mut app, "Third SIR");
    apply(&mut app);
    assert!(app.editor.replace_question.is_some());
    new_grid_rule(&mut app);
    assert_eq!(app.editor.replace_question, None, "another model replaced the sheet");
    apply_replacing(&mut app);
    assert_eq!(name_of(&app, &id), "Rates SIR");
}

/// The dialog that asks before Apply replaces a model closes on Cancel and on a click outside it, and goes once the
/// models change places, each time with the applied model left as it is.
#[test]
fn the_replace_question_closes_on_cancel_a_click_outside_and_a_swap() {
    use henad_graph::sheet::sentence::{CANCEL, REPLACE_APPLIED_HEADING};

    use crate::tests::panes::press_in_dialog;
    use crate::tests::support::Harness;
    use crate::ui::editor::expert::{convert_sheet, swap_counterpart};

    let Some(mut harness) = Harness::new() else {
        return;
    };
    let app = &mut harness.app;
    create_blank_model(app, UNTITLED_MODEL);
    let id = sheet_id(app);
    add_a_number(app);
    rename_sheet(app, "Chance SIR");
    apply_and_build(app);
    create_blank_model(app, UNTITLED_MODEL);
    add_a_number(app);
    rename_sheet(app, "Rates SIR");
    apply(app);
    assert!(app.editor.replace_question.is_some());

    press_in_dialog(&mut harness, REPLACE_APPLIED_HEADING, CANCEL);
    assert_eq!(harness.app.editor.replace_question, None, "Cancel closes the dialog");
    assert_eq!(name_of(&harness.app, &id), "Chance SIR");

    apply(&mut harness.app);
    harness.settle();
    harness.click_at(egui::pos2(4.0, 996.0));
    assert_eq!(
        harness.app.editor.replace_question, None,
        "a click outside closes the dialog"
    );
    assert_eq!(name_of(&harness.app, &id), "Chance SIR");

    // The node graph copy shows after the conversion, and a swap brings the sheet back.
    convert_sheet(&mut harness.app);
    swap_counterpart(&mut harness.app.editor);
    assert!(harness.app.editor.model.sheet().is_some());
    apply(&mut harness.app);
    assert!(harness.app.editor.replace_question.is_some());
    swap_counterpart(&mut harness.app.editor);
    assert_eq!(harness.app.editor.replace_question, None, "a swap drops the question");
    harness.settle();
    assert_eq!(name_of(&harness.app, &id), "Chance SIR");
}
