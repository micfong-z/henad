//! The Cells section of the Sheet pane and the parts of it a world of agents shares: the table of a kind of thing's
//! properties, the property form or "+ Add property" under it, each property's start with its select, its chance rows
//! and its number fields, a number field that takes a parameter, and the New parameter dialog it opens.

use egui::{Button, Id, Label, Response, RichText, Ui};
use henad_graph::sheet::document::{
    CELLS_ID, GROUND_ID, LineKind, PropertyType, SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetProperty,
    SheetValue, Slot, StartRule, StartsAs, StartsAsRow,
};
use henad_graph::sheet::edit::{SheetEdit, SheetEditError};
use henad_graph::sheet::sentence::{
    A_RANDOM_NUMBER, A_VALUE, ADD_PROPERTY, ADD_ROW, BY_CHANCE, Blank, CELLS_COLUMNS, CHOOSE_A_PARAMETER, ChipKind,
    INTEGER_KIND, INTEGER_KIND_TOOLTIP, NUMBER_KIND, OTHERWISE, RANDOM_NUMBER_AND, RANDOM_NUMBER_BETWEEN,
    REMOVE_ROW_LABEL, SHEET_EMPTY, START_AGAIN_TO_SEE, STATES_KIND, SentenceChip, Sentences, StartsField,
    TYPE_A_CHANCE, TYPE_ONE_NUMBER, USE_A_PARAMETER, WITH_CHANCE, add_row_name, changed_by, edit_property_heading,
    literal_words, remove_row_name, starts_all, starts_field_name, starts_high_place, starts_low_place,
    starts_otherwise_name, starts_place, starts_row_place,
};
use henad_graph::sheet::sentence::{
    ANYWHERE_AT_RANDOM, ARROW_KIND, HEADING_KIND, HEADING_START, NO, PAINTED, POSITION_KIND, YES, YES_NO_KIND,
    every_starts_at, on_ground_start, painted_shapes,
};

use super::layout::{Column, ITEM_GAP, Table, delete_button, line, text_cell, text_width, wrapped_line};
use super::property_form::{changing_lines, property_form};
use super::{
    FIELD_WIDTH, NumberField, NumberTarget, PaneState, Pending, PropertyForm, StartSlot, select, select_width,
};
use crate::ui::editor::sheet::picker::{Choice, ParameterPicker};
use crate::ui::editor::sheet::widgets::FocusTooltip as _;
use crate::ui::editor::sheet::widgets::{
    CHIP_HEIGHT, DialogOutcome, NewParameterDialog, NumberUse, OverlayItem, caption_width, chip_within, color32,
    commit_number, field_message, forget_field, message_ui, new_parameter_modal, parameter_names, swatch_ui,
    text_control, text_control_width,
};
use crate::ui::editor::sheet::{find_property, properties_of};
use crate::ui::sweep::layout::{add_button, filled_button};

/// Draws the Cells table, with each property's start as controls, and the property form or "+ Add property" under
/// it. A sheet with no property says how to start.
pub(super) fn cells(
    ui: &mut Ui,
    document: &SheetDocument,
    pane: &mut PaneState,
    stale: &[String],
    pending: &mut Vec<Pending>,
) {
    let properties = properties_of(document, CELLS_ID);
    if properties.is_empty() {
        text_cell(ui, Label::new(SHEET_EMPTY));
    } else {
        property_table(ui, document, CELLS_ID, pane, stale, pending);
    }
    ui.add_space(6.0);
    property_form_or_add(
        ui,
        document,
        CELLS_ID,
        ADD_PROPERTY,
        properties.is_empty(),
        pane,
        pending,
    );
    if let Some(refusal) = &pane.refusal {
        message_ui(ui, refusal);
    }
}

