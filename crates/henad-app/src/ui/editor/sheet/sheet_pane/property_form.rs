//! The property form of the Sheet pane: its name, kind and states, the checks Done runs in the order of the page, and
//! the commands that add or change the property.

use egui::{Key, TextEdit, Ui};
use henad_graph::sheet::document::{
    CELLS_ID, CardRuns, GROUND_ID, LineKind, PropertyType, SheetChip, SheetDocument, SheetLine, SheetLiteral,
    SheetProperty, SheetValue, Slot, StartRule, StartsAs,
};
use henad_graph::sheet::edit::{
    IdSpace, STATE_COLORS, SheetBuilder, SheetEdit, SheetEditError, SheetItem, is_reserved_name, suggested_state_color,
};
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::name_reserved;
use henad_graph::sheet::sentence::{
    ADD_STATE, DELETE_PROPERTY, DELETE_PROPERTY_HEADING, INTEGER_KIND_TOOLTIP, KIND_LABEL, NAME_LABEL, NEW_PROPERTY,
    NUMBER_KIND_DESCRIPTION, PROPERTY_NAME_FIELD, PROPERTY_NAME_MISSING, PROPERTY_NAME_PLACEHOLDER,
    STATE_NAMES_REPEATED, STATES_KIND_DESCRIPTION, TOO_FEW_STATES, edit_property_heading, property_name_taken,
    remove_state_name, state_color_name, state_name_field,
};
use henad_graph::sheet::sentence::{
    ARROW_KIND_DESCRIPTION, HEADING_KIND_DESCRIPTION, POSITION_KIND_DESCRIPTION, YES_NO_KIND_DESCRIPTION,
};
use henad_graph::value::Color;

use super::colors::shaded_rule;
use super::layout::{Form, ITEM_GAP, delete_button, line};
use super::starts::kind_words;
use super::{
    DELETE_WIDTH, EditRun, FormKind, FormState, Pending, PendingDelete, PropertyForm, STATE_FIELD_WIDTH, delete_or_ask,
    form_buttons, form_heading, form_name_field,
};
use crate::ui::editor::sheet::widgets::FocusTooltip as _;
use crate::ui::editor::sheet::widgets::{color_button, color_name, message_ui, name_taken, set_name};
use crate::ui::editor::sheet::{find_property, properties_of};
use crate::ui::sweep::layout::destructive_button;

/// Draws the property form, and sets `close` on Cancel. Done checks the form and adds or changes the property. Delete
/// property deletes it, or holds the delete in `waiting` while it empties formulas that the card cannot write again.
pub(super) fn property_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut PropertyForm,
    close: &mut bool,
    waiting: &mut Option<PendingDelete>,
    pending: &mut Vec<Pending>,
) {
    let heading = match &form.editing {
        Some(_) => edit_property_heading(&form.original),
        None => NEW_PROPERTY.to_owned(),
    };
    form_heading(ui, &heading);
    let rows = Form::new(ui, &[NAME_LABEL, KIND_LABEL]);
    rows.row(ui, NAME_LABEL, |ui| {
        form_name_field(
            ui,
            &mut form.name,
            PROPERTY_NAME_PLACEHOLDER,
            PROPERTY_NAME_FIELD,
            &mut form.focus_name,
        );
    });
    rows.row(ui, KIND_LABEL, |ui| {
        for kind in form_kinds(document, form) {
            let radio = ui.radio_value(&mut form.kind, kind, kind_words(kind));
            if kind == PropertyType::WholeNumber {
                radio.on_hover_or_focus_text(INTEGER_KIND_TOOLTIP);
            }
        }
    });
    rows.under(ui, |ui| {
        let description = match form.kind {
            PropertyType::States => STATES_KIND_DESCRIPTION,
            PropertyType::WholeNumber => INTEGER_KIND_TOOLTIP,
            PropertyType::YesNo => YES_NO_KIND_DESCRIPTION,
            PropertyType::Position => POSITION_KIND_DESCRIPTION,
            PropertyType::Heading => HEADING_KIND_DESCRIPTION,
            PropertyType::Arrow => ARROW_KIND_DESCRIPTION,
            _ => NUMBER_KIND_DESCRIPTION,
        };
        ui.weak(description);
        if form.kind == PropertyType::States {
            state_rows(ui, form);
        }
        if let Some(message) = &form.message {
            message_ui(ui, message);
        }
    });
    let mut delete = false;
    let (done, cancel) = form_buttons(ui, |ui| {
        delete = form.editing.is_some() && ui.add(destructive_button(DELETE_PROPERTY)).clicked();
    });
    if done {
        match check_form(document, form) {
            Ok(()) => pending.push(Pending::from_form(FormKind::Property, form_edit(form.clone()))),
            Err(message) => form.message = Some(message),
        }
    }
    *close |= cancel;
    if let Some(id) = form.editing.as_ref().filter(|_| delete) {
        let edit = SheetEdit::Delete {
            item: SheetItem::Property(id.clone()),
        };
        let asked = (DELETE_PROPERTY_HEADING, form.original.as_str());
        delete_or_ask(document, edit, asked, Some(FormKind::Property), waiting, pending);
    }
}

