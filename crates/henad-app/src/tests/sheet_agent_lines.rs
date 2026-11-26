//! Tests of writing the lines of a card over agents: the random walk and boids rebuilt from a blank model of agents by
//! accessible names, "my new Velocity" offered only below the last change to Velocity, the refusals of a property
//! changed two ways and of a look inside a Repeat shown on their lines, the words rule over every agent menu and
//! picker opened on the way, the Number box opening on a number typed in each kind of agent slot, and a block's add
//! menu that scrolls to its last item in a short window.

use egui::{Key, Modifiers};
use henad_graph::sheet::check::check;
use henad_graph::sheet::document::{
    CellWhich, LineKind, ParameterType, PropertyType, SheetChip, SheetDocument, SheetLine, SheetLiteral,
    SheetParameter, SheetValue, Slot,
};
use henad_graph::sheet::edit::{SheetBuilder, SheetEdit};
use henad_graph::sheet::sentence::remove_choose_row_name;
use henad_graph::sheet::sentence::{
    ADD_BUTTON, ADD_ROW_TOOLTIP, ADD_SUMMARY_TOOLTIP, ADD_VALUE_TOOLTIP, AHEAD_NEEDS_CELLS, ARROW_KIND, AddButton,
    BUTTON_NAME_FIELD, Blank, CHANGE_OR_REMOVE_SUMMARY, CHOOSE_WHEN, CREATE_PARAMETER, DONE, DrawKind,
    FORMULA_PART_TOOLTIP, INCLUDING_ME, LOOK_IN_REPEAT, NEW_PARAMETER, NO_AGENT_OR_GROUND_STATES, NO_AGENT_STATES,
    NO_STATES, NOT_INCLUDING_ME, NOT_YET_AGENT_WALLS, NOT_YET_BLOCKED_MOVE_BY, NOT_YET_REPEAT_CHANGE, NUMBER_BOX_HINT,
    OR_A_NEW_VALUE, PlaceTarget, Sentences, add_property_name, add_summary_name, blocked_state_name, moved_by_already,
    moved_to_already, run_once, sheet_parameter_name_field, steered_already, value_field_name, value_slider_name,
};

use crate::tests::panes::{
    Seen, add_average_measure, add_property, agent_sheet, apply_sheet_edit, assert_controls_hold_their_words,
    assert_no_forbidden_word, click, click_in, click_last, click_on_line, document_of, field_value, is_open, labels_of,
    line_picker, line_text, named, new_blank_agents, open_starter, pick, picker_labels, picker_offers, sheet,
    sheet_mut, shows, starter_document, text, text_and_code, type_and_enter, type_into,
};
use crate::tests::support::{Harness, align_with_starter, edited, starter_text};
use crate::ui::editor::sheet::card_pane::{NoteColor, card_lines};
use crate::ui::editor::sheet::picker::{looked_at_picker, mark_refusals, picker_for, takes_ground_states};

/// Opens the menu of the add button `button` of a card over agents, the first such button when `first` holds and the
/// last otherwise, records what it shows, and chooses `item`.
fn add_agent_line(harness: &mut Harness, seen: &mut Seen, button: AddButton, item: &str, first: bool) {
    let found = named(harness, button.label());
    let node = if first { found.first() } else { found.last() };
    let position = node.and_then(|node| node.bounds).expect("an add button").center();
    harness.click_at(position);
    assert!(
        shows(harness, button.heading_for(true)),
        "{} opens its menu",
        button.label()
    );
    seen.look(harness);
    click_last(harness, item);
    harness.settle();
}

/// Creates a parameter in the open New parameter dialog, with `default` in `min` to `max`.
fn new_parameter(harness: &mut Harness, seen: &mut Seen, name: &str, default: &str, min: &str, max: &str) {
    assert!(
        is_open(harness, "New parameter"),
        "the New parameter dialog is open for {name}"
    );
    seen.look(harness);
    type_into(harness, "Parameter name", name);
    type_into(harness, "Max", max);
    type_into(harness, "Min", min);
    type_into(harness, "Default value", default);
    click(harness, CREATE_PARAMETER);
    harness.settle();
}

/// Clicks the control named `control`, picks "New parameter…" in the picker titled `dialog` it opens, and creates
/// the parameter `name` with `default` in `min` to `max`.
#[expect(clippy::too_many_arguments, reason = "a parameter's four values each take a part")]
fn parameter_at(
    harness: &mut Harness,
    seen: &mut Seen,
    control: &str,
    dialog: &str,
    name: &str,
    default: &str,
    min: &str,
    max: &str,
) {
    if !is_open(harness, dialog) {
        click(harness, control);
    }
    pick(harness, seen, dialog, NEW_PARAMETER);
    new_parameter(harness, seen, name, default, min, max);
}

/// Clicks the control named `control` and picks `item` in the picker titled `dialog` it opens, unless that picker is
/// open already.
fn choose_at(harness: &mut Harness, seen: &mut Seen, control: &str, dialog: &str, item: &str) {
    if !is_open(harness, dialog) {
        click(harness, control);
    }
    pick(harness, seen, dialog, item);
}

/// Renames the result of the line whose line word is `word` to `name` through its menu.
fn rename(harness: &mut Harness, seen: &mut Seen, prefix: &str, word: &str, name: &str) {
    click(harness, &format!("{prefix} actions"));
    seen.look(harness);
    click(harness, &format!("Rename {word}"));
    harness.settle();
    type_and_enter(harness, name);
}

#[test]
fn a_blank_agent_model_builds_the_random_walk_with_its_lines_and_equals_the_starter() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    let starter = starter_document("random_walk.toml");
    new_blank_agents(&mut harness, "Random walk");
    agent_sheet(&mut harness, &starter, "Walker", "10000");
    add_average_measure(&mut harness, "Position");

    // Line 1: Repeat Steps per tick times, with an integer parameter made in place.
    add_agent_line(&mut harness, &mut seen, AddButton::Repeat, "Repeat", false);
    parameter_at(
        &mut harness,
        &mut seen,
        "Line 1: choose how many times",
        "Choose how many times",
        "Steps per tick",
        "4",
        "0",
        "16",
    );
    assert_eq!(line_text(&mut harness, "Line 1"), "Repeat Steps per tick times:");

    // Line 2, inside line 1: Move by a random direction of length Step size.
    click(&mut harness, "+ Add inside line 1");
    assert!(is_open(&mut harness, "Add a line inside line 1"));
    seen.look(&mut harness);
    click_in(&mut harness, "Add a line inside line 1", "Move by");
    harness.settle();
    choose_at(
        &mut harness,
        &mut seen,
        "Line 2: choose a value",
        "Choose a value",
        "a random direction of length …",
    );
    parameter_at(
        &mut harness,
        &mut seen,
        "Line 2: choose a number",
        "Choose a number",
        "Step size",
        "2",
        "0.1",
        "20",
    );
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Move by a random direction of length Step size"
    );

    let aligned = align_with_starter(sheet(&harness).document(), &starter);
    assert_eq!(
        text_and_code(&aligned),
        text_and_code(&starter),
        "the rebuild equals the random walk in canonical text and lowered code"
    );
    assert_controls_hold_their_words(&mut harness);
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);
}

/// Returns the boids starter with the parts no control of the editor builds replaced: the Velocity starts as a new
/// Arrow does, in a random direction of length 1, and the two measures, an average of an Arrow and of its length, are
/// taken out.
fn boids_as_the_editor_builds_it() -> SheetDocument {
    let mut expected = starter_document("boids.toml");
    expected.measures.clear();
    let velocity = expected.agents[0]
        .properties
        .iter_mut()
        .find(|property| property.id == "velocity")
        .expect("the Velocity");
    let mut direction = SheetLine::new(LineKind::RandomDirection);
    direction.length = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0)));
    velocity.starts_as.as_mut().expect("a start").value = Slot::Filled(SheetValue::Phrase(Box::new(direction)));
    expected
}

/// Writes boids' lines 1 and 2 on the card of the sheet in the harness, the two looks at birds.
fn build_boids_looks(harness: &mut Harness, seen: &mut Seen) {
    // Line 1: Flockmates, the average velocity of the birds within Visual range and the way to their centre.
    add_agent_line(harness, seen, AddButton::LookAround, "Look around", true);
    parameter_at(
        harness,
        seen,
        "Line 1: choose a number",
        "Choose a number",
        "Visual range",
        "50",
        "1",
        "200",
    );
    choose_at(
        harness,
        seen,
        "Line 1: choose a value",
        "Choose a value",
        "their Velocity",
    );
    click(harness, &add_summary_name("Line 1"));
    pick(harness, seen, "Choose a summary", "average of …");
    pick(harness, seen, "Choose a value", "offset to them");
    rename(harness, seen, "Line 1", "line 1", "Flockmates");
    assert_eq!(
        line_text(harness, "Line 1"),
        "Flockmates = look at birds within Visual range: average of their Velocity (average velocity), average of \
         offset to them (centre)"
    );

    // Line 2: Too close, the total push away from the birds within Protected range.
    add_agent_line(harness, seen, AddButton::LookAround, "Look around", true);
    parameter_at(
        harness,
        seen,
        "Line 2: choose a number",
        "Choose a number",
        "Protected range",
        "8",
        "0.5",
        "50",
    );
    click(harness, "Line 2: summary 1");
    seen.look(harness);
    click_last(harness, "total");
    harness.settle();
    choose_at(
        harness,
        seen,
        "Line 2: choose a value",
        "Choose a value",
        "offset away from them",
    );
    rename(harness, seen, "Line 2", "line 2", "Too close");
    assert_eq!(
        line_text(harness, "Line 2"),
        "Too close = look at birds within Protected range: total of offset away from them (push)"
    );
}