/// Draws the table of the properties of `thing`: each property's name, which opens the property form, its kind, its
/// start as controls, and the lines that change it. `stale` holds the properties whose start waits for Start again.
///
/// Starts as takes the room left over. Short of room, Starts as gives up width down to 150 points, then Changed by,
/// Kind and Name down to their longest words, wrapping at them, then Starts as down to the width of "Use a parameter",
/// its selects and chips ending in an ellipsis, then Kind down to the longest word of a state's name without its
/// swatch, the names wrapping under their swatches, and last Kind and Name down to their least widths, a word too long
/// for them ending in an ellipsis.
pub(super) fn property_table(
    ui: &mut Ui,
    document: &SheetDocument,
    thing: &str,
    pane: &mut PaneState,
    stale: &[String],
    pending: &mut Vec<Pending>,
) {
    let properties = properties_of(document, thing);
    let widest = |width: &dyn Fn(&SheetProperty) -> f32| properties.iter().map(width).fold(0.0, f32::max);
    let changed = |property: &SheetProperty| changed_by(&changing_lines(document, &property.id));
    let names = widest(&|property| text_width(ui, &property.name));
    let name_words = widest(&|property| longest_word(ui, &property.name));
    let kinds = widest(&|property| kind_width(ui, property, text_width, STATE_SWATCH_WIDTH));
    let kind_words = widest(&|property| kind_width(ui, property, longest_word, STATE_SWATCH_WIDTH));
    // A state's name that wraps under its swatch needs no room beside the swatch.
    let kind_under = widest(&|property| kind_width(ui, property, longest_word, 0.0));
    let starts = widest(&|property| starts_width(ui, document, thing, property));
    let changes = widest(&|property| text_width(ui, &changed(property)));
    let change_words = widest(&|property| longest_word(ui, &changed(property)));
    // "Use a parameter" is the widest control of Starts as that neither wraps nor ends in an ellipsis.
    let starts_least = text_control_width(ui, USE_A_PARAMETER);
    let header_word = |index: usize| {
        CELLS_COLUMNS[index]
            .split_whitespace()
            .map(|word| caption_width(ui, word))
            .fold(0.0, f32::max)
    };
    let columns = vec![
        Column::new(
            CELLS_COLUMNS[0],
            names.max(caption_width(ui, CELLS_COLUMNS[0])),
            header_word(0).max(40.0),
        ),
        Column::new(
            CELLS_COLUMNS[1],
            kinds.max(caption_width(ui, CELLS_COLUMNS[1])),
            header_word(1).max(56.0),
        ),
        Column::new(CELLS_COLUMNS[2], starts, starts_least),
        Column::new(
            CELLS_COLUMNS[3],
            changes.max(caption_width(ui, CELLS_COLUMNS[3])),
            change_words.max(header_word(3)),
        ),
    ];
    let id = ui.make_persistent_id(("henad_sheet_properties", thing));
    let give_up = [
        (2, 150.0),
        (3, 0.0),
        (1, kind_words),
        (0, name_words),
        (2, starts_least),
        (1, kind_under),
        (1, 0.0),
        (0, 0.0),
    ];
    Table::new(id, columns, 2, &give_up).show(ui, |ui, rows| {
        for property in properties {
            rows.row(ui, &property.id, |row| {
                row.cell(0, |ui| {
                    line(ui, |ui| {
                        if property_name_ui(ui, property).clicked() {
                            pane.property_form = Some(PropertyForm::edit(property, thing));
                        }
                    });
                });
                row.cell(1, |ui| kind_ui(ui, property));
                row.cell(2, |ui| {
                    starts_ui(ui, document, thing, property, pane, pending);
                    if stale.contains(&property.id) {
                        ui.weak(START_AGAIN_TO_SEE);
                    }
                });
                row.cell(3, |ui| {
                    text_cell(ui, Label::new(changed(property)));
                });
            });
        }
    });
}

/// Returns the width in points of the longest word of `text` in the body font.
fn longest_word(ui: &Ui, text: &str) -> f32 {
    text.split_whitespace()
        .map(|word| text_width(ui, word))
        .fold(0.0, f32::max)
}

