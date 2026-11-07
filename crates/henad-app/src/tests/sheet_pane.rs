//! Tests of the Sheet pane's editing: a blank model built into Game of Life's start by accessible names, the empty
//! world, the restart rule after the first tick, the grid limit, the New parameter dialog's starting values and the
//! words rule over the edited sections.

use egui::{Event, Key, Modifiers};
use henad_graph::limits::MAX_GRID_SIDE;
use henad_graph::sheet::document::{
    Edges, MeasureRule, PropertyType, SheetChip, SheetLiteral, SheetValue, Slot, StartRule,
};
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::sentence::{
    ADD_PROPERTY, ADD_ROW, ADD_STATE, BLANK_MODEL_CARD, CANCEL, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE,
    CREATE_PARAMETER, DELETE_PROPERTY, DIALOG_NAME_MISSING, DONE, INTEGER_KIND, NEW_PARAMETER, NUMBER_KIND,
    PROPERTY_NAME_MISSING, RESTART_NOTICE, SHEET_EMPTY, START_AGAIN_TO_SEE, STATES_KIND, TYPE_A_CHANCE,
    TYPE_ONE_NUMBER, USE_A_PARAMETER, WALLS_SENTENCE, WORLD_EMPTY, WORLD_QUESTION, world_side_out_of_range,
};
use henad_graph::sheet::starters;
use web_time::Instant;

use crate::state::PreviewRun;
use crate::tests::support::{Harness, Node, forbidden_words, tick_zero_stat, wait_for_snapshot};
use crate::ui::editor::EditorModel;
use crate::ui::editor::preview::{DEBOUNCE, PreviewState, poll_at};
use crate::ui::editor::sheet::SheetEditor;
use crate::ui::editor::sheet::new_model::create_blank_model;
use crate::ui::editor::sheet::widgets::{NewParameterDialog, NumberUse, default_max};

/// Returns a harness whose Editor holds a blank model, or `None` to skip on a machine without a device.
fn blank() -> Option<Harness> {
    let mut harness = Harness::new()?;
    create_blank_model(&mut harness.app, "");
    harness.settle();
    Some(harness)
}

/// Sheet in the harness's Editor.
fn sheet(harness: &Harness) -> &SheetEditor {
    harness.app.editor.model.sheet().expect("a sheet")
}

/// Sheet in the harness's Editor, to change.
fn sheet_mut(harness: &mut Harness) -> &mut SheetEditor {
    match &mut harness.app.editor.model {
        EditorModel::Sheet(sheet) => sheet,
        _ => panic!("no sheet"),
    }
}

/// Polls the preview once now and once after the debounce, as two frames that far apart would.
fn settle_preview(harness: &mut Harness) {
    let ctx = egui::Context::default();
    let now = Instant::now();
    poll_at(&ctx, &mut harness.app, now);
    poll_at(&ctx, &mut harness.app, now + DEBOUNCE);
}

/// Returns the centre of the one node of a settled frame named exactly `name`.
fn center_of(harness: &mut Harness, name: &str) -> egui::Pos2 {
    let nodes = harness.settle();
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.label == name && node.bounds.is_some())
        .collect();
    assert_eq!(found.len(), 1, "one widget is named {name}: {found:?}");
    found[0].bounds.expect("bounds").center()
}

/// Clicks the one widget named exactly `name`.
fn click(harness: &mut Harness, name: &str) {
    let position = center_of(harness, name);
    harness.click_at(position);
}

/// Replaces the text of the field named `name` with `text`, without committing it.
fn type_into(harness: &mut Harness, name: &str, text: &str) {
    click(harness, name);
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text(text.to_owned())]);
}

/// Replaces the text of the field named `name` with `text` and presses Enter.
fn enter_into(harness: &mut Harness, name: &str, text: &str) {
    type_into(harness, name, text);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
}

/// Chooses `item` in the select named `select`.
fn choose(harness: &mut Harness, select: &str, item: &str) {
    click(harness, select);
    click(harness, item);
    harness.settle();
}

