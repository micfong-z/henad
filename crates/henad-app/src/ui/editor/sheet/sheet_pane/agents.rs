//! The sections of a world of agents on the Sheet pane: Agents, Ground and Events.
//!
//! Agents names the one kind of agent, says how many there are at the start and how many the world holds at most, and
//! lists the properties every agent has. Ground lists the properties of the cells under the agents, how the marks
//! agents leave on a Number property combine and fade, and the states agents cannot enter. Its table appears with its
//! first property. Events lists what Record lines count, each name a field, and adds an event through the *event
//! form*.

use egui::{Button, ComboBox, Frame, Key, TextEdit, Ui};
use henad_graph::limits::MAX_AGENTS;
use henad_graph::sheet::document::{
    AgentKind, Marks, PropertyType, SheetDocument, SheetLiteral, SheetProperty, SheetValue, Slot,
};
use henad_graph::sheet::edit::{IdSpace, SheetEdit, SheetItem, is_reserved_name, mint_id};
use henad_graph::sheet::sentence::{
    ADD_EVENT, ADD_GROUND_PROPERTY_NAME, AGENT_COUNT_LABEL, AGENT_COUNT_NAME, AGENT_MAX_LABEL, AGENT_MAX_NAME,
    AGENT_MAX_TOOLTIP, AGENT_NAME_FIELD, AGENT_NAME_MISSING, CANCEL, DONE, EVENT_NAME_FIELD, EVENT_NAME_MISSING,
    EVENT_NAME_PLACEHOLDER, EVENTS_EMPTY, GROUND_EMPTY, KEPT_LEAD, KEPT_OUT_OF_RANGE, KEPT_TAIL, MARKS_ADD_UP,
    MARKS_LABEL, MARKS_LARGEST, MARKS_NONE, MARKS_TOOLTIP, NAME_LABEL, NEW_EVENT, REMOVE_ROW_LABEL, add_property_name,
    blocked_lead, blocked_state_name, delete_event_name, event_name_name, event_name_taken, integer_up_to, kept_place,
    marks_name, name_reserved, read_typed_number,
};

use super::{
    FormKind, GROUND, NumberField, NumberTarget, PARAMETER_NAME_WIDTH, PaneState, Pending, number_field, properties_of,
    property_form_or_add, property_table,
};
use crate::ui::editor::sheet::widgets::{NumberUse, commit_field, field_message, message_ui, set_name};
use crate::ui::sweep::layout::{add_button, filled_button};

/// Width in points of a field that holds a number of agents.
const COUNT_WIDTH: f32 = 72.0;

/// The form that adds an event.
#[derive(Debug, Clone)]
pub struct EventForm {
    /// Text of the Name field.
    name: String,
    /// Message of the last refused Done, `None` while none shows.
    pub message: Option<String>,
    /// Whether the Name field takes the focus on the next frame.
    focus_name: bool,
}

