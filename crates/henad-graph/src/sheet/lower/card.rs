//! Cards: the tick pass of the card that runs each tick, and the lines of a card a button runs once.
//!
//! Lines lower in display order, each into its own block of node ids. A Count and a Look around over the cells lower
//! as [`super::look`] describes, a chance and a random number as [`super::draw`] does, a rate and a competing group as
//! [`super::rates`] does, and a Calculate to one expression node ([`super::calculate`]). A Yes or no line is the
//! comparisons and logic nodes it names, a Choose a `logic.select` chain from its last row up, and a Depending on a
//! `cat.switch` over my state. A When line's condition is joined by `logic.and` into the condition of every change
//! inside it. The changes of each property lower after the card's lines, as the [module](super) describes. On a card of
//! agents, a look at agents within a distance lowers as [`super::agents`] describes, the changes that steer, hold and
//! move as [`super::motion`] does, and a Repeat as [`super::repeat`] does. The lines that pick, look at, move to, mark
//! and record on the ground lower as [`super::ground`] and [`super::pick`] describe, and the looks and picks over one
//! neighbourhood share a fold, as [`super::fuse`] does.

use std::collections::{BTreeMap, HashMap};

use super::agents::AgentLooks;
use super::fuse::CellFolds;
use super::ground::LeaveMark;
use super::ids::{GROUP_CHANGE, Owner, SheetKey, line_number};
use super::look::Fold;
use super::rates::Group;
use super::repeat::RepeatBlock;
use super::value::{Operand, PlaceValue, common_type, shared_type};
use super::{Builder, Site, text};
use crate::document::{Pass, PassStage};
use crate::limits::MAX_TRANSITION_RULES;
use crate::sheet::check::Place;
use crate::sheet::document::{
    Around, CardRuns, CompareTest, LineKind, LineMode, PropertyType, SheetCard, SheetChip, SheetLine, SheetLiteral,
    SheetProperty, SheetValue, Slot,
};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{self, ANY_STATE_ID};
use crate::types::ValueType;
use crate::value::{ConfigValue, Literal, PortRef};

/// Writes the tick pass: the lines of the card that runs each tick over the cells or the agents, or no line when there
/// is none.
///
/// A sheet runs one tick pass, so a second card that runs each tick is refused.
pub(super) fn lower_tick(builder: &mut Builder<'_>) {
    let document = builder.document;
    let thing = builder.thing_id();
    let cards: Vec<usize> = document
        .cards
        .iter()
        .enumerate()
        .filter(|(_, card)| card.every == thing && card.runs == CardRuns::EachTick)
        .map(|(index, _)| index)
        .collect();
    if let Some(&second) = cards.get(1) {
        let words = builder.thing_words();
        builder.problem(
            format!(
                "card '{}' is a second card that runs each tick over {words}, and {words} run one",
                document.cards[second].id
            ),
            Place::Card(second),
        );
    }
    let id = cards.first().map_or_else(
        || format!("tick_{thing}"),
        |&index| format!("tick_{}", document.cards[index].id),
    );
    let pass = builder.add_pass(Pass::new(&id, PassStage::Tick, Some(builder.domain())));
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
        empty: HashMap::new(),
        looks: None,
        repeats: HashMap::new(),
        groups: Vec::new(),
        cell_folds: CellFolds::default(),
        leaves: Vec::new(),
    };
    lowering.run();
}

/// A reason a line cannot lower in this version.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Refusal {
    /// A line kind this version does not lower, with the error in the author's words.
    NotYet(&'static str),
    /// A Record line, since a grid counts no events.
    Record,
    /// A line kind that runs on agents alone.
    Agents(LineKind),
    /// A change of a kind another line of the card already changes the property by, with the error in the author's
    /// words.
    Conflict(String),
}

/// Returns the first reason `line` cannot lower on a card of cells, or of agents with `agents`, its own kind first and
/// then the phrases in its slots.
fn refusal(line: &SheetLine, agents: bool) -> Option<Refusal> {
    let own = if agents {
        None
    } else {
        match line.kind {
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
        }
    };
    own.or_else(|| line.slot_values().find_map(|value| value_refusal(value, agents)))
}