/// Returns whether a settled frame shows `text` as a label, a value or a description, or as the end of one after an
/// icon.
fn shows(harness: &mut Harness, text: &str) -> bool {
    harness
        .accessible_texts()
        .iter()
        .any(|shown| shown == text || shown.ends_with(&format!(" {text}")))
}

/// Adds the property Status with the states Dead and Alive through the property form.
fn add_status(harness: &mut Harness) {
    click(harness, ADD_PROPERTY);
    type_into(harness, "Property name", "Status");
    type_into(harness, "State 1 name", "Dead");
    type_into(harness, "State 2 name", "Alive");
    click(harness, DONE);
    harness.settle();
}

/// Sets Status to start Alive by chance, with a new parameter Initial density at 0.3, through "Use a parameter".
fn start_alive_by_chance(harness: &mut Harness) {
    choose(harness, "Status starts as", "By chance");
    click(harness, "Status starts as, row 1: use a parameter");
    click(harness, NEW_PARAMETER);
    type_into(harness, "Parameter name", "Initial density");
    type_into(harness, "Default value", "0.3");
    click(harness, CREATE_PARAMETER);
    harness.settle();
}

#[test]
fn a_blank_model_builds_the_life_start_by_accessible_names() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    click(&mut harness, "New model");
    assert!(
        shows(&mut harness, WORLD_QUESTION),
        "Blank asks what the world is made of"
    );
    click(&mut harness, "Create");
    harness.settle();
    let document = sheet(&harness).document();
    assert_eq!(document.sheet.name, "Untitled model");
    assert_eq!((document.world.width, document.world.height), (100, 100));
    assert_eq!(document.world.edges, Edges::Wrap);
    assert!(shows(&mut harness, SHEET_EMPTY) && shows(&mut harness, BLANK_MODEL_CARD));

    add_status(&mut harness);
    let status = &sheet(&harness).document().cells.as_ref().expect("cells")[0];
    assert_eq!(status.name, "Status");
    let states: Vec<&str> = status.states.iter().map(|state| state.name.as_str()).collect();
    assert_eq!(states, ["Dead", "Alive"]);
    assert_eq!(
        status.starts_as.as_ref().and_then(|start| start.value.value()),
        Some(&SheetValue::Chip(SheetChip::State("dead".to_owned()))),
        "a new States property starts in its first state"
    );

    start_alive_by_chance(&mut harness);
    let document = sheet(&harness).document();
    let parameter = &document.parameters[0];
    assert_eq!(
        (parameter.name.as_str(), parameter.default, parameter.min, parameter.max),
        ("Initial density", 0.3, 0.0, 1.0)
    );
    let start = document.cells.as_ref().expect("cells")[0]
        .starts_as
        .clone()
        .expect("a start");
    assert_eq!(start.rule, StartRule::ByChance);
    assert_eq!(start.otherwise.as_deref(), Some("dead"));
    assert_eq!(start.rows[0].state.as_deref(), Some("alive"));
    assert_eq!(
        start.rows[0].chance,
        Slot::Filled(SheetValue::Chip(SheetChip::Param("initial_density".to_owned())))
    );
    assert!(
        shows(&mut harness, "Initial density"),
        "the parameter shows as its chip"
    );

    // The preview draws the start at tick 0. A count of Alive, added here by command, reads its share.
    settle_preview(&mut harness);
    sheet_mut(&mut harness)
        .edit(false, false, |builder| {
            builder
                .apply(SheetEdit::AddMeasure {
                    name: "Alive".to_owned(),
                    rule: MeasureRule::CountInState,
                    over: Some("cells".to_owned()),
                    property: Some("status".to_owned()),
                    state: Some("alive".to_owned()),
                    event: None,
                    value: Slot::Blank,
                    color: None,
                })
                .map(drop)
        })
        .expect("the measure is added");
    settle_preview(&mut harness);
    assert_eq!(
        harness.app.preview(),
        Some(PreviewRun::Grid {
            width: 100,
            height: 100
        })
    );
    let share = tick_zero_stat(&mut harness.app, "Alive") / 10_000.0;
    assert!((share - 0.3).abs() <= 0.02, "the Alive share at tick 0 is {share}");
}

