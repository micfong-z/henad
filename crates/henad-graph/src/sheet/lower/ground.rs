//! The ground under agents: its fields, its own passes over the cells, and the lines of a card of agents that read and
//! mark it.
//!
//! A States property of the ground is a static field of its category. The Number properties that take marks, fade or
//! change by a button are one field per pair of marks rule and share kept per tick, each property a layer, so the
//! trails of the ants share one field and one buffer of deposits. A field whose properties keep the largest mark merges
//! with `max`, and one whose properties add up their marks with `sum` at a resolution of 2^-16 (D49). A share kept per
//! tick is the field's `finish`, a function `fade_<field>` that multiplies by it. Any other Number property is a static
//! field of its own. Each event is a `[[counter]]`.
//!
//! The ground's starts, colours and measures lower over the cells of the field grid, in the passes `start_ground`,
//! `colours_ground` and `measures_ground`. A painted start gives each cell its base state and then the state of each
//! shape that covers the cell's centre, in the shapes' order: a cell shape covers one cell, and an ellipse or a
//! rectangle is centred at a fraction of the world, its sides a fraction of the world's sides, turned by its angle in
//! degrees from the x axis towards smaller rows. A start on the ground places an agent at the centre of one of the
//! cells painted in the state it names, drawn at random when there are several.
//!
//! On a card of agents, my cell is the cell of the field grid under my position, from one `space.cell_of` per pass. A
//! *place* is a cell a line names: here, ahead, a picked cell, or the choice between them. A cell is *open* when it
//! lies on the grid and no blocked state of the ground covers it, and the cell ahead is open only when the agent has a
//! heading. A place's property is the field read at its cell. Move to takes the agent to the centre of the place of
//! the first line that applies where that place is open, and its Heading becomes the step to that cell, the shorter
//! way round a world that wraps. An agent whose place is not open stays where it is.
//! Leave deposits its amount at my cell, 0 where its condition fails, and Leaves on one field that cannot both apply
//! merge into one deposit whose layer and amount are chosen by their conditions. Record adds 1 to the event's counter
//! where its condition holds.

use std::collections::BTreeMap;

use super::card::{CardLowering, Change, ChangeKind};
use super::colours;
use super::ids::{GROUP_CHANGE, GROUP_GROUND, Owner, SheetKey};
use super::value::{Operand, PlaceValue};
use super::{Builder, Site, text};
use crate::document::{Counter, Field, FieldCombine, Function, FunctionPort, Palette};
use crate::ids::NodeId;
use crate::node::{Node, NodeBody};
use crate::sheet::check::Place;
use crate::sheet::document::{
    ButtonStepRule, CellWhich, ColorRuleKind, Edges, LineKind, Marks, PropertyType, ShadeScale, ShapeKind, SheetChip,
    SheetLine, SheetProperty, SheetValue, Slot, StartsAs,
};
use crate::sheet::sentence;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, MemberName, PortRef};

/// Exponent `k` of the resolution 2^-`k` of a ground property that adds up its marks.
const ADD_UP_RESOLUTION_SHIFT: u8 = 16;

/// Number of bands each shade of the ground takes in its palette.
const SHADE_BANDS: u32 = 6;

