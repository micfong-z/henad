//! Lines that look at the cells around: Count and Look around.
//!
//! A Count is a `grid.count` per counted state when it counts neighbours in some states of a States property, and a
//! `grid.fold` with a `count` accumulator otherwise. A Look around is one `grid.fold` with a `count` accumulator and an
//! accumulator per summary: a total, an average or a largest or smallest value of a number, and whether any or every
//! neighbour holds yes. An average, a largest and a smallest value of no neighbour are 0. Inside a fold, a `their`
//! chip reads the neighbour's property through `lane.get_at` at the fold's `cell`, and the `only` test decides which
//! neighbours count.

use std::collections::{BTreeMap, HashMap};

use super::card::CardLowering;
use super::text;
use super::value::Operand;
use super::{Builder, Site};
use crate::ids::NodeId;
use crate::sheet::document::{
    Around, CompareTest, LineKind, PropertyType, SheetChip, SheetLine, SheetValue, Slot, SummaryKind,
};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, MemberName, PortRef};

/// A `grid.fold` over the neighbours that `their` chips read inside.
#[derive(Debug)]
pub(super) struct Fold {
    /// Temporary id of the fold.
    node: NodeId,
    /// Read of each property of the neighbour, by property id.
    reads: HashMap<String, Operand>,
}

/// Name of the accumulator of a fold that counts the neighbours.
const COUNT: &str = "count";

/// Returns whether `around` is the 8 cells or the 3 by 3 block, and whether it includes the cell itself.
///
/// # Errors
///
/// Returns the message for agents within a distance, which are no cells.
fn stencil(around: Option<Around>) -> Result<(bool, bool), String> {
    match around {
        Some(Around::Cells8) => Ok((true, false)),
        Some(Around::Cells4) => Ok((false, false)),
        Some(Around::Block3x3) => Ok((true, true)),
        Some(Around::AgentsWithin) => Err("agents within a distance are no cells".to_owned()),
        None => Err("the line names no cells to look at".to_owned()),
    }
}

/// Returns the number of cells that `around` looks at, `None` for agents within a distance.
pub(super) fn neighbour_count(around: Around) -> Option<u32> {
    match around {
        Around::Cells8 => Some(8),
        Around::Cells4 => Some(4),
        Around::Block3x3 => Some(9),
        Around::AgentsWithin => None,
    }
}

/// Returns the accumulator `name` of `value_type` that combines by `combine`, as a fold's configuration holds it.
fn accumulator(name: &str, value_type: &str, combine: &str) -> ConfigValue {
    ConfigValue::Table(BTreeMap::from([
        ("name".to_owned(), text(name)),
        ("type".to_owned(), text(value_type)),
        ("combine".to_owned(), text(combine)),
    ]))
}

/// Returns the output `port` of the node `node`.
fn port_of(node: NodeId, port: &str, value_type: ValueType) -> Operand {
    Operand::Link(
        PortRef {
            node,
            port: Some(port.to_owned()),
        },
        value_type,
    )
}

