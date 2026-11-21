//! Lines that look at the cells around: Count and Look around.
//!
//! A Count is a `grid.count` per counted state when it counts neighbours in some states of a States property, and a
//! `count` accumulator of a fold over the cells around otherwise. A Look around is a `count` accumulator and an
//! accumulator per summary: a total, an average or a largest or smallest value of a number, and whether any or every
//! neighbour holds yes. The looks of a card over one neighbourhood share a fold, as [`super::fuse`] describes. An
//! average, a largest and a smallest value of no neighbour are 0. Inside a fold, a `their` chip reads the neighbour's
//! property through `lane.get_at` at the fold's `cell`, or on a card of agents the ground's property there through
//! `field.get`, and the `only` test decides which neighbours count.

use std::collections::{BTreeMap, HashMap};

use super::card::CardLowering;
use super::text;
use super::value::{Operand, port, port_of, state_literal};
use super::{Builder, Site};
use crate::ids::NodeId;
use crate::sheet::document::{
    Around, CompareTest, LineKind, LookSummary, PropertyType, SheetChip, SheetLine, SheetValue, Slot, SummaryKind,
};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

/// A `grid.fold` over the cells around, or an `agent.radius_fold` over the agents within a distance, that `their`
/// chips read inside.
#[derive(Debug)]
pub(super) struct Fold {
    /// Temporary id of the fold.
    pub(super) node: NodeId,
    /// Read of each property of the neighbour, by property id.
    pub(super) reads: HashMap<String, Operand>,
    /// Things the fold walks over.
    pub(super) kind: FoldKind,
    /// Offset to the agent the fold visits and its squared length, without wrapping at the edges, in a world of agents
    /// with walls. The fold's own `delta` and `dist_sq` wrap.
    pub(super) walled: Option<(Operand, Operand)>,
}

/// Things a fold walks over. They decide how `their` reads the neighbour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FoldKind {
    /// The cells of a grid, whose inner output `cell` names the neighbour's lanes.
    Cells,
    /// The ground around an agent, whose inner output `cell` names the neighbour's fields.
    Ground,
    /// The agents within a distance, whose inner output `other` names the neighbour.
    Agents,
}

/// Returns whether `around` is the 8 cells or the 3 by 3 block, and whether it includes the cell itself.
///
/// # Errors
///
/// Returns the message for agents within a distance, which are no cells.
pub(super) fn stencil(around: Option<Around>) -> Result<(bool, bool), String> {
    match around {
        Some(Around::Cells8) => Ok((true, false)),
        Some(Around::Cells4) => Ok((false, false)),
        Some(Around::Block3x3) => Ok((true, true)),
        Some(Around::AgentsWithin) => Err("agents within a distance are not cells".to_owned()),
        None => Err("the line needs cells to look at".to_owned()),
    }
}

/// Returns the number of cells that `around` looks at, `None` for agents within a distance.
pub(super) fn neighbor_count(around: Around) -> Option<u32> {
    match around {
        Around::Cells8 => Some(8),
        Around::Cells4 => Some(4),
        Around::Block3x3 => Some(9),
        Around::AgentsWithin => None,
    }
}

/// Returns the accumulator `name` of `value_type` that combines by `combine`, as a fold's configuration holds it.
pub(super) fn accumulator(name: &str, value_type: &str, combine: &str) -> ConfigValue {
    ConfigValue::Table(BTreeMap::from([
        ("name".to_owned(), text(name)),
        ("type".to_owned(), text(value_type)),
        ("combine".to_owned(), text(combine)),
    ]))
}

