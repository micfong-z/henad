//! Parts of a sheet that an edit takes away besides the item it deletes or sets: values that lines, starts, shares
//! kept, buttons and measures lose, and the lines, measures, colour rules, shades and painted shapes that go.
//!
//! A loss is found by comparing the sheet before an edit with the sheet after it. A line, a share kept, a button or a
//! measure *loses* a value when it holds fewer filled slots, list items, fields, formulas or steps after the edit. A
//! start loses a value when it starts otherwise, or when the ground no longer paints the state it starts on. An edit
//! that only clears references, as a delete does, loses something exactly where the sheet holds less, so a rename or a
//! new colour finds nothing.

use std::collections::HashMap;

use super::lines::slots;
use super::references::all_properties;
use super::{SheetBuilder, SheetEdit, SheetItem};
use crate::sheet::document::{
    ButtonStep, ColorRule, ColorRuleKind, PropertyType, SheetDocument, SheetLine, SheetProperty, SheetValue, Slot,
    StartRule, StartsAs,
};
use crate::sheet::relations::CardRelations;

/// Parts of a sheet that an edit takes away, by id.
///
/// Every list is in the order the sheet lists its items: lines in card order and then in the order each card shows
/// them, properties as the cells, each kind of agent and the ground list them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SheetLosses {
    /// Ids of the lines that lose a value, a list item, a field or a formula.
    pub lines: Vec<String>,
    /// Ids of the properties whose start loses a value or a row, or falls back on another state.
    pub starts: Vec<String>,
    /// Starts of agents on a state of the ground that the ground no longer paints on single cells, each as the id of
    /// the agents' property, the id of the ground property and the id of the state.
    pub start_cells: Vec<(String, String, String)>,
    /// Ids of the ground properties whose share kept each tick loses its value.
    pub kept: Vec<String>,
    /// Ids of the buttons that lose a step or a property of a step.
    pub buttons: Vec<String>,
    /// Ids of the measures whose value loses a part.
    pub measures: Vec<String>,
    /// Ids of the lines that go, with a card that goes with a deleted button among them.
    pub deleted_lines: Vec<String>,
    /// Ids of the measures that go.
    pub removed_measures: Vec<String>,
    /// Kinds of thing no longer coloured by a property, each with the id of that property, the Heading of agents
    /// coloured by heading included.
    pub colors: Vec<(String, String)>,
    /// Kinds of thing whose colour rule loses shades, each with the ids of the Number properties that no longer shade
    /// it.
    pub shades: Vec<(String, Vec<String>)>,
    /// Properties whose painted start loses shapes, each with the number of shapes lost.
    pub shapes: Vec<(String, usize)>,
}

impl SheetLosses {
    /// Returns what `after` lost from `before`.
    ///
    /// A property or a button that `after` no longer holds is compared no further. A line that `after` no longer holds
    /// counts as deleted, and a measure as removed.
    pub fn between(before: &SheetDocument, after: &SheetDocument) -> Self {
        let mut losses = Self::default();
        for card in &before.cards {
            let relations = CardRelations::new(before, card);
            for &id in relations.order() {
                let Some(line) = card.line_tables.get(id) else {
                    continue;
                };
                match after.line(id) {
                    Some((_, kept)) if filled_in_line(kept) < filled_in_line(line) => losses.lines.push(id.to_owned()),
                    Some(_) => {}
                    None => losses.deleted_lines.push(id.to_owned()),
                }
            }
        }
        let kept_properties: HashMap<&str, &SheetProperty> = all_properties(after)
            .map(|property| (property.id.as_str(), property))
            .collect();
        for property in all_properties(before) {
            let Some(kept) = kept_properties.get(property.id.as_str()) else {
                continue;
            };
            let shapes = shape_count(property).saturating_sub(shape_count(kept));
            if shapes > 0 {
                losses.shapes.push((property.id.clone(), shapes));
            } else if without_key(property.starts_as.as_ref()) != without_key(kept.starts_as.as_ref()) {
                losses.starts.push(property.id.clone());
            }
            if let Some((ground, state)) = unpainted_start(before, after, kept) {
                losses.start_cells.push((property.id.clone(), ground, state));
            }
            if filled_in_slot(&kept.kept_per_tick) < filled_in_slot(&property.kept_per_tick) {
                losses.kept.push(property.id.clone());
            }
        }
        for button in &before.buttons {
            let kept = after.buttons.iter().find(|kept| kept.id == button.id);
            if let Some(kept) = kept
                && filled_in_steps(&kept.does) < filled_in_steps(&button.does)
            {
                losses.buttons.push(button.id.clone());
            }
        }
        for measure in &before.measures {
            match after.measures.iter().find(|kept| kept.id == measure.id) {
                None => losses.removed_measures.push(measure.id.clone()),
                Some(kept) if filled_in_slot(&kept.value) < filled_in_slot(&measure.value) => {
                    losses.measures.push(measure.id.clone());
                }
                Some(_) => {}
            }
        }
        for (thing, rule) in &before.colors {
            let kept = after.colors.get(thing);
            if let Some(property) = colored_by(before, thing, rule)
                && kept.and_then(|kept| colored_by(after, thing, kept)) != Some(property.clone())
            {
                losses.colors.push((thing.clone(), property));
            }
            let still: Vec<&str> = kept
                .filter(|kept| kept.shade_state.is_some() || rule.shade_state.is_none())
                .map(|kept| kept.shades.iter().map(|shade| shade.property.as_str()).collect())
                .unwrap_or_default();
            let lost: Vec<String> = rule
                .shades
                .iter()
                .filter(|shade| !still.contains(&shade.property.as_str()))
                .map(|shade| shade.property.clone())
                .collect();
            if !lost.is_empty() {
                losses.shades.push((thing.clone(), lost));
            }
        }
        losses
    }

