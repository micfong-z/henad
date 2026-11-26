//! The Buttons section of the Sheet pane: the buttons beside the world, the card each runs once, and the button form.

use egui::{Label, RichText, Ui};
use henad_graph::sheet::check::SheetReport;
use henad_graph::sheet::document::{
    ButtonStep, ButtonStepRule, CELLS_ID, GROUND_ID, SheetButton, SheetDocument, SheetProperty,
};
use henad_graph::sheet::edit::SheetLosses;
use henad_graph::sheet::edit::{IdSpace, SheetEdit, SheetItem, is_reserved_name};
use henad_graph::sheet::sentence::name_reserved;
use henad_graph::sheet::sentence::set_everywhere;
use henad_graph::sheet::sentence::{
    ADD_BUTTON, BUTTON_CARD_EMPTY, BUTTON_DOES, BUTTON_NAME_FIELD, BUTTON_NAME_MISSING, BUTTON_NAME_PLACEHOLDER, EDIT,
    EVERY_PROPERTY, NAME_LABEL, NEW_BUTTON, START_AGAIN, START_AGAIN_FOR, Sentences, button_name_taken,
    delete_button_name, edit_button_name, run_once, start_again_for,
};
use henad_graph::sheet::sentence::{CHANGE, CHANGE_BUTTON_HEADING, DELETE_BUTTON_HEADING};

use super::layout::{Column, Form, Table, line, text_cell, text_width};
use super::{
    ButtonForm, DELETE_WIDTH, EditRun, FormKind, PaneState, Pending, form_buttons, form_heading, form_name_field,
    measured_thing, select,
};
use super::{PendingDelete, delete_or_ask, run_or_ask};
use crate::ui::editor::sheet::card_pane::card_lines;
use crate::ui::editor::sheet::widgets::{
    CHIP_HEIGHT, OverlayItem, caption_width, hairline, message_ui, name_taken, region, small_label, text_control,
};
use crate::ui::editor::sheet::{find_property, properties_of};

/// Returns what `button` does in words, such as "Start again: Status", "Start again: every property", "Run once,
/// every cell" or "Set To food and To home to 0 everywhere".
///
/// Steps one after another that set properties to the same value everywhere read as one.
pub(crate) fn button_does(document: &SheetDocument, button: &SheetButton) -> String {
    let sentences = Sentences::new(document);
    let name_of = |id: &str| find_property(document, id).map_or_else(|| id.to_owned(), |known| known.name.clone());
    let mut does: Vec<String> = Vec::new();
    let mut set: Option<(Vec<String>, String)> = None;
    for step in &button.does {
        if step.rule == ButtonStepRule::SetEverywhere {
            let property = step.property.as_deref().unwrap_or_default();
            let value = sentences.slot(&step.value, Some(property));
            match &mut set {
                Some((properties, held)) if *held == value => properties.push(name_of(property)),
                _ => {
                    if let Some((properties, value)) = set.take() {
                        does.push(set_everywhere(&properties, &value));
                    }
                    set = Some((vec![name_of(property)], value));
                }
            }
            continue;
        }
        if let Some((properties, value)) = set.take() {
            does.push(set_everywhere(&properties, &value));
        }
        does.push(if step.rule == ButtonStepRule::StartAgain {
            match step.properties.as_slice() {
                [] => start_again_for(EVERY_PROPERTY),
                properties => {
                    let names: Vec<String> = properties.iter().map(|id| name_of(id)).collect();
                    start_again_for(&names.join(", "))
                }
            }
        } else {
            let thing = step
                .card
                .as_deref()
                .and_then(|card| document.cards.iter().find(|known| known.id == card))
                .map_or(CELLS_ID, |card| card.every.as_str());
            run_once(&sentences.thing_word(thing))
        });
    }
    if let Some((properties, value)) = set {
        does.push(set_everywhere(&properties, &value));
    }
    does.join(", ")
}

