//! Tests of writing lines on the Card pane: Game of Life, the chance SIR and the forest fire rebuilt from Blank by
//! accessible names, the list blanks of numbers and of states, a list typed in a blank of one number, a chance refused
//! in the field, shared state names, the line menu, a cleared blank's skip note, When blocks built through their add
//! rows, the offer to remove a When, moves that skip a line, the "otherwise" notice, a button's card with its notice
//! and a When, a Calculate line written through its draft, and the words rule over every menu, picker, select and note
//! opened on the way.

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use henad_graph::sheet::ParsedSheet;
use henad_graph::sheet::document::{LineKind, SheetChip, SheetDocument, SheetLiteral, SheetValue, Slot};
use henad_graph::sheet::sentence::{
    ADD_AT_END_NOTE, ADD_BUTTON, ADD_PARAMETER, ADD_PROPERTY, AddButton, BUTTON_NAME_FIELD, CHANCE_OUT_OF_RANGE,
    CHOOSE_WHEN, COLOR_CELLS_BY, CONDITIONS, DONE, DRAW_NEW_NUMBERS, LIST_HINT, LIST_IN_ONE_NUMBER, NEW_PARAMETER,
    NOTHING_INSIDE, NUMBER_BOX_HINT, REMOVE_FROM_LIST, SHARED_DRAW_GROUP, TYPE_ONE_NUMBER, add_inside_label,
    add_inside_name, run_once,
};

use henad_graph::sheet::sentence::{Blank, DrawKind, Sentences};
use henad_graph::sheet::sentence::{
    add_above_name, add_below_name, add_line_above, add_line_below, add_note_tool, duplicate_line, edit_note_tool,
    note_field_name,
};
use henad_graph::sheet::starters;

use crate::tests::panes::{
    Seen, add, add_new_outbreak, add_states_property, add_to_first, assert_controls_hold_their_words,
    assert_no_forbidden_word, bounds_of, build_life_sections, build_sir_sections, choose, click, click_in,
    click_on_line, create_parameter, document_of, enter_into, is_open, line_text, named, new_blank, open_and_pick,
    open_starter, pick, picker_labels, sheet, shows, shows_notice, start_infected_by_chance, starter_document, text,
    text_and_code, type_and_enter, type_into,
};
use crate::tests::support::{
    Harness, Node, SIR_CHANCE, align_with_starter, edited, settle_preview, starter_text, wait_for_snapshot,
};
use crate::ui::editor::sheet::widgets::{OVERLAY_PADDING, OVERLAY_SCREEN_GAP};

/// Chooses `item` in the select named `select`, after recording what its open list shows.
fn choose_seen(harness: &mut Harness, seen: &mut Seen, select: &str, item: &str) {
    click(harness, select);
    seen.look(harness);
    click(harness, item);
    harness.settle();
}

/// Asserts that Undo walks the sheet back to the text `first` and Redo forward to where it was.
fn assert_undo_walks_back_and_forth(harness: &mut Harness, first: &str) {
    let built = text(harness);
    while sheet(harness).can_undo() {
        click(harness, "Undo");
    }
    assert_eq!(text(harness), first, "undo walks back to Blank");
    while sheet(harness).can_redo() {
        click(harness, "Redo");
    }
    assert_eq!(text(harness), built, "redo walks forward to the end");
}

#[test]
fn a_blank_model_builds_game_of_life_with_its_lines_and_equals_the_starter() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    let starter = starter_document("game_of_life.toml");
    let first = build_life_sections(&mut harness, &mut seen).remove(0);

    // Line 1: a Count. Its property's picker opens by itself, then its state's.
    add(&mut harness, &mut seen, AddButton::LookAround, "Count");
    pick(&mut harness, &mut seen, "Choose a property", "Status");
    pick(&mut harness, &mut seen, "Choose a state", "Alive");
    assert_eq!(
        line_text(&mut harness, "Line 1"),
        "Alive neighbours = count of the 8 cells around me whose Status is Alive"
    );

    // Line 2: Become Alive from Dead, when Alive neighbours is 3.
    add(&mut harness, &mut seen, AddButton::Change, "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Alive");
    pick(&mut harness, &mut seen, "Choose the old state", "Dead");
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 2: choose when",
        CHOOSE_WHEN,
        "when … is …",
    );
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 2: choose a value",
        "Choose a value",
        "Alive neighbours",
    );
    click(&mut harness, "Line 2: choose a number");
    assert!(is_open(&mut harness, "Choose a number"));
    seen.look(&mut harness);
    type_and_enter(&mut harness, "3");
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Alive from Dead, when Alive neighbours is 3"
    );

    // Line 3: Become Dead from Alive, when Alive neighbours is none of the numbers 2, 3.
    add(&mut harness, &mut seen, AddButton::Change, "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Dead");
    pick(&mut harness, &mut seen, "Choose the old state", "Alive");
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 3: choose when",
        CHOOSE_WHEN,
        "when … is …",
    );
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 3: choose a value",
        "Choose a value",
        "Alive neighbours",
    );
    choose_seen(&mut harness, &mut seen, "Line 3: comparison", "is none of the numbers");
    click(&mut harness, "Line 3: choose a list of numbers");
    assert!(is_open(&mut harness, "Choose a list of numbers"));
    seen.look(&mut harness);
    type_and_enter(&mut harness, "2, 3");
    assert_eq!(
        line_text(&mut harness, "Line 3"),
        "Become Dead from Alive, when Alive neighbours is none of the numbers 2, 3"
    );

    // Clear, a button that runs its own card once, and its line.
    click(&mut harness, ADD_BUTTON);
    type_into(&mut harness, BUTTON_NAME_FIELD, "Clear");
    click(&mut harness, &run_once("cell"));
    click(&mut harness, DONE);
    harness.settle();
    add(&mut harness, &mut seen, AddButton::Change, "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Dead");
    pick(&mut harness, &mut seen, "Choose the old state", "any state");
    open_and_pick(
        &mut harness,
        &mut seen,
        "Clear line 1: choose when",
        CHOOSE_WHEN,
        "always",
    );
    assert_eq!(
        line_text(&mut harness, "Clear line 1"),
        "Become Dead from any state, always"
    );

    let aligned = align_with_starter(sheet(&harness).document(), &starter);
    assert_eq!(
        text_and_code(&aligned),
        text_and_code(&starter),
        "the rebuild equals the starter in canonical text and lowered code"
    );
    assert_controls_hold_their_words(&mut harness);
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);

    assert_undo_walks_back_and_forth(&mut harness, &first);
}

#[test]
fn a_blank_model_builds_the_chance_sir_with_its_lines_and_equals_the_sheet() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    let expected = document_of(SIR_CHANCE);
    build_sir_sections(&mut harness, &mut seen, "SIR chance", &expected, None);

    add(&mut harness, &mut seen, AddButton::Chance, "Become, with chance");
    pick(&mut harness, &mut seen, "Choose a state", "Infected");
    pick(&mut harness, &mut seen, "Choose the old state", "Susceptible");
    click(&mut harness, "Line 2: choose a chance");
    pick(&mut harness, &mut seen, "Choose a chance", NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Infection chance", "0.3", None);
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 2: for each, optional",
        "Choose what to count",
        "Infected neighbours",
    );
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Infected from Susceptible, with chance Infection chance for each infected neighbour"
    );
    assert!(
        shows(
            &mut harness,
            "Each tick, a Susceptible cell becomes Infected with chance 0.3 with 1 infected neighbour, and 0.51 with \
             2. Each one counted is an independent trial: 1 − (1 − 0.3)^n, where n is Infected neighbours."
        ),
        "a chance for each has its note"
    );

    add(&mut harness, &mut seen, AddButton::Chance, "Become, with chance");
    pick(&mut harness, &mut seen, "Choose a state", "Recovered");
    pick(&mut harness, &mut seen, "Choose the old state", "Infected");
    click(&mut harness, "Line 3: choose a chance");
    pick(&mut harness, &mut seen, "Choose a chance", NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Recovery chance", "0.05", Some("1"));

    start_infected_by_chance(&mut harness, &mut seen);
    add_new_outbreak(&mut harness, &mut seen);

    let aligned = align_with_starter(sheet(&harness).document(), &expected);
    assert_eq!(
        text_and_code(&aligned),
        text_and_code(&expected),
        "the rebuild equals the chance SIR in canonical text and lowered code"
    );
    assert_controls_hold_their_words(&mut harness);
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);
}

