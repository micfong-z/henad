//! Relations between the lines of one card: display order, the blocks that hold each line, the "otherwise" rule and
//! groups of competing rate lines.
//!
//! - The *display order* of a card lists its lines from the top, the lines inside a When or Repeat right after the
//!   block line. A line's *number* is its place in that order, counted from 1.
//! - A *change* is a Become or a Set line. Two changes *collide* when both are Become or both are Set, they change
//!   the same property, both run on one element of the card, and for Become both name the state they change from.
//! - Two changes *can both apply* unless some value is tested by both in ways no value passes together. The tests a
//!   change needs are its own condition, the conditions of the When lines around it, and for Become the state it
//!   changes from. An all of contributes each of its items, nested all of included, and any of, none of, a result
//!   chip and a draw contribute nothing.
//! - A change reads *otherwise* when a higher change collides with it. The higher lines are its partners.
//! - A *competing group* is two or more Become lines of one property from one named state, each drawing a rate and no
//!   chance in its own condition and none of them inside a Repeat. The rate stands as the condition or as an item of
//!   an all of, nested all of included. Its lines compete for the cells in that state and read "or" against each other
//!   instead of "otherwise".
//!
//! - A *relation note* is a note under a line that the relations between the card's lines decide: the gap, coverage
//!   and order notes of Become lines, the offers to fold a When into its lines, and the notes inside and under a When.
//!
//! The sheet checker, the writer's comments and the card's notes take these relations from here.

use std::collections::HashMap;

use super::check::is_result_kind;
use super::document::{
    Around, CompareTest, LineKind, PropertyType, SheetCard, SheetChip, SheetDocument, SheetLine, SheetLiteral,
    SheetProperty, SheetValue, Slot, SummaryKind,
};
use super::edit::automatic_name;
use super::sentence::{
    ANY_STATE_ID, NOTHING_INSIDE, Sentences, coverage_note, gap_note, only_one_inside_note, only_when_note, order_note,
    when_repeats_note, worked_out_everywhere_note,
};

/// Allowance of the comparisons of the "otherwise" rule, so that two tests of one listed number agree on it.
const NEAR: f64 = 1e-9;

/// Relations between the lines of one card.
///
/// ```
/// use henad_graph::sheet::ParsedSheet;
/// use henad_graph::sheet::relations::CardRelations;
///
/// let sheet = ParsedSheet::parse(henad_graph::sheet::starters()[0].text).expect("the starter reads");
/// let document = sheet.document();
/// let relations = CardRelations::new(document, &document.cards[0]);
/// assert_eq!(relations.order(), ["l1", "l2", "l3"]);
/// // Become Alive from Dead and Become Dead from Alive change different cells.
/// assert!(relations.otherwise_of("l3").is_empty());
/// ```
pub struct CardRelations<'a> {
    /// The sheet the card belongs to.
    document: &'a SheetDocument,
    /// The card.
    card: &'a SheetCard,
    /// Ids of the card's lines in display order. An id listed with no line table is left out.
    order: Vec<&'a str>,
    /// Block line that holds each line inside a When or Repeat, by line id.
    parents: HashMap<&'a str, &'a str>,
    /// Index of each line in [`Self::order`], by line id.
    indices: HashMap<&'a str, usize>,
}

/// Prints the card's id and its number of lines.
impl std::fmt::Debug for CardRelations<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CardRelations")
            .field("card", &self.card.id)
            .field("lines", &self.order.len())
            .finish_non_exhaustive()
    }
}

impl<'a> CardRelations<'a> {
    /// Creates the relations between the lines of `card`, a card of `document`.
    ///
    /// A line listed twice, or inside its own block, is taken at its first place in display order.
    pub fn new(document: &'a SheetDocument, card: &'a SheetCard) -> Self {
        let mut order = Vec::new();
        let mut parents = HashMap::new();
        let mut indices = HashMap::new();
        let mut stack: Vec<(&str, Option<&str>)> = card.lines.iter().rev().map(|id| (id.as_str(), None)).collect();
        while let Some((id, parent)) = stack.pop() {
            let Some(line) = card.line_tables.get(id) else {
                continue;
            };
            if indices.contains_key(id) {
                continue;
            }
            indices.insert(id, order.len());
            order.push(id);
            if let Some(parent) = parent {
                parents.insert(id, parent);
            }
            stack.extend(line.lines.iter().rev().map(|inner| (inner.as_str(), Some(id))));
        }
        Self {
            document,
            card,
            order,
            parents,
            indices,
        }
    }

