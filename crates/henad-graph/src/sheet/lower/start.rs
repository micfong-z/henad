//! Starts of the cells or agents: the init pass `start_cells` or `start_<kind>`, and the init passes a button that
//! starts some properties again reruns.
//!
//! An agent's Position starts anywhere at random, at a uniform point of the world. An Arrow starts as a random
//! direction, its length a number, a parameter or a random number between two values. "All Dead" and a number write
//! one value. A random number between two values draws `low + (high - low) * u`, and
//! starts a Number property only. "By chance" draws one number `u` per cell and gives the state of the first row whose
//! running total of chances passes `u`, and the "otherwise" state when none does. One row draws with `rand.chance`, a
//! node that makes that comparison itself.

use super::ids::{GROUP_PART, Owner, SheetKey};
use super::value::Operand;
use super::{Builder, Site, keys, text};
use crate::document::{Pass, PassEnabled, PassStage};
use crate::sheet::check::Place;
use crate::sheet::document::{
    LineKind, PropertyType, SheetChip, SheetLine, SheetLiteral, SheetProperty, SheetValue, StartRule, StartsAs,
};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

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
    let site = Site {
        pass,
        owner: Owner::Sheet(SheetKey {
            group: GROUP_PART,
            index: index as u32,
        }),
        path: format!("{}.{}.starts_as", builder.property_path(), property.id),
        region: None,
    };
    let place = builder.property_place(index);
    if let Err(message) = start_value(builder, &site, property, start, button) {
        builder.problem(format!("property '{}': {message}", property.id), place);
    }
}

/// Writes the nodes of start `start` of `property` at `site`, ending in its `lane.set`.
fn start_value(
    builder: &mut Builder<'_>,
    site: &Site,
    property: &SheetProperty,
    start: &StartsAs,
    button: Option<&str>,
) -> Result<(), String> {
    let lane_type = super::world::lane_type(property);
    let document = builder.document;
    let key = keys::owner_key(start.key, &document.sheet.id, &property.id);
    let draw_key = |path: &str| {
        let path = match button {
            Some(button) => format!("{path}.{button}"),
            None => path.to_owned(),
        };
        keys::draw_key(key, &path)
    };
    let owner = format!("the start of property '{}'", property.id);
    let place = builder.property_place(builder.property(&property.id).map_or(0, |(index, _)| index));
    let value = match start.rule {
        StartRule::Anywhere if property.property_type == PropertyType::Position => anywhere(
            builder,
            site,
            draw_key("starts_as.x"),
            draw_key("starts_as.y"),
            &owner,
            &place,
        ),
        StartRule::Value if lane_type == ValueType::Vec2F => match start.value.value() {
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomDirection => {
                let length = start_number(builder, site, phrase, &draw_key, &owner, &place)?;
                let draw = draw_key("starts_as.value");
                builder.record_draw(draw, owner.clone(), place.clone());
                random_direction(builder, site, draw, length)
            }
            _ => return Err("an arrow starts as a random direction".to_owned()),
        },
        StartRule::Value => match start.value.value() {
            Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::RandomNumber => {
                if lane_type != ValueType::F32 {
                    // A draw truncated to a whole number would never reach the high end.
                    return Err("a random number between two values starts a Number property only".to_owned());
                }
                let low = start_operand(builder, phrase.low.value())?;
                let high = start_operand(builder, phrase.high.value())?;
                let low = builder.input_as(site, &low, &ValueType::F32, false)?;
                let high = builder.input_as(site, &high, &ValueType::F32, false)?;
                let span = builder.node_value(site, "math.sub", vec![("a", high), ("b", low.clone())], ValueType::F32);
                let span = builder.input_as(site, &span, &ValueType::F32, false)?;
                let draw = draw_key("starts_as.value");
                builder.record_draw(draw, owner, place);
                let node = builder.emit(
                    site,
                    "rand.float",
                    vec![("key", ConfigValue::Integer(i64::from(draw)))],
                    vec![("max".to_owned(), span)],
                );
                let drawn = InputValue::Link(PortRef { node, port: None });
                let sum = builder.node_value(site, "math.add", vec![("a", drawn), ("b", low)], ValueType::F32);
                builder.input_as(site, &sum, &lane_type, true)?
            }
            value => {
                let operand = start_operand(builder, value)?;
                builder.input_as(site, &operand, &lane_type, true)?
            }
        },
        StartRule::ByChance => by_chance(builder, site, property, start, draw_key("starts_as.row"), owner, place)?,
        StartRule::Painted | StartRule::OnGround if builder.is_agents() => {
            return Err(crate::sheet::sentence::NOT_YET_GROUND.to_owned());
        }
        StartRule::Painted | StartRule::Anywhere | StartRule::OnGround => {
            return Err(format!(
                "a start of rule {} places agents or paints the ground of agents",
                start.rule.as_str()
            ));
        }
    };
    builder.emit(
        site,
        "lane.set",
        vec![("lane", text(&property.id))],
        vec![("value".to_owned(), value)],
    );
    Ok(())
}

