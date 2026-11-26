//! The lines of a card as the Card pane, the Diagram and the Sheet pane read them, with no `Ui`: each line's sentence,
//! its controls with their words and names, the notes under it, its swatches and its red chips.

use egui::Color32;
use henad_core::diagnostic::DiagnosticSeverity;
use henad_graph::sheet::check::{SheetDiagnostic, SheetReport};
use henad_graph::sheet::document::{CardRuns, LineKind, PropertyType, SheetCard, SheetDocument};
use henad_graph::sheet::edit::SheetEdit;
use henad_graph::sheet::relations::CardRelations;
use henad_graph::sheet::sentence::{
    ALWAYS, ANY_STATE_ID, BLANK_TEXT, ChipKind, FOR_EACH_BLANK_WORDS, FOR_EACH_WORDS, LineSelect, OtherwiseRepeat,
    PartPlace, SentenceChip, SentencePart, Sentences, UNIT_PLACEHOLDER, USES_OWN_RESULT, always_name,
    for_each_change_name, for_each_optional_name, line_name, line_prefix, line_word, move_to_otherwise_notice,
    move_to_otherwise_tooltip, otherwise_notice, otherwise_tooltip, otherwise_words, phrase_lead_name,
    result_below_tooltip, result_inside_tooltip, result_own_tooltip, sentence_text, uses_result_below,
    uses_result_inside,
};
use henad_graph::sheet::sentence::{COMPETING_WORD, competing_parts, competing_tooltip};

use super::find_property;
use crate::ui::mcs;

/// Colour of a note under a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteColor {
    /// A note that asks the author to look again, in the theme's warn colour.
    Amber,
    /// A note of a line that cannot run because of what it uses, or of an error, in the theme's error colour.
    Red,
    /// A note that explains or confirms, in grey.
    Grey,
}

/// A button of a note. A click runs one edit of the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct NoteAction {
    /// Words of the button.
    pub label: String,
    /// Accessible name of the button.
    pub name: String,
    /// The edit the button runs.
    pub edit: SheetEdit,
}

/// A note under a line.
#[derive(Debug, Clone, PartialEq)]
pub struct LineNote {
    /// Words of the note.
    pub text: String,
    /// Colour of the note.
    pub color: NoteColor,
    /// Button of the note, `None` for a note with none.
    pub action: Option<NoteAction>,
    /// Tooltip and accessible description of the note, `None` for a note with none.
    pub description: Option<String>,
}

/// A control inside a line: a chip, a blank, a select, "always", "otherwise" or the optional "for each …" blank.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineControl {
    /// Words the control shows.
    pub visible: String,
    /// Accessible name of the control.
    pub name: String,
    /// Whether the control is a select, whose visible words are its value rather than its label.
    pub select: bool,
}

/// One line of a card as the Card pane shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct CardLine {
    /// Id of the line.
    pub id: String,
    /// Number of the line on its card, from 1.
    pub number: usize,
    /// Number of blocks around the line.
    pub depth: usize,
    /// Prefix of the line's accessible names, such as "Line 2" or "Clear line 1".
    pub prefix: String,
    /// Line word of the line in a tool's name, such as "line 2" or "Clear line 1".
    pub word: String,
    /// Sentence of the line as plain text, with its "otherwise" words.
    pub sentence: String,
    /// Parts of the sentence, in order.
    pub parts: Vec<SentencePart>,
    /// Place of each part of [`Self::parts`], `None` for words and for a part that no edit sets.
    pub places: Vec<Option<PartPlace>>,
    /// Ids of the higher lines that override this line, empty for a line with no "otherwise".
    pub otherwise: Vec<String>,
    /// Tooltip and accessible description of the "otherwise" words, empty for a line with no "otherwise".
    pub otherwise_tooltip: String,
    /// Notice shown the first time in a session that the line starts with "otherwise", empty for a line with no
    /// "otherwise".
    pub otherwise_notice: String,
    /// Ids of the other lines of the competing group the line starts with "or" in, empty for a line with no "or".
    pub competing: Vec<String>,
    /// Tooltip and accessible description of the word "or", empty for a line with no "or".
    pub competing_tooltip: String,
    /// Index in [`Self::parts`] of each result chip that the line cannot use, with the chip's tooltip.
    pub red_chips: Vec<(usize, String)>,
    /// Index in [`Self::parts`] of each state chip, with the colour of its state. A state that no property has is
    /// grey.
    pub swatches: Vec<(usize, Color32)>,
    /// Notes of the sheet's check about the line, in report order, a skip note first.
    pub notes: Vec<LineNote>,
    /// Number of skip notes at the start of [`Self::notes`]. The check skips a line that has one.
    pub skips: usize,
}

