//! The Measures section of the Sheet pane: the plotted measures, each a count of a state, an average of a number or
//! the number of times an event happened, and the measure form that adds one.

use egui::{Label, RichText, Ui};
use henad_graph::sheet::document::{
    CELLS_ID, GROUND_ID, MeasureRule, PropertyType, SheetChip, SheetDocument, SheetMeasure, SheetProperty, SheetState,
    SheetValue, Slot,
};
use henad_graph::sheet::edit::{SheetEdit, SheetItem};
use henad_graph::sheet::sentence::{
    ADD_MEASURE, AVERAGE_MEASURE, AVERAGE_MEASURE_LEAD, COUNT_MEASURE, COUNT_MEASURE_IS, COUNT_MEASURE_LEAD,
    MEASURE_ALREADY_PLOTTED, MEASURE_NEEDS_NUMBER_PROPERTY, MEASURE_NEEDS_STATE, MEASURE_NUMBER_PROPERTY_EMPTY,
    MEASURE_PROPERTY_EMPTY, MEASURE_PROPERTY_NAME, MEASURE_STATE_EMPTY, MEASURE_STATE_NAME, NEW_MEASURE, PLOT, PLOTTED,
    average_measure, count_measure, measure_color_name, plot_state, remove_measure_name,
};
use henad_graph::sheet::sentence::{COUNT_AGENTS_LEAD, COUNT_AGENTS_MEASURE, count_agents_measure};
use henad_graph::sheet::sentence::{
    EVENT_COUNT_LEAD, EVENT_COUNT_MEASURE, EVENT_COUNT_TAIL, MEASURE_EVENT_EMPTY, MEASURE_EVENT_NAME,
    MEASURE_NEEDS_EVENT, NO_EVENT, event_count_measure, event_count_name,
};

use super::layout::{Column, Form, Table, line, text_cell, wrapped_line};
use super::starts::{STATE_SWATCH_WIDTH, state_name};
use super::{
    DELETE_WIDTH, FormKind, MeasureForm, MeasureKind, PaneState, Pending, form_buttons, form_heading, measured_thing,
    select,
};
use crate::ui::editor::sheet::widgets::{
    OverlayItem, color_button, color_name, color32, message_ui, set_name, swatch_ui,
};
use crate::ui::editor::sheet::{find_property, properties_of};

/// Returns how `state` of `property` is named on its own: its name, or the property's name and its name when another
/// property of the kind of thing measured holds a state of that name.
fn state_label(document: &SheetDocument, property: &SheetProperty, state: &SheetState) -> String {
    let shared = properties_of(document, measured_thing(document)).iter().any(|other| {
        other.id != property.id
            && other
                .states
                .iter()
                .any(|known| known.name.eq_ignore_ascii_case(&state.name))
    });
    if shared {
        format!("{} {}", property.name, state.name)
    } else {
        state.name.clone()
    }
}

/// Returns the measures of `document` that count the cells whose property `property` is `state`.
fn counts_of<'a>(document: &'a SheetDocument, property: &str, state: &str) -> Vec<&'a SheetMeasure> {
    document
        .measures
        .iter()
        .filter(|measure| {
            measure.rule == MeasureRule::CountInState
                && measure.property.as_deref() == Some(property)
                && measure.state.as_deref() == Some(state)
        })
        .collect()
}

/// Returns the measures of `document` that average the property `property`.
fn averages_of<'a>(document: &'a SheetDocument, property: &str) -> Vec<&'a SheetMeasure> {
    document
        .measures
        .iter()
        .filter(|measure| {
            measure.rule == MeasureRule::Average
                && matches!(measure.value.value(), Some(SheetValue::Chip(SheetChip::My(id))) if id == property)
        })
        .collect()
}

/// Returns the command that adds a measure counting the cells or agents whose `property` is `state`, named by the
/// state.
fn add_count(document: &SheetDocument, property: &SheetProperty, state: &SheetState) -> SheetEdit {
    let taken = document
        .measures
        .iter()
        .any(|measure| measure.name.eq_ignore_ascii_case(&state.name));
    let name = if taken {
        format!("{} {}", property.name, state.name)
    } else {
        state_label(document, property, state)
    };
    SheetEdit::AddMeasure {
        name,
        rule: MeasureRule::CountInState,
        over: Some(measured_thing(document).to_owned()),
        property: Some(property.id.clone()),
        state: Some(state.id.clone()),
        event: None,
        value: Slot::Blank,
        color: None,
    }
}

