//! Repeat lines over agents: the lines inside run a number of times within the tick, each time with fresh chances.
//!
//! A Repeat is one `region.repeat`, its count the line's number or Whole number parameter and its `max` that number or
//! the parameter's maximum. The Move by and Steer lines inside add up across the times: each property they change has
//! a `sum` accumulator, and its total is one change of the property, owned by the Repeat line. Every time reads the
//! start of the tick.

use std::collections::BTreeMap;

use super::card::{CardLowering, ChangeKind};
use super::value::Operand;
use super::{Site, text};
use crate::ids::NodeId;
use crate::limits::MAX_REPEAT_ITERATIONS;
use crate::sheet::document::{LineKind, LineMode, PropertyType, SheetChip, SheetLine, SheetValue};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

/// A Repeat line lowered: its region and the accumulator of each property its lines change.
#[derive(Debug, Clone)]
pub(super) struct RepeatBlock {
    /// Temporary id of the `region.repeat`.
    pub node: NodeId,
    /// Name of the accumulator of each property the lines inside change, by the property's index.
    accumulators: BTreeMap<usize, String>,
}

impl<'a> CardLowering<'_, 'a> {
    /// Lowers the Repeat line `id`: its region, and one change for each property the Move by and Steer lines inside it
    /// change.
    pub(super) fn repeat(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let (count, max) = self.repeat_count(line)?;
        let mut changed: BTreeMap<usize, ValueType> = BTreeMap::new();
        let inside: Vec<&'a str> = self
            .relations
            .order()
            .iter()
            .copied()
            .filter(|&inner| self.repeat_of(inner) == Some(id) && !self.builder.report.is_skipped(inner))
            .collect();
        for inner in inside {
            let Some(inner_line) = self.relations.line(inner) else {
                continue;
            };
            let index = match inner_line.kind {
                LineKind::Move if inner_line.mode == Some(LineMode::By) => self
                    .builder
                    .properties()
                    .iter()
                    .position(|property| property.property_type == PropertyType::Position),
                LineKind::Steer => inner_line
                    .property
                    .as_deref()
                    .and_then(|property| self.builder.property(property))
                    .map(|(index, _)| index),
                _ => None,
            };
            if let Some(index) = index {
                changed.insert(index, super::world::lane_type(&self.builder.properties()[index]));
            }
        }
        let mut accumulators = BTreeMap::new();
        let mut config = Vec::with_capacity(changed.len());
        for (&index, value_type) in &changed {
            let name = format!("change_{}", self.builder.properties()[index].id);
            config.push(ConfigValue::Table(BTreeMap::from([
                ("name".to_owned(), text(&name)),
                ("type".to_owned(), text(&value_type.to_string())),
                ("combine".to_owned(), text("sum")),
            ])));
            accumulators.insert(index, name);
        }
        let node = self.builder.emit(
            site,
            "region.repeat",
            vec![
                ("max", ConfigValue::Integer(i64::from(max))),
                ("acc", ConfigValue::Array(config)),
            ],
            vec![
                ("count".to_owned(), count),
                ("when".to_owned(), InputValue::Literal(Literal::Bool(true))),
            ],
        );
        for (index, value_type) in changed {
            let total = Operand::Link(
                PortRef {
                    node,
                    port: Some(accumulators[&index].clone()),
                },
                value_type.clone(),
            );
            let kind = if value_type == ValueType::Vec2F
                && self.builder.properties()[index].property_type == PropertyType::Position
            {
                ChangeKind::MoveBy { arrow: total }
            } else {
                ChangeKind::Steer { term: total }
            };
            self.add_change(index, id, kind, Operand::Constant(Literal::Bool(true)))?;
        }
        self.repeats.insert(id, RepeatBlock { node, accumulators });
        Ok(())
    }

    /// Returns the count input of a Repeat and its most times: a whole number, or a Whole number parameter from 0 and
    /// its maximum.
    fn repeat_count(&self, line: &SheetLine) -> Result<(InputValue, u32), String> {
        let (count, max, parameter) = match line.times.value() {
            Some(SheetValue::Literal(literal)) => {
                let number = literal.number().unwrap_or(-1.0);
                if number.fract() != 0.0 || !(0.0..=f64::from(MAX_REPEAT_ITERATIONS)).contains(&number) {
                    return Err(format!(
                        "a Repeat runs a whole number of times from 0 to {MAX_REPEAT_ITERATIONS}"
                    ));
                }
                // The range check above makes the cast exact.
                let times = number as u32;
                (InputValue::Literal(Literal::U32(times)), times, None)
            }
            Some(SheetValue::Chip(SheetChip::Param(id))) => {
                let parameter = self
                    .builder
                    .document
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == *id)
                    .ok_or_else(|| format!("'{id}' is no parameter"))?;
                if self.builder.param(id)?.value_type() != Some(ValueType::U32) {
                    return Err(format!(
                        "a Repeat runs a Whole number parameter from 0 of times, and '{}' is not one",
                        parameter.name
                    ));
                }
                // The parameter's descriptor holds its maximum as a `u32`, so the cast is exact.
                let max = parameter.max as u32;
                (InputValue::Param(id.clone()), max, Some(parameter.name.as_str()))
            }
            _ => return Err("a Repeat runs a whole number or a parameter of times".to_owned()),
        };
        if max > MAX_REPEAT_ITERATIONS {
            return Err(format!(
                "a Repeat runs at most {MAX_REPEAT_ITERATIONS} times, and the maximum of {} is {max}",
                parameter.unwrap_or("its parameter")
            ));
        }
        Ok((count, max.max(1)))
    }

    /// Adds `value` to the accumulator of the property at `index` in the Repeat line `repeat`, where `condition` holds,
    /// writing the `region.put` at `site`.
    pub(super) fn put_in_repeat(
        &mut self,
        site: &Site,
        repeat: &str,
        index: usize,
        value: &Operand,
        condition: &Operand,
    ) -> Result<(), String> {
        let name = self
            .repeats
            .get(repeat)
            .and_then(|block| block.accumulators.get(&index))
            .cloned()
            .ok_or_else(|| format!("the Repeat line {repeat} did not lower"))?;
        let value_type = super::world::lane_type(&self.builder.properties()[index]);
        let value = self.builder.input_as(site, value, &value_type, false)?;
        let when = self.builder.condition_input(site, condition)?;
        self.builder.emit(
            site,
            "region.put",
            vec![("acc", text(&name))],
            vec![("value".to_owned(), value), ("when".to_owned(), when)],
        );
        Ok(())
    }
}
