//! The ground under agents: its fields, its own passes over the cells, and the lines of a card of agents that read and
//! mark it.
//!
//! A States property of the ground is a static field of its category. The Number properties that take marks, fade or
//! change by a button are one field per pair of marks rule and share kept per tick, each property a layer, so the
//! trails of the ants share one field and one buffer of deposits. A field whose properties keep the largest mark merges
//! with `max`, and one whose properties add up their marks with `sum` at a resolution of 2^-16. A share kept per
//! tick is the field's `finish`, a function `fade_<field>` that multiplies by it. Any other Number property is a static
//! field of its own. Each event is a `[[counter]]`.
//!
//! The ground's starts, colours and measures lower over the cells of the field grid, in the passes `start_ground`,
//! `colours_ground` and `measures_ground`. [`super::paint`] holds the starts, [`super::shades`] the colours, and
//! [`super::place`] the lines of a card of agents that read and mark the ground.
//!
//! On a card of agents, my cell is the cell of the field grid under my position, from one `space.cell_of` per pass.

use std::collections::BTreeMap;

use super::ids::GROUP_GROUND;
use super::value::{Operand, state_literal};
use super::{Builder, Site, text};
use crate::document::{Counter, Field, FieldCombine, Function, FunctionPort};
use crate::ids::NodeId;
use crate::node::{Node, NodeBody};
use crate::sheet::check::Place;
use crate::sheet::document::{
    ButtonStepRule, LineKind, Marks, PropertyType, SheetChip, SheetProperty, SheetValue, Slot,
};
use crate::sheet::limits::ADD_UP_RESOLUTION_SHIFT;
use crate::sheet::sentence;
use crate::types::ValueType;
use crate::value::{InputValue, Literal, PortRef};

/// Returns whether a line of a card of `document` leaves marks on the ground property `id`.
fn is_left(document: &crate::sheet::document::SheetDocument, id: &str) -> bool {
    document
        .cards
        .iter()
        .flat_map(|card| card.line_tables.values())
        .any(|line| line.kind == LineKind::Leave && line.ground_property.as_deref() == Some(id))
}

/// Returns whether a button of `document` sets the ground property `id` everywhere.
fn is_set_by_button(document: &crate::sheet::document::SheetDocument, id: &str) -> bool {
    document.buttons.iter().any(|button| {
        button
            .does
            .iter()
            .any(|step| step.rule == ButtonStepRule::SetEverywhere && step.property.as_deref() == Some(id))
    })
}

/// Returns the share kept per tick in `slot` as an input of a function: a number, or a Number parameter.
///
/// # Errors
///
/// Returns the message for any other value.
fn kept_input(builder: &Builder<'_>, slot: &Slot) -> Result<InputValue, String> {
    match slot.value() {
        Some(SheetValue::Literal(literal)) => literal
            .number()
            .map(|number| InputValue::Literal(Literal::F32(number as f32)))
            .ok_or_else(|| "the share kept per tick holds no number".to_owned()),
        Some(SheetValue::Chip(SheetChip::Param(id))) => match builder.param(id)? {
            Operand::Param(id, ValueType::F32) => Ok(InputValue::Param(id)),
            _ => Err(format!(
                "the share kept per tick reads '{}', which is not a Number parameter",
                builder
                    .document
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == *id)
                    .map_or(id.as_str(), |parameter| parameter.name.as_str())
            )),
        },
        _ => Err("the share kept per tick must be a number or a parameter".to_owned()),
    }
}

/// Returns the key that tells two shares kept per tick apart: a parameter's id or a number's bits.
fn kept_key(slot: &Slot) -> String {
    match slot.value() {
        Some(SheetValue::Chip(SheetChip::Param(id))) => format!("p {id}"),
        Some(SheetValue::Literal(literal)) => format!("n {:?}", literal.number().map(f64::to_bits)),
        _ => String::new(),
    }
}

/// Declares the fields of the ground, a category per States property of the ground, and a counter per event.
pub(super) fn declare(builder: &mut Builder<'_>) {
    if !builder.is_agents() {
        return;
    }
    let document = builder.document;
    for event in &document.events {
        builder.graph.counters.push(Counter { id: event.id.clone() });
    }
    let Some(ground) = &document.ground else {
        return;
    };
    // Field index of each shared field of Number properties, by marks rule and share kept per tick.
    let mut shared: BTreeMap<(bool, String), usize> = BTreeMap::new();
    for (index, property) in ground.properties.iter().enumerate() {
        let place = Place::Property(super::Thing::Ground, index);
        match property.property_type {
            PropertyType::States => {
                builder.graph.categories.push(super::world::category(property));
                builder.graph.fields.push(Field {
                    id: property.id.clone(),
                    value_type: ValueType::Category(property.id.clone()),
                    layers: vec![property.id.clone()],
                    is_static: true,
                    combine: None,
                    finish: None,
                    initial: property
                        .states
                        .first()
                        .map(|state| state_literal(&property.id, &state.id)),
                });
                builder.ground_fields.insert(property.id.clone(), property.id.clone());
            }
            PropertyType::Number => declare_number(builder, index, property, &mut shared),
            other => builder.problem(
                format!(
                    "property '{}' of the ground is {}, expected number or states",
                    property.name,
                    other.as_str()
                ),
                place,
            ),
        }
    }
}