/// Writes boids' lines 3 to 5 on the card of the sheet in the harness, the three Steer lines on Velocity.
fn build_boids_steers(harness: &mut Harness, seen: &mut Seen) {
    // Lines 3 to 5: three Steer lines on Velocity.
    let steers = [
        ("Line 3", "by", "Too close's push", "Separation", "0.05", "0", "2"),
        (
            "Line 4",
            "towards",
            "Flockmates' average velocity",
            "Alignment",
            "0.05",
            "0",
            "2",
        ),
        ("Line 5", "by", "Flockmates' centre", "Cohesion", "0.0005", "0", "0.01"),
    ];
    for (prefix, mode, value, strength, default, min, max) in steers {
        add_agent_line(harness, seen, AddButton::Change, "Steer", true);
        choose_at(
            harness,
            seen,
            &format!("{prefix}: choose a property"),
            "Choose a property",
            "Velocity",
        );
        if mode == "towards" {
            click(harness, &format!("{prefix}: by or towards"));
            seen.look(harness);
            click_last(harness, "towards");
            harness.settle();
        }
        choose_at(
            harness,
            seen,
            &format!("{prefix}: choose a value"),
            "Choose a value",
            value,
        );
        parameter_at(
            harness,
            seen,
            &format!("{prefix}: choose a number"),
            "Choose a number",
            strength,
            default,
            min,
            max,
        );
        assert_eq!(
            line_text(harness, prefix),
            format!("Steer Velocity {mode} {value}, strength {strength}")
        );
    }
}

/// Writes boids' lines 6 and 7 and the line of Randomise headings on the cards of the sheet in the harness.
fn build_boids_motion(harness: &mut Harness, seen: &mut Seen) {
    // Line 6: Keep Velocity's length between Min speed and Max speed, the upper number first as the starter lists it.
    add_agent_line(harness, seen, AddButton::Change, "Keep between", true);
    choose_at(
        harness,
        seen,
        "Line 6: choose a property",
        "Choose a property",
        "Velocity",
    );
    parameter_at(
        harness,
        seen,
        "Line 6: choose the upper number",
        "Choose the upper number",
        "Max speed",
        "15",
        "1",
        "50",
    );
    parameter_at(
        harness,
        seen,
        "Line 6: choose the lower number",
        "Choose the lower number",
        "Min speed",
        "3",
        "0.5",
        "20",
    );
    assert_eq!(
        line_text(harness, "Line 6"),
        "Keep Velocity's length between Min speed and Max speed"
    );

    // Line 7: Move by my new Velocity, offered below the last change to Velocity.
    add_agent_line(harness, seen, AddButton::Change, "Move by", true);
    choose_at(
        harness,
        seen,
        "Line 7: choose a value",
        "Choose a value",
        "my new Velocity",
    );
    assert_eq!(line_text(harness, "Line 7"), "Move by my new Velocity");

    // Randomise headings: Set Velocity to a random direction of length (length of my Velocity), always.
    add_agent_line(harness, seen, AddButton::Change, "Set", false);
    choose_at(
        harness,
        seen,
        "Randomise headings line 1: choose a property",
        "Choose a property",
        "Velocity",
    );
    click(harness, "Randomise headings line 1: choose a value");
    assert!(shows(harness, OR_A_NEW_VALUE), "the value picker offers new values");
    pick(harness, seen, "Choose a value", "a random direction of length …");
    choose_at(
        harness,
        seen,
        "Randomise headings line 1: choose a number",
        "Choose a number",
        "length of my Velocity",
    );
    choose_at(
        harness,
        seen,
        "Randomise headings line 1: choose when",
        CHOOSE_WHEN,
        "always",
    );
    assert_eq!(
        line_text(harness, "Randomise headings line 1"),
        "Set Velocity to a random direction of length (length of my Velocity), always"
    );
}

#[test]
fn a_blank_agent_model_builds_boids_with_its_lines_and_equals_the_starter() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    let expected = boids_as_the_editor_builds_it();
    new_blank_agents(&mut harness, "Boids");
    agent_sheet(&mut harness, &expected, "Bird", "50000");
    add_property(&mut harness, &add_property_name("Bird"), "Velocity", ARROW_KIND);
    click(&mut harness, ADD_BUTTON);
    type_into(&mut harness, BUTTON_NAME_FIELD, "Randomise headings");
    click(&mut harness, &run_once("bird"));
    click(&mut harness, DONE);
    harness.settle();

    build_boids_looks(&mut harness, &mut seen);
    build_boids_steers(&mut harness, &mut seen);
    build_boids_motion(&mut harness, &mut seen);

    let aligned = align_with_starter(sheet(&harness).document(), &expected);
    assert_eq!(
        text_and_code(&aligned),
        text_and_code(&expected),
        "the rebuild equals boids in canonical text and lowered code"
    );
    assert_controls_hold_their_words(&mut harness);
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);
}

/// Returns the labels of the items the value picker of line `line_id` of `document` offers.
fn value_items(document: &SheetDocument, line_id: &str) -> Vec<String> {
    picker_labels(document, line_id, Blank::Value, false, DrawKind::Chance)
        .into_iter()
        .map(|(_, label)| label)
        .collect()
}

#[test]
fn my_new_velocity_is_offered_below_the_last_change_to_velocity_and_nowhere_above() {
    let boids = starter_document("boids.toml");
    // Lines 3 to 6 change Velocity. Line 7 sits below the last of them.
    for line in ["l1", "l2", "l3", "l4", "l5", "l6"] {
        assert!(
            !value_items(&boids, line).iter().any(|item| item == "my new Velocity"),
            "line {line} is not below the last change to Velocity"
        );
    }
    let below = value_items(&boids, "l7");
    assert!(below.iter().any(|item| item == "my new Velocity"), "{below:?}");
    assert!(below.iter().any(|item| item == "my Velocity"), "{below:?}");

    // A Steer added below line 7 makes it the last change, so line 7 no longer offers the new value.
    let mut builder = SheetBuilder::new(boids).with_key_seed(1);
    let mut steer = SheetLine::new(LineKind::Steer);
    steer.property = Some("velocity".to_owned());
    builder
        .apply(SheetEdit::AddLine {
            card: "bird_tick".to_owned(),
            inside: None,
            line: Box::new(steer),
        })
        .expect("the Steer adds");
    let moved = builder.into_document();
    assert!(!value_items(&moved, "l7").iter().any(|item| item == "my new Velocity"));

    // The card shows the picker's item under its name on line 7 of the starter.
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "boids.toml");
    click(&mut harness, "my new Velocity");
    assert!(is_open(&mut harness, "Choose a value"));
    assert!(shows(&mut harness, "this bird's Velocity after line 6"));
}

/// Returns the notes the card draws under line `line_id` of `document` with `report`, with their colours.
fn notes_of(
    document: &SheetDocument,
    report: &henad_graph::sheet::check::SheetReport,
    line_id: &str,
) -> Vec<(String, NoteColor)> {
    let sentences = Sentences::new(document);
    let (card, _) = document.line(line_id).expect("the line");
    card_lines(document, &sentences, report, card)
        .into_iter()
        .find(|line| line.id == line_id)
        .expect("the card draws the line")
        .notes
        .into_iter()
        .map(|note| (note.text, note.color))
        .collect()
}

/// Opens `document` in the harness and returns the notes the card draws under line `line_id`.
fn opened_notes(harness: &mut Harness, document: &SheetDocument, line_id: &str) -> Vec<(String, NoteColor)> {
    harness.open_sheet(&document.to_text().expect("the sheet writes"));
    let editor = sheet(harness);
    notes_of(editor.document(), editor.report(), line_id)
}

