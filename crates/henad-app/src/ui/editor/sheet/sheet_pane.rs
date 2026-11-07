//! The Sheet pane: what a sheet model holds, one section per heading of
//! [`SheetSection`](henad_graph::sheet::sentence::SheetSection), each heading with its "?" help.
//!
//! Purpose, World (with the length of one tick), Cells (each property with its kind, its start and the lines that
//! change it), Parameters, Measures, Colours and Buttons. The World and Cells sections are edited here: the size and
//! the edges of the grid, the length of one tick, and each property of the cells with its kind, its states and how it
//! starts. The other sections are read-only.
//!
//! Every edit runs commands through [`SheetEditor::edit`]. A number field commits on Enter or on losing focus, and
//! text it cannot read stays in the field with a red message under its row. A property is added or changed through
//! the *property form* under the Cells table. A Starts as number is a plain field with "Use a parameter" beside it,
//! and a chosen parameter shows as its chip.

use egui::{Button, ComboBox, CornerRadius, Frame, Id, Key, RichText, Sense, TextEdit, Ui, Vec2};
use henad_core::metadata::TimeUnit;
use henad_graph::limits::MAX_GRID_SIDE;
use henad_graph::sheet::document::{
    ButtonStepRule, CardRuns, ColorRuleKind, Edges, LineKind, MadeOf, PropertyType, SheetChip, SheetDocument,
    SheetLine, SheetLiteral, SheetParameter, SheetProperty, SheetTime, SheetValue, Slot, StartRule, StartsAs,
    StartsAsRow,
};
use henad_graph::sheet::edit::{
    STATE_COLORS, SheetBuilder, SheetEdit, SheetEditError, SheetItem, suggested_state_color,
};
use henad_graph::sheet::format_number;
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    A_RANDOM_NUMBER, A_VALUE, ADD_PROPERTY, ADD_ROW, ADD_STATE, AGENTS_IN_OPEN_SPACE, BY_CHANCE, Blank, CANCEL,
    CELLS_COLUMNS, CELLS_ON_A_GRID, CHOOSE_A_PARAMETER, COLOR_CELLS_BY, ChipKind, DELETE_PROPERTY, DONE, EDGES_LABEL,
    HEIGHT_NAME, INTEGER_KIND, INTEGER_KIND_TOOLTIP, KIND_LABEL, NAME_LABEL, NEW_PROPERTY, NUMBER_KIND,
    NUMBER_KIND_DESCRIPTION, ONE_TICK_IS, OTHERWISE, PARAMETERS, PROPERTY_NAME_FIELD, PROPERTY_NAME_MISSING,
    PROPERTY_NAME_PLACEHOLDER, RANDOM_NUMBER_AND, RANDOM_NUMBER_BETWEEN, REMOVE_ROW_LABEL, SHEET_EMPTY, SIZE_BY,
    SIZE_LABEL, START_AGAIN, START_AGAIN_TO_SEE, STARTS_AT, STATE_NAMES_REPEATED, STATES_KIND, STATES_KIND_DESCRIPTION,
    SentenceChip, SheetSection, StartsField, TICK_LENGTH_NAME, TICK_UNIT_NAME, TOO_FEW_STATES, TYPE_A_CHANCE,
    TYPE_ONE_NUMBER, USE_A_PARAMETER, WALLS, WALLS_SENTENCE, WIDTH_NAME, WITH_CHANCE, WRAP_AROUND, WRAP_SENTENCE,
    WRAP_TOOLTIP, add_row_name, changed_by, edit_property_heading, literal_words, property_name_taken,
    read_typed_number, remove_row_name, remove_state_name, run_once, sheet_trying_value, starts_all, starts_field_name,
    starts_high_place, starts_low_place, starts_otherwise_name, starts_place, starts_row_place, state_color_name,
    state_name_field, tick_unit_choice, world_side_out_of_range,
};
use henad_graph::value::Color;

use super::SheetEditor;
use super::help::{Help, help_button};
use super::widgets::{
    DialogOutcome, NewParameterDialog, NumberUse, Picker, PickerChoice, PickerGroup, PickerItem, chip_ui, color_button,
    color_name, color32, commit_field, field_message, forget_field, message_ui, new_parameter_modal, set_name,
};
use crate::ui::kv_grid;
use crate::ui::sweep::layout::{add_button, destructive_button, filled_button};

/// A value the world tries for a parameter of the sheet while the model keeps its own (D88).
#[derive(Debug, Clone, PartialEq)]
pub struct SheetTrial {
    /// Id of the parameter.
    pub parameter: String,
    /// Value the world tries.
    pub value: f64,
}

/// Kind of thing whose properties the Cells section lists.
const CELLS: &str = "cells";

/// Width in points of a number field.
const FIELD_WIDTH: f32 = 56.0;

/// Width in points of a state's name field in the property form.
const STATE_FIELD_WIDTH: f32 = 140.0;

/// Side in points of a state's swatch in the Cells table.
const SWATCH_SIDE: f32 = 10.0;

/// State of the Sheet pane between frames: the id its widgets are keyed under, the property form, the New parameter
/// dialog and the reason the last edit was refused.
#[derive(Debug)]
pub struct PaneState {
    /// Id the pane's widgets are keyed under, one for each sheet opened. A field's refused text then stays with the
    /// sheet it was typed in.
    widgets: Id,
    /// Property form under the Cells table, `None` while it is closed.
    property_form: Option<PropertyForm>,
    /// New parameter dialog with the Starts as number it fills, `None` while it is closed.
    new_parameter: Option<(StartTarget, NewParameterDialog)>,
    /// Reason the last edit outside the property form was refused, `None` while none shows.
    refusal: Option<String>,
}

impl PaneState {
    /// Returns the state of a new pane, its widgets keyed by `identity`, the number of the load that opened the sheet.
    pub fn new(identity: u64) -> Self {
        Self {
            widgets: Id::new(("henad_sheet_pane", identity)),
            property_form: None,
            new_parameter: None,
            refusal: None,
        }
    }
}

/// The form that adds a property to the cells or changes one.
#[derive(Debug, Clone)]
struct PropertyForm {
    /// Id of the property the form changes, `None` for a new property.
    editing: Option<String>,
    /// Name of the property when the form opened.
    original: String,
    /// Text of the Name field.
    name: String,
    /// Kind chosen.
    kind: PropertyType,
    /// Rows of states, for a States property.
    states: Vec<FormState>,
    /// Message of the last refused Done, `None` while none shows.
    message: Option<String>,
    /// Row of states whose name field takes the focus on the next frame.
    focus_state: Option<usize>,
    /// Whether the Name field takes the focus on the next frame.
    focus_name: bool,
}