/// Returns the state of a start by chance of `property` at `site`: one draw with key `draw` per cell, and the state of
/// the first row whose running total of chances passes it, or the "otherwise" state.
///
/// `owner` and `place` name the start for the check of the draw's key.
fn by_chance(
    builder: &mut Builder<'_>,
    site: &Site,
    property: &SheetProperty,
    start: &StartsAs,
    draw: u32,
    owner: String,
    place: Place,
) -> Result<InputValue, String> {
    let lane_type = super::world::lane_type(property);
    let state = |id: &str| {
        InputValue::Literal(Literal::Category(crate::value::MemberName {
            owner: property.id.clone(),
            member: id.to_owned(),
        }))
    };
    let otherwise = start
        .otherwise
        .as_deref()
        .or_else(|| property.states.first().map(|first| first.id.as_str()))
        .ok_or_else(|| "a start by chance needs a state".to_owned())?;
    let mut rows = Vec::new();
    for row in &start.rows {
        let row_state = row.state.as_deref().ok_or_else(|| "a row has no state".to_owned())?;
        let chance = start_operand(builder, row.chance.value())?;
        rows.push((row_state, builder.input_as(site, &chance, &ValueType::F32, false)?));
    }
    let key = ("key", ConfigValue::Integer(i64::from(draw)));
    let conditions: Vec<InputValue> = match rows.as_slice() {
        [] => Vec::new(),
        [(_, chance)] => {
            builder.record_draw(draw, owner, place);
            let node = builder.emit(site, "rand.chance", vec![key], vec![("p".to_owned(), chance.clone())]);
            vec![InputValue::Link(PortRef { node, port: None })]
        }
        _ => {
            builder.record_draw(draw, owner, place);
            let node = builder.emit(
                site,
                "rand.float",
                vec![key],
                vec![("max".to_owned(), InputValue::Literal(Literal::F32(1.0)))],
            );
            let drawn = InputValue::Link(PortRef { node, port: None });
            let mut total: Option<InputValue> = None;
            let mut conditions = Vec::new();
            for (_, chance) in &rows {
                let running = match total {
                    None => chance.clone(),
                    Some(sum) => {
                        let added = builder.node_value(
                            site,
                            "math.add",
                            vec![("a", sum), ("b", chance.clone())],
                            ValueType::F32,
                        );
                        builder.input_as(site, &added, &ValueType::F32, false)?
                    }
                };
                let below = builder.node_value(
                    site,
                    "cmp.lt",
                    vec![("a", drawn.clone()), ("b", running.clone())],
                    ValueType::Bool,
                );
                conditions.push(builder.condition_input(site, &below)?);
                total = Some(running);
            }
            conditions
        }
    };
    let mut value = state(otherwise);
    for ((row_state, _), condition) in rows.iter().zip(conditions).rev() {
        let selected = builder.node_value(
            site,
            "logic.select",
            vec![("cond", condition), ("if_true", state(row_state)), ("if_false", value)],
            lane_type.clone(),
        );
        value = builder.input_as(site, &selected, &lane_type, false)?;
    }
    Ok(value)
}

/// Returns the operand of a value a start holds: a number, a percent, yes or no, a parameter or a state.
fn start_operand(builder: &Builder<'_>, value: Option<&SheetValue>) -> Result<Operand, String> {
    match value {
        Some(SheetValue::Literal(SheetLiteral::Bool(value))) => Ok(Operand::Constant(Literal::Bool(*value))),
        Some(SheetValue::Literal(literal)) => literal
            .number()
            .map(Operand::Number)
            .ok_or_else(|| "a start holds no number".to_owned()),
        Some(SheetValue::Chip(SheetChip::State(state))) => Ok(Operand::State(state.clone())),
        Some(SheetValue::Chip(SheetChip::Param(id))) => builder.param(id),
        _ => Err("a start holds a value it cannot start from".to_owned()),
    }
}

