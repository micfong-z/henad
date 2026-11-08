//! Slot paths, phrase depth and the outline moves of a card's lines.
//!
//! A *slot path* names one slot of a line, or of a phrase inside it, by the keys of the author format joined with
//! dots: `condition`, `condition.rate`, `whose.right`, `items.2`, `row.0.condition` or `branches.alive`. Every step
//! but the last descends into the phrase that a slot holds.
//!
//! The *outline* of a card is its lines with the lines inside each When or Repeat. A move follows it: the first line
//! inside a block moves up out of it and the last moves down out of it, a line just below or above a block moves into
//! it at its end or its start, and a block moves with its lines and swaps with the line or block beside it.

use super::{MoveDirection, SheetEditError};
use crate::sheet::document::{ChooseRow, LineKind, SheetCard, SheetLine, SheetValue, Slot};

/// One step of a slot path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PathStep {
    /// A slot by its key, such as `condition`.
    Slot(String),
    /// An item of an all of, any of or none of, by index.
    Item(usize),
    /// The value or the condition of a row of a Choose line, by index.
    Row(usize, RowSlot),
    /// The value of a Depending on line for a state, by state id.
    Branch(String),
    /// The value of a summary of a Look around line, by index.
    Summary(usize),
}

/// A slot of a row of a Choose line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RowSlot {
    /// The row's value.
    Value,
    /// The row's condition.
    Condition,
}

/// Returns the steps of the slot path `path`.
///
/// # Errors
///
/// Returns [`SheetEditError::Invalid`] for an empty path, an unknown key or an index that is not a number.
pub(crate) fn parse_path(path: &str) -> Result<Vec<PathStep>, SheetEditError> {
    let invalid = || SheetEditError::Invalid(format!("'{path}' is no slot path"));
    let index = |text: Option<&&str>| -> Option<usize> { text?.parse().ok() };
    let segments: Vec<&str> = path.split('.').collect();
    let mut steps = Vec::new();
    let mut position = 0;
    while let Some(segment) = segments.get(position) {
        match *segment {
            "items" => {
                steps.push(PathStep::Item(index(segments.get(position + 1)).ok_or_else(invalid)?));
                position += 2;
            }
            "row" => {
                let row = index(segments.get(position + 1)).ok_or_else(invalid)?;
                let slot = match segments.get(position + 2) {
                    Some(&"value") => RowSlot::Value,
                    Some(&"condition") => RowSlot::Condition,
                    _ => return Err(invalid()),
                };
                steps.push(PathStep::Row(row, slot));
                position += 3;
            }
            "branches" => {
                let state = segments.get(position + 1).ok_or_else(invalid)?;
                steps.push(PathStep::Branch((*state).to_owned()));
                position += 2;
            }
            "summaries" => {
                let summary = index(segments.get(position + 1)).ok_or_else(invalid)?;
                if segments.get(position + 2) != Some(&"value") {
                    return Err(invalid());
                }
                steps.push(PathStep::Summary(summary));
                position += 3;
            }
            name if SLOT_KEYS.contains(&name) => {
                steps.push(PathStep::Slot(name.to_owned()));
                position += 1;
            }
            _ => return Err(invalid()),
        }
    }
    if steps.is_empty() {
        return Err(invalid());
    }
    Ok(steps)
}

/// Keys of the slots of a line, in the order of the author format.
const SLOT_KEYS: [&str; 21] = [
    "distance",
    "place",
    "left",
    "right",
    "tolerance",
    "chance",
    "rate",
    "tries",
    "low",
    "high",
    "length",
    "times",
    "value",
    "strength",
    "minimum",
    "maximum",
    "amount",
    "only",
    "whose",
    "condition",
    "otherwise",
];

/// Returns the slot of `line` with the key `name`.
pub(super) fn slot_mut<'a>(line: &'a mut SheetLine, name: &str) -> Option<&'a mut Slot> {
    Some(match name {
        "distance" => &mut line.distance,
        "place" => &mut line.place,
        "left" => &mut line.left,
        "right" => &mut line.right,
        "tolerance" => &mut line.tolerance,
        "chance" => &mut line.chance,
        "rate" => &mut line.rate,
        "tries" => &mut line.tries,
        "low" => &mut line.low,
        "high" => &mut line.high,
        "length" => &mut line.length,
        "times" => &mut line.times,
        "value" => &mut line.value,
        "strength" => &mut line.strength,
        "minimum" => &mut line.minimum,
        "maximum" => &mut line.maximum,
        "amount" => &mut line.amount,
        "only" => &mut line.only,
        "whose" => &mut line.whose,
        "condition" => &mut line.condition,
        "otherwise" => &mut line.otherwise,
        _ => return None,
    })
}