/// One row of states in the property form.
#[derive(Debug, Clone)]
struct FormState {
    /// Id of the state, `None` for a state the form adds.
    id: Option<String>,
    /// Text of the state's name field.
    name: String,
    /// Colour of the state.
    color: Color,
    /// Whether the colour was chosen by hand. Until then a usual name gives its usual colour.
    picked: bool,
}

impl FormState {
    /// Returns an empty row at `index`, in the colour new states take there.
    fn empty(index: usize) -> Self {
        Self {
            id: None,
            name: String::new(),
            color: default_color(index),
            picked: false,
        }
    }
}

impl PropertyForm {
    /// Returns the form that adds a property: a States property with two empty states.
    fn add() -> Self {
        Self {
            editing: None,
            original: String::new(),
            name: String::new(),
            kind: PropertyType::States,
            states: vec![FormState::empty(0), FormState::empty(1)],
            message: None,
            focus_state: None,
            focus_name: true,
        }
    }

    /// Returns the form that changes `property`. Its states count as coloured by hand, and a new name keeps their
    /// colours.
    fn edit(property: &SheetProperty) -> Self {
        let states = if property.property_type == PropertyType::States {
            property
                .states
                .iter()
                .map(|state| FormState {
                    id: Some(state.id.clone()),
                    name: state.name.clone(),
                    color: state.color,
                    picked: true,
                })
                .collect()
        } else {
            vec![FormState::empty(0), FormState::empty(1)]
        };
        Self {
            editing: Some(property.id.clone()),
            original: property.name.clone(),
            name: property.name.clone(),
            kind: property.property_type,
            states,
            message: None,
            focus_state: None,
            focus_name: true,
        }
    }
}

/// Place of a number in a Starts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StartSlot {
    /// The chance of a row of a by-chance start.
    Row(usize),
    /// The number every cell starts at.
    Value,
    /// The low end of a random number.
    Low,
    /// The high end of a random number.
    High,
}

/// A number of a property's Starts as.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StartTarget {
    /// Id of the property.
    property: String,
    /// Place of the number.
    slot: StartSlot,
}

/// One edit of the sheet: commands run on a copy of it, kept when every command is accepted.
type EditRun = Box<dyn FnOnce(&mut SheetBuilder) -> Result<(), SheetEditError>>;

/// An edit made during the frame, run once the pane is drawn.
struct Pending {
    /// Whether the edit changes how properties start and nothing else.
    starts_only: bool,
    /// Whether the edit comes from the property form. The form closes once the edit is kept, and shows a refusal as
    /// its message.
    from_form: bool,
    /// The commands.
    run: EditRun,
}

impl Pending {
    /// Returns the edit that runs the one command `edit`.
    fn one(edit: SheetEdit) -> Self {
        Self {
            starts_only: matches!(edit, SheetEdit::SetStartsAs { .. }),
            from_form: false,
            run: Box::new(move |builder| builder.apply(edit).map(drop)),
        }
    }
}

/// Draws every section of `sheet`, in the order of [`SheetSection::ALL`], each heading with its help, and runs the
/// edits made in it.
///
/// `past_start` is whether the preview has passed tick 0 or a button was pressed in it. A changed Starts as then says
/// "Start again to see it". `trials` holds the values the world tries, each shown under its parameter.
pub fn sheet_pane(ui: &mut Ui, sheet: &mut SheetEditor, past_start: bool, trials: &[SheetTrial]) {
    let stale: Vec<String> = sheet
        .document()
        .cells
        .iter()
        .flatten()
        .filter(|property| past_start && sheet.start_changed(&property.id))
        .map(|property| property.id.clone())
        .collect();
    let mut pending = None;
    {
        let SheetEditor { builder, pane, .. } = &mut *sheet;
        let document = builder.document();
        ui.push_id(pane.widgets, |ui| {
            for section in SheetSection::ALL {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(section.heading()).strong());
                    help_button(ui, Help::Section(section));
                });
                match section {
                    SheetSection::Purpose => purpose(ui, document),
                    SheetSection::World => world(ui, document, pane.widgets, &mut pending),
                    SheetSection::Cells => cells(ui, document, pane, &stale, &mut pending),
                    SheetSection::Parameters => parameters(ui, document, trials),
                    SheetSection::Measures => measures(ui, document),
                    SheetSection::Colours => colours(ui, document),
                    SheetSection::Buttons => buttons(ui, document),
                    _ => {}
                }
            }
            new_parameter(ui, document, pane, &mut pending);
        });
    }
    let Some(Pending {
        starts_only,
        from_form,
        run,
    }) = pending
    else {
        return;
    };
    let outcome = sheet.edit(past_start, starts_only, run);
    let pane = &mut sheet.pane;
    match outcome {
        Ok(()) => {
            pane.refusal = None;
            if from_form {
                pane.property_form = None;
            }
        }
        Err(error) => {
            let message = refusal_text(&error);
            match pane.property_form.as_mut().filter(|_| from_form) {
                Some(form) => form.message = Some(message),
                None => pane.refusal = Some(message),
            }
        }
    }
}

/// Returns the reason `error` gives for a refused edit as a sentence.
fn refusal_text(error: &SheetEditError) -> String {
    let text = error.to_string();
    let mut characters = text.chars();
    match characters.next() {
        Some(first) => format!("{}{}.", first.to_uppercase(), characters.as_str()),
        None => text,
    }
}

/// Draws what the model is for.
fn purpose(ui: &mut Ui, document: &SheetDocument) {
    if let Some(purpose) = &document.sheet.purpose {
        ui.label(purpose);
    }
}