    /// The card the relations are of.
    pub fn card(&self) -> &'a SheetCard {
        self.card
    }

    /// Ids of the card's lines in display order, the lines inside a When or Repeat right after the block line.
    pub fn order(&self) -> &[&'a str] {
        &self.order
    }

    /// Returns the index of line `line_id` in [`Self::order`], `None` for a line the card does not list.
    pub fn index(&self, line_id: &str) -> Option<usize> {
        self.indices.get(line_id).copied()
    }

    /// Returns the number of line `line_id` as the card shows it, counted from 1 in display order.
    pub fn number(&self, line_id: &str) -> Option<usize> {
        self.index(line_id).map(|index| index + 1)
    }

    /// Returns the line `line_id` of the card.
    pub fn line(&self, line_id: &str) -> Option<&'a SheetLine> {
        self.card.line_tables.get(line_id)
    }

    /// Returns the When or Repeat line that holds line `line_id`, `None` for a line at the top of the card.
    pub fn parent(&self, line_id: &str) -> Option<&'a str> {
        self.parents.get(line_id).copied()
    }

    /// Returns whether line `line_id` sits inside the block line `block_id`, directly or inside a block within it.
    pub fn is_within(&self, line_id: &str, block_id: &str) -> bool {
        let mut parent = self.parent(line_id);
        while let Some(id) = parent {
            if id == block_id {
                return true;
            }
            parent = self.parent(id);
        }
        false
    }

    /// Returns whether lines `first` and `second` can both apply to one element: whether no value is tested by both
    /// in ways no value passes together.
    ///
    /// A line the card does not hold can apply to anything.
    pub fn can_both_apply(&self, first: &str, second: &str) -> bool {
        let first_tests = self.tests_of(first);
        let second_tests = self.tests_of(second);
        !first_tests.iter().any(|one| {
            second_tests
                .iter()
                .any(|other| one.operand == other.operand && one.excludes(other))
        })
    }

    /// Returns the higher lines that change the property of line `line_id` and collide with it, in display order,
    /// empty for a line that reads no "otherwise".
    ///
    /// The lines of a competing group read "or" against each other, so a partner in the line's own group is left out.
    pub fn otherwise_of(&self, line_id: &str) -> Vec<&'a str> {
        let Some(index) = self.index(line_id) else {
            return Vec::new();
        };
        let Some(line) = self.line(line_id) else {
            return Vec::new();
        };
        if change_key(line).is_none() {
            return Vec::new();
        }
        let group = self.group_key(line_id);
        self.order[..index]
            .iter()
            .copied()
            .filter(|higher| {
                self.line(higher).is_some_and(|other| {
                    change_key(other) == change_key(line)
                        && !(group.is_some() && self.group_key(higher) == group)
                        && self.can_both_apply(higher, line_id)
                })
            })
            .collect()
    }

    /// Returns the numbers of the lines of [`Self::otherwise_of`], as the "otherwise" words show them.
    pub fn otherwise_numbers(&self, line_id: &str) -> Vec<usize> {
        self.otherwise_of(line_id)
            .into_iter()
            .filter_map(|higher| self.number(higher))
            .collect()
    }

    /// Returns the competing groups of the card, each its line ids in display order, the groups ordered by their
    /// first line.
    pub fn competing_groups(&self) -> Vec<Vec<&'a str>> {
        let mut groups: Vec<((&str, &str), Vec<&'a str>)> = Vec::new();
        for &id in &self.order {
            let Some(key) = self.group_key(id) else {
                continue;
            };
            match groups.iter_mut().find(|(other, _)| *other == key) {
                Some((_, lines)) => lines.push(id),
                None => groups.push((key, vec![id])),
            }
        }
        groups
            .into_iter()
            .filter(|(_, lines)| lines.len() > 1)
            .map(|(_, lines)| lines)
            .collect()
    }

    /// Returns the property and the start state of the competing group that line `line_id` can join, `None` for a line
    /// that joins no group.
    ///
    /// A line inside a Repeat joins no group and draws alone each time the Repeat runs it.
    fn group_key(&self, line_id: &str) -> Option<(&'a str, &'a str)> {
        if self.repeat_of(line_id).is_some() {
            return None;
        }
        competing_key(self.line(line_id)?)
    }

    /// Returns the Repeat line that holds line `line_id`, directly or inside a block within it, `None` for a line
    /// outside every Repeat.
    pub fn repeat_of(&self, line_id: &str) -> Option<&'a str> {
        let mut parent = self.parent(line_id);
        while let Some(id) = parent {
            if self.line(id).is_some_and(|block| block.kind == LineKind::Repeat) {
                return Some(id);
            }
            parent = self.parent(id);
        }
        None
    }

    /// Returns every test of one value that line `line_id` needs to apply: its own condition, the conditions of the
    /// When lines around it, and for Become a named state it changes from.
    fn tests_of(&self, line_id: &str) -> Vec<ValueTest> {
        let mut tests = Vec::new();
        let Some(line) = self.line(line_id) else {
            return tests;
        };
        self.collect_tests(&line.condition, &mut tests);
        let mut parent = self.parent(line_id);
        while let Some(id) = parent {
            if let Some(block) = self.line(id).filter(|block| block.kind == LineKind::When) {
                self.collect_tests(&block.condition, &mut tests);
            }
            parent = self.parent(id);
        }
        if line.kind == LineKind::Become
            && let (Some(property), Some(state)) = (line.property.as_deref(), line.start_state.as_deref())
            && let Some(value) = self.state_index(property, state)
        {
            tests.push(ValueTest {
                operand: Operand::My(property.to_owned()),
                test: Test::Is(value),
                values: vec![value],
                whole: true,
            });
        }
        tests
    }

    /// Adds the tests of one value that the condition in `slot` holds to `tests`.
    fn collect_tests(&self, slot: &Slot, tests: &mut Vec<ValueTest>) {
        let Some(SheetValue::Phrase(phrase)) = slot.value() else {
            return;
        };
        match phrase.kind {
            LineKind::AllOf => {
                for item in &phrase.items {
                    self.collect_tests(&Slot::Filled(item.clone()), tests);
                }
            }
            LineKind::Compare => tests.extend(self.compare_test(phrase)),
            _ => {}
        }
    }

    /// Returns the compare phrase `phrase` as a test of one value, `None` when it reads anything but fixed values of
    /// a property of mine or a result.
    ///
    /// A result of a line inside a Repeat gives no test. The Repeat works it out again each time round, so two
    /// changes can each hold at a different time and both apply.
    fn compare_test(&self, phrase: &SheetLine) -> Option<ValueTest> {
        let operand = match phrase.left.value()? {
            SheetValue::Chip(SheetChip::My(property)) => Operand::My(property.clone()),
            SheetValue::Chip(SheetChip::Result { line, .. }) if self.repeat_of(line).is_some() => return None,
            SheetValue::Chip(SheetChip::Result { line, part, property }) => Operand::Result {
                line: line.clone(),
                part: part.clone(),
                property: property.clone(),
            },
            _ => return None,
        };
        let about = match &operand {
            Operand::My(property) => Some(property.as_str()),
            Operand::Result { property, .. } => property.as_deref(),
        };
        let whole = match &operand {
            Operand::My(property) => self
                .property(property)
                .is_some_and(|found| found.property_type == PropertyType::States),
            Operand::Result {
                line,
                part: None,
                property: None,
            } => self
                .document
                .line(line)
                .is_some_and(|(_, source)| source.kind == LineKind::Count),
            Operand::Result {
                line,
                part: Some(part),
                property: None,
            } => self.document.line(line).is_some_and(|(_, source)| {
                source.kind == LineKind::LookAround
                    && (part == "count"
                        || source
                            .summaries
                            .iter()
                            .any(|summary| summary.id == *part && summary.summary == Some(SummaryKind::Count)))
            }),
            Operand::Result {
                property: Some(property),
                ..
            } => self
                .property(property)
                .is_some_and(|found| found.property_type == PropertyType::States),
        };
        let fixed = |slot: &Slot| self.fixed_number(slot.value()?, about);
        let test = phrase.test?;
        let (test, values) = match test {
            CompareTest::Outside => {
                let (low, high) = (fixed(&phrase.low)?, fixed(&phrase.high)?);
                (Test::Outside(low, high), vec![low, high])
            }
            CompareTest::OneOf | CompareTest::NotOneOf => {
                let Some(SheetValue::List(items)) = phrase.right.value() else {
                    return None;
                };
                let values = items
                    .iter()
                    .map(|item| self.fixed_number(item, about))
                    .collect::<Option<Vec<f64>>>()?;
                if values.is_empty() {
                    return None;
                }
                let test = if test == CompareTest::OneOf {
                    Test::OneOf(values.clone())
                } else {
                    Test::NotOneOf(values.clone())
                };
                (test, values)
            }
            CompareTest::Within => return None,
            _ => {
                let value = fixed(&phrase.right)?;
                let test = match test {
                    CompareTest::Is => Test::Is(value),
                    CompareTest::IsNot => Test::IsNot(value),
                    CompareTest::AtLeast => Test::AtLeast(value),
                    CompareTest::AtMost => Test::AtMost(value),
                    CompareTest::MoreThan => Test::MoreThan(value),
                    _ => Test::LessThan(value),
                };
                (test, vec![value])
            }
        };
        Some(ValueTest {
            operand,
            test,
            values,
            whole,
        })
    }

    /// Returns the number a fixed value stands for: a number or a percent, or the index of a state of the property
    /// `about`.
    fn fixed_number(&self, value: &SheetValue, about: Option<&str>) -> Option<f64> {
        match value {
            SheetValue::Literal(literal @ (SheetLiteral::Number(_) | SheetLiteral::Percent(_))) => literal.number(),
            SheetValue::Chip(SheetChip::State(state)) => self.state_index(about?, state),
            _ => None,
        }
    }

    /// Returns the index of state `state` among the states of property `property`, as a number.
    fn state_index(&self, property: &str, state: &str) -> Option<f64> {
        let index = self
            .property(property)?
            .states
            .iter()
            .position(|found| found.id == state)?;
        u32::try_from(index).ok().map(f64::from)
    }

    /// Returns the property `id` of any kind of thing.
    fn property(&self, id: &str) -> Option<&'a super::document::SheetProperty> {
        let document = self.document;
        document
            .cells
            .iter()
            .flatten()
            .chain(document.agents.iter().flat_map(|kind| &kind.properties))
            .chain(document.ground.iter().flat_map(|ground| &ground.properties))
            .find(|property| property.id == id)
    }
}