/// Draws the Agents section: the kind of agent's name, its number at the start and at most, the table of its
/// properties, and the property form or "+ Add property" under it.
pub(super) fn agents(
    ui: &mut Ui,
    document: &SheetDocument,
    pane: &mut PaneState,
    stale: &[String],
    pending: &mut Option<Pending>,
) {
    let Some(kind) = document.agents.first() else {
        return;
    };
    let widgets = pane.widgets;
    let name_id = widgets.with("henad_sheet_agent_name");
    let count_id = widgets.with("henad_sheet_agent_count");
    let max_id = widgets.with("henad_sheet_agent_max");
    ui.horizontal(|ui| {
        ui.label(NAME_LABEL);
        commit_field(
            ui,
            name_id,
            AGENT_NAME_FIELD,
            &kind.name,
            PARAMETER_NAME_WIDTH,
            |text| {
                let name = text.trim();
                if name.is_empty() {
                    return Some(AGENT_NAME_MISSING.to_owned());
                }
                *pending = Some(rename_kind(kind, name));
                None
            },
        );
    });
    ui.horizontal(|ui| {
        ui.label(AGENT_COUNT_LABEL);
        commit_field(
            ui,
            count_id,
            AGENT_COUNT_NAME,
            &kind.count.to_string(),
            COUNT_WIDTH,
            |text| match whole_number(text, kind.max_count) {
                Ok(count) => {
                    *pending = Some(set_kind(kind, count, kind.max_count));
                    None
                }
                Err(message) => Some(message),
            },
        );
    });
    ui.horizontal(|ui| {
        ui.label(AGENT_MAX_LABEL).on_hover_text(AGENT_MAX_TOOLTIP);
        commit_field(
            ui,
            max_id,
            AGENT_MAX_NAME,
            &kind.max_count.to_string(),
            COUNT_WIDTH,
            |text| {
                match whole_number(text, MAX_AGENTS) {
                    Ok(max_count) => {
                        // A smaller world holds fewer agents at the start too.
                        *pending = Some(set_kind(kind, kind.count.min(max_count), max_count));
                        None
                    }
                    Err(message) => Some(message),
                }
            },
        );
    });
    if let Some(message) = [name_id, count_id, max_id]
        .into_iter()
        .find_map(|id| field_message(ui, id))
    {
        message_ui(ui, &message);
    }
    property_table(ui, document, &kind.id, pane, stale, pending);
    ui.add_space(4.0);
    property_form_or_add(
        ui,
        document,
        &kind.id,
        &add_property_name(&kind.name),
        false,
        pane,
        pending,
    );
    if let Some(refusal) = &pane.refusal {
        message_ui(ui, refusal);
    }
}

/// Returns the whole number from 1 to `most` typed as `text`, or the message for any other text.
fn whole_number(text: &str, most: u32) -> Result<u32, String> {
    let number = read_typed_number(text)
        .and_then(SheetLiteral::number)
        .filter(|number| number.fract() == 0.0 && (1.0..=f64::from(most)).contains(number));
    // The filter holds the number to a whole number within `u32`.
    number.map(|number| number as u32).ok_or_else(|| integer_up_to(most))
}

/// Returns the edit that sets the number of agents of `kind` at the start to `count` and at most to `max_count`.
fn set_kind(kind: &AgentKind, count: u32, max_count: u32) -> Pending {
    Pending::one(SheetEdit::SetAgentKind {
        kind: kind.id.clone(),
        count,
        max_count,
    })
}

/// Returns the edit that renames the kind of agent `kind` to `name`.
///
/// The kind's id follows its name while it is the id the old name mints, as it is for a kind the editor made. An id
/// the sheet refuses, such as one a card already holds, leaves the old id.
fn rename_kind(kind: &AgentKind, name: &str) -> Pending {
    let id = kind.id.clone();
    let old_name = kind.name.clone();
    let name = name.to_owned();
    Pending {
        starts_only: false,
        form: None,
        run: Box::new(move |builder| {
            builder.apply(SheetEdit::Rename {
                item: SheetItem::AgentKind(id.clone()),
                name: name.clone(),
            })?;
            let others: Vec<String> = builder
                .document()
                .agents
                .iter()
                .filter(|other| other.id != id)
                .map(|other| other.id.clone())
                .collect();
            let minted = |name: &str| {
                mint_id(name, IdSpace::Thing, |candidate| {
                    others.iter().any(|other| other == candidate)
                })
                .ok()
            };
            if minted(&old_name).as_deref() == Some(id.as_str())
                && let Some(new_id) = minted(&name)
                && new_id != id
            {
                builder
                    .apply(SheetEdit::ChangeId {
                        item: SheetItem::AgentKind(id),
                        id: new_id,
                    })
                    .ok();
            }
            Ok(())
        }),
    }
}