/// Draws the world's kind, its size and edges as fields, and the length of one tick. `widgets` is the id the pane's
/// widgets are keyed under.
fn world(ui: &mut Ui, document: &SheetDocument, widgets: Id, pending: &mut Option<Pending>) {
    let world = document.world;
    ui.label(match world.made_of {
        MadeOf::Agents => AGENTS_IN_OPEN_SPACE,
        _ => CELLS_ON_A_GRID,
    });
    let width_id = widgets.with("henad_sheet_world_width");
    let height_id = widgets.with("henad_sheet_world_height");
    ui.horizontal(|ui| {
        ui.label(SIZE_LABEL);
        side_field(ui, width_id, WIDTH_NAME, world.width, pending, |width| {
            SheetEdit::SetWorld {
                width,
                height: world.height,
                edges: world.edges,
            }
        });
        ui.label(SIZE_BY);
        side_field(ui, height_id, HEIGHT_NAME, world.height, pending, |height| {
            SheetEdit::SetWorld {
                width: world.width,
                height,
                edges: world.edges,
            }
        });
    });
    if let Some(message) = field_message(ui, width_id).or_else(|| field_message(ui, height_id)) {
        message_ui(ui, &message);
    }
    let (words, sentence, tooltip) = match world.edges {
        Edges::Walls => (WALLS, WALLS_SENTENCE, WALLS_SENTENCE),
        _ => (WRAP_AROUND, WRAP_SENTENCE, WRAP_TOOLTIP),
    };
    ui.horizontal(|ui| {
        ui.label(EDGES_LABEL);
        let mut edges = world.edges;
        let combo = ComboBox::from_id_salt("henad_sheet_edges")
            .selected_text(words)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut edges, Edges::Wrap, WRAP_AROUND);
                ui.selectable_value(&mut edges, Edges::Walls, WALLS);
            });
        set_name(ui, combo.response.id, EDGES_LABEL);
        combo.response.on_hover_text(tooltip);
        if edges != world.edges {
            *pending = Some(Pending::one(SheetEdit::SetWorld {
                width: world.width,
                height: world.height,
                edges,
            }));
        }
    });
    ui.weak(sentence);
    time_line(ui, document, widgets, pending);
}

/// Adds the field of one side of the grid, named `name`, holding `side` cells. A whole number of cells from 1 to
/// [`MAX_GRID_SIDE`] commits the command `edit` returns for it.
fn side_field(
    ui: &mut Ui,
    id: Id,
    name: &str,
    side: u32,
    pending: &mut Option<Pending>,
    edit: impl FnOnce(u32) -> SheetEdit,
) {
    commit_field(ui, id, name, &side.to_string(), FIELD_WIDTH, |text| {
        let Some(number) = read_typed_number(text).and_then(SheetLiteral::number) else {
            return Some(TYPE_ONE_NUMBER.to_owned());
        };
        if number.fract() != 0.0 || !(1.0..=f64::from(MAX_GRID_SIDE)).contains(&number) {
            return Some(world_side_out_of_range(MAX_GRID_SIDE));
        }
        // The checks above hold the side to a whole number within `u32`.
        let side = number as u32;
        *pending = Some(Pending::one(edit(side)));
        None
    });
}

/// Draws the Time line, "One tick is [number] [unit]", with the number hidden while the tick has no unit.
fn time_line(ui: &mut Ui, document: &SheetDocument, widgets: Id, pending: &mut Option<Pending>) {
    let length_id = widgets.with("henad_sheet_tick_length");
    let time = document.time;
    ui.horizontal(|ui| {
        ui.label(ONE_TICK_IS);
        if let Some(time) = time {
            let current = format_number(time.tick_length).unwrap_or_default();
            commit_field(ui, length_id, TICK_LENGTH_NAME, &current, FIELD_WIDTH, |text| {
                let Some(length) = read_typed_number(text).and_then(SheetLiteral::number) else {
                    return Some(TYPE_ONE_NUMBER.to_owned());
                };
                // A length of 0 or less becomes 1, as the page has it.
                let length = if length > 0.0 { length } else { 1.0 };
                *pending = Some(Pending::one(SheetEdit::SetTime {
                    time: Some(SheetTime::new(length, time.tick_unit)),
                }));
                None
            });
        }
        let current = time.map(|time| time.tick_unit);
        let mut unit = current;
        let combo = ComboBox::from_id_salt("henad_sheet_tick_unit")
            .selected_text(tick_unit_choice(current))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut unit, None, tick_unit_choice(None));
                for choice in TimeUnit::ALL {
                    ui.selectable_value(&mut unit, Some(choice), tick_unit_choice(Some(choice)));
                }
            });
        set_name(ui, combo.response.id, TICK_UNIT_NAME);
        if unit != current {
            let length = time.map_or(1.0, |time| time.tick_length);
            *pending = Some(Pending::one(SheetEdit::SetTime {
                time: unit.map(|unit| SheetTime::new(length, unit)),
            }));
        }
    });
    if time.is_some()
        && let Some(message) = field_message(ui, length_id)
    {
        message_ui(ui, &message);
    }
}

/// Draws the Cells table, with each property's start as controls, and the property form or "+ Add property" under
/// it. A sheet with no property says how to start.
fn cells(ui: &mut Ui, document: &SheetDocument, pane: &mut PaneState, stale: &[String], pending: &mut Option<Pending>) {
    let properties = document.cells.as_deref().unwrap_or_default();
    if properties.is_empty() {
        ui.label(SHEET_EMPTY);
    } else {
        egui::Grid::new("henad_sheet_cells").num_columns(4).show(ui, |ui| {
            for column in CELLS_COLUMNS {
                ui.weak(column);
            }
            ui.end_row();
            for property in properties {
                let edit = add_button(
                    ui,
                    true,
                    Button::new(&property.name).frame(false),
                    &edit_property_heading(&property.name),
                );
                if edit.clicked() {
                    pane.property_form = Some(PropertyForm::edit(property));
                }
                kind_ui(ui, property);
                ui.vertical(|ui| {
                    starts_ui(ui, document, property, pane, pending);
                    if stale.contains(&property.id) {
                        ui.weak(START_AGAIN_TO_SEE);
                    }
                });
                ui.label(changed_by(&changing_lines(document, &property.id)));
                ui.end_row();
            }
        });
    }
    ui.add_space(4.0);
    if let Some(form) = &mut pane.property_form {
        let mut close = false;
        property_form(ui, document, form, &mut close, pending);
        if close {
            pane.property_form = None;
        }
    } else {
        let button = if properties.is_empty() {
            filled_button(ADD_PROPERTY.to_owned())
        } else {
            Button::new(ADD_PROPERTY)
        };
        if ui.add(button).clicked() {
            pane.property_form = Some(PropertyForm::add());
        }
    }
    if let Some(refusal) = &pane.refusal {
        message_ui(ui, refusal);
    }
}