/// Draws the rows of states of the property form: each with its colour, its name and its remove button, then "+ Add
/// state". Enter in a name moves to the next state, or adds one after the last.
fn state_rows(ui: &mut Ui, form: &mut PropertyForm) {
    let mut remove = None;
    let mut entered = None;
    let focus = form.focus_state.take();
    for (index, state) in form.states.iter_mut().enumerate() {
        line(ui, |ui| {
            let color_label = state_color_name(index + 1, color_name(state.color));
            if let Some(color) = color_button(ui, &color_label, state.color) {
                state.color = color;
                state.picked = true;
            }
            let width = (ui.available_width() - DELETE_WIDTH - ITEM_GAP).clamp(48.0, STATE_FIELD_WIDTH);
            let field = ui.add(TextEdit::singleline(&mut state.name).desired_width(width));
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
            if delete_button(ui, &remove_state_name(index + 1)).clicked() {
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
    let add = line(ui, |ui| {
        crate::ui::editor::sheet::widgets::add_row_button(ui, ADD_STATE, ADD_STATE, None, false)
    });
    if add.clicked() {
        form.states.push(FormState::empty(form.states.len()));
        form.focus_state = Some(form.states.len() - 1);
    }
}

/// Returns the kinds the property form offers: States and Number for every kind of thing, Integer for cells and
/// agents, then for agents Boolean, and a Position or a Heading while the agents have none. A property's own kind is
/// always offered.
///
/// The ground holds only numbers and states.
fn form_kinds(document: &SheetDocument, form: &PropertyForm) -> Vec<PropertyType> {
    let mut kinds = vec![PropertyType::States, PropertyType::Number];
    if form.thing != GROUND_ID {
        kinds.push(PropertyType::WholeNumber);
    }
    if form.thing != CELLS_ID && form.thing != GROUND_ID {
        kinds.push(PropertyType::YesNo);
        kinds.push(PropertyType::Arrow);
        for built_in in [PropertyType::Position, PropertyType::Heading] {
            let held = properties_of(document, &form.thing)
                .iter()
                .any(|property| property.property_type == built_in && Some(&property.id) != form.editing.as_ref());
            if !held {
                kinds.push(built_in);
            }
        }
    }
    let own = form
        .editing
        .as_deref()
        .and_then(|id| find_property(document, id))
        .map(|property| property.property_type);
    if let Some(own) = own
        && !kinds.contains(&own)
    {
        kinds.push(own);
    }
    kinds
}

/// Returns the colour a new state takes at row `index`.
pub(super) fn default_color(index: usize) -> Color {
    STATE_COLORS[index % STATE_COLORS.len()]
}

/// Returns the message for the first check of the property form that fails, in the order of the page.
///
/// # Errors
///
/// Returns the message on an empty or taken name, the name of a new property that Henad keeps for itself, a States
/// property with fewer than two named states, or two states of one name.
fn check_form(document: &SheetDocument, form: &PropertyForm) -> Result<(), String> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(PROPERTY_NAME_MISSING.to_owned());
    }
    let others = properties_of(document, &form.thing)
        .iter()
        .filter(|property| Some(&property.id) != form.editing.as_ref());
    if name_taken(name, others.map(|property| property.name.as_str())) {
        return Err(property_name_taken(name));
    }
    // A rename keeps the id, so only a new property mints one.
    if form.editing.is_none() && is_reserved_name(name, IdSpace::Item) {
        return Err(name_reserved(name));
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

/// Returns the edit Done makes from `form`: a new property with its states and its first start, or the changes to a
/// property the form edits.
///
/// A new property starts in its first state, at 0, at no, anywhere at random for a Position, and with no start for a
/// Heading. A changed kind clears every reference to the property and starts it afresh. A removed state clears its
/// references, its measures and its start rows.
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
                    thing: form.thing.clone(),
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
        let current = find_property(builder.document(), &id)
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
        let start = match form.kind {
            PropertyType::Heading => None,
            PropertyType::Position => Some(StartsAs::new(StartRule::Anywhere)),
            PropertyType::Arrow => {
                // An arrow starts in a random direction of length 1, a start no control of the sheet changes.
                let mut direction = SheetLine::new(LineKind::RandomDirection);
                direction.length = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0)));
                let mut start = StartsAs::new(StartRule::Value);
                start.value = Slot::Filled(SheetValue::Phrase(Box::new(direction)));
                Some(start)
            }
            kind => {
                let mut start = StartsAs::new(StartRule::Value);
                start.value = Slot::Filled(match first {
                    Some(state) => SheetValue::Chip(SheetChip::State(state)),
                    None if kind == PropertyType::YesNo => SheetValue::Literal(SheetLiteral::Bool(false)),
                    None => SheetValue::Literal(SheetLiteral::Number(0.0)),
                });
                Some(start)
            }
        };
        builder.apply(SheetEdit::SetStartsAs {
            property: id.clone(),
            starts_as: start.map(Box::new),
        })?;
        // The first property made colours the cells. A States property does so through its command.
        let uncolored = form.thing == CELLS_ID
            && !builder.document().colors.contains_key(CELLS_ID)
            && !properties_of(builder.document(), CELLS_ID)
                .iter()
                .any(|property| property.property_type == PropertyType::States);
        if form.editing.is_none() && uncolored {
            builder.apply(SheetEdit::SetColorRule {
                thing: CELLS_ID.to_owned(),
                rule: Some(shaded_rule(&id, None)),
            })?;
        }
        Ok(())
    })
}

