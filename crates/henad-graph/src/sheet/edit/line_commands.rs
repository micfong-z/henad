//! Edit commands on the lines of a card: new lines, slots and fields, the condition offers, the rate units and the
//! changes between a chance and a rate.

use std::collections::BTreeSet;

use henad_core::metadata::TimeUnit;

use super::lines::{self, PathStep};
use super::references::{Change, Target, change_references};
use super::{Draft, LineField, SheetEditError, clean_name, missing, refuse_taken};
use crate::sheet::document::{
    Around, CardRuns, CompareTest, LineKind, LookSummary, SheetChip, SheetLine, SheetLiteral, SheetTime, SheetValue,
    Slot, SummaryKind,
};
use crate::sheet::limits::MAX_LINES_PER_CARD;
use crate::sheet::rates::{chance_from_rate, rate_from_chance, shortest_f32, tick_in, typed_number};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{
    ANY_STATE_ID, ConditionItem, RATE_IN_BUTTON_CARD, RATE_IN_REPEAT, rate_unit_word, unit_suggestion,
};

/// Returns the first unit of time that `text` names as "per second" to "per year", `None` when it names none.
///
/// Case is ignored, one or more spaces may follow "per", the unit may take a plural "s", and both ends fall on a word
/// boundary. "Per days" names day, and "per daylight" names none.
pub(crate) fn unit_in_text(text: &str) -> Option<TimeUnit> {
    let is_word = |character: char| character.is_ascii_alphanumeric() || character == '_';
    let lower = text.to_ascii_lowercase();
    let boundary_before = |at: usize| !lower[..at].chars().next_back().is_some_and(is_word);
    lower.match_indices("per").find_map(|(at, _)| {
        if !boundary_before(at) {
            return None;
        }
        let after = &lower[at + "per".len()..];
        let rest = after.trim_start_matches(char::is_whitespace);
        if rest.len() == after.len() {
            return None;
        }
        TimeUnit::ALL.into_iter().find(|unit| {
            rest.strip_prefix(rate_unit_word(*unit)).is_some_and(|tail| {
                let tail = tail.strip_prefix('s').unwrap_or(tail);
                !tail.chars().next().is_some_and(is_word)
            })
        })
    })
}

/// Returns the id of the parameter in the `rate` slot of `line`, when `line` is a rate.
fn rate_parameter(line: &SheetLine) -> Option<String> {
    match line.rate.value() {
        Some(SheetValue::Chip(SheetChip::Param(parameter))) if line.kind == LineKind::Rate => Some(parameter.clone()),
        _ => None,
    }
}

/// Gives every rate phrase in `value` with no unit the unit of one tick, marked as taken from the tick.
fn rate_from_tick(value: &mut SheetValue, tick: Option<TimeUnit>) {
    match value {
        SheetValue::Phrase(phrase) => line_rates_from_tick(phrase, tick),
        SheetValue::List(items) => {
            for item in items {
                rate_from_tick(item, tick);
            }
        }
        _ => {}
    }
}

/// Gives `line`, when it is a rate with no unit, and every rate phrase in its slots, the unit of one tick, marked.
fn line_rates_from_tick(line: &mut SheetLine, tick: Option<TimeUnit>) {
    let Some(tick) = tick else {
        return;
    };
    if line.kind == LineKind::Rate && line.rate_unit.is_none() {
        line.rate_unit = Some(tick);
        line.rate_unit_from_tick = true;
    }
    for slot in [
        &mut line.chance,
        &mut line.rate,
        &mut line.left,
        &mut line.right,
        &mut line.value,
        &mut line.condition,
        &mut line.whose,
        &mut line.only,
    ] {
        if let Slot::Filled(value) = slot {
            rate_from_tick(value, Some(tick));
        }
    }
    for item in &mut line.items {
        rate_from_tick(item, Some(tick));
    }
}