#[test]
fn a_blank_world_shows_nothing_until_a_property_is_added() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    // A preview of another sheet runs, and the blank model takes it down.
    harness.open_sheet(starters()[0].text);
    settle_preview(&mut harness);
    assert!(harness.app.preview().is_some());
    create_blank_model(&mut harness.app, "");
    settle_preview(&mut harness);
    assert_eq!(harness.app.preview(), None, "the Viewport draws nothing");
    assert_eq!(harness.app.editor.preview.state, PreviewState::Idle);
    assert!(
        shows(&mut harness, WORLD_EMPTY),
        "the preview line says how to see the world"
    );

    add_status(&mut harness);
    assert!(!shows(&mut harness, WORLD_EMPTY));
    settle_preview(&mut harness);
    assert!(matches!(harness.app.editor.preview.state, PreviewState::Running(_)));
}

#[test]
fn after_play_a_start_edit_waits_and_a_size_edit_restarts_with_the_notice() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starters()[0].text);
    settle_preview(&mut harness);
    harness.app.wait_for_build();
    // Until the first tick, a start edit draws the start again.
    let key = sheet(&harness).preview_key();
    choose(&mut harness, "Status starts as", "All Dead");
    assert_ne!(
        sheet(&harness).preview_key(),
        key,
        "a start edit at tick 0 redraws the start"
    );
    assert!(!shows(&mut harness, START_AGAIN_TO_SEE));
    settle_preview(&mut harness);
    harness.app.wait_for_build();

    // Play, as Playback's button presses it.
    harness.app.sim_thread.as_mut().expect("a run").play();
    harness.app.sim_running = true;
    wait_for_snapshot(&mut harness.app, |tick| tick > 0);
    assert!(harness.app.sheet_preview_past_start());

    let key = sheet(&harness).preview_key();
    choose(&mut harness, "Status starts as", "All Alive");
    assert_eq!(sheet(&harness).preview_key(), key, "a start edit after Play waits");
    assert!(shows(&mut harness, START_AGAIN_TO_SEE));
    assert!(!shows(&mut harness, RESTART_NOTICE));
    // A parameter made from a start's field is a changed start too.
    choose(&mut harness, "Status starts as", "By chance");
    click(&mut harness, "Status starts as, row 1: use a parameter");
    click(&mut harness, NEW_PARAMETER);
    type_into(&mut harness, "Parameter name", "Seed share");
    click(&mut harness, CREATE_PARAMETER);
    harness.settle();
    assert!(
        sheet(&harness)
            .document()
            .parameters
            .iter()
            .any(|parameter| parameter.name == "Seed share")
    );
    assert_eq!(sheet(&harness).preview_key(), key, "a new parameter in a start waits");
    assert!(shows(&mut harness, START_AGAIN_TO_SEE));
    assert!(!shows(&mut harness, RESTART_NOTICE));

    enter_into(&mut harness, "Width in cells", "200");
    assert_eq!(sheet(&harness).document().world.width, 200);
    assert_ne!(sheet(&harness).preview_key(), key, "a size edit restarts the world");
    assert!(shows(&mut harness, RESTART_NOTICE));
    settle_preview(&mut harness);
    harness.app.wait_for_build();
    assert_eq!(
        harness.app.preview(),
        Some(PreviewRun::Grid {
            width: 200,
            height: 256
        })
    );
    assert!(!harness.app.sheet_preview_past_start(), "the world is back at tick 0");
    assert!(
        !sheet(&harness).start_changed("status"),
        "the restart drew the new start"
    );
}

