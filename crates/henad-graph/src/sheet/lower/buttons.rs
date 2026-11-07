//! Buttons: an `[[action]]` per button, its id and label the button's.
//!
//! A Start again step reruns the start of its properties: the init pass of the starts when it names every property
//! that has a start, and otherwise a disabled init pass `restart_<button>` of the properties it names. A rerun keys its
//! draws on the press count, so each press draws a new layout. Set everywhere and Run once steps write the action pass
//! `button_<button>`. That pass runs on every cell or agent when the button is pressed. A button holds steps of one of
//! the two forms.
//! An action's reruns and its action pass have no order that a button's steps could set.

use super::ids::{GROUP_PART, Owner, SheetKey};
use super::start;
use super::value::Operand;
use super::{Builder, Site, card};
use crate::document::{Action, Pass, PassStage};
use crate::sheet::check::Place;
use crate::sheet::document::{ButtonStepRule, CardRuns, SheetButton, SheetChip, SheetLiteral, SheetValue};
use crate::value::Literal;

/// Writes the action of every button, with its reruns and its action pass.
pub(super) fn lower(builder: &mut Builder<'_>) {
    let document = builder.document;
    let mut run_by: Vec<(&str, &str)> = Vec::new();
    for (index, button) in document.buttons.iter().enumerate() {
        let place = Place::Button(index);
        let mut action = Action::new(&button.id, &button.name);
        let restarts = button.does.iter().any(|step| step.rule == ButtonStepRule::StartAgain);
        let acts = button.does.iter().any(|step| step.rule != ButtonStepRule::StartAgain);
        if restarts && acts {
            builder.problem(
                format!(
                    "button '{}' starts properties again and also changes them, and a button does one or the other",
                    button.id
                ),
                place,
            );
            continue;
        }
        if restarts {
            action.runs = restart(builder, button);
        } else if acts {
            let mut pass = Pass::new(
                &format!("button_{}", button.id),
                PassStage::Action,
                Some(builder.domain()),
            );
            pass.action = Some(button.id.clone());
            let pass = builder.add_pass(pass);
            for (step_index, step) in button.does.iter().enumerate() {
                let outcome = match step.rule {
                    ButtonStepRule::SetEverywhere => set_everywhere(builder, pass, index, step_index),
                    ButtonStepRule::RunOnce => {
                        let card_id = step.card.as_deref().unwrap_or("");
                        match document
                            .cards
                            .iter()
                            .position(|card| card.id == card_id && card.runs == CardRuns::Once)
                        {
                            Some(_) if run_by.iter().any(|(card, _)| *card == card_id) => Err(format!(
                                "card '{card_id}' runs for button '{}' and button '{}', and a card runs for one",
                                run_by
                                    .iter()
                                    .find(|(card, _)| *card == card_id)
                                    .map_or("", |(_, other)| *other),
                                button.id
                            )),
                            Some(card_index) => {
                                run_by.push((card_id, button.id.as_str()));
                                card::lower_card(builder, card_index, pass);
                                Ok(())
                            }
                            None => Err(format!("'{card_id}' is no card that runs once")),
                        }
                    }
                    ButtonStepRule::StartAgain => Ok(()),
                };
                if let Err(message) = outcome {
                    builder.problem(format!("button '{}': {message}", button.id), place.clone());
                }
            }
        }
        builder.graph.actions.push(action);
    }
}

/// Returns the init passes button `button` reruns, writing a pass of its own when it starts some properties again.
fn restart(builder: &mut Builder<'_>, button: &SheetButton) -> Vec<String> {
    let cells = builder.properties();
    let mut indices: Vec<usize> = Vec::new();
    for step in &button.does {
        let named: Vec<usize> = match step.thing.as_deref() {
            Some(_) => (0..cells.len()).collect(),
            None => step
                .properties
                .iter()
                .filter_map(|id| builder.property(id).map(|(index, _)| index))
                .collect(),
        };
        indices.extend(named);
    }
    indices.retain(|&index| cells[index].starts_as.is_some());
    indices.sort_unstable();
    indices.dedup();
    let starting: Vec<usize> = (0..cells.len())
        .filter(|&index| cells[index].starts_as.is_some())
        .collect();
    if indices.is_empty() {
        Vec::new()
    } else if indices == starting {
        vec![start::start_pass(builder)]
    } else {
        let id = format!("restart_{}", button.id);
        start::lower_restart(builder, &id, &button.id, &indices);
        vec![id]
    }
}

/// Writes step `step_index` of the button at `button_index`, a Set everywhere, into the action pass `pass`.
fn set_everywhere(
    builder: &mut Builder<'_>,
    pass: usize,
    button_index: usize,
    step_index: usize,
) -> Result<(), String> {
    let button = &builder.document.buttons[button_index];
    let step = &button.does[step_index];
    let property_id = step.property.as_deref().ok_or("a Set everywhere names no property")?;
    let (_, property) = builder
        .property(property_id)
        .ok_or_else(|| format!("'{property_id}' is no property of {}", builder.thing_words()))?;
    let site = Site {
        pass,
        owner: Owner::Sheet(SheetKey {
            group: GROUP_PART,
            index: step_index as u32,
        }),
        path: format!("button.{}.does.{step_index}", button.id),
        region: None,
    };
    let operand = match step.value.value() {
        Some(SheetValue::Chip(SheetChip::State(state))) => Operand::State(state.clone()),
        Some(SheetValue::Chip(SheetChip::Param(id))) => builder.param(id)?,
        Some(SheetValue::Literal(SheetLiteral::Bool(value))) => Operand::Constant(Literal::Bool(*value)),
        Some(SheetValue::Literal(literal)) => literal
            .number()
            .map(Operand::Number)
            .ok_or("a Set everywhere holds no number")?,
        _ => return Err("a Set everywhere holds no value it can set".to_owned()),
    };
    let value = builder.input_as(&site, &operand, &super::world::lane_type(property), true)?;
    builder.write(&site, property_id, value)
}