/// Returns the right side of the condition of line `line` of the sheet in the harness.
fn condition_right(harness: &Harness, line: &str) -> Slot {
    let (_, line) = sheet(harness).document().line(line).expect("the line");
    match line.condition.value() {
        Some(SheetValue::Phrase(phrase)) => phrase.right.clone(),
        _ => Slot::Blank,
    }
}

/// Returns the right side of the Compare line at index `index` of the first card of the sheet in the harness.
fn compare_right(harness: &Harness, index: usize) -> Slot {
    let card = &sheet(harness).document().cards[0];
    let line = card
        .lines
        .get(index)
        .and_then(|id| card.line_tables.get(id))
        .expect("the line");
    line.right.clone()
}

/// Returns the slot holding the numbers `numbers` as a list.
fn list(numbers: &[f64]) -> Slot {
    Slot::Filled(SheetValue::List(
        numbers
            .iter()
            .map(|number| SheetValue::Literal(SheetLiteral::Number(*number)))
            .collect(),
    ))
}

#[test]
fn a_list_blank_takes_two_values() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    // A filled item offers "Remove from list". Both removed, the list is a blank again.
    click_on_line(&mut harness, "Line 3", "2");
    click_in(&mut harness, "Choose a list of numbers", REMOVE_FROM_LIST);
    assert_eq!(condition_right(&harness, "l3"), list(&[3.0]));
    click_on_line(&mut harness, "Line 3", "3");
    click_in(&mut harness, "Choose a list of numbers", REMOVE_FROM_LIST);
    assert_eq!(condition_right(&harness, "l3"), Slot::Blank);
    // A line whose blank is cleared shows its skip note, and the preview still runs without it.
    assert!(shows(&mut harness, "Not run yet: choose a list of numbers"));
    assert!(!sheet(&harness).has_errors(), "a skipped line stops nothing");
    // The blank takes "2, 3" typed in one go, and "+" adds another value.
    click(&mut harness, "Line 3: choose a list of numbers");
    assert!(shows(&mut harness, LIST_HINT));
    type_and_enter(&mut harness, "2, 3");
    assert_eq!(condition_right(&harness, "l3"), list(&[2.0, 3.0]));
    click(&mut harness, "Line 3: add another value");
    type_and_enter(&mut harness, "4");
    assert_eq!(condition_right(&harness, "l3"), list(&[2.0, 3.0, 4.0]));
}

#[test]
fn a_states_blank_starts_a_list_that_takes_another_state() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    add(&mut harness, &mut seen, AddButton::YesOrNo, "Compare");
    click_in(&mut harness, "Choose a value", "my Status");
    choose(&mut harness, "Line 4: comparison", "is one of the states");
    click(&mut harness, "Line 4: choose a list of states");
    pick(&mut harness, &mut seen, "Choose a list of states", "Dead");
    let state = |id: &str| SheetValue::Chip(SheetChip::State(id.to_owned()));
    assert_eq!(
        compare_right(&harness, 3),
        Slot::Filled(SheetValue::List(vec![state("dead")])),
        "the first state starts a list"
    );
    click(&mut harness, "Line 4: add another value");
    pick(&mut harness, &mut seen, "Choose a list of states", "Alive");
    assert_eq!(
        compare_right(&harness, 3),
        Slot::Filled(SheetValue::List(vec![state("dead"), state("alive")]))
    );
    assert!(
        line_text(&mut harness, "Line 4").ends_with("my Status is one of the states Dead, Alive"),
        "{}",
        line_text(&mut harness, "Line 4")
    );
}

#[test]
fn a_single_blank_given_a_list_says_choose_is_one_of() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    let before = text(&harness);
    click_on_line(&mut harness, "Line 2", "3");
    assert!(is_open(&mut harness, "Choose a number"));
    // The Number box shows the number as it was typed.
    let nodes = harness.settle();
    assert!(
        nodes
            .iter()
            .any(|node| node.label == NUMBER_BOX_HINT && node.value == "3"),
        "the Number box reopens with 3"
    );
    type_and_enter(&mut harness, "2, 3");
    assert!(shows(&mut harness, TYPE_ONE_NUMBER) && shows(&mut harness, LIST_IN_ONE_NUMBER));
    assert_eq!(text(&harness), before, "the refused list changes nothing");
}

#[test]
fn a_chance_of_1_5_is_refused_in_the_field() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "forest_fire.toml");
    let before = text(&harness);
    click(&mut harness, "Growth chance");
    assert!(is_open(&mut harness, "Choose a chance"));
    type_and_enter(&mut harness, "1.5");
    assert!(shows(&mut harness, CHANCE_OUT_OF_RANGE));
    assert_eq!(text(&harness), before, "the refused chance changes nothing");
    type_and_enter(&mut harness, "150%");
    assert!(shows(&mut harness, CHANCE_OUT_OF_RANGE));
    assert_eq!(text(&harness), before);
    // A percent in range is kept as typed.
    type_and_enter(&mut harness, "30%");
    assert!(named(&mut harness, "30%").len() == 1, "the chance shows as typed");
}

#[test]
fn chips_and_pickers_name_the_property_when_two_share_a_state_name() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let shared = edited(
        starter_text("game_of_life.toml"),
        "[[parameter]]",
        "[[cells.property]]\nid = \"mood\"\nname = \"Mood\"\ntype = \"states\"\nstates = [\n  { id = \"calm\", name = \
         \"Calm\", color = \"#007AF5\" },\n  { id = \"alive_mood\", name = \"Alive\", color = \"#FFC107\" },\n]\n\n\
         [[parameter]]",
    );
    let document = document_of(&shared);
    harness.open_sheet(&document.to_text().expect("the sheet writes"));
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Status Alive from Dead, when Alive neighbours is 3"
    );
    let items: Vec<String> = picker_labels(sheet(&harness).document(), "l2", Blank::State, true, DrawKind::Chance)
        .into_iter()
        .map(|(_, label)| label)
        .collect();
    assert_eq!(items, ["Dead", "Status Alive", "Calm", "Mood Alive"]);
    click_on_line(&mut harness, "Line 2", "Status Alive");
    assert!(is_open(&mut harness, "Choose a state"));
    click_in(&mut harness, "Choose a state", "Mood Alive");
    let (_, line) = sheet(&harness).document().line("l2").expect("line 2");
    assert_eq!(
        (
            line.property.as_deref(),
            line.new_state.as_deref(),
            line.start_state.as_deref()
        ),
        (Some("mood"), Some("alive_mood"), None),
        "the new state's property is the line's, and an old state of another property is cleared"
    );
}

#[test]
fn the_line_menu_renames_a_result_and_deletes_a_line() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    click(&mut harness, "Line 1 actions");
    click(&mut harness, "Rename line 1");
    let nodes = harness.settle();
    assert!(nodes.iter().any(|node| node.label == "Line 1: new name"));
    type_and_enter(&mut harness, "Neighbours");
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Alive from Dead, when Neighbours is 3"
    );
    // Delete takes the line out, and its result's chips turn blank.
    click(&mut harness, "Line 1 actions");
    click(&mut harness, "Delete line 1");
    let document = sheet(&harness).document().clone();
    assert!(document.line("l1").is_none());
    assert_eq!(line_text(&mut harness, "Line 1"), "Become Alive from Dead, when … is 3");
    assert!(
        matches!(condition_right(&harness, "l2").value(), Some(SheetValue::Literal(_))),
        "the number stays"
    );
}

/// Returns the kind of the line at `index` of the top level of card `card` of the sheet in the harness.
fn kind_at(harness: &Harness, card: usize, index: usize) -> LineKind {
    let document = sheet(harness).document();
    let id = &document.cards[card].lines[index];
    document.cards[card].line_tables[id].kind
}