/// Changes the states of `property` to the rows `states` of the property form: removed states go, the others take
/// their new names and colours, and new rows become new states.
///
/// A rename onto a name another state still holds, as in a swap of two names, waits for that state to give the name up.
/// Every renamed state then takes a spare name first and its new name after. New states come last, once every name a
/// rename frees is free.
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
    let item = |id: &str| SheetItem::State {
        property: property.id.clone(),
        state: id.to_owned(),
    };
    let renamed: Vec<(&str, String)> = states
        .iter()
        .filter_map(|row| {
            let id = row.id.as_deref()?;
            let state = property.states.iter().find(|state| state.id == id)?;
            let name = row.name.trim().to_owned();
            (state.name != name).then_some((id, name))
        })
        .collect();
    let held: Vec<(String, String)> = find_property(builder.document(), &property.id)
        .map(|current| {
            current
                .states
                .iter()
                .map(|state| (state.id.clone(), state.name.clone()))
                .collect()
        })
        .unwrap_or_default();
    let clash = renamed.iter().any(|(id, name)| {
        held.iter()
            .any(|(other, known)| other != id && known.to_lowercase() == name.to_lowercase())
    });
    if clash {
        let mut taken: Vec<String> = held.into_iter().map(|(_, name)| name).collect();
        taken.extend(states.iter().map(|row| row.name.trim().to_owned()));
        for (id, _) in &renamed {
            let spare = spare_name(id, &taken);
            taken.push(spare.clone());
            builder.apply(SheetEdit::Rename {
                item: item(id),
                name: spare,
            })?;
        }
    }
    for (id, name) in renamed {
        builder.apply(SheetEdit::Rename { item: item(id), name })?;
    }
    for row in states {
        let Some(state) = row
            .id
            .as_ref()
            .and_then(|id| property.states.iter().find(|state| state.id == *id))
        else {
            continue;
        };
        if state.color != row.color {
            builder.apply(SheetEdit::Recolor {
                item: item(&state.id),
                color: row.color,
            })?;
        }
    }
    for row in states.iter().filter(|row| row.id.is_none()) {
        builder.apply(SheetEdit::AddState {
            property: property.id.clone(),
            name: row.name.trim().to_owned(),
            color: Some(row.color),
        })?;
    }
    Ok(())
}

/// Returns a name for the state `id` that none of `taken` holds, case aside, such as "alive 1".
fn spare_name(id: &str, taken: &[String]) -> String {
    (1..)
        .map(|number| format!("{id} {number}"))
        .find(|candidate| !name_taken(candidate, taken.iter().map(String::as_str)))
        .unwrap_or_default()
}

/// Returns the numbers of the lines that change property `property_id` on the cards that run each tick: a Become, a
/// Set, a Steer or a Keep between of it, a Leave on it, and for an agent's Position or Heading a Move.
pub(super) fn changing_lines(document: &SheetDocument, property_id: &str) -> Vec<usize> {
    let mut numbers = Vec::new();
    for card in document.cards.iter().filter(|card| card.runs == CardRuns::EachTick) {
        let moved = properties_of(document, &card.every).iter().any(|property| {
            property.id == property_id
                && matches!(property.property_type, PropertyType::Position | PropertyType::Heading)
        });
        let relations = CardRelations::new(document, card);
        for &line_id in relations.order() {
            let changes = relations.line(line_id).is_some_and(|line| match line.kind {
                LineKind::Become | LineKind::Set | LineKind::Steer | LineKind::KeepBetween => {
                    line.property.as_deref() == Some(property_id)
                }
                LineKind::Leave => line.ground_property.as_deref() == Some(property_id),
                LineKind::Move => moved,
                _ => false,
            });
            if changes && let Some(number) = relations.number(line_id) {
                numbers.push(number);
            }
        }
    }
    numbers
}