/// A row of a Choose line is removed by the "×" after it while two or more rows are left, one step of Undo. The last row
/// has none.
#[test]
fn a_choose_row_is_removed_by_its_cross_while_two_rows_are_left() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "ants.toml");
    let rows = |harness: &Harness| sheet(harness).document().cards[0].line_tables["l7"].rows.clone();
    let before = rows(&harness);
    assert_eq!(before.len(), 2);
    let starter = text(&harness);
    assert_eq!(named(&mut harness, &remove_choose_row_name("Line 7", 2)).len(), 1);
    click(&mut harness, &remove_choose_row_name("Line 7", 1));
    harness.settle();
    assert_eq!(rows(&harness), before[1..]);
    assert!(
        named(&mut harness, &remove_choose_row_name("Line 7", 1)).is_empty(),
        "the last row stays"
    );
    assert!(line_text(&mut harness, "Line 7").starts_with("Next cell = choose, first that applies: a random"));
    click(&mut harness, "Undo");
    assert_eq!(text(&harness), starter, "one step of Undo");
}

#[test]
fn a_property_changed_two_ways_and_a_look_inside_a_repeat_show_their_refusals_on_the_line() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let boids = starter_document("boids.toml");
    let add = |document: &SheetDocument, inside: Option<&str>, line: SheetLine| {
        let mut builder = SheetBuilder::new(document.clone()).with_key_seed(7);
        let made = builder
            .apply(SheetEdit::AddLine {
                card: "bird_tick".to_owned(),
                inside: inside.map(str::to_owned),
                line: Box::new(line),
            })
            .expect("the line adds");
        (builder.into_document(), made.into_iter().next().expect("a line id"))
    };

    // A Set of Velocity below three lines that steer it.
    let mut set = SheetLine::new(LineKind::Set);
    set.property = Some("velocity".to_owned());
    set.value = Slot::Filled(SheetValue::Chip(henad_graph::sheet::document::SheetChip::My(
        "velocity".to_owned(),
    )));
    set.condition = Slot::Filled(SheetValue::Phrase(Box::new(SheetLine::new(LineKind::Always))));
    let (with_set, set_id) = add(&boids, None, set);
    assert!(
        !check(&with_set).has_errors(),
        "the check passes, and the lowering refuses"
    );
    let refusal = steered_already("Line 3", "Velocity");
    assert_eq!(
        opened_notes(&mut harness, &with_set, &set_id),
        vec![(refusal.clone(), NoteColor::Red)]
    );
    assert!(shows(&mut harness, &refusal), "the card shows the refusal");

    // A Move to below line 7, which moves by an arrow.
    let mut move_to = SheetLine::new(LineKind::Move);
    move_to.mode = Some(henad_graph::sheet::document::LineMode::To);
    let mut ahead = SheetLine::new(LineKind::LookAtCell);
    ahead.which = Some(henad_graph::sheet::document::CellWhich::Here);
    move_to.value = Slot::Filled(SheetValue::Phrase(Box::new(ahead)));
    let (with_move, move_id) = add(&boids, None, move_to);
    let refusal = moved_by_already("Line 7");
    assert_eq!(
        opened_notes(&mut harness, &with_move, &move_id),
        vec![(refusal.clone(), NoteColor::Red)]
    );
    assert!(shows(&mut harness, &refusal));

    // A Look around inside a Repeat.
    let mut repeat = SheetLine::new(LineKind::Repeat);
    repeat.times = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(2.0)));
    let (with_repeat, repeat_id) = add(&boids, None, repeat);
    let mut look = with_repeat.line("l1").expect("Flockmates").1.clone();
    look.name = Some("Again".to_owned());
    let (inside, look_id) = add(&with_repeat, Some(&repeat_id), look);
    assert_eq!(
        opened_notes(&mut harness, &inside, &look_id),
        vec![(LOOK_IN_REPEAT.to_owned(), NoteColor::Red)]
    );
    assert!(shows(&mut harness, LOOK_IN_REPEAT));
}

#[test]
fn opening_boids_leaves_a_default_off_its_slider_steps_as_it_is() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    // Protected range keeps 8 in 0.5 to 50, off the slider's steps of 0.2 from 0.5.
    open_starter(&mut harness, "boids.toml");
    let starter = starter_document("boids.toml");
    assert_eq!(sheet(&harness).document(), &starter, "drawing the sheet edits nothing");
    assert!(!sheet(&harness).can_undo());
    assert_eq!(
        field_value(&mut harness, &value_field_name("Protected range")),
        "8",
        "the value field shows the stored default"
    );
}

/// The slider of a default off its steps takes the keyboard focus without an edit, and an Undo while it holds the
/// focus stays undone. An arrow key edits the default.
#[test]
fn a_slider_that_takes_the_focus_keeps_a_default_off_its_steps() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "boids.toml");
    let starter = starter_document("boids.toml");
    click(&mut harness, &sheet_parameter_name_field("Protected range"));
    harness.press(Key::Tab, Modifiers::NONE);
    harness.settle();
    assert_eq!(
        harness.focused,
        value_slider_name("Protected range"),
        "Tab moves to the slider"
    );
    assert_eq!(sheet(&harness).document(), &starter, "the focus alone edits nothing");
    assert!(!sheet(&harness).can_undo());

    harness.press(Key::ArrowRight, Modifiers::NONE);
    harness.settle();
    let default = |harness: &Harness| {
        let parameters = &sheet(harness).document().parameters;
        parameters
            .iter()
            .find(|parameter| parameter.name == "Protected range")
            .expect("Protected range")
            .default
    };
    assert!(default(&harness) > 8.0, "an arrow key edits the default");
    harness.press(Key::Z, Modifiers::COMMAND);
    harness.settle();
    assert_eq!(
        harness.focused,
        value_slider_name("Protected range"),
        "the slider keeps the focus"
    );
    assert_eq!(sheet(&harness).document(), &starter, "Undo restores the default");
    assert!(sheet(&harness).can_redo(), "nothing edited the default again");
}

/// Keys of a count of the ants near me.
const AGENTS_NEAR: &str = "around = \"agents_within\"\nagent_kind = \"ant\"\ndistance = 3\ninclude_me = false\n";

/// Keys of a count of the 8 cells around me.
const CELLS_AROUND: &str = "around = \"cells_8\"\n";

/// Returns the ants starter with a line 17 that counts what the keys `around` name, whose property and state `whose`
/// names, written as the table of a `whose` compare, such as `left = { their = "cargo" }`.
fn ants_with_a_count(around: &str, whose: &str) -> String {
    let text = edited(starter_text("ants.toml"), "next_line = 17", "next_line = 18");
    let text = edited(&text, "\"l12\", \"l16\"]", "\"l12\", \"l16\", \"l17\"]");
    format!(
        "{text}\n[card.line.l17]\nkind = \"count\"\nname = \"Count near\"\n{around}\n[card.line.l17.whose]\n\
         kind = \"compare\"\ntest = \"is\"\n{whose}"
    )
}

/// Returns the ants starter with a line 17 that counts the ants near me whose Cargo `whose` names, as
/// [`ants_with_a_count`] writes it.
fn ants_with_a_count_of_agents(whose: &str) -> String {
    ants_with_a_count(AGENTS_NEAR, whose)
}

/// The state of a Become and of a count of agents on a card over agents offers the agent's own states, and a compare
/// of a cell's Ground offers the ground's.
#[test]
fn a_state_picker_offers_the_ground_only_where_a_cell_is_compared() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "ants.toml");
    click_on_line(&mut harness, "Line 10", "Carrying food");
    assert!(is_open(&mut harness, "Choose a state"));
    assert!(picker_offers(&mut harness, "Choose a state", "Searching"));
    for ground in ["Open", "Obstacle", "Food", "Nest"] {
        assert!(
            !picker_offers(&mut harness, "Choose a state", ground),
            "a Become of an ant offers {ground}"
        );
    }
    harness.press(Key::Escape, Modifiers::NONE);
    click_on_line(&mut harness, "Line 9", "Food");
    assert!(is_open(&mut harness, "Choose a state"));
    assert!(picker_offers(&mut harness, "Choose a state", "Food"));
    assert!(picker_offers(&mut harness, "Choose a state", "Nest"));
    harness.press(Key::Escape, Modifiers::NONE);

    // The state of a count of agents whose property is still blank.
    assert_eq!(blank_count_states(AGENTS_NEAR), ["Searching", "Carrying food"]);
}

/// Choosing again the property a count of agents already compares keeps the state it is compared with.
#[test]
fn choosing_the_same_property_of_a_count_of_agents_keeps_its_state() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    harness.open_sheet(&ants_with_a_count_of_agents(
        "left = { their = \"cargo\" }\nright = { state = \"carrying_food\" }\n",
    ));
    let whose = |harness: &Harness| {
        let (_, line) = sheet(harness).document().line("l17").expect("line 17");
        let Some(SheetValue::Phrase(whose)) = line.whose.value() else {
            panic!("the count compares");
        };
        whose.right.clone()
    };
    assert!(matches!(whose(&harness), Slot::Filled(_)));
    click_on_line(&mut harness, "Line 17", "Cargo");
    click_in(&mut harness, "Choose a property", "Cargo");
    harness.settle();
    assert!(matches!(whose(&harness), Slot::Filled(_)), "the state stays");
}

