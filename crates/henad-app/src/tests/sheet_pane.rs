//! Tests of the Sheet pane's editing: a blank model built into Game of Life's start by accessible names, the empty
//! world, the restart rule after the first tick, the grid limit, the New parameter dialog's starting values, the
//! parameters, measures, colours and buttons, the model name, Undo and Redo, the rebuild of the Game of Life starter,
//! the pane's share of the Editor's width in each layout, and the words rule over every section.

use egui::{Key, Modifiers};
use henad_graph::limits::MAX_GRID_SIDE;
use henad_graph::sheet::document::{
    ButtonStepRule, ColorRuleKind, Edges, MeasureRule, PropertyType, ShadeHue, SheetChip, SheetLiteral, SheetValue,
    Slot, StartRule,
};
use henad_graph::sheet::edit::{PALETTE_COLORS, SheetEdit};
use henad_graph::sheet::sentence::{
    ADD_BUTTON, ADD_MEASURE, ADD_PARAMETER, ADD_PROPERTY, ADD_ROW, ADD_STATE, AVERAGE_MEASURE, BLANK_MODEL_CARD,
    BUTTON_CARD_EMPTY, BUTTON_NAME_FIELD, BUTTON_NAME_MISSING, CANCEL, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE,
    COLOR_CELLS_BY, CREATE_PARAMETER, DARKEST_NAME, DELETE_PROPERTY, DIALOG_NAME_MISSING, DONE, EDIT, EVERY_PROPERTY,
    INTEGER_KIND, LIGHTEST_NAME, MEASURE_ALREADY_PLOTTED, MEASURE_NEEDS_NUMBER_PROPERTY, MEASURE_NEEDS_STATE,
    NEW_MEASURE, NEW_PARAMETER, NEW_PARAMETER_HEADING, NUMBER_KIND, PARAMETER_NAME_MISSING, PLOT, PLOT_EMPTY, PLOTTED,
    PROPERTY_NAME_MISSING, REDO, RESTART_NOTICE, SHADE, SHADE_ENDS_EQUAL, SHEET_EMPTY, START_AGAIN_FOR,
    START_AGAIN_TO_SEE, STATES_KIND, TYPE_A_CHANCE, TYPE_ONE_NUMBER, UNDO, USE_A_PARAMETER, WALLS_SENTENCE,
    WORLD_EMPTY, WORLD_QUESTION, add_row_name, name_reserved, parameter_max_name, run_once, start_again_for,
    value_slider_name, world_side_out_of_range,
};
use henad_graph::sheet::starters;

use crate::state::PreviewRun;
use crate::tests::panes::{
    Seen, add_property, add_states_property, apply_sheet_edit, assert_control_holds, assert_no_forbidden_word,
    build_life_sections, center_of, choose, click, enter_into, field_value, sheet, sheet_mut, shows_or_notice,
    starter_document, text, type_and_enter, type_into,
};
use crate::tests::support::{
    Harness, TALL_SCREEN, align_with_starter, settle_preview, starter_text, tick_zero_stat, wait_for_snapshot,
    without_card_lines,
};
use crate::ui::dock::Layout;
use crate::ui::editor::preview::PreviewState;
use crate::ui::editor::sheet::new_model::create_blank_model;
use crate::ui::editor::sheet::sheet_pane::slider_step;
use crate::ui::editor::sheet::widgets::{NewParameterDialog, NumberUse, default_max};
use crate::ui::editor::sheet::{CARD_PANE, SHEET_PANE};

/// Returns a harness whose Editor holds a blank model, or `None` to skip on a machine without a device.
fn blank() -> Option<Harness> {
    let mut harness = Harness::new()?;
    create_blank_model(&mut harness.app, "");
    harness.settle();
    Some(harness)
}

/// Drags the divider between the Sheet pane and the Card pane to `x` points from the Editor's left edge.
fn drag_sheet_divider(harness: &mut Harness, x: f32) {
    let nodes = harness.settle();
    let bounds = |name: &str| {
        nodes
            .iter()
            .find(|node| node.role == egui::accesskit::Role::Group && node.label == name)
            .and_then(|node| node.bounds)
            .expect("the pane")
    };
    let divider = f32::midpoint(bounds(SHEET_PANE).right(), bounds(CARD_PANE).left());
    let y = harness.screen.y / 2.0;
    let button = |pressed, x: f32| egui::Event::PointerButton {
        pos: egui::pos2(x, y),
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness.frame(vec![egui::Event::PointerMoved(egui::pos2(divider, y))]);
    harness.frame(vec![button(true, divider)]);
    for step in 1..=10 {
        let to = divider + (x - divider) * step as f32 / 10.0;
        harness.frame(vec![egui::Event::PointerMoved(egui::pos2(to, y))]);
    }
    harness.frame(vec![button(false, x)]);
    harness.frame(vec![egui::Event::PointerGone]);
}

/// Adds the property Status with the states Dead and Alive through the property form.
fn add_status(harness: &mut Harness) {
    add_states_property(harness, ADD_PROPERTY, "Status", &["Dead", "Alive"]);
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
        shows_or_notice(&mut harness, WORLD_QUESTION),
        "Blank asks what the world is made of"
    );
    click(&mut harness, "Create");
    harness.settle();
    let document = sheet(&harness).document();
    assert_eq!(document.sheet.name, "Untitled model");
    assert_eq!((document.world.width, document.world.height), (100, 100));
    assert_eq!(document.world.edges, Edges::Wrap);
    assert!(shows_or_notice(&mut harness, SHEET_EMPTY) && shows_or_notice(&mut harness, BLANK_MODEL_CARD));

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
        shows_or_notice(&mut harness, "Initial density"),
        "the parameter shows as its chip"
    );

    // The preview draws the start at tick 0. A count of Alive, added here by command, reads its share.
    settle_preview(&mut harness.app);
    apply_sheet_edit(
        sheet_mut(&mut harness),
        SheetEdit::AddMeasure {
            name: "Alive".to_owned(),
            rule: MeasureRule::CountInState,
            over: Some("cells".to_owned()),
            property: Some("status".to_owned()),
            state: Some("alive".to_owned()),
            event: None,
            value: Slot::Blank,
            color: None,
        },
    );
    settle_preview(&mut harness.app);
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
    settle_preview(&mut harness.app);
    assert!(harness.app.preview().is_some());
    create_blank_model(&mut harness.app, "");
    settle_preview(&mut harness.app);
    assert_eq!(harness.app.preview(), None, "the Viewport draws nothing");
    assert_eq!(harness.app.editor.preview.state, PreviewState::Idle);
    assert!(
        shows_or_notice(&mut harness, WORLD_EMPTY),
        "the preview line says how to see the world"
    );

    add_status(&mut harness);
    assert!(!shows_or_notice(&mut harness, WORLD_EMPTY));
    settle_preview(&mut harness.app);
    assert!(matches!(harness.app.editor.preview.state, PreviewState::Running(_)));
}