/// The line menu adds a line right above or below another, from a menu of every kind of line, and copies a line, each
/// one step of Undo. Below a When is after its lines, and a copy of a When copies them too.
#[test]
fn the_line_menu_adds_a_line_beside_another_and_duplicates_a_line() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "sir.toml");
    let lines = |harness: &Harness| sheet(harness).document().cards[0].lines.clone();
    let before = lines(&harness);
    let starter = text(&harness);
    click(&mut harness, "Line 1 actions");
    click(&mut harness, &add_line_below("line 1"));
    assert!(is_open(&mut harness, &add_below_name("line 1")), "the menu opens");
    click_in(&mut harness, &add_below_name("line 1"), "Set");
    harness.press(Key::Escape, Modifiers::NONE);
    let after = lines(&harness);
    assert_eq!(after.len(), 4);
    assert_eq!((&after[0], &after[2..]), (&before[0], &before[1..]));
    assert_eq!(kind_at(&harness, 0, 1), LineKind::Set);
    click(&mut harness, "Undo");
    assert_eq!(text(&harness), starter, "one step of Undo");
    click(&mut harness, "Line 1 actions");
    click(&mut harness, &add_line_above("line 1"));
    click_in(&mut harness, &add_above_name("line 1"), "Become");
    harness.press(Key::Escape, Modifiers::NONE);
    assert_eq!(lines(&harness)[1..], before[..]);
    assert_eq!(kind_at(&harness, 0, 0), LineKind::Become);
    // Escape closes the menu and adds nothing.
    click(&mut harness, "Undo");
    click(&mut harness, "Line 2 actions");
    click(&mut harness, &add_line_below("line 2"));
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!is_open(&mut harness, &add_below_name("line 2")));
    assert_eq!(text(&harness), starter);

    click(&mut harness, "Line 2 actions");
    click(&mut harness, &duplicate_line("line 2"));
    let after = lines(&harness);
    assert_eq!((after.len(), &after[..2], &after[3..]), (4, &before[..2], &before[2..]));
    let document = sheet(&harness).document();
    let (original, copy) = (
        &document.cards[0].line_tables[&after[1]],
        &document.cards[0].line_tables[&after[2]],
    );
    assert_eq!(
        (copy.kind, &copy.property, &copy.condition),
        (original.kind, &original.property, &original.condition)
    );
    assert_ne!(copy.key, original.key, "the copy draws its own numbers");
    click(&mut harness, "Undo");
    assert_eq!(text(&harness), starter, "one step of Undo");

    open_starter(&mut harness, "ants.toml");
    let before = lines(&harness);
    let when = before.iter().position(|id| *id == "l9").expect("the ants' When l9");
    click(&mut harness, "Line 9 actions");
    click(&mut harness, &add_line_below("line 9"));
    click_in(&mut harness, &add_below_name("line 9"), "Set");
    harness.press(Key::Escape, Modifiers::NONE);
    let after = lines(&harness);
    assert_eq!(after.len(), before.len() + 1);
    assert_eq!(after[when], "l9");
    assert_eq!(
        kind_at(&harness, 0, when + 1),
        LineKind::Set,
        "the new line follows the When's lines"
    );
    click(&mut harness, "Undo");
    click(&mut harness, "Line 9 actions");
    let item = named(&mut harness, &duplicate_line("line 9"));
    assert!(item.len() == 1 && item[0].role == Role::MenuItem, "{item:?}");
    click(&mut harness, &duplicate_line("line 9"));
    let document = sheet(&harness).document();
    let copy = &document.cards[0].lines[when + 1];
    assert_eq!(
        document.cards[0].line_tables[copy].lines.len(),
        2,
        "the copy holds copies of both lines"
    );
}

/// The menu that adds a line beside another offers what the line's place takes: the Change group alone on a card that
/// runs once, and every look greyed inside a Repeat, where a click adds nothing. Escape closes it and returns the focus
/// to the line's menu button.
#[test]
fn the_menu_that_adds_a_line_offers_what_its_place_takes() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let items = |harness: &mut Harness, menu: &str| -> Vec<Node> {
        harness
            .settle()
            .into_iter()
            .filter(|node| node.dialog == menu && node.role == Role::MenuItem)
            .collect()
    };
    open_starter(&mut harness, "game_of_life.toml");
    click(&mut harness, "Clear line 1 actions");
    click(&mut harness, &add_line_below("Clear line 1"));
    let menu = add_below_name("Clear line 1");
    let offered: Vec<String> = items(&mut harness, &menu).into_iter().map(|node| node.label).collect();
    assert!(offered.contains(&"Become".to_owned()), "{offered:?}");
    assert!(
        !offered.contains(&"Count".to_owned()),
        "a card that runs once takes changes alone: {offered:?}"
    );
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert!(!is_open(&mut harness, &menu));
    assert_eq!(
        harness.focused, "Clear line 1 actions",
        "Escape returns the focus to the menu button"
    );

    open_starter(&mut harness, "random_walk.toml");
    let starter = text(&harness);
    for (actions, tool, menu, inside) in [
        (
            "Line 2 actions",
            add_line_above("line 2"),
            add_above_name("line 2"),
            true,
        ),
        (
            "Line 1 actions",
            add_line_below("line 1"),
            add_below_name("line 1"),
            false,
        ),
    ] {
        click(&mut harness, actions);
        click(&mut harness, &tool);
        let count = items(&mut harness, &menu)
            .into_iter()
            .find(|node| node.label == "Count")
            .unwrap_or_else(|| panic!("{menu} offers Count"));
        assert_eq!(count.disabled, inside, "{menu}: a look is greyed inside a Repeat alone");
        if inside {
            harness.click_at(count.bounds.expect("bounds").center());
            harness.settle();
            assert_eq!(text(&harness), starter, "a greyed item adds nothing");
        }
        harness.press(Key::Escape, Modifiers::NONE);
        harness.settle();
    }
}

/// The line menu writes the note under a line: a new note, a changed one, and an empty field that removes it, each one
/// step of Undo. Escape keeps the note as it was, and a note leaves a played preview running.
#[test]
fn the_line_menu_writes_a_note_that_leaves_a_played_preview_running() {
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
    let note = |harness: &Harness| sheet(harness).document().cards[0].line_tables["l2"].note.clone();
    click(&mut harness, "Line 2 actions");
    click(&mut harness, &add_note_tool("line 2"));
    harness.settle();
    assert_eq!(harness.focused, note_field_name("Line 2"), "the field takes the focus");
    type_and_enter(&mut harness, "Birth needs exactly three.");
    assert_eq!(note(&harness).as_deref(), Some("Birth needs exactly three."));
    assert!(shows(&mut harness, "Birth needs exactly three."));
    assert_eq!(sheet(&harness).preview_key(), key, "the world keeps running");
    click(&mut harness, "Undo");
    assert_eq!(note(&harness), None);
    assert_eq!(sheet(&harness).preview_key(), key, "the undo leaves the world running");
    click(&mut harness, "Redo");
    click(&mut harness, "Line 2 actions");
    click(&mut harness, &edit_note_tool("line 2"));
    harness.replace_text("Not this.");
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert_eq!(
        note(&harness).as_deref(),
        Some("Birth needs exactly three."),
        "Escape keeps the note"
    );
    click(&mut harness, "Line 2 actions");
    click(&mut harness, &edit_note_tool("line 2"));
    type_and_enter(&mut harness, "  ");
    assert_eq!(note(&harness), None, "an empty field removes the note");
}

/// Asserts that a settled frame shows each item of `items`, an item and the name of its menu, as a menu item inside
/// that menu, and, when `dialog` names one, inside that dialog under a heading of the same name.
fn assert_menu_items(harness: &mut Harness, dialog: Option<&str>, items: &[(&str, &str)]) {
    let nodes = harness.settle();
    if let Some(dialog) = dialog {
        assert!(
            nodes
                .iter()
                .any(|node| node.role == Role::Heading && node.label == dialog && node.dialog == dialog),
            "{dialog} is headed by its title"
        );
    }
    for (item, menu) in items {
        let found = nodes
            .iter()
            .find(|node| node.label == *item && node.bounds.is_some())
            .unwrap_or_else(|| panic!("{item} is shown"));
        assert_eq!(found.role, Role::MenuItem, "{item} is a menu item");
        assert_eq!(found.menu, *menu, "{item} is in the menu {menu}");
        if let Some(dialog) = dialog {
            assert_eq!(found.dialog, dialog, "{item} is inside {dialog}");
        }
    }
}

