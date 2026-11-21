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

use super::ids::GROUP_PART;
use super::start;
use super::value::{Operand, literal_operand};
use super::{Builder, Site, card};
use crate::document::{Action, Pass, PassStage};
use crate::sheet::check::Place;
use crate::sheet::check::Thing;
use crate::sheet::document::{ButtonStepRule, CardRuns, PropertyType, SheetButton, SheetChip, SheetValue};

/// Writes the action of every button, with its reruns and its action pass.
pub(super) fn lower(builder: &mut Builder<'_>) {
    let document = builder.document;
    let mut run_by: Vec<(&str, &str)> = Vec::new();
    for (index, button) in document.buttons.iter().enumerate() {
        let place = Place::Button(index);
        let mut action = Action::new(&button.id, &button.name);
        let ground_fields = &builder.ground_fields;
        let ground_steps: Vec<bool> = button
            .does
            .iter()
            .map(|step| match step.rule {
                ButtonStepRule::StartAgain => {
                    step.thing.as_deref() == Some("ground")
                        || step.properties.iter().any(|id| ground_fields.contains_key(id))
                }
                ButtonStepRule::SetEverywhere => step
                    .property
                    .as_deref()
                    .is_some_and(|id| ground_fields.contains_key(id)),
                ButtonStepRule::RunOnce => false,
            })
            .collect();
        let any_step = |start_again: bool, ground: bool| {
            button.does.iter().zip(&ground_steps).any(|(step, &on_ground)| {
                (step.rule == ButtonStepRule::StartAgain) == start_again && on_ground == ground
            })
        };
        let restarts = |ground: bool| any_step(true, ground);
        let acts = |ground: bool| any_step(false, ground);
        if (restarts(false) && acts(false)) || (restarts(true) && acts(true)) {
            builder.problem(
                format!(
                    "button '{}' cannot both start properties again and change them",
                    button.id
                ),
                place,
            );
            continue;
        }
        if acts(false) && acts(true) {
            builder.problem(
                format!(
                    "button '{}' cannot change both {} and the ground",
                    button.id,
                    builder.thing_words()
                ),
                place,
            );
            continue;
        }
        action.runs = restart(builder, button);
        if restarts(true) {
            let reruns = builder.with_thing(Thing::Ground, |builder| restart(builder, button));
            action.runs.extend(reruns);
        }
        let ground = acts(true);
        if acts(false) || ground {
            let thing = if ground { Thing::Ground } else { builder.thing };
            builder.with_thing(thing, |builder| {
                write_action_pass(builder, index, ground, &ground_steps, &mut run_by);
            });
        }
        builder.graph.actions.push(action);
    }
}

/// Writes the action pass `button_<button>` of the button at `index`: its Set everywhere and Run once steps over the
/// ground when `ground` holds, and over the sheet's kind of thing otherwise. `ground_steps` says which steps are about
/// the ground, and `run_by` holds the button that runs each card that runs once.
///
/// The pass runs over the kind of thing the builder lowers over, which the caller sets.
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
                let found = document
                    .cards
                    .iter()
                    .position(|card| card.id == card_id && card.runs == CardRuns::Once);
                let earlier = run_by
                    .iter()
                    .find(|(card, _)| *card == card_id)
                    .map(|(_, other)| *other);
                match (found, earlier) {
                    (Some(_), Some(other)) => Err(format!(
                        "card '{card_id}' cannot run for both button '{other}' and button '{}'",
                        button.id
                    )),
                    (Some(card_index), None) => {
                        run_by.push((card_id, button.id.as_str()));
                        card::lower_card(builder, card_index, pass);
                        Ok(())
                    }
                    (None, _) => Err(format!("'{card_id}' is not a card that runs once")),
                }
            }
            ButtonStepRule::StartAgain => Ok(()),
        };
        if let Err(message) = outcome {
            builder.problem(format!("button '{}': {message}", button.id), place.clone());
        }
    }
}

/// Returns the init passes button `button` reruns, writing a pass of its own when it starts some properties again.
fn restart(builder: &mut Builder<'_>, button: &SheetButton) -> Vec<String> {
    let properties = builder.properties();
    let mut indices: Vec<usize> = Vec::new();
    for step in &button.does {
        if step.rule != ButtonStepRule::StartAgain {
            continue;
        }
        let named: Vec<usize> = match step.thing.as_deref() {
            Some(thing) if thing == builder.thing_id() => (0..properties.len()).collect(),
            Some(_) => Vec::new(),
            None => step
                .properties
                .iter()
                .filter_map(|id| builder.property(id).map(|(index, _)| index))
                .collect(),
        };
        indices.extend(named);
    }
    indices.retain(|&index| properties[index].starts_as.is_some());
    indices.sort_unstable();
    indices.dedup();
    let starting: Vec<usize> = (0..properties.len())
        .filter(|&index| properties[index].starts_as.is_some())
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
    let property_id = step.property.as_deref().ok_or("a Set everywhere needs a property")?;
    let (_, property) = builder.required_property(property_id)?;
    if builder.is_ground() && property.property_type == PropertyType::States {
        return Err(format!(
            "the ground's '{property_id}' has painted states and cannot be set by a button"
        ));
    }
    let site = Site::sheet(
        pass,
        GROUP_PART,
        step_index as u32,
        format!("button.{}.does.{step_index}", button.id),
    );
    let operand = match step.value.value() {
        Some(SheetValue::Chip(SheetChip::State(state))) => Operand::State(state.clone()),
        Some(SheetValue::Chip(SheetChip::Param(id))) => builder.param(id)?,
        Some(SheetValue::Literal(literal)) => literal_operand(literal).ok_or("a Set everywhere holds no number")?,
        _ => return Err("a Set everywhere holds no value it can set".to_owned()),
    };
    let value = builder.input_truncated(&site, &operand, &builder.lane_type(property))?;
    builder.write(&site, property_id, value)
}