impl CardLine {
    /// Returns the accessible name of the line, such as "Line 2: Become Alive from Dead, when Alive neighbours is 3".
    pub fn label(&self) -> String {
        line_name(&self.prefix, &self.sentence)
    }

    /// Returns the control that `part` of the line draws, with its visible words and its accessible name, `None` for
    /// words.
    pub fn control(&self, part: &SentencePart) -> Option<LineControl> {
        let (visible, name, select) = match part {
            SentencePart::Chip(chip) => (chip.text(), chip.text(), false),
            SentencePart::Blank(blank) => (blank.word().to_owned(), blank.name(&self.prefix), false),
            SentencePart::Select(select, words) => {
                (select_words(*select, words).to_owned(), select.name(&self.prefix), true)
            }
            SentencePart::Always => (ALWAYS.to_owned(), always_name(&self.prefix), false),
            SentencePart::Otherwise(lines) => (otherwise_words(lines), otherwise_words(lines), false),
            SentencePart::Competing(_) => (COMPETING_WORD.to_owned(), COMPETING_WORD.to_owned(), false),
            SentencePart::ForEachBlank(_) => (
                FOR_EACH_BLANK_WORDS.to_owned(),
                for_each_optional_name(&self.prefix),
                false,
            ),
            SentencePart::ForEach(_) => (FOR_EACH_WORDS.to_owned(), for_each_change_name(&self.prefix), false),
            SentencePart::PhraseLead(words) => (
                words.trim().to_owned(),
                phrase_lead_name(&self.prefix, words.trim()),
                false,
            ),
            _ => return None,
        };
        Some(LineControl { visible, name, select })
    }

    /// Returns every control of the line in order. A chip, blank or select that no edit sets is no control: it opens
    /// nothing, and the card draws a label or words in its place.
    #[cfg(test)]
    pub fn controls(&self) -> Vec<LineControl> {
        self.parts
            .iter()
            .zip(&self.places)
            .filter(|(part, place)| {
                place.is_some()
                    || !matches!(
                        part,
                        SentencePart::Chip(_) | SentencePart::Blank(_) | SentencePart::Select(..)
                    )
            })
            .filter_map(|(part, _)| self.control(part))
            .collect()
    }
}

/// Returns the lines of card `card_index` of `document` in display order, each with its number, depth, sentence and
/// parts, and the notes of `report` about it.
pub fn card_lines(
    document: &SheetDocument,
    sentences: &Sentences<'_>,
    report: &SheetReport,
    card_index: usize,
) -> Vec<CardLine> {
    let Some(card) = document.cards.get(card_index) else {
        return Vec::new();
    };
    let button = button_of(document, card);
    let relations = CardRelations::new(document, card);
    let groups = relations.competing_groups();
    relations
        .order()
        .iter()
        .enumerate()
        .map(|(index, &line_id)| {
            let mut depth = 0;
            let mut block = relations.parent(line_id);
            // Display order holds each block once, before its lines. The bound guards against a cycle all the same.
            while let Some(parent) = block
                && depth < relations.order().len()
            {
                depth += 1;
                block = relations.parent(parent);
            }
            let number = relations.number(line_id).unwrap_or(index + 1);
            let prefix = line_prefix(button, number);
            let otherwise_numbers = relations.otherwise_numbers(line_id);
            let mut placed = sentences.placed_parts(line_id, &otherwise_numbers);
            // A lower line of a competing group starts with "or", unless a line outside the group overrides it.
            let group = groups
                .iter()
                .find(|group| group.contains(&line_id))
                .filter(|group| group.first() != Some(&line_id) && otherwise_numbers.is_empty());
            let (competing, competing_tooltip) = match group {
                Some(group) => {
                    let numbers: Vec<usize> = group.iter().filter_map(|id| relations.number(id)).collect();
                    let state = relations
                        .line(line_id)
                        .and_then(|line| state_of(document, line.property.as_deref(), line.start_state.as_deref()))
                        .unwrap_or(BLANK_TEXT);
                    let thing = sentences.thing_word(&card.every);
                    let tooltip = competing_tooltip(&numbers, state, &thing);
                    placed = competing_parts(placed, numbers);
                    let others = group.iter().filter(|id| **id != line_id).map(|id| (*id).to_owned());
                    (others.collect(), tooltip)
                }
                None => (Vec::new(), String::new()),
            };
            let (parts, places): (Vec<SentencePart>, Vec<Option<PartPlace>>) =
                placed.into_iter().map(|placed| (placed.part, placed.place)).unzip();
            let red_chips = red_chips(&relations, line_id, index, &parts);
            let (notes, skips) = line_notes(report, card_index, line_id, &red_chips);
            let swatches = swatches(document, &parts);
            let (otherwise_tooltip, otherwise_notice) = if otherwise_numbers.is_empty() {
                (String::new(), String::new())
            } else {
                otherwise_words_of(document, &relations, line_id, &prefix, &otherwise_numbers)
            };
            CardLine {
                id: line_id.to_owned(),
                number,
                depth,
                prefix,
                word: line_word(button, number),
                sentence: sentence_text(&parts),
                otherwise: relations.otherwise_of(line_id).into_iter().map(str::to_owned).collect(),
                otherwise_tooltip,
                otherwise_notice,
                competing,
                competing_tooltip,
                parts,
                places,
                red_chips: red_chips.into_iter().map(|red| (red.index, red.tooltip)).collect(),
                swatches,
                notes,
                skips,
            }
        })
        .collect()
}

