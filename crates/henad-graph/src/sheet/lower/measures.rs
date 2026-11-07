//! Measures of the cells: the stats pass `measures`, and a `[[stat]]` per measure labelled with the measure's name, its
//! column in results.
//!
//! A count in a state reads one output of the `stat.count_by` of its property. The measures of one property share that
//! node.
//! An average is a `stat.mean`, a total a `stat.sum`, and a spread the square root of the mean square less the
//! squared mean, in `f64` after the reduction.

use super::ids::{GROUP_MEASURE, GROUP_PART, Owner, SheetKey};
use super::value::Operand;
use super::{Builder, Site, text};
use crate::document::{Domain, Pass, PassStage, Stat};
use crate::ids::NodeId;
use crate::sheet::check::Place;
use crate::sheet::document::{MeasureRule, SheetChip, SheetLiteral, SheetMeasure, SheetValue};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

/// Id of the stats pass of the measures.
const MEASURE_PASS: &str = "measures";

/// Writes the stats pass and a `[[stat]]` for every measure.
pub(super) fn lower(builder: &mut Builder<'_>) {
    let measures = &builder.document.measures;
    if measures.is_empty() {
        return;
    }
    let pass = builder.add_pass(Pass::new(MEASURE_PASS, PassStage::Stats, Some(Domain::Cells)));
    let mut counts: Vec<(String, NodeId)> = Vec::new();
    for (index, measure) in measures.iter().enumerate() {
        match measure_source(builder, pass, index, measure, &mut counts) {
            Ok(from) => builder.graph.stats.push(Stat::new(&measure.name, from, measure.color)),
            Err(message) => builder.problem(format!("measure '{}': {message}", measure.id), Place::Measure(index)),
        }
    }
}

/// Writes the nodes of measure `measure` at `index` in pass `pass`, and returns the output its `[[stat]]` reads.
///
/// `counts` holds the `stat.count_by` of each property written so far.
fn measure_source(
    builder: &mut Builder<'_>,
    pass: usize,
    index: usize,
    measure: &SheetMeasure,
    counts: &mut Vec<(String, NodeId)>,
) -> Result<PortRef, String> {
    if measure.over.as_deref() != Some("cells") && measure.rule != MeasureRule::EventCount {
        return Err("a measure of a grid is over the cells".to_owned());
    }
    let site = Site {
        pass,
        owner: Owner::Sheet(SheetKey {
            group: GROUP_MEASURE,
            index: index as u32,
        }),
        path: format!("measure.{}", measure.id),
        region: None,
    };
    match measure.rule {
        MeasureRule::CountInState => {
            let property = measure
                .property
                .as_deref()
                .ok_or("a count in a state names no property")?;
            let state = measure.state.as_deref().ok_or("a count in a state names no state")?;
            let (property_index, _) = builder
                .property(property)
                .ok_or_else(|| format!("'{property}' is no property of the cells"))?;
            let node = if let Some((_, node)) = counts.iter().find(|(id, _)| id == property) {
                *node
            } else {
                let read = builder.read(pass, property)?;
                let count_site = Site {
                    owner: Owner::Sheet(SheetKey {
                        group: GROUP_PART,
                        index: property_index as u32,
                    }),
                    path: format!("cells.{property}.count"),
                    ..site.clone()
                };
                let value = builder.input_as(&count_site, &read, &ValueType::Category(property.to_owned()), false)?;
                let node = builder.emit(
                    &count_site,
                    "stat.count_by",
                    vec![("category", text(property))],
                    vec![("value".to_owned(), value)],
                );
                counts.push((property.to_owned(), node));
                node
            };
            Ok(PortRef {
                node,
                port: Some(state.to_owned()),
            })
        }
        MeasureRule::Average | MeasureRule::Total => {
            let read = measure_value(builder, pass, measure)?;
            let value = builder.input_as(&site, &read, &ValueType::F32, false)?;
            let (op, port) = if measure.rule == MeasureRule::Average {
                ("stat.mean", "mean")
            } else {
                ("stat.sum", "sum")
            };
            let node = builder.emit(
                &site,
                op,
                vec![("chunk_acc", text("f64"))],
                vec![("value".to_owned(), value)],
            );
            Ok(PortRef {
                node,
                port: Some(port.to_owned()),
            })
        }
        MeasureRule::Spread => {
            let read = measure_value(builder, pass, measure)?;
            spread(builder, &site, &read)
        }
        MeasureRule::EventCount => Err("a grid records no events to count".to_owned()),
    }
}

/// Returns the output of the spread of `read` over the cells, written at `site`: the square root of the mean square
/// less the squared mean, at least 0.
fn spread(builder: &mut Builder<'_>, site: &Site, read: &Operand) -> Result<PortRef, String> {
    let value = builder.input_as(site, read, &ValueType::F32, false)?;
    let mean = mean_of(builder, site, value.clone());
    let square = builder.node_value(
        site,
        "math.mul",
        vec![("a", value.clone()), ("b", value)],
        ValueType::F32,
    );
    let square = builder.input_as(site, &square, &ValueType::F32, false)?;
    let mean_square = mean_of(builder, site, square);
    let squared_mean = builder.node_value(site, "math.mul", vec![("a", mean.clone()), ("b", mean)], ValueType::F64);
    let squared_mean = builder.input_as(site, &squared_mean, &ValueType::F64, false)?;
    let variance = builder.node_value(
        site,
        "math.sub",
        vec![("a", mean_square), ("b", squared_mean)],
        ValueType::F64,
    );
    let variance = builder.input_as(site, &variance, &ValueType::F64, false)?;
    let clamped = builder.node_value(
        site,
        "math.max",
        vec![("a", variance), ("b", InputValue::Literal(Literal::F64(0.0)))],
        ValueType::F64,
    );
    let clamped = builder.input_as(site, &clamped, &ValueType::F64, false)?;
    let node = builder.emit(site, "math.sqrt", Vec::new(), vec![("a".to_owned(), clamped)]);
    Ok(PortRef { node, port: None })
}

/// Returns the `mean` output of a `stat.mean` of `value` written at `site`.
fn mean_of(builder: &mut Builder<'_>, site: &Site, value: InputValue) -> InputValue {
    let node = builder.emit(
        site,
        "stat.mean",
        vec![("chunk_acc", ConfigValue::String("f64".to_owned()))],
        vec![("value".to_owned(), value)],
    );
    InputValue::Link(PortRef {
        node,
        port: Some("mean".to_owned()),
    })
}

/// Returns the value measure `measure` reads for each cell: one of my properties, a parameter or a number.
fn measure_value(builder: &mut Builder<'_>, pass: usize, measure: &SheetMeasure) -> Result<Operand, String> {
    match measure.value.value() {
        Some(SheetValue::Chip(SheetChip::My(property))) => builder.read(pass, property),
        Some(SheetValue::Chip(SheetChip::Param(id))) => builder.param(id),
        Some(SheetValue::Literal(SheetLiteral::Bool(value))) => Ok(Operand::Constant(Literal::Bool(*value))),
        Some(SheetValue::Literal(literal)) => literal
            .number()
            .map(Operand::Number)
            .ok_or_else(|| "the measure reads no number".to_owned()),
        _ => Err("a measure of a grid reads one of the cells' properties".to_owned()),
    }
}