impl<'a> CardLowering<'_, 'a> {
    /// Returns the read of the neighbour's `property` in the innermost fold.
    pub(super) fn their(&mut self, site: &Site, property: &str) -> Result<Operand, String> {
        let fold = self
            .folds
            .last()
            .ok_or_else(|| "their is used outside a look around".to_owned())?;
        if let Some(read) = fold.reads.get(property) {
            return Ok(read.clone());
        }
        let fold_node = fold.node;
        let kind = fold.kind;
        let neighbour = port(fold_node, if kind == FoldKind::Agents { "other" } else { "cell" });
        let read = if kind == FoldKind::Ground {
            self.ground_at(&site.inside(fold_node), property, neighbour)?
        } else {
            let (_, found) = self.property(Some(property))?;
            let value_type = self.builder.lane_type(found);
            let node = self.builder.emit(
                &site.inside(fold_node),
                "lane.get_at",
                vec![("lane", text(property))],
                vec![("at".to_owned(), neighbour)],
            );
            Operand::Link(PortRef { node, port: None }, value_type)
        };
        if let Some(fold) = self.folds.last_mut() {
            fold.reads.insert(property.to_owned(), read.clone());
        }
        Ok(read)
    }

    /// Returns `test` joined with the test of the `only` slot `only`, lowered at `only_site` inside a fold, when it is
    /// filled.
    pub(super) fn with_only(&mut self, only_site: &Site, test: Operand, only: &'a Slot) -> Result<Operand, String> {
        match only.value() {
            None => Ok(test),
            Some(value) => {
                let operand = self.value(only_site, value)?;
                self.builder.join(only_site, "logic.and", &test, &operand)
            }
        }
    }

    /// Returns the count of a Count line or phrase over the cells around.
    pub(super) fn count(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let (moore, include_center) = stencil(line.around)?;
        let whose = line.whose.value().ok_or_else(|| "a Count needs a test".to_owned())?;
        let only_site = site.at("only");
        let whose_site = site.at("whose");
        if line.only.is_blank()
            && let Some((property, states)) = self.counted_states(whose)
        {
            let mut total: Option<Operand> = None;
            for state in states {
                let node = self.builder.emit(
                    &whose_site,
                    "grid.count",
                    vec![
                        ("lane", text(&property)),
                        ("neighborhood", text(if moore { "moore" } else { "von_neumann" })),
                        ("include_center", ConfigValue::Bool(include_center)),
                    ],
                    vec![
                        (
                            "value".to_owned(),
                            InputValue::Literal(state_literal(&property, &state)),
                        ),
                        ("when".to_owned(), InputValue::Literal(Literal::Bool(true))),
                    ],
                );
                let count = Operand::Link(PortRef { node, port: None }, ValueType::U32);
                total = Some(match total {
                    None => count,
                    Some(sum) => {
                        let a = self.builder.input_as(&whose_site, &sum, &ValueType::U32)?;
                        let b = self.builder.input_as(&whose_site, &count, &ValueType::U32)?;
                        self.builder
                            .node_value(&whose_site, "math.add", vec![("a", a), ("b", b)], ValueType::U32)
                    }
                });
            }
            return Ok(total.unwrap_or(Operand::Constant(Literal::U32(0))));
        }
        let look = self.enter_cells(site, line, 1)?;
        let name = format!("{}_count", look.prefix);
        let inside = whose_site.inside(look.node);
        let test = self.value(&inside, whose).and_then(|test| {
            let test = self.with_only(&only_site.inside(look.node), test, &line.only)?;
            let test = self.builder.join(&inside, "logic.and", &look.within, &test)?;
            let when = self.builder.condition_input(&inside, &test)?;
            self.builder.put(&inside, &name, None, when);
            Ok(())
        });
        self.folds.pop();
        test?;
        self.add_cell_accumulator(&look, 0, accumulator(&name, "u32", "count"));
        Ok(port_of(look.node, &name, ValueType::U32))
    }

    /// Returns the States property and the states a Count's test `whose` counts, when it tests their States property
    /// against states alone, `None` for any other test.
    pub(super) fn counted_states(&self, whose: &SheetValue) -> Option<(String, Vec<String>)> {
        let SheetValue::Phrase(phrase) = whose else {
            return None;
        };
        if phrase.kind != LineKind::Compare {
            return None;
        }
        let Some(SheetValue::Chip(SheetChip::Their(property))) = phrase.left.value() else {
            return None;
        };
        let (_, found) = self.builder.property(property)?;
        if found.property_type != PropertyType::States {
            return None;
        }
        let state_of = |value: &SheetValue| match value {
            SheetValue::Chip(SheetChip::State(state)) => Some(state.clone()),
            _ => None,
        };
        let listed: Vec<String> = match phrase.right.value()? {
            SheetValue::List(items) => items.iter().map(state_of).collect::<Option<_>>()?,
            value => vec![state_of(value)?],
        };
        let states = match phrase.test? {
            CompareTest::Is | CompareTest::OneOf => listed,
            CompareTest::IsNot | CompareTest::NotOneOf => found
                .states
                .iter()
                .map(|state| state.id.clone())
                .filter(|state| !listed.contains(state))
                .collect(),
            _ => return None,
        };
        Some((property.clone(), states))
    }

    /// Lowers the Look around line `id` over the cells around, and records each part of its result: `count` and every
    /// summary by its id.
    pub(super) fn look_around(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let mut accumulators = Vec::new();
        for summary in &line.summaries {
            let kind = summary
                .summary
                .ok_or_else(|| format!("summary '{}' does not say how to summarize", summary.id))?;
            if kind != SummaryKind::Count {
                let (value_type, combine) = summary_accumulator_type(kind, false);
                accumulators.push((summary.id.as_str(), value_type, combine));
            }
        }
        let look = self.enter_cells(site, line, accumulators.len() + 1)?;
        self.builder.record_result(look.node, id);
        let inside = site.inside(look.node);
        let body = self.look_body(&inside, line, &look);
        self.folds.pop();
        body?;
        let count_name = format!("{}_count", look.prefix);
        self.add_cell_accumulator(&look, 0, accumulator(&count_name, "u32", "count"));
        for (order, (summary, value_type, combine)) in accumulators.iter().enumerate() {
            let name = format!("{}_{}", look.prefix, summary_accumulator(summary));
            self.add_cell_accumulator(&look, order + 1, accumulator(&name, &value_type.to_string(), combine));
        }
        let types = vec![ValueType::F32; line.summaries.len()];
        self.summary_parts(site, id, look.node, &look.prefix, &line.summaries, &types)
    }

    /// Records each part of the result of the Look around line `id`, from the fold `node` whose accumulators are named
    /// from `prefix`: `count`, and each of `summaries` by its id, written at `site`.
    ///
    /// `types` holds the type of the accumulator of each summary. Only a total or an average reads it.
    pub(super) fn summary_parts(
        &mut self,
        site: &Site,
        id: &'a str,
        node: NodeId,
        prefix: &str,
        summaries: &'a [LookSummary],
        types: &[ValueType],
    ) -> Result<(), String> {
        let count = port_of(node, &format!("{prefix}_count"), ValueType::U32);
        self.parts.insert((id, "count"), count.clone());
        for (summary, value_type) in summaries.iter().zip(types) {
            let name = format!("{prefix}_{}", summary_accumulator(&summary.id));
            let part = match summary.summary {
                Some(SummaryKind::Count) => count.clone(),
                Some(SummaryKind::Total) => port_of(node, &name, value_type.clone()),
                Some(SummaryKind::Average) => {
                    let total = port_of(node, &name, value_type.clone());
                    let total = self.builder.input_as(site, &total, value_type)?;
                    let number = self.builder.input_as(site, &count, &ValueType::F32)?;
                    let average = if *value_type == ValueType::Vec2F {
                        let share = self.builder.node_input(site, "math.recip", vec![("a", number)]);
                        self.builder
                            .node_value(site, "vec.scale", vec![("v", total), ("s", share)], ValueType::Vec2F)
                    } else {
                        self.builder
                            .node_value(site, "math.div", vec![("a", total), ("b", number)], ValueType::F32)
                    };
                    self.empty.insert((id, summary.id.as_str()), count.clone());
                    guard_by_count(self.builder, site, &count, &average, value_type)?
                }
                Some(SummaryKind::Largest | SummaryKind::Smallest) => {
                    let extreme = port_of(node, &name, ValueType::F32);
                    self.empty.insert((id, summary.id.as_str()), count.clone());
                    guard_by_count(self.builder, site, &count, &extreme, &ValueType::F32)?
                }
                Some(SummaryKind::Any | SummaryKind::All) => port_of(node, &name, ValueType::Bool),
                None => continue,
            };
            self.parts.insert((id, summary.id.as_str()), part);
        }
        Ok(())
    }

    /// Writes the body of a Look around's fold at `inside`: the `only` test, the count and each summary's value.
    fn look_body(&mut self, inside: &Site, line: &'a SheetLine, look: &super::fuse::CellLook) -> Result<(), String> {
        let test = self.with_only(&inside.at("only"), look.within.clone(), &line.only)?;
        let when = self.builder.condition_input(inside, &test)?;
        self.builder
            .put(inside, &format!("{}_count", look.prefix), None, when.clone());
        for (index, summary) in line.summaries.iter().enumerate() {
            let value_type = match summary.summary {
                Some(SummaryKind::Count) | None => continue,
                Some(kind) => summary_accumulator_type(kind, false).0,
            };
            let summary_site = inside.at(&format!("summaries.{index}.value"));
            let value = self.slot(&summary_site, &summary.value)?;
            let value = self.builder.input_as(&summary_site, &value, &value_type)?;
            self.builder.put(
                &summary_site,
                &format!("{}_{}", look.prefix, summary_accumulator(&summary.id)),
                Some(value),
                when.clone(),
            );
        }
        Ok(())
    }
}