/// Returns the index of each state chip of `parts` with the colour of its state in `document`.
fn swatches(document: &SheetDocument, parts: &[SentencePart]) -> Vec<(usize, Color32)> {
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let SentencePart::Chip(SentenceChip {
                kind: ChipKind::State { property, state },
                ..
            }) = part
            else {
                return None;
            };
            let color = property
                .as_deref()
                .and_then(|property| find_property(document, property))
                .and_then(|property| property.states.iter().find(|candidate| candidate.id == *state))
                .map_or(mcs::GRAY_500, |state| {
                    let [red, green, blue, alpha] = state.color;
                    Color32::from_rgba_unmultiplied(red, green, blue, alpha)
                });
            Some((index, color))
        })
        .collect()
}

/// A result chip that its line cannot use.
struct RedChip {
    /// Index of the chip in the line's parts.
    index: usize,
    /// Tooltip of the chip.
    tooltip: String,
    /// Skip note the sheet's check writes for the chip.
    note: String,
}

/// Returns the result chips of line `line_id`, at `position` in display order, that name its own result, a result
/// below it or a result inside a When the line is not in.
fn red_chips(relations: &CardRelations<'_>, line_id: &str, position: usize, parts: &[SentencePart]) -> Vec<RedChip> {
    parts
        .iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let SentencePart::Chip(SentenceChip {
                kind: ChipKind::Result(result),
                name,
                ..
            }) = part
            else {
                return None;
            };
            let result_name = relations
                .line(result)
                .and_then(|line| line.name.clone())
                .unwrap_or_else(|| name.clone());
            let result_position = relations.index(result)?;
            if result_position == position {
                return Some(RedChip {
                    index,
                    tooltip: result_own_tooltip(&result_name),
                    note: USES_OWN_RESULT.to_owned(),
                });
            }
            if result_position > position {
                return Some(RedChip {
                    index,
                    tooltip: result_below_tooltip(&result_name),
                    note: uses_result_below(&result_name),
                });
            }
            let block = relations.parent(result)?;
            if relations.is_within(line_id, block) {
                return None;
            }
            let number = relations.number(block).unwrap_or(0);
            Some(RedChip {
                index,
                tooltip: result_inside_tooltip(&result_name),
                note: uses_result_inside(&result_name, number),
            })
        })
        .collect()
}

/// Returns the notes of `report` about line `line_id` of card `card_index`, the skip notes first, with the number of
/// skip notes.
///
/// A skip note about a chip in `red_chips` and an error are red, and any other note amber.
fn line_notes(report: &SheetReport, card_index: usize, line_id: &str, red_chips: &[RedChip]) -> (Vec<LineNote>, usize) {
    let about_line = report
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.card == Some(card_index) && diagnostic.line.as_deref() == Some(line_id));
    let (skips, others): (Vec<_>, Vec<_>) = about_line.partition(|diagnostic| diagnostic.skips_line);
    let skip_count = skips.len();
    let notes = skips
        .into_iter()
        .chain(others)
        .map(|diagnostic| {
            let text = diagnostic.diagnostic.message.clone();
            let red = diagnostic.diagnostic.severity == DiagnosticSeverity::Error
                || red_chips.iter().any(|chip| chip.note == text);
            LineNote {
                text,
                color: if red { NoteColor::Red } else { NoteColor::Amber },
                action: None,
                description: None,
            }
        })
        .collect();
    (notes, skip_count)
}