impl Draft {
    /// Returns the next line id and moves `next_line` past it.
    fn mint_line_id(&mut self) -> Result<String, SheetEditError> {
        loop {
            let number = self.document.sheet.next_line.max(1);
            let next = number
                .checked_add(1)
                .ok_or_else(|| SheetEditError::Invalid("the sheet has minted every line number".to_owned()))?;
            self.document.sheet.next_line = next;
            let id = format!("l{number}");
            if self.document.line(&id).is_none() {
                return Ok(id);
            }
        }
    }

    /// Returns the state the nearest When around `parent` tests as "my X is S", with its property.
    fn tested_state(&self, card: usize, parent: Option<&str>) -> Option<(String, String)> {
        lines::ancestors(&self.document.cards[card], parent).find_map(|line| {
            if line.kind == LineKind::When
                && let Some(SheetValue::Phrase(test)) = line.condition.value()
                && test.kind == LineKind::Compare
                && test.test == Some(CompareTest::Is)
                && let Some(SheetValue::Chip(SheetChip::My(property))) = test.left.value()
                && let Some(SheetValue::Chip(SheetChip::State(state))) = test.right.value()
            {
                Some((property.clone(), state.clone()))
            } else {
                None
            }
        })
    }

    /// Returns whether a line inside `parent` of card `card` sits inside a When, directly or deeper.
    fn inside_when(&self, card: usize, parent: Option<&str>) -> bool {
        lines::ancestors(&self.document.cards[card], parent).any(|line| line.kind == LineKind::When)
    }

    /// Adds `line` at the end of card `card_id`, or of the block line `inside`.
    pub(super) fn add_line(
        &mut self,
        card_id: &str,
        inside: Option<&str>,
        mut line: SheetLine,
    ) -> Result<Vec<String>, SheetEditError> {
        let card = self
            .document
            .cards
            .iter()
            .position(|card| card.id == card_id)
            .ok_or_else(|| missing("card", card_id))?;
        if let Some(block) = inside {
            let holder = self.document.cards[card]
                .line_tables
                .get(block)
                .ok_or_else(|| SheetEditError::Missing(format!("card '{card_id}' has no line '{block}'")))?;
            if !holder.kind.is_block() {
                return Err(SheetEditError::Invalid(format!(
                    "line '{block}' holds no lines: it is no When or Repeat"
                )));
            }
        }
        if self.document.cards[card].line_tables.len() >= MAX_LINES_PER_CARD {
            return Err(SheetEditError::Invalid(format!(
                "card '{card_id}' holds {MAX_LINES_PER_CARD} lines, the most a card holds"
            )));
        }
        line.lines.clear();
        if self.inside_when(card, inside) {
            if matches!(line.kind, LineKind::Become | LineKind::Set) && line.condition.is_blank() {
                line.condition = Slot::Filled(SheetValue::Phrase(Box::new(SheetLine::new(LineKind::Always))));
            }
            if line.kind == LineKind::Become
                && line.start_state.is_none()
                && let Some((property, state)) = self.tested_state(card, inside)
                && line.property.as_ref().is_none_or(|own| *own == property)
            {
                line.property = Some(property);
                line.start_state = Some(state);
            }
        }
        line_rates_from_tick(&mut line, self.document.time.map(|time| time.tick_unit));
        let id = self.mint_line_id()?;
        let target = &mut self.document.cards[card];
        target.line_tables.insert(id.clone(), line);
        match inside {
            Some(block) => {
                if let Some(holder) = target.line_tables.get_mut(block) {
                    holder.lines.push(id.clone());
                }
            }
            None => target.lines.push(id.clone()),
        }
        self.touched.push(id.clone());
        self.refuse_deep(&id)?;
        Ok(vec![id])
    }