#[test]
fn after_play_a_start_edit_waits_and_a_size_edit_restarts_with_the_notice() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starters()[0].text);
    settle_preview(&mut harness.app);
    harness.app.wait_for_build();
    // Until the first tick, a start edit draws the start again.
    let key = sheet(&harness).preview_key();
    choose(&mut harness, "Status starts as", "All Dead");
    assert_ne!(
        sheet(&harness).preview_key(),
        key,
        "a start edit at tick 0 redraws the start"
    );
    assert!(!shows_or_notice(&mut harness, START_AGAIN_TO_SEE));
    settle_preview(&mut harness.app);
    harness.app.wait_for_build();

    // Play, as Playback's button presses it.
    harness.app.play();
    wait_for_snapshot(&mut harness.app, |tick| tick > 0);
    assert!(harness.app.sheet_preview_past_start());

    let key = sheet(&harness).preview_key();
    choose(&mut harness, "Status starts as", "All Alive");
    assert_eq!(sheet(&harness).preview_key(), key, "a start edit after Play waits");
    assert!(shows_or_notice(&mut harness, START_AGAIN_TO_SEE));
    assert!(!shows_or_notice(&mut harness, RESTART_NOTICE));
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
    assert!(shows_or_notice(&mut harness, START_AGAIN_TO_SEE));
    assert!(!shows_or_notice(&mut harness, RESTART_NOTICE));

    enter_into(&mut harness, "Width in cells", "200");
    assert_eq!(sheet(&harness).document().world.width, 200);
    assert_ne!(sheet(&harness).preview_key(), key, "a size edit restarts the world");
    assert!(shows_or_notice(&mut harness, RESTART_NOTICE));
    settle_preview(&mut harness.app);
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
    assert!(shows_or_notice(&mut harness, &world_side_out_of_range(MAX_GRID_SIDE)));
    assert_eq!(
        field_value(&mut harness, "Width in cells"),
        past,
        "the field keeps the refused text"
    );
    enter_into(&mut harness, "Width in cells", "abc");
    assert!(shows_or_notice(&mut harness, TYPE_ONE_NUMBER));
    for part in ["2.5", "50%"] {
        enter_into(&mut harness, "Width in cells", part);
        assert_eq!(
            sheet(&harness).document().world.width,
            1024,
            "{part} is no whole number of cells"
        );
        assert!(shows_or_notice(&mut harness, &world_side_out_of_range(MAX_GRID_SIDE)));
    }
    enter_into(&mut harness, "Width in cells", "120");
    assert_eq!(sheet(&harness).document().world.width, 120);
    assert!(
        !shows_or_notice(&mut harness, TYPE_ONE_NUMBER),
        "a kept value clears the message"
    );

    choose(&mut harness, "Edges", "Walls");
    assert_eq!(sheet(&harness).document().world.edges, Edges::Walls);
    assert!(shows_or_notice(&mut harness, WALLS_SENTENCE));

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

#[test]
fn a_refused_text_stays_with_the_sheet_and_the_row_it_was_typed_in() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let past = world_side_out_of_range(MAX_GRID_SIDE);
    harness.open_sheet(starters()[0].text);
    enter_into(&mut harness, "Width in cells", "20000");
    assert!(shows_or_notice(&mut harness, &past));
    create_blank_model(&mut harness.app, "");
    assert_eq!(field_value(&mut harness, "Width in cells"), "100");
    assert!(
        !shows_or_notice(&mut harness, &past),
        "a new sheet shows no message of the last"
    );

    // Two blank models both name their first property status.
    let chance = "Status starts as, row 1: chance";
    add_status(&mut harness);
    choose(&mut harness, "Status starts as", "By chance");
    enter_into(&mut harness, chance, "1.5");
    assert!(shows_or_notice(&mut harness, CHANCE_OUT_OF_RANGE));
    create_blank_model(&mut harness.app, "");
    add_status(&mut harness);
    choose(&mut harness, "Status starts as", "By chance");
    assert_eq!(field_value(&mut harness, chance), "");
    assert!(!shows_or_notice(&mut harness, CHANCE_OUT_OF_RANGE));

    // A row that takes the place of a removed row shows its own chance.
    click(&mut harness, "Add row to Status starts as");
    enter_into(&mut harness, "Status starts as, row 2: chance", "0.2");
    enter_into(&mut harness, chance, "1.5");
    assert!(shows_or_notice(&mut harness, CHANCE_OUT_OF_RANGE));
    click(&mut harness, "Remove row 1 of Status starts as");
    harness.settle();
    assert_eq!(field_value(&mut harness, chance), "0.2");
    assert!(!shows_or_notice(&mut harness, CHANCE_OUT_OF_RANGE));
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
    assert!(shows_or_notice(&mut harness, CHANCE_OUT_OF_RANGE));
    assert_eq!(chance(&harness), Slot::Blank);
    enter_into(&mut harness, field, "abc");
    assert!(shows_or_notice(&mut harness, TYPE_A_CHANCE));
    enter_into(&mut harness, field, "30%");
    assert_eq!(
        chance(&harness),
        Slot::Filled(SheetValue::Literal(SheetLiteral::Percent(30.0)))
    );
    assert_eq!(field_value(&mut harness, field), "30%", "a percent shows as typed");

    // A Number property starts at a value, or at a random number between two values.
    add_property(&mut harness, ADD_PROPERTY, "Food", NUMBER_KIND);
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
    assert!(shows_or_notice(&mut harness, SHEET_EMPTY));
}