/// Declares the field of the Number property `property` at `index` of the ground: a static field of its own, or a layer
/// of the field `shared` holds for its marks rule and share kept per tick.
fn declare_number(
    builder: &mut Builder<'_>,
    index: usize,
    property: &SheetProperty,
    shared: &mut BTreeMap<(bool, String), usize>,
) {
    let document = builder.document;
    let place = Place::Property(super::Thing::Ground, index);
    let changes = property.marks.is_some()
        || !property.kept_per_tick.is_blank()
        || is_left(document, &property.id)
        || is_set_by_button(document, &property.id);
    if !changes {
        builder.graph.fields.push(static_number(&property.id));
        builder.ground_fields.insert(property.id.clone(), property.id.clone());
        return;
    }
    let adds_up = property.marks == Some(Marks::AddUp);
    let key = (adds_up, kept_key(&property.kept_per_tick));
    if let Some(&field) = shared.get(&key) {
        let field = &mut builder.graph.fields[field];
        field.layers.push(property.id.clone());
        let id = field.id.clone();
        builder.ground_fields.insert(property.id.clone(), id);
        return;
    }
    let finish = if property.kept_per_tick.is_blank() {
        None
    } else {
        match kept_input(builder, &property.kept_per_tick) {
            Ok(kept) => {
                let function = format!("fade_{}", property.id);
                builder
                    .fades
                    .push((function.clone(), kept, format!("ground.{}.kept_per_tick", property.id)));
                Some(function)
            }
            Err(message) => {
                builder.problem(format!("{}: {message}", sentence::kept_place(&property.name)), place);
                None
            }
        }
    };
    shared.insert(key, builder.graph.fields.len());
    builder.graph.fields.push(Field {
        id: property.id.clone(),
        value_type: ValueType::F32,
        layers: vec![property.id.clone()],
        is_static: false,
        combine: Some(if adds_up {
            FieldCombine::Sum {
                resolution_shift: ADD_UP_RESOLUTION_SHIFT,
            }
        } else {
            FieldCombine::Max
        }),
        finish,
        initial: Some(Literal::F32(0.0)),
    });
    builder.ground_fields.insert(property.id.clone(), property.id.clone());
}

/// Returns the static field of the Number property `id` of the ground that nothing marks or changes.
fn static_number(id: &str) -> Field {
    Field {
        id: id.to_owned(),
        value_type: ValueType::F32,
        layers: vec![id.to_owned()],
        is_static: true,
        combine: None,
        finish: None,
        initial: Some(Literal::F32(0.0)),
    }
}

/// Returns the function `id` that multiplies a field's value by `kept`, its node ids from `next_id`.
///
/// # Errors
///
/// Returns the error `next_id` returns.
pub(super) fn fade_function<E>(
    id: &str,
    kept: InputValue,
    mut next_id: impl FnMut() -> Result<NodeId, E>,
) -> Result<Function, E> {
    let port = || FunctionPort {
        port: "value".to_owned(),
        value_type: ValueType::F32,
    };
    let input_id = next_id()?;
    let product_id = next_id()?;
    let output_id = next_id()?;
    let mut input = Node::new(input_id, "fn.input");
    input.config.insert("port".to_owned(), text("value"));
    let mut product = Node::new(product_id, "math.mul");
    product.inputs.insert(
        "a".to_owned(),
        InputValue::Link(PortRef {
            node: input_id,
            port: None,
        }),
    );
    product.inputs.insert("b".to_owned(), kept);
    let mut output = Node::new(output_id, "fn.output");
    output.config.insert("port".to_owned(), text("value"));
    output.inputs.insert(
        "value".to_owned(),
        InputValue::Link(PortRef {
            node: product_id,
            port: None,
        }),
    );
    Ok(Function {
        id: id.to_owned(),
        inputs: vec![port()],
        outputs: vec![port()],
        body: NodeBody {
            nodes: vec![input, product, output],
        },
    })
}

impl Builder<'_> {
    /// Returns the `space.cell_of` of my position in the pass over agents `pass`, writing it the first time.
    ///
    /// # Errors
    ///
    /// Returns the message for agents with no Position.
    pub(super) fn my_cell(&mut self, pass: usize) -> Result<NodeId, String> {
        if let Some(cell) = self.passes[pass].my_cell {
            return Ok(cell);
        }
        let (_, position) = self
            .position()
            .ok_or_else(|| "the agents have no Position to stand on the ground".to_owned())?;
        let read = self.read(pass, &position.id)?;
        let site = Site::sheet(pass, GROUP_GROUND, 0, format!("agents.{}", position.id));
        let read = self.input_as(&site, &read, &ValueType::Vec2F)?;
        let cell = self.emit(&site, "space.cell_of", Vec::new(), vec![("p".to_owned(), read)]);
        self.passes[pass].my_cell = Some(cell);
        Ok(cell)
    }
}
