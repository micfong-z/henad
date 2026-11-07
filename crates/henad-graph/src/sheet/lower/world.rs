//! The world of a lowered grid sheet: the grid, a category and a lane per cells property, and the parameters.

use henad_core::authoring::primitives::space::Boundary;
use henad_core::params::{ParamDescriptor, ParamKind};

use super::Builder;
use crate::document::{Category, Domain, GridWorld, Lane, LaneStorage, Variant, World};
use crate::sheet::check::Place;
use crate::sheet::document::{
    Edges, ParameterFormat, ParameterType, PropertyType, SheetDocument, SheetParameter, SheetProperty,
};
use crate::types::ValueType;
use crate::value::{Literal, MemberName};

/// Returns the grid of `document`, a sheet of cells.
pub(super) fn grid(document: &SheetDocument) -> World {
    World::Grid(GridWorld {
        width: document.world.width,
        height: document.world.height,
        boundary: match document.world.edges {
            Edges::Wrap => Boundary::Torus,
            Edges::Walls => Boundary::Bounded,
        },
    })
}

/// Returns the type of the lane of cells property `property`.
pub(super) fn lane_type(property: &SheetProperty) -> ValueType {
    match property.property_type {
        PropertyType::States => ValueType::Category(property.id.clone()),
        PropertyType::WholeNumber => ValueType::I32,
        PropertyType::YesNo => ValueType::Bool,
        PropertyType::Number | PropertyType::Arrow | PropertyType::Position | PropertyType::Heading => ValueType::F32,
    }
}

/// Returns the type of parameter `parameter`: `u32` for a Whole number from 0, `f32` otherwise.
pub(super) fn parameter_type(parameter: &SheetParameter) -> ValueType {
    match parameter.parameter_type {
        ParameterType::WholeNumber if parameter.min >= 0.0 => ValueType::U32,
        _ => ValueType::F32,
    }
}

/// Declares a category per States property and a lane per property of the cells, then the parameters.
pub(super) fn declare(builder: &mut Builder<'_>) {
    for (index, property) in builder.cells().iter().enumerate() {
        match property.property_type {
            PropertyType::States | PropertyType::Number | PropertyType::WholeNumber | PropertyType::YesNo => {}
            other => {
                builder.problem(
                    format!(
                        "property '{}' is {}, which only agents have",
                        property.id,
                        other.as_str()
                    ),
                    Place::Property(crate::sheet::check::Thing::Cells, index),
                );
                continue;
            }
        }
        let mut lane = Lane::new(&property.id, Domain::Cells, lane_type(property));
        if property.property_type == PropertyType::States {
            let variants = property
                .states
                .iter()
                .map(|state| Variant {
                    label: state.name.clone(),
                    ..Variant::new(&state.id, state.color)
                })
                .collect();
            builder.graph.categories.push(Category::new(&property.id, variants));
            // A category holds at most 256 variants, and one byte holds each.
            lane.storage = Some(LaneStorage::U8);
            lane.initial = property.states.first().map(|state| {
                Literal::Category(MemberName {
                    owner: property.id.clone(),
                    member: state.id.clone(),
                })
            });
        }
        builder.graph.lanes.push(lane);
    }
    for (index, parameter) in builder.document.parameters.iter().enumerate() {
        match descriptor(parameter) {
            Some(descriptor) => builder.graph.params.push(descriptor),
            None => builder.problem(
                format!(
                    "parameter '{}' holds a number a parameter of its type cannot hold",
                    parameter.id
                ),
                Place::Parameter(index),
            ),
        }
    }
}

/// Returns the descriptor of `parameter`, `None` for a value its type cannot hold.
fn descriptor(parameter: &SheetParameter) -> Option<ParamDescriptor> {
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
            narrowed.is_finite().then_some(narrowed)
        };
        ParamKind::F32 {
            min: float(parameter.min)?,
            max: float(parameter.max)?,
            default: float(parameter.default)?,
            step: match parameter.step {
                Some(step) => Some(float(step)?),
                None if parameter.parameter_type == ParameterType::WholeNumber => Some(1.0),
                None => None,
            },
        }
    };
    let descriptor = ParamDescriptor::new(parameter.id.clone(), parameter.name.clone(), kind);
    Some(match parameter.format {
        Some(ParameterFormat::Percent) => descriptor.percent(),
        None => descriptor,
    })
}