#[test]
fn the_property_form_checks_in_the_order_of_the_page() {
    let Some(mut harness) = blank() else {
        return;
    };
    click(&mut harness, ADD_PROPERTY);
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, PROPERTY_NAME_MISSING));
    type_into(&mut harness, "Property name", "Status");
    type_into(&mut harness, "State 1 name", "Dead");
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, "Give the property at least two states."));
    type_into(&mut harness, "State 2 name", "dead");
    click(&mut harness, DONE);
    assert!(shows_or_notice(
        &mut harness,
        "Two states have the same name. Give each state its own name."
    ));
    click(&mut harness, CANCEL);
    assert!(sheet(&harness).shows_nothing(), "Cancel adds nothing");
    add_status(&mut harness);
    click(&mut harness, ADD_PROPERTY);
    type_into(&mut harness, "Property name", "status");
    click(&mut harness, DONE);
    assert!(shows_or_notice(
        &mut harness,
        "A property called status already exists."
    ));

    // A name whose id Henad keeps for itself is refused in words, never by its id.
    type_into(&mut harness, "Property name", "Count");
    type_into(&mut harness, "State 1 name", "Few");
    type_into(&mut harness, "State 2 name", "Many");
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, &name_reserved("Count")));
    assert!(!shows_or_notice(&mut harness, "The id 'count' is reserved."));
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
    let mut reserved = NewParameterDialog::new(NumberUse::Number, None, None);
    reserved.name = "Grid width".to_owned();
    assert_eq!(reserved.parameter(&[]), Err(name_reserved("Grid width")));

    // A typed chance outside 0 to 1 is refused, and a percent reads as its hundredth.
    assert_eq!(NumberUse::Chance.read("1.5"), Err(CHANCE_OUT_OF_RANGE));
    assert_eq!(NumberUse::Chance.read("30%"), Ok(SheetLiteral::Percent(30.0)));
}

#[test]
fn the_edited_sections_show_no_forbidden_word_in_any_state() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let mut seen = Seen::default();
    click(&mut harness, "New model");
    seen.look(&mut harness);
    click(&mut harness, "Create");
    seen.look(&mut harness);
    click(&mut harness, ADD_PROPERTY);
    seen.look(&mut harness);
    click(&mut harness, DONE);
    seen.look(&mut harness);
    click(&mut harness, "State 1 colour: Dark grey");
    seen.look(&mut harness);
    click(&mut harness, "Purple");
    seen.look(&mut harness);
    click(&mut harness, NUMBER_KIND);
    seen.look(&mut harness);
    click(&mut harness, INTEGER_KIND);
    seen.look(&mut harness);
    click(&mut harness, STATES_KIND);
    seen.look(&mut harness);
    click(&mut harness, CANCEL);
    add_status(&mut harness);
    seen.look(&mut harness);
    click(&mut harness, "Status starts as");
    seen.look(&mut harness);
    click(&mut harness, "By chance");
    seen.look(&mut harness);
    click(&mut harness, "Status starts as, row 1: use a parameter");
    seen.look(&mut harness);
    click(&mut harness, NEW_PARAMETER);
    seen.look(&mut harness);
    click(&mut harness, CREATE_PARAMETER);
    seen.look(&mut harness);
    type_into(&mut harness, "Parameter name", "Initial density");
    click(&mut harness, CREATE_PARAMETER);
    seen.look(&mut harness);
    click(&mut harness, "Initial density");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Add row to Status starts as");
    seen.look(&mut harness);
    enter_into(&mut harness, "Status starts as, row 2: chance", "1.5");
    seen.look(&mut harness);
    enter_into(&mut harness, "Status starts as, row 2: chance", "abc");
    seen.look(&mut harness);
    click(&mut harness, "Edges");
    seen.look(&mut harness);
    click(&mut harness, "Walls");
    seen.look(&mut harness);
    click(&mut harness, "Unit of one tick");
    seen.look(&mut harness);
    click(&mut harness, "days");
    seen.look(&mut harness);
    enter_into(&mut harness, "Length of one tick", "abc");
    seen.look(&mut harness);
    enter_into(&mut harness, "Width in cells", "0");
    seen.look(&mut harness);
    click(&mut harness, "Edit property Status");
    seen.look(&mut harness);
    click(&mut harness, CANCEL);
    add_property(&mut harness, ADD_PROPERTY, "Food", NUMBER_KIND);
    seen.look(&mut harness);
    choose(&mut harness, "Food starts as", "A random number");
    seen.look(&mut harness);
    assert!(seen.looks() >= 25, "only {} states were drawn", seen.looks());
    assert_no_forbidden_word(&seen);

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

// Parameters, measures, colours, buttons, the model name and undo