#[test]
fn add_menus_line_menus_and_pickers_hold_menu_items_in_menus() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    click(&mut harness, AddButton::Chance.label());
    let heading = AddButton::Chance.heading();
    assert_menu_items(
        &mut harness,
        Some(heading),
        &[
            ("Become, with chance", heading),
            ("Become, at a rate", heading),
            ("With chance", SHARED_DRAW_GROUP),
            ("At a rate", SHARED_DRAW_GROUP),
        ],
    );
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Line 1 actions");
    assert_menu_items(
        &mut harness,
        None,
        &[("Rename line 1", "Line 1 actions"), ("Delete line 1", "Line 1 actions")],
    );
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Line 2: change the kind of condition");
    assert_menu_items(
        &mut harness,
        Some(CHOOSE_WHEN),
        &[
            ("when … is …", CONDITIONS),
            ("when … is at least …", CONDITIONS),
            ("with chance …", CONDITIONS),
        ],
    );
}

/// Returns each pair of menu items of the open overlay or menu named `name` in which the first item's name holds the
/// second item's name, case aside.
fn names_holding_others(harness: &mut Harness, name: &str) -> Vec<(String, String)> {
    let names: Vec<String> = harness
        .settle()
        .into_iter()
        .filter(|node| node.role == Role::MenuItem && (node.dialog == name || node.menu == name))
        .map(|node| node.label.to_lowercase())
        .collect();
    assert!(names.len() > 1, "{name} shows its menu items: {names:?}");
    names
        .iter()
        .flat_map(|holder| {
            names
                .iter()
                .filter(move |held| *held != holder && holder.contains(held.as_str()))
                .map(move |held| (holder.clone(), held.clone()))
        })
        .collect()
}

#[test]
fn no_name_in_choose_when_or_a_line_menu_holds_another() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    click(&mut harness, "Line 2: change the kind of condition");
    assert_eq!(names_holding_others(&mut harness, CHOOSE_WHEN), Vec::new());
    harness.press(Key::Escape, Modifiers::NONE);
    click(&mut harness, "Line 1 actions");
    assert_eq!(names_holding_others(&mut harness, "Line 1 actions"), Vec::new());
    harness.press(Key::Escape, Modifiers::NONE);
    add(&mut harness, &mut seen, AddButton::When, "When");
    assert!(is_open(&mut harness, CHOOSE_WHEN), "the new When's condition opens");
    assert_eq!(names_holding_others(&mut harness, CHOOSE_WHEN), Vec::new());
}

/// Opens "Choose when" on line 2 of Game of Life in a window `height` points high, and returns the bounds of the arrow
/// that opens it and of the picker.
fn choose_when_in_window(height: f32) -> Option<(egui::Rect, egui::Rect)> {
    let mut harness = Harness::new()?;
    harness.screen = egui::vec2(1400.0, height);
    open_starter(&mut harness, "game_of_life.toml");
    let arrow = "Line 2: change the kind of condition";
    let anchor = bounds_of(&harness.settle(), Role::Button, arrow);
    click(&mut harness, arrow);
    let picker = bounds_of(&harness.settle(), Role::Dialog, CHOOSE_WHEN);
    Some((anchor, picker))
}

/// Opens the menu of "+ Chance" of Game of Life in a window `height` points high, and returns the bounds of the button
/// and of the menu.
fn chance_menu_in_window(height: f32) -> Option<(egui::Rect, egui::Rect)> {
    let mut harness = Harness::new()?;
    harness.screen = egui::vec2(1400.0, height);
    open_starter(&mut harness, "game_of_life.toml");
    let button = AddButton::Chance.label();
    let anchor = bounds_of(&harness.settle(), Role::Button, button);
    click(&mut harness, button);
    let menu = bounds_of(&harness.settle(), Role::Dialog, AddButton::Chance.heading());
    Some((anchor, menu))
}

#[test]
fn a_picker_opens_below_or_above_its_anchor_and_scrolls_where_neither_fits() {
    let Some((anchor, picker)) = choose_when_in_window(1000.0) else {
        return;
    };
    assert_eq!(picker.top(), anchor.bottom(), "below the arrow, where it fits");
    let natural = picker.height();
    // On neither side does it fit. Below has more room, 240 points or more, and the picker takes that room.
    let height = anchor.bottom() + natural - 8.0;
    let (_, picker) = choose_when_in_window(height).expect("a device");
    assert!(height - OVERLAY_SCREEN_GAP - anchor.bottom() >= 240.0);
    assert_eq!(
        picker.top(),
        anchor.bottom(),
        "below the arrow, the side with more room"
    );
    assert_eq!(
        picker.bottom(),
        height - OVERLAY_SCREEN_GAP,
        "as tall as the room below"
    );
    // Neither side has 240 points. The picker takes the window's height from its top.
    let height = anchor.bottom() + 200.0;
    let (_, picker) = choose_when_in_window(height).expect("a device");
    assert_eq!(picker.top(), OVERLAY_SCREEN_GAP, "at the top of the window");
    assert!(
        picker.bottom() <= height - OVERLAY_SCREEN_GAP,
        "inside the window: {picker:?}"
    );
    // A menu that does not fit below its button but fits above it opens above it. The menu's dialog is the area
    // inside its frame.
    let frame = OVERLAY_PADDING + 1.0;
    let (anchor, menu) = chance_menu_in_window(1000.0).expect("a device");
    assert_eq!(menu.top() - frame, anchor.bottom(), "below the button, where it fits");
    let height = anchor.bottom() + menu.height() + frame;
    let (anchor, menu) = chance_menu_in_window(height).expect("a device");
    assert_eq!(menu.bottom() + frame, anchor.top(), "above the button");
}

/// Opens "Choose when" on line 2 of Game of Life in a window `height` points high, presses ↑ to focus its last item,
/// and returns the bounds of the arrow before and after, of the picker and of the focused item.
fn last_item_by_key(height: f32) -> Option<[egui::Rect; 4]> {
    let mut harness = Harness::new()?;
    harness.screen = egui::vec2(1400.0, height);
    open_starter(&mut harness, "game_of_life.toml");
    let arrow = "Line 2: change the kind of condition";
    let before = bounds_of(&harness.settle(), Role::Button, arrow);
    click(&mut harness, arrow);
    harness.settle();
    harness.press(Key::ArrowUp, Modifiers::NONE);
    let nodes = harness.settle();
    assert_eq!(harness.focused, "at rate … per …", "↑ moves to the last item");
    let item = nodes
        .iter()
        .find(|node| node.label == harness.focused)
        .and_then(|node| node.bounds)
        .expect("the focused item");
    let picker = bounds_of(&nodes, Role::Dialog, CHOOSE_WHEN);
    let after = bounds_of(&nodes, Role::Button, arrow);
    Some([before, after, picker, item])
}

#[test]
fn a_focused_item_scrolls_into_its_picker_and_the_card_stays_put() {
    let Some([before, _, picker, _]) = last_item_by_key(1000.0) else {
        return;
    };
    let natural = picker.height();
    // The picker fits under the arrow and runs over the list under the panes. The card under it stays put.
    let height = before.bottom() + natural + 20.0;
    let [before, after, picker, item] = last_item_by_key(height).expect("a device");
    assert_eq!(before, after, "the card stays put");
    assert!(picker.contains_rect(item));
    // The picker scrolls on the side with more room, and brings the item into view.
    let height = before.bottom() + natural - 8.0;
    let [before, after, picker, item] = last_item_by_key(height).expect("a device");
    assert_eq!(before, after, "the card stays put");
    assert!(picker.contains(item.center()), "{item:?} shows in {picker:?}");
}

#[test]
fn escape_returns_the_focus_from_a_select() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    click(&mut harness, "Edges");
    harness.settle();
    harness.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(harness.focused, "Wrap around", "↓ moves into the select's list");
    harness.press(Key::Escape, Modifiers::NONE);
    assert_eq!(harness.focused, "Edges");
}

