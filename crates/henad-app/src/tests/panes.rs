//! Helpers that drive the Sheet pane and the Card pane of the sheet editor through a [`Harness`]: the sheet in the
//! Editor, clicks by accessible name, fields, selects and pickers, the property and New parameter forms, the texts a
//! frame shows for the words rule, the documents of sheet starters, and the sections of the Game of Life, SIR and
//! random walk sheets that two rebuilds from Blank share.

use egui::accesskit::Role;
use egui::{Key, Modifiers};
use henad_core::metadata::TimeUnit;
use henad_graph::sheet::document::SheetDocument;
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    ADD_BUTTON, ADD_MEASURE, ADD_PROPERTY, ADD_STATE, AGENT_COUNT_NAME, AGENT_NAME_FIELD, AGENTS_IN_OPEN_SPACE,
    AVERAGE_MEASURE, AddButton, BUTTON_NAME_FIELD, Blank, CHOOSE_WHEN, COLOR_AGENTS_BY, COLOR_CELLS_BY,
    CREATE_PARAMETER, DONE, DrawKind, MEASURE_PROPERTY_NAME, NEW_PARAMETER, START_AGAIN_FOR, Sentences, TICK_UNIT_NAME,
    run_once, start_again_for, tick_unit_choice,
};
use henad_graph::sheet::{ParsedSheet, SheetFile};

use crate::tests::support::{Harness, Node, forbidden_words, starter_text, top};
use crate::ui::editor::EditorModel;
use crate::ui::editor::sheet::SheetEditor;
use crate::ui::editor::sheet::picker::{CardPicker, LineContext, picker_for};

/// Sheet in the harness's Editor.
pub fn sheet(harness: &Harness) -> &SheetEditor {
    harness.app.editor.model.sheet().expect("a sheet")
}

/// Sheet in the harness's Editor, to change.
pub fn sheet_mut(harness: &mut Harness) -> &mut SheetEditor {
    match &mut harness.app.editor.model {
        EditorModel::Sheet(sheet) => sheet,
        _ => panic!("no sheet"),
    }
}

/// Applies `edit` to `sheet` as one edit that a pane makes while the preview is at its start.
pub fn apply_sheet_edit(sheet: &mut SheetEditor, edit: SheetEdit) {
    sheet
        .edit(false, false, |builder| builder.apply(edit).map(drop))
        .expect("the edit applies");
}

/// Returns the canonical text of the sheet in the harness's Editor.
pub fn text(harness: &Harness) -> String {
    sheet(harness).text().to_owned()
}

/// Returns the nodes of a settled frame named exactly `name`, top to bottom.
pub fn named(harness: &mut Harness, name: &str) -> Vec<Node> {
    let mut found: Vec<Node> = harness
        .settle()
        .into_iter()
        .filter(|node| node.label == name && node.bounds.is_some())
        .collect();
    found.sort_by(|a, b| top(a).total_cmp(&top(b)));
    found
}

/// Returns the centre of the one node of a settled frame named exactly `name`.
pub fn center_of(harness: &mut Harness, name: &str) -> egui::Pos2 {
    let found = named(harness, name);
    assert_eq!(found.len(), 1, "one widget is named {name}: {found:?}");
    found[0].bounds.expect("bounds").center()
}

/// Clicks the one widget named exactly `name`.
pub fn click(harness: &mut Harness, name: &str) {
    let position = center_of(harness, name);
    harness.click_at(position);
}

/// Clicks the last of the widgets named exactly `name`, the one lowest on the screen.
pub fn click_last(harness: &mut Harness, name: &str) {
    let found = named(harness, name);
    let last = found.last().unwrap_or_else(|| panic!("no widget is named {name}"));
    let position = last.bounds.expect("bounds").center();
    harness.click_at(position);
}

/// Returns the bounds of the one node of role `role` whose name starts with `name`.
pub fn bounds_of(nodes: &[Node], role: Role, name: &str) -> egui::Rect {
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.role == role && node.label.starts_with(name) && node.bounds.is_some())
        .collect();
    assert_eq!(found.len(), 1, "one {role:?} is named {name}: {found:?}");
    found[0].bounds.expect("bounds")
}