/// A note under a line that the relations between the card's lines decide.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelationNote {
    /// Id of the line the note sits under.
    pub line: String,
    /// Words of the note.
    pub text: String,
    /// Kind of the note.
    pub kind: RelationNoteKind,
}

/// Kind of a [`RelationNote`], in the order the notes stand under one line.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RelationNoteKind {
    /// Values of a count that none of a group of Become lines applies to, such as "None of lines 2 and 3 applies when
    /// B in my block is 4 or 5. Those cells keep their Opinion."
    Gap,
    /// The state a cell ends the tick in for each value of a count, such as "Lines 2 and 3 cover every value of B in
    /// my block. ..."
    Coverage,
    /// Become lines from different states, whose order does not matter.
    Order,
    /// A When whose test every line inside it already makes. The offer removes the When and keeps its lines.
    WhenRepeats,
    /// A When around one change that holds always. The offer moves the When's condition into that change, the line
    /// `inside`.
    OnlyOneInside {
        /// Id of the change inside the When.
        inside: String,
        /// Number of the change on the card.
        number: usize,
    },
    /// A change, When or result line inside a When.
    Inside,
    /// A When that holds no line.
    NothingInside,
}

impl<'a> CardRelations<'a> {
    /// Returns every relation note of the card, the notes of one line in the order they stand under it.
    ///
    /// `sentences` writes the conditions the notes quote, and `runs` returns whether a line runs. The gap, coverage
    /// and order notes speak only of lines that run.
    pub fn notes(&self, sentences: &Sentences<'_>, runs: impl Fn(&str) -> bool) -> Vec<RelationNote> {
        let mut notes = Vec::new();
        let thing = sentences.thing_word(&self.card.every);
        notes.extend(self.gap_notes(&runs, &thing));
        notes.extend(self.coverage_notes(&runs, &thing));
        notes.extend(self.order_notes(&runs, &thing));
        notes.extend(self.fold_notes());
        notes.extend(self.inside_notes(sentences, &thing));
        // A stable sort keeps the kinds of one line in the order they were added.
        notes.sort_by_key(|note| self.index(&note.line).unwrap_or(usize::MAX));
        notes
    }

