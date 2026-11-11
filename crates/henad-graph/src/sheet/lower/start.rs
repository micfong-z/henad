//! Starts of the cells, agents or ground: the init pass `start_cells`, `start_<kind>` or `start_ground`, and the init
//! passes a button that starts some properties again reruns. A painted ground and a start on the ground lower as
//! [`super::paint`] describes.
//!
//! An agent's Position starts anywhere at random, at a uniform point of the world. An Arrow starts as a random
//! direction, its length a number, a parameter or a random number between two values. "All Dead" and a number write
//! one value. A random number between two values draws `low + (high - low) * u`, and
//! starts a Number property only. "By chance" draws one number `u` per cell and gives the state of the first row whose
//! running total of chances passes `u`, and the "otherwise" state when none does. One row draws with `rand.chance`, a
//! node that makes that comparison itself.

use super::ids::GROUP_PART;
use super::value::{Operand, literal_operand, number, output, port, state_literal};
use super::{Builder, Site, keys};
use crate::document::{Pass, PassEnabled, PassStage};
use crate::sheet::check::Place;
use crate::sheet::document::{
    LineKind, PropertyType, SheetChip, SheetLine, SheetProperty, SheetValue, StartRule, StartsAs,
};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue};

/// Returns the id of the init pass of the starts: `start_cells`, or `start_<kind>` for a kind of agent.
pub(super) fn start_pass(builder: &Builder<'_>) -> String {
    format!("start_{}", builder.thing_id())
}

/// Writes the init pass of every property that has a start.
pub(super) fn lower(builder: &mut Builder<'_>) {
    let starting: Vec<usize> = (0..builder.properties().len())
        .filter(|&index| builder.properties()[index].starts_as.is_some())
        .collect();
    if starting.is_empty() {
        return;
    }
    let id = start_pass(builder);
    let pass = builder.add_pass(Pass::new(&id, PassStage::Init, Some(builder.domain())));
    for index in starting {
        start_property(builder, pass, index, None);
    }
}

/// Writes a disabled init pass `id` holding the starts of the properties at `indices`, the pass a button reruns
/// to start some properties again.
///
/// Its draws take keys composed with `button`, since no two draws of a file share a key.
pub(super) fn lower_restart(builder: &mut Builder<'_>, id: &str, button: &str, indices: &[usize]) {
    let mut pass = Pass::new(id, PassStage::Init, Some(builder.domain()));
    pass.enabled = PassEnabled::Literal(false);
    let pass = builder.add_pass(pass);
    for &index in indices {
        start_property(builder, pass, index, Some(button));
    }
}

/// Writes the start of the property at `index` into pass `pass`. A start rerun by button `button` composes its
/// draws' keys with the button's id.
fn start_property(builder: &mut Builder<'_>, pass: usize, index: usize, button: Option<&str>) {
    let property = &builder.properties()[index];
    let Some(start) = property.starts_as.as_ref() else {
        return;
    };
    let site = Site::sheet(
        pass,
        GROUP_PART,
        index as u32,
        format!("{}.{}.starts_as", builder.property_path(), property.id),
    );
    let place = builder.property_place(index);
    let draws = StartDraws {
        key: keys::owner_key(start.key, &builder.document.sheet.id, &property.id),
        button,
        owner: format!("the start of property '{}'", property.id),
        place: place.clone(),
    };
    if let Err(message) = start_value(builder, &site, property, start, &draws) {
        builder.problem(format!("property '{}': {message}", property.id), place);
    }
}

/// Draws of one start: the key they compose with their slot paths, and the start in words and as a table for the check
/// of their keys.
#[derive(Debug)]
pub(super) struct StartDraws<'a> {
    /// Key of the property that owns the start.
    key: u32,
    /// Button that reruns the start, whose id the keys are composed with.
    button: Option<&'a str>,
    /// Start in words.
    owner: String,
    /// Table of the start.
    place: Place,
}

impl StartDraws<'_> {
    /// Records the draw at the slot path `path` and returns its key.
    pub(super) fn record(&self, builder: &mut Builder<'_>, path: &str) -> u32 {
        let path = match self.button {
            Some(button) => format!("{path}.{button}"),
            None => path.to_owned(),
        };
        let key = keys::draw_key(self.key, &path);
        builder.record_draw(key, self.owner.clone(), self.place.clone());
        key
    }
}