/// Draws the Buttons section: the table of buttons, each with what it does and its Edit and delete buttons, the button
/// form under the button it changes, and the card of a button that runs once. "+ Add button" opens the form for a new
/// button.
///
/// The columns give up width in the order Does, Name, and Does takes the room left over.
pub(super) fn buttons(
    ui: &mut Ui,
    document: &SheetDocument,
    report: &SheetReport,
    pane: &mut PaneState,
    pending: &mut Vec<Pending>,
) {
    let sentences = Sentences::new(document);
    if !document.buttons.is_empty() {
        let names = document
            .buttons
            .iter()
            .map(|button| text_width(ui, &button.name))
            .fold(caption_width(ui, NAME_LABEL), f32::max);
        let edit = text_width(ui, EDIT) + 6.0;
        let columns = vec![
            Column::new(NAME_LABEL, names.min(140.0), 56.0),
            Column::new(BUTTON_DOES, 160.0, 80.0),
            Column::fixed(edit),
            Column::fixed(DELETE_WIDTH),
        ];
        let id = ui.make_persistent_id("henad_sheet_buttons");
        Table::new(id, columns, 1, &[(1, 0.0), (0, 0.0)]).show(ui, |ui, rows| {
            for button in &document.buttons {
                rows.row(ui, &button.id, |row| {
                    row.cell(0, |ui| {
                        text_cell(ui, Label::new(RichText::new(&button.name).strong()));
                    });
                    row.cell(1, |ui| {
                        text_cell(ui, Label::new(RichText::new(button_does(document, button)).weak()));
                    });
                    row.cell(2, |ui| {
                        line(ui, |ui| {
                            let edit = text_control(ui, None, EDIT, &edit_button_name(&button.name), CHIP_HEIGHT);
                            if edit.clicked() {
                                pane.button_form = Some(ButtonForm::edit(button));
                            }
                        });
                    });
                    if row.delete_cell(3, &delete_button_name(&button.name)) {
                        let edit = SheetEdit::Delete {
                            item: SheetItem::Button(button.id.clone()),
                        };
                        let asked = (DELETE_BUTTON_HEADING, button.name.as_str());
                        delete_or_ask(document, edit, asked, None, &mut pane.delete_question, pending);
                    }
                });
                if let Some(form) = pane
                    .button_form
                    .as_mut()
                    .filter(|form| form.editing.as_deref() == Some(button.id.as_str()))
                {
                    let mut close = false;
                    let drawn =
                        ui.scope(|ui| button_form(ui, document, form, &mut close, &mut pane.delete_question, pending));
                    // A form opened by Edit scrolls into view, since it can open below the pane's edge.
                    if std::mem::take(&mut form.reveal) {
                        pane.reveal = Some(drawn.response.rect);
                    }
                    if close {
                        pane.button_form = None;
                    }
                    ui.add_space(6.0);
                    hairline(ui);
                }
                for card in button.does.iter().filter_map(|step| step.card.as_deref()) {
                    ui.add_space(6.0);
                    run_once_card(ui, document, &sentences, report, card);
                    ui.add_space(6.0);
                    hairline(ui);
                }
            }
        });
        ui.add_space(6.0);
    }
    match &mut pane.button_form {
        Some(form) if form.editing.is_none() => {
            let mut close = false;
            let drawn = ui.scope(|ui| button_form(ui, document, form, &mut close, &mut pane.delete_question, pending));
            // A form opened by "+ Add button" scrolls into view, since it can open below the pane's edge.
            if std::mem::take(&mut form.reveal) {
                pane.reveal = Some(drawn.response.rect);
            }
            if close {
                pane.button_form = None;
            }
        }
        _ => {
            if crate::ui::editor::sheet::widgets::add_row_button(ui, ADD_BUTTON, ADD_BUTTON, None, false).clicked() {
                pane.button_form = Some(ButtonForm::add());
            }
        }
    }
}

/// Draws the card `card` that a button runs once, read-only, as a region inside the Buttons section: its heading, then
/// each line's number and sentence, or the text of a card with no line.
fn run_once_card(ui: &mut Ui, document: &SheetDocument, sentences: &Sentences<'_>, report: &SheetReport, card: &str) {
    let Some(index) = document.cards.iter().position(|known| known.id == card) else {
        return;
    };
    region(ui, |ui| {
        egui::Frame::NONE
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 2.0;
                if let Some(card) = document.cards.get(index) {
                    ui.label(RichText::new(sentences.card_heading(card)).small());
                }
                let lines = card_lines(document, sentences, report, index);
                if lines.is_empty() {
                    ui.weak(BUTTON_CARD_EMPTY);
                }
                for line in lines {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.add_space(20.0 * line.depth as f32);
                        ui.label(small_label(line.number.to_string()));
                        ui.vertical(|ui| {
                            ui.set_max_width(ui.available_width());
                            ui.add(Label::new(&line.sentence).wrap());
                        });
                    });
                }
            });
    });
}