#[test]
fn the_world_takes_the_engine_grid_limit_and_refuses_past_it() {
    let Some(mut harness) = blank() else {
        return;
    };
    enter_into(&mut harness, "Width in cells", "1024");
    enter_into(&mut harness, "Height in cells", "1024");
    let world = sheet(&harness).document().world;
    assert_eq!((world.width, world.height), (1024, 1024));

    let past = (MAX_GRID_SIDE + 1).to_string();
    enter_into(&mut harness, "Width in cells", &past);
    assert_eq!(
        sheet(&harness).document().world.width,
        1024,
        "a side past the limit is refused"
    );
    assert!(shows(&mut harness, &world_side_out_of_range(MAX_GRID_SIDE)));
    let field = harness
        .settle()
        .into_iter()
        .find(|node| node.label == "Width in cells")
        .expect("the width field");
    assert_eq!(field.value, past, "the field keeps the refused text");
    enter_into(&mut harness, "Width in cells", "abc");
    assert!(shows(&mut harness, TYPE_ONE_NUMBER));
    for part in ["2.5", "50%"] {
        enter_into(&mut harness, "Width in cells", part);
        assert_eq!(
            sheet(&harness).document().world.width,
            1024,
            "{part} is no whole number of cells"
        );
        assert!(shows(&mut harness, &world_side_out_of_range(MAX_GRID_SIDE)));
    }
    enter_into(&mut harness, "Width in cells", "120");
    assert_eq!(sheet(&harness).document().world.width, 120);
    assert!(!shows(&mut harness, TYPE_ONE_NUMBER), "a kept value clears the message");

    choose(&mut harness, "Edges", "Walls");
    assert_eq!(sheet(&harness).document().world.edges, Edges::Walls);
    assert!(shows(&mut harness, WALLS_SENTENCE));

    choose(&mut harness, "Unit of one tick", "days");
    let time = sheet(&harness).document().time.expect("a time scale");
    assert_eq!((time.tick_length, time.tick_unit.as_str()), (1.0, "day"));
    enter_into(&mut harness, "Length of one tick", "0");
    assert_eq!(sheet(&harness).document().time.map(|time| time.tick_length), Some(1.0));
    enter_into(&mut harness, "Length of one tick", "7");
    assert_eq!(sheet(&harness).document().time.map(|time| time.tick_length), Some(7.0));
    choose(&mut harness, "Unit of one tick", "one step (no unit)");
    assert_eq!(sheet(&harness).document().time, None);
}

/// Returns the value of the field named exactly `name` in a settled frame.
fn field_value(harness: &mut Harness, name: &str) -> String {
    harness
        .settle()
        .into_iter()
        .find(|node| node.label == name)
        .map(|node| node.value)
        .unwrap_or_else(|| panic!("no field is named {name}"))
}

#[test]
fn a_refused_text_stays_with_the_sheet_and_the_row_it_was_typed_in() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let past = world_side_out_of_range(MAX_GRID_SIDE);
    harness.open_sheet(starters()[0].text);
    enter_into(&mut harness, "Width in cells", "20000");
    assert!(shows(&mut harness, &past));
    create_blank_model(&mut harness.app, "");
    assert_eq!(field_value(&mut harness, "Width in cells"), "100");
    assert!(!shows(&mut harness, &past), "a new sheet shows no message of the last");

    // Two blank models both name their first property status.
    let chance = "Status starts as, row 1: chance";
    add_status(&mut harness);
    choose(&mut harness, "Status starts as", "By chance");
    enter_into(&mut harness, chance, "1.5");
    assert!(shows(&mut harness, CHANCE_OUT_OF_RANGE));
    create_blank_model(&mut harness.app, "");
    add_status(&mut harness);
    choose(&mut harness, "Status starts as", "By chance");
    assert_eq!(field_value(&mut harness, chance), "");
    assert!(!shows(&mut harness, CHANCE_OUT_OF_RANGE));

    // A row that takes the place of a removed row shows its own chance.
    click(&mut harness, "Add row to Status starts as");
    enter_into(&mut harness, "Status starts as, row 2: chance", "0.2");
    enter_into(&mut harness, chance, "1.5");
    assert!(shows(&mut harness, CHANCE_OUT_OF_RANGE));
    click(&mut harness, "Remove row 1 of Status starts as");
    harness.settle();
    assert_eq!(field_value(&mut harness, chance), "0.2");
    assert!(!shows(&mut harness, CHANCE_OUT_OF_RANGE));
}