/// Returns a uniform point of the world, drawn at `site` with the keys `x` and `y`.
///
/// `owner` and `place` name the start for the check of the draws' keys.
fn anywhere(builder: &mut Builder<'_>, site: &Site, x: u32, y: u32, owner: &str, place: &Place) -> InputValue {
    let world = builder.emit(site, "ctx.world", Vec::new(), Vec::new());
    let sides = builder.emit(
        site,
        "vec.split",
        Vec::new(),
        vec![(
            "v".to_owned(),
            InputValue::Link(PortRef {
                node: world,
                port: None,
            }),
        )],
    );
    let mut drawn = Vec::with_capacity(2);
    for (key, side) in [(x, "x"), (y, "y")] {
        builder.record_draw(key, owner.to_owned(), place.clone());
        let node = builder.emit(
            site,
            "rand.float",
            vec![("key", ConfigValue::Integer(i64::from(key)))],
            vec![(
                "max".to_owned(),
                InputValue::Link(PortRef {
                    node: sides,
                    port: Some(side.to_owned()),
                }),
            )],
        );
        drawn.push(InputValue::Link(PortRef { node, port: None }));
    }
    let y = drawn.pop().unwrap_or_else(|| unreachable!("two draws were written"));
    let x = drawn.pop().unwrap_or_else(|| unreachable!("two draws were written"));
    let node = builder.emit(
        site,
        "vec.make",
        Vec::new(),
        vec![("x".to_owned(), x), ("y".to_owned(), y)],
    );
    InputValue::Link(PortRef { node, port: None })
}

/// Returns an arrow of length `length` in a random direction, its angle drawn at `site` with key `key`.
pub(super) fn random_direction(builder: &mut Builder<'_>, site: &Site, key: u32, length: InputValue) -> InputValue {
    let angle = builder.emit(
        site,
        "rand.float",
        vec![("key", ConfigValue::Integer(i64::from(key)))],
        vec![(
            "max".to_owned(),
            InputValue::Literal(Literal::F32(std::f32::consts::TAU)),
        )],
    );
    let node = builder.emit(
        site,
        "vec.from_angle",
        Vec::new(),
        vec![
            (
                "angle".to_owned(),
                InputValue::Link(PortRef {
                    node: angle,
                    port: None,
                }),
            ),
            ("length".to_owned(), length),
        ],
    );
    InputValue::Link(PortRef { node, port: None })
}

/// Returns the length of the random direction `phrase` a start holds: a number, a parameter, or a random number
/// between two values drawn with the key of the path `starts_as.value.length`.
fn start_number(
    builder: &mut Builder<'_>,
    site: &Site,
    phrase: &SheetLine,
    draw_key: &dyn Fn(&str) -> u32,
    owner: &str,
    place: &Place,
) -> Result<InputValue, String> {
    match phrase.length.value() {
        Some(SheetValue::Phrase(length)) if length.kind == LineKind::RandomNumber => {
            let low = start_operand(builder, length.low.value())?;
            let high = start_operand(builder, length.high.value())?;
            let low = builder.input_as(site, &low, &ValueType::F32, false)?;
            let high = builder.input_as(site, &high, &ValueType::F32, false)?;
            let span = builder.node_value(site, "math.sub", vec![("a", high), ("b", low.clone())], ValueType::F32);
            let span = builder.input_as(site, &span, &ValueType::F32, false)?;
            let draw = draw_key("starts_as.value.length");
            builder.record_draw(draw, owner.to_owned(), place.clone());
            let node = builder.emit(
                site,
                "rand.float",
                vec![("key", ConfigValue::Integer(i64::from(draw)))],
                vec![("max".to_owned(), span)],
            );
            let drawn = InputValue::Link(PortRef { node, port: None });
            let sum = builder.node_value(site, "math.add", vec![("a", drawn), ("b", low)], ValueType::F32);
            builder.input_as(site, &sum, &ValueType::F32, false)
        }
        value => {
            let operand = start_operand(builder, value)?;
            builder.input_as(site, &operand, &ValueType::F32, false)
        }
    }
}