    /// Returns the gap notes: for two or more top-level Become lines of one property from the same state, each running
    /// and testing one count with a compare or outside condition, with two or more new states, the values of the count
    /// that none of them applies to. The note sits under the last line of the group.
    fn gap_notes(&self, runs: &impl Fn(&str) -> bool, thing: &str) -> Vec<RelationNote> {
        let mut groups: Vec<((&str, &str), Vec<&str>)> = Vec::new();
        for &id in &self.order {
            let Some(line) = self.line(id) else {
                continue;
            };
            if line.kind != LineKind::Become || self.parent(id).is_some() || line.new_state.is_none() {
                continue;
            }
            let (Some(property), Some(start)) = (line.property.as_deref(), line.start_state.as_deref()) else {
                continue;
            };
            if line.condition.is_blank() {
                continue;
            }
            match groups.iter_mut().find(|(key, _)| *key == (property, start)) {
                Some((_, lines)) => lines.push(id),
                None => groups.push(((property, start), vec![id])),
            }
        }
        let mut notes = Vec::new();
        for ((property, _), lines) in groups {
            if lines.len() < 2 || self.new_states(&lines) < 2 || !lines.iter().all(|&id| runs(id)) {
                continue;
            }
            let Some((count, tests)) = self.count_tests(&lines, false) else {
                continue;
            };
            let Some(most) = most_counted(count) else {
                continue;
            };
            let gaps: Vec<u32> = (0..=most)
                .filter(|&x| !tests.iter().flatten().any(|test| test.test.passes(f64::from(x))))
                .collect();
            let Some(&last) = lines.last() else {
                continue;
            };
            if gaps.is_empty() {
                continue;
            }
            let property_name = self.property(property).map_or(property, |found| found.name.as_str());
            notes.push(RelationNote {
                line: last.to_owned(),
                text: gap_note(
                    &self.numbers(&lines),
                    &self.result_name(count),
                    &gaps,
                    property_name,
                    thing,
                ),
                kind: RelationNoteKind::Gap,
            });
        }
        notes
    }