    /// Returns what `edit` takes away from `document` besides the item it deletes or sets itself.
    ///
    /// The start that [`SheetEdit::SetStartsAs`] or [`SheetEdit::SetPropertyType`] sets afresh, with the share kept of
    /// a retyped property, is left out, but its painted shapes are not, since no control paints them again. So are the
    /// colour rule that [`SheetEdit::SetColorRule`] sets, but not its shades, the measure and the line a delete
    /// names, and the value of the summary that [`SheetEdit::DeleteSummary`] removes. An edit that is refused takes
    /// nothing.
    ///
    /// ```
    /// use henad_graph::sheet::ParsedSheet;
    /// use henad_graph::sheet::edit::{SheetEdit, SheetItem, SheetLosses};
    ///
    /// let sir = henad_graph::sheet::starters()
    ///     .iter()
    ///     .find(|starter| starter.file_name == "sir.toml")
    ///     .expect("SIR is a starter");
    /// let document = ParsedSheet::parse(sir.text).expect("the starter reads").into_document();
    /// let delete = SheetEdit::Delete {
    ///     item: SheetItem::Property("health".to_owned()),
    /// };
    /// let losses = SheetLosses::of_edit(&document, &delete);
    /// assert_eq!(losses.lines, ["l1", "l2", "l3", "l4"]);
    /// assert_eq!(losses.removed_measures, ["susceptible", "infected", "recovered"]);
    /// assert_eq!(losses.colors, [("cells".to_owned(), "health".to_owned())]);
    /// ```
    pub fn of_edit(document: &SheetDocument, edit: &SheetEdit) -> Self {
        let mut builder = SheetBuilder::new(document.clone()).with_key_seed(0);
        if builder.apply(edit.clone()).is_err() {
            return Self::default();
        }
        let after = builder.document();
        let mut losses = Self::between(document, after);
        match edit {
            SheetEdit::SetStartsAs { property, .. } | SheetEdit::SetPropertyType { property, .. } => {
                losses.leave_out_start(property);
            }
            SheetEdit::SetColorRule { thing, .. } => losses.colors.retain(|(colored, _)| colored != thing),
            SheetEdit::Delete {
                item: SheetItem::Measure(id),
            } => losses.removed_measures.retain(|measure| measure != id),
            SheetEdit::DeleteLine { line } => losses.deleted_lines.retain(|deleted| deleted != line),
            SheetEdit::DeleteSummary { line, index } => {
                // A removed summary takes its own value with it, and its line loses nothing else of its own.
                let before = document.line(line).map(|(_, kept)| kept);
                let removed = before
                    .and_then(|kept| kept.summaries.get(*index))
                    .map_or(0, |summary| filled_in_slot(&summary.value));
                let loses = before
                    .zip(after.line(line))
                    .is_some_and(|(before, (_, kept))| filled_in_line(kept) + removed < filled_in_line(before));
                if !loses {
                    losses.lines.retain(|id| id != line);
                }
            }
            _ => {}
        }
        losses
    }

    /// Leaves out the start and the share kept of property `property`, which an edit sets afresh. Its painted shapes
    /// stay.
    pub fn leave_out_start(&mut self, property: &str) {
        self.starts.retain(|id| id != property);
        self.kept.retain(|id| id != property);
    }

