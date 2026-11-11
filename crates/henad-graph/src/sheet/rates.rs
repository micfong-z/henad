//! Rates and chances per tick: the conversions between a rate and a chance, and the note that states the chance a
//! rate line gives in one tick.
//!
//! A *rate* `r` per unit of time holds within one tick of length `dt`, counted in the rate's unit, with the chance
//! `1 - e^(-r dt)`. A rate for each of a count `n` holds with `1 - e^(-r dt n)`, and a *competing group* of rates out
//! of one state leaves it with the chance of the sum of their rates, each line taking the share its rate gives it.
//! These are the chances the lowering draws against.
//!
//! A note prints a chance by the sheet's number rule: three significant figures, and more until the chance prints
//! differently from `r dt n` when the two differ. A note then never shows a chance equal to its rate. The numbers come
//! from the parameter values passed in, the defaults while no run is loaded.
//!
//! ```
//! use henad_graph::sheet::rates::{chance_from_rate, rate_from_chance};
//!
//! let chance = chance_from_rate(0.1, 1.0);
//! assert!((chance - 0.095_162_58).abs() < 1e-8);
//! assert!((rate_from_chance(chance, 1.0) - 0.1).abs() < 1e-12);
//! ```

use std::collections::BTreeMap;

use henad_core::metadata::TimeUnit;

use super::check::formula::{self, FormulaTree};
use super::check::formula_chip;
use super::document::{
    CardRuns, CompareTest, LineKind, SheetChip, SheetDocument, SheetLine, SheetParameter, SheetTime, SheetValue, Slot,
};
use super::relations::{CardRelations, most_counted, note_name};
use super::sentence::{self, ANY_STATE_ID, Sentences};
use super::writer::format_number;

/// Chance per tick at the largest count above which a tick is long for its rate.
pub const LONG_TICK_CHANCE: f64 = 0.5;

/// Returns the chance that a rate `rate` holds within a tick `tick` long, both in one unit of time:
/// `1 - e^(-rate tick)`.
pub fn chance_from_rate(rate: f64, tick: f64) -> f64 {
    -(-rate * tick).exp_m1()
}

/// Returns the rate whose chance within a tick `tick` long is `chance`: `-ln(1 - chance) / tick`.
///
/// A chance of 1 gives an infinite rate, and a chance above 1 or a tick of 0 gives a number that is not finite.
pub fn rate_from_chance(chance: f64, tick: f64) -> f64 {
    -(-chance).ln_1p() / tick
}

/// Returns the length of one tick of `time` counted in `unit`, such as 0.25 for a tick of 6 hours in days.
pub fn tick_in(time: SheetTime, unit: TimeUnit) -> f64 {
    time.tick_length * time.tick_unit.days() / unit.days()
}

/// Returns the chance per tick of `time` that a rate `rate` per `unit` holds for each of `count`:
/// `1 - e^(-rate dt count)`.
pub fn chance_per_tick(rate: f64, unit: TimeUnit, time: SheetTime, count: f64) -> f64 {
    chance_from_rate(rate * count, tick_in(time, unit))
}

/// Returns how long, on average, a rate `rate` takes to hold when it is drawn once in each tick `tick` long.
///
/// Both are in one unit of time, and so is the result, `tick / (1 - e^(-rate tick))`. It approaches `1 / rate` as the
/// tick shrinks.
pub fn mean_stay(rate: f64, tick: f64) -> f64 {
    tick / chance_from_rate(rate, tick)
}

/// Returns the default of every parameter of `document`, by id, the values a note reads while no run is loaded.
pub fn parameter_defaults(document: &SheetDocument) -> BTreeMap<String, f64> {
    document
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.default))
        .collect()
}

/// Returns `value` rounded to `digits` significant figures, written by the sheet's number rule.
fn significant(value: f64, digits: usize) -> String {
    let rounded: f64 = format!("{value:.*e}", digits.saturating_sub(1))
        .parse()
        .unwrap_or(value);
    format_number(rounded).unwrap_or_default()
}

