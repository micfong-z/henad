//! Automatic result names: the name a result line takes after what it works out, and the rule that keeps them up to
//! date and unique.
//!
//! A result line *follows* its automatic name while its stored name equals the automatic name the line had before an
//! edit, a number such as " 2" after it included. An edit renames every line that follows, and leaves a name typed by
//! hand as it is. Result names are unique across the sheet, case aside: a repeated automatic name takes " 2", " 3" and
//! so on, in display order, after the names typed by hand.

use std::collections::{HashMap, HashSet};

use super::references::{Change, Target, change_references};
use super::{IdSpace, mint_id};
use crate::sheet::check::RESERVED_PREFIX;
use crate::sheet::document::{
    Around, ChipRelation, LineKind, LookSummary, SheetChip, SheetDocument, SheetLine, SheetValue, Slot, SummaryKind,
};
use crate::sheet::relations::CardRelations;
use crate::sheet::sentence::{
    self, ALL_OF_RESULT, ANY_OF_RESULT, BLANK_TEXT, CALCULATE_RESULT_EMPTY, COUNT_RESULT_EMPTY, HAPPENS, Sentences,
    TEST_RESULT_EMPTY, UNIT_PLACEHOLDER,
};

/// Returns the automatic name of the result line `line` of `document`, `None` for a line of a kind that has none.
///
/// The names follow the editor's wording table:
///
/// - Count: "Alive neighbours", or "B in my block" for the 3 by 3 block, and "Count" while its state is empty.
/// - Total or average, a Look around over cells with one total or average: "Total Food around".
/// - With chance and at a rate: "Recovery happens" for the parameter Recovery rate, "Infection happens" for a
///   Calculate result that holds the parameter Infection rate, "0.1 per day happens" or "30% chance happens" for a
///   typed number, another value's name otherwise, and "Happens" while the value is empty.
/// - Compare and Calculate: their sentence, "Test" or "Result" while empty.
/// - All of and any of: "All of" and "Any of".
///
/// ```
/// use henad_graph::sheet::ParsedSheet;
/// use henad_graph::sheet::edit::automatic_name;
///
/// let sheet = ParsedSheet::parse(henad_graph::sheet::starters()[0].text).expect("the starter reads");
/// let document = sheet.document();
/// let (_, count) = document.line("l1").expect("Game of Life has line 1");
/// assert_eq!(automatic_name(document, count).as_deref(), Some("Alive neighbours"));
/// ```
pub fn automatic_name(document: &SheetDocument, line: &SheetLine) -> Option<String> {
    let sentences = Sentences::new(document);
    automatic_name_with(document, &sentences, line)
}

/// Returns the automatic name of `line`, with the sentences of `document` already made.
fn automatic_name_with(document: &SheetDocument, sentences: &Sentences<'_>, line: &SheetLine) -> Option<String> {
    match line.kind {
        LineKind::Count => Some(count_name(sentences, line)),
        LineKind::LookAround => total_name(document, line),
        LineKind::Chance => Some(happens_name(document, sentences, line, &line.chance)),
        LineKind::Rate => Some(happens_name(document, sentences, line, &line.rate)),
        LineKind::Compare => Some(if compare_is_filled(line) {
            sentences.result_words(line)
        } else {
            TEST_RESULT_EMPTY.to_owned()
        }),
        LineKind::Calculate => Some(
            if line
                .formula
                .as_deref()
                .is_some_and(|formula| !formula.trim().is_empty())
            {
                sentences.result_words(line)
            } else {
                CALCULATE_RESULT_EMPTY.to_owned()
            },
        ),
        LineKind::AllOf => Some(ALL_OF_RESULT.to_owned()),
        LineKind::AnyOf => Some(ANY_OF_RESULT.to_owned()),
        _ => None,
    }
}