/// Draws the kind of `property`: "States" with each state's swatch and name, "Number" or "Integer".
fn kind_ui(ui: &mut Ui, property: &SheetProperty) {
    match property.property_type {
        PropertyType::States => {
            ui.vertical(|ui| {
                ui.label(STATES_KIND);
                for state in &property.states {
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(SWATCH_SIDE), Sense::hover());
                        ui.painter().rect_filled(rect, CornerRadius::ZERO, color32(state.color));
                        ui.label(&state.name);
                    });
                }
            });
        }
        PropertyType::WholeNumber => {
            ui.label(INTEGER_KIND).on_hover_text(INTEGER_KIND_TOOLTIP);
        }
        _ => {
            ui.label(NUMBER_KIND);
        }
    }
}

/// Returns how `property` starts: "All Dead", "By chance", "A value" or "A random number".
///
/// A start of another rule returns an empty string. Only the ground or the agents of an agent world take one.
pub fn starts_text(property: &SheetProperty) -> String {
    let Some(starts) = &property.starts_as else {
        return String::new();
    };
    match starts.rule {
        StartRule::ByChance => BY_CHANCE.to_owned(),
        StartRule::Value => match starts.value.value() {
            Some(SheetValue::Chip(SheetChip::State(state))) => match state_name(property, state) {
                Some(name) => starts_all(name),
                None => A_VALUE.to_owned(),
            },
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomNumber => A_RANDOM_NUMBER.to_owned(),
            _ => A_VALUE.to_owned(),
        },
        _ => String::new(),
    }
}

/// Returns whether `slot` holds a random number between two numbers.
fn holds_random_number(slot: &Slot) -> bool {
    matches!(slot.value(), Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomNumber)
}

/// Returns the name of state `state_id` of `property`.
fn state_name<'a>(property: &'a SheetProperty, state_id: &str) -> Option<&'a str> {
    property
        .states
        .iter()
        .find(|state| state.id == state_id)
        .map(|state| state.name.as_str())
}

/// A choice in the select of a Starts as.
enum StartChoice {
    /// Every cell starts in the state of this id.
    All(String),
    /// Each cell draws its state.
    ByChance,
    /// Every cell starts at one number.
    Value,
    /// Every cell draws a number between two numbers.
    RandomNumber,
}

/// Draws the Starts as of `property`: its select and, under it, the rows of a by-chance start or the numbers of a
/// Number property.
fn starts_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let selected = starts_text(property);
    let mut choice = None;
    let states = property.property_type == PropertyType::States;
    let combo = ComboBox::from_id_salt(("henad_sheet_starts", &property.id))
        .selected_text(&selected)
        .show_ui(ui, |ui| {
            if states {
                for state in &property.states {
                    let words = starts_all(&state.name);
                    if ui.selectable_label(selected == words, &words).clicked() {
                        choice = Some(StartChoice::All(state.id.clone()));
                    }
                }
                if ui.selectable_label(selected == BY_CHANCE, BY_CHANCE).clicked() {
                    choice = Some(StartChoice::ByChance);
                }
            } else {
                if ui.selectable_label(selected == A_VALUE, A_VALUE).clicked() {
                    choice = Some(StartChoice::Value);
                }
                // A draw truncated to a whole number would never reach the high end.
                if property.property_type == PropertyType::Number
                    && ui
                        .selectable_label(selected == A_RANDOM_NUMBER, A_RANDOM_NUMBER)
                        .clicked()
                {
                    choice = Some(StartChoice::RandomNumber);
                }
            }
        });
    set_name(ui, combo.response.id, &starts_place(&property.name));
    if let Some(choice) = choice {
        choose_start(property, &selected, choice, pending);
    }
    let Some(start) = &property.starts_as else {
        return;
    };
    match start.rule {
        StartRule::ByChance => chance_rows(ui, document, property, start, pane, pending),
        StartRule::Value if !states => {
            let random = holds_random_number(&start.value);
            let slots: &[StartSlot] = if random {
                &[StartSlot::Low, StartSlot::High]
            } else {
                &[StartSlot::Value]
            };
            ui.horizontal_wrapped(|ui| {
                if random {
                    ui.label(RANDOM_NUMBER_BETWEEN);
                    start_number(ui, document, property, start, StartSlot::Low, pane, pending);
                    ui.label(RANDOM_NUMBER_AND);
                    start_number(ui, document, property, start, StartSlot::High, pane, pending);
                } else {
                    ui.label(STARTS_AT);
                    start_number(ui, document, property, start, StartSlot::Value, pane, pending);
                }
            });
            start_messages(ui, pane.widgets, property, slots);
        }
        _ => {}
    }
}

/// Sets the start of `property` to `choice`, unless `selected`, the words of its start, already reads as it.
fn choose_start(property: &SheetProperty, selected: &str, choice: StartChoice, pending: &mut Option<Pending>) {
    let number = |value: f64| Slot::Filled(SheetValue::Literal(SheetLiteral::Number(value)));
    let start = match choice {
        StartChoice::All(state) => {
            let mut start = StartsAs::new(StartRule::Value);
            start.value = Slot::Filled(SheetValue::Chip(SheetChip::State(state)));
            start
        }
        StartChoice::ByChance if selected != BY_CHANCE => {
            // A new by-chance start draws the second state, and every other cell takes the first.
            let mut start = StartsAs::new(StartRule::ByChance);
            let first = property.states.first().map(|state| state.id.clone());
            let drawn = property.states.get(1).or(property.states.first());
            start.rows = drawn
                .map(|state| StartsAsRow::new(state.id.clone(), Slot::Blank))
                .into_iter()
                .collect();
            start.otherwise = first;
            start
        }
        StartChoice::Value if selected != A_VALUE => {
            let mut start = StartsAs::new(StartRule::Value);
            start.value = number(0.0);
            start
        }
        StartChoice::RandomNumber if selected != A_RANDOM_NUMBER => {
            let mut phrase = SheetLine::new(LineKind::RandomNumber);
            phrase.low = number(0.0);
            phrase.high = number(1.0);
            let mut start = StartsAs::new(StartRule::Value);
            start.value = Slot::Filled(SheetValue::Phrase(Box::new(phrase)));
            start
        }
        _ => return,
    };
    *pending = Some(Pending::one(SheetEdit::SetStartsAs {
        property: property.id.clone(),
        starts_as: Some(Box::new(start)),
    }));
}

