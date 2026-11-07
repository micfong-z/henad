//! Cards: the tick pass of the card that runs each tick, and the lines of a card a button runs once.
//!
//! Lines lower in display order, each into its own block of node ids. A Count and a Look around over the cells lower
//! as [`super::look`] describes, a chance and a random number as [`super::draw`] does, and a Calculate to one
//! expression node ([`super::calculate`]). A Yes or no line is the comparisons and logic nodes it names, a Choose a
//! `logic.select` chain from its last row up, and a Depending on a `cat.switch` over my state. A When line's condition
//! is joined by `logic.and` into the condition of every change inside it. The changes of each property lower after the
//! card's lines, as the [module](super) describes.

use std::collections::{BTreeMap, HashMap};

use super::ids::{GROUP_CHANGE, Owner, SheetKey, line_number};
use super::look::Fold;
use super::value::{Operand, common_type, shared_type};
use super::{Builder, Site, text};
use crate::document::{Domain, Pass, PassStage};
use crate::limits::MAX_TRANSITION_RULES;
use crate::sheet::check::Place;
use crate::sheet::document::{
    Around, CardRuns, CompareTest, LineKind, PropertyType, SheetCard, SheetChip, SheetLine, SheetLiteral,
    SheetProperty, SheetValue, Slot,
};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{self, ANY_STATE_ID};
use crate::types::ValueType;
use crate::value::{ConfigValue, Literal, PortRef};

/// Writes the tick pass: the lines of the card that runs each tick over the cells, or no line when there is none.
///
/// A grid runs one tick pass, so a second card that runs each tick is refused.
pub(super) fn lower_tick(builder: &mut Builder<'_>) {
    let document = builder.document;
    let cards: Vec<usize> = document
        .cards
        .iter()
        .enumerate()
        .filter(|(_, card)| card.every == "cells" && card.runs == CardRuns::EachTick)
        .map(|(index, _)| index)
        .collect();
    if let Some(&second) = cards.get(1) {
        builder.problem(
            format!(
                "card '{}' is a second card that runs each tick over the cells, and the cells run one",
                document.cards[second].id
            ),
            Place::Card(second),
        );
    }
    let id = cards.first().map_or_else(
        || "tick_cells".to_owned(),
        |&index| format!("tick_{}", document.cards[index].id),
    );
    let pass = builder.add_pass(Pass::new(&id, PassStage::Tick, Some(Domain::Cells)));
    if let Some(&index) = cards.first() {
        lower_card(builder, index, pass);
    }
}

/// Writes the lines of the card at `card_index` into pass `pass`.
pub(super) fn lower_card(builder: &mut Builder<'_>, card_index: usize, pass: usize) {
    let document = builder.document;
    let card = &document.cards[card_index];
    let mut lowering = CardLowering {
        relations: CardRelations::new(document, card),
        builder,
        card_index,
        card,
        pass,
        results: HashMap::new(),
        conditions: HashMap::new(),
        parts: HashMap::new(),
        changes: BTreeMap::new(),
        done: HashMap::new(),
        folds: Vec::new(),
    };
    lowering.run();
}

/// A reason a line cannot lower in this version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    /// A line kind this version does not lower, with the error in the author's words.
    NotYet(&'static str),
    /// A Record line, since a grid counts no events.
    Record,
    /// A line kind that runs on agents alone.
    Agents(LineKind),
}

/// Returns the first reason `line` cannot lower, its own kind first and then the phrases in its slots.
fn refusal(line: &SheetLine) -> Option<Refusal> {
    let own = match line.kind {
        LineKind::Rate => Some(Refusal::NotYet(sentence::NOT_YET_RATE)),
        LineKind::Repeat => Some(Refusal::NotYet(sentence::NOT_YET_REPEAT)),
        LineKind::Record => Some(Refusal::Record),
        kind @ (LineKind::PickCell
        | LineKind::LookAtCell
        | LineKind::RandomDirection
        | LineKind::Steer
        | LineKind::KeepBetween
        | LineKind::Move
        | LineKind::Leave) => Some(Refusal::Agents(kind)),
        kind @ (LineKind::Count | LineKind::LookAround) if line.around == Some(Around::AgentsWithin) => {
            Some(Refusal::Agents(kind))
        }
        _ => None,
    };
    own.or_else(|| line.slot_values().find_map(value_refusal))
}