/// Smallest value a shade on a log scale takes the logarithm of.
const SMALLEST_SHADE: f32 = 1e-30;

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
                "the share kept per tick reads '{id}', which is no Number parameter"
            )),
        },
        _ => Err("the share kept per tick is a number or a parameter".to_owned()),
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
                    initial: property.states.first().map(|state| {
                        Literal::Category(MemberName {
                            owner: property.id.clone(),
                            member: state.id.clone(),
                        })
                    }),
                });
                builder
                    .ground_layers
                    .insert(property.id.clone(), (property.id.clone(), property.id.clone()));
            }
            PropertyType::Number => declare_number(builder, index, property, &mut shared),
            other => builder.problem(
                format!(
                    "property '{}' of the ground is {}, and the ground holds numbers and states",
                    property.id,
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
        builder
            .ground_layers
            .insert(property.id.clone(), (property.id.clone(), property.id.clone()));
        return;
    }
    let adds_up = property.marks == Some(Marks::AddUp);
    let key = (adds_up, kept_key(&property.kept_per_tick));
    if let Some(&field) = shared.get(&key) {
        let field = &mut builder.graph.fields[field];
        field.layers.push(property.id.clone());
        let id = field.id.clone();
        builder
            .ground_layers
            .insert(property.id.clone(), (id, property.id.clone()));
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
                builder.problem(format!("property '{}': {message}", property.id), place);
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
    builder
        .ground_layers
        .insert(property.id.clone(), (property.id.clone(), property.id.clone()));
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

/// Returns a number written into an expression as an `f32`.
fn expression_number(number: f64) -> String {
    format!("({:?})", number as f32)
}

/// Returns the column and row of the cell of a pass over cells and the size of the field grid, each an `f32` input,
/// written at `site`.
fn cell_and_grid(builder: &mut Builder<'_>, site: &Site) -> [InputValue; 4] {
    let link = |node: NodeId, port: &str| {
        InputValue::Link(PortRef {
            node,
            port: Some(port.to_owned()),
        })
    };
    let cell = builder.emit(site, "ctx.cell_xy", Vec::new(), Vec::new());
    let cell = builder.emit(
        site,
        "vec.split",
        Vec::new(),
        vec![("v".to_owned(), InputValue::Link(PortRef { node: cell, port: None }))],
    );
    let [x, y] = ["x", "y"].map(|side| {
        let node = builder.emit(
            site,
            "conv.to_f32",
            Vec::new(),
            vec![("a".to_owned(), link(cell, side))],
        );
        InputValue::Link(PortRef { node, port: None })
    });
    let [width, height] = grid_size(builder, site);
    [x, y, width, height]
}

/// Returns the width and height of the field grid as `f32` inputs, written at `site`.
fn grid_size(builder: &mut Builder<'_>, site: &Site) -> [InputValue; 2] {
    let grid = builder.emit(site, "ctx.grid", Vec::new(), Vec::new());
    let sides = builder.emit(
        site,
        "vec.split",
        Vec::new(),
        vec![("v".to_owned(), InputValue::Link(PortRef { node: grid, port: None }))],
    );
    ["x", "y"].map(|side| {
        let node = builder.emit(
            site,
            "conv.to_f32",
            Vec::new(),
            vec![(
                "a".to_owned(),
                InputValue::Link(PortRef {
                    node: sides,
                    port: Some(side.to_owned()),
                }),
            )],
        );
        InputValue::Link(PortRef { node, port: None })
    })
}

/// Writes the `math.expr` node `out = <expression>` over the inputs `inputs`, each an `f32`, and returns its output.
fn expression(builder: &mut Builder<'_>, site: &Site, expression: &str, inputs: Vec<(&str, InputValue)>) -> InputValue {
    let declared = inputs
        .iter()
        .map(|(name, _)| {
            ConfigValue::Table(BTreeMap::from([
                ("name".to_owned(), text(name)),
                ("type".to_owned(), text("f32")),
            ]))
        })
        .collect();
    let node = builder.emit(
        site,
        "math.expr",
        vec![
            ("text", text(&format!("out = {expression}"))),
            ("inputs", ConfigValue::Array(declared)),
            ("outputs", ConfigValue::Array(vec![text("out")])),
        ],
        inputs
            .into_iter()
            .map(|(name, input)| (name.to_owned(), input))
            .collect(),
    );
    InputValue::Link(PortRef {
        node,
        port: Some("out".to_owned()),
    })
}

/// Returns the state a painted start `start` gives the cell of a pass over cells, for the States property `property`.
///
/// # Errors
///
/// Returns the message for a property with no states, a start with no base, or a shape with no centre or no size.
pub(super) fn painted(
    builder: &mut Builder<'_>,
    site: &Site,
    property: &SheetProperty,
    start: &StartsAs,
) -> Result<InputValue, String> {
    if property.property_type != PropertyType::States {
        return Err("a painted start paints a States property".to_owned());
    }
    let state = |id: &str| {
        InputValue::Literal(Literal::Category(MemberName {
            owner: property.id.clone(),
            member: id.to_owned(),
        }))
    };
    let base = start.base.as_deref().ok_or("a painted start needs a base state")?;
    let mut value = state(base);
    if start.shapes.is_empty() {
        return Ok(value);
    }
    let [x, y, width, height] = cell_and_grid(builder, site);
    let category = ValueType::Category(property.id.clone());
    for (index, shape) in start.shapes.iter().enumerate() {
        let [cx, cy] = shape
            .center
            .ok_or_else(|| format!("shape {} has no center", index + 1))?;
        let (cx, cy) = (expression_number(cx), expression_number(cy));
        let test = match shape.shape {
            ShapeKind::Cell => format!("(abs(x - floor({cx} * w)) + abs(y - floor({cy} * h))) < 0.5"),
            ShapeKind::Ellipse | ShapeKind::Rectangle => {
                let [sx, sy] = shape.size.ok_or_else(|| format!("shape {} has no size", index + 1))?;
                let angle = shape.angle.unwrap_or(0.0).to_radians();
                let (cos, sin) = (expression_number(angle.cos()), expression_number(angle.sin()));
                let dx = format!("(x + 0.5 - {cx} * w)");
                let dy = format!("(y + 0.5 - {cy} * h)");
                let u = format!("({dx} * {cos} - {dy} * {sin})");
                let v = format!("({dx} * {sin} + {dy} * {cos})");
                let a = format!("({} * w * 0.5)", expression_number(sx));
                let b = format!("({} * h * 0.5)", expression_number(sy));
                if shape.shape == ShapeKind::Ellipse {
                    format!("(({u} * {u}) / ({a} * {a}) + ({v} * {v}) / ({b} * {b})) <= 1.0")
                } else {
                    format!("max(abs({u}) / {a}, abs({v}) / {b}) <= 1.0")
                }
            }
        };
        let covered = expression(
            builder,
            site,
            &test,
            vec![
                ("x", x.clone()),
                ("y", y.clone()),
                ("w", width.clone()),
                ("h", height.clone()),
            ],
        );
        let selected = builder.node_value(
            site,
            "logic.select",
            vec![("cond", covered), ("if_true", state(&shape.state)), ("if_false", value)],
            category.clone(),
        );
        value = builder.input_as(site, &selected, &category, false)?;
    }
    Ok(value)
}

/// Returns the position a start on the ground `start` gives an agent: the centre of a cell painted in its state, one
/// drawn with key `draw` when there are several.
///
/// `owner` and `place` name the start for the check of the draw's key.
///
/// # Errors
///
/// Returns [`sentence::ON_GROUND_CELLS`] for a state that the ground's base or a shape larger than a cell paints, or
/// that no cell shape paints.
pub(super) fn on_ground(
    builder: &mut Builder<'_>,
    site: &Site,
    start: &StartsAs,
    draw: u32,
    owner: String,
    place: Place,
) -> Result<InputValue, String> {
    let state = start.state.as_deref().unwrap_or("");
    let painted = start
        .property
        .as_deref()
        .and_then(|id| {
            builder
                .document
                .ground
                .as_ref()?
                .properties
                .iter()
                .find(|property| property.id == id)
        })
        .and_then(|property| property.starts_as.as_ref())
        .filter(|painted| painted.paints_single_cells(state));
    let Some(painted) = painted else {
        return Err(sentence::ON_GROUND_CELLS.to_owned());
    };
    let cells: Vec<[f64; 2]> = painted
        .shapes
        .iter()
        .filter(|shape| shape.state == state)
        .filter_map(|shape| shape.center)
        .collect();
    let [width, height] = grid_size(builder, site);
    let mut centers = Vec::with_capacity(cells.len());
    for [cx, cy] in &cells {
        let [x, y] = [(cx, "w", &width), (cy, "h", &height)].map(|(fraction, side, size)| {
            expression(
                builder,
                site,
                &format!("floor({} * {side}) + 0.5", expression_number(*fraction)),
                vec![(side, size.clone())],
            )
        });
        let center = builder.node_value(site, "vec.make", vec![("x", x), ("y", y)], ValueType::Vec2F);
        centers.push(builder.input_as(site, &center, &ValueType::Vec2F, false)?);
    }
    let mut chosen = centers.pop().ok_or_else(|| sentence::ON_GROUND_CELLS.to_owned())?;
    if centers.is_empty() {
        return Ok(chosen);
    }
    builder.record_draw(draw, owner, place);
    let total = (centers.len() + 1) as f32;
    let node = builder.emit(
        site,
        "rand.float",
        vec![("key", ConfigValue::Integer(i64::from(draw)))],
        vec![("max".to_owned(), InputValue::Literal(Literal::F32(total)))],
    );
    let drawn = InputValue::Link(PortRef { node, port: None });
    for (index, center) in centers.into_iter().enumerate().rev() {
        let below = builder.node_value(
            site,
            "cmp.lt",
            vec![
                ("a", drawn.clone()),
                ("b", InputValue::Literal(Literal::F32((index + 1) as f32))),
            ],
            ValueType::Bool,
        );
        let below = builder.condition_input(site, &below)?;
        let selected = builder.node_value(
            site,
            "logic.select",
            vec![("cond", below), ("if_true", center), ("if_false", chosen)],
            ValueType::Vec2F,
        );
        chosen = builder.input_as(site, &selected, &ValueType::Vec2F, false)?;
    }
    Ok(chosen)
}

/// Writes the view pass of the ground's colours: the colours of a States property of the ground, the cells in its
/// shade state shaded by the largest of its shades, or a Number property shaded as [`colours::shade`] does.
pub(super) fn colours(builder: &mut Builder<'_>) {
    let Some(rule) = builder.document.colors.get("ground").cloned() else {
        return;
    };
    let rule = &rule;
    let place = Place::Color("ground".to_owned());
    let outcome = match rule.rule {
        ColorRuleKind::ByState => by_state(builder, rule),
        ColorRuleKind::Shaded if rule.scale != Some(ShadeScale::Log) && rule.decades.is_none() => {
            match rule.property.as_deref().and_then(|id| builder.property(id)) {
                Some((_, property)) if property.property_type == PropertyType::Number => {
                    let site = colours::color_site(builder);
                    builder
                        .read(site.pass, &property.id)
                        .and_then(|read| colours::shade(builder, &site, &read, &property.id, rule))
                }
                _ => Err("the colour rule shades no Number property of the ground".to_owned()),
            }
        }
        ColorRuleKind::Shaded => {
            Err("the colour rule of the ground shades on a log scale, and only shades of a state do".to_owned())
        }
        ColorRuleKind::ByHeading => Err("the ground has no heading to colour it by".to_owned()),
    };
    if let Err(message) = outcome {
        builder.problem(message, place);
    }
}

/// Writes the colours of the ground by the states of a property, its shade state shaded by its shades.
///
/// # Errors
///
/// Returns the message for a rule that names no States property of the ground, a shade of no Number property, or a
/// palette id that a category takes.
fn by_state(builder: &mut Builder<'_>, rule: &crate::sheet::document::ColorRule) -> Result<(), String> {
    let (_, property) = rule
        .property
        .as_deref()
        .and_then(|id| builder.property(id))
        .filter(|(_, property)| property.property_type == PropertyType::States)
        .ok_or("the colour rule names no States property of the ground")?;
    let site = colours::color_site(builder);
    let read = builder.read(site.pass, &property.id)?;
    let category = ValueType::Category(property.id.clone());
    let read = builder.input_as(&site, &read, &category, false)?;
    let shade_state = match (&rule.shade_state, rule.shades.is_empty()) {
        (None, true) => {
            builder.emit(
                &site,
                "view.color",
                vec![("palette", text(&property.id))],
                vec![("index".to_owned(), read)],
            );
            return Ok(());
        }
        (Some(state), false) => state.as_str(),
        _ => return Err("a colour rule shades one state by its shades".to_owned()),
    };
    let shaded_index = property
        .states
        .iter()
        .position(|state| state.id == shade_state)
        .ok_or_else(|| format!("'{shade_state}' is no state of '{}'", property.id))?;
    let palette_id = format!("shades_{}", property.id);
    if builder
        .graph
        .categories
        .iter()
        .any(|category| category.id == palette_id)
    {
        return Err(format!(
            "the palette of the shades would take the id '{palette_id}' of a property"
        ));
    }
    let colors = shade_palette(property, rule, shaded_index);
    builder.graph.palettes.push(Palette::new(&palette_id, colors));
    let (best, offset) = largest_shade(builder, &site, rule, property.states.len())?;
    let band = shade_band(builder, &site, best, rule.scale == Some(ShadeScale::Log), rule.decades)?;
    let shaded = expression_u32(builder, &site, band, offset, shaded_index as u32)?;
    let mut inputs = vec![("select".to_owned(), read)];
    for (index, state) in property.states.iter().enumerate() {
        let entry = if index == shaded_index {
            shaded.clone()
        } else {
            InputValue::Literal(Literal::U32(index as u32))
        };
        inputs.push((format!("case.{}", state.id), entry));
    }
    let index = builder.emit(&site, "cat.switch", vec![("category", text(&property.id))], inputs);
    builder.emit(
        &site,
        "view.color",
        vec![("palette", text(&palette_id))],
        vec![(
            "index".to_owned(),
            InputValue::Link(PortRef {
                node: index,
                port: None,
            }),
        )],
    );
    Ok(())
}

/// Returns the palette of a States property `property` shaded by `rule`: each state's colour, then for each shade
/// [`SHADE_BANDS`] colours that blend from the shade state's colour, the state at `shaded_index`, to the shade's.
fn shade_palette(
    property: &SheetProperty,
    rule: &crate::sheet::document::ColorRule,
    shaded_index: usize,
) -> Vec<[u8; 4]> {
    let base = property.states[shaded_index].color;
    let mut colors: Vec<[u8; 4]> = property.states.iter().map(|state| state.color).collect();
    for shade in &rule.shades {
        for band in 1..=SHADE_BANDS {
            let share = band as f32 / SHADE_BANDS as f32;
            colors.push(std::array::from_fn(|channel| {
                let (low, high) = (f32::from(base[channel]), f32::from(shade.color[channel]));
                // A share from 0 to 1 keeps the blend between two channel values.
                (low + (high - low) * share).round() as u8
            }));
        }
    }
    colors
}

/// Returns the largest of the shades of `rule` at the cell of the colour pass, and the palette entry its bands start
/// at after the `states` entries of the states, written at `site`.
///
/// # Errors
///
/// Returns the message for a rule with no shades, or a shade of no Number property of the ground.
fn largest_shade(
    builder: &mut Builder<'_>,
    site: &Site,
    rule: &crate::sheet::document::ColorRule,
    states: usize,
) -> Result<(InputValue, InputValue), String> {
    // The largest shade, and the first entry of its bands.
    let mut largest: Option<(InputValue, InputValue)> = None;
    for (index, shade) in rule.shades.iter().enumerate() {
        let value = builder.read(site.pass, &shade.property)?;
        let value = builder.input_as(site, &value, &ValueType::F32, false)?;
        let offset = InputValue::Literal(Literal::U32(states as u32 + index as u32 * SHADE_BANDS));
        largest = Some(match largest {
            None => (value, offset),
            Some((best, best_offset)) => {
                let above = builder.node_value(
                    site,
                    "cmp.gt",
                    vec![("a", value.clone()), ("b", best.clone())],
                    ValueType::Bool,
                );
                let above = builder.condition_input(site, &above)?;
                let best = builder.node_value(
                    site,
                    "logic.select",
                    vec![("cond", above.clone()), ("if_true", value), ("if_false", best)],
                    ValueType::F32,
                );
                let offset = builder.node_value(
                    site,
                    "logic.select",
                    vec![("cond", above), ("if_true", offset), ("if_false", best_offset)],
                    ValueType::U32,
                );
                (
                    builder.input_as(site, &best, &ValueType::F32, false)?,
                    builder.input_as(site, &offset, &ValueType::U32, false)?,
                )
            }
        });
    }
    largest.ok_or_else(|| "a colour rule shades one state by its shades".to_owned())
}

/// Returns the band from 0 to [`SHADE_BANDS`] that the value `best` falls in, written at `site`: 0 for a value at or
/// below 0, and on a log scale over `decades` decades below 1 for a value below the lowest decade.
fn shade_band(
    builder: &mut Builder<'_>,
    site: &Site,
    best: InputValue,
    log: bool,
    decades: Option<u32>,
) -> Result<InputValue, String> {
    let share = if log {
        let decades = decades.unwrap_or(3).max(1);
        expression(
            builder,
            site,
            &format!(
                "log10(max(v, {})) / {} + 1.0",
                expression_number(f64::from(SMALLEST_SHADE)),
                expression_number(f64::from(decades))
            ),
            vec![("v", best.clone())],
        )
    } else {
        best.clone()
    };
    let none = expression(builder, site, "min(v, s)", vec![("v", best), ("s", share.clone())]);
    let none = builder.node_value(
        site,
        "cmp.le",
        vec![("a", none), ("b", InputValue::Literal(Literal::F32(0.0)))],
        ValueType::Bool,
    );
    let none = builder.condition_input(site, &none)?;
    let scaled = builder.node_value(
        site,
        "math.mul",
        vec![
            ("a", share),
            ("b", InputValue::Literal(Literal::F32(SHADE_BANDS as f32))),
        ],
        ValueType::F32,
    );
    let scaled = builder.input_as(site, &scaled, &ValueType::F32, false)?;
    let whole = builder.node_value(site, "conv.to_u32", vec![("a", scaled)], ValueType::U32);
    let whole = builder.input_as(site, &whole, &ValueType::U32, false)?;
    let clamped = builder.node_value(
        site,
        "math.clamp",
        vec![
            ("x", whole),
            ("lo", InputValue::Literal(Literal::U32(1))),
            ("hi", InputValue::Literal(Literal::U32(SHADE_BANDS))),
        ],
        ValueType::U32,
    );
    let clamped = builder.input_as(site, &clamped, &ValueType::U32, false)?;
    let band = builder.node_value(
        site,
        "logic.select",
        vec![
            ("cond", none),
            ("if_true", InputValue::Literal(Literal::U32(0))),
            ("if_false", clamped),
        ],
        ValueType::U32,
    );
    builder.input_as(site, &band, &ValueType::U32, false)
}

/// Returns the palette entry of a shaded cell, written at `site`: `offset + band - 1` for a band above 0, and the
/// shade state's own entry `own` otherwise.
fn expression_u32(
    builder: &mut Builder<'_>,
    site: &Site,
    band: InputValue,
    offset: InputValue,
    own: u32,
) -> Result<InputValue, String> {
    let some = builder.node_value(
        site,
        "cmp.gt",
        vec![("a", band.clone()), ("b", InputValue::Literal(Literal::U32(0)))],
        ValueType::Bool,
    );
    let some = builder.condition_input(site, &some)?;
    let entry = builder.node_value(site, "math.add", vec![("a", offset), ("b", band)], ValueType::U32);
    let entry = builder.input_as(site, &entry, &ValueType::U32, false)?;
    let entry = builder.node_value(
        site,
        "math.sub",
        vec![("a", entry), ("b", InputValue::Literal(Literal::U32(1)))],
        ValueType::U32,
    );
    let entry = builder.input_as(site, &entry, &ValueType::U32, false)?;
    let chosen = builder.node_value(
        site,
        "logic.select",
        vec![
            ("cond", some),
            ("if_true", entry),
            ("if_false", InputValue::Literal(Literal::U32(own))),
        ],
        ValueType::U32,
    );
    builder.input_as(site, &chosen, &ValueType::U32, false)
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
        let position = self
            .properties()
            .iter()
            .find(|property| property.property_type == PropertyType::Position)
            .map(|property| property.id.clone())
            .ok_or_else(|| "the agents have no Position to stand on the ground".to_owned())?;
        let read = self.read(pass, &position)?;
        let site = Site {
            pass,
            owner: Owner::Sheet(SheetKey {
                group: GROUP_GROUND,
                index: 0,
            }),
            path: format!("agents.{position}"),
            region: None,
        };
        let read = self.input_as(&site, &read, &ValueType::Vec2F, false)?;
        let cell = self.emit(&site, "space.cell_of", Vec::new(), vec![("p".to_owned(), read)]);
        self.passes[pass].my_cell = Some(cell);
        Ok(cell)
    }
}

/// A Leave line that runs: the field layer it marks, its amount and its condition.
#[derive(Debug, Clone)]
pub(super) struct LeaveMark<'a> {
    /// Id of the line.
    line: &'a str,
    /// Field the line marks.
    field: String,
    /// Layer of the field the line marks.
    layer: String,
    /// Amount the line leaves.
    amount: Operand,
    /// Condition under which the line leaves its amount, the conditions of the When lines around it joined in.
    condition: Operand,
}

