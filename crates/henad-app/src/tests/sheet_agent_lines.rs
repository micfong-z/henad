//! Tests of writing the lines of a card over agents: the random walk and boids rebuilt from a blank model of agents by
//! accessible names, "my new Velocity" offered only below the last change to Velocity, the refusals of a property
//! changed two ways and of a look inside a Repeat shown on their lines, and the words rule over every agent menu and
//! picker opened on the way.

use egui::{Event, Key, Modifiers};
use henad_graph::sheet::check::check;
use henad_graph::sheet::document::{
    LineKind, ParameterType, SheetDocument, SheetLine, SheetLiteral, SheetParameter, SheetValue, Slot,
};
use henad_graph::sheet::edit::{SheetBuilder, SheetEdit};
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    ADD_BUTTON, ADD_MEASURE, AGENTS_IN_OPEN_SPACE, ARROW_KIND, AVERAGE_MEASURE, AddButton, BUTTON_NAME_FIELD, Blank,
    CHOOSE_WHEN, COLOR_AGENTS_BY, CREATE_PARAMETER, DONE, DrawKind, LOOK_IN_REPEAT, MEASURE_PROPERTY_NAME,
    NEW_PARAMETER, OR_A_NEW_VALUE, Sentences, add_property_name, add_summary_name, moved_by_already, run_once,
    steered_already, value_field_name,
};

use super::sheet_card::{
    Seen, TALL_SCREEN, assert_controls_hold_their_words, assert_no_forbidden_word, click, click_in, click_last,
    enter_into, is_open, line_text, named, open_starter, pick, sheet, shows, starter_document, text_and_code,
    type_into,
};
use super::sheet_pane::field_value;
use crate::tests::support::{Harness, align_with_starter};
use crate::ui::editor::sheet::card_pane::{NoteColor, card_lines};
use crate::ui::editor::sheet::picker::{LineContext, looked_at_picker, picker_for};

/// Opens the New model dialog, names the model `name`, and creates a blank model of agents.
fn new_blank_agents(harness: &mut Harness, name: &str) {
    click(harness, "New model");
    harness.click_name_field();
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text(name.to_owned())]);
    click(harness, AGENTS_IN_OPEN_SPACE);
    click(harness, "Create");
    harness.settle();
}

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

/// Clicks the control named `control`, picks "New parameter..." in the picker titled `dialog` it opens, and creates
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
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text(name.to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
}

/// Sets the world to 1000 by 1000, names the agents `agents` with `count` of them at the start, colours them by
/// heading and types the purpose of `starter`.
fn agent_sheet(harness: &mut Harness, starter: &SheetDocument, agents: &str, count: &str) {
    enter_into(harness, "Width in cells", "1000");
    enter_into(harness, "Height in cells", "1000");
    enter_into(harness, "Name of the agents", agents);
    enter_into(harness, "Number of agents at the start", count);
    click(harness, COLOR_AGENTS_BY);
    let heading = named(harness, "Heading")
        .into_iter()
        .rfind(|node| node.role != egui::accesskit::Role::Label)
        .and_then(|node| node.bounds)
        .expect("the Heading colour");
    harness.click_at(heading.center());
    harness.settle();
    type_into(harness, "Purpose", starter.sheet.purpose.as_deref().unwrap_or_default());
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
}

#[test]
fn a_blank_agent_model_builds_the_random_walk_with_its_lines_and_equals_the_starter() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let mut seen = Seen::default();
    let starter = starter_document("random_walk.toml");
    new_blank_agents(&mut harness, "Random walk");
    agent_sheet(&mut harness, &starter, "Walker", "10000");
    click(&mut harness, ADD_MEASURE);
    click(&mut harness, AVERAGE_MEASURE);
    click(&mut harness, MEASURE_PROPERTY_NAME);
    let position = named(&mut harness, "Position")
        .into_iter()
        .rfind(|node| node.role != egui::accesskit::Role::Label)
        .and_then(|node| node.bounds)
        .expect("the Position item");
    harness.click_at(position.center());
    click(&mut harness, DONE);
    harness.settle();

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
        "a random direction of length ...",
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
    click(&mut harness, "Help: lines");
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
    pick(harness, seen, "Choose a summary", "average of ...");
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
    pick(harness, seen, "Choose a value", "a random direction of length ...");
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let mut seen = Seen::default();
    let expected = boids_as_the_editor_builds_it();
    new_blank_agents(&mut harness, "Boids");
    agent_sheet(&mut harness, &expected, "Bird", "50000");
    click(&mut harness, &add_property_name("Bird"));
    type_into(&mut harness, "Property name", "Velocity");
    click(&mut harness, ARROW_KIND);
    click(&mut harness, DONE);
    harness.settle();
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
    click(&mut harness, "Help: lines");
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);
}

/// Returns the labels of the items the value picker of line `line_id` of `document` offers.
fn value_items(document: &SheetDocument, line_id: &str) -> Vec<String> {
    let sentences = Sentences::new(document);
    let (card, line) = document.line(line_id).expect("the line");
    let relations = CardRelations::new(document, &document.cards[card]);
    let context = LineContext {
        document,
        sentences: &sentences,
        relations: &relations,
        line_id,
        line,
    };
    picker_for(&context, Blank::Value, None, false, "", DrawKind::Chance)
        .groups
        .iter()
        .flat_map(|group| group.items.iter().map(|item| item.label.clone()))
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    open_starter(&mut harness, "boids.toml");
    click(&mut harness, "my new Velocity");
    assert!(is_open(&mut harness, "Choose a value"));
    assert!(shows(&mut harness, "this bird's Velocity after line 6"));
}

/// Returns the notes the card draws under line `line_id` of the sheet `text`, with their colours.
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

#[test]
fn a_property_changed_two_ways_and_a_look_inside_a_repeat_show_their_refusals_on_the_line() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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

/// Returns the labels of the items the picker of a `want` place on line `line_id` of `document` offers, every group
/// in order.
fn picker_items(document: &SheetDocument, line_id: &str, want: Blank) -> Vec<String> {
    let sentences = Sentences::new(document);
    let (card, line) = document.line(line_id).expect("the line");
    let relations = CardRelations::new(document, &document.cards[card]);
    let context = LineContext {
        document,
        sentences: &sentences,
        relations: &relations,
        line_id,
        line,
    };
    let picker = if want == Blank::Value && line.kind == LineKind::LookAround {
        looked_at_picker(&context)
    } else {
        picker_for(&context, want, None, false, "", DrawKind::Chance)
    };
    picker
        .groups
        .iter()
        .flat_map(|group| group.items.iter().map(|item| item.label.clone()))
        .collect()
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
    // Move to takes a cell picked or looked at above, and the Choose that names one.
    assert_eq!(
        picker_items(&ants, "l8", Blank::Value),
        ["Food way", "Home way", "Way", "Ahead", "Next cell"]
    );
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
    assert!(number.iter().any(|item| item == "a random number between ... and ..."));
    assert!(!number.iter().any(|item| item == "a random direction of length ..."));
    assert!(steer.iter().any(|item| item == "a random direction of length ..."));
}

#[test]
fn the_agent_menus_and_pickers_show_no_forbidden_word_and_name_each_control_by_its_words() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