/// Draws the Ground section: the table of the ground's properties with the marks of each Number property and the
/// blocked states of each States property, then the property form or "+ Add property". A sheet with no ground says
/// what the ground is for.
pub(super) fn ground(
    ui: &mut Ui,
    document: &SheetDocument,
    pane: &mut PaneState,
    stale: &[String],
    pending: &mut Option<Pending>,
) {
    let properties = properties_of(document, GROUND);
    if properties.is_empty() {
        ui.label(GROUND_EMPTY);
    } else {
        property_table(ui, document, GROUND, pane, stale, pending);
        for property in properties {
            match property.property_type {
                PropertyType::Number => marks_row(ui, document, property, pane, pending),
                PropertyType::States => blocked_row(ui, document, property, pending),
                _ => {}
            }
        }
    }
    ui.add_space(4.0);
    property_form_or_add(ui, document, GROUND, ADD_GROUND_PROPERTY_NAME, false, pane, pending);
}

/// Draws how the marks agents leave on the Number property `property` of the ground combine and, while it takes
/// marks, the share of it each tick keeps, with the message of a refused share under the row.
fn marks_row(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    pane: &mut PaneState,
    pending: &mut Option<Pending>,
) {
    let kept_id = pane.widgets.with(("henad_sheet_kept", property.id.as_str()));
    let marks_words = |marks: Option<Marks>| match marks {
        Some(Marks::AddUp) => MARKS_ADD_UP,
        Some(_) => MARKS_LARGEST,
        None => MARKS_NONE,
    };
    ui.horizontal_wrapped(|ui| {
        ui.weak(&property.name);
        ui.label(MARKS_LABEL).on_hover_text(MARKS_TOOLTIP);
        let mut marks = property.marks;
        let combo = ComboBox::from_id_salt(("henad_sheet_marks", &property.id))
            .selected_text(marks_words(marks))
            .show_ui(ui, |ui| {
                for choice in [None, Some(Marks::Largest), Some(Marks::AddUp)] {
                    ui.selectable_value(&mut marks, choice, marks_words(choice));
                }
            });
        set_name(ui, combo.response.id, &marks_name(&property.name));
        combo.response.on_hover_text(MARKS_TOOLTIP);
        if marks != property.marks {
            // A property that takes marks says what share each tick keeps, all of it to start with.
            let kept_per_tick = match &property.kept_per_tick {
                Slot::Blank => Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0))),
                kept => kept.clone(),
            };
            *pending = Some(Pending::one(SheetEdit::SetMarks {
                property: property.id.clone(),
                marks,
                kept_per_tick,
            }));
        }
        if property.marks.is_some() {
            ui.label(KEPT_LEAD);
            let field = NumberField {
                place: kept_place(&property.name),
                id: kept_id,
                current: Some(&property.kept_per_tick),
                target: NumberTarget::Kept {
                    property: property.id.clone(),
                },
                number_use: NumberUse::Number,
                limit: Some((0.0, 1.0, KEPT_OUT_OF_RANGE)),
            };
            number_field(ui, document, field, pane, pending);
            ui.label(KEPT_TAIL);
        }
    });
    if property.marks.is_some()
        && let Some(message) = field_message(ui, kept_id)
    {
        message_ui(ui, &message);
    }
}

/// Draws the states of the States property `property` of the ground, each with a box that keeps agents out of its
/// cells.
fn blocked_row(ui: &mut Ui, document: &SheetDocument, property: &SheetProperty, pending: &mut Option<Pending>) {
    let blocked = |state: &str| {
        document.ground.as_ref().is_some_and(|ground| {
            ground
                .blocked
                .iter()
                .any(|known| known.property == property.id && known.state == state)
        })
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(blocked_lead(&property.name));
        for state in &property.states {
            let mut kept_out = blocked(&state.id);
            let response = ui.checkbox(&mut kept_out, &state.name);
            set_name(ui, response.id, &blocked_state_name(&state.name));
            if response.changed() {
                *pending = Some(Pending::one(SheetEdit::SetBlocked {
                    property: property.id.clone(),
                    state: state.id.clone(),
                    blocked: kept_out,
                }));
            }
        }
    });
}