    /// Puts `value` in the slot at `path` of line `line_id`.
    pub(super) fn set_slot(&mut self, line_id: &str, path: &str, mut value: Slot) -> Result<(), SheetEditError> {
        let steps = lines::parse_path(path)?;
        let tick = self.document.time.map(|time| time.tick_unit);
        if let Slot::Filled(inner) = &mut value {
            rate_from_tick(inner, tick);
        }
        // A parameter put into a rate gives the rate its unit.
        let parameter = match (&value, steps.last()) {
            (Slot::Filled(SheetValue::Chip(SheetChip::Param(id))), Some(PathStep::Slot(slot))) if slot == "rate" => {
                Some(id.clone())
            }
            _ => None,
        };
        let line = self.line_mut(line_id)?;
        lines::set_value(line, &steps, value)?;
        if let Some(parameter) = parameter {
            self.rate_takes_parameter(line_id, &steps, &parameter)?;
        }
        self.refuse_deep(line_id)
    }

    /// Gives the rate at `steps` of line `line_id` the unit of `parameter`, when the rate's unit is empty or marked, and
    /// gives `parameter` the rate's unit when the parameter has none.
    fn rate_takes_parameter(
        &mut self,
        line_id: &str,
        steps: &[PathStep],
        parameter: &str,
    ) -> Result<(), SheetEditError> {
        let parameter_unit = self
            .document
            .parameters
            .iter()
            .find(|known| known.id == parameter)
            .and_then(|known| known.unit.as_deref())
            .and_then(unit_in_text);
        let inner = steps.split_last().map_or(&[][..], |(_, inner)| inner);
        let line = self.line_mut(line_id)?;
        let rate = lines::phrase_mut(line, inner)?;
        if rate.kind != LineKind::Rate {
            return Ok(());
        }
        let chosen = parameter_unit.filter(|_| rate.rate_unit.is_none() || rate.rate_unit_from_tick);
        if let Some(unit) = chosen {
            rate.rate_unit = Some(unit);
            rate.rate_unit_from_tick = false;
        }
        let unit = rate.rate_unit;
        if let Some(unit) = chosen
            && self.document.time.is_none()
        {
            self.document.time = Some(SheetTime::new(1.0, unit));
        }
        if let Some(unit) = unit {
            self.parameter_takes_unit(parameter, unit);
        }
        Ok(())
    }

    /// Gives `parameter`, a parameter a rate in `unit` reads, the words of [`unit_suggestion`] for the unit when it has
    /// no unit.
    fn parameter_takes_unit(&mut self, parameter: &str, unit: TimeUnit) {
        if let Some(known) = self.document.parameters.iter_mut().find(|known| known.id == parameter)
            && known.unit.as_deref().is_none_or(|text| text.trim().is_empty())
        {
            known.unit = Some(unit_suggestion(unit));
        }
    }

