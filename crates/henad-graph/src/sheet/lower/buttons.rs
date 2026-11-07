//! Buttons: an `[[action]]` per button, its id and label the button's.
//!
//! A Start again step reruns the start of its properties: the init pass of the starts when it names every property
//! that has a start, and otherwise a disabled init pass `restart_<button>` of the properties it names, or
//! `restart_<button>_ground` of the ground properties it names. A rerun keys its draws on the press count, so each
//! press draws a new layout. Set everywhere and Run once steps write the action pass `button_<button>`. That pass
//! runs on every cell or agent when the button is pressed, or on every cell of the ground for a Set everywhere of a
//! ground property. A button starts again or changes each kind of thing, and changes one kind of thing, so the ants'
//! Reset colony starts the ants again and clears the trails.
//! An action's reruns and its action pass have no order that a button's steps could set.

use super::ids::{GROUP_PART, Owner, SheetKey};
use super::start;
use super::value::Operand;
use super::{Builder, Site, card};
use crate::document::{Action, Pass, PassStage};
use crate::sheet::check::Place;
use crate::sheet::check::Thing;
use crate::sheet::document::{
    ButtonStep, ButtonStepRule, CardRuns, PropertyType, SheetButton, SheetChip, SheetLiteral, SheetValue,
};
use crate::value::Literal;

/// Writes the action of every button, with its reruns and its action pass.
pub(super) fn lower(builder: &mut Builder<'_>) {
    let document = builder.document;
    let mut run_by: Vec<(&str, &str)> = Vec::new();
    let ground_ids: Vec<String> = builder.ground_layers.keys().cloned().collect();
    for (index, button) in document.buttons.iter().enumerate() {
        let place = Place::Button(index);
        let mut action = Action::new(&button.id, &button.name);
        let on_ground = |step: &ButtonStep| match step.rule {
            ButtonStepRule::StartAgain => {
                step.thing.as_deref() == Some("ground") || step.properties.iter().any(|id| ground_ids.contains(id))
            }
            ButtonStepRule::SetEverywhere => step
                .property
                .as_deref()
                .is_some_and(|id| ground_ids.iter().any(|known| known == id)),
            ButtonStepRule::RunOnce => false,
        };
        let restarts = |ground: bool| {
            button
                .does
                .iter()
                .any(|step| step.rule == ButtonStepRule::StartAgain && on_ground(step) == ground)
        };
        let acts = |ground: bool| {
            button
                .does
                .iter()
                .any(|step| step.rule != ButtonStepRule::StartAgain && on_ground(step) == ground)
        };
        if (restarts(false) && acts(false)) || (restarts(true) && acts(true)) {
            builder.problem(
                format!(
                    "button '{}' starts properties again and also changes them, and a button does one or the other",
                    button.id
                ),
                place,
            );
            continue;
        }
        if acts(false) && acts(true) {
            builder.problem(
                format!(
                    "button '{}' changes {} and the ground, and a button changes one of them",
                    button.id,
                    builder.thing_words()
                ),
                place,
            );
            continue;
        }
        let ground_steps: Vec<bool> = button.does.iter().map(on_ground).collect();
        action.runs = restart(builder, button);
        if restarts(true) {
            let thing = builder.thing;
            builder.thing = Thing::Ground;
            action.runs.extend(restart(builder, button));
            builder.thing = thing;
        }
        let ground = acts(true);
        if acts(false) || ground {
            write_action_pass(builder, index, ground, &ground_steps, &mut run_by);
        }
        builder.graph.actions.push(action);
    }
}

/// Writes the action pass `button_<button>` of the button at `index`: its Set everywhere and Run once steps over the
/// ground when `ground` holds, and over the sheet's kind of thing otherwise. `ground_steps` says which steps are about
/// the ground, and `run_by` holds the button that runs each card that runs once.
fn write_action_pass<'a>(
    builder: &mut Builder<'a>,
    index: usize,
    ground: bool,
    ground_steps: &[bool],
    run_by: &mut Vec<(&'a str, &'a str)>,
) {
    let document = builder.document;
    let button = &document.buttons[index];
    let place = Place::Button(index);
    let thing = builder.thing;
    if ground {
        builder.thing = Thing::Ground;
    }
    let mut pass = Pass::new(
        &format!("button_{}", button.id),
        PassStage::Action,
        Some(builder.domain()),
    );
    pass.action = Some(button.id.clone());
    let pass = builder.add_pass(pass);
    for (step_index, step) in button.does.iter().enumerate() {
        if ground_steps[step_index] != ground {
            continue;
        }
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
    builder.thing = thing;
}

/// Returns the init passes button `button` reruns, writing a pass of its own when it starts some properties again.
fn restart(builder: &mut Builder<'_>, button: &SheetButton) -> Vec<String> {
    let cells = builder.properties();
    let mut indices: Vec<usize> = Vec::new();
    for step in &button.does {
        if step.rule != ButtonStepRule::StartAgain {
            continue;
        }
        let named: Vec<usize> = match step.thing.as_deref() {
            Some(thing) if thing == builder.thing_id() => (0..cells.len()).collect(),
            Some(_) => Vec::new(),
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
        let suffix = if builder.is_ground() { "_ground" } else { "" };
        let id = format!("restart_{}{suffix}", button.id);
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
    if builder.is_ground() && property.property_type == PropertyType::States {
        return Err(format!(
            "the ground's '{property_id}' keeps its painted states, so no button sets it"
        ));
    }
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
    let value = builder.input_as(&site, &operand, &builder.lane_type(property), true)?;
    builder.write(&site, property_id, value)
}
