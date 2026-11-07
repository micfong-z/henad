//! Tests of writing lines on the Card pane: Game of Life and the chance SIR rebuilt from Blank by accessible names,
//! the list blanks of numbers and of states, a list typed in a blank of one number, a chance refused in the field,
//! shared state names, the line menu, a cleared blank's skip note, and the words rule over every menu, picker, select
//! and help popover opened on the way.

use egui::accesskit::Role;
use egui::{Event, Key, Modifiers};
use henad_graph::sheet::document::{SheetChip, SheetDocument, SheetLiteral, SheetValue, Slot};
use henad_graph::sheet::sentence::{
    ADD_BUTTON, ADD_PROPERTY, ADD_STATE, AddButton, BUTTON_NAME_FIELD, CHANCE_OUT_OF_RANGE, CHOOSE_WHEN,
    COLOR_CELLS_BY, CREATE_PARAMETER, DONE, LIST_HINT, LIST_IN_ONE_NUMBER, NEW_PARAMETER, NUMBER_BOX_HINT,
    REMOVE_FROM_LIST, TYPE_ONE_NUMBER, run_once, start_again_for,
};
use henad_graph::sheet::{ParsedSheet, SheetFile, starters};

use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{Blank, DrawKind, Sentences};

use crate::tests::support::{Harness, Node, align_with_starter, forbidden_words};
use crate::ui::editor::sheet::SheetEditor;
use crate::ui::editor::sheet::picker::{LineContext, picker_for};

/// Height in points of a screen that holds every section of a long sheet and every line of its card.
const TALL_SCREEN: egui::Vec2 = egui::vec2(1800.0, 2600.0);

/// Text of the chance SIR, a copy of the sheet henad-graph's and henad-models' tests read.
const SIR_CHANCE: &str = include_str!("sheets/sir_chance.toml");

/// Sheet in the harness's Editor.
fn sheet(harness: &Harness) -> &SheetEditor {
    harness.app.editor.model.sheet().expect("a sheet")
}

/// Returns the canonical text of the sheet in the harness's Editor.
fn text(harness: &Harness) -> String {
    sheet(harness).text().to_owned()
}

/// Returns the nodes of a settled frame named exactly `name`, top to bottom.
fn named(harness: &mut Harness, name: &str) -> Vec<Node> {
    let mut found: Vec<Node> = harness
        .settle()
        .into_iter()
        .filter(|node| node.label == name && node.bounds.is_some())
        .collect();
    found.sort_by(|a, b| top_of(a).total_cmp(&top_of(b)));
    found
}

/// Returns the top of `node` on the screen.
fn top_of(node: &Node) -> f32 {
    node.bounds.map_or(0.0, |rect| rect.top())
}

/// Clicks the one widget named exactly `name`.
fn click(harness: &mut Harness, name: &str) {
    let found = named(harness, name);
    assert_eq!(found.len(), 1, "one widget is named {name}: {found:?}");
    let position = found[0].bounds.expect("bounds").center();
    harness.click_at(position);
}

/// Clicks the last of the widgets named exactly `name`, the one lowest on the screen.
fn click_last(harness: &mut Harness, name: &str) {
    let found = named(harness, name);
    let last = found.last().unwrap_or_else(|| panic!("no widget is named {name}"));
    let position = last.bounds.expect("bounds").center();
    harness.click_at(position);
}

/// Returns the bounds of the one node of role `role` whose name starts with `name`.
fn bounds_of(nodes: &[Node], role: Role, name: &str) -> egui::Rect {
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.role == role && node.label.starts_with(name) && node.bounds.is_some())
        .collect();
    assert_eq!(found.len(), 1, "one {role:?} is named {name}: {found:?}");
    found[0].bounds.expect("bounds")
}

/// Clicks the widget named exactly `item` inside the open picker or menu titled `dialog`.
fn click_in(harness: &mut Harness, dialog: &str, item: &str) {
    let nodes = harness.settle();
    let area = bounds_of(&nodes, Role::Dialog, dialog);
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.label == item && node.bounds.is_some_and(|rect| area.contains(rect.center())))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "one widget of {dialog} is named {item}: {found:?} in {area:?} among {:?}",
        nodes
            .iter()
            .filter(|node| node.label == item || node.label == dialog)
            .collect::<Vec<_>>()
    );
    let position = found[0].bounds.expect("bounds").center();
    harness.click_at(position);
}