#[test]
fn a_blank_model_builds_the_life_sheet_and_undo_walks_back_to_blank() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut texts = build_life_sections(&mut harness, &mut Seen::default());
    click(&mut harness, ADD_BUTTON);
    type_into(&mut harness, BUTTON_NAME_FIELD, "Clear");
    click(&mut harness, &run_once("cell"));
    click(&mut harness, DONE);
    harness.settle();
    texts.push(text(&harness));
    assert!(
        shows_or_notice(&mut harness, BUTTON_CARD_EMPTY),
        "the Clear card has no line yet"
    );

    let expected = without_card_lines(&starter_document("game_of_life.toml"));
    let aligned = align_with_starter(sheet(&harness).document(), &expected);
    assert_eq!(
        aligned.to_text().expect("the rebuild writes"),
        expected.to_text().expect("the starter writes"),
        "the rebuild equals the starter without its card lines"
    );

    // Undo walks back each step to Blank, and Redo forward again.
    let built = texts.len();
    for step in (0..built - 1).rev() {
        click(&mut harness, UNDO);
        assert_eq!(text(&harness), texts[step], "undo back to step {step}");
    }
    assert!(!sheet(&harness).can_undo());
    for (step, expected) in texts.iter().enumerate().skip(1) {
        click(&mut harness, REDO);
        assert_eq!(&text(&harness), expected, "redo forward to step {step}");
    }
    assert!(!sheet(&harness).can_redo());

    // The keys work anywhere in the Editor tab while no text field holds the focus, and walk the same steps.
    for step in (0..built - 1).rev() {
        harness.press(Key::Z, Modifiers::COMMAND);
        assert_eq!(text(&harness), texts[step], "Cmd+Z back to step {step}");
    }
    assert!(!sheet(&harness).can_undo());
    for (step, expected) in texts.iter().enumerate().skip(1) {
        harness.press(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(&text(&harness), expected, "Shift+Cmd+Z forward to step {step}");
    }
    assert!(!sheet(&harness).can_redo());
    harness.press(Key::Z, Modifiers::COMMAND);
    harness.press(Key::Y, Modifiers::COMMAND);
    assert_eq!(text(&harness), texts[built - 1], "Cmd+Y redoes too");

    // An open modal of the app keeps the keys from the sheet underneath.
    harness.app.about_open = true;
    harness.settle();
    harness.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(text(&harness), texts[built - 1], "the About modal keeps Cmd+Z");
    harness.app.about_open = false;
    harness.settle();

    // A focused text field keeps the keys for its own undo.
    click(&mut harness, "Width in cells");
    harness.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(text(&harness), texts[built - 1]);
}

#[test]
fn aligning_a_starter_with_itself_changes_nothing_and_restores_a_changed_key() {
    let starter = starter_document("game_of_life.toml");
    assert_eq!(align_with_starter(&starter, &starter), starter);
    let mut changed = starter.clone();
    for property in changed.cells.iter_mut().flatten() {
        if let Some(start) = &mut property.starts_as {
            start.key = Some(7);
        }
    }
    for card in &mut changed.cards {
        for line in card.line_tables.values_mut() {
            if line.key.is_some() {
                line.key = Some(9);
            }
        }
    }
    assert_ne!(changed, starter);
    assert_eq!(align_with_starter(&changed, &starter), starter);

    // The starts of agents' and ground properties are aligned too. The ants' starts draw nothing, so each is given a
    // key first.
    let ants = starter_document("ants.toml");
    assert_eq!(align_with_starter(&ants, &ants), ants);
    let with_keys = |key: u32| {
        let mut keyed = ants.clone();
        let agents = keyed.agents.iter_mut().flat_map(|kind| &mut kind.properties);
        let ground = keyed.ground.iter_mut().flat_map(|ground| &mut ground.properties);
        for start in agents.chain(ground).filter_map(|property| property.starts_as.as_mut()) {
            start.key = Some(key);
        }
        keyed
    };
    let (keyed, changed) = (with_keys(1), with_keys(7));
    assert!(
        changed
            .ground
            .as_ref()
            .is_some_and(|ground| ground.properties[0].starts_as.is_some())
    );
    assert_ne!(changed, keyed);
    assert_eq!(align_with_starter(&changed, &keyed), keyed);
}