/// Returns the value at `step` of `line`, `None` when the line holds none there.
pub(crate) fn value_mut<'a>(line: &'a mut SheetLine, step: &PathStep) -> Option<&'a mut SheetValue> {
    match step {
        PathStep::Slot(name) => match slot_mut(line, name)? {
            Slot::Filled(value) => Some(value),
            Slot::Blank => None,
        },
        PathStep::Item(index) => line.items.get_mut(*index),
        PathStep::Row(index, slot) => {
            let row = line.rows.get_mut(*index)?;
            match slot {
                RowSlot::Value => match &mut row.value {
                    Slot::Filled(value) => Some(value),
                    Slot::Blank => None,
                },
                RowSlot::Condition => match &mut row.condition {
                    Slot::Filled(value) => Some(value),
                    Slot::Blank => None,
                },
            }
        }
        PathStep::Branch(state) => line.branches.get_mut(state),
        PathStep::Summary(index) => match &mut line.summaries.get_mut(*index)?.value {
            Slot::Filled(value) => Some(value),
            Slot::Blank => None,
        },
    }
}

/// Returns the phrase that `steps` lead to from `line`, or `line` itself for no steps.
///
/// # Errors
///
/// Returns [`SheetEditError::Missing`] when a step finds no phrase.
pub(crate) fn phrase_mut<'a>(line: &'a mut SheetLine, steps: &[PathStep]) -> Result<&'a mut SheetLine, SheetEditError> {
    let mut current = line;
    for step in steps {
        current = match value_mut(current, step) {
            Some(SheetValue::Phrase(phrase)) => phrase,
            _ => return Err(SheetEditError::Missing(format!("no phrase at {step:?}"))),
        };
    }
    Ok(current)
}

/// Puts `value` at the last step of `steps` in `line`.
///
/// A filled value at the index one past the last item or row adds one. A blank value takes an item or a branch out,
/// and leaves a slot or a row's slot blank.
///
/// # Errors
///
/// Returns [`SheetEditError::Missing`] when the path finds no phrase or no item, row or summary at its index.
pub(super) fn set_value(line: &mut SheetLine, steps: &[PathStep], value: Slot) -> Result<(), SheetEditError> {
    let (last, inner) = steps
        .split_last()
        .ok_or_else(|| SheetEditError::Invalid("an empty slot path".to_owned()))?;
    let phrase = phrase_mut(line, inner)?;
    let missing = || SheetEditError::Missing(format!("no slot at {last:?}"));
    match last {
        PathStep::Slot(name) => *slot_mut(phrase, name).ok_or_else(missing)? = value,
        PathStep::Item(index) => match value {
            Slot::Filled(value) if *index == phrase.items.len() => phrase.items.push(value),
            Slot::Filled(value) => *phrase.items.get_mut(*index).ok_or_else(missing)? = value,
            Slot::Blank if *index < phrase.items.len() => {
                phrase.items.remove(*index);
            }
            Slot::Blank => return Err(missing()),
        },
        PathStep::Row(index, slot) => {
            if *index == phrase.rows.len() && !value.is_blank() {
                phrase.rows.push(ChooseRow::default());
            }
            let row = phrase.rows.get_mut(*index).ok_or_else(missing)?;
            match slot {
                RowSlot::Value => row.value = value,
                RowSlot::Condition => row.condition = value,
            }
        }
        PathStep::Branch(state) => match value {
            Slot::Filled(value) => {
                phrase.branches.insert(state.clone(), value);
            }
            Slot::Blank => {
                phrase.branches.remove(state);
            }
        },
        PathStep::Summary(index) => phrase.summaries.get_mut(*index).ok_or_else(missing)?.value = value,
    }
    Ok(())
}

/// Returns the depth of the tables and lists that `value` writes in a sheet's text, counted generously.
pub(super) fn value_depth(value: &SheetValue) -> usize {
    match value {
        SheetValue::Literal(_) => 0,
        SheetValue::Chip(_) => 1,
        SheetValue::Phrase(phrase) => 1 + line_depth(phrase),
        SheetValue::List(items) => 2 + items.iter().map(value_depth).max().unwrap_or(0),
    }
}

/// Returns the depth of the tables and lists below a line's own table, counted generously.
pub(super) fn line_depth(line: &SheetLine) -> usize {
    let slots = [
        &line.distance,
        &line.place,
        &line.left,
        &line.right,
        &line.tolerance,
        &line.chance,
        &line.rate,
        &line.tries,
        &line.low,
        &line.high,
        &line.length,
        &line.times,
        &line.value,
        &line.strength,
        &line.minimum,
        &line.maximum,
        &line.amount,
        &line.only,
        &line.whose,
        &line.condition,
        &line.otherwise,
    ];
    let slot_depth = slots
        .into_iter()
        .filter_map(Slot::value)
        .map(value_depth)
        .max()
        .unwrap_or(0);
    let items = line.items.iter().map(|item| 2 + value_depth(item)).max().unwrap_or(0);
    let branches = line
        .branches
        .values()
        .map(|value| 1 + value_depth(value))
        .max()
        .unwrap_or(0);
    let rows = line
        .rows
        .iter()
        .flat_map(|row| [&row.value, &row.condition])
        .filter_map(Slot::value)
        .map(|value| 2 + value_depth(value))
        .max()
        .unwrap_or(0);
    let summaries = line
        .summaries
        .iter()
        .filter_map(|summary| summary.value.value())
        .map(|value| 2 + value_depth(value))
        .max()
        .unwrap_or(0);
    slot_depth.max(items).max(branches).max(rows).max(summaries)
}