/// Clicks the widget named exactly `item` on the line whose accessible name starts with `line`.
fn click_on_line(harness: &mut Harness, line: &str, item: &str) {
    let nodes = harness.settle();
    let area = bounds_of(&nodes, Role::Group, line);
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.label == item && node.bounds.is_some_and(|rect| area.contains(rect.center())))
        .collect();
    assert_eq!(found.len(), 1, "one widget of {line} is named {item}: {found:?}");
    let position = found[0].bounds.expect("bounds").center();
    harness.click_at(position);
}

/// Returns whether a picker or menu titled `dialog` is open.
fn is_open(harness: &mut Harness, dialog: &str) -> bool {
    harness
        .settle()
        .iter()
        .any(|node| node.role == Role::Dialog && node.label == dialog)
}

/// Types `text` into the field that holds the focus and presses Enter.
fn type_and_enter(harness: &mut Harness, text: &str) {
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text(text.to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
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

/// Chooses `item` in the select named `select`, after recording what its open list shows.
fn choose_seen(harness: &mut Harness, seen: &mut Seen, select: &str, item: &str) {
    click(harness, select);
    seen.look(harness);
    click(harness, item);
    harness.settle();
}

/// Asserts that every control of every line of the sheet in the harness is drawn under its accessible name, and that
/// the name of each control but a select holds the words it shows.
fn assert_controls_hold_their_words(harness: &mut Harness) {
    use crate::ui::editor::sheet::card_pane::card_lines;

    let nodes = harness.settle();
    let editor = sheet(harness);
    let document = editor.document();
    let sentences = Sentences::new(document);
    for index in 0..document.cards.len() {
        for line in card_lines(document, &sentences, editor.report(), index) {
            for control in line.controls() {
                assert!(
                    nodes.iter().any(|node| node.label == control.name),
                    "{} has no control named {}",
                    line.label(),
                    control.name
                );
                let words = control.visible.trim_end_matches("...").trim().to_lowercase();
                assert!(
                    control.select || control.name.to_lowercase().contains(&words),
                    "{} does not hold {}",
                    control.name,
                    control.visible
                );
            }
        }
    }
}

/// Returns whether a settled frame shows `text` as a label, a value or a description.
fn shows(harness: &mut Harness, text: &str) -> bool {
    harness.accessible_texts().iter().any(|shown| shown == text)
}

/// Returns the sentence of line `number` of the card pane's first card, read from its accessible name.
fn line_text(harness: &mut Harness, prefix: &str) -> String {
    let nodes = harness.settle();
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.role == Role::Group && node.label.starts_with(&format!("{prefix}: ")))
        .collect();
    assert_eq!(found.len(), 1, "one line is named {prefix}: {found:?}");
    found[0].label[prefix.len() + 2..].to_owned()
}

/// Every text a settled frame shows, gathered while menus, pickers and help popovers are open.
#[derive(Debug, Default)]
struct Seen(Vec<String>);

impl Seen {
    /// Adds every text the harness shows now.
    fn look(&mut self, harness: &mut Harness) {
        self.0.extend(harness.accessible_texts());
    }
}

/// Opens the add button `button`'s menu under the lowest card, records what it shows, and chooses `item`.
fn add(harness: &mut Harness, seen: &mut Seen, button: AddButton, item: &str) {
    click_last(harness, button.label());
    choose_add_item(harness, seen, button, item);
}

/// Opens the add button `button`'s menu under the first card, records what it shows, and chooses `item`.
fn add_to_first(harness: &mut Harness, seen: &mut Seen, button: AddButton, item: &str) {
    let found = named(harness, button.label());
    let position = found
        .first()
        .and_then(|node| node.bounds)
        .expect("an add button")
        .center();
    harness.click_at(position);
    choose_add_item(harness, seen, button, item);
}

/// Records what the open menu of `button` shows, and chooses `item`.
fn choose_add_item(harness: &mut Harness, seen: &mut Seen, button: AddButton, item: &str) {
    assert!(is_open(harness, button.heading()) || shows(harness, button.heading()));
    seen.look(harness);
    click_last(harness, item);
    harness.settle();
}

/// Picks `item` in the open picker titled `dialog`, after recording what it shows.
fn pick(harness: &mut Harness, seen: &mut Seen, dialog: &str, item: &str) {
    assert!(is_open(harness, dialog), "{dialog} is open");
    seen.look(harness);
    click_in(harness, dialog, item);
    harness.settle();
}

/// Clicks the control named `control` and picks `item` in the picker titled `dialog` it opens.
fn open_and_pick(harness: &mut Harness, seen: &mut Seen, control: &str, dialog: &str, item: &str) {
    click(harness, control);
    pick(harness, seen, dialog, item);
}

/// Returns the canonical text and the lowered code of `document`.
fn text_and_code(document: &SheetDocument) -> (String, String) {
    let text = document.to_text().expect("the sheet writes");
    let code = SheetFile::parse(&text)
        .expect("the sheet lowers")
        .lowered()
        .text()
        .to_owned();
    (text, code)
}

/// Returns the document of `text`.
fn document_of(text: &str) -> SheetDocument {
    ParsedSheet::parse(text).expect("the sheet reads").into_document()
}

/// Returns the document of the starter whose file is named `file_name`.
fn starter_document(file_name: &str) -> SheetDocument {
    let starter = starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .expect("the starter");
    document_of(starter.text)
}

/// Opens the New model dialog, names the model `name`, and creates a blank model.
fn new_blank(harness: &mut Harness, name: &str) {
    click(harness, "New model");
    harness.click_name_field();
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text(name.to_owned())]);
    click(harness, "Create");
    harness.settle();
}