/// Returns the words of `measure` in the Plotted list, such as "Number of cells whose Status is Alive", or its name for
/// a measure no form of the pane makes.
fn measure_words(document: &SheetDocument, measure: &SheetMeasure) -> String {
    let property = |id: &str| find_property(document, id);
    match measure.rule {
        MeasureRule::CountInState => {
            let found = measure.property.as_deref().and_then(property);
            let state = found
                .zip(measure.state.as_deref())
                .and_then(|(found, state)| found.states.iter().find(|known| known.id == state));
            match (found, state) {
                (Some(found), Some(state)) if measure.over.as_deref() == Some(CELLS_ID) => {
                    count_measure(&found.name, &state.name)
                }
                (Some(found), Some(state)) if measure.over.as_deref() != Some(GROUND_ID) => {
                    count_agents_measure(&found.name, &state.name)
                }
                _ => measure.name.clone(),
            }
        }
        MeasureRule::Average => match measure.value.value() {
            Some(SheetValue::Chip(SheetChip::My(id))) => {
                property(id).map_or_else(|| measure.name.clone(), |found| average_measure(&found.name))
            }
            _ => measure.name.clone(),
        },
        MeasureRule::EventCount => document
            .events
            .iter()
            .find(|event| measure.event.as_deref() == Some(event.id.as_str()))
            .map_or_else(|| measure.name.clone(), |event| event_count_measure(&event.name)),
        _ => measure.name.clone(),
    }
}

/// Draws the Measures section: a Plot box beside each state, the table of measures plotted with each measure's colour
/// and delete button, and the measure form or "+ Add measure".
pub(super) fn measures(ui: &mut Ui, document: &SheetDocument, pane: &mut PaneState, pending: &mut Vec<Pending>) {
    let states: Vec<&SheetProperty> = properties_of(document, measured_thing(document))
        .iter()
        .filter(|property| property.property_type == PropertyType::States)
        .collect();
    let labels: Vec<&str> = states
        .iter()
        .flat_map(|property| property.states.iter().map(|state| state.name.as_str()))
        .collect();
    let swatch_room = STATE_SWATCH_WIDTH;
    let form = Form::new(ui, &labels).wider(swatch_room);
    for property in states {
        text_cell(ui, Label::new(RichText::new(&property.name).weak()));
        // One row per state. egui wraps no nested group, so a row of groups would run past a narrow pane.
        for state in &property.states {
            form.row_with(
                ui,
                |ui| {
                    ui.spacing_mut().item_spacing.x = swatch_room - crate::ui::editor::sheet::widgets::CHIP_SWATCH;
                    swatch_ui(ui, color32(state.color));
                    ui.add(Label::new(&state.name).truncate());
                },
                |ui| {
                    let plotted = counts_of(document, &property.id, &state.id);
                    let mut on = !plotted.is_empty();
                    let response = ui.checkbox(&mut on, PLOT);
                    set_name(ui, response.id, &plot_state(&state_label(document, property, state)));
                    if response.changed() {
                        pending.push(if on {
                            Pending::one(add_count(document, property, state))
                        } else {
                            let items: Vec<SheetItem> = plotted
                                .iter()
                                .map(|measure| SheetItem::Measure(measure.id.clone()))
                                .collect();
                            Pending {
                                starts_only: false,
                                form: None,
                                run: Box::new(move |builder| {
                                    items
                                        .into_iter()
                                        .try_for_each(|item| builder.apply(SheetEdit::Delete { item }).map(drop))
                                }),
                            }
                        });
                    }
                },
            );
        }
        ui.add_space(4.0);
    }
    if !document.measures.is_empty() {
        let columns = vec![
            Column::new(PLOTTED, 18.0, 18.0),
            Column::new("", 160.0, 80.0),
            Column::fixed(DELETE_WIDTH),
        ];
        let id = ui.make_persistent_id("henad_sheet_plotted");
        Table::new(id, columns, 1, &[(1, 0.0)]).show(ui, |ui, rows| {
            for measure in &document.measures {
                rows.row(ui, &measure.id, |row| {
                    row.cell(0, |ui| {
                        line(ui, |ui| {
                            let name = measure_color_name(&measure.name, color_name(measure.color));
                            if let Some(color) = color_button(ui, &name, measure.color) {
                                pending.push(Pending::one(SheetEdit::Recolor {
                                    item: SheetItem::Measure(measure.id.clone()),
                                    color,
                                }));
                            }
                        });
                    });
                    row.cell(1, |ui| {
                        text_cell(ui, Label::new(measure_words(document, measure)));
                    });
                    if row.delete_cell(2, &remove_measure_name(&measure.name)) {
                        pending.push(Pending::one(SheetEdit::Delete {
                            item: SheetItem::Measure(measure.id.clone()),
                        }));
                    }
                });
            }
        });
    }
    ui.add_space(6.0);
    if let Some(form) = &mut pane.measure_form {
        let mut close = false;
        measure_form(ui, document, form, &mut close, pending);
        if close {
            pane.measure_form = None;
        }
    } else if crate::ui::editor::sheet::widgets::add_row_button(ui, ADD_MEASURE, ADD_MEASURE, None, false).clicked() {
        pane.measure_form = Some(MeasureForm::add(document));
    }
}