/// Returns `chance` as a note prints it: three significant figures, and more until it prints differently from
/// `linear`, the rate times the tick times the count, when the two differ.
pub fn chance_words(chance: f64, linear: f64) -> String {
    for digits in 3..=17 {
        let words = significant(chance, digits);
        if chance == linear || words != significant(linear, digits) {
            return words;
        }
    }
    significant(chance, 17)
}

/// Returns a length of time `value` long in `unit`, with three significant figures: "1 day", "0.25 days".
fn time_words(value: f64, unit: TimeUnit) -> String {
    let number = significant(value, 3);
    let unit = if number == "1" {
        sentence::rate_unit_word(unit)
    } else {
        unit.plural()
    };
    format!("{number} {unit}")
}

/// Chance per tick of a rate line at the largest count its "for each" can reach.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct LargestCount {
    /// Chance per tick at the largest count.
    pub chance: f64,
    /// Largest count: 8, 4 or 9 cells around.
    pub count: u32,
}

impl LargestCount {
    /// Returns whether the tick is long for the rate: the chance is above [`LONG_TICK_CHANCE`].
    pub fn is_long(&self) -> bool {
        self.chance > LONG_TICK_CHANCE
    }

    /// Returns the words of the chance, such as "up to 0.909 with 8", or "almost 1 with 8" for a chance below 1 that
    /// rounds to 1 at three significant figures.
    pub fn words(&self) -> String {
        let chance = significant(self.chance, 3);
        if chance == "1" && self.chance < 1.0 {
            sentence::long_tick_almost_one(self.count)
        } else {
            sentence::long_tick_chance(&chance, self.count)
        }
    }
}

/// The note under a rate line, or under the highest line of a competing group.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RateNote {
    /// Text of the note, such as "Each tick (1 day), an Infected cell becomes Recovered with chance 0.0952. A rate is
    /// not a chance. Henad turns it into one as 1 − e^(−0.1 × 1 day)."
    pub text: String,
    /// Mean time a cell stays in the state it leaves, the note's description, for a line that leaves a named state at a
    /// fixed rate with no "for each", the rate its whole condition and no block around it.
    pub mean_stay: Option<String>,
    /// Chance at the largest count, such as "up to 0.909 with 8", on a tick long for the rate. The editor shows it
    /// with [`sentence::LONG_TICK`].
    pub long_tick: Option<String>,
}

/// Returns the rate phrase of `line`: the line itself, its condition, or an item of an all of in its condition.
fn rate_phrase(line: &SheetLine) -> Option<&SheetLine> {
    if line.kind == LineKind::Rate {
        return Some(line);
    }
    joined_rate(line.condition.value()?)
}

/// Returns the rate phrase that `value` is, or that an item of it holds when it is an all of.
fn joined_rate(value: &SheetValue) -> Option<&SheetLine> {
    match value {
        SheetValue::Phrase(phrase) if phrase.kind == LineKind::Rate => Some(phrase),
        SheetValue::Phrase(phrase) if phrase.kind == LineKind::AllOf => phrase.items.iter().find_map(joined_rate),
        _ => None,
    }
}

/// Returns whether the rate of line `line_id` is the whole of what the line needs: its condition is the rate phrase
/// alone, and no block holds the line.
///
/// A mean stay holds only for such a line. Otherwise the cells for which the rest of the condition fails stay longer.
fn rate_alone(relations: &CardRelations<'_>, line_id: &str, line: &SheetLine) -> bool {
    relations.parent(line_id).is_none()
        && matches!(line.condition.value(), Some(SheetValue::Phrase(phrase)) if phrase.kind == LineKind::Rate)
}

/// Returns the number a value stands for when it is a typed number or a parameter, read from `values`.
fn fixed_number(value: Option<&SheetValue>, values: &BTreeMap<String, f64>) -> Option<f64> {
    match value? {
        SheetValue::Literal(literal) => literal.number(),
        SheetValue::Chip(SheetChip::Param(id)) => values.get(id).copied(),
        _ => None,
    }
}

/// Returns the Calculate line or phrase that a rate's value names, `None` for any other value.
fn calculation<'d>(document: &'d SheetDocument, value: Option<&'d SheetValue>) -> Option<&'d SheetLine> {
    match value? {
        SheetValue::Phrase(phrase) => Some(phrase.as_ref()).filter(|line| line.kind == LineKind::Calculate),
        SheetValue::Chip(chip) => whole_result(document, chip, LineKind::Calculate),
        _ => None,
    }
}