/// Draws the rows of the by-chance `start` of `property`, each on a line of its own, then "+ Add row" and the state
/// of every other cell.
fn chance_rows(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    start: &StartsAs,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    for (index, row) in start.rows.iter().enumerate() {
        let place = starts_row_place(&property.name, index + 1);
        ui.horizontal_wrapped(|ui| {
            if let Some(state) = state_select(
                ui,
                property,
                row.state.as_deref(),
                ("henad_sheet_start_row", &property.id, index),
                &starts_field_name(&place, StartsField::State),
            ) {
                *pending = Some(start_edit(property.id.clone(), move |start| {
                    if let Some(row) = start.rows.get_mut(index) {
                        row.state = Some(state);
                    }
                }));
            }
            ui.label(WITH_CHANCE);
            start_number(ui, document, property, start, StartSlot::Row(index), pane, pending);
            let remove = add_button(
                ui,
                true,
                Button::new(REMOVE_ROW_LABEL).small(),
                &remove_row_name(index + 1, &property.name),
            );
            if remove.clicked() {
                // The rows after it move up a place. Otherwise a refused text would show on the row that takes its
                // place.
                for later in index..start.rows.len() {
                    forget_field(ui, start_field_id(pane.widgets, &property.id, StartSlot::Row(later)));
                }
                *pending = Some(start_edit(property.id.clone(), move |start| {
                    if index < start.rows.len() {
                        start.rows.remove(index);
                    }
                }));
            }
        });
        start_messages(ui, pane.widgets, property, &[StartSlot::Row(index)]);
    }
    let add = add_button(ui, true, Button::new(ADD_ROW).small(), &add_row_name(&property.name));
    if add.clicked() {
        // A new row draws the first state that no row draws yet.
        let drawn: Vec<Option<&str>> = start.rows.iter().map(|row| row.state.as_deref()).collect();
        let state = property
            .states
            .iter()
            .find(|state| !drawn.contains(&Some(state.id.as_str())))
            .or(property.states.first())
            .map(|state| state.id.clone());
        *pending = Some(start_edit(property.id.clone(), move |start| {
            let mut row = StartsAsRow::default();
            row.state = state;
            start.rows.push(row);
        }));
    }
    ui.horizontal(|ui| {
        ui.label(OTHERWISE);
        if let Some(state) = state_select(
            ui,
            property,
            start.otherwise.as_deref(),
            ("henad_sheet_start_otherwise", &property.id, 0),
            &starts_otherwise_name(&property.name),
        ) {
            *pending = Some(start_edit(property.id.clone(), move |start| {
                start.otherwise = Some(state);
            }));
        }
    });
}

/// Adds a select of the states of `property` showing `current`, named `name`, and returns the id of the state chosen.
fn state_select(
    ui: &mut Ui,
    property: &SheetProperty,
    current: Option<&str>,
    id_salt: (&str, &str, usize),
    name: &str,
) -> Option<String> {
    let shown = current
        .and_then(|state| state_name(property, state))
        .unwrap_or_default();
    let mut chosen = None;
    let combo = ComboBox::from_id_salt(id_salt).selected_text(shown).show_ui(ui, |ui| {
        for state in &property.states {
            if ui
                .selectable_label(current == Some(state.id.as_str()), &state.name)
                .clicked()
            {
                chosen = Some(state.id.clone());
            }
        }
    });
    set_name(ui, combo.response.id, name);
    chosen.filter(|state| current != Some(state.as_str()))
}

/// Returns the edit that changes the start of `property` by `change`, applied to the start as the sheet holds it.
fn start_edit(property: String, change: impl FnOnce(&mut StartsAs) + 'static) -> Pending {
    Pending {
        starts_only: true,
        from_form: false,
        run: Box::new(move |builder| {
            let mut start = start_of(builder.document(), &property)
                .ok_or_else(|| SheetEditError::Missing(format!("the property '{property}' has no start")))?;
            change(&mut start);
            builder
                .apply(SheetEdit::SetStartsAs {
                    property,
                    starts_as: Some(Box::new(start)),
                })
                .map(drop)
        }),
    }
}

/// Returns the start of property `property` of the cells of `document`.
fn start_of(document: &SheetDocument, property: &str) -> Option<StartsAs> {
    document
        .cells
        .iter()
        .flatten()
        .find(|known| known.id == property)
        .and_then(|known| known.starts_as.clone())
}

/// Returns the number at `slot` of `start`.
fn start_slot(start: &StartsAs, slot: StartSlot) -> Option<&Slot> {
    match slot {
        StartSlot::Row(index) => start.rows.get(index).map(|row| &row.chance),
        StartSlot::Value => Some(&start.value),
        StartSlot::Low | StartSlot::High => match start.value.value() {
            Some(SheetValue::Phrase(phrase)) => Some(if slot == StartSlot::Low {
                &phrase.low
            } else {
                &phrase.high
            }),
            _ => None,
        },
    }
}

/// Puts `value` at `slot` of `start`. A slot the start does not hold is left alone.
fn set_start_slot(start: &mut StartsAs, slot: StartSlot, value: Slot) {
    match slot {
        StartSlot::Row(index) => {
            if let Some(row) = start.rows.get_mut(index) {
                row.chance = value;
            }
        }
        StartSlot::Value => start.value = value,
        StartSlot::Low | StartSlot::High => {
            if let Slot::Filled(SheetValue::Phrase(phrase)) = &mut start.value {
                if slot == StartSlot::Low {
                    phrase.low = value;
                } else {
                    phrase.high = value;
                }
            }
        }
    }
}

/// Returns the id of the field of the number at `slot` of the start of `property`, under `widgets`, the id the pane's
/// widgets are keyed under.
fn start_field_id(widgets: Id, property: &str, slot: StartSlot) -> Id {
    widgets.with(("henad_sheet_start_field", property, slot))
}

/// Draws the message under the row of the numbers at `slots` of the start of `property`, if one shows.
fn start_messages(ui: &mut Ui, widgets: Id, property: &SheetProperty, slots: &[StartSlot]) {
    if let Some(message) = slots
        .iter()
        .find_map(|slot| field_message(ui, start_field_id(widgets, &property.id, *slot)))
    {
        message_ui(ui, &message);
    }
}