/// Draws the button form, and sets `close` on Cancel. Done checks the form and adds or changes the button, or holds
/// the change in `waiting` while it deletes the lines of the button's card.
fn button_form(
    ui: &mut Ui,
    document: &SheetDocument,
    form: &mut ButtonForm,
    close: &mut bool,
    waiting: &mut Option<PendingDelete>,
    pending: &mut Vec<Pending>,
) {
    let thing = measured_thing(document);
    // Agents' buttons start the ground's properties again too.
    let properties: Vec<&SheetProperty> = properties_of(document, thing)
        .iter()
        .chain(if thing == CELLS_ID {
            &[][..]
        } else {
            properties_of(document, GROUND_ID)
        })
        .filter(|property| property.starts_as.is_some())
        .collect();
    let thing_word = Sentences::new(document).thing_word(thing);
    let heading = match &form.editing {
        Some(_) => edit_button_name(&form.original),
        None => NEW_BUTTON.to_owned(),
    };
    form_heading(ui, &heading);
    let rows = Form::new(ui, &[NAME_LABEL, BUTTON_DOES]);
    let mut done = false;
    rows.row(ui, NAME_LABEL, |ui| {
        done |= form_name_field(
            ui,
            &mut form.name,
            BUTTON_NAME_PLACEHOLDER,
            BUTTON_NAME_FIELD,
            &mut form.focus_name,
        );
    });
    rows.row(ui, BUTTON_DOES, |ui| {
        ui.radio_value(&mut form.run_once, Some(false), START_AGAIN);
        ui.add_enabled_ui(form.run_once == Some(false), |ui| {
            let shown = form
                .property
                .as_deref()
                .and_then(|id| properties.iter().find(|property| property.id == id))
                .map_or(EVERY_PROPERTY, |property| property.name.as_str());
            let choices: Vec<Option<&SheetProperty>> = std::iter::once(None)
                .chain(properties.iter().copied().map(Some))
                .collect();
            let items: Vec<OverlayItem<'_>> = choices
                .iter()
                .map(|choice| OverlayItem {
                    label: choice.map_or(EVERY_PROPERTY, |property| property.name.as_str()),
                    current: form.property.as_deref() == choice.map(|property| property.id.as_str()),
                    ..OverlayItem::default()
                })
                .collect();
            let (_, chosen) = select(ui, "henad_sheet_button_property", START_AGAIN_FOR, shown, &items);
            if let Some(choice) = chosen.and_then(|index| choices.get(index)) {
                form.property = choice.map(|property| property.id.clone());
            }
        });
    });
    rows.under(ui, |ui| {
        ui.radio_value(&mut form.run_once, Some(true), run_once(&thing_word));
        // The steps that set a property everywhere, which no control of the form changes and Done keeps.
        if let Some(sets) = set_steps(document, form.editing.as_deref()) {
            ui.weak(button_does(document, &sets));
        }
        if let Some(message) = &form.message {
            message_ui(ui, message);
        }
    });
    let (pressed, cancel) = form_buttons(ui, |_| {});
    done |= pressed;
    *close |= cancel;
    if done {
        match check_button_form(document, form) {
            Ok(()) => {
                let make = || Pending::from_form(FormKind::Button, button_edit(form.clone(), thing.to_owned()));
                match &form.editing {
                    // A button that runs once and starts again in its place takes its card's lines with it.
                    Some(editing) => {
                        let asked = (CHANGE_BUTTON_HEADING, form.original.as_str(), CHANGE);
                        let own = |losses: &mut SheetLosses| losses.buttons.retain(|button| button != editing);
                        run_or_ask(document, make, asked, own, waiting, pending);
                    }
                    None => pending.push(make()),
                }
            }
            Err(message) => form.message = Some(message),
        }
    }
}

/// Returns the message for the first check of the button form that fails.
///
/// # Errors
///
/// Returns the message on an empty name, a name another button holds, or the name of a new button that Henad keeps for
/// itself.
fn check_button_form(document: &SheetDocument, form: &ButtonForm) -> Result<(), String> {
    let name = form.name.trim();
    if name.is_empty() {
        return Err(BUTTON_NAME_MISSING.to_owned());
    }
    let others = document
        .buttons
        .iter()
        .filter(|button| Some(&button.id) != form.editing.as_ref());
    if name_taken(name, others.map(|button| button.name.as_str())) {
        return Err(button_name_taken(name));
    }
    if form.editing.is_none() && is_reserved_name(name, IdSpace::Button) {
        return Err(name_reserved(name));
    }
    Ok(())
}

