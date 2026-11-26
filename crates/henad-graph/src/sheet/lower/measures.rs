//! Measures of the cells or agents: the stats pass `measures`, and a `[[stat]]` per measure labelled with the
//! measure's name, its column in results.
//!
//! A count in a state reads one output of the `stat.count_by` of its property. The measures of one property share that
//! node.
//! An average is a `stat.mean`, a total a `stat.sum`, and a spread the square root of the mean square less the
//! squared mean, in `f64` after the reduction.

use super::ids::{GROUP_MEASURE, GROUP_PART};
use super::value::{Operand, literal_operand, port};
use super::{Builder, Site, text};
use crate::document::{Pass, PassStage, Stat};
use crate::ids::NodeId;
use crate::sheet::check::Place;
use crate::sheet::document::{LineKind, MeasureRule, SheetChip, SheetMeasure, SheetValue};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

/// Id of the stats pass of the measures.
const MEASURE_PASS: &str = "measures";

/// Writes the stats pass and a `[[stat]]` for every measure.
///
/// A sheet of agents on a ground with properties lowers the measures of the agents and its events here, and the
/// measures of the ground in a second pass `measures_ground` over the ground's cells. The `[[stat]]` tables keep the
/// measures' order.
pub(super) fn lower(builder: &mut Builder<'_>) {
    let measures = &builder.document.measures;
    let ground = builder.is_ground();
    let has_ground = builder.is_agents() && !builder.ground_fields.is_empty();
    let mine: Vec<usize> = (0..measures.len())
        .filter(|&index| {
            let over_ground =
                measures[index].over.as_deref() == Some("ground") && measures[index].rule != MeasureRule::EventCount;
            if ground {
                over_ground
            } else {
                !(has_ground && over_ground)
            }
        })
        .collect();
    if mine.is_empty() {
        return;
    }
    let id = if ground {
        format!("{MEASURE_PASS}_ground")
    } else {
        MEASURE_PASS.to_owned()
    };
    let pass = builder.add_pass(Pass::new(&id, PassStage::Stats, Some(builder.domain())));
    let mut counts: Vec<(String, NodeId)> = Vec::new();
    for index in mine {
        let measure = &measures[index];
        match measure_source(builder, pass, index, measure, &mut counts) {
            Ok(from) => {
                let place = builder.stat_measures.iter().filter(|&&known| known < index).count();
                builder.stat_measures.insert(place, index);
                builder
                    .graph
                    .stats
                    .insert(place, Stat::new(&measure.name, from, measure.color));
            }
            Err(message) => builder.problem(format!("measure '{}': {message}", measure.name), Place::Measure(index)),
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
    let thing = builder.thing_id();
    if measure.over.as_deref() != Some(thing) && measure.rule != MeasureRule::EventCount {
        return Err(format!(
            "a measure of this sheet must be over {}",
            builder.thing_words()
        ));
    }
    let site = Site::sheet(pass, GROUP_MEASURE, index as u32, format!("measure.{}", measure.id));
    match measure.rule {
        MeasureRule::CountInState => {
            let property = measure
                .property
                .as_deref()
                .ok_or("a count in a state needs a property")?;
            let state = measure.state.as_deref().ok_or("a count in a state needs a state")?;
            let (property_index, _) = builder.required_property(property)?;
            let node = if let Some((_, node)) = counts.iter().find(|(id, _)| id == property) {
                *node
            } else {
                let read = builder.read(pass, property)?;
                let count_site = Site::sheet(
                    pass,
                    GROUP_PART,
                    property_index as u32,
                    format!("{}.{property}.count", builder.property_path()),
                );
                let value = builder.input_as(&count_site, &read, &ValueType::Category(property.to_owned()))?;
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
            let read = measure_value(builder, &site, pass, measure)?;
            let value_type = if read.value_type() == Some(ValueType::Vec2F) {
                ValueType::Vec2F
            } else {
                ValueType::F32
            };
            let value = builder.input_as(&site, &read, &value_type)?;
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
            let read = measure_value(builder, &site, pass, measure)?;
            spread(builder, &site, &read)
        }
        MeasureRule::EventCount if builder.is_agents() => {
            let event = measure.event.as_deref().ok_or("an event count needs an event")?;
            let node = builder.emit(&site, "stat.counter", vec![("counter", text(event))], Vec::new());
            Ok(PortRef {
                node,
                port: Some("total".to_owned()),
            })
        }
        MeasureRule::EventCount => Err("a grid records no events to count".to_owned()),
    }
}

/// Returns the output of the spread of `read` over the cells, written at `site`: the square root of the mean square
/// less the squared mean, at least 0.
fn spread(builder: &mut Builder<'_>, site: &Site, read: &Operand) -> Result<PortRef, String> {
    let value = builder.input_as(site, read, &ValueType::F32)?;
    let mean = mean_of(builder, site, value.clone());
    let square = builder.node_input(site, "math.mul", vec![("a", value.clone()), ("b", value)]);
    let mean_square = mean_of(builder, site, square);
    let squared_mean = builder.node_input(site, "math.mul", vec![("a", mean.clone()), ("b", mean)]);
    let variance = builder.node_input(site, "math.sub", vec![("a", mean_square), ("b", squared_mean)]);
    let clamped = builder.node_input(
        site,
        "math.max",
        vec![("a", variance), ("b", InputValue::Literal(Literal::F64(0.0)))],
    );
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
    port(node, "mean")
}

/// Returns the value measure `measure` reads for each cell or agent, written at `site`: one of my properties, a
/// parameter, a number, or a Calculate over these.
fn measure_value(
    builder: &mut Builder<'_>,
    site: &Site,
    pass: usize,
    measure: &SheetMeasure,
) -> Result<Operand, String> {
    match measure.value.value() {
        Some(SheetValue::Chip(SheetChip::My(property))) => builder.read(pass, property),
        Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::Calculate => {
            let mut scope = super::calculate::PassScope { builder, pass };
            super::calculate::calculate(&mut scope, &site.at("value"), phrase)
        }
        Some(SheetValue::Chip(SheetChip::Param(id))) => builder.param(id),
        Some(SheetValue::Literal(literal)) => {
            literal_operand(literal).ok_or_else(|| "the measure reads no number".to_owned())
        }
        _ => Err("a measure can only read a property, a parameter, a number or a Calculate of them".to_owned()),
    }
}
