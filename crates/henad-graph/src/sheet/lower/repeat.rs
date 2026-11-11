//! Repeat lines over agents: the lines inside run a number of times within the tick, each time with fresh chances.
//!
//! A Repeat is one `region.repeat`, its count the line's number or Whole number parameter and its `max` that number or
//! the parameter's maximum. Each pass through the lines inside is a *time round*, counted from 0 by the region's `k`.
//! Every time round reads the start of the tick.
//!
//! The Move by and Steer lines inside add up across the time rounds: each property they change has a `sum`
//! accumulator, and its total is one change of the property, owned by the Repeat line.
//!
//! A Become or Set inside applies when its condition holds at any time round, and a Set's value is worked out once,
//! outside the Repeat. Two Become or two Set lines of one property inside one Repeat that can both apply *compete*.
//! Among competing lines, the line whose condition first held at the earliest time round wins, and the higher line
//! wins when two first held at the same time round. Each competing line keeps that time round in a `u32` `min`
//! accumulator `<line>_first`, which stays at the largest `u32` when the line never held. A line applies when its
//! first time round is below the largest `u32` and no lower competing line first held at an earlier time round, and
//! the rows of the change, in display order, then pick the winner. A Become from a state that competes with a higher
//! Become from any state keeps its time round only for an element in that state, since it cannot change any other
//! element. A change with no competitor reads an `or` accumulator `<line>_applies` alone. Each change inside takes
//! one accumulator either way.

use std::collections::BTreeMap;

use super::card::{CardLowering, ChangeKind};
use super::look::accumulator;
use super::value::{Operand, port, port_of};
use super::{Site, visit_value_chips};
use crate::ids::NodeId;
use crate::limits::MAX_REPEAT_ITERATIONS;
use crate::sheet::document::{LineKind, LineMode, PropertyType, SheetChip, SheetLine, SheetValue, Slot};
use crate::sheet::sentence::ANY_STATE_ID;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal};

/// A Repeat line lowered: its region, the accumulator of each property its lines change and its competing changes.
#[derive(Debug, Clone)]
pub(super) struct RepeatBlock {
    /// Temporary id of the `region.repeat`.
    pub node: NodeId,
    /// Name of the accumulator of each property the lines inside change, by the property's index.
    accumulators: BTreeMap<usize, String>,
    /// Each Become or Set line inside that competes with another change of its property inside, by line id.
    contests: BTreeMap<String, Contest>,
}

/// A Become or Set line inside a Repeat that competes with another change of its property inside the same Repeat.
#[derive(Debug, Clone, Default)]
struct Contest {
    /// Competing lines below the line, in display order.
    lower: Vec<String>,
    /// State the line changes from, when a higher competing line changes from any state. The line's first time round
    /// is then kept only for an element in that state.
    gate: Option<String>,
}

/// Returns the name of the `or` accumulator of the Become or Set line `line` inside a Repeat.
fn applies_name(line: &str) -> String {
    format!("{line}_applies")
}

/// Returns the name of the `min` accumulator of the first time round at which the competing line `line` held.
fn first_round_name(line: &str) -> String {
    format!("{line}_first")
}