impl<'a> CardLowering<'_, 'a> {
    /// Returns the read of the neighbour's `property` in the innermost fold.
    pub(super) fn their(&mut self, site: &Site, property: &str) -> Result<Operand, String> {
        let fold = self
            .folds
            .last()
            .ok_or_else(|| "their stands outside a look at the cells around".to_owned())?;
        if let Some(read) = fold.reads.get(property) {
            return Ok(read.clone());
        }
        let fold_node = fold.node;
        let (_, found) = self.property(Some(property))?;
        let value_type = super::world::lane_type(found);
        let node = self.builder.emit(
            &site.inside(fold_node),
            "lane.get_at",
            vec![("lane", text(property))],
            vec![(
                "at".to_owned(),
                InputValue::Link(PortRef {
                    node: fold_node,
                    port: Some("cell".to_owned()),
                }),
            )],
        );
        let read = Operand::Link(PortRef { node, port: None }, value_type);
        if let Some(fold) = self.folds.last_mut() {
            fold.reads.insert(property.to_owned(), read.clone());
        }
        Ok(read)
    }

    /// Writes a `grid.fold` over the cells `around` with `accumulators` at `site`, and returns its temporary id.
    fn open_fold(
        &mut self,
        site: &Site,
        around: Option<Around>,
        accumulators: Vec<ConfigValue>,
    ) -> Result<NodeId, String> {
        let (moore, include_center) = stencil(around)?;
        Ok(self.builder.emit(
            site,
            "grid.fold",
            vec![
                ("order", text(if moore { "moore_row_major" } else { "von_neumann" })),
                ("include_center", ConfigValue::Bool(include_center)),
                ("boundary", text("domain")),
                ("acc", ConfigValue::Array(accumulators)),
            ],
            vec![("when".to_owned(), InputValue::Literal(Literal::Bool(true)))],
        ))
    }

    /// Returns `test` joined with the test of the `only` slot `only`, lowered at `only_site` inside a fold, when it is
    /// filled.
    fn with_only(&mut self, only_site: &Site, test: Operand, only: &'a Slot) -> Result<Operand, String> {
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
        let site = site.at("whose");
        if line.only.is_blank()
            && let Some((property, states)) = self.counted_states(whose)
        {
            let mut total: Option<Operand> = None;
            for state in states {
                let node = self.builder.emit(
                    &site,
                    "grid.count",
                    vec![
                        ("lane", text(&property)),
                        ("neighborhood", text(if moore { "moore" } else { "von_neumann" })),
                        ("include_center", ConfigValue::Bool(include_center)),
                    ],
                    vec![
                        (
                            "value".to_owned(),
                            InputValue::Literal(Literal::Category(MemberName {
                                owner: property.clone(),
                                member: state,
                            })),
                        ),
                        ("when".to_owned(), InputValue::Literal(Literal::Bool(true))),
                    ],
                );
                let count = Operand::Link(PortRef { node, port: None }, ValueType::U32);
                total = Some(match total {
                    None => count,
                    Some(sum) => {
                        let a = self.builder.input_as(&site, &sum, &ValueType::U32, false)?;
                        let b = self.builder.input_as(&site, &count, &ValueType::U32, false)?;
                        self.builder
                            .node_value(&site, "math.add", vec![("a", a), ("b", b)], ValueType::U32)
                    }
                });
            }
            return Ok(total.unwrap_or(Operand::Constant(Literal::U32(0))));
        }
        let fold = self.open_fold(&site, line.around, vec![accumulator(COUNT, "u32", "count")])?;
        self.folds.push(Fold {
            node: fold,
            reads: HashMap::new(),
        });
        let inside = site.inside(fold);
        let test = self.value(&inside, whose).and_then(|test| {
            let test = self.with_only(&only_site.inside(fold), test, &line.only)?;
            let when = self.builder.condition_input(&inside, &test)?;
            self.builder.emit(
                &inside,
                "region.put",
                vec![("acc", text(COUNT))],
                vec![("when".to_owned(), when)],
            );
            Ok(())
        });
        self.folds.pop();
        test?;
        Ok(port_of(fold, COUNT, ValueType::U32))
    }

    /// Returns the States property and the states a Count's test `whose` counts, when it tests their States property
    /// against states alone, `None` for any other test.
    fn counted_states(&self, whose: &SheetValue) -> Option<(String, Vec<String>)> {
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
        let mut accumulators = vec![accumulator(COUNT, "u32", "count")];
        for summary in &line.summaries {
            let (value_type, combine) = match summary.summary {
                Some(SummaryKind::Count) => continue,
                Some(SummaryKind::Total | SummaryKind::Average) => ("f32", "sum"),
                Some(SummaryKind::Largest) => ("f32", "max"),
                Some(SummaryKind::Smallest) => ("f32", "min"),
                Some(SummaryKind::Any) => ("bool", "or"),
                Some(SummaryKind::All) => ("bool", "and"),
                None => return Err(format!("summary '{}' says nothing of how to summarize", summary.id)),
            };
            accumulators.push(accumulator(&summary_accumulator(&summary.id), value_type, combine));
        }
        let fold = self.open_fold(site, line.around, accumulators)?;
        self.folds.push(Fold {
            node: fold,
            reads: HashMap::new(),
        });
        let inside = site.inside(fold);
        let body = self.look_body(&inside, line);
        self.folds.pop();
        body?;
        let count = port_of(fold, COUNT, ValueType::U32);
        self.parts.insert((id, COUNT), count.clone());
        for summary in &line.summaries {
            let name = summary_accumulator(&summary.id);
            let part = match summary.summary {
                Some(SummaryKind::Count) => count.clone(),
                Some(SummaryKind::Total) => port_of(fold, &name, ValueType::F32),
                Some(SummaryKind::Average) => {
                    let total = port_of(fold, &name, ValueType::F32);
                    let total = self.builder.input_as(site, &total, &ValueType::F32, false)?;
                    let number = self.builder.input_as(site, &count, &ValueType::F32, false)?;
                    let average =
                        self.builder
                            .node_value(site, "math.div", vec![("a", total), ("b", number)], ValueType::F32);
                    guard_by_count(self.builder, site, &count, &average)?
                }
                Some(SummaryKind::Largest | SummaryKind::Smallest) => {
                    let extreme = port_of(fold, &name, ValueType::F32);
                    guard_by_count(self.builder, site, &count, &extreme)?
                }
                Some(SummaryKind::Any | SummaryKind::All) => port_of(fold, &name, ValueType::Bool),
                None => continue,
            };
            self.parts.insert((id, summary.id.as_str()), part);
        }
        Ok(())
    }

    /// Writes the body of a Look around's fold at `inside`: the `only` test, the count and each summary's value.
    fn look_body(&mut self, inside: &Site, line: &'a SheetLine) -> Result<(), String> {
        let test = self.with_only(&inside.at("only"), Operand::Constant(Literal::Bool(true)), &line.only)?;
        let when = self.builder.condition_input(inside, &test)?;
        self.builder.emit(
            inside,
            "region.put",
            vec![("acc", text(COUNT))],
            vec![("when".to_owned(), when.clone())],
        );
        for (index, summary) in line.summaries.iter().enumerate() {
            let value_type = match summary.summary {
                Some(SummaryKind::Count) | None => continue,
                Some(SummaryKind::Any | SummaryKind::All) => ValueType::Bool,
                Some(_) => ValueType::F32,
            };
            let summary_site = inside.at(&format!("summaries.{index}.value"));
            let value = self.slot(&summary_site, &summary.value)?;
            let value = self.builder.input_as(&summary_site, &value, &value_type, false)?;
            self.builder.emit(
                &summary_site,
                "region.put",
                vec![("acc", text(&summary_accumulator(&summary.id)))],
                vec![("value".to_owned(), value), ("when".to_owned(), when.clone())],
            );
        }
        Ok(())
    }
}

/// Returns the name of the accumulator of the summary `id`, apart from the inner outputs of a fold such as `cell`.
fn summary_accumulator(id: &str) -> String {
    format!("summary_{id}")
}

/// Returns `value` when `count` is above 0, and 0 otherwise, written at `site`.
fn guard_by_count(builder: &mut Builder<'_>, site: &Site, count: &Operand, value: &Operand) -> Result<Operand, String> {
    let count = builder.input_as(site, count, &ValueType::U32, false)?;
    let some = builder.node_value(
        site,
        "cmp.gt",
        vec![("a", count), ("b", InputValue::Literal(Literal::U32(0)))],
        ValueType::Bool,
    );
    let condition = builder.condition_input(site, &some)?;
    let value = builder.input_as(site, value, &ValueType::F32, false)?;
    Ok(builder.node_value(
        site,
        "logic.select",
        vec![
            ("cond", condition),
            ("if_true", value),
            ("if_false", InputValue::Literal(Literal::F32(0.0))),
        ],
        ValueType::F32,
    ))
}