/// Returns the first reason a value cannot lower, searching its phrases and lists.
fn value_refusal(value: &SheetValue) -> Option<Refusal> {
    match value {
        SheetValue::Phrase(phrase) => refusal(phrase),
        SheetValue::List(items) => items.iter().find_map(value_refusal),
        SheetValue::Literal(_) | SheetValue::Chip(_) => None,
    }
}

/// Returns the type the values `operands` of a Choose or Depending on share: the type that holds each number, `f32`
/// for numbers with no type, or the type of the yes or no or the property values.
///
/// # Errors
///
/// Returns the message for values of kinds that cannot meet, and for a state. The checker refuses a state in these
/// slots, where no property governs it.
fn choice_type(operands: &[&Operand]) -> Result<ValueType, String> {
    if operands.iter().any(|operand| matches!(operand, Operand::State(_))) {
        return Err("a state in a choice has no property to belong to".to_owned());
    }
    Ok(shared_type(operands)?.unwrap_or(ValueType::F32))
}

/// One change of a property: a Become or a Set line that runs.
#[derive(Debug, Clone)]
struct Change<'a> {
    /// Id of the line.
    line: &'a str,
    /// Number `k` of the line id `l<k>`.
    number: u32,
    /// What the line changes the property to.
    kind: ChangeKind,
    /// Condition under which the change applies, the conditions of the When lines around it joined in.
    condition: Operand,
}

/// What a change gives its property.
#[derive(Debug, Clone)]
enum ChangeKind {
    /// A Become: a state, from one state or, with `None`, from any state.
    Become {
        /// State the line changes from, `None` for any state.
        from: Option<String>,
        /// State the line changes to.
        to: String,
    },
    /// A Set: a value.
    Set {
        /// Value the line sets.
        value: Operand,
    },
}

/// State of the lowering of one card.
pub(super) struct CardLowering<'b, 'a> {
    /// The lowering.
    pub(super) builder: &'b mut Builder<'a>,
    /// Index of the card in the sheet's cards.
    pub(super) card_index: usize,
    /// The card.
    pub(super) card: &'a SheetCard,
    /// Relations between the card's lines.
    pub(super) relations: CardRelations<'a>,
    /// Index of the pass the card writes.
    pub(super) pass: usize,
    /// Result of each line lowered so far, by line id.
    results: HashMap<&'a str, Operand>,
    /// Each part of the result of a Look around line lowered so far, by line id and part.
    pub(super) parts: HashMap<(&'a str, &'a str), Operand>,
    /// Condition of each When line lowered so far, the conditions of the When lines around it joined in, by line id.
    conditions: HashMap<&'a str, Operand>,
    /// Changes of each property, by the property's index among the cells properties, in display order.
    changes: BTreeMap<usize, Vec<Change<'a>>>,
    /// New value of each property whose changes are written, by the property's index.
    done: HashMap<usize, Operand>,
    /// The folds over the neighbours around the value being lowered, innermost last.
    pub(super) folds: Vec<Fold>,
}