    /// Sets `field` of the phrase at `phrase` of line `line_id`.
    pub(super) fn set_field(&mut self, line_id: &str, phrase: &str, field: LineField) -> Result<(), SheetEditError> {
        let steps = if phrase.is_empty() {
            Vec::new()
        } else {
            lines::parse_path(phrase)?
        };
        let state_owner = match &field {
            LineField::NewState(Some((property, state))) => {
                let holder = self.property(property).ok_or_else(|| missing("property", property))?;
                if !holder.states.iter().any(|known| known.id == *state) {
                    return Err(missing("state", state));
                }
                Some(holder.states.iter().map(|known| known.id.clone()).collect::<Vec<_>>())
            }
            LineField::Property(Some(property)) => Some(
                self.property(property)
                    .map(|holder| holder.states.iter().map(|known| known.id.clone()).collect())
                    .unwrap_or_default(),
            ),
            _ => None,
        };
        let mut chosen_unit = None;
        let line = self.line_mut(line_id)?;
        let target = lines::phrase_mut(line, &steps)?;
        match field {
            LineField::Property(property) => {
                let states = state_owner.unwrap_or_default();
                let keep = |state: &Option<String>| {
                    state
                        .as_deref()
                        .is_some_and(|state| state == ANY_STATE_ID || states.iter().any(|known| known == state))
                };
                if target.property != property {
                    if !keep(&target.new_state) {
                        target.new_state = None;
                    }
                    if !keep(&target.start_state) {
                        target.start_state = None;
                    }
                }
                target.property = property;
            }
            LineField::NewState(new_state) => match new_state {
                Some((property, state)) => {
                    let states = state_owner.unwrap_or_default();
                    let old_fits = target
                        .start_state
                        .as_deref()
                        .is_some_and(|old| old == ANY_STATE_ID || states.iter().any(|known| known == old));
                    if !old_fits {
                        target.start_state = None;
                    }
                    target.property = Some(property);
                    target.new_state = Some(state);
                }
                None => target.new_state = None,
            },
            LineField::StartState(state) => target.start_state = state,
            LineField::Test(test) => {
                let list = |test: Option<CompareTest>| matches!(test, Some(CompareTest::OneOf | CompareTest::NotOneOf));
                match (list(target.test), list(test)) {
                    (false, true) => {
                        if let Slot::Filled(value) = &target.right
                            && !matches!(value, SheetValue::List(_))
                        {
                            target.right = Slot::Filled(SheetValue::List(vec![value.clone()]));
                        }
                    }
                    (true, false) => {
                        if let Slot::Filled(SheetValue::List(items)) = &target.right {
                            target.right = items.first().cloned().map_or(Slot::Blank, Slot::Filled);
                        }
                    }
                    _ => {}
                }
                target.test = test;
            }
            LineField::Around(around) => target.around = around,
            LineField::RateUnit(unit) => {
                target.rate_unit = unit;
                target.rate_unit_from_tick = false;
                chosen_unit = unit.map(|unit| (unit, rate_parameter(target)));
            }
            LineField::Formula(formula) => target.formula = formula.filter(|formula| !formula.trim().is_empty()),
            LineField::Mode(mode) => target.mode = mode,
            LineField::Pick(pick) => target.pick = pick,
            LineField::RankedBy(property) => target.ranked_by = property,
            LineField::Which(which) => target.which = which,
            LineField::GroundProperty(property) => target.ground_property = property,
            LineField::Event(event) => target.event = event,
            LineField::AgentKind(kind) => target.agent_kind = kind,
            LineField::IncludeMe(include) => target.include_me = include,
            LineField::OpenOnly(open) => target.open_only = open,
            LineField::Summaries(summaries) => target.summaries = summaries,
        }
        if let Some((unit, parameter)) = chosen_unit {
            self.rate_unit_chosen(unit, parameter.as_deref());
        }
        self.refuse_deep(line_id)
    }

    /// Sets one tick to 1 `unit` when the sheet has no length of one tick, and gives `parameter`, the parameter a rate
    /// reads, the unit when it has none.
    fn rate_unit_chosen(&mut self, unit: TimeUnit, parameter: Option<&str>) {
        if self.document.time.is_none() {
            self.document.time = Some(SheetTime::new(1.0, unit));
        }
        if let Some(parameter) = parameter {
            self.parameter_takes_unit(parameter, unit);
        }
    }

    /// Chooses the form `item` for the condition of line `line_id`.
    pub(super) fn choose_condition(&mut self, line_id: &str, item: ConditionItem) -> Result<(), SheetEditError> {
        let tick = self.document.time.map(|time| time.tick_unit);
        let line = self.line_mut(line_id)?;
        if item == ConditionItem::Always && line.kind == LineKind::When {
            return Err(SheetEditError::Invalid("a When cannot be always".to_owned()));
        }
        let phrase = condition_phrase(&line.condition, item, tick);
        line.condition = Slot::Filled(SheetValue::Phrase(Box::new(phrase)));
        Ok(())
    }

    /// Chooses the form `item` for the condition of row `row` of the Choose line `line_id`.
    pub(super) fn choose_row_condition(
        &mut self,
        line_id: &str,
        row: usize,
        item: ConditionItem,
    ) -> Result<(), SheetEditError> {
        let tick = self.document.time.map(|time| time.tick_unit);
        let line = self.line_mut(line_id)?;
        if line.kind != LineKind::Choose {
            return Err(SheetEditError::Invalid(format!("line '{line_id}' is no Choose line")));
        }
        let Some(target) = line.rows.get_mut(row) else {
            return Err(SheetEditError::Missing(format!(
                "line '{line_id}' has no row {}",
                row + 1
            )));
        };
        let phrase = condition_phrase(&target.condition, item, tick);
        target.condition = Slot::Filled(SheetValue::Phrase(Box::new(phrase)));
        Ok(())
    }