/// Returns the line of `kind` whose whole result `chip` names, `None` for a part, a property, another kind or any
/// other chip.
fn whole_result<'d>(document: &'d SheetDocument, chip: &SheetChip, kind: LineKind) -> Option<&'d SheetLine> {
    match chip {
        SheetChip::Result {
            line,
            part: None,
            property: None,
        } => document
            .line(line)
            .map(|(_, line)| line)
            .filter(|line| line.kind == kind),
        _ => None,
    }
}

/// A Calculate of a fixed number times a count, in either order.
struct Product<'d> {
    /// The fixed number, a typed number or a parameter's value.
    fixed: f64,
    /// The Count line it multiplies.
    count: &'d SheetLine,
}

/// Returns `calculation` as a fixed number times a count above it, `None` for any other formula.
fn product<'d>(
    document: &'d SheetDocument,
    calculation: &SheetLine,
    values: &BTreeMap<String, f64>,
) -> Option<Product<'d>> {
    let tree = formula::parse(calculation.formula.as_deref()?).ok()?;
    let FormulaTree::Binary('*', left, right) = tree else {
        return None;
    };
    let fixed = |side: &FormulaTree<'_>| match side {
        FormulaTree::Number(text) => text.parse::<f64>().ok(),
        FormulaTree::Reference(reference) => match formula_chip(reference, document) {
            SheetChip::Param(id) => values.get(&id).copied(),
            _ => None,
        },
        _ => None,
    };
    let count = |side: &FormulaTree<'_>| match side {
        FormulaTree::Reference(reference) => {
            whole_result(document, &formula_chip(reference, document), LineKind::Count)
        }
        _ => None,
    };
    let (fixed, count) = match (fixed(&left), count(&right)) {
        (Some(fixed), Some(count)) => (fixed, count),
        _ => (fixed(&right)?, count(&left)?),
    };
    Some(Product { fixed, count })
}

/// Returns the Count line that the "for each" of the rate phrase `phrase` names, `None` for any other count.
fn tries_count<'d>(document: &'d SheetDocument, phrase: &SheetLine) -> Option<&'d SheetLine> {
    match phrase.tries.value()? {
        SheetValue::Chip(chip) => whole_result(document, chip, LineKind::Count),
        _ => None,
    }
}

/// Returns the name of the state `state` of the property `property`, the id when the sheet holds none.
fn state_name(document: &SheetDocument, property: Option<&str>, state: &str) -> String {
    document
        .cells
        .iter()
        .flatten()
        .chain(document.agents.iter().flat_map(|kind| &kind.properties))
        .filter(|found| Some(found.id.as_str()) == property)
        .flat_map(|found| &found.states)
        .find(|found| found.id == state)
        .map_or_else(|| state.to_owned(), |found| found.name.clone())
}

/// Returns the state line `line` changes from, by name, `None` for a line from any state or with none chosen.
fn old_state(document: &SheetDocument, line: &SheetLine) -> Option<String> {
    line.start_state
        .as_deref()
        .filter(|state| *state != ANY_STATE_ID)
        .map(|state| state_name(document, line.property.as_deref(), state))
}

/// Returns the subject of the note on line `line_id`: what happens when its rate holds.
fn subject(document: &SheetDocument, relations: &CardRelations<'_>, line_id: &str, line: &SheetLine) -> String {
    let thing = sentence::thing_word(document, &relations.card().every);
    match line.kind {
        LineKind::Become => {
            let new = line
                .new_state
                .as_deref()
                .map(|state| state_name(document, line.property.as_deref(), state));
            sentence::become_subject(&thing, old_state(document, line).as_deref(), new.as_deref())
        }
        LineKind::Set => sentence::set_subject(&thing),
        LineKind::When => sentence::when_subject(relations.number(line_id).unwrap_or(0), &thing),
        _ => {
            let name = note_name(document, line).unwrap_or_default();
            sentence::result_subject(&name, &thing)
        }
    }
}

