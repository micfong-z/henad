//! Cards, lines and the values in their slots: references, types, blanks, scopes, the refusals of rates and of
//! chances outside 0 to 1, and the warning on a rate read in another unit than its parameter's.
//!
//! The slots of a line are checked in the order its sentence reads them, so the first blank or unusable value of a
//! line words its skip note, as the card shows it.

use std::collections::{HashMap, HashSet};

use henad_core::metadata::TimeUnit;

use super::{Checker, Place};
use super::{declarations, formula};
use crate::sheet::document::{
    Around, CardRuns, CellWhich, CompareTest, LineKind, LineMode, MadeOf, PickRule, PropertyType, SheetChip, SheetLine,
    SheetLiteral, SheetValue, Slot, SummaryKind,
};
use crate::sheet::edit::unit_in_text;
use crate::sheet::limits::MAX_NESTING;
use crate::sheet::sentence::{
    self, ANY_STATE_ID, Blank, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE, LineSelect, NOT_RUN_TICK, RATE_BELOW_ZERO,
    RATE_IN_BUTTON_CARD,
};

/// What a blank wants. Its words fill the skip note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Want {
    /// A blank of a chip or a value.
    Blank(Blank),
    /// A select inside a line.
    Select(LineSelect),
}

impl Want {
    /// Words after "choose" in the skip note, such as "a state" or "unit of time".
    fn words(self) -> &'static str {
        match self {
            Self::Blank(blank) => blank.want(),
            Self::Select(select) => select.words(),
        }
    }
}

/// Type of a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ValueType {
    /// Yes or no.
    YesNo,
    /// A number.
    Number,
    /// An arrow: a position, a heading or an offset.
    Arrow,
    /// A place: a cell of the grid or the ground.
    Place,
    /// A state of a States property.
    State,
    /// A list of values.
    List,
}

impl ValueType {
    /// Words of the type in a message.
    fn words(self) -> &'static str {
        match self {
            Self::YesNo => "yes or no",
            Self::Number => "a number",
            Self::Arrow => "an arrow",
            Self::Place => "a place",
            Self::State => "a state",
            Self::List => "a list",
        }
    }
}

/// The line a value belongs to.
#[derive(Debug, Clone, Copy)]
pub(super) struct LineAt<'a> {
    /// Index of the card.
    card: usize,
    /// Id of the line at the top of the walk, the line whose table holds the value.
    id: &'a str,
    /// Index of the line in the card's display order.
    position: usize,
    /// Whether the line sits inside a Repeat.
    in_repeat: bool,
    /// Whether the card runs once, when a button is pressed.
    runs_once: bool,
}

/// Where a value is checked.
#[derive(Debug, Clone)]
pub(super) struct Context<'a> {
    /// Table a problem is placed at.
    place: Place,
    /// Line the value belongs to, `None` for a value outside the cards.
    line: Option<LineAt<'a>>,
    /// Id of the kind of thing the value is about: `cells`, `ground` or an agent kind's id.
    thing: &'a str,
    /// Id of the kind of thing a Count or Look around looks at, `None` outside one.
    looked: Option<&'a str>,
    /// Whether the value stands in a condition slot, where the outside test is allowed.
    condition: bool,
    /// Whether the value is the test of a Count, whose left side wants a property.
    whose: bool,
}

impl<'a> Context<'a> {
    /// Returns the context of a value outside the cards, placed at `place` and about `thing`.
    pub(super) fn outside(place: Place, thing: &'a str) -> Self {
        Self {
            place,
            line: None,
            thing,
            looked: None,
            condition: false,
            whose: false,
        }
    }

    /// Returns this context for a slot that is a condition, or that is not, as `condition` says.
    fn with_condition(&self, condition: bool) -> Self {
        Self {
            condition,
            whose: false,
            ..self.clone()
        }
    }

    /// Returns this context for a slot of a phrase that takes a value: neither a condition nor the test of a Count.
    fn inner(&self) -> Self {
        Self {
            condition: false,
            whose: false,
            ..self.clone()
        }
    }
}

/// What the walk of one line found that decides whether it runs.
#[derive(Debug, Default)]
pub(super) struct LineState {
    /// Note of the first problem that keeps the line from running.
    skip: Option<String>,
    /// Ids of the lines whose results the line uses, in order.
    results: Vec<String>,
}

impl LineState {
    /// Records `note` as the line's skip note, unless an earlier problem already holds it.
    fn skip(&mut self, note: impl FnOnce() -> String) {
        if self.skip.is_none() {
            self.skip = Some(note());
        }
    }
}

/// Checks every card and line of the sheet.
pub(super) fn check(checker: &mut Checker<'_>) {
    check_line_ids(checker);
    for index in 0..checker.document.cards.len() {
        check_card(checker, index);
    }
}

/// Checks the id of every line: its form, its number below `next_line`, and one card per line.
fn check_line_ids(checker: &mut Checker<'_>) {
    let document = checker.document;
    let next_line = document.sheet.next_line;
    let mut seen: HashSet<&str> = HashSet::new();
    for (index, card) in document.cards.iter().enumerate() {
        for line_id in card.line_tables.keys() {
            let place = Place::Line(index, line_id.clone());
            match line_number(line_id) {
                None => checker.error(
                    format!("'{line_id}' is no line id, which is l and a number from 1"),
                    &place,
                ),
                Some(number) if number >= u64::from(next_line) => {
                    checker.error(
                        format!("line {line_id} is not below [sheet] next_line {next_line}"),
                        &place,
                    );
                }
                Some(_) => {}
            }
            if !seen.insert(line_id) {
                checker.error(format!("line {line_id} sits in two cards"), &place);
            }
        }
    }
}