/// Returns whether `kind` holds lines: a When or a Repeat.
pub(super) fn is_block(kind: LineKind) -> bool {
    matches!(kind, LineKind::When | LineKind::Repeat)
}

/// Returns the block line of `card` that lists `line_id`, `None` for a line at the top of the card.
pub(super) fn parent_of(card: &SheetCard, line_id: &str) -> Option<String> {
    card.line_tables
        .iter()
        .find(|(_, line)| line.lines.iter().any(|inner| inner == line_id))
        .map(|(id, _)| id.clone())
}

/// Returns the list of line ids that holds the lines inside `parent`, or the card's top-level list for `None`.
pub(super) fn list_mut<'a>(card: &'a mut SheetCard, parent: Option<&str>) -> Option<&'a mut Vec<String>> {
    match parent {
        None => Some(&mut card.lines),
        Some(parent) => card.line_tables.get_mut(parent).map(|line| &mut line.lines),
    }
}

/// Returns whether the line `line_id` of `card` holds lines.
fn holds_lines(card: &SheetCard, line_id: &str) -> bool {
    card.line_tables.get(line_id).is_some_and(|line| is_block(line.kind))
}

/// Moves the line `line_id` of `card`, with the lines inside it, one step up or down the outline.
///
/// # Errors
///
/// Returns [`SheetEditError::Missing`] for a line the card does not list, and [`SheetEditError::Invalid`] for a line
/// already at the top or the bottom of the card.
pub(super) fn move_line(card: &mut SheetCard, line_id: &str, direction: MoveDirection) -> Result<(), SheetEditError> {
    let card_id = card.id.clone();
    let missing = || SheetEditError::Missing(format!("card '{card_id}' lists no line '{line_id}'"));
    let parent = parent_of(card, line_id);
    let siblings = list_mut(card, parent.as_deref()).ok_or_else(missing)?.clone();
    let index = siblings.iter().position(|id| id == line_id).ok_or_else(missing)?;
    let block = holds_lines(card, line_id);
    let (edge, neighbor) = match direction {
        MoveDirection::Up => (index == 0, index.checked_sub(1)),
        MoveDirection::Down => (index + 1 == siblings.len(), Some(index + 1)),
    };
    if edge {
        // The first line of a block moves up out of it, the last moves down out of it.
        let Some(parent) = parent else {
            return Err(SheetEditError::Invalid(format!(
                "line '{line_id}' is already at the {} of the card",
                match direction {
                    MoveDirection::Up => "top",
                    MoveDirection::Down => "bottom",
                }
            )));
        };
        let grandparent = parent_of(card, &parent);
        list_mut(card, Some(&parent)).ok_or_else(missing)?.remove(index);
        let outer = list_mut(card, grandparent.as_deref()).ok_or_else(missing)?;
        let at = outer.iter().position(|id| *id == parent).ok_or_else(missing)?;
        let at = match direction {
            MoveDirection::Up => at,
            MoveDirection::Down => at + 1,
        };
        outer.insert(at, line_id.to_owned());
        return Ok(());
    }
    let neighbor_index = neighbor.ok_or_else(missing)?;
    let neighbor = siblings[neighbor_index].clone();
    if !block && holds_lines(card, &neighbor) {
        // A line beside a block moves into it, at its end from below and at its start from above.
        list_mut(card, parent.as_deref()).ok_or_else(missing)?.remove(index);
        let inner = list_mut(card, Some(&neighbor)).ok_or_else(missing)?;
        match direction {
            MoveDirection::Up => inner.push(line_id.to_owned()),
            MoveDirection::Down => inner.insert(0, line_id.to_owned()),
        }
        return Ok(());
    }
    list_mut(card, parent.as_deref())
        .ok_or_else(missing)?
        .swap(index, neighbor_index);
    Ok(())
}

/// Takes the line `line_id` out of the outline of `card` and its table, and puts the lines inside it in its place.
///
/// Returns the line taken out.
///
/// # Errors
///
/// Returns [`SheetEditError::Missing`] for a line the card does not list.
pub(super) fn remove_line(card: &mut SheetCard, line_id: &str) -> Result<SheetLine, SheetEditError> {
    let card_id = card.id.clone();
    let missing = || SheetEditError::Missing(format!("card '{card_id}' lists no line '{line_id}'"));
    let parent = parent_of(card, line_id);
    let line = card.line_tables.remove(line_id).ok_or_else(missing)?;
    let list = list_mut(card, parent.as_deref()).ok_or_else(missing)?;
    let index = list.iter().position(|id| id == line_id).ok_or_else(missing)?;
    list.remove(index);
    for (offset, inner) in line.lines.iter().enumerate() {
        list.insert(index + offset, inner.clone());
    }
    Ok(line)
}
