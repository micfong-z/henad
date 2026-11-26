//! The world of a lowered sheet: the grid or the open space of the agents, a category and a lane per property, and
//! the parameters.
//!
//! An agent's Position is the `vec2f` lane the spatial index reads, and an Arrow or Heading a `vec2f` lane. The
//! Heading of agents that move to places is a `vec2i` lane, a step between cells, and has no lane when nothing reads
//! it, a look at the cell ahead included. A sheet whose tick has a length declares it first among the parameters, as
//! the hidden parameter `tick_length` that names the tick's unit and that a sweep can vary.

use henad_core::authoring::primitives::space::Boundary;
use henad_core::metadata::TimeScale;
use henad_core::params::{ParamDescriptor, ParamKind};

use super::rates::TICK_LENGTH;
use super::value::state_literal;
use super::{Builder, visit_chips, visit_value_chips};
use crate::document::{AgentsWorld, Category, GridWorld, Lane, LaneRole, LaneStorage, Variant, World};
use crate::sheet::check::Place;
use crate::sheet::document::{
    CellWhich, ColorRuleKind, Edges, LineKind, MadeOf, ParameterFormat, ParameterType, PropertyType, SheetChip,
    SheetDocument, SheetLine, SheetParameter, SheetProperty, SheetTime, SheetValue,
};
use crate::sheet::sentence::ONE_TICK_IS;
use crate::types::ValueType;

/// Returns the world of `document`: its grid, or the open space of its one kind of agent.
pub(super) fn world(document: &SheetDocument) -> World {
    if document.world.made_of == MadeOf::Agents {
        let mut world = AgentsWorld::default();
        if let Some(kind) = document.agents.first() {
            world.count = kind.count;
            world.max_count = kind.max_count;
        }
        // A world is at most a few thousand cells wide, which `f32` holds exactly.
        world.width = document.world.width as f32;
        world.height = document.world.height as f32;
        return World::Agents(world);
    }
    World::Grid(GridWorld {
        width: document.world.width,
        height: document.world.height,
        boundary: match document.world.edges {
            Edges::Wrap => Boundary::Torus,
            Edges::Walls => Boundary::Bounded,
        },
    })
}

/// Returns the type of the lane of property `property`.
pub(super) fn lane_type(property: &SheetProperty) -> ValueType {
    match property.property_type {
        PropertyType::States => ValueType::Category(property.id.clone()),
        PropertyType::WholeNumber => ValueType::I32,
        PropertyType::YesNo => ValueType::Bool,
        PropertyType::Number => ValueType::F32,
        PropertyType::Arrow | PropertyType::Position | PropertyType::Heading => ValueType::Vec2F,
    }
}

/// Returns whether anything in `document` reads property `id`: a line, a formula, a measure or a colour rule.
fn is_read(document: &SheetDocument, id: &str, kind: &str) -> bool {
    let colored = document.colors.get(kind).is_some_and(|rule| {
        rule.rule == ColorRuleKind::ByHeading
            || rule.property.as_deref() == Some(id)
            || rule.shades.iter().any(|shade| shade.property == id)
    });
    let heading_ahead = document.agents.iter().any(|agent| {
        agent
            .properties
            .iter()
            .any(|property| property.id == id && property.property_type == PropertyType::Heading)
    }) && document
        .cards
        .iter()
        .flat_map(|card| card.line_tables.values())
        .any(looks_ahead);
    colored
        || heading_ahead
        || document
            .cards
            .iter()
            .flat_map(|card| card.line_tables.values())
            .any(|line| line_reads(document, line, id))
        || document.measures.iter().any(|measure| {
            measure.property.as_deref() == Some(id)
                || measure
                    .value
                    .value()
                    .is_some_and(|value| value_reads(document, value, id))
        })
}

/// Returns whether `line`, or a phrase in one of its slots at any depth, looks at the cell ahead. The cell ahead reads
/// the Heading.
fn looks_ahead(line: &SheetLine) -> bool {
    let own = line.kind == LineKind::LookAtCell && line.which == Some(CellWhich::Ahead);
    own || line.slot_values().any(|value| match value {
        SheetValue::Phrase(phrase) => looks_ahead(phrase),
        _ => false,
    })
}

/// Returns whether `line`, a phrase in it or its formula reads property `id`.
fn line_reads(document: &SheetDocument, line: &SheetLine, id: &str) -> bool {
    let mut reads = false;
    visit_chips(document, line, &mut |chip| reads |= chip_reads(chip, id));
    reads
}

/// Returns whether `value` reads property `id`.
fn value_reads(document: &SheetDocument, value: &SheetValue, id: &str) -> bool {
    let mut reads = false;
    visit_value_chips(document, value, &mut |chip| reads |= chip_reads(chip, id));
    reads
}

/// Returns whether `chip` reads property `id` of mine or of another agent.
fn chip_reads(chip: &SheetChip, id: &str) -> bool {
    matches!(chip, SheetChip::My(property) | SheetChip::MyNew(property) | SheetChip::Their(property) if property == id)
}