/// Returns the number of the line id `id`, `None` for text that is not `l` and a number from 1.
pub(super) fn line_number(id: &str) -> Option<u64> {
    let digits = id.strip_prefix('l')?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Checks card `index`: its kind of thing, its lists of lines and each line in display order.
fn check_card(checker: &mut Checker<'_>, index: usize) {
    let document = checker.document;
    let card = &document.cards[index];
    let place = Place::Card(index);
    let every_is_known = document.world.made_of == MadeOf::Cells && card.every == "cells"
        || document.agents.iter().any(|kind| kind.id == card.every);
    if !every_is_known {
        checker.error(
            format!(
                "card '{}' is for '{}', which is no kind of thing with a card",
                card.id, card.every
            ),
            &place,
        );
        return;
    }
    // Every line sits in exactly one list.
    let mut listed: HashMap<&str, usize> = HashMap::new();
    for id in card
        .lines
        .iter()
        .chain(card.line_tables.values().flat_map(|line| &line.lines))
    {
        *listed.entry(id.as_str()).or_default() += 1;
    }
    for (id, line) in &card.line_tables {
        let line_place = Place::Line(index, id.clone());
        match listed.get(id.as_str()).copied().unwrap_or(0) {
            0 => checker.error(format!("line {id} is listed in no lines list"), &line_place),
            1 if checker.relations[index].index(id).is_none() => checker.error(
                format!("line {id} sits inside a block that no line of the card reaches"),
                &line_place,
            ),
            1 => {}
            _ => checker.error(format!("line {id} is listed more than once"), &line_place),
        }
        if !line.lines.is_empty() && !matches!(line.kind, LineKind::When | LineKind::Repeat) {
            checker.error(format!("line {id}: only When and Repeat hold lines"), &line_place);
        }
    }
    let mut missing: Vec<&str> = listed
        .keys()
        .copied()
        .filter(|id| !card.line_tables.contains_key(*id))
        .collect();
    missing.sort_unstable();
    for id in missing {
        checker.error(format!("card '{}' lists {id}, which it does not hold", card.id), &place);
    }

    let order: Vec<&str> = checker.relations[index].order().to_vec();
    let mut names: HashMap<&str, &str> = HashMap::new();
    let mut skipped: HashSet<&str> = HashSet::new();
    for (position, &id) in order.iter().enumerate() {
        let Some(line) = card.line_tables.get(id) else {
            continue;
        };
        let at = LineAt {
            card: index,
            id,
            position,
            in_repeat: in_repeat(checker, index, id),
            runs_once: card.runs == CardRuns::Once,
        };
        if let Some(note) = check_line(checker, line, at, &skipped) {
            checker.skip(note, index, id);
            skipped.insert(id);
        }
        if let Some(name) = line.name.as_deref().filter(|_| is_result_kind(line.kind)) {
            match names.get(name) {
                Some(other) => checker.warning(
                    format!("lines {other} and {id} are both named '{name}'"),
                    &Place::Line(index, id.to_owned()),
                ),
                None => {
                    names.insert(name, id);
                }
            }
        }
    }
}

/// Returns whether line `id` of card `card` sits inside a Repeat.
fn in_repeat(checker: &Checker<'_>, card: usize, id: &str) -> bool {
    let relations = &checker.relations[card];
    let mut parent = relations.parent(id);
    while let Some(block) = parent {
        if relations.line(block).is_some_and(|line| line.kind == LineKind::Repeat) {
            return true;
        }
        parent = relations.parent(block);
    }
    false
}

/// Checks one line of a card and returns its skip note, `None` for a line that runs.
///
/// `skipped` holds the lines above it that do not run.
fn check_line<'a>(
    checker: &mut Checker<'a>,
    line: &SheetLine,
    at: LineAt<'a>,
    skipped: &HashSet<&str>,
) -> Option<String> {
    let document = checker.document;
    let card = &document.cards[at.card];
    let place = Place::Line(at.card, at.id.to_owned());
    let context = Context {
        place: place.clone(),
        line: Some(at),
        thing: card.every.as_str(),
        looked: None,
        condition: false,
        whose: false,
    };
    if line.kind == LineKind::Always {
        checker.error(
            format!("line {}: always is a condition, and a line cannot be one", at.id),
            &place,
        );
        return None;
    }
    if line.name.is_some() && !is_result_kind(line.kind) {
        checker.error(
            format!("line {}: a line that makes a change has no name", at.id),
            &place,
        );
    }
    if at.in_repeat && matches!(line.kind, LineKind::Count | LineKind::LookAround | LineKind::PickCell) {
        checker.error(sentence::LOOK_IN_REPEAT.to_owned(), &place);
    }
    if at.runs_once && matches!(line.kind, LineKind::Count | LineKind::LookAround | LineKind::PickCell) {
        checker.error(
            format!("line {}: a card that runs once cannot look around", at.id),
            &place,
        );
    }
    let mut state = LineState::default();
    checker.body(line, &context, &mut state);
    checker.key(line.key, format!("line {}", at.id), holds_draw(line), &place);

    if state.skip.is_none() {
        let relations = &checker.relations[at.card];
        let unfinished = state.results.iter().find_map(|result| {
            let source = relations.index(result)?;
            (source < at.position && skipped.contains(result.as_str())).then_some((result, source + 1))
        });
        if let Some((result, number)) = unfinished {
            let name = checker.result_name(result);
            state.skip = Some(sentence::not_run_result_unfinished(&name, number));
        }
    }
    if state.skip.is_none() && holds_rate(line) && document.time.is_none() {
        state.skip(|| NOT_RUN_TICK.to_owned());
    }
    if state.skip.is_none() {
        let relations = &checker.relations[at.card];
        // A result inside a skipped When still runs, and every line inside a skipped Repeat is skipped, however deep.
        let blocked_by_parent = relations.parent(at.id).filter(|parent| {
            let parent_kind = relations.line(parent).map(|block| block.kind);
            let depends = parent_kind == Some(LineKind::Repeat) || !is_result_kind(line.kind);
            depends && skipped.contains(parent)
        });
        let blocked = blocked_by_parent.or_else(|| {
            let mut ancestor = relations.parent(at.id);
            while let Some(block) = ancestor {
                if relations
                    .line(block)
                    .is_some_and(|found| found.kind == LineKind::Repeat)
                    && skipped.contains(block)
                {
                    return Some(block);
                }
                ancestor = relations.parent(block);
            }
            None
        });
        if let Some(block) = blocked {
            let number = relations.number(block).unwrap_or(0);
            state.skip(|| sentence::not_run_when_unfinished(number));
        }
    }
    state.skip
}