    /// Returns the coverage notes: for a States property whose Become lines on the card all sit at the top level,
    /// name a state they change from, run, and test one count or hold always, the state a cell ends the tick in for
    /// each value of the count, when it never depends on the cell's own state and takes two or more states. A cell
    /// no line changes keeps its state. The note sits under the last Become line of the property.
    fn coverage_notes(&self, runs: &impl Fn(&str) -> bool, thing: &str) -> Vec<RelationNote> {
        let mut notes = Vec::new();
        for (property, lines) in self.becomes_by_property() {
            let Some(found) = self
                .property(property)
                .filter(|found| found.property_type == PropertyType::States)
            else {
                continue;
            };
            if lines.len() < 2
                || lines.iter().any(|&id| {
                    self.parent(id).is_some()
                        || !runs(id)
                        || self.line(id).is_none_or(|line| {
                            line.start_state.is_none() || line.new_state.is_none() || line.condition.is_blank()
                        })
                })
            {
                continue;
            }
            let Some((count, tests)) = self.count_tests(&lines, true) else {
                continue;
            };
            let Some(most) = most_counted(count) else {
                continue;
            };
            let mut ends: Vec<&str> = Vec::new();
            for x in 0..=most {
                let mut end: Option<&str> = None;
                let mut agree = true;
                for state in &found.states {
                    let mut result = state.id.as_str();
                    for (&id, test) in lines.iter().zip(&tests) {
                        let Some(line) = self.line(id) else {
                            continue;
                        };
                        let from = line.start_state.as_deref();
                        let applies = from == Some(ANY_STATE_ID) || from == Some(state.id.as_str());
                        if applies && test.as_ref().is_none_or(|test| test.test.passes(f64::from(x))) {
                            result = line.new_state.as_deref().unwrap_or(result);
                            break;
                        }
                    }
                    match end {
                        None => end = Some(result),
                        Some(other) if other != result => {
                            agree = false;
                            break;
                        }
                        Some(_) => {}
                    }
                }
                let Some(end) = end.filter(|_| agree) else {
                    ends.clear();
                    break;
                };
                ends.push(end);
            }
            let mut order: Vec<&str> = Vec::new();
            for end in &ends {
                if !order.contains(end) {
                    order.push(end);
                }
            }
            let Some(&last) = lines.last() else {
                continue;
            };
            if order.len() < 2 {
                continue;
            }
            let values: Vec<(&str, Vec<u32>)> = order
                .iter()
                .map(|&state| {
                    let values = (0..=most).zip(&ends).filter(|(_, end)| **end == state).map(|(x, _)| x);
                    (state_name(found, state), values.collect())
                })
                .collect();
            let ends: Vec<(&str, &[u32])> = values
                .iter()
                .map(|(state, values)| (*state, values.as_slice()))
                .collect();
            notes.push(RelationNote {
                line: last.to_owned(),
                text: coverage_note(&self.numbers(&lines), &self.result_name(count), &ends, thing),
                kind: RelationNoteKind::Coverage,
            });
        }
        notes
    }