/// Adds a States property `name` with `states` through the property form, each state in its usual or default colour.
fn add_states_property(harness: &mut Harness, name: &str, states: &[&str]) {
    click(harness, ADD_PROPERTY);
    type_into(harness, "Property name", name);
    for (index, state) in states.iter().enumerate() {
        if index >= 2 {
            click(harness, ADD_STATE);
        }
        type_into(harness, &format!("State {} name", index + 1), state);
    }
}

/// Creates a parameter in the open New parameter dialog, with the default `default` and the top `max` when given.
fn create_parameter(harness: &mut Harness, seen: &mut Seen, name: &str, default: &str, max: Option<&str>) {
    seen.look(harness);
    type_into(harness, "Parameter name", name);
    type_into(harness, "Default value", default);
    if let Some(max) = max {
        type_into(harness, "Max", max);
    }
    click(harness, CREATE_PARAMETER);
    harness.settle();
}

/// Returns every text of `seen` that holds a forbidden word, with the word.
fn assert_no_forbidden_word(seen: &Seen) {
    let found = forbidden_words(&seen.0);
    assert!(found.is_empty(), "forbidden words: {found:?}");
}

/// Builds the sheet of the Game of Life starter from Blank in the harness, every section but the card's lines and
/// Clear, and returns the text of the blank model.
fn build_life_sheet(harness: &mut Harness, seen: &mut Seen, starter: &SheetDocument) -> String {
    new_blank(harness, "Game of Life");
    let first = text(harness);
    enter_into(harness, "Width in cells", "1024");
    enter_into(harness, "Height in cells", "1024");
    add_states_property(harness, "Status", &["Dead", "Alive"]);
    click(harness, "State 2 colour: Blue");
    click(harness, "Green");
    click(harness, DONE);
    harness.settle();
    choose(harness, "Status starts as", "By chance");
    click(harness, "Status starts as, row 1: use a parameter");
    click(harness, NEW_PARAMETER);
    create_parameter(harness, seen, "Initial density", "0.3", None);
    click(harness, "Plot Alive");
    choose(harness, COLOR_CELLS_BY, "Status");
    type_into(harness, "Purpose", starter.sheet.purpose.as_deref().unwrap_or_default());
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    click(harness, ADD_BUTTON);
    type_into(harness, BUTTON_NAME_FIELD, "Randomise");
    choose(harness, "Start again for", "Status");
    click(harness, DONE);
    harness.settle();
    assert!(shows(harness, &start_again_for("Status")));
    first
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let mut seen = Seen::default();
    let starter = starter_document("game_of_life.toml");
    let first = build_life_sheet(&mut harness, &mut seen, &starter);

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
        "when ... is ...",
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
        "when ... is ...",
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
    click(&mut harness, "Help: lines");
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);

    assert_undo_walks_back_and_forth(&mut harness, &first);
}