/// Returns button `id` with its steps that set a property everywhere alone, `None` for a button with none or no
/// button.
fn set_steps(document: &SheetDocument, id: Option<&str>) -> Option<SheetButton> {
    let mut button = document
        .buttons
        .iter()
        .find(|button| Some(button.id.as_str()) == id)?
        .clone();
    button.does.retain(|step| step.rule == ButtonStepRule::SetEverywhere);
    (!button.does.is_empty()).then_some(button)
}

/// Returns the step of a button that starts `property` again, or every property of `thing` for `None`.
fn start_again_step(property: Option<String>, thing: &str) -> ButtonStep {
    let mut step = ButtonStep::new(ButtonStepRule::StartAgain);
    match property {
        Some(property) => step.properties = vec![property],
        None => step.thing = Some(thing.to_owned()),
    }
    step
}

/// Returns the edit Done of the button form makes from `form` for a button of `thing`: a new button, or the changes to
/// the button it edits.
///
/// A button whose kind changes, from Start again to Run once or back, is deleted and added again under its new name,
/// and a Run once button's card goes with it. The steps that set a property everywhere, which no control of the form
/// sets, stay in their order, and the step of the new kind takes the place of the old one. A Start again button's
/// new start-again step takes the place of the old ones. A button that only sets properties everywhere and was given no
/// kind keeps its steps and takes the new name.
fn button_edit(form: ButtonForm, thing: String) -> EditRun {
    Box::new(move |builder| {
        let name = form.name.trim().to_owned();
        let current = form.editing.as_ref().and_then(|id| {
            builder
                .document()
                .buttons
                .iter()
                .find(|button| button.id == *id)
                .cloned()
        });
        let Some(run_once) = form.run_once else {
            if let Some(current) = current.filter(|current| current.name != name) {
                builder.apply(SheetEdit::Rename {
                    item: SheetItem::Button(current.id),
                    name,
                })?;
            }
            return Ok(());
        };
        let rule = if run_once {
            ButtonStepRule::RunOnce
        } else {
            ButtonStepRule::StartAgain
        };
        let is_set = |step: &ButtonStep| step.rule == ButtonStepRule::SetEverywhere;
        // The set steps of the button before the edit, and the place of its first other step among its steps.
        let sets: Vec<ButtonStep> = current
            .iter()
            .flat_map(|button| button.does.iter().filter(|step| is_set(step)).cloned())
            .collect();
        let kind_index = current
            .as_ref()
            .and_then(|button| button.does.iter().position(|step| !is_set(step)))
            .unwrap_or_default();
        let id = match current {
            Some(current) if current.does.iter().any(|step| step.rule == rule) => {
                if current.name != name {
                    builder.apply(SheetEdit::Rename {
                        item: SheetItem::Button(current.id.clone()),
                        name,
                    })?;
                }
                current.id
            }
            current => {
                if let Some(current) = current {
                    builder.apply(SheetEdit::Delete {
                        item: SheetItem::Button(current.id),
                    })?;
                }
                builder
                    .apply(SheetEdit::AddButton {
                        name,
                        rule,
                        thing: thing.clone(),
                    })?
                    .into_iter()
                    .next()
                    .unwrap_or_default()
            }
        };
        let held: Vec<ButtonStep> = builder
            .document()
            .buttons
            .iter()
            .find(|button| button.id == id)
            .map(|button| button.does.clone())
            .unwrap_or_default();
        let mut own: Vec<ButtonStep> = held.iter().filter(|step| !is_set(step)).cloned().collect();
        if !run_once {
            let start_again_index = own
                .iter()
                .position(|step| step.rule == ButtonStepRule::StartAgain)
                .unwrap_or_default();
            own.retain(|step| step.rule != ButtonStepRule::StartAgain);
            own.insert(
                start_again_index.min(own.len()),
                start_again_step(form.property, &thing),
            );
        }
        let mut does = sets;
        for (offset, step) in own.into_iter().enumerate() {
            does.insert((kind_index + offset).min(does.len()), step);
        }
        if does == held {
            return Ok(());
        }
        builder.apply(SheetEdit::SetButtonSteps { button: id, does }).map(drop)
    })
}