    /// Returns the order notes: for a property with two or more Become lines on the card, every one running and
    /// changing from a named state of the property, no two from the same state, a note that their order does not
    /// matter. Lines inside a When count too. The note sits under the last Become line of the property.
    fn order_notes(&self, runs: &impl Fn(&str) -> bool, thing: &str) -> Vec<RelationNote> {
        let mut notes = Vec::new();
        for (property, lines) in self.becomes_by_property() {
            let Some(found) = self.property(property) else {
                continue;
            };
            let starts: Option<Vec<&str>> = lines
                .iter()
                .map(|&id| {
                    let start = self.line(id)?.start_state.as_deref()?;
                    let known = found.states.iter().any(|state| state.id == start);
                    (runs(id) && known).then_some(start)
                })
                .collect();
            let Some(starts) = starts else {
                continue;
            };
            let distinct = starts
                .iter()
                .enumerate()
                .all(|(index, start)| !starts[..index].contains(start));
            let Some(&last) = lines.last() else {
                continue;
            };
            if lines.len() < 2 || !distinct {
                continue;
            }
            let names: Vec<&str> = starts.iter().map(|start| state_name(found, start)).collect();
            notes.push(RelationNote {
                line: last.to_owned(),
                text: order_note(&self.numbers(&lines), &names, thing),
                kind: RelationNoteKind::Order,
            });
        }
        notes
    }

    /// Returns the offers to fold a When into its lines, each under its When. A When whose test is "my [property] is
    /// [state]" and whose lines are all Become lines from that state repeats them. Otherwise a When around one change
    /// that holds always can move its condition into the change. A When holding another When gets neither.
    fn fold_notes(&self) -> Vec<RelationNote> {
        let mut notes = Vec::new();
        for &id in &self.order {
            let Some(when) = self.line(id).filter(|line| line.kind == LineKind::When) else {
                continue;
            };
            let Some(SheetValue::Phrase(condition)) = when.condition.value() else {
                continue;
            };
            if condition.kind == LineKind::Always {
                continue;
            }
            let inside: Vec<(&str, &SheetLine)> = self
                .order
                .iter()
                .filter(|&&inner| self.parent(inner) == Some(id))
                .filter_map(|&inner| Some((inner, self.line(inner)?)))
                .collect();
            if inside.is_empty() || inside.iter().any(|(_, line)| line.kind == LineKind::When) {
                continue;
            }
            if let Some((property, state)) = self.tested_state(condition)
                && inside.iter().all(|(_, line)| {
                    line.kind == LineKind::Become
                        && line.property.as_deref() == Some(property)
                        && line.start_state.as_deref() == Some(state)
                })
            {
                let numbers: Vec<usize> = inside.iter().filter_map(|(inner, _)| self.number(inner)).collect();
                let name = self.property(property).map_or(state, |found| state_name(found, state));
                notes.push(RelationNote {
                    line: id.to_owned(),
                    text: when_repeats_note(&numbers, name),
                    kind: RelationNoteKind::WhenRepeats,
                });
                continue;
            }
            if let [(inner, line)] = inside.as_slice()
                && matches!(line.kind, LineKind::Become | LineKind::Set)
                && matches!(line.condition.value(), Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::Always)
                && let Some(number) = self.number(inner)
            {
                notes.push(RelationNote {
                    line: id.to_owned(),
                    text: only_one_inside_note(number),
                    kind: RelationNoteKind::OnlyOneInside {
                        inside: (*inner).to_owned(),
                        number,
                    },
                });
            }
        }
        notes
    }

    /// Returns the notes inside and under a When: under a change or a When inside one, the condition it holds only
    /// under, under a result line inside one, that it is worked out for every cell, and under an empty When, that it
    /// holds nothing.
    fn inside_notes(&self, sentences: &Sentences<'_>, thing: &str) -> Vec<RelationNote> {
        let mut notes = Vec::new();
        for &id in &self.order {
            let Some(line) = self.line(id) else {
                continue;
            };
            if let Some(parent) = self.parent(id)
                && let Some(block) = self.line(parent).filter(|block| block.kind == LineKind::When)
                && let Some(number) = self.number(parent)
            {
                let text = if matches!(line.kind, LineKind::Become | LineKind::Set | LineKind::When) {
                    let condition = (!block.condition.is_blank()).then(|| sentences.slot(&block.condition, None));
                    Some(only_when_note(number, condition.as_deref()))
                } else {
                    is_result_kind(line.kind).then(|| worked_out_everywhere_note(number, thing))
                };
                if let Some(text) = text {
                    notes.push(RelationNote {
                        line: id.to_owned(),
                        text,
                        kind: RelationNoteKind::Inside,
                    });
                }
            }
            if line.kind == LineKind::When && !line.lines.iter().any(|inner| self.index(inner).is_some()) {
                notes.push(RelationNote {
                    line: id.to_owned(),
                    text: NOTHING_INSIDE.to_owned(),
                    kind: RelationNoteKind::NothingInside,
                });
            }
        }
        notes
    }