/// Returns the states the picker of the state of the count on line 17 offers while its property is blank, for a count
/// over what the keys `around` name.
fn blank_count_states(around: &str) -> Vec<String> {
    let document = document_of(&ants_with_a_count(around, ""));
    let whose = PlaceTarget::Slot("whose.right".to_owned());
    let picker = line_picker(&document, "l17", |context| {
        let ground = takes_ground_states(context.line, &whose);
        picker_for(context, Blank::State, None, ground, false, "", DrawKind::Chance)
    });
    labels_of(&picker).into_iter().map(|(_, label)| label).collect()
}

/// A count of the cells around an ant compares their Ground, and its state picker offers the ground's states.
#[test]
fn a_count_of_the_cells_around_an_agent_offers_the_ground_states() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    harness.open_sheet(&ants_with_a_count(
        CELLS_AROUND,
        "left = { their = \"ground\" }\nright = { state = \"food\" }\n",
    ));
    assert!(!sheet(&harness).has_errors());
    click_on_line(&mut harness, "Line 17", "Food");
    assert!(is_open(&mut harness, "Choose a state"));
    for ground in ["Open", "Obstacle", "Food", "Nest"] {
        assert!(
            picker_offers(&mut harness, "Choose a state", ground),
            "a count of the cells around offers {ground}"
        );
    }
    harness.press(Key::Escape, Modifiers::NONE);
    let states = blank_count_states(CELLS_AROUND);
    for ground in ["Open", "Obstacle", "Food", "Nest"] {
        assert!(states.iter().any(|state| state == ground), "{states:?} lacks {ground}");
    }
}

/// Returns the labels of the items the picker of a `want` place on line `line_id` of `document` offers, every group
/// in order.
fn picker_items(document: &SheetDocument, line_id: &str, want: Blank) -> Vec<String> {
    let picker = line_picker(document, line_id, |context| {
        if want == Blank::Value && context.line.kind == LineKind::LookAround {
            looked_at_picker(context)
        } else {
            picker_for(context, want, None, true, false, "", DrawKind::Chance)
        }
    });
    labels_of(&picker).into_iter().map(|(_, label)| label).collect()
}

/// Returns the labels of the items the picker of a `want` blank at the slot `path` of line `line_id` of `document`
/// offers: those its place takes, then each it refuses with the reason.
fn marked_items(
    document: &SheetDocument,
    line_id: &str,
    want: Blank,
    path: &str,
) -> (Vec<String>, Vec<(String, String)>) {
    let picker = line_picker(document, line_id, |context| {
        let mut picker = picker_for(context, want, None, true, false, "", DrawKind::Chance);
        mark_refusals(&mut picker, context, &PlaceTarget::Slot(path.to_owned()));
        picker
    });
    let items: Vec<_> = picker.groups.iter().flat_map(|group| &group.items).collect();
    let taken = items
        .iter()
        .filter(|item| item.refusal.is_none())
        .map(|item| item.label.clone())
        .collect();
    let refused = items
        .iter()
        .filter_map(|item| Some((item.label.clone(), item.refusal.clone()?)))
        .collect();
    (taken, refused)
}

#[test]
fn each_agent_slot_offers_what_fits_it() {
    let boids = starter_document("boids.toml");
    let ants = starter_document("ants.toml");
    let walk = starter_document("random_walk.toml");
    let cases: [(&SheetDocument, &str, Blank, &[&str]); 9] = [
        // Steer, Keep between and Set change a number or an arrow, never a place or a direction moved in.
        (&boids, "l3", Blank::Property, &["Velocity"]),
        (&boids, "l6", Blank::Property, &["Velocity"]),
        (&boids, "l8", Blank::Property, &["Velocity"]),
        (&ants, "l11", Blank::Property, &["Reward held"]),
        // Leave and Pick a cell take a Number property of the ground, and Record an event.
        (&ants, "l3", Blank::GroundProperty, &["To food", "To home"]),
        (&ants, "l1", Blank::GroundProperty, &["To food", "To home"]),
        (&ants, "l15", Blank::Event, &["Delivery"]),
        // A summary reads each bird's properties and the relations to it.
        (
            &boids,
            "l1",
            Blank::Value,
            &[
                "their Position",
                "their Heading",
                "their Velocity",
                "offset to them",
                "offset away from them",
                "distance to them",
            ],
        ),
        // A number of times takes an integer parameter, never Step size.
        (&walk, "l1", Blank::Times, &["Steps per tick"]),
    ];
    for (document, line, want, expected) in cases {
        assert_eq!(
            picker_items(document, line, want),
            *expected,
            "{} line {line} wants {want:?}",
            document.sheet.id
        );
    }
    // Move to takes a cell picked or looked at above, and the Choose that names one. Every other value above shows
    // greyed, with the reason.
    let (taken, refused) = marked_items(&ants, "l8", Blank::Value, "value");
    assert_eq!(taken, ["Food way", "Home way", "Way", "Ahead", "Next cell"]);
    assert!(!refused.is_empty());
    for (label, reason) in &refused {
        assert!(reason.ends_with(", and Move to needs a cell."), "{label}: {reason}");
    }
    // Steer by takes the summaries above it, and line 7 the value its changes give Velocity.
    let steer = picker_items(&boids, "l3", Blank::Value);
    for part in ["Flockmates' average velocity", "Flockmates' centre", "Too close's push"] {
        assert!(steer.iter().any(|item| item == part), "{part} in {steer:?}");
    }
    // Below the ants' results and with Reward held, a Number property, a number of times still lists the integer
    // parameters alone.
    let mut ants_with_turns = ants.clone();
    ants_with_turns.parameters.push(SheetParameter::new(
        "turns",
        "Turns",
        ParameterType::WholeNumber,
        0.0,
        8.0,
        2.0,
    ));
    let mut builder = SheetBuilder::new(ants_with_turns).with_key_seed(7);
    let repeat = builder
        .apply(SheetEdit::AddLine {
            card: "ant_tick".to_owned(),
            inside: None,
            line: Box::new(SheetLine::new(LineKind::Repeat)),
        })
        .expect("the Repeat adds")
        .remove(0);
    assert_eq!(picker_items(builder.document(), &repeat, Blank::Times), ["Turns"]);
    // A number on a card over agents offers an arrow's length and a random number, a value a random direction too.
    let number = picker_items(&boids, "l8", Blank::Number);
    assert!(number.iter().any(|item| item == "length of my Velocity"), "{number:?}");
    assert!(number.iter().any(|item| item == "a random number between … and …"));
    assert!(!number.iter().any(|item| item == "a random direction of length …"));
    assert!(steer.iter().any(|item| item == "a random direction of length …"));
}

#[test]
fn the_agent_menus_and_pickers_show_no_forbidden_word_and_name_each_control_by_its_words() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "ants.toml");
    for button in AddButton::AGENTS {
        let found = named(&mut harness, button.label());
        let position = found
            .first()
            .and_then(|node| node.bounds)
            .expect("an add button")
            .center();
        harness.click_at(position);
        assert!(
            shows(&mut harness, button.heading_for(true)),
            "{} opens its menu",
            button.label()
        );
        seen.look(&mut harness);
        harness.press(Key::Escape, Modifiers::NONE);
        harness.settle();
    }
    click(&mut harness, "+ Add inside line 9");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    for (control, dialog) in [
        ("To home", "Choose a ground property"),
        ("Delivery", "Choose an event"),
        ("Line 1: most, least or random", ""),
        ("Line 6: which cell", ""),
        ("Line 8: by or to", ""),
    ] {
        let nodes = named(&mut harness, control);
        let position = nodes
            .first()
            .and_then(|node| node.bounds)
            .expect("the control")
            .center();
        harness.click_at(position);
        if !dialog.is_empty() {
            assert!(is_open(&mut harness, dialog), "{control} opens {dialog}");
        }
        seen.look(&mut harness);
        harness.press(Key::Escape, Modifiers::NONE);
        harness.settle();
    }
    click(&mut harness, "Line 1 actions");
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    open_starter(&mut harness, "boids.toml");
    for (control, dialog) in [
        ("Line 1: summary 1", ""),
        (add_summary_name("Line 1").as_str(), "Choose a summary"),
        ("their Velocity", "Choose a value"),
        ("Line 3: by or towards", ""),
        ("my new Velocity", "Choose a value"),
    ] {
        click(&mut harness, control);
        if !dialog.is_empty() {
            assert!(is_open(&mut harness, dialog), "{control} opens {dialog}");
        }
        seen.look(&mut harness);
        harness.press(Key::Escape, Modifiers::NONE);
        harness.settle();
    }
    click(&mut harness, "Line 1 actions");
    assert!(shows(&mut harness, "Include me in line 1"));
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    assert_controls_hold_their_words(&mut harness);
    assert_no_forbidden_word(&seen);
}

