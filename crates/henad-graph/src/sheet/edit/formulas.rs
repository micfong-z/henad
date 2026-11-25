//! Formulas that an edit empties and the editor cannot write again.
//!
//! The card writes a formula of one operation between two values, such as `l1 * infection_rate`, on a Calculate line
//! of its own, and a picker offers the length of one of my properties, `length(my.velocity)`, as a value. The Sheet
//! pane writes no formula of a measure. Any other formula is written in the model file alone. A delete empties every
//! formula that names what it deletes.

use super::lines::slots;
use super::{SheetBuilder, SheetEdit};
use crate::ids::is_id;
use crate::sheet::document::{LineKind, SheetDocument, SheetLine, SheetMeasure, SheetValue, Slot};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::binary_formula;

/// Returns the ids of the lines that `edit` leaves without a formula the card cannot write again, in card order and
/// then in the order each card shows its lines.
///
/// A line that `edit` deletes, or that goes with what it deletes, is left out, and so is the value of a summary that
/// `edit` removes. `edit` runs on a copy of `document`, and an edit that is refused leaves out every line.
///
/// ```
/// use henad_graph::sheet::ParsedSheet;
/// use henad_graph::sheet::edit::{SheetEdit, SheetItem, lines_losing_formulas};
///
/// let ants = henad_graph::sheet::starters()
///     .iter()
///     .find(|starter| starter.file_name == "ants.toml")
///     .expect("the ants are a starter");
/// let document = ParsedSheet::parse(ants.text).expect("the starter reads").into_document();
/// // Lines 3 and 4 leave trail_falloff * l2.amount + my.reward_held and its twin on the ground.
/// let delete = SheetEdit::Delete {
///     item: SheetItem::Parameter("trail_falloff".to_owned()),
/// };
/// assert_eq!(lines_losing_formulas(&document, &delete), ["l3", "l4"]);
/// ```
pub fn lines_losing_formulas(document: &SheetDocument, edit: &SheetEdit) -> Vec<String> {
    let Some(after) = edited(document, edit) else {
        return Vec::new();
    };
    let after = after.document();
    let mut lines = Vec::new();
    for card in &document.cards {
        let relations = CardRelations::new(document, card);
        for &id in relations.order() {
            let (Some(line), Some((_, kept))) = (card.line_tables.get(id), after.line(id)) else {
                continue;
            };
            // A removed summary takes its own value with it, as a deleted line does.
            let removed = match edit {
                SheetEdit::DeleteSummary { line: target, index } if target == id => line
                    .summaries
                    .get(*index)
                    .map_or(0, |summary| count_in_slot(&summary.value, &card_cannot_write)),
                _ => 0,
            };
            // A delete empties formulas and rewrites none, so a line that holds fewer of them lost one.
            let count = |line: &SheetLine| count_in_line(line, false, &card_cannot_write);
            if count(kept) + removed < count(line) {
                lines.push(id.to_owned());
            }
        }
    }
    lines
}

/// Returns the ids of the measures whose value `edit` leaves without a formula, in the order the sheet lists them.
///
/// The Sheet pane writes no formula of a measure, so every formula counts. A measure that `edit` deletes, or that goes
/// with what it deletes, is left out. `edit` runs on a copy of `document`, and an edit that is refused leaves out every
/// measure.
///
/// ```
/// use henad_graph::sheet::ParsedSheet;
/// use henad_graph::sheet::edit::{SheetEdit, SheetItem, measures_losing_formulas};
///
/// let ants = henad_graph::sheet::starters()
///     .iter()
///     .find(|starter| starter.file_name == "ants.toml")
///     .expect("the ants are a starter");
/// let document = ParsedSheet::parse(ants.text).expect("the starter reads").into_document();
/// // Total trail adds up my.to_food + my.to_home over the ground.
/// let delete = SheetEdit::Delete {
///     item: SheetItem::Property("to_food".to_owned()),
/// };
/// assert_eq!(measures_losing_formulas(&document, &delete), ["total_trail"]);
/// ```
pub fn measures_losing_formulas(document: &SheetDocument, edit: &SheetEdit) -> Vec<String> {
    let Some(after) = edited(document, edit) else {
        return Vec::new();
    };
    let after = after.document();
    let count = |measure: &SheetMeasure| count_in_slot(&measure.value, &|_, _| true);
    document
        .measures
        .iter()
        .filter(|measure| {
            after
                .measures
                .iter()
                .find(|kept| kept.id == measure.id)
                .is_some_and(|kept| count(kept) < count(measure))
        })
        .map(|measure| measure.id.clone())
        .collect()
}

/// Returns a builder over a copy of `document` with `edit` applied, `None` for an edit that is refused.
fn edited(document: &SheetDocument, edit: &SheetEdit) -> Option<SheetBuilder> {
    let mut builder = SheetBuilder::new(document.clone()).with_key_seed(0);
    builder.apply(edit.clone()).ok()?;
    Some(builder)
}

/// Returns whether the card cannot write `formula` again. `inline` is whether the formula sits inside a slot.
///
/// The card writes one operation between two values on a Calculate line of its own, and in a slot only the length of
/// one of my properties that a picker offers.
fn card_cannot_write(formula: &str, inline: bool) -> bool {
    if inline {
        !is_length_of_mine(formula)
    } else {
        binary_formula(formula).is_none()
    }
}

/// Returns whether `formula` is the length of one of my properties, `length(my.<id>)`, whitespace aside.
fn is_length_of_mine(formula: &str) -> bool {
    let compact: String = formula.chars().filter(|c| !c.is_whitespace()).collect();
    compact
        .strip_prefix("length(my.")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(is_id)
}

/// Returns the number of formulas of `line` and the phrases inside it for which `unwritten` holds. `inline` is whether
/// `line` is a phrase inside a slot, and `unwritten` takes a formula and whether it sits inside a slot.
fn count_in_line(line: &SheetLine, inline: bool, unwritten: &impl Fn(&str, bool) -> bool) -> usize {
    let own = match &line.formula {
        Some(formula) if line.kind == LineKind::Calculate && unwritten(formula, inline) => 1,
        _ => 0,
    };
    let values = slots(line)
        .into_iter()
        .chain(line.summaries.iter().map(|summary| &summary.value))
        .chain(line.rows.iter().flat_map(|row| [&row.value, &row.condition]))
        .filter_map(Slot::value)
        .chain(line.branches.values())
        .chain(&line.items);
    own + values.map(|value| count_in_value(value, unwritten)).sum::<usize>()
}

/// Returns the number of formulas in the value of `slot` for which `unwritten` holds, 0 for a blank slot.
fn count_in_slot(slot: &Slot, unwritten: &impl Fn(&str, bool) -> bool) -> usize {
    slot.value().map_or(0, |value| count_in_value(value, unwritten))
}

/// Returns the number of formulas of the phrases inside `value` for which `unwritten` holds.
fn count_in_value(value: &SheetValue, unwritten: &impl Fn(&str, bool) -> bool) -> usize {
    match value {
        SheetValue::Phrase(phrase) => count_in_line(phrase, true, unwritten),
        SheetValue::List(items) => items.iter().map(|item| count_in_value(item, unwritten)).sum(),
        SheetValue::Literal(_) | SheetValue::Chip(_) => 0,
    }
}