/// Returns the chance per tick at the largest count of the rate line `line_id`, read with the parameter values
/// `values`, `None` for a line with no rate or no count of the cells around.
///
/// A count reaches its largest value through the line's "for each" with a fixed rate, or through a Calculate of a
/// fixed number times the count with no "for each".
pub fn largest_count_chance(
    document: &SheetDocument,
    line_id: &str,
    values: &BTreeMap<String, f64>,
) -> Option<LargestCount> {
    let (_, line) = document.line(line_id)?;
    let phrase = rate_phrase(line)?;
    let dt = tick_in(document.time?, phrase.rate_unit?);
    let (rate, count) = match fixed_number(phrase.rate.value(), values) {
        Some(rate) => (rate, tries_count(document, phrase)?),
        None if phrase.tries.is_blank() => {
            let product = product(document, calculation(document, phrase.rate.value())?, values)?;
            (product.fixed, product.count)
        }
        None => return None,
    };
    let count = most_counted(count)?;
    Some(LargestCount {
        chance: chance_from_rate(rate * f64::from(count), dt),
        count,
    })
}

/// Returns the note under the rate line `line_id`, read with the parameter values `values`, or `None` for a line with
/// no note.
///
/// A line with no rate, an empty rate, a rate with no unit, and a sheet whose tick has no length have no note.
/// The highest line of a competing group takes the group's note and the group's other lines none. A group has a note
/// only when each of its lines holds a fixed rate in one unit with no "for each" as its whole condition, outside any
/// block.
pub fn rate_note(document: &SheetDocument, line_id: &str, values: &BTreeMap<String, f64>) -> Option<RateNote> {
    let (card_index, line) = document.line(line_id)?;
    let time = document.time?;
    let relations = CardRelations::new(document, &document.cards[card_index]);
    if let Some(group) = relations
        .competing_groups()
        .into_iter()
        .find(|group| group.contains(&line_id))
    {
        return if group.first() == Some(&line_id) {
            group_note(document, &relations, time, &group, values)
        } else {
            None
        };
    }
    let phrase = rate_phrase(line)?;
    if phrase.rate.is_blank() {
        return None;
    }
    let unit = phrase.rate_unit?;
    let dt = tick_in(time, unit);
    let tick = time_words(dt, unit);
    let subject = subject(document, &relations, line_id, line);
    let sentences = Sentences::new(document);
    let count = (!phrase.tries.is_blank()).then(|| sentences.slot(&phrase.tries, None));
    let at = |rate: f64, n: f64| chance_words(chance_from_rate(rate * n, dt), rate * n * dt);
    let long_tick = largest_count_chance(document, line_id, values)
        .filter(LargestCount::is_long)
        .map(|largest| largest.words());
    if let Some(rate) = fixed_number(phrase.rate.value(), values) {
        let rate_words = format_number(rate)?;
        let (text, mean_stay) = match &count {
            None => {
                let text = sentence::rate_note(&tick, &subject, &at(rate, 1.0), &rate_words);
                let stay = old_state(document, line)
                    .filter(|_| line.kind == LineKind::Become && rate > 0.0 && rate_alone(&relations, line_id, line));
                let stay = stay.map(|state| {
                    sentence::mean_stay_note(
                        &state,
                        &time_words(mean_stay(rate, dt), unit),
                        &rate_words,
                        &time_words(1.0 / rate, unit),
                        &sentence::thing_word(document, &relations.card().every),
                    )
                });
                (text, stay)
            }
            Some(count) => (
                sentence::rate_for_each_note(&tick, &subject, count, &at(rate, 1.0), &at(rate, 2.0), &rate_words),
                None,
            ),
        };
        return Some(RateNote {
            text,
            mean_stay,
            long_tick,
        });
    }
    let calculated = calculation(document, phrase.rate.value());
    let rate_words = calculated.map_or_else(
        || sentences.slot(&phrase.rate, None),
        |calculated| sentences.result_words(calculated),
    );
    let product = calculated
        .filter(|_| count.is_none())
        .and_then(|calculated| product(document, calculated, values));
    let text = match product {
        Some(product) => {
            let count_name = note_name(document, product.count).unwrap_or_default();
            sentence::worked_out_rate_note(
                &tick,
                &subject,
                &count_name,
                &at(product.fixed, 1.0),
                &at(product.fixed, 2.0),
                &rate_words,
            )
        }
        None => sentence::growing_rate_note(&tick, &subject, &rate_words, count.as_deref()),
    };
    Some(RateNote {
        text,
        mean_stay: None,
        long_tick,
    })
}

