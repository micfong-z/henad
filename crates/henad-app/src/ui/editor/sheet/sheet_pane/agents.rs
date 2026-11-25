//! The sections of a world of agents on the Sheet pane: Agents, Ground and Events.
//!
//! Agents names the one kind of agent, says how many there are at the start and how many the world holds at most, and
//! lists the properties every agent has. Ground lists the properties of the cells under the agents, how the marks
//! agents leave on a Number property combine and fade, and the states agents cannot enter. Its table appears with its
//! first property. Events lists what Record lines count, each name a field, and adds an event through the *event
//! form*.

use egui::{Label, RichText, Ui};
use henad_graph::limits::MAX_AGENTS;
use henad_graph::sheet::document::{
    AgentKind, Marks, PropertyType, SheetDocument, SheetLiteral, SheetProperty, SheetValue, Slot,
};
use henad_graph::sheet::edit::{IdSpace, SheetEdit, SheetItem, is_reserved_name, mint_id};
use henad_graph::sheet::sentence::{
    ADD_EVENT, ADD_GROUND_PROPERTY_NAME, AGENT_COUNT_LABEL, AGENT_COUNT_NAME, AGENT_MAX_LABEL, AGENT_MAX_NAME,
    AGENT_MAX_TOOLTIP, AGENT_NAME_FIELD, AGENT_NAME_MISSING, EVENT_NAME_FIELD, EVENT_NAME_MISSING,
    EVENT_NAME_PLACEHOLDER, EVENTS_EMPTY, GROUND_EMPTY, KEPT_LEAD, KEPT_OUT_OF_RANGE, KEPT_TAIL, MARKS_ADD_UP,
    MARKS_LABEL, MARKS_LARGEST, MARKS_NONE, MARKS_TOOLTIP, NAME_LABEL, NEW_EVENT, add_property_name, blocked_lead,
    blocked_state_name, delete_event_name, event_name_name, event_name_taken, integer_up_to, kept_place, marks_name,
    name_reserved, read_typed_number,
};

use super::layout::{Column, Form, Table, line, text_cell, wrapped_line};
use super::{
    FormKind, GROUND_ID, NumberField, NumberTarget, PARAMETER_NAME_WIDTH, PaneState, Pending, form_buttons,
    form_heading, form_name_field, number_field, properties_of, property_form_or_add, property_table, select,
};
use crate::ui::editor::sheet::widgets::FocusTooltip as _;
use crate::ui::editor::sheet::widgets::{
    NumberUse, OverlayItem, add_row_button, commit_field, commit_number, field_message, message_ui, name_taken,
    set_name,
};

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
    pending: &mut Vec<Pending>,
) {
    let Some(kind) = document.agents.first() else {
        return;
    };
    let widgets = pane.widgets;
    let name_id = widgets.with("henad_sheet_agent_name");
    let count_id = widgets.with("henad_sheet_agent_count");
    let max_id = widgets.with("henad_sheet_agent_max");
    let form = Form::new(ui, &[NAME_LABEL, AGENT_COUNT_LABEL, AGENT_MAX_LABEL]);
    form.row(ui, NAME_LABEL, |ui| {
        commit_field(
            ui,
            name_id,
            AGENT_NAME_FIELD,
            &kind.name,
            PARAMETER_NAME_WIDTH.min(ui.available_width()),
            |text| {
                let name = text.trim();
                if name.is_empty() {
                    return Some(AGENT_NAME_MISSING.to_owned());
                }
                pending.push(rename_kind(kind, name));
                None
            },
        );
    });
    if let Some(message) = field_message(ui, name_id) {
        form.under(ui, |ui| message_ui(ui, &message));
    }
    form.row(ui, AGENT_COUNT_LABEL, |ui| {
        commit_number(
            ui,
            count_id,
            AGENT_COUNT_NAME,
            &kind.count.to_string(),
            COUNT_WIDTH,
            |text| match whole_number(text, kind.max_count) {
                Ok(count) => {
                    pending.push(set_kind(kind, count, kind.max_count));
                    None
                }
                Err(message) => Some(message),
            },
        );
    });
    if let Some(message) = field_message(ui, count_id) {
        form.under(ui, |ui| message_ui(ui, &message));
    }
    form.row_with(
        ui,
        |ui| {
            ui.add(Label::new(AGENT_MAX_LABEL).wrap())
                .on_hover_text(AGENT_MAX_TOOLTIP);
        },
        |ui| {
            commit_number(
                ui,
                max_id,
                AGENT_MAX_NAME,
                &kind.max_count.to_string(),
                COUNT_WIDTH,
                |text| {
                    match whole_number(text, MAX_AGENTS) {
                        Ok(max_count) => {
                            // A smaller world holds fewer agents at the start too.
                            pending.push(set_kind(kind, kind.count.min(max_count), max_count));
                            None
                        }
                        Err(message) => Some(message),
                    }
                },
            );
        },
    );
    if let Some(message) = field_message(ui, max_id) {
        form.under(ui, |ui| message_ui(ui, &message));
    }
    ui.add_space(6.0);
    property_table(ui, document, &kind.id, pane, stale, pending);
    ui.add_space(6.0);
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
    pending: &mut Vec<Pending>,
) {
    let properties = properties_of(document, GROUND_ID);
    if properties.is_empty() {
        text_cell(ui, Label::new(GROUND_EMPTY));
    } else {
        property_table(ui, document, GROUND_ID, pane, stale, pending);
        ui.add_space(4.0);
        for property in properties {
            match property.property_type {
                PropertyType::Number => marks_row(ui, document, property, pane, pending),
                PropertyType::States => blocked_row(ui, document, property, pending),
                _ => {}
            }
        }
    }
    ui.add_space(6.0);
    property_form_or_add(ui, document, GROUND_ID, ADD_GROUND_PROPERTY_NAME, false, pane, pending);
}