/// Draws the Events section: each event's name as a field with its delete button, then the event form or "+ Add
/// event". A sheet with no event says what one is for.
pub(super) fn events(ui: &mut Ui, document: &SheetDocument, pane: &mut PaneState, pending: &mut Option<Pending>) {
    if document.events.is_empty() {
        ui.weak(EVENTS_EMPTY);
    }
    for event in &document.events {
        let id = pane.widgets.with(("henad_sheet_event", event.id.as_str()));
        ui.horizontal(|ui| {
            commit_field(
                ui,
                id,
                &event_name_name(&event.name),
                &event.name,
                PARAMETER_NAME_WIDTH,
                |text| {
                    let name = text.trim();
                    if name.is_empty() {
                        return Some(EVENT_NAME_MISSING.to_owned());
                    }
                    if event_taken(document, name, Some(&event.id)) {
                        return Some(event_name_taken(name));
                    }
                    *pending = Some(Pending::one(SheetEdit::Rename {
                        item: SheetItem::Event(event.id.clone()),
                        name: name.to_owned(),
                    }));
                    None
                },
            );
            let delete = add_button(
                ui,
                true,
                Button::new(REMOVE_ROW_LABEL).small(),
                &delete_event_name(&event.name),
            );
            if delete.clicked() {
                *pending = Some(Pending::one(SheetEdit::Delete {
                    item: SheetItem::Event(event.id.clone()),
                }));
            }
        });
        if let Some(message) = field_message(ui, id) {
            message_ui(ui, &message);
        }
    }
    ui.add_space(4.0);
    match &mut pane.event_form {
        Some(form) => {
            let mut close = false;
            event_form(ui, document, form, &mut close, pending);
            if close {
                pane.event_form = None;
            }
        }
        None => {
            if ui.button(ADD_EVENT).clicked() {
                pane.event_form = Some(EventForm {
                    name: String::new(),
                    message: None,
                    focus_name: true,
                });
            }
        }
    }
}

/// Returns whether an event of `document` other than `own` is named `name`, case aside.
fn event_taken(document: &SheetDocument, name: &str, own: Option<&str>) -> bool {
    document
        .events
        .iter()
        .filter(|event| Some(event.id.as_str()) != own)
        .any(|event| event.name.to_lowercase() == name.to_lowercase())
}

/// Draws the event form, and sets `close` on Cancel. Done, or Enter in the Name field, checks the name and adds the
/// event.
fn event_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut EventForm,
    close: &mut bool,
    pending: &mut Option<Pending>,
) {
    Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.strong(NEW_EVENT);
        let mut done = false;
        ui.horizontal(|ui| {
            ui.label(NAME_LABEL);
            let field = ui.add(TextEdit::singleline(&mut form.name).hint_text(EVENT_NAME_PLACEHOLDER));
            set_name(ui, field.id, EVENT_NAME_FIELD);
            if std::mem::take(&mut form.focus_name) {
                field.request_focus();
            }
            done |= field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
        });
        if let Some(message) = &form.message {
            message_ui(ui, message);
        }
        ui.horizontal(|ui| {
            done |= ui.add(filled_button(DONE.to_owned())).clicked();
            if ui.button(CANCEL).clicked() {
                *close = true;
            }
        });
        if done {
            let name = form.name.trim().to_owned();
            if name.is_empty() {
                form.message = Some(EVENT_NAME_MISSING.to_owned());
            } else if event_taken(document, &name, None) {
                form.message = Some(event_name_taken(&name));
            } else if is_reserved_name(&name, IdSpace::Item) {
                form.message = Some(name_reserved(&name));
            } else {
                *pending = Some(Pending::from_form(
                    FormKind::Event,
                    Box::new(move |builder| builder.apply(SheetEdit::AddEvent { name }).map(drop)),
                ));
            }
        }
    });
}
