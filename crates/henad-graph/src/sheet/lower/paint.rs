//! Starts on the ground: a painted ground, and agents started on its cells.
//!
//! A painted start gives each cell its base state and then the state of each shape that covers the cell's centre, in
//! the shapes' order: a cell shape covers one cell, and an ellipse or a rectangle is centred at a fraction of the
//! world, its sides a fraction of the world's sides, turned by its angle in degrees from the x axis towards smaller
//! rows. A start on the ground places an agent at the centre of one of the cells painted in the state it names, drawn
//! at random when there are several.

use super::calculate::expression_node;
use super::start::StartDraws;
use super::value::{number, output, port, state_literal};
use super::{Builder, Site};
use crate::sheet::document::{PropertyType, ShapeKind, SheetProperty, StartsAs};
use crate::sheet::sentence;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue};

/// Returns a number written into an expression as an `f32`.
pub(super) fn expression_number(number: f64) -> String {
    format!("({:?})", number as f32)
}

/// Returns the column and row of the cell of a pass over cells and the size of the field grid, each an `f32` input,
/// written at `site`.
fn cell_and_grid(builder: &mut Builder<'_>, site: &Site) -> [InputValue; 4] {
    let cell = builder.emit(site, "ctx.cell_xy", Vec::new(), Vec::new());
    let cell = builder.emit(site, "vec.split", Vec::new(), vec![("v".to_owned(), output(cell))]);
    let [x, y] = ["x", "y"].map(|side| builder.node_input(site, "conv.to_f32", vec![("a", port(cell, side))]));
    let [width, height] = grid_size(builder, site);
    [x, y, width, height]
}

/// Returns the width and height of the field grid as `f32` inputs, written at `site`.
fn grid_size(builder: &mut Builder<'_>, site: &Site) -> [InputValue; 2] {
    let grid = builder.emit(site, "ctx.grid", Vec::new(), Vec::new());
    let sides = builder.emit(site, "vec.split", Vec::new(), vec![("v".to_owned(), output(grid))]);
    ["x", "y"].map(|side| builder.node_input(site, "conv.to_f32", vec![("a", port(sides, side))]))
}

/// Writes the `math.expr` node `out = <expression>` over the inputs `inputs`, each an `f32`, and returns its output.
pub(super) fn expression(
    builder: &mut Builder<'_>,
    site: &Site,
    expression: &str,
    inputs: Vec<(&str, InputValue)>,
) -> InputValue {
    let inputs = inputs
        .into_iter()
        .map(|(name, input)| (name.to_owned(), input, ValueType::F32))
        .collect();
    let node = expression_node(builder, site, expression, inputs);
    port(node, "out")
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
    let state = |id: &str| InputValue::Literal(state_literal(&property.id, id));
    let base = start.base.as_deref().ok_or("a painted start needs a base state")?;
    let mut value = state(base);
    if start.shapes.is_empty() {
        return Ok(value);
    }
    let [x, y, width, height] = cell_and_grid(builder, site);
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
        value = builder.select(site, covered, state(&shape.state), value);
    }
    Ok(value)
}

/// Returns the position a start on the ground `start` gives an agent: the centre of a cell painted in its state, one
/// drawn with the draw of `draws` at the path `starts_as.cell` when there are several.
///
/// # Errors
///
/// Returns [`sentence::ON_GROUND_CELLS`] for a state that the ground's base or a shape larger than a cell paints, or
/// that no cell shape paints.
pub(super) fn on_ground(
    builder: &mut Builder<'_>,
    site: &Site,
    start: &StartsAs,
    draws: &StartDraws<'_>,
) -> Result<InputValue, String> {
    let state = start.state.as_deref().unwrap_or("");
    let painted = start
        .property
        .as_deref()
        .and_then(|id| builder.ground_property(id).ok())
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
        centers.push(builder.node_input(site, "vec.make", vec![("x", x), ("y", y)]));
    }
    let mut chosen = centers.pop().ok_or_else(|| sentence::ON_GROUND_CELLS.to_owned())?;
    if centers.is_empty() {
        return Ok(chosen);
    }
    let draw = draws.record(builder, "starts_as.cell");
    let total = (centers.len() + 1) as f32;
    let node = builder.emit(
        site,
        "rand.float",
        vec![("key", ConfigValue::Integer(i64::from(draw)))],
        vec![("max".to_owned(), number(total))],
    );
    let drawn = output(node);
    for (index, center) in centers.into_iter().enumerate().rev() {
        let below = builder.node_input(
            site,
            "cmp.lt",
            vec![("a", drawn.clone()), ("b", number((index + 1) as f32))],
        );
        chosen = builder.select(site, below, center, chosen);
    }
    Ok(chosen)
}