/// Draws how the marks agents leave on the Number property `property` of the ground combine and, while it takes
/// marks, the share of it each tick keeps, with the message of a refused share under the row.
fn marks_row(
    ui: &mut Ui,
    document: &SheetDocument,
    property: &SheetProperty,
    pane: &mut PaneState,
    pending: &mut Vec<Pending>,
) {
    let kept_id = pane.widgets.with(("henad_sheet_kept", property.id.as_str()));
    let marks_words = |marks: Option<Marks>| match marks {
        Some(Marks::AddUp) => MARKS_ADD_UP,
        Some(_) => MARKS_LARGEST,
        None => MARKS_NONE,
    };
    wrapped_line(ui, |ui| {
        ui.weak(&property.name);
        ui.label(MARKS_LABEL).on_hover_text(MARKS_TOOLTIP);
        let choices = [None, Some(Marks::Largest), Some(Marks::AddUp)];
        let items = choices.map(|choice| OverlayItem {
            label: marks_words(choice),
            current: choice == property.marks,
            ..OverlayItem::default()
        });
        let (response, chosen) = select(
            ui,
            ("henad_sheet_marks", &property.id),
            &marks_name(&property.name),
            marks_words(property.marks),
            &items,
        );
        response.on_hover_or_focus_text(MARKS_TOOLTIP);
        let marks = chosen
            .and_then(|index| choices.get(index).copied())
            .unwrap_or(property.marks);
        if marks != property.marks {
            // A property that takes marks says what share each tick keeps, all of it to start with.
            let kept_per_tick = match &property.kept_per_tick {
                Slot::Blank => Slot::Filled(SheetValue::Literal(SheetLiteral::Number(1.0))),
                kept => kept.clone(),
            };
            pending.push(Pending::one(SheetEdit::SetMarks {
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
                number_use: NumberUse::Share,
                limit: Some((0.0, 1.0, KEPT_OUT_OF_RANGE)),
                after: 0.0,
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
fn blocked_row(ui: &mut Ui, document: &SheetDocument, property: &SheetProperty, pending: &mut Vec<Pending>) {
    let blocked = |state: &str| {
        document.ground.as_ref().is_some_and(|ground| {
            ground
                .blocked
                .iter()
                .any(|known| known.property == property.id && known.state == state)
        })
    };
    wrapped_line(ui, |ui| {
        ui.label(blocked_lead(&property.name));
        for state in &property.states {
            let mut kept_out = blocked(&state.id);
            let response = ui.checkbox(&mut kept_out, &state.name);
            set_name(ui, response.id, &blocked_state_name(&state.name));
            if response.changed() {
                pending.push(Pending::one(SheetEdit::SetBlocked {
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
pub(super) fn events(ui: &mut Ui, document: &SheetDocument, pane: &mut PaneState, pending: &mut Vec<Pending>) {
    if document.events.is_empty() {
        text_cell(ui, Label::new(RichText::new(EVENTS_EMPTY).weak()));
    } else {
        let columns = vec![
            Column::new("", PARAMETER_NAME_WIDTH, 64.0),
            Column::fixed(super::DELETE_WIDTH),
        ];
        let id = ui.make_persistent_id("henad_sheet_events");
        Table::new(id, columns, 0, &[(0, 0.0)]).show(ui, |ui, rows| {
            for event in &document.events {
                let id = pane.widgets.with(("henad_sheet_event", event.id.as_str()));
                rows.row(ui, &event.id, |row| {
                    row.cell(0, |ui| {
                        line(ui, |ui| {
                            commit_field(
                                ui,
                                id,
                                &event_name_name(&event.name),
                                &event.name,
                                PARAMETER_NAME_WIDTH.min(ui.available_width()),
                                |text| {
                                    let name = text.trim();
                                    if name.is_empty() {
                                        return Some(EVENT_NAME_MISSING.to_owned());
                                    }
                                    if event_taken(document, name, Some(&event.id)) {
                                        return Some(event_name_taken(name));
                                    }
                                    pending.push(Pending::one(SheetEdit::Rename {
                                        item: SheetItem::Event(event.id.clone()),
                                        name: name.to_owned(),
                                    }));
                                    None
                                },
                            );
                        });
                    });
                    if row.delete_cell(1, &delete_event_name(&event.name)) {
                        pending.push(Pending::one(SheetEdit::Delete {
                            item: SheetItem::Event(event.id.clone()),
                        }));
                    }
                    if let Some(message) = row.message(id) {
                        row.below(0, |ui| message_ui(ui, &message));
                    }
                });
            }
        });
    }
    ui.add_space(6.0);
    match &mut pane.event_form {
        Some(form) => {
            let mut close = false;
            event_form(ui, document, form, &mut close, pending);
            if close {
                pane.event_form = None;
            }
        }
        None => {
            if add_row_button(ui, ADD_EVENT, ADD_EVENT, None, false).clicked() {
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
    let others = document.events.iter().filter(|event| Some(event.id.as_str()) != own);
    name_taken(name, others.map(|event| event.name.as_str()))
}

/// Draws the event form, and sets `close` on Cancel. Done, or Enter in the Name field, checks the name and adds the
/// event.
fn event_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut EventForm,
    close: &mut bool,
    pending: &mut Vec<Pending>,
) {
    form_heading(ui, NEW_EVENT);
    let rows = Form::new(ui, &[NAME_LABEL]);
    let mut done = false;
    rows.row(ui, NAME_LABEL, |ui| {
        done |= form_name_field(
            ui,
            &mut form.name,
            EVENT_NAME_PLACEHOLDER,
            EVENT_NAME_FIELD,
            &mut form.focus_name,
        );
    });
    if let Some(message) = &form.message {
        rows.under(ui, |ui| message_ui(ui, message));
    }
    let (pressed, cancel) = form_buttons(ui, |_| {});
    done |= pressed;
    *close |= cancel;
    if done {
        let name = form.name.trim().to_owned();
        if name.is_empty() {
            form.message = Some(EVENT_NAME_MISSING.to_owned());
        } else if event_taken(document, &name, None) {
            form.message = Some(event_name_taken(&name));
        } else if is_reserved_name(&name, IdSpace::Item) {
            form.message = Some(name_reserved(&name));
        } else {
            pending.push(Pending::from_form(
                FormKind::Event,
                Box::new(move |builder| builder.apply(SheetEdit::AddEvent { name }).map(drop)),
            ));
        }
    }
}