    /// Returns the Become lines of the card by the property they change, each list in display order, the properties
    /// in the order of their first line.
    fn becomes_by_property(&self) -> Vec<(&'a str, Vec<&'a str>)> {
        let mut groups: Vec<(&'a str, Vec<&'a str>)> = Vec::new();
        for &id in &self.order {
            let Some(property) = self
                .line(id)
                .filter(|line| line.kind == LineKind::Become)
                .and_then(|line| line.property.as_deref())
            else {
                continue;
            };
            match groups.iter_mut().find(|(key, _)| *key == property) {
                Some((_, lines)) => lines.push(id),
                None => groups.push((property, vec![id])),
            }
        }
        groups
    }

    /// Returns the Count line that the own conditions of `lines` all test, with each line's test of it, `None` unless
    /// every condition is a compare or outside test of that count. With `always`, a condition that holds always
    /// passes every value and reads as `None`, and one line must still test the count.
    fn count_tests(&self, lines: &[&str], always: bool) -> Option<(&'a SheetLine, Vec<Option<ValueTest>>)> {
        let mut count: Option<&str> = None;
        let mut tests = Vec::new();
        for &id in lines {
            let SheetValue::Phrase(condition) = self.line(id)?.condition.value()? else {
                return None;
            };
            if always && condition.kind == LineKind::Always {
                tests.push(None);
                continue;
            }
            if condition.kind != LineKind::Compare {
                return None;
            }
            let test = self.compare_test(condition)?;
            let Operand::Result {
                line,
                part: None,
                property: None,
            } = &test.operand
            else {
                return None;
            };
            if count.is_some_and(|count| count != line) {
                return None;
            }
            count = Some(self.order.iter().copied().find(|&known| known == line)?);
            tests.push(Some(test));
        }
        let source = self.line(count?).filter(|line| line.kind == LineKind::Count)?;
        Some((source, tests))
    }

    /// Returns the number of different new states among the Become `lines`.
    fn new_states(&self, lines: &[&str]) -> usize {
        let mut states: Vec<&str> = Vec::new();
        for state in lines.iter().filter_map(|&id| self.line(id)?.new_state.as_deref()) {
            if !states.contains(&state) {
                states.push(state);
            }
        }
        states.len()
    }

    /// Returns the numbers of `lines` on the card.
    fn numbers(&self, lines: &[&str]) -> Vec<usize> {
        lines.iter().filter_map(|&id| self.number(id)).collect()
    }

    /// Returns the name of the result of `line`: its own name, its automatic name, or "result".
    fn result_name(&self, line: &SheetLine) -> String {
        line.name
            .clone()
            .or_else(|| automatic_name(self.document, line))
            .unwrap_or_else(|| "result".to_owned())
    }

    /// Returns the property and the state that `condition` tests as "my [property] is [state]", `None` for any other
    /// condition.
    fn tested_state<'b>(&self, condition: &'b SheetLine) -> Option<(&'b str, &'b str)> {
        if condition.kind != LineKind::Compare || condition.test != Some(CompareTest::Is) {
            return None;
        }
        let Some(SheetValue::Chip(SheetChip::My(property))) = condition.left.value() else {
            return None;
        };
        let Some(SheetValue::Chip(SheetChip::State(state))) = condition.right.value() else {
            return None;
        };
        let found = self.property(property)?;
        found
            .states
            .iter()
            .any(|known| known.id == *state)
            .then_some((property.as_str(), state.as_str()))
    }
}

/// Returns the name of state `state` of `property`, or the id of a state it does not hold.
fn state_name<'b>(property: &'b SheetProperty, state: &'b str) -> &'b str {
    property
        .states
        .iter()
        .find(|found| found.id == state)
        .map_or(state, |found| found.name.as_str())
}

/// Returns the most a Count line `count` can count: 8, 4 or 9 cells, `None` for a count of agents.
fn most_counted(count: &SheetLine) -> Option<u32> {
    match count.around? {
        Around::Cells8 => Some(8),
        Around::Cells4 => Some(4),
        Around::Block3x3 => Some(9),
        Around::AgentsWithin => None,
    }
}

/// Returns what a change line changes, as its kind and property, `None` for a line that is no change or that cannot
/// collide.
///
/// A Become collides only when it names the state it changes from, `any_state` included.
fn change_key(line: &SheetLine) -> Option<(LineKind, &str)> {
    match line.kind {
        LineKind::Become if line.start_state.is_some() => Some((line.kind, line.property.as_deref()?)),
        LineKind::Set => Some((line.kind, line.property.as_deref()?)),
        _ => None,
    }
}

/// Returns the property and the state that put `line` in a competing group, `None` for a line in none.
fn competing_key(line: &SheetLine) -> Option<(&str, &str)> {
    if line.kind != LineKind::Become {
        return None;
    }
    let property = line.property.as_deref()?;
    let state = line.start_state.as_deref().filter(|state| *state != ANY_STATE_ID)?;
    let mut draws = Draws::default();
    draws.count(line.condition.value(), true);
    (draws.rates == 1 && draws.loose_rates == 0 && draws.chances == 0).then_some((property, state))
}