#[test]
fn the_ants_note_on_slower_trail_formation_stays_under_its_line_after_an_edit() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "ants.toml");
    let note = starter_document("ants.toml")
        .cards
        .iter()
        .flat_map(|card| card.line_tables.values())
        .find_map(|line| line.note.clone())
        .expect("the ants hold a note");
    assert!(shows(&mut harness, &note), "the card shows the note");
    click(&mut harness, "Line 1: most, least or random");
    click_last(&mut harness, "least");
    harness.settle();
    assert_eq!(
        line_text(&mut harness, "Line 1"),
        "Food way = the open neighbouring cell with the least To food (ties at random)"
    );
    assert!(
        shows(&mut harness, &note),
        "the note stays after an edit of another line"
    );
}

/// Opens the starter `file_name` with the number `typed` in the slot at `path` of line `line_id`, clicks that number
/// on the line whose name starts with `line`, and returns the text the Number box of the open picker holds.
fn number_box_on_typed(
    harness: &mut Harness,
    file_name: &str,
    (line_id, path): (&str, &str),
    typed: &str,
    line: &str,
) -> String {
    let mut builder = SheetBuilder::new(starter_document(file_name)).with_key_seed(1);
    builder
        .apply(SheetEdit::SetSlot {
            line: line_id.to_owned(),
            path: path.to_owned(),
            value: Slot::Filled(SheetValue::Literal(SheetLiteral::Number(
                typed.parse().expect("a number"),
            ))),
        })
        .expect("the number goes into the slot");
    harness.open_sheet(&builder.document().to_text().expect("the sheet writes"));
    open_typed(harness, typed, line)
}

/// Clicks the number `typed` on the line whose name starts with `line`, and returns the text the Number box of the
/// open picker holds.
fn open_typed(harness: &mut Harness, typed: &str, line: &str) -> String {
    click_on_line(harness, line, typed);
    harness
        .settle()
        .into_iter()
        .find(|node| node.label == NUMBER_BOX_HINT)
        .map(|node| node.value)
        .expect("the picker shows a Number box")
}

#[test]
fn a_typed_number_of_times_opens_in_the_number_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let shown = number_box_on_typed(&mut harness, "random_walk.toml", ("l1", "times"), "3", "Line 1");
    assert_eq!(shown, "3");
}

#[test]
fn a_typed_length_opens_in_the_number_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let shown = number_box_on_typed(
        &mut harness,
        "random_walk.toml",
        ("l2", "value.length"),
        "1.5",
        "Line 2",
    );
    assert_eq!(shown, "1.5");
}

#[test]
fn a_typed_distance_opens_in_the_number_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let shown = number_box_on_typed(&mut harness, "boids.toml", ("l1", "distance"), "25", "Line 1");
    assert_eq!(shown, "25");
}

#[test]
fn a_typed_strength_opens_in_the_number_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let shown = number_box_on_typed(&mut harness, "boids.toml", ("l3", "strength"), "0.05", "Line 3");
    assert_eq!(shown, "0.05");
}

#[test]
fn a_typed_minimum_opens_in_the_number_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let shown = number_box_on_typed(&mut harness, "boids.toml", ("l6", "minimum"), "2", "Line 6");
    assert_eq!(shown, "2");
}

#[test]
fn a_typed_maximum_opens_in_the_number_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let shown = number_box_on_typed(&mut harness, "boids.toml", ("l6", "maximum"), "6", "Line 6");
    assert_eq!(shown, "6");
}

#[test]
fn a_typed_amount_opens_in_the_number_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let shown = number_box_on_typed(&mut harness, "ants.toml", ("l3", "amount"), "4", "Line 3");
    assert_eq!(shown, "4");
}

#[test]
fn a_typed_number_inside_a_row_of_a_choose_opens_in_the_number_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    // Line 7 compares Way's amount with 0 inside the condition of its first row, `row.0.condition.items.0.right`.
    open_starter(&mut harness, "ants.toml");
    assert_eq!(open_typed(&mut harness, "0", "Line 7"), "0");
}

/// Moves the pointer to `position` and turns the mouse wheel by `delta` points there.
fn wheel_at(harness: &mut Harness, position: egui::Pos2, delta: f32) {
    harness.frame(vec![egui::Event::PointerMoved(position)]);
    harness.frame(vec![egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        delta: egui::vec2(0.0, delta),
        phase: egui::TouchPhase::Move,
        modifiers: Modifiers::NONE,
    }]);
}

#[test]
fn the_add_inside_menu_of_an_agent_card_fits_a_short_window_and_scrolls_to_its_last_item() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    // The Editor tab of a window 1500 by 950 points.
    harness.screen = egui::vec2(938.0, 904.0);
    open_starter(&mut harness, "random_walk.toml");
    harness.settle();
    let dialog = "Add a line inside line 1";
    click(&mut harness, "+ Add inside line 1");
    assert!(is_open(&mut harness, dialog));
    let mut reached = None;
    for _ in 0..30 {
        let nodes = harness.settle();
        let menu = nodes
            .iter()
            .find(|node| node.role == egui::accesskit::Role::Dialog && node.label == dialog)
            .and_then(|node| node.bounds)
            .expect("the menu is open");
        assert!(
            menu.bottom() <= harness.screen.y && menu.top() >= 0.0,
            "the menu fits in the window: {menu:?}"
        );
        let item = nodes
            .iter()
            .find(|node| node.label == "Depending on" && node.dialog == dialog)
            .and_then(|node| node.bounds)
            .expect("the menu lists Depending on");
        if item.bottom() <= menu.bottom() {
            reached = Some(item.center());
            break;
        }
        wheel_at(&mut harness, menu.center(), -200.0);
    }
    let position = reached.expect("the wheel brings the last item of the menu into view");
    harness.click_at(position);
    harness.settle();
    let document = sheet(&harness).document();
    let (_, repeat) = document.line("l1").expect("the Repeat");
    assert!(
        repeat.lines.iter().any(|id| document
            .line(id)
            .is_some_and(|(_, line)| line.kind == LineKind::DependingOn)),
        "the Depending on line sits inside the Repeat: {:?}",
        repeat.lines
    );
}

#[test]
fn a_repeats_add_menu_offers_its_looks_weak_with_the_reason() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "random_walk.toml");
    let dialog = "Add a line inside line 1";
    click(&mut harness, "+ Add inside line 1");
    assert!(is_open(&mut harness, dialog));
    let nodes = harness.settle();
    let item = |label: &str| {
        nodes
            .iter()
            .find(|node| node.label == label && node.dialog == dialog)
            .unwrap_or_else(|| panic!("the menu lists {label}"))
            .clone()
    };
    for label in ["Count", "Look around", "Pick a cell"] {
        assert!(item(label).disabled, "{label} cannot sit inside a Repeat");
    }
    for label in ["The cell here or ahead", "Move by", "Calculate"] {
        assert!(!item(label).disabled, "{label} can sit inside a Repeat");
    }
    let lines = sheet(&harness).document().cards[0].lines.clone();
    let count = item("Count").bounds.expect("bounds").center();
    harness.click_at(count);
    harness.settle();
    assert_eq!(
        sheet(&harness).document().cards[0].lines,
        lines,
        "a weak item adds nothing"
    );
    assert_eq!(
        sheet(&harness).document().line("l1").expect("the Repeat").1.lines,
        ["l2"],
        "a weak item adds nothing inside the Repeat"
    );

    // A When inside the Repeat holds its lines inside the Repeat too.
    harness.press(Key::Escape, Modifiers::NONE);
    apply_sheet_edit(
        sheet_mut(&mut harness),
        SheetEdit::AddLine {
            card: "walker_tick".to_owned(),
            inside: Some("l1".to_owned()),
            line: Box::new(SheetLine::new(LineKind::When)),
        },
    );
    let dialog = "Add a line inside line 3";
    click(&mut harness, "+ Add inside line 3");
    assert!(is_open(&mut harness, dialog));
    let nodes = harness.settle();
    let count = nodes
        .iter()
        .find(|node| node.label == "Count" && node.dialog == dialog)
        .expect("the When's menu lists Count");
    assert!(count.disabled, "Count cannot sit in a When inside a Repeat");
}

/// Returns the one node of `harness` named `label`, inside the dialog `dialog` when one is given, after checking that
/// it is disabled and that a pointer resting on it shows `tooltip`.
fn refused_with(harness: &mut Harness, dialog: Option<&str>, label: &str, tooltip: &str) {
    let nodes = harness.settle();
    let found: Vec<_> = nodes
        .iter()
        .filter(|node| node.label == label && dialog.is_none_or(|dialog| node.dialog == dialog))
        .collect();
    assert_eq!(found.len(), 1, "one control reads {label}: {found:?}");
    assert!(found[0].disabled, "{label} is weak");
    let centre = found[0].bounds.expect("bounds").center();
    harness.frame(vec![egui::Event::PointerMoved(centre)]);
    // A tooltip shows once the pointer has rested past egui's delay.
    for _ in 0..60 {
        harness.frame(Vec::new());
    }
    let texts = harness.accessible_texts();
    assert!(
        texts.iter().any(|text| text == tooltip),
        "{label} shows the tooltip {tooltip}: {texts:?}"
    );
}