    /// Replaces the one-sided outside condition of line `line_id` with the plain comparison it means.
    pub(super) fn use_one_side(&mut self, line_id: &str) -> Result<(), SheetEditError> {
        let line = self.line_mut(line_id)?;
        let not_one_sided =
            || SheetEditError::Invalid(format!("line '{line_id}' has no condition with one side filled"));
        let Some(SheetValue::Phrase(condition)) = line.condition.value() else {
            return Err(not_one_sided());
        };
        if condition.kind != LineKind::Compare || condition.test != Some(CompareTest::Outside) {
            return Err(not_one_sided());
        }
        let (test, right) = match (&condition.low, &condition.high) {
            (Slot::Filled(low), Slot::Blank) => (CompareTest::LessThan, low.clone()),
            (Slot::Blank, Slot::Filled(high)) => (CompareTest::MoreThan, high.clone()),
            _ => return Err(not_one_sided()),
        };
        let mut compare = SheetLine::new(LineKind::Compare);
        compare.left = condition.left.clone();
        compare.test = Some(test);
        compare.right = Slot::Filled(right);
        line.condition = Slot::Filled(SheetValue::Phrase(Box::new(compare)));
        Ok(())
    }

    /// Moves the count that the chance or rate of line `line_id` holds as its value into "for each".
    pub(super) fn use_count_for_each(&mut self, line_id: &str) -> Result<(), SheetEditError> {
        let counts: BTreeSet<String> = self
            .document
            .cards
            .iter()
            .flat_map(|card| &card.line_tables)
            .filter(|(_, line)| line.kind == LineKind::Count)
            .map(|(id, _)| id.clone())
            .collect();
        let line = self.line_mut(line_id)?;
        let holder = draw_phrase_mut(line, |kind| matches!(kind, LineKind::Chance | LineKind::Rate))
            .ok_or_else(|| SheetEditError::Invalid(format!("line '{line_id}' holds no chance or rate")))?;
        let slot = if holder.kind == LineKind::Rate {
            &mut holder.rate
        } else {
            &mut holder.chance
        };
        let count = match slot.value() {
            Some(
                chip @ SheetValue::Chip(SheetChip::Result {
                    line: count_line,
                    part: None,
                    property: None,
                }),
            ) if counts.contains(count_line) => chip.clone(),
            _ => {
                return Err(SheetEditError::Invalid(format!(
                    "the chance or rate of line '{line_id}' is no count"
                )));
            }
        };
        *slot = Slot::Blank;
        holder.tries = Slot::Filled(count);
        Ok(())
    }