#[test]
fn parameters_edit_on_the_sheet_by_the_rules_of_the_page() {
    let Some(mut harness) = blank() else {
        return;
    };
    click(&mut harness, ADD_PARAMETER);
    assert!(shows_or_notice(&mut harness, NEW_PARAMETER_HEADING));
    assert_eq!(field_value(&mut harness, "Default value"), "1");
    assert_eq!(field_value(&mut harness, "Max"), "10");
    type_into(&mut harness, "Parameter name", "Growth");
    click(&mut harness, CREATE_PARAMETER);
    harness.settle();
    let parameter = |harness: &Harness| sheet(harness).document().parameters[0].clone();
    assert_eq!(
        (
            parameter(&harness).default,
            parameter(&harness).min,
            parameter(&harness).max
        ),
        (1.0, 0.0, 10.0)
    );

    // A typed value outside the range widens it, and a percent reads as its hundredth.
    enter_into(&mut harness, "Growth value", "12");
    assert_eq!((parameter(&harness).default, parameter(&harness).max), (12.0, 12.0));
    enter_into(&mut harness, "Growth value", "50%");
    assert_eq!(parameter(&harness).default, 0.5);
    // Min above Max swaps them, and equal ends make Max one more.
    enter_into(&mut harness, "Growth min", "20");
    assert_eq!((parameter(&harness).min, parameter(&harness).max), (12.0, 20.0));
    assert_eq!(parameter(&harness).default, 12.0, "the value moves into the range");
    enter_into(&mut harness, "Growth max", "12");
    assert_eq!((parameter(&harness).min, parameter(&harness).max), (12.0, 13.0));
    enter_into(&mut harness, "Growth min", "abc");
    assert!(shows_or_notice(&mut harness, TYPE_ONE_NUMBER));
    enter_into(&mut harness, "Growth min", "0");

    // A refused name puts the old name back with the reason.
    enter_into(&mut harness, "Name of Growth", " ");
    assert!(shows_or_notice(&mut harness, PARAMETER_NAME_MISSING));
    assert_eq!(field_value(&mut harness, "Name of Growth"), "Growth");
    click(&mut harness, ADD_PARAMETER);
    type_into(&mut harness, "Parameter name", "Decay");
    click(&mut harness, CREATE_PARAMETER);
    enter_into(&mut harness, "Name of Decay", "growth");
    assert!(shows_or_notice(
        &mut harness,
        "A parameter called growth already exists."
    ));
    enter_into(&mut harness, "Name of Decay", "Death rate");
    assert_eq!(sheet(&harness).document().parameters[1].name, "Death rate");

    // The unit is free text, with suggestions under the field.
    enter_into(&mut harness, "Growth unit", "per cell");
    assert_eq!(parameter(&harness).unit.as_deref(), Some("per cell"));
    click(&mut harness, "Death rate unit");
    click(&mut harness, "per week");
    harness.settle();
    assert_eq!(
        sheet(&harness).document().parameters[1].unit.as_deref(),
        Some("per week")
    );

    click(&mut harness, "Delete parameter Death rate");
    harness.settle();
    assert_eq!(sheet(&harness).document().parameters.len(), 1);

    assert_eq!(slider_step(1.0), 0.005);
    assert_eq!(slider_step(10.0), 0.05);
    assert_eq!(slider_step(5.0), 0.02);
    assert_eq!(slider_step(0.001), 5e-6);
}

#[test]
fn a_parameter_edit_after_play_restarts_at_tick_zero_with_the_notice() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starters()[0].text);
    settle_preview(&mut harness.app);
    harness.app.wait_for_build();
    harness.app.play();
    wait_for_snapshot(&mut harness.app, |tick| tick > 0);
    assert!(harness.app.sheet_preview_past_start());
    let key = sheet(&harness).preview_key();
    enter_into(&mut harness, "Initial density value", "0.5");
    assert_eq!(sheet(&harness).document().parameters[0].default, 0.5);
    assert_ne!(sheet(&harness).preview_key(), key, "the world starts again");
    assert!(shows_or_notice(&mut harness, RESTART_NOTICE));

    // A new name or purpose leaves the running world alone.
    let key = sheet(&harness).preview_key();
    let purpose = sheet(&harness).document().sheet.purpose.clone();
    type_into(&mut harness, "Purpose", "Gliders");
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert_eq!(sheet(&harness).document().sheet.purpose.as_deref(), Some("Gliders"));
    assert_eq!(sheet(&harness).preview_key(), key);

    // Undo and Redo of a new purpose leave it alone too, with no notice.
    assert!(harness.app.sheet_preview_past_start());
    click(&mut harness, UNDO);
    assert_eq!(sheet(&harness).document().sheet.purpose, purpose);
    assert_eq!(sheet(&harness).preview_key(), key, "the undo leaves the world running");
    assert!(!shows_or_notice(&mut harness, RESTART_NOTICE));
    click(&mut harness, REDO);
    assert_eq!(sheet(&harness).document().sheet.purpose.as_deref(), Some("Gliders"));
    assert_eq!(sheet(&harness).preview_key(), key, "the redo leaves the world running");
    assert!(!shows_or_notice(&mut harness, RESTART_NOTICE));
}

#[test]
fn measures_count_states_average_numbers_and_follow_their_state_colour() {
    let Some(mut harness) = blank() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    add_status(&mut harness);
    add_property(&mut harness, ADD_PROPERTY, "Food", NUMBER_KIND);

    click(&mut harness, ADD_MEASURE);
    assert!(shows_or_notice(&mut harness, NEW_MEASURE));
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, MEASURE_NEEDS_STATE));
    click(&mut harness, AVERAGE_MEASURE);
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, MEASURE_NEEDS_NUMBER_PROPERTY));
    choose(&mut harness, "Measure property", "Food");
    click(&mut harness, DONE);
    harness.settle();
    let measure = sheet(&harness).document().measures[0].clone();
    assert_eq!(measure.name, "Average of Food");
    assert_eq!(measure.rule, MeasureRule::Average);
    assert!(shows_or_notice(&mut harness, PLOTTED) && shows_or_notice(&mut harness, "Average of Food"));
    click(&mut harness, ADD_MEASURE);
    click(&mut harness, AVERAGE_MEASURE);
    choose(&mut harness, "Measure property", "Food");
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, MEASURE_ALREADY_PLOTTED));
    click(&mut harness, CANCEL);

    // The preview plots the average, as the Charts tab draws it.
    settle_preview(&mut harness.app);
    assert_eq!(tick_zero_stat(&mut harness.app, "Average of Food"), 0.0);

    // A Plot measure takes its state's colour and follows a recoloured state.
    click(&mut harness, "Plot Alive");
    harness.settle();
    let alive = |harness: &Harness| {
        sheet(harness)
            .document()
            .measures
            .iter()
            .find(|measure| measure.name == "Alive")
            .cloned()
            .expect("the Alive measure")
    };
    assert!(shows_or_notice(&mut harness, "Number of cells whose Status is Alive"));
    click(&mut harness, "Edit property Status");
    click(&mut harness, "State 2 colour: Blue");
    click(&mut harness, "Red");
    click(&mut harness, DONE);
    harness.settle();
    assert_eq!(
        alive(&harness).color,
        PALETTE_COLORS[4],
        "the measure follows its state"
    );
    click(&mut harness, "Colour of Alive: Red");
    click(&mut harness, "Purple");
    harness.settle();
    assert_eq!(alive(&harness).color, PALETTE_COLORS[11]);
    click(&mut harness, "Plot Alive");
    harness.settle();
    assert_eq!(sheet(&harness).document().measures.len(), 1, "unticking removes it");
    click(&mut harness, "Remove measure Average of Food");
    harness.settle();
    assert!(sheet(&harness).document().measures.is_empty());
}