/// Draws the number at `slot` of the start of `property`: a field with "Use a parameter" beside it, or the chip of the
/// parameter chosen. A click on the chip reopens the full picker.
fn start_number(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    start: &StartsAs,
    slot: StartSlot,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let chance = matches!(slot, StartSlot::Row(_));
    let number_use = if chance { NumberUse::Chance } else { NumberUse::Number };
    let place = match slot {
        StartSlot::Row(index) => starts_row_place(&property.name, index + 1),
        StartSlot::Value => starts_place(&property.name),
        StartSlot::Low => starts_low_place(&property.name),
        StartSlot::High => starts_high_place(&property.name),
    };
    let current = start_slot(start, slot);
    let target = StartTarget {
        property: property.id.clone(),
        slot,
    };
    let choice = match current.and_then(Slot::value) {
        Some(SheetValue::Chip(SheetChip::Param(parameter))) => {
            let name = document
                .parameters
                .iter()
                .find(|known| known.id == *parameter)
                .map_or_else(|| parameter.clone(), |known| known.name.clone());
            let chip = SentenceChip {
                kind: ChipKind::Parameter(parameter.clone()),
                lead: "",
                name,
            };
            let response = chip_ui(ui, &chip, false, None);
            let blank = if chance { Blank::Chance } else { Blank::Number };
            let title = blank.picker_title();
            Picker {
                title: &title,
                groups: vec![parameters_group(document)],
                new_parameter: true,
                number_box: Some(number_use),
                empty: "",
            }
            .show(ui, &response)
        }
        value => {
            let text = match value {
                Some(SheetValue::Literal(literal)) => literal_words(*literal),
                _ => String::new(),
            };
            let field = if chance {
                StartsField::Chance
            } else {
                StartsField::Number
            };
            commit_field(
                ui,
                start_field_id(pane.widgets, &property.id, slot),
                &starts_field_name(&place, field),
                &text,
                FIELD_WIDTH,
                |typed| {
                    let value = if typed.trim().is_empty() {
                        Slot::Blank
                    } else {
                        match number_use.read(typed) {
                            Ok(literal) => Slot::Filled(SheetValue::Literal(literal)),
                            Err(message) if message == TYPE_ONE_NUMBER && chance => {
                                return Some(TYPE_A_CHANCE.to_owned());
                            }
                            Err(message) => return Some(message.to_owned()),
                        }
                    };
                    *pending = Some(start_edit(target.property.clone(), move |start| {
                        set_start_slot(start, slot, value);
                    }));
                    None
                },
            );
            let button = add_button(
                ui,
                true,
                Button::new(USE_A_PARAMETER).small(),
                &starts_field_name(&place, StartsField::UseParameter),
            );
            Picker {
                title: CHOOSE_A_PARAMETER,
                groups: vec![parameters_group(document)],
                new_parameter: true,
                number_box: None,
                empty: "",
            }
            .show(ui, &button)
        }
    };
    pick_start_number(
        document,
        &property.id,
        current,
        target,
        number_use,
        choice,
        pane,
        pending,
    );
}

/// Takes `choice`, made in the picker of the number `target` of a start that holds `current`: a parameter or a typed
/// number goes into the start, and "New parameter..." opens the New parameter dialog.
#[expect(
    clippy::too_many_arguments,
    reason = "the picker, the start and the pane each take a part"
)]
fn pick_start_number(
    document: &SheetDocument,
    property: &str,
    current: Option<&Slot>,
    target: StartTarget,
    number_use: NumberUse,
    choice: Option<PickerChoice>,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let slot = target.slot;
    let value = match choice {
        Some(PickerChoice::Item { item, .. }) => match document.parameters.get(item) {
            Some(parameter) => Slot::Filled(SheetValue::Chip(SheetChip::Param(parameter.id.clone()))),
            None => return,
        },
        Some(PickerChoice::NewParameter) => {
            let typed = match current.and_then(Slot::value) {
                Some(SheetValue::Literal(literal)) => literal.number(),
                _ => None,
            };
            pane.new_parameter = Some((target, NewParameterDialog::new(number_use, typed, None)));
            return;
        }
        Some(PickerChoice::Number(literal)) => Slot::Filled(SheetValue::Literal(literal)),
        None => return,
    };
    *pending = Some(start_edit(property.to_owned(), move |start| {
        set_start_slot(start, slot, value);
    }));
}

/// Returns the group of the sheet's parameters in a picker, each with its value and unit in grey.
fn parameters_group(document: &SheetDocument) -> PickerGroup {
    PickerGroup {
        heading: PARAMETERS,
        items: document
            .parameters
            .iter()
            .map(|parameter| {
                let value = format_number(parameter.default).unwrap_or_default();
                PickerItem {
                    label: parameter.name.clone(),
                    detail: Some(match &parameter.unit {
                        Some(unit) => format!("{value} {unit}"),
                        None => value,
                    }),
                }
            })
            .collect(),
    }
}

/// Draws the New parameter dialog while it is open, and on Create adds the parameter and puts it in the Starts as
/// number it was opened from, as one edit.
///
/// The edit counts as a changed start, since only the start uses the new parameter. Past the start it therefore
/// waits for Start again, as a parameter chosen from the picker does.
fn new_parameter(ui: &Ui, document: &SheetDocument, pane: &mut PaneState, pending: &mut Option<Pending>) {
    let Some((target, dialog)) = &mut pane.new_parameter else {
        return;
    };
    let taken: Vec<&str> = document.parameters.iter().map(|known| known.name.as_str()).collect();
    match new_parameter_modal(ui.ctx(), dialog, &taken) {
        DialogOutcome::Open => {}
        DialogOutcome::Canceled => pane.new_parameter = None,
        DialogOutcome::Created(parameter) => {
            let StartTarget { property, slot } = target.clone();
            pane.new_parameter = None;
            *pending = Some(Pending {
                starts_only: true,
                from_form: false,
                run: Box::new(move |builder| {
                    let made = builder.apply(SheetEdit::AddParameter {
                        name: parameter.name,
                        parameter_type: henad_graph::sheet::document::ParameterType::Number,
                        min: parameter.min,
                        max: parameter.max,
                        default: parameter.default,
                        unit: parameter.unit,
                    })?;
                    let id = made.into_iter().next().unwrap_or_default();
                    (start_edit(property, move |start| {
                        set_start_slot(start, slot, Slot::Filled(SheetValue::Chip(SheetChip::Param(id))));
                    })
                    .run)(builder)
                }),
            });
        }
    }
}