#[test]
fn arrow_keys_move_round_an_open_menu_and_escape_returns_to_its_button() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    let button = AddButton::Chance.label();
    click(&mut harness, button);
    harness.settle();
    let items = ["Become, with chance", "Become, at a rate", "With chance", "At a rate"];
    for item in items.iter().chain(&items[..1]) {
        harness.press(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(harness.focused, *item, "ArrowDown moves to {item}");
    }
    harness.press(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(harness.focused, "At a rate", "ArrowUp moves round to the last item");
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!is_open(&mut harness, AddButton::Chance.heading()));
    assert_eq!(harness.focused, button, "Escape returns the focus to the button");

    click(&mut harness, "Line 1 actions");
    harness.settle();
    harness.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(harness.focused, "Rename line 1");
    harness.press(Key::Escape, Modifiers::NONE);
    assert_eq!(harness.focused, "Line 1 actions");

    let arrow = "Line 2: change the kind of condition";
    click(&mut harness, arrow);
    harness.settle();
    harness.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(harness.focused, "always");
    harness.press(Key::ArrowUp, Modifiers::NONE);
    harness.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(harness.focused, "always", "the keys stay inside the picker");
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!is_open(&mut harness, CHOOSE_WHEN));
    assert_eq!(harness.focused, arrow);
}

/// Returns whether the node that holds the keyboard focus in a settled frame is a menu item of the open picker
/// titled `dialog`.
fn focus_is_an_item_of(harness: &mut Harness, dialog: &str) -> bool {
    let nodes = harness.settle();
    nodes
        .iter()
        .any(|node| node.label == harness.focused && node.role == Role::MenuItem && node.dialog == dialog)
}

#[test]
fn the_number_box_keeps_the_arrow_keys_and_its_use_button_passes_them_to_the_items() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    let picker = "Choose a number";
    for key in [Key::ArrowDown, Key::ArrowUp] {
        click_on_line(&mut harness, "Line 2", "3");
        assert!(is_open(&mut harness, picker));
        assert_eq!(harness.focused, NUMBER_BOX_HINT, "the Number box takes the focus");
        harness.press(key, Modifiers::NONE);
        assert_eq!(harness.focused, NUMBER_BOX_HINT, "{key:?} moves the text cursor");
        harness.press(Key::Tab, Modifiers::NONE);
        assert_eq!(harness.focused, "Use");
        harness.press(key, Modifiers::NONE);
        assert!(
            focus_is_an_item_of(&mut harness, picker),
            "{key:?} on Use moves to an item of the picker, not to {}",
            harness.focused
        );
        harness.press(Key::Escape, Modifiers::NONE);
        assert!(!is_open(&mut harness, picker));
    }
}

/// An empty Number box passes the arrow keys to the picker's items, down to the first and up to the last. Once it holds
/// text, the keys move its text cursor.
#[test]
fn an_empty_number_box_passes_the_arrow_keys_to_the_items() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    let picker = "Choose a value";
    for (key, item) in [(Key::ArrowDown, "Alive neighbours"), (Key::ArrowUp, NEW_PARAMETER)] {
        click_on_line(&mut harness, "Line 2", "Alive neighbours");
        assert!(is_open(&mut harness, picker));
        assert_eq!(harness.focused, NUMBER_BOX_HINT, "the Number box takes the focus");
        harness.press(key, Modifiers::NONE);
        assert!(focus_is_an_item_of(&mut harness, picker), "{key:?} moves to an item");
        assert_eq!(harness.focused, item, "{key:?} from the empty box");
        harness.press(Key::Escape, Modifiers::NONE);
        assert!(!is_open(&mut harness, picker));
    }
    click_on_line(&mut harness, "Line 2", "Alive neighbours");
    harness.frame(vec![egui::Event::Text("4".to_owned())]);
    for key in [Key::ArrowDown, Key::ArrowUp] {
        harness.press(key, Modifiers::NONE);
        assert_eq!(
            harness.focused, NUMBER_BOX_HINT,
            "{key:?} moves the text cursor of a box with text"
        );
    }
    harness.press(Key::Escape, Modifiers::NONE);
}

/// A picker whose Number box comes last, the value left of a comparison, focuses the box on open too.
#[test]
fn a_picker_with_its_number_box_last_focuses_the_box() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    click_on_line(&mut harness, "Line 2", "Alive neighbours");
    assert!(is_open(&mut harness, "Choose a value"));
    assert_eq!(harness.focused, NUMBER_BOX_HINT, "the Number box takes the focus");
    type_and_enter(&mut harness, "4");
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Alive from Dead, when 4 is 3",
        "the typed number fills the value"
    );

    // A picker that scrolls keeps its focused box in view under its list.
    open_starter(&mut harness, "ants.toml");
    click_on_line(&mut harness, "Line 11", "Site reward");
    let nodes = harness.settle();
    assert_eq!(harness.focused, NUMBER_BOX_HINT, "the Number box takes the focus");
    let picker = bounds_of(&nodes, Role::Dialog, "Choose a value");
    let field = nodes
        .iter()
        .find(|node| node.role == Role::TextInput && node.label == NUMBER_BOX_HINT)
        .and_then(|node| node.bounds)
        .expect("the Number box");
    assert!(
        picker.contains_rect(field),
        "the box {field:?} shows in the picker {picker:?}"
    );
}

#[test]
fn a_tooltip_shows_on_keyboard_focus_but_not_while_a_menu_is_open() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    let button = AddButton::Chance.label();
    let tooltip = AddButton::Chance.tooltip();
    click(&mut harness, button);
    harness.settle();
    harness.press(Key::Escape, Modifiers::NONE);
    assert_eq!(harness.focused, button);
    assert!(shows(&mut harness, tooltip), "the focused button shows its tooltip");
    harness.press(Key::Enter, Modifiers::NONE);
    assert!(is_open(&mut harness, AddButton::Chance.heading()));
    assert!(!shows(&mut harness, tooltip), "an open menu hides the tooltip");
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(shows(&mut harness, tooltip));
    // A press of the pointer ends the keyboard's turn. The button keeps the focus and shows no tooltip.
    click(&mut harness, button);
    click(&mut harness, button);
    assert!(!is_open(&mut harness, AddButton::Chance.heading()));
    assert_eq!(harness.focused, button);
    assert!(!shows(&mut harness, tooltip), "the pointer's focus shows no tooltip");
}

#[test]
fn the_new_model_button_shows_its_tooltip_on_keyboard_focus() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    for _ in 0..40 {
        if harness.focused == "New model" {
            break;
        }
        harness.press(Key::Tab, Modifiers::NONE);
    }
    assert_eq!(harness.focused, "New model", "Tab reaches New model");
    assert!(shows(&mut harness, "Start new model"));
}

#[test]
fn a_click_on_a_control_under_a_focus_tooltip_reaches_the_control() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    // In a window this narrow, "+ When, for several lines" wraps under "+ Chance", and the tooltip of "+ Chance"
    // covers part of it.
    harness.screen = egui::vec2(1000.0, 1000.0);
    open_starter(&mut harness, "game_of_life.toml");
    let button = AddButton::Chance.label();
    click(&mut harness, button);
    harness.settle();
    harness.press(Key::Escape, Modifiers::NONE);
    let nodes = harness.settle();
    let tooltip = nodes
        .iter()
        .find(|node| node.value == AddButton::Chance.tooltip())
        .and_then(|node| node.bounds)
        .expect("the tooltip of the focused button");
    let under = bounds_of(&nodes, Role::Button, AddButton::When.label()).intersect(tooltip);
    assert!(under.is_positive(), "the tooltip covers part of the When button");
    harness.click_at(under.center());
    assert!(
        is_open(&mut harness, AddButton::When.heading()),
        "the click opens the menu of the button under the tooltip"
    );
}

#[test]
fn focus_moves_to_the_next_blank_and_only_state_blanks_open_by_themselves() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    // A new line's first blank opens, and a click on it keeps it open.
    add_to_first(&mut harness, &mut seen, AddButton::Change, "Set");
    assert!(is_open(&mut harness, "Choose a number property"));
    click(&mut harness, "Line 4: choose a number property");
    assert!(
        is_open(&mut harness, "Choose a number property"),
        "a click keeps it open"
    );
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!is_open(&mut harness, "Choose a number property"));
    assert_eq!(
        harness.focused, "Line 4: choose a number property",
        "Escape returns the focus"
    );
    // In a compare, the value leads to the comparison select, then the number, each without opening.
    add(&mut harness, &mut seen, AddButton::YesOrNo, "Compare");
    assert!(is_open(&mut harness, "Choose a value"));
    click_in(&mut harness, "Choose a value", "Alive neighbours");
    assert!(!is_open(&mut harness, "Choose a number"));
    assert_eq!(harness.focused, "Line 5: comparison");
    choose(&mut harness, "Line 5: comparison", "is at least");
    assert_eq!(harness.focused, "Line 5: choose a number");
    assert!(!is_open(&mut harness, "Choose a number"));
    // The words "for each" are a button once on, and the empty optional blank is in the tree.
    add(&mut harness, &mut seen, AddButton::Chance, "With chance");
    assert!(is_open(&mut harness, "Choose a chance"));
    harness.press(Key::Escape, Modifiers::NONE);
    assert!(!named(&mut harness, "Line 6: for each, optional").is_empty());
}