    /// Turns the chance of line `line_id` into a rate for `into` [`LineKind::Rate`], or its rate into a chance for
    /// `into` [`LineKind::Chance`], keeping its chance per tick.
    ///
    /// A chance on a card that runs once is refused in the words of [`RATE_IN_BUTTON_CARD`]. The card has no duration.
    /// A chance on a line inside a Repeat is refused in the words of [`RATE_IN_REPEAT`].
    pub(super) fn change_draw(&mut self, line_id: &str, into: LineKind) -> Result<(), SheetEditError> {
        let time = self
            .document
            .time
            .ok_or_else(|| SheetEditError::Invalid("the sheet sets no length of one tick".to_owned()))?;
        let from = if into == LineKind::Rate {
            LineKind::Chance
        } else {
            LineKind::Rate
        };
        let card = &self.document.cards[self.card_of_line(line_id)?];
        let runs_once = card.runs == CardRuns::Once;
        let in_repeat = CardRelations::new(&self.document, card).repeat_of(line_id).is_some();
        let line = self.line_mut(line_id)?;
        let holder = draw_phrase_mut(line, |kind| kind == from)
            .ok_or_else(|| SheetEditError::Invalid(format!("line '{line_id}' holds no {}", from.as_str())))?;
        let slot = if from == LineKind::Rate {
            &holder.rate
        } else {
            &holder.chance
        };
        let typed = typed_number(slot).ok_or_else(|| {
            SheetEditError::Invalid(format!("the {} of line '{line_id}' is no typed number", from.as_str()))
        })?;
        let converted = if into == LineKind::Rate {
            if !(0.0..1.0).contains(&typed) {
                return Err(SheetEditError::Invalid(format!(
                    "a chance of {typed} gives no rate, which needs a chance from 0 to below 1"
                )));
            }
            if runs_once {
                return Err(SheetEditError::Invalid(RATE_IN_BUTTON_CARD.to_owned()));
            }
            if in_repeat {
                return Err(SheetEditError::Invalid(RATE_IN_REPEAT.to_owned()));
            }
            rate_from_chance(typed, time.tick_length)
        } else {
            let unit = holder
                .rate_unit
                .ok_or_else(|| SheetEditError::Invalid(format!("the rate of line '{line_id}' has no unit")))?;
            chance_from_rate(typed, tick_in(time, unit))
        };
        let converted = Slot::Filled(SheetValue::Literal(SheetLiteral::Number(shortest_f32(converted))));
        holder.kind = into;
        if into == LineKind::Rate {
            holder.rate = converted;
            holder.chance = Slot::Blank;
            holder.rate_unit = Some(time.tick_unit);
            holder.rate_unit_from_tick = true;
        } else {
            holder.chance = converted;
            holder.rate = Slot::Blank;
            holder.rate_unit = None;
            holder.rate_unit_from_tick = false;
        }
        Ok(())
    }

    /// Renames the result of line `line_id` to `name`.
    pub(super) fn rename_result(&mut self, line_id: &str, name: &str) -> Result<(), SheetEditError> {
        let name = clean_name(name, "result")?;
        let others = self
            .document
            .cards
            .iter()
            .flat_map(|card| &card.line_tables)
            .filter(|(id, other)| id.as_str() != line_id && other.kind.works_out_result())
            .filter_map(|(_, other)| other.name.as_deref());
        refuse_taken(&name, others, "result")?;
        let target = self.line_mut(line_id)?;
        if !target.kind.works_out_result() {
            return Err(SheetEditError::Invalid(format!("line '{line_id}' works out no result")));
        }
        target.name = Some(name);
        Ok(())
    }

    /// Adds a summary of the kind `summary`, with its value empty, at the end of the summaries of line `line_id`.
    pub(super) fn add_summary(&mut self, line_id: &str, summary: SummaryKind) -> Result<(), SheetEditError> {
        let target = self.line_mut(line_id)?;
        if target.around != Some(Around::AgentsWithin) {
            return Err(SheetEditError::Invalid(format!("line '{line_id}' looks at no agents")));
        }
        let mut added = LookSummary::new("", "");
        added.summary = Some(summary);
        target.summaries.push(added);
        self.touched.push(line_id.to_owned());
        Ok(())
    }

    /// Removes the summary at `index` of line `line_id`, and clears every reference to it.
    pub(super) fn delete_summary(&mut self, line_id: &str, index: usize) -> Result<(), SheetEditError> {
        let target = self.line_mut(line_id)?;
        if index >= target.summaries.len() {
            return Err(SheetEditError::Missing(format!(
                "line '{line_id}' has no summary {}",
                index + 1
            )));
        }
        if target.summaries.len() == 1 {
            return Err(SheetEditError::Invalid(format!(
                "line '{line_id}' keeps one summary at least"
            )));
        }
        let removed = target.summaries.remove(index);
        change_references(
            &mut self.document,
            &Target::Summary {
                line: line_id.to_owned(),
                summary: removed.id,
            },
            Change::Clear,
        );
        Ok(())
    }