/// Adds a select named `name` of `properties` showing the one of id `current`, or `empty` while none is chosen, and
/// returns the id of the property chosen.
pub(super) fn property_select(
    ui: &mut Ui,
    id_salt: &str,
    name: &str,
    properties: &[&SheetProperty],
    current: Option<&str>,
    empty: &str,
) -> Option<String> {
    let shown = current
        .and_then(|id| properties.iter().find(|property| property.id == id))
        .map_or(empty, |property| property.name.as_str());
    let items: Vec<OverlayItem<'_>> = properties
        .iter()
        .map(|property| OverlayItem {
            label: &property.name,
            current: current == Some(property.id.as_str()),
            ..OverlayItem::default()
        })
        .collect();
    let (_, chosen) = select(ui, id_salt, name, shown, &items);
    let chosen = chosen.and_then(|index| properties.get(index))?;
    (current != Some(chosen.id.as_str())).then(|| chosen.id.clone())
}

/// Draws the measure form, and sets `close` on Cancel. Done checks the form and adds the measure.
fn measure_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut MeasureForm,
    close: &mut bool,
    pending: &mut Vec<Pending>,
) {
    let properties = properties_of(document, measured_thing(document));
    let agents = measured_thing(document) != CELLS_ID;
    let of_kind = |kinds: &[PropertyType]| -> Vec<&SheetProperty> {
        properties
            .iter()
            .filter(|property| kinds.contains(&property.property_type))
            .collect()
    };
    let states = of_kind(&[PropertyType::States]);
    // An average of agents' positions is their centre.
    let numbers = of_kind(&[PropertyType::Number, PropertyType::WholeNumber, PropertyType::Position]);
    form_heading(ui, NEW_MEASURE);
    kind_radios(ui, document, form, agents);
    wrapped_line(ui, |ui| {
        if form.kind == MeasureKind::EventCount {
            event_count_row(ui, document, form);
        } else if form.kind == MeasureKind::Average {
            ui.label(AVERAGE_MEASURE_LEAD);
            if let Some(chosen) = property_select(
                ui,
                "henad_sheet_measure_number",
                MEASURE_PROPERTY_NAME,
                &numbers,
                form.property.as_deref(),
                MEASURE_NUMBER_PROPERTY_EMPTY,
            ) {
                form.property = Some(chosen);
            }
        } else {
            ui.label(if agents { COUNT_AGENTS_LEAD } else { COUNT_MEASURE_LEAD });
            if let Some(chosen) = property_select(
                ui,
                "henad_sheet_measure_property",
                MEASURE_PROPERTY_NAME,
                &states,
                form.property.as_deref(),
                MEASURE_PROPERTY_EMPTY,
            ) {
                form.property = Some(chosen);
                form.state = None;
            }
            ui.label(COUNT_MEASURE_IS);
            let property = form
                .property
                .as_deref()
                .and_then(|id| states.iter().find(|property| property.id == id));
            let shown = property
                .zip(form.state.as_deref())
                .and_then(|(property, state)| state_name(property, state))
                .unwrap_or(MEASURE_STATE_EMPTY);
            let offered = property.map(|property| property.states.as_slice()).unwrap_or_default();
            let items: Vec<OverlayItem<'_>> = offered
                .iter()
                .map(|state| OverlayItem {
                    label: &state.name,
                    current: form.state.as_deref() == Some(state.id.as_str()),
                    swatch: Some(color32(state.color)),
                    ..OverlayItem::default()
                })
                .collect();
            let (_, chosen) = select(ui, "henad_sheet_measure_state", MEASURE_STATE_NAME, shown, &items);
            if let Some(state) = chosen.and_then(|index| offered.get(index)) {
                form.state = Some(state.id.clone());
            }
        }
    });
    if let Some(message) = &form.message {
        message_ui(ui, message);
    }
    let (done, cancel) = form_buttons(ui, |_| {});
    if done {
        match measure_edit(document, form) {
            Ok(edit) => {
                pending.push(Pending::from_form(
                    FormKind::Measure,
                    Box::new(move |builder| builder.apply(edit).map(drop)),
                ));
            }
            Err(message) => form.message = Some(message),
        }
    }
    *close |= cancel;
}