/// Adds the name of `property` as a frameless button named `edit_property_heading`. A click opens the property form.
/// A name too long for its column wraps, and a word too long for it ends in an ellipsis, whole in the tooltip.
fn property_name_ui(ui: &mut Ui, property: &SheetProperty) -> Response {
    // With no padding, the name starts on the column's edge, under its header.
    ui.spacing_mut().button_padding.x = 0.0;
    let elided = longest_word(ui, &property.name) > ui.available_width();
    let button = Button::new(&property.name).frame(false);
    let button = if elided { button.truncate() } else { button.wrap() };
    let response = add_button(ui, true, button, &edit_property_heading(&property.name));
    if elided {
        response.on_hover_or_focus_text(&property.name)
    } else {
        response
    }
}

/// Returns a label of `text` that wraps at the right edge of `ui`, or ends in an ellipsis, whole in its tooltip, where
/// a word of it is too long to wrap.
fn fitted_label(ui: &Ui, text: impl Into<egui::WidgetText>) -> Label {
    let text = text.into();
    if longest_word(ui, text.text()) > ui.available_width() {
        Label::new(text).truncate()
    } else {
        Label::new(text).wrap()
    }
}

/// Returns the width in points the kind of `property` takes: its word, or the name of its widest state with `swatch`
/// points for the swatch before it, each measured by `measure`, the width of a whole text or of its longest word.
fn kind_width(ui: &Ui, property: &SheetProperty, measure: fn(&Ui, &str) -> f32, swatch: f32) -> f32 {
    let word = measure(ui, kind_words(property.property_type));
    property
        .states
        .iter()
        .map(|state| measure(ui, &state.name) + swatch)
        .fold(word, f32::max)
}

/// Width in points of the swatch before a state's name, with the gap after it.
pub(super) const STATE_SWATCH_WIDTH: f32 = 13.0;

/// Returns the width in points the Starts as of `property`, a property of `thing`, takes while its first rows fit on
/// one line each.
fn starts_width(ui: &Ui, document: &SheetDocument, thing: &str, property: &SheetProperty) -> f32 {
    let selected = starts_text(document, property);
    let mut width = select_width(ui, &selected);
    let Some(start) = &property.starts_as else {
        return width;
    };
    let widest_state = property
        .states
        .iter()
        .map(|state| select_width(ui, &state.name))
        .fold(0.0, f32::max);
    let row = match start.rule {
        StartRule::ByChance => widest_state + text_width(ui, WITH_CHANCE) + FIELD_WIDTH + 2.0 * ITEM_GAP,
        StartRule::Value if !matches!(property.property_type, PropertyType::States | PropertyType::YesNo) => {
            let lead = if holds_random_number(&start.value) {
                RANDOM_NUMBER_BETWEEN.to_owned()
            } else {
                every_starts_at(&Sentences::new(document).thing_word(thing))
            };
            text_width(ui, &lead) + FIELD_WIDTH + ITEM_GAP
        }
        _ => 0.0,
    };
    width = width.max(row);
    width.min(STARTS_MOST)
}

/// Width in points the Starts as column takes at most while the table fits.
const STARTS_MOST: f32 = 280.0;

/// Draws the property form under the table of `thing` while it is open for that thing, or the button that opens it,
/// named `add_name`. The button is the section's primary action while `primary` holds, and an add button otherwise.
pub(super) fn property_form_or_add(
    ui: &mut Ui,
    document: &SheetDocument,
    thing: &str,
    add_name: &str,
    primary: bool,
    pane: &mut PaneState,
    pending: &mut Vec<Pending>,
) {
    if let Some(form) = pane.property_form.as_mut().filter(|form| form.thing == thing) {
        let mut close = false;
        property_form(ui, document, form, &mut close, pending);
        if close {
            pane.property_form = None;
        }
        return;
    }
    let opened = if primary {
        add_button(ui, true, filled_button(ADD_PROPERTY.to_owned()), add_name).clicked()
    } else {
        crate::ui::editor::sheet::widgets::add_row_button(ui, ADD_PROPERTY, add_name, None, false).clicked()
    };
    if opened {
        pane.property_form = Some(PropertyForm::add(thing));
    }
}