/// Numbers of rate and chance draws a condition holds, those inside an all of, an any of or a none of included.
#[derive(Default)]
struct Draws {
    /// Rate phrases that stand as the condition or as an item of an all of.
    rates: usize,
    /// Rate phrases inside an any of or a none of. These hold no share of a competing group.
    loose_rates: usize,
    /// Chance phrases.
    chances: usize,
}

impl Draws {
    /// Counts the draws of the condition `value`, where `joined` says whether every enclosing phrase is an all of.
    fn count(&mut self, value: Option<&SheetValue>, joined: bool) {
        let Some(SheetValue::Phrase(phrase)) = value else {
            return;
        };
        match phrase.kind {
            LineKind::Rate if joined => self.rates += 1,
            LineKind::Rate => self.loose_rates += 1,
            LineKind::Chance => self.chances += 1,
            LineKind::AllOf | LineKind::AnyOf | LineKind::NoneOf => {
                let joined = joined && phrase.kind == LineKind::AllOf;
                for item in &phrase.items {
                    self.count(Some(item), joined);
                }
            }
            _ => {}
        }
    }
}

/// The value a test reads.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Operand {
    /// A property of mine at the start of the tick.
    My(String),
    /// The result of a line, one part of it, or a property of the place it names.
    Result {
        /// Id of the line.
        line: String,
        /// Part of the result.
        part: Option<String>,
        /// Property of the place.
        property: Option<String>,
    },
}

/// A test of a value against fixed numbers.
#[derive(Debug, Clone)]
enum Test {
    /// Equal to the number.
    Is(f64),
    /// Not equal to the number.
    IsNot(f64),
    /// The number or more.
    AtLeast(f64),
    /// The number or less.
    AtMost(f64),
    /// More than the number.
    MoreThan(f64),
    /// Less than the number.
    LessThan(f64),
    /// Equal to one of the numbers.
    OneOf(Vec<f64>),
    /// Equal to none of the numbers.
    NotOneOf(Vec<f64>),
    /// Less than the first number or more than the second.
    Outside(f64, f64),
}

impl Test {
    /// Returns whether `x` passes the test.
    fn passes(&self, x: f64) -> bool {
        let near = |value: f64| (x - value).abs() < NEAR;
        match self {
            Self::Is(value) => near(*value),
            Self::IsNot(value) => !near(*value),
            Self::AtLeast(value) => x >= value - NEAR,
            Self::AtMost(value) => x <= value + NEAR,
            Self::MoreThan(value) => x > value + NEAR,
            Self::LessThan(value) => x < value - NEAR,
            Self::OneOf(values) => values.iter().any(|value| near(*value)),
            Self::NotOneOf(values) => !values.iter().any(|value| near(*value)),
            Self::Outside(low, high) => x < low - NEAR || x > high + NEAR,
        }
    }
}

/// A test of one value that a change needs.
#[derive(Debug, Clone)]
struct ValueTest {
    /// Value the test reads.
    operand: Operand,
    /// The test.
    test: Test,
    /// Numbers the test lists, where its outcome can change.
    values: Vec<f64>,
    /// Whether the value is a whole number: a count or a state.
    whole: bool,
}

impl ValueTest {
    /// Returns whether no value passes both this test and `other`, two tests of the same value.
    ///
    /// Both tests change outcome only at their listed numbers, so trying each number, its neighbours and the gaps
    /// between them decides it exactly. A whole value tries only whole numbers.
    fn excludes(&self, other: &Self) -> bool {
        let mut values: Vec<f64> = self.values.iter().chain(&other.values).copied().collect();
        values.sort_by(f64::total_cmp);
        let (Some(&lowest), Some(&highest)) = (values.first(), values.last()) else {
            return false;
        };
        let whole = self.whole && other.whole;
        let mut tries = vec![lowest - 2.0, highest + 2.0];
        for &value in &values {
            if whole {
                let (floor, ceil) = (value.floor(), value.ceil());
                tries.extend([floor - 1.0, floor, floor + 1.0, ceil - 1.0, ceil, ceil + 1.0]);
            } else {
                tries.extend([value, value - 0.5, value + 0.5]);
            }
        }
        if !whole {
            tries.extend(values.windows(2).map(|pair| f64::midpoint(pair[0], pair[1])));
        }
        !tries.iter().any(|&x| self.test.passes(x) && other.test.passes(x))
    }
}