/// The Sheet pane keeps a share of the Editor's width of its own in each layout: a share dragged in one layout leaves
/// the other layout alone, and a narrower window keeps the share.
#[test]
fn each_layout_keeps_a_width_of_its_own_for_the_sheet_pane() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    let sheet_share = |harness: &mut Harness| {
        let nodes = harness.settle();
        let pane = |name: &str| {
            nodes
                .iter()
                .find(|node| node.role == egui::accesskit::Role::Group && node.label == name)
                .and_then(|node| node.bounds)
                .expect("the pane")
        };
        // The divider sits between the two panes' margins.
        f32::midpoint(pane(SHEET_PANE).right(), pane(CARD_PANE).left()) / harness.screen.x
    };
    harness.screen = egui::vec2(1000.0, 1000.0);
    harness.open_sheet(starter_text("game_of_life.toml"));
    let share = sheet_share(&mut harness);
    assert!((share - 0.4).abs() < 0.02, "the default share is 0.4, not {share}");

    // A drag of the divider in the default layout to 600 points.
    drag_sheet_divider(&mut harness, 600.0);
    let dragged = sheet_share(&mut harness);
    assert!((dragged - 0.6).abs() < 0.02, "the drag left a share of {dragged}");

    harness.app.layout = Layout::Editor;
    let editor = sheet_share(&mut harness);
    assert!(
        (editor - 0.4).abs() < 0.02,
        "the Editor layout took the share of the default layout: {editor}"
    );

    harness.app.layout = Layout::Default;
    harness.screen = egui::vec2(700.0, 1000.0);
    let narrower = sheet_share(&mut harness);
    assert!(
        (narrower - 0.6).abs() < 0.02,
        "the default layout lost its share in a narrower window: {narrower}"
    );
}

#[test]
fn every_plot_box_of_three_states_stays_in_the_sheet_pane_and_takes_a_click() {
    let Some(mut harness) = blank() else {
        return;
    };
    // A narrow Editor, as a docked tab beside the Viewport and Charts leaves it, its Sheet pane dragged to 290 points.
    harness.screen = egui::vec2(450.0, 1000.0);
    drag_sheet_divider(&mut harness, 290.0);
    add_states_property(
        &mut harness,
        ADD_PROPERTY,
        "Health",
        &["Susceptible", "Infected", "Recovered"],
    );
    click(&mut harness, DONE);
    harness.settle();

    let nodes = harness.settle();
    let pane_right = nodes
        .iter()
        .filter(|node| node.value == CARD_PANE)
        .find_map(|node| node.bounds)
        .expect("the Card heading")
        .left();
    for state in ["Susceptible", "Infected", "Recovered"] {
        let name = format!("Plot {state}");
        let right = nodes
            .iter()
            .filter(|node| node.label == name)
            .find_map(|node| node.bounds)
            .expect("a Plot box")
            .right();
        assert!(
            right <= pane_right,
            "{name} ends at {right}, past the sheet pane at {pane_right}"
        );
        click(&mut harness, &name);
        harness.settle();
        assert!(
            sheet(&harness)
                .document()
                .measures
                .iter()
                .any(|measure| measure.name == state),
            "{name} adds its measure"
        );
    }
}

#[test]
fn a_number_property_shades_the_cells_in_a_hue_between_two_values() {
    let Some(mut harness) = blank() else {
        return;
    };
    add_property(&mut harness, ADD_PROPERTY, "Food", NUMBER_KIND);
    let rule = |harness: &Harness| sheet(harness).document().colors["cells"].clone();
    assert_eq!(
        rule(&harness).rule,
        ColorRuleKind::Shaded,
        "the first property colours the cells"
    );
    assert_eq!(rule(&harness).hue, Some(ShadeHue::Blue));
    choose(&mut harness, SHADE, "Green");
    enter_into(&mut harness, DARKEST_NAME, "2");
    enter_into(&mut harness, LIGHTEST_NAME, "10");
    let shaded = rule(&harness);
    assert_eq!(
        (shaded.hue, shaded.darkest, shaded.lightest),
        (Some(ShadeHue::Green), Some(2.0), Some(10.0))
    );
    assert!(
        !sheet(&harness).has_errors(),
        "{:?}",
        sheet(&harness).lowering_problems()
    );
    // Equal ends are refused in the field, and reversed ends shade dark at the higher value.
    enter_into(&mut harness, LIGHTEST_NAME, "2");
    assert!(shows_or_notice(&mut harness, SHADE_ENDS_EQUAL));
    assert_eq!(rule(&harness).lightest, Some(10.0));
    enter_into(&mut harness, DARKEST_NAME, "20");
    assert_eq!(
        (rule(&harness).darkest, rule(&harness).lightest),
        (Some(20.0), Some(10.0))
    );
    assert!(!sheet(&harness).has_errors());

    add_status(&mut harness);
    choose(&mut harness, COLOR_CELLS_BY, "Status");
    assert_eq!(rule(&harness).rule, ColorRuleKind::ByState);
    choose(&mut harness, COLOR_CELLS_BY, "Food");
    assert_eq!(rule(&harness).hue, Some(ShadeHue::Blue), "a new shading starts blue");
}