/// Draws the kind of `property`: "States" with each state's swatch and name under it, "Number", "Integer", "Yes or
/// no", "Position", "Heading" or "Arrow". A state's name too long for the column wraps, or ends in an ellipsis where a
/// word of it does not fit.
fn kind_ui(ui: &mut Ui, property: &SheetProperty) {
    match property.property_type {
        PropertyType::States => {
            text_cell(ui, fitted_label(ui, STATES_KIND));
            ui.spacing_mut().item_spacing.y = 0.0;
            for state in &property.states {
                // The column's width, taken before the swatch: a name that fits it wraps under the swatch.
                let label = fitted_label(ui, &state.name);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x =
                        STATE_SWATCH_WIDTH - crate::ui::editor::sheet::widgets::CHIP_SWATCH;
                    swatch_ui(ui, color32(state.color));
                    ui.add(label);
                });
            }
        }
        PropertyType::WholeNumber => {
            text_cell(ui, fitted_label(ui, INTEGER_KIND)).on_hover_text(INTEGER_KIND_TOOLTIP);
        }
        kind => {
            text_cell(ui, fitted_label(ui, kind_words(kind)));
        }
    }
}

/// Returns the word of a property's kind, such as "Number" or "Yes or no".
pub(super) fn kind_words(kind: PropertyType) -> &'static str {
    match kind {
        PropertyType::States => STATES_KIND,
        PropertyType::WholeNumber => INTEGER_KIND,
        PropertyType::YesNo => YES_NO_KIND,
        PropertyType::Position => POSITION_KIND,
        PropertyType::Heading => HEADING_KIND,
        PropertyType::Arrow => ARROW_KIND,
        _ => NUMBER_KIND,
    }
}

