//! Cards, lines and the values in their slots: references, types, blanks, scopes, the refusals of rates and of
//! chances outside 0 to 1, and the warning on a rate read in another unit than its parameter's.
//!
//! The slots of a line are checked in the order its sentence reads them, so the first blank or unusable value of a
//! line words its skip note, as the card shows it.

use std::collections::{HashMap, HashSet};

use henad_core::diagnostic::DiagnosticSeverity;
use henad_core::metadata::TimeUnit;

use super::{Checker, Place, Thing};
use super::{declarations, formula};
use crate::sheet::document::SheetDocument;
use crate::sheet::document::{
    Around, CELLS_ID, CardRuns, CellWhich, ChipRelation, CompareTest, GROUND_ID, LineKind, LineMode, MadeOf, Marks,
    PickRule, PropertyType, SheetChip, SheetLine, SheetLiteral, SheetProperty, SheetValue, Slot, SummaryKind,
};
use crate::sheet::edit::lines::{parse_path, set_value};
use crate::sheet::edit::{slot_value, unit_in_text};
use crate::sheet::limits::{ADD_UP_RESOLUTION_SHIFT, MAX_NESTING};
use crate::sheet::sentence::{
    self, ANY_STATE_ID, ARROW_COMPARED_WITH_NUMBER, Blank, CHANCE_LIMITS_OUTSIDE, CHANCE_OUT_OF_RANGE, LineSelect,
    NOT_RUN_TICK, PlaceTarget, RATE_BELOW_ZERO, RATE_IN_BUTTON_CARD, RATE_IN_REPEAT,
};
use crate::sheet::writer::format_number;

/// Kind of value a blank wants. Its words fill the skip note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Want {
    /// A blank of a chip or a value.
    Blank(Blank),
    /// A select inside a line.
    Select(LineSelect),
}

impl Want {
    /// Words after "choose" in the skip note, such as "a state" or "a unit of time".
    fn words(self) -> &'static str {
        match self {
            Self::Blank(blank) => blank.want(),
            Self::Select(select) => select.want(),
        }
    }
}

/// Kind of a value, as the checker reads it: what a place of a line takes and what a value there gives.
///
/// [`value_kind`] returns the kind of a value, and [`place_refusal`] the kinds a refused value and its place differ in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueKind {
    /// Yes or no, a Boolean.
    YesNo,
    /// A number.
    Number,
    /// A vector: a position, a heading or an offset.
    Arrow,
    /// A place: a cell of the grid or the ground.
    Place,
    /// A state of a States property.
    State,
    /// A list of values.
    List,
}

impl ValueKind {
    /// Returns the type of a property of type `property_type`.
    fn of_property(property_type: PropertyType) -> Self {
        match property_type {
            PropertyType::States => Self::State,
            PropertyType::Number | PropertyType::WholeNumber => Self::Number,
            PropertyType::YesNo => Self::YesNo,
            PropertyType::Arrow | PropertyType::Position | PropertyType::Heading => Self::Arrow,
        }
    }

    /// Returns the type of the value of the relation `relation`: a number for the distance to them, and an arrow
    /// otherwise.
    fn of_relation(relation: ChipRelation) -> Self {
        if relation == ChipRelation::DistanceToThem {
            Self::Number
        } else {
            Self::Arrow
        }
    }

    /// Words of the kind in a sentence of the card, such as "a vector" or "a cell".
    pub fn sentence_words(self) -> &'static str {
        match self {
            Self::YesNo => sentence::BOOLEAN_WORDS,
            Self::Number => sentence::NUMBER_WORDS,
            Self::Arrow => sentence::VECTOR_WORDS,
            Self::Place => sentence::CELL_WORDS,
            Self::State => sentence::STATE_WORDS,
            Self::List => sentence::LIST_WORDS,
        }
    }

    /// Words of the type in a message.
    fn words(self) -> &'static str {
        match self {
            Self::YesNo => "a Boolean",
            Self::Number => "a number",
            Self::Arrow => "a vector",
            Self::Place => "a place",
            Self::State => "a state",
            Self::List => "a list",
        }
    }
}

/// The line a value belongs to.
#[derive(Debug, Clone, Copy)]
pub(super) struct OwnerLine<'a> {
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

/// Place a value is checked in.
#[derive(Debug, Clone)]
pub(super) struct Context<'a> {
    /// Table a problem is placed at.
    place: Place,
    /// Line the value belongs to, `None` for a value outside the cards.
    line: Option<OwnerLine<'a>>,
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