#[test]
fn a_field_keeps_typed_text_while_the_window_lacks_the_system_focus() {
    let Some(mut harness) = blank() else {
        return;
    };
    harness.window_focused = false;
    type_into(&mut harness, "Width in cells", "120");
    assert_eq!(
        field_value(&mut harness, "Width in cells"),
        "120",
        "the typed text stays"
    );
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
    assert_eq!(sheet(&harness).document().world.width, 120);
}

#[test]
fn a_starts_as_chance_takes_a_percent_and_refuses_what_is_no_chance() {
    let Some(mut harness) = blank() else {
        return;
    };
    add_status(&mut harness);
    choose(&mut harness, "Status starts as", "By chance");
    let chance = |harness: &Harness| {
        sheet(harness).document().cells.as_ref().expect("cells")[0]
            .starts_as
            .as_ref()
            .expect("a start")
            .rows[0]
            .chance
            .clone()
    };
    assert_eq!(chance(&harness), Slot::Blank, "a new row's chance is empty");
    let field = "Status starts as, row 1: chance";
    enter_into(&mut harness, field, "1.5");
    assert!(shows(&mut harness, CHANCE_OUT_OF_RANGE));
    assert_eq!(chance(&harness), Slot::Blank);
    enter_into(&mut harness, field, "abc");
    assert!(shows(&mut harness, TYPE_A_CHANCE));
    enter_into(&mut harness, field, "30%");
    assert_eq!(
        chance(&harness),
        Slot::Filled(SheetValue::Literal(SheetLiteral::Percent(30.0)))
    );
    let shown = harness
        .settle()
        .into_iter()
        .find(|node| node.label == field)
        .expect("the chance field");
    assert_eq!(shown.value, "30%", "a percent shows as typed");

    // A Number property starts at a value, or at a random number between two values.
    click(&mut harness, ADD_PROPERTY);
    type_into(&mut harness, "Property name", "Food");
    click(&mut harness, NUMBER_KIND);
    click(&mut harness, DONE);
    harness.settle();
    enter_into(&mut harness, "Food starts as: number", "2.5");
    let food = |harness: &Harness| {
        sheet(harness).document().cells.as_ref().expect("cells")[1]
            .starts_as
            .clone()
            .expect("a start")
    };
    assert_eq!(
        food(&harness).value,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(2.5)))
    );
    choose(&mut harness, "Food starts as", "A random number");
    enter_into(&mut harness, "Food starts as, high end: number", "4");
    let Some(SheetValue::Phrase(phrase)) = food(&harness).value.value().cloned() else {
        panic!("a random number");
    };
    assert_eq!(phrase.low, Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.0))));
    assert_eq!(
        phrase.high,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(4.0)))
    );
}

#[test]
fn editing_a_property_renames_recolors_and_changes_its_kind() {
    let Some(mut harness) = blank() else {
        return;
    };
    add_status(&mut harness);
    click(&mut harness, "Edit property Status");
    type_into(&mut harness, "Property name", "Life");
    click(&mut harness, ADD_STATE);
    type_into(&mut harness, "State 3 name", "Infected");
    click(&mut harness, "Remove state 1");
    click(&mut harness, DONE);
    harness.settle();
    let property = sheet(&harness).document().cells.as_ref().expect("cells")[0].clone();
    assert_eq!(property.name, "Life");
    let states: Vec<&str> = property.states.iter().map(|state| state.name.as_str()).collect();
    assert_eq!(states, ["Alive", "Infected"]);
    // A usual name takes its usual colour while no colour is chosen by hand.
    assert_eq!(property.states[1].color, [0xE4, 0x37, 0x48, 0xFF]);
    assert_eq!(
        property.starts_as.as_ref().and_then(|start| start.value.value()),
        Some(&SheetValue::Chip(SheetChip::State("alive".to_owned()))),
        "a start in a removed state takes the first state left"
    );

    click(&mut harness, "Edit property Life");
    click(&mut harness, INTEGER_KIND);
    click(&mut harness, DONE);
    harness.settle();
    let property = sheet(&harness).document().cells.as_ref().expect("cells")[0].clone();
    assert_eq!(property.property_type, PropertyType::WholeNumber);
    assert!(property.states.is_empty());

    click(&mut harness, "Edit property Life");
    click(&mut harness, DELETE_PROPERTY);
    harness.settle();
    assert!(sheet(&harness).shows_nothing());
    assert!(shows(&mut harness, SHEET_EMPTY));
}

