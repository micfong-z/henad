//! Tests of the Parameters tab's preview mode (D54, D88): a change rebuilds the preview at tick 0 with the value, or
//! reaches the running preview at once for a live parameter (L1), the value survives a rebuild after another sheet
//! edit, Reset and Keep end the trial, and the selected model stays as it was.

use henad_compute::entry::ModelEntry;
use henad_core::params::ParamValue;
use henad_graph::sheet::document::SheetDocument;
use henad_graph::sheet::sentence::{FORBIDDEN_WORDS, sheet_trying_value};
use henad_graph::sheet::{ParsedSheet, starters};
use web_time::Instant;

use crate::state::AppState;
use crate::tests::support::accessible_texts;
use crate::ui::editor::preview::{DEBOUNCE, poll_at};
use crate::ui::editor::sheet::SheetEditor;
use crate::ui::editor::sheet::sheet_pane::{SheetTrial, sheet_pane};
use crate::ui::editor::{EditorModel, open_text};
use crate::ui::params::{TrialChoice, apply_trial, params_ui, sheet_number};

/// Returns an app over the example models on a headless device that runs compute shaders, or `None` to skip on a
/// machine without a device.
fn app() -> Option<AppState> {
    AppState::headless(henad_models::example_models(), true)
}

/// Returns the text of the sheet starter whose file is named `file_name`.
fn starter_text(file_name: &str) -> &'static str {
    starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .expect("a sheet starter")
        .text
}

/// Polls the preview once now and once after the debounce, as two frames that far apart would.
fn settle(app: &mut AppState) {
    let ctx = egui::Context::default();
    let now = Instant::now();
    poll_at(&ctx, app, now);
    poll_at(&ctx, app, now + DEBOUNCE);
}

/// Opens the sheet starter `file_name` in the Editor and waits for its preview to build.
fn preview(app: &mut AppState, file_name: &str) {
    open_text(
        &mut app.editor,
        starter_text(file_name).to_owned(),
        Some(file_name.to_owned()),
    );
    settle(app);
    app.wait_for_build();
    assert!(app.previews_editor_sheet(), "{file_name} previews");
    assert!(app.params_drive_preview(), "the Parameters tab drives the preview");
}

/// Entry the preview runs.
fn preview_entry(app: &AppState) -> ModelEntry {
    app.loaded_entry().expect("a preview").clone()
}

/// Returns the position of parameter `id` among the parameters of `entry`.
fn index_of(entry: &ModelEntry, id: &str) -> usize {
    entry
        .param_descriptors()
        .iter()
        .position(|descriptor| descriptor.id() == id)
        .unwrap_or_else(|| panic!("no parameter {id}"))
}

/// Moves the slider of parameter `id` of the preview to `value`, as the Parameters tab does.
fn try_value(app: &mut AppState, id: &str, value: f32) {
    let entry = preview_entry(app);
    let index = index_of(&entry, id);
    apply_trial(app, &entry, index, TrialChoice::Try(ParamValue::F32(value)));
}