/// Returns the first reason a value cannot lower, searching its phrases and lists.
fn value_refusal(value: &SheetValue, agents: bool) -> Option<Refusal> {
    match value {
        SheetValue::Phrase(phrase) => refusal(phrase, agents),
        SheetValue::List(items) => items.iter().find_map(|item| value_refusal(item, agents)),
        SheetValue::Literal(_) | SheetValue::Chip(_) => None,
    }
}

/// Way a line changes a property, for the rule that a card changes a property one way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Way {
    /// Set.
    Set,
    /// Steer.
    Steer,
    /// Move by an arrow.
    MoveBy,
    /// Move to a place.
    MoveTo,
}

impl Way {
    /// Returns the way `line` changes a property, with the property's id, `None` for a line of another kind.
    fn of(line: &SheetLine) -> Option<(Self, &str)> {
        match line.kind {
            LineKind::Set => Some((Self::Set, line.property.as_deref()?)),
            LineKind::Steer => Some((Self::Steer, line.property.as_deref()?)),
            LineKind::Move => match line.mode? {
                LineMode::To => Some((Self::MoveTo, "")),
                _ => Some((Self::MoveBy, "")),
            },
            _ => None,
        }
    }

    /// Returns the error on a line that changes `property` another way than line `number`.
    ///
    /// Line `number` changes the property this way.
    fn conflict(self, number: usize, property: &str) -> String {
        let prefix = format!("Line {number}");
        match self {
            Self::Set => sentence::set_already(&prefix, property),
            Self::Steer => sentence::steered_already(&prefix, property),
            Self::MoveBy => sentence::moved_by_already(&prefix),
            Self::MoveTo => sentence::moved_to_already(&prefix),
        }
    }