#[test]
fn the_property_form_checks_in_the_order_of_the_page() {
    let Some(mut harness) = blank() else {
        return;
    };
    click(&mut harness, ADD_PROPERTY);
    click(&mut harness, DONE);
    assert!(shows(&mut harness, PROPERTY_NAME_MISSING));
    type_into(&mut harness, "Property name", "Status");
    type_into(&mut harness, "State 1 name", "Dead");
    click(&mut harness, DONE);
    assert!(shows(&mut harness, "Give the property at least two states."));
    type_into(&mut harness, "State 2 name", "dead");
    click(&mut harness, DONE);
    assert!(shows(
        &mut harness,
        "Two states have the same name. Give each state its own name."
    ));
    click(&mut harness, CANCEL);
    assert!(sheet(&harness).shows_nothing(), "Cancel adds nothing");
    add_status(&mut harness);
    click(&mut harness, ADD_PROPERTY);
    type_into(&mut harness, "Property name", "status");
    click(&mut harness, DONE);
    assert!(shows(&mut harness, "A property called status already exists."));
}

#[test]
fn the_new_parameter_dialog_starts_from_the_values_of_the_page() {
    let fields = |dialog: &NewParameterDialog| (dialog.default.clone(), dialog.min.clone(), dialog.max.clone());
    let text = |default: &str, min: &str, max: &str| (default.to_owned(), min.to_owned(), max.to_owned());
    assert_eq!(
        fields(&NewParameterDialog::new(NumberUse::Chance, None, None)),
        text("0.1", "0", "1")
    );
    assert_eq!(
        fields(&NewParameterDialog::new(NumberUse::Number, None, None)),
        text("1", "0", "10")
    );
    assert_eq!(
        fields(&NewParameterDialog::new(NumberUse::Number, Some(3.0), None)),
        text("3", "0", "50"),
        "a typed number starts the range at about ten times it"
    );
    assert_eq!(
        fields(&NewParameterDialog::new(NumberUse::Number, Some(-2.0), None)),
        text("-2", "-4", "20"),
        "a negative default sets Min to twice it"
    );
    assert_eq!(
        fields(&NewParameterDialog::new(NumberUse::Chance, Some(0.3), None)),
        text("0.3", "0", "1")
    );
    assert_eq!(default_max(0.02, NumberUse::Number), 0.2);
    assert_eq!(default_max(0.0, NumberUse::Number), 10.0);
    assert_eq!(default_max(120.0, NumberUse::Number), 2000.0);

    // Max follows Default until it is typed, and every field takes a percent.
    let mut dialog = NewParameterDialog::new(NumberUse::Number, None, None);
    dialog.set_default("4".to_owned());
    assert_eq!(dialog.max, "50");
    dialog.name = "Step size".to_owned();
    dialog.set_default("30%".to_owned());
    let parameter = dialog.parameter(&[]).expect("the fields read");
    assert_eq!((parameter.default, parameter.max), (0.3, 1.0));

    let mut chance = NewParameterDialog::new(NumberUse::Chance, None, None);
    assert_eq!(chance.parameter(&[]), Err(DIALOG_NAME_MISSING.to_owned()));
    chance.name = "Initial density".to_owned();
    assert_eq!(
        chance.parameter(&["initial density"]),
        Err("A parameter called Initial density already exists.".to_owned())
    );
    chance.max = "1.5".to_owned();
    assert_eq!(chance.parameter(&[]), Err(CHANCE_LIMITS_OUTSIDE.to_owned()));

    // A typed chance outside 0 to 1 is refused, and a percent reads as its hundredth.
    assert_eq!(NumberUse::Chance.read("1.5"), Err(CHANCE_OUT_OF_RANGE));
    assert_eq!(NumberUse::Chance.read("30%"), Ok(SheetLiteral::Percent(30.0)));
}

