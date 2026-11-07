//! Looks at agents within a distance: Look around and Count over agents.
//!
//! The looks of a card share one `agent.radius_fold` at the largest of their distances, and a look with a smaller
//! distance keeps the agents whose `dist_sq` is within its own. Looks that include me and looks that leave me out take
//! a fold each, and so do looks past the [`MAX_ACCUMULATORS`] of one fold, split in line id order. Each look holds a
//! `count` accumulator, `<line>_count`, and an accumulator `<line>_summary_<id>` per summary, ordered by line id so a
//! reorder of the looks leaves the folds as they were. The index's `cell_size` is the largest current distance divided
//! by 3, from one node every fold reads (V14), so the engine sizes the index from the parameters' current values.
//!
//! Inside a fold, `their` reads the other agent's property through `lane.get_at` at the fold's `other`, the offset to
//! them is the fold's `delta`, the offset away from them its negation, and the distance to them the square root of its
//! `dist_sq`. An average, largest and smallest value of no agent are 0, and a Steer towards one adds nothing.

use std::collections::{BTreeMap, HashMap};

use super::card::CardLowering;
use super::ids::{GROUP_LOOK, Owner, SheetKey, line_number};
use super::look::{Fold, accumulator, guard_by_count, port_of, summary_accumulator};
use super::value::Operand;
use super::{Site, text};
use crate::ids::NodeId;
use crate::limits::MAX_ACCUMULATORS;
use crate::sheet::document::{
    Around, ChipRelation, LineKind, PropertyType, SheetChip, SheetLine, SheetValue, SummaryKind,
};
use crate::sheet::sentence;
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

/// The folds of a card's looks at agents.
#[derive(Debug)]
pub(super) struct AgentLooks<'a> {
    /// Every fold, in the order the looks they hold first appear in line id order.
    folds: Vec<AgentFold>,
    /// Index of the fold of each look, by line id.
    fold_of: HashMap<&'a str, usize>,
    /// Distance of each look, by line id.
    distance: HashMap<&'a str, Operand>,
}

/// One `agent.radius_fold` and the looks it holds.
#[derive(Debug)]
struct AgentFold {
    /// Temporary id of the fold.
    node: NodeId,
    /// Whether the looks of the fold have more than one distance, so each keeps the agents within its own.
    filtered: bool,
    /// Accumulators of the fold, each with the number of its line and its place among the line's accumulators.
    accumulators: Vec<((u32, usize), ConfigValue)>,
}

/// Returns the number of accumulators a look at agents holds: its count, and one per summary that is no count.
fn accumulator_count(line: &SheetLine) -> usize {
    1 + line
        .summaries
        .iter()
        .filter(|summary| summary.summary != Some(SummaryKind::Count))
        .count()
}

/// Returns the key that tells two distances apart: a parameter's id or a number's bits.
fn distance_key(distance: &Operand) -> String {
    match distance {
        Operand::Param(id, _) => format!("p {id}"),
        Operand::Number(number) => format!("n {:020}", number.to_bits()),
        _ => String::new(),
    }
}