/// Returns the chance phrase of `line`: the line itself or its condition.
fn chance_phrase(line: &SheetLine) -> Option<&SheetLine> {
    if line.kind == LineKind::Chance {
        return Some(line);
    }
    match line.condition.value()? {
        SheetValue::Phrase(phrase) if phrase.kind == LineKind::Chance => Some(phrase),
        _ => None,
    }
}

/// Returns the note under the chance line `line_id` whose chance holds "for each", read with the parameter values
/// `values`, or `None` for a line with no such note.
///
/// A fixed chance `p` gives the chances `1 - (1 - p)^n` for 1 and for 2 of the count, each printed by the number rule
/// against `p n`. A chance that is a result or a property leads with words. A chance without "for each" has no note.
pub fn chance_note(document: &SheetDocument, line_id: &str, values: &BTreeMap<String, f64>) -> Option<String> {
    let (card_index, line) = document.line(line_id)?;
    let phrase = chance_phrase(line)?;
    if phrase.tries.is_blank() {
        return None;
    }
    let relations = CardRelations::new(document, document.cards.get(card_index)?);
    let subject = subject(document, &relations, line_id, line);
    let tick = document.time.map(|time| time_words(time.tick_length, time.tick_unit));
    let sentences = Sentences::new(document);
    let count = sentences.slot(&phrase.tries, None);
    if let Some(chance) = fixed_number(phrase.chance.value(), values) {
        let chance = chance.clamp(0.0, 1.0);
        // With one counted, the chance is the chance itself. The subtraction would blur it.
        let first = chance_words(chance, chance);
        let second = chance_words(1.0 - (1.0 - chance).powi(2), 2.0 * chance);
        return Some(sentence::chance_for_each_note(
            tick.as_deref(),
            &subject,
            &count,
            &first,
            &second,
        ));
    }
    let chance_words = sentences.slot(&phrase.chance, None);
    Some(sentence::growing_chance_note(
        tick.as_deref(),
        &subject,
        &chance_words,
        &count,
    ))
}

/// Returns the name of the Count line that the chance or rate of line `line_id` takes as its value, with the kind of
/// draw, or `None` when its value is no count.
///
/// The line still runs, and the card offers to move the count into "for each".
pub fn count_as_draw(document: &SheetDocument, line_id: &str) -> Option<(String, sentence::DrawKind)> {
    let (_, line) = document.line(line_id)?;
    let (phrase, draw) = match (chance_phrase(line), rate_phrase(line)) {
        (Some(phrase), _) => (phrase, sentence::DrawKind::Chance),
        (None, Some(phrase)) => (phrase, sentence::DrawKind::Rate),
        _ => return None,
    };
    let value = match draw {
        sentence::DrawKind::Chance => phrase.chance.value(),
        sentence::DrawKind::Rate => phrase.rate.value(),
    };
    let Some(SheetValue::Chip(chip)) = value else {
        return None;
    };
    let count = whole_result(document, chip, LineKind::Count)?;
    Some((note_name(document, count)?, draw))
}