/// Findings of the walk of one line that decide whether it runs.
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
                    format!("invalid line id '{line_id}', expected l followed by a positive integer"),
                    &place,
                ),
                Some(number) if number >= u64::from(next_line) => {
                    checker.error(
                        format!("the number of line {line_id} must be less than [sheet] next_line {next_line}"),
                        &place,
                    );
                }
                Some(_) => {}
            }
            if !seen.insert(line_id) {
                checker.error(format!("line {line_id} is in two cards"), &place);
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
    let every_is_known = document.world.made_of == MadeOf::Cells && card.every == CELLS_ID
        || document.agents.iter().any(|kind| kind.id == card.every);
    if !every_is_known {
        checker.error(
            format!(
                "card '{}' is for '{}', which is not a kind of thing with a card",
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
            0 => checker.error(format!("line {id} is not listed in any lines list"), &line_place),
            1 if checker.relations[index].index(id).is_none() => checker.error(
                format!("line {id} is inside a block that no line of the card reaches"),
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
        let owner = OwnerLine {
            card: index,
            id,
            position,
            in_repeat: checker.relations[index].repeat_of(id).is_some(),
            runs_once: card.runs == CardRuns::Once,
        };
        if let Some(note) = check_line(checker, line, owner, &skipped) {
            checker.skip(note, index, id);
            skipped.insert(id);
        }
        if let Some(name) = line.name.as_deref().filter(|_| line.kind.works_out_result()) {
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

/// Checks one line of a card and returns its skip note, `None` for a line that runs.
///
/// `skipped` holds the lines above it that do not run.
fn check_line<'a>(
    checker: &mut Checker<'a>,
    line: &SheetLine,
    owner: OwnerLine<'a>,
    skipped: &HashSet<&str>,
) -> Option<String> {
    let document = checker.document;
    let card = &document.cards[owner.card];
    let place = Place::Line(owner.card, owner.id.to_owned());
    let context = Context {
        place: place.clone(),
        line: Some(owner),
        thing: card.every.as_str(),
        looked: None,
        condition: false,
        whose: false,
    };
    // The card's words for the line, such as "line 3" or "Clear line 1".
    let word = checker.relations[owner.card].line_word(owner.id);
    if line.kind == LineKind::Always {
        checker.error(
            format!("{word}: always is a condition and cannot be a line kind"),
            &place,
        );
        return None;
    }
    if line.name.is_some() && !line.kind.works_out_result() {
        checker.error(format!("{word}: a line that makes a change cannot have a name"), &place);
    }
    if owner.in_repeat && matches!(line.kind, LineKind::Count | LineKind::LookAround | LineKind::PickCell) {
        checker.error(sentence::LOOK_IN_REPEAT.to_owned(), &place);
    }
    if owner.runs_once && matches!(line.kind, LineKind::Count | LineKind::LookAround | LineKind::PickCell) {
        checker.error(format!("{word}: a card that runs once cannot look around"), &place);
    }
    let mut state = LineState::default();
    checker.body(line, &context, &mut state);
    checker.key(line.key, word, holds_draw(line), &place);

    if state.skip.is_none() {
        let relations = &checker.relations[owner.card];
        let unfinished = state.results.iter().find_map(|result| {
            let source = relations.index(result)?;
            (source < owner.position && skipped.contains(result.as_str())).then_some((result, source + 1))
        });
        if let Some((result, number)) = unfinished {
            // A result that reads as its line word names the line already.
            let name = checker.result_name(result);
            state.skip = Some(if name == sentence::line_word(None, number) {
                sentence::not_run_when_unfinished(number)
            } else {
                sentence::not_run_result_unfinished(&name, number)
            });
        }
    }
    if state.skip.is_none() && holds_rate(line) && document.time.is_none() {
        state.skip(|| NOT_RUN_TICK.to_owned());
    }
    if state.skip.is_none() {
        let relations = &checker.relations[owner.card];
        // A result inside a skipped When still runs, and every line inside a skipped Repeat is skipped, however deep.
        let blocked_by_parent = relations.parent(owner.id).filter(|parent| {
            let parent_kind = relations.line(parent).map(|block| block.kind);
            let depends = parent_kind == Some(LineKind::Repeat) || !line.kind.works_out_result();
            depends && skipped.contains(parent)
        });
        let blocked = blocked_by_parent.or_else(|| {
            let mut ancestor = relations.parent(owner.id);
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
    /// Returns the name of the result of line `id` as its chip reads: its name, the automatic name of an unnamed
    /// Count, or else its line word, such as "line 4".
    fn result_name(&self, id: &str) -> String {
        let chip = SheetChip::Result {
            line: id.to_owned(),
            part: None,
            property: None,
        };
        self.sentences.value(&SheetValue::Chip(chip), None)
    }

    /// Returns the name the sheet shows for `place`: the name of its property, measure or button, or the heading of
    /// the section it sits in.
    fn place_name(&self, place: &Place) -> String {
        let document = self.document;
        let named = match place {
            Place::Property(thing, index) => thing
                .properties(document)
                .get(*index)
                .map(|property| property.name.clone()),
            Place::Measure(index) => document.measures.get(*index).map(|measure| measure.name.clone()),
            Place::Button(index) => document.buttons.get(*index).map(|button| button.name.clone()),
            _ => None,
        };
        named.unwrap_or_else(|| {
            match place {
                Place::World => "World",
                Place::Time => "Time",
                Place::Parameter(_) => "Parameters",
                Place::Measure(_) => "Measures",
                Place::Button(_) => "Buttons",
                Place::Color(_) => "Colours",
                _ => "The sheet",
            }
            .to_owned()
        })
    }

    /// Records a blank `slot` that wants `want`: a skip note for a slot in a line, and the error of an empty field for
    /// a slot outside the cards.
    fn blank(&mut self, context: &Context<'_>, state: &mut LineState, want: Want) {
        let error = sentence::empty_field(&self.place_name(&context.place), want.words());
        self.unusable(context, state, || sentence::not_run_choose(want.words()), &error);
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
    /// `blank` is what a blank slot wants, `expect` the type the slot takes, and `governing` the property that a state
    /// in the slot belongs to.
    fn slot(
        &mut self,
        slot: &Slot,
        context: &Context<'a>,
        blank: Blank,
        expect: Option<ValueKind>,
        governing: Option<&str>,
        state: &mut LineState,
    ) -> Option<ValueKind> {
        match slot.value() {
            None => {
                self.blank(context, state, Want::Blank(blank));
                None
            }
            Some(value) => self.value(value, context, blank, expect, governing, state),
        }
    }

    /// Checks the number in `slot`, as [`Self::slot`] does, where `blank` is what a blank slot wants.
    fn number_slot(&mut self, slot: &Slot, context: &Context<'a>, blank: Blank, state: &mut LineState) {
        self.slot(slot, context, blank, Some(ValueKind::Number), None, state);
    }

    /// Checks the yes or no in `slot`, a condition, as [`Self::slot`] does.
    fn condition_slot(&mut self, slot: &Slot, context: &Context<'a>, state: &mut LineState) {
        let condition = context.with_condition(true);
        self.slot(slot, &condition, Blank::When, Some(ValueKind::YesNo), None, state);
    }

    /// Checks the value in `slot`, a slot outside the cards, as [`Self::slot`] does.
    pub(super) fn outside_slot(
        &mut self,
        slot: &Slot,
        context: &Context<'a>,
        blank: Blank,
        expect: Option<ValueKind>,
        governing: Option<&str>,
    ) {
        self.slot(slot, context, blank, expect, governing, &mut LineState::default());
    }

    /// Checks `value`, as [`Self::slot`] checks a filled slot.
    fn value(
        &mut self,
        value: &SheetValue,
        context: &Context<'a>,
        blank: Blank,
        expect: Option<ValueKind>,
        governing: Option<&str>,
        state: &mut LineState,
    ) -> Option<ValueKind> {
        let found = match value {
            SheetValue::Literal(SheetLiteral::Bool(_)) => Some(ValueKind::YesNo),
            SheetValue::Literal(_) => Some(ValueKind::Number),
            SheetValue::List(items) => {
                // An empty list reads as a blank. It holds nothing to compare with.
                if items.is_empty() {
                    self.blank(context, state, Want::Blank(blank));
                    return None;
                }
                for item in items {
                    self.value(item, context, blank, None, governing, state);
                }
                return Some(ValueKind::List);
            }
            SheetValue::Phrase(phrase) => {
                self.phrase(phrase, context, state);
                self.phrase_type(phrase)
            }
            SheetValue::Chip(chip) => self.chip(chip, context, blank, governing, state),
        };
        if let (Some(expect), Some(found)) = (expect, found)
            && expect != found
        {
            self.error(
                format!("expected {} in a slot, got {}", expect.words(), found.words()),
                &context.place,
            );
            self.mismatches.push((expect, found));
        }
        found
    }

    /// Checks a phrase in a slot.
    fn phrase(&mut self, phrase: &SheetLine, context: &Context<'a>, state: &mut LineState) {
        if phrase.kind == LineKind::Always {
            if !context.condition {
                self.error("always can only be used in a condition", &context.place);
            }
            return;
        }
        if !phrase.kind.works_out_result() {
            self.error(
                format!(
                    "a {} line returns no value and cannot be used in a slot",
                    phrase.kind.as_str()
                ),
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
        let place = &context.place;
        match line.kind {
            LineKind::Count | LineKind::LookAround => self.look(line, context, state),
            LineKind::PickCell => {
                match line.around {
                    None => self.blank(context, state, Want::Select(LineSelect::WhichCells)),
                    Some(Around::Cells8 | Around::Cells4) => {}
                    Some(_) => self.error("Pick a cell can only pick among the 8 or the 4 cells around", place),
                }
                let ground = ground_of(context.thing);
                match line.pick {
                    None => self.error("Pick a cell needs a pick rule: most, least or random", place),
                    Some(PickRule::Random) => {}
                    Some(_) => match line.ranked_by.as_deref() {
                        None => self.blank(context, state, Want::Blank(Blank::GroundProperty)),
                        Some(property) if self.property_of(property, ground).is_none() => {
                            self.error(format!("'{property}' is not a property of '{ground}'"), place);
                        }
                        Some(_) => {}
                    },
                }
                if !line.only.is_blank() {
                    let looking = Context {
                        looked: Some(ground),
                        ..inner.with_condition(true)
                    };
                    self.condition_slot(&line.only, &looking, state);
                }
            }
            LineKind::LookAtCell => match line.which {
                None => self.error("Look at a cell needs a cell: here, ahead or at", place),
                Some(CellWhich::At) => {
                    self.slot(&line.place, &inner, Blank::Value, Some(ValueKind::Place), None, state);
                }
                Some(_) => {}
            },
            LineKind::Chance => {
                self.number_slot(&line.chance, &inner, Blank::Chance, state);
                self.draw_range(&line.chance, context, Draw::Chance);
                if !line.tries.is_blank() {
                    self.number_slot(&line.tries, &inner, Blank::Count, state);
                }
            }
            LineKind::Rate => {
                if context.line.is_some_and(|owner| owner.runs_once) {
                    self.error(RATE_IN_BUTTON_CARD, place);
                }
                if context.line.is_some_and(|owner| owner.in_repeat) {
                    self.error(RATE_IN_REPEAT, place);
                }
                self.number_slot(&line.rate, &inner, Blank::Rate, state);
                self.draw_range(&line.rate, context, Draw::Rate);
                match line.rate_unit {
                    None => self.blank(context, state, Want::Select(LineSelect::UnitOfTime)),
                    Some(unit) => self.rate_unit(&line.rate, unit, context),
                }
                if !line.tries.is_blank() {
                    self.number_slot(&line.tries, &inner, Blank::Count, state);
                }
            }
            LineKind::RandomNumber => {
                self.number_slot(&line.low, &inner, Blank::LowerNumber, state);
                self.number_slot(&line.high, &inner, Blank::UpperNumber, state);
            }
            LineKind::RandomDirection => {
                self.number_slot(&line.length, &inner, Blank::Number, state);
            }
            LineKind::Compare => self.compare(line, context, state),
            LineKind::AllOf | LineKind::AnyOf | LineKind::NoneOf => {
                if line.items.is_empty() {
                    self.blank(context, state, Want::Blank(Blank::YesOrNoList));
                }
                for item in &line.items {
                    self.value(
                        item,
                        &context.with_condition(context.condition),
                        Blank::YesOrNo,
                        Some(ValueKind::YesNo),
                        None,
                        state,
                    );
                }
            }
            LineKind::Choose => {
                for row in &line.rows {
                    self.slot(&row.value, &inner, Blank::Value, None, None, state);
                    self.condition_slot(&row.condition, context, state);
                }
                self.slot(&line.otherwise, &inner, Blank::Value, None, None, state);
            }
            LineKind::DependingOn => {
                let Some(property) = self.changed_property(line, context, state, Want::Blank(Blank::Property)) else {
                    for value in line.branches.values() {
                        self.value(value, &inner, Blank::Value, None, None, state);
                    }
                    return;
                };
                if property.property_type != PropertyType::States {
                    self.error(
                        format!("Depending on needs a States property, got '{}'", property.id),
                        place,
                    );
                    for value in line.branches.values() {
                        self.value(value, &inner, Blank::Value, None, None, state);
                    }
                    return;
                }
                for (key, value) in &line.branches {
                    if !property.states.iter().any(|held| held.id == *key) {
                        self.error(format!("'{key}' is not a state of '{}'", property.id), place);
                        self.value(value, &inner, Blank::Value, None, None, state);
                    }
                }
                // A state with no value yet is a blank of the line, in the order the sentence shows the states.
                for held in &property.states {
                    match line.branches.get(&held.id) {
                        None => self.blank(context, state, Want::Blank(Blank::Value)),
                        Some(value) => {
                            self.value(value, &inner, Blank::Value, None, None, state);
                        }
                    }
                }
            }
            LineKind::Calculate => match line.formula.as_deref().filter(|formula| !formula.trim().is_empty()) {
                None => self.blank(context, state, Want::Blank(Blank::Number)),
                Some(formula) => match formula::checked_references(formula) {
                    Err(reason) => self.error(reason, place),
                    Ok(references) => {
                        for reference in references {
                            let chip = formula::formula_chip(reference, self.document);
                            self.chip(&chip, &inner, Blank::Number, None, state);
                        }
                    }
                },
            },
            LineKind::Become => {
                match (line.new_state.as_deref(), line.property.as_deref()) {
                    (None, _) | (_, None) => self.blank(context, state, Want::Blank(Blank::State)),
                    (Some(new), Some(id)) => match self.property_of(id, context.thing) {
                        None => self.error(format!("'{id}' is not a property of '{}'", context.thing), place),
                        Some(property) => {
                            if property.property_type != PropertyType::States {
                                self.error(format!("Become needs a States property, got '{id}'"), place);
                            } else if !self.has_state(id, new) {
                                self.error(format!("new_state '{new}' is not a state of '{id}'"), place);
                            }
                        }
                    },
                }
                match line.start_state.as_deref() {
                    None => self.blank(context, state, Want::Blank(Blank::OldState)),
                    Some(ANY_STATE_ID) => {}
                    Some(old) => {
                        if let Some(id) = line.property.as_deref()
                            && self.property_of(id, context.thing).is_some()
                            && !self.has_state(id, old)
                        {
                            self.error(format!("start_state '{old}' is not a state of '{id}'"), place);
                        }
                    }
                }
                self.condition(line, context, state, true);
            }
            LineKind::Set => {
                // A Set over cells changes a number, and a Set over agents any property but its place.
                let (want, value_want) = if matches!(context.thing, CELLS_ID | GROUND_ID) {
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
                            "Set cannot change '{}': it is a States, Position or Heading property",
                            property.id
                        ),
                        place,
                    );
                }
                let governing = property.map(|property| property.id.as_str());
                let found = self.slot(&line.value, &inner, value_want, None, governing, state);
                if let (Some(property), Some(found)) = (property, found)
                    && let Some(expect) = set_takes(property.property_type, found, line.value.value())
                {
                    self.error(
                        format!("expected {} in a slot, got {}", expect.words(), found.words()),
                        place,
                    );
                    self.mismatches.push((expect, found));
                }
                self.condition(line, context, state, true);
            }
            LineKind::Steer => {
                let property = self.changed_property(line, context, state, Want::Blank(Blank::Property));
                let steered = property.map(|property| property.property_type);
                if let Some(property) = property
                    && !matches!(property.property_type, PropertyType::Number | PropertyType::Arrow)
                {
                    self.error(
                        format!("Steer needs a Number or a Vector, got '{}'", property.id),
                        place,
                    );
                }
                if line.mode.is_none_or(|mode| mode == LineMode::To) {
                    self.error("Steer needs a mode: by or towards", place);
                }
                // The value takes the type of the property it steers.
                let expect = steered
                    .filter(|steered| matches!(steered, PropertyType::Number | PropertyType::Arrow))
                    .map(ValueKind::of_property);
                self.slot(&line.value, &inner, Blank::Value, expect, None, state);
                self.number_slot(&line.strength, &inner, Blank::Number, state);
                self.condition(line, context, state, false);
            }
            LineKind::KeepBetween => {
                let property = self.changed_property(line, context, state, Want::Blank(Blank::Property));
                if let Some(property) = property
                    && matches!(property.property_type, PropertyType::States | PropertyType::YesNo)
                {
                    self.error(
                        format!("Keep between needs a Number or a Vector, got '{}'", property.id),
                        place,
                    );
                }
                self.number_slot(&line.minimum, &inner, Blank::LowerNumber, state);
                self.number_slot(&line.maximum, &inner, Blank::UpperNumber, state);
                self.condition(line, context, state, false);
            }
            LineKind::Move => {
                if matches!(context.thing, CELLS_ID | GROUND_ID) {
                    self.error("only agents move", place);
                }
                let expect = match line.mode {
                    Some(LineMode::To) => Some(ValueKind::Place),
                    Some(LineMode::By) => Some(ValueKind::Arrow),
                    _ => {
                        self.error("Move needs a mode: by or to", place);
                        None
                    }
                };
                self.slot(&line.value, &inner, Blank::Value, expect, None, state);
                self.condition(line, context, state, false);
            }
            LineKind::Leave => {
                self.number_slot(&line.amount, &inner, Blank::Number, state);
                match line.ground_property.as_deref() {
                    None => self.blank(context, state, Want::Blank(Blank::GroundProperty)),
                    Some(id) => {
                        let fits = self
                            .property_of(id, GROUND_ID)
                            .is_some_and(|property| property.property_type == PropertyType::Number);
                        if fits {
                            self.add_up_limit(line, id, context);
                        } else {
                            self.error(
                                format!("Leave needs a Number property of the ground, got '{id}'"),
                                place,
                            );
                        }
                    }
                }
                self.condition(line, context, state, true);
            }
            LineKind::Record => {
                match line.event.as_deref() {
                    None => self.blank(context, state, Want::Blank(Blank::Event)),
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
                self.condition_slot(&line.condition, context, state);
            }
            LineKind::Repeat => {
                self.number_slot(&line.times, &inner, Blank::Times, state);
            }
            LineKind::Always => {}
        }
    }

    /// Checks the condition of a change. `required` says whether the change must hold one.
    fn condition(&mut self, line: &SheetLine, context: &Context<'a>, state: &mut LineState, required: bool) {
        if required || !line.condition.is_blank() {
            self.condition_slot(&line.condition, context, state);
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
    ) -> Option<&'a SheetProperty> {
        let Some(id) = line.property.as_deref() else {
            self.blank(context, state, want);
            return None;
        };
        let property = self.property_of(id, context.thing);
        if property.is_none() {
            self.error(
                format!("'{id}' is not a property of '{}'", context.thing),
                &context.place,
            );
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
            None => self.blank(context, state, Want::Select(LineSelect::WhichCells)),
            Some(Around::AgentsWithin) => {
                let document = self.document;
                match line.agent_kind.as_deref() {
                    Some(kind) => match document.agents.iter().find(|agents| agents.id == kind) {
                        Some(agents) => looked = agents.id.as_str(),
                        None => self.error(format!("unknown kind of agent '{kind}'"), place),
                    },
                    None => self.error("a look at agents needs a kind of agent", place),
                }
                self.number_slot(&line.distance, &context.inner(), Blank::Number, state);
                if line.distance.value().is_some_and(|distance| !fixed_number(distance)) {
                    self.error(sentence::DISTANCE_NOT_FIXED, place);
                }
                if line.include_me.is_none() {
                    self.error("a look at agents must say whether it includes me (include_me)", place);
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
                Blank::Property,
                Some(ValueKind::YesNo),
                None,
                state,
            );
        } else if !line.whose.is_blank() {
            self.condition_slot(&line.whose, &looking, state);
        }
        if !line.only.is_blank() {
            self.condition_slot(&line.only, &looking, state);
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
                None => self.blank(context, state, Want::Select(LineSelect::TotalOrAverage)),
                Some(SummaryKind::Count) => {}
                Some(kind) => {
                    let want = if grid { Blank::NumberProperty } else { Blank::Value };
                    // The largest and the smallest compare numbers, and any and all join yes or no values.
                    let expect = match kind {
                        SummaryKind::Largest | SummaryKind::Smallest => Some(ValueKind::Number),
                        SummaryKind::Any | SummaryKind::All => Some(ValueKind::YesNo),
                        _ => None,
                    };
                    self.slot(&summary.value, &looking, want, expect, None, state);
                }
            }
        }
    }

    /// Checks a compare phrase or line.
    fn compare(&mut self, line: &SheetLine, context: &Context<'a>, state: &mut LineState) {
        let inner = context.inner();
        let left_want = if context.whose { Blank::Property } else { Blank::Value };
        let left = self.slot(&line.left, &inner, left_want, None, None, state);
        let governing = line.left.value().and_then(governing_property);
        let of_states = governing.is_some_and(|id| {
            self.properties
                .get(id)
                .is_some_and(|(property, _)| property.property_type == PropertyType::States)
        });
        match line.test {
            None => self.blank(context, state, Want::Select(LineSelect::Comparison)),
            Some(CompareTest::Outside) => {
                if !context.condition {
                    self.error(
                        "is less than … or more than … can only be used in a condition",
                        &context.place,
                    );
                }
                self.number_slot(&line.low, &inner, Blank::LowerNumber, state);
                self.number_slot(&line.high, &inner, Blank::UpperNumber, state);
                if left == Some(ValueKind::Arrow) {
                    self.error(ARROW_COMPARED_WITH_NUMBER, &context.place);
                } else if let Some(left) = left {
                    self.compared(
                        CompareTest::Outside,
                        (left, false),
                        (ValueKind::Number, false),
                        &context.place,
                    );
                }
            }
            Some(test) => {
                let list = matches!(test, CompareTest::OneOf | CompareTest::NotOneOf);
                let want = match (of_states, list) {
                    (true, true) => Blank::StateList,
                    (true, false) => Blank::State,
                    (false, true) => Blank::NumberList,
                    (false, false) => Blank::Number,
                };
                // Each item of a list is compared with the left side on its own.
                let right: Vec<(Option<ValueKind>, bool)> = match line.right.value() {
                    Some(SheetValue::List(items)) if !items.is_empty() => items
                        .iter()
                        .map(|item| (self.value(item, &inner, want, None, governing, state), is_yes_no(item)))
                        .collect(),
                    value => vec![(
                        self.slot(&line.right, &inner, want, None, governing, state),
                        value.is_some_and(is_yes_no),
                    )],
                };
                if test == CompareTest::Within {
                    self.number_slot(&line.tolerance, &inner, Blank::Number, state);
                }
                let left_literal = line.left.value().is_some_and(is_yes_no);
                for (found, literal) in right {
                    let (Some(left), Some(found)) = (left, found) else {
                        continue;
                    };
                    let sides = [left, found];
                    let fits = if sides.contains(&ValueKind::Arrow) && sides.contains(&ValueKind::Number) {
                        self.error(ARROW_COMPARED_WITH_NUMBER, &context.place);
                        false
                    } else {
                        self.compared(test, (left, left_literal), (found, literal), &context.place)
                    };
                    // One problem is reported for a whole list.
                    if !fits {
                        break;
                    }
                }
            }
        }
    }

    /// Checks the two sides of a comparison by `test`, each a type and whether it is a yes or no typed as such, and
    /// returns whether they can be compared.
    ///
    /// Equal types can be told apart, numbers, numbers and arrows, and arrows can be ordered, and "is within" works
    /// out the distance between two numbers. A yes or no worked out by a line counts as 1 or 0 there.
    fn compared(
        &mut self,
        test: CompareTest,
        (left, left_literal): (ValueKind, bool),
        (right, right_literal): (ValueKind, bool),
        place: &Place,
    ) -> bool {
        let sides = [left, right];
        let problem = match test {
            CompareTest::Is | CompareTest::IsNot | CompareTest::OneOf | CompareTest::NotOneOf => {
                (left != right).then(|| format!("cannot compare {} with {}", left.words(), right.words()))
            }
            CompareTest::Within => sides
                .into_iter()
                .zip([left_literal, right_literal])
                .find(|(side, literal)| *literal || !matches!(side, ValueKind::Number | ValueKind::YesNo))
                .map(|(side, _)| format!("is within compares numbers, got {}", side.words())),
            _ if sides
                .iter()
                .any(|side| matches!(side, ValueKind::YesNo | ValueKind::State)) =>
            {
                Some("states and Boolean values cannot be ordered".to_owned())
            }
            _ => (left != right).then(|| format!("cannot compare {} with {}", left.words(), right.words())),
        };
        match problem {
            Some(problem) => {
                self.error(problem, place);
                false
            }
            None => true,
        }
    }

    /// Checks a chip and returns its type.
    fn chip(
        &mut self,
        chip: &SheetChip,
        context: &Context<'a>,
        blank: Blank,
        governing: Option<&str>,
        state: &mut LineState,
    ) -> Option<ValueKind> {
        let place = &context.place;
        match chip {
            SheetChip::Param(id) => {
                if self.parameters.contains_key(id.as_str()) {
                    Some(ValueKind::Number)
                } else {
                    self.error(format!("the parameter '{id}' does not exist"), place);
                    None
                }
            }
            SheetChip::State(id) => {
                match governing {
                    None => self.error(format!("the state '{id}' has no property to belong to"), place),
                    Some(property) if !self.has_state(property, id) => {
                        self.error(format!("'{id}' is not a state of '{property}'"), place);
                    }
                    Some(_) => {}
                }
                Some(ValueKind::State)
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
                    self.error(format!("'{id}' is not a property of '{thing}'"), place);
                    return None;
                };
                if matches!(chip, SheetChip::MyNew(_)) {
                    self.my_new(id, context, blank, state);
                }
                Some(ValueKind::of_property(property.property_type))
            }
            SheetChip::Relation(relation) => {
                if context.looked.is_none() {
                    self.error(
                        format!("the relation '{}' is read outside a look around", relation.as_str()),
                        place,
                    );
                }
                Some(ValueKind::of_relation(*relation))
            }
            SheetChip::Result { line, part, property } => {
                self.result(line, part.as_deref(), property.as_deref(), context, blank, state)
            }
        }
    }

    /// Checks a `my_new` chip of property `id`. A line offers one only below the last change to the property.
    fn my_new(&mut self, id: &str, context: &Context<'a>, blank: Blank, state: &mut LineState) {
        let Some(owner) = context.line else {
            self.error(format!("my new {id} is read outside a line"), &context.place);
            return;
        };
        let relations = &self.relations[owner.card];
        let last = relations
            .order()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, line_id)| relations.line(line_id).is_some_and(|line| self.changes(line, id)))
            .map(|(index, _)| index);
        let name = format!("my new {}", self.property_name(id));
        match last {
            None => state.skip(|| sentence::not_run_choose(blank.want())),
            Some(last) if last >= owner.position => state.skip(|| sentence::uses_result_below(&name)),
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
        blank: Blank,
        state: &mut LineState,
    ) -> Option<ValueKind> {
        let place = &context.place;
        let Some(owner) = context.line else {
            self.error(format!("the result of {id} is read outside a line"), place);
            return None;
        };
        let Some(&card) = self.line_cards.get(id) else {
            // The result of a deleted line reads as a blank.
            state.skip(|| sentence::not_run_choose(blank.want()));
            return None;
        };
        if card != owner.card {
            self.error(format!("{id} is in another card"), place);
            return None;
        }
        let source = &self.document.cards[card].line_tables[id];
        if !source.kind.works_out_result() {
            self.error(format!("{id} makes a change and returns no result"), place);
            return None;
        }
        let relations = &self.relations[owner.card];
        let Some(index) = relations.index(id) else {
            state.skip(|| sentence::not_run_choose(blank.want()));
            return None;
        };
        if index == owner.position {
            state.skip(|| sentence::USES_OWN_RESULT.to_owned());
        } else if index > owner.position {
            let name = self.result_name(id);
            state.skip(|| sentence::uses_result_below(&name));
        } else if let Some(block) = relations.parent(id)
            && !relations.is_within(owner.id, block)
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
            if self.phrase_type(source) != Some(ValueKind::Place) {
                self.error(format!("{id} is not a place, so it has no properties"), place);
                return None;
            }
            let ground = ground_of(context.thing);
            let Some(found) = self.property_of(property, ground) else {
                self.error(format!("'{property}' is not a property of '{ground}'"), place);
                return None;
            };
            return Some(ValueKind::of_property(found.property_type));
        }
        if source.kind == LineKind::LookAround {
            self.error(
                format!("{id} returns several values, so its chip must name a part"),
                place,
            );
            return None;
        }
        self.phrase_type(source)
    }

    /// Returns the type of part `part` of `source`, the line `id`.
    fn part_type(&mut self, source: &SheetLine, part: &str, id: &str, place: &Place) -> Option<ValueKind> {
        if source.kind == LineKind::LookAround {
            if part == "count" {
                return Some(ValueKind::Number);
            }
            if let Some(summary) = source.summaries.iter().find(|summary| summary.id == part) {
                return match summary.summary {
                    Some(SummaryKind::Any | SummaryKind::All) => Some(ValueKind::YesNo),
                    Some(SummaryKind::Count) => Some(ValueKind::Number),
                    _ => match summary.value.value() {
                        Some(SheetValue::Chip(SheetChip::Relation(relation))) => {
                            Some(ValueKind::of_relation(*relation))
                        }
                        Some(SheetValue::Chip(SheetChip::Their(property))) => self
                            .properties
                            .get(property.as_str())
                            .map(|(found, _)| ValueKind::of_property(found.property_type)),
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
            return Some(ValueKind::Number);
        }
        if part == "open" && self.phrase_type(source) == Some(ValueKind::Place) {
            return Some(ValueKind::YesNo);
        }
        self.error(
            format!("'{part}' is not a part of {id}, a {} line", source.kind.as_str()),
            place,
        );
        None
    }

    /// Returns the type of the value a result line or phrase gives, `None` when it cannot be known.
    fn phrase_type(&self, line: &SheetLine) -> Option<ValueKind> {
        self.line_type(line, 0)
    }

    /// Returns the type of the value `line` gives, `depth` results and phrases deep in the walk that reached it.
    ///
    /// A walk past [`MAX_NESTING`] returns `None`. A walk around a cycle of results ends there.
    fn line_type(&self, line: &SheetLine, depth: usize) -> Option<ValueKind> {
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
            | LineKind::Always => Some(ValueKind::YesNo),
            LineKind::Count | LineKind::RandomNumber => Some(ValueKind::Number),
            LineKind::RandomDirection => Some(ValueKind::Arrow),
            LineKind::PickCell | LineKind::LookAtCell => Some(ValueKind::Place),
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
    fn one_type<'v>(&self, values: impl Iterator<Item = &'v SheetValue>, depth: usize) -> Option<ValueKind> {
        let mut types = values.map(|value| self.value_type(value, depth + 1));
        let first = types.next()??;
        types.all(|other| other == Some(first)).then_some(first)
    }

    /// Returns the type of `value` without checking it, `depth` deep in the walk that reached it.
    fn value_type(&self, value: &SheetValue, depth: usize) -> Option<ValueKind> {
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
            SheetValue::Literal(SheetLiteral::Bool(_)) => Some(ValueKind::YesNo),
            SheetValue::Literal(_) => Some(ValueKind::Number),
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

    /// Warns when the share of a mark kept each tick of `property` leaves 0 to 1: a literal share, or a parameter
    /// whose Max is above 1 or whose Min is below 0.
    ///
    /// The lowering multiplies the property by the share as it is, and the warning says so. Above 1 the marks grow
    /// every tick. A parameter whose range leaves 0 to 1 at both ends warns of its Max.
    pub(super) fn kept_range(&mut self, property: &SheetProperty, context: &Context<'a>) {
        let words = |number: f64| format_number(number).unwrap_or_else(|| number.to_string());
        let message = match property.kept_per_tick.value() {
            Some(SheetValue::Literal(literal)) => match literal.number() {
                Some(number) if !(0.0..=1.0).contains(&number) => Some(sentence::kept_outside_note(
                    &property.name,
                    &sentence::literal_words(*literal),
                    number > 1.0,
                )),
                _ => None,
            },
            Some(SheetValue::Chip(SheetChip::Param(id))) => self.parameters.get(id.as_str()).and_then(|parameter| {
                let (limit, grows) = if parameter.max > 1.0 {
                    (parameter.max, true)
                } else if parameter.min < 0.0 {
                    (parameter.min, false)
                } else {
                    return None;
                };
                Some(sentence::kept_limits_outside_note(
                    &parameter.name,
                    &property.name,
                    &words(limit),
                    grows,
                ))
            }),
            _ => None,
        };
        if let Some(message) = message {
            self.warning(message, &context.place);
        }
    }
}

impl Checker<'_> {
    /// Warns when the Leave `line` on the ground property `id`, which adds up its marks, leaves an amount that the most
    /// agents of the card's kind, all on one cell, can take past what a cell adds up in a tick.
    ///
    /// A number or a parameter's largest value is a fixed amount, and the warning compares their total with the limit.
    /// Any other amount is worked out as the model runs and has no bound. It warns once the kind's `max_count` reaches
    /// the limit.
    fn add_up_limit(&mut self, line: &SheetLine, id: &str, context: &Context<'_>) {
        let Some(property) = self.property_of(id, GROUND_ID) else {
            return;
        };
        if property.marks != Some(Marks::AddUp) {
            return;
        }
        let Some(kind) = self.document.agents.iter().find(|kind| kind.id == context.thing) else {
            return;
        };
        let limit = 2f64.powi(32 - i32::from(ADD_UP_RESOLUTION_SHIFT));
        let words = |number: f64| format_number(number).unwrap_or_else(|| number.to_string());
        let thing = sentence::thing_word(self.document, &kind.id);
        let fixed = match line.amount.value() {
            None => return,
            Some(SheetValue::Literal(literal)) => literal.number(),
            Some(SheetValue::Chip(SheetChip::Param(parameter))) => {
                self.parameters.get(parameter.as_str()).map(|parameter| parameter.max)
            }
            Some(_) => {
                if f64::from(kind.max_count) >= limit {
                    let message =
                        sentence::add_up_worked_out_note(kind.max_count, &thing, &property.name, &words(limit));
                    self.warning(message, &context.place);
                }
                return;
            }
        };
        let Some(amount) = fixed.map(f64::abs) else {
            return;
        };
        let total = f64::from(kind.max_count) * amount;
        if total < limit {
            return;
        }
        let message = sentence::add_up_limit_note(
            kind.max_count,
            &thing,
            &words(amount),
            &words(total),
            &property.name,
            &words(limit),
        );
        self.warning(message, &context.place);
    }

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

/// Returns the kind of thing the cells around a thing of `thing` belong to: `cells` on a grid, `ground` under agents.
fn ground_of(thing: &str) -> &'static str {
    if thing == CELLS_ID { CELLS_ID } else { GROUND_ID }
}

/// Returns the property that a state on the right of a comparison with `left` on its left belongs to.
pub(in crate::sheet) fn governing_property(left: &SheetValue) -> Option<&str> {
    match left {
        SheetValue::Chip(SheetChip::My(id) | SheetChip::MyNew(id) | SheetChip::Their(id)) => Some(id),
        SheetValue::Chip(SheetChip::Result {
            property: Some(property),
            ..
        }) => Some(property),
        _ => None,
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

/// Returns whether `value` is a yes or no typed as such, `true` or `false`.
fn is_yes_no(value: &SheetValue) -> bool {
    matches!(value, SheetValue::Literal(SheetLiteral::Bool(_)))
}

/// Returns the type a Set of a property of type `property_type` expects when it cannot take `value`, of type
/// `found`, and `None` when it takes it.
///
/// A Set takes a value of the property's own type. A Number property also takes a yes or no that a line works out
/// or a property holds, as 1 or 0, and an Arrow property takes a place, as its column and row. A States, Position or
/// Heading property is not checked, since no Set changes one.
fn set_takes(property_type: PropertyType, found: ValueKind, value: Option<&SheetValue>) -> Option<ValueKind> {
    let expect = ValueKind::of_property(property_type);
    let takes = match property_type {
        PropertyType::States | PropertyType::Position | PropertyType::Heading => true,
        PropertyType::Number => found == expect || found == ValueKind::YesNo && !value.is_some_and(is_yes_no),
        PropertyType::Arrow => matches!(found, ValueKind::Arrow | ValueKind::Place),
        _ => found == expect,
    };
    (!takes).then_some(expect)
}

/// Returns whether `value` is the same for every agent and every tick: a typed number or a parameter. The distance of a
/// look at agents takes nothing else.
pub(in crate::sheet) fn fixed_number(value: &SheetValue) -> bool {
    match value {
        SheetValue::Literal(literal) => literal.number().is_some(),
        SheetValue::Chip(SheetChip::Param(_)) => true,
        _ => false,
    }
}

/// Errors the checker finds on one line of a card, and the kinds of the values it finds in a place of another kind.
#[derive(Debug, Default)]
struct LineFindings {
    /// Message of each error, in the order found.
    errors: Vec<String>,
    /// Kind each mismatched place takes and kind of its value, in the order found.
    mismatches: Vec<(ValueKind, ValueKind)>,
}

/// Returns what the checker finds on line `line_id` of card `card` of `document` alone, `None` for a line the card does
/// not show.
fn line_findings(document: &SheetDocument, card: usize, line_id: &str) -> Option<LineFindings> {
    let mut checker = Checker::new(document);
    let relations = checker.relations.get(card)?;
    let position = relations.index(line_id)?;
    let in_repeat = relations.repeat_of(line_id).is_some();
    let card_table = document.cards.get(card)?;
    let (id, line) = card_table.line_tables.get_key_value(line_id)?;
    let owner = OwnerLine {
        card,
        id: id.as_str(),
        position,
        in_repeat,
        runs_once: card_table.runs == CardRuns::Once,
    };
    check_line(&mut checker, line, owner, &HashSet::new());
    let errors = checker
        .findings
        .iter()
        .filter(|finding| finding.severity == DiagnosticSeverity::Error)
        .map(|finding| finding.message.clone())
        .collect();
    Some(LineFindings {
        errors,
        mismatches: checker.mismatches,
    })
}

/// Returns the kind of `value` put in a place of line `line_id` of `document`, as the checker reads it, `None` when
/// the kind cannot be known or no card shows the line.
///
/// A property of the agents a look at agents looks at, as "their Velocity", reads as the property of the line's kind
/// of agent.
pub fn value_kind(document: &SheetDocument, line_id: &str, value: &SheetValue) -> Option<ValueKind> {
    let (card, line) = document.line(line_id)?;
    let mut checker = Checker::new(document);
    let card_table = document.cards.get(card)?;
    let relations = checker.relations.get(card)?;
    let position = relations.index(line_id)?;
    let (id, _) = card_table.line_tables.get_key_value(line_id)?;
    let context = Context {
        place: Place::Line(card, line_id.to_owned()),
        line: Some(OwnerLine {
            card,
            id: id.as_str(),
            position,
            in_repeat: false,
            runs_once: card_table.runs == CardRuns::Once,
        }),
        thing: card_table.every.as_str(),
        looked: line.agent_kind.as_deref(),
        condition: false,
        whose: false,
    };
    checker.value(value, &context, Blank::Value, None, None, &mut LineState::default())
}

/// Why the checker refuses a value in a place of a line, as [`place_refusal`] finds it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlaceRefusal {
    /// Message of the first error the value gives that the line does not have already.
    pub message: String,
    /// Kind the place takes and kind of the value, when the error is that the two differ.
    pub kinds: Option<(ValueKind, ValueKind)>,
    /// Words that name the line the place belongs to, as "Move to" or "Set Speed", for the value of a Move, a Set or a
    /// Steer, `None` for any other place.
    pub owner: Option<String>,
}

impl PlaceRefusal {
    /// Returns the reason in the card's words, where `value` names the value refused, as "Flockmates' centre is a
    /// vector, and Move to needs a cell." or the error's message as a sentence.
    pub fn reason(&self, value: &str) -> String {
        match (self.kinds, self.owner.as_deref()) {
            (Some((expected, found)), Some(owner)) => {
                sentence::kind_needed_by(value, found.sentence_words(), owner, expected.sentence_words())
            }
            (Some((expected, found)), None) => {
                sentence::kind_needed(value, found.sentence_words(), expected.sentence_words())
            }
            (None, _) => sentence::as_sentence(&self.message),
        }
    }
}

/// Returns why the checker refuses `value` put at `target` of line `line_id` of `document`, `None` when it takes the
/// value there.
///
/// The value is tried in a copy of the sheet, and the checker reads the line as it reads it in the sheet. An error
/// the line has already, before the value is put in, is not the value's. Only a slot or an item of a list takes a
/// value. A field returns `None`.
pub fn place_refusal(
    document: &SheetDocument,
    line_id: &str,
    target: &PlaceTarget,
    value: &SheetValue,
) -> Option<PlaceRefusal> {
    let (card, line) = document.line(line_id)?;
    let (path, filled) = match target {
        PlaceTarget::Slot(path) => (path.as_str(), value.clone()),
        PlaceTarget::ListItem { slot, index } => {
            let mut items = match slot_value(line, slot) {
                Some(SheetValue::List(items)) => items.clone(),
                _ => Vec::new(),
            };
            let start = (*index).min(items.len());
            let end = (index + 1).min(items.len());
            items.splice(start..end, [value.clone()]);
            (slot.as_str(), SheetValue::List(items))
        }
        _ => return None,
    };
    let steps = parse_path(path).ok()?;
    let mut trial = document.clone();
    let tried = trial.cards.get_mut(card)?.line_tables.get_mut(line_id)?;
    set_value(tried, &steps, Slot::Filled(filled)).ok()?;
    let before = line_findings(document, card, line_id)?;
    let after = line_findings(&trial, card, line_id)?;
    let message = after
        .errors
        .iter()
        .find(|error| !before.errors.contains(error))?
        .clone();
    let mut known = before.mismatches;
    let kinds = after.mismatches.into_iter().find(|mismatch| {
        let index = known.iter().position(|old| old == mismatch);
        index.map(|index| known.remove(index)).is_none()
    });
    let owner = (path == "value").then(|| owner_words(document, line)).flatten();
    Some(PlaceRefusal { message, kinds, owner })
}

/// Returns the words that name `line` as the owner of its value, as "Move to", "Move by", "Set Speed" or "Steer
/// Velocity", `None` for a line of another kind.
fn owner_words(document: &SheetDocument, line: &SheetLine) -> Option<String> {
    let property = || {
        let id = line.property.as_deref()?;
        Some(
            Thing::all(document)
                .into_iter()
                .flat_map(|thing| thing.properties(document))
                .find(|property| property.id == id)
                .map_or_else(|| id.to_owned(), |property| property.name.clone()),
        )
    };
    match line.kind {
        LineKind::Move => Some(sentence::move_owner(line.mode? == LineMode::To).to_owned()),
        LineKind::Set => Some(sentence::set_owner(&property()?)),
        LineKind::Steer => Some(sentence::steer_owner(&property()?)),
        _ => None,
    }
}