/// Returns the accessible texts of `harness` while a pointer rests on the one node named `label`, its tooltip among
/// them.
fn texts_at_rest(harness: &mut Harness, label: &str) -> Vec<String> {
    let nodes = harness.settle();
    let found: Vec<_> = nodes.iter().filter(|node| node.label == label).collect();
    assert_eq!(found.len(), 1, "one control reads {label}: {found:?}");
    let centre = found[0].bounds.expect("bounds").center();
    harness.frame(vec![egui::Event::PointerMoved(centre)]);
    // A tooltip shows once the pointer has rested past egui's delay.
    for _ in 0..60 {
        harness.frame(Vec::new());
    }
    harness.accessible_texts()
}

/// Each "+" of a card says what it adds under the pointer, a summary's select says it removes the summary while the
/// look holds another, and the words "including me" are a select that leaves the agent out, one step of Undo.
#[test]
fn the_plus_buttons_say_what_they_add_and_including_me_is_a_select() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let hovered = [
        ("game_of_life.toml", "Line 3: add another value", ADD_VALUE_TOOLTIP),
        ("ants.toml", "Line 7: add another row", ADD_ROW_TOOLTIP),
        (
            "bounded_confidence.toml",
            "Line 1: add another summary",
            ADD_SUMMARY_TOOLTIP,
        ),
        ("boids.toml", "Line 1: summary 1", CHANGE_OR_REMOVE_SUMMARY),
    ];
    for (file, label, tooltip) in hovered {
        open_starter(&mut harness, file);
        let texts = texts_at_rest(&mut harness, label);
        assert!(
            texts.iter().any(|text| text == tooltip),
            "{label} shows {tooltip}: {texts:?}"
        );
    }
    open_starter(&mut harness, "bounded_confidence.toml");
    let texts = texts_at_rest(&mut harness, "Line 1: summary 1");
    assert!(
        !texts.iter().any(|text| text == CHANGE_OR_REMOVE_SUMMARY),
        "a look's only summary cannot be removed"
    );

    // "including me" is a select of bounded confidence's line 1, and "not including me" leaves the agent out.
    let starter = text(&harness);
    let select = "Line 1: including me";
    assert!(
        harness
            .settle()
            .iter()
            .any(|node| node.label == select && node.value == INCLUDING_ME),
        "line 1 shows its select"
    );
    click(&mut harness, select);
    click(&mut harness, NOT_INCLUDING_ME);
    let (_, line) = sheet(&harness).document().line("l1").expect("line 1");
    assert_eq!(line.include_me, Some(false));
    assert!(
        named(&mut harness, select).is_empty(),
        "a look that leaves me out reads no select"
    );
    click(&mut harness, "Undo");
    assert_eq!(text(&harness), starter, "one Undo includes me again");
    assert_controls_hold_their_words(&mut harness);
}

/// Returns whether the one node of `harness` named `label` inside the dialog `dialog` is enabled.
fn offered(harness: &mut Harness, dialog: &str, label: &str) -> bool {
    harness
        .settle()
        .iter()
        .find(|node| node.label == label && node.dialog == dialog)
        .is_some_and(|node| !node.disabled)
}

/// A value inside the formula of the ants' trail lines opens nothing: it is a label, not a button, keeps no keyboard
/// focus, says under the pointer how to change it, and a click on it opens no picker.
#[test]
fn a_value_inside_a_formula_opens_nothing_and_says_how_to_change_it() {
    use egui::accesskit::Role;

    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "ants.toml");
    let nodes = harness.settle();
    let chips: Vec<_> = nodes
        .iter()
        .filter(|node| node.value == "Trail falloff" && node.role == Role::Label && node.bounds.is_some())
        .collect();
    assert_eq!(chips.len(), 2, "lines 3 and 4 read Trail falloff in their formulas");
    assert!(
        !nodes
            .iter()
            .any(|node| node.role == Role::Button && node.label == "Trail falloff"),
        "no button reads Trail falloff"
    );
    let chip = chips[0].bounds.expect("bounds").center();
    harness.frame(vec![egui::Event::PointerMoved(chip)]);
    // A tooltip shows once the pointer has rested past egui's delay.
    for _ in 0..60 {
        harness.frame(Vec::new());
    }
    assert!(
        harness
            .accessible_texts()
            .iter()
            .any(|text| text == FORMULA_PART_TOOLTIP),
        "the chip says how to change it"
    );
    let before = text(&harness);
    harness.click_at(chip);
    let nodes = harness.settle();
    assert!(
        !nodes.iter().any(|node| node.role == Role::Dialog),
        "a click on a value inside a formula opens no picker"
    );
    assert_eq!(text(&harness), before);
    assert_ne!(harness.focused, "Trail falloff", "the chip takes no focus");
    assert_controls_hold_their_words(&mut harness);

    // The Velocity inside boids' "length of my Velocity" points at the words before it, which open its slot's picker.
    open_starter(&mut harness, "boids.toml");
    let nodes = harness.settle();
    let chip = nodes
        .iter()
        .find(|node| node.value == "my Velocity" && node.role == Role::Label && node.bounds.is_some())
        .and_then(|node| node.bounds)
        .expect("the Velocity of length of my Velocity");
    harness.frame(vec![egui::Event::PointerMoved(chip.center())]);
    for _ in 0..60 {
        harness.frame(Vec::new());
    }
    let texts = harness.accessible_texts();
    let tooltip = texts
        .iter()
        .find(|text| text.ends_with("\" to choose another value."))
        .unwrap_or_else(|| panic!("the chip points at the words before it: {texts:?}"));
    assert!(tooltip.contains("length of"), "{tooltip}");
    assert!(!texts.iter().any(|text| text == FORMULA_PART_TOOLTIP));
}

#[test]
fn the_add_menus_and_selects_grey_each_choice_the_build_refuses_with_its_reason() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    // Inside the walkers' Repeat, the changes a Repeat cannot hold are weak, and a Move to names the Move by above.
    open_starter(&mut harness, "random_walk.toml");
    let dialog = "Add a line inside line 1";
    click(&mut harness, "+ Add inside line 1");
    for label in ["Keep between", "When", "Repeat", "Leave", "Record"] {
        refused_with(&mut harness, Some(dialog), label, NOT_YET_REPEAT_CHANGE);
    }
    refused_with(&mut harness, Some(dialog), "Move to", &moved_by_already("Line 2"));
    for label in ["Move by", "Steer", "Set", "Become"] {
        assert!(offered(&mut harness, dialog, label), "{label} can sit inside a Repeat");
    }
    harness.press(Key::Escape, Modifiers::NONE);

    // On the card, Move to is weak beside the walkers' Move by, and adds nothing.
    let dialog = AddButton::Change.heading_for(true);
    click_last(&mut harness, AddButton::Change.label());
    refused_with(&mut harness, Some(dialog), "Move to", &moved_by_already("Line 2"));
    assert!(offered(&mut harness, dialog, "Move by"));
    let before = text(&harness);
    let move_to = harness
        .settle()
        .iter()
        .find(|node| node.label == "Move to" && node.dialog == dialog)
        .and_then(|node| node.bounds)
        .expect("Move to");
    harness.click_at(move_to.center());
    harness.settle();
    assert_eq!(text(&harness), before, "a weak item adds nothing");
    harness.press(Key::Escape, Modifiers::NONE);

    // Walls around walkers that move by a vector, the cell ahead of agents that never move to cells, and Move to in
    // the place of the walkers' Move by inside the Repeat.
    click(&mut harness, "Edges");
    refused_with(&mut harness, None, "Walls", NOT_YET_AGENT_WALLS);
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Line 2: by or to");
    refused_with(&mut harness, None, "to", NOT_YET_REPEAT_CHANGE);
    harness.press(Key::Escape, Modifiers::NONE);
    apply_sheet_edit(
        sheet_mut(&mut harness),
        SheetEdit::AddLine {
            card: "walker_tick".to_owned(),
            inside: None,
            line: Box::new({
                let mut here = SheetLine::new(LineKind::LookAtCell);
                here.which = Some(CellWhich::Here);
                here
            }),
        },
    );
    click(&mut harness, "Line 3: which cell");
    refused_with(&mut harness, None, "the cell ahead of me", AHEAD_NEEDS_CELLS);
    harness.press(Key::Escape, Modifiers::NONE);

    // A ground made on the sheet under walkers that move by a vector blocks no state, and a ticked box stays open.
    for edit in [
        SheetEdit::AddProperty {
            thing: "ground".to_owned(),
            name: "Terrain".to_owned(),
            property_type: PropertyType::States,
        },
        SheetEdit::AddState {
            property: "terrain".to_owned(),
            name: "Open".to_owned(),
            color: None,
        },
        SheetEdit::AddState {
            property: "terrain".to_owned(),
            name: "Rock".to_owned(),
            color: None,
        },
    ] {
        apply_sheet_edit(sheet_mut(&mut harness), edit);
    }
    refused_with(&mut harness, None, &blocked_state_name("Rock"), NOT_YET_BLOCKED_MOVE_BY);
    apply_sheet_edit(
        sheet_mut(&mut harness),
        SheetEdit::SetBlocked {
            property: "terrain".to_owned(),
            state: "rock".to_owned(),
            blocked: true,
        },
    );
    let nodes = harness.settle();
    let rock = nodes
        .iter()
        .find(|node| node.label == blocked_state_name("Rock"))
        .expect("the box of Rock");
    assert!(!rock.disabled, "a ticked box can be cleared");

    // The ants move to places behind walls over a ground that blocks a state, so a Move by is weak three ways and
    // names the Move to first.
    open_starter(&mut harness, "ants.toml");
    click_last(&mut harness, AddButton::Change.label());
    refused_with(&mut harness, Some(dialog), "Move by", &moved_to_already("Line 8"));
    assert!(offered(&mut harness, dialog, "Move to"));
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Line 8: by or to");
    refused_with(&mut harness, None, "by", NOT_YET_AGENT_WALLS);
}