/// Takes the loaded run's snapshots until one whose tick passes `accept`, and returns its tick and the value of the
/// stat `label`, or panics after ten seconds.
fn snapshot_where(app: &mut AppState, label: &str, accept: impl Fn(u64) -> bool) -> (u64, f64) {
    app.wait_for_build();
    let thread = app.sim_thread.as_mut().expect("a model is loaded");
    for _ in 0..1000 {
        if let Some(snapshot) = thread.take_snapshot()
            && accept(snapshot.tick)
        {
            let value = snapshot
                .stats
                .iter()
                .find(|entry| entry.label() == label)
                .map(|entry| entry.value().scalar())
                .expect("the stat is published");
            return (snapshot.tick, value);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("no snapshot of the loaded model passed the test");
}

/// Returns the document of the sheet in the Editor.
fn document(app: &AppState) -> SheetDocument {
    app.editor.model.sheet().expect("a sheet").document().clone()
}

/// Puts `document` in the Editor in place of its sheet, as an edit that starts the preview again does, and keeps the
/// preview's tried values.
fn replace_sheet(app: &mut AppState, document: SheetDocument) {
    app.editor.model = EditorModel::Sheet(Box::new(SheetEditor::new(document)));
}

/// A change of Initial density on the Game of Life sheet rebuilds the preview at tick 0 with the value, and the value
/// survives the rebuild after an edit of the world's size. An edit of the value on the sheet ends the trial.
#[test]
fn a_params_tab_change_rebuilds_the_preview_with_the_value_and_keeps_it_across_an_edit() {
    let Some(mut app) = app() else {
        return;
    };
    preview(&mut app, "game_of_life.toml");
    let (_, before) = snapshot_where(&mut app, "Alive", |tick| tick == 0);

    try_value(&mut app, "initial_density", 0.9);
    assert!(
        app.editor.preview.is_due(),
        "a reload-only parameter rebuilds the preview"
    );
    settle(&mut app);
    let (tick, alive) = snapshot_where(&mut app, "Alive", |tick| tick == 0);
    assert_eq!(tick, 0);
    assert!(alive > 2.0 * before, "{alive} alive at 0.9, {before} at 0.3");
    let entry = preview_entry(&app);
    assert_eq!(
        app.loaded_values[index_of(&entry, "initial_density")],
        ParamValue::F32(0.9)
    );
    assert!(!app.sim_running, "the rebuilt preview of a sheet waits at tick 0");

    // An edit of the world's size starts the preview again, with the tried value.
    let mut edited = document(&app);
    edited.world.width = 128;
    edited.world.height = 128;
    replace_sheet(&mut app, edited);
    settle(&mut app);
    let (_, alive) = snapshot_where(&mut app, "Alive", |tick| tick == 0);
    let cells = 128.0 * 128.0;
    assert!(alive > 0.8 * cells, "{alive} of {cells} alive after the edit");

    // An edit of the value the model keeps ends the trial, and the next rebuild runs at that value.
    let mut edited = document(&app);
    edited.parameters[0].default = 0.1;
    replace_sheet(&mut app, edited);
    settle(&mut app);
    assert!(app.editor.preview.tried("initial_density").is_none(), "the trial ended");
    let (_, alive) = snapshot_where(&mut app, "Alive", |tick| tick == 0);
    assert!(alive < 0.2 * cells, "{alive} of {cells} alive at the sheet's 0.1");
}

/// A change of the infection rate during a SIR preview's run reaches the running preview: nothing rebuilds and the tick
/// keeps counting from where it was.
#[test]
fn a_live_parameter_reaches_the_running_preview_and_the_tick_keeps_counting() {
    let Some(mut app) = app() else {
        return;
    };
    preview(&mut app, "sir.toml");
    app.sim_thread.as_mut().expect("a run").play();
    app.sim_running = true;
    let (reached, _) = snapshot_where(&mut app, "Infected", |tick| tick >= 5);

    try_value(&mut app, "infection_rate", 2.0);
    assert!(!app.editor.preview.is_due(), "a live parameter rebuilds nothing");
    let entry = preview_entry(&app);
    assert_eq!(
        app.loaded_values[index_of(&entry, "infection_rate")],
        ParamValue::F32(2.0)
    );
    settle(&mut app);
    let (later, _) = snapshot_where(&mut app, "Infected", |tick| tick > reached + 5);
    assert!(later > reached, "the tick went on from {reached} to {later}");
    assert!(app.sim_running && app.preview().is_some());
}

/// The preview mode leaves the selected model's values, seed and schedule alone, and gives way once another model is
/// selected.
#[test]
fn the_preview_mode_leaves_the_selected_model_alone() {
    let Some(mut app) = app() else {
        return;
    };
    preview(&mut app, "sir.toml");
    let selected = app.selected_model.clone();
    let values = app.param_values.clone();
    let seed = app.seed;
    try_value(&mut app, "infection_rate", 0.7);
    try_value(&mut app, "initial_infected", 0.05);
    settle(&mut app);
    app.wait_for_build();
    assert_eq!(app.selected_model, selected);
    assert_eq!(app.param_values, values);
    assert_eq!(app.seed, seed);
    assert!(app.params_drive_preview());

    app.select_model("boids");
    assert!(!app.params_drive_preview(), "the tab draws the selected model");
    assert!(app.preview().is_some(), "selecting builds nothing");
}

/// Reset returns a tried value to the value the model keeps, and Keep makes it the value the sheet keeps as one edit,
/// leaving the preview's key alone.
#[test]
fn reset_and_keep_end_a_trial() {
    let Some(mut app) = app() else {
        return;
    };
    preview(&mut app, "sir.toml");
    let entry = preview_entry(&app);
    let rate = index_of(&entry, "infection_rate");

    try_value(&mut app, "infection_rate", 0.5);
    assert!(app.editor.preview.tried("infection_rate").is_some());
    apply_trial(&mut app, &entry, rate, TrialChoice::Reset);
    assert!(app.editor.preview.tried("infection_rate").is_none());
    assert_eq!(
        app.loaded_values[rate],
        ParamValue::F32(0.3),
        "Reset sends the kept value"
    );

    try_value(&mut app, "infection_rate", 0.5);
    let key = app.editor.model.preview_source().map(|source| source.key);
    apply_trial(&mut app, &entry, rate, TrialChoice::Keep);
    assert!(
        app.editor.preview.tried("infection_rate").is_none(),
        "Keep ends the trial"
    );
    let kept = document(&app)
        .parameters
        .into_iter()
        .find(|parameter| parameter.id == "infection_rate")
        .expect("the rate");
    assert_eq!(kept.default, 0.5, "the sheet keeps the tried value");
    assert_eq!(
        app.editor.model.preview_source().map(|source| source.key),
        key,
        "Keep starts nothing again"
    );
    assert_eq!(app.loaded_values[rate], ParamValue::F32(0.5), "the preview runs at it");

    // Moving the slider back to the value the model keeps ends a trial.
    try_value(&mut app, "infection_rate", 0.8);
    try_value(&mut app, "infection_rate", 0.5);
    assert!(app.editor.preview.tried("infection_rate").is_none());
}

/// Keep widens the range of a parameter to hold a value outside it.
#[test]
fn keep_widens_the_range_to_hold_the_value() {
    let parsed = ParsedSheet::parse(starter_text("sir.toml")).expect("the starter reads");
    let mut sheet = SheetEditor::new(parsed.document().clone());
    sheet.keep_value("infection_rate", 6.0).expect("the edit");
    let rate = sheet
        .document()
        .parameters
        .iter()
        .find(|parameter| parameter.id == "infection_rate")
        .expect("the rate");
    assert_eq!((rate.min, rate.max, rate.default), (0.0, 6.0, 6.0));
    assert!(sheet.keep_value("no_such_parameter", 1.0).is_err());
}

/// A tried value shows its note with Reset and Keep in the Parameters tab, and its line under the parameter on the
/// sheet, in the wording table's words and with no forbidden word.
#[test]
fn a_tried_value_shows_its_note_reset_keep_and_the_sheets_line() {
    let Some(mut app) = app() else {
        return;
    };
    preview(&mut app, "sir.toml");
    try_value(&mut app, "infection_rate", 0.5);

    let texts = accessible_texts(
        |ui| params_ui(ui, &mut app),
        |node| node.label().or_else(|| node.value()),
    );
    for wanted in [
        "Trying this value. The model keeps 0.3.",
        "Reset Infection rate to 0.3",
        "Keep 0.5 for Infection rate",
    ] {
        assert!(texts.iter().any(|text| text == wanted), "{wanted:?} in {texts:?}");
    }
    assert!(
        !texts.iter().any(|text| text.contains("Seed")),
        "the preview mode shows no Seed field: {texts:?}"
    );
    assert!(
        texts.iter().any(|text| text == "New outbreak"),
        "the preview's button: {texts:?}"
    );

    let mut sheet = SheetEditor::new(document(&app));
    let trials = [SheetTrial {
        parameter: "infection_rate".to_owned(),
        value: 0.5,
    }];
    let sheet_texts = accessible_texts(
        |ui| sheet_pane(ui, &mut sheet, false, &trials),
        |node| node.label().or_else(|| node.value()),
    );
    let line = sheet_trying_value("0.5", "0.3");
    assert_eq!(line, "The world is trying 0.5. The model keeps 0.3.");
    assert!(sheet_texts.contains(&line), "{sheet_texts:?}");

    for text in texts.iter().chain(&sheet_texts) {
        for word in text.split(|c: char| !c.is_alphanumeric()) {
            assert!(
                !FORBIDDEN_WORDS
                    .iter()
                    .any(|forbidden| forbidden.eq_ignore_ascii_case(word)),
                "{word:?} in {text:?}"
            );
        }
    }
}

/// A tried float reaches the sheet's line and Keep as its shortest decimal form, and a kind a sheet parameter cannot
/// hold reaches neither.
#[test]
fn a_tried_value_reaches_the_sheet_as_its_shortest_decimal() {
    assert_eq!(sheet_number(&ParamValue::F32(0.3)), Some(0.3));
    assert_eq!(sheet_number(&ParamValue::F32(0.000_32)), Some(0.000_32));
    assert_eq!(sheet_number(&ParamValue::U32(7)), Some(7.0));
    assert_eq!(sheet_number(&ParamValue::Bool(true)), None);
}