/// Returns whether the card pane draws `diagnostic` under a line, which it does for a problem about a line in the
/// display order of its card.
///
/// Note that the checker also places a problem at a line the card never shows, such as a line table no lines list
/// names. Such a problem stays in the list under the panes.
pub fn draws_diagnostic(document: &SheetDocument, diagnostic: &SheetDiagnostic) -> bool {
    let (Some(card), Some(line)) = (diagnostic.card, diagnostic.line.as_deref()) else {
        return false;
    };
    document
        .cards
        .get(card)
        .is_some_and(|card| CardRelations::new(document, card).index(line).is_some())
}

/// Returns the tooltip of the "otherwise" words of line `line_id`, whose line prefix is `prefix`, overridden by the
/// lines numbered `numbers`, and the notice shown the first time the line starts with "otherwise".
///
/// Both name the property the lines change, and the old state a cell must be in for both to apply. When a higher line
/// sits in the Repeat that holds line `line_id`, both say the higher line wins only at the same time. Otherwise, when
/// two higher lines share a Repeat, both say the first time round decides between them. Below a Move to, both name the
/// Position and say that the agent stays where it is when the cell of the line that wins is not open.
fn otherwise_words_of(
    document: &SheetDocument,
    relations: &CardRelations<'_>,
    line_id: &str,
    prefix: &str,
    numbers: &[usize],
) -> (String, String) {
    let line = relations.line(line_id);
    if line.is_some_and(|line| line.kind == LineKind::Move) {
        let every = &relations.card().every;
        let position = document
            .agents
            .iter()
            .find(|kind| kind.id == *every)
            .and_then(|kind| {
                kind.properties
                    .iter()
                    .find(|property| property.property_type == PropertyType::Position)
            })
            .map_or(BLANK_TEXT, |property| property.name.as_str());
        let thing = Sentences::new(document).thing_word(every);
        return (
            move_to_otherwise_tooltip(numbers, position, &thing),
            move_to_otherwise_notice(prefix, numbers, position, &thing),
        );
    }
    let property = line
        .and_then(|line| line.property.as_deref())
        .and_then(|id| find_property(document, id));
    let state = line.and_then(|line| state_of(document, line.property.as_deref(), line.start_state.as_deref()));
    let property_name = property.map_or(BLANK_TEXT, |property| property.name.as_str());
    let thing = Sentences::new(document).thing_word(&relations.card().every);
    let own = relations.repeat_of(line_id);
    let higher: Vec<Option<&str>> = relations
        .otherwise_of(line_id)
        .into_iter()
        .map(|higher| relations.repeat_of(higher))
        .collect();
    let repeat = if own.is_some() && higher.contains(&own) {
        OtherwiseRepeat::Own
    } else if higher
        .iter()
        .enumerate()
        .any(|(index, repeat)| repeat.is_some() && higher[index + 1..].contains(repeat))
    {
        OtherwiseRepeat::Higher
    } else {
        OtherwiseRepeat::Apart
    };
    (
        otherwise_tooltip(numbers, property_name, state, &thing, repeat),
        otherwise_notice(prefix, numbers, property_name, state, &thing, repeat),
    )
}

/// Returns the name of the named state `state` of the property `property` of `document`, `None` for any state or a
/// state the property does not hold.
fn state_of<'a>(document: &'a SheetDocument, property: Option<&str>, state: Option<&str>) -> Option<&'a str> {
    let state = state.filter(|state| *state != ANY_STATE_ID)?;
    find_property(document, property?)?
        .states
        .iter()
        .find(|candidate| candidate.id == state)
        .map(|state| state.name.as_str())
}

/// Returns the name of the button that runs `card` once, `None` for a card that runs every tick or that no button
/// runs.
pub(super) fn button_of<'a>(document: &'a SheetDocument, card: &SheetCard) -> Option<&'a str> {
    if card.runs == CardRuns::EachTick {
        return None;
    }
    document
        .buttons
        .iter()
        .find(|button| {
            button
                .does
                .iter()
                .any(|step| step.card.as_deref() == Some(card.id.as_str()))
        })
        .map(|button| button.name.as_str())
}

/// Returns the words a select shows: its chosen words, or while empty [`UNIT_PLACEHOLDER`] for a unit and
/// [`BLANK_TEXT`] otherwise.
pub(super) fn select_words(select: LineSelect, words: &str) -> &str {
    match (words.is_empty(), select) {
        (false, _) => words,
        (true, LineSelect::UnitOfTime) => UNIT_PLACEHOLDER,
        (true, _) => BLANK_TEXT,
    }
}