#[test]
fn a_chance_result_is_named_happens_until_its_chance_names_it() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "forest_fire.toml");
    add(&mut harness, &mut seen, AddButton::Chance, "With chance");
    assert_eq!(line_text(&mut harness, "Line 6"), "Happens = with chance …");
    assert!(is_open(&mut harness, "Choose a chance"), "the new line's chance opens");
    type_and_enter(&mut harness, "30%");
    assert_eq!(
        line_text(&mut harness, "Line 6"),
        "30% chance happens = with chance 30%"
    );
    click(&mut harness, "Line 6 actions");
    click(&mut harness, "Rename line 6");
    type_and_enter(&mut harness, "Burning neighbours");
    assert!(
        shows(&mut harness, "Another result is already called Burning neighbours."),
        "a taken name is refused under the field"
    );
    type_and_enter(&mut harness, "Spark");
    assert_eq!(line_text(&mut harness, "Line 6"), "Spark = with chance 30%");
}

#[test]
fn a_negative_rate_is_refused_in_the_field() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "sir.toml");
    let before = text(&harness);
    click(&mut harness, "Recovery rate");
    assert!(is_open(&mut harness, "Choose a rate"));
    type_and_enter(&mut harness, "-1");
    assert!(shows(&mut harness, henad_graph::sheet::sentence::RATE_BELOW_ZERO));
    assert_eq!(text(&harness), before);
}

#[test]
fn the_one_side_offer_waits_while_the_range_is_being_filled() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "vote.toml");
    click(&mut harness, "Line 2: change the kind of condition");
    pick(
        &mut harness,
        &mut seen,
        CHOOSE_WHEN,
        "when … is less than … or more than …",
    );
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 2: choose a value",
        "Choose a value",
        "B in my block",
    );
    click(&mut harness, "Line 2: choose the lower number");
    type_and_enter(&mut harness, "2");
    // The upper number opens by itself after the lower, and the offer waits.
    assert!(is_open(&mut harness, "Choose the upper number"));
    assert!(!shows(&mut harness, henad_graph::sheet::sentence::ONE_SIDED_RANGE));
    // Closing the picker with no value ends the wait.
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    assert!(shows(&mut harness, henad_graph::sheet::sentence::ONE_SIDED_RANGE));
    seen.look(&mut harness);
    click(&mut harness, "Line 2: use is less than 2 instead");
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become B from any state, when B in my block is less than 2"
    );
    assert_no_forbidden_word(&seen);
}

#[test]
fn a_count_chosen_as_a_chance_offers_to_count_it_for_each() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    harness.open_sheet(SIR_CHANCE);
    click(&mut harness, "Recovery chance");
    click_in(&mut harness, "Choose a chance", "Infected neighbours");
    let note = "Infected neighbours is a count, not a chance. Did you mean a chance for each infected neighbour?";
    assert!(shows(&mut harness, note));
    click(&mut harness, "Line 3: use a chance for each infected neighbour");
    assert_eq!(
        line_text(&mut harness, "Line 3"),
        "Become Recovered from Infected, with chance … for each infected neighbour"
    );
    assert!(
        is_open(&mut harness, "Choose a chance"),
        "the emptied chance opens its picker"
    );
}

#[test]
fn changed_by_and_the_controls_of_the_rebuilt_lines_follow_the_wording_table() {
    use crate::ui::editor::sheet::card_pane::card_lines;

    let Some(mut harness) = Harness::tall() else {
        return;
    };
    harness.open_sheet(SIR_CHANCE);
    assert!(
        shows(&mut harness, "lines 2, 3"),
        "Changed by lists the lines that change Health"
    );
    click(&mut harness, "Line 3 actions");
    click(&mut harness, "Delete line 3");
    assert!(shows(&mut harness, "line 2"), "Changed by follows a deleted line");
    assert_controls_hold_their_words(&mut harness);
    let editor = sheet(&harness);
    let document = editor.document();
    let sentences = Sentences::new(document);
    assert!(
        card_lines(document, &sentences, editor.report(), 0)[1]
            .controls()
            .iter()
            .any(|control| control.name == "Line 2: change or remove for each"),
        "the words for each are a control once on"
    );
}

// Card structure: When blocks, moves and "otherwise"

/// Returns the ids of the lines inside line `line` of the sheet in the harness.
fn inside(harness: &Harness, line: &str) -> Vec<String> {
    let (_, line) = sheet(harness).document().line(line).expect("the line");
    line.lines.clone()
}

/// Returns the depth of line `line` on the first card of the sheet in the harness, as the card indents it.
fn depth(harness: &Harness, line: &str) -> usize {
    use crate::ui::editor::sheet::card_pane::card_lines;

    let editor = sheet(harness);
    let document = editor.document();
    card_lines(document, &Sentences::new(document), editor.report(), 0)
        .into_iter()
        .find(|found| found.id == line)
        .map(|found| found.depth)
        .expect("the line is on the card")
}

/// Builds the sheet of the forest fire starter from Blank in the harness, every section but the card's lines: Cover
/// with Empty, Tree and Burning, the trees by chance, two parameters, three measures and Replant.
fn build_forest_fire_sheet(harness: &mut Harness, seen: &mut Seen, starter: &SheetDocument) {
    new_blank(harness, "Forest fire");
    enter_into(harness, "Width in cells", "1024");
    enter_into(harness, "Height in cells", "1024");
    add_states_property(harness, ADD_PROPERTY, "Cover", &["Empty", "Tree", "Burning"]);
    click(harness, "State 1 colour: Dark grey");
    click(harness, "Black");
    click(harness, DONE);
    harness.settle();
    choose(harness, "Cover starts as", "By chance");
    click(harness, "Cover starts as, row 1: use a parameter");
    click(harness, NEW_PARAMETER);
    create_parameter(harness, seen, "Initial trees", "0.5", None);
    click(harness, ADD_PARAMETER);
    create_parameter(harness, seen, "Growth chance", "0.01", Some("0.1"));
    for state in ["Empty", "Tree", "Burning"] {
        click(harness, &format!("Plot {state}"));
    }
    click(harness, "Colour of Empty: Black");
    click(harness, "Grey");
    harness.settle();
    choose(harness, COLOR_CELLS_BY, "Cover");
    type_into(harness, "Purpose", starter.sheet.purpose.as_deref().unwrap_or_default());
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    click(harness, ADD_BUTTON);
    type_into(harness, BUTTON_NAME_FIELD, "Replant");
    choose(harness, "Start again for", "Cover");
    click(harness, DONE);
    harness.settle();
}

