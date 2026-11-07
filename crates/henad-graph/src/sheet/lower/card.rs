//! Cards: the tick pass of the card that runs each tick, and the lines of a card a button runs once.
//!
//! Lines lower in display order, each into its own block of node ids. A Count over cells is a `grid.count` when it
//! counts neighbours in some states of a States property, and a `grid.fold` with a `count` accumulator otherwise. A
//! Yes or no line is the comparisons and logic nodes it names. A When line's condition is joined by `logic.and` into
//! the condition of every change inside it. The changes of each property lower after the card's lines, as the
//! [module](super) describes.

use std::collections::{BTreeMap, HashMap};

use super::ids::{GROUP_CHANGE, Owner, SheetKey, line_number};
use super::value::{Operand, common_type};
use super::{Builder, Site, text};
use crate::document::{Domain, Pass, PassStage};
use crate::ids::NodeId;
use crate::limits::MAX_TRANSITION_RULES;
use crate::sheet::check::Place;
use crate::sheet::document::{
    Around, CardRuns, CompareTest, LineKind, PropertyType, SheetCard, SheetChip, SheetLine, SheetLiteral, SheetValue,
    Slot,
};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{self, ANY_STATE_ID};
use crate::types::ValueType;
use crate::value::{ConfigValue, InputValue, Literal, PortRef};

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
        LineKind::Chance => Some(Refusal::NotYet(sentence::NOT_YET_CHANCE)),
        LineKind::Rate => Some(Refusal::NotYet(sentence::NOT_YET_RATE)),
        LineKind::LookAround => Some(Refusal::NotYet(sentence::NOT_YET_LOOK_AROUND)),
        LineKind::Calculate => Some(Refusal::NotYet(sentence::NOT_YET_CALCULATE)),
        LineKind::Choose => Some(Refusal::NotYet(sentence::NOT_YET_CHOOSE)),
        LineKind::DependingOn => Some(Refusal::NotYet(sentence::NOT_YET_DEPENDING_ON)),
        LineKind::RandomNumber => Some(Refusal::NotYet(sentence::NOT_YET_RANDOM_NUMBER)),
        LineKind::Repeat => Some(Refusal::NotYet(sentence::NOT_YET_REPEAT)),
        LineKind::Record => Some(Refusal::Record),
        kind @ (LineKind::PickCell
        | LineKind::LookAtCell
        | LineKind::RandomDirection
        | LineKind::Steer
        | LineKind::KeepBetween
        | LineKind::Move
        | LineKind::Leave) => Some(Refusal::Agents(kind)),
        LineKind::Count if line.around == Some(Around::AgentsWithin) => Some(Refusal::Agents(LineKind::Count)),
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

/// A Count's `grid.fold` that `their` chips read inside.
#[derive(Debug)]
struct Fold {
    /// Temporary id of the fold.
    node: NodeId,
    /// Read of each property of the neighbour, by property id.
    reads: HashMap<String, Operand>,
}

/// State of the lowering of one card.
struct CardLowering<'b, 'a> {
    /// The lowering.
    builder: &'b mut Builder<'a>,
    /// Index of the card in the sheet's cards.
    card_index: usize,
    /// The card.
    card: &'a SheetCard,
    /// Relations between the card's lines.
    relations: CardRelations<'a>,
    /// Index of the pass the card writes.
    pass: usize,
    /// Result of each line lowered so far, by line id.
    results: HashMap<&'a str, Operand>,
    /// Condition of each When line lowered so far, the conditions of the When lines around it joined in, by line id.
    conditions: HashMap<&'a str, Operand>,
    /// Changes of each property, by the property's index among the cells properties, in display order.
    changes: BTreeMap<usize, Vec<Change<'a>>>,
    /// New value of each property whose changes are written, by the property's index.
    done: HashMap<usize, Operand>,
    /// The Count folds around the value being lowered, innermost last.
    folds: Vec<Fold>,
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
            LineKind::Count | LineKind::Compare | LineKind::AllOf | LineKind::AnyOf | LineKind::NoneOf => {
                let result = self.phrase(&site, line)?;
                self.results.insert(id, result);
            }
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
    fn property(&self, id: Option<&str>) -> Result<(usize, &'a crate::sheet::document::SheetProperty), String> {
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
    fn slot(&mut self, site: &Site, slot: &'a Slot) -> Result<Operand, String> {
        let value = slot.value().ok_or_else(|| "a slot is blank".to_owned())?;
        self.value(site, value)
    }

    /// Returns the operand of `value` lowered at `site`.
    fn value(&mut self, site: &Site, value: &'a SheetValue) -> Result<Operand, String> {
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
    fn chip(&mut self, site: &Site, chip: &'a SheetChip) -> Result<Operand, String> {
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
            SheetChip::Result { line, .. } => Err(format!("line {line} has no parts or places to read")),
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

    /// Returns the read of the neighbour's `property` in the innermost Count fold.
    fn their(&mut self, site: &Site, property: &str) -> Result<Operand, String> {
        let fold = self
            .folds
            .last()
            .ok_or_else(|| "their stands outside a Count".to_owned())?;
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
            other => Err(format!("a {} phrase has no lowering", other.as_str())),
        }
    }

    /// Returns the items of an all of, any of or none of joined by `op`.
    fn logic(&mut self, site: &Site, line: &'a SheetLine, op: &str) -> Result<Operand, String> {
        let mut joined = Operand::Constant(Literal::Bool(op == "logic.and"));
        for (index, item) in line.items.iter().enumerate() {
            let item_site = site.at(&format!("items.{}", index + 1));
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
                    let item_site = site.at(&format!("right.{}", index + 1));
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

    /// Returns the count of a Count line or phrase over the cells around.
    fn count(&mut self, site: &Site, line: &'a SheetLine) -> Result<Operand, String> {
        let around = line
            .around
            .ok_or_else(|| "a Count needs the cells it looks at".to_owned())?;
        let (moore, include_center) = match around {
            Around::Cells8 => (true, false),
            Around::Cells4 => (false, false),
            Around::Block3x3 => (true, true),
            Around::AgentsWithin => return Err("agents within a distance are no cells".to_owned()),
        };
        let whose = line.whose.value().ok_or_else(|| "a Count needs a test".to_owned())?;
        let site = site.at("whose");
        if let Some((property, states)) = self.counted_states(whose) {
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
                            InputValue::Literal(Literal::Category(crate::value::MemberName {
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
        let accumulator = ConfigValue::Table(BTreeMap::from([
            ("name".to_owned(), text("count")),
            ("type".to_owned(), text("u32")),
            ("combine".to_owned(), text("count")),
        ]));
        let fold = self.builder.emit(
            &site,
            "grid.fold",
            vec![
                ("order", text(if moore { "moore_row_major" } else { "von_neumann" })),
                ("include_center", ConfigValue::Bool(include_center)),
                ("boundary", text("domain")),
                ("acc", ConfigValue::Array(vec![accumulator])),
            ],
            vec![("when".to_owned(), InputValue::Literal(Literal::Bool(true)))],
        );
        self.folds.push(Fold {
            node: fold,
            reads: HashMap::new(),
        });
        let inside = site.inside(fold);
        let test = self.value(&inside, whose);
        self.folds.pop();
        let test = test?;
        let when = self.builder.condition_input(&inside, &test)?;
        self.builder.emit(
            &inside,
            "region.put",
            vec![("acc", text("count"))],
            vec![("when".to_owned(), when)],
        );
        Ok(Operand::Link(
            PortRef {
                node: fold,
                port: Some("count".to_owned()),
            },
            ValueType::U32,
        ))
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