#[test]
fn a_blank_model_builds_the_chance_sir_with_its_lines_and_equals_the_sheet() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let mut seen = Seen::default();
    // The id comes from the name given in the New model dialog, and the toolbar renames the model keeping it.
    new_blank(&mut harness, "SIR chance");
    click(&mut harness, "Model name: SIR chance. Rename");
    harness.settle();
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text("SIR epidemic".to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
    enter_into(&mut harness, "Width in cells", "1024");
    enter_into(&mut harness, "Height in cells", "1024");
    add_states_property(&mut harness, "Health", &["Susceptible", "Infected", "Recovered"]);
    click(&mut harness, DONE);
    harness.settle();
    for state in ["Susceptible", "Infected", "Recovered"] {
        click(&mut harness, &format!("Plot {state}"));
    }
    let expected = document_of(SIR_CHANCE);
    type_into(
        &mut harness,
        "Purpose",
        expected.sheet.purpose.as_deref().unwrap_or_default(),
    );
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();

    add(&mut harness, &mut seen, AddButton::LookAround, "Count");
    pick(&mut harness, &mut seen, "Choose a property", "Health");
    pick(&mut harness, &mut seen, "Choose a state", "Infected");

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
             2. Each one counted is a separate try: 1 − (1 − 0.3)^n, where n is Infected neighbours."
        ),
        "a chance for each has its note"
    );

    add(&mut harness, &mut seen, AddButton::Chance, "Become, with chance");
    pick(&mut harness, &mut seen, "Choose a state", "Recovered");
    pick(&mut harness, &mut seen, "Choose the old state", "Infected");
    click(&mut harness, "Line 3: choose a chance");
    pick(&mut harness, &mut seen, "Choose a chance", NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Recovery chance", "0.05", Some("1"));

    choose(&mut harness, "Health starts as", "By chance");
    click(&mut harness, "Health starts as, row 1: use a parameter");
    click(&mut harness, NEW_PARAMETER);
    create_parameter(&mut harness, &mut seen, "Initial infected", "0.01", Some("1"));

    click(&mut harness, ADD_BUTTON);
    type_into(&mut harness, BUTTON_NAME_FIELD, "New outbreak");
    click(&mut harness, &run_once("cell"));
    click(&mut harness, DONE);
    harness.settle();
    add(&mut harness, &mut seen, AddButton::Change, "Become");
    pick(&mut harness, &mut seen, "Choose a state", "Infected");
    pick(&mut harness, &mut seen, "Choose the old state", "Susceptible");
    open_and_pick(
        &mut harness,
        &mut seen,
        "New outbreak line 1: choose when",
        CHOOSE_WHEN,
        "with chance ...",
    );
    open_and_pick(
        &mut harness,
        &mut seen,
        "New outbreak line 1: choose a chance",
        "Choose a chance",
        "Initial infected",
    );

    let aligned = align_with_starter(sheet(&harness).document(), &expected);
    assert_eq!(
        text_and_code(&aligned),
        text_and_code(&expected),
        "the rebuild equals the chance SIR in canonical text and lowered code"
    );
    assert_controls_hold_their_words(&mut harness);
    click(&mut harness, "Help: lines");
    seen.look(&mut harness);
    assert_no_forbidden_word(&seen);
}