#[test]
fn the_edited_sections_show_no_forbidden_word_in_any_state() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let mut texts = Vec::new();
    let mut states = 0;
    let mut record = |harness: &mut Harness| {
        texts.push(harness.accessible_texts());
        states += 1;
    };
    click(&mut harness, "New model");
    record(&mut harness);
    click(&mut harness, "Create");
    record(&mut harness);
    click(&mut harness, ADD_PROPERTY);
    record(&mut harness);
    click(&mut harness, DONE);
    record(&mut harness);
    click(&mut harness, "State 1 colour: Dark grey");
    record(&mut harness);
    click(&mut harness, "Purple");
    record(&mut harness);
    click(&mut harness, NUMBER_KIND);
    record(&mut harness);
    click(&mut harness, INTEGER_KIND);
    record(&mut harness);
    click(&mut harness, STATES_KIND);
    record(&mut harness);
    click(&mut harness, CANCEL);
    add_status(&mut harness);
    record(&mut harness);
    click(&mut harness, "Status starts as");
    record(&mut harness);
    click(&mut harness, "By chance");
    record(&mut harness);
    click(&mut harness, "Status starts as, row 1: use a parameter");
    record(&mut harness);
    click(&mut harness, NEW_PARAMETER);
    record(&mut harness);
    click(&mut harness, CREATE_PARAMETER);
    record(&mut harness);
    type_into(&mut harness, "Parameter name", "Initial density");
    click(&mut harness, CREATE_PARAMETER);
    record(&mut harness);
    click(&mut harness, "Initial density");
    record(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Add row to Status starts as");
    record(&mut harness);
    enter_into(&mut harness, "Status starts as, row 2: chance", "1.5");
    record(&mut harness);
    enter_into(&mut harness, "Status starts as, row 2: chance", "abc");
    record(&mut harness);
    click(&mut harness, "Edges");
    record(&mut harness);
    click(&mut harness, "Walls");
    record(&mut harness);
    click(&mut harness, "Unit of one tick");
    record(&mut harness);
    click(&mut harness, "days");
    record(&mut harness);
    enter_into(&mut harness, "Length of one tick", "abc");
    record(&mut harness);
    enter_into(&mut harness, "Width in cells", "0");
    record(&mut harness);
    click(&mut harness, "Edit property Status");
    record(&mut harness);
    click(&mut harness, CANCEL);
    click(&mut harness, ADD_PROPERTY);
    type_into(&mut harness, "Property name", "Food");
    click(&mut harness, NUMBER_KIND);
    click(&mut harness, DONE);
    record(&mut harness);
    choose(&mut harness, "Food starts as", "A random number");
    record(&mut harness);
    assert!(states >= 25, "only {states} states were drawn");
    let all: Vec<String> = texts.into_iter().flatten().collect();
    let found = forbidden_words(&all);
    assert!(found.is_empty(), "forbidden words: {found:?}");

    // Each control with words of its own holds them in its name.
    let nodes = harness.settle();
    for visible in [ADD_PROPERTY, ADD_ROW, USE_A_PARAMETER, "Status", "Food"] {
        assert_control_holds(&nodes, visible);
    }
    click(&mut harness, "Edit property Status");
    let nodes = harness.settle();
    for visible in [
        DONE,
        CANCEL,
        DELETE_PROPERTY,
        ADD_STATE,
        STATES_KIND,
        NUMBER_KIND,
        INTEGER_KIND,
    ] {
        assert_control_holds(&nodes, visible);
    }
}

/// Asserts that a button or a radio button of `nodes` shows `visible` and that its name holds it, a leading "+ " and
/// a trailing "..." aside.
fn assert_control_holds(nodes: &[Node], visible: &str) {
    use egui::accesskit::Role;

    let words = visible.trim_start_matches("+ ").trim_end_matches("...").to_lowercase();
    let named = nodes.iter().any(|node| {
        matches!(node.role, Role::Button | Role::RadioButton | Role::CheckBox)
            && node.label.to_lowercase().contains(&words)
    });
    assert!(named, "no control's name holds its visible words {visible:?}");
}