/// Returns the automatic name of a Count line: its state and neighbourhood, or "Count" while its state is empty.
fn count_name(sentences: &Sentences<'_>, line: &SheetLine) -> String {
    let Some(SheetValue::Phrase(whose)) = line.whose.value() else {
        return COUNT_RESULT_EMPTY.to_owned();
    };
    let property = match whose.left.value() {
        Some(SheetValue::Chip(SheetChip::Their(property))) => property.as_str(),
        _ => return COUNT_RESULT_EMPTY.to_owned(),
    };
    match whose.right.value() {
        Some(state @ SheetValue::Chip(SheetChip::State(_))) => sentence::count_result_name(
            &sentences.value(state, Some(property)),
            line.around == Some(Around::Block3x3),
        ),
        _ => COUNT_RESULT_EMPTY.to_owned(),
    }
}

/// Returns the automatic name of a Total or average line, `None` for any other Look around.
fn total_name(document: &SheetDocument, line: &SheetLine) -> Option<String> {
    let around = line.around?;
    if around == Around::AgentsWithin || !line.only.is_blank() {
        return None;
    }
    let [summary] = line.summaries.as_slice() else {
        return None;
    };
    let average = match summary.summary? {
        SummaryKind::Total => false,
        SummaryKind::Average => true,
        _ => return None,
    };
    let property = match summary.value.value() {
        Some(SheetValue::Chip(SheetChip::Their(id))) => super::references::all_properties(document)
            .find(|property| property.id == *id)
            .map_or(id.as_str(), |property| property.name.as_str()),
        _ => BLANK_TEXT,
    };
    Some(sentence::total_result_name(property, average))
}

/// Returns the automatic name of a chance or rate result whose value sits in `slot`.
fn happens_name(document: &SheetDocument, sentences: &Sentences<'_>, line: &SheetLine, slot: &Slot) -> String {
    let Some(value) = slot.value() else {
        return HAPPENS.to_owned();
    };
    match value {
        SheetValue::Chip(SheetChip::Param(id)) => {
            match document.parameters.iter().find(|parameter| parameter.id == *id) {
                Some(parameter) => sentence::happens_result_name(sentence::happens_base(&parameter.name)),
                None => HAPPENS.to_owned(),
            }
        }
        SheetValue::Literal(literal) if literal.number().is_some() => {
            let number = sentences.value(value, None);
            let base = match (line.kind, line.rate_unit) {
                (LineKind::Rate, Some(unit)) => sentence::typed_rate_base(&number, unit),
                (LineKind::Rate, None) => format!("{number} per {UNIT_PLACEHOLDER}"),
                _ => sentence::typed_chance_base(&number),
            };
            sentence::happens_result_name(&base)
        }
        SheetValue::Chip(SheetChip::Result {
            line: id,
            part: None,
            property: None,
        }) => {
            // A worked-out rate, such as Infected neighbours times Infection rate, takes the name of the parameter in
            // it.
            let parameter = document
                .line(id)
                .filter(|(_, source)| source.kind == LineKind::Calculate)
                .and_then(|(_, source)| source.formula.as_deref())
                .and_then(|formula| {
                    let mut found = None;
                    super::references::rewrite_formula(formula, |head, tail| {
                        if found.is_none()
                            && tail.is_none()
                            && let Some(parameter) = document.parameters.iter().find(|parameter| parameter.id == head)
                        {
                            found = Some(parameter);
                        }
                        None
                    });
                    found
                });
            match parameter {
                Some(parameter) => sentence::happens_result_name(sentence::happens_base(&parameter.name)),
                None => sentence::happens_result_name(&sentences.value(value, None)),
            }
        }
        _ => sentence::happens_result_name(&sentences.value(value, None)),
    }
}

/// Returns whether every value a Compare line compares is filled.
fn compare_is_filled(line: &SheetLine) -> bool {
    let right = match line.test {
        Some(crate::sheet::document::CompareTest::Outside) => !line.low.is_blank() && !line.high.is_blank(),
        _ => match line.right.value() {
            Some(SheetValue::List(items)) => !items.is_empty(),
            Some(_) => true,
            None => false,
        },
    };
    !line.left.is_blank() && line.test.is_some() && right
}