impl<'a> CardLowering<'_, 'a> {
    /// Lowers every line of the card in display order, then the changes of each property.
    fn run(&mut self) {
        let order: Vec<&'a str> = self.relations.order().to_vec();
        let mut refused = false;
        for &id in &order {
            let Some(line) = self.relations.line(id) else {
                continue;
            };
            if let Some(reason) = refusal(line) {
                refused = true;
                let message = match reason {
                    Refusal::NotYet(words) => words.to_owned(),
                    Refusal::Record => format!("line {id}: Record counts an event, and a grid counts no events"),
                    Refusal::Agents(kind) => format!(
                        "line {id}: a {} line runs on agents, and this card runs on the cells",
                        kind.as_str()
                    ),
                };
                self.builder
                    .problem(message, Place::Line(self.card_index, id.to_owned()));
            }
        }
        if refused {
            return;
        }
        for id in order {
            if self.builder.report.is_skipped(id) {
                continue;
            }
            let Some(line) = self.relations.line(id) else {
                continue;
            };
            if let Err(message) = self.line(id, line) {
                self.builder.problem(
                    format!("line {id}: {message}"),
                    Place::Line(self.card_index, id.to_owned()),
                );
            }
        }
        let pending: Vec<usize> = self.changes.keys().copied().collect();
        for index in pending {
            if !self.done.contains_key(&index)
                && let Err(message) = self.write_changes(index)
            {
                self.builder.problem(message, Place::Card(self.card_index));
            }
        }
    }

    /// Returns the site of the nodes of line `id`.
    fn line_site(&self, id: &str) -> Result<Site, String> {
        let number = line_number(id).ok_or_else(|| format!("'{id}' is no line id"))?;
        Ok(Site {
            pass: self.pass,
            owner: Owner::Line {
                id: id.to_owned(),
                number,
            },
            path: String::new(),
            region: None,
        })
    }

    /// Lowers line `id`.
    fn line(&mut self, id: &'a str, line: &'a SheetLine) -> Result<(), String> {
        let site = self.line_site(id)?;
        match line.kind {
            LineKind::Count
            | LineKind::Compare
            | LineKind::AllOf
            | LineKind::AnyOf
            | LineKind::NoneOf
            | LineKind::Chance
            | LineKind::RandomNumber
            | LineKind::Calculate
            | LineKind::Choose
            | LineKind::DependingOn => {
                let result = self.phrase(&site, line)?;
                self.results.insert(id, result);
            }
            LineKind::LookAround => self.look_around(&site, id, line)?,
            LineKind::When => {
                let own = self.slot(&site.at("condition"), &line.condition)?;
                let condition = self.inherit(&site, id, own)?;
                self.conditions.insert(id, condition);
            }
            LineKind::Become => {
                let (index, property) = self.property(line.property.as_deref())?;
                if property.property_type != PropertyType::States {
                    return Err(format!("'{}' has no states to become", property.id));
                }
                let to = line
                    .new_state
                    .clone()
                    .ok_or_else(|| "a Become needs a state".to_owned())?;
                let from = match line.start_state.as_deref() {
                    Some(ANY_STATE_ID) => None,
                    Some(state) => Some(state.to_owned()),
                    None => return Err("a Become needs the state it changes from".to_owned()),
                };
                let own = self.slot(&site.at("condition"), &line.condition)?;
                let condition = self.inherit(&site, id, own)?;
                self.add_change(index, id, ChangeKind::Become { from, to }, condition)?;
            }
            LineKind::Set => {
                let (index, _) = self.property(line.property.as_deref())?;
                let value = self.slot(&site.at("value"), &line.value)?;
                let own = self.slot(&site.at("condition"), &line.condition)?;
                let condition = self.inherit(&site, id, own)?;
                self.add_change(index, id, ChangeKind::Set { value }, condition)?;
            }
            other => return Err(format!("a {} line has no lowering", other.as_str())),
        }
        Ok(())
    }

    /// Returns the cells property `id` with its index.
    pub(super) fn property(&self, id: Option<&str>) -> Result<(usize, &'a SheetProperty), String> {
        let id = id.ok_or_else(|| "the line names no property".to_owned())?;
        self.builder
            .property(id)
            .ok_or_else(|| format!("'{id}' is no property of the cells"))
    }

    /// Records a change of the property at `index` by line `id`.
    fn add_change(&mut self, index: usize, id: &'a str, kind: ChangeKind, condition: Operand) -> Result<(), String> {
        if self.done.contains_key(&index) {
            return Err("the line changes a property that a line above reads as new".to_owned());
        }
        let number = line_number(id).ok_or_else(|| format!("'{id}' is no line id"))?;
        self.changes.entry(index).or_default().push(Change {
            line: id,
            number,
            kind,
            condition,
        });
        Ok(())
    }

    /// Returns `own`, the condition of line `id`, joined with the condition of the When line that holds it.
    fn inherit(&mut self, site: &Site, id: &str, own: Operand) -> Result<Operand, String> {
        let mut parent = self.relations.parent(id);
        while let Some(block) = parent {
            if self
                .relations
                .line(block)
                .is_some_and(|line| line.kind == LineKind::When)
            {
                let outer = self
                    .conditions
                    .get(block)
                    .cloned()
                    .ok_or_else(|| format!("the When line {block} around the line did not lower"))?;
                return self.builder.join(&site.at("when"), "logic.and", &outer, &own);
            }
            parent = self.relations.parent(block);
        }
        Ok(own)
    }

    /// Returns the operand of `slot` lowered at `site`.
    pub(super) fn slot(&mut self, site: &Site, slot: &'a Slot) -> Result<Operand, String> {
        let value = slot.value().ok_or_else(|| "a slot is blank".to_owned())?;
        self.value(site, value)
    }

    /// Returns the operand of `value` lowered at `site`.
    pub(super) fn value(&mut self, site: &Site, value: &'a SheetValue) -> Result<Operand, String> {
        match value {
            SheetValue::Literal(SheetLiteral::Bool(value)) => Ok(Operand::Constant(Literal::Bool(*value))),
            SheetValue::Literal(literal) => literal
                .number()
                .map(Operand::Number)
                .ok_or_else(|| "a literal holds no number".to_owned()),
            SheetValue::Chip(chip) => self.chip(site, chip),
            SheetValue::Phrase(phrase) => self.phrase(site, phrase),
            SheetValue::List(_) => Err("a list stands where one value is wanted".to_owned()),
        }
    }

    /// Returns the operand of `chip`.
    pub(super) fn chip(&mut self, site: &Site, chip: &SheetChip) -> Result<Operand, String> {
        match chip {
            SheetChip::Param(id) => self.builder.param(id),
            SheetChip::Result {
                line,
                part: None,
                property: None,
            } => self
                .results
                .get(line.as_str())
                .cloned()
                .ok_or_else(|| format!("the result of line {line} did not lower")),
            SheetChip::Result {
                line,
                part: Some(part),
                property: None,
            } => self
                .parts
                .get(&(line.as_str(), part.as_str()))
                .cloned()
                .ok_or_else(|| format!("line {line} has no part '{part}' to read")),
            SheetChip::Result { line, .. } => Err(format!("line {line} names no place, so it has no properties")),
            SheetChip::My(property) => self.builder.read(self.pass, property),
            SheetChip::MyNew(property) => {
                let (index, _) = self.property(Some(property))?;
                match self.done.get(&index) {
                    Some(value) => Ok(value.clone()),
                    None => self.write_changes(index),
                }
            }
            SheetChip::Their(property) => self.their(site, property),
            SheetChip::Relation(relation) => Err(format!("{} relates agents", relation.as_str())),
            SheetChip::State(state) => Ok(Operand::State(state.clone())),
        }
    }

    /// Returns the result of a line or phrase of a kind that works out a value.
    fn phrase(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        match line.kind {
            LineKind::Always => Ok(Operand::Constant(Literal::Bool(true))),
            LineKind::Compare => self.compare(site, line),
            LineKind::AllOf => self.logic(site, line, "logic.and"),
            LineKind::AnyOf => self.logic(site, line, "logic.or"),
            LineKind::NoneOf => {
                let any = self.logic(site, line, "logic.or")?;
                match any {
                    Operand::Constant(Literal::Bool(value)) => Ok(Operand::Constant(Literal::Bool(!value))),
                    any => {
                        let input = self.builder.condition_input(site, &any)?;
                        Ok(self
                            .builder
                            .node_value(site, "logic.not", vec![("a", input)], ValueType::Bool))
                    }
                }
            }
            LineKind::Count => self.count(site, line),
            LineKind::Chance => self.chance(site, line),
            LineKind::RandomNumber => self.random_number(site, line),
            LineKind::Calculate => self.calculate(site, line),
            LineKind::Choose => self.choose(site, line),
            LineKind::DependingOn => self.depending_on(site, line),
            LineKind::LookAround => {
                Err("a Look around gives several values, so it stands on a line of its own".to_owned())
            }
            other => Err(format!("a {} phrase has no lowering", other.as_str())),
        }
    }

    /// Returns the items of an all of, any of or none of joined by `op`.
    fn logic(&mut self, site: &Site, line: &'a SheetLine, op: &str) -> Result<Operand, String> {
        let mut joined = Operand::Constant(Literal::Bool(op == "logic.and"));
        for (index, item) in line.items.iter().enumerate() {
            let item_site = site.at(&format!("items.{index}"));
            let operand = self.value(&item_site, item)?;
            joined = self.builder.join(&item_site, op, &joined, &operand)?;
        }
        Ok(joined)
    }

    /// Returns the comparison of a compare line or phrase.
    fn compare(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let test = line.test.ok_or_else(|| "a comparison needs a test".to_owned())?;
        let left = self.slot(&site.at("left"), &line.left)?;
        match test {
            CompareTest::Outside => {
                let low = self.slot(&site.at("low"), &line.low)?;
                let high = self.slot(&site.at("high"), &line.high)?;
                let below = self.order(&site.at("low"), "cmp.lt", &left, &low)?;
                let above = self.order(&site.at("high"), "cmp.gt", &left, &high)?;
                self.builder.join(site, "logic.or", &below, &above)
            }
            CompareTest::OneOf | CompareTest::NotOneOf => {
                let (op, join, empty) = if test == CompareTest::OneOf {
                    ("cmp.eq", "logic.or", false)
                } else {
                    ("cmp.ne", "logic.and", true)
                };
                let items: Vec<&'a SheetValue> = match line.right.value() {
                    Some(SheetValue::List(items)) => items.iter().collect(),
                    Some(value) => vec![value],
                    None => return Err("a comparison needs a right side".to_owned()),
                };
                let mut joined = Operand::Constant(Literal::Bool(empty));
                for (index, item) in items.into_iter().enumerate() {
                    let item_site = site.at(&format!("right.{index}"));
                    let right = self.value(&item_site, item)?;
                    let compared = self.order(&item_site, op, &left, &right)?;
                    joined = self.builder.join(&item_site, join, &joined, &compared)?;
                }
                Ok(joined)
            }
            CompareTest::Within => {
                let right = self.slot(&site.at("right"), &line.right)?;
                let tolerance = self.slot(&site.at("tolerance"), &line.tolerance)?;
                let a = self.builder.input_as(site, &left, &ValueType::F32, false)?;
                let b = self.builder.input_as(site, &right, &ValueType::F32, false)?;
                let difference = self
                    .builder
                    .node_value(site, "math.sub", vec![("a", a), ("b", b)], ValueType::F32);
                let difference = self.builder.input_as(site, &difference, &ValueType::F32, false)?;
                let distance = self
                    .builder
                    .node_value(site, "math.abs", vec![("a", difference)], ValueType::F32);
                self.order(site, "cmp.le", &distance, &tolerance)
            }
            CompareTest::Is
            | CompareTest::IsNot
            | CompareTest::AtLeast
            | CompareTest::AtMost
            | CompareTest::MoreThan
            | CompareTest::LessThan => {
                let right = self.slot(&site.at("right"), &line.right)?;
                let op = match test {
                    CompareTest::Is => "cmp.eq",
                    CompareTest::IsNot => "cmp.ne",
                    CompareTest::AtLeast => "cmp.ge",
                    CompareTest::AtMost => "cmp.le",
                    CompareTest::MoreThan => "cmp.gt",
                    _ => "cmp.lt",
                };
                self.order(site, op, &left, &right)
            }
        }
    }

    /// Returns `a` compared with `b` by the comparison node `op`, both in the type they take together.
    fn order(&mut self, site: &Site, op: &str, a: &Operand, b: &Operand) -> Result<Operand, String> {
        let value_type = common_type(a, b)?;
        if matches!(value_type, ValueType::Category(_) | ValueType::Bool) && !matches!(op, "cmp.eq" | "cmp.ne") {
            return Err("states and yes or no values have no order".to_owned());
        }
        let a = self.builder.input_as(site, a, &value_type, false)?;
        let b = self.builder.input_as(site, b, &value_type, false)?;
        Ok(self
            .builder
            .node_value(site, op, vec![("a", a), ("b", b)], ValueType::Bool))
    }

    /// Returns the value of a Choose: the value of its first row whose condition holds, or its "otherwise" value.
    ///
    /// The rows lower to a `logic.select` chain from the last row up. A row whose condition always holds ends the
    /// chain, and a row whose condition never holds is left out.
    fn choose(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let mut rows = Vec::with_capacity(line.rows.len());
        for (index, row) in line.rows.iter().enumerate() {
            let row_site = site.at(&format!("row.{index}"));
            let value = self.slot(&row_site.at("value"), &row.value)?;
            let condition = self.slot(&row_site.at("condition"), &row.condition)?;
            rows.push((row_site, value, condition));
        }
        let otherwise_site = site.at("otherwise");
        let otherwise = self.slot(&otherwise_site, &line.otherwise)?;
        let operands: Vec<&Operand> = rows.iter().map(|(_, value, _)| value).chain([&otherwise]).collect();
        let value_type = choice_type(&operands)?;
        let mut chosen = otherwise.clone();
        for (row_site, value, condition) in rows.into_iter().rev() {
            chosen = match condition {
                Operand::Constant(Literal::Bool(true)) => value,
                Operand::Constant(Literal::Bool(false)) => chosen,
                condition => {
                    let condition = self.builder.condition_input(&row_site, &condition)?;
                    let if_true = self.builder.input_as(&row_site, &value, &value_type, false)?;
                    let if_false = self.builder.input_as(&row_site, &chosen, &value_type, false)?;
                    self.builder.node_value(
                        &row_site,
                        "logic.select",
                        vec![("cond", condition), ("if_true", if_true), ("if_false", if_false)],
                        value_type.clone(),
                    )
                }
            };
        }
        Ok(chosen)
    }

    /// Returns the value of a Depending on: the value its branch for my state gives, through a `cat.switch`.
    fn depending_on(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let (_, property) = self.property(line.property.as_deref())?;
        if property.property_type != PropertyType::States {
            return Err(format!("'{}' has no states to depend on", property.id));
        }
        let mut branches = Vec::with_capacity(property.states.len());
        for state in &property.states {
            let value = line
                .branches
                .get(&state.id)
                .ok_or_else(|| format!("no value for the state '{}'", state.id))?;
            let branch_site = site.at(&format!("branches.{}", state.id));
            branches.push((state.id.as_str(), self.value(&branch_site, value)?));
        }
        let operands: Vec<&Operand> = branches.iter().map(|(_, value)| value).collect();
        let value_type = choice_type(&operands)?;
        let category = ValueType::Category(property.id.clone());
        let current = self.builder.read(self.pass, &property.id)?;
        let mut inputs = vec![(
            "select".to_owned(),
            self.builder.input_as(site, &current, &category, false)?,
        )];
        for (state, value) in &branches {
            inputs.push((
                format!("case.{state}"),
                self.builder.input_as(site, value, &value_type, false)?,
            ));
        }
        let node = self
            .builder
            .emit(site, "cat.switch", vec![("category", text(&property.id))], inputs);
        Ok(Operand::Link(PortRef { node, port: None }, value_type))
    }

    /// Writes the changes of the property at `index` and its new value, and returns that value.
    fn write_changes(&mut self, index: usize) -> Result<Operand, String> {
        let property = &self.builder.cells()[index];
        let changes = self.ordered_changes(index);
        let current = self.builder.read(self.pass, &property.id)?;
        let site = Site {
            pass: self.pass,
            owner: Owner::Sheet(SheetKey {
                group: GROUP_CHANGE,
                index: index as u32,
            }),
            path: format!("card.{}.{}", self.card.id, property.id),
            region: None,
        };
        if changes.is_empty() {
            self.done.insert(index, current.clone());
            return Ok(current);
        }
        let lane_type = super::world::lane_type(property);
        let new_value = if property.property_type == PropertyType::States {
            let mut rules = Vec::new();
            let mut inputs = vec![(
                "state".to_owned(),
                self.builder.input_as(&site, &current, &lane_type, false)?,
            )];
            for change in &changes {
                let ChangeKind::Become { from, to } = &change.kind else {
                    return Err(format!("line {}: a Set changes a States property", change.line));
                };
                let froms: Vec<&str> = match from {
                    Some(from) => vec![from.as_str()],
                    None => property.states.iter().map(|state| state.id.as_str()).collect(),
                };
                let when = self.builder.condition_input(&site, &change.condition)?;
                for state in froms {
                    let rule = if from.is_some() {
                        change.line.to_owned()
                    } else {
                        format!("{}_{state}", change.line)
                    };
                    rules.push(ConfigValue::Table(BTreeMap::from([
                        ("id".to_owned(), text(&rule)),
                        ("from".to_owned(), text(state)),
                        ("to".to_owned(), text(to)),
                        ("phase".to_owned(), ConfigValue::Integer(0)),
                    ])));
                    inputs.push((format!("rule.{rule}.when"), when.clone()));
                }
            }
            if rules.len() > MAX_TRANSITION_RULES {
                return Err(format!(
                    "'{}' changes by {} rows, counting a row for each state of a change from any state, and a \
                     property changes by at most {MAX_TRANSITION_RULES}",
                    property.id,
                    rules.len()
                ));
            }
            let node = self.builder.emit(
                &site,
                "cat.transition",
                vec![("category", text(&property.id)), ("rules", ConfigValue::Array(rules))],
                inputs,
            );
            Operand::Link(
                PortRef {
                    node,
                    port: Some("next".to_owned()),
                },
                lane_type.clone(),
            )
        } else {
            let mut value = current;
            for change in changes.iter().rev() {
                let ChangeKind::Set { value: set } = &change.kind else {
                    return Err(format!(
                        "line {}: a Become changes a property with no states",
                        change.line
                    ));
                };
                let line_site = Site {
                    path: "change".to_owned(),
                    ..self.line_site(change.line)?
                };
                let condition = self.builder.condition_input(&line_site, &change.condition)?;
                let if_true = self.builder.input_as(&line_site, set, &lane_type, true)?;
                let if_false = self.builder.input_as(&line_site, &value, &lane_type, true)?;
                value = self.builder.node_value(
                    &line_site,
                    "logic.select",
                    vec![("cond", condition), ("if_true", if_true), ("if_false", if_false)],
                    lane_type.clone(),
                );
            }
            value
        };
        let written = self.builder.input_as(&site, &new_value, &lane_type, false)?;
        self.builder.write(&site, &property.id, written)?;
        self.done.insert(index, new_value.clone());
        Ok(new_value)
    }

    /// Returns the changes of the property at `index` in row order: display order where two can both apply to one
    /// cell, and line id order where they cannot, as a stable topological sort.
    fn ordered_changes(&self, index: usize) -> Vec<Change<'a>> {
        let changes = self.changes.get(&index).cloned().unwrap_or_default();
        let count = changes.len();
        let mut before: Vec<Vec<usize>> = vec![Vec::new(); count];
        let mut waiting = vec![0usize; count];
        for (upper, first) in changes.iter().enumerate() {
            for (lower, second) in changes.iter().enumerate().skip(upper + 1) {
                if self.relations.can_both_apply(first.line, second.line) {
                    before[upper].push(lower);
                    waiting[lower] += 1;
                }
            }
        }
        let mut placed = vec![false; count];
        let mut ordered = Vec::with_capacity(count);
        while ordered.len() < count {
            let next = (0..count)
                .filter(|&candidate| !placed[candidate] && waiting[candidate] == 0)
                .min_by_key(|&candidate| changes[candidate].number);
            let Some(next) = next else {
                break;
            };
            placed[next] = true;
            for &lower in &before[next] {
                waiting[lower] -= 1;
            }
            ordered.push(changes[next].clone());
        }
        ordered
    }
}