/// Draws the property form, and sets `close` on Cancel. Done checks the form and adds or changes the property.
fn property_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut PropertyForm,
    close: &mut bool,
    pending: &mut Option<Pending>,
) {
    Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let heading = match &form.editing {
            Some(_) => edit_property_heading(&form.original),
            None => NEW_PROPERTY.to_owned(),
        };
        ui.strong(heading);
        ui.horizontal(|ui| {
            ui.label(NAME_LABEL);
            let field = ui.add(TextEdit::singleline(&mut form.name).hint_text(PROPERTY_NAME_PLACEHOLDER));
            set_name(ui, field.id, PROPERTY_NAME_FIELD);
            if std::mem::take(&mut form.focus_name) {
                field.request_focus();
            }
        });
        ui.horizontal(|ui| {
            ui.label(KIND_LABEL);
            ui.radio_value(&mut form.kind, PropertyType::States, STATES_KIND);
            ui.radio_value(&mut form.kind, PropertyType::Number, NUMBER_KIND);
            ui.radio_value(&mut form.kind, PropertyType::WholeNumber, INTEGER_KIND)
                .on_hover_text(INTEGER_KIND_TOOLTIP);
        });
        match form.kind {
            PropertyType::States => state_rows(ui, form),
            PropertyType::WholeNumber => {
                ui.weak(INTEGER_KIND_TOOLTIP);
            }
            _ => {
                ui.weak(NUMBER_KIND_DESCRIPTION);
            }
        }
        if let Some(message) = &form.message {
            message_ui(ui, message);
        }
        ui.horizontal(|ui| {
            if ui.add(filled_button(DONE.to_owned())).clicked() {
                match check_form(document, form) {
                    Ok(()) => {
                        *pending = Some(Pending {
                            starts_only: false,
                            from_form: true,
                            run: form_edit(form.clone()),
                        });
                    }
                    Err(message) => form.message = Some(message),
                }
            }
            if ui.button(CANCEL).clicked() {
                *close = true;
            }
            if let Some(id) = &form.editing
                && ui.add(destructive_button(DELETE_PROPERTY)).clicked()
            {
                *pending = Some(Pending {
                    starts_only: false,
                    from_form: true,
                    run: Box::new({
                        let item = SheetItem::Property(id.clone());
                        move |builder| builder.apply(SheetEdit::Delete { item }).map(drop)
                    }),
                });
            }
        });
    });
}

/// Draws the rows of states of the property form: each with its colour, its name and its remove button, then "+ Add
/// state". Enter in a name moves to the next state, or adds one after the last.
fn state_rows(ui: &mut Ui, form: &mut PropertyForm) {
    ui.weak(STATES_KIND_DESCRIPTION);
    let mut remove = None;
    let mut entered = None;
    let focus = form.focus_state.take();
    for (index, state) in form.states.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            let color_label = state_color_name(index + 1, color_name(state.color));
            if let Some(color) = color_button(ui, &color_label, state.color) {
                state.color = color;
                state.picked = true;
            }
            let field = ui.add(TextEdit::singleline(&mut state.name).desired_width(STATE_FIELD_WIDTH));
            set_name(ui, field.id, &state_name_field(index + 1));
            if focus == Some(index) {
                field.request_focus();
            }
            if field.changed() && !state.picked {
                state.color = suggested_state_color(&state.name).unwrap_or_else(|| default_color(index));
            }
            if field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                entered = Some(index);
            }
            let button = add_button(
                ui,
                true,
                Button::new(REMOVE_ROW_LABEL).small(),
                &remove_state_name(index + 1),
            );
            if button.clicked() {
                remove = Some(index);
            }
        });
    }
    if let Some(index) = remove {
        form.states.remove(index);
    }
    if let Some(index) = entered {
        if index + 1 == form.states.len() {
            form.states.push(FormState::empty(index + 1));
        }
        form.focus_state = Some(index + 1);
    }
    if ui.add(Button::new(ADD_STATE).small()).clicked() {
        form.states.push(FormState::empty(form.states.len()));
        form.focus_state = Some(form.states.len() - 1);
    }
}

/// Returns the colour a new state takes at row `index`.
fn default_color(index: usize) -> Color {
    STATE_COLORS[index % STATE_COLORS.len()]
}

/// Returns the message for the first check of the property form that fails, in the order of the page.
///
/// # Errors
///
/// Returns the message on an empty or taken name, a States property with fewer than two named states, or two states
/// of one name.
fn check_form(document: &SheetDocument, form: &PropertyForm) -> Result<(), String> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(PROPERTY_NAME_MISSING.to_owned());
    }
    let taken = document
        .cells
        .iter()
        .flatten()
        .filter(|property| Some(&property.id) != form.editing.as_ref())
        .any(|property| property.name.to_lowercase() == name.to_lowercase());
    if taken {
        return Err(property_name_taken(name));
    }
    if form.kind == PropertyType::States {
        let names: Vec<String> = form
            .states
            .iter()
            .map(|state| state.name.trim().to_lowercase())
            .filter(|state| !state.is_empty())
            .collect();
        if names.len() < 2 {
            return Err(TOO_FEW_STATES.to_owned());
        }
        if (1..names.len()).any(|index| names[..index].contains(&names[index])) {
            return Err(STATE_NAMES_REPEATED.to_owned());
        }
    }
    Ok(())
}

/// Returns the edit Done makes from `form`: a new property with its states, starting in its first state or at 0, or
/// the changes to a property the form edits.
///
/// A changed kind clears every reference to the property and starts it afresh. A removed state clears its references,
/// its measures and its start rows.
fn form_edit(form: PropertyForm) -> EditRun {
    Box::new(move |builder| {
        let states: Vec<FormState> = form
            .states
            .into_iter()
            .filter(|state| !state.name.trim().is_empty())
            .collect();
        let name = form.name.trim().to_owned();
        let id = match &form.editing {
            None => builder
                .apply(SheetEdit::AddProperty {
                    thing: CELLS.to_owned(),
                    name,
                    property_type: form.kind,
                })?
                .into_iter()
                .next()
                .unwrap_or_default(),
            Some(id) => {
                if name != form.original {
                    builder.apply(SheetEdit::Rename {
                        item: SheetItem::Property(id.clone()),
                        name,
                    })?;
                }
                id.clone()
            }
        };
        let current = builder
            .document()
            .cells
            .iter()
            .flatten()
            .find(|property| property.id == id)
            .cloned()
            .ok_or_else(|| SheetEditError::Missing(format!("the sheet has no property '{id}'")))?;
        let fresh = form.editing.is_none() || current.property_type != form.kind;
        if !fresh {
            if form.kind == PropertyType::States {
                change_states(builder, &current, &states)?;
            }
            return Ok(());
        }
        if form.editing.is_some() {
            builder.apply(SheetEdit::SetPropertyType {
                property: id.clone(),
                property_type: form.kind,
            })?;
        }
        let mut first = None;
        if form.kind == PropertyType::States {
            for state in &states {
                let made = builder.apply(SheetEdit::AddState {
                    property: id.clone(),
                    name: state.name.trim().to_owned(),
                    color: Some(state.color),
                })?;
                if first.is_none() {
                    first = made.into_iter().next();
                }
            }
        }
        let mut start = StartsAs::new(StartRule::Value);
        start.value = Slot::Filled(match first {
            Some(state) => SheetValue::Chip(SheetChip::State(state)),
            None => SheetValue::Literal(SheetLiteral::Number(0.0)),
        });
        builder
            .apply(SheetEdit::SetStartsAs {
                property: id,
                starts_as: Some(Box::new(start)),
            })
            .map(drop)
    })
}