impl<'a> CardLowering<'_, 'a> {
    /// Returns the ground property `property` at the cell `at`, read at `site`.
    pub(super) fn ground_at(&mut self, site: &Site, property: &str, at: InputValue) -> Result<Operand, String> {
        let found = self
            .builder
            .document
            .ground
            .as_ref()
            .and_then(|ground| ground.properties.iter().find(|candidate| candidate.id == property))
            .ok_or_else(|| format!("'{property}' is no property of the ground"))?;
        let value_type = super::world::lane_type(found);
        let (config, mut inputs) = self.builder.ground_access(property)?;
        inputs.push(("at".to_owned(), at));
        let node = self.builder.emit(site, "field.get", config, inputs);
        Ok(Operand::Link(PortRef { node, port: None }, value_type))
    }

    /// Returns whether no blocked state of the ground covers the cell `at`, written at `site`.
    pub(super) fn open_at(&mut self, site: &Site, at: &InputValue) -> Result<Operand, String> {
        let blocked = self
            .builder
            .document
            .ground
            .as_ref()
            .map(|ground| ground.blocked.clone())
            .unwrap_or_default();
        let mut open = Operand::Constant(Literal::Bool(true));
        for entry in &blocked {
            let value = self.ground_at(site, &entry.property, at.clone())?;
            let category = ValueType::Category(entry.property.clone());
            let value = self.builder.input_as(site, &value, &category, false)?;
            let other = self.builder.node_value(
                site,
                "cmp.ne",
                vec![
                    ("a", value),
                    (
                        "b",
                        InputValue::Literal(Literal::Category(MemberName {
                            owner: entry.property.clone(),
                            member: entry.state.clone(),
                        })),
                    ),
                ],
                ValueType::Bool,
            );
            open = self.builder.join(site, "logic.and", &open, &other)?;
        }
        Ok(open)
    }

    /// Returns my cell as a place.
    pub(super) fn my_place(&mut self) -> Result<Operand, String> {
        let cell = self.builder.my_cell(self.pass)?;
        Ok(Operand::Place(Box::new(PlaceValue {
            xy: Operand::Link(
                PortRef {
                    node: cell,
                    port: Some("xy".to_owned()),
                },
                ValueType::Vec2I,
            ),
            amount: None,
            open: None,
        })))
    }

    /// Returns the cell of the place `place` and whether it lies on the grid, through a `space.cell_index` at `site`.
    fn place_cell(&mut self, site: &Site, place: &PlaceValue) -> Result<NodeId, String> {
        let xy = self.builder.input_as(site, &place.xy, &ValueType::Vec2I, false)?;
        Ok(self
            .builder
            .emit(site, "space.cell_index", Vec::new(), vec![("xy".to_owned(), xy)]))
    }

    /// Returns whether the place `place` is open, written at `site`.
    pub(super) fn place_open(&mut self, site: &Site, place: &PlaceValue) -> Result<Operand, String> {
        if let Some(open) = &place.open {
            return Ok(open.clone());
        }
        let cell = self.place_cell(site, place)?;
        let open = self.open_at(
            site,
            &InputValue::Link(PortRef {
                node: cell,
                port: Some("cell".to_owned()),
            }),
        )?;
        let valid = Operand::Link(
            PortRef {
                node: cell,
                port: Some("valid".to_owned()),
            },
            ValueType::Bool,
        );
        self.builder.join(site, "logic.and", &valid, &open)
    }

    /// Returns the ground property `property` at the place `place`, written at `site`.
    pub(super) fn place_property(
        &mut self,
        site: &Site,
        place: &PlaceValue,
        property: &str,
    ) -> Result<Operand, String> {
        let cell = self.place_cell(site, place)?;
        self.ground_at(
            site,
            property,
            InputValue::Link(PortRef {
                node: cell,
                port: Some("cell".to_owned()),
            }),
        )
    }

    /// Returns the part `part` of the place `place` that line `line` gives, written at `site`: its amount or whether
    /// it is open.
    pub(super) fn place_part(
        &mut self,
        site: &Site,
        line: &str,
        place: &PlaceValue,
        part: &str,
    ) -> Result<Operand, String> {
        match part {
            "amount" => place
                .amount
                .clone()
                .ok_or_else(|| format!("line {line} gives a place that no pick ranked, so it has no amount")),
            "open" => self.place_open(site, place),
            other => Err(format!("line {line} has no part '{other}' to read")),
        }
    }

    /// Returns the place a Look at a cell line or phrase names: my cell, the cell ahead of me, or the place in its
    /// slot.
    pub(super) fn look_at_cell(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        match line.which {
            Some(CellWhich::Here) => self.my_place(),
            Some(CellWhich::Ahead) => self.ahead(site),
            Some(CellWhich::At) => match self.slot(&site.at("place"), &line.place)? {
                place @ Operand::Place(_) => Ok(place),
                _ => Err("the cell to look at is no place".to_owned()),
            },
            None => Err("the line names no cell to look at".to_owned()),
        }
    }

    /// Returns the cell ahead of me, one step of my Heading from my cell, open only when I have a heading.
    fn ahead(&mut self, site: &Site) -> Result<Operand, String> {
        let heading = self
            .builder
            .properties()
            .iter()
            .find(|property| property.property_type == PropertyType::Heading)
            .map(|property| property.id.clone());
        let heading = match heading {
            Some(heading) if self.builder.cell_mover => heading,
            _ => return Err(sentence::AHEAD_NEEDS_CELLS.to_owned()),
        };
        let step = self.builder.read(self.pass, &heading)?;
        let step = self.builder.input_as(site, &step, &ValueType::Vec2I, false)?;
        let cell = self.builder.my_cell(self.pass)?;
        let here = InputValue::Link(PortRef {
            node: cell,
            port: Some("xy".to_owned()),
        });
        let mut xy = self.builder.node_value(
            site,
            "math.add",
            vec![("a", here), ("b", step.clone())],
            ValueType::Vec2I,
        );
        if self.builder.document.world.edges == Edges::Wrap {
            let world = self.builder.emit(site, "ctx.world", Vec::new(), Vec::new());
            let point = self.builder.input_as(site, &xy, &ValueType::Vec2F, false)?;
            let wrapped = self.builder.node_value(
                site,
                "space.wrap",
                vec![
                    ("v", point),
                    (
                        "world",
                        InputValue::Link(PortRef {
                            node: world,
                            port: None,
                        }),
                    ),
                ],
                ValueType::Vec2F,
            );
            let whole = self.builder.input_as(site, &wrapped, &ValueType::Vec2I, false)?;
            xy = match whole {
                InputValue::Link(link) => Operand::Link(link, ValueType::Vec2I),
                _ => return Err("the cell ahead has no column and row".to_owned()),
            };
        }
        let mut place = PlaceValue {
            xy,
            amount: None,
            open: None,
        };
        let open = self.place_open(site, &place)?;
        let moving = self.builder.node_value(
            site,
            "cmp.ne",
            vec![("a", step), ("b", InputValue::Literal(Literal::Vec2I([0, 0])))],
            ValueType::Bool,
        );
        place.open = Some(self.builder.join(site, "logic.and", &open, &moving)?);
        Ok(Operand::Place(Box::new(place)))
    }

    /// Returns the place a Choose gives from the places `places`, each with the site of its row and the condition that
    /// picks it, as `logic.select` chains from the last place up over `otherwise`.
    ///
    /// The chosen place keeps an amount when every place has one.
    pub(super) fn choose_place(
        &mut self,
        places: Vec<(Site, PlaceValue, InputValue)>,
        otherwise: PlaceValue,
    ) -> Result<Operand, String> {
        let mut chosen = otherwise;
        for (row_site, place, condition) in places.into_iter().rev() {
            let pick = |builder: &mut Builder<'_>, a: &Operand, b: &Operand, value_type: ValueType| {
                let if_true = builder.input_as(&row_site, a, &value_type, false)?;
                let if_false = builder.input_as(&row_site, b, &value_type, false)?;
                Ok::<_, String>(builder.node_value(
                    &row_site,
                    "logic.select",
                    vec![
                        ("cond", condition.clone()),
                        ("if_true", if_true),
                        ("if_false", if_false),
                    ],
                    value_type,
                ))
            };
            let xy = pick(self.builder, &place.xy, &chosen.xy, ValueType::Vec2I)?;
            let amount = match (&place.amount, &chosen.amount) {
                (Some(a), Some(b)) => Some(pick(self.builder, a, b, ValueType::F32)?),
                _ => None,
            };
            chosen = PlaceValue { xy, amount, open: None };
        }
        Ok(Operand::Place(Box::new(chosen)))
    }

    /// Records the Move to line `id`, a change of my Position to the place in its slot.
    pub(super) fn move_to(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let index = self
            .builder
            .properties()
            .iter()
            .position(|property| property.property_type == PropertyType::Position)
            .ok_or_else(|| "the agents have no Position to move".to_owned())?;
        let Operand::Place(place) = self.slot(&site.at("value"), &line.value)? else {
            return Err("Move to takes a place".to_owned());
        };
        let condition = self.change_condition(site, id, line)?;
        self.add_change(index, id, ChangeKind::MoveTo { place: *place }, condition)
    }

    /// Writes my Position after the Move to lines among `changes`, and my Heading when it has a lane, and returns the
    /// new position. `site` is the site of the position's write and `current` its value at the start of the tick.
    ///
    /// A Move to applies only where its place is open, so an agent never enters a blocked cell or leaves the grid.
    pub(super) fn write_moves(
        &mut self,
        site: &Site,
        current: &Operand,
        changes: &[Change<'a>],
    ) -> Result<Operand, String> {
        let cell = self.builder.my_cell(self.pass)?;
        let here = Operand::Link(
            PortRef {
                node: cell,
                port: Some("xy".to_owned()),
            },
            ValueType::Vec2I,
        );
        let mut xy = here.clone();
        let mut moved = Operand::Constant(Literal::Bool(false));
        for change in changes.iter().rev() {
            let ChangeKind::MoveTo { place } = &change.kind else {
                continue;
            };
            let line_site = self.change_site(change.line)?;
            let open = self.place_open(&line_site, place)?;
            let applies = self.builder.join(&line_site, "logic.and", &change.condition, &open)?;
            let condition = self.builder.condition_input(&line_site, &applies)?;
            let if_true = self.builder.input_as(&line_site, &place.xy, &ValueType::Vec2I, false)?;
            let if_false = self.builder.input_as(&line_site, &xy, &ValueType::Vec2I, false)?;
            xy = self.builder.node_value(
                &line_site,
                "logic.select",
                vec![("cond", condition), ("if_true", if_true), ("if_false", if_false)],
                ValueType::Vec2I,
            );
            moved = self.builder.join(&line_site, "logic.or", &applies, &moved)?;
        }
        if moved == Operand::Constant(Literal::Bool(false)) {
            return Ok(current.clone());
        }
        let corner = self.builder.input_as(site, &xy, &ValueType::Vec2F, false)?;
        let center = self.builder.node_value(
            site,
            "math.add",
            vec![
                ("a", corner.clone()),
                ("b", InputValue::Literal(Literal::Vec2F([0.5, 0.5]))),
            ],
            ValueType::Vec2F,
        );
        let position = if moved == Operand::Constant(Literal::Bool(true)) {
            center
        } else {
            let condition = self.builder.condition_input(site, &moved)?;
            let if_true = self.builder.input_as(site, &center, &ValueType::Vec2F, false)?;
            let if_false = self.builder.input_as(site, current, &ValueType::Vec2F, false)?;
            self.builder.node_value(
                site,
                "logic.select",
                vec![("cond", condition), ("if_true", if_true), ("if_false", if_false)],
                ValueType::Vec2F,
            )
        };
        let (_, position_property) = self
            .builder
            .properties()
            .iter()
            .enumerate()
            .find(|(_, property)| property.property_type == PropertyType::Position)
            .ok_or_else(|| "the agents have no Position to move".to_owned())?;
        let written = self.builder.input_as(site, &position, &ValueType::Vec2F, false)?;
        self.builder.write(site, &position_property.id, written)?;
        self.write_step_heading(site, &here, corner)?;
        Ok(position)
    }

    /// Writes my Heading, when it has a lane, as the step from my cell `here` to the cell `corner`, the shorter way
    /// round a world that wraps, keeping the last step when the two are one cell. `site` is the site of the position's
    /// write.
    fn write_step_heading(&mut self, site: &Site, here: &Operand, corner: InputValue) -> Result<(), String> {
        let heading = self.builder.properties().iter().enumerate().find(|(_, property)| {
            property.property_type == PropertyType::Heading && !self.builder.dropped.contains(&property.id)
        });
        if let Some((index, heading)) = heading {
            let heading_site = Site {
                owner: Owner::Sheet(SheetKey {
                    group: GROUP_CHANGE,
                    index: index as u32,
                }),
                path: format!("card.{}.{}", self.card.id, heading.id),
                ..site.clone()
            };
            let world = self.builder.emit(&heading_site, "ctx.world", Vec::new(), Vec::new());
            let from = self.builder.input_as(&heading_site, here, &ValueType::Vec2F, false)?;
            let delta = self.builder.node_value(
                &heading_site,
                "space.delta",
                vec![
                    ("a", from),
                    ("b", corner),
                    (
                        "world",
                        InputValue::Link(PortRef {
                            node: world,
                            port: None,
                        }),
                    ),
                ],
                ValueType::Vec2F,
            );
            let step = self.builder.input_as(&heading_site, &delta, &ValueType::Vec2I, false)?;
            let last = self.builder.read(self.pass, &heading.id)?;
            let last = self.builder.input_as(&heading_site, &last, &ValueType::Vec2I, false)?;
            let stepped = self.builder.node_value(
                &heading_site,
                "cmp.ne",
                vec![("a", step.clone()), ("b", InputValue::Literal(Literal::Vec2I([0, 0])))],
                ValueType::Bool,
            );
            let stepped = self.builder.condition_input(&heading_site, &stepped)?;
            let new_heading = self.builder.node_value(
                &heading_site,
                "logic.select",
                vec![("cond", stepped), ("if_true", step), ("if_false", last)],
                ValueType::Vec2I,
            );
            let new_heading = self
                .builder
                .input_as(&heading_site, &new_heading, &ValueType::Vec2I, false)?;
            self.builder.write(&heading_site, &heading.id, new_heading)?;
        }
        Ok(())
    }

    /// Records the Leave line `id`, a mark of its amount on a property of the ground at my cell.
    pub(super) fn leave(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let property = line
            .ground_property
            .as_deref()
            .ok_or_else(|| "the line names no property of the ground".to_owned())?;
        let (field, layer) = self
            .builder
            .ground_layers
            .get(property)
            .cloned()
            .ok_or_else(|| format!("'{property}' is no property of the ground"))?;
        let amount = self.slot(&site.at("amount"), &line.amount)?;
        let condition = self.change_condition(site, id, line)?;
        self.fires_when(id, &condition);
        self.leaves.push(LeaveMark {
            line: id,
            field,
            layer,
            amount,
            condition,
        });
        Ok(())
    }

    /// Writes the Record line `id`: 1 added to its event's counter where its condition holds.
    pub(super) fn record(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let event = line
            .event
            .as_deref()
            .ok_or_else(|| "the line names no event".to_owned())?;
        let condition = self.change_condition(site, id, line)?;
        self.fires_when(id, &condition);
        let amount = match condition {
            Operand::Constant(Literal::Bool(true)) => InputValue::Literal(Literal::U32(1)),
            condition => {
                let condition = self.builder.condition_input(site, &condition)?;
                let node = self
                    .builder
                    .emit(site, "conv.to_u32", Vec::new(), vec![("a".to_owned(), condition)]);
                InputValue::Link(PortRef { node, port: None })
            }
        };
        self.builder.emit(
            site,
            "counter.add",
            vec![("counter", text(event))],
            vec![("amount".to_owned(), amount)],
        );
        Ok(())
    }

    /// Writes the deposits of the card's Leave lines: one per line, or one per field for Leaves on that field that
    /// cannot both apply.
    pub(super) fn write_leaves(&mut self) -> Result<(), String> {
        let leaves = std::mem::take(&mut self.leaves);
        if leaves.is_empty() {
            return Ok(());
        }
        let cell = self.builder.my_cell(self.pass)?;
        let at = InputValue::Link(PortRef {
            node: cell,
            port: Some("cell".to_owned()),
        });
        let mut fields: Vec<&str> = Vec::new();
        for leave in &leaves {
            if !fields.contains(&leave.field.as_str()) {
                fields.push(&leave.field);
            }
        }
        for (field_index, field) in fields.into_iter().enumerate() {
            let marks: Vec<&LeaveMark<'a>> = leaves.iter().filter(|leave| leave.field == field).collect();
            let exclusive = marks.len() > 1
                && marks.iter().enumerate().all(|(upper, first)| {
                    marks
                        .iter()
                        .skip(upper + 1)
                        .all(|second| !self.relations.can_both_apply(first.line, second.line))
                });
            if exclusive {
                let site = Site {
                    pass: self.pass,
                    owner: Owner::Sheet(SheetKey {
                        group: GROUP_CHANGE,
                        index: 10_000 + field_index as u32,
                    }),
                    path: format!("card.{}.{field}", self.card.id),
                    region: None,
                };
                let mut chosen: Option<(InputValue, InputValue)> = None;
                for mark in marks.iter().rev() {
                    let line_site = self.change_site(mark.line)?;
                    let layer = layer_literal(field, &mark.layer);
                    let amount = self
                        .builder
                        .input_as(&line_site, &mark.amount, &ValueType::F32, false)?;
                    chosen = Some(match chosen {
                        None => (layer, self.where_marked(&line_site, &mark.condition, amount)?),
                        Some((other_layer, other_amount)) => {
                            let condition = self.builder.condition_input(&line_site, &mark.condition)?;
                            let layer = self.builder.node_value(
                                &line_site,
                                "logic.select",
                                vec![
                                    ("cond", condition.clone()),
                                    ("if_true", layer),
                                    ("if_false", other_layer),
                                ],
                                ValueType::Layer(field.to_owned()),
                            );
                            let amount = self.builder.node_value(
                                &line_site,
                                "logic.select",
                                vec![("cond", condition), ("if_true", amount), ("if_false", other_amount)],
                                ValueType::F32,
                            );
                            (
                                self.builder.input_as(
                                    &line_site,
                                    &layer,
                                    &ValueType::Layer(field.to_owned()),
                                    false,
                                )?,
                                self.builder.input_as(&line_site, &amount, &ValueType::F32, false)?,
                            )
                        }
                    });
                }
                if let Some((layer, amount)) = chosen {
                    self.deposit(&site, field, layer, at.clone(), amount);
                }
            } else {
                for mark in marks {
                    let line_site = self.change_site(mark.line)?;
                    let amount = self
                        .builder
                        .input_as(&line_site, &mark.amount, &ValueType::F32, false)?;
                    let amount = self.where_marked(&line_site, &mark.condition, amount)?;
                    self.deposit(&line_site, field, layer_literal(field, &mark.layer), at.clone(), amount);
                }
            }
        }
        Ok(())
    }

    /// Returns `amount` where `condition` holds and 0 elsewhere, written at `site`.
    fn where_marked(&mut self, site: &Site, condition: &Operand, amount: InputValue) -> Result<InputValue, String> {
        if *condition == Operand::Constant(Literal::Bool(true)) {
            return Ok(amount);
        }
        let condition = self.builder.condition_input(site, condition)?;
        let selected = self.builder.node_value(
            site,
            "logic.select",
            vec![
                ("cond", condition),
                ("if_true", amount),
                ("if_false", InputValue::Literal(Literal::F32(0.0))),
            ],
            ValueType::F32,
        );
        self.builder.input_as(site, &selected, &ValueType::F32, false)
    }

    /// Writes a `field.deposit` of `amount` into layer `layer` of field `field` at the cell `at`, at `site`.
    fn deposit(&mut self, site: &Site, field: &str, layer: InputValue, at: InputValue, amount: InputValue) {
        self.builder.emit(
            site,
            "field.deposit",
            vec![("field", text(field))],
            vec![
                ("layer".to_owned(), layer),
                ("at".to_owned(), at),
                ("value".to_owned(), amount),
            ],
        );
    }
}

/// Returns the literal of layer `layer` of field `field`.
fn layer_literal(field: &str, layer: &str) -> InputValue {
    InputValue::Literal(Literal::Layer(MemberName {
        owner: field.to_owned(),
        member: layer.to_owned(),
    }))
}