/// Clicks the widget named exactly `item` inside the open picker or menu titled `dialog`.
pub fn click_in(harness: &mut Harness, dialog: &str, item: &str) {
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
pub fn click_on_line(harness: &mut Harness, line: &str, item: &str) {
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
pub fn is_open(harness: &mut Harness, dialog: &str) -> bool {
    harness
        .settle()
        .iter()
        .any(|node| node.role == Role::Dialog && node.label == dialog)
}

/// Types `text` into the field that holds the focus and presses Enter.
pub fn type_and_enter(harness: &mut Harness, text: &str) {
    harness.replace_text(text);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
}

/// Replaces the text of the field named `name` with `text`, without committing it.
pub fn type_into(harness: &mut Harness, name: &str, text: &str) {
    click(harness, name);
    harness.replace_text(text);
}

/// Replaces the text of the field named `name` with `text` and presses Enter.
pub fn enter_into(harness: &mut Harness, name: &str, text: &str) {
    type_into(harness, name, text);
    harness.press(Key::Enter, Modifiers::NONE);
    harness.settle();
}

/// Chooses `item` in the select named `select`.
pub fn choose(harness: &mut Harness, select: &str, item: &str) {
    click(harness, select);
    click(harness, item);
    harness.settle();
}

/// Returns the value of the field named exactly `name` in a settled frame.
pub fn field_value(harness: &mut Harness, name: &str) -> String {
    harness
        .settle()
        .into_iter()
        .find(|node| node.label == name)
        .map(|node| node.value)
        .unwrap_or_else(|| panic!("no field is named {name}"))
}

/// Returns the sentence of the line whose accessible name starts with `prefix` and a colon, such as "Line 2" or "Clear
/// line 1", read from its accessible name.
pub fn line_text(harness: &mut Harness, prefix: &str) -> String {
    let nodes = harness.settle();
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.role == Role::Group && node.label.starts_with(&format!("{prefix}: ")))
        .collect();
    assert_eq!(found.len(), 1, "one line is named {prefix}: {found:?}");
    found[0].label[prefix.len() + 2..].to_owned()
}

/// Every text a settled frame shows, gathered while menus and pickers are open, with the number of
/// frames looked at.
#[derive(Debug, Default)]
pub struct Seen {
    texts: Vec<String>,
    looks: usize,
}

impl Seen {
    /// Adds every text the harness shows now.
    pub fn look(&mut self, harness: &mut Harness) {
        self.texts.extend(harness.accessible_texts());
        self.looks += 1;
    }

    /// Number of frames looked at.
    pub fn looks(&self) -> usize {
        self.looks
    }
}

/// Asserts that no text of `seen` holds a forbidden word.
pub fn assert_no_forbidden_word(seen: &Seen) {
    let found = forbidden_words(&seen.texts);
    assert!(found.is_empty(), "forbidden words: {found:?}");
}

/// Opens the add button `button`'s menu under the lowest card, records what it shows, and chooses `item`.
pub fn add(harness: &mut Harness, seen: &mut Seen, button: AddButton, item: &str) {
    click_last(harness, button.label());
    choose_add_item(harness, seen, button, item);
}

/// Opens the add button `button`'s menu under the first card, records what it shows, and chooses `item`.
pub fn add_to_first(harness: &mut Harness, seen: &mut Seen, button: AddButton, item: &str) {
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

/// Returns whether a settled frame shows `text` as a whole label, value or description.
pub fn shows(harness: &mut Harness, text: &str) -> bool {
    harness.accessible_texts().iter().any(|shown| shown == text)
}

/// Returns whether a settled frame shows the notice `notice`, its text after the notice's icon and a space as
/// [`crate::ui::editor::sheet::widgets::Notice`] draws it.
pub fn shows_notice(harness: &mut Harness, notice: &str) -> bool {
    harness
        .accessible_texts()
        .iter()
        .any(|shown| shown.strip_suffix(notice).is_some_and(|icon| icon.ends_with(' ')))
}

/// Returns whether a settled frame shows `text` as a whole label, value or description, or as a notice after its
/// icon.
pub fn shows_or_notice(harness: &mut Harness, text: &str) -> bool {
    harness
        .accessible_texts()
        .iter()
        .any(|shown| shown == text || shown.ends_with(&format!(" {text}")))
}

/// Picks `item` in the open picker titled `dialog`, after recording what it shows.
pub fn pick(harness: &mut Harness, seen: &mut Seen, dialog: &str, item: &str) {
    assert!(is_open(harness, dialog), "{dialog} is open");
    seen.look(harness);
    click_in(harness, dialog, item);
    harness.settle();
}

/// Clicks the control named `control` and picks `item` in the picker titled `dialog` it opens.
pub fn open_and_pick(harness: &mut Harness, seen: &mut Seen, control: &str, dialog: &str, item: &str) {
    click(harness, control);
    pick(harness, seen, dialog, item);
}

/// Adds a States property `name` with `states` through the property form that the button named `add` opens, each
/// state in its usual or default colour, and leaves the form open.
pub fn add_states_property(harness: &mut Harness, add: &str, name: &str, states: &[&str]) {
    click(harness, add);
    type_into(harness, "Property name", name);
    for (index, state) in states.iter().enumerate() {
        if index >= 2 {
            click(harness, ADD_STATE);
        }
        type_into(harness, &format!("State {} name", index + 1), state);
    }
}

/// Adds a property named `name` of the kind named `kind` through the property form that the button named `add` opens.
pub fn add_property(harness: &mut Harness, add: &str, name: &str, kind: &str) {
    click(harness, add);
    type_into(harness, "Property name", name);
    click(harness, kind);
    click(harness, DONE);
    harness.settle();
}

/// Creates a parameter in the open New parameter dialog, with the default `default` and the top `max` when given.
pub fn create_parameter(harness: &mut Harness, seen: &mut Seen, name: &str, default: &str, max: Option<&str>) {
    seen.look(harness);
    type_into(harness, "Parameter name", name);
    type_into(harness, "Default value", default);
    if let Some(max) = max {
        type_into(harness, "Max", max);
    }
    click(harness, CREATE_PARAMETER);
    harness.settle();
}

/// Asserts that every control of every line of the sheet in the harness, every button of a note and every add row of
/// a When block is drawn under its accessible name, and that the name of each but a select holds the words it shows.
pub fn assert_controls_hold_their_words(harness: &mut Harness) {
    use crate::ui::editor::sheet::card_pane::{card_buttons, card_lines};

    let nodes = harness.settle();
    let editor = sheet(harness);
    let document = editor.document();
    let sentences = Sentences::new(document);
    for index in 0..document.cards.len() {
        let lines = card_lines(document, &sentences, editor.report(), index);
        let controls = lines
            .iter()
            .flat_map(|line| line.controls().into_iter().map(|control| (line.label(), control)));
        let buttons = card_buttons(document, &sentences, editor.report(), index)
            .into_iter()
            .map(|button| (format!("Card {}", index + 1), button));
        for (owner, control) in controls.chain(buttons) {
            assert!(
                nodes.iter().any(|node| node.label == control.name),
                "{owner} has no control named {}",
                control.name
            );
            let words = control.visible.trim_end_matches('…').trim().to_lowercase();
            assert!(
                control.select || control.name.to_lowercase().contains(&words),
                "{} does not hold {}",
                control.name,
                control.visible
            );
        }
    }
}

/// Asserts that a button, a radio button or a check box of `nodes` shows `visible` and that its name holds it, a
/// leading "+ " and a trailing "…" aside.
pub fn assert_control_holds(nodes: &[Node], visible: &str) {
    let words = visible.trim_start_matches("+ ").trim_end_matches('…').to_lowercase();
    let named = nodes.iter().any(|node| {
        matches!(node.role, Role::Button | Role::RadioButton | Role::CheckBox)
            && node.label.to_lowercase().contains(&words)
    });
    assert!(named, "no control's name holds its visible words {visible:?}");
}

/// Returns the canonical text and the lowered code of `document`.
pub fn text_and_code(document: &SheetDocument) -> (String, String) {
    let text = document.to_text().expect("the sheet writes");
    let code = SheetFile::parse(&text)
        .expect("the sheet lowers")
        .lowered()
        .text()
        .to_owned();
    (text, code)
}

/// Returns the document of `text`.
pub fn document_of(text: &str) -> SheetDocument {
    ParsedSheet::parse(text).expect("the sheet reads").into_document()
}

/// Returns the document of the starter whose file is named `file_name`.
pub fn starter_document(file_name: &str) -> SheetDocument {
    document_of(starter_text(file_name))
}

/// Opens the starter `file_name` in the harness's Editor.
pub fn open_starter(harness: &mut Harness, file_name: &str) {
    harness.open_sheet(starter_text(file_name));
}

/// Opens the New model dialog, names the model `name`, and creates a blank model.
pub fn new_blank(harness: &mut Harness, name: &str) {
    new_model_named(harness, name, None);
}

/// Opens the New model dialog, names the model `name`, and creates a blank model of agents.
pub fn new_blank_agents(harness: &mut Harness, name: &str) {
    new_model_named(harness, name, Some(AGENTS_IN_OPEN_SPACE));
}

/// Opens the New model dialog, names the model `name`, answers the world question under Blank with `world` when
/// given, and creates the blank model.
fn new_model_named(harness: &mut Harness, name: &str, world: Option<&str>) {
    click(harness, "New model");
    harness.click_name_field();
    harness.replace_text(name);
    if let Some(world) = world {
        click(harness, world);
    }
    click(harness, "Create");
    harness.settle();
}

/// Clicks the one control named exactly `name` that is no label, such as an item of an open select whose words a
/// label of the sheet also shows.
pub fn click_control(harness: &mut Harness, name: &str) {
    let nodes = harness.settle();
    let found: Vec<&Node> = nodes
        .iter()
        .filter(|node| node.label == name && node.role != Role::Label && node.bounds.is_some())
        .collect();
    assert_eq!(found.len(), 1, "one control is named {name}: {found:?}");
    let position = found[0].bounds.expect("bounds").center();
    harness.click_at(position);
    harness.settle();
}

/// Returns whether the open picker titled `dialog` offers an item named `item`.
pub fn picker_offers(harness: &mut Harness, dialog: &str, item: &str) -> bool {
    let nodes = harness.settle();
    let area = nodes
        .iter()
        .find(|node| node.role == Role::Dialog && node.label == dialog)
        .and_then(|node| node.bounds)
        .unwrap_or_else(|| panic!("{dialog} is open"));
    nodes
        .iter()
        .any(|node| node.label == item && node.bounds.is_some_and(|rect| area.contains(rect.center())))
}

// Sections that two rebuilds from Blank share

/// Builds the sheet of the Game of Life starter from Blank in the harness, every section but the card's lines and
/// Clear: 1024 by 1024 cells, Status with Alive in green, Status by chance through a new parameter Initial density,
/// Plot Alive, the cells coloured by Status, the starter's purpose and the button Randomise.
///
/// Returns the sheet's text after each step that changed it, the blank model's text first.
pub fn build_life_sections(harness: &mut Harness, seen: &mut Seen) -> Vec<String> {
    let purpose = starter_document("game_of_life.toml").sheet.purpose.expect("a purpose");
    new_blank(harness, "Game of Life");
    let mut texts = vec![text(harness)];
    let mut record = |harness: &Harness| {
        let now = text(harness);
        if texts.last() != Some(&now) {
            texts.push(now);
        }
    };
    enter_into(harness, "Width in cells", "1024");
    record(harness);
    enter_into(harness, "Height in cells", "1024");
    record(harness);
    add_states_property(harness, ADD_PROPERTY, "Status", &["Dead", "Alive"]);
    click(harness, "State 2 colour: Blue");
    click(harness, "Green");
    click(harness, DONE);
    harness.settle();
    record(harness);
    choose(harness, "Status starts as", "By chance");
    record(harness);
    click(harness, "Status starts as, row 1: use a parameter");
    click(harness, NEW_PARAMETER);
    create_parameter(harness, seen, "Initial density", "0.3", None);
    record(harness);
    click(harness, "Plot Alive");
    record(harness);
    choose(harness, COLOR_CELLS_BY, "Status");
    record(harness);
    type_into(harness, "Purpose", &purpose);
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
    record(harness);
    click(harness, ADD_BUTTON);
    type_into(harness, BUTTON_NAME_FIELD, "Randomise");
    choose(harness, START_AGAIN_FOR, "Status");
    click(harness, DONE);
    harness.settle();
    record(harness);
    assert!(shows(harness, &start_again_for("Status")));
    texts
}

/// Builds the sections of an SIR sheet from Blank in the harness, and its line 1: a blank model named `name` that the
/// toolbar renames SIR epidemic, 1024 by 1024 cells, one tick of 1 `tick_unit` when given, Health with Susceptible,
/// Infected and Recovered each plotted, the purpose of `expected`, and the count of Infected neighbours.
pub fn build_sir_sections(
    harness: &mut Harness,
    seen: &mut Seen,
    name: &str,
    expected: &SheetDocument,
    tick_unit: Option<TimeUnit>,
) {
    // The id comes from the name given in the New model dialog, and the toolbar renames the model keeping it.
    new_blank(harness, name);
    click(harness, &format!("Model name: {name}. Rename"));
    harness.settle();
    type_and_enter(harness, "SIR epidemic");
    enter_into(harness, "Width in cells", "1024");
    enter_into(harness, "Height in cells", "1024");
    if let Some(unit) = tick_unit {
        choose(harness, TICK_UNIT_NAME, tick_unit_choice(Some(unit)));
    }
    let states = ["Susceptible", "Infected", "Recovered"];
    add_states_property(harness, ADD_PROPERTY, "Health", &states);
    click(harness, DONE);
    harness.settle();
    for state in states {
        click(harness, &format!("Plot {state}"));
    }
    type_into(
        harness,
        "Purpose",
        expected.sheet.purpose.as_deref().unwrap_or_default(),
    );
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();

    add(harness, seen, AddButton::LookAround, "Count");
    pick(harness, seen, "Choose a property", "Health");
    pick(harness, seen, "Choose a state", "Infected");
}

/// Sets Health to start Infected by chance, with a new parameter Initial infected at 0.01 up to 1, through "Use a
/// parameter".
pub fn start_infected_by_chance(harness: &mut Harness, seen: &mut Seen) {
    choose(harness, "Health starts as", "By chance");
    click(harness, "Health starts as, row 1: use a parameter");
    click(harness, NEW_PARAMETER);
    create_parameter(harness, seen, "Initial infected", "0.01", Some("1"));
}

/// Adds the button New outbreak, whose card makes a Susceptible cell Infected with chance Initial infected, and
/// checks that its card offers no rate and its chance picker no rate parameter.
pub fn add_new_outbreak(harness: &mut Harness, seen: &mut Seen) {
    click(harness, ADD_BUTTON);
    type_into(harness, BUTTON_NAME_FIELD, "New outbreak");
    click(harness, &run_once("cell"));
    click(harness, DONE);
    harness.settle();
    add(harness, seen, AddButton::Change, "Become");
    pick(harness, seen, "Choose a state", "Infected");
    pick(harness, seen, "Choose the old state", "Susceptible");
    click(harness, "New outbreak line 1: choose when");
    assert!(is_open(harness, CHOOSE_WHEN));
    assert!(!shows(harness, "at rate … per …"), "a button's card takes no rate");
    pick(harness, seen, CHOOSE_WHEN, "with chance …");
    click(harness, "New outbreak line 1: choose a chance");
    assert!(picker_offers(harness, "Choose a chance", "Initial infected"));
    for rate in ["Infection rate", "Recovery rate"] {
        assert!(
            !picker_offers(harness, "Choose a chance", rate),
            "a chance's picker leaves out the rate parameter {rate}"
        );
    }
    pick(harness, seen, "Choose a chance", "Initial infected");
}

/// Sets the world to 1000 by 1000, names the agents `agents` with `count` of them at the start, colours them by
/// heading and types the purpose of `starter`.
pub fn agent_sheet(harness: &mut Harness, starter: &SheetDocument, agents: &str, count: &str) {
    enter_into(harness, "Width in cells", "1000");
    enter_into(harness, "Height in cells", "1000");
    enter_into(harness, AGENT_NAME_FIELD, agents);
    enter_into(harness, AGENT_COUNT_NAME, count);
    click(harness, COLOR_AGENTS_BY);
    click_control(harness, "Heading");
    type_into(harness, "Purpose", starter.sheet.purpose.as_deref().unwrap_or_default());
    harness.press(Key::Escape, Modifiers::NONE);
    harness.settle();
}

/// Adds the measure Average of `property` through "+ Add measure".
pub fn add_average_measure(harness: &mut Harness, property: &str) {
    click(harness, ADD_MEASURE);
    click(harness, AVERAGE_MEASURE);
    click(harness, MEASURE_PROPERTY_NAME);
    click_control(harness, property);
    click(harness, DONE);
    harness.settle();
}

// Pickers of the Card pane

/// Returns the picker that `pick` builds for line `line_id` of `document`, in the context the Card pane draws the line
/// in, with the relations of the line's own card.
pub fn line_picker(
    document: &SheetDocument,
    line_id: &str,
    pick: impl FnOnce(&LineContext<'_>) -> CardPicker,
) -> CardPicker {
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
    pick(&context)
}

/// Returns the group heading and the label of each item that the picker of a `want` blank on line `line_id` of
/// `document` offers, every group in order. `filled` is whether the blank holds a value, and `draw` the kind of draw a
/// "for each" grows.
pub fn picker_labels(
    document: &SheetDocument,
    line_id: &str,
    want: Blank,
    filled: bool,
    draw: DrawKind,
) -> Vec<(String, String)> {
    labels_of(&line_picker(document, line_id, |context| {
        picker_for(context, want, None, true, filled, "", draw)
    }))
}

/// Returns the group heading and the label of each item of `picker`, every group in order.
pub fn labels_of(picker: &CardPicker) -> Vec<(String, String)> {
    picker
        .groups
        .iter()
        .flat_map(|group| {
            group
                .items
                .iter()
                .map(|item| (group.heading.clone(), item.label.clone()))
        })
        .collect()
}