/// Returns whether `name` is `base`, or `base` with a number from 2 after a space.
fn is_numbered(name: &str, base: &str) -> bool {
    name == base
        || name
            .strip_prefix(base)
            .and_then(|rest| rest.strip_prefix(' '))
            .is_some_and(|number| {
                !number.is_empty() && !number.starts_with('0') && number.bytes().all(|byte| byte.is_ascii_digit())
            })
}

/// Returns the automatic name of every result line of `document` that has one, by line id.
fn automatic_names(document: &SheetDocument) -> HashMap<String, String> {
    let sentences = Sentences::new(document);
    document
        .cards
        .iter()
        .flat_map(|card| &card.line_tables)
        .filter_map(|(id, line)| automatic_name_with(document, &sentences, line).map(|name| (id.clone(), name)))
        .collect()
}

/// Renames the result lines of `after` that follow their automatic names, where `before` is the sheet before the
/// edit, and makes each automatic name unique.
///
/// A line new in `after`, or one whose name the edit set, follows when it has no name or its name is its automatic
/// name.
pub(super) fn update_names(before: &SheetDocument, after: &mut SheetDocument) {
    let old_names = automatic_names(before);
    let new_names = automatic_names(after);
    let follows = |id: &str, line: &SheetLine| -> bool {
        let Some(new_base) = new_names.get(id) else {
            return false;
        };
        match before.line(id) {
            // A name the edit left alone follows when it followed before the edit.
            Some((_, old)) if old.name == line.name => match (&old.name, old_names.get(id)) {
                (None, _) => true,
                (Some(name), Some(old_base)) => is_numbered(name, old_base),
                (Some(_), None) => false,
            },
            // A new line, or a name the edit set, follows when it has no name or its automatic name.
            _ => line.name.as_deref().is_none_or(|name| is_numbered(name, new_base)),
        }
    };
    let mut taken: HashSet<String> = HashSet::new();
    let mut order: Vec<(usize, String)> = Vec::new();
    for (index, card) in after.cards.iter().enumerate() {
        let relations = CardRelations::new(after, card);
        for id in relations.order() {
            let Some(line) = card.line_tables.get(*id) else {
                continue;
            };
            if follows(id, line) {
                order.push((index, (*id).to_owned()));
            } else if let Some(name) = &line.name {
                taken.insert(name.to_lowercase());
            }
        }
    }
    for (index, id) in order {
        let Some(base) = new_names.get(&id) else {
            continue;
        };
        let name = free_name(base, &mut taken);
        if let Some(line) = after.cards[index].line_tables.get_mut(&id) {
            line.name = Some(name);
        }
    }
}

/// Returns the automatic name of `summary`, a summary of a look at agents of `document`.
///
/// The names are "count", "average velocity" for the average of their Velocity, "centre" for the average offset to
/// them, "push" for the total offset away from them, "average distance to them" for another relation, and the
/// summary's word alone, "average", while its value is empty.
///
/// ```
/// use henad_graph::sheet::document::{LookSummary, SheetChip, SheetValue, Slot, SummaryKind};
/// use henad_graph::sheet::edit::automatic_summary_name;
///
/// let boids = henad_graph::sheet::starters()
///     .iter()
///     .find(|starter| starter.file_name == "boids.toml")
///     .expect("boids is a starter");
/// let sheet = henad_graph::sheet::ParsedSheet::parse(boids.text).expect("boids reads");
/// let mut summary = LookSummary::new("", "");
/// summary.summary = Some(SummaryKind::Average);
/// summary.value = Slot::Filled(SheetValue::Chip(SheetChip::Their("velocity".to_owned())));
/// assert_eq!(automatic_summary_name(sheet.document(), &summary), "average velocity");
/// ```
pub fn automatic_summary_name(document: &SheetDocument, summary: &LookSummary) -> String {
    let Some(kind) = summary.summary else {
        return BLANK_TEXT.to_owned();
    };
    if kind == SummaryKind::Count {
        return kind.as_str().to_owned();
    }
    match summary.value.value() {
        Some(SheetValue::Chip(SheetChip::Their(id))) => {
            let name = Sentences::new(document).property_words(id);
            format!("{} {}", kind.as_str(), sentence::drop_ordinary_capital(&name))
        }
        Some(SheetValue::Chip(SheetChip::Relation(relation))) => match (kind, relation) {
            (SummaryKind::Average, ChipRelation::OffsetToThem) => sentence::CENTER_SUMMARY.to_owned(),
            (SummaryKind::Total, ChipRelation::OffsetAwayFromThem) => sentence::PUSH_SUMMARY.to_owned(),
            _ => format!("{} {}", kind.as_str(), sentence::relation_words(*relation)),
        },
        _ => kind.as_str().to_owned(),
    }
}