#[test]
fn a_blank_model_builds_the_forest_fire_with_otherwise_and_equals_the_starter() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    let starter = starter_document("forest_fire.toml");

    build_forest_fire_sheet(&mut harness, &mut seen, &starter);

    // Line 1: Burning neighbours over the 4 cells around me.
    add(&mut harness, &mut seen, AddButton::LookAround, "Count");
    pick(&mut harness, &mut seen, "Choose a property", "Cover");
    pick(&mut harness, &mut seen, "Choose a state", "Burning");
    choose_seen(&mut harness, &mut seen, "Line 1: which cells", "the 4 cells around me");
    assert_eq!(
        line_text(&mut harness, "Line 1"),
        "Burning neighbours = count of the 4 cells around me whose Cover is Burning"
    );

    // Line 2: Become Burning from Tree, when Burning neighbours is at least 1.
    add(&mut harness, &mut seen, AddButton::Change, "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Burning");
    pick(&mut harness, &mut seen, "Choose the old state", "Tree");
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 2: choose when",
        CHOOSE_WHEN,
        "when … is at least …",
    );
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 2: choose a value",
        "Choose a value",
        "Burning neighbours",
    );
    click(&mut harness, "Line 2: choose a number");
    type_and_enter(&mut harness, "1");
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Burning from Tree, when Burning neighbours is at least 1"
    );

    // Line 3: a second Become from Tree reads "otherwise", and the first time it does a notice says why.
    add(&mut harness, &mut seen, AddButton::Chance, "Become, with chance");
    pick(&mut harness, &mut seen, "Choose a state", "Burning");
    pick(&mut harness, &mut seen, "Choose the old state", "Tree");
    let notice = "Line 3 now starts with otherwise. Line 2 also changes Cover for a cell that is Tree. When both \
                  apply, line 2 wins.";
    assert!(
        shows_notice(&mut harness, notice),
        "the notice explains the new otherwise"
    );
    seen.look(&mut harness);
    click(&mut harness, "Line 3: choose a chance");
    pick(&mut harness, &mut seen, "Choose a chance", NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Lightning chance", "0.00001", Some("0.001"));
    assert_eq!(
        line_text(&mut harness, "Line 3"),
        "otherwise (when line 2 does not apply) become Burning from Tree, with chance Lightning chance"
    );
    // The notice shows once. The next edit takes it away.
    assert!(!harness.accessible_texts().iter().any(|shown| shown.contains(notice)));

    // Line 4: Become Empty from Burning, always. Line 5: Become Tree from Empty, with chance Growth chance.
    add(&mut harness, &mut seen, AddButton::Change, "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Empty");
    pick(&mut harness, &mut seen, "Choose the old state", "Burning");
    open_and_pick(&mut harness, &mut seen, "Line 4: choose when", CHOOSE_WHEN, "always");
    add(&mut harness, &mut seen, AddButton::Chance, "Become, with chance");
    pick(&mut harness, &mut seen, "Choose a state", "Tree");
    pick(&mut harness, &mut seen, "Choose the old state", "Empty");
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 5: choose a chance",
        "Choose a chance",
        "Growth chance",
    );

    // "Draw new numbers" gives line 3 a new key.
    let key = |harness: &Harness| sheet(harness).document().line("l3")?.1.key;
    let before = key(&harness);
    click(&mut harness, "Line 3 actions");
    seen.look(&mut harness);
    click(&mut harness, DRAW_NEW_NUMBERS);
    assert_ne!(key(&harness), before, "line 3 draws new numbers");

    let aligned = align_with_starter(sheet(&harness).document(), &starter);
    assert_eq!(
        text_and_code(&aligned),
        text_and_code(&starter),
        "the rebuild equals the starter in canonical text and lowered code"
    );
    assert_controls_hold_their_words(&mut harness);
    // Line 3 starts with otherwise and keeps it after the alignment.
    assert!(line_text(&mut harness, "Line 3").starts_with("otherwise (when line 2 does not apply)"));
    assert_no_forbidden_word(&seen);
}

#[test]
fn the_run_once_condition_picker_offers_no_rate() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "game_of_life.toml");
    click(&mut harness, "Clear line 1: choose when, now always");
    assert!(is_open(&mut harness, CHOOSE_WHEN));
    let texts = harness.accessible_texts();
    assert!(texts.iter().any(|text| text == "with chance …"), "a chance is offered");
    assert!(
        !texts.iter().any(|text| text.contains("at rate")),
        "no rate is offered: {texts:?}"
    );
    // The card of a button offers "+ Change" alone, with no When.
    let nodes = harness.settle();
    let when_buttons = nodes
        .iter()
        .filter(|node| node.label == AddButton::When.label())
        .count();
    assert_eq!(when_buttons, 1, "only the card that runs each tick offers a When");
}

#[test]
fn a_button_card_explains_its_otherwise_and_its_when_offers_changes_alone() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    // Clear line 2, under Clear line 1's "from any state", reads "otherwise", and the notice names it by its prefix.
    add(&mut harness, &mut seen, AddButton::Change, "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Alive");
    pick(&mut harness, &mut seen, "Choose the old state", "Dead");
    assert!(line_text(&mut harness, "Clear line 2").starts_with("otherwise (when line 1 does not apply)"));
    assert!(
        shows_notice(
            &mut harness,
            "Clear line 2 now starts with otherwise. Line 1 also changes Status for a cell that is Dead. When both \
             apply, line 1 wins."
        ),
        "the notice explains the otherwise of a button's card"
    );
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);

    // A When on a button's card, which only a written file can hold, offers changes alone inside it.
    let when = "lines = [\"l5\"]\n\n[card.line.l5]\nkind = \"when\"\nlines = [\"l4\"]\n\n[card.line.l5.condition]\n\
                kind = \"compare\"\ntest = \"is\"\nleft = { my = \"status\" }\nright = { state = \"alive\" }\n\n# Become \
                Dead from any state";
    let text = edited(
        &edited(starter_text("game_of_life.toml"), "next_line = 5", "next_line = 6"),
        "lines = [\"l4\"]\n\n# Become Dead from any state",
        when,
    );
    assert!(ParsedSheet::parse(&text).is_ok_and(|parsed| parsed.check().diagnostics.is_empty()));
    harness.open_sheet(&text);
    assert_eq!(line_text(&mut harness, "Clear line 1"), "When my Status is Alive:");
    click(&mut harness, &add_inside_label(1));
    let nodes = harness.settle();
    let menu = bounds_of(&nodes, Role::Dialog, &add_inside_name(1));
    let texts: Vec<&str> = nodes
        .iter()
        .filter(|node| node.bounds.is_some_and(|rect| menu.contains(rect.center())))
        .flat_map(|node| [node.label.as_str(), node.value.as_str()])
        .collect();
    assert!(texts.contains(&AddButton::Change.group()), "{texts:?}");
    assert!(texts.contains(&"Become"), "{texts:?}");
    for group in [
        AddButton::LookAround,
        AddButton::Chance,
        AddButton::YesOrNo,
        AddButton::Calculate,
        AddButton::When,
    ] {
        assert!(
            !texts.contains(&group.group()),
            "{} is offered: {texts:?}",
            group.group()
        );
    }
    harness.press(Key::Escape, Modifiers::NONE);
    assert_controls_hold_their_words(&mut harness);
}

/// Adds line 4 of Game of Life, "When my Status is Dead:", through "+ When, for several lines", and asserts that its
/// "Choose when" opens by itself with no "always".
fn add_dead_when(harness: &mut Harness, seen: &mut Seen) {
    add_to_first(harness, seen, AddButton::When, "When");
    assert!(is_open(harness, CHOOSE_WHEN), "a new When's condition opens by itself");
    seen.look(harness);
    assert!(!shows(harness, "always"), "a When cannot be always");
    click_in(harness, CHOOSE_WHEN, "When … is …");
    harness.settle();
    open_and_pick(harness, seen, "Line 4: choose a value", "Choose a value", "my Status");
    open_and_pick(harness, seen, "Line 4: choose a state", "Choose a state", "Dead");
}