    /// Copies the condition of the When `when_id` into the one change inside it and removes the When.
    pub(super) fn move_condition_into(&mut self, when_id: &str) -> Result<(), SheetEditError> {
        let card = self.card_of_line(when_id)?;
        let when = self.line(when_id)?;
        let refused = || SheetEditError::Invalid(format!("line '{when_id}' is no When around exactly one change"));
        if when.kind != LineKind::When {
            return Err(refused());
        }
        let [inner] = when.lines.as_slice() else {
            return Err(refused());
        };
        let inner = inner.clone();
        let condition = when.condition.clone();
        let change = self.line(&inner)?;
        if !matches!(change.kind, LineKind::Become | LineKind::Set) {
            return Err(refused());
        }
        let always =
            matches!(change.condition.value(), Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::Always);
        if !(change.condition.is_blank() || always) {
            return Err(SheetEditError::Invalid(format!(
                "line '{inner}' has a condition of its own, and the When's condition would replace it"
            )));
        }
        self.line_mut(&inner)?.condition = condition;
        lines::remove_line(&mut self.document.cards[card], when_id)?;
        Ok(())
    }

    /// Deletes line `line_id`, keeps its lines one level up, and clears every reference to its result.
    pub(super) fn delete_line(&mut self, line_id: &str) -> Result<(), SheetEditError> {
        let card = self.card_of_line(line_id)?;
        lines::remove_line(&mut self.document.cards[card], line_id)?;
        change_references(&mut self.document, &Target::Line(line_id.to_owned()), Change::Clear);
        Ok(())
    }
}

/// Returns the draw of `line`: the line itself when `is_draw` holds for its kind, or else the phrase in its condition
/// when `is_draw` holds for the phrase's kind.
fn draw_phrase_mut(line: &mut SheetLine, is_draw: impl Fn(LineKind) -> bool) -> Option<&mut SheetLine> {
    if is_draw(line.kind) {
        return Some(line);
    }
    match &mut line.condition {
        Slot::Filled(SheetValue::Phrase(phrase)) if is_draw(phrase.kind) => Some(phrase.as_mut()),
        _ => None,
    }
}

/// Returns the condition of the form `item` that replaces the condition in `slot`, keeping its values when the form
/// stays, where `tick` is the unit of one tick that a new rate takes.
fn condition_phrase(slot: &Slot, item: ConditionItem, tick: Option<TimeUnit>) -> SheetLine {
    let current = match slot.value() {
        Some(SheetValue::Phrase(phrase)) => Some(phrase.as_ref().clone()),
        _ => None,
    };
    match item {
        ConditionItem::Always => SheetLine::new(LineKind::Always),
        ConditionItem::Is | ConditionItem::AtLeast | ConditionItem::AtMost => {
            let test = match item {
                ConditionItem::Is => CompareTest::Is,
                ConditionItem::AtLeast => CompareTest::AtLeast,
                _ => CompareTest::AtMost,
            };
            let mut compare = match current {
                Some(compare) if compare.kind == LineKind::Compare && compare.test != Some(CompareTest::Outside) => {
                    compare
                }
                _ => SheetLine::new(LineKind::Compare),
            };
            if let Slot::Filled(SheetValue::List(items)) = &compare.right {
                compare.right = items.first().cloned().map_or(Slot::Blank, Slot::Filled);
            }
            compare.test = Some(test);
            compare
        }
        ConditionItem::Outside => match current {
            Some(compare) if compare.kind == LineKind::Compare && compare.test == Some(CompareTest::Outside) => compare,
            _ => {
                let mut compare = SheetLine::new(LineKind::Compare);
                compare.test = Some(CompareTest::Outside);
                compare
            }
        },
        ConditionItem::WithChance => match current {
            Some(chance) if chance.kind == LineKind::Chance => chance,
            _ => SheetLine::new(LineKind::Chance),
        },
        ConditionItem::AtRate => match current {
            Some(rate) if rate.kind == LineKind::Rate => rate,
            _ => {
                let mut rate = SheetLine::new(LineKind::Rate);
                line_rates_from_tick(&mut rate, tick);
                rate
            }
        },
    }
}