/// Renames the summaries of the looks at agents of `after` that follow their automatic names, where `before` is the
/// sheet before the edit, and gives each the id minted from its name, unique within its line. A new id is written into
/// every reference to the summary.
///
/// A summary follows when the edit made it with an empty name or its automatic name, or when its name before the edit
/// was its automatic name then, a number such as " 2" after it included. A repeated automatic name takes " 2", " 3"
/// and so on after the names typed by hand. A summary whose automatic name the edit left alone takes its name before
/// one whose automatic name the edit changed.
pub(super) fn update_summary_names(before: &SheetDocument, after: &mut SheetDocument) {
    let mut renames = Vec::new();
    for card_index in 0..after.cards.len() {
        let ids: Vec<String> = after.cards[card_index].line_tables.keys().cloned().collect();
        for id in ids {
            let old_line = before.line(&id).map(|(_, line)| line);
            let Some(line) = after.cards[card_index].line_tables.get(&id) else {
                continue;
            };
            if line.around != Some(Around::AgentsWithin) || line.summaries.is_empty() {
                continue;
            }
            let names = summary_names_of(before, after, old_line, line);
            if let Some(line) = after.cards[card_index].line_tables.get_mut(&id) {
                take_summary_names(line, names, &id, &mut renames);
            }
        }
    }
    rename_summaries(after, renames);
}

/// A new id of a summary, written into every reference to its old id.
struct SummaryRename {
    /// Id of the summary's line.
    line: String,
    /// Id of the summary before the edit.
    old: String,
    /// Id of the summary after the edit.
    new: String,
}

/// Returns the name of each summary of `line`, a look at agents of `after`, `None` for one that does not follow its
/// automatic name, where `old_line` is the line in `before`, the sheet before the edit.
fn summary_names_of(
    before: &SheetDocument,
    after: &SheetDocument,
    old_line: Option<&SheetLine>,
    line: &SheetLine,
) -> Vec<Option<String>> {
    let follows: Vec<bool> = line
        .summaries
        .iter()
        .enumerate()
        .map(
            |(index, summary)| match old_line.and_then(|old| old.summaries.get(index)) {
                Some(old) if old.name == summary.name => is_numbered(&old.name, &automatic_summary_name(before, old)),
                _ => summary.name.is_empty() || summary.name == automatic_summary_name(after, summary),
            },
        )
        .collect();
    let bases: Vec<String> = line
        .summaries
        .iter()
        .map(|summary| automatic_summary_name(after, summary))
        .collect();
    // A summary is steady when the line held it, by id, before the edit with the same automatic name.
    let steady: Vec<bool> = line
        .summaries
        .iter()
        .zip(&bases)
        .map(|(summary, base)| {
            old_line
                .and_then(|old| {
                    old.summaries
                        .iter()
                        .find(|old| !old.id.is_empty() && old.id == summary.id)
                })
                .is_some_and(|old| automatic_summary_name(before, old) == *base)
        })
        .collect();
    let taken: HashSet<String> = line
        .summaries
        .iter()
        .zip(&follows)
        .filter(|(_, follows)| !**follows)
        .map(|(summary, _)| summary.name.to_lowercase())
        .collect();
    summary_names(&bases, &follows, &steady, taken)
}