    /// Returns whether a change `self` and a change `other` of one property conflict.
    fn conflicts_with(self, other: Self) -> bool {
        matches!(
            (self, other),
            (Self::Set, Self::Steer)
                | (Self::Steer, Self::Set)
                | (Self::MoveBy, Self::MoveTo)
                | (Self::MoveTo, Self::MoveBy)
        )
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

/// One change of a property: a Become, Set, Steer, Keep between or Move line that runs.
#[derive(Debug, Clone)]
pub(super) struct Change<'a> {
    /// Id of the line.
    pub(super) line: &'a str,
    /// Number `k` of the line id `l<k>`.
    pub(super) number: u32,
    /// Value the line changes the property to.
    pub(super) kind: ChangeKind,
    /// Condition under which the change applies, the conditions of the When lines around it joined in.
    pub(super) condition: Operand,
}

/// Value a change gives its property.
#[derive(Debug, Clone)]
pub(super) enum ChangeKind {
    /// A Become: a state, from one state or, with `None`, from any state.
    Become {
        /// State the line changes from, `None` for any state.
        old_state: Option<String>,
        /// State the line changes to.
        new_state: String,
    },
    /// A Set: a value.
    Set {
        /// Value the line sets.
        value: Operand,
    },
    /// A Steer: a term added to the value at the start of the tick, 0 where the line does not apply.
    Steer {
        /// Term the line adds.
        term: Operand,
    },
    /// A Keep between: the limits the property is held inside after the changes above it.
    Keep {
        /// Lower limit.
        low: Operand,
        /// Upper limit.
        high: Operand,
    },
    /// A Move by: an arrow added to the position, 0 where the line does not apply.
    MoveBy {
        /// Arrow the line moves by.
        arrow: Operand,
    },
    /// A Move to: the place the line moves to.
    MoveTo {
        /// The place, whose open test decides whether the move applies.
        place: PlaceValue,
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
    pub(super) results: HashMap<&'a str, Operand>,
    /// Each part of the result of a Look around line lowered so far, by line id and part.
    pub(super) parts: HashMap<(&'a str, &'a str), Operand>,
    /// Condition of each When line lowered so far, the conditions of the When lines around it joined in, by line id.
    pub(super) conditions: HashMap<&'a str, Operand>,
    /// Changes of each property, by the property's index among the cells properties, in display order.
    pub(super) changes: BTreeMap<usize, Vec<Change<'a>>>,
    /// New value of each property whose changes are written, by the property's index.
    pub(super) done: HashMap<usize, Operand>,
    /// The folds over the neighbours around the value being lowered, innermost last.
    pub(super) folds: Vec<Fold>,
    /// Count of each part of a Look around that is empty when the look finds no one, by line id and part.
    pub(super) empty: HashMap<(&'a str, &'a str), Operand>,
    /// The folds of the card's looks at agents, written at the first such look.
    pub(super) looks: Option<AgentLooks<'a>>,
    /// Each Repeat line lowered so far, by line id.
    pub(super) repeats: HashMap<&'a str, RepeatBlock>,
    /// Competing groups of the card whose lines run, each of two lines or more.
    pub(super) groups: Vec<Group<'a>>,
    /// The folds over the cells around that the card's looks and picks share.
    pub(super) cell_folds: CellFolds,
    /// The Leave lines lowered so far, in display order.
    pub(super) leaves: Vec<LeaveMark<'a>>,
}

impl<'a> CardLowering<'_, 'a> {
    /// Lowers every line of the card in display order, then the changes of each property.
    fn run(&mut self) {
        let order: Vec<&'a str> = self.relations.order().to_vec();
        let agents = self.builder.is_agents();
        let mut refused = false;
        let mut ways: Vec<(Way, &'a str, usize)> = Vec::new();
        for &id in &order {
            let Some(line) = self.relations.line(id) else {
                continue;
            };
            let number = self.relations.number(id).unwrap_or(0);
            let conflict = Way::of(line).and_then(|(way, property)| {
                let first = ways
                    .iter()
                    .find(|(known, changed, _)| *changed == property && way.conflicts_with(*known))
                    .map(|(known, _, at)| {
                        known.conflict(
                            *at,
                            &self
                                .builder
                                .property(property)
                                .map_or_else(|| property.to_owned(), |(_, found)| found.name.clone()),
                        )
                    });
                ways.push((way, property, number));
                first.map(Refusal::Conflict)
            });
            let in_repeat = agents && self.repeat_of(id).is_some();
            let reason = conflict
                .or_else(|| {
                    (in_repeat
                        && (matches!(
                            line.kind,
                            LineKind::KeepBetween
                                | LineKind::When
                                | LineKind::Repeat
                                | LineKind::Leave
                                | LineKind::Record
                        ) || (line.kind == LineKind::Move && line.mode == Some(LineMode::To))))
                    .then_some(Refusal::NotYet(sentence::NOT_YET_REPEAT_CHANGE))
                })
                .or_else(|| refusal(line, agents));
            if let Some(reason) = reason {
                refused = true;
                let message = match reason {
                    Refusal::NotYet(words) => words.to_owned(),
                    Refusal::Conflict(words) => words,
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
        let report = self.builder.report;
        self.groups = self
            .relations
            .competing_groups()
            .into_iter()
            .map(|lines| {
                lines
                    .into_iter()
                    .filter(|id| !report.is_skipped(id))
                    .collect::<Vec<_>>()
            })
            .filter(|lines| lines.len() > 1)
            .map(Group::new)
            .collect();
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
        self.finish_looks();
        self.finish_cell_folds();
        let pending: Vec<usize> = self.changes.keys().copied().collect();
        for index in pending {
            if !self.done.contains_key(&index)
                && let Err(message) = self.write_changes(index)
            {
                self.builder.problem(message, Place::Card(self.card_index));
            }
        }
        if let Err(message) = self.write_leaves() {
            self.builder.problem(message, Place::Card(self.card_index));
        }
    }

    /// Returns the site of the nodes of line `id`, inside the region of the Repeat that holds it.
    pub(super) fn line_site(&self, id: &str) -> Result<Site, String> {
        let number = line_number(id).ok_or_else(|| format!("'{id}' is no line id"))?;
        Ok(Site {
            pass: self.pass,
            owner: Owner::Line {
                id: id.to_owned(),
                number,
            },
            path: String::new(),
            region: self
                .repeat_of(id)
                .and_then(|repeat| self.repeats.get(repeat))
                .map(|block| block.node),
        })
    }

    /// Returns the id of the Repeat line that holds line `id`, at any depth.
    pub(super) fn repeat_of(&self, id: &str) -> Option<&'a str> {
        let mut parent = self.relations.parent(id);
        while let Some(block) = parent {
            if self
                .relations
                .line(block)
                .is_some_and(|line| line.kind == LineKind::Repeat)
            {
                return Some(block);
            }
            parent = self.relations.parent(block);
        }
        None
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
            | LineKind::Rate
            | LineKind::RandomNumber
            | LineKind::Calculate
            | LineKind::Choose
            | LineKind::DependingOn
            | LineKind::RandomDirection
            | LineKind::PickCell
            | LineKind::LookAtCell => {
                let result = match line.kind {
                    LineKind::Count if line.around == Some(Around::AgentsWithin) => {
                        self.count_agents(&site, id, line)?
                    }
                    _ => self.phrase(&site, line)?,
                };
                let output = match &result {
                    Operand::Place(place) => &place.xy,
                    other => other,
                };
                if let Operand::Link(port, _) = output {
                    self.builder.record_result(port.node, id);
                }
                self.results.insert(id, result);
            }
            LineKind::Move if line.mode == Some(LineMode::To) => self.move_to(&site, id, line)?,
            LineKind::Leave => self.leave(&site, id, line)?,
            LineKind::Record => self.record(&site, id, line)?,
            LineKind::LookAround if line.around == Some(Around::AgentsWithin) => {
                self.look_around_agents(&site, id, line)?;
            }
            LineKind::LookAround => self.look_around(&site, id, line)?,
            LineKind::Repeat => self.repeat(&site, id, line)?,
            LineKind::Steer => self.steer(&site, id, line)?,
            LineKind::KeepBetween => self.keep_between(&site, id, line)?,
            LineKind::Move => self.move_by(&site, id, line)?,
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
                let new_state = line
                    .new_state
                    .clone()
                    .ok_or_else(|| "a Become needs a state".to_owned())?;
                let old_state = match line.start_state.as_deref() {
                    Some(ANY_STATE_ID) => None,
                    Some(state) => Some(state.to_owned()),
                    None => return Err("a Become needs the state it changes from".to_owned()),
                };
                if let Some(group) = self.group_of(id) {
                    let placeholder = self.group_line(group, &site, id, line)?;
                    let kind = ChangeKind::Become { old_state, new_state };
                    self.add_change(index, id, kind, placeholder)?;
                    self.finish_group(group)?;
                } else {
                    let own = self.slot(&site.at("condition"), &line.condition)?;
                    let condition = self.inherit(&site, id, own)?;
                    let condition = self.any_time(&site, id, condition)?;
                    let kind = ChangeKind::Become { old_state, new_state };
                    self.add_change(index, id, kind, condition)?;
                }
            }
            LineKind::Set => {
                let (index, _) = self.property(line.property.as_deref())?;
                let value_site = Site {
                    region: None,
                    ..site.at("value")
                };
                let value = self.slot(&value_site, &line.value)?;
                if let Some(repeat) = self.repeat_of(id)
                    && self.reads_inside(&line.value, repeat)
                {
                    return Err(sentence::SET_IN_REPEAT.to_owned());
                }
                let own = self.slot(&site.at("condition"), &line.condition)?;
                let condition = self.inherit(&site, id, own)?;
                let condition = self.any_time(&site, id, condition)?;
                if !self.overridden_set(id) {
                    self.fires_when(id, &condition);
                }
                self.add_change(index, id, ChangeKind::Set { value }, condition)?;
            }
            other => return Err(format!("a {} line has no lowering", other.as_str())),
        }
        Ok(())
    }

    /// Returns whether a Set line above the Set line `id` changes the same property and can apply to an element that
    /// line `id` applies to. Line `id` then reads "otherwise", and applies only where its condition holds and the
    /// higher line's does not.
    fn overridden_set(&self, id: &str) -> bool {
        self.relations
            .otherwise_of(id)
            .iter()
            .any(|higher| !self.builder.report.is_skipped(higher))
    }

    /// Records `condition`, the condition under which line `id` changes an element, as the line's fire when it is the
    /// output of a node. A condition that always holds has no node and gives the line no count.
    ///
    /// A line inside a Repeat has no count, wherever its condition is lowered. The Repeat may run the line several
    /// times in one tick, or not at all.
    pub(super) fn fires_when(&mut self, id: &str, condition: &Operand) {
        if self.repeat_of(id).is_some() {
            return;
        }
        if let Operand::Link(port, ValueType::Bool) = condition {
            self.builder.record_fire(port.node, port.port.clone(), id);
        }
    }

    /// Returns the property `id` of the card's kind of thing with its index.
    pub(super) fn property(&self, id: Option<&str>) -> Result<(usize, &'a SheetProperty), String> {
        let id = id.ok_or_else(|| "the line names no property".to_owned())?;
        self.builder
            .property(id)
            .ok_or_else(|| format!("'{id}' is no property of {}", self.builder.thing_words()))
    }

    /// Records a change of the property at `index` by line `id`.
    pub(super) fn add_change(
        &mut self,
        index: usize,
        id: &'a str,
        kind: ChangeKind,
        condition: Operand,
    ) -> Result<(), String> {
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
    pub(super) fn inherit(&mut self, site: &Site, id: &str, own: Operand) -> Result<Operand, String> {
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
            } => {
                if let Some(found) = self.parts.get(&(line.as_str(), part.as_str())) {
                    return Ok(found.clone());
                }
                match self.results.get(line.as_str()).cloned() {
                    Some(Operand::Place(place)) => self.place_part(site, line, &place, part),
                    _ => Err(format!("line {line} has no part '{part}' to read")),
                }
            }
            SheetChip::Result {
                line,
                property: Some(property),
                ..
            } => match self.results.get(line.as_str()).cloned() {
                Some(Operand::Place(place)) => self.place_property(site, &place, property),
                _ => Err(format!("line {line} names no place, so it has no properties")),
            },
            SheetChip::My(property) => self.builder.read(self.pass, property),
            SheetChip::MyNew(property) => {
                let (index, _) = self.property(Some(property))?;
                match self.done.get(&index) {
                    Some(value) => Ok(value.clone()),
                    None => self.write_changes(index),
                }
            }
            SheetChip::Their(property) => self.their(site, property),
            SheetChip::Relation(relation) => self.relation(site, *relation),
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
            LineKind::Count if line.around == Some(Around::AgentsWithin) => {
                Err("a count of agents within a distance stands on a line of its own".to_owned())
            }
            LineKind::Count => self.count(site, line),
            LineKind::Chance => self.chance(site, line),
            LineKind::Rate => self.rate(site, line),
            LineKind::RandomDirection => self.random_direction(site, line),
            LineKind::RandomNumber => self.random_number(site, line),
            LineKind::Calculate => self.calculate(site, line),
            LineKind::Choose => self.choose(site, line),
            LineKind::DependingOn => self.depending_on(site, line),
            LineKind::PickCell => self.pick_cell(site, line),
            LineKind::LookAtCell => self.look_at_cell(site, line),
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
        if let Operand::Place(otherwise) = &otherwise {
            let mut places = Vec::with_capacity(rows.len());
            for (row_site, value, condition) in rows {
                let Operand::Place(place) = value else {
                    return Err("a choice of places holds a value that is no place".to_owned());
                };
                let condition = self.builder.condition_input(&row_site, &condition)?;
                places.push((row_site, *place, condition));
            }
            return self.choose_place(places, (**otherwise).clone());
        }
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
        if branches.iter().all(|(_, value)| matches!(value, Operand::Place(_))) && !branches.is_empty() {
            return self.depending_on_place(site, property, &branches);
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

    /// Returns the place a Depending on gives from the place `branches` of each state of `property`: one `cat.switch`
    /// over my state for the column and row, and one for the amount when every place has one.
    fn depending_on_place(
        &mut self,
        site: &Site,
        property: &SheetProperty,
        branches: &[(&str, Operand)],
    ) -> Result<Operand, String> {
        let category = ValueType::Category(property.id.clone());
        let current = self.builder.read(self.pass, &property.id)?;
        let select = self.builder.input_as(site, &current, &category, false)?;
        let places: Vec<(&str, &PlaceValue)> = branches
            .iter()
            .filter_map(|(state, value)| match value {
                Operand::Place(place) => Some((*state, &**place)),
                _ => None,
            })
            .collect();
        let switch = |lowering: &mut Self, part: &dyn Fn(&PlaceValue) -> Option<Operand>, value_type: ValueType| {
            let mut inputs = vec![("select".to_owned(), select.clone())];
            for (state, place) in &places {
                let Some(value) = part(place) else {
                    return Ok(None);
                };
                inputs.push((
                    format!("case.{state}"),
                    lowering.builder.input_as(site, &value, &value_type, false)?,
                ));
            }
            let node = lowering
                .builder
                .emit(site, "cat.switch", vec![("category", text(&property.id))], inputs);
            Ok::<_, String>(Some(Operand::Link(PortRef { node, port: None }, value_type)))
        };
        let xy = switch(self, &|place| Some(place.xy.clone()), ValueType::Vec2I)?
            .ok_or_else(|| "a choice of places has no place".to_owned())?;
        let amount = switch(self, &|place| place.amount.clone(), ValueType::F32)?;
        Ok(Operand::Place(Box::new(PlaceValue { xy, amount, open: None })))
    }

    /// Writes the changes of the property at `index` and its new value, and returns that value.
    pub(super) fn write_changes(&mut self, index: usize) -> Result<Operand, String> {
        let property = &self.builder.properties()[index];
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
        let lane_type = self.builder.lane_type(property);
        if property.property_type == PropertyType::Position {
            let new_value = if changes
                .iter()
                .any(|change| matches!(change.kind, ChangeKind::MoveTo { .. }))
            {
                self.write_moves(&site, &current, &changes)?
            } else {
                self.write_position(&site, &current, &changes)?
            };
            self.done.insert(index, new_value.clone());
            return Ok(new_value);
        }
        let new_value = if property.property_type == PropertyType::States {
            self.transition(&site, property, &current, &changes)?
        } else {
            let mut value = current.clone();
            for change in changes.iter().rev() {
                let set = match &change.kind {
                    ChangeKind::Set { value: set } => set,
                    ChangeKind::Become { .. } => {
                        return Err(format!(
                            "line {}: a Become changes a property with no states",
                            change.line
                        ));
                    }
                    _ => continue,
                };
                let line_site = self.change_site(change.line)?;
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
            self.steered(value, &changes, &lane_type)?
        };
        let written = self.builder.input_as(&site, &new_value, &lane_type, false)?;
        self.builder.write(&site, &property.id, written)?;
        self.done.insert(index, new_value.clone());
        Ok(new_value)
    }

    /// Returns the next state of the States property `property` after its Becomes `changes`, through one
    /// `cat.transition` written at `site` over `current`.
    fn transition(
        &mut self,
        site: &Site,
        property: &SheetProperty,
        current: &Operand,
        changes: &[Change<'a>],
    ) -> Result<Operand, String> {
        let lane_type = self.builder.lane_type(property);
        let mut rules = Vec::new();
        let mut fired = Vec::new();
        let mut inputs = vec![(
            "state".to_owned(),
            self.builder.input_as(site, current, &lane_type, false)?,
        )];
        for change in changes {
            let ChangeKind::Become { old_state, new_state } = &change.kind else {
                return Err(format!("line {}: a Set changes a States property", change.line));
            };
            let old_states: Vec<&str> = match old_state {
                Some(old_state) => vec![old_state.as_str()],
                None => property.states.iter().map(|state| state.id.as_str()).collect(),
            };
            let when = self.builder.condition_input(site, &change.condition)?;
            for state in old_states {
                let rule = if old_state.is_some() {
                    change.line.to_owned()
                } else {
                    format!("{}_{state}", change.line)
                };
                fired.push((format!("fired.{rule}"), change.line));
                rules.push(ConfigValue::Table(BTreeMap::from([
                    ("id".to_owned(), text(&rule)),
                    ("from".to_owned(), text(state)),
                    ("to".to_owned(), text(new_state)),
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
            site,
            "cat.transition",
            vec![("category", text(&property.id)), ("rules", ConfigValue::Array(rules))],
            inputs,
        );
        for (port, line) in fired {
            self.builder.record_fire(node, Some(port), line);
        }
        Ok(Operand::Link(
            PortRef {
                node,
                port: Some("next".to_owned()),
            },
            lane_type,
        ))
    }

    /// Returns the site of the nodes line `id` writes for its share of a change, outside every region.
    pub(super) fn change_site(&self, id: &str) -> Result<Site, String> {
        Ok(Site {
            path: "change".to_owned(),
            region: None,
            ..self.line_site(id)?
        })
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