/// Returns the note of the competing group `group`, its line ids in display order, under its highest line.
fn group_note(
    document: &SheetDocument,
    relations: &CardRelations<'_>,
    time: SheetTime,
    group: &[&str],
    values: &BTreeMap<String, f64>,
) -> Option<RateNote> {
    let mut unit = None;
    let mut rates = Vec::with_capacity(group.len());
    for id in group {
        let (_, line) = document.line(id)?;
        let phrase = rate_phrase(line)?;
        if !rate_alone(relations, id, line)
            || !phrase.tries.is_blank()
            || unit.is_some_and(|unit| Some(unit) != phrase.rate_unit)
        {
            return None;
        }
        unit = phrase.rate_unit;
        let rate = fixed_number(phrase.rate.value(), values)?;
        let new_state = line
            .new_state
            .as_deref()
            .map(|state| state_name(document, line.property.as_deref(), state))?;
        rates.push((new_state, rate));
    }
    let unit = unit?;
    let (_, first) = document.line(group.first()?)?;
    let state = old_state(document, first)?;
    let dt = tick_in(time, unit);
    let total: f64 = rates.iter().map(|(_, rate)| rate).sum();
    let leaves = chance_from_rate(total, dt);
    let shares: Vec<(String, String)> = rates
        .iter()
        .map(|(new_state, rate)| {
            let share = if total > 0.0 { leaves * rate / total } else { 0.0 };
            (new_state.clone(), chance_words(share, rate * dt))
        })
        .collect();
    let share_words: Vec<(&str, &str)> = shares
        .iter()
        .map(|(new_state, share)| (new_state.as_str(), share.as_str()))
        .collect();
    let rate_words: Vec<String> = rates
        .iter()
        .map(|(_, rate)| format_number(*rate))
        .collect::<Option<_>>()?;
    let rate_refs: Vec<&str> = rate_words.iter().map(String::as_str).collect();
    let tick = time_words(dt, unit);
    let thing = sentence::thing_word(document, &relations.card().every);
    let text = sentence::competing_note(
        &tick,
        &state,
        &chance_words(leaves, total * dt),
        &share_words,
        &rate_refs,
        &thing,
    );
    let mean_stay = (total > 0.0).then(|| {
        sentence::mean_stay_note(
            &state,
            &time_words(mean_stay(total, dt), unit),
            &format!("({})", rate_refs.join(" + ")),
            &time_words(1.0 / total, unit),
            &thing,
        )
    });
    Some(RateNote {
        text,
        mean_stay,
        long_tick: None,
    })
}

/// Returns the unit of time that the Unit of `parameter` names, as "per day" names a day, `None` for a parameter whose
/// Unit names none.
///
/// The first "per second" to "per year" anywhere in the Unit counts, so "per day per infected neighbour" names a day.
/// A parameter whose Unit names a unit of time is a *rate parameter*.
pub fn parameter_rate_unit(parameter: &SheetParameter) -> Option<TimeUnit> {
    parameter.unit.as_deref().and_then(super::edit::unit_in_text)
}

/// Returns the number typed in `slot`, `None` for a slot that holds no typed number.
pub(in crate::sheet) fn typed_number(slot: &Slot) -> Option<f64> {
    match slot.value()? {
        SheetValue::Literal(literal) => literal.number(),
        _ => None,
    }
}

/// Returns the kind of draw that line `line_id` can change into while keeping its chance per tick, `None` for none.
///
/// A line that draws with a chance can change into a rate, and a line that draws at a rate into a chance.
/// The draw is the line itself or its condition, and holds a typed number: a chance from 0 to below 1, or a rate with
/// a unit. One tick of the sheet needs a length. A card that runs once has no duration and takes no rate, so a chance
/// there changes into nothing. [`SheetEdit::ChangeToRate`](super::edit::SheetEdit::ChangeToRate) and
/// [`SheetEdit::ChangeToChance`](super::edit::SheetEdit::ChangeToChance) make the change.
pub fn conversion(document: &SheetDocument, line_id: &str) -> Option<sentence::DrawKind> {
    document.time?;
    let (card_index, line) = document.line(line_id)?;
    let holder = match line.kind {
        LineKind::Chance | LineKind::Rate => line,
        _ => match line.condition.value()? {
            SheetValue::Phrase(phrase) if matches!(phrase.kind, LineKind::Chance | LineKind::Rate) => phrase,
            _ => return None,
        },
    };
    if holder.kind == LineKind::Chance {
        let runs_once = document.cards.get(card_index)?.runs == CardRuns::Once;
        let chance = typed_number(&holder.chance)?;
        (!runs_once && (0.0..1.0).contains(&chance)).then_some(sentence::DrawKind::Rate)
    } else {
        holder.rate_unit?;
        typed_number(&holder.rate)?;
        Some(sentence::DrawKind::Chance)
    }
}