/// Returns how `property` of `document` starts: "All Dead", "By chance", "A value", "A random number", "Anywhere at
/// random", "On a cell whose Ground is Nest", "Painted" or "All Yes".
///
/// A Heading, which takes its first value from the first move, returns [`HEADING_START`], and a property with no
/// start an empty string.
pub fn starts_text(document: &SheetDocument, property: &SheetProperty) -> String {
    let Some(starts) = &property.starts_as else {
        return if property.property_type == PropertyType::Heading {
            HEADING_START.to_owned()
        } else {
            String::new()
        };
    };
    match starts.rule {
        StartRule::ByChance => BY_CHANCE.to_owned(),
        StartRule::Anywhere => ANYWHERE_AT_RANDOM.to_owned(),
        StartRule::Painted => PAINTED.to_owned(),
        StartRule::OnGround => {
            let ground = starts.property.as_deref().and_then(|id| find_property(document, id));
            let state = ground
                .zip(starts.state.as_deref())
                .and_then(|(ground, state)| state_name(ground, state));
            match (ground, state) {
                (Some(ground), Some(state)) => on_ground_start(&ground.name, state),
                _ => String::new(),
            }
        }
        StartRule::Value => match starts.value.value() {
            Some(SheetValue::Chip(SheetChip::State(state))) => match state_name(property, state) {
                Some(name) => starts_all(name),
                None => A_VALUE.to_owned(),
            },
            Some(SheetValue::Literal(SheetLiteral::Bool(yes))) => starts_all(if *yes { YES } else { NO }),
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
pub(super) fn state_name<'a>(property: &'a SheetProperty, state_id: &str) -> Option<&'a str> {
    property
        .states
        .iter()
        .find(|state| state.id == state_id)
        .map(|state| state.name.as_str())
}

/// A choice in the select of a Starts as.
enum StartChoice {
    /// Every element starts in the state of this id.
    All(String),
    /// Each element draws its state.
    ByChance,
    /// Every element starts at one number.
    Value,
    /// Every element draws a number between two numbers.
    RandomNumber,
    /// Every element starts at yes or at no.
    YesNo(bool),
    /// Every agent starts at a random place.
    Anywhere,
    /// Every agent starts on a cell whose ground property of the first id is the state of the second.
    OnGround(String, String),
    /// The painted start. Choosing it changes nothing.
    Painted,
}

/// Adds the select of the Starts as of `property`, showing `selected`, and returns the choice made in it.
fn start_select(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    selected: &str,
) -> Option<StartChoice> {
    let mut offered: Vec<(String, StartChoice)> = Vec::new();
    match property.property_type {
        PropertyType::States => {
            // No control paints, so a painted start shows as its own entry, and choosing it again changes nothing.
            if selected == PAINTED {
                offered.push((PAINTED.to_owned(), StartChoice::Painted));
            }
            for state in &property.states {
                offered.push((starts_all(&state.name), StartChoice::All(state.id.clone())));
            }
            offered.push((BY_CHANCE.to_owned(), StartChoice::ByChance));
        }
        PropertyType::YesNo => {
            offered.push((starts_all(YES), StartChoice::YesNo(true)));
            offered.push((starts_all(NO), StartChoice::YesNo(false)));
        }
        PropertyType::Position => {
            offered.push((ANYWHERE_AT_RANDOM.to_owned(), StartChoice::Anywhere));
            for ground in properties_of(document, GROUND_ID)
                .iter()
                .filter(|ground| ground.property_type == PropertyType::States)
            {
                let painted = ground.starts_as.as_ref();
                for state in ground
                    .states
                    .iter()
                    .filter(|state| painted.is_some_and(|start| start.paints_single_cells(&state.id)))
                {
                    offered.push((
                        on_ground_start(&ground.name, &state.name),
                        StartChoice::OnGround(ground.id.clone(), state.id.clone()),
                    ));
                }
            }
        }
        kind => {
            offered.push((A_VALUE.to_owned(), StartChoice::Value));
            // A draw truncated to a whole number would never reach the high end.
            if kind == PropertyType::Number {
                offered.push((A_RANDOM_NUMBER.to_owned(), StartChoice::RandomNumber));
            }
        }
    }
    let items: Vec<OverlayItem<'_>> = offered
        .iter()
        .map(|(words, _)| OverlayItem {
            label: words,
            current: words == selected,
            ..OverlayItem::default()
        })
        .collect();
    let (_, chosen) = line(ui, |ui| {
        select(
            ui,
            ("henad_sheet_starts", &property.id),
            &starts_place(&property.name),
            selected,
            &items,
        )
    });
    let index = chosen?;
    offered.into_iter().nth(index).map(|(_, choice)| choice)
}

/// Draws the Starts as of `property`, a property of `thing`: its select and, under it, the rows of a by-chance start,
/// the numbers of a Number property or the shapes of a painted one.
///
/// A Heading shows that it starts with the first move, and an Arrow its start in words, since neither has a control
/// of its own.
fn starts_ui(
    ui: &mut Ui,
    document: &SheetDocument,
    thing: &str,
    property: &SheetProperty,
    pane: &mut PaneState,
    pending: &mut Vec<Pending>,
) {
    let selected = starts_text(document, property);
    match property.property_type {
        PropertyType::Heading => {
            text_cell(ui, Label::new(RichText::new(&selected).weak()));
            return;
        }
        PropertyType::Arrow => {
            if let Some(start) = &property.starts_as {
                let words = Sentences::new(document).slot(&start.value, Some(&property.id));
                text_cell(ui, Label::new(RichText::new(words).weak()));
            }
            return;
        }
        _ => {}
    }
    if let Some(choice) = start_select(ui, document, property, &selected) {
        choose_start(property, &selected, choice, pending);
    }
    let Some(start) = &property.starts_as else {
        return;
    };
    let states = property.property_type == PropertyType::States;
    match start.rule {
        StartRule::ByChance => chance_rows(ui, document, property, start, pane, pending),
        StartRule::Painted => {
            let base = start
                .base
                .as_deref()
                .and_then(|base| state_name(property, base))
                .unwrap_or_default();
            ui.add(Label::new(RichText::new(painted_shapes(base, start.shapes.len())).weak()).wrap());
        }
        StartRule::Value if !states && property.property_type != PropertyType::YesNo => {
            let random = holds_random_number(&start.value);
            let slots: &[StartSlot] = if random {
                &[StartSlot::Low, StartSlot::High]
            } else {
                &[StartSlot::Value]
            };
            wrapped_line(ui, |ui| {
                if random {
                    ui.label(RANDOM_NUMBER_BETWEEN);
                    start_number(ui, document, property, start, StartSlot::Low, pane, pending);
                    ui.label(RANDOM_NUMBER_AND);
                    start_number(ui, document, property, start, StartSlot::High, pane, pending);
                } else {
                    ui.label(every_starts_at(&Sentences::new(document).thing_word(thing)));
                    start_number(ui, document, property, start, StartSlot::Value, pane, pending);
                }
            });
            start_messages(ui, pane.widgets, property, slots);
        }
        _ => {}
    }
}

/// Sets the start of `property` to `choice`, unless `selected`, the words of its start, already reads as it.
fn choose_start(property: &SheetProperty, selected: &str, choice: StartChoice, pending: &mut Vec<Pending>) {
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
        StartChoice::YesNo(yes) => {
            let mut start = StartsAs::new(StartRule::Value);
            start.value = Slot::Filled(SheetValue::Literal(SheetLiteral::Bool(yes)));
            start
        }
        StartChoice::Anywhere if selected != ANYWHERE_AT_RANDOM => StartsAs::new(StartRule::Anywhere),
        StartChoice::OnGround(ground, state) => {
            let mut start = StartsAs::new(StartRule::OnGround);
            start.property = Some(ground);
            start.state = Some(state);
            start
        }
        _ => return,
    };
    pending.push(Pending::one(SheetEdit::SetStartsAs {
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
    pending: &mut Vec<Pending>,
) {
    for (index, row) in start.rows.iter().enumerate() {
        let place = starts_row_place(&property.name, index + 1);
        wrapped_line(ui, |ui| {
            if let Some(state) = state_select(
                ui,
                property,
                row.state.as_deref(),
                ("henad_sheet_start_row", &property.id, index),
                &starts_field_name(&place, StartsField::State),
            ) {
                pending.push(start_edit(property.id.clone(), move |start| {
                    if let Some(row) = start.rows.get_mut(index) {
                        row.state = Some(state);
                    }
                }));
            }
            ui.add(Label::new(WITH_CHANCE).extend());
            start_number(ui, document, property, start, StartSlot::Row(index), pane, pending);
            let remove = delete_button(ui, &remove_row_name(index + 1, &property.name));
            if remove.clicked() {
                // The rows after it move up a place. Otherwise a refused text would show on the row that takes its
                // place.
                for later in index..start.rows.len() {
                    forget_field(ui, start_field_id(pane.widgets, &property.id, StartSlot::Row(later)));
                }
                pending.push(start_edit(property.id.clone(), move |start| {
                    if index < start.rows.len() {
                        start.rows.remove(index);
                    }
                }));
            }
        });
        start_messages(ui, pane.widgets, property, &[StartSlot::Row(index)]);
    }
    let add = line(ui, |ui| {
        crate::ui::editor::sheet::widgets::add_row_button(ui, ADD_ROW, &add_row_name(&property.name), None, false)
    });
    if add.clicked() {
        // A new row draws the first state that no row draws yet.
        let drawn: Vec<Option<&str>> = start.rows.iter().map(|row| row.state.as_deref()).collect();
        let state = property
            .states
            .iter()
            .find(|state| !drawn.contains(&Some(state.id.as_str())))
            .or(property.states.first())
            .map(|state| state.id.clone());
        pending.push(start_edit(property.id.clone(), move |start| {
            let mut row = StartsAsRow::default();
            row.state = state;
            start.rows.push(row);
        }));
    }
    wrapped_line(ui, |ui| {
        ui.label(OTHERWISE);
        if let Some(state) = state_select(
            ui,
            property,
            start.otherwise.as_deref(),
            ("henad_sheet_start_otherwise", &property.id, 0),
            &starts_otherwise_name(&property.name),
        ) {
            pending.push(start_edit(property.id.clone(), move |start| {
                start.otherwise = Some(state);
            }));
        }
    });
}

/// Adds a select of the states of `property` showing `current`, named `name`, and returns the id of the state chosen.
///
/// Each state in the list shows its swatch.
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
    let items: Vec<OverlayItem<'_>> = property
        .states
        .iter()
        .map(|state| OverlayItem {
            label: &state.name,
            current: current == Some(state.id.as_str()),
            swatch: Some(color32(state.color)),
            ..OverlayItem::default()
        })
        .collect();
    let (_, chosen) = select(ui, id_salt, name, shown, &items);
    let chosen = chosen.and_then(|index| property.states.get(index))?;
    (current != Some(chosen.id.as_str())).then(|| chosen.id.clone())
}

/// Returns the edit that changes the start of `property` by `change`, applied to the start as the sheet holds it.
pub(super) fn start_edit(property: String, change: impl FnOnce(&mut StartsAs) + 'static) -> Pending {
    Pending {
        starts_only: true,
        form: None,
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

/// Returns the start of property `property` of `document`.
fn start_of(document: &SheetDocument, property: &str) -> Option<StartsAs> {
    find_property(document, property)?.starts_as.clone()
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
pub(super) fn set_start_slot(start: &mut StartsAs, slot: StartSlot, value: Slot) {
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
    pending: &mut Vec<Pending>,
) {
    let chance = matches!(slot, StartSlot::Row(_));
    let place = match slot {
        StartSlot::Row(index) => starts_row_place(&property.name, index + 1),
        StartSlot::Value => starts_place(&property.name),
        StartSlot::Low => starts_low_place(&property.name),
        StartSlot::High => starts_high_place(&property.name),
    };
    let field = NumberField {
        place,
        id: start_field_id(pane.widgets, &property.id, slot),
        current: start_slot(start, slot),
        target: NumberTarget::Start {
            property: property.id.clone(),
            slot,
        },
        number_use: if chance { NumberUse::Chance } else { NumberUse::Number },
        limit: None,
        after: if chance {
            text_control_width(ui, REMOVE_ROW_LABEL) + ITEM_GAP
        } else {
            0.0
        },
    };
    number_field(ui, document, field, pane, pending);
}

/// Draws `field`: a text field with "Use a parameter" beside it, or the chip of the parameter it holds. A click on the
/// chip reopens the full picker. A typed number, a parameter picked or a new parameter fills the field's target.
pub(super) fn number_field(
    ui: &mut Ui,
    document: &SheetDocument,
    field: NumberField<'_>,
    pane: &mut PaneState,
    pending: &mut Vec<Pending>,
) {
    let NumberField {
        place,
        id,
        current,
        target,
        number_use,
        limit,
        after,
    } = field;
    let chance = number_use == NumberUse::Chance;
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
            let width = ui.max_rect().width() - after;
            let response = chip_within(ui, Some(id.with("chip")), &chip, false, None, width);
            let blank = if chance { Blank::Chance } else { Blank::Number };
            let title = blank.picker_title();
            ParameterPicker {
                title: &title,
                document,
                number_use,
                number_box: true,
            }
            .show(ui, &response)
        }
        value => {
            let text = match value {
                Some(SheetValue::Literal(literal)) => literal_words(*literal),
                _ => String::new(),
            };
            let word = if chance {
                StartsField::Chance
            } else {
                StartsField::Number
            };
            commit_number(ui, id, &starts_field_name(&place, word), &text, FIELD_WIDTH, |typed| {
                let value = if typed.trim().is_empty() {
                    if let Some((_, _, message)) = limit {
                        return Some(message.to_owned());
                    }
                    Slot::Blank
                } else {
                    match number_use.read(typed) {
                        Ok(literal) => {
                            if let Some((low, high, message)) = limit
                                && !literal.number().is_some_and(|number| (low..=high).contains(&number))
                            {
                                return Some(message.to_owned());
                            }
                            Slot::Filled(SheetValue::Literal(literal))
                        }
                        Err(message) if message == TYPE_ONE_NUMBER && chance => {
                            return Some(TYPE_A_CHANCE.to_owned());
                        }
                        Err(message) => return Some(message.to_owned()),
                    }
                };
                pending.push(target.edit(value));
                None
            });
            let button = text_control(
                ui,
                Some(id.with("use")),
                USE_A_PARAMETER,
                &starts_field_name(&place, StartsField::UseParameter),
                CHIP_HEIGHT,
            );
            ParameterPicker {
                title: CHOOSE_A_PARAMETER,
                document,
                number_use,
                number_box: false,
            }
            .show(ui, &button)
        }
    };
    pick_number(current, target, number_use, choice, pane, pending);
}

/// Takes `choice`, made in the picker of the number `target` that holds `current`: a parameter or a typed number goes
/// into the target, and "New parameter..." opens the New parameter dialog.
fn pick_number(
    current: Option<&Slot>,
    target: NumberTarget,
    number_use: NumberUse,
    choice: Option<Choice>,
    pane: &mut PaneState,
    pending: &mut Vec<Pending>,
) {
    let value = match choice {
        Some(Choice::Value(value)) => Slot::Filled(value),
        Some(Choice::NewParameter) => {
            let typed = match current.and_then(Slot::value) {
                Some(SheetValue::Literal(literal)) => literal.number(),
                _ => None,
            };
            pane.new_parameter = Some((Some(target), NewParameterDialog::new(number_use, typed, None)));
            return;
        }
        _ => return,
    };
    pending.push(target.edit(value));
}

/// Draws the New parameter dialog while it is open. Create adds the parameter and puts it in the number it was opened
/// from, as one edit, or adds it alone when "+ Add parameter" opened the dialog.
///
/// An edit that fills a start counts as a changed start, since only the start uses the new parameter. Past the start
/// it therefore waits for Start again, as a parameter chosen from the picker does.
pub(super) fn new_parameter(ui: &Ui, document: &SheetDocument, pane: &mut PaneState, pending: &mut Vec<Pending>) {
    let Some((target, dialog)) = &mut pane.new_parameter else {
        return;
    };
    match new_parameter_modal(ui.ctx(), dialog, &parameter_names(document)) {
        DialogOutcome::Open => {}
        DialogOutcome::Canceled => pane.new_parameter = None,
        DialogOutcome::Created(parameter) => {
            let target = target.clone();
            pane.new_parameter = None;
            pending.push(Pending {
                starts_only: matches!(target, Some(NumberTarget::Start { .. })),
                form: None,
                run: Box::new(move |builder| {
                    let made = builder.apply(parameter.add_edit())?;
                    let Some(target) = target else {
                        return Ok(());
                    };
                    let id = made.into_iter().next().unwrap_or_default();
                    (target.edit(Slot::Filled(SheetValue::Chip(SheetChip::Param(id)))).run)(builder)
                }),
            });
        }
    }
}