/// Draws a radio button for each kind of measure the form adds, on a world of agents when `agents` holds, and starts
/// `form` afresh when another kind is chosen.
fn kind_radios(ui: &mut Ui, document: &SheetDocument, form: &mut MeasureForm, agents: bool) {
    let kind = form.kind;
    line(ui, |ui| {
        ui.radio_value(
            &mut form.kind,
            MeasureKind::Count,
            if agents { COUNT_AGENTS_MEASURE } else { COUNT_MEASURE },
        );
    });
    line(ui, |ui| {
        ui.radio_value(&mut form.kind, MeasureKind::Average, AVERAGE_MEASURE);
    });
    // Only agents record events. With none to count, the choice shows greyed with what to do first.
    if agents {
        line(ui, |ui| {
            let events = !document.events.is_empty();
            let radio = ui
                .add_enabled_ui(events, |ui| {
                    ui.radio_value(&mut form.kind, MeasureKind::EventCount, EVENT_COUNT_MEASURE)
                })
                .inner;
            if !events {
                radio.on_disabled_hover_text(NO_EVENT);
            }
        });
    }
    if form.kind != kind {
        *form = MeasureForm {
            kind: form.kind,
            ..MeasureForm::default()
        };
    }
}

/// Draws the words and the event select of an event count, "Number of times [event] happened", in `form`.
fn event_count_row(ui: &mut Ui, document: &SheetDocument, form: &mut MeasureForm) {
    ui.label(EVENT_COUNT_LEAD);
    let shown = document
        .events
        .iter()
        .find(|event| form.event.as_deref() == Some(event.id.as_str()))
        .map_or(MEASURE_EVENT_EMPTY, |event| event.name.as_str());
    let items: Vec<OverlayItem<'_>> = document
        .events
        .iter()
        .map(|event| OverlayItem {
            label: &event.name,
            current: form.event.as_deref() == Some(event.id.as_str()),
            ..OverlayItem::default()
        })
        .collect();
    let (_, chosen) = select(ui, "henad_sheet_measure_event", MEASURE_EVENT_NAME, shown, &items);
    if let Some(event) = chosen.and_then(|index| document.events.get(index)) {
        form.event = Some(event.id.clone());
    }
    ui.label(EVENT_COUNT_TAIL);
}

/// Returns the command that adds a measure counting the times the event of id `event` happened, named by the
/// event's plural, or by the measure's words when another measure holds that name.
///
/// # Errors
///
/// Returns the message on an event the sheet does not hold or one already counted.
fn event_count_edit(document: &SheetDocument, event: Option<&str>) -> Result<SheetEdit, String> {
    let Some(event) = event.and_then(|id| document.events.iter().find(|known| known.id == id)) else {
        return Err(MEASURE_NEEDS_EVENT.to_owned());
    };
    let counted = document
        .measures
        .iter()
        .any(|measure| measure.rule == MeasureRule::EventCount && measure.event.as_deref() == Some(&event.id));
    if counted {
        return Err(MEASURE_ALREADY_PLOTTED.to_owned());
    }
    let plural = event_count_name(&event.name);
    let taken = document
        .measures
        .iter()
        .any(|measure| measure.name.eq_ignore_ascii_case(&plural));
    Ok(SheetEdit::AddMeasure {
        name: if taken {
            event_count_measure(&event.name)
        } else {
            plural
        },
        rule: MeasureRule::EventCount,
        over: None,
        property: None,
        state: None,
        event: Some(event.id.clone()),
        value: Slot::Blank,
        color: None,
    })
}

/// Returns the command Done of the measure form makes, or the message for the first check that fails.
///
/// # Errors
///
/// Returns the message on a count with no property or no state, an average with no number property, an event count
/// with no event, or a measure already plotted.
fn measure_edit(document: &SheetDocument, form: &MeasureForm) -> Result<SheetEdit, String> {
    if form.kind == MeasureKind::EventCount {
        return event_count_edit(document, form.event.as_deref());
    }
    let property = form.property.as_deref().and_then(|id| {
        properties_of(document, measured_thing(document))
            .iter()
            .find(|known| known.id == id)
    });
    if form.kind == MeasureKind::Average {
        let Some(property) = property else {
            return Err(MEASURE_NEEDS_NUMBER_PROPERTY.to_owned());
        };
        if !averages_of(document, &property.id).is_empty() {
            return Err(MEASURE_ALREADY_PLOTTED.to_owned());
        }
        return Ok(SheetEdit::AddMeasure {
            name: average_measure(&property.name),
            rule: MeasureRule::Average,
            over: Some(measured_thing(document).to_owned()),
            property: None,
            state: None,
            event: None,
            value: Slot::Filled(SheetValue::Chip(SheetChip::My(property.id.clone()))),
            color: None,
        });
    }
    let state = property.zip(form.state.as_deref()).and_then(|(property, state)| {
        property
            .states
            .iter()
            .find(|known| known.id == state)
            .map(|state| (property, state))
    });
    let Some((property, state)) = state else {
        return Err(MEASURE_NEEDS_STATE.to_owned());
    };
    if !counts_of(document, &property.id, &state.id).is_empty() {
        return Err(MEASURE_ALREADY_PLOTTED.to_owned());
    }
    Ok(add_count(document, property, state))
}
