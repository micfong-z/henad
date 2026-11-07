//! Automatic result names: the name a result line takes after what it works out, and the rule that keeps them up to
//! date and unique.
//!
//! A result line *follows* its automatic name while its stored name equals the automatic name the line had before an
//! edit, a number such as " 2" after it included. An edit renames every line that follows, and leaves a name typed by
//! hand as it is. Result names are unique across the sheet, case aside: a repeated automatic name takes " 2", " 3" and
//! so on, in display order, after the names typed by hand.

use std::collections::{HashMap, HashSet};

use crate::sheet::document::{Around, LineKind, SheetChip, SheetDocument, SheetLine, SheetValue, Slot, SummaryKind};
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
        let mut count = 1;
        let mut name = sentence::repeated_result_name(base, count);
        while taken.contains(&name.to_lowercase()) {
            count += 1;
            name = sentence::repeated_result_name(base, count);
        }
        taken.insert(name.to_lowercase());
        if let Some(line) = after.cards[index].line_tables.get_mut(&id) {
            line.name = Some(name);
        }
    }
}