    /// Returns whether nothing is lost.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
            && self.starts.is_empty()
            && self.start_cells.is_empty()
            && self.kept.is_empty()
            && self.buttons.is_empty()
            && self.measures.is_empty()
            && self.deleted_lines.is_empty()
            && self.removed_measures.is_empty()
            && self.colors.is_empty()
            && self.shades.is_empty()
            && self.shapes.is_empty()
    }
}

/// Returns `start` without its key, which a command can give anew while the start draws as before.
fn without_key(start: Option<&StartsAs>) -> Option<StartsAs> {
    start.cloned().map(|mut start| {
        start.key = None;
        start
    })
}

/// Returns the number of shapes of the painted start of `property`, 0 for any other start.
fn shape_count(property: &SheetProperty) -> usize {
    property
        .starts_as
        .as_ref()
        .filter(|start| start.rule == StartRule::Painted)
        .map_or(0, |start| start.shapes.len())
}

/// Returns the ground property and the state that `property` of `after` starts on, when `before` paints that state on
/// single cells and `after` does not. `None` for any other start.
fn unpainted_start(
    before: &SheetDocument,
    after: &SheetDocument,
    property: &SheetProperty,
) -> Option<(String, String)> {
    let start = property
        .starts_as
        .as_ref()
        .filter(|start| start.rule == StartRule::OnGround)?;
    let (ground, state) = (start.property.as_deref()?, start.state.as_deref()?);
    let paints = |document: &SheetDocument| {
        all_properties(document)
            .find(|known| known.id == ground)
            .and_then(|known| known.starts_as.as_ref())
            .is_some_and(|start| start.paints_single_cells(state))
    };
    (paints(before) && !paints(after)).then(|| (ground.to_owned(), state.to_owned()))
}

/// Returns the id of the property that `rule` colours `thing` of `document` by: its property, or the Heading of
/// agents coloured by heading. `None` for a rule that names no property.
fn colored_by(document: &SheetDocument, thing: &str, rule: &ColorRule) -> Option<String> {
    if rule.rule != ColorRuleKind::ByHeading {
        return rule.property.clone();
    }
    document
        .agents
        .iter()
        .find(|kind| kind.id == thing)?
        .properties
        .iter()
        .find(|property| property.property_type == PropertyType::Heading)
        .map(|property| property.id.clone())
}

/// Returns the number of filled parts of `steps`: each step, and each thing, property, card and filled value in it.
fn filled_in_steps(steps: &[ButtonStep]) -> usize {
    steps
        .iter()
        .map(|step| {
            let fields = [&step.thing, &step.property, &step.card]
                .into_iter()
                .filter(|field| field.is_some())
                .count();
            1 + fields + step.properties.len() + filled_in_slot(&step.value)
        })
        .sum()
}

/// Returns the number of filled parts of `line` and the phrases inside it: each field that names an item, its formula,
/// each filled slot, summary value, row, branch and list item.
///
/// The lines inside a block are lines of their own and do not count.
fn filled_in_line(line: &SheetLine) -> usize {
    let fields = [
        &line.property,
        &line.new_state,
        &line.start_state,
        &line.agent_kind,
        &line.ground_property,
        &line.ranked_by,
        &line.event,
        &line.formula,
    ]
    .into_iter()
    .filter(|field| field.is_some())
    .count();
    let slots: usize = slots(line).into_iter().map(filled_in_slot).sum();
    let summaries: usize = line
        .summaries
        .iter()
        .map(|summary| filled_in_slot(&summary.value))
        .sum();
    let rows: usize = line
        .rows
        .iter()
        .map(|row| filled_in_slot(&row.value) + filled_in_slot(&row.condition))
        .sum();
    let branches: usize = line.branches.values().map(|value| 1 + filled_in_value(value)).sum();
    let items: usize = line.items.iter().map(|item| 1 + filled_in_value(item)).sum();
    fields + slots + summaries + rows + branches + items
}

/// Returns the number of filled parts of `slot`: none for a blank slot, and otherwise the value and the parts inside
/// it.
fn filled_in_slot(slot: &Slot) -> usize {
    slot.value().map_or(0, |value| 1 + filled_in_value(value))
}

/// Returns the number of filled parts inside `value`: those of a phrase, and each item of a list with its parts.
fn filled_in_value(value: &SheetValue) -> usize {
    match value {
        SheetValue::Phrase(phrase) => filled_in_line(phrase),
        SheetValue::List(items) => items.iter().map(|item| 1 + filled_in_value(item)).sum(),
        SheetValue::Literal(_) | SheetValue::Chip(_) => 0,
    }
}