/// Opens the starter `file_name` in the harness's Editor.
fn open_starter(harness: &mut Harness, file_name: &str) {
    let starter = starters()
        .iter()
        .find(|starter| starter.file_name == file_name)
        .expect("the starter");
    harness.open_sheet(starter.text);
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let starter = starters()
        .iter()
        .find(|starter| starter.file_name == "game_of_life.toml")
        .expect("the starter");
    let shared = starter.text.replacen(
        "[[parameter]]",
        "[[cells.property]]\nid = \"mood\"\nname = \"Mood\"\ntype = \"states\"\nstates = [\n  { id = \"calm\", name = \
         \"Calm\", color = \"#007AF5\" },\n  { id = \"alive_mood\", name = \"Alive\", color = \"#FFC107\" },\n]\n\n\
         [[parameter]]",
        1,
    );
    let document = document_of(&shared);
    harness.open_sheet(&document.to_text().expect("the sheet writes"));
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Status Alive from Dead, when Alive neighbours is 3"
    );
    let items: Vec<String> = {
        let editor = sheet(&harness);
        let document = editor.document();
        let sentences = Sentences::new(document);
        let relations = CardRelations::new(document, &document.cards[0]);
        let (_, line) = document.line("l2").expect("line 2");
        let context = LineContext {
            document,
            sentences: &sentences,
            relations: &relations,
            line_id: "l2",
            line,
        };
        picker_for(&context, Blank::State, None, true, "", DrawKind::Chance)
            .groups
            .iter()
            .flat_map(|group| group.items.iter().map(|item| item.label.clone()))
            .collect()
    };
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    open_starter(&mut harness, "game_of_life.toml");
    click(&mut harness, "Line 1 actions");
    click(&mut harness, "Rename line 1");
    let nodes = harness.settle();
    assert!(nodes.iter().any(|node| node.label == "Line 1: new name"));
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text("Neighbours".to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
    assert_eq!(
        line_text(&mut harness, "Line 2"),
        "Become Alive from Dead, when Neighbours is 3"
    );
    // Delete takes the line out, and its result's chips turn blank.
    click(&mut harness, "Line 1 actions");
    click(&mut harness, "Delete line 1");
    let document = sheet(&harness).document().clone();
    assert!(document.line("l1").is_none());
    assert_eq!(
        line_text(&mut harness, "Line 1"),
        "Become Alive from Dead, when ... is 3"
    );
    assert!(
        matches!(condition_right(&harness, "l2").value(), Some(SheetValue::Literal(_))),
        "the number stays"
    );
}

#[test]
fn focus_moves_to_the_next_blank_and_only_state_blanks_open_by_themselves() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let mut seen = Seen::default();
    open_starter(&mut harness, "forest_fire.toml");
    add(&mut harness, &mut seen, AddButton::Chance, "With chance");
    assert_eq!(line_text(&mut harness, "Line 6"), "Happens = with chance ...");
    assert!(is_open(&mut harness, "Choose a chance"), "the new line's chance opens");
    type_and_enter(&mut harness, "30%");
    assert_eq!(
        line_text(&mut harness, "Line 6"),
        "30% chance happens = with chance 30%"
    );
    click(&mut harness, "Line 6 actions");
    click(&mut harness, "Rename line 6");
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text("Burning neighbours".to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    assert!(
        shows(&mut harness, "Another result is already called Burning neighbours."),
        "a taken name is refused under the field"
    );
    harness.press(Key::A, Modifiers::COMMAND);
    harness.frame(vec![Event::Text("Spark".to_owned())]);
    harness.press(Key::Enter, Modifiers::NONE);
    assert_eq!(line_text(&mut harness, "Line 6"), "Spark = with chance 30%");
}

#[test]
fn a_negative_rate_is_refused_in_the_field() {
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    let mut seen = Seen::default();
    open_starter(&mut harness, "vote.toml");
    click(&mut harness, "Line 2: change the kind of condition");
    pick(
        &mut harness,
        &mut seen,
        CHOOSE_WHEN,
        "when ... is less than ... or more than ...",
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
    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
    harness.open_sheet(SIR_CHANCE);
    click(&mut harness, "Recovery chance");
    click_in(&mut harness, "Choose a chance", "Infected neighbours");
    let note = "Infected neighbours is a count, not a chance. Did you mean a chance for each infected neighbour?";
    assert!(shows(&mut harness, note));
    click(&mut harness, "Line 3: use a chance for each infected neighbour");
    assert_eq!(
        line_text(&mut harness, "Line 3"),
        "Become Recovered from Infected, with chance ... for each infected neighbour"
    );
    assert!(
        is_open(&mut harness, "Choose a chance"),
        "the emptied chance opens its picker"
    );
}

#[test]
fn changed_by_and_the_controls_of_the_rebuilt_lines_follow_the_wording_table() {
    use crate::ui::editor::sheet::card_pane::card_lines;

    let Some(mut harness) = Harness::new() else {
        return;
    };
    harness.screen = TALL_SCREEN;
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