impl<'a> CardLowering<'_, 'a> {
    /// Writes the folds of the card's looks at agents, once, at the first of them.
    fn open_looks(&mut self) -> Result<(), String> {
        if self.looks.is_some() {
            return Ok(());
        }
        let lines = self.agent_looks();
        let mut distance = HashMap::new();
        for &(_, id, line) in &lines {
            let operand = match line.distance.value() {
                Some(SheetValue::Literal(literal)) => literal.number().map(Operand::Number),
                Some(SheetValue::Chip(SheetChip::Param(parameter))) => self.builder.param(parameter).ok(),
                _ => None,
            }
            .ok_or_else(|| format!("the distance of line {id} is a number or a parameter, the same for every agent"))?;
            distance.insert(id, operand);
        }
        let groups = group_looks(&lines)?;
        let card_site = |pass: usize, index: u32, card: &str| Site {
            pass,
            owner: Owner::Sheet(SheetKey {
                group: GROUP_LOOK,
                index,
            }),
            path: format!("card.{card}"),
            region: None,
        };
        let shared = card_site(self.pass, 0, &self.card.id);
        let every: Vec<Operand> = lines
            .iter()
            .filter_map(|(_, id, _)| distance.get(id).cloned())
            .collect();
        let largest = self.largest(&shared, &every)?;
        let cell_size = self.builder.node_value(
            &shared,
            "math.div",
            vec![("a", largest.clone()), ("b", InputValue::Literal(Literal::F32(3.0)))],
            ValueType::F32,
        );
        let cell_size = self.builder.input_as(&shared, &cell_size, &ValueType::F32, false)?;
        let position = self
            .builder
            .properties()
            .iter()
            .find(|property| property.property_type == PropertyType::Position)
            .map(|property| property.id.clone())
            .ok_or_else(|| "the agents have no Position to look around from".to_owned())?;
        let center = self.builder.read(self.pass, &position)?;
        let center = self.builder.input_as(&shared, &center, &ValueType::Vec2F, false)?;
        let mut folds = Vec::with_capacity(groups.len());
        let mut fold_of = HashMap::new();
        for (index, group) in groups.into_iter().enumerate() {
            let LookGroup {
                include_me, members, ..
            } = group;
            let site = card_site(self.pass, index as u32 + 1, &self.card.id);
            let own: Vec<Operand> = members.iter().filter_map(|id| distance.get(id).cloned()).collect();
            let mut keys: Vec<String> = own.iter().map(distance_key).collect();
            keys.sort_unstable();
            keys.dedup();
            let radius = if keys.len()
                == every
                    .iter()
                    .map(distance_key)
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
            {
                largest.clone()
            } else {
                self.largest(&site, &own)?
            };
            let node = self.builder.emit(
                &site,
                "agent.radius_fold",
                vec![
                    ("exclude_self", ConfigValue::Bool(!include_me)),
                    ("acc", ConfigValue::Array(Vec::new())),
                ],
                vec![
                    ("center".to_owned(), center.clone()),
                    ("radius".to_owned(), radius),
                    ("cell_size".to_owned(), cell_size.clone()),
                    ("when".to_owned(), InputValue::Literal(Literal::Bool(true))),
                ],
            );
            let name = self.look_name(&members);
            self.builder.fault_names.push((node, name));
            for id in &members {
                fold_of.insert(*id, index);
            }
            folds.push(AgentFold {
                node,
                filtered: keys.len() > 1,
                accumulators: Vec::new(),
            });
        }
        self.looks = Some(AgentLooks {
            folds,
            fold_of,
            distance,
        });
        Ok(())
    }

    /// Returns the card's looks at agents that run, in line id order, each with its line number.
    fn agent_looks(&self) -> Vec<(u32, &'a str, &'a SheetLine)> {
        let mut lines: Vec<(u32, &'a str, &'a SheetLine)> = self
            .relations
            .order()
            .iter()
            .copied()
            .filter(|&id| !self.builder.report.is_skipped(id))
            .filter_map(|id| {
                let line = self.relations.line(id)?;
                let is_look = matches!(line.kind, LineKind::Count | LineKind::LookAround)
                    && line.around == Some(Around::AgentsWithin);
                is_look.then(|| (line_number(id).unwrap_or(0), id, line))
            })
            .collect();
        lines.sort_by_key(|(number, _, _)| *number);
        lines
    }

    /// Returns the name of the looks `members` in the author's words, by their numbers on the card and their names.
    fn look_name(&self, members: &[&str]) -> String {
        let mut named: Vec<(usize, Option<&str>)> = members
            .iter()
            .map(|id| {
                (
                    self.relations.number(id).unwrap_or(0),
                    self.relations.line(id).and_then(|line| line.name.as_deref()),
                )
            })
            .collect();
        named.sort_by_key(|(number, _)| *number);
        sentence::look_name(&named)
    }

    /// Returns the largest of `distances` as an `f32` input, written at `site`, each distinct distance once in a fixed
    /// order.
    fn largest(&mut self, site: &Site, distances: &[Operand]) -> Result<InputValue, String> {
        let mut distinct: BTreeMap<String, &Operand> = BTreeMap::new();
        for distance in distances {
            distinct.entry(distance_key(distance)).or_insert(distance);
        }
        let mut largest: Option<InputValue> = None;
        for distance in distinct.into_values() {
            let input = self.builder.input_as(site, distance, &ValueType::F32, false)?;
            largest = Some(match largest {
                None => input,
                Some(known) => {
                    let node =
                        self.builder
                            .node_value(site, "math.max", vec![("a", known), ("b", input)], ValueType::F32);
                    self.builder.input_as(site, &node, &ValueType::F32, false)?
                }
            });
        }
        largest.ok_or_else(|| "a look at agents has no distance".to_owned())
    }

    /// Opens the fold of the look `id` for its body: pushes it on [`CardLowering::folds`] and returns its id with the
    /// test that keeps the agents within the look's own distance, written at `site`.
    fn enter_look(&mut self, site: &Site, id: &'a str) -> Result<(NodeId, Operand), String> {
        self.open_looks()?;
        let looks = self
            .looks
            .as_ref()
            .ok_or_else(|| "the looks at agents did not lower".to_owned())?;
        let index = *looks
            .fold_of
            .get(id)
            .ok_or_else(|| format!("line {id} has no fold of its own"))?;
        let fold = &looks.folds[index];
        let node = fold.node;
        let filtered = fold.filtered;
        let distance = looks.distance.get(id).cloned();
        self.folds.push(Fold {
            node,
            reads: HashMap::new(),
            agents: true,
        });
        let within = match (filtered, distance) {
            (true, Some(distance)) => {
                let distance_site = site.at("distance");
                let d = self
                    .builder
                    .input_as(&distance_site, &distance, &ValueType::F32, false)?;
                let squared = self.builder.node_value(
                    &distance_site,
                    "math.mul",
                    vec![("a", d.clone()), ("b", d)],
                    ValueType::F32,
                );
                let squared = self
                    .builder
                    .input_as(&distance_site, &squared, &ValueType::F32, false)?;
                let inside = distance_site.inside(node);
                self.builder.node_value(
                    &inside,
                    "cmp.le",
                    vec![("a", inner(node, "dist_sq")), ("b", squared)],
                    ValueType::Bool,
                )
            }
            _ => Operand::Constant(Literal::Bool(true)),
        };
        Ok((node, within))
    }

    /// Records accumulator `config` of the look `id`, the `order`th of the line, in the fold of the look.
    fn add_accumulator(&mut self, id: &str, order: usize, config: ConfigValue) {
        let number = line_number(id).unwrap_or(0);
        if let Some(looks) = self.looks.as_mut()
            && let Some(&index) = looks.fold_of.get(id)
        {
            looks.folds[index].accumulators.push(((number, order), config));
        }
    }

    /// Returns the count of a Count line over agents within a distance.
    pub(super) fn count_agents(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<Operand, String> {
        let (node, within) = self.enter_look(site, id)?;
        let body = self.count_body(site, id, line, node, &within);
        self.folds.pop();
        body
    }

    /// Writes the body of the Count line `id` in the fold `node`, `within` its distance, and returns its count.
    fn count_body(
        &mut self,
        site: &Site,
        id: &'a str,
        line: &'a SheetLine,
        node: NodeId,
        within: &Operand,
    ) -> Result<Operand, String> {
        let whose_site = site.at("whose").inside(node);
        let whose = self.slot(&whose_site, &line.whose)?;
        let test = self.builder.join(&whose_site, "logic.and", within, &whose)?;
        let test = self.with_only(&site.at("only").inside(node), test, &line.only)?;
        let when = self.builder.condition_input(&whose_site, &test)?;
        let name = format!("{id}_count");
        self.builder.emit(
            &whose_site,
            "region.put",
            vec![("acc", text(&name))],
            vec![("when".to_owned(), when)],
        );
        self.add_accumulator(id, 0, accumulator(&name, "u32", "count"));
        Ok(port_of(node, &name, ValueType::U32))
    }

    /// Lowers the Look around line `id` over agents within a distance, and records each part of its result: `count`
    /// and every summary by its id.
    pub(super) fn look_around_agents(&mut self, site: &Site, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let (node, within) = self.enter_look(site, id)?;
        let body = self.look_agents_body(site, id, line, node, &within);
        self.folds.pop();
        let types = body?;
        let count_name = format!("{id}_count");
        let count = port_of(node, &count_name, ValueType::U32);
        self.parts.insert((id, "count"), count.clone());
        for (summary, value_type) in line.summaries.iter().zip(types) {
            let name = format!("{id}_{}", summary_accumulator(&summary.id));
            let part = match summary.summary {
                Some(SummaryKind::Count) => count.clone(),
                Some(SummaryKind::Total) => port_of(node, &name, value_type),
                Some(SummaryKind::Average) => {
                    let total = port_of(node, &name, value_type.clone());
                    let total = self.builder.input_as(site, &total, &value_type, false)?;
                    let number = self.builder.input_as(site, &count, &ValueType::F32, false)?;
                    let average = if value_type == ValueType::Vec2F {
                        let share = self
                            .builder
                            .node_value(site, "math.recip", vec![("a", number)], ValueType::F32);
                        let share = self.builder.input_as(site, &share, &ValueType::F32, false)?;
                        self.builder
                            .node_value(site, "vec.scale", vec![("v", total), ("s", share)], ValueType::Vec2F)
                    } else {
                        self.builder
                            .node_value(site, "math.div", vec![("a", total), ("b", number)], ValueType::F32)
                    };
                    self.empty.insert((id, summary.id.as_str()), count.clone());
                    guard_by_count(self.builder, site, &count, &average, &value_type)?
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

    /// Writes the body of the Look around line `id` in the fold `node`, `within` its distance: the `only` test, the
    /// count and each summary's value. Returns the type of each summary's accumulator.
    fn look_agents_body(
        &mut self,
        site: &Site,
        id: &'a str,
        line: &'a SheetLine,
        node: NodeId,
        within: &Operand,
    ) -> Result<Vec<ValueType>, String> {
        let inside = site.inside(node);
        let test = self.with_only(&site.at("only").inside(node), within.clone(), &line.only)?;
        let when = self.builder.condition_input(&inside, &test)?;
        let count_name = format!("{id}_count");
        self.builder.emit(
            &inside,
            "region.put",
            vec![("acc", text(&count_name))],
            vec![("when".to_owned(), when.clone())],
        );
        self.add_accumulator(id, 0, accumulator(&count_name, "u32", "count"));
        let mut types = Vec::with_capacity(line.summaries.len());
        for (index, summary) in line.summaries.iter().enumerate() {
            let Some(kind) = summary.summary else {
                return Err(format!("summary '{}' says nothing of how to summarize", summary.id));
            };
            if kind == SummaryKind::Count {
                types.push(ValueType::U32);
                continue;
            }
            let summary_site = inside.at(&format!("summaries.{index}.value"));
            let value = self.slot(&summary_site, &summary.value)?;
            let (value_type, combine) = match kind {
                SummaryKind::Total | SummaryKind::Average if value.value_type() == Some(ValueType::Vec2F) => {
                    (ValueType::Vec2F, "sum")
                }
                SummaryKind::Total | SummaryKind::Average => (ValueType::F32, "sum"),
                SummaryKind::Largest => (ValueType::F32, "max"),
                SummaryKind::Smallest => (ValueType::F32, "min"),
                SummaryKind::Any => (ValueType::Bool, "or"),
                _ => (ValueType::Bool, "and"),
            };
            let value = self.builder.input_as(&summary_site, &value, &value_type, false)?;
            let name = format!("{id}_{}", summary_accumulator(&summary.id));
            self.builder.emit(
                &summary_site,
                "region.put",
                vec![("acc", text(&name))],
                vec![("value".to_owned(), value), ("when".to_owned(), when.clone())],
            );
            self.add_accumulator(id, index + 1, accumulator(&name, &value_type.to_string(), combine));
            types.push(value_type);
        }
        Ok(types)
    }

    /// Returns the value of `relation` between me and the agent the innermost fold visits, written at `site`.
    pub(super) fn relation(&mut self, site: &Site, relation: ChipRelation) -> Result<Operand, String> {
        let fold = self
            .folds
            .last()
            .filter(|fold| fold.agents)
            .ok_or_else(|| format!("{} stands outside a look at agents", relation.as_str()))?;
        let node = fold.node;
        let site = site.inside(node);
        Ok(match relation {
            ChipRelation::OffsetToThem => Operand::Link(
                PortRef {
                    node,
                    port: Some("delta".to_owned()),
                },
                ValueType::Vec2F,
            ),
            ChipRelation::OffsetAwayFromThem => {
                self.builder
                    .node_value(&site, "math.neg", vec![("a", inner(node, "delta"))], ValueType::Vec2F)
            }
            ChipRelation::DistanceToThem => {
                self.builder
                    .node_value(&site, "math.sqrt", vec![("a", inner(node, "dist_sq"))], ValueType::F32)
            }
        })
    }

    /// Writes the accumulators of every fold of the card's looks at agents, in line id order.
    pub(super) fn finish_looks(&mut self) {
        let Some(looks) = self.looks.as_mut() else {
            return;
        };
        let pass = self.pass;
        let mut written = Vec::with_capacity(looks.folds.len());
        for fold in &mut looks.folds {
            fold.accumulators.sort_by_key(|(key, _)| *key);
            written.push((
                fold.node,
                fold.accumulators
                    .iter()
                    .map(|(_, config)| config.clone())
                    .collect::<Vec<_>>(),
            ));
        }
        for (node, accumulators) in written {
            if let Some(found) = self.builder.node_mut(pass, node) {
                found.config.insert("acc".to_owned(), ConfigValue::Array(accumulators));
            }
        }
    }
}

/// The looks one fold holds.
#[derive(Debug)]
struct LookGroup<'a> {
    /// Whether the looks include me.
    include_me: bool,
    /// Ids of the looks, in line id order.
    members: Vec<&'a str>,
    /// Number of accumulators the looks hold.
    accumulators: usize,
}

/// Returns the looks `lines`, sorted by line id, grouped into folds: by whether they include me, then split past the
/// accumulators of one fold.
///
/// # Errors
///
/// Returns the message for a look that holds more accumulators than a fold can.
fn group_looks<'a>(lines: &[(u32, &'a str, &'a SheetLine)]) -> Result<Vec<LookGroup<'a>>, String> {
    let mut groups: Vec<LookGroup<'a>> = Vec::new();
    for &(_, id, line) in lines {
        let include_me = line.include_me.unwrap_or(false);
        let count = accumulator_count(line);
        if count > MAX_ACCUMULATORS {
            return Err(format!(
                "line {id} holds {count} summaries with its count, and a look at agents holds at most \
                 {MAX_ACCUMULATORS}"
            ));
        }
        match groups.iter_mut().rev().find(|group| group.include_me == include_me) {
            Some(group) if group.accumulators + count <= MAX_ACCUMULATORS => {
                group.members.push(id);
                group.accumulators += count;
            }
            _ => groups.push(LookGroup {
                include_me,
                members: vec![id],
                accumulators: count,
            }),
        }
    }
    Ok(groups)
}

/// Returns the inner output `port` of the fold `node` as an input.
fn inner(node: NodeId, port: &str) -> InputValue {
    InputValue::Link(PortRef {
        node,
        port: Some(port.to_owned()),
    })
}