/// Returns the name of the accumulator of the summary `id`, apart from the inner outputs of a fold such as `cell`.
pub(super) fn summary_accumulator(id: &str) -> String {
    format!("summary_{id}")
}

/// Sets the accumulators of the fold `node` in pass `pass` to `accumulators`, sorted by their keys.
pub(super) fn write_accumulators<K: Ord>(
    builder: &mut Builder<'_>,
    pass: usize,
    node: NodeId,
    accumulators: &mut [(K, ConfigValue)],
) {
    accumulators.sort_by(|(first, _), (second, _)| first.cmp(second));
    if let Some(found) = builder.node_mut(pass, node) {
        let configs = accumulators.iter().map(|(_, config)| config.clone()).collect();
        found.config.insert("acc".to_owned(), ConfigValue::Array(configs));
    }
}

/// Returns the type and the combine of the accumulator of a summary of kind `kind`, a sum of arrows for a total or an
/// average when `arrow` holds.
pub(super) fn summary_accumulator_type(kind: SummaryKind, arrow: bool) -> (ValueType, &'static str) {
    match kind {
        SummaryKind::Count => (ValueType::U32, "count"),
        SummaryKind::Total | SummaryKind::Average if arrow => (ValueType::Vec2F, "sum"),
        SummaryKind::Total | SummaryKind::Average => (ValueType::F32, "sum"),
        SummaryKind::Largest => (ValueType::F32, "max"),
        SummaryKind::Smallest => (ValueType::F32, "min"),
        SummaryKind::Any => (ValueType::Bool, "or"),
        SummaryKind::All => (ValueType::Bool, "and"),
    }
}

/// Returns `value`, of `value_type`, when `count` is above 0, and its zero otherwise, written at `site`.
pub(super) fn guard_by_count(
    builder: &mut Builder<'_>,
    site: &Site,
    count: &Operand,
    value: &Operand,
    value_type: &ValueType,
) -> Result<Operand, String> {
    let count = builder.input_as(site, count, &ValueType::U32)?;
    let some = builder.node_value(
        site,
        "cmp.gt",
        vec![("a", count), ("b", InputValue::Literal(Literal::U32(0)))],
        ValueType::Bool,
    );
    builder.where_applies(site, &some, value, value_type)
}