/// Changes the states of `property` to the rows `states` of the property form: removed states go, the others take
/// their new names and colours, and new rows become new states.
fn change_states(
    builder: &mut SheetBuilder,
    property: &SheetProperty,
    states: &[FormState],
) -> Result<(), SheetEditError> {
    let kept: Vec<&str> = states.iter().filter_map(|state| state.id.as_deref()).collect();
    for state in &property.states {
        if !kept.contains(&state.id.as_str()) {
            builder.apply(SheetEdit::Delete {
                item: SheetItem::State {
                    property: property.id.clone(),
                    state: state.id.clone(),
                },
            })?;
        }
    }
    for row in states {
        let name = row.name.trim().to_owned();
        let Some(id) = &row.id else {
            builder.apply(SheetEdit::AddState {
                property: property.id.clone(),
                name,
                color: Some(row.color),
            })?;
            continue;
        };
        let Some(state) = property.states.iter().find(|state| state.id == *id) else {
            continue;
        };
        let item = || SheetItem::State {
            property: property.id.clone(),
            state: id.clone(),
        };
        if state.name != name {
            builder.apply(SheetEdit::Rename { item: item(), name })?;
        }
        if state.color != row.color {
            builder.apply(SheetEdit::Recolor {
                item: item(),
                color: row.color,
            })?;
        }
    }
    Ok(())
}

/// Returns the numbers of the lines that change property `property_id` on the cards that run each tick.
fn changing_lines(document: &SheetDocument, property_id: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    for card in document.cards.iter().filter(|card| card.runs == CardRuns::EachTick) {
        let relations = CardRelations::new(document, card);
        for &line_id in relations.order() {
            let changes = relations.line(line_id).is_some_and(|line| {
                matches!(line.kind, LineKind::Become | LineKind::Set) && line.property.as_deref() == Some(property_id)
            });
            if changes && let Some(number) = relations.number(line_id) {
                numbers.push(number);
            }
        }
    }
    numbers
}

/// Draws each parameter's name and value, with its unit, and under a parameter in `trials` the value the world tries.
///
/// The rows sit in one grid until a tried parameter. The trying line then spans the pane, and the next rows start a
/// new grid. Every grid takes the same minimum first-column width, so the values stay aligned. The line wraps at the
/// visible right edge of the pane. The pane's content can be wider than the pane, as with a wide Cells table, and a
/// line wrapped at the content's edge would be clipped.
fn parameters(ui: &mut Ui, document: &SheetDocument, trials: &[SheetTrial]) {
    let tried = |parameter: &SheetParameter| trials.iter().find(|trial| trial.parameter == parameter.id);
    let mut rest = document.parameters.as_slice();
    let mut segment = 0;
    while !rest.is_empty() {
        let end = rest
            .iter()
            .position(|parameter| tried(parameter).is_some())
            .map_or(rest.len(), |index| index + 1);
        let (shown, tail) = rest.split_at(end);
        kv_grid(ui, &format!("henad_sheet_parameters_{segment}")).show(ui, |ui, rows| {
            for parameter in shown {
                ui.label(&parameter.name);
                let value = format_number(parameter.default).unwrap_or_default();
                match &parameter.unit {
                    Some(unit) => ui.label(format!("{value} {unit}")),
                    None => ui.label(value),
                };
                rows.end_row(ui);
            }
        });
        if let Some(parameter) = shown.last()
            && let Some(trial) = tried(parameter)
        {
            let kept = format_number(parameter.default).unwrap_or_default();
            let tried = format_number(trial.value).unwrap_or_default();
            ui.scope(|ui| {
                ui.set_max_width((ui.clip_rect().right() - ui.cursor().left()).max(0.0));
                ui.add(egui::Label::new(RichText::new(sheet_trying_value(&tried, &kept)).weak()).wrap());
            });
        }
        rest = tail;
        segment += 1;
    }
}

/// Draws each measure's name.
fn measures(ui: &mut Ui, document: &SheetDocument) {
    for measure in &document.measures {
        ui.label(&measure.name);
    }
}

/// Draws the property that colours the cells.
fn colours(ui: &mut Ui, document: &SheetDocument) {
    let Some(rule) = document.colors.get(CELLS) else {
        return;
    };
    let property = rule.property.as_deref().and_then(|id| {
        document
            .cells
            .iter()
            .flatten()
            .find(|property| property.id == id)
            .map(|property| property.name.as_str())
    });
    if let (ColorRuleKind::ByState | ColorRuleKind::Shaded, Some(property)) = (rule.rule, property) {
        ui.label(format!("{COLOR_CELLS_BY} {property}"));
    }
}

/// Draws each button's name and what it does.
fn buttons(ui: &mut Ui, document: &SheetDocument) {
    for button in &document.buttons {
        let does: Vec<String> = button
            .does
            .iter()
            .map(|step| match step.rule {
                ButtonStepRule::StartAgain => START_AGAIN.to_owned(),
                ButtonStepRule::RunOnce => run_once("cell"),
                _ => SET_EVERYWHERE.to_owned(),
            })
            .collect();
        ui.label(format!("{}: {}", button.name, does.join(", ")));
    }
}

/// What a button step does that sets one property everywhere. No control of a grid sheet writes such a step.
const SET_EVERYWHERE: &str = "Set everywhere";