impl<'a> CardLowering<'_, 'a> {
    /// Lowers the Repeat line `id`: its region, and one change for each property the Move by and Steer lines inside it
    /// change.
    pub(super) fn repeat(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let (count, max) = self.repeat_count(line)?;
        let mut changed: BTreeMap<usize, ValueType> = BTreeMap::new();
        let mut changes: Vec<&'a str> = Vec::new();
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
            if matches!(inner_line.kind, LineKind::Become | LineKind::Set) {
                changes.push(inner);
            }
            let index = match inner_line.kind {
                LineKind::Move if inner_line.mode == Some(LineMode::By) => {
                    self.builder.position().map(|(index, _)| index)
                }
                LineKind::Steer => inner_line
                    .property
                    .as_deref()
                    .and_then(|property| self.builder.property(property))
                    .map(|(index, _)| index),
                _ => None,
            };
            if let Some(index) = index {
                changed.insert(index, self.builder.lane_type(&self.builder.properties()[index]));
            }
        }
        let mut accumulators = BTreeMap::new();
        let mut config = Vec::with_capacity(changed.len());
        for (&index, value_type) in &changed {
            let name = format!("change_{}", self.builder.properties()[index].id);
            config.push(accumulator(&name, &value_type.to_string(), "sum"));
            accumulators.insert(index, name);
        }
        let contests = self.contests(&changes);
        changes.sort_by_key(|line| super::ids::line_number(line));
        for line in &changes {
            let (name, value_type, combine) = if contests.contains_key(*line) {
                (first_round_name(line), "u32", "min")
            } else {
                (applies_name(line), "bool", "or")
            };
            config.push(accumulator(&name, value_type, combine));
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
            let total = port_of(node, &accumulators[&index], value_type.clone());
            let kind = if value_type == ValueType::Vec2F
                && self.builder.properties()[index].property_type == PropertyType::Position
            {
                ChangeKind::MoveBy { arrow: total }
            } else {
                ChangeKind::Steer { term: total }
            };
            self.add_change(index, id, kind, Operand::Constant(Literal::Bool(true)))?;
        }
        self.repeats.insert(
            id,
            RepeatBlock {
                node,
                accumulators,
                contests,
            },
        );
        Ok(())
    }

    /// Returns the competing lines among `changes`, the Become and Set lines inside one Repeat in display order.
    ///
    /// Two lines compete when both are Become or both are Set lines of one property and the relations say they can
    /// both apply.
    fn contests(&self, changes: &[&'a str]) -> BTreeMap<String, Contest> {
        let mut contests: BTreeMap<String, Contest> = BTreeMap::new();
        for (index, &upper) in changes.iter().enumerate() {
            let Some(upper_line) = self.relations.line(upper) else {
                continue;
            };
            for &lower in &changes[index + 1..] {
                let Some(lower_line) = self.relations.line(lower) else {
                    continue;
                };
                if lower_line.kind != upper_line.kind
                    || lower_line.property != upper_line.property
                    || !self.relations.can_both_apply(upper, lower)
                {
                    continue;
                }
                contests
                    .entry(upper.to_owned())
                    .or_default()
                    .lower
                    .push(lower.to_owned());
                let contest = contests.entry(lower.to_owned()).or_default();
                if upper_line.kind == LineKind::Become
                    && upper_line.start_state.as_deref() == Some(ANY_STATE_ID)
                    && let Some(state) = lower_line.start_state.as_deref().filter(|state| *state != ANY_STATE_ID)
                {
                    contest.gate = Some(state.to_owned());
                }
            }
        }
        contests
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

    /// Returns the condition of the Become or Set line `id` across the time rounds of the Repeat that holds it, from
    /// `condition`, written inside the Repeat at `site`. Returns `condition` for a line outside a Repeat.
    ///
    /// A line with no competitor applies when `condition` holds at any time round. A competing line applies when its
    /// first time round is below the largest `u32` and no lower competing line first held at an earlier time round.
    pub(super) fn any_time(&mut self, site: &Site, id: &str, condition: Operand) -> Result<Operand, String> {
        let Some(repeat) = self.repeat_of(id) else {
            return Ok(condition);
        };
        let (node, contest) = self
            .repeats
            .get(repeat)
            .map(|block| (block.node, block.contests.get(id).cloned()))
            .ok_or_else(|| format!("the Repeat line {repeat} did not lower"))?;
        let Some(contest) = contest else {
            let name = applies_name(id);
            let value = self.builder.condition_input(site, &condition)?;
            self.builder
                .put(site, &name, Some(value), InputValue::Literal(Literal::Bool(true)));
            return Ok(port_of(node, &name, ValueType::Bool));
        };
        let mut kept = condition;
        if let Some(state) = &contest.gate {
            let in_state = self.in_state(site, id, state)?;
            kept = self.builder.join(site, "logic.and", &kept, &in_state)?;
        }
        let when = self.builder.condition_input(site, &kept)?;
        let round = port(node, "k");
        self.builder.put(site, &first_round_name(id), Some(round), when);
        let own = port(node, &first_round_name(id));
        let change_site = self.change_site(id)?;
        // The line held where its first time round is below the largest `u32`, the start of a `min` accumulator.
        let mut decided = self.builder.node_value(
            &change_site,
            "cmp.lt",
            vec![("a", own.clone()), ("b", InputValue::Literal(Literal::U32(u32::MAX)))],
            ValueType::Bool,
        );
        for lower in &contest.lower {
            let not_earlier = self.builder.node_value(
                &change_site,
                "cmp.ge",
                vec![("a", port(node, &first_round_name(lower))), ("b", own.clone())],
                ValueType::Bool,
            );
            decided = self.builder.join(&change_site, "logic.and", &decided, &not_earlier)?;
        }
        Ok(decided)
    }

    /// Returns whether the States property that the Become line `id` changes holds `state` at the start of the tick,
    /// compared outside the Repeat that holds the line at `site`.
    fn in_state(&mut self, site: &Site, id: &str, state: &str) -> Result<Operand, String> {
        let property = self
            .relations
            .line(id)
            .and_then(|line| line.property.as_deref())
            .ok_or_else(|| "the line names no property".to_owned())?;
        let current = self.builder.read(self.pass, property)?;
        let outside = Site {
            region: None,
            ..site.clone()
        };
        let category = ValueType::Category(property.to_owned());
        let a = self.builder.input_as(&outside, &current, &category)?;
        let b = self
            .builder
            .input_as(&outside, &Operand::State(state.to_owned()), &category)?;
        Ok(self
            .builder
            .node_value(&outside, "cmp.eq", vec![("a", a), ("b", b)], ValueType::Bool))
    }

    /// Returns whether `slot` reads, at any depth, the result of a line inside the Repeat line `repeat`.
    pub(super) fn reads_inside(&self, slot: &Slot, repeat: &str) -> bool {
        let mut reads = false;
        if let Some(value) = slot.value() {
            visit_value_chips(self.builder.document, value, &mut |chip| {
                if let SheetChip::Result { line, .. } = chip {
                    reads |= self.repeat_of(line) == Some(repeat);
                }
            });
        }
        reads
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
        let value_type = self.builder.lane_type(&self.builder.properties()[index]);
        let value = self.builder.input_as(site, value, &value_type)?;
        let when = self.builder.condition_input(site, condition)?;
        self.builder.put(site, &name, Some(value), when);
        Ok(())
    }
}