#[test]
fn buttons_start_again_or_run_once_and_check_their_names() {
    let Some(mut harness) = blank() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    add_status(&mut harness);
    click(&mut harness, ADD_BUTTON);
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, BUTTON_NAME_MISSING));
    type_into(&mut harness, BUTTON_NAME_FIELD, "Reset");
    click(&mut harness, DONE);
    harness.settle();
    assert!(shows_or_notice(&mut harness, &start_again_for(EVERY_PROPERTY)));
    click(&mut harness, ADD_BUTTON);
    type_into(&mut harness, BUTTON_NAME_FIELD, "reset");
    click(&mut harness, DONE);
    assert!(shows_or_notice(&mut harness, "A button called reset already exists."));
    click(&mut harness, CANCEL);

    click(&mut harness, "Edit button Reset");
    click(&mut harness, &run_once("cell"));
    click(&mut harness, DONE);
    harness.settle();
    let document = sheet(&harness).document();
    assert_eq!(document.buttons.len(), 1);
    assert_eq!(document.buttons[0].does[0].rule, ButtonStepRule::RunOnce);
    assert!(document.cards.iter().any(|card| card.id == "reset"));
    assert!(shows_or_notice(
        &mut harness,
        "Lines of Reset: run once, every cell, when pressed"
    ));

    click(&mut harness, "Delete button Reset");
    harness.settle();
    let document = sheet(&harness).document();
    assert!(document.buttons.is_empty() && document.cards.is_empty());
}

#[test]
fn the_model_name_renames_from_the_toolbar() {
    let Some(mut harness) = blank() else {
        return;
    };
    click(&mut harness, "Model name: Untitled model. Rename");
    harness.settle();
    harness.replace_text("Life");
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert_eq!(
        sheet(&harness).document().sheet.name,
        "Untitled model",
        "Escape cancels"
    );
    click(&mut harness, "Model name: Untitled model. Rename");
    harness.settle();
    type_and_enter(&mut harness, "Life");
    assert_eq!(sheet(&harness).document().sheet.name, "Life");
    center_of(&mut harness, "Model name: Life. Rename");
    harness.press(Key::Z, Modifiers::COMMAND);
    harness.settle();
    assert_eq!(sheet(&harness).document().sheet.name, "Untitled model");
}

#[test]
fn the_charts_tab_says_how_to_plot_while_the_sheet_has_no_measure() {
    let Some(mut harness) = blank() else {
        return;
    };
    add_status(&mut harness);
    settle_preview(&mut harness.app);
    harness.app.wait_for_build();
    let charts = |harness: &mut Harness| {
        crate::tests::support::accessible_texts(
            |ui| crate::ui::charts::charts_ui(ui, &mut harness.app),
            |node| node.value(),
        )
    };
    assert!(charts(&mut harness).iter().any(|text| text == PLOT_EMPTY));
    click(&mut harness, "Plot Alive");
    settle_preview(&mut harness.app);
    harness.app.wait_for_build();
    assert!(!charts(&mut harness).iter().any(|text| text == PLOT_EMPTY));
}

#[test]
fn the_remaining_sections_show_no_forbidden_word_in_any_state() {
    let Some(mut harness) = blank() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let mut seen = Seen::default();
    seen.look(&mut harness);
    add_status(&mut harness);
    seen.look(&mut harness);
    click(&mut harness, ADD_PARAMETER);
    seen.look(&mut harness);
    type_into(&mut harness, "Parameter name", "Growth");
    click(&mut harness, CREATE_PARAMETER);
    seen.look(&mut harness);
    enter_into(&mut harness, "Growth value", "abc");
    seen.look(&mut harness);
    enter_into(&mut harness, "Name of Growth", " ");
    seen.look(&mut harness);
    click(&mut harness, "Growth unit");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Plot Alive");
    seen.look(&mut harness);
    click(&mut harness, "Colour of Alive: Blue");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, ADD_MEASURE);
    seen.look(&mut harness);
    click(&mut harness, DONE);
    seen.look(&mut harness);
    click(&mut harness, "Measure property");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, AVERAGE_MEASURE);
    seen.look(&mut harness);
    click(&mut harness, DONE);
    seen.look(&mut harness);
    click(&mut harness, CANCEL);
    click(&mut harness, COLOR_CELLS_BY);
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    add_property(&mut harness, ADD_PROPERTY, "Food", NUMBER_KIND);
    choose(&mut harness, COLOR_CELLS_BY, "Food");
    seen.look(&mut harness);
    click(&mut harness, SHADE);
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    enter_into(&mut harness, DARKEST_NAME, "abc");
    seen.look(&mut harness);
    click(&mut harness, ADD_BUTTON);
    seen.look(&mut harness);
    click(&mut harness, DONE);
    seen.look(&mut harness);
    click(&mut harness, START_AGAIN_FOR);
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    type_into(&mut harness, BUTTON_NAME_FIELD, "Clear");
    click(&mut harness, &run_once("cell"));
    click(&mut harness, DONE);
    seen.look(&mut harness);
    click(&mut harness, "Edit button Clear");
    seen.look(&mut harness);
    click(&mut harness, CANCEL);
    click(&mut harness, "Model name: Untitled model. Rename");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, UNDO);
    seen.look(&mut harness);
    click(&mut harness, REDO);
    seen.look(&mut harness);
    assert!(seen.looks() >= 25, "only {} states were drawn", seen.looks());
    assert_no_forbidden_word(&seen);

    let nodes = harness.settle();
    for visible in [
        ADD_PARAMETER,
        ADD_MEASURE,
        ADD_BUTTON,
        PLOT,
        EDIT,
        UNDO,
        REDO,
        "Untitled model",
    ] {
        assert_control_holds(&nodes, visible);
    }
}