#[test]
fn a_nested_when_stores_its_parent_and_deleting_the_outer_when_keeps_its_lines() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    let starter_text = text(&harness);

    add_dead_when(&mut harness, &mut seen);
    assert_eq!(line_text(&mut harness, "Line 4"), "When my Status is Dead:");
    assert!(shows(&mut harness, NOTHING_INSIDE), "an empty When says so");
    assert!(shows(&mut harness, ADD_AT_END_NOTE));

    // Line 5, inside line 4: When Alive neighbours is at least 3.
    assert_controls_hold_their_words(&mut harness);
    click(&mut harness, &add_inside_label(4));
    assert!(is_open(&mut harness, &add_inside_name(4)));
    seen.look(&mut harness);
    click_in(&mut harness, &add_inside_name(4), "When");
    harness.settle();
    click_in(&mut harness, CHOOSE_WHEN, "When … is at least …");
    harness.settle();
    open_and_pick(
        &mut harness,
        &mut seen,
        "Line 5: choose a value",
        "Choose a value",
        "Alive neighbours",
    );
    click(&mut harness, "Line 5: choose a number");
    type_and_enter(&mut harness, "3");
    assert_eq!(
        line_text(&mut harness, "Line 5"),
        "When Alive neighbours is at least 3:"
    );
    assert!(shows(&mut harness, "Only when line 4 holds: my Status is Dead."));

    // Line 6, inside line 5: a Become starts always, from the state of the nearest When around it testing
    // "my Status is …", which is line 4. Line 5 tests a count.
    click(&mut harness, &add_inside_label(5));
    seen.look(&mut harness);
    click_in(&mut harness, &add_inside_name(5), "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Alive");
    // Line 2 also makes a Dead cell Alive when it has 3 Alive neighbours, so line 6 reads "otherwise".
    assert_eq!(
        line_text(&mut harness, "Line 6"),
        "otherwise (when line 2 does not apply) become Alive from Dead, always"
    );
    assert!(shows(
        &mut harness,
        "Only when line 5 holds: Alive neighbours is at least 3."
    ));
    click(&mut harness, "Line 6: choose when, now always");
    assert!(shows(
        &mut harness,
        "Line 5 already has a condition. Choose always to use only that condition."
    ));
    seen.look(&mut harness);
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();

    assert_eq!(inside(&harness, "l5"), ["l6"]);
    assert_eq!(inside(&harness, "l6"), ["l7"]);
    assert_eq!(
        (depth(&harness, "l5"), depth(&harness, "l6"), depth(&harness, "l7")),
        (0, 1, 2)
    );
    // Line 5 holds line 6 alone, which holds always, so line 5 offers to move its condition there.
    assert!(shows(
        &mut harness,
        "Only line 6 is inside. Move this condition into that line's own when."
    ));
    assert!(!named(&mut harness, "Line 5: move it into line 6").is_empty());
    assert_controls_hold_their_words(&mut harness);

    // Deleting the outer When keeps its lines one level up, and Undo puts it back.
    click(&mut harness, "Line 4 actions");
    seen.look(&mut harness);
    click(&mut harness, "Delete line 4");
    assert!(sheet(&harness).document().line("l5").is_none());
    assert_eq!(inside(&harness, "l6"), ["l7"]);
    assert_eq!((depth(&harness, "l6"), depth(&harness, "l7")), (0, 1));
    assert_eq!(
        line_text(&mut harness, "Line 4"),
        "When Alive neighbours is at least 3:"
    );
    click(&mut harness, "Undo");
    assert_eq!(inside(&harness, "l5"), ["l6"]);
    assert_eq!((depth(&harness, "l6"), depth(&harness, "l7")), (1, 2));

    // Undo walks back to the starter.
    while sheet(&harness).can_undo() {
        click(&mut harness, "Undo");
    }
    assert_eq!(text(&harness), starter_text);
    assert_no_forbidden_word(&seen);
}

#[test]
fn a_when_that_repeats_its_lines_offers_to_remove_itself() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    add_dead_when(&mut harness, &mut seen);
    click(&mut harness, &add_inside_label(4));
    click_in(&mut harness, &add_inside_name(4), "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Alive");
    assert!(shows(
        &mut harness,
        "Line 5 already says from Dead, so this When repeats it."
    ));
    seen.look(&mut harness);
    assert_controls_hold_their_words(&mut harness);
    click(&mut harness, "Line 4: remove this When and keep the lines inside");
    assert!(sheet(&harness).document().line("l5").is_none());
    assert_eq!(
        line_text(&mut harness, "Line 4"),
        "otherwise (when line 2 does not apply) become Alive from Dead, always"
    );
    assert_eq!(depth(&harness, "l6"), 0);
    assert_no_forbidden_word(&seen);
}

#[test]
fn moving_a_line_above_its_result_skips_it_and_moving_it_back_runs_it() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    let starter_text = text(&harness);
    assert!(shows(
        &mut harness,
        "Lines 2 and 3 change different cells (Dead and Alive), so their order does not matter."
    ));
    click(&mut harness, "Line 2 actions");
    seen.look(&mut harness);
    click(&mut harness, "Move line 2 up");
    assert_eq!(
        line_text(&mut harness, "Line 1"),
        "Become Alive from Dead, when Alive neighbours is 3"
    );
    assert!(shows(&mut harness, "Uses Alive neighbours, which is below this line"));
    assert!(sheet(&harness).report().is_skipped("l2"), "the moved line is skipped");
    assert!(!sheet(&harness).has_errors(), "a skipped line stops nothing");
    click(&mut harness, "Line 1 actions");
    seen.look(&mut harness);
    click(&mut harness, "Move line 1 down");
    assert!(!shows(&mut harness, "Uses Alive neighbours, which is below this line"));
    assert!(!sheet(&harness).report().is_skipped("l2"), "the line runs again");
    assert_eq!(text(&harness), starter_text);
    assert_no_forbidden_word(&seen);
}

/// Returns the words the operation select of line 4 shows.
fn operation_of_line_4(harness: &mut Harness) -> String {
    named(harness, "Line 4: operation")
        .first()
        .map(|node| node.value.clone())
        .expect("the operation select")
}

/// Returns the formula of the Calculate line `l5` of the sheet in the harness.
fn formula_of_l5(harness: &Harness) -> Option<String> {
    let (_, line) = sheet(harness).document().line("l5").expect("the Calculate line");
    line.formula.clone()
}

#[test]
fn a_calculate_line_keeps_its_values_as_a_draft_until_both_are_chosen() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    let mut seen = Seen::default();
    open_starter(&mut harness, "game_of_life.toml");
    add(&mut harness, &mut seen, AddButton::Calculate, "Calculate");
    let empty = text(&harness);
    // The first value and the operation wait in the line's draft, and the sheet stays as it was. A new line's first
    // blank opens by itself.
    assert!(is_open(&mut harness, "Choose a number"));
    type_and_enter(&mut harness, "2");
    choose(&mut harness, "Line 4: operation", "times");
    assert_eq!(text(&harness), empty, "a half-chosen line changes nothing");
    assert_eq!(operation_of_line_4(&mut harness), "times");
    // A new parameter in the second value writes the formula from the draft.
    let second = named(&mut harness, "Line 4: choose a number")
        .iter()
        .filter_map(|node| node.bounds)
        .max_by(|a, b| a.left().total_cmp(&b.left()))
        .expect("the second value's blank")
        .center();
    harness.click_at(second);
    pick(&mut harness, &mut seen, "Choose a number", NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Factor", "3", None);
    assert_eq!(formula_of_l5(&harness).as_deref(), Some("2 * factor"));
    assert_eq!(line_text(&mut harness, "Line 4"), "2 times Factor = 2 times Factor");
    // One undo takes the parameter and the formula back, and drops the draft.
    harness.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(text(&harness), empty, "the parameter and the formula are one step");
    assert_eq!(operation_of_line_4(&mut harness), "plus");
    harness.press(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT);
    assert_eq!(formula_of_l5(&harness).as_deref(), Some("2 * factor"));
    // The written line's operation and values edit the formula.
    choose(&mut harness, "Line 4: operation", "minus");
    assert_eq!(formula_of_l5(&harness).as_deref(), Some("2 - factor"));
    click_on_line(&mut harness, "Line 4", "2");
    type_and_enter(&mut harness, "5");
    assert_eq!(formula_of_l5(&harness).as_deref(), Some("5 - factor"));
    assert_no_forbidden_word(&seen);
}

/// The first group label of an overlay shares the hairline under the overlay's title. It sits as far under the title
/// as a later group label sits under the item before it, each with one hairline between.
#[test]
fn a_group_right_under_an_overlay_title_takes_no_second_hairline() {
    let Some(mut harness) = Harness::tall() else {
        return;
    };
    open_starter(&mut harness, "random_walk.toml");
    click(&mut harness, &add_inside_label(1));
    let title = add_inside_name(1);
    let nodes = harness.settle();
    let inside = |role: Role| {
        let mut found: Vec<egui::Rect> = nodes
            .iter()
            .filter(|node| node.role == role && node.dialog == title)
            .filter_map(|node| node.bounds)
            .collect();
        found.sort_by(|a, b| a.top().total_cmp(&b.top()));
        found
    };
    let heading = inside(Role::Heading);
    let labels = inside(Role::Label);
    let items = inside(Role::MenuItem);
    assert_eq!(heading.len(), 1, "{heading:?}");
    assert!(labels.len() >= 2, "two groups at least: {labels:?}");
    let under_title = labels[0].top() - heading[0].bottom();
    let item_before = items
        .iter()
        .rev()
        .find(|item| item.bottom() <= labels[1].top())
        .expect("an item of the first group");
    let under_item = labels[1].top() - item_before.bottom();
    assert!(
        (under_title - under_item).abs() < 0.5,
        "{under_title} under the title, {under_item} under an item"
    );
}