#[test]
fn a_state_picker_on_an_agent_card_with_no_states_points_at_the_agents_section() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "random_walk.toml");
    add_agent_line(&mut harness, &mut seen, AddButton::Chance, "Become, with chance", false);
    if !is_open(&mut harness, "Choose a state") {
        click(&mut harness, "Line 3: choose a state");
    }
    assert!(is_open(&mut harness, "Choose a state"));
    assert!(
        shows(&mut harness, NO_AGENT_STATES),
        "the picker names the Agents section"
    );
    assert!(!shows(&mut harness, NO_STATES), "the picker names no Cells section");

    // A place that takes a state of the ground as well names both sections, and a card over cells names Cells.
    let walk = starter_document("random_walk.toml");
    let empty = |document: &SheetDocument, line: &str, ground: bool| {
        line_picker(document, line, |context| {
            picker_for(context, Blank::State, None, ground, false, "", DrawKind::Chance)
        })
        .empty
    };
    assert_eq!(empty(&walk, "l2", true), NO_AGENT_OR_GROUND_STATES);
    assert_eq!(empty(&walk, "l2", false), NO_AGENT_STATES);
    let life = starter_document("game_of_life.toml");
    assert_eq!(empty(&life, "l1", false), NO_STATES);
}

/// Removing a summary that lines read asks first and names the lines, and Remove removes it. A summary that no line
/// reads goes at once, and Undo brings it back.
#[test]
fn removing_a_summary_that_lines_read_asks_first() {
    use henad_graph::sheet::document::SummaryKind;
    use henad_graph::sheet::sentence::{CANCEL, REMOVE, REMOVE_SUMMARY, REMOVE_SUMMARY_HEADING, UNDO};

    use crate::tests::panes::{press_in_dialog, question_in};

    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "boids.toml");
    let summaries = |harness: &Harness| sheet(harness).document().line("l1").expect("line 1").1.summaries.len();
    let text = sheet(&harness).text().to_owned();

    // Line 5 steers by the centre.
    click(&mut harness, "Line 1: summary 2");
    click(&mut harness, REMOVE_SUMMARY);
    assert_eq!(
        question_in(&mut harness, REMOVE_SUMMARY_HEADING).as_deref(),
        Some("Line 5 will lose centre.")
    );
    press_in_dialog(&mut harness, REMOVE_SUMMARY_HEADING, CANCEL);
    assert!(!is_open(&mut harness, REMOVE_SUMMARY_HEADING));
    assert_eq!(sheet(&harness).text(), text, "Cancel keeps the summary");
    click(&mut harness, "Line 1: summary 2");
    click(&mut harness, REMOVE_SUMMARY);
    press_in_dialog(&mut harness, REMOVE_SUMMARY_HEADING, REMOVE);
    assert!(!is_open(&mut harness, REMOVE_SUMMARY_HEADING));
    assert_eq!(summaries(&harness), 1, "Remove removes the summary");
    click(&mut harness, UNDO);
    assert_eq!(sheet(&harness).text(), text);

    // A count of the birds around that no line reads.
    apply_sheet_edit(
        sheet_mut(&mut harness),
        SheetEdit::AddSummary {
            line: "l1".to_owned(),
            summary: SummaryKind::Count,
        },
    );
    let added = summaries(&harness);
    click(&mut harness, &format!("Line 1: summary {added}"));
    click(&mut harness, REMOVE_SUMMARY);
    assert!(!is_open(&mut harness, REMOVE_SUMMARY_HEADING));
    assert_eq!(summaries(&harness), added - 1, "the summary goes at once");
}

/// Each value of a Calculate line on a card over agents takes a part of a look above, its count among them, chosen
/// from the value's picker as a typed number is.
#[test]
fn a_calculate_line_takes_the_parts_of_the_lines_above() {
    use henad_graph::sheet::document::SummaryKind;

    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "boids.toml");
    let sheet_edit = |harness: &mut Harness, edit: SheetEdit| apply_sheet_edit(sheet_mut(harness), edit);
    sheet_edit(
        &mut harness,
        SheetEdit::AddSummary {
            line: "l1".to_owned(),
            summary: SummaryKind::Count,
        },
    );
    sheet_edit(
        &mut harness,
        SheetEdit::AddLine {
            card: "bird_tick".to_owned(),
            inside: None,
            line: Box::new(SheetLine::new(LineKind::Calculate)),
        },
    );
    let formula = |harness: &Harness| {
        let document = sheet(harness).document();
        let card = &document.cards[0];
        let last = card.lines.last().expect("the Calculate line");
        card.line_tables[last].formula.clone()
    };
    let first_blank = |harness: &mut Harness| {
        named(harness, "Line 8: choose a number")
            .iter()
            .filter_map(|node| node.bounds)
            .min_by(|a, b| a.left().total_cmp(&b.left()))
            .expect("a value's blank")
            .center()
    };
    let position = first_blank(&mut harness);
    harness.click_at(position);
    click_in(&mut harness, "Choose a number", "Flockmates' count");
    assert_eq!(formula(&harness), None, "one value waits in the draft");
    let position = first_blank(&mut harness);
    harness.click_at(position);
    type_and_enter(&mut harness, "2");
    assert_eq!(formula(&harness).as_deref(), Some("l1.count_summary + 2"));
    assert_eq!(
        line_text(&mut harness, "Line 8"),
        "Flockmates' count plus 2 = Flockmates' count plus 2"
    );
    // A written value takes a part of another look, through its chip.
    click_on_line(&mut harness, "Line 8", "2");
    click_in(&mut harness, "Choose a number", "Too close's push");
    assert_eq!(formula(&harness).as_deref(), Some("l1.count_summary + l2.push"));
}

/// Returns the problems that stop `document` lowering at line `line_id`, for a sheet that checks.
fn lowering_problems_at(document: &SheetDocument, line_id: &str) -> Vec<String> {
    use henad_graph::sheet::{ParsedSheet, SheetFile};

    let text = document.to_text().expect("the sheet writes");
    let parsed = ParsedSheet::parse(&text).expect("the sheet reads");
    SheetFile::lowering_problems(&parsed, &parsed.check())
        .into_iter()
        .filter(|problem| problem.line.as_deref() == Some(line_id))
        .map(|problem| problem.diagnostic.message)
        .collect()
}