/// Writes the nodes of start `start` of `property` at `site`, ending in its `lane.set`, or its `field.set` for a
/// property of the ground.
fn start_value(
    builder: &mut Builder<'_>,
    site: &Site,
    property: &SheetProperty,
    start: &StartsAs,
    draws: &StartDraws<'_>,
) -> Result<(), String> {
    let lane_type = builder.lane_type(property);
    let value = match start.rule {
        StartRule::Anywhere if property.property_type == PropertyType::Position => anywhere(builder, site, draws),
        StartRule::Value if lane_type == ValueType::Vec2F => match start.value.value() {
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomDirection => {
                let length = start_number(builder, site, phrase, draws)?;
                let key = draws.record(builder, "starts_as.value");
                random_direction(builder, site, key, length)
            }
            _ => return Err("an arrow starts as a random direction".to_owned()),
        },
        StartRule::Value => match start.value.value() {
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomNumber => {
                if lane_type != ValueType::F32 {
                    // A draw truncated to a whole number would never reach the high end.
                    return Err("a random number between two values starts a Number property only".to_owned());
                }
                let drawn = random_between(builder, site, phrase, draws, "starts_as.value")?;
                builder.input_truncated(site, &drawn, &lane_type)?
            }
            value => {
                let operand = start_operand(builder, value)?;
                builder.input_truncated(site, &operand, &lane_type)?
            }
        },
        StartRule::ByChance => by_chance(builder, site, property, start, draws)?,
        StartRule::Painted if builder.is_ground() => super::paint::painted(builder, site, property, start)?,
        StartRule::OnGround if builder.is_agents() && property.property_type == PropertyType::Position => {
            super::paint::on_ground(builder, site, start, draws)?
        }
        StartRule::Painted | StartRule::Anywhere | StartRule::OnGround => {
            return Err(format!(
                "a start of rule {} places agents or paints the ground of agents",
                start.rule.as_str()
            ));
        }
    };
    builder.write(site, &property.id, value)
}

/// Returns the state of a start by chance of `property` at `site`: one draw of `draws` at the path `starts_as.row` per
/// cell, and the state of the first row whose running total of chances passes it, or the "otherwise" state.
fn by_chance(
    builder: &mut Builder<'_>,
    site: &Site,
    property: &SheetProperty,
    start: &StartsAs,
    draws: &StartDraws<'_>,
) -> Result<InputValue, String> {
    let state = |id: &str| InputValue::Literal(state_literal(&property.id, id));
    let otherwise = start
        .otherwise
        .as_deref()
        .or_else(|| property.states.first().map(|first| first.id.as_str()))
        .ok_or_else(|| "a start by chance needs a state".to_owned())?;
    let mut rows = Vec::new();
    for row in &start.rows {
        let row_state = row.state.as_deref().ok_or_else(|| "a row has no state".to_owned())?;
        let chance = start_operand(builder, row.chance.value())?;
        rows.push((row_state, builder.input_as(site, &chance, &ValueType::F32)?));
    }
    let conditions: Vec<InputValue> = match rows.as_slice() {
        [] => Vec::new(),
        [(_, chance)] => {
            let key = (
                "key",
                ConfigValue::Integer(i64::from(draws.record(builder, "starts_as.row"))),
            );
            let node = builder.emit(site, "rand.chance", vec![key], vec![("p".to_owned(), chance.clone())]);
            vec![output(node)]
        }
        _ => {
            let key = (
                "key",
                ConfigValue::Integer(i64::from(draws.record(builder, "starts_as.row"))),
            );
            let node = builder.emit(site, "rand.float", vec![key], vec![("max".to_owned(), number(1.0))]);
            let drawn = output(node);
            let mut total: Option<InputValue> = None;
            let mut conditions = Vec::new();
            for (_, chance) in &rows {
                let running = match total {
                    None => chance.clone(),
                    Some(sum) => builder.node_input(site, "math.add", vec![("a", sum), ("b", chance.clone())]),
                };
                conditions.push(builder.node_input(site, "cmp.lt", vec![("a", drawn.clone()), ("b", running.clone())]));
                total = Some(running);
            }
            conditions
        }
    };
    let mut value = state(otherwise);
    for ((row_state, _), condition) in rows.iter().zip(conditions).rev() {
        value = builder.select(site, condition, state(row_state), value);
    }
    Ok(value)
}