/// Gives each summary of `line`, the line `line_id`, its name of `names`, and the id minted from its name when that
/// changes, and adds each new id of a summary that held one to `renames`.
fn take_summary_names(
    line: &mut SheetLine,
    names: Vec<Option<String>>,
    line_id: &str,
    renames: &mut Vec<SummaryRename>,
) {
    // A summary whose name changes, or that has no id yet, takes the id minted from its name, unique within the line.
    // Every other summary keeps its id.
    let mut renamed = Vec::with_capacity(names.len());
    for (summary, name) in line.summaries.iter_mut().zip(names) {
        let changed = name.as_ref().is_some_and(|name| *name != summary.name);
        if let Some(name) = name {
            summary.name = name;
        }
        renamed.push(changed || summary.id.is_empty());
    }
    let mut held: Vec<String> = line
        .summaries
        .iter()
        .zip(&renamed)
        .filter(|(_, renamed)| !**renamed)
        .map(|(summary, _)| summary.id.clone())
        .collect();
    for (summary, _) in line.summaries.iter_mut().zip(&renamed).filter(|(_, renamed)| **renamed) {
        let minted = summary_id(&summary.name, &held);
        if minted != summary.id && !summary.id.is_empty() {
            renames.push(SummaryRename {
                line: line_id.to_owned(),
                old: summary.id.clone(),
                new: minted.clone(),
            });
        }
        summary.id.clone_from(&minted);
        held.push(minted);
    }
}

/// Returns the name of each summary of a line whose automatic names are `bases`, `None` for one that does not follow
/// its automatic name, where `follows` and `steady` hold for each summary and `taken` holds the names typed by hand,
/// in lower case.
///
/// The steady summaries take their names first, then the others, each in line order.
fn summary_names(
    bases: &[String],
    follows: &[bool],
    steady: &[bool],
    mut taken: HashSet<String>,
) -> Vec<Option<String>> {
    let mut names: Vec<Option<String>> = vec![None; bases.len()];
    for steady_pass in [true, false] {
        for (index, base) in bases.iter().enumerate() {
            if !follows[index] || steady[index] != steady_pass {
                continue;
            }
            names[index] = Some(free_name(base, &mut taken));
        }
    }
    names
}

/// Returns the first of `base`, `base 2`, `base 3` and so on whose lower case `taken` does not hold, and adds its lower
/// case to `taken`.
fn free_name(base: &str, taken: &mut HashSet<String>) -> String {
    let mut count = 1;
    let mut name = sentence::repeated_result_name(base, count);
    while taken.contains(&name.to_lowercase()) {
        count += 1;
        name = sentence::repeated_result_name(base, count);
    }
    taken.insert(name.to_lowercase());
    name
}

/// Writes each new summary id of `renames` into every reference of `document` to the old id.
///
/// A new id can be the old id of another summary of the same line. Each old id first moves to an id that no summary
/// holds, and no reference is renamed twice.
fn rename_summaries(document: &mut SheetDocument, renames: Vec<SummaryRename>) {
    let mut parked = Vec::with_capacity(renames.len());
    for (index, SummaryRename { line, old, new }) in renames.into_iter().enumerate() {
        let free = format!("{RESERVED_PREFIX}summary_{index}");
        change_references(
            document,
            &Target::Summary {
                line: line.clone(),
                summary: old,
            },
            Change::Rename(&free),
        );
        parked.push(SummaryRename { line, old: free, new });
    }
    for SummaryRename { line, old, new } in parked {
        change_references(document, &Target::Summary { line, summary: old }, Change::Rename(&new));
    }
}

/// Returns the id of a summary named `name`, unique among `held`, the ids of the other summaries of its line.
///
/// A name that mints a reserved id, such as "count", takes "summary" after it, as in `count_summary`.
fn summary_id(name: &str, held: &[String]) -> String {
    let is_taken = |candidate: &str| held.iter().any(|other| other == candidate);
    mint_id(name, IdSpace::Item, is_taken)
        .or_else(|_| mint_id(&format!("{name} summary"), IdSpace::Item, is_taken))
        .unwrap_or_else(|_| {
            (1..)
                .map(|number| format!("summary_{number}"))
                .find(|candidate| !is_taken(candidate))
                .unwrap_or_default()
        })
}