/// Returns whether `line` or a phrase inside it draws with a chance.
fn holds_chance(line: &SheetLine) -> bool {
    line.kind == LineKind::Chance || line.slot_values().any(value_holds_chance)
}

/// Returns whether `value` draws with a chance.
fn value_holds_chance(value: &SheetValue) -> bool {
    match value {
        SheetValue::Phrase(phrase) => holds_chance(phrase),
        SheetValue::List(items) => items.iter().any(value_holds_chance),
        _ => false,
    }
}

/// Returns the numbers of the lines that draw with a chance on the cards of `document` that run each tick, card by
/// card in file order and each card in display order.
///
/// A chance is per tick and does not follow the time scale. A card that runs once has no duration, and its lines are
/// left out.
pub fn chance_line_numbers(document: &SheetDocument) -> Vec<usize> {
    document
        .cards
        .iter()
        .filter(|card| card.runs == CardRuns::EachTick)
        .flat_map(|card| {
            let relations = CardRelations::new(document, card);
            relations
                .order()
                .iter()
                .filter(|id| relations.line(id).is_some_and(holds_chance))
                .filter_map(|id| relations.number(id))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The Count line that a fixed rate can grow with through "for each".
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct GrowCount {
    /// Id of the Count line.
    pub line: String,
    /// Name of the Count line's result, such as "Infected neighbours".
    pub name: String,
}

/// Returns the Count line that the rate of line `line_id` could grow with, `None` for a line that the editor does not
/// ask about.
///
/// The line is a Become whose condition is a rate, or a Rate line whose result is the whole condition of a Become. The
/// rate is a typed number or a parameter with no "for each". The count is the nearest Count line above the line, open
/// to it, that counts the cells around whose property, the one the Become changes, is the state the Become leads to,
/// as "Infected neighbours" counts for "Become Infected".
pub fn grow_count(document: &SheetDocument, line_id: &str) -> Option<GrowCount> {
    let (card_index, line) = document.line(line_id)?;
    let card = document.cards.get(card_index)?;
    let relations = CardRelations::new(document, card);
    let (rate, change) = match line.kind {
        LineKind::Become => match line.condition.value()? {
            SheetValue::Phrase(phrase) if phrase.kind == LineKind::Rate => (phrase.as_ref(), line),
            _ => return None,
        },
        LineKind::Rate => {
            let change = card.line_tables.values().find(|other| {
                other.kind == LineKind::Become
                    && matches!(
                        other.condition.value(),
                        Some(SheetValue::Chip(SheetChip::Result { line, part: None, property: None }))
                            if line == line_id
                    )
            })?;
            (line, change)
        }
        _ => return None,
    };
    if !rate.tries.is_blank() || fixed_number(rate.rate.value(), &parameter_defaults(document)).is_none() {
        return None;
    }
    let property = change.property.as_deref()?;
    let state = change.new_state.as_deref()?;
    let position = relations.index(line_id)?;
    let counts = |count: &SheetLine| {
        let Some(SheetValue::Phrase(whose)) = count.whose.value() else {
            return false;
        };
        count.kind == LineKind::Count
            && most_counted(count).is_some()
            && whose.test == Some(CompareTest::Is)
            && matches!(whose.left.value(), Some(SheetValue::Chip(SheetChip::Their(their))) if their == property)
            && matches!(whose.right.value(), Some(SheetValue::Chip(SheetChip::State(counted))) if counted == state)
    };
    let found = relations.order()[..position].iter().rev().find(|id| {
        let open = relations
            .parent(id)
            .is_none_or(|block| relations.is_within(line_id, block));
        open && relations.line(id).is_some_and(counts)
    })?;
    let (_, count) = document.line(found)?;
    Some(GrowCount {
        line: (*found).to_owned(),
        name: note_name(document, count)?,
    })
}

/// Returns the shortest decimal number that reads back as the same `f32` as `value`, the number a lowered sheet
/// computes with.
pub(super) fn shortest_f32(value: f64) -> f64 {
    let narrowed = value as f32;
    narrowed.to_string().parse().unwrap_or(value)
}