/// Returns the operand of a value a start holds: a number, a percent, yes or no, a parameter or a state.
fn start_operand(builder: &Builder<'_>, value: Option<&SheetValue>) -> Result<Operand, String> {
    match value {
        Some(SheetValue::Literal(literal)) => {
            literal_operand(literal).ok_or_else(|| "a start holds no number".to_owned())
        }
        Some(SheetValue::Chip(SheetChip::State(state))) => Ok(Operand::State(state.clone())),
        Some(SheetValue::Chip(SheetChip::Param(id))) => builder.param(id),
        _ => Err("a start holds a value it cannot start from".to_owned()),
    }
}

/// Returns a uniform point of the world, drawn at `site` with the draws of `draws` at the paths `starts_as.x` and
/// `starts_as.y`.
fn anywhere(builder: &mut Builder<'_>, site: &Site, draws: &StartDraws<'_>) -> InputValue {
    let world = builder.emit(site, "ctx.world", Vec::new(), Vec::new());
    let sides = builder.emit(site, "vec.split", Vec::new(), vec![("v".to_owned(), output(world))]);
    let [x, y] = [("starts_as.x", "x"), ("starts_as.y", "y")].map(|(path, side)| {
        let key = draws.record(builder, path);
        let node = builder.emit(
            site,
            "rand.float",
            vec![("key", ConfigValue::Integer(i64::from(key)))],
            vec![("max".to_owned(), port(sides, side))],
        );
        output(node)
    });
    let node = builder.emit(
        site,
        "vec.make",
        Vec::new(),
        vec![("x".to_owned(), x), ("y".to_owned(), y)],
    );
    output(node)
}

/// Returns an arrow of length `length` in a random direction, its angle drawn at `site` with key `key`.
pub(super) fn random_direction(builder: &mut Builder<'_>, site: &Site, key: u32, length: InputValue) -> InputValue {
    let angle = builder.emit(
        site,
        "rand.float",
        vec![("key", ConfigValue::Integer(i64::from(key)))],
        vec![("max".to_owned(), number(std::f32::consts::TAU))],
    );
    builder.node_input(
        site,
        "vec.from_angle",
        vec![("angle", output(angle)), ("length", length)],
    )
}

/// Returns the random number between two values of the phrase `phrase` a start holds, `low + rand.float(high - low)`,
/// written at `site` with the draw of `draws` at the slot path `path`.
fn random_between(
    builder: &mut Builder<'_>,
    site: &Site,
    phrase: &SheetLine,
    draws: &StartDraws<'_>,
    path: &str,
) -> Result<Operand, String> {
    let low = start_operand(builder, phrase.low.value())?;
    let high = start_operand(builder, phrase.high.value())?;
    let low = builder.input_as(site, &low, &ValueType::F32)?;
    let high = builder.input_as(site, &high, &ValueType::F32)?;
    let span = builder.node_input(site, "math.sub", vec![("a", high), ("b", low.clone())]);
    let key = draws.record(builder, path);
    let node = builder.emit(
        site,
        "rand.float",
        vec![("key", ConfigValue::Integer(i64::from(key)))],
        vec![("max".to_owned(), span)],
    );
    Ok(builder.node_value(site, "math.add", vec![("a", output(node)), ("b", low)], ValueType::F32))
}

/// Returns the length of the random direction `phrase` a start holds: a number, a parameter, or a random number
/// between two values drawn with the draw of `draws` at the path `starts_as.value.length`.
fn start_number(
    builder: &mut Builder<'_>,
    site: &Site,
    phrase: &SheetLine,
    draws: &StartDraws<'_>,
) -> Result<InputValue, String> {
    match phrase.length.value() {
        Some(SheetValue::Phrase(length)) if length.kind == LineKind::RandomNumber => {
            let drawn = random_between(builder, site, length, draws, "starts_as.value.length")?;
            builder.input_as(site, &drawn, &ValueType::F32)
        }
        value => {
            let operand = start_operand(builder, value)?;
            builder.input_as(site, &operand, &ValueType::F32)
        }
    }
}