impl<'a> Checker<'a> {
    /// Returns the name of the result of line `id`: its name, or its sentence when it has none.
    fn result_name(&self, id: &str) -> String {
        match self.document.line(id).and_then(|(_, line)| line.name.clone()) {
            Some(name) => name,
            None => self.sentences.line(id, &[]),
        }
    }

    /// Records a problem of a value that keeps its line from running: a skip note for a value in a line, and the error
    /// `error` for a value outside the cards.
    fn unusable(&mut self, context: &Context<'_>, state: &mut LineState, note: impl FnOnce() -> String, error: &str) {
        if context.line.is_some() {
            state.skip(note);
        } else {
            self.error(error, &context.place);
        }
    }

    /// Checks the value in `slot` and returns its type, `None` for a blank slot or a type that cannot be known.
    ///
    /// `want` is what a blank slot wants, `expect` the type the slot takes, and `governing` the property that a state
    /// in the slot belongs to.
    pub(super) fn slot(
        &mut self,
        slot: &Slot,
        context: &Context<'a>,
        want: Want,
        expect: Option<ValueType>,
        governing: Option<&str>,
        state: &mut LineState,
    ) -> Option<ValueType> {
        match slot.value() {
            None => {
                self.unusable(
                    context,
                    state,
                    || sentence::not_run_choose(want.words()),
                    &format!("a value is blank, where it wants {}", want.words()),
                );
                None
            }
            Some(value) => self.value(value, context, want, expect, governing, state),
        }
    }

    /// Checks `value`, as [`Self::slot`] checks a filled slot.
    fn value(
        &mut self,
        value: &SheetValue,
        context: &Context<'a>,
        want: Want,
        expect: Option<ValueType>,
        governing: Option<&str>,
        state: &mut LineState,
    ) -> Option<ValueType> {
        let found = match value {
            SheetValue::Literal(SheetLiteral::Bool(_)) => Some(ValueType::YesNo),
            SheetValue::Literal(_) => Some(ValueType::Number),
            SheetValue::List(items) => {
                for item in items {
                    self.value(item, context, want, None, governing, state);
                }
                return Some(ValueType::List);
            }
            SheetValue::Phrase(phrase) => {
                self.phrase(phrase, context, state);
                self.phrase_type(phrase)
            }
            SheetValue::Chip(chip) => self.chip(chip, context, want, governing, state),
        };
        if let (Some(expect), Some(found)) = (expect, found)
            && expect != found
        {
            self.error(
                format!("a slot wants {} and holds {}", expect.words(), found.words()),
                &context.place,
            );
        }
        found
    }

    /// Checks a phrase in a slot.
    fn phrase(&mut self, phrase: &SheetLine, context: &Context<'a>, state: &mut LineState) {
        if phrase.kind == LineKind::Always {
            if !context.condition {
                self.error("always stands only in a condition", &context.place);
            }
            return;
        }
        if !is_result_kind(phrase.kind) {
            self.error(
                format!("a slot holds a {} line, which gives no value", phrase.kind.as_str()),
                &context.place,
            );
            return;
        }
        self.body(phrase, context, state);
    }