#[test]
fn keep_on_the_parameters_tab_is_one_step_of_undo() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.open_sheet(starters()[0].text);
    let before = text(&harness);
    sheet_mut(&mut harness)
        .keep_value("initial_density", 0.6)
        .expect("the parameter is kept");
    assert_eq!(sheet(&harness).document().parameters[0].default, 0.6);
    click(&mut harness, UNDO);
    assert_eq!(text(&harness), before);
}

/// A field whose typed text is left by a click on another control commits in the same frame as that control. Both
/// edits are kept, each as one step of Undo.
#[test]
fn a_typed_number_and_a_click_in_one_frame_both_commit() {
    let Some(mut harness) = blank() else {
        return;
    };
    add_status(&mut harness);
    choose(&mut harness, "Status starts as", "By chance");
    type_into(&mut harness, "Status starts as, row 1: chance", "0.3");
    click(&mut harness, &add_row_name("Status"));
    let start = sheet(&harness).document().cells.as_ref().expect("cells")[0]
        .starts_as
        .clone()
        .expect("a start");
    assert_eq!(start.rows.len(), 2, "+ Add row added a row");
    assert_eq!(
        start.rows[0].chance,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.3))),
        "the chance typed before the click is kept"
    );
    click(&mut harness, UNDO);
    let rows = |harness: &Harness| {
        sheet(harness).document().cells.as_ref().expect("cells")[0]
            .starts_as
            .as_ref()
            .expect("a start")
            .rows
            .clone()
    };
    assert_eq!(rows(&harness).len(), 1, "Undo takes the new row back first");
    assert_eq!(
        rows(&harness)[0].chance,
        Slot::Filled(SheetValue::Literal(SheetLiteral::Number(0.3)))
    );

    // A slider let go in the frame a range field commits keeps the new range.
    click(&mut harness, ADD_PARAMETER);
    type_into(&mut harness, "Parameter name", "Growth");
    click(&mut harness, CREATE_PARAMETER);
    harness.settle();
    type_into(&mut harness, &parameter_max_name("Growth"), "20");
    click(&mut harness, &value_slider_name("Growth"));
    let parameter = sheet(&harness).document().parameters[0].clone();
    assert_eq!(parameter.max, 20.0, "the Max typed before the click is kept");
    assert_ne!(parameter.default, 1.0, "the slider's click is kept");
}

/// Escape in a field of one line puts back what the sheet holds, a refused text and its message included, and edits
/// nothing. Escape in Purpose keeps what is typed.
#[test]
fn escape_cancels_a_typed_number_and_keeps_a_typed_purpose() {
    let Some(mut harness) = blank() else {
        return;
    };
    let before = text(&harness);
    type_into(&mut harness, "Width in cells", "2000");
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert_eq!(text(&harness), before, "Escape edits nothing");
    assert_eq!(field_value(&mut harness, "Width in cells"), "100");

    enter_into(&mut harness, "Width in cells", "abc");
    assert!(shows_or_notice(&mut harness, TYPE_ONE_NUMBER));
    click(&mut harness, "Width in cells");
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert_eq!(field_value(&mut harness, "Width in cells"), "100");
    assert!(
        !shows_or_notice(&mut harness, TYPE_ONE_NUMBER),
        "the message goes with the text"
    );

    type_into(&mut harness, "Purpose", "Cells live and die.");
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert_eq!(
        sheet(&harness).document().sheet.purpose.as_deref(),
        Some("Cells live and die.")
    );
}

/// Two states that swap names in the property form keep their ids and colours, and so do the states of a chain of
/// renames in which each name frees the next.
#[test]
fn the_property_form_swaps_the_names_of_two_states() {
    let Some(mut harness) = blank() else {
        return;
    };
    add_status(&mut harness);
    let states = |harness: &Harness| {
        sheet(harness).document().cells.as_ref().expect("cells")[0]
            .states
            .iter()
            .map(|state| (state.id.clone(), state.name.clone(), state.color))
            .collect::<Vec<_>>()
    };
    let before = states(&harness);
    click(&mut harness, "Edit property Status");
    type_into(&mut harness, "State 1 name", "Alive");
    type_into(&mut harness, "State 2 name", "Dead");
    click(&mut harness, DONE);
    harness.settle();
    let after = states(&harness);
    assert_eq!(
        after,
        [
            (before[0].0.clone(), "Alive".to_owned(), before[0].2),
            (before[1].0.clone(), "Dead".to_owned(), before[1].2),
        ],
        "the ids and colours stay, and the names swap"
    );

    // A chain: Alive takes Dead's name, Dead takes a new one, and a new state takes Alive's.
    click(&mut harness, "Edit property Status");
    type_into(&mut harness, "State 1 name", "Dead");
    type_into(&mut harness, "State 2 name", "Zombie");
    click(&mut harness, ADD_STATE);
    type_into(&mut harness, "State 3 name", "Alive");
    click(&mut harness, DONE);
    harness.settle();
    let names: Vec<String> = states(&harness).into_iter().map(|(_, name, _)| name).collect();
    assert_eq!(names, ["Dead", "Zombie", "Alive"]);

    // On Game of Life, the count that follows its state's name follows the swap, as it follows a rename.
    harness.open_sheet(starters()[0].text);
    click(&mut harness, "Edit property Status");
    type_into(&mut harness, "State 1 name", "Alive");
    type_into(&mut harness, "State 2 name", "Dead");
    click(&mut harness, DONE);
    harness.settle();
    let (_, count) = sheet(&harness).document().line("l1").expect("line 1");
    assert_eq!(count.name.as_deref(), Some("Dead neighbours"));
    assert!(!sheet(&harness).has_errors());
}