/// Every value a picker of a slot offers enabled, a typed number where it shows a Number box included, put in the slot,
/// gives the line no new error and lowers, and every value it shows greyed gives the line an error: the picker and the
/// checker agree. A random value has its own numbers filled in, as a line with a blank never lowers.
#[test]
fn every_offered_value_the_checker_takes_and_every_greyed_one_it_refuses() {
    use henad_graph::sheet::relations::CardRelations;

    use crate::ui::editor::sheet::picker::{Choice, PickerItem, apply_choice};

    let errors_at = |document: &SheetDocument, line_id: &str| -> Vec<String> {
        check(document)
            .errors()
            .filter(|error| error.line.as_deref() == Some(line_id))
            .map(|error| error.diagnostic.message.clone())
            .collect()
    };
    let number = |value: f64| Slot::Filled(SheetValue::Literal(SheetLiteral::Number(value)));
    let mut tried = 0;
    let mut refused = 0;
    let mut lowered = 0;
    for file in ["boids.toml", "ants.toml", "sir.toml", "random_walk.toml"] {
        let document = starter_document(file);
        let sentences = Sentences::new(&document);
        for (card_index, card) in document.cards.iter().enumerate() {
            let relations = CardRelations::new(&document, card);
            for &line_id in relations.order() {
                let before = errors_at(&document, line_id);
                let otherwise = relations.otherwise_numbers(line_id);
                for placed in sentences.placed_parts(line_id, &otherwise) {
                    let Some(place) = placed.place else {
                        continue;
                    };
                    let Some(want) = place.want.filter(|want| *want != Blank::When) else {
                        continue;
                    };
                    if !matches!(&place.target, PlaceTarget::Slot(path) if !path.starts_with("summaries."))
                        && !matches!(place.target, PlaceTarget::ListItem { .. })
                    {
                        continue;
                    }
                    let picker = line_picker(&document, line_id, |context| {
                        let ground = takes_ground_states(context.line, &place.target);
                        let mut picker = picker_for(context, want, None, ground, false, "", DrawKind::Chance);
                        mark_refusals(&mut picker, context, &place.target);
                        picker
                    });
                    // A typed number stands for the Number box, which shows where the place takes one.
                    let typed = picker.number_box.as_ref().map(|_| PickerItem {
                        refusal: None,
                        ..PickerItem::new("1", Choice::Value(SheetValue::Literal(SheetLiteral::Number(1.0))))
                    });
                    for item in picker
                        .groups
                        .iter()
                        .flat_map(|group| &group.items)
                        .chain(typed.as_ref())
                    {
                        let Choice::Value(value) = &item.choice else {
                            continue;
                        };
                        let mut value = value.clone();
                        if let SheetValue::Phrase(phrase) = &mut value {
                            phrase.low = number(1.0);
                            phrase.high = number(2.0);
                            phrase.length = number(1.0);
                        }
                        let applied = apply_choice(&document, None, line_id, &place, &Choice::Value(value));
                        let mut builder = SheetBuilder::new(document.clone()).with_key_seed(1);
                        for edit in applied.edits {
                            builder.apply(edit).expect("the choice applies");
                        }
                        let after = builder.document();
                        let new: Vec<String> = errors_at(after, line_id)
                            .into_iter()
                            .filter(|error| !before.contains(error))
                            .collect();
                        let what = format!("{file} card {card_index} {line_id} {:?}: {}", place.target, item.label);
                        tried += 1;
                        refused += usize::from(item.refusal.is_some());
                        if let Some(reason) = &item.refusal {
                            assert!(!new.is_empty(), "{what} is greyed ({reason}) but checks");
                            continue;
                        }
                        assert!(new.is_empty(), "{what} is offered but gives {new:?}");
                        let report = check(after);
                        if report.has_errors() || report.is_skipped(line_id) {
                            continue;
                        }
                        lowered += 1;
                        let problems = lowering_problems_at(after, line_id);
                        assert!(
                            problems.is_empty(),
                            "{what} is offered but does not lower: {problems:?}"
                        );
                    }
                }
            }
        }
    }
    assert!(
        tried > 600 && refused > 300 && lowered > 200,
        "{tried} values tried, {refused} greyed and {lowered} lowered"
    );
}

/// A random value put in a slot opens the slot's picker from its first words, and a typed number takes its place: a
/// random number back to a number, and the length of a random direction, worked out as the length of my Velocity,
/// back to a number too.
#[test]
fn a_random_value_returns_to_a_typed_number_from_its_first_words() {
    use henad_graph::sheet::edit::slot_value;
    use henad_graph::sheet::sentence::phrase_lead_name;

    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "boids.toml");
    let value = |harness: &Harness, line: &str, path: &str| {
        let (_, line) = sheet(harness).document().line(line).expect("the line");
        slot_value(line, path).cloned()
    };
    let number = |number: f64| Some(SheetValue::Literal(SheetLiteral::Number(number)));
    click_on_line(&mut harness, "Line 6", "Min speed");
    click_in(
        &mut harness,
        "Choose the lower number",
        "a random number between … and …",
    );
    assert!(matches!(value(&harness, "l6", "minimum"), Some(SheetValue::Phrase(_))));
    let lead = phrase_lead_name("Line 6", "a random number between");
    click(&mut harness, &lead);
    assert!(
        is_open(&mut harness, "Choose the lower number"),
        "the words open the slot's picker"
    );
    type_and_enter(&mut harness, "3");
    assert_eq!(value(&harness, "l6", "minimum"), number(3.0));
    assert!(named(&mut harness, &lead).is_empty());

    let prefix = "Randomise headings line 1";
    click(&mut harness, &phrase_lead_name(prefix, "(length of"));
    assert!(is_open(&mut harness, "Choose a number"));
    type_and_enter(&mut harness, "2");
    assert_eq!(value(&harness, "l8", "value.length"), number(2.0));
    // Set Velocity takes a vector, so its picker has no Number box, and the vector a property of mine.
    click(&mut harness, &phrase_lead_name(prefix, "a random direction of length"));
    assert!(is_open(&mut harness, "Choose a value"));
    assert!(
        named(&mut harness, NUMBER_BOX_HINT).is_empty(),
        "a vector takes no typed number"
    );
    click_in(&mut harness, "Choose a value", "my Velocity");
    assert_eq!(
        value(&harness, "l8", "value"),
        Some(SheetValue::Chip(SheetChip::My("velocity".to_owned())))
    );
}

/// A symbol of its type shows before each property and parameter on the sheet and before each value of a picker, the
/// symbols the Statistics tab shows for a scalar and a vector among them, and the setting hides them all.
#[test]
fn the_type_symbols_show_before_properties_parameters_and_values_until_hidden() {
    use egui::accesskit::Role;
    use henad_graph::sheet::sentence::type_symbol_name;

    use crate::icons::material_design_icons::{MDI_ARROW_TOP_RIGHT_THIN, MDI_CIRCLE_SMALL};
    use crate::ui::editor::sheet::widgets::{TypeSymbol, show_type_symbols};

    let boids = starter_document("boids.toml");
    let picker = line_picker(&boids, "l7", |context| {
        picker_for(context, Blank::Value, None, true, false, "", DrawKind::Chance)
    });
    let symbol_of = |label: &str| {
        picker
            .groups
            .iter()
            .flat_map(|group| &group.items)
            .find(|item| item.label == label)
            .and_then(|item| item.symbol)
    };
    for (label, symbol) in [
        ("Flockmates' centre", TypeSymbol::Vector),
        ("Too close's push", TypeSymbol::Vector),
        ("my Velocity", TypeSymbol::Vector),
        ("my Position", TypeSymbol::Position),
        ("my Heading", TypeSymbol::Heading),
        ("a random number between … and …", TypeSymbol::Number),
        ("Visual range", TypeSymbol::Number),
    ] {
        assert_eq!(symbol_of(label), Some(symbol), "{label}");
    }
    assert_eq!(TypeSymbol::Number.glyph(), MDI_CIRCLE_SMALL);
    assert_eq!(TypeSymbol::Vector.glyph(), MDI_ARROW_TOP_RIGHT_THIN);

    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "boids.toml");
    let symbols = |harness: &mut Harness, kind: &str| {
        harness
            .settle()
            .iter()
            .filter(|node| node.role == Role::Image && node.label == type_symbol_name(kind))
            .count()
    };
    assert_eq!(symbols(&mut harness, "Vector"), 1, "Velocity");
    assert_eq!(symbols(&mut harness, "Position"), 1);
    assert_eq!(symbols(&mut harness, "Number"), boids.parameters.len());
    show_type_symbols(&harness.context, false);
    for kind in ["Vector", "Position", "Number"] {
        assert_eq!(symbols(&mut harness, kind), 0, "{kind} still shows");
    }
}

/// The symbol before a picker's item names its type under the pointer, and the item's words do not.
#[test]
fn a_pickers_symbol_names_its_type_under_the_pointer() {
    use henad_graph::sheet::sentence::NUMBER_KIND;

    use crate::ui::editor::sheet::widgets::OVERLAY_PADDING;

    // The symbol's column starts after the overlay's padding and the column of an item's marks, 16 points wide.
    const SYMBOL_X: f32 = OVERLAY_PADDING + 16.0 + 8.0;
    const PICKER: &str = "Choose the lower number";

    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "boids.toml");
    click_on_line(&mut harness, "Line 6", "Min speed");
    assert!(is_open(&mut harness, PICKER));
    let row = harness
        .settle()
        .into_iter()
        .find(|node| node.dialog == PICKER && node.label == "Visual range")
        .and_then(|node| node.bounds)
        .expect("the picker lists the parameter Visual range");
    let named_number = |harness: &mut Harness, pointer: egui::Pos2| {
        harness.frame(vec![egui::Event::PointerMoved(pointer)]);
        // A tooltip shows once the pointer has rested past egui's delay.
        for _ in 0..60 {
            harness.frame(Vec::new());
        }
        let texts = harness.accessible_texts();
        texts.iter().filter(|text| *text == NUMBER_KIND).count()
    };
    let on_words = named_number(&mut harness, row.center());
    let on_symbol = named_number(&mut harness, egui::pos2(row.left() + SYMBOL_X, row.center().y));
    assert_eq!(on_symbol, on_words + 1, "the symbol's tooltip names the type");
    assert!(is_open(&mut harness, PICKER), "the pointer leaves the picker open");
}