    /// Checks the fields and slots of a line or of a phrase in a slot, in the order its sentence reads them.
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per line kind, each in its sentence's order, reads best as one match"
    )]
    fn body(&mut self, line: &SheetLine, context: &Context<'a>, state: &mut LineState) {
        let inner = context.inner();
        let number = Some(ValueType::Number);
        let place = &context.place;
        match line.kind {
            LineKind::Count | LineKind::LookAround => self.look(line, context, state),
            LineKind::PickCell => {
                match line.around {
                    None => state_skip(self, context, state, Want::Select(LineSelect::WhichCells)),
                    Some(Around::Cells8 | Around::Cells4) => {}
                    Some(_) => self.error("Pick a cell picks among the 8 or the 4 cells around", place),
                }
                let ground = ground_of(context.thing);
                match line.pick {
                    None => self.error("Pick a cell needs a pick rule: most, least or random", place),
                    Some(PickRule::Random) => {}
                    Some(_) => match line.ranked_by.as_deref() {
                        None => state_skip(self, context, state, Want::Blank(Blank::GroundProperty)),
                        Some(property) if self.property_of(property, ground).is_none() => {
                            self.error(format!("'{property}' is no property of '{ground}'"), place);
                        }
                        Some(_) => {}
                    },
                }
                if !line.only.is_blank() {
                    let looking = Context {
                        looked: Some(ground),
                        ..inner.with_condition(true)
                    };
                    self.slot(
                        &line.only,
                        &looking,
                        Want::Blank(Blank::When),
                        Some(ValueType::YesNo),
                        None,
                        state,
                    );
                }
            }
            LineKind::LookAtCell => match line.which {
                None => self.error("Look at a cell needs a cell: here, ahead or at", place),
                Some(CellWhich::At) => {
                    self.slot(
                        &line.place,
                        &inner,
                        Want::Blank(Blank::Value),
                        Some(ValueType::Place),
                        None,
                        state,
                    );
                }
                Some(_) => {}
            },
            LineKind::Chance => {
                self.slot(&line.chance, &inner, Want::Blank(Blank::Chance), number, None, state);
                self.draw_range(&line.chance, context, Draw::Chance);
                if !line.tries.is_blank() {
                    self.slot(&line.tries, &inner, Want::Blank(Blank::Count), number, None, state);
                }
            }
            LineKind::Rate => {
                if context.line.is_some_and(|at| at.runs_once) {
                    self.error(RATE_IN_BUTTON_CARD, place);
                }
                if context.line.is_some_and(|at| at.in_repeat) {
                    self.error("a rate cannot stand inside a Repeat", place);
                }
                self.slot(&line.rate, &inner, Want::Blank(Blank::Rate), number, None, state);
                self.draw_range(&line.rate, context, Draw::Rate);
                match line.rate_unit {
                    None => state_skip(self, context, state, Want::Select(LineSelect::UnitOfTime)),
                    Some(unit) => self.rate_unit(&line.rate, unit, context),
                }
                if !line.tries.is_blank() {
                    self.slot(&line.tries, &inner, Want::Blank(Blank::Count), number, None, state);
                }
            }
            LineKind::RandomNumber => {
                self.slot(&line.low, &inner, Want::Blank(Blank::LowerNumber), number, None, state);
                self.slot(&line.high, &inner, Want::Blank(Blank::UpperNumber), number, None, state);
            }
            LineKind::RandomDirection => {
                self.slot(&line.length, &inner, Want::Blank(Blank::Number), number, None, state);
            }
            LineKind::Compare => self.compare(line, context, state),
            LineKind::AllOf | LineKind::AnyOf | LineKind::NoneOf => {
                if line.items.is_empty() {
                    state_skip(self, context, state, Want::Blank(Blank::YesOrNoList));
                }
                for item in &line.items {
                    self.value(
                        item,
                        &context.with_condition(context.condition),
                        Want::Blank(Blank::YesOrNo),
                        Some(ValueType::YesNo),
                        None,
                        state,
                    );
                }
            }
            LineKind::Choose => {
                for row in &line.rows {
                    self.slot(&row.value, &inner, Want::Blank(Blank::Value), None, None, state);
                    self.slot(
                        &row.condition,
                        &context.with_condition(true),
                        Want::Blank(Blank::When),
                        Some(ValueType::YesNo),
                        None,
                        state,
                    );
                }
                self.slot(&line.otherwise, &inner, Want::Blank(Blank::Value), None, None, state);
            }
            LineKind::DependingOn => {
                if let Some(property) = self.changed_property(line, context, state, Want::Blank(Blank::Property)) {
                    if property.property_type != PropertyType::States {
                        self.error(
                            format!("Depending on reads a States property, and '{}' is not one", property.id),
                            place,
                        );
                    }
                    let states: Vec<&str> = property.states.iter().map(|item| item.id.as_str()).collect();
                    let keys: Vec<&str> = line.branches.keys().map(String::as_str).collect();
                    let mut sorted = states.clone();
                    sorted.sort_unstable();
                    if keys != sorted {
                        self.error(
                            format!("Depending on needs one value for each state of '{}'", property.id),
                            place,
                        );
                    }
                }
                for value in line.branches.values() {
                    self.value(value, &inner, Want::Blank(Blank::Value), None, None, state);
                }
            }
            LineKind::Calculate => match line.formula.as_deref().filter(|formula| !formula.trim().is_empty()) {
                None => state_skip(self, context, state, Want::Blank(Blank::Number)),
                Some(formula) => match formula::references(formula) {
                    Err(reason) => self.error(reason, place),
                    Ok(references) => {
                        for reference in references {
                            let chip = formula_chip(reference, self.document);
                            self.chip(&chip, &inner, Want::Blank(Blank::Number), None, state);
                        }
                    }
                },
            },
            LineKind::Become => {
                match (line.new_state.as_deref(), line.property.as_deref()) {
                    (None, _) | (_, None) => state_skip(self, context, state, Want::Blank(Blank::State)),
                    (Some(new), Some(id)) => match self.property_of(id, context.thing) {
                        None => self.error(format!("'{id}' is no property of '{}'", context.thing), place),
                        Some(property) => {
                            if property.property_type != PropertyType::States {
                                self.error(
                                    format!("Become changes a States property, and '{id}' is not one"),
                                    place,
                                );
                            } else if !self.has_state(id, new) {
                                self.error(format!("new_state '{new}' is no state of '{id}'"), place);
                            }
                        }
                    },
                }
                match line.start_state.as_deref() {
                    None => state_skip(self, context, state, Want::Blank(Blank::OldState)),
                    Some(ANY_STATE_ID) => {}
                    Some(old) => {
                        if let Some(id) = line.property.as_deref()
                            && self.property_of(id, context.thing).is_some()
                            && !self.has_state(id, old)
                        {
                            self.error(format!("start_state '{old}' is no state of '{id}'"), place);
                        }
                    }
                }
                self.condition(line, context, state, true);
            }
            LineKind::Set => {
                // A Set over cells changes a number, and a Set over agents any property but its place.
                let (want, value_want) = if matches!(context.thing, "cells" | "ground") {
                    (Blank::NumberProperty, Blank::Number)
                } else {
                    (Blank::Property, Blank::Value)
                };
                let property = self.changed_property(line, context, state, Want::Blank(want));
                if let Some(property) = property
                    && matches!(
                        property.property_type,
                        PropertyType::States | PropertyType::Position | PropertyType::Heading
                    )
                {
                    self.error(
                        format!(
                            "Set changes a property that is not States, Position or Heading, and '{}' is one",
                            property.id
                        ),
                        place,
                    );
                }
                let governing = property.map(|property| property.id.as_str());
                self.slot(&line.value, &inner, Want::Blank(value_want), None, governing, state);
                self.condition(line, context, state, true);
            }
            LineKind::Steer => {
                let property = self.changed_property(line, context, state, Want::Blank(Blank::Property));
                if let Some(property) = property
                    && !matches!(property.property_type, PropertyType::Number | PropertyType::Arrow)
                {
                    self.error(
                        format!("Steer changes a Number or an Arrow, and '{}' is neither", property.id),
                        place,
                    );
                }
                if line.mode.is_none_or(|mode| mode == LineMode::To) {
                    self.error("Steer needs a mode: by or towards", place);
                }
                self.slot(&line.value, &inner, Want::Blank(Blank::Value), None, None, state);
                self.slot(&line.strength, &inner, Want::Blank(Blank::Number), number, None, state);
                self.condition(line, context, state, false);
            }
            LineKind::KeepBetween => {
                self.changed_property(line, context, state, Want::Blank(Blank::Property));
                self.slot(
                    &line.minimum,
                    &inner,
                    Want::Blank(Blank::LowerNumber),
                    number,
                    None,
                    state,
                );
                self.slot(
                    &line.maximum,
                    &inner,
                    Want::Blank(Blank::UpperNumber),
                    number,
                    None,
                    state,
                );
                self.condition(line, context, state, false);
            }
            LineKind::Move => {
                if matches!(context.thing, "cells" | "ground") {
                    self.error("only agents move", place);
                }
                let expect = match line.mode {
                    Some(LineMode::To) => Some(ValueType::Place),
                    Some(LineMode::By) => Some(ValueType::Arrow),
                    _ => {
                        self.error("Move needs a mode: by or to", place);
                        None
                    }
                };
                self.slot(&line.value, &inner, Want::Blank(Blank::Value), expect, None, state);
                self.condition(line, context, state, false);
            }
            LineKind::Leave => {
                self.slot(&line.amount, &inner, Want::Blank(Blank::Number), number, None, state);
                match line.ground_property.as_deref() {
                    None => state_skip(self, context, state, Want::Blank(Blank::GroundProperty)),
                    Some(id) => {
                        let fits = self
                            .property_of(id, "ground")
                            .is_some_and(|property| property.property_type == PropertyType::Number);
                        if !fits {
                            self.error(
                                format!("Leave needs a Number property of the ground, and '{id}' is not one"),
                                place,
                            );
                        }
                    }
                }
                self.condition(line, context, state, true);
            }
            LineKind::Record => {
                match line.event.as_deref() {
                    None => state_skip(self, context, state, Want::Blank(Blank::Event)),
                    Some(event) if !self.events.contains(event) => {
                        self.error(format!("the event '{event}' does not exist"), place);
                    }
                    Some(_) => {}
                }
                self.condition(line, context, state, true);
            }
            LineKind::When => {
                if let Some(SheetValue::Phrase(phrase)) = line.condition.value()
                    && phrase.kind == LineKind::Always
                {
                    self.error("a When cannot be always", place);
                }
                self.slot(
                    &line.condition,
                    &context.with_condition(true),
                    Want::Blank(Blank::When),
                    Some(ValueType::YesNo),
                    None,
                    state,
                );
            }
            LineKind::Repeat => {
                self.slot(&line.times, &inner, Want::Blank(Blank::Times), number, None, state);
            }
            LineKind::Always => {}
        }
    }

    /// Checks the condition of a change. `required` says whether the change must hold one.
    fn condition(&mut self, line: &SheetLine, context: &Context<'a>, state: &mut LineState, required: bool) {
        if required || !line.condition.is_blank() {
            self.slot(
                &line.condition,
                &context.with_condition(true),
                Want::Blank(Blank::When),
                Some(ValueType::YesNo),
                None,
                state,
            );
        }
    }

    /// Checks the `property` of a change or a Depending on line and returns the property, `None` when it is blank or
    /// names no property of the line's kind of thing.
    fn changed_property(
        &mut self,
        line: &SheetLine,
        context: &Context<'a>,
        state: &mut LineState,
        want: Want,
    ) -> Option<&'a crate::sheet::document::SheetProperty> {
        let Some(id) = line.property.as_deref() else {
            state_skip(self, context, state, want);
            return None;
        };
        let property = self.property_of(id, context.thing);
        if property.is_none() {
            self.error(format!("'{id}' is no property of '{}'", context.thing), &context.place);
        }
        property
    }

    /// Checks a Count or Look around line: what it looks at, its test and its summaries.
    fn look(&mut self, line: &SheetLine, context: &Context<'a>, state: &mut LineState) {
        let place = &context.place;
        let grid = line.around != Some(Around::AgentsWithin);
        let mut looked = ground_of(context.thing);
        // A grid Total or average reads "average of Food over the 8 cells around me", its summary first.
        if line.kind == LineKind::LookAround && grid {
            self.summaries(line, context, looked, state);
        }
        match line.around {
            None => state_skip(self, context, state, Want::Select(LineSelect::WhichCells)),
            Some(Around::AgentsWithin) => {
                let document = self.document;
                match line.agent_kind.as_deref() {
                    Some(kind) => match document.agents.iter().find(|agents| agents.id == kind) {
                        Some(agents) => looked = agents.id.as_str(),
                        None => self.error(format!("'{kind}' is no kind of agent"), place),
                    },
                    None => self.error("a look at agents needs a kind of agent", place),
                }
                self.slot(
                    &line.distance,
                    &context.inner(),
                    Want::Blank(Blank::Number),
                    Some(ValueType::Number),
                    None,
                    state,
                );
                if line.include_me.is_none() {
                    self.error("a look at agents says whether it includes me: include_me", place);
                }
            }
            Some(_) => {}
        }
        let looking = Context {
            looked: Some(looked),
            ..context.with_condition(true)
        };
        if line.kind == LineKind::Count && grid {
            let whose = Context {
                whose: true,
                ..looking.clone()
            };
            self.slot(
                &line.whose,
                &whose,
                Want::Blank(Blank::Property),
                Some(ValueType::YesNo),
                None,
                state,
            );
        } else if !line.whose.is_blank() {
            self.slot(
                &line.whose,
                &looking,
                Want::Blank(Blank::When),
                Some(ValueType::YesNo),
                None,
                state,
            );
        }
        if !line.only.is_blank() {
            self.slot(
                &line.only,
                &looking,
                Want::Blank(Blank::When),
                Some(ValueType::YesNo),
                None,
                state,
            );
        }
        if line.kind == LineKind::LookAround && !grid {
            self.summaries(line, context, looked, state);
        }
    }

    /// Checks the summaries of a Look around line that looks at the kind of thing `looked`.
    fn summaries(&mut self, line: &SheetLine, context: &Context<'a>, looked: &'a str, state: &mut LineState) {
        let looking = Context {
            looked: Some(looked),
            ..context.with_condition(false)
        };
        let grid = line.around != Some(Around::AgentsWithin);
        let mut ids = HashSet::new();
        for summary in &line.summaries {
            declarations::claim_summary(self, &summary.id, &context.place);
            if !ids.insert(summary.id.as_str()) {
                self.error(format!("two summaries share the id '{}'", summary.id), &context.place);
            }
            match summary.summary {
                None => state_skip(self, context, state, Want::Select(LineSelect::TotalOrAverage)),
                Some(SummaryKind::Count) => {}
                Some(_) => {
                    let want = if grid { Blank::NumberProperty } else { Blank::Value };
                    self.slot(&summary.value, &looking, Want::Blank(want), None, None, state);
                }
            }
        }
    }

    /// Checks a compare phrase or line.
    fn compare(&mut self, line: &SheetLine, context: &Context<'a>, state: &mut LineState) {
        let inner = context.inner();
        let left_want = if context.whose { Blank::Property } else { Blank::Value };
        self.slot(&line.left, &inner, Want::Blank(left_want), None, None, state);
        let governing = line.left.value().and_then(governing_property);
        let of_states = governing.is_some_and(|id| {
            self.properties
                .get(id)
                .is_some_and(|(property, _)| property.property_type == PropertyType::States)
        });
        let number = Some(ValueType::Number);
        match line.test {
            None => state_skip(self, context, state, Want::Select(LineSelect::Comparison)),
            Some(CompareTest::Outside) => {
                if !context.condition {
                    self.error(
                        "is less than ... or more than ... stands only in a condition",
                        &context.place,
                    );
                }
                self.slot(&line.low, &inner, Want::Blank(Blank::LowerNumber), number, None, state);
                self.slot(&line.high, &inner, Want::Blank(Blank::UpperNumber), number, None, state);
            }
            Some(test) => {
                let list = matches!(test, CompareTest::OneOf | CompareTest::NotOneOf);
                let want = match (of_states, list) {
                    (true, true) => Blank::StateList,
                    (true, false) => Blank::State,
                    (false, true) => Blank::NumberList,
                    (false, false) => Blank::Number,
                };
                self.slot(&line.right, &inner, Want::Blank(want), None, governing, state);
                if test == CompareTest::Within {
                    self.slot(&line.tolerance, &inner, Want::Blank(Blank::Number), number, None, state);
                }
            }
        }
    }

    /// Checks a chip and returns its type.
    fn chip(
        &mut self,
        chip: &SheetChip,
        context: &Context<'a>,
        want: Want,
        governing: Option<&str>,
        state: &mut LineState,
    ) -> Option<ValueType> {
        let place = &context.place;
        match chip {
            SheetChip::Param(id) => {
                if self.parameters.contains_key(id.as_str()) {
                    Some(ValueType::Number)
                } else {
                    self.error(format!("the parameter '{id}' does not exist"), place);
                    None
                }
            }
            SheetChip::State(id) => {
                match governing {
                    None => self.error(format!("the state '{id}' has no property to belong to"), place),
                    Some(property) if !self.has_state(property, id) => {
                        self.error(format!("'{id}' is no state of '{property}'"), place);
                    }
                    Some(_) => {}
                }
                Some(ValueType::State)
            }
            SheetChip::My(id) | SheetChip::MyNew(id) | SheetChip::Their(id) => {
                let thing = if matches!(chip, SheetChip::Their(_)) {
                    let Some(looked) = context.looked else {
                        self.error("'their' is read outside a look around", place);
                        return None;
                    };
                    looked
                } else {
                    context.thing
                };
                let Some(property) = self.property_of(id, thing) else {
                    self.error(format!("'{id}' is no property of '{thing}'"), place);
                    return None;
                };
                if matches!(chip, SheetChip::MyNew(_)) {
                    self.my_new(id, context, want, state);
                }
                Some(property_type(property.property_type))
            }
            SheetChip::Relation(relation) => {
                if context.looked.is_none() {
                    self.error(
                        format!("the relation '{}' is read outside a look around", relation.as_str()),
                        place,
                    );
                }
                if *relation == crate::sheet::document::ChipRelation::DistanceToThem {
                    Some(ValueType::Number)
                } else {
                    Some(ValueType::Arrow)
                }
            }
            SheetChip::Result { line, part, property } => {
                self.result(line, part.as_deref(), property.as_deref(), context, want, state)
            }
        }
    }

    /// Checks a `my_new` chip of property `id`. A line offers one only below the last change to the property.
    fn my_new(&mut self, id: &str, context: &Context<'a>, want: Want, state: &mut LineState) {
        let Some(at) = context.line else {
            self.error(format!("my new {id} is read outside a line"), &context.place);
            return;
        };
        let relations = &self.relations[at.card];
        let last = relations
            .order()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line_id)| relations.line(line_id).is_some_and(|line| self.changes(line, id)))
            .map(|(index, _)| index);
        let name = format!("my new {}", self.property_name(id));
        match last {
            None => state.skip(|| sentence::not_run_choose(want.words())),
            Some(last) if last >= at.position => state.skip(|| sentence::uses_result_below(&name)),
            Some(_) => {}
        }
    }

    /// Returns whether `line` changes property `id`. A Move changes the Position and the Heading.
    fn changes(&self, line: &SheetLine, id: &str) -> bool {
        changes_property(
            line,
            id,
            self.properties.get(id).map(|(property, _)| property.property_type),
        )
    }

    /// Returns the name of property `id`, its id when no property has it.
    fn property_name(&self, id: &str) -> String {
        self.properties
            .get(id)
            .map_or_else(|| id.to_owned(), |(property, _)| property.name.clone())
    }

    /// Checks a result chip of line `id`, with its `part` or the `property` of the place it names, and returns its
    /// type.
    fn result(
        &mut self,
        id: &str,
        part: Option<&str>,
        property: Option<&str>,
        context: &Context<'a>,
        want: Want,
        state: &mut LineState,
    ) -> Option<ValueType> {
        let place = &context.place;
        let Some(at) = context.line else {
            self.error(format!("the result of {id} is read outside a line"), place);
            return None;
        };
        let Some(&card) = self.line_cards.get(id) else {
            // The result of a deleted line reads as a blank.
            state.skip(|| sentence::not_run_choose(want.words()));
            return None;
        };
        if card != at.card {
            self.error(format!("{id} sits in another card"), place);
            return None;
        }
        let source = &self.document.cards[card].line_tables[id];
        if !is_result_kind(source.kind) {
            self.error(format!("{id} makes a change and gives no result"), place);
            return None;
        }
        let relations = &self.relations[at.card];
        let Some(index) = relations.index(id) else {
            state.skip(|| sentence::not_run_choose(want.words()));
            return None;
        };
        if index >= at.position {
            let name = self.result_name(id);
            state.skip(|| sentence::uses_result_below(&name));
        } else if let Some(block) = relations.parent(id)
            && !relations.is_within(at.id, block)
        {
            let number = relations.number(block).unwrap_or(0);
            let name = self.result_name(id);
            state.skip(|| sentence::uses_result_inside(&name, number));
        }
        state.results.push(id.to_owned());
        if let Some(part) = part {
            return self.part_type(source, part, id, place);
        }
        if let Some(property) = property {
            if self.phrase_type(source) != Some(ValueType::Place) {
                self.error(format!("{id} is no place, so it has no properties"), place);
                return None;
            }
            let ground = ground_of(context.thing);
            let Some(found) = self.property_of(property, ground) else {
                self.error(format!("'{property}' is no property of '{ground}'"), place);
                return None;
            };
            return Some(property_type(found.property_type));
        }
        if source.kind == LineKind::LookAround {
            self.error(format!("{id} gives several values, so its chip names a part"), place);
            return None;
        }
        self.phrase_type(source)
    }

    /// Returns the type of part `part` of `source`, the line `id`.
    fn part_type(&mut self, source: &SheetLine, part: &str, id: &str, place: &Place) -> Option<ValueType> {
        if source.kind == LineKind::LookAround {
            if part == "count" {
                return Some(ValueType::Number);
            }
            if let Some(summary) = source.summaries.iter().find(|summary| summary.id == part) {
                return match summary.summary {
                    Some(SummaryKind::Any | SummaryKind::All) => Some(ValueType::YesNo),
                    Some(SummaryKind::Count) => Some(ValueType::Number),
                    _ => match summary.value.value() {
                        Some(SheetValue::Chip(SheetChip::Relation(relation))) => {
                            if *relation == crate::sheet::document::ChipRelation::DistanceToThem {
                                Some(ValueType::Number)
                            } else {
                                Some(ValueType::Arrow)
                            }
                        }
                        Some(SheetValue::Chip(SheetChip::Their(property))) => self
                            .properties
                            .get(property.as_str())
                            .map(|(found, _)| property_type(found.property_type)),
                        _ => None,
                    },
                };
            }
        }
        if part == "amount"
            && matches!(
                source.kind,
                LineKind::PickCell | LineKind::DependingOn | LineKind::Choose
            )
        {
            return Some(ValueType::Number);
        }
        if part == "open" && self.phrase_type(source) == Some(ValueType::Place) {
            return Some(ValueType::YesNo);
        }
        self.error(
            format!("'{part}' is no part of {id}, a {} line", source.kind.as_str()),
            place,
        );
        None
    }

    /// Returns the type of the value a result line or phrase gives, `None` when it cannot be known.
    fn phrase_type(&self, line: &SheetLine) -> Option<ValueType> {
        self.line_type(line, 0)
    }

    /// Returns the type of the value `line` gives, `depth` results and phrases deep in the walk that reached it.
    ///
    /// A walk past [`MAX_NESTING`] returns `None`. A walk around a cycle of results ends there.
    fn line_type(&self, line: &SheetLine, depth: usize) -> Option<ValueType> {
        if depth > MAX_NESTING {
            return None;
        }
        match line.kind {
            LineKind::Compare
            | LineKind::AllOf
            | LineKind::AnyOf
            | LineKind::NoneOf
            | LineKind::Chance
            | LineKind::Rate
            | LineKind::Always => Some(ValueType::YesNo),
            LineKind::Count | LineKind::RandomNumber => Some(ValueType::Number),
            LineKind::RandomDirection => Some(ValueType::Arrow),
            LineKind::PickCell | LineKind::LookAtCell => Some(ValueType::Place),
            LineKind::Choose => {
                let values = line
                    .rows
                    .iter()
                    .map(|row| &row.value)
                    .chain([&line.otherwise])
                    .filter_map(Slot::value);
                self.one_type(values, depth)
            }
            LineKind::DependingOn => self.one_type(line.branches.values(), depth),
            _ => None,
        }
    }

    /// Returns the type all of `values` share, `None` when they share none that can be known.
    fn one_type<'v>(&self, values: impl Iterator<Item = &'v SheetValue>, depth: usize) -> Option<ValueType> {
        let mut types = values.map(|value| self.value_type(value, depth + 1));
        let first = types.next()??;
        types.all(|other| other == Some(first)).then_some(first)
    }

    /// Returns the type of `value` without checking it, `depth` deep in the walk that reached it.
    fn value_type(&self, value: &SheetValue, depth: usize) -> Option<ValueType> {
        match value {
            SheetValue::Phrase(phrase) => self.line_type(phrase, depth),
            SheetValue::Chip(SheetChip::Result {
                line,
                part: None,
                property: None,
            }) => {
                let (_, source) = self.document.line(line)?;
                self.line_type(source, depth)
            }
            SheetValue::Literal(SheetLiteral::Bool(_)) => Some(ValueType::YesNo),
            SheetValue::Literal(_) => Some(ValueType::Number),
            _ => None,
        }
    }

    /// Checks the range of a chance or a rate in `slot`: a literal chance from 0 to 1 and a literal rate from 0, and a
    /// parameter used as a chance whose range stays from 0 to 1 or used as a rate whose minimum is 0 or more.
    pub(super) fn draw_range(&mut self, slot: &Slot, context: &Context<'a>, draw: Draw) {
        match slot.value() {
            Some(SheetValue::Literal(literal)) => {
                let Some(number) = literal.number() else {
                    return;
                };
                match draw {
                    Draw::Chance if !(0.0..=1.0).contains(&number) => self.error(CHANCE_OUT_OF_RANGE, &context.place),
                    Draw::Rate if number < 0.0 => self.error(RATE_BELOW_ZERO, &context.place),
                    _ => {}
                }
            }
            Some(SheetValue::Chip(SheetChip::Param(id))) => {
                let Some(parameter) = self.parameters.get(id.as_str()) else {
                    return;
                };
                match draw {
                    Draw::Chance if parameter.min < 0.0 || parameter.max > 1.0 => {
                        self.error(CHANCE_LIMITS_OUTSIDE, &context.place);
                    }
                    Draw::Rate if parameter.min < 0.0 => self.error(RATE_BELOW_ZERO, &context.place),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

impl Checker<'_> {
    /// Warns when the rate in `slot` is a parameter whose Unit names another unit of time than `unit`, the line's
    /// unit. The line's unit governs the run.
    fn rate_unit(&mut self, slot: &Slot, unit: TimeUnit, context: &Context<'_>) {
        let Some(SheetValue::Chip(SheetChip::Param(id))) = slot.value() else {
            return;
        };
        let Some(parameter) = self.parameters.get(id.as_str()) else {
            return;
        };
        let Some(own) = parameter.unit.as_deref().and_then(unit_in_text) else {
            return;
        };
        if own != unit {
            let message = sentence::unit_mismatch(&parameter.name, own, unit);
            self.warning(message, &context.place);
        }
    }
}

/// A draw whose range [`Checker::draw_range`] checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Draw {
    /// A chance, from 0 to 1.
    Chance,
    /// A rate, from 0.
    Rate,
}

/// Records the skip note of a blank that wants `want`, or the error of a blank outside the cards.
fn state_skip(checker: &mut Checker<'_>, context: &Context<'_>, state: &mut LineState, want: Want) {
    checker.unusable(
        context,
        state,
        || sentence::not_run_choose(want.words()),
        &format!("a value is blank, where it wants {}", want.words()),
    );
}

/// Returns the kind of thing the cells around a thing of `thing` belong to: `cells` on a grid, `ground` under agents.
fn ground_of(thing: &str) -> &'static str {
    if thing == "cells" { "cells" } else { "ground" }
}

/// Returns the type of a property of `property_type`.
fn property_type(property_type: PropertyType) -> ValueType {
    match property_type {
        PropertyType::States => ValueType::State,
        PropertyType::Number | PropertyType::WholeNumber => ValueType::Number,
        PropertyType::YesNo => ValueType::YesNo,
        PropertyType::Arrow | PropertyType::Position | PropertyType::Heading => ValueType::Arrow,
    }
}

/// Returns the property that a state on the right of a comparison with `left` on its left belongs to.
fn governing_property(left: &SheetValue) -> Option<&str> {
    match left {
        SheetValue::Chip(SheetChip::My(id) | SheetChip::MyNew(id) | SheetChip::Their(id)) => Some(id),
        SheetValue::Chip(SheetChip::Result {
            property: Some(property),
            ..
        }) => Some(property),
        _ => None,
    }
}

/// Returns whether a line of `kind` works out a result. The same kinds stand in a slot as a phrase.
pub(crate) fn is_result_kind(kind: LineKind) -> bool {
    matches!(
        kind,
        LineKind::Count
            | LineKind::LookAround
            | LineKind::PickCell
            | LineKind::LookAtCell
            | LineKind::Chance
            | LineKind::Rate
            | LineKind::RandomNumber
            | LineKind::RandomDirection
            | LineKind::Compare
            | LineKind::AllOf
            | LineKind::AnyOf
            | LineKind::NoneOf
            | LineKind::Choose
            | LineKind::DependingOn
            | LineKind::Calculate
    )
}

/// Returns the chip a reference in a formula names.
pub(in crate::sheet) fn formula_chip(reference: &str, document: &crate::sheet::document::SheetDocument) -> SheetChip {
    let (head, tail) = reference.split_once('.').unwrap_or((reference, ""));
    match head {
        "my" => return SheetChip::My(tail.to_owned()),
        "my_new" => return SheetChip::MyNew(tail.to_owned()),
        _ => {}
    }
    if line_number(head).is_none() {
        return SheetChip::Param(reference.to_owned());
    }
    let is_part = matches!(tail, "amount" | "open" | "count")
        || document
            .line(head)
            .is_some_and(|(_, line)| line.summaries.iter().any(|summary| summary.id == tail));
    SheetChip::Result {
        line: head.to_owned(),
        part: (!tail.is_empty() && is_part).then(|| tail.to_owned()),
        property: (!tail.is_empty() && !is_part).then(|| tail.to_owned()),
    }
}

/// Returns whether `line` changes the property `id`, whose type is `property_type`: a Become, Set, Steer or Keep
/// between of it, or a Move when it is a Position or a Heading.
///
/// A line below the last line that changes a property can read the property's new value, as "my new Velocity".
pub fn changes_property(line: &SheetLine, id: &str, property_type: Option<PropertyType>) -> bool {
    match line.kind {
        LineKind::Become | LineKind::Set | LineKind::Steer | LineKind::KeepBetween => {
            line.property.as_deref() == Some(id)
        }
        LineKind::Move => matches!(property_type, Some(PropertyType::Position | PropertyType::Heading)),
        _ => false,
    }
}

/// Returns whether `line`, a phrase or a value inside it draws random numbers.
///
/// A line that draws holds a key, and [`SheetEdit::DrawNewNumbers`](crate::sheet::edit::SheetEdit::DrawNewNumbers)
/// gives it a new one.
pub fn holds_draw(line: &SheetLine) -> bool {
    matches!(
        line.kind,
        LineKind::Chance | LineKind::Rate | LineKind::RandomNumber | LineKind::RandomDirection | LineKind::PickCell
    ) || line.slot_values().any(value_draws)
}

/// Returns whether `value` draws random numbers.
pub(super) fn value_draws(value: &SheetValue) -> bool {
    match value {
        SheetValue::Phrase(phrase) => holds_draw(phrase),
        SheetValue::List(items) => items.iter().any(value_draws),
        _ => false,
    }
}

/// Returns whether `line` or a phrase inside it draws a rate.
fn holds_rate(line: &SheetLine) -> bool {
    line.kind == LineKind::Rate || line.slot_values().any(value_rates)
}

/// Returns whether `value` draws a rate.
pub(super) fn value_rates(value: &SheetValue) -> bool {
    match value {
        SheetValue::Phrase(phrase) => holds_rate(phrase),
        SheetValue::List(items) => items.iter().any(value_rates),
        _ => false,
    }
}
