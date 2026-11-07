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
//! The sheet checker and the writer's comments take these relations from here.

use std::collections::HashMap;

use super::document::{
    CompareTest, LineKind, PropertyType, SheetCard, SheetChip, SheetDocument, SheetLine, SheetLiteral, SheetValue,
    Slot, SummaryKind,
};
use super::sentence::ANY_STATE_ID;

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
        let mut parent = self.parent(line_id);
        while let Some(id) = parent {
            if self.line(id).is_some_and(|block| block.kind == LineKind::Repeat) {
                return None;
            }
            parent = self.parent(id);
        }
        competing_key(self.line(line_id)?)
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
    fn compare_test(&self, phrase: &SheetLine) -> Option<ValueTest> {
        let operand = match phrase.left.value()? {
            SheetValue::Chip(SheetChip::My(property)) => Operand::My(property.clone()),
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