/// Returns the type of parameter `parameter`: `u32` for a Whole number from 0, `f32` otherwise.
pub(super) fn parameter_type(parameter: &SheetParameter) -> ValueType {
    match parameter.parameter_type {
        ParameterType::WholeNumber if parameter.min >= 0.0 => ValueType::U32,
        _ => ValueType::F32,
    }
}

/// Returns the hidden parameter that holds the length of one tick of `time`, in the tick's unit.
///
/// Its default is the sheet's length, its minimum a hundredth of it and its maximum 30 times it. A sweep can then halve
/// the tick several times or lengthen it.
fn tick_length(time: SheetTime) -> Option<ParamDescriptor> {
    let length = time.tick_length as f32;
    if !(length.is_finite() && length > 0.0) {
        return None;
    }
    let min = length / 100.0;
    let max = length * 30.0;
    (min > 0.0 && max.is_finite()).then(|| {
        ParamDescriptor::new(
            TICK_LENGTH,
            format!("{ONE_TICK_IS} ({})", time.tick_unit.plural()),
            ParamKind::F32 {
                min,
                max,
                default: length,
                step: Some(min),
            },
        )
        .hidden()
    })
}

/// Declares a category per States property and a lane per property, then the length of one tick and the parameters.
pub(super) fn declare(builder: &mut Builder<'_>) {
    let agents = builder.is_agents();
    let kind = builder.thing_id();
    for (index, property) in builder.properties().iter().enumerate() {
        match property.property_type {
            PropertyType::States | PropertyType::Number | PropertyType::WholeNumber | PropertyType::YesNo => {}
            PropertyType::Arrow | PropertyType::Position if agents => {}
            PropertyType::Heading if agents => {
                if !is_read(builder.document, &property.id, kind) {
                    builder.dropped.insert(property.id.clone());
                    continue;
                }
            }
            other => {
                builder.problem(
                    format!(
                        "property '{}' of type {} is only for agents",
                        property.name,
                        other.as_str()
                    ),
                    builder.property_place(index),
                );
                continue;
            }
        }
        let mut lane = Lane::new(&property.id, builder.domain(), builder.lane_type(property));
        if property.property_type == PropertyType::Position {
            lane.role = Some(LaneRole::Position);
        }
        if property.property_type == PropertyType::States {
            builder.graph.categories.push(category(property));
            // A category holds at most 256 variants, and one byte holds each.
            lane.storage = Some(LaneStorage::U8);
            lane.initial = property
                .states
                .first()
                .map(|state| state_literal(&property.id, &state.id));
        }
        builder.graph.lanes.push(lane);
    }
    if let Some(time) = builder.document.time {
        match tick_length(time) {
            Some(descriptor) => {
                builder.graph.params.push(descriptor);
                builder.graph.time = Some(TimeScale::new(time.tick_unit, TICK_LENGTH));
            }
            None => builder.problem(
                format!("one tick of {} cannot be a parameter's value", time.tick_length),
                Place::Time,
            ),
        }
    }
    for (index, parameter) in builder.document.parameters.iter().enumerate() {
        match descriptor(parameter) {
            Some(descriptor) => builder.graph.params.push(descriptor),
            None => builder.problem(
                format!(
                    "parameter '{}' holds a number its type does not support",
                    parameter.name
                ),
                Place::Parameter(index),
            ),
        }
    }
}

/// Returns the category of the States property `property`: a variant per state, named by the state's id and labelled
/// with its name.
pub(super) fn category(property: &SheetProperty) -> Category {
    let variants = property
        .states
        .iter()
        .map(|state| Variant {
            label: state.name.clone(),
            ..Variant::new(&state.id, state.color)
        })
        .collect();
    Category::new(&property.id, variants)
}

/// Returns the descriptor of `parameter`, `None` for a value its type cannot hold.
///
/// A Whole number with a negative minimum is an `f32` marked [`ParamDescriptor::integer`], with a step of 1 unless the
/// sheet gives one. Its minimum, maximum, default and step are whole numbers.
fn descriptor(parameter: &SheetParameter) -> Option<ParamDescriptor> {
    let integer = parameter.parameter_type == ParameterType::WholeNumber;
    let kind = if parameter_type(parameter) == ValueType::U32 {
        let whole =
            |value: f64| (value.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&value)).then_some(value as u32);
        ParamKind::U32 {
            min: whole(parameter.min)?,
            max: whole(parameter.max)?,
            default: whole(parameter.default)?,
        }
    } else {
        let float = |value: f64| {
            let narrowed = value as f32;
            (narrowed.is_finite() && (!integer || narrowed.fract() == 0.0)).then_some(narrowed)
        };
        ParamKind::F32 {
            min: float(parameter.min)?,
            max: float(parameter.max)?,
            default: float(parameter.default)?,
            step: match parameter.step {
                Some(step) => Some(float(step)?),
                None if integer => Some(1.0),
                None => None,
            },
        }
    };
    let mut descriptor = ParamDescriptor::new(parameter.id.clone(), parameter.name.clone(), kind);
    if integer {
        descriptor = descriptor.integer();
    }
    Some(match parameter.format {
        Some(ParameterFormat::Percent) => descriptor.percent(),
        None => descriptor,
    })
}
